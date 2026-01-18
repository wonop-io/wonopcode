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

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // UX-Critical: Action Serialization Tests
    // If these fail, client-server communication breaks
    // =========================================================================

    #[test]
    fn action_send_prompt_serializes_correctly() {
        // UX: User sends a message to the AI
        let action = Action::SendPrompt {
            prompt: "Hello, world!".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("send_prompt"));
        assert!(json.contains("Hello, world!"));

        // Roundtrip
        let parsed: Action = serde_json::from_str(&json).unwrap();
        if let Action::SendPrompt { prompt } = parsed {
            assert_eq!(prompt, "Hello, world!");
        } else {
            panic!("Wrong action type");
        }
    }

    #[test]
    fn action_change_model_serializes_correctly() {
        // UX: User changes the AI model
        let action = Action::ChangeModel {
            model: "anthropic/claude-3-5-sonnet".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("change_model"));
        assert!(json.contains("anthropic/claude-3-5-sonnet"));
    }

    #[test]
    fn action_switch_session_serializes_correctly() {
        // UX: User switches to a different session
        let action = Action::SwitchSession {
            session_id: "ses_123abc".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("switch_session"));
        assert!(json.contains("ses_123abc"));
    }

    #[test]
    fn action_permission_response_serializes_correctly() {
        // UX: User responds to a permission request
        let action = Action::PermissionResponse {
            request_id: "req_456".to_string(),
            allow: true,
            remember: true,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("permission_response"));
        assert!(json.contains("req_456"));
        assert!(json.contains("true"));
    }

    #[test]
    fn action_save_settings_with_json_value() {
        // UX: User saves settings
        let config = serde_json::json!({
            "theme": "dark",
            "model": "anthropic/claude-3-5-sonnet"
        });
        let action = Action::SaveSettings {
            scope: SaveScope::Project,
            config,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("save_settings"));
        assert!(json.contains("project"));
        assert!(json.contains("dark"));
    }

    #[test]
    fn all_simple_actions_serialize() {
        // UX: All actions can be sent over the wire
        let actions = vec![
            Action::Cancel,
            Action::NewSession,
            Action::Undo,
            Action::Redo,
            Action::Unrevert,
            Action::Compact,
            Action::SandboxStart,
            Action::SandboxStop,
            Action::SandboxRestart,
            Action::ShareSession,
            Action::UnshareSession,
            Action::Quit,
        ];

        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let _parsed: Action = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn action_endpoints_are_unique() {
        // UX: Each action has a unique endpoint
        use std::collections::HashSet;

        let actions = vec![
            Action::SendPrompt {
                prompt: "".to_string(),
            },
            Action::Cancel,
            Action::ChangeModel {
                model: "".to_string(),
            },
            Action::ChangeAgent {
                agent: "".to_string(),
            },
            Action::NewSession,
            Action::SwitchSession {
                session_id: "".to_string(),
            },
            Action::RenameSession {
                title: "".to_string(),
            },
            Action::ForkSession { message_id: None },
            Action::Undo,
            Action::Redo,
            Action::Revert {
                message_id: "".to_string(),
            },
            Action::Unrevert,
            Action::Compact,
            Action::SandboxStart,
            Action::SandboxStop,
            Action::SandboxRestart,
            Action::McpToggle {
                name: "".to_string(),
            },
            Action::McpReconnect {
                name: "".to_string(),
            },
            Action::ShareSession,
            Action::UnshareSession,
            Action::GotoMessage {
                message_id: "".to_string(),
            },
            Action::SaveSettings {
                scope: SaveScope::Project,
                config: serde_json::Value::Null,
            },
            Action::PermissionResponse {
                request_id: "".to_string(),
                allow: false,
                remember: false,
            },
            Action::UpdateTestProviderSettings {
                emulate_thinking: false,
                emulate_tool_calls: false,
                emulate_tool_observed: false,
                emulate_streaming: false,
            },
            Action::Quit,
        ];

        let endpoints: HashSet<_> = actions.iter().map(|a| a.endpoint()).collect();
        assert_eq!(
            endpoints.len(),
            actions.len(),
            "Some actions share the same endpoint"
        );
    }

    #[test]
    fn save_scope_serialization() {
        // UX: Save scope determines where settings are stored
        let project = SaveScope::Project;
        let global = SaveScope::Global;

        let project_json = serde_json::to_string(&project).unwrap();
        let global_json = serde_json::to_string(&global).unwrap();

        assert_eq!(project_json, "\"project\"");
        assert_eq!(global_json, "\"global\"");

        // Roundtrip
        let parsed_project: SaveScope = serde_json::from_str(&project_json).unwrap();
        let parsed_global: SaveScope = serde_json::from_str(&global_json).unwrap();

        assert_eq!(parsed_project, SaveScope::Project);
        assert_eq!(parsed_global, SaveScope::Global);
    }
}
