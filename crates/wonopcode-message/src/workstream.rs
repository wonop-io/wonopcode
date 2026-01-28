//! Workstream identification and routing types.

use serde::{Deserialize, Serialize};

/// Unique identifier for a workstream.
///
/// In Community Edition, this is always "default".
/// In Pro Edition, this identifies specific git worktrees/workstreams.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkstreamId(pub String);

impl WorkstreamId {
    /// Create a new workstream ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string reference.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check if this is the default workstream.
    pub fn is_default(&self) -> bool {
        self.0 == "default"
    }
}

impl Default for WorkstreamId {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl std::fmt::Display for WorkstreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for WorkstreamId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for WorkstreamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Information about a workstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstreamInfo {
    /// Unique workstream identifier.
    pub id: WorkstreamId,
    /// Human-readable name.
    pub name: String,
    /// Path to the worktree directory.
    pub path: String,
    /// Git branch name.
    pub branch: String,
    /// Current status.
    pub status: WorkstreamStatus,
    /// Whether this workstream is active (has a running Runner).
    pub is_active: bool,
    /// Number of connected clients.
    pub client_count: usize,
}

/// Workstream status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkstreamStatus {
    /// Workstream is idle (no active operation).
    Idle,
    /// Workstream is processing a request.
    Working,
    /// Workstream is in an error state.
    Error,
}

impl Default for WorkstreamStatus {
    fn default() -> Self {
        Self::Idle
    }
}

/// Events related to workstream lifecycle and status changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum WorkstreamEvent {
    /// A new workstream was created.
    Created {
        workstream_id: WorkstreamId,
        info: WorkstreamInfo,
    },
    /// A workstream was activated.
    Activated { workstream_id: WorkstreamId },
    /// A workstream was deactivated.
    Deactivated { workstream_id: WorkstreamId },
    /// A workstream was removed.
    Removed { workstream_id: WorkstreamId },
    /// A workstream's status changed.
    StatusChanged {
        workstream_id: WorkstreamId,
        old_status: WorkstreamStatus,
        new_status: WorkstreamStatus,
    },
    /// A client connected to a workstream.
    ClientConnected {
        workstream_id: WorkstreamId,
        client_count: usize,
    },
    /// A client disconnected from a workstream.
    ClientDisconnected {
        workstream_id: WorkstreamId,
        client_count: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstream_id_default() {
        let id = WorkstreamId::default();
        assert_eq!(id.as_str(), "default");
        assert!(id.is_default());
    }

    #[test]
    fn workstream_id_custom() {
        let id = WorkstreamId::new("feature-xyz");
        assert_eq!(id.as_str(), "feature-xyz");
        assert!(!id.is_default());
    }

    #[test]
    fn workstream_id_from_string() {
        let id: WorkstreamId = "test".into();
        assert_eq!(id.as_str(), "test");
    }

    #[test]
    fn workstream_info_serialization() {
        let info = WorkstreamInfo {
            id: WorkstreamId::new("feature-abc"),
            name: "Feature ABC".to_string(),
            path: "/path/to/worktree".to_string(),
            branch: "feature/abc".to_string(),
            status: WorkstreamStatus::Idle,
            is_active: true,
            client_count: 2,
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: WorkstreamInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id.as_str(), "feature-abc");
        assert_eq!(parsed.name, "Feature ABC");
    }

    #[test]
    fn workstream_event_serialization() {
        let event = WorkstreamEvent::StatusChanged {
            workstream_id: WorkstreamId::new("test"),
            old_status: WorkstreamStatus::Idle,
            new_status: WorkstreamStatus::Working,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("status_changed"));

        let parsed: WorkstreamEvent = serde_json::from_str(&json).unwrap();
        if let WorkstreamEvent::StatusChanged { workstream_id, .. } = parsed {
            assert_eq!(workstream_id.as_str(), "test");
        } else {
            panic!("Wrong event type");
        }
    }
}
