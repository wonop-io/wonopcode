//! Action types sent from client to server.

use serde::{Deserialize, Serialize};

/// Actions that can be sent from the client to the server.
///
/// These map to HTTP POST endpoints on the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Send a prompt to the AI.
    SendPrompt { prompt: String },

    /// Cancel the current operation.
    Cancel,

    /// Change the model.
    ChangeModel { model: String },

    /// Change the agent.
    ChangeAgent { agent: String },

    /// Create a new session.
    NewSession,

    /// Switch to a different session.
    SwitchSession { session_id: String },

    /// Rename the current session.
    RenameSession { title: String },

    /// Fork the session from a specific message.
    ForkSession { message_id: Option<String> },

    /// Undo the last message.
    Undo,

    /// Redo an undone message.
    Redo,

    /// Revert to a specific message.
    Revert { message_id: String },

    /// Cancel a pending revert.
    Unrevert,

    /// Compact the conversation.
    Compact,

    /// Start the sandbox.
    SandboxStart,

    /// Stop the sandbox.
    SandboxStop,

    /// Restart the sandbox.
    SandboxRestart,

    /// Toggle an MCP server.
    McpToggle { name: String },

    /// Reconnect an MCP server.
    McpReconnect { name: String },

    /// Share the current session.
    ShareSession,

    /// Unshare the current session.
    UnshareSession,

    /// Go to a specific message.
    GotoMessage { message_id: String },

    /// Save settings.
    SaveSettings {
        scope: SaveScope,
        config: serde_json::Value,
    },

    /// Respond to a permission request.
    PermissionResponse {
        request_id: String,
        allow: bool,
        remember: bool,
    },

    /// Update test provider settings.
    UpdateTestProviderSettings {
        emulate_thinking: bool,
        emulate_tool_calls: bool,
        emulate_tool_observed: bool,
        emulate_streaming: bool,
    },

    /// Request to quit (for graceful shutdown).
    Quit,
}

/// Scope for saving settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaveScope {
    /// Save to project-level config.
    Project,
    /// Save to global config.
    Global,
}

impl Action {
    /// Get the HTTP endpoint for this action.
    pub fn endpoint(&self) -> &'static str {
        match self {
            Action::SendPrompt { .. } => "/action/prompt",
            Action::Cancel => "/action/cancel",
            Action::ChangeModel { .. } => "/action/model",
            Action::ChangeAgent { .. } => "/action/agent",
            Action::NewSession => "/action/session/new",
            Action::SwitchSession { .. } => "/action/session/switch",
            Action::RenameSession { .. } => "/action/session/rename",
            Action::ForkSession { .. } => "/action/session/fork",
            Action::Undo => "/action/undo",
            Action::Redo => "/action/redo",
            Action::Revert { .. } => "/action/revert",
            Action::Unrevert => "/action/unrevert",
            Action::Compact => "/action/compact",
            Action::SandboxStart => "/action/sandbox/start",
            Action::SandboxStop => "/action/sandbox/stop",
            Action::SandboxRestart => "/action/sandbox/restart",
            Action::McpToggle { .. } => "/action/mcp/toggle",
            Action::McpReconnect { .. } => "/action/mcp/reconnect",
            Action::ShareSession => "/action/session/share",
            Action::UnshareSession => "/action/session/unshare",
            Action::GotoMessage { .. } => "/action/goto",
            Action::SaveSettings { .. } => "/action/settings",
            Action::PermissionResponse { .. } => "/action/permission",
            Action::UpdateTestProviderSettings { .. } => "/action/test-settings",
            Action::Quit => "/action/quit",
        }
    }
}
