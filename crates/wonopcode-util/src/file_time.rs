//! File time tracking for concurrent edit detection.
//!
//! This module tracks when files are read during a session to detect if
//! files have been modified externally between read and write operations.
//! This prevents data loss from overwriting changes made outside the session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Error type for file time assertions.
#[derive(Debug, Clone)]
pub enum FileTimeError {
    /// File has not been read in this session.
    NotRead { path: PathBuf },
    /// File was modified after it was read.
    ModifiedSinceRead {
        path: PathBuf,
        last_read: SystemTime,
        last_modified: SystemTime,
    },
    /// File system error while checking modification time.
    IoError { path: PathBuf, error: String },
}

impl std::fmt::Display for FileTimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileTimeError::NotRead { path } => {
                write!(
                    f,
                    "You must read the file {} before overwriting it. Use the Read tool first.",
                    path.display()
                )
            }
            FileTimeError::ModifiedSinceRead {
                path,
                last_read,
                last_modified,
            } => {
                let read_str = format_time(*last_read);
                let modified_str = format_time(*last_modified);
                write!(
                    f,
                    "File {} has been modified since it was last read.\n\
                     Last modification: {}\n\
                     Last read: {}\n\n\
                     Please read the file again before modifying it.",
                    path.display(),
                    modified_str,
                    read_str
                )
            }
            FileTimeError::IoError { path, error } => {
                write!(
                    f,
                    "Failed to check modification time for {}: {}",
                    path.display(),
                    error
                )
            }
        }
    }
}

impl std::error::Error for FileTimeError {}

/// Format a SystemTime for display.
fn format_time(time: SystemTime) -> String {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => {
            let secs = duration.as_secs();
            // Simple formatting - could use chrono for better output
            format!("{}s since epoch", secs)
        }
        Err(_) => "unknown time".to_string(),
    }
}

/// File time tracker for a session.
///
/// This tracks when files are read during a session, allowing us to detect
/// if files have been modified externally before we write to them.
#[derive(Debug, Default)]
pub struct FileTimeTracker {
    /// Map from file path to the time it was last read in this session.
    read_times: HashMap<PathBuf, SystemTime>,
}

impl FileTimeTracker {
    /// Create a new file time tracker.
    pub fn new() -> Self {
        Self {
            read_times: HashMap::new(),
        }
    }

    /// Record that a file was read at the current time.
    pub fn record_read(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref().to_path_buf();
        let now = SystemTime::now();
        debug!(path = %path.display(), "Recording file read time");
        self.read_times.insert(path, now);
    }

    /// Get the last read time for a file.
    pub fn get_read_time(&self, path: impl AsRef<Path>) -> Option<SystemTime> {
        self.read_times.get(path.as_ref()).copied()
    }

    /// Check if a file has been modified since it was last read.
    ///
    /// Returns `Ok(())` if the file can be safely modified.
    /// Returns `Err(FileTimeError)` if:
    /// - The file hasn't been read in this session
    /// - The file has been modified since it was read
    pub fn assert_not_modified(&self, path: impl AsRef<Path>) -> Result<(), FileTimeError> {
        let path = path.as_ref();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Get the last read time
        let read_time = self.read_times.get(&canonical).or_else(|| {
            // Try without canonicalization
            self.read_times.get(path)
        });

        let read_time = match read_time {
            Some(t) => *t,
            None => {
                return Err(FileTimeError::NotRead {
                    path: path.to_path_buf(),
                });
            }
        };

        // Get the file's current modification time
        let metadata = std::fs::metadata(path).map_err(|e| FileTimeError::IoError {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;

        let mtime = metadata.modified().map_err(|e| FileTimeError::IoError {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;

        // Check if the file was modified after we read it
        if mtime > read_time {
            warn!(
                path = %path.display(),
                "File modified since last read"
            );
            return Err(FileTimeError::ModifiedSinceRead {
                path: path.to_path_buf(),
                last_read: read_time,
                last_modified: mtime,
            });
        }

        Ok(())
    }

    /// Assert that a file can be modified, but only if it exists.
    /// For new files, no check is performed.
    pub fn assert_if_exists(&self, path: impl AsRef<Path>) -> Result<(), FileTimeError> {
        let path = path.as_ref();
        if path.exists() {
            self.assert_not_modified(path)
        } else {
            Ok(())
        }
    }

    /// Clear all tracked read times.
    pub fn clear(&mut self) {
        self.read_times.clear();
    }

    /// Remove tracking for a specific file.
    pub fn forget(&mut self, path: impl AsRef<Path>) {
        self.read_times.remove(path.as_ref());
    }
}

/// Global file time state manager for all sessions.
///
/// This provides thread-safe access to per-session file time trackers.
pub struct FileTimeState {
    /// Map from session ID to file time tracker.
    sessions: RwLock<HashMap<String, FileTimeTracker>>,
}

impl FileTimeState {
    /// Create a new global file time state.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Record that a file was read in a session.
    pub async fn record_read(&self, session_id: &str, path: impl AsRef<Path>) {
        let mut sessions = self.sessions.write().await;
        sessions
            .entry(session_id.to_string())
            .or_insert_with(FileTimeTracker::new)
            .record_read(path);
    }

    /// Assert that a file has not been modified since it was read.
    pub async fn assert_not_modified(
        &self,
        session_id: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), FileTimeError> {
        let sessions = self.sessions.read().await;
        match sessions.get(session_id) {
            Some(tracker) => tracker.assert_not_modified(path),
            None => Err(FileTimeError::NotRead {
                path: path.as_ref().to_path_buf(),
            }),
        }
    }

    /// Assert that a file can be modified, but only if it exists.
    pub async fn assert_if_exists(
        &self,
        session_id: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), FileTimeError> {
        let path = path.as_ref();
        if path.exists() {
            self.assert_not_modified(session_id, path).await
        } else {
            Ok(())
        }
    }

    /// Clear all tracked read times for a session.
    pub async fn clear_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }
}

impl Default for FileTimeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a shared file time state.
pub fn shared_file_time_state() -> Arc<FileTimeState> {
    Arc::new(FileTimeState::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_tracker_read_and_assert() {
        let mut tracker = FileTimeTracker::new();

        // Create a temp file
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test content").unwrap();
        let path = file.path();

        // Should fail before reading
        assert!(matches!(
            tracker.assert_not_modified(path),
            Err(FileTimeError::NotRead { .. })
        ));

        // Record read
        tracker.record_read(path);

        // Should succeed after reading (file unchanged)
        assert!(tracker.assert_not_modified(path).is_ok());
    }

    #[test]
    fn test_tracker_detects_modification() {
        let mut tracker = FileTimeTracker::new();

        // Create a temp file
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test content").unwrap();
        let path = file.path().to_path_buf();

        // Record read
        tracker.record_read(&path);

        // Wait a bit and modify the file
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        writeln!(file, "modified content").unwrap();

        // Should fail because file was modified
        assert!(matches!(
            tracker.assert_not_modified(&path),
            Err(FileTimeError::ModifiedSinceRead { .. })
        ));
    }

    #[test]
    fn test_assert_if_exists_new_file() {
        let tracker = FileTimeTracker::new();

        // Non-existent file should pass
        let path = PathBuf::from("/nonexistent/file.txt");
        assert!(tracker.assert_if_exists(path).is_ok());
    }

    #[tokio::test]
    async fn test_state_multiple_sessions() {
        let state = FileTimeState::new();

        // Create temp files
        let mut file1 = NamedTempFile::new().unwrap();
        writeln!(file1, "file 1").unwrap();
        let path1 = file1.path().to_path_buf();

        let mut file2 = NamedTempFile::new().unwrap();
        writeln!(file2, "file 2").unwrap();
        let path2 = file2.path().to_path_buf();

        // Record reads in different sessions
        state.record_read("session1", &path1).await;
        state.record_read("session2", &path2).await;

        // Session 1 can access file 1 but not file 2
        assert!(state.assert_not_modified("session1", &path1).await.is_ok());
        assert!(matches!(
            state.assert_not_modified("session1", &path2).await,
            Err(FileTimeError::NotRead { .. })
        ));

        // Session 2 can access file 2 but not file 1
        assert!(state.assert_not_modified("session2", &path2).await.is_ok());
        assert!(matches!(
            state.assert_not_modified("session2", &path1).await,
            Err(FileTimeError::NotRead { .. })
        ));
    }
}
