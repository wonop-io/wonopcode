//! Server-to-client message types.
//!
//! These messages are sent from the server to the desktop/TUI client.

use serde::{Deserialize, Serialize};

use super::events::WorkstreamEventData;
use super::input::UserInputRequest;
use super::snapshot::WorkstreamStateSnapshot;

/// Messages sent from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    // =========================================================================
    // Connection State
    // =========================================================================
    /// Authentication result.
    Authenticated {
        /// Whether authentication succeeded.
        success: bool,
        /// Error message if failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Server version for compatibility checks.
        server_version: String,
        /// Protocol version (for future upgrades).
        protocol_version: u32,
    },

    /// Connection state notification.
    ConnectionState {
        /// Connection state information.
        state: ConnectionStateInfo,
    },

    // =========================================================================
    // Acknowledgments
    // =========================================================================
    /// Acknowledge receipt of a client message.
    Ack {
        /// The message ID being acknowledged.
        message_id: String,
        /// Error if server rejected the message.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // =========================================================================
    // Responses to Requests
    // =========================================================================
    /// Response to a client request.
    Response {
        /// The request ID this responds to.
        request_id: String,
        /// Result (success or error).
        result: ResponseResult,
    },

    // =========================================================================
    // State Snapshots
    // =========================================================================
    /// Complete workstream state snapshot.
    ///
    /// Sent on connect/reconnect to establish baseline state.
    WorkstreamSnapshot {
        /// Workstream ID.
        workstream_id: String,
        /// Complete state snapshot.
        snapshot: WorkstreamStateSnapshot,
        /// Last sequence number included in this snapshot.
        ///
        /// Client should subscribe from `last_seq + 1` to receive
        /// events that happened after the snapshot.
        last_seq: u64,
    },

    // =========================================================================
    // Events (for subscriptions)
    // =========================================================================
    /// Workstream list changed.
    WorkstreamListEvent {
        /// Current list of workstreams.
        workstreams: Vec<WorkstreamInfo>,
    },

    /// Workstream-specific event.
    WorkstreamEvent {
        /// Workstream ID.
        workstream_id: String,
        /// The event data.
        event: WorkstreamEventData,
    },

    /// System event.
    SystemEvent {
        /// The event data.
        event: SystemEventData,
    },

    // =========================================================================
    // Input Requests
    // =========================================================================
    /// Request for user input (permission, confirmation, etc.).
    InputRequest {
        /// Workstream ID.
        workstream_id: String,
        /// The input request.
        request: UserInputRequest,
    },

    /// Input request was resolved (by another client or timeout).
    InputRequestResolved {
        /// Workstream ID.
        workstream_id: String,
        /// Request ID.
        request_id: String,
        /// Reason for resolution.
        reason: String,
    },
}

/// Connection state information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionStateInfo {
    /// Successfully connected.
    Connected {
        /// Unique client ID for this connection.
        client_id: String,
        /// Number of other clients connected.
        peer_count: u32,
    },

    /// Reconnected after disconnect.
    Reconnected {
        /// Client ID (same as before if session persisted).
        client_id: String,
        /// Number of events that will be replayed.
        replay_count: u64,
    },

    /// Server is disconnecting this client.
    Disconnecting {
        /// Reason for disconnection.
        reason: String,
    },
}

/// Response result (success or error).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseResult {
    /// Successful response.
    Success {
        /// Response data.
        data: ResponseData,
    },

    /// Error response.
    Error {
        /// Error code (machine-readable).
        code: String,
        /// Error message (human-readable).
        message: String,
        /// Whether this error is retryable.
        retryable: bool,
        /// Additional error context.
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

impl ResponseResult {
    /// Create a success response.
    pub fn success(data: ResponseData) -> Self {
        ResponseResult::Success { data }
    }

    /// Create an error response.
    pub fn error(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        ResponseResult::Error {
            code: code.into(),
            message: message.into(),
            retryable,
            details: None,
        }
    }

    /// Check if this is a success.
    pub fn is_success(&self) -> bool {
        matches!(self, ResponseResult::Success { .. })
    }
}

/// Successful response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseData {
    /// Response to ListWorkstreams.
    WorkstreamList {
        /// List of workstreams.
        workstreams: Vec<WorkstreamInfo>,
    },

    /// Response to ConnectWorkstream.
    WorkstreamConnected {
        /// Workstream info.
        info: WorkstreamInfo,
    },

    /// Response to CreateWorktree.
    WorkstreamCreated {
        /// Created workstream info.
        info: WorkstreamInfo,
    },

    /// Response to SendPrompt.
    PromptAccepted {
        /// Prompt ID for tracking.
        prompt_id: String,
    },

    /// Response to CancelPrompt.
    PromptCancelled {
        /// Cancelled prompt ID.
        prompt_id: String,
    },

    /// Response to ResetSession.
    SessionReset,

    /// Response to ChangeModel.
    ModelChanged {
        /// New model ID.
        model_id: String,
        /// New provider.
        provider: String,
    },

    /// Response to StartSandbox.
    SandboxStarted {
        /// Container ID.
        container_id: String,
    },

    /// Response to StopSandbox.
    SandboxStopped,

    /// Generic success with no data.
    Ok,

    // =========================================================================
    // Extended Response Types (for feature parity with V1)
    // =========================================================================
    /// Response to RefreshWorkstreams.
    WorkstreamsRefreshed {
        /// Number of workstreams found.
        count: usize,
    },

    /// Response to StopAgent.
    AgentStopped,

    /// Response to GitStatus.
    GitStatus {
        /// List of file statuses.
        files: Vec<GitFileStatus>,
    },

    /// Response to GitCommitAndPush.
    GitOperationComplete {
        /// Whether the operation succeeded.
        success: bool,
        /// Operation type (e.g., "commit_and_push").
        operation: String,
        /// Result message.
        message: Option<String>,
    },

    /// Response to GetDiffFiles.
    DiffFiles {
        /// List of changed files.
        files: Vec<DiffFileInfo>,
    },

    /// Response to GetFileDiff.
    FileDiff {
        /// File path.
        file_path: String,
        /// Diff lines.
        lines: Vec<DiffLine>,
    },

    /// Response to GetAvailableModels.
    AvailableModels {
        /// List of available models.
        models: Vec<ModelInfo>,
    },

    /// Response to CloseWorkstream.
    WorkstreamClosed {
        /// Workstream ID.
        workstream_id: String,
    },

    /// Response to GetArtifacts.
    Artifacts {
        /// List of artifacts.
        artifacts: Vec<ArtifactInfo>,
    },

    /// Response to GetArtifactDetail.
    ArtifactDetail {
        /// Artifact info.
        artifact: ArtifactInfo,
        /// Full artifact content.
        content: String,
    },

    /// Response to GetConfig.
    Config {
        /// Configuration data.
        config: serde_json::Value,
    },

    /// Response to GetProviderStatus.
    ProviderStatus {
        /// List of provider statuses.
        providers: Vec<ProviderStatusInfo>,
    },

    /// Response to SetApiKey or SetAuthMethod.
    ConfigUpdated,
}

/// Git file status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    /// File path relative to worktree root.
    pub path: String,
    /// Git status (e.g., "modified", "added", "deleted").
    pub status: String,
    /// Whether the file is staged.
    pub staged: bool,
}

/// Diff file info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFileInfo {
    /// File path.
    pub path: String,
    /// Change type (e.g., "added", "modified", "deleted").
    pub change_type: String,
    /// Number of additions.
    #[serde(default)]
    pub additions: usize,
    /// Number of deletions.
    #[serde(default)]
    pub deletions: usize,
}

/// Diff line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    /// Line type (e.g., "header", "added", "removed", "unchanged").
    pub line_type: String,
    /// Old file line number (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_line_num: Option<usize>,
    /// New file line number (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_line_num: Option<usize>,
    /// Line content.
    pub content: String,
}

/// Model info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Provider name.
    pub provider: String,
    /// Context window size.
    #[serde(default)]
    pub context_window: usize,
}

/// Artifact info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    /// Artifact ID (e.g., "REQ-001").
    pub id: String,
    /// Artifact type (e.g., "requirement", "design").
    pub artifact_type: String,
    /// Title.
    pub title: String,
    /// Status.
    pub status: String,
}

/// Provider status info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatusInfo {
    /// Provider name.
    pub name: String,
    /// Whether authenticated.
    pub authenticated: bool,
    /// Authentication method.
    pub auth_method: String,
}

/// Information about a workstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstreamInfo {
    /// Workstream ID (branch name).
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Worktree path on disk.
    pub path: String,

    /// Current branch.
    pub branch: String,

    /// Workstream status.
    pub status: WorkstreamStatus,

    /// Whether this workstream is currently active.
    pub is_active: bool,

    /// Whether this is the main working tree.
    pub is_main_worktree: bool,

    /// Ticket ID if associated with a ticket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,

    /// Sandbox container ID if running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_container_id: Option<String>,

    /// Sandbox state.
    pub sandbox_state: SandboxState,

    /// Base branch this workstream was created from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,

    /// Whether the agent is currently busy.
    #[serde(default)]
    pub agent_busy: bool,
}

/// Workstream status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkstreamStatus {
    /// Passive - not running, just a git worktree reference.
    Passive,
    /// Active - running with an agent.
    Active,
    /// Starting - being activated.
    Starting,
    /// Stopping - being deactivated.
    Stopping,
    /// Error - failed to activate or crashed.
    Error,
}

/// Sandbox state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    /// Sandbox is stopped.
    #[default]
    Stopped,
    /// Sandbox is starting.
    Starting,
    /// Sandbox is running.
    Running,
    /// Sandbox is stopping.
    Stopping,
    /// Sandbox encountered an error.
    Error,
}

/// System-wide events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SystemEventData {
    /// Server health status.
    HealthStatus {
        /// Whether server is healthy.
        healthy: bool,
        /// Number of active agents.
        agent_count: u32,
        /// Memory usage in MB.
        memory_usage_mb: u64,
    },

    /// Configuration changed.
    ConfigChanged {
        /// Configuration key.
        key: String,
        /// New value.
        value: serde_json::Value,
    },

    /// Server is shutting down.
    ServerShutdown {
        /// Reason for shutdown.
        reason: String,
        /// Whether a restart is expected.
        restart_expected: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_message_ack() {
        let ack = ServerMessage::Ack {
            message_id: "msg-123".to_string(),
            error: None,
        };
        let json = serde_json::to_string(&ack).unwrap();
        assert!(json.contains("ack"));
        assert!(json.contains("msg-123"));

        let parsed: ServerMessage = serde_json::from_str(&json).unwrap();
        if let ServerMessage::Ack { message_id, error } = parsed {
            assert_eq!(message_id, "msg-123");
            assert!(error.is_none());
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_response_result_success() {
        let result = ResponseResult::success(ResponseData::Ok);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(result.is_success());
    }

    #[test]
    fn test_response_result_error() {
        let result = ResponseResult::error("NOT_FOUND", "Workstream not found", false);
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("NOT_FOUND"));
        assert!(!result.is_success());
    }

    #[test]
    fn test_workstream_info_serialization() {
        let info = WorkstreamInfo {
            id: "feature/login".to_string(),
            name: "Login Feature".to_string(),
            path: "/path/to/worktree".to_string(),
            branch: "feature/login".to_string(),
            status: WorkstreamStatus::Active,
            is_active: true,
            is_main_worktree: false,
            ticket_id: Some("JIRA-123".to_string()),
            sandbox_container_id: None,
            sandbox_state: SandboxState::Stopped,
            base_branch: Some("main".to_string()),
            agent_busy: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("feature/login"));
        assert!(json.contains("active"));
        assert!(json.contains("JIRA-123"));
    }

    #[test]
    fn test_connection_state_serialization() {
        let connected = ConnectionStateInfo::Connected {
            client_id: "client-123".to_string(),
            peer_count: 2,
        };
        let json = serde_json::to_string(&connected).unwrap();
        assert!(json.contains("connected"));
        assert!(json.contains("client-123"));

        let reconnected = ConnectionStateInfo::Reconnected {
            client_id: "client-123".to_string(),
            replay_count: 5,
        };
        let json = serde_json::to_string(&reconnected).unwrap();
        assert!(json.contains("reconnected"));
        assert!(json.contains("5"));
    }

    #[test]
    fn test_system_event_serialization() {
        let health = SystemEventData::HealthStatus {
            healthy: true,
            agent_count: 3,
            memory_usage_mb: 512,
        };
        let json = serde_json::to_string(&health).unwrap();
        assert!(json.contains("health_status"));
        assert!(json.contains("512"));

        let shutdown = SystemEventData::ServerShutdown {
            reason: "Maintenance".to_string(),
            restart_expected: true,
        };
        let json = serde_json::to_string(&shutdown).unwrap();
        assert!(json.contains("server_shutdown"));
        assert!(json.contains("Maintenance"));
    }
}
