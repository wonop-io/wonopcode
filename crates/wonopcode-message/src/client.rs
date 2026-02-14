//! Client-to-server message types.

use crate::workstream::WorkstreamId;
use crate::{generate_message_id, timestamp_millis};
use serde::{Deserialize, Serialize};

/// Client-to-server message envelope.
///
/// All client actions are wrapped in this envelope for transport via Iggy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    /// Unique message ID for correlation.
    pub id: String,
    /// Target workstream (defaults to "default" for Community Edition).
    pub workstream_id: WorkstreamId,
    /// The payload containing the actual action.
    pub payload: ClientPayload,
    /// Timestamp in milliseconds since Unix epoch.
    pub timestamp: u64,
}

impl ClientMessage {
    /// Create a new client message with auto-generated ID and timestamp.
    pub fn new(workstream_id: WorkstreamId, payload: ClientPayload) -> Self {
        Self {
            id: generate_message_id(),
            workstream_id,
            payload,
            timestamp: timestamp_millis(),
        }
    }

    /// Create a new client message for the default workstream.
    pub fn for_default(payload: ClientPayload) -> Self {
        Self::new(WorkstreamId::default(), payload)
    }

    /// Create a message with a specific ID (for testing or correlation).
    pub fn with_id(id: String, workstream_id: WorkstreamId, payload: ClientPayload) -> Self {
        Self {
            id,
            workstream_id,
            payload,
            timestamp: timestamp_millis(),
        }
    }
}

/// Client message payload variants.
///
/// This enum contains all actions that can be sent from client to server.
/// Actions are divided into categories:
/// - Agent actions: Interact with the AI agent
/// - Session actions: Manage conversation sessions
/// - Sandbox actions: Control the sandbox environment
/// - MCP actions: Manage MCP servers
/// - Workstream actions: Pro-only workstream management
/// - Control actions: Connection management and health checks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientPayload {
    // === Agent Actions ===
    /// Send a prompt to the agent.
    SendPrompt { prompt: String },

    /// Cancel the current operation.
    Cancel,

    /// Change the AI model.
    ChangeModel { model: String },

    /// Change the agent mode.
    ChangeAgent { agent: String },

    // === Session Actions ===
    /// Create a new session.
    NewSession,

    /// Switch to a different session.
    SwitchSession { session_id: String },

    /// Rename the current session.
    RenameSession { title: String },

    /// Fork the session from a specific message.
    ForkSession { message_id: Option<String> },

    /// Share the current session.
    ShareSession,

    /// Unshare the current session.
    UnshareSession,

    /// Undo the last message.
    Undo,

    /// Redo an undone message.
    Redo,

    /// Revert to a specific message.
    Revert { message_id: String },

    /// Cancel a pending revert.
    Unrevert,

    /// Compact/summarize the conversation.
    Compact,

    /// Go to a specific message.
    GotoMessage { message_id: String },

    // === Sandbox Actions ===
    /// Start the sandbox.
    SandboxStart,

    /// Stop the sandbox.
    SandboxStop,

    /// Restart the sandbox.
    SandboxRestart,

    /// Set "allow all" mode - when enabled, all tool executions are auto-approved.
    SetAllowAll { enabled: bool },

    // === MCP Actions ===
    /// Toggle an MCP server on/off.
    McpToggle { name: String },

    /// Reconnect to an MCP server.
    McpReconnect { name: String },

    // === Settings ===
    /// Save settings to the specified scope.
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

    /// Enable "allow all" mode - allow all tool executions without prompting.
    /// Also responds to the current pending permission request.
    EnableAllowAll {
        request_id: String,
    },

    // === Workstream Actions (Pro Edition) ===
    /// List all available workstreams.
    ListWorkstreams,

    /// Create a new git worktree (creates a passive workstream).
    CreateWorktree {
        branch_name: String,
        base_branch: String,
    },

    /// Delete a git worktree (must be passive).
    DeleteWorktree,

    /// Explicitly activate a passive workstream.
    ActivateWorkstream,

    /// Explicitly deactivate an active workstream.
    DeactivateWorkstream,

    /// Connect to a workstream (auto-activates if passive).
    ConnectWorkstream,

    /// Disconnect from the current workstream.
    DisconnectWorkstream,

    /// Refresh the workstream list.
    RefreshWorkstreams,

    // === Control Actions ===
    /// Ping for keep-alive.
    Ping,

    /// Request full state synchronization.
    RequestState,

    /// Request to quit (for graceful shutdown).
    Quit,
    
    /// Request list of available models.
    GetAvailableModels,
}

/// Scope for saving settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveScope {
    /// Save to project-level configuration.
    Project,
    /// Save to global configuration.
    Global,
}

impl ClientPayload {
    /// Check if this is a workstream-specific action.
    pub fn is_workstream_action(&self) -> bool {
        matches!(
            self,
            ClientPayload::ListWorkstreams
                | ClientPayload::CreateWorktree { .. }
                | ClientPayload::DeleteWorktree
                | ClientPayload::ActivateWorkstream
                | ClientPayload::DeactivateWorkstream
                | ClientPayload::ConnectWorkstream
                | ClientPayload::DisconnectWorkstream
                | ClientPayload::RefreshWorkstreams
        )
    }

    /// Check if this is a control action (not routed to agent).
    pub fn is_control_action(&self) -> bool {
        matches!(
            self,
            ClientPayload::Ping | ClientPayload::RequestState | ClientPayload::Quit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_message_new() {
        let msg = ClientMessage::new(
            WorkstreamId::default(),
            ClientPayload::SendPrompt {
                prompt: "Hello".to_string(),
            },
        );
        assert!(!msg.id.is_empty());
        assert!(msg.workstream_id.is_default());
        assert!(msg.timestamp > 0);
    }

    #[test]
    fn client_message_for_default() {
        let msg = ClientMessage::for_default(ClientPayload::Cancel);
        assert!(msg.workstream_id.is_default());
    }

    #[test]
    fn send_prompt_serialization() {
        let payload = ClientPayload::SendPrompt {
            prompt: "Hello, world!".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("send_prompt"));
        assert!(json.contains("Hello, world!"));

        let parsed: ClientPayload = serde_json::from_str(&json).unwrap();
        if let ClientPayload::SendPrompt { prompt } = parsed {
            assert_eq!(prompt, "Hello, world!");
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn change_model_serialization() {
        let payload = ClientPayload::ChangeModel {
            model: "anthropic/claude-sonnet-4".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("change_model"));
        assert!(json.contains("anthropic/claude-sonnet-4"));
    }

    #[test]
    fn permission_response_serialization() {
        let payload = ClientPayload::PermissionResponse {
            request_id: "req_123".to_string(),
            allow: true,
            remember: true,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("permission_response"));
        assert!(json.contains("req_123"));
    }

    #[test]
    fn save_settings_serialization() {
        let payload = ClientPayload::SaveSettings {
            scope: SaveScope::Project,
            config: serde_json::json!({"theme": "dark"}),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("save_settings"));
        assert!(json.contains("project"));
        assert!(json.contains("dark"));
    }

    #[test]
    fn workstream_actions_identification() {
        assert!(ClientPayload::ListWorkstreams.is_workstream_action());
        assert!(ClientPayload::ConnectWorkstream.is_workstream_action());
        assert!(!ClientPayload::SendPrompt { prompt: "".into() }.is_workstream_action());
        assert!(!ClientPayload::Cancel.is_workstream_action());
    }

    #[test]
    fn control_actions_identification() {
        assert!(ClientPayload::Ping.is_control_action());
        assert!(ClientPayload::RequestState.is_control_action());
        assert!(ClientPayload::Quit.is_control_action());
        assert!(!ClientPayload::SendPrompt { prompt: "".into() }.is_control_action());
    }

    #[test]
    fn all_simple_actions_serialize() {
        let payloads = vec![
            ClientPayload::Cancel,
            ClientPayload::NewSession,
            ClientPayload::Undo,
            ClientPayload::Redo,
            ClientPayload::Unrevert,
            ClientPayload::Compact,
            ClientPayload::SandboxStart,
            ClientPayload::SandboxStop,
            ClientPayload::SandboxRestart,
            ClientPayload::ShareSession,
            ClientPayload::UnshareSession,
            ClientPayload::ListWorkstreams,
            ClientPayload::DeleteWorktree,
            ClientPayload::ActivateWorkstream,
            ClientPayload::DeactivateWorkstream,
            ClientPayload::ConnectWorkstream,
            ClientPayload::DisconnectWorkstream,
            ClientPayload::RefreshWorkstreams,
            ClientPayload::Ping,
            ClientPayload::RequestState,
            ClientPayload::Quit,
        ];

        for payload in payloads {
            let json = serde_json::to_string(&payload).unwrap();
            let _parsed: ClientPayload = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn client_message_roundtrip() {
        let msg = ClientMessage::new(
            WorkstreamId::new("feature-xyz"),
            ClientPayload::SendPrompt {
                prompt: "Test prompt".to_string(),
            },
        );

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ClientMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, msg.id);
        assert_eq!(parsed.workstream_id.as_str(), "feature-xyz");
        assert_eq!(parsed.timestamp, msg.timestamp);
    }
}
