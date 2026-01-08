//! OpenRouter provider implementation.
//!
//! OpenRouter provides access to many models through a unified API.
//! It uses an OpenAI-compatible format with custom headers.

use crate::{
    error::ProviderError,
    message::{ContentPart, Message, Role},
    model::ModelInfo,
    stream::{FinishReason, StreamChunk, Usage},
    GenerateOptions, LanguageModel, ProviderResult, ToolDefinition,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, trace, warn};

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1";

/// OpenRouter provider.
pub struct OpenRouterProvider {
    client: reqwest::Client,
    model: ModelInfo,
}

impl OpenRouterProvider {
    /// Create a new OpenRouter provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|_| ProviderError::invalid_api_key("openrouter"))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Required OpenRouter headers
        headers.insert(
            "HTTP-Referer",
            HeaderValue::from_static("https://wonopcode.com/"),
        );
        headers.insert("X-Title", HeaderValue::from_static("wonopcode"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self { client, model })
    }

    /// Convert our messages to OpenAI format.
    fn convert_messages(messages: &[Message], system: Option<&str>) -> Vec<Value> {
        let mut result = Vec::new();

        // Add system message if provided
        if let Some(sys) = system {
            result.push(json!({
                "role": "system",
                "content": sys
            }));
        }

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::System => "system",
            };

            // Build content
            let content = convert_content(&msg.content);

            // Handle tool results specially
            if msg.role == Role::Tool {
                for part in &msg.content {
                    if let ContentPart::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } = part
                    {
                        result.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content
                        }));
                    }
                }
            } else if !content.is_null() {
                let mut message = json!({
                    "role": role,
                    "content": content
                });

                // Add tool calls for assistant messages
                if msg.role == Role::Assistant {
                    if let Some(tool_calls) = convert_tool_calls(&msg.content) {
                        message["tool_calls"] = tool_calls;
                    }
                }

                result.push(message);
            }
        }

        result
    }

    /// Convert tools to OpenAI format.
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters
                    }
                })
            })
            .collect()
    }
}

/// Convert content parts to OpenAI format.
fn convert_content(parts: &[ContentPart]) -> Value {
    let content_parts: Vec<Value> = parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(json!({
                "type": "text",
                "text": text
            })),
            ContentPart::Image { source } => {
                let url = match source {
                    crate::message::ImageSource::Base64 { media_type, data } => {
                        format!("data:{};base64,{}", media_type, data)
                    }
                    crate::message::ImageSource::Url { url } => url.clone(),
                };
                Some(json!({
                    "type": "image_url",
                    "image_url": { "url": url }
                }))
            }
            _ => None,
        })
        .collect();

    if content_parts.len() == 1 {
        // If just text, return as string
        if let Some(text) = content_parts[0].get("text") {
            return text.clone();
        }
    }

    if content_parts.is_empty() {
        Value::Null
    } else {
        Value::Array(content_parts)
    }
}

/// Convert tool calls to OpenAI format.
fn convert_tool_calls(parts: &[ContentPart]) -> Option<Value> {
    let calls: Vec<Value> = parts
        .iter()
        .filter_map(|part| {
            if let ContentPart::ToolUse { id, name, input } = part {
                Some(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(input).unwrap_or_default()
                    }
                }))
            } else {
                None
            }
        })
        .collect();

    if calls.is_empty() {
        None
    } else {
        Some(Value::Array(calls))
    }
}

/// OpenAI chat completion request.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// OpenAI streaming chunk.
#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<ChunkUsage>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChunkUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl LanguageModel for OpenRouterProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let request = ChatRequest {
            model: self.model.id.clone(),
            messages: Self::convert_messages(&messages, options.system.as_deref()),
            max_tokens: options.max_tokens,
            temperature: options.temperature,
            top_p: options.top_p,
            tools: Self::convert_tools(&options.tools),
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        debug!(model = %self.model.id, "Sending OpenRouter request");
        trace!(request = ?request, "Full request");

        let response = self
            .client
            .post(format!("{}/chat/completions", OPENROUTER_API_URL))
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %error_text, "OpenRouter API error");
            return Err(ProviderError::Internal {
                message: format!("OpenRouter API error {}: {}", status, error_text),
            });
        }

        let byte_stream = response.bytes_stream();
        let abort = options.abort.clone();

        Ok(Box::pin(try_stream! {
            use futures::StreamExt;
            use tokio::io::AsyncBufReadExt;
            use tokio_util::io::StreamReader;

            let reader = StreamReader::new(
                byte_stream.map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
            );
            let mut lines = reader.lines();

            // Track tool calls being built
            let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
            let mut text_started = false;

            while let Some(line) = lines.next_line().await? {
                // Check for cancellation
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        Err(ProviderError::Cancelled)?;
                    }
                }

                let line = line.trim();

                // Skip empty lines and SSE prefix
                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }

                let data = line.strip_prefix("data: ").unwrap_or(line);
                if data.is_empty() {
                    continue;
                }

                let chunk: ChatChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(e) => {
                        trace!(error = %e, data = %data, "Failed to parse chunk");
                        continue;
                    }
                };

                for choice in &chunk.choices {
                    // Handle text content
                    if let Some(content) = &choice.delta.content {
                        if !content.is_empty() {
                            if !text_started {
                                yield StreamChunk::TextStart;
                                text_started = true;
                            }
                            yield StreamChunk::TextDelta(content.clone());
                        }
                    }

                    // Handle tool calls
                    if let Some(tool_deltas) = &choice.delta.tool_calls {
                        for delta in tool_deltas {
                            // Ensure we have a slot for this tool call
                            while tool_calls.len() <= delta.index {
                                tool_calls.push((String::new(), String::new(), String::new()));
                            }

                            let call = &mut tool_calls[delta.index];

                            if let Some(id) = &delta.id {
                                call.0 = id.clone();
                            }

                            if let Some(func) = &delta.function {
                                if let Some(name) = &func.name {
                                    call.1 = name.clone();
                                    // Emit tool call start
                                    yield StreamChunk::ToolCallStart {
                                        id: call.0.clone(),
                                        name: name.clone(),
                                    };
                                }
                                if let Some(args) = &func.arguments {
                                    call.2.push_str(args);
                                }
                            }
                        }
                    }

                    // Handle finish reason
                    if let Some(reason) = &choice.finish_reason {
                        // End text if it was started
                        if text_started {
                            yield StreamChunk::TextEnd;
                        }

                        // Emit completed tool calls
                        for (id, name, args) in &tool_calls {
                            if !id.is_empty() && !name.is_empty() {
                                yield StreamChunk::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: args.clone(),
                                };
                            }
                        }

                        // Parse finish reason
                        let finish_reason = match reason.as_str() {
                            "stop" => FinishReason::EndTurn,
                            "tool_calls" => FinishReason::ToolUse,
                            "length" => FinishReason::MaxTokens,
                            "content_filter" => FinishReason::ContentFilter,
                            _ => FinishReason::EndTurn,
                        };

                        // Emit finish with usage if available
                        let usage = chunk.usage.as_ref().map(|u| {
                            Usage::new(u.prompt_tokens, u.completion_tokens)
                        }).unwrap_or_else(|| Usage::new(0, 0));

                        yield StreamChunk::FinishStep {
                            usage,
                            finish_reason,
                        };
                    }
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "openrouter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_messages() {
        let messages = vec![Message::user("Hello, world!")];

        let converted = OpenRouterProvider::convert_messages(&messages, Some("You are helpful"));

        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0]["role"], "system");
        assert_eq!(converted[1]["role"], "user");
        assert_eq!(converted[1]["content"], "Hello, world!");
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }];

        let converted = OpenRouterProvider::convert_tools(&tools);

        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0]["type"], "function");
        assert_eq!(converted[0]["function"]["name"], "read");
    }
}
