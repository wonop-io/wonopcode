//! Stream processing utilities for the standard loop.

use std::collections::HashMap;

use wonopcode_provider::stream::{FinishReason, StreamChunk, Usage};

/// Accumulated result from processing a provider stream.
#[derive(Debug, Default)]
pub struct StreamResult {
    /// Accumulated text response.
    pub text: String,
    /// Tool calls extracted from stream (id, name, args).
    pub tool_calls: Vec<(String, String, String)>,
    /// Token usage statistics.
    pub usage: Usage,
    /// How the stream finished.
    pub finish_reason: FinishReason,
}

/// Pending tool call during streaming.
#[derive(Debug, Default)]
pub struct PendingToolCall {
    /// Tool call ID.
    #[allow(dead_code)]
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Arguments being accumulated.
    pub arguments: String,
}

/// Processes streaming responses from providers.
///
/// This is a stateful processor that accumulates stream chunks
/// into a final result.
#[derive(Debug, Default)]
pub struct StreamProcessor {
    /// Accumulated text.
    text: String,
    /// Pending tool calls (by ID).
    pending_calls: HashMap<String, PendingToolCall>,
    /// Completed tool calls.
    tool_calls: Vec<(String, String, String)>,
    /// Usage statistics.
    usage: Usage,
    /// Finish reason.
    finish_reason: FinishReason,
}

impl StreamProcessor {
    /// Create a new stream processor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a stream chunk.
    ///
    /// Returns the chunk type for the caller to handle UI updates.
    pub fn process(&mut self, chunk: StreamChunk) -> ProcessedChunk {
        match chunk {
            StreamChunk::TextStart => ProcessedChunk::None,
            StreamChunk::TextDelta(delta) => {
                self.text.push_str(&delta);
                ProcessedChunk::TextDelta(delta)
            }
            StreamChunk::TextEnd => ProcessedChunk::None,
            StreamChunk::ToolCallStart { id, name } => {
                self.pending_calls.insert(
                    id.clone(),
                    PendingToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                    },
                );
                ProcessedChunk::ToolStart { id, name }
            }
            StreamChunk::ToolCallDelta { id, delta } => {
                if let Some(pending) = self.pending_calls.get_mut(&id) {
                    pending.arguments.push_str(&delta);
                }
                ProcessedChunk::None
            }
            StreamChunk::ToolCall {
                id,
                name,
                arguments,
            } => {
                // Complete tool call - either finalize pending or add new
                self.pending_calls.remove(&id);
                self.tool_calls
                    .push((id.clone(), name.clone(), arguments.clone()));
                ProcessedChunk::ToolComplete {
                    id,
                    name,
                    arguments,
                }
            }
            StreamChunk::ReasoningStart => ProcessedChunk::None,
            StreamChunk::ReasoningDelta(delta) => ProcessedChunk::ThinkingDelta(delta),
            StreamChunk::ReasoningEnd => ProcessedChunk::None,
            StreamChunk::ToolObserved { id, name, input } => {
                ProcessedChunk::ToolObserved { id, name, input }
            }
            StreamChunk::ToolResultObserved {
                id,
                success,
                output,
            } => ProcessedChunk::ToolResultObserved {
                id,
                success,
                output,
            },
            StreamChunk::FinishStep {
                usage,
                accumulated_usage: _,  // Handled by caller
                finish_reason,
            } => {
                self.usage.merge(&usage);
                self.finish_reason = finish_reason;
                ProcessedChunk::Finished {
                    usage,
                    finish_reason,
                }
            }
            StreamChunk::Error(e) => ProcessedChunk::Error(e),
        }
    }

    /// Finalize processing and return the result.
    ///
    /// Any pending tool calls that weren't completed via ToolCall chunks
    /// are finalized here.
    pub fn finalize(mut self) -> StreamResult {
        // Finalize any remaining pending tool calls
        for (id, pending) in self.pending_calls {
            self.tool_calls.push((id, pending.name, pending.arguments));
        }

        StreamResult {
            text: self.text,
            tool_calls: self.tool_calls,
            usage: self.usage,
            finish_reason: self.finish_reason,
        }
    }

    /// Get the current accumulated text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Get the current tool calls.
    pub fn tool_calls(&self) -> &[(String, String, String)] {
        &self.tool_calls
    }
}

/// Result of processing a single chunk.
#[derive(Debug)]
pub enum ProcessedChunk {
    /// No action needed.
    None,
    /// Text delta to display.
    TextDelta(String),
    /// Thinking/reasoning delta.
    ThinkingDelta(String),
    /// Tool execution started.
    ToolStart { id: String, name: String },
    /// Tool execution completed.
    ToolComplete {
        id: String,
        name: String,
        arguments: String,
    },
    /// Tool observed (external execution).
    ToolObserved {
        id: String,
        name: String,
        input: String,
    },
    /// Tool result observed.
    ToolResultObserved {
        id: String,
        success: bool,
        output: String,
    },
    /// Stream finished.
    Finished {
        usage: Usage,
        finish_reason: FinishReason,
    },
    /// Error occurred.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_processor_text() {
        let mut processor = StreamProcessor::new();

        processor.process(StreamChunk::TextStart);
        let result = processor.process(StreamChunk::TextDelta("Hello ".into()));
        assert!(matches!(result, ProcessedChunk::TextDelta(ref s) if s == "Hello "));

        processor.process(StreamChunk::TextDelta("World".into()));
        processor.process(StreamChunk::TextEnd);

        let result = processor.finalize();
        assert_eq!(result.text, "Hello World");
        assert!(result.tool_calls.is_empty());
    }

    #[test]
    fn test_stream_processor_tool_call() {
        let mut processor = StreamProcessor::new();

        processor.process(StreamChunk::ToolCallStart {
            id: "call1".into(),
            name: "read".into(),
        });
        processor.process(StreamChunk::ToolCallDelta {
            id: "call1".into(),
            delta: r#"{"file":"#.into(),
        });
        processor.process(StreamChunk::ToolCallDelta {
            id: "call1".into(),
            delta: r#""test.txt"}"#.into(),
        });
        processor.process(StreamChunk::ToolCall {
            id: "call1".into(),
            name: "read".into(),
            arguments: r#"{"file":"test.txt"}"#.into(),
        });

        let result = processor.finalize();
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].0, "call1");
        assert_eq!(result.tool_calls[0].1, "read");
    }

    #[test]
    fn test_stream_processor_finalize_pending() {
        let mut processor = StreamProcessor::new();

        // Start a tool call but don't complete it with ToolCall chunk
        processor.process(StreamChunk::ToolCallStart {
            id: "call1".into(),
            name: "read".into(),
        });
        processor.process(StreamChunk::ToolCallDelta {
            id: "call1".into(),
            delta: r#"{"file":"test.txt"}"#.into(),
        });

        // Finalize should complete the pending call
        let result = processor.finalize();
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].2, r#"{"file":"test.txt"}"#);
    }
}
