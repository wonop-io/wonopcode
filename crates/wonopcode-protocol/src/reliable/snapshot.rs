//! Workstream state snapshot for connection/reconnection.
//!
//! The snapshot contains the complete state needed to render the UI
//! immediately on connect, without needing to wait for events.

use serde::{Deserialize, Serialize};

use super::events::{
    ArtifactSummary, LspServerSnapshot, McpServerSnapshot, ModifiedFileSnapshot, TodoPhaseSnapshot,
};
use super::input::UserInputRequest;
use super::server::SandboxState;

/// Complete state snapshot for a workstream.
///
/// This is sent when a client connects to provide them with all current state.
/// It's the "source of truth" for the workstream at a point in time.
///
/// # State Reconstruction
///
/// Clients should:
/// 1. Clear any existing state for the workstream
/// 2. Load all fields from the snapshot
/// 3. Subscribe for events from `last_seq` to receive updates
///
/// # Streaming Messages
///
/// If the agent is currently streaming a response, the snapshot includes:
/// - `has_streaming_message: true`
/// - The last message in `messages` is the partial/streaming message
///
/// This allows the client to show the current state of the streaming message
/// and continue receiving `TextDelta` and `ToolStarted` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstreamStateSnapshot {
    /// Workstream ID.
    pub workstream_id: String,

    // =========================================================================
    // Agent State
    // =========================================================================
    /// Whether the agent is currently processing.
    pub agent_busy: bool,

    /// Agent mode ("native", "subprocess").
    pub agent: String,

    /// Current model ID (e.g., "claude-sonnet-4-5-20250929").
    pub model_id: String,

    /// Current provider (e.g., "anthropic").
    pub provider: String,

    // =========================================================================
    // Sandbox State
    // =========================================================================
    /// Sandbox state.
    pub sandbox_state: SandboxState,

    /// Sandbox container ID if running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_container_id: Option<String>,

    // =========================================================================
    // Session Metrics
    // =========================================================================
    /// Total input tokens this session.
    pub tokens_in: u64,

    /// Total output tokens this session.
    pub tokens_out: u64,

    /// Total cost this session (USD).
    pub total_cost: f64,

    /// Model context window limit.
    pub context_limit: u32,

    // =========================================================================
    // Conversation History
    // =========================================================================
    /// Complete conversation history.
    ///
    /// Includes all messages with full `MessagePart` detail for rich reconstruction.
    /// If there's an in-progress streaming message, it's the last item.
    pub messages: Vec<ConversationHistoryMessage>,

    /// Whether the last message is an in-progress streaming message.
    ///
    /// If true, the client should:
    /// 1. Display the last message as "streaming"
    /// 2. Continue receiving `TextDelta` and `ToolStarted` events
    #[serde(default)]
    pub has_streaming_message: bool,

    // =========================================================================
    // TODO State
    // =========================================================================
    /// Current TODO phases.
    pub todo_phases: Vec<TodoPhaseSnapshot>,

    // =========================================================================
    // Server Status
    // =========================================================================
    /// MCP server statuses.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerSnapshot>,

    /// LSP server statuses.
    #[serde(default)]
    pub lsp_servers: Vec<LspServerSnapshot>,

    // =========================================================================
    // Git State
    // =========================================================================
    /// Modified files in the workstream.
    #[serde(default)]
    pub modified_files: Vec<ModifiedFileSnapshot>,

    // =========================================================================
    // Input Requests
    // =========================================================================
    /// Number of pending permission requests.
    #[serde(default)]
    pub permissions_pending: usize,

    /// Pending input requests from the InputRequestQueue.
    ///
    /// This allows the client to show pending permission dialogs
    /// immediately on connect, without needing to poll.
    #[serde(default)]
    pub pending_input_requests: Vec<UserInputRequest>,

    // =========================================================================
    // ACE Artifacts
    // =========================================================================
    /// ACE artifacts in this workstream.
    #[serde(default)]
    pub artifacts: Vec<ArtifactSummary>,

    /// Current ACE workflow phase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ace_phase: Option<String>,

    // =========================================================================
    // Settings
    // =========================================================================
    /// Whether "allow all" mode is enabled.
    #[serde(default)]
    pub allow_all: bool,

    /// Whether context compaction is currently in progress.
    #[serde(default)]
    pub is_compacting: bool,

    /// Current compaction progress details (if compacting).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_progress: Option<CompactionProgressSnapshot>,

    // =========================================================================
    // Observational Memory State
    // =========================================================================
    /// Observational Memory state snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observational_memory: Option<ObservationalMemorySnapshot>,

    // =========================================================================
    // Validation Fields
    // =========================================================================
    /// Sequence number for this snapshot (prevents out-of-order updates).
    #[serde(default)]
    pub sequence: u64,

    /// Timestamp when this snapshot was created (milliseconds since epoch).
    #[serde(default)]
    pub timestamp: u64,
}

/// Snapshot of current compaction progress.
///
/// This allows the client to restore the compaction-in-progress message
/// with accurate details when switching workstreams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionProgressSnapshot {
    /// Type of compaction: "automatic", "emergency", or "manual".
    pub compaction_type: String,
    /// Number of messages being compacted.
    pub messages_before: usize,
    /// Current chunk being processed (1-indexed).
    pub current_chunk: usize,
    /// Total number of chunks.
    pub total_chunks: usize,
    /// Current phase: "summarizing" or "combining".
    pub phase: String,
}

/// A message in conversation history.
///
/// This is the V2 format with rich `MessagePart` support for full reconstruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHistoryMessage {
    /// Message ID.
    pub id: String,

    /// Timestamp (ISO8601).
    pub timestamp: String,

    /// Sender ("user" or "assistant").
    pub sender: String,

    /// Combined text content (for backward compatibility).
    ///
    /// This is a simple concatenation of all text parts.
    /// For rich display, use `parts` instead.
    pub content: String,

    /// Message type ("text", "tool", "mixed").
    pub message_type: String,

    /// Full message parts for rich reconstruction.
    ///
    /// If present, use this for display instead of `content`.
    /// Parts are ordered and may interleave text and tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<super::events::MessagePart>>,

    /// Schema version (2 = has parts).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

fn default_schema_version() -> u32 {
    2
}

// =============================================================================
// Observational Memory Types
// =============================================================================

/// Snapshot of observational memory state for the UI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservationalMemorySnapshot {
    /// Whether observational memory is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Current observations.
    #[serde(default)]
    pub observations: Vec<ObservationSnapshot>,

    /// Tokens used by observations.
    #[serde(default)]
    pub observation_tokens: u32,

    /// Reflector threshold (when reflector triggers).
    #[serde(default)]
    pub reflector_threshold: u32,

    /// Tokens used by unobserved messages.
    #[serde(default)]
    pub message_tokens: u32,

    /// Observer threshold (when observer triggers).
    #[serde(default)]
    pub observer_threshold: u32,

    /// System prompt tokens (relatively static).
    #[serde(default)]
    pub system_tokens: u32,

    /// Total observations created this session.
    #[serde(default)]
    pub total_observations: u32,

    /// Number of reflections performed.
    #[serde(default)]
    pub reflections_count: u32,

    /// Average compression ratio achieved.
    #[serde(default)]
    pub avg_compression: f32,

    /// Estimated cost savings from prompt caching.
    #[serde(default)]
    pub cache_savings: f64,
}

/// Single observation in the OM system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationSnapshot {
    /// Unique identifier.
    pub id: String,

    /// Priority: "high", "medium", "low" (🔴, 🟡, 🟢).
    pub priority: String,

    /// When the observation was created (ISO timestamp or time string).
    pub timestamp: String,

    /// The observation content.
    pub content: String,

    /// Child observations (hierarchical structure).
    #[serde(default)]
    pub children: Vec<ObservationSnapshot>,

    /// Whether the user has pinned this observation.
    #[serde(default)]
    pub pinned: bool,
}

impl Default for WorkstreamStateSnapshot {
    fn default() -> Self {
        Self {
            workstream_id: String::new(),
            agent_busy: false,
            agent: "native".to_string(),
            model_id: "claude-sonnet-4-5-20250929".to_string(),
            provider: "anthropic".to_string(),
            sandbox_state: SandboxState::Stopped,
            sandbox_container_id: None,
            tokens_in: 0,
            tokens_out: 0,
            total_cost: 0.0,
            context_limit: 200000,
            messages: Vec::new(),
            has_streaming_message: false,
            todo_phases: Vec::new(),
            mcp_servers: Vec::new(),
            lsp_servers: Vec::new(),
            modified_files: Vec::new(),
            permissions_pending: 0,
            pending_input_requests: Vec::new(),
            artifacts: Vec::new(),
            ace_phase: None,
            allow_all: false,
            is_compacting: false,
            compaction_progress: None,
            observational_memory: None,
            sequence: 0,
            timestamp: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_default() {
        let snapshot = WorkstreamStateSnapshot::default();
        assert!(!snapshot.agent_busy);
        assert_eq!(snapshot.context_limit, 200000);
        assert!(snapshot.messages.is_empty());
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = WorkstreamStateSnapshot {
            workstream_id: "feature/login".to_string(),
            agent_busy: true,
            agent: "native".to_string(),
            model_id: "claude-sonnet-4-5-20250929".to_string(),
            provider: "anthropic".to_string(),
            sandbox_state: SandboxState::Running,
            sandbox_container_id: Some("container-123".to_string()),
            tokens_in: 1000,
            tokens_out: 500,
            total_cost: 0.05,
            context_limit: 200000,
            messages: vec![ConversationHistoryMessage {
                id: "msg-1".to_string(),
                timestamp: "2025-02-19T12:00:00Z".to_string(),
                sender: "user".to_string(),
                content: "Hello".to_string(),
                message_type: "text".to_string(),
                parts: None,
                schema_version: 2,
            }],
            has_streaming_message: false,
            todo_phases: vec![],
            mcp_servers: vec![],
            lsp_servers: vec![],
            modified_files: vec![],
            permissions_pending: 1,
            pending_input_requests: vec![],
            artifacts: vec![],
            ace_phase: Some("implementation".to_string()),
            allow_all: false,
            is_compacting: false,
            compaction_progress: None,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("feature/login"));
        assert!(json.contains("running"));
        assert!(json.contains("implementation"));

        let parsed: WorkstreamStateSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.workstream_id, "feature/login");
        assert!(parsed.agent_busy);
        assert_eq!(parsed.tokens_in, 1000);
    }

    #[test]
    fn test_conversation_history_message_serialization() {
        use super::super::events::MessagePart;

        let msg = ConversationHistoryMessage {
            id: "msg-1".to_string(),
            timestamp: "2025-02-19T12:00:00Z".to_string(),
            sender: "assistant".to_string(),
            content: "Let me help you with that.".to_string(),
            message_type: "mixed".to_string(),
            parts: Some(vec![
                MessagePart::Text {
                    id: "part-1".to_string(),
                    text: "Let me help you with that.".to_string(),
                    order: Some(0),
                },
                MessagePart::Tool {
                    id: "part-2".to_string(),
                    call_id: "call-1".to_string(),
                    name: "bash".to_string(),
                    state: super::super::events::ToolState::Completed {
                        input: serde_json::json!({"command": "ls"}),
                        output: "file1.txt".to_string(),
                        title: None,
                    },
                    order: Some(1),
                },
            ]),
            schema_version: 2,
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("mixed"));
        assert!(json.contains("bash"));
        assert!(json.contains("file1.txt"));

        let parsed: ConversationHistoryMessage = serde_json::from_str(&json).unwrap();
        assert!(parsed.parts.is_some());
        assert_eq!(parsed.parts.unwrap().len(), 2);
    }

    #[test]
    fn test_snapshot_with_streaming_message() {
        let snapshot = WorkstreamStateSnapshot {
            workstream_id: "main".to_string(),
            agent_busy: true,
            has_streaming_message: true,
            messages: vec![
                ConversationHistoryMessage {
                    id: "msg-1".to_string(),
                    timestamp: "2025-02-19T12:00:00Z".to_string(),
                    sender: "user".to_string(),
                    content: "Hello".to_string(),
                    message_type: "text".to_string(),
                    parts: None,
                    schema_version: 2,
                },
                ConversationHistoryMessage {
                    id: "streaming_msg".to_string(),
                    timestamp: "2025-02-19T12:00:01Z".to_string(),
                    sender: "assistant".to_string(),
                    content: "I'm working on".to_string(), // Partial
                    message_type: "text".to_string(),
                    parts: None,
                    schema_version: 2,
                },
            ],
            ..Default::default()
        };

        assert!(snapshot.has_streaming_message);
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[1].id, "streaming_msg");
    }
}
