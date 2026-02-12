//! Information types used in updates and state.

use serde::{Deserialize, Serialize};

/// Session information for session list updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID.
    pub id: String,
    /// Session title.
    pub title: String,
    /// Last activity timestamp (ISO8601 format).
    pub timestamp: String,
}

/// Phase information containing grouped todos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseInfo {
    /// Phase ID.
    pub id: String,
    /// Phase name.
    pub name: String,
    /// Phase status.
    pub status: String,
    /// Todos in this phase.
    pub todos: Vec<TodoInfo>,
}

/// Todo item information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoInfo {
    /// Todo ID.
    pub id: String,
    /// Todo content/description.
    pub content: String,
    /// Todo status (pending, in_progress, completed, cancelled).
    pub status: String,
    /// Todo priority (high, medium, low).
    pub priority: String,
    /// Optional phase ID this todo belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    /// Parent artifact IDs (e.g., ["REQ-WON-135-001", "DES-WON-135-003"])
    #[serde(default)]
    pub parents: Vec<String>,
}

/// LSP server information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspInfo {
    /// Server ID.
    pub id: String,
    /// Server name (e.g., "rust-analyzer").
    pub name: String,
    /// Root directory for this server.
    pub root: String,
    /// Whether the server is connected.
    pub connected: bool,
}

/// MCP server information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInfo {
    /// Server name.
    pub name: String,
    /// Whether the server is connected.
    pub connected: bool,
    /// Error message if connection failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Modified file information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedFileInfo {
    /// File path.
    pub path: String,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
}

/// Conversation message for history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// Message ID.
    pub id: String,
    /// Timestamp (ISO8601 format).
    pub timestamp: String,
    /// Sender type: "user" or "assistant".
    pub sender: String,
    /// Message content.
    pub content: String,
    /// Message type for UI rendering.
    pub message_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_info_serialization() {
        let info = SessionInfo {
            id: "ses_123".to_string(),
            title: "Debug issue".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "ses_123");
        assert_eq!(parsed.title, "Debug issue");
    }

    #[test]
    fn todo_info_serialization() {
        let info = TodoInfo {
            id: "todo_1".to_string(),
            content: "Fix the bug".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
            phase_id: Some("phase_1".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("phase_id"));

        let parsed: TodoInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.phase_id, Some("phase_1".to_string()));
    }

    #[test]
    fn todo_info_without_phase() {
        let info = TodoInfo {
            id: "todo_2".to_string(),
            content: "Add feature".to_string(),
            status: "in_progress".to_string(),
            priority: "medium".to_string(),
            phase_id: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("phase_id"));
    }

    #[test]
    fn mcp_info_serialization() {
        let info = McpInfo {
            name: "aup".to_string(),
            connected: true,
            error: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("error"));

        let with_error = McpInfo {
            name: "aup".to_string(),
            connected: false,
            error: Some("Connection refused".to_string()),
        };

        let json = serde_json::to_string(&with_error).unwrap();
        assert!(json.contains("Connection refused"));
    }

    #[test]
    fn modified_file_info_serialization() {
        let info = ModifiedFileInfo {
            path: "src/main.rs".to_string(),
            added: 10,
            removed: 5,
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: ModifiedFileInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.path, "src/main.rs");
        assert_eq!(parsed.added, 10);
        assert_eq!(parsed.removed, 5);
    }
}
