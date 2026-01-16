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

    #[test]
    fn snapshot_config_default_has_expected_values() {
        let config = SnapshotConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_age_days, 30);
        assert_eq!(config.max_per_session, 100);
        assert_eq!(config.max_total_size_mb, 500);
        assert!(config.auto_cleanup);
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

    #[tokio::test]
    async fn list_by_message_returns_snapshots_for_that_message() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        store
            .take(&[PathBuf::from("test.txt")], "s1", "msg_1", "First")
            .await
            .unwrap();
        store
            .take(&[PathBuf::from("test.txt")], "s1", "msg_1", "Second same msg")
            .await
            .unwrap();
        store
            .take(&[PathBuf::from("test.txt")], "s2", "msg_2", "Different msg")
            .await
            .unwrap();

        let msg1_snapshots = store.list_by_message("msg_1").await.unwrap();
        assert_eq!(msg1_snapshots.len(), 2);
        assert!(msg1_snapshots.iter().all(|s| s.message_id == "msg_1"));

        let msg2_snapshots = store.list_by_message("msg_2").await.unwrap();
        assert_eq!(msg2_snapshots.len(), 1);
    }

    #[tokio::test]
    async fn latest_for_file_returns_most_recent_snapshot() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("test.txt");
        let other_file = dir.path().join("other.txt");
        fs::write(&test_file, "content").await.unwrap();
        fs::write(&other_file, "other").await.unwrap();

        store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "First")
            .await
            .unwrap();
        let second = store
            .take(&[PathBuf::from("test.txt")], "s1", "m2", "Second")
            .await
            .unwrap();

        // Only other.txt
        store
            .take(&[PathBuf::from("other.txt")], "s1", "m3", "Other only")
            .await
            .unwrap();

        let latest = store
            .latest_for_file(Path::new("test.txt"))
            .await
            .unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, second.id);
    }

    #[tokio::test]
    async fn latest_for_file_returns_none_when_no_match() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "Test")
            .await
            .unwrap();

        let latest = store
            .latest_for_file(Path::new("nonexistent.txt"))
            .await
            .unwrap();
        assert!(latest.is_none());
    }

    #[tokio::test]
    async fn take_fails_when_snapshots_disabled() {
        let dir = TempDir::new().unwrap();
        let snapshot_dir = dir.path().join(".wonopcode/snapshots");
        let mut config = SnapshotConfig::default();
        config.enabled = false;

        let store = SnapshotStore::new(snapshot_dir, dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        let result = store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "Test")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("disabled"));
    }

    #[tokio::test]
    async fn take_fails_when_no_files_exist() {
        let (_, store) = setup_test().await;

        let result = store
            .take(&[PathBuf::from("nonexistent.txt")], "s1", "m1", "Test")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No files"));
    }

    #[tokio::test]
    async fn take_skips_nonexistent_files_but_succeeds_with_existing() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        let snapshot = store
            .take(
                &[PathBuf::from("test.txt"), PathBuf::from("nonexistent.txt")],
                "s1",
                "m1",
                "Test",
            )
            .await
            .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0], PathBuf::from("test.txt"));
    }

    #[tokio::test]
    async fn get_returns_not_found_for_missing_snapshot() {
        let (_, store) = setup_test().await;

        let fake_id = SnapshotId::from_string("nonexistent".to_string());
        let result = store.get(&fake_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_returns_not_found_for_missing_snapshot() {
        let (_, store) = setup_test().await;

        let fake_id = SnapshotId::from_string("nonexistent".to_string());
        let result = store.delete(&fake_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn diff_fails_when_file_not_in_snapshot() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        let snapshot = store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "Test")
            .await
            .unwrap();

        let result = store.diff(&snapshot.id, Path::new("other.txt")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not in snapshot"));
    }

    #[tokio::test]
    async fn normalize_path_strips_project_root_from_absolute_paths() {
        let (dir, store) = setup_test().await;

        let test_file = dir.path().join("subdir/test.txt");
        fs::create_dir_all(dir.path().join("subdir")).await.unwrap();
        fs::write(&test_file, "content").await.unwrap();

        // Use absolute path
        let snapshot = store
            .take(&[test_file.clone()], "s1", "m1", "Test")
            .await
            .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0], PathBuf::from("subdir/test.txt"));
    }

    #[tokio::test]
    async fn cleanup_deletes_excess_per_session_snapshots() {
        let dir = TempDir::new().unwrap();
        let snapshot_dir = dir.path().join(".wonopcode/snapshots");
        let mut config = SnapshotConfig::default();
        config.max_per_session = 2;
        config.auto_cleanup = false; // Manual cleanup

        let store = SnapshotStore::new(snapshot_dir, dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let test_file = dir.path().join("test.txt");
        fs::write(&test_file, "content").await.unwrap();

        // Create 3 snapshots for same session
        store
            .take(&[PathBuf::from("test.txt")], "s1", "m1", "First")
            .await
            .unwrap();
        store
            .take(&[PathBuf::from("test.txt")], "s1", "m2", "Second")
            .await
            .unwrap();
        store
            .take(&[PathBuf::from("test.txt")], "s1", "m3", "Third")
            .await
            .unwrap();

        let before = store.list().await.unwrap();
        assert_eq!(before.len(), 3);

        let deleted = store.cleanup().await.unwrap();
        assert!(deleted >= 1);

        let after = store.list().await.unwrap();
        assert!(after.len() <= 2);
    }

    #[test]
    fn generate_diff_produces_unified_diff_format() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nmodified\nline 3\n";

        let diff = generate_diff(old, new, Path::new("test.txt"));

        assert!(diff.contains("--- a/test.txt"));
        assert!(diff.contains("+++ b/test.txt"));
        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+modified"));
    }

    #[test]
    fn generate_diff_handles_empty_files() {
        let diff = generate_diff("", "new content\n", Path::new("new.txt"));
        assert!(diff.contains("+new content"));

        let diff2 = generate_diff("old content\n", "", Path::new("deleted.txt"));
        assert!(diff2.contains("-old content"));
    }

    #[test]
    fn generate_diff_handles_no_changes() {
        let content = "same\n";
        let diff = generate_diff(content, content, Path::new("same.txt"));
        // Should just have headers, no +/- lines
        assert!(diff.contains("--- a/same.txt"));
        assert!(!diff.contains("-same"));
        assert!(!diff.contains("+same"));
    }

    #[tokio::test]
    async fn restore_creates_parent_directories() {
        let (dir, store) = setup_test().await;

        // Create nested file
        let nested = dir.path().join("a/b/c/test.txt");
        fs::create_dir_all(nested.parent().unwrap()).await.unwrap();
        fs::write(&nested, "content").await.unwrap();

        let snapshot = store
            .take(&[PathBuf::from("a/b/c/test.txt")], "s1", "m1", "Test")
            .await
            .unwrap();

        // Delete the directories
        fs::remove_dir_all(dir.path().join("a")).await.unwrap();
        assert!(!nested.exists());

        // Restore should recreate directories
        store.restore(&snapshot.id).await.unwrap();
        assert!(nested.exists());

        let content = fs::read_to_string(&nested).await.unwrap();
        assert_eq!(content, "content");
    }
}
