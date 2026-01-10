//! State types for initial sync and reconnection.

use serde::{Deserialize, Serialize};

use crate::update::{LspInfo, McpInfo, ModifiedFileInfo, TodoInfo};

/// Full application state for initial sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// Project directory path.
    pub project: String,

    /// Current model (provider/model-id format).
    pub model: String,

    /// Current agent name.
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

    /// Todo items.
    pub todos: Vec<TodoInfo>,

    /// Modified files.
    pub modified_files: Vec<ModifiedFileInfo>,

    /// Token usage.
    pub token_usage: TokenUsage,

    /// Context limit for current model.
    pub context_limit: u32,

    /// Available sessions.
    pub sessions: Vec<SessionListItem>,

    /// Current configuration (for settings dialog).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigState>,
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

    /// Timestamp.
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

/// Session list item (summary without messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListItem {
    /// Session ID.
    pub id: String,

    /// Session title.
    pub title: String,

    /// Last activity timestamp.
    pub timestamp: String,
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

impl Default for SandboxState {
    fn default() -> Self {
        Self {
            state: "disabled".to_string(),
            runtime_type: None,
            error: None,
        }
    }
}

impl Default for State {
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
            todos: Vec::new(),
            modified_files: Vec::new(),
            token_usage: TokenUsage::default(),
            context_limit: 200000,
            sessions: Vec::new(),
            config: None,
        }
    }
}
