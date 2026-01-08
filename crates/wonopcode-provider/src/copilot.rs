//! GitHub Copilot provider implementation.
//!
//! Supports GitHub Copilot chat models via GitHub's API.
//! Requires a GitHub Copilot subscription and authentication token.

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
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, trace, warn};

/// GitHub Copilot API base URL.
const COPILOT_API_URL: &str = "https://api.githubcopilot.com";

/// GitHub Copilot provider configuration.
#[derive(Debug, Clone, Default)]
pub struct CopilotConfig {
    /// GitHub Copilot token.
    pub token: Option<String>,
    /// Model information.
    pub model: ModelInfo,
    /// Whether this is an enterprise deployment.
    pub enterprise: bool,
    /// Enterprise URL (if applicable).
    pub enterprise_url: Option<String>,
}

/// GitHub Copilot provider.
pub struct CopilotProvider {
    client: reqwest::Client,
    base_url: String,
    model: ModelInfo,
}

impl CopilotProvider {
    /// Create a new GitHub Copilot provider.
    pub fn new(config: CopilotConfig) -> ProviderResult<Self> {
        // Get token from config or environment
        let token = config
            .token
            .or_else(|| std::env::var("GITHUB_COPILOT_TOKEN").ok())
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .ok_or_else(|| {
                ProviderError::missing_api_key("github-copilot (GITHUB_COPILOT_TOKEN)")
            })?;

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|_| ProviderError::invalid_api_key("github-copilot"))?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("wonopcode/1.0"));
        headers.insert(
            "Copilot-Integration-Id",
            HeaderValue::from_static("wonopcode"),
        );
        headers.insert("Editor-Version", HeaderValue::from_static("wonopcode/1.0"));

        let base_url = if config.enterprise {
            config.enterprise_url.unwrap_or_else(|| {
                std::env::var("GITHUB_COPILOT_ENTERPRISE_URL")
                    .unwrap_or_else(|_| COPILOT_API_URL.to_string())
            })
        } else {
            COPILOT_API_URL.to_string()
        };

        debug!(
            model = %config.model.id,
            enterprise = config.enterprise,
            "Creating GitHub Copilot provider"
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            model: config.model,
        })
    }

    /// Check if this model uses the responses API (for Codex models).
    fn use_responses_api(&self) -> bool {
        self.model.id.contains("codex")
    }

    /// Convert messages to OpenAI format.
    fn convert_messages(&self, messages: &[Message], system: Option<&str>) -> Vec<Value> {
        let mut result = Vec::new();

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

            let content = Self::convert_content(&msg.content);

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

    /// Convert content parts to format.
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

    /// Convert tool calls to format.
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

    /// Convert tools to format.
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

/// Chat streaming chunk.
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
impl LanguageModel for CopilotProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let mut request = json!({
            "model": self.model.id,
            "messages": self.convert_messages(&messages, options.system.as_deref()),
            "stream": true
        });

        if let Some(temp) = options.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(top_p) = options.top_p {
            request["top_p"] = json!(top_p);
        }
        if let Some(max_tokens) = options.max_tokens {
            request["max_tokens"] = json!(max_tokens);
        }

        let tools = Self::convert_tools(&options.tools);
        if !tools.is_empty() {
            request["tools"] = json!(tools);
        }

        let endpoint = if self.use_responses_api() {
            format!("{}/responses", self.base_url)
        } else {
            format!("{}/chat/completions", self.base_url)
        };

        debug!(model = %self.model.id, "Sending GitHub Copilot request");
        trace!(request = %request, "Full request");

        let response = self.client.post(&endpoint).json(&request).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %error_text, "GitHub Copilot API error");
            return Err(ProviderError::Internal {
                message: format!("GitHub Copilot API error {}: {}", status, error_text),
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
        "github-copilot"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_responses_api() {
        let config = CopilotConfig {
            token: Some("test".to_string()),
            model: ModelInfo {
                id: "codex-4".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let provider = CopilotProvider::new(config).unwrap();
        assert!(provider.use_responses_api());
    }

    #[test]
    fn test_chat_model() {
        let config = CopilotConfig {
            token: Some("test".to_string()),
            model: ModelInfo {
                id: "gpt-4".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let provider = CopilotProvider::new(config).unwrap();
        assert!(!provider.use_responses_api());
    }
}
