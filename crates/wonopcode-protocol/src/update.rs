//! Update types sent from server to client via SSE.

use serde::{Deserialize, Serialize};

/// Updates sent from server to client via SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Update {
    /// Processing started.
    Started,

    /// Text delta from streaming.
    TextDelta { delta: String },

    /// Tool call started.
    ToolStarted {
        id: String,
        name: String,
        input: String,
    },

    /// Tool call completed.
    ToolCompleted {
        id: String,
        success: bool,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Response completed.
    Completed { text: String },

    /// Error occurred.
    Error { error: String },

    /// Status message.
    Status { message: String },

    /// Token usage update.
    TokenUsage {
        input: u32,
        output: u32,
        cost: f64,
        context_limit: u32,
    },

    /// Model info update.
    ModelInfo { context_limit: u32 },

    /// Session list update.
    Sessions { sessions: Vec<SessionInfo> },

    /// Todos updated.
    TodosUpdated { todos: Vec<TodoInfo> },

    /// LSP servers updated.
    LspUpdated { servers: Vec<LspInfo> },

    /// MCP servers updated.
    McpUpdated { servers: Vec<McpInfo> },

    /// Modified files updated.
    ModifiedFilesUpdated { files: Vec<ModifiedFileInfo> },

    /// Permission pending count.
    PermissionsPending { count: usize },

    /// Sandbox status updated.
    SandboxUpdated {
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        runtime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// System message to display.
    SystemMessage { message: String },

    /// Agent changed.
    AgentChanged { agent: String },

    /// Permission request from the agent.
    PermissionRequest {
        id: String,
        tool: String,
        action: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
}

/// Session info for session list updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub timestamp: String,
}

/// Todo item info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoInfo {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// LSP server info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspInfo {
    pub id: String,
    pub name: String,
    pub root: String,
    pub connected: bool,
}

/// MCP server info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInfo {
    pub name: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Modified file info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedFileInfo {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

impl Update {
    /// Get the SSE event type name.
    pub fn event_type(&self) -> &'static str {
        match self {
            Update::Started => "started",
            Update::TextDelta { .. } => "text_delta",
            Update::ToolStarted { .. } => "tool_started",
            Update::ToolCompleted { .. } => "tool_completed",
            Update::Completed { .. } => "completed",
            Update::Error { .. } => "error",
            Update::Status { .. } => "status",
            Update::TokenUsage { .. } => "token_usage",
            Update::ModelInfo { .. } => "model_info",
            Update::Sessions { .. } => "sessions",
            Update::TodosUpdated { .. } => "todos_updated",
            Update::LspUpdated { .. } => "lsp_updated",
            Update::McpUpdated { .. } => "mcp_updated",
            Update::ModifiedFilesUpdated { .. } => "modified_files_updated",
            Update::PermissionsPending { .. } => "permissions_pending",
            Update::SandboxUpdated { .. } => "sandbox_updated",
            Update::SystemMessage { .. } => "system_message",
            Update::AgentChanged { .. } => "agent_changed",
            Update::PermissionRequest { .. } => "permission_request",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // UX-Critical: Update Serialization Tests
    // If these fail, the TUI won't receive correct updates from the server
    // =========================================================================

    #[test]
    fn update_text_delta_for_streaming() {
        // UX: Streaming text from AI response
        let update = Update::TextDelta {
            delta: "Hello".to_string(),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("text_delta"));
        assert!(json.contains("Hello"));

        let parsed: Update = serde_json::from_str(&json).unwrap();
        if let Update::TextDelta { delta } = parsed {
            assert_eq!(delta, "Hello");
        } else {
            panic!("Wrong update type");
        }
    }

    #[test]
    fn update_tool_started_serializes() {
        // UX: Shows user when a tool starts executing
        let update = Update::ToolStarted {
            id: "tool_123".to_string(),
            name: "bash".to_string(),
            input: "ls -la".to_string(),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("tool_started"));
        assert!(json.contains("bash"));
        assert!(json.contains("ls -la"));
    }

    #[test]
    fn update_tool_completed_with_metadata() {
        // UX: Shows user tool result with optional metadata
        let update = Update::ToolCompleted {
            id: "tool_123".to_string(),
            success: true,
            output: "file1.txt\nfile2.txt".to_string(),
            metadata: Some(serde_json::json!({"exit_code": 0})),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("tool_completed"));
        assert!(json.contains("exit_code"));
    }

    #[test]
    fn update_error_serializes() {
        // UX: Shows user when an error occurs
        let update = Update::Error {
            error: "Rate limit exceeded".to_string(),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("Rate limit exceeded"));
    }

    #[test]
    fn update_token_usage_serializes() {
        // UX: Shows user token consumption and cost
        let update = Update::TokenUsage {
            input: 1000,
            output: 500,
            cost: 0.02,
            context_limit: 128000,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("token_usage"));
        assert!(json.contains("128000"));
    }

    #[test]
    fn update_sessions_list() {
        // UX: Shows user their session list
        let update = Update::Sessions {
            sessions: vec![
                SessionInfo {
                    id: "ses_1".to_string(),
                    title: "Debug issue".to_string(),
                    timestamp: "2024-01-15T10:30:00Z".to_string(),
                },
                SessionInfo {
                    id: "ses_2".to_string(),
                    title: "Add feature".to_string(),
                    timestamp: "2024-01-14T09:00:00Z".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("sessions"));
        assert!(json.contains("Debug issue"));
    }

    #[test]
    fn update_permission_request_serializes() {
        // UX: Prompts user for permission
        let update = Update::PermissionRequest {
            id: "perm_123".to_string(),
            tool: "bash".to_string(),
            action: "execute".to_string(),
            description: "Run npm install".to_string(),
            path: Some("/project/package.json".to_string()),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("permission_request"));
        assert!(json.contains("npm install"));
    }

    #[test]
    fn update_sandbox_status() {
        // UX: Shows sandbox state
        let update = Update::SandboxUpdated {
            state: "running".to_string(),
            runtime_type: Some("docker".to_string()),
            error: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("sandbox_updated"));
        assert!(json.contains("running"));
        assert!(json.contains("docker"));
    }

    #[test]
    fn update_modified_files() {
        // UX: Shows which files were modified
        let update = Update::ModifiedFilesUpdated {
            files: vec![
                ModifiedFileInfo {
                    path: "src/main.rs".to_string(),
                    added: 10,
                    removed: 5,
                },
                ModifiedFileInfo {
                    path: "Cargo.toml".to_string(),
                    added: 2,
                    removed: 0,
                },
            ],
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("modified_files_updated"));
        assert!(json.contains("src/main.rs"));
    }

    #[test]
    fn all_updates_have_event_type() {
        // UX: All updates can be sent via SSE
        let updates = vec![
            Update::Started,
            Update::TextDelta {
                delta: "".to_string(),
            },
            Update::ToolStarted {
                id: "".to_string(),
                name: "".to_string(),
                input: "".to_string(),
            },
            Update::ToolCompleted {
                id: "".to_string(),
                success: true,
                output: "".to_string(),
                metadata: None,
            },
            Update::Completed {
                text: "".to_string(),
            },
            Update::Error {
                error: "".to_string(),
            },
            Update::Status {
                message: "".to_string(),
            },
            Update::TokenUsage {
                input: 0,
                output: 0,
                cost: 0.0,
                context_limit: 0,
            },
            Update::ModelInfo { context_limit: 0 },
            Update::Sessions { sessions: vec![] },
            Update::TodosUpdated { todos: vec![] },
            Update::LspUpdated { servers: vec![] },
            Update::McpUpdated { servers: vec![] },
            Update::ModifiedFilesUpdated { files: vec![] },
            Update::PermissionsPending { count: 0 },
            Update::SandboxUpdated {
                state: "".to_string(),
                runtime_type: None,
                error: None,
            },
            Update::SystemMessage {
                message: "".to_string(),
            },
            Update::AgentChanged {
                agent: "".to_string(),
            },
            Update::PermissionRequest {
                id: "".to_string(),
                tool: "".to_string(),
                action: "".to_string(),
                description: "".to_string(),
                path: None,
            },
        ];

        for update in updates {
            let event_type = update.event_type();
            assert!(!event_type.is_empty(), "Event type should not be empty");

            // Verify it can be serialized
            let json = serde_json::to_string(&update).unwrap();
            let _parsed: Update = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn info_types_serialize() {
        // Session info
        let session = SessionInfo {
            id: "ses_1".to_string(),
            title: "Test".to_string(),
            timestamp: "2024-01-01".to_string(),
        };
        let json = serde_json::to_string(&session).unwrap();
        let _: SessionInfo = serde_json::from_str(&json).unwrap();

        // Todo info
        let todo = TodoInfo {
            id: "todo_1".to_string(),
            content: "Fix bug".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
        };
        let json = serde_json::to_string(&todo).unwrap();
        let _: TodoInfo = serde_json::from_str(&json).unwrap();

        // LSP info
        let lsp = LspInfo {
            id: "lsp_1".to_string(),
            name: "rust-analyzer".to_string(),
            root: "/project".to_string(),
            connected: true,
        };
        let json = serde_json::to_string(&lsp).unwrap();
        let _: LspInfo = serde_json::from_str(&json).unwrap();

        // MCP info
        let mcp = McpInfo {
            name: "aup".to_string(),
            connected: true,
            error: None,
        };
        let json = serde_json::to_string(&mcp).unwrap();
        let _: McpInfo = serde_json::from_str(&json).unwrap();
    }
}
