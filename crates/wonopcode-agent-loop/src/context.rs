//! Context passed to agent loop implementations.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use wonopcode_provider::{BoxedLanguageModel, Message as ProviderMessage, ToolDefinition};
use wonopcode_sandbox::SandboxRuntime;
use wonopcode_snapshot::SnapshotStore;
use wonopcode_tools::ToolRegistry;
use wonopcode_util::FileTimeState;

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
    pub max_iterations: u32,

    /// Whether to include tool documentation in system prompt.
    pub include_tool_docs: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            system_prompt: None,
            max_tokens: Some(8192),
            temperature: Some(0.7),
            max_iterations: 50,
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
        /// Input tokens used.
        input: u32,
        /// Output tokens used.
        output: u32,
        /// Estimated cost.
        cost: f64,
        /// Context limit.
        context_limit: u32,
    },

    /// Status message.
    Status(String),

    /// Error message.
    Error(String),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_config_default() {
        let config = LoopConfig::default();
        assert_eq!(config.max_iterations, 50);
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
