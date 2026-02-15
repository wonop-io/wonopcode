//! Server-to-client message types.

use crate::info::*;
use crate::state::AgentState;
use crate::workstream::{WorkstreamEvent, WorkstreamId, WorkstreamInfo};
use crate::{generate_message_id, timestamp_millis};
use serde::{Deserialize, Serialize};

/// Server-to-client message envelope.
///
/// All server updates are wrapped in this envelope for transport via Iggy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMessage {
    /// Message ID (matches client request ID if this is a response).
    pub id: String,
    /// Source workstream.
    pub workstream_id: WorkstreamId,
    /// The payload containing the actual update.
    pub payload: ServerPayload,
    /// Timestamp in milliseconds since Unix epoch.
    pub timestamp: u64,
    /// Iggy message offset (for replay support).
    #[serde(default)]
    pub offset: u64,
}

impl ServerMessage {
    /// Create a new server message with auto-generated ID and timestamp.
    pub fn new(workstream_id: WorkstreamId, payload: ServerPayload) -> Self {
        Self {
            id: generate_message_id(),
            workstream_id,
            payload,
            timestamp: timestamp_millis(),
            offset: 0,
        }
    }

    /// Create a new server message for the default workstream.
    pub fn for_default(payload: ServerPayload) -> Self {
        Self::new(WorkstreamId::default(), payload)
    }

    /// Create a reply to a specific client message.
    pub fn reply_to(
        message_id: String,
        workstream_id: WorkstreamId,
        payload: ServerPayload,
    ) -> Self {
        Self {
            id: message_id,
            workstream_id,
            payload,
            timestamp: timestamp_millis(),
            offset: 0,
        }
    }

    /// Set the Iggy message offset.
    pub fn with_offset(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }
}

/// Server message payload variants.
///
/// This enum contains all updates that can be sent from server to client.
/// Updates are divided into categories:
/// - Agent updates: Streaming responses, tool calls, completions
/// - State updates: Sessions, todos, files, servers
/// - Workstream updates: Pro-only workstream management
/// - Control updates: Connection health and server status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerPayload {
    // === Agent Updates ===
    /// Processing started.
    Started,

    /// Text delta from streaming response.
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

    // === State Updates ===
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
    TodosUpdated {
        phases: Vec<PhaseInfo>,
        todos: Vec<TodoInfo>,
    },

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
        #[serde(skip_serializing_if = "Option::is_none")]
        container_id: Option<String>,
    },

    /// System message to display.
    SystemMessage { message: String },

    /// Agent mode changed.
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

    /// Permission request was resolved (broadcast to dismiss dialogs on all clients).
    PermissionResolved {
        request_id: String,
        allowed: bool,
    },

    /// Full state synchronization.
    State(Box<AgentState>),

    // === Workstream Updates (Pro Edition) ===
    /// List of available workstreams.
    WorkstreamList { workstreams: Vec<WorkstreamInfo> },

    /// A new workstream was created.
    WorkstreamCreated { info: WorkstreamInfo },

    /// Connected to a workstream.
    WorkstreamConnected { info: WorkstreamInfo },

    /// Disconnected from a workstream.
    WorkstreamDisconnected,

    /// A workstream was removed.
    WorkstreamRemoved,

    /// A workstream was activated.
    WorkstreamActivated { info: WorkstreamInfo },

    /// A workstream was deactivated.
    WorkstreamDeactivated,

    /// A worktree was created.
    WorktreeCreated { info: WorkstreamInfo },

    /// A worktree was deleted.
    WorktreeDeleted,

    /// Workstreams were refreshed.
    WorkstreamsRefreshed { count: usize },

    /// Workstream lifecycle event.
    WorkstreamEvent { event: WorkstreamEvent },

    /// Conversation history for a workstream.
    ConversationHistory { messages: Vec<ConversationMessage> },

    // === Control Updates ===
    /// Pong response to ping.
    Pong,

    /// Server status update.
    ServerStatus {
        workstream_count: usize,
        uptime_secs: u64,
    },
    
    // === Agent Control Updates ===
    /// Model changed successfully.
    ModelChanged {
        model: String,
    },
    
    /// Sandbox started successfully.
    SandboxStarted {
        container_id: String,
    },
    
    /// Sandbox stopped successfully.
    SandboxStopped,
    
    /// Sandbox restarted successfully.
    SandboxRestarted {
        container_id: String,
    },
    
    /// Sandbox error occurred.
    SandboxError {
        message: String,
    },
    
    /// Agent stopped successfully.
    AgentStopped,
    
    /// Session statistics update.
    SessionStats {
        total_cost: f64,
        tokens_in: u64,
        tokens_out: u64,
    },
    
    /// Available models list.
    AvailableModels {
        models: Vec<ModelInfoSummary>,
    },
    
    /// Allow-all mode changed.
    AllowAllChanged {
        enabled: bool,
    },
}

/// Simplified model info for transmission
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfoSummary {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub description: Option<String>,
    pub context_window: u32,
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
}

impl ServerPayload {
    /// Check if this is a workstream-specific update.
    pub fn is_workstream_update(&self) -> bool {
        matches!(
            self,
            ServerPayload::WorkstreamList { .. }
                | ServerPayload::WorkstreamCreated { .. }
                | ServerPayload::WorkstreamConnected { .. }
                | ServerPayload::WorkstreamDisconnected
                | ServerPayload::WorkstreamRemoved
                | ServerPayload::WorkstreamActivated { .. }
                | ServerPayload::WorkstreamDeactivated
                | ServerPayload::WorktreeCreated { .. }
                | ServerPayload::WorktreeDeleted
                | ServerPayload::WorkstreamsRefreshed { .. }
                | ServerPayload::WorkstreamEvent { .. }
                | ServerPayload::ConversationHistory { .. }
        )
    }

    /// Check if this is a streaming update (high frequency).
    pub fn is_streaming(&self) -> bool {
        matches!(
            self,
            ServerPayload::Started
                | ServerPayload::TextDelta { .. }
                | ServerPayload::ToolStarted { .. }
                | ServerPayload::ToolCompleted { .. }
                | ServerPayload::Completed { .. }
        )
    }

    /// Get the SSE event type name (for backward compatibility).
    pub fn event_type(&self) -> &'static str {
        match self {
            ServerPayload::Started => "started",
            ServerPayload::TextDelta { .. } => "text_delta",
            ServerPayload::ToolStarted { .. } => "tool_started",
            ServerPayload::ToolCompleted { .. } => "tool_completed",
            ServerPayload::Completed { .. } => "completed",
            ServerPayload::Error { .. } => "error",
            ServerPayload::Status { .. } => "status",
            ServerPayload::TokenUsage { .. } => "token_usage",
            ServerPayload::ModelInfo { .. } => "model_info",
            ServerPayload::Sessions { .. } => "sessions",
            ServerPayload::TodosUpdated { .. } => "todos_updated",
            ServerPayload::LspUpdated { .. } => "lsp_updated",
            ServerPayload::McpUpdated { .. } => "mcp_updated",
            ServerPayload::ModifiedFilesUpdated { .. } => "modified_files_updated",
            ServerPayload::PermissionsPending { .. } => "permissions_pending",
            ServerPayload::SandboxUpdated { .. } => "sandbox_updated",
            ServerPayload::SystemMessage { .. } => "system_message",
            ServerPayload::AgentChanged { .. } => "agent_changed",
            ServerPayload::PermissionRequest { .. } => "permission_request",
            ServerPayload::PermissionResolved { .. } => "permission_resolved",
            ServerPayload::State(_) => "state",
            ServerPayload::WorkstreamList { .. } => "workstream_list",
            ServerPayload::WorkstreamCreated { .. } => "workstream_created",
            ServerPayload::WorkstreamConnected { .. } => "workstream_connected",
            ServerPayload::WorkstreamDisconnected => "workstream_disconnected",
            ServerPayload::WorkstreamRemoved => "workstream_removed",
            ServerPayload::WorkstreamActivated { .. } => "workstream_activated",
            ServerPayload::WorkstreamDeactivated => "workstream_deactivated",
            ServerPayload::WorktreeCreated { .. } => "worktree_created",
            ServerPayload::WorktreeDeleted => "worktree_deleted",
            ServerPayload::WorkstreamsRefreshed { .. } => "workstreams_refreshed",
            ServerPayload::WorkstreamEvent { .. } => "workstream_event",
            ServerPayload::ConversationHistory { .. } => "conversation_history",
            ServerPayload::Pong => "pong",
            ServerPayload::ServerStatus { .. } => "server_status",
            ServerPayload::ModelChanged { .. } => "model_changed",
            ServerPayload::SandboxStarted { .. } => "sandbox_started",
            ServerPayload::SandboxStopped => "sandbox_stopped",
            ServerPayload::SandboxRestarted { .. } => "sandbox_restarted",
            ServerPayload::SandboxError { .. } => "sandbox_error",
            ServerPayload::AgentStopped => "agent_stopped",
            ServerPayload::SessionStats { .. } => "session_stats",
            ServerPayload::AvailableModels { .. } => "available_models",
            ServerPayload::AllowAllChanged { .. } => "allow_all_changed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_message_new() {
        let msg = ServerMessage::new(WorkstreamId::default(), ServerPayload::Started);
        assert!(!msg.id.is_empty());
        assert!(msg.workstream_id.is_default());
        assert!(msg.timestamp > 0);
        assert_eq!(msg.offset, 0);
    }

    #[test]
    fn server_message_with_offset() {
        let msg =
            ServerMessage::new(WorkstreamId::default(), ServerPayload::Started).with_offset(42);
        assert_eq!(msg.offset, 42);
    }

    #[test]
    fn text_delta_serialization() {
        let payload = ServerPayload::TextDelta {
            delta: "Hello".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("text_delta"));
        assert!(json.contains("Hello"));

        let parsed: ServerPayload = serde_json::from_str(&json).unwrap();
        if let ServerPayload::TextDelta { delta } = parsed {
            assert_eq!(delta, "Hello");
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn tool_completed_serialization() {
        let payload = ServerPayload::ToolCompleted {
            id: "tool_123".to_string(),
            success: true,
            output: "result".to_string(),
            metadata: Some(serde_json::json!({"exit_code": 0})),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("tool_completed"));
        assert!(json.contains("exit_code"));
    }

    #[test]
    fn tool_completed_without_metadata() {
        let payload = ServerPayload::ToolCompleted {
            id: "tool_123".to_string(),
            success: true,
            output: "result".to_string(),
            metadata: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(!json.contains("metadata"));
    }

    #[test]
    fn permission_request_serialization() {
        let payload = ServerPayload::PermissionRequest {
            id: "perm_123".to_string(),
            tool: "bash".to_string(),
            action: "execute".to_string(),
            description: "Run npm install".to_string(),
            path: Some("/project/package.json".to_string()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("permission_request"));
        assert!(json.contains("npm install"));
    }

    #[test]
    fn workstream_updates_identification() {
        assert!(ServerPayload::WorkstreamList {
            workstreams: vec![]
        }
        .is_workstream_update());
        assert!(ServerPayload::WorkstreamConnected {
            info: WorkstreamInfo {
                id: WorkstreamId::default(),
                name: "test".into(),
                path: "/test".into(),
                branch: "main".into(),
                status: crate::workstream::WorkstreamStatus::Idle,
                is_active: true,
                client_count: 1,
            }
        }
        .is_workstream_update());
        assert!(!ServerPayload::Started.is_workstream_update());
    }

    #[test]
    fn streaming_updates_identification() {
        assert!(ServerPayload::Started.is_streaming());
        assert!(ServerPayload::TextDelta { delta: "".into() }.is_streaming());
        assert!(ServerPayload::Completed { text: "".into() }.is_streaming());
        assert!(!ServerPayload::Sessions { sessions: vec![] }.is_streaming());
    }

    #[test]
    fn all_event_types_unique() {
        use std::collections::HashSet;

        let payloads = vec![
            ServerPayload::Started,
            ServerPayload::TextDelta { delta: "".into() },
            ServerPayload::ToolStarted {
                id: "".into(),
                name: "".into(),
                input: "".into(),
            },
            ServerPayload::ToolCompleted {
                id: "".into(),
                success: true,
                output: "".into(),
                metadata: None,
            },
            ServerPayload::Completed { text: "".into() },
            ServerPayload::Error { error: "".into() },
            ServerPayload::Status { message: "".into() },
            ServerPayload::TokenUsage {
                input: 0,
                output: 0,
                cost: 0.0,
                context_limit: 0,
            },
            ServerPayload::ModelInfo { context_limit: 0 },
            ServerPayload::Sessions { sessions: vec![] },
            ServerPayload::TodosUpdated {
                phases: vec![],
                todos: vec![],
            },
            ServerPayload::LspUpdated { servers: vec![] },
            ServerPayload::McpUpdated { servers: vec![] },
            ServerPayload::ModifiedFilesUpdated { files: vec![] },
            ServerPayload::PermissionsPending { count: 0 },
            ServerPayload::SandboxUpdated {
                state: "".into(),
                runtime_type: None,
                error: None,
                container_id: None,
            },
            ServerPayload::SystemMessage { message: "".into() },
            ServerPayload::AgentChanged { agent: "".into() },
            ServerPayload::PermissionRequest {
                id: "".into(),
                tool: "".into(),
                action: "".into(),
                description: "".into(),
                path: None,
            },
            ServerPayload::Pong,
            ServerPayload::ServerStatus {
                workstream_count: 0,
                uptime_secs: 0,
            },
        ];

        let event_types: HashSet<_> = payloads.iter().map(|p| p.event_type()).collect();
        assert_eq!(
            event_types.len(),
            payloads.len(),
            "Some payloads share the same event_type"
        );
    }

    #[test]
    fn server_message_roundtrip() {
        let msg = ServerMessage::new(
            WorkstreamId::new("feature-xyz"),
            ServerPayload::TextDelta {
                delta: "Test delta".to_string(),
            },
        )
        .with_offset(100);

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ServerMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, msg.id);
        assert_eq!(parsed.workstream_id.as_str(), "feature-xyz");
        assert_eq!(parsed.timestamp, msg.timestamp);
        assert_eq!(parsed.offset, 100);
    }
}
