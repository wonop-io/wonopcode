//! Artifact storage operations.
//!
//! Manages reading and writing artifacts to the changelog directory.
//! 
//! Storage model:
//! - All artifacts stored in `changelog/{ticket_id}/`
//! - Naming convention: `{KIND}-{index}--{ticket_id}--{title-slug}.md`
//! - Staging/committed status tracked via frontmatter `artifact_status` field

use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

use super::config::WonopCodeConfig;
use super::state::WorkstreamState;
use super::types::{
    Artifact, ArtifactMetadata, ArtifactStatus, ArtifactType, Priority, Progress, SessionLogImportance,
};

/// Manages artifact storage in the changelog directory.
pub struct ArtifactStore {
    #[allow(dead_code)]
    root_dir: PathBuf,
    /// Base changelog directory (e.g., /project/changelog)
    changelog_dir: PathBuf,
}

impl ArtifactStore {
    /// Create a new artifact store for the given root directory.
    pub fn new(root_dir: &Path) -> Result<Self> {
        tracing::debug!(
            "🗄️ ArtifactStore::new: Creating store for root_dir={}",
            root_dir.display()
        );

        let config = WonopCodeConfig::load(root_dir)?;
        let changelog_dir = config.specs_dir(root_dir);

        tracing::info!(
            "🗄️ ArtifactStore::new: root_dir={}, changelog_dir={}, ace_enabled={}",
            root_dir.display(),
            changelog_dir.display(),
            config.is_enabled()
        );

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            changelog_dir,
        })
    }

    /// Create a store with a custom changelog directory.
    pub fn with_specs_dir(root_dir: &Path, changelog_dir: PathBuf) -> Self {
        tracing::debug!(
            "🗄️ ArtifactStore::with_specs_dir: root_dir={}, changelog_dir={}",
            root_dir.display(),
            changelog_dir.display()
        );
        Self {
            root_dir: root_dir.to_path_buf(),
            changelog_dir,
        }
    }

    /// Get the changelog directory path.
    pub fn specs_dir(&self) -> &Path {
        &self.changelog_dir
    }

    /// Get the ticket directory path for a given ticket ID.
    pub fn ticket_dir(&self, ticket_id: &str) -> PathBuf {
        self.changelog_dir.join(ticket_id)
    }

    /// Ensure the directory structure exists for a ticket.
    pub fn ensure_directories(&self) -> Result<()> {
        tracing::debug!(
            "🗄️ ArtifactStore::ensure_directories: Creating base changelog dir {}",
            self.changelog_dir.display()
        );
        std::fs::create_dir_all(&self.changelog_dir)?;
        Ok(())
    }

    /// Ensure the ticket directory exists.
    pub fn ensure_ticket_dir(&self, ticket_id: &str) -> Result<PathBuf> {
        let dir = self.ticket_dir(ticket_id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Convert title to URL-safe slug.
    fn title_to_slug(title: &str) -> String {
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

    /// Get the next available index for an artifact type within a ticket.
    fn get_next_index(&self, ticket_id: &str, artifact_type: &ArtifactType) -> Result<u32> {
        let ticket_dir = self.ticket_dir(ticket_id);
        let prefix = artifact_type.prefix();
        
        if !ticket_dir.exists() {
            return Ok(1);
        }

        let mut max_index: u32 = 0;
        
        for entry in std::fs::read_dir(&ticket_dir)? {
            let entry = entry?;
            let filename = entry.file_name();
            let name = filename.to_string_lossy();
            
            // Check if filename starts with our prefix (e.g., "UC-01--")
            if name.starts_with(prefix) && name.ends_with(".md") {
                // Extract index from {PREFIX}-{INDEX}--
                if let Some(idx_str) = name.strip_prefix(prefix).and_then(|s| s.strip_prefix('-')) {
                    if let Some(idx_end) = idx_str.find("--") {
                        if let Ok(idx) = idx_str[..idx_end].parse::<u32>() {
                            max_index = max_index.max(idx);
                        }
                    }
                }
            }
        }

        Ok(max_index + 1)
    }

    /// Generate artifact filename: {KIND}-{index}--{ticket_id}--{slug}.md
    fn generate_filename(&self, artifact_type: &ArtifactType, index: u32, ticket_id: &str, title: &str) -> String {
        let slug = Self::title_to_slug(title);
        format!("{}-{:02}--{}--{}.md", artifact_type.prefix(), index, ticket_id, slug)
    }

    /// Generate artifact ID: {KIND}-{index}--{ticket_id}--{slug}
    fn generate_id(&self, artifact_type: &ArtifactType, index: u32, ticket_id: &str, title: &str) -> String {
        let slug = Self::title_to_slug(title);
        format!("{}-{:02}--{}--{}", artifact_type.prefix(), index, ticket_id, slug)
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
        let ticket_id = &state.ticket_id;
        
        tracing::info!(
            "🗄️ ArtifactStore::create_artifact_with_phase: type={}, title='{}', ticket={}, parents={:?}, phase={:?}, staging={}",
            artifact_type,
            title,
            ticket_id,
            parents,
            phase,
            staging
        );

        // Validate parents
        self.validate_parents(&artifact_type, &parents, ticket_id)?;

        // Get next index for this artifact type within the ticket
        let index = self.get_next_index(ticket_id, &artifact_type)?;

        // Generate ID and filename
        let id = self.generate_id(&artifact_type, index, ticket_id, title);
        let filename = self.generate_filename(&artifact_type, index, ticket_id, title);

        tracing::info!("🗄️ ArtifactStore: Generated artifact ID: {}", id);

        // Ensure ticket directory exists
        let ticket_dir = self.ensure_ticket_dir(ticket_id)?;
        let path = ticket_dir.join(&filename);

        // Create metadata with artifact_status
        let now = Utc::now();
        let artifact_status = if staging { ArtifactStatus::Staged } else { ArtifactStatus::Committed };
        
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
            artifact_status,
            ticket: Some(ticket_id.to_string()),
            kind: Some(artifact_type.directory().to_string()),
        };

        tracing::info!("🗄️ ArtifactStore: Writing file to: {}", path.display());

        // Write file
        let file_content = format_artifact(&metadata, title, content);
        std::fs::write(&path, &file_content)?;

        // Verify file was written
        if path.exists() {
            let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            tracing::info!(
                "🗄️ ArtifactStore: ✓ File written successfully: {} ({} bytes)",
                path.display(),
                file_size
            );
        }

        // Update sequence counter in state
        state.next_sequence(artifact_type.directory());

        Ok(Artifact {
            metadata,
            title: title.to_string(),
            content: content.to_string(),
            path,
        })
    }

    /// Read an artifact by ID.
    pub fn read_artifact(&self, id: &str) -> Result<Option<Artifact>> {
        // Extract ticket ID from artifact ID using the new format: {KIND}-{idx}--{ticket}--{slug}
        let ticket_id = match extract_ticket_id_from_new_format(id) {
            Some(t) => t,
            None => {
                // Try legacy format: {KIND}-{TICKET}-{SEQ}
                match super::types::extract_ticket_id_from_artifact_id(id) {
                    Some(t) => t,
                    None => return Ok(None),
                }
            }
        };

        let ticket_dir = self.ticket_dir(&ticket_id);
        
        if !ticket_dir.exists() {
            return Ok(None);
        }

        // Search for file starting with the ID
        for entry in std::fs::read_dir(&ticket_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let filename = path.file_stem().unwrap_or_default().to_string_lossy();
                if filename == id || filename.starts_with(&format!("{}-", id.split("--").next().unwrap_or(id))) {
                    // Parse and check if ID matches
                    if let Ok(artifact) = self.parse_artifact_file(&path) {
                        if artifact.metadata.id == id {
                            return Ok(Some(artifact));
                        }
                    }
                }
            }
        }

        // Also search in legacy format locations for backwards compatibility
        self.read_artifact_legacy(id)
    }

    /// Read artifact from legacy storage format (specs/{type}/).
    fn read_artifact_legacy(&self, id: &str) -> Result<Option<Artifact>> {
        let artifact_type = match parse_artifact_type_from_id(id) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        // Check legacy directories
        let dirs = [
            self.changelog_dir.join(artifact_type.directory()),
            self.changelog_dir.join("workspace").join("staging").join(artifact_type.directory()),
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

    /// List all artifacts for a specific ticket.
    pub fn list_all_artifacts_for_ticket(&self, ticket_id: &str) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();
        let ticket_dir = self.ticket_dir(ticket_id);

        if !ticket_dir.exists() {
            return Ok(artifacts);
        }

        for entry in std::fs::read_dir(&ticket_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Skip state files and non-markdown
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let filename = path.file_name().unwrap_or_default().to_string_lossy();
                if !filename.starts_with('_') {
                    if let Ok(artifact) = self.parse_artifact_file(&path) {
                        artifacts.push(artifact);
                    }
                }
            }
        }

        Ok(artifacts)
    }

    /// List artifacts of a given type for a ticket.
    pub fn list_artifacts_for_ticket(
        &self,
        artifact_type: ArtifactType,
        ticket_id: &str,
    ) -> Result<Vec<Artifact>> {
        let all = self.list_all_artifacts_for_ticket(ticket_id)?;
        Ok(all
            .into_iter()
            .filter(|a| a.metadata.artifact_type == artifact_type)
            .collect())
    }

    /// List all artifacts of a given type (across all tickets).
    pub fn list_artifacts(&self, artifact_type: ArtifactType) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();

        if !self.changelog_dir.exists() {
            return Ok(artifacts);
        }

        // Scan all ticket directories
        for entry in std::fs::read_dir(&self.changelog_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                // Skip special directories (products, etc.)
                if !dir_name.starts_with('.') && dir_name != "products" {
                    let ticket_artifacts = self.list_artifacts_for_ticket(artifact_type, &dir_name)?;
                    artifacts.extend(ticket_artifacts);
                }
            }
        }

        Ok(artifacts)
    }

    /// List all artifacts across all tickets.
    pub fn list_all_artifacts(&self) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();

        if !self.changelog_dir.exists() {
            return Ok(artifacts);
        }

        // Scan all ticket directories
        for entry in std::fs::read_dir(&self.changelog_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                // Skip special directories
                if !dir_name.starts_with('.') && dir_name != "products" {
                    let ticket_artifacts = self.list_all_artifacts_for_ticket(&dir_name)?;
                    artifacts.extend(ticket_artifacts);
                }
            }
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

    /// Promote an artifact from staged to committed.
    pub fn promote_artifact(&self, id: &str) -> Result<Artifact> {
        let artifact = self
            .read_artifact(id)?
            .ok_or_else(|| anyhow::anyhow!("Artifact not found: {}", id))?;

        if artifact.metadata.artifact_status == ArtifactStatus::Committed {
            bail!("Artifact {} is already committed", id);
        }

        let mut metadata = artifact.metadata.clone();
        metadata.artifact_status = ArtifactStatus::Committed;
        metadata.updated = Utc::now();

        // Rewrite file with updated status
        let file_content = format_artifact(&metadata, &artifact.title, &artifact.content);
        std::fs::write(&artifact.path, &file_content)?;

        Ok(Artifact {
            metadata,
            ..artifact
        })
    }

    /// Promote all staged artifacts of the given types.
    pub fn promote_artifacts_by_types(&self, types: &[ArtifactType]) -> Result<usize> {
        let mut count = 0;

        // Get all artifacts and filter by type and staged status
        let all_artifacts = self.list_all_artifacts()?;
        
        for artifact in all_artifacts {
            if types.contains(&artifact.metadata.artifact_type) 
                && artifact.metadata.artifact_status == ArtifactStatus::Staged 
            {
                if self.promote_artifact(&artifact.metadata.id).is_ok() {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Check if an artifact is staged.
    pub fn is_staged(&self, artifact: &Artifact) -> bool {
        artifact.metadata.artifact_status == ArtifactStatus::Staged
    }

    /// Ensure a session exists for the workstream.
    pub fn ensure_session(
        &self,
        state: &mut WorkstreamState,
        root_dir: &std::path::Path,
    ) -> Result<String> {
        // Check if session_id is set AND the session artifact actually exists
        if let Some(ref session_id) = state.session_id {
            if self.read_artifact(session_id)?.is_some() {
                return Ok(session_id.clone());
            }
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

    /// Create a new session artifact.
    pub fn create_session(
        &self,
        state: &mut WorkstreamState,
        title: &str,
    ) -> Result<Artifact> {
        let now = Utc::now();
        let date_str = now.format("%Y-%m-%d").to_string();

        let content = format!(
            "## Context

(Session context will be added here)

## Changelog

### {}

- 🟢 {} Session started
",
            date_str,
            now.format("%H:%M")
        );

        self.create_artifact_with_phase(
            state,
            ArtifactType::Session,
            title,
            &content,
            vec![],
            Priority::High,
            false, // Sessions go directly to committed
            None,
        )
    }

    /// Append a log entry to the session's changelog.
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
        let entry = format!("{}- {} {} {}
", indent_str, emoji, time_str, message);

        let mut new_content = session.content.clone();
        let date_header = format!("### {}", date_str);

        if !new_content.contains(&date_header) {
            new_content.push_str(&format!("
{}

", date_header));
        }

        new_content.push_str(&entry);

        self.update_artifact_content(session_id, &session.title, &new_content)
    }

    /// Validate parents for the given artifact type.
    fn validate_parents(
        &self,
        artifact_type: &ArtifactType,
        parents: &[String],
        ticket_id: &str,
    ) -> Result<()> {
        let valid_types = artifact_type.valid_parent_types();

        if artifact_type.requires_parent() && parents.is_empty() {
            bail!(
                "{} requires at least one parent of type: {}",
                artifact_type,
                valid_types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
            );
        }

        for parent_id in parents {
            let parent_type = parse_artifact_type_from_id(parent_id)?;

            if !valid_types.contains(&parent_type) {
                bail!(
                    "Invalid parent type {} for {}. Valid types: {}",
                    parent_type,
                    artifact_type,
                    valid_types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
                );
            }

            // Verify parent exists
            let parent_artifact = self.read_artifact(parent_id)?
                .ok_or_else(|| anyhow::anyhow!("Parent artifact not found: {}", parent_id))?;

            // Verify parent belongs to the same ticket
            if !parent_artifact.belongs_to_ticket(ticket_id) {
                bail!(
                    "Parent artifact {} does not belong to the current workstream (ticket: {})",
                    parent_id,
                    ticket_id
                );
            }
        }

        Ok(())
    }

    /// Parse an artifact file.
    fn parse_artifact_file(&self, path: &Path) -> Result<Artifact> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read artifact: {}", path.display()))?;

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            bail!("Invalid artifact format: missing frontmatter in {}", path.display());
        }

        let frontmatter = parts[1].trim();
        let body = parts[2].trim();

        let metadata: ArtifactMetadata = serde_yaml::from_str(frontmatter)
            .with_context(|| format!("Failed to parse frontmatter in {}", path.display()))?;

        let title = body
            .lines()
            .find(|line| line.starts_with("# "))
            .map(|line| line.trim_start_matches("# ").to_string())
            .unwrap_or_default();

        let content_lines: Vec<&str> = body.lines().collect();
        let title_index = content_lines.iter().position(|line| line.starts_with("# ")).unwrap_or(0);
        let content = content_lines
            .into_iter()
            .skip(title_index + 1)
            .collect::<Vec<_>>()
            .join("
")
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

/// Extract ticket ID from new format: {KIND}-{idx}--{ticket}--{slug}
fn extract_ticket_id_from_new_format(id: &str) -> Option<String> {
    let parts: Vec<&str> = id.split("--").collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Parse artifact type from ID prefix.
pub fn parse_artifact_type_from_id(id: &str) -> Result<ArtifactType> {
    // Handle new format: {KIND}-{idx}--{ticket}--{slug}
    let prefix = if id.contains("--") {
        id.split('-').next().unwrap_or("")
    } else {
        // Legacy format: {KIND}-{TICKET}-{SEQ}
        id.split('-').next().unwrap_or("")
    };
    
    ArtifactType::from_prefix(prefix)
        .ok_or_else(|| anyhow::anyhow!("Unknown artifact type prefix: {}", prefix))
}

/// Format artifact as markdown with YAML frontmatter.
fn format_artifact(metadata: &ArtifactMetadata, title: &str, content: &str) -> String {
    let frontmatter = serde_yaml::to_string(metadata).unwrap_or_default();
    format!("---
{}---

# {}

{}", frontmatter, title, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_store() -> (tempfile::TempDir, ArtifactStore) {
        let dir = tempdir().unwrap();
        let changelog_dir = dir.path().join("changelog");
        let store = ArtifactStore::with_specs_dir(dir.path(), changelog_dir);
        store.ensure_directories().unwrap();
        (dir, store)
    }

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

        // New ID format: UC-01--WON-123--user-logs-in
        assert!(artifact.metadata.id.starts_with("UC-01--WON-123--"));
        assert_eq!(artifact.title, "User logs in");
        assert!(artifact.path.exists());

        // Verify stored in changelog/WON-123/
        assert!(artifact.path.to_string_lossy().contains("changelog/WON-123/"));
    }

    #[test]
    fn test_artifact_status() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        let session_id = create_test_session(&store, &mut state);

        // Create staged artifact
        let staged = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Staged UC",
                "Content",
                vec![session_id.clone()],
                Priority::Medium,
                true, // staging = true
            )
            .unwrap();

        assert_eq!(staged.metadata.artifact_status, ArtifactStatus::Staged);
        assert!(store.is_staged(&staged));

        // Create committed artifact
        let committed = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Committed UC",
                "Content",
                vec![session_id],
                Priority::Medium,
                false, // staging = false
            )
            .unwrap();

        assert_eq!(committed.metadata.artifact_status, ArtifactStatus::Committed);
        assert!(!store.is_staged(&committed));
    }

    #[test]
    fn test_promote_artifact() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        let session_id = create_test_session(&store, &mut state);

        let artifact = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Content",
                vec![session_id],
                Priority::Medium,
                true, // staged
            )
            .unwrap();

        assert!(store.is_staged(&artifact));

        // Promote
        let promoted = store.promote_artifact(&artifact.metadata.id).unwrap();
        assert_eq!(promoted.metadata.artifact_status, ArtifactStatus::Committed);
        assert!(!store.is_staged(&promoted));
    }

    #[test]
    fn test_title_to_slug() {
        assert_eq!(ArtifactStore::title_to_slug("Hello World"), "hello-world");
        assert_eq!(ArtifactStore::title_to_slug("Test: Something (v2)"), "test-something-v2");
        assert_eq!(
            ArtifactStore::title_to_slug("Very Long Title That Should Be Truncated Eventually"),
            "very-long-title-that-should-be"
        );
    }
}
