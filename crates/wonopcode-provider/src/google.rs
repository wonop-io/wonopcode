//! Google Gemini provider implementation.
//!
//! Implements the Google Generative AI API with streaming support.

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
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, trace, warn};

/// Google Gemini provider.
pub struct GoogleProvider {
    client: reqwest::Client,
    api_key: String,
    model: ModelInfo,
}

impl GoogleProvider {
    /// Create a new Google provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self {
            client,
            api_key: api_key.to_string(),
            model,
        })
    }

    /// Get the API URL for streaming.
    fn stream_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model.id,
            self.api_key
        )
    }

    /// Convert our messages to Gemini format.
    fn convert_messages(messages: &[Message]) -> Vec<Value> {
        let mut result = Vec::new();

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "model",
                Role::Tool => "function", // Gemini uses "function" for tool results
                Role::System => continue, // System is handled separately
            };

            let parts = convert_parts(&msg.content);

            if !parts.is_empty() {
                result.push(json!({
                    "role": role,
                    "parts": parts
                }));
            }
        }

        result
    }

    /// Convert tools to Gemini format.
    fn convert_tools(tools: &[ToolDefinition]) -> Value {
        if tools.is_empty() {
            return Value::Null;
        }

        let function_declarations: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                })
            })
            .collect();

        json!([{
            "functionDeclarations": function_declarations
        }])
    }
}

/// Convert content parts to Gemini format.
fn convert_parts(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|part| match part {
            ContentPart::Text { text } => json!({
                "text": text
            }),
            ContentPart::ToolUse { id: _, name, input } => json!({
                "functionCall": {
                    "name": name,
                    "args": input
                }
            }),
            ContentPart::ToolResult {
                tool_use_id,
                content,
                ..
            } => json!({
                "functionResponse": {
                    "name": tool_use_id, // Gemini uses the function name here
                    "response": {
                        "content": content
                    }
                }
            }),
            ContentPart::Image { source } => match source {
                crate::message::ImageSource::Base64 { media_type, data } => json!({
                    "inlineData": {
                        "mimeType": media_type,
                        "data": data
                    }
                }),
                crate::message::ImageSource::Url { url } => json!({
                    "fileData": {
                        "fileUri": url
                    }
                }),
            },
            ContentPart::Thinking { text } => json!({
                "text": format!("[Thinking: {}]", text)
            }),
        })
        .collect()
}

#[async_trait]
impl LanguageModel for GoogleProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let url = self.stream_url();

        // Extract system instruction from options
        let system_instruction = options.system.as_ref().map(|s| {
            json!({
                "parts": [{"text": s}]
            })
        });

        // Build generation config
        let mut generation_config = json!({});

        if let Some(temp) = options.temperature {
            generation_config["temperature"] = json!(temp);
        }
        if let Some(top_p) = options.top_p {
            generation_config["topP"] = json!(top_p);
        }
        if let Some(max_tokens) = options.max_tokens {
            generation_config["maxOutputTokens"] = json!(max_tokens);
        }

        // Build request body
        let mut body = json!({
            "contents": Self::convert_messages(&messages),
            "generationConfig": generation_config
        });

        if let Some(sys) = system_instruction {
            body["systemInstruction"] = sys;
        }

        let tools = Self::convert_tools(&options.tools);
        if !tools.is_null() {
            body["tools"] = tools;
        }

        debug!(
            "Gemini request: {}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );

        let response = self.client.post(&url).json(&body).send().await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            warn!("Gemini error response: {} - {}", status, text);
            return Err(ProviderError::api_error(status.as_u16(), text));
        }

        let abort = options.abort.clone();
        let byte_stream = response.bytes_stream();

        Ok(Box::pin(try_stream! {
            use futures::StreamExt;
            use tokio::io::AsyncBufReadExt;
            use tokio_util::io::StreamReader;

            let reader = StreamReader::new(
                byte_stream.map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
            );
            let mut lines = reader.lines();

            let mut text_started = false;
            let mut tool_calls: Vec<(String, String, Value)> = Vec::new();

            while let Some(line) = lines.next_line().await? {
                // Check for abort
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        break;
                    }
                }

                let line = line.trim();

                // Skip empty lines
                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }

                let data = line.strip_prefix("data: ").unwrap_or(line);
                if data.is_empty() {
                    continue;
                }

                match serde_json::from_str::<GeminiStreamResponse>(data) {
                    Ok(response) => {
                        for candidate in response.candidates.unwrap_or_default() {
                            if let Some(content) = candidate.content {
                                for part in content.parts.unwrap_or_default() {
                                    if let Some(text) = part.text {
                                        if !text_started {
                                            yield StreamChunk::TextStart;
                                            text_started = true;
                                        }
                                        yield StreamChunk::TextDelta(text);
                                    }
                                    if let Some(fc) = part.function_call {
                                        // Generate a unique ID for the tool call
                                        let id = format!("call_{}", tool_calls.len());
                                        yield StreamChunk::ToolCallStart {
                                            id: id.clone(),
                                            name: fc.name.clone(),
                                        };
                                        let args = fc.args.unwrap_or(json!({}));
                                        tool_calls.push((id.clone(), fc.name.clone(), args.clone()));
                                        yield StreamChunk::ToolCall {
                                            id,
                                            name: fc.name,
                                            arguments: args.to_string(),
                                        };
                                    }
                                }
                            }

                            // Handle finish reason
                            if let Some(finish_reason) = candidate.finish_reason {
                                if text_started {
                                    yield StreamChunk::TextEnd;
                                    text_started = false;
                                }

                                let reason = match finish_reason.as_str() {
                                    "STOP" => FinishReason::Stop,
                                    "MAX_TOKENS" => FinishReason::MaxTokens,
                                    "SAFETY" => FinishReason::ContentFilter,
                                    _ => FinishReason::Stop,
                                };

                                // Extract usage if available
                                let usage = response.usage_metadata.as_ref().map(|u| Usage::new(
                                    u.prompt_token_count.unwrap_or(0),
                                    u.candidates_token_count.unwrap_or(0),
                                )).unwrap_or_default();

                                yield StreamChunk::FinishStep {
                                    usage,
                                    finish_reason: reason,
                                };
                            }
                        }
                    }
                    Err(e) => {
                        trace!("Failed to parse Gemini chunk: {} - data: {}", e, data);
                    }
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "google"
    }
}

/// Gemini streaming response structure.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiStreamResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiContent {
    /// Role field from API response (we already know the role from context).
    #[serde(default)]
    _role: Option<String>,
    parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    text: Option<String>,
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionCall {
    name: String,
    args: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: Option<u32>,
    candidates_token_count: Option<u32>,
    /// Total is sum of prompt + candidates, we compute separately.
    #[serde(default)]
    _total_token_count: Option<u32>,
}
