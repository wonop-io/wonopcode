//! Artifact storage operations.
//!
//! Manages reading and writing artifacts to the specs directory.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

use super::config::WonopCodeConfig;
use super::state::WorkstreamState;
use super::types::{
    Artifact, ArtifactMetadata, ArtifactType, Priority, Progress, SessionLogImportance,
};

/// Manages artifact storage in the specs directory.
pub struct ArtifactStore {
    #[allow(dead_code)]
    root_dir: PathBuf,
    specs_dir: PathBuf,
}

impl ArtifactStore {
    /// Create a new artifact store for the given root directory.
    pub fn new(root_dir: &Path) -> Result<Self> {
        tracing::debug!(
            "🗄️ ArtifactStore::new: Creating store for root_dir={}",
            root_dir.display()
        );

        let config = WonopCodeConfig::load(root_dir)?;
        let specs_dir = config.specs_dir(root_dir);

        tracing::info!(
            "🗄️ ArtifactStore::new: root_dir={}, specs_dir={}, ace_enabled={}",
            root_dir.display(),
            specs_dir.display(),
            config.is_enabled()
        );

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            specs_dir,
        })
    }

    /// Create a store with a custom specs directory.
    pub fn with_specs_dir(root_dir: &Path, specs_dir: PathBuf) -> Self {
        tracing::debug!(
            "🗄️ ArtifactStore::with_specs_dir: root_dir={}, specs_dir={}",
            root_dir.display(),
            specs_dir.display()
        );
        Self {
            root_dir: root_dir.to_path_buf(),
            specs_dir,
        }
    }

    /// Get the specs directory path.
    pub fn specs_dir(&self) -> &Path {
        &self.specs_dir
    }

    /// Ensure the specs directory structure exists.
    pub fn ensure_directories(&self) -> Result<()> {
        tracing::debug!(
            "🗄️ ArtifactStore::ensure_directories: Creating directories under {}",
            self.specs_dir.display()
        );

        for dir in [
            "sessions",
            "use-cases",
            "requirements",
            "designs",
            "tests",
            "tasks",
        ] {
            std::fs::create_dir_all(self.specs_dir.join(dir))?;
        }
        // Create staging directory
        let staging = self.specs_dir.join("workspace").join("staging");
        for dir in [
            "sessions",
            "use-cases",
            "requirements",
            "designs",
            "tests",
            "tasks",
        ] {
            std::fs::create_dir_all(staging.join(dir))?;
        }
        Ok(())
    }

    /// Create a new artifact.
    #[allow(clippy::too_many_arguments)]
    pub fn create_artifact(
        &self,
        state: &mut WorkstreamState,
        artifact_type: ArtifactType,
        title: &str,
        content: &str,
        parents: Vec<String>,
        priority: Priority,
        staging: bool,
    ) -> Result<Artifact> {
        self.create_artifact_with_phase(
            state,
            artifact_type,
            title,
            content,
            parents,
            priority,
            staging,
            None,
        )
    }

    /// Create a new artifact with an optional phase.
    ///
    /// The phase parameter is primarily used for Task artifacts to group them
    /// in the implementation plan view. For other artifact types, phase is ignored.
    #[allow(clippy::too_many_arguments)]
    pub fn create_artifact_with_phase(
        &self,
        state: &mut WorkstreamState,
        artifact_type: ArtifactType,
        title: &str,
        content: &str,
        parents: Vec<String>,
        priority: Priority,
        staging: bool,
        phase: Option<String>,
    ) -> Result<Artifact> {
        tracing::info!(
            "🗄️ ArtifactStore::create_artifact_with_phase: type={}, title='{}', parents={:?}, phase={:?}, staging={}",
            artifact_type,
            title,
            parents,
            phase,
            staging
        );
        tracing::debug!(
            "🗄️ ArtifactStore: specs_dir={}, ticket_id={}",
            self.specs_dir.display(),
            state.ticket_id
        );

        // Validate parents (including ticket ID check)
        tracing::debug!("🗄️ ArtifactStore: Validating parents...");
        self.validate_parents(&artifact_type, &parents, &state.ticket_id)?;
        tracing::debug!("🗄️ ArtifactStore: Parents validated successfully");

        // Generate ID
        let seq = state.next_sequence(artifact_type.directory());
        let id = format!("{}-{}-{:03}", artifact_type.prefix(), state.ticket_id, seq);
        tracing::info!("🗄️ ArtifactStore: Generated artifact ID: {}", id);

        // Create metadata
        let now = Utc::now();
        let metadata = ArtifactMetadata {
            id: id.clone(),
            artifact_type,
            progress: Progress::Backlog,
            parents,
            priority,
            phase,
            created: now,
            updated: now,
            author: "agent".to_string(),
            approved_by: None,
            approved_at: None,
        };

        // Determine path
        let dir = if staging {
            self.specs_dir
                .join("workspace")
                .join("staging")
                .join(artifact_type.directory())
        } else {
            self.specs_dir.join(artifact_type.directory())
        };
        tracing::debug!("🗄️ ArtifactStore: Target directory: {}", dir.display());

        tracing::debug!("🗄️ ArtifactStore: Creating directory if needed...");
        std::fs::create_dir_all(&dir)?;
        tracing::debug!("🗄️ ArtifactStore: Directory exists: {}", dir.exists());

        let filename = format!("{}-{}.md", id, sanitize_title(title));
        let path = dir.join(&filename);
        tracing::info!("🗄️ ArtifactStore: Writing file to: {}", path.display());

        // Write file
        let file_content = format_artifact(&metadata, title, content);
        tracing::debug!(
            "🗄️ ArtifactStore: File content length: {} bytes",
            file_content.len()
        );

        std::fs::write(&path, &file_content)?;

        // Verify file was written
        if path.exists() {
            let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            tracing::info!(
                "🗄️ ArtifactStore: ✓ File written successfully: {} ({} bytes)",
                path.display(),
                file_size
            );
        } else {
            tracing::error!(
                "🗄️ ArtifactStore: ✗ File write appeared to succeed but file not found: {}",
                path.display()
            );
        }

        Ok(Artifact {
            metadata,
            title: title.to_string(),
            content: content.to_string(),
            path,
        })
    }

    /// Read an artifact by ID.
    pub fn read_artifact(&self, id: &str) -> Result<Option<Artifact>> {
        // Determine artifact type from ID prefix
        let artifact_type = match parse_artifact_type_from_id(id) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        // Search in specs and staging
        let dirs = [
            self.specs_dir.join(artifact_type.directory()),
            self.specs_dir
                .join("workspace")
                .join("staging")
                .join(artifact_type.directory()),
        ];

        for dir in dirs {
            if !dir.exists() {
                continue;
            }

            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    let filename = path.file_stem().unwrap_or_default().to_string_lossy();
                    if filename.starts_with(id) {
                        return self.parse_artifact_file(&path).map(Some);
                    }
                }
            }
        }

        Ok(None)
    }

    /// List all artifacts of a given type.
    ///
    /// **Note**: This returns ALL artifacts of the type, regardless of ticket ID.
    /// For workstream-scoped queries, use `list_artifacts_for_ticket()` instead.
    pub fn list_artifacts(&self, artifact_type: ArtifactType) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();

        let dirs = [
            self.specs_dir.join(artifact_type.directory()),
            self.specs_dir
                .join("workspace")
                .join("staging")
                .join(artifact_type.directory()),
        ];

        for dir in dirs {
            if !dir.exists() {
                continue;
            }

            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Ok(artifact) = self.parse_artifact_file(&path) {
                        artifacts.push(artifact);
                    }
                }
            }
        }

        Ok(artifacts)
    }

    /// List artifacts of a given type that belong to a specific ticket.
    ///
    /// This filters artifacts by extracting the ticket ID from their artifact ID
    /// and comparing it to the provided ticket ID (case-insensitive).
    pub fn list_artifacts_for_ticket(
        &self,
        artifact_type: ArtifactType,
        ticket_id: &str,
    ) -> Result<Vec<Artifact>> {
        let all_artifacts = self.list_artifacts(artifact_type)?;
        Ok(all_artifacts
            .into_iter()
            .filter(|a| a.belongs_to_ticket(ticket_id))
            .collect())
    }

    /// List all artifacts of all types.
    ///
    /// **Note**: This returns ALL artifacts regardless of ticket ID.
    /// For workstream-scoped queries, use `list_all_artifacts_for_ticket()` instead.
    pub fn list_all_artifacts(&self) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();

        for artifact_type in [
            ArtifactType::Session,
            ArtifactType::UseCase,
            ArtifactType::Requirement,
            ArtifactType::Design,
            ArtifactType::TestCase,
            ArtifactType::Task,
        ] {
            artifacts.extend(self.list_artifacts(artifact_type)?);
        }

        Ok(artifacts)
    }

    /// List all artifacts of all types that belong to a specific ticket.
    pub fn list_all_artifacts_for_ticket(&self, ticket_id: &str) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();

        for artifact_type in [
            ArtifactType::Session,
            ArtifactType::UseCase,
            ArtifactType::Requirement,
            ArtifactType::Design,
            ArtifactType::TestCase,
            ArtifactType::Task,
        ] {
            artifacts.extend(self.list_artifacts_for_ticket(artifact_type, ticket_id)?);
        }

        Ok(artifacts)
    }

    /// Update an artifact's progress status.
    pub fn update_artifact_progress(&self, id: &str, progress: Progress) -> Result<Artifact> {
        let artifact = self
            .read_artifact(id)?
            .ok_or_else(|| anyhow::anyhow!("Artifact not found: {}", id))?;

        let mut metadata = artifact.metadata.clone();
        metadata.progress = progress;
        metadata.updated = Utc::now();

        // Rewrite file
        let file_content = format_artifact(&metadata, &artifact.title, &artifact.content);
        std::fs::write(&artifact.path, &file_content)?;

        Ok(Artifact {
            metadata,
            ..artifact
        })
    }

    /// Update an artifact's content.
    pub fn update_artifact_content(
        &self,
        id: &str,
        title: &str,
        content: &str,
    ) -> Result<Artifact> {
        let artifact = self
            .read_artifact(id)?
            .ok_or_else(|| anyhow::anyhow!("Artifact not found: {}", id))?;

        let mut metadata = artifact.metadata.clone();
        metadata.updated = Utc::now();

        // Rewrite file
        let file_content = format_artifact(&metadata, title, content);
        std::fs::write(&artifact.path, &file_content)?;

        Ok(Artifact {
            metadata,
            title: title.to_string(),
            content: content.to_string(),
            path: artifact.path,
        })
    }

    /// Promote an artifact from staging to specs.
    pub fn promote_artifact(&self, id: &str) -> Result<Artifact> {
        let artifact = self
            .read_artifact(id)?
            .ok_or_else(|| anyhow::anyhow!("Artifact not found: {}", id))?;

        // Check if already in specs (not staging)
        let staging_path = self
            .specs_dir
            .join("workspace")
            .join("staging")
            .join(artifact.metadata.artifact_type.directory());

        if !artifact.path.starts_with(&staging_path) {
            bail!("Artifact {} is not in staging", id);
        }

        // Ensure target directory exists
        let target_dir = self
            .specs_dir
            .join(artifact.metadata.artifact_type.directory());
        std::fs::create_dir_all(&target_dir)?;

        // New path in specs
        let file_name = artifact
            .path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Artifact path has no file name: {:?}", artifact.path))?;
        let new_path = target_dir.join(file_name);

        // Move file
        std::fs::rename(&artifact.path, &new_path)?;

        Ok(Artifact {
            path: new_path,
            ..artifact
        })
    }

    /// Promote all staged artifacts of the given types.
    /// Returns the number of artifacts promoted.
    pub fn promote_artifacts_by_types(&self, types: &[ArtifactType]) -> Result<usize> {
        let mut count = 0;

        for artifact_type in types {
            let staging_dir = self
                .specs_dir
                .join("workspace")
                .join("staging")
                .join(artifact_type.directory());

            if !staging_dir.exists() {
                continue;
            }

            // Collect IDs first to avoid borrow issues
            let ids: Vec<String> = std::fs::read_dir(&staging_dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().map(|e| e == "md").unwrap_or(false))
                .filter_map(|entry| {
                    let filename = entry.path().file_stem()?.to_string_lossy().to_string();
                    // Extract ID (first part before the title)
                    let id = filename.split('-').take(3).collect::<Vec<_>>().join("-");
                    // Check if this looks like a valid artifact ID
                    if id.starts_with(artifact_type.prefix()) {
                        Some(filename.split('-').take(3).collect::<Vec<_>>().join("-"))
                    } else {
                        // Full ID might be different format, try parsing from file
                        self.parse_artifact_file(&entry.path())
                            .ok()
                            .map(|a| a.metadata.id)
                    }
                })
                .collect();

            for id in ids {
                if self.promote_artifact(&id).is_ok() {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Check if an artifact is in staging.
    pub fn is_staged(&self, artifact: &Artifact) -> bool {
        artifact
            .path
            .to_string_lossy()
            .contains("/workspace/staging/")
    }

    /// Ensure a session exists for the workstream.
    ///
    /// If the workstream doesn't have a session, or the session file is missing,
    /// creates one. Returns the session ID.
    pub fn ensure_session(
        &self,
        state: &mut WorkstreamState,
        root_dir: &std::path::Path,
    ) -> Result<String> {
        // Check if session_id is set AND the session artifact actually exists
        if let Some(ref session_id) = state.session_id {
            // Verify the session file exists on disk
            if self.read_artifact(session_id)?.is_some() {
                return Ok(session_id.clone());
            }
            // Session ID was set but file doesn't exist - clear it and create new
            tracing::warn!(
                "Session {} referenced in state but file not found, creating new session",
                session_id
            );
            state.session_id = None;
        }

        // Create a new session
        let title = state
            .ticket_title
            .clone()
            .unwrap_or_else(|| format!("Session for {}", state.ticket_id));

        self.ensure_directories()?;
        let session = self.create_session(state, &title)?;
        state.session_id = Some(session.metadata.id.clone());
        state.save(root_dir)?;

        Ok(session.metadata.id)
    }

    /// Create a new session artifact for the workstream.
    ///
    /// Sessions are the root artifacts that contain changelogs and context.
    /// Each workstream has exactly one session.
    pub fn create_session(
        &self,
        state: &mut WorkstreamState,
        title: &str,
    ) -> Result<Artifact> {
        let now = Utc::now();
        let date_str = now.format("%Y-%m-%d").to_string();

        // Initial changelog content
        let content = format!(
            "## Context\n\n(Session context will be added here)\n\n## Changelog\n\n### {}\n\n- 🟢 {} Session started\n",
            date_str,
            now.format("%H:%M")
        );

        self.create_artifact_with_phase(
            state,
            ArtifactType::Session,
            title,
            &content,
            vec![], // No parents for session
            Priority::High,
            false, // Sessions go directly to specs
            None,  // No phase for sessions
        )
    }

    /// Append a log entry to the session's changelog.
    ///
    /// The entry format is:
    /// - 🔴 HH:MM message (important)
    /// - 🟡 HH:MM message (maybe important)
    /// - 🟢 HH:MM message (info only)
    pub fn append_session_log(
        &self,
        session_id: &str,
        importance: SessionLogImportance,
        message: &str,
        indent: usize,
    ) -> Result<Artifact> {
        let session = self
            .read_artifact(session_id)?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

        let now = Utc::now();
        let time_str = now.format("%H:%M").to_string();
        let date_str = now.format("%Y-%m-%d").to_string();

        let emoji = match importance {
            SessionLogImportance::Important => "🔴",
            SessionLogImportance::MaybeImportant => "🟡",
            SessionLogImportance::InfoOnly => "🟢",
        };

        let indent_str = "  ".repeat(indent);
        let entry = format!("{}- {} {} {}\n", indent_str, emoji, time_str, message);

        // Check if we need to add a new date header
        let mut new_content = session.content.clone();
        let date_header = format!("### {}", date_str);

        if !new_content.contains(&date_header) {
            // Add new date section
            new_content.push_str(&format!("\n{}\n\n", date_header));
        }

        // Append the entry
        new_content.push_str(&entry);

        self.update_artifact_content(session_id, &session.title, &new_content)
    }

    /// Validate that parents are valid for the given artifact type and belong to the same ticket.
    fn validate_parents(
        &self,
        artifact_type: &ArtifactType,
        parents: &[String],
        ticket_id: &str,
    ) -> Result<()> {
        tracing::debug!(
            "🔍 validate_parents: artifact_type={}, parents={:?}, ticket_id={}",
            artifact_type,
            parents,
            ticket_id
        );

        let valid_types = artifact_type.valid_parent_types();
        tracing::debug!(
            "🔍 validate_parents: Valid parent types for {}: {:?}",
            artifact_type,
            valid_types
        );

        if artifact_type.requires_parent() && parents.is_empty() {
            tracing::error!(
                "🔍 validate_parents: {} requires parent but none provided",
                artifact_type
            );
            bail!(
                "{} requires at least one parent of type: {}",
                artifact_type,
                valid_types
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        for parent_id in parents {
            tracing::debug!("🔍 validate_parents: Checking parent '{}'", parent_id);

            let parent_type = match parse_artifact_type_from_id(parent_id) {
                Ok(t) => {
                    tracing::debug!(
                        "🔍 validate_parents: Parsed parent type: {} from id '{}'",
                        t,
                        parent_id
                    );
                    t
                }
                Err(e) => {
                    tracing::error!(
                        "🔍 validate_parents: Failed to parse artifact type from '{}': {}",
                        parent_id,
                        e
                    );
                    return Err(e);
                }
            };

            if !valid_types.contains(&parent_type) {
                tracing::error!(
                    "🔍 validate_parents: Invalid parent type {} for {}",
                    parent_type,
                    artifact_type
                );
                bail!(
                    "Invalid parent type {} for {}. Valid types: {}",
                    parent_type,
                    artifact_type,
                    valid_types
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            // Verify parent exists
            tracing::debug!(
                "🔍 validate_parents: Looking for parent artifact '{}' in specs_dir={}",
                parent_id,
                self.specs_dir.display()
            );

            let parent_artifact = match self.read_artifact(parent_id) {
                Ok(Some(a)) => {
                    tracing::debug!(
                        "🔍 validate_parents: Found parent artifact at {}",
                        a.path.display()
                    );
                    a
                }
                Ok(None) => {
                    tracing::error!(
                        "🔍 validate_parents: Parent artifact '{}' NOT FOUND in specs_dir={}",
                        parent_id,
                        self.specs_dir.display()
                    );
                    // List what files exist in the parent type directory
                    let parent_dir = self.specs_dir.join(parent_type.directory());
                    if parent_dir.exists() {
                        if let Ok(entries) = std::fs::read_dir(&parent_dir) {
                            let files: Vec<_> = entries
                                .filter_map(|e| e.ok())
                                .map(|e| e.file_name().to_string_lossy().to_string())
                                .collect();
                            tracing::debug!(
                                "🔍 validate_parents: Files in {}: {:?}",
                                parent_dir.display(),
                                files
                            );
                        }
                    } else {
                        tracing::debug!(
                            "🔍 validate_parents: Directory {} does not exist",
                            parent_dir.display()
                        );
                    }
                    bail!("Parent artifact not found: {}", parent_id);
                }
                Err(e) => {
                    tracing::error!(
                        "🔍 validate_parents: Error reading parent artifact '{}': {}",
                        parent_id,
                        e
                    );
                    return Err(e);
                }
            };

            // Verify parent belongs to the same ticket
            let parent_ticket = parent_artifact.ticket_id();
            tracing::debug!(
                "🔍 validate_parents: Parent ticket_id={:?}, expected={}",
                parent_ticket,
                ticket_id
            );

            if !parent_artifact.belongs_to_ticket(ticket_id) {
                tracing::error!(
                    "🔍 validate_parents: Parent '{}' belongs to ticket {:?}, not '{}'",
                    parent_id,
                    parent_ticket,
                    ticket_id
                );
                bail!(
                    "Parent artifact {} does not belong to the current workstream (ticket: {}).\n\
                     Parent artifacts must be from the same ticket.",
                    parent_id,
                    ticket_id
                );
            }

            tracing::debug!("🔍 validate_parents: Parent '{}' validated successfully", parent_id);
        }

        tracing::debug!("🔍 validate_parents: All parents validated successfully");
        Ok(())
    }

    /// Parse an artifact file.
    fn parse_artifact_file(&self, path: &Path) -> Result<Artifact> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read artifact: {}", path.display()))?;

        // Split frontmatter and content
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            bail!(
                "Invalid artifact format: missing frontmatter in {}",
                path.display()
            );
        }

        let frontmatter = parts[1].trim();
        let body = parts[2].trim();

        // Parse frontmatter
        let metadata: ArtifactMetadata = serde_yaml::from_str(frontmatter)
            .with_context(|| format!("Failed to parse frontmatter in {}", path.display()))?;

        // Extract title from first # heading
        let title = body
            .lines()
            .find(|line| line.starts_with("# "))
            .map(|line| line.trim_start_matches("# ").to_string())
            .unwrap_or_default();

        // Content is everything after the title line
        let content_lines: Vec<&str> = body.lines().collect();
        let title_index = content_lines
            .iter()
            .position(|line| line.starts_with("# "))
            .unwrap_or(0);
        let content = content_lines
            .into_iter()
            .skip(title_index + 1)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        Ok(Artifact {
            metadata,
            title,
            content,
            path: path.to_path_buf(),
        })
    }
}

/// Parse artifact type from ID prefix.
pub fn parse_artifact_type_from_id(id: &str) -> Result<ArtifactType> {
    let prefix = id.split('-').next().unwrap_or("");
    ArtifactType::from_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("Unknown artifact type prefix: {}", prefix))
}

/// Sanitize title for filename.
fn sanitize_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6) // Limit words
        .collect::<Vec<_>>()
        .join("-")
}

/// Format artifact as markdown with YAML frontmatter.
fn format_artifact(metadata: &ArtifactMetadata, title: &str, content: &str) -> String {
    let frontmatter = serde_yaml::to_string(metadata).unwrap_or_default();
    format!("---\n{}---\n\n# {}\n\n{}", frontmatter, title, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_store() -> (tempfile::TempDir, ArtifactStore) {
        let dir = tempdir().unwrap();
        let specs_dir = dir.path().join("specs");
        let store = ArtifactStore::with_specs_dir(dir.path(), specs_dir);
        store.ensure_directories().unwrap();
        (dir, store)
    }

    /// Helper to create a session for tests
    fn create_test_session(store: &ArtifactStore, state: &mut WorkstreamState) -> String {
        let session = store
            .create_artifact(
                state,
                ArtifactType::Session,
                "Test Session",
                "Session for testing",
                vec![],
                Priority::Medium,
                false,
            )
            .unwrap();
        session.metadata.id
    }

    #[test]
    fn test_create_use_case() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        // Create session first (UseCases require a Session parent)
        let session_id = create_test_session(&store, &mut state);

        let artifact = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "User logs in",
                "As a user, I want to log in.",
                vec![session_id],
                Priority::High,
                false,
            )
            .unwrap();

        assert_eq!(artifact.metadata.id, "UC-WON-123-001");
        assert_eq!(artifact.title, "User logs in");
        assert!(artifact.path.exists());

        // Verify file content
        let content = std::fs::read_to_string(&artifact.path).unwrap();
        assert!(content.contains("id: UC-WON-123-001"));
        assert!(content.contains("# User logs in"));
    }

    #[test]
    fn test_create_requirement_with_parent() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        // Create session first
        let session_id = create_test_session(&store, &mut state);

        // First create a use case
        let uc = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Content",
                vec![session_id],
                Priority::Medium,
                false,
            )
            .unwrap();

        // Then create a requirement
        let req = store
            .create_artifact(
                &mut state,
                ArtifactType::Requirement,
                "Test REQ",
                "Content",
                vec![uc.metadata.id.clone()],
                Priority::Medium,
                false,
            )
            .unwrap();

        assert_eq!(req.metadata.id, "REQ-WON-123-001");
        assert_eq!(req.metadata.parents, vec![uc.metadata.id]);
    }

    #[test]
    fn test_create_requirement_without_parent_fails() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        let result = store.create_artifact(
            &mut state,
            ArtifactType::Requirement,
            "Test REQ",
            "Content",
            vec![], // No parents
            Priority::Medium,
            false,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("requires at least one parent"));
    }

    #[test]
    fn test_read_artifact() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        // Create session first
        let session_id = create_test_session(&store, &mut state);

        let created = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Some content here.",
                vec![session_id],
                Priority::High,
                false,
            )
            .unwrap();

        let read = store.read_artifact(&created.metadata.id).unwrap().unwrap();

        assert_eq!(read.metadata.id, created.metadata.id);
        assert_eq!(read.title, "Test UC");
        assert_eq!(read.content, "Some content here.");
    }

    #[test]
    fn test_list_artifacts() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        // Create session first
        let session_id = create_test_session(&store, &mut state);

        store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "UC 1",
                "Content",
                vec![session_id.clone()],
                Priority::Medium,
                false,
            )
            .unwrap();

        store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "UC 2",
                "Content",
                vec![session_id],
                Priority::Medium,
                false,
            )
            .unwrap();

        let artifacts = store.list_artifacts(ArtifactType::UseCase).unwrap();
        assert_eq!(artifacts.len(), 2);
    }

    #[test]
    fn test_update_progress() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        // Create session first
        let session_id = create_test_session(&store, &mut state);

        let created = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Content",
                vec![session_id],
                Priority::Medium,
                false,
            )
            .unwrap();

        assert_eq!(created.metadata.progress, Progress::Backlog);

        let updated = store
            .update_artifact_progress(&created.metadata.id, Progress::InProgress)
            .unwrap();

        assert_eq!(updated.metadata.progress, Progress::InProgress);

        // Verify persisted
        let read = store.read_artifact(&created.metadata.id).unwrap().unwrap();
        assert_eq!(read.metadata.progress, Progress::InProgress);
    }

    #[test]
    fn test_staging() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        // Create session first
        let session_id = create_test_session(&store, &mut state);

        let artifact = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Staged UC",
                "Content",
                vec![session_id],
                Priority::Medium,
                true, // staging
            )
            .unwrap();

        assert!(artifact.path.to_string_lossy().contains("staging"));

        // Can still read it
        let read = store.read_artifact(&artifact.metadata.id).unwrap();
        assert!(read.is_some());
    }

    #[test]
    fn test_sanitize_title() {
        assert_eq!(sanitize_title("Hello World"), "hello-world");
        assert_eq!(sanitize_title("Test: Something (v2)"), "test-something-v2");
        assert_eq!(
            sanitize_title("Very Long Title That Should Be Truncated Eventually"),
            "very-long-title-that-should-be"
        );
    }

    #[test]
    fn test_parse_artifact_type_from_id() {
        assert_eq!(
            parse_artifact_type_from_id("UC-WON-123-001").unwrap(),
            ArtifactType::UseCase
        );
        assert_eq!(
            parse_artifact_type_from_id("REQ-WON-123-001").unwrap(),
            ArtifactType::Requirement
        );
        assert_eq!(
            parse_artifact_type_from_id("TASK-WON-123-001").unwrap(),
            ArtifactType::Task
        );
        assert!(parse_artifact_type_from_id("INVALID-123").is_err());
    }
}
