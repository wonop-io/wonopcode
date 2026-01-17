//! Snapshot data structures.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    /// Create a new random snapshot ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create a snapshot ID from a string.
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the ID as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SnapshotId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A snapshot of one or more files at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unique identifier for this snapshot.
    pub id: SnapshotId,

    /// ID of the session that created this snapshot.
    pub session_id: String,

    /// ID of the message that triggered this snapshot.
    pub message_id: String,

    /// When the snapshot was taken.
    pub timestamp: DateTime<Utc>,

    /// Description of what changed.
    pub description: String,

    /// Files included in this snapshot (relative paths).
    pub files: Vec<PathBuf>,

    /// The tool or operation that triggered this snapshot.
    #[serde(default)]
    pub trigger: Option<String>,
}

impl Snapshot {
    /// Create a new snapshot.
    pub fn new(
        session_id: impl Into<String>,
        message_id: impl Into<String>,
        description: impl Into<String>,
        files: Vec<PathBuf>,
    ) -> Self {
        Self {
            id: SnapshotId::new(),
            session_id: session_id.into(),
            message_id: message_id.into(),
            timestamp: Utc::now(),
            description: description.into(),
            files,
            trigger: None,
        }
    }

    /// Set the trigger for this snapshot.
    pub fn with_trigger(mut self, trigger: impl Into<String>) -> Self {
        self.trigger = Some(trigger.into());
        self
    }

    /// Check if this snapshot includes a specific file.
    pub fn contains_file(&self, path: &PathBuf) -> bool {
        self.files.iter().any(|f| f == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_id_new_is_unique() {
        let id1 = SnapshotId::new();
        let id2 = SnapshotId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn snapshot_id_from_string() {
        let id = SnapshotId::from_string("test-id-123");
        assert_eq!(id.as_str(), "test-id-123");
    }

    #[test]
    fn snapshot_id_default() {
        let id = SnapshotId::default();
        assert!(!id.as_str().is_empty());
    }

    #[test]
    fn snapshot_id_display() {
        let id = SnapshotId::from_string("snap-abc");
        assert_eq!(format!("{}", id), "snap-abc");
    }

    #[test]
    fn snapshot_new_creates_with_timestamp() {
        let snapshot = Snapshot::new("session-1", "message-1", "test snapshot", vec![]);
        assert_eq!(snapshot.session_id, "session-1");
        assert_eq!(snapshot.message_id, "message-1");
        assert_eq!(snapshot.description, "test snapshot");
        assert!(snapshot.trigger.is_none());
    }

    #[test]
    fn snapshot_with_trigger() {
        let snapshot =
            Snapshot::new("session-1", "message-1", "desc", vec![]).with_trigger("edit_tool");
        assert_eq!(snapshot.trigger, Some("edit_tool".to_string()));
    }

    #[test]
    fn snapshot_contains_file() {
        let snapshot = Snapshot::new(
            "session-1",
            "message-1",
            "desc",
            vec![PathBuf::from("src/main.rs"), PathBuf::from("README.md")],
        );

        assert!(snapshot.contains_file(&PathBuf::from("src/main.rs")));
        assert!(snapshot.contains_file(&PathBuf::from("README.md")));
        assert!(!snapshot.contains_file(&PathBuf::from("Cargo.toml")));
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let snapshot = Snapshot::new(
            "session-1",
            "message-1",
            "test",
            vec![PathBuf::from("file.txt")],
        );

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("session_id"));
        assert!(json.contains("message_id"));
        assert!(json.contains("file.txt"));
    }

    #[test]
    fn snapshot_deserializes_from_json() {
        let json = r#"{
            "id": "test-id",
            "session_id": "session-1",
            "message_id": "message-1",
            "timestamp": "2024-01-01T00:00:00Z",
            "description": "test",
            "files": ["file.txt"]
        }"#;

        let snapshot: Snapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snapshot.session_id, "session-1");
        assert_eq!(snapshot.files.len(), 1);
    }
}
