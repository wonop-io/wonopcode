//! Anthropic (Claude) provider implementation.
// @ace:implements COMP-T90R60-JC0

use crate::{
    error::ProviderError, message::Message, model::ModelInfo, stream::StreamChunk, GenerateOptions,
    LanguageModel, ProviderResult, ToolDefinition,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, warn};

/// The Anthropic API base URL.
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com";

/// The Anthropic API version.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Beta features enabled for Claude.
const ANTHROPIC_BETA: &str =
    "claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14";

/// Anthropic (Claude) provider.
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    model: ModelInfo,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider with API key.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        Self::with_base_url(api_key, ANTHROPIC_API_URL, model)
    }

    /// Create a new Anthropic provider with a custom base URL.
    pub fn with_base_url(api_key: &str, base_url: &str, model: ModelInfo) -> ProviderResult<Self> {
        let mut headers = HeaderMap::new();

        headers.insert(
            "x-api-key",
            HeaderValue::from_str(api_key)
                .map_err(|_| ProviderError::invalid_api_key("anthropic"))?,
        );
        headers.insert("anthropic-beta", HeaderValue::from_static(ANTHROPIC_BETA));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        debug!(
            model = %model.id,
            "Creating Anthropic provider"
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

    /// Sanitize messages to ensure valid tool_use/tool_result pairs.
    /// 
    /// The Anthropic API requires:
    /// 1. Every `tool_use` in an assistant message must have a corresponding `tool_result` in the next user message
    /// 2. Every `tool_result` must have a corresponding `tool_use` in the previous assistant message
    ///
    /// This function removes orphaned tool_use/tool_result blocks to prevent API errors.
    fn sanitize_messages(&self, messages: &[Message]) -> Vec<Message> {
        use crate::message::{ContentPart, Role};
        use std::collections::HashSet;
        
        let mut sanitized = Vec::with_capacity(messages.len());
        
        for (i, msg) in messages.iter().enumerate() {
            match msg.role {
                Role::Assistant => {
                    // Check if this assistant message has tool_use blocks
                    let tool_use_ids: HashSet<String> = msg.content.iter()
                        .filter_map(|part| {
                            if let ContentPart::ToolUse { id, .. } = part {
                                Some(id.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    
                    if tool_use_ids.is_empty() {
                        // No tool_use, keep as-is
                        sanitized.push(msg.clone());
                    } else {
                        // Check if next message has all the tool_results
                        let next_msg = messages.get(i + 1);
                        let next_tool_result_ids: HashSet<String> = next_msg
                            .map(|m| {
                                m.content.iter()
                                    .filter_map(|part| {
                                        if let ContentPart::ToolResult { tool_use_id, .. } = part {
                                            Some(tool_use_id.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        
                        // Find tool_use IDs that don't have matching tool_results
                        let orphaned_ids: HashSet<&String> = tool_use_ids
                            .iter()
                            .filter(|id| !next_tool_result_ids.contains(*id))
                            .collect();
                        
                        if orphaned_ids.is_empty() {
                            // All tool_use have matching tool_results
                            sanitized.push(msg.clone());
                        } else {
                            // Remove orphaned tool_use blocks
                            warn!(
                                "ANTHROPIC: Removing {} orphaned tool_use blocks from message {}: {:?}",
                                orphaned_ids.len(), i, orphaned_ids
                            );
                            
                            let filtered_content: Vec<ContentPart> = msg.content.iter()
                                .filter(|part| {
                                    if let ContentPart::ToolUse { id, .. } = part {
                                        !orphaned_ids.contains(id)
                                    } else {
                                        true
                                    }
                                })
                                .cloned()
                                .collect();
                            
                            if filtered_content.is_empty() {
                                // All content was tool_use that got removed
                                // Add a placeholder text to avoid empty message
                                sanitized.push(Message {
                                    role: Role::Assistant,
                                    content: vec![ContentPart::Text {
                                        text: "[Tool calls removed due to missing results]".to_string(),
                                    }],
                                });
                            } else {
                                sanitized.push(Message {
                                    role: msg.role.clone(),
                                    content: filtered_content,
                                });
                            }
                        }
                    }
                }
                Role::User | Role::Tool => {
                    // Check if this message has tool_results
                    let tool_result_ids: HashSet<String> = msg.content.iter()
                        .filter_map(|part| {
                            if let ContentPart::ToolResult { tool_use_id, .. } = part {
                                Some(tool_use_id.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    
                    if tool_result_ids.is_empty() {
                        // No tool_results, keep as-is
                        sanitized.push(msg.clone());
                    } else {
                        // Check if previous message has all the tool_use
                        let prev_tool_use_ids: HashSet<String> = sanitized.last()
                            .filter(|m| m.role == Role::Assistant)
                            .map(|m| {
                                m.content.iter()
                                    .filter_map(|part| {
                                        if let ContentPart::ToolUse { id, .. } = part {
                                            Some(id.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        
                        // Find tool_result IDs that don't have matching tool_use
                        let orphaned_ids: HashSet<&String> = tool_result_ids
                            .iter()
                            .filter(|id| !prev_tool_use_ids.contains(*id))
                            .collect();
                        
                        if orphaned_ids.is_empty() {
                            // All tool_results have matching tool_use
                            sanitized.push(msg.clone());
                        } else {
                            // Remove orphaned tool_result blocks
                            warn!(
                                "ANTHROPIC: Removing {} orphaned tool_result blocks from message {}: {:?}",
                                orphaned_ids.len(), i, orphaned_ids
                            );
                            
                            let filtered_content: Vec<ContentPart> = msg.content.iter()
                                .filter(|part| {
                                    if let ContentPart::ToolResult { tool_use_id, .. } = part {
                                        !orphaned_ids.contains(tool_use_id)
                                    } else {
                                        true
                                    }
                                })
                                .cloned()
                                .collect();
                            
                            if filtered_content.is_empty() {
                                // All content was tool_result that got removed
                                // Add a placeholder text to avoid empty message
                                sanitized.push(Message {
                                    role: msg.role.clone(),
                                    content: vec![ContentPart::Text {
                                        text: "[Tool results removed due to missing tool calls]".to_string(),
                                    }],
                                });
                            } else {
                                sanitized.push(Message {
                                    role: msg.role.clone(),
                                    content: filtered_content,
                                });
                            }
                        }
                    }
                }
                Role::System => {
                    sanitized.push(msg.clone());
                }
            }
        }
        
        sanitized
    }

    /// Convert messages to Anthropic format.
    fn convert_messages(&self, messages: &[Message]) -> (Option<String>, Vec<AnthropicMessage>) {
        // First sanitize messages to fix any broken tool pairs
        let messages = self.sanitize_messages(messages);
        
        let mut system = None;
        let mut converted = Vec::new();

        for msg in &messages {
            match msg.role {
                crate::message::Role::System => {
                    // Collect system messages
                    match system {
                        None => system = Some(msg.text()),
                        Some(ref existing) => {
                            system = Some(format!("{existing}\n\n{}", msg.text()));
                        }
                    }
                }
                crate::message::Role::User => {
                    converted.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: self.convert_content(&msg.content),
                    });
                }
                crate::message::Role::Assistant => {
                    converted.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: self.convert_content(&msg.content),
                    });
                }
                crate::message::Role::Tool => {
                    // Tool results go to user messages in Anthropic format
                    converted.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: self.convert_content(&msg.content),
                    });
                }
            }
        }

        (system, converted)
    }

    /// Convert content parts to Anthropic format.
    fn convert_content(&self, content: &[crate::message::ContentPart]) -> Vec<serde_json::Value> {
        content
            .iter()
            .map(|part| match part {
                crate::message::ContentPart::Text { text } => {
                    json!({ "type": "text", "text": text })
                }
                crate::message::ContentPart::Image { source } => match source {
                    crate::message::ImageSource::Base64 { media_type, data } => {
                        json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": media_type,
                                "data": data
                            }
                        })
                    }
                    crate::message::ImageSource::Url { url } => {
                        json!({
                            "type": "image",
                            "source": {
                                "type": "url",
                                "url": url
                            }
                        })
                    }
                },
                crate::message::ContentPart::ToolUse { id, name, input } => {
                    // Anthropic API requires input to be a JSON object (dictionary)
                    // If input is not an object, convert it to one
                    let valid_input = if input.is_object() {
                        input.clone()
                    } else if input.is_null() {
                        // Null becomes empty object
                        json!({})
                    } else {
                        // For other types (string, array, etc.), wrap in an object
                        // This shouldn't normally happen, but prevents API errors
                        warn!(
                            tool_id = %id,
                            tool_name = %name,
                            input_type = ?input,
                            "Tool use input is not an object, wrapping in empty object"
                        );
                        json!({})
                    };
                    json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": valid_input
                    })
                }
                crate::message::ContentPart::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                        "is_error": is_error.unwrap_or(false)
                    })
                }
                crate::message::ContentPart::Thinking { text } => {
                    json!({ "type": "thinking", "thinking": text })
                }
            })
            .collect()
    }

    /// Convert tool definitions to Anthropic format.
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters
                })
            })
            .collect()
    }

    /// Parse SSE events from the response stream.
    fn parse_stream(
        &self,
        response: reqwest::Response,
        abort: Option<tokio_util::sync::CancellationToken>,
    ) -> BoxStream<'static, ProviderResult<StreamChunk>> {
        Box::pin(try_stream! {
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut current_tool_id: Option<String> = None;
            let mut current_tool_name: Option<String> = None;
            let mut current_tool_args = String::new();

            while let Some(chunk) = stream.next().await {
                // Check for cancellation
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        Err(ProviderError::Cancelled)?;
                    }
                }

                let chunk = chunk.map_err(ProviderError::RequestFailed)?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete SSE events
                while let Some(event) = Self::extract_sse_event(&mut buffer) {
                    // Check for cancellation before processing each event
                    if let Some(ref token) = abort {
                        if token.is_cancelled() {
                            Err(ProviderError::Cancelled)?;
                        }
                    }

                    if let Some(chunk) = Self::parse_sse_event(
                        &event,
                        &mut current_tool_id,
                        &mut current_tool_name,
                        &mut current_tool_args,
                    )? {
                        yield chunk;
                    }
                }
            }
        })
    }

    /// Extract a complete SSE event from the buffer.
    fn extract_sse_event(buffer: &mut String) -> Option<SseEvent> {
        // Look for double newline (event boundary)
        let end = buffer.find("\n\n")?;
        let event_str = buffer[..end].to_string();
        buffer.drain(..end + 2);

        let mut event = SseEvent::default();

        for line in event_str.lines() {
            if let Some(data) = line.strip_prefix("event: ") {
                event.event = data.to_string();
            } else if let Some(data) = line.strip_prefix("data: ") {
                event.data = data.to_string();
            }
        }

        if event.event.is_empty() && event.data.is_empty() {
            None
        } else {
            Some(event)
        }
    }

    /// Parse an SSE event into a StreamChunk.
    fn parse_sse_event(
        event: &SseEvent,
        current_tool_id: &mut Option<String>,
        current_tool_name: &mut Option<String>,
        current_tool_args: &mut String,
    ) -> ProviderResult<Option<StreamChunk>> {
        match event.event.as_str() {
            "message_start" => {
                // Message started, nothing to emit yet
                Ok(None)
            }
            "content_block_start" => {
                let data: ContentBlockStart = serde_json::from_str(&event.data)?;
                match data.content_block.r#type.as_str() {
                    "text" => Ok(Some(StreamChunk::TextStart)),
                    "thinking" => Ok(Some(StreamChunk::ReasoningStart)),
                    "tool_use" => {
                        let id = data.content_block.id.unwrap_or_default();
                        let name = data.content_block.name.unwrap_or_default();
                        *current_tool_id = Some(id.clone());
                        *current_tool_name = Some(name.clone());
                        current_tool_args.clear(); // Reset for new tool call
                        Ok(Some(StreamChunk::ToolCallStart { id, name }))
                    }
                    _ => Ok(None),
                }
            }
            "content_block_delta" => {
                let data: ContentBlockDelta = serde_json::from_str(&event.data)?;
                match data.delta.r#type.as_str() {
                    "text_delta" => {
                        let text = data.delta.text.unwrap_or_default();
                        Ok(Some(StreamChunk::TextDelta(text)))
                    }
                    "thinking_delta" => {
                        let text = data.delta.thinking.unwrap_or_default();
                        Ok(Some(StreamChunk::ReasoningDelta(text)))
                    }
                    "input_json_delta" => {
                        let delta = data.delta.partial_json.unwrap_or_default();
                        // Accumulate the partial JSON
                        current_tool_args.push_str(&delta);
                        if let Some(id) = current_tool_id.clone() {
                            Ok(Some(StreamChunk::ToolCallDelta { id, delta }))
                        } else {
                            Ok(None)
                        }
                    }
                    _ => Ok(None),
                }
            }
            "content_block_stop" => {
                // If we were building a tool call, emit the complete ToolCall
                if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                    let arguments = std::mem::take(current_tool_args);
                    Ok(Some(StreamChunk::ToolCall {
                        id,
                        name,
                        arguments,
                    }))
                } else {
                    Ok(Some(StreamChunk::TextEnd))
                }
            }
            "message_delta" => {
                let data: MessageDelta = serde_json::from_str(&event.data)?;
                let usage = crate::stream::Usage {
                    input_tokens: data.usage.input_tokens.unwrap_or(0),
                    output_tokens: data.usage.output_tokens.unwrap_or(0),
                    ..Default::default()
                };
                let finish_reason = crate::stream::FinishReason::from_anthropic(
                    &data.delta.stop_reason.unwrap_or_default(),
                );
                Ok(Some(StreamChunk::FinishStep {
                    usage,
                    accumulated_usage: None, // API providers don't track accumulated usage
                    finish_reason,
                }))
            }
            "message_stop" => {
                // End of message
                Ok(None)
            }
            "ping" => {
                // Keep-alive, ignore
                Ok(None)
            }
            "error" => {
                let data: ErrorEvent = serde_json::from_str(&event.data)?;
                Err(ProviderError::internal(data.error.message))
            }
            _ => {
                debug!(event = %event.event, "Unknown SSE event");
                Ok(None)
            }
        }
    }
}

#[async_trait]
impl LanguageModel for AnthropicProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let (system, converted_messages) = self.convert_messages(&messages);

        let request = AnthropicRequest {
            model: self.model.id.clone(),
            messages: converted_messages,
            max_tokens: options.max_tokens.unwrap_or(self.model.limit.output),
            system,
            temperature: options.temperature,
            top_p: options.top_p,
            tools: if options.tools.is_empty() {
                None
            } else {
                Some(self.convert_tools(&options.tools))
            },
            stream: true,
        };

        // Log request details for debugging
        tracing::info!(
            model = %self.model.id,
            message_count = request.messages.len(),
            tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0),
            "Sending Anthropic API request"
        );
        debug!(
            model = %self.model.id,
            message_count = request.messages.len(),
            has_tools = request.tools.is_some(),
            tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0),
            "Sending request to Anthropic"
        );

        // Log message roles for debugging
        for (i, msg) in request.messages.iter().enumerate() {
            debug!(
                index = i,
                role = %msg.role,
                content_blocks = msg.content.len(),
                "Request message"
            );
        }

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .json(&request)
            .send()
            .await?;

        tracing::info!(status = %response.status(), "Anthropic API response received");

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "Anthropic request failed");

            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimited { retry_after: None });
            }

            return Err(ProviderError::invalid_response(format!(
                "HTTP {status}: {body}"
            )));
        }

        Ok(self.parse_stream(response, options.abort))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "anthropic"
    }
}

// Request/response types

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<serde_json::Value>,
}

#[derive(Debug, Default)]
struct SseEvent {
    event: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct ContentBlockStart {
    content_block: ContentBlock,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    r#type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContentBlockDelta {
    delta: Delta,
}

#[derive(Debug, Deserialize)]
struct Delta {
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageDelta {
    delta: MessageDeltaContent,
    usage: MessageUsage,
}

#[derive(Debug, Deserialize)]
struct MessageDeltaContent {
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ErrorEvent {
    error: ErrorContent,
}

#[derive(Debug, Deserialize)]
struct ErrorContent {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_messages() {
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        let messages = vec![
            Message::system("You are helpful"),
            Message::user("Hello"),
            Message::assistant("Hi there!"),
        ];

        let (system, converted) = provider.convert_messages(&messages);

        assert_eq!(system, Some("You are helpful".to_string()));
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "user");
        assert_eq!(converted[1].role, "assistant");
    }

    #[test]
    fn test_extract_sse_event() {
        let mut buffer = "event: message_start\ndata: {\"type\":\"message\"}\n\n".to_string();
        let event = AnthropicProvider::extract_sse_event(&mut buffer).unwrap();

        assert_eq!(event.event, "message_start");
        assert_eq!(event.data, "{\"type\":\"message\"}");
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_sanitize_messages_valid_tool_pairs() {
        use crate::message::ContentPart;
        
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        // Valid messages with matching tool_use and tool_result
        let messages = vec![
            Message::user("Hello"),
            Message {
                role: crate::message::Role::Assistant,
                content: vec![ContentPart::ToolUse {
                    id: "tool_1".to_string(),
                    name: "read".to_string(),
                    input: serde_json::json!({}),
                }],
            },
            Message {
                role: crate::message::Role::User,
                content: vec![ContentPart::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: "result".to_string(),
                    is_error: None,
                }],
            },
            Message::assistant("Done"),
        ];

        let sanitized = provider.sanitize_messages(&messages);
        
        // Should keep all messages unchanged
        assert_eq!(sanitized.len(), 4);
        // Check tool_use is preserved
        assert!(sanitized[1].content.iter().any(|p| matches!(p, ContentPart::ToolUse { .. })));
        // Check tool_result is preserved
        assert!(sanitized[2].content.iter().any(|p| matches!(p, ContentPart::ToolResult { .. })));
    }

    #[test]
    fn test_sanitize_messages_orphaned_tool_use() {
        use crate::message::ContentPart;
        
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        // Tool_use without matching tool_result in next message
        let messages = vec![
            Message::user("Hello"),
            Message {
                role: crate::message::Role::Assistant,
                content: vec![
                    ContentPart::Text { text: "Let me help".to_string() },
                    ContentPart::ToolUse {
                        id: "orphan_tool".to_string(),
                        name: "read".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
            },
            Message::user("Never mind"), // No tool_result here
        ];

        let sanitized = provider.sanitize_messages(&messages);
        
        // Should have 3 messages
        assert_eq!(sanitized.len(), 3);
        // Tool_use should be removed from second message
        assert!(!sanitized[1].content.iter().any(|p| matches!(p, ContentPart::ToolUse { .. })));
        // Text should be preserved
        assert!(sanitized[1].content.iter().any(|p| matches!(p, ContentPart::Text { .. })));
    }

    #[test]
    fn test_sanitize_messages_orphaned_tool_result() {
        use crate::message::ContentPart;
        
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        // Tool_result without matching tool_use in previous message
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi there"), // No tool_use here
            Message {
                role: crate::message::Role::User,
                content: vec![
                    ContentPart::Text { text: "Here's the result".to_string() },
                    ContentPart::ToolResult {
                        tool_use_id: "orphan_result".to_string(),
                        content: "result".to_string(),
                        is_error: None,
                    },
                ],
            },
        ];

        let sanitized = provider.sanitize_messages(&messages);
        
        // Should have 3 messages
        assert_eq!(sanitized.len(), 3);
        // Tool_result should be removed from third message
        assert!(!sanitized[2].content.iter().any(|p| matches!(p, ContentPart::ToolResult { .. })));
        // Text should be preserved
        assert!(sanitized[2].content.iter().any(|p| matches!(p, ContentPart::Text { .. })));
    }

    #[test]
    fn test_sanitize_messages_partial_tool_match() {
        use crate::message::ContentPart;
        
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        // Multiple tool_use but only some have matching tool_result
        let messages = vec![
            Message::user("Hello"),
            Message {
                role: crate::message::Role::Assistant,
                content: vec![
                    ContentPart::ToolUse {
                        id: "tool_1".to_string(),
                        name: "read".to_string(),
                        input: serde_json::json!({}),
                    },
                    ContentPart::ToolUse {
                        id: "tool_2".to_string(),
                        name: "write".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
            },
            Message {
                role: crate::message::Role::User,
                content: vec![
                    ContentPart::ToolResult {
                        tool_use_id: "tool_1".to_string(), // Only tool_1 has result
                        content: "result".to_string(),
                        is_error: None,
                    },
                ],
            },
        ];

        let sanitized = provider.sanitize_messages(&messages);
        
        // Should have 3 messages
        assert_eq!(sanitized.len(), 3);
        // Only tool_1 should remain in assistant message
        let tool_uses: Vec<_> = sanitized[1].content.iter()
            .filter_map(|p| if let ContentPart::ToolUse { id, .. } = p { Some(id.as_str()) } else { None })
            .collect();
        assert_eq!(tool_uses, vec!["tool_1"]);
    }

    #[test]
    fn test_sanitize_messages_only_tool_use_removed() {
        use crate::message::ContentPart;
        
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        // Assistant message with only tool_use and no matching result
        let messages = vec![
            Message::user("Hello"),
            Message {
                role: crate::message::Role::Assistant,
                content: vec![
                    ContentPart::ToolUse {
                        id: "orphan".to_string(),
                        name: "read".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
            },
            Message::user("Continue"), // No tool_result
        ];

        let sanitized = provider.sanitize_messages(&messages);
        
        // Should have 3 messages
        assert_eq!(sanitized.len(), 3);
        // Second message should have placeholder text
        assert!(sanitized[1].content.iter().any(|p| {
            if let ContentPart::Text { text } = p {
                text.contains("removed")
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_convert_content_tool_use_input_validation() {
        use crate::message::ContentPart;
        
        let provider = AnthropicProvider {
            client: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
            model: crate::model::anthropic::claude_sonnet_4(),
        };

        // Test with valid object input
        let content_valid = vec![ContentPart::ToolUse {
            id: "tool_1".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({"path": "/test"}),
        }];
        let converted = provider.convert_content(&content_valid);
        assert_eq!(converted.len(), 1);
        let input = &converted[0]["input"];
        assert!(input.is_object());
        assert_eq!(input["path"], "/test");

        // Test with null input - should become empty object
        let content_null = vec![ContentPart::ToolUse {
            id: "tool_2".to_string(),
            name: "list".to_string(),
            input: serde_json::Value::Null,
        }];
        let converted = provider.convert_content(&content_null);
        assert_eq!(converted.len(), 1);
        let input = &converted[0]["input"];
        assert!(input.is_object());
        assert_eq!(input.as_object().unwrap().len(), 0);

        // Test with string input - should become empty object
        let content_string = vec![ContentPart::ToolUse {
            id: "tool_3".to_string(),
            name: "echo".to_string(),
            input: serde_json::json!("invalid string input"),
        }];
        let converted = provider.convert_content(&content_string);
        assert_eq!(converted.len(), 1);
        let input = &converted[0]["input"];
        assert!(input.is_object());

        // Test with array input - should become empty object
        let content_array = vec![ContentPart::ToolUse {
            id: "tool_4".to_string(),
            name: "batch".to_string(),
            input: serde_json::json!(["a", "b", "c"]),
        }];
        let converted = provider.convert_content(&content_array);
        assert_eq!(converted.len(), 1);
        let input = &converted[0]["input"];
        assert!(input.is_object());
    }
}
