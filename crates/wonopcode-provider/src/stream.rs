//! Streaming response types.

use serde::{Deserialize, Serialize};

/// A chunk from a streaming AI response.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Text content is starting.
    TextStart,
    /// Text content delta.
    TextDelta(String),
    /// Text content ended.
    TextEnd,

    /// Reasoning/thinking content is starting.
    ReasoningStart,
    /// Reasoning content delta.
    ReasoningDelta(String),
    /// Reasoning content ended.
    ReasoningEnd,

    /// A tool call is starting.
    ToolCallStart {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
    },
    /// Tool call arguments delta (streaming JSON).
    ToolCallDelta {
        /// Tool call ID.
        id: String,
        /// JSON delta.
        delta: String,
    },
    /// Tool call completed.
    ToolCall {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Complete arguments JSON.
        arguments: String,
    },

    /// Tool execution observed (for CLI providers where tools are executed externally).
    /// This signals that a tool was executed but the runner should NOT execute it again.
    ToolObserved {
        /// Tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Tool input arguments.
        input: String,
    },

    /// Tool result observed (for CLI providers where tools are executed externally).
    ToolResultObserved {
        /// Tool call ID.
        id: String,
        /// Whether the tool succeeded.
        success: bool,
        /// Tool output.
        output: String,
    },

    /// A step in the response is finishing.
    FinishStep {
        /// Token usage for this step.
        usage: Usage,
        /// Reason for finishing.
        finish_reason: FinishReason,
    },

    /// An error occurred.
    Error(String),
}

impl StreamChunk {
    /// Create a text delta chunk.
    pub fn text(delta: impl Into<String>) -> Self {
        Self::TextDelta(delta.into())
    }

    /// Create a reasoning delta chunk.
    pub fn reasoning(delta: impl Into<String>) -> Self {
        Self::ReasoningDelta(delta.into())
    }

    /// Create a tool call start chunk.
    pub fn tool_call_start(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self::ToolCallStart {
            id: id.into(),
            name: name.into(),
        }
    }

    /// Create a tool call completed chunk.
    pub fn tool_call(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    /// Check if this is a text-related chunk.
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            StreamChunk::TextStart | StreamChunk::TextDelta(_) | StreamChunk::TextEnd
        )
    }

    /// Check if this is a reasoning-related chunk.
    pub fn is_reasoning(&self) -> bool {
        matches!(
            self,
            StreamChunk::ReasoningStart
                | StreamChunk::ReasoningDelta(_)
                | StreamChunk::ReasoningEnd
        )
    }

    /// Check if this is a tool-related chunk.
    pub fn is_tool(&self) -> bool {
        matches!(
            self,
            StreamChunk::ToolCallStart { .. }
                | StreamChunk::ToolCallDelta { .. }
                | StreamChunk::ToolCall { .. }
                | StreamChunk::ToolObserved { .. }
                | StreamChunk::ToolResultObserved { .. }
        )
    }
}

/// Token usage information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens used.
    pub input_tokens: u32,
    /// Output tokens generated.
    pub output_tokens: u32,
    /// Tokens from cache read.
    #[serde(default)]
    pub cache_read_tokens: u32,
    /// Tokens written to cache.
    #[serde(default)]
    pub cache_write_tokens: u32,
    /// Reasoning tokens (for models with thinking).
    #[serde(default)]
    pub reasoning_tokens: u32,
}

impl Usage {
    /// Create a new usage with input and output tokens.
    pub fn new(input_tokens: u32, output_tokens: u32) -> Self {
        Self {
            input_tokens,
            output_tokens,
            ..Default::default()
        }
    }

    /// Total tokens (input + output).
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }

    /// Merge with another usage (adding all counts).
    pub fn merge(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
    }
}

/// Reason for finishing a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Normal completion (end of turn).
    EndTurn,
    /// Stopped due to stop sequence.
    Stop,
    /// Stopped due to max tokens.
    MaxTokens,
    /// Stopped for tool use.
    ToolUse,
    /// Content was filtered.
    ContentFilter,
    /// Unknown or other reason.
    Other,
}

impl Default for FinishReason {
    fn default() -> Self {
        Self::EndTurn
    }
}

impl FinishReason {
    /// Parse from Anthropic's stop_reason.
    pub fn from_anthropic(reason: &str) -> Self {
        match reason {
            "end_turn" => Self::EndTurn,
            "stop_sequence" => Self::Stop,
            "max_tokens" => Self::MaxTokens,
            "tool_use" => Self::ToolUse,
            _ => Self::Other,
        }
    }

    /// Parse from OpenAI's finish_reason.
    pub fn from_openai(reason: &str) -> Self {
        match reason {
            "stop" => Self::EndTurn,
            "length" => Self::MaxTokens,
            "tool_calls" | "function_call" => Self::ToolUse,
            "content_filter" => Self::ContentFilter,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_merge() {
        let mut usage1 = Usage::new(100, 50);
        let usage2 = Usage::new(200, 100);

        usage1.merge(&usage2);

        assert_eq!(usage1.input_tokens, 300);
        assert_eq!(usage1.output_tokens, 150);
        assert_eq!(usage1.total(), 450);
    }

    #[test]
    fn test_finish_reason_parsing() {
        assert_eq!(
            FinishReason::from_anthropic("end_turn"),
            FinishReason::EndTurn
        );
        assert_eq!(
            FinishReason::from_anthropic("tool_use"),
            FinishReason::ToolUse
        );

        assert_eq!(FinishReason::from_openai("stop"), FinishReason::EndTurn);
        assert_eq!(
            FinishReason::from_openai("tool_calls"),
            FinishReason::ToolUse
        );
    }

    #[test]
    fn test_chunk_classification() {
        assert!(StreamChunk::TextStart.is_text());
        assert!(StreamChunk::text("hello").is_text());
        assert!(!StreamChunk::text("hello").is_reasoning());

        assert!(StreamChunk::reasoning("thinking").is_reasoning());
        assert!(!StreamChunk::reasoning("thinking").is_text());

        assert!(StreamChunk::tool_call_start("id", "name").is_tool());
    }
}
