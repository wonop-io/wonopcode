//! Mock provider for testing.

use crate::{
    error::ProviderError, message::Message, model::ModelInfo, stream::StreamChunk, GenerateOptions,
    LanguageModel, ProviderResult,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::sync::{Arc, Mutex};

/// A mock response for testing.
#[derive(Debug, Clone)]
pub enum MockResponse {
    /// Return a text response.
    Text(String),
    /// Return a tool call.
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// Return an error.
    Error(String),
    /// Return a sequence of responses.
    Sequence(Vec<MockResponse>),
}

/// Mock provider for testing.
pub struct MockProvider {
    model: ModelInfo,
    responses: Arc<Mutex<Vec<MockResponse>>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockProvider {
    /// Create a new mock provider.
    pub fn new(model: ModelInfo) -> Self {
        Self {
            model,
            responses: Arc::new(Mutex::new(Vec::new())),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a mock provider that returns a fixed text response.
    pub fn with_text_response(model: ModelInfo, text: impl Into<String>) -> Self {
        let provider = Self::new(model);
        provider.expect_text(text);
        provider
    }

    /// Expect a text response.
    pub fn expect_text(&self, text: impl Into<String>) {
        let mut responses = self.responses.lock().unwrap();
        responses.push(MockResponse::Text(text.into()));
    }

    /// Expect a tool call response.
    pub fn expect_tool_call(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) {
        let mut responses = self.responses.lock().unwrap();
        responses.push(MockResponse::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        });
    }

    /// Expect an error response.
    pub fn expect_error(&self, error: impl Into<String>) {
        let mut responses = self.responses.lock().unwrap();
        responses.push(MockResponse::Error(error.into()));
    }

    /// Get the number of times generate was called.
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait]
impl LanguageModel for MockProvider {
    async fn generate(
        &self,
        _messages: Vec<Message>,
        _options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        // Increment call count
        {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
        }

        // Get next response
        let response = {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                MockResponse::Text("Mock response".to_string())
            } else {
                responses.remove(0)
            }
        };

        Ok(Box::pin(try_stream! {
            match response {
                MockResponse::Text(text) => {
                    yield StreamChunk::TextStart;
                    yield StreamChunk::TextDelta(text);
                    yield StreamChunk::TextEnd;
                    yield StreamChunk::FinishStep {
                        usage: crate::stream::Usage::new(100, 50),
                        finish_reason: crate::stream::FinishReason::EndTurn,
                    };
                }
                MockResponse::ToolCall { id, name, arguments } => {
                    yield StreamChunk::ToolCallStart { id: id.clone(), name: name.clone() };
                    yield StreamChunk::ToolCall { id, name, arguments };
                    yield StreamChunk::FinishStep {
                        usage: crate::stream::Usage::new(100, 50),
                        finish_reason: crate::stream::FinishReason::ToolUse,
                    };
                }
                MockResponse::Error(msg) => {
                    Err(ProviderError::internal(msg))?;
                }
                MockResponse::Sequence(items) => {
                    for item in items {
                        match item {
                            MockResponse::Text(text) => {
                                yield StreamChunk::TextDelta(text);
                            }
                            MockResponse::ToolCall { id, name, arguments } => {
                                yield StreamChunk::ToolCall { id, name, arguments };
                            }
                            _ => {}
                        }
                    }
                    yield StreamChunk::FinishStep {
                        usage: crate::stream::Usage::new(100, 50),
                        finish_reason: crate::stream::FinishReason::EndTurn,
                    };
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_mock_text_response() {
        let model = crate::model::anthropic::claude_sonnet_4();
        let provider = MockProvider::with_text_response(model, "Hello, world!");

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

        assert_eq!(text, "Hello, world!");
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_tool_call() {
        let model = crate::model::anthropic::claude_sonnet_4();
        let provider = MockProvider::new(model);
        provider.expect_tool_call("call_1", "read", r#"{"filePath": "/test.txt"}"#);

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
}
