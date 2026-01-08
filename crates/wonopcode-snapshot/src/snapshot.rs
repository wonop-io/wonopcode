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
