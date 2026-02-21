//! Client-to-server message types.
//!
//! These messages are sent from the desktop/TUI client to the server.

use serde::{Deserialize, Serialize};

/// Messages sent from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    // =========================================================================
    // Connection Management
    // =========================================================================
    /// Authenticate the connection.
    Authenticate {
        /// Authentication token (JWT or API key).
        token: String,
    },

    /// Subscribe to event streams for specific topics.
    ///
    /// If `from_seq` is provided, the server will replay events from that
    /// sequence number (for reconnection). If `None`, the server sends
    /// a state snapshot first.
    Subscribe {
        /// Topics to subscribe to.
        topics: Vec<Topic>,
        /// Resume from this sequence number (for reconnection).
        #[serde(skip_serializing_if = "Option::is_none")]
        from_seq: Option<u64>,
    },

    /// Unsubscribe from topics.
    Unsubscribe {
        /// Topics to unsubscribe from.
        topics: Vec<Topic>,
    },

    // =========================================================================
    // Acknowledgments
    // =========================================================================
    /// Acknowledge receipt of a server message.
    ///
    /// This is sent in response to any server message with `Reliable` delivery.
    Ack {
        /// The message ID being acknowledged.
        message_id: String,
        /// Optional error if processing failed on client side.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // =========================================================================
    // Workstream Operations
    // =========================================================================
    /// List all workstreams.
    ListWorkstreams,

    /// Connect to a workstream.
    ///
    /// If the workstream is passive (not running), it will be activated.
    /// The server will send a `WorkstreamSnapshot` in response.
    ConnectWorkstream {
        /// Workstream ID (typically the branch name).
        workstream_id: String,
    },

    /// Disconnect from current workstream.
    DisconnectWorkstream {
        /// Workstream ID to disconnect from.
        workstream_id: String,
    },

    /// Create a new worktree/workstream.
    CreateWorktree {
        /// Branch name for the new workstream.
        branch_name: String,
        /// Base branch to create from (defaults to current branch).
        #[serde(skip_serializing_if = "Option::is_none")]
        base_branch: Option<String>,
    },

    /// Delete a workstream.
    DeleteWorkstream {
        /// Workstream ID to delete.
        workstream_id: String,
    },

    // =========================================================================
    // Agent Interaction
    // =========================================================================
    /// Send a prompt to the agent.
    SendPrompt {
        /// Workstream ID.
        workstream_id: String,
        /// The prompt text.
        prompt: String,
        /// Client-generated key for idempotency.
        ///
        /// If the server sees the same key within 5 minutes, it returns
        /// the existing result instead of processing again.
        idempotency_key: String,
        /// Optional image attachments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageAttachment>,
    },

    /// Cancel the current prompt execution.
    CancelPrompt {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Reset the agent session (clear history, restart).
    ResetSession {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Change the model for a workstream.
    ChangeModel {
        /// Workstream ID.
        workstream_id: String,
        /// Provider name (e.g., "anthropic").
        provider: String,
        /// Model ID (e.g., "claude-sonnet-4-5-20250929").
        model_id: String,
    },

    // =========================================================================
    // Input Responses
    // =========================================================================
    /// Respond to a permission request.
    PermissionResponse {
        /// Workstream ID.
        workstream_id: String,
        /// Request ID from the permission request.
        request_id: String,
        /// The response.
        response: PermissionResponseData,
    },

    /// Respond to any input request (generic).
    InputResponse {
        /// Workstream ID.
        workstream_id: String,
        /// Request ID from the input request.
        request_id: String,
        /// The response.
        response: InputResponseData,
    },

    // =========================================================================
    // Sandbox
    // =========================================================================
    /// Start the sandbox container.
    StartSandbox {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Stop the sandbox container.
    StopSandbox {
        /// Workstream ID.
        workstream_id: String,
    },

    // =========================================================================
    // Settings
    // =========================================================================
    /// Set "allow all" mode.
    ///
    /// When enabled, all tool executions are auto-approved.
    SetAllowAll {
        /// Workstream ID.
        workstream_id: String,
        /// Whether to enable allow-all mode.
        enabled: bool,
    },

    /// Update MCP server state.
    McpToggle {
        /// Workstream ID.
        workstream_id: String,
        /// MCP server name.
        name: String,
    },

    /// Reconnect an MCP server.
    McpReconnect {
        /// Workstream ID.
        workstream_id: String,
        /// MCP server name.
        name: String,
    },

    // =========================================================================
    // Extended Operations (for feature parity with V1)
    // =========================================================================
    /// Refresh the workstream list (re-scan git worktrees).
    RefreshWorkstreams,

    /// Stop/cancel the currently running agent.
    StopAgent {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Get git status for a workstream.
    GitStatus {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Commit and push changes for a workstream.
    GitCommitAndPush {
        /// Workstream ID.
        workstream_id: String,
        /// Files to commit (paths relative to worktree root).
        files: Vec<String>,
        /// Commit message.
        message: String,
    },

    /// Get diff files list for a workstream.
    GetDiffFiles {
        /// Workstream ID.
        workstream_id: String,
        /// Diff mode: "all", "branch", or "uncommitted".
        #[serde(default)]
        mode: String,
    },

    /// Get diff for a specific file.
    GetFileDiff {
        /// Workstream ID.
        workstream_id: String,
        /// Path to the file.
        file_path: String,
        /// Diff mode: "all", "branch", or "uncommitted".
        #[serde(default)]
        mode: String,
    },

    /// Get list of available models.
    GetAvailableModels,

    /// Close a workstream (deactivate agent, optionally remove worktree).
    CloseWorkstream {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Get list of ACE artifacts for a workstream.
    GetArtifacts {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Get full detail for a specific artifact.
    GetArtifactDetail {
        /// Workstream ID.
        workstream_id: String,
        /// Artifact ID (e.g., "REQ-001").
        artifact_id: String,
    },

    /// Get server configuration.
    GetConfig,

    /// Get provider status (authenticated status for each provider).
    GetProviderStatus,

    /// Set API key for a provider.
    SetApiKey {
        /// Provider name (e.g., "anthropic", "openai").
        provider: String,
        /// API key.
        key: String,
    },

    /// Set authentication method for a provider.
    SetAuthMethod {
        /// Provider name.
        provider: String,
        /// Method: "api_key" or "oauth".
        method: String,
    },

    // =========================================================================
    // Agents Template Management (AGENTS.md/CLAUDE.md editor)
    // =========================================================================
    /// Get the AGENTS.TEMPLATE.md content for a workstream.
    GetAgentsTemplate {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Save the AGENTS.TEMPLATE.md content (renders to AGENTS.md + CLAUDE.md).
    SaveAgentsTemplate {
        /// Workstream ID.
        workstream_id: String,
        /// The raw Tera template content.
        template_content: String,
    },

    /// Preview rendering of the template without saving.
    PreviewAgentsTemplate {
        /// Workstream ID.
        workstream_id: String,
        /// The raw Tera template content to preview.
        template_content: String,
    },
}

/// Topics that clients can subscribe to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Topic {
    /// All workstream list changes (created, deleted, status changes).
    Workstreams,

    /// Events for a specific workstream.
    Workstream {
        /// Workstream ID.
        id: String,
    },

    /// System-wide events (health, config changes).
    System,
}

impl Topic {
    /// Create a topic for a specific workstream.
    pub fn workstream(id: impl Into<String>) -> Self {
        Topic::Workstream { id: id.into() }
    }
}

/// Image attachment for prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    /// Base64-encoded image data.
    pub data: String,
    /// MIME type (e.g., "image/png", "image/jpeg").
    pub mime_type: String,
    /// Optional filename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// Permission response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponseData {
    /// Whether the permission is granted.
    pub allowed: bool,
    /// Remember this decision for the session.
    #[serde(default)]
    pub remember: bool,
    /// Scope of "remember": "tool", "directory", or "all".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Generic input response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputResponseData {
    /// Response to a permission request.
    Permission {
        /// Whether allowed.
        allowed: bool,
        /// Remember this decision.
        #[serde(default)]
        remember: bool,
    },

    /// Response to a selection request.
    Selection {
        /// Selected option IDs.
        selected_ids: Vec<String>,
    },

    /// Response to a free text request.
    FreeText {
        /// The entered text.
        text: String,
    },

    /// Response to a confirmation request.
    Confirmation {
        /// Whether confirmed.
        confirmed: bool,
    },

    /// Request was cancelled.
    Cancelled {
        /// Reason for cancellation.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_serialization() {
        let msg = ClientMessage::SendPrompt {
            workstream_id: "feature/login".to_string(),
            prompt: "Hello, world!".to_string(),
            idempotency_key: "key-123".to_string(),
            images: vec![],
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("send_prompt"));
        assert!(json.contains("feature/login"));
        assert!(json.contains("Hello, world!"));

        let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
        if let ClientMessage::SendPrompt {
            workstream_id,
            prompt,
            ..
        } = parsed
        {
            assert_eq!(workstream_id, "feature/login");
            assert_eq!(prompt, "Hello, world!");
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_topic_serialization() {
        let workstream = Topic::Workstream {
            id: "feature/test".to_string(),
        };
        let json = serde_json::to_string(&workstream).unwrap();
        assert!(json.contains("workstream"));
        assert!(json.contains("feature/test"));

        let system = Topic::System;
        let json = serde_json::to_string(&system).unwrap();
        assert!(json.contains("system"));
    }

    #[test]
    fn test_input_response_data_serialization() {
        let permission = InputResponseData::Permission {
            allowed: true,
            remember: true,
        };
        let json = serde_json::to_string(&permission).unwrap();
        assert!(json.contains("permission"));
        assert!(json.contains("true"));

        let cancelled = InputResponseData::Cancelled {
            reason: "User timeout".to_string(),
        };
        let json = serde_json::to_string(&cancelled).unwrap();
        assert!(json.contains("cancelled"));
        assert!(json.contains("User timeout"));
    }

    #[test]
    fn test_ack_message() {
        let ack = ClientMessage::Ack {
            message_id: "msg-123".to_string(),
            error: None,
        };
        let json = serde_json::to_string(&ack).unwrap();
        assert!(json.contains("ack"));
        assert!(json.contains("msg-123"));

        let ack_with_error = ClientMessage::Ack {
            message_id: "msg-456".to_string(),
            error: Some("Processing failed".to_string()),
        };
        let json = serde_json::to_string(&ack_with_error).unwrap();
        assert!(json.contains("Processing failed"));
    }

    #[test]
    fn test_subscribe_with_replay() {
        let subscribe = ClientMessage::Subscribe {
            topics: vec![Topic::Workstream {
                id: "main".to_string(),
            }],
            from_seq: Some(42),
        };
        let json = serde_json::to_string(&subscribe).unwrap();
        assert!(json.contains("subscribe"));
        assert!(json.contains("42"));
    }
}
