//! Azure OpenAI provider implementation.
//!
//! Supports Azure-hosted OpenAI models including GPT-4, GPT-4o, and o1 series.
//! Uses Azure's deployment-based API with Azure AD or API key authentication.

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
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, trace, warn};

/// Default Azure OpenAI API version.
const DEFAULT_API_VERSION: &str = "2024-10-21";

/// Azure OpenAI provider configuration.
#[derive(Debug, Clone)]
pub struct AzureConfig {
    /// Azure OpenAI resource name.
    pub resource_name: String,
    /// Azure OpenAI deployment name.
    pub deployment_name: String,
    /// API key for authentication.
    pub api_key: Option<String>,
    /// Azure AD token for authentication (alternative to API key).
    pub azure_ad_token: Option<String>,
    /// API version to use.
    pub api_version: String,
    /// Model information.
    pub model: ModelInfo,
    /// Use chat completions URL format instead of responses.
    pub use_completions_url: bool,
}

impl Default for AzureConfig {
    fn default() -> Self {
        Self {
            resource_name: String::new(),
            deployment_name: String::new(),
            api_key: None,
            azure_ad_token: None,
            api_version: DEFAULT_API_VERSION.to_string(),
            model: ModelInfo::default(),
            use_completions_url: false,
        }
    }
}

/// Azure OpenAI provider.
pub struct AzureProvider {
    client: reqwest::Client,
    resource_name: String,
    deployment_name: String,
    api_version: String,
    model: ModelInfo,
    use_completions_url: bool,
}

impl AzureProvider {
    /// Create a new Azure OpenAI provider.
    pub fn new(config: AzureConfig) -> ProviderResult<Self> {
        // Get resource name from config or environment
        let resource_name = if !config.resource_name.is_empty() {
            config.resource_name
        } else {
            std::env::var("AZURE_OPENAI_RESOURCE_NAME")
                .or_else(|_| std::env::var("AZURE_RESOURCE_NAME"))
                .map_err(|_| ProviderError::missing_api_key("azure (AZURE_OPENAI_RESOURCE_NAME)"))?
        };

        // Get deployment name
        let deployment_name = if !config.deployment_name.is_empty() {
            config.deployment_name
        } else {
            std::env::var("AZURE_OPENAI_DEPLOYMENT_NAME")
                .unwrap_or_else(|_| config.model.id.clone())
        };

        // Get authentication
        let api_key = config
            .api_key
            .or_else(|| std::env::var("AZURE_OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("AZURE_API_KEY").ok());

        let azure_ad_token = config
            .azure_ad_token
            .or_else(|| std::env::var("AZURE_AD_TOKEN").ok());

        if api_key.is_none() && azure_ad_token.is_none() {
            return Err(ProviderError::missing_api_key(
                "azure (AZURE_OPENAI_API_KEY or AZURE_AD_TOKEN)",
            ));
        }

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(ref key) = api_key {
            headers.insert(
                "api-key",
                HeaderValue::from_str(key).map_err(|_| ProviderError::invalid_api_key("azure"))?,
            );
        } else if let Some(ref token) = azure_ad_token {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .map_err(|_| ProviderError::invalid_api_key("azure"))?,
            );
        }

        debug!(
            resource = %resource_name,
            deployment = %deployment_name,
            model = %config.model.id,
            "Creating Azure OpenAI provider"
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self {
            client,
            resource_name,
            deployment_name,
            api_version: config.api_version,
            model: config.model,
            use_completions_url: config.use_completions_url,
        })
    }

    /// Get the Azure OpenAI endpoint URL.
    fn endpoint(&self) -> String {
        format!(
            "https://{}.openai.azure.com/openai/deployments/{}",
            self.resource_name, self.deployment_name
        )
    }

    /// Check if this is a reasoning model (o1/o3 series).
    fn is_reasoning_model(&self) -> bool {
        let id = self.model.id.to_lowercase();
        id.starts_with("o1") || id.starts_with("o3") || id.contains("-o1") || id.contains("-o3")
    }

    /// Convert messages to OpenAI format.
    fn convert_messages(&self, messages: &[Message], system: Option<&str>) -> Vec<Value> {
        let mut result = Vec::new();
        let is_reasoning = self.is_reasoning_model();

        // Add system message if provided
        if let Some(sys) = system {
            let role = if is_reasoning { "developer" } else { "system" };
            result.push(json!({
                "role": role,
                "content": sys
            }));
        }

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::System => {
                    if is_reasoning {
                        "developer"
                    } else {
                        "system"
                    }
                }
            };

            let content = Self::convert_content(&msg.content);

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
                    if let Some(tool_calls) = Self::convert_tool_calls(&msg.content) {
                        message["tool_calls"] = tool_calls;
                    }
                }

                result.push(message);
            }
        }

        result
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

/// Azure OpenAI streaming chunk.
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

#[derive(Debug, Clone, Deserialize)]
struct ChunkUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl LanguageModel for AzureProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let is_reasoning = self.is_reasoning_model();

        let mut request = json!({
            "messages": self.convert_messages(&messages, options.system.as_deref()),
            "stream": true,
            "stream_options": { "include_usage": true }
        });

        // Add parameters (skip for reasoning models)
        if !is_reasoning {
            if let Some(temp) = options.temperature {
                request["temperature"] = json!(temp);
            }
            if let Some(top_p) = options.top_p {
                request["top_p"] = json!(top_p);
            }
        }

        if let Some(max_tokens) = options.max_tokens {
            request["max_tokens"] = json!(max_tokens);
        }

        // Add tools
        let tools = Self::convert_tools(&options.tools);
        if !tools.is_empty() {
            request["tools"] = json!(tools);
        }

        // Build URL
        let endpoint_suffix = if self.use_completions_url {
            "chat/completions"
        } else {
            "responses"
        };
        let url = format!(
            "{}/{}?api-version={}",
            self.endpoint(),
            endpoint_suffix,
            self.api_version
        );

        debug!(
            deployment = %self.deployment_name,
            model = %self.model.id,
            "Sending Azure OpenAI request"
        );
        trace!(request = %request, "Full request");

        let response = self.client.post(&url).json(&request).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %error_text, "Azure OpenAI API error");
            return Err(ProviderError::Internal {
                message: format!("Azure OpenAI API error {}: {}", status, error_text),
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

            let mut tool_calls: Vec<(String, String, String)> = Vec::new();
            let mut text_started = false;
            let mut last_usage: Option<ChunkUsage> = None;

            while let Some(line) = lines.next_line().await? {
                // Check for cancellation
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        Err(ProviderError::Cancelled)?;
                    }
                }

                let line = line.trim();

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

                if chunk.usage.is_some() {
                    last_usage = chunk.usage.clone();
                }

                for choice in &chunk.choices {
                    if let Some(content) = &choice.delta.content {
                        if !content.is_empty() {
                            if !text_started {
                                yield StreamChunk::TextStart;
                                text_started = true;
                            }
                            yield StreamChunk::TextDelta(content.clone());
                        }
                    }

                    if let Some(tool_deltas) = &choice.delta.tool_calls {
                        for delta in tool_deltas {
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

                    if let Some(reason) = &choice.finish_reason {
                        if text_started {
                            yield StreamChunk::TextEnd;
                        }

                        for (id, name, args) in &tool_calls {
                            if !id.is_empty() && !name.is_empty() {
                                yield StreamChunk::ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: args.clone(),
                                };
                            }
                        }

                        let finish_reason = match reason.as_str() {
                            "stop" => FinishReason::EndTurn,
                            "tool_calls" => FinishReason::ToolUse,
                            "length" => FinishReason::MaxTokens,
                            "content_filter" => FinishReason::ContentFilter,
                            _ => FinishReason::EndTurn,
                        };

                        let usage = last_usage.as_ref().map(|u| {
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
        "azure"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_reasoning_model() {
        let config = AzureConfig {
            resource_name: "test".to_string(),
            deployment_name: "o1-preview".to_string(),
            api_key: Some("test".to_string()),
            model: ModelInfo {
                id: "o1-preview".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let provider = AzureProvider::new(config).unwrap();
        assert!(provider.is_reasoning_model());
    }

    #[test]
    fn test_endpoint() {
        let config = AzureConfig {
            resource_name: "myresource".to_string(),
            deployment_name: "gpt-4".to_string(),
            api_key: Some("test".to_string()),
            model: ModelInfo::default(),
            ..Default::default()
        };

        let provider = AzureProvider::new(config).unwrap();
        assert!(provider.endpoint().contains("myresource.openai.azure.com"));
        assert!(provider.endpoint().contains("gpt-4"));
    }
}
