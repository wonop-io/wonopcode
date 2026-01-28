//! State types for initial sync and full state transfer.

use crate::info::*;
use serde::{Deserialize, Serialize};

/// Full agent state for initial synchronization.
///
/// Sent to clients when they connect or request full state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Project directory path.
    pub project: String,

    /// Current model (provider/model-id format).
    pub model: String,

    /// Current agent mode name.
    pub agent: String,

    /// Optional project ID (e.g., organization project identifier).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,

    /// Optional work ID (e.g., ticket ID, issue number).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_id: Option<String>,

    /// Current session state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionState>,

    /// Sandbox state.
    pub sandbox: SandboxState,

    /// MCP servers.
    pub mcp_servers: Vec<McpInfo>,

    /// LSP servers.
    pub lsp_servers: Vec<LspInfo>,

    /// Phases containing grouped todos.
    pub phases: Vec<PhaseInfo>,

    /// Todo items (flat list for backward compatibility).
    pub todos: Vec<TodoInfo>,

    /// Modified files.
    pub modified_files: Vec<ModifiedFileInfo>,

    /// Token usage statistics.
    pub token_usage: TokenUsage,

    /// Context limit for current model.
    pub context_limit: u32,

    /// Available sessions.
    pub sessions: Vec<SessionInfo>,

    /// Current configuration state (for settings dialog).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigState>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            project: String::new(),
            model: String::new(),
            agent: "default".to_string(),
            project_id: None,
            work_id: None,
            session: None,
            sandbox: SandboxState::default(),
            mcp_servers: Vec::new(),
            lsp_servers: Vec::new(),
            phases: Vec::new(),
            todos: Vec::new(),
            modified_files: Vec::new(),
            token_usage: TokenUsage::default(),
            context_limit: 200000,
            sessions: Vec::new(),
            config: None,
        }
    }
}

/// Session state including messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Session ID.
    pub id: String,

    /// Session title.
    pub title: String,

    /// Messages in the session.
    pub messages: Vec<Message>,

    /// Whether the session is shared.
    pub is_shared: bool,

    /// Share URL if shared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_url: Option<String>,

    /// Whether the assistant is currently streaming a response.
    #[serde(default)]
    pub is_streaming: bool,

    /// The in-progress message being streamed (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming_message: Option<Message>,
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message ID.
    pub id: String,

    /// Message role (user, assistant, system).
    pub role: String,

    /// Message content segments.
    pub content: Vec<MessageSegment>,

    /// Timestamp (ISO8601 format).
    pub timestamp: String,

    /// Tool calls in this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,

    /// Model used for this message (for assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Agent mode used for this message (for assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// A segment of message content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageSegment {
    /// Plain text.
    Text { text: String },

    /// Code block.
    Code { language: String, code: String },

    /// Thinking/reasoning block.
    Thinking { text: String },

    /// Tool call (inline, preserves order).
    Tool { tool: ToolCall },
}

/// A tool call within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool call ID.
    pub id: String,

    /// Tool name.
    pub name: String,

    /// Tool input (JSON string).
    pub input: String,

    /// Tool output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,

    /// Whether the tool call succeeded.
    pub success: bool,

    /// Tool status (pending, running, completed, failed).
    pub status: String,
}

/// Sandbox state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxState {
    /// Current state: "disabled", "stopped", "starting", "running", "error".
    pub state: String,

    /// Runtime type (e.g., "Docker", "Lima", "Podman").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_type: Option<String>,

    /// Error message if state is "error".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for SandboxState {
    fn default() -> Self {
        Self {
            state: "disabled".to_string(),
            runtime_type: None,
            error: None,
        }
    }
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens used.
    pub input: u32,

    /// Output tokens used.
    pub output: u32,

    /// Total cost in USD.
    pub cost: f64,
}

/// Configuration state for settings dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigState {
    /// Sandbox configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxConfigState>,

    /// Permission configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionConfigState>,
}

/// Sandbox configuration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfigState {
    /// Whether sandbox is enabled.
    pub enabled: bool,

    /// Runtime type (docker, podman, lima, auto).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
}

/// Permission configuration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionConfigState {
    /// Whether to allow all operations in sandbox.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_all_in_sandbox: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_state_default() {
        let state = AgentState::default();
        assert!(state.project.is_empty());
        assert_eq!(state.agent, "default");
        assert_eq!(state.context_limit, 200000);
        assert_eq!(state.sandbox.state, "disabled");
    }

    #[test]
    fn agent_state_serialization() {
        let state = AgentState {
            project: "/path/to/project".to_string(),
            model: "anthropic/claude-sonnet-4".to_string(),
            agent: "coding".to_string(),
            project_id: Some("proj_123".to_string()),
            work_id: None,
            session: None,
            sandbox: SandboxState::default(),
            mcp_servers: vec![],
            lsp_servers: vec![],
            phases: vec![],
            todos: vec![],
            modified_files: vec![],
            token_usage: TokenUsage {
                input: 1000,
                output: 500,
                cost: 0.02,
            },
            context_limit: 200000,
            sessions: vec![],
            config: None,
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.project, "/path/to/project");
        assert_eq!(parsed.model, "anthropic/claude-sonnet-4");
        assert_eq!(parsed.token_usage.input, 1000);
    }

    #[test]
    fn session_state_serialization() {
        let session = SessionState {
            id: "ses_123".to_string(),
            title: "Debug session".to_string(),
            messages: vec![],
            is_shared: false,
            share_url: None,
            is_streaming: false,
            streaming_message: None,
        };

        let json = serde_json::to_string(&session).unwrap();
        let parsed: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "ses_123");
        assert!(!parsed.is_shared);
    }

    #[test]
    fn message_serialization() {
        let msg = Message {
            id: "msg_123".to_string(),
            role: "assistant".to_string(),
            content: vec![
                MessageSegment::Text {
                    text: "Hello!".to_string(),
                },
                MessageSegment::Code {
                    language: "rust".to_string(),
                    code: "fn main() {}".to_string(),
                },
            ],
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            tool_calls: vec![],
            model: Some("claude-sonnet-4".to_string()),
            agent: None,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, "assistant");
        assert_eq!(parsed.content.len(), 2);
    }

    #[test]
    fn message_segment_types() {
        let segments = vec![
            MessageSegment::Text {
                text: "Hello".into(),
            },
            MessageSegment::Code {
                language: "rust".into(),
                code: "fn main() {}".into(),
            },
            MessageSegment::Thinking {
                text: "Let me think...".into(),
            },
            MessageSegment::Tool {
                tool: ToolCall {
                    id: "tool_1".into(),
                    name: "bash".into(),
                    input: "ls".into(),
                    output: Some("files".into()),
                    success: true,
                    status: "completed".into(),
                },
            },
        ];

        for segment in segments {
            let json = serde_json::to_string(&segment).unwrap();
            let _: MessageSegment = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn tool_call_serialization() {
        let tool_call = ToolCall {
            id: "tool_123".to_string(),
            name: "bash".to_string(),
            input: r#"{"command": "ls -la"}"#.to_string(),
            output: Some("file1\nfile2".to_string()),
            success: true,
            status: "completed".to_string(),
        };

        let json = serde_json::to_string(&tool_call).unwrap();
        let parsed: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "bash");
        assert!(parsed.success);
    }

    #[test]
    fn sandbox_state_default() {
        let state = SandboxState::default();
        assert_eq!(state.state, "disabled");
        assert!(state.runtime_type.is_none());
        assert!(state.error.is_none());
    }

    #[test]
    fn config_state_serialization() {
        let config = ConfigState {
            sandbox: Some(SandboxConfigState {
                enabled: true,
                runtime: Some("docker".to_string()),
            }),
            permission: Some(PermissionConfigState {
                allow_all_in_sandbox: Some(true),
            }),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("docker"));
        assert!(json.contains("allow_all_in_sandbox"));
    }
}
