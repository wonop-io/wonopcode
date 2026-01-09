//! Amazon Bedrock provider implementation.
//!
//! Supports Claude and other models via AWS Bedrock's converse API.
//! Uses AWS credentials from environment or credential chain.

use crate::{
    error::ProviderError,
    message::{ContentPart, ImageSource, Message, Role},
    model::ModelInfo,
    stream::{FinishReason, StreamChunk, Usage},
    GenerateOptions, LanguageModel, ProviderResult, ToolDefinition,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::stream::BoxStream;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{json, Value};
use std::time::SystemTime;
use tracing::{debug, trace, warn};

/// Default AWS region for Bedrock.
const DEFAULT_REGION: &str = "us-east-1";

/// Bedrock provider configuration.
#[derive(Debug, Clone)]
pub struct BedrockConfig {
    /// AWS region.
    pub region: String,
    /// AWS access key ID.
    pub access_key_id: Option<String>,
    /// AWS secret access key.
    pub secret_access_key: Option<String>,
    /// AWS session token (optional, for temporary credentials).
    pub session_token: Option<String>,
    /// Model information.
    pub model: ModelInfo,
}

impl Default for BedrockConfig {
    fn default() -> Self {
        Self {
            region: DEFAULT_REGION.to_string(),
            access_key_id: None,
            secret_access_key: None,
            session_token: None,
            model: ModelInfo::default(),
        }
    }
}

/// Amazon Bedrock provider.
pub struct BedrockProvider {
    client: reqwest::Client,
    region: String,
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    model: ModelInfo,
}

impl BedrockProvider {
    /// Create a new Bedrock provider.
    pub fn new(config: BedrockConfig) -> ProviderResult<Self> {
        // Get credentials from config or environment
        let access_key_id = config
            .access_key_id
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .ok_or_else(|| ProviderError::missing_api_key("amazon-bedrock (AWS_ACCESS_KEY_ID)"))?;

        let secret_access_key = config
            .secret_access_key
            .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
            .ok_or_else(|| {
                ProviderError::missing_api_key("amazon-bedrock (AWS_SECRET_ACCESS_KEY)")
            })?;

        let session_token = config
            .session_token
            .or_else(|| std::env::var("AWS_SESSION_TOKEN").ok());

        let region = if !config.region.is_empty() {
            config.region
        } else {
            std::env::var("AWS_REGION").unwrap_or_else(|_| DEFAULT_REGION.to_string())
        };

        debug!(region = %region, model = %config.model.id, "Creating Bedrock provider");

        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        Ok(Self {
            client,
            region,
            access_key_id,
            secret_access_key,
            session_token,
            model: config.model,
        })
    }

    /// Get the Bedrock endpoint URL.
    fn endpoint(&self) -> String {
        format!("https://bedrock-runtime.{}.amazonaws.com", self.region)
    }

    /// Apply region prefix to model ID if needed.
    fn apply_model_prefix(&self, model_id: &str) -> String {
        // Skip if already has global prefix
        if model_id.starts_with("global.") {
            return model_id.to_string();
        }

        let region_prefix = self.region.split('-').next().unwrap_or("us");

        match region_prefix {
            "us" => {
                let needs_prefix = ["nova", "claude", "deepseek"]
                    .iter()
                    .any(|m| model_id.contains(m));
                let is_govcloud = self.region.starts_with("us-gov");

                if needs_prefix && !is_govcloud {
                    format!("us.{model_id}")
                } else {
                    model_id.to_string()
                }
            }
            "eu" => {
                let region_needs_prefix = [
                    "eu-west-1",
                    "eu-west-2",
                    "eu-west-3",
                    "eu-north-1",
                    "eu-central-1",
                    "eu-south-1",
                    "eu-south-2",
                ]
                .iter()
                .any(|r| self.region.contains(r));

                let model_needs_prefix = ["claude", "nova-lite", "nova-micro", "llama3", "pixtral"]
                    .iter()
                    .any(|m| model_id.contains(m));

                if region_needs_prefix && model_needs_prefix {
                    format!("eu.{model_id}")
                } else {
                    model_id.to_string()
                }
            }
            "ap" => {
                let is_australia =
                    ["ap-southeast-2", "ap-southeast-4"].contains(&self.region.as_str());

                if is_australia
                    && (model_id.contains("claude-sonnet-4") || model_id.contains("claude-haiku"))
                {
                    format!("au.{model_id}")
                } else {
                    let needs_prefix = ["claude", "nova-lite", "nova-micro", "nova-pro"]
                        .iter()
                        .any(|m| model_id.contains(m));

                    if needs_prefix {
                        format!("apac.{model_id}")
                    } else {
                        model_id.to_string()
                    }
                }
            }
            _ => model_id.to_string(),
        }
    }

    /// Convert messages to Bedrock converse format.
    fn convert_messages(&self, messages: &[Message]) -> (Option<Vec<Value>>, Vec<Value>) {
        let mut system_messages = Vec::new();
        let mut converse_messages = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    system_messages.push(json!({
                        "text": msg.text()
                    }));
                }
                Role::User | Role::Tool => {
                    let content = self.convert_content(&msg.content, false);
                    if !content.is_empty() {
                        converse_messages.push(json!({
                            "role": "user",
                            "content": content
                        }));
                    }
                }
                Role::Assistant => {
                    let content = self.convert_content(&msg.content, true);
                    if !content.is_empty() {
                        converse_messages.push(json!({
                            "role": "assistant",
                            "content": content
                        }));
                    }
                }
            }
        }

        let system = if system_messages.is_empty() {
            None
        } else {
            Some(system_messages)
        };

        (system, converse_messages)
    }

    /// Convert content parts to Bedrock format.
    fn convert_content(&self, parts: &[ContentPart], is_assistant: bool) -> Vec<Value> {
        parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(json!({ "text": text })),
                ContentPart::Image { source } => {
                    let (format, bytes) = match source {
                        ImageSource::Base64 { media_type, data } => {
                            let format = media_type.split('/').next_back().unwrap_or("png");
                            (format.to_string(), data.clone())
                        }
                        ImageSource::Url { .. } => {
                            // Bedrock doesn't support URL images directly
                            warn!("URL images not supported by Bedrock, skipping");
                            return None;
                        }
                    };
                    Some(json!({
                        "image": {
                            "format": format,
                            "source": {
                                "bytes": bytes
                            }
                        }
                    }))
                }
                ContentPart::ToolUse { id, name, input } if is_assistant => Some(json!({
                    "toolUse": {
                        "toolUseId": id,
                        "name": name,
                        "input": input
                    }
                })),
                ContentPart::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => Some(json!({
                    "toolResult": {
                        "toolUseId": tool_use_id,
                        "content": [{ "text": content }],
                        "status": if is_error.unwrap_or(false) { "error" } else { "success" }
                    }
                })),
                _ => None,
            })
            .collect()
    }

    /// Convert tools to Bedrock format.
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }

        let tool_configs: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "toolSpec": {
                        "name": tool.name,
                        "description": tool.description,
                        "inputSchema": {
                            "json": tool.parameters
                        }
                    }
                })
            })
            .collect();

        Some(json!({ "tools": tool_configs }))
    }

    /// Sign a request with AWS Signature Version 4.
    fn sign_request(
        &self,
        method: &str,
        path: &str,
        body: &str,
        headers: &mut HeaderMap,
    ) -> ProviderResult<()> {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| ProviderError::internal(e.to_string()))?;

        let datetime = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
            .ok_or_else(|| ProviderError::internal("Invalid timestamp"))?;

        let amz_date = datetime.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = datetime.format("%Y%m%d").to_string();

        // Create content hash
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let content_hash = hex::encode(hasher.finalize());

        // Add required headers
        headers.insert(
            "x-amz-date",
            HeaderValue::from_str(&amz_date).map_err(|e| ProviderError::internal(e.to_string()))?,
        );
        headers.insert(
            "x-amz-content-sha256",
            HeaderValue::from_str(&content_hash)
                .map_err(|e| ProviderError::internal(e.to_string()))?,
        );

        if let Some(ref token) = self.session_token {
            headers.insert(
                "x-amz-security-token",
                HeaderValue::from_str(token).map_err(|e| ProviderError::internal(e.to_string()))?,
            );
        }

        let host = format!("bedrock-runtime.{}.amazonaws.com", self.region);
        headers.insert(
            "host",
            HeaderValue::from_str(&host).map_err(|e| ProviderError::internal(e.to_string()))?,
        );

        // Create canonical request
        let signed_headers = if self.session_token.is_some() {
            "content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token"
        } else {
            "content-type;host;x-amz-content-sha256;x-amz-date"
        };

        let canonical_headers = if let Some(ref token) = self.session_token {
            format!(
                "content-type:application/json\nhost:{host}\nx-amz-content-sha256:{content_hash}\nx-amz-date:{amz_date}\nx-amz-security-token:{token}\n"
            )
        } else {
            format!(
                "content-type:application/json\nhost:{host}\nx-amz-content-sha256:{content_hash}\nx-amz-date:{amz_date}\n"
            )
        };

        let canonical_request =
            format!("{method}\n{path}\n\n{canonical_headers}\n{signed_headers}\n{content_hash}");

        // Create string to sign
        let algorithm = "AWS4-HMAC-SHA256";
        let credential_scope = format!("{date_stamp}/{}/bedrock/aws4_request", self.region);

        let mut hasher = Sha256::new();
        hasher.update(canonical_request.as_bytes());
        let canonical_hash = hex::encode(hasher.finalize());

        let string_to_sign =
            format!("{algorithm}\n{amz_date}\n{credential_scope}\n{canonical_hash}");

        // Create signing key
        type HmacSha256 = Hmac<Sha256>;

        let k_date =
            HmacSha256::new_from_slice(format!("AWS4{}", self.secret_access_key).as_bytes())
                .map_err(|e| ProviderError::internal(e.to_string()))?
                .chain_update(date_stamp.as_bytes())
                .finalize()
                .into_bytes();

        let k_region = HmacSha256::new_from_slice(&k_date)
            .map_err(|e| ProviderError::internal(e.to_string()))?
            .chain_update(self.region.as_bytes())
            .finalize()
            .into_bytes();

        let k_service = HmacSha256::new_from_slice(&k_region)
            .map_err(|e| ProviderError::internal(e.to_string()))?
            .chain_update(b"bedrock")
            .finalize()
            .into_bytes();

        let k_signing = HmacSha256::new_from_slice(&k_service)
            .map_err(|e| ProviderError::internal(e.to_string()))?
            .chain_update(b"aws4_request")
            .finalize()
            .into_bytes();

        // Create signature
        let signature = HmacSha256::new_from_slice(&k_signing)
            .map_err(|e| ProviderError::internal(e.to_string()))?
            .chain_update(string_to_sign.as_bytes())
            .finalize();

        let signature_hex = hex::encode(signature.into_bytes());

        // Create authorization header
        let authorization = format!(
            "{} Credential={}/{}, SignedHeaders={}, Signature={}",
            algorithm, self.access_key_id, credential_scope, signed_headers, signature_hex
        );

        headers.insert(
            "authorization",
            HeaderValue::from_str(&authorization)
                .map_err(|e| ProviderError::internal(e.to_string()))?,
        );

        Ok(())
    }
}

#[async_trait]
impl LanguageModel for BedrockProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        let model_id = self.apply_model_prefix(&self.model.id);
        let (system, converse_messages) = self.convert_messages(&messages);

        // Add system from options if provided
        let system = match (system, &options.system) {
            (Some(mut s), Some(opt_system)) => {
                s.insert(0, json!({ "text": opt_system }));
                Some(s)
            }
            (None, Some(opt_system)) => Some(vec![json!({ "text": opt_system })]),
            (s, None) => s,
        };

        let mut request = json!({
            "messages": converse_messages,
        });

        if let Some(sys) = system {
            request["system"] = json!(sys);
        }

        // Add inference config
        let mut inference_config = json!({});
        if let Some(max_tokens) = options.max_tokens {
            inference_config["maxTokens"] = json!(max_tokens);
        }
        if let Some(temp) = options.temperature {
            inference_config["temperature"] = json!(temp);
        }
        if let Some(top_p) = options.top_p {
            inference_config["topP"] = json!(top_p);
        }
        if inference_config
            .as_object()
            .map(|o| !o.is_empty())
            .unwrap_or(false)
        {
            request["inferenceConfig"] = inference_config;
        }

        // Add tools
        if let Some(tool_config) = self.convert_tools(&options.tools) {
            request["toolConfig"] = tool_config;
        }

        let body =
            serde_json::to_string(&request).map_err(|e| ProviderError::internal(e.to_string()))?;

        let path = format!("/model/{}/converse-stream", urlencoding::encode(&model_id));
        let url = format!("{}{path}", self.endpoint());

        debug!(model = %model_id, region = %self.region, "Sending Bedrock request");
        trace!(request = %body, "Full request");

        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        self.sign_request("POST", &path, &body, &mut headers)?;

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .body(body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!(status = %status, error = %error_text, "Bedrock API error");
            return Err(ProviderError::Internal {
                message: format!("Bedrock API error {status}: {error_text}"),
            });
        }

        let byte_stream = response.bytes_stream();
        let abort = options.abort.clone();

        Ok(Box::pin(try_stream! {
            use futures::StreamExt;

            let mut buffer = Vec::new();
            let mut text_started = false;
            let mut current_tool_id = None;
            let mut current_tool_name = None;
            let mut tool_args = String::new();
            // Defer FinishStep until we have usage from Metadata event
            let mut pending_finish_reason: Option<FinishReason> = None;
            let mut final_usage = Usage::default();

            futures::pin_mut!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                // Check for cancellation
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        Err(ProviderError::Cancelled)?;
                    }
                }

                let chunk = chunk_result.map_err(|e| ProviderError::internal(e.to_string()))?;
                buffer.extend_from_slice(&chunk);

                // Parse Bedrock event stream format
                while let Some(event) = parse_bedrock_event(&mut buffer)? {
                    trace!(event = ?event, "Bedrock event");

                    match event {
                        BedrockEvent::ContentBlockStart { content_block } => {
                            if let Some(text) = content_block.get("text") {
                                if text.is_string() {
                                    yield StreamChunk::TextStart;
                                    text_started = true;
                                }
                            }
                            if let Some(tool_use) = content_block.get("toolUse") {
                                current_tool_id = tool_use.get("toolUseId").and_then(|v| v.as_str()).map(String::from);
                                current_tool_name = tool_use.get("name").and_then(|v| v.as_str()).map(String::from);
                                tool_args.clear();

                                if let (Some(id), Some(name)) = (&current_tool_id, &current_tool_name) {
                                    yield StreamChunk::ToolCallStart {
                                        id: id.clone(),
                                        name: name.clone(),
                                    };
                                }
                            }
                        }
                        BedrockEvent::ContentBlockDelta { delta } => {
                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    if !text_started {
                                        yield StreamChunk::TextStart;
                                        text_started = true;
                                    }
                                    yield StreamChunk::TextDelta(text.to_string());
                                }
                            }
                            if let Some(tool_use) = delta.get("toolUse") {
                                if let Some(input) = tool_use.get("input").and_then(|v| v.as_str()) {
                                    tool_args.push_str(input);
                                }
                            }
                        }
                        BedrockEvent::ContentBlockStop => {
                            if text_started {
                                yield StreamChunk::TextEnd;
                                text_started = false;
                            }
                            if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                                yield StreamChunk::ToolCall {
                                    id,
                                    name,
                                    arguments: std::mem::take(&mut tool_args),
                                };
                            }
                        }
                        BedrockEvent::MessageStop { stop_reason } => {
                            // Store finish reason, defer emitting until we have usage
                            pending_finish_reason = Some(match stop_reason.as_str() {
                                "end_turn" => FinishReason::EndTurn,
                                "tool_use" => FinishReason::ToolUse,
                                "max_tokens" => FinishReason::MaxTokens,
                                "content_filtered" => FinishReason::ContentFilter,
                                _ => FinishReason::EndTurn,
                            });
                        }
                        BedrockEvent::Metadata { usage } => {
                            if let (Some(input), Some(output)) = (
                                usage.get("inputTokens").and_then(|v| v.as_u64()),
                                usage.get("outputTokens").and_then(|v| v.as_u64()),
                            ) {
                                final_usage = Usage::new(input as u32, output as u32);
                                debug!(input_tokens = input, output_tokens = output, "Bedrock usage");
                            }

                            // Now emit FinishStep with actual usage
                            if let Some(finish_reason) = pending_finish_reason.take() {
                                yield StreamChunk::FinishStep {
                                    usage: final_usage.clone(),
                                    finish_reason,
                                };
                            }
                        }
                        BedrockEvent::Unknown => {}
                    }
                }
            }

            // Emit FinishStep if we got MessageStop but no Metadata (shouldn't happen, but be safe)
            if let Some(finish_reason) = pending_finish_reason.take() {
                yield StreamChunk::FinishStep {
                    usage: final_usage,
                    finish_reason,
                };
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "amazon-bedrock"
    }
}

/// Bedrock event types.
#[derive(Debug)]
enum BedrockEvent {
    ContentBlockStart { content_block: Value },
    ContentBlockDelta { delta: Value },
    ContentBlockStop,
    MessageStop { stop_reason: String },
    Metadata { usage: Value },
    Unknown,
}

/// Parse a Bedrock event from the buffer.
fn parse_bedrock_event(buffer: &mut Vec<u8>) -> ProviderResult<Option<BedrockEvent>> {
    // Bedrock uses AWS event stream format with :message-type header
    // For simplicity, we parse the JSON payloads that contain event type

    // Look for complete JSON objects
    let text = String::from_utf8_lossy(buffer).to_string();

    // Find complete JSON objects (simplified parsing)
    if let Some(start) = text.find('{') {
        let remaining = &text[start..];

        // Try to find matching brace
        let mut depth = 0;
        let mut end = 0;

        for (i, c) in remaining.chars().enumerate() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        if depth == 0 && end > start {
            let json_str = text[start..end].to_string();

            // Remove parsed bytes from buffer
            *buffer = buffer[end..].to_vec();

            // Parse JSON
            if let Ok(event) = serde_json::from_str::<Value>(&json_str) {
                // Determine event type
                if event.get("contentBlockStart").is_some() {
                    if let Some(content_block) = event
                        .get("contentBlockStart")
                        .and_then(|v| v.get("contentBlock"))
                    {
                        return Ok(Some(BedrockEvent::ContentBlockStart {
                            content_block: content_block.clone(),
                        }));
                    }
                } else if event.get("contentBlockDelta").is_some() {
                    if let Some(delta) = event.get("contentBlockDelta").and_then(|v| v.get("delta"))
                    {
                        return Ok(Some(BedrockEvent::ContentBlockDelta {
                            delta: delta.clone(),
                        }));
                    }
                } else if event.get("contentBlockStop").is_some() {
                    return Ok(Some(BedrockEvent::ContentBlockStop));
                } else if let Some(stop) = event.get("messageStop") {
                    let stop_reason = stop
                        .get("stopReason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("end_turn")
                        .to_string();
                    return Ok(Some(BedrockEvent::MessageStop { stop_reason }));
                } else if let Some(metadata) = event.get("metadata") {
                    if let Some(usage) = metadata.get("usage") {
                        return Ok(Some(BedrockEvent::Metadata {
                            usage: usage.clone(),
                        }));
                    }
                }

                return Ok(Some(BedrockEvent::Unknown));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_model_prefix_us() {
        let config = BedrockConfig {
            region: "us-east-1".to_string(),
            access_key_id: Some("test".to_string()),
            secret_access_key: Some("test".to_string()),
            ..Default::default()
        };

        let provider = BedrockProvider::new(config).unwrap();

        assert_eq!(
            provider.apply_model_prefix("anthropic.claude-3-sonnet"),
            "us.anthropic.claude-3-sonnet"
        );
        assert_eq!(
            provider.apply_model_prefix("global.meta-llama"),
            "global.meta-llama"
        );
    }

    #[test]
    fn test_apply_model_prefix_eu() {
        let config = BedrockConfig {
            region: "eu-west-1".to_string(),
            access_key_id: Some("test".to_string()),
            secret_access_key: Some("test".to_string()),
            ..Default::default()
        };

        let provider = BedrockProvider::new(config).unwrap();

        assert_eq!(
            provider.apply_model_prefix("anthropic.claude-3-sonnet"),
            "eu.anthropic.claude-3-sonnet"
        );
    }
}
