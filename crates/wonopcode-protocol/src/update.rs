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
