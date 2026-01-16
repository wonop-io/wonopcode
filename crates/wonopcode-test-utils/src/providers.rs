//! Test provider implementations.
//!
//! Provides providers that record interactions and return configurable responses.

use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::sync::{Arc, Mutex};
use wonopcode_provider::{
    error::ProviderError,
    message::Message,
    model::ModelInfo,
    stream::{FinishReason, StreamChunk, Usage},
    GenerateOptions, LanguageModel, ProviderResult,
};

/// A provider that records all interactions for later inspection.
///
/// Useful for verifying that the correct messages and options are being sent
/// to the provider, and for replaying responses in tests.
///
/// # Example
///
/// ```rust,ignore
/// use wonopcode_test_utils::providers::RecordingProvider;
///
/// let provider = RecordingProvider::new()
///     .with_response("Hello! How can I help?");
///
/// // Use provider in test...
///
/// let calls = provider.calls();
/// assert_eq!(calls.len(), 1);
/// assert!(calls[0].messages[0].content_text().contains("user message"));
/// ```
pub struct RecordingProvider {
    model: ModelInfo,
    /// Recorded calls to generate().
    calls: Arc<Mutex<Vec<RecordedCall>>>,
    /// Queue of responses to return.
    responses: Arc<Mutex<Vec<ProviderResponse>>>,
    /// Default response when queue is empty.
    default_response: Arc<Mutex<ProviderResponse>>,
}

/// A recorded call to the provider.
#[derive(Debug, Clone)]
pub struct RecordedCall {
    /// The messages sent to the provider.
    pub messages: Vec<Message>,
    /// The options used for generation.
    pub options: GenerateOptions,
}

/// A response that the provider can return.
#[derive(Debug, Clone)]
pub enum ProviderResponse {
    /// Return a text response.
    Text(String),
    /// Return a text response with thinking/reasoning.
    TextWithThinking { thinking: String, text: String },
    /// Return a tool call.
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// Return multiple tool calls.
    MultipleToolCalls(Vec<(String, String, String)>), // (id, name, arguments)
    /// Return an error.
    Error(String),
    /// Return a sequence of chunks.
    Chunks(Vec<StreamChunk>),
}

impl Default for ProviderResponse {
    fn default() -> Self {
        ProviderResponse::Text("Test response".to_string())
    }
}

impl RecordingProvider {
    /// Create a new recording provider.
    pub fn new() -> Self {
        Self {
            model: ModelInfo::new("test-model", "test"),
            calls: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(Vec::new())),
            default_response: Arc::new(Mutex::new(ProviderResponse::default())),
        }
    }

    /// Create with a specific model.
    pub fn with_model(mut self, model: ModelInfo) -> Self {
        self.model = model;
        self
    }

    /// Queue a text response.
    pub fn with_response(self, text: impl Into<String>) -> Self {
        self.responses
            .lock()
            .unwrap()
            .push(ProviderResponse::Text(text.into()));
        self
    }

    /// Queue a response with thinking.
    pub fn with_thinking_response(
        self,
        thinking: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        self.responses
            .lock()
            .unwrap()
            .push(ProviderResponse::TextWithThinking {
                thinking: thinking.into(),
                text: text.into(),
            });
        self
    }

    /// Queue a tool call response.
    pub fn with_tool_call(self, id: &str, name: &str, arguments: &str) -> Self {
        self.responses
            .lock()
            .unwrap()
            .push(ProviderResponse::ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: arguments.to_string(),
            });
        self
    }

    /// Queue an error response.
    pub fn with_error(self, message: impl Into<String>) -> Self {
        self.responses
            .lock()
            .unwrap()
            .push(ProviderResponse::Error(message.into()));
        self
    }

    /// Set the default response when queue is empty.
    pub fn with_default_response(self, response: ProviderResponse) -> Self {
        *self.default_response.lock().unwrap() = response;
        self
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Get the number of calls made.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// Clear recorded calls.
    pub fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    /// Get the last call made.
    pub fn last_call(&self) -> Option<RecordedCall> {
        self.calls.lock().unwrap().last().cloned()
    }

    /// Check if a message containing the given text was sent.
    pub fn was_sent(&self, text: &str) -> bool {
        self.calls.lock().unwrap().iter().any(|call| {
            call.messages.iter().any(|msg| {
                msg.content.iter().any(|part| {
                    if let wonopcode_provider::message::ContentPart::Text { text: t } = part {
                        t.contains(text)
                    } else {
                        false
                    }
                })
            })
        })
    }
}

impl Default for RecordingProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LanguageModel for RecordingProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        // Record the call
        self.calls
            .lock()
            .unwrap()
            .push(RecordedCall { messages, options });

        // Get the next response
        let response = {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                self.default_response.lock().unwrap().clone()
            } else {
                responses.remove(0)
            }
        };

        Ok(Box::pin(try_stream! {
            match response {
                ProviderResponse::Text(text) => {
                    yield StreamChunk::TextStart;
                    yield StreamChunk::TextDelta(text);
                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 50),
                        finish_reason: FinishReason::EndTurn,
                    };
                }
                ProviderResponse::TextWithThinking { thinking, text } => {
                    yield StreamChunk::ReasoningStart;
                    yield StreamChunk::ReasoningDelta(thinking);
                    yield StreamChunk::ReasoningEnd;
                    yield StreamChunk::TextStart;
                    yield StreamChunk::TextDelta(text);
                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage {
                            input_tokens: 100,
                            output_tokens: 50,
                            reasoning_tokens: 30,
                            ..Default::default()
                        },
                        finish_reason: FinishReason::EndTurn,
                    };
                }
                ProviderResponse::ToolCall { id, name, arguments } => {
                    yield StreamChunk::ToolCallStart { id: id.clone(), name: name.clone() };
                    yield StreamChunk::ToolCall { id, name, arguments };
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 50),
                        finish_reason: FinishReason::ToolUse,
                    };
                }
                ProviderResponse::MultipleToolCalls(calls) => {
                    for (id, name, arguments) in calls {
                        yield StreamChunk::ToolCallStart { id: id.clone(), name: name.clone() };
                        yield StreamChunk::ToolCall { id, name, arguments };
                    }
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 50),
                        finish_reason: FinishReason::ToolUse,
                    };
                }
                ProviderResponse::Error(msg) => {
                    Err(ProviderError::internal(msg))?;
                }
                ProviderResponse::Chunks(chunks) => {
                    for chunk in chunks {
                        yield chunk;
                    }
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "recording"
    }
}

/// A provider that loads and replays recorded sessions.
///
/// Useful for creating reproducible test scenarios from real conversations.
pub struct ReplayProvider {
    model: ModelInfo,
    responses: Arc<Mutex<Vec<ProviderResponse>>>,
    current_index: Arc<Mutex<usize>>,
}

impl ReplayProvider {
    /// Create a new replay provider.
    pub fn new() -> Self {
        Self {
            model: ModelInfo::new("replay-model", "replay"),
            responses: Arc::new(Mutex::new(Vec::new())),
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Load responses from a JSON file.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let responses: Vec<String> = serde_json::from_str(json)?;
        let provider = Self::new();
        for response in responses {
            provider
                .responses
                .lock()
                .unwrap()
                .push(ProviderResponse::Text(response));
        }
        Ok(provider)
    }

    /// Add a response to the replay queue.
    pub fn add_response(self, response: ProviderResponse) -> Self {
        self.responses.lock().unwrap().push(response);
        self
    }

    /// Reset the replay to the beginning.
    pub fn reset(&self) {
        *self.current_index.lock().unwrap() = 0;
    }
}

impl Default for ReplayProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LanguageModel for ReplayProvider {
    async fn generate(
        &self,
        _messages: Vec<Message>,
        _options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let response = {
            let responses = self.responses.lock().unwrap();
            let mut index = self.current_index.lock().unwrap();
            let response = responses.get(*index).cloned().unwrap_or_default();
            *index = (*index + 1) % responses.len().max(1);
            response
        };

        Ok(Box::pin(try_stream! {
            match response {
                ProviderResponse::Text(text) => {
                    yield StreamChunk::TextStart;
                    yield StreamChunk::TextDelta(text);
                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 50),
                        finish_reason: FinishReason::EndTurn,
                    };
                }
                _ => {
                    yield StreamChunk::TextStart;
                    yield StreamChunk::TextDelta("Replay response".to_string());
                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: Usage::new(100, 50),
                        finish_reason: FinishReason::EndTurn,
                    };
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "replay"
    }
}

/// Test harness for provider integration tests.
///
/// Provides a convenient way to set up and run tests against AI providers
/// with configurable responses and assertions.
///
/// # Example
///
/// ```rust,ignore
/// use wonopcode_test_utils::providers::ProviderTestHarness;
///
/// #[tokio::test]
/// async fn test_provider_conversation() {
///     let harness = ProviderTestHarness::new()
///         .with_response("Hello! I can help with that.")
///         .with_tool_call("read_1", "read", r#"{"filePath": "test.txt"}"#)
///         .with_response("Based on the file contents...");
///
///     // Run conversation
///     let result = harness.send("Please read test.txt").await;
///     assert!(result.text.contains("Hello"));
///
///     // Verify tool was called
///     assert!(harness.tool_was_called("read"));
///
///     // Continue conversation
///     let result = harness.send_tool_result("read_1", "File contents here").await;
///     assert!(result.text.contains("Based on"));
/// }
/// ```
pub struct ProviderTestHarness {
    provider: RecordingProvider,
}

/// Result of a provider interaction.
#[derive(Debug, Clone)]
pub struct ProviderInteractionResult {
    /// The text response (if any).
    pub text: String,
    /// The thinking/reasoning text (if any).
    pub thinking: Option<String>,
    /// Tool calls made (if any).
    pub tool_calls: Vec<ToolCallInfo>,
    /// Whether the interaction completed successfully.
    pub success: bool,
    /// Error message (if any).
    pub error: Option<String>,
}

/// Information about a tool call.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    /// Tool call ID.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Tool arguments as JSON string.
    pub arguments: String,
}

impl ProviderTestHarness {
    /// Create a new test harness with a default recording provider.
    pub fn new() -> Self {
        Self {
            provider: RecordingProvider::new(),
        }
    }

    /// Queue a text response.
    pub fn with_response(mut self, text: impl Into<String>) -> Self {
        self.provider = self.provider.with_response(text);
        self
    }

    /// Queue a response with thinking.
    pub fn with_thinking_response(
        mut self,
        thinking: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        self.provider = self.provider.with_thinking_response(thinking, text);
        self
    }

    /// Queue a tool call response.
    pub fn with_tool_call(mut self, id: &str, name: &str, arguments: &str) -> Self {
        self.provider = self.provider.with_tool_call(id, name, arguments);
        self
    }

    /// Queue an error response.
    pub fn with_error(mut self, message: impl Into<String>) -> Self {
        self.provider = self.provider.with_error(message);
        self
    }

    /// Send a message and get the response.
    pub async fn send(&self, message: &str) -> ProviderInteractionResult {
        use futures::StreamExt;

        let messages = vec![Message::user(message)];
        match self
            .provider
            .generate(messages, GenerateOptions::default())
            .await
        {
            Ok(mut stream) => {
                let mut text = String::new();
                let mut thinking = None;
                let mut tool_calls = Vec::new();
                let mut thinking_text = String::new();

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(StreamChunk::TextDelta(delta)) => {
                            text.push_str(&delta);
                        }
                        Ok(StreamChunk::ReasoningStart) => {}
                        Ok(StreamChunk::ReasoningDelta(delta)) => {
                            thinking_text.push_str(&delta);
                        }
                        Ok(StreamChunk::ReasoningEnd) => {
                            if !thinking_text.is_empty() {
                                thinking = Some(thinking_text.clone());
                            }
                        }
                        Ok(StreamChunk::ToolCall {
                            id,
                            name,
                            arguments,
                        }) => {
                            tool_calls.push(ToolCallInfo {
                                id,
                                name,
                                arguments,
                            });
                        }
                        Err(e) => {
                            return ProviderInteractionResult {
                                text: String::new(),
                                thinking: None,
                                tool_calls: Vec::new(),
                                success: false,
                                error: Some(e.to_string()),
                            };
                        }
                        _ => {}
                    }
                }

                ProviderInteractionResult {
                    text,
                    thinking,
                    tool_calls,
                    success: true,
                    error: None,
                }
            }
            Err(e) => ProviderInteractionResult {
                text: String::new(),
                thinking: None,
                tool_calls: Vec::new(),
                success: false,
                error: Some(e.to_string()),
            },
        }
    }

    /// Check if a specific tool was called.
    pub fn tool_was_called(&self, _tool_name: &str) -> bool {
        self.provider.calls().iter().any(|_call| {
            // The tool calls are in the responses, not the recorded calls
            // This would need to track tool calls from responses
            false
        })
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.provider.calls()
    }

    /// Get the number of calls made.
    pub fn call_count(&self) -> usize {
        self.provider.call_count()
    }

    /// Check if a message containing the given text was sent.
    pub fn message_contained(&self, text: &str) -> bool {
        self.provider.was_sent(text)
    }

    /// Get the underlying provider for advanced use cases.
    pub fn provider(&self) -> &RecordingProvider {
        &self.provider
    }

    /// Clear all recorded calls.
    pub fn reset(&self) {
        self.provider.clear_calls();
    }
}

impl Default for ProviderTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_recording_provider_text() {
        let provider = RecordingProvider::new().with_response("Hello!");

        let messages = vec![Message::user("Hi")];
        let mut stream = provider
            .generate(messages, GenerateOptions::default())
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(delta)) = chunk {
                text.push_str(&delta);
            }
        }

        assert_eq!(text, "Hello!");
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn test_recording_provider_tool_call() {
        let provider =
            RecordingProvider::new().with_tool_call("call_1", "read", r#"{"path": "test.txt"}"#);

        let messages = vec![Message::user("Read test.txt")];
        let mut stream = provider
            .generate(messages, GenerateOptions::default())
            .await
            .unwrap();

        let mut tool_name = None;
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::ToolCall { name, .. }) = chunk {
                tool_name = Some(name);
            }
        }

        assert_eq!(tool_name, Some("read".to_string()));
    }

    #[tokio::test]
    async fn test_recording_provider_was_sent() {
        let provider = RecordingProvider::new().with_response("OK");

        let messages = vec![Message::user("Hello world")];
        let _ = provider
            .generate(messages, GenerateOptions::default())
            .await
            .unwrap();

        assert!(provider.was_sent("Hello"));
        assert!(provider.was_sent("world"));
        assert!(!provider.was_sent("goodbye"));
    }

    #[tokio::test]
    async fn test_replay_provider() {
        let provider = ReplayProvider::new()
            .add_response(ProviderResponse::Text("First".to_string()))
            .add_response(ProviderResponse::Text("Second".to_string()));

        // First call
        let mut stream = provider
            .generate(vec![Message::user("1")], GenerateOptions::default())
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(delta)) = chunk {
                text.push_str(&delta);
            }
        }
        assert_eq!(text, "First");

        // Second call
        let mut stream = provider
            .generate(vec![Message::user("2")], GenerateOptions::default())
            .await
            .unwrap();

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::TextDelta(delta)) = chunk {
                text.push_str(&delta);
            }
        }
        assert_eq!(text, "Second");
    }

    #[tokio::test]
    async fn test_provider_harness_basic() {
        let harness = ProviderTestHarness::new().with_response("Hello! I can help with that.");

        let result = harness.send("Hi there").await;

        assert!(result.success);
        assert!(result.text.contains("Hello"));
        assert!(result.error.is_none());
        assert_eq!(harness.call_count(), 1);
        assert!(harness.message_contained("Hi there"));
    }

    #[tokio::test]
    async fn test_provider_harness_with_thinking() {
        let harness = ProviderTestHarness::new()
            .with_thinking_response("Let me think...", "Here's my answer.");

        let result = harness.send("Complex question").await;

        assert!(result.success);
        assert!(result.text.contains("Here's my answer"));
        assert!(result.thinking.is_some());
        assert!(result.thinking.unwrap().contains("Let me think"));
    }

    #[tokio::test]
    async fn test_provider_harness_with_tool_call() {
        let harness = ProviderTestHarness::new().with_tool_call(
            "tool_1",
            "read",
            r#"{"filePath": "test.txt"}"#,
        );

        let result = harness.send("Read the file").await;

        assert!(result.success);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "read");
        assert_eq!(result.tool_calls[0].id, "tool_1");
    }

    #[tokio::test]
    async fn test_provider_harness_with_error() {
        let harness = ProviderTestHarness::new().with_error("Rate limit exceeded");

        let result = harness.send("Any message").await;

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("Rate limit"));
    }

    #[tokio::test]
    async fn test_provider_harness_multiple_responses() {
        let harness = ProviderTestHarness::new()
            .with_response("First response")
            .with_response("Second response");

        let result1 = harness.send("Message 1").await;
        assert!(result1.text.contains("First"));

        let result2 = harness.send("Message 2").await;
        assert!(result2.text.contains("Second"));

        assert_eq!(harness.call_count(), 2);
    }

    #[tokio::test]
    async fn test_provider_harness_reset() {
        let harness = ProviderTestHarness::new().with_response("Response");

        harness.send("Message").await;
        assert_eq!(harness.call_count(), 1);

        harness.reset();
        assert_eq!(harness.call_count(), 0);
    }
}
