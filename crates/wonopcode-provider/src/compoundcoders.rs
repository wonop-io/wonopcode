//! Compound Coders AI provider implementation.
//!
//! This provider connects to the Compound Coders API, which offers an OpenAI-compatible
//! chat completions endpoint with custom models:
//! - `wonop/gpt` - Wonop GPT model
//! - `wonop/qwen` - Wonop QWEN model

use crate::{
    error::ProviderError,
    message::{ContentPart, Message, Role},
    model::{ModelCapabilities, ModelCost, ModelInfo, ModelLimit, ModelStatus, ModalitySupport},
    stream::{FinishReason, StreamChunk, Usage},
    GenerateOptions, LanguageModel, ProviderResult, ToolDefinition,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, trace, warn};

/// Default API URL for Compound Coders.
const DEFAULT_API_URL: &str = "https://api.compoundcoders.com";

/// Compound Coders AI provider.
pub struct CompoundCodersProvider {
    client: reqwest::Client,
    base_url: String,
    model: ModelInfo,
}

impl CompoundCodersProvider {
    /// Create a new Compound Coders provider with an API key.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        Self::with_base_url(api_key, model, DEFAULT_API_URL)
    }

    /// Create a new Compound Coders provider with a custom base URL.
    pub fn with_base_url(api_key: &str, model: ModelInfo, base_url: &str) -> ProviderResult<Self> {
        if api_key.is_empty() {
            return Err(ProviderError::invalid_api_key("compoundcoders"));
        }

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|_| ProviderError::invalid_api_key("compoundcoders"))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        })
    }

    /// Convert our messages to OpenAI-compatible format.
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

    /// Convert tools to OpenAI-compatible format.
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

/// Convert content parts to OpenAI-compatible format.
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
                        format!("data:{media_type};base64,{data}")
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

/// Convert tool calls to OpenAI-compatible format.
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

/// OpenAI-compatible streaming response chunk.
#[derive(Debug, Deserialize)]
struct StreamingChunk {
    choices: Vec<StreamingChoice>,
    usage: Option<StreamingUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamingChoice {
    delta: StreamingDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamingDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamingToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamingToolCall {
    index: Option<usize>,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<StreamingFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamingFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamingUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl LanguageModel for CompoundCodersProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        
        debug!(
            url = %url,
            model = %self.model.id,
            "Sending request to Compound Coders"
        );

        // Build request body
        let mut body = json!({
            "model": self.model.id,
            "messages": Self::convert_messages(&messages, options.system.as_deref()),
            "stream": true
        });

        // Add optional parameters
        if let Some(temp) = options.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = options.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(top_p) = options.top_p {
            body["top_p"] = json!(top_p);
        }

        // Add tools if provided (don't send tool_choice - let API use default)
        if !options.tools.is_empty() {
            body["tools"] = json!(Self::convert_tools(&options.tools));
        }

        trace!(body = %serde_json::to_string_pretty(&body).unwrap_or_default(), "Request body");

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %error_text, "Compound Coders API error");
            return Err(ProviderError::api_error(
                status.as_u16(),
                format!("Compound Coders API error: {}", error_text),
            ));
        }

        let abort_token = options.abort.clone();

        let stream = try_stream! {
            let mut reader = response.bytes_stream();
            let mut buffer = String::new();
            let mut tool_calls: std::collections::HashMap<usize, (String, String, String)> = std::collections::HashMap::new();

            while let Some(result) = {
                use futures::StreamExt;
                reader.next().await
            } {
                // Check for cancellation
                if let Some(ref token) = abort_token {
                    if token.is_cancelled() {
                        yield StreamChunk::FinishStep {
                            finish_reason: FinishReason::Other,
                            usage: Usage::default(),
                            accumulated_usage: None,
                        };
                        break;
                    }
                }

                let chunk = result.map_err(|e| ProviderError::internal(e.to_string()))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete lines
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(parsed) = serde_json::from_str::<StreamingChunk>(data) {
                            for choice in &parsed.choices {
                                // Handle text content
                                if let Some(ref content) = choice.delta.content {
                                    if !content.is_empty() {
                                        yield StreamChunk::TextDelta(content.clone());
                                    }
                                }

                                // Handle tool calls
                                if let Some(ref calls) = choice.delta.tool_calls {
                                    for call in calls {
                                        let index = call.index.unwrap_or(0);

                                        // Initialize or update tool call
                                        let entry = tool_calls
                                            .entry(index)
                                            .or_insert_with(|| (String::new(), String::new(), String::new()));

                                        if let Some(ref id) = call.id {
                                            entry.0 = id.clone();
                                        }
                                        if let Some(ref func) = call.function {
                                            if let Some(ref name) = func.name {
                                                entry.1 = name.clone();
                                            }
                                            if let Some(ref args) = func.arguments {
                                                entry.2.push_str(args);
                                            }
                                        }
                                    }
                                }

                                // Handle finish reason
                                if let Some(ref reason) = choice.finish_reason {
                                    // Emit any accumulated tool calls
                                    for (_, (id, name, args)) in tool_calls.drain() {
                                        if !id.is_empty() && !name.is_empty() {
                                            yield StreamChunk::ToolCall {
                                                id,
                                                name,
                                                arguments: args,
                                            };
                                        }
                                    }

                                    let finish_reason = match reason.as_str() {
                                        "stop" => FinishReason::EndTurn,
                                        "tool_calls" => FinishReason::ToolUse,
                                        "length" => FinishReason::MaxTokens,
                                        "content_filter" => FinishReason::ContentFilter,
                                        _ => FinishReason::Other,
                                    };

                                    // Include usage if available
                                    let usage = parsed.usage.clone().map(|u| Usage {
                                        input_tokens: u.prompt_tokens,
                                        output_tokens: u.completion_tokens,
                                        cache_read_tokens: 0,
                                        cache_write_tokens: 0,
                                        reasoning_tokens: 0,
                                    }).unwrap_or_default();

                                    yield StreamChunk::FinishStep {
                                        finish_reason,
                                        usage,
                                        accumulated_usage: None,
                                    };
                                }
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "compoundcoders"
    }
}

/// Built-in model definitions for Compound Coders.
pub mod models {
    use super::*;
    use crate::model::TokenizerInfo;

    /// Wonop GPT - General purpose GPT model.
    pub fn wonop_gpt() -> ModelInfo {
        ModelInfo {
            id: "wonop/gpt".to_string(),
            provider_id: "compoundcoders".to_string(),
            name: "Wonop GPT".to_string(),
            family: Some("wonop".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: false,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: false,
                    audio: false,
                    video: false,
                    pdf: false,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.0,  // Free tier assumed
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
            tokenizer: TokenizerInfo::default(),
        }
    }

    /// Wonop QWEN - QWEN-based model.
    pub fn wonop_qwen() -> ModelInfo {
        ModelInfo {
            id: "wonop/qwen".to_string(),
            provider_id: "compoundcoders".to_string(),
            name: "Wonop QWEN".to_string(),
            family: Some("wonop".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: false,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: false,
                    audio: false,
                    video: false,
                    pdf: false,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
            tokenizer: TokenizerInfo::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_info() {
        let gpt = models::wonop_gpt();
        assert_eq!(gpt.id, "wonop/gpt");
        assert_eq!(gpt.provider_id, "compoundcoders");

        let qwen = models::wonop_qwen();
        assert_eq!(qwen.id, "wonop/qwen");
        assert_eq!(qwen.provider_id, "compoundcoders");
    }

    #[test]
    fn test_convert_messages() {
        let messages = vec![Message {
            role: Role::User,
            content: vec![ContentPart::Text {
                text: "Hello".to_string(),
            }],
        }];

        let converted = CompoundCodersProvider::convert_messages(&messages, Some("You are helpful"));
        assert_eq!(converted.len(), 2); // System + User
        assert_eq!(converted[0]["role"], "system");
        assert_eq!(converted[1]["role"], "user");
    }
}
