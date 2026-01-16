//! Snapshot storage implementation.

use crate::{Snapshot, SnapshotError, SnapshotId, SnapshotResult};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Configuration for snapshot storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Whether snapshots are enabled.
    pub enabled: bool,

    /// Maximum age of snapshots in days.
    pub max_age_days: u32,

    /// Maximum number of snapshots per session.
    pub max_per_session: u32,

    /// Maximum total storage size in MB.
    pub max_total_size_mb: u32,

    /// Whether to automatically clean up old snapshots.
    pub auto_cleanup: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_age_days: 30,
            max_per_session: 100,
            max_total_size_mb: 500,
            auto_cleanup: true,
        }
    }
}

/// Storage for file snapshots.
///
/// Snapshots are stored as simple file copies in a directory structure:
/// ```text
/// base_dir/
///   metadata.json          # List of all snapshots
///   snapshots/
///     <snapshot_id>/
///       metadata.json      # Snapshot metadata
///       files/
///         <relative_path>  # Actual file copies
/// ```
pub struct SnapshotStore {
    /// Base directory for snapshot storage.
    base_dir: PathBuf,

    /// Project root directory (for resolving relative paths).
    project_root: PathBuf,

    /// Configuration.
    config: SnapshotConfig,
}

impl SnapshotStore {
    /// Create a new snapshot store.
    ///
    /// # Arguments
    /// * `base_dir` - Directory to store snapshots (e.g., `.wonopcode/snapshots`)
    /// * `project_root` - Root directory of the project
    /// * `config` - Snapshot configuration
    pub async fn new(
        base_dir: PathBuf,
        project_root: PathBuf,
        config: SnapshotConfig,
    ) -> SnapshotResult<Self> {
        if !config.enabled {
            debug!("Snapshots are disabled");
        }

        // Create base directory if it doesn't exist
        fs::create_dir_all(&base_dir).await?;
        fs::create_dir_all(base_dir.join("snapshots")).await?;

        Ok(Self {
            base_dir,
            project_root,
            config,
        })
    }

    /// Take a snapshot of the specified files.
    ///
    /// # Arguments
    /// * `files` - List of file paths (absolute or relative to project root)
    /// * `session_id` - ID of the current session
    /// * `message_id` - ID of the current message
    /// * `description` - Description of why the snapshot was taken
    #[allow(clippy::cognitive_complexity)]
    pub async fn take(
        &self,
        files: &[PathBuf],
        session_id: &str,
        message_id: &str,
        description: &str,
    ) -> SnapshotResult<Snapshot> {
        if !self.config.enabled {
            return Err(SnapshotError::operation_failed("Snapshots are disabled"));
        }

        // Normalize file paths to be relative to project root
        let mut normalized_files = Vec::new();
        for file in files {
            let normalized = self.normalize_path(file)?;
            if self.project_root.join(&normalized).exists() {
                normalized_files.push(normalized);
            } else {
                warn!("Skipping non-existent file: {:?}", file);
            }
        }

        if normalized_files.is_empty() {
            return Err(SnapshotError::operation_failed("No files to snapshot"));
        }

        let snapshot = Snapshot::new(session_id, message_id, description, normalized_files);

        // Create snapshot directory
        let snapshot_dir = self.snapshot_dir(&snapshot.id);
        let files_dir = snapshot_dir.join("files");
        fs::create_dir_all(&files_dir).await?;

        // Copy files
        for file in &snapshot.files {
            let src = self.project_root.join(file);
            let dst = files_dir.join(file);

            // Create parent directories
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).await?;
            }

            // Copy file
            fs::copy(&src, &dst).await.map_err(|e| {
                SnapshotError::operation_failed(format!("Failed to copy {}: {}", src.display(), e))
            })?;

            debug!("Snapshotted: {:?}", file);
        }

        // Save metadata
        let metadata_path = snapshot_dir.join("metadata.json");
        let metadata_json = serde_json::to_string_pretty(&snapshot)?;
        fs::write(&metadata_path, metadata_json).await?;

        info!(
            "Created snapshot {} with {} files",
            snapshot.id,
            snapshot.files.len()
        );

        // Auto cleanup if enabled
        if self.config.auto_cleanup {
            if let Err(e) = self.cleanup().await {
                warn!("Snapshot cleanup failed: {}", e);
            }
        }

        Ok(snapshot)
    }

    /// Restore files from a snapshot.
    ///
    /// # Arguments
    /// * `snapshot_id` - ID of the snapshot to restore
    #[allow(clippy::cognitive_complexity)]
    pub async fn restore(&self, snapshot_id: &SnapshotId) -> SnapshotResult<Snapshot> {
        let snapshot = self.get(snapshot_id).await?;
        let snapshot_dir = self.snapshot_dir(snapshot_id);
        let files_dir = snapshot_dir.join("files");

        for file in &snapshot.files {
            let src = files_dir.join(file);
            let dst = self.project_root.join(file);

            if !src.exists() {
                warn!("Snapshot file missing: {:?}", src);
                continue;
            }

            // Create parent directories
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).await?;
            }

            // Copy file back
            fs::copy(&src, &dst).await.map_err(|e| {
                SnapshotError::operation_failed(format!(
                    "Failed to restore {}: {}",
                    dst.display(),
                    e
                ))
            })?;

            debug!("Restored: {:?}", file);
        }

        info!(
            "Restored snapshot {} ({} files)",
            snapshot_id,
            snapshot.files.len()
        );

        Ok(snapshot)
    }

    /// Get a snapshot by ID.
    pub async fn get(&self, snapshot_id: &SnapshotId) -> SnapshotResult<Snapshot> {
        let snapshot_dir = self.snapshot_dir(snapshot_id);
        let metadata_path = snapshot_dir.join("metadata.json");

        if !metadata_path.exists() {
            return Err(SnapshotError::not_found(snapshot_id.as_str()));
        }

        let metadata_json = fs::read_to_string(&metadata_path).await?;
        let snapshot: Snapshot = serde_json::from_str(&metadata_json)?;

        Ok(snapshot)
    }

    /// List all snapshots.
    pub async fn list(&self) -> SnapshotResult<Vec<Snapshot>> {
        let snapshots_dir = self.base_dir.join("snapshots");
        let mut snapshots = Vec::new();

        let mut entries = fs::read_dir(&snapshots_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let snapshot_id =
                    SnapshotId::from_string(entry.file_name().to_string_lossy().to_string());
                match self.get(&snapshot_id).await {
                    Ok(snapshot) => snapshots.push(snapshot),
                    Err(e) => warn!("Failed to load snapshot {:?}: {}", entry.path(), e),
                }
            }
        }

        // Sort by timestamp (newest first)
        snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(snapshots)
    }

    /// List snapshots for a specific session.
    pub async fn list_by_session(&self, session_id: &str) -> SnapshotResult<Vec<Snapshot>> {
        let all = self.list().await?;
        Ok(all
            .into_iter()
            .filter(|s| s.session_id == session_id)
            .collect())
    }

    /// List snapshots for a specific message.
    pub async fn list_by_message(&self, message_id: &str) -> SnapshotResult<Vec<Snapshot>> {
        let all = self.list().await?;
        Ok(all
            .into_iter()
            .filter(|s| s.message_id == message_id)
            .collect())
    }

    /// Get the most recent snapshot for a file.
    pub async fn latest_for_file(&self, file: &Path) -> SnapshotResult<Option<Snapshot>> {
        let normalized = self.normalize_path(file)?;
        let all = self.list().await?;

        Ok(all.into_iter().find(|s| s.files.contains(&normalized)))
    }

    /// Generate a diff between a snapshot and the current file state.
    pub async fn diff(&self, snapshot_id: &SnapshotId, file: &Path) -> SnapshotResult<String> {
        let snapshot = self.get(snapshot_id).await?;
        let normalized = self.normalize_path(file)?;

        if !snapshot.files.contains(&normalized) {
            return Err(SnapshotError::operation_failed(format!(
                "File {file:?} not in snapshot {snapshot_id}"
            )));
        }

        let snapshot_file = self
            .snapshot_dir(snapshot_id)
            .join("files")
            .join(&normalized);
        let current_file = self.project_root.join(&normalized);

        let old_content = fs::read_to_string(&snapshot_file).await.unwrap_or_default();
        let new_content = fs::read_to_string(&current_file).await.unwrap_or_default();

        Ok(generate_diff(&old_content, &new_content, &normalized))
    }

    /// Delete a snapshot.
    pub async fn delete(&self, snapshot_id: &SnapshotId) -> SnapshotResult<()> {
        let snapshot_dir = self.snapshot_dir(snapshot_id);

        if !snapshot_dir.exists() {
            return Err(SnapshotError::not_found(snapshot_id.as_str()));
        }

        fs::remove_dir_all(&snapshot_dir).await?;
        info!("Deleted snapshot {}", snapshot_id);

        Ok(())
    }

    /// Clean up old snapshots based on configuration.
    #[allow(clippy::cognitive_complexity)]
    pub async fn cleanup(&self) -> SnapshotResult<u32> {
        let mut deleted = 0;
        let cutoff = Utc::now() - Duration::days(self.config.max_age_days as i64);

        let snapshots = self.list().await?;

        // Delete old snapshots
        for snapshot in &snapshots {
            if snapshot.timestamp < cutoff {
                if let Err(e) = self.delete(&snapshot.id).await {
                    warn!("Failed to delete old snapshot {}: {}", snapshot.id, e);
                } else {
                    deleted += 1;
                }
            }
        }

        // Count per session and delete excess
        let mut session_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        for snapshot in &snapshots {
            let count = session_counts
                .entry(snapshot.session_id.clone())
                .or_insert(0);
            *count += 1;

            if *count > self.config.max_per_session {
                if let Err(e) = self.delete(&snapshot.id).await {
                    warn!("Failed to delete excess snapshot {}: {}", snapshot.id, e);
                } else {
                    deleted += 1;
                }
            }
        }

        if deleted > 0 {
            info!("Cleaned up {} snapshots", deleted);
        }

        Ok(deleted)
    }

    /// Get the directory for a snapshot.
    fn snapshot_dir(&self, snapshot_id: &SnapshotId) -> PathBuf {
        self.base_dir.join("snapshots").join(snapshot_id.as_str())
    }

    /// Normalize a file path to be relative to project root.
    fn normalize_path(&self, path: &Path) -> SnapshotResult<PathBuf> {
        if path.is_absolute() {
            // Strip project root if present
            path.strip_prefix(&self.project_root)
                .map(|p| p.to_path_buf())
                .map_err(|_| {
                    SnapshotError::operation_failed(format!(
                        "Path {:?} is not under project root {:?}",
                        path, self.project_root
                    ))
                })
        } else {
            Ok(path.to_path_buf())
        }
    }
}

/// Generate a unified diff between two strings.
fn generate_diff(old: &str, new: &str, path: &Path) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    output.push_str(&format!("--- a/{}\n", path.display()));
    output.push_str(&format!("+++ b/{}\n", path.display()));

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            output.push_str("...\n");
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                output.push_str(sign);
                output.push_str(change.value());
                if !change.value().ends_with('\n') {
                    output.push('\n');
                }
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test() -> (TempDir, SnapshotStore) {
        let dir = TempDir::new().unwrap();
        let snapshot_dir = dir.path().join(".wonopcode/snapshots");
        let store = SnapshotStore::new(
            snapshot_dir,
            dir.path().to_path_buf(),
            SnapshotConfig::default(),
        )
        .await
        .unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn test_take_and_restore_snapshot() {
        let (dir, store) = setup_test().await;

        // Create a test file
        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "original content").await.unwrap();

        // Take snapshot
        let snapshot = store
            .take(
                &[PathBuf::from("test.txt")],
                "session_1",
                "msg_1",
                "Before edit",
            )
            .await
            .unwrap();

        assert_eq!(snapshot.session_id, "session_1");
        assert_eq!(snapshot.files.len(), 1);

        // Modify the file
        fs::write(&test_file, "modified content").await.unwrap();

        // Restore snapshot
        store.restore(&snapshot.id).await.unwrap();

        // Verify content restored
        let content = fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(content, "original content");
    }

    #[tokio::test]
    async fn test_list_snapshots() {
        let (dir, store) = setup_test().await;

        // Create test files
        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        // Take multiple snapshots
        store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "Snapshot 1")
            .await
            .unwrap();
        store
            .take(&[PathBuf::from("test.txt")], "s1", "m2", "Snapshot 2")
            .await
            .unwrap();
        store
            .take(&[PathBuf::from("test.txt")], "s2", "m3", "Snapshot 3")
            .await
            .unwrap();

        // List all
        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 3);

        // List by session
        let s1_snapshots = store.list_by_session("s1").await.unwrap();
        assert_eq!(s1_snapshots.len(), 2);
    }

    #[tokio::test]
    async fn test_diff() {
        let (dir, store) = setup_test().await;

        // Create test file
        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "line 1\nline 2\nline 3\n")
            .await
            .unwrap();

        // Take snapshot
        let snapshot = store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "Original")
            .await
            .unwrap();

        // Modify file
        fs::write(&test_file, "line 1\nmodified line\nline 3\n")
            .await
            .unwrap();

        // Get diff
        let diff = store
            .diff(&snapshot.id, Path::new("test.txt"))
            .await
            .unwrap();

        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+modified line"));
    }

    #[tokio::test]
    async fn test_delete_snapshot() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        let snapshot = store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "Test")
            .await
            .unwrap();

        // Delete
        store.delete(&snapshot.id).await.unwrap();

        // Verify deleted
        let result = store.get(&snapshot.id).await;
        assert!(result.is_err());
    }
}
