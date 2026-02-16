//! Artifact storage operations.
//!
//! Manages reading and writing artifacts to the specs directory.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

use super::config::WonopCodeConfig;
use super::state::WorkstreamState;
use super::types::{Artifact, ArtifactMetadata, ArtifactType, Priority, Progress};

/// Manages artifact storage in the specs directory.
pub struct ArtifactStore {
    #[allow(dead_code)]
    root_dir: PathBuf,
    specs_dir: PathBuf,
}

impl ArtifactStore {
    /// Create a new artifact store for the given root directory.
    pub fn new(root_dir: &Path) -> Result<Self> {
        let config = WonopCodeConfig::load(root_dir)?;
        let specs_dir = config.specs_dir(root_dir);

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            specs_dir,
        })
    }

    /// Create a store with a custom specs directory.
    pub fn with_specs_dir(root_dir: &Path, specs_dir: PathBuf) -> Self {
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
        for dir in ["use-cases", "requirements", "designs", "tests", "tasks"] {
            std::fs::create_dir_all(self.specs_dir.join(dir))?;
        }
        // Create staging directory
        let staging = self.specs_dir.join("workspace").join("staging");
        for dir in ["use-cases", "requirements", "designs", "tests", "tasks"] {
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
        // Validate parents
        self.validate_parents(&artifact_type, &parents)?;

        // Generate ID
        let seq = state.next_sequence(artifact_type.directory());
        let id = format!("{}-{}-{:03}", artifact_type.prefix(), state.ticket_id, seq);

        // Create metadata
        let now = Utc::now();
        let metadata = ArtifactMetadata {
            id: id.clone(),
            artifact_type,
            progress: Progress::Backlog,
            parents,
            priority,
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
        std::fs::create_dir_all(&dir)?;

        let filename = format!("{}-{}.md", id, sanitize_title(title));
        let path = dir.join(&filename);

        // Write file
        let file_content = format_artifact(&metadata, title, content);
        std::fs::write(&path, &file_content)?;

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

    /// List all artifacts of all types.
    pub fn list_all_artifacts(&self) -> Result<Vec<Artifact>> {
        let mut artifacts = Vec::new();

        for artifact_type in [
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
        let new_path = target_dir.join(artifact.path.file_name().unwrap());

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

    /// Validate that parents are valid for the given artifact type.
    fn validate_parents(&self, artifact_type: &ArtifactType, parents: &[String]) -> Result<()> {
        let valid_types = artifact_type.valid_parent_types();

        if artifact_type.requires_parent() && parents.is_empty() {
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
            let parent_type = parse_artifact_type_from_id(parent_id)?;
            if !valid_types.contains(&parent_type) {
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
            if self.read_artifact(parent_id)?.is_none() {
                bail!("Parent artifact not found: {}", parent_id);
            }
        }

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

    #[test]
    fn test_create_use_case() {
        let (_dir, store) = create_test_store();
        let mut state = WorkstreamState::new("WON-123");

        let artifact = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "User logs in",
                "As a user, I want to log in.",
                vec![],
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

        // First create a use case
        let uc = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Content",
                vec![],
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

        let created = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Some content here.",
                vec![],
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

        store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "UC 1",
                "Content",
                vec![],
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
                vec![],
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

        let created = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Content",
                vec![],
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

        let artifact = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Staged UC",
                "Content",
                vec![],
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
