//! Workstream event types.
//!
//! Events are pushed from the server to subscribed clients in real-time.
//! They use sequence numbers for ordering and gap detection.

use serde::{Deserialize, Serialize};

/// Events for a specific workstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkstreamEventData {
    // =========================================================================
    // Agent State
    // =========================================================================
    /// Agent started processing a prompt.
    AgentBusy,

    /// Agent finished processing.
    AgentIdle,

    // =========================================================================
    // Streaming Content
    // =========================================================================
    /// Text delta from the agent.
    TextDelta {
        /// Incremental text content.
        delta: String,
        /// Position in the message (for ordering when interleaved with tools).
        position: u32,
    },

    /// Thinking/reasoning delta (extended thinking).
    ThinkingDelta {
        /// Incremental thinking content.
        delta: String,
    },

    // =========================================================================
    // Tool Execution
    // =========================================================================
    /// Tool execution started.
    ToolStarted {
        /// Tool call ID.
        call_id: String,
        /// Tool name.
        tool_name: String,
        /// Tool input as JSON.
        tool_input: serde_json::Value,
        /// Position in the message (for interleaving with text).
        position: u32,
    },

    /// Tool execution progress update.
    ToolProgress {
        /// Tool call ID.
        call_id: String,
        /// Progress message or partial output.
        progress: String,
    },

    /// Tool execution completed.
    ToolCompleted {
        /// Tool call ID.
        call_id: String,
        /// Tool output.
        output: String,
        /// Whether the tool succeeded.
        success: bool,
        /// Duration in milliseconds.
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// Optional metadata.
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    // =========================================================================
    // Message Lifecycle
    // =========================================================================
    /// A complete message was persisted to history.
    ///
    /// This is sent after the agent completes a turn and the message
    /// is saved to the session store.
    MessagePersisted {
        /// Message ID.
        message_id: String,
        /// Sender role ("user" or "assistant").
        sender: String,
        /// Full message parts for reconstruction.
        parts: Vec<MessagePart>,
    },

    /// Turn completed (all tool calls resolved).
    TurnCompleted {
        /// Final text content.
        content: String,
        /// Stop reason ("end_turn", "tool_use", "max_tokens").
        stop_reason: String,
    },

    // =========================================================================
    // Token Usage
    // =========================================================================
    /// Token usage update.
    TokenUsage {
        /// Input tokens for this request.
        input_tokens: u64,
        /// Output tokens for this request.
        output_tokens: u64,
        /// Cache read tokens (if caching enabled).
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_read_tokens: Option<u64>,
        /// Cache write tokens (if caching enabled).
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_write_tokens: Option<u64>,
        /// Total cost in USD for this request.
        total_cost: f64,
        /// Model context window limit.
        context_limit: u32,
    },

    // =========================================================================
    // TODO Updates
    // =========================================================================
    /// TODO list updated.
    TodosUpdated {
        /// Updated phases.
        phases: Vec<TodoPhaseSnapshot>,
    },

    // =========================================================================
    // Sandbox State
    // =========================================================================
    /// Sandbox state changed.
    SandboxStateChanged {
        /// New sandbox state.
        state: super::server::SandboxState,
        /// Container ID if running.
        #[serde(skip_serializing_if = "Option::is_none")]
        container_id: Option<String>,
        /// Error message if state is error.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // =========================================================================
    // Git State
    // =========================================================================
    /// Git files changed.
    GitFilesChanged {
        /// Modified files.
        modified_files: Vec<ModifiedFileSnapshot>,
    },

    // =========================================================================
    // Artifacts (ACE workflow)
    // =========================================================================
    /// Artifacts updated.
    ArtifactsUpdated {
        /// Updated artifacts.
        artifacts: Vec<ArtifactSummary>,
        /// Current ACE phase.
        #[serde(skip_serializing_if = "Option::is_none")]
        ace_phase: Option<String>,
    },

    // =========================================================================
    // MCP/LSP Status
    // =========================================================================
    /// MCP servers status changed.
    McpServersUpdated {
        /// Server statuses.
        servers: Vec<McpServerSnapshot>,
    },

    /// LSP servers status changed.
    LspServersUpdated {
        /// Server statuses.
        servers: Vec<LspServerSnapshot>,
    },

    // =========================================================================
    // Error
    // =========================================================================
    /// Error occurred.
    Error {
        /// Error message.
        error: String,
        /// Whether the error is recoverable.
        recoverable: bool,
    },

    // =========================================================================
    // Status
    // =========================================================================
    /// Status message from the agent.
    Status {
        /// Status message.
        message: String,
    },

    // =========================================================================
    // Permission State
    // =========================================================================
    /// Allow-all mode changed.
    AllowAllChanged {
        /// Whether allow-all mode is enabled.
        enabled: bool,
    },

    // =========================================================================
    // Context Compaction
    // =========================================================================
    /// Context compaction has started (show running indicator).
    CompactionStarted {
        /// Type of compaction: "automatic", "emergency", or "manual".
        compaction_type: String,
        /// Number of messages before compaction.
        messages_before: usize,
    },

    /// Context compaction was performed (completed).
    CompactionPerformed {
        /// Type of compaction: "automatic", "emergency", or "manual".
        compaction_type: String,
        /// Number of messages before compaction.
        messages_before: usize,
        /// Number of messages after compaction.
        messages_after: usize,
        /// Token count before compaction.
        tokens_before: u32,
        /// Token count after compaction.
        tokens_after: u32,
        /// Optional summary of compacted content.
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    /// Context compaction was not needed (message count below threshold).
    CompactionNotNeeded,

    /// Context compaction progress update (for chunked summarization).
    CompactionProgress {
        /// Current chunk being processed (1-indexed).
        current_chunk: usize,
        /// Total number of chunks.
        total_chunks: usize,
        /// Phase: "summarizing" or "combining".
        phase: String,
    },

    // =========================================================================
    // Observational Memory
    // =========================================================================
    /// Observational Memory state update.
    ObservationalMemoryUpdate {
        /// Whether OM is enabled.
        enabled: bool,
        /// Current observations.
        observations: Vec<super::ObservationSnapshot>,
        /// Token count for observations.
        observation_tokens: u32,
        /// Reflector token threshold.
        reflector_threshold: u32,
        /// Token count for unobserved messages.
        message_tokens: u32,
        /// Observer token threshold.
        observer_threshold: u32,
        /// System prompt token estimate.
        system_tokens: u32,
        /// Total observations created.
        total_observations: u32,
        /// Number of reflections performed.
        reflections_count: u32,
        /// Average compression ratio achieved.
        avg_compression: f32,
        /// Estimated cost savings from caching.
        cache_savings: f64,
        /// Whether observations were loaded from a previous session.
        #[serde(default)]
        loaded_from_previous_session: bool,
        /// When the previous session was saved (human-readable).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loaded_session_date: Option<String>,
    },

    // =========================================================================
    // Developer Mode Statistics
    // =========================================================================
    /// Completion statistics recorded (Developer Mode feature).
    ///
    /// Emitted after each LLM completion finishes, containing timing
    /// and usage data for debugging and monitoring.
    CompletionRecorded {
        /// Unique ID for this completion.
        id: String,
        /// Unix timestamp (ms) when completion started.
        timestamp: f64,
        /// Model ID used.
        model: String,
        /// Input tokens for this completion.
        input_tokens: u64,
        /// Output tokens for this completion.
        output_tokens: u64,
        /// Cache read tokens (if applicable).
        cache_read_tokens: u64,
        /// Estimated cost for this completion.
        cost: f64,
        /// Time to first token (ms).
        latency_ms: u64,
        /// Total duration from start to finish (ms).
        total_duration_ms: u64,
        /// Finish reason (end_turn, tool_use, max_tokens, etc.).
        finish_reason: String,
        /// Optional: Full request JSON (if recording enabled).
        #[serde(skip_serializing_if = "Option::is_none")]
        request: Option<serde_json::Value>,
        /// Optional: Full response JSON (if recording enabled).
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<serde_json::Value>,
    },
}

/// A part of a message (for rich reconstruction).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePart {
    /// Text content.
    Text {
        /// Part ID.
        id: String,
        /// Text content.
        text: String,
        /// Order within the message.
        #[serde(skip_serializing_if = "Option::is_none")]
        order: Option<u32>,
    },

    /// Thinking/reasoning content.
    Thinking {
        /// Part ID.
        id: String,
        /// Thinking content.
        text: String,
        /// Order within the message.
        #[serde(skip_serializing_if = "Option::is_none")]
        order: Option<u32>,
    },

    /// Tool call.
    Tool {
        /// Part ID.
        id: String,
        /// Tool call ID.
        call_id: String,
        /// Tool name.
        name: String,
        /// Tool state.
        state: ToolState,
        /// Order within the message.
        #[serde(skip_serializing_if = "Option::is_none")]
        order: Option<u32>,
    },

    /// Compaction marker indicating context was compacted.
    Compaction {
        /// Part ID.
        id: String,
        /// Type of compaction: "automatic", "emergency", or "manual".
        compaction_type: String,
        /// Number of messages before compaction.
        messages_before: usize,
        /// Number of messages after compaction.
        messages_after: usize,
        /// Token count before compaction.
        tokens_before: u32,
        /// Token count after compaction.
        tokens_after: u32,
        /// Optional summary of compacted content.
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

/// Tool execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolState {
    /// Tool is pending (not yet started).
    Pending {
        /// Tool input.
        input: serde_json::Value,
    },

    /// Tool is running.
    Running {
        /// Tool input.
        input: serde_json::Value,
        /// Optional title/description.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },

    /// Tool completed successfully.
    Completed {
        /// Tool input.
        input: serde_json::Value,
        /// Tool output.
        output: String,
        /// Title/description.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },

    /// Tool failed.
    Error {
        /// Tool input.
        input: serde_json::Value,
        /// Error message.
        error: String,
    },
}

/// TODO phase snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoPhaseSnapshot {
    /// Phase ID.
    pub id: String,
    /// Phase name.
    pub name: String,
    /// Phase status ("not_started", "in_progress", "finished").
    pub status: String,
    /// TODO items in this phase.
    pub todos: Vec<TodoItemSnapshot>,
}

/// TODO item snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItemSnapshot {
    /// TODO ID.
    pub id: String,
    /// TODO content/description.
    pub content: String,
    /// Status ("pending", "in_progress", "completed", "cancelled").
    pub status: String,
    /// Priority ("high", "medium", "low").
    pub priority: String,
    /// Parent artifact IDs.
    #[serde(default)]
    pub parents: Vec<String>,
}

/// Modified file snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedFileSnapshot {
    /// File path relative to workstream root.
    pub path: String,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
}

/// ACE artifact summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSummary {
    /// Artifact ID (e.g., "UC-001", "REQ-002").
    pub id: String,
    /// Artifact type ("use-case", "requirement", "design", etc.).
    pub artifact_type: String,
    /// Artifact title.
    pub title: String,
    /// Progress status ("draft", "ready", "approved").
    pub progress: String,
    /// Priority ("high", "medium", "low").
    pub priority: String,
    /// Parent artifact IDs.
    #[serde(default)]
    pub parents: Vec<String>,
    /// Whether artifact is in staging.
    #[serde(default)]
    pub is_staged: bool,
}

/// MCP server snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSnapshot {
    /// Server name.
    pub name: String,
    /// Whether connected.
    pub connected: bool,
    /// Whether enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Error message if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Number of tools provided.
    #[serde(default)]
    pub tool_count: usize,
}

/// LSP server snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerSnapshot {
    /// Server ID.
    pub id: String,
    /// Server name/language.
    pub name: String,
    /// Root path being served.
    pub root: String,
    /// Whether connected successfully.
    pub connected: bool,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_delta_serialization() {
        let event = WorkstreamEventData::TextDelta {
            delta: "Hello, world!".to_string(),
            position: 0,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("text_delta"));
        assert!(json.contains("Hello, world!"));

        let parsed: WorkstreamEventData = serde_json::from_str(&json).unwrap();
        if let WorkstreamEventData::TextDelta { delta, position } = parsed {
            assert_eq!(delta, "Hello, world!");
            assert_eq!(position, 0);
        } else {
            panic!("Wrong event type");
        }
    }

    #[test]
    fn test_tool_started_serialization() {
        let event = WorkstreamEventData::ToolStarted {
            call_id: "call-123".to_string(),
            tool_name: "bash".to_string(),
            tool_input: serde_json::json!({"command": "ls -la"}),
            position: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_started"));
        assert!(json.contains("bash"));
        assert!(json.contains("ls -la"));
    }

    #[test]
    fn test_tool_completed_serialization() {
        let event = WorkstreamEventData::ToolCompleted {
            call_id: "call-123".to_string(),
            output: "file1.txt\nfile2.txt".to_string(),
            success: true,
            duration_ms: Some(150),
            metadata: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("tool_completed"));
        assert!(json.contains("file1.txt"));
    }

    #[test]
    fn test_message_part_serialization() {
        let text_part = MessagePart::Text {
            id: "part-1".to_string(),
            text: "Hello".to_string(),
            order: Some(0),
        };
        let json = serde_json::to_string(&text_part).unwrap();
        assert!(json.contains("text"));
        assert!(json.contains("Hello"));

        let tool_part = MessagePart::Tool {
            id: "part-2".to_string(),
            call_id: "call-123".to_string(),
            name: "bash".to_string(),
            state: ToolState::Completed {
                input: serde_json::json!({}),
                output: "done".to_string(),
                title: None,
            },
            order: Some(1),
        };
        let json = serde_json::to_string(&tool_part).unwrap();
        assert!(json.contains("tool"));
        assert!(json.contains("completed"));
    }

    #[test]
    fn test_todo_phase_serialization() {
        let phase = TodoPhaseSnapshot {
            id: "phase-1".to_string(),
            name: "Implementation".to_string(),
            status: "in_progress".to_string(),
            todos: vec![TodoItemSnapshot {
                id: "todo-1".to_string(),
                content: "Write tests".to_string(),
                status: "pending".to_string(),
                priority: "high".to_string(),
                parents: vec!["REQ-001".to_string()],
            }],
        };
        let json = serde_json::to_string(&phase).unwrap();
        assert!(json.contains("Implementation"));
        assert!(json.contains("Write tests"));
        assert!(json.contains("REQ-001"));
    }

    #[test]
    fn test_token_usage_serialization() {
        let event = WorkstreamEventData::TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: Some(200),
            cache_write_tokens: None,
            total_cost: 0.02,
            context_limit: 200000,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("token_usage"));
        assert!(json.contains("1000"));
        assert!(json.contains("200000"));
    }
}
