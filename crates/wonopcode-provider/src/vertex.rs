//! Google Vertex AI provider implementation.
//!
//! Implements Google Vertex AI with streaming support.
//! Supports both Gemini models and Anthropic models on Vertex.

use crate::{
    error::ProviderError,
    message::{ContentPart, ImageSource, Message, Role},
    model::{ModalitySupport, ModelCapabilities, ModelCost, ModelInfo, ModelLimit, ModelStatus},
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

/// Google Vertex AI provider.
pub struct VertexProvider {
    client: reqwest::Client,
    /// Access token stored for potential token refresh (currently unused as we use ADC).
    _access_token: String,
    project: String,
    location: String,
    model: ModelInfo,
}

impl VertexProvider {
    /// Create a new Vertex AI provider.
    ///
    /// # Arguments
    /// * `access_token` - Google Cloud access token (or use GOOGLE_APPLICATION_CREDENTIALS)
    /// * `project` - GCP project ID
    /// * `location` - GCP region (e.g., "us-central1", "us-east5")
    /// * `model` - Model information
    pub fn new(
        access_token: &str,
        project: &str,
        location: &str,
        model: ModelInfo,
    ) -> ProviderResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !access_token.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {access_token}"))
                    .map_err(|e| ProviderError::internal(e.to_string()))?,
            );
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self {
            client,
            _access_token: access_token.to_string(),
            project: project.to_string(),
            location: location.to_string(),
            model,
        })
    }

    /// Create provider from environment.
    /// Uses GOOGLE_APPLICATION_CREDENTIALS for auth.
    pub async fn from_env(project: &str, location: &str, model: ModelInfo) -> ProviderResult<Self> {
        // Try to get access token from gcloud CLI or ADC
        let token = get_access_token().await?;
        Self::new(&token, project, location, model)
    }

    /// Get the API URL for streaming.
    fn stream_url(&self) -> String {
        // Vertex AI uses a different URL structure than the public Gemini API
        format!(
            "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:streamGenerateContent",
            self.location,
            self.project,
            self.location,
            self.model.id
        )
    }

    /// Convert our messages to Vertex AI format.
    fn convert_messages(messages: &[Message]) -> Vec<Value> {
        let mut result = Vec::new();

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "model",
                Role::Tool => "function",
                Role::System => continue,
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

    /// Convert tools to Vertex AI format.
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

/// Convert content parts to Vertex AI format.
fn convert_parts(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(json!({ "text": text })),
            ContentPart::Image { source } => match source {
                ImageSource::Base64 { media_type, data } => Some(json!({
                    "inline_data": {
                        "mime_type": media_type,
                        "data": data
                    }
                })),
                ImageSource::Url { url } => Some(json!({
                    "file_data": {
                        "file_uri": url
                    }
                })),
            },
            ContentPart::ToolUse { id: _, name, input } => Some(json!({
                "functionCall": {
                    "name": name,
                    "args": input
                }
            })),
            ContentPart::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some(json!({
                "functionResponse": {
                    "name": tool_use_id,
                    "response": {
                        "content": content,
                        "is_error": is_error.unwrap_or(false)
                    }
                }
            })),
            ContentPart::Thinking { .. } => None,
        })
        .collect()
}

#[async_trait]
impl LanguageModel for VertexProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let contents = Self::convert_messages(&messages);

        // Build generation config
        let mut generation_config = json!({});
        if let Some(temp) = options.temperature {
            generation_config["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = options.max_tokens {
            generation_config["maxOutputTokens"] = json!(max_tokens);
        }
        if let Some(top_p) = options.top_p {
            generation_config["topP"] = json!(top_p);
        }

        // Build request body
        let mut body = json!({
            "contents": contents,
            "generationConfig": generation_config,
        });

        // Add system instruction
        if let Some(ref system) = options.system {
            body["systemInstruction"] = json!({
                "parts": [{ "text": system }]
            });
        }

        // Add tools
        if !options.tools.is_empty() {
            body["tools"] = Self::convert_tools(&options.tools);
        }

        debug!(url = %self.stream_url(), "Sending request to Vertex AI");
        trace!(body = %serde_json::to_string_pretty(&body).unwrap_or_default(), "Request body");

        let response = self
            .client
            .post(self.stream_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ProviderError::api_error(
                    e.status().map(|s| s.as_u16()).unwrap_or(500),
                    e.to_string(),
                )
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::api_error(status, text));
        }

        let stream = try_stream! {
            let mut reader = response.bytes_stream();
            use futures::StreamExt;

            let mut buffer = String::new();
            let mut tool_call_counter: u64 = 0;

            while let Some(chunk_result) = reader.next().await {
                let chunk = chunk_result.map_err(|e| ProviderError::internal(e.to_string()))?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);

                // Process SSE lines
                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }

                    // Strip "data: " prefix
                    let Some(json_str) = line.strip_prefix("data: ") else {
                        continue;
                    };

                    // Parse JSON
                    let data: VertexResponse = match serde_json::from_str(json_str) {
                        Ok(d) => d,
                        Err(e) => {
                            warn!(error = %e, json = %json_str, "Failed to parse Vertex response");
                            continue;
                        }
                    };

                    // Process candidates
                    if let Some(candidates) = data.candidates {
                        for candidate in candidates {
                            if let Some(content) = candidate.content {
                                for part in content.parts.unwrap_or_default() {
                                    // Text content
                                    if let Some(text) = part.text {
                                        yield StreamChunk::TextDelta(text);
                                    }

                                    // Function call
                                    if let Some(fc) = part.function_call {
                                        tool_call_counter += 1;
                                        let id = format!("call_{tool_call_counter}");
                                        let args = serde_json::to_string(&fc.args).unwrap_or_default();

                                        yield StreamChunk::ToolCall {
                                            id,
                                            name: fc.name,
                                            arguments: args,
                                        };
                                    }
                                }
                            }

                            // Check finish reason
                            if let Some(reason) = candidate.finish_reason {
                                let finish_reason = match reason.as_str() {
                                    "STOP" => FinishReason::EndTurn,
                                    "MAX_TOKENS" => FinishReason::MaxTokens,
                                    "SAFETY" => FinishReason::ContentFilter,
                                    _ => FinishReason::Other,
                                };

                                // Get usage if available
                                let usage = if let Some(meta) = &data.usage_metadata {
                                    Usage {
                                        input_tokens: meta.prompt_token_count.unwrap_or(0),
                                        output_tokens: meta.candidates_token_count.unwrap_or(0),
                                        ..Default::default()
                                    }
                                } else {
                                    Usage::default()
                                };

                                yield StreamChunk::FinishStep { usage, finish_reason };
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
        "vertex"
    }
}

// Response types
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VertexResponse {
    candidates: Option<Vec<VertexCandidate>>,
    usage_metadata: Option<VertexUsageMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VertexCandidate {
    content: Option<VertexContent>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VertexContent {
    parts: Option<Vec<VertexPart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VertexPart {
    text: Option<String>,
    function_call: Option<VertexFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct VertexFunctionCall {
    name: String,
    args: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VertexUsageMetadata {
    prompt_token_count: Option<u32>,
    candidates_token_count: Option<u32>,
}

/// Get access token from gcloud CLI or Application Default Credentials.
async fn get_access_token() -> ProviderResult<String> {
    // Try to get token from gcloud CLI
    let output = tokio::process::Command::new("gcloud")
        .args(["auth", "print-access-token"])
        .output()
        .await
        .map_err(|e| ProviderError::internal(format!("Failed to run gcloud: {e}")))?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    Err(ProviderError::internal(
        "Could not get access token. Please run 'gcloud auth login' or set GOOGLE_APPLICATION_CREDENTIALS".to_string()
    ))
}

/// Pre-defined Vertex AI models.
pub mod models {
    use super::*;

    /// Gemini 2.0 Flash on Vertex AI
    pub fn gemini_2_flash() -> ModelInfo {
        ModelInfo {
            id: "gemini-2.0-flash-exp".to_string(),
            provider_id: "vertex".to_string(),
            name: "Gemini 2.0 Flash".to_string(),
            family: Some("gemini-2".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                temperature: true,
                reasoning: false,
                attachment: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: true,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    image: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.075, // per 1M tokens
                output: 0.30,
                cache_read: 0.01875,
                cache_write: 0.01875,
            },
            limit: ModelLimit {
                context: 1_048_576,
                output: 8192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Gemini 1.5 Pro on Vertex AI
    pub fn gemini_1_5_pro() -> ModelInfo {
        ModelInfo {
            id: "gemini-1.5-pro".to_string(),
            provider_id: "vertex".to_string(),
            name: "Gemini 1.5 Pro".to_string(),
            family: Some("gemini-1.5".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                temperature: true,
                reasoning: false,
                attachment: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: true,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 1.25,
                output: 5.0,
                cache_read: 0.3125,
                cache_write: 0.3125,
            },
            limit: ModelLimit {
                context: 2_097_152,
                output: 8192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Gemini 1.5 Flash on Vertex AI
    pub fn gemini_1_5_flash() -> ModelInfo {
        ModelInfo {
            id: "gemini-1.5-flash".to_string(),
            provider_id: "vertex".to_string(),
            name: "Gemini 1.5 Flash".to_string(),
            family: Some("gemini-1.5".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                temperature: true,
                reasoning: false,
                attachment: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: true,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.075,
                output: 0.30,
                cache_read: 0.01875,
                cache_write: 0.01875,
            },
            limit: ModelLimit {
                context: 1_048_576,
                output: 8192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude 3.5 Sonnet on Vertex AI (Anthropic partnership)
    pub fn claude_3_5_sonnet() -> ModelInfo {
        ModelInfo {
            id: "claude-3-5-sonnet@20241022".to_string(),
            provider_id: "vertex-anthropic".to_string(),
            name: "Claude 3.5 Sonnet (Vertex)".to_string(),
            family: Some("claude-3.5".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                temperature: true,
                reasoning: false,
                attachment: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    pdf: true,
                    ..Default::default()
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: 0.30,
                cache_write: 3.75,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 8192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude 3.5 Haiku on Vertex AI (Anthropic partnership)
    pub fn claude_3_5_haiku() -> ModelInfo {
        ModelInfo {
            id: "claude-3-5-haiku@20241022".to_string(),
            provider_id: "vertex-anthropic".to_string(),
            name: "Claude 3.5 Haiku (Vertex)".to_string(),
            family: Some("claude-3.5".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                temperature: true,
                reasoning: false,
                attachment: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    pdf: true,
                    ..Default::default()
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 1.0,
                output: 5.0,
                cache_read: 0.10,
                cache_write: 1.25,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 8192,
            },
            status: ModelStatus::Active,
        }
    }
}
