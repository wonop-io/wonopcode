//! Context passed to agent loop implementations.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use wonopcode_observational_memory::{MemoryState, TokenStateMachine};
use wonopcode_provider::{BoxedLanguageModel, Message as ProviderMessage, ToolDefinition};
use wonopcode_sandbox::SandboxRuntime;
use wonopcode_snapshot::SnapshotStore;
use wonopcode_tools::{ToolEvent, ToolRegistry};
use wonopcode_util::FileTimeState;

/// Details for a permission check request.
#[derive(Debug, Clone)]
pub struct PermissionCheckRequest {
    /// Unique identifier for this permission request.
    pub id: String,
    /// Tool name (e.g., "bash", "write", "edit").
    pub tool: String,
    /// Action being performed (e.g., "execute", "write", "read").
    pub action: String,
    /// Optional path involved in the operation.
    pub path: Option<String>,
    /// Human-readable description of the operation.
    pub description: String,
    /// Additional details (e.g., command being executed).
    pub details: serde_json::Value,
}

/// Trait for checking tool execution permissions.
///
/// This trait abstracts permission checking so the agent loop doesn't need
/// to depend on the full permission system. Implementations can integrate
/// with the PermissionManager or provide alternative behavior.
#[async_trait]
pub trait PermissionChecker: Send + Sync {
    /// Check if a tool operation is allowed.
    ///
    /// Returns `true` if the operation is allowed, `false` if denied.
    /// Implementations may block waiting for user input.
    ///
    /// The `timeout` parameter specifies how long to wait for a permission
    /// decision. If `None`, the implementation should wait indefinitely.
    /// If the timeout expires before a decision is made, returns `false`.
    async fn check_permission(
        &self,
        session_id: &str,
        request: PermissionCheckRequest,
        timeout: Option<std::time::Duration>,
    ) -> bool;

    /// Check if sandbox is currently running.
    ///
    /// When sandbox is running, some tools may be auto-approved.
    fn is_sandbox_running(&self) -> bool;
}

/// Trait for providing a fresh system prompt before each LLM invocation.
///
/// Implementors render the system prompt dynamically (e.g., from a Tera template)
/// with up-to-date values for date, time, git branch, AGENTS.md content, etc.
///
/// This is called by the agent loop right before every `generate()` call to ensure
/// the system prompt reflects the current state.
#[async_trait]
pub trait SystemPromptSource: Send + Sync {
    /// Render and return a fresh system prompt.
    ///
    /// Called before each LLM invocation. Implementations should:
    /// - Re-render AGENTS.md from HMS if available
    /// - Use current date/time
    /// - Check the current git branch
    async fn render_system_prompt(&self) -> Option<String>;
}

/// Configuration for the agent loop.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// System prompt to use.
    pub system_prompt: Option<String>,

    /// Maximum tokens for generation.
    pub max_tokens: Option<u32>,

    /// Temperature for generation.
    pub temperature: Option<f32>,

    /// Maximum iterations before stopping.
    /// Maximum iterations before stopping. None means unlimited.
    pub max_iterations: Option<u32>,

    /// Whether to include tool documentation in system prompt.
    pub include_tool_docs: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_tokens: Some(8192),
            temperature: Some(0.7),
            max_iterations: None,
            include_tool_docs: false,
        }
    }
}

/// Configuration for message compaction.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Maximum messages before compaction.
    pub max_messages: usize,

    /// Maximum estimated tokens before compaction.
    pub max_tokens: usize,

    /// Number of recent messages to preserve during compaction.
    pub preserve_recent: usize,

    /// Number of turns (user+assistant pairs) to keep.
    pub preserve_turns: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            max_messages: 100,
            max_tokens: 100_000,
            preserve_recent: 10,
            preserve_turns: 5,
        }
    }
}

/// Update events sent from the loop to the UI.
#[derive(Debug, Clone)]
pub enum LoopUpdate {
    /// Text delta from streaming.
    TextDelta(String),

    /// Thinking/reasoning text delta.
    ThinkingDelta(String),

    /// Tool execution started.
    ToolStarted {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Tool input JSON.
        input: String,
    },

    /// Tool execution completed.
    ToolCompleted {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Whether execution succeeded.
        success: bool,
        /// Tool output.
        output: String,
        /// Optional metadata.
        metadata: Option<serde_json::Value>,
    },

    /// Response generation completed.
    ///
    /// This signals that the agent has finished processing and the TUI
    /// should exit the "thinking" state.
    ResponseComplete {
        /// Final response text.
        text: String,
    },

    /// Token usage update.
    TokenUsage {
        /// Input tokens used (delta for this step).
        input: u32,
        /// Output tokens used (delta for this step).
        output: u32,
        /// Estimated cost (delta for this step).
        cost: f64,
        /// Context limit.
        context_limit: u32,
        /// Accumulated usage across all steps (from provider).
        /// This is the total that should be displayed to the user.
        /// If None, the receiver should accumulate input/output manually.
        accumulated_input: Option<u64>,
        accumulated_output: Option<u64>,
        accumulated_cost: Option<f64>,
        /// Input tokens for the last/current request (full context size sent to model).
        last_request_input: Option<u64>,
        /// Output tokens for the last/current request.
        last_request_output: Option<u64>,
        /// Cache read tokens for the last/current request.
        last_request_cache_read: Option<u64>,
    },

    /// Context status update for token-based compaction awareness.
    ///
    /// This update is sent after each generation step to inform the runner
    /// about the current context usage and whether compaction may be needed.
    ContextUpdate {
        /// Estimated total tokens currently in context.
        estimated_tokens: u32,
        /// Context limit from the model.
        context_limit: u32,
        /// Usage percentage (0-100).
        usage_percent: u8,
        /// Whether compaction is recommended (>80% usage).
        needs_compaction: bool,
    },

    /// Status message.
    Status(String),

    /// Error message.
    Error(String),

    /// Observational Memory state update.
    ///
    /// Sent when observations are created or reflections occur.
    /// Contains a snapshot of the current OM state for UI display.
    ObservationalMemoryUpdate(ObservationalMemoryStateSnapshot),

    /// New assistant messages to persist to session repository.
    ///
    /// Emitted BEFORE the observer drains messages from context.
    /// The runner should collect these and persist them instead of
    /// iterating ctx.messages (which may be drained by the observer).
    MessagesForPersistence(Vec<wonopcode_provider::Message>),

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
        request: Option<serde_json::Value>,
        /// Optional: Full response JSON (if recording enabled).
        response: Option<serde_json::Value>,
    },
}

/// Snapshot of Observational Memory state for UI updates.
#[derive(Debug, Clone)]
pub struct ObservationalMemoryStateSnapshot {
    /// Whether OM is enabled.
    pub enabled: bool,
    /// Current observations.
    pub observations: Vec<ObservationSnapshot>,
    /// Token count for observations.
    pub observation_tokens: u32,
    /// Reflector token threshold.
    pub reflector_threshold: u32,
    /// Token count for unobserved messages.
    pub message_tokens: u32,
    /// Observer token threshold.
    pub observer_threshold: u32,
    /// System prompt token estimate.
    pub system_tokens: u32,
    /// Total observations created.
    pub total_observations: u32,
    /// Number of reflections performed.
    pub reflections_count: u32,
    /// Average compression ratio achieved.
    pub avg_compression: f32,
    /// Estimated cost savings from caching.
    pub cache_savings: f64,
    /// Whether observations were loaded from a previous session.
    pub loaded_from_previous_session: bool,
    /// When the previous session was saved (human-readable).
    pub loaded_session_date: Option<String>,
}

/// Snapshot of a single observation for UI display.
#[derive(Debug, Clone)]
pub struct ObservationSnapshot {
    /// Unique identifier for this observation.
    pub id: String,
    /// Priority level: "high", "medium", or "low".
    pub priority: String,
    /// Timestamp when the observation was created.
    pub timestamp: String,
    /// Content of the observation.
    pub content: String,
    /// Child observations (hierarchical structure).
    pub children: Vec<ObservationSnapshot>,
    /// Whether this observation is pinned by the user.
    pub pinned: bool,
}

/// Context passed to agent loop implementations.
///
/// Contains all state and resources needed to execute a prompt.
/// Passed by mutable reference to allow loops to update conversation history.
///
/// # Lifetime
///
/// The lifetime `'a` represents the lifetime of the Runner that owns
/// the underlying resources.
pub struct LoopContext<'a> {
    /// Current working directory for file operations.
    pub cwd: &'a Path,

    /// Conversation history.
    ///
    /// Loops should append user messages, assistant responses,
    /// and tool results to this vector.
    pub messages: &'a mut Vec<ProviderMessage>,

    /// The active LLM provider.
    ///
    /// Use for streaming prompts. The provider is pre-configured
    /// based on user settings.
    pub provider: &'a BoxedLanguageModel,

    /// Registry of available tools.
    ///
    /// Query to get tool definitions for the provider.
    pub tools: &'a ToolRegistry,

    /// Tool definitions for the provider (pre-built).
    pub tool_defs: Vec<ToolDefinition>,

    /// Cancellation token.
    ///
    /// Check `is_cancelled()` periodically and return
    /// `LoopError::Cancelled` if triggered.
    pub cancel: &'a CancellationToken,

    /// Optional snapshot store for file versioning.
    pub snapshot_store: Option<&'a Arc<SnapshotStore>>,

    /// File modification time tracker.
    pub file_time: Arc<FileTimeState>,

    /// Optional sandbox runtime for isolated execution.
    pub sandbox: Option<Arc<dyn SandboxRuntime>>,

    /// Loop configuration.
    pub config: &'a LoopConfig,

    /// Message compaction configuration.
    pub compaction_config: &'a CompactionConfig,

    /// Channel for sending updates to the UI.
    pub update_tx: &'a mpsc::UnboundedSender<LoopUpdate>,

    /// Session ID (for tracking).
    pub session_id: String,

    /// Optional channel for tools to emit events (like `TodosUpdated`).
    ///
    /// When set, tools can send events through this channel to notify
    /// the system of state changes (e.g., task creation, status updates).
    /// These events can be forwarded to the UI for real-time updates.
    pub tool_event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,

    /// Optional permission checker for tool execution.
    ///
    /// When set, tool execution will check permissions before running.
    /// If permission is denied, the tool will not execute.
    pub permission_checker: Option<Arc<dyn PermissionChecker>>,

    /// Optional ticket service for ticket management tools.
    ///
    /// When set, ticket tools (list, search, read, create) can access
    /// configured issue trackers.
    pub ticket_service: Option<Arc<dyn wonopcode_tools::TicketService>>,

    /// Optional memory service for memory tools (store, recall, search, clear).
    ///
    /// When set, memory tools can access the Super Memory system for
    /// persistent storage across global, workstream, and session scopes.
    pub memory_service: Option<wonopcode_tools::SharedMemoryService>,

    /// Optional HMS service for hierarchical memory system.
    ///
    /// When set, HMS tools can access memory.yaml files and render AGENTS.md.
    pub hms_service: Option<wonopcode_tools::SharedHmsService>,

    /// Optional system prompt source for dynamic rendering.
    ///
    /// When set, the agent loop calls this before every LLM invocation to get
    /// a fresh system prompt with up-to-date date/time, git branch, and
    /// re-rendered AGENTS.md content. This replaces `config.system_prompt`.
    pub system_prompt_source: Option<Arc<dyn SystemPromptSource>>,

    /// Optional permission checker for TypeScript tools.
    ///
    /// When set, TypeScript code running in the execute_typescript tool
    /// can request fine-grained permissions for operations like file writes.
    pub ts_permission_checker: Option<Arc<dyn wonopcode_tools::TsPermissionChecker>>,

    /// Optional workstream ticket ID.
    ///
    /// When set, this is the ticket ID associated with the current workstream.
    /// Used to determine the default tracker for ticket operations.
    pub workstream_ticket_id: Option<String>,

    /// Optional default tracker ID for the workstream.
    ///
    /// When set, ticket operations will use this tracker as the default
    /// instead of the first available tracker.
    pub workstream_default_tracker_id: Option<String>,

    /// Images attached to the current prompt.
    ///
    /// When set, these images should be included with the user message.
    /// Each image contains base64 data and MIME type.
    pub prompt_images: Vec<PromptImage>,

    // =========================================================================
    // Observational Memory (OM) fields
    // =========================================================================

    /// Optional memory state for Observational Memory.
    ///
    /// When set, the agent loop can use OM to compress messages into
    /// observations, maintaining a bounded context window.
    pub memory_state: Option<&'a mut MemoryState>,

    /// Optional token state machine for OM threshold management.
    ///
    /// When set, tracks message/observation token counts and triggers
    /// Observer/Reflector when thresholds are exceeded.
    pub token_state_machine: Option<&'a mut TokenStateMachine>,

    /// Whether Observational Memory is enabled.
    ///
    /// When true and memory_state/token_state_machine are set,
    /// the agent loop will use OM instead of legacy compaction.
    pub om_enabled: bool,

    /// Number of messages that existed at the start of the loop.
    ///
    /// Used by the agent loop to determine which messages are "new" (added during this turn)
    /// and need to be persisted before the observer drains them.
    pub messages_count_at_start: usize,
    
    /// Project directory for Observational Memory persistence.
    ///
    /// When set, observations are saved to disk continuously after
    /// Observer and Reflector runs. This ensures observations survive
    /// unexpected app termination (e.g., Cmd+Q).
    pub om_project_dir: Option<PathBuf>,

    /// Optional TypeScript executor for worker process mode.
    ///
    /// When set, TypeScript execution will be routed to an isolated
    /// worker process instead of running in-process with V8.
    pub typescript_executor: Option<wonopcode_tools::SharedTypescriptExecutor>,
}

/// Image attached to a prompt.
#[derive(Debug, Clone)]
pub struct PromptImage {
    /// Unique identifier for this image.
    pub id: String,
    /// Base64-encoded image data (without data: prefix).
    pub data: String,
    /// MIME type (e.g., "image/png", "image/jpeg").
    pub media_type: String,
}

impl<'a> LoopContext<'a> {
    /// Check if the operation has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Send an update to the UI.
    pub fn send_update(&self, update: LoopUpdate) {
        // Best effort - don't fail if channel is closed
        let _ = self.update_tx.send(update);
    }

    /// Send a text delta update.
    pub fn send_text(&self, text: impl Into<String>) {
        self.send_update(LoopUpdate::TextDelta(text.into()));
    }

    /// Send a status message.
    pub fn send_status(&self, status: impl Into<String>) {
        self.send_update(LoopUpdate::Status(status.into()));
    }

    /// Check if sandbox execution is enabled.
    pub fn is_sandboxed(&self) -> bool {
        self.sandbox.is_some()
    }

    /// Get the context limit from the provider.
    pub fn context_limit(&self) -> u32 {
        self.provider.model_info().limit.context
    }

    /// Get the model info from the provider.
    pub fn model_info(&self) -> &wonopcode_provider::ModelInfo {
        self.provider.model_info()
    }

    /// Check if Observational Memory is enabled and ready.
    pub fn is_om_ready(&self) -> bool {
        self.om_enabled && self.memory_state.is_some() && self.token_state_machine.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_config_default() {
        let config = LoopConfig::default();
        assert_eq!(config.max_iterations, None);
        assert_eq!(config.max_tokens, Some(8192));
    }

    #[test]
    fn test_compaction_config_default() {
        let config = CompactionConfig::default();
        assert_eq!(config.max_messages, 100);
        assert_eq!(config.preserve_turns, 5);
    }

    #[test]
    fn test_loop_update_variants() {
        let update = LoopUpdate::TextDelta("hello".into());
        if let LoopUpdate::TextDelta(text) = update {
            assert_eq!(text, "hello");
        }

        let update = LoopUpdate::Status("working...".into());
        if let LoopUpdate::Status(status) = update {
            assert_eq!(status, "working...");
        }
    }
}