//! Provider client wrapper for workflow execution
//!
//! This module provides a provider-agnostic wrapper around wonopcode-provider's
//! streaming API, collecting streamed responses into complete messages.

use crate::config::EngineConfig;
use crate::error::{Result, WorkflowError};
use crate::io::{WorkflowEvent, WorkflowIO};
use futures::StreamExt;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use wonopcode_provider::{
    anthropic::AnthropicProvider,
    codex::CodexProvider,
    compoundcoders::CompoundCodersProvider,
    openai::OpenAIProvider,
    registry::{get_model_info, get_provider, infer_provider},
    BoxedLanguageModel, GenerateOptions, Message, StreamChunk, ToolDefinition,
};

/// A collected tool call from streaming response
#[derive(Debug, Clone)]
pub struct CollectedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Result of collecting a streamed response
#[derive(Debug, Clone)]
pub struct CollectedResponse {
    /// The text content of the response
    pub content: String,
    /// Any tool calls requested by the assistant
    pub tool_calls: Vec<CollectedToolCall>,
    /// Token usage (input, output)
    pub usage: Option<(u32, u32)>,
}

/// Provider client wrapper with retry logic
pub struct ProviderClient {
    provider: BoxedLanguageModel,
    max_retries: usize,
    retry_delay_ms: u64,
}

impl Clone for ProviderClient {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            max_retries: self.max_retries,
            retry_delay_ms: self.retry_delay_ms,
        }
    }
}

impl ProviderClient {
    /// Create a new provider client from configuration
    pub async fn new(config: &EngineConfig) -> Result<Self> {
        log::debug!("Creating provider from configuration");

        // Determine provider: infer from model if model is specified
        let effective_provider = if let Some(ref model) = config.model {
            let inferred = infer_provider(model);
            log::info!(
                "Inferred provider '{}' from model '{}' (configured: '{}')",
                inferred,
                model,
                config.provider
            );
            inferred.to_string()
        } else {
            config.provider.clone()
        };

        let provider = create_provider(&effective_provider, config.model.as_deref()).await?;

        log::info!(
            "Provider client configured: {} with model {}",
            provider.provider_id(),
            provider.model_info().id
        );

        Ok(Self {
            provider,
            max_retries: 3,
            retry_delay_ms: 1000,
        })
    }

    /// Send messages and get a collected response (handles streaming internally)
    pub async fn send_message(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> Result<CollectedResponse> {
        self.send_message_with_io(messages, options, Option::<&crate::io::NullIO>::None).await
    }

    /// Send messages with IO reporting for progress/streaming
    pub async fn send_message_with_io<IO: WorkflowIO>(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
        io: Option<&IO>,
    ) -> Result<CollectedResponse> {
        log::debug!("Preparing request with {} messages", messages.len());

        let mut last_error = None;
        let mut retry_delay = self.retry_delay_ms;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                log::warn!(
                    "Retrying request (attempt {}/{}) after {}ms delay",
                    attempt,
                    self.max_retries,
                    retry_delay
                );
                if let Some(io) = io {
                    io.emit(WorkflowEvent::Progress {
                        message: format!("Retrying request (attempt {})...", attempt + 1),
                    })
                    .await;
                }
                tokio::time::sleep(Duration::from_millis(retry_delay)).await;
                retry_delay *= 2;
            } else {
                log::debug!(
                    "Sending request to provider (attempt 1/{})",
                    self.max_retries + 1
                );
            }

            match self.collect_response(&messages, &options, io).await {
                Ok(response) => {
                    log::info!("Successfully received response from provider");
                    if let Some((input, output)) = response.usage {
                        log::debug!("Response usage: input={}, output={}", input, output);
                        if let Some(io) = io {
                            io.emit(WorkflowEvent::TokenUsage {
                                input_tokens: input,
                                output_tokens: output,
                                total_tokens: input + output,
                            })
                            .await;
                        }
                    }
                    return Ok(response);
                }
                Err(e) => {
                    let error_str = e.to_string();
                    log::warn!("Provider request failed: {}", error_str);

                    let is_retryable = Self::is_retryable_error(&error_str);

                    if !is_retryable || attempt == self.max_retries {
                        log::error!(
                            "Provider error after {} attempt(s): {}",
                            attempt + 1,
                            error_str
                        );
                        return Err(WorkflowError::Provider(format!(
                            "Provider error after {} attempt(s): {}",
                            attempt + 1,
                            error_str
                        )));
                    }

                    last_error = Some(error_str);
                }
            }
        }

        Err(WorkflowError::Provider(format!(
            "Max retries ({}) exceeded: {}",
            self.max_retries,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )))
    }

    /// Collect a streaming response into a single response
    async fn collect_response<IO: WorkflowIO>(
        &self,
        messages: &[Message],
        options: &GenerateOptions,
        io: Option<&IO>,
    ) -> Result<CollectedResponse> {
        let stream = self
            .provider
            .generate(messages.to_vec(), options.clone())
            .await
            .map_err(|e| WorkflowError::Provider(e.to_string()))?;

        let mut content = String::new();
        let mut tool_calls: Vec<CollectedToolCall> = Vec::new();
        let mut pending_tool_calls: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        let mut usage: Option<(u32, u32)> = None;

        futures::pin_mut!(stream);

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| WorkflowError::Provider(e.to_string()))?;

            match chunk {
                StreamChunk::TextDelta(text) => {
                    content.push_str(&text);
                    if let Some(io) = io {
                        io.emit(WorkflowEvent::TextOutput {
                            text,
                            is_complete: false,
                        })
                        .await;
                    }
                }
                StreamChunk::ToolCallStart { id, name } => {
                    pending_tool_calls.insert(id.clone(), (name.clone(), String::new()));
                    if let Some(io) = io {
                        io.emit(WorkflowEvent::ToolCallStarted {
                            tool_name: name,
                            tool_id: id,
                        })
                        .await;
                    }
                }
                StreamChunk::ToolCallDelta { id, delta } => {
                    if let Some((_, args)) = pending_tool_calls.get_mut(&id) {
                        args.push_str(&delta);
                    }
                }
                StreamChunk::ToolCall { id, name, arguments } => {
                    tool_calls.push(CollectedToolCall { id, name, arguments });
                }
                StreamChunk::FinishStep {
                    usage: step_usage, ..
                } => {
                    usage = Some((step_usage.input_tokens, step_usage.output_tokens));
                }
                StreamChunk::Error(err) => {
                    return Err(WorkflowError::Provider(err));
                }
                _ => {}
            }
        }

        // Signal text output complete
        if let Some(io) = io {
            if !content.is_empty() {
                io.emit(WorkflowEvent::TextOutput {
                    text: String::new(),
                    is_complete: true,
                })
                .await;
            }
        }

        // Finalize any pending tool calls
        for (id, (name, arguments)) in pending_tool_calls {
            if !tool_calls.iter().any(|tc| tc.id == id) {
                tool_calls.push(CollectedToolCall { id, name, arguments });
            }
        }

        Ok(CollectedResponse {
            content,
            tool_calls,
            usage,
        })
    }

    fn is_retryable_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("connection")
            || e.contains("timeout")
            || e.contains("network")
            || e.contains("rate limit")
            || e.contains("429")
            || e.contains("500")
            || e.contains("502")
            || e.contains("503")
            || e.contains("504")
    }

    pub fn provider_name(&self) -> &str {
        self.provider.provider_id()
    }

    pub fn model_name(&self) -> &str {
        &self.provider.model_info().id
    }

    pub fn build_options(&self, config: &EngineConfig, system: Option<String>) -> GenerateOptions {
        let mut options = GenerateOptions {
            system,
            max_tokens: config.max_tokens.map(|t| t as u32),
            ..Default::default()
        };

        if config.enable_tools {
            options.tools = build_tool_definitions(config);
        }

        options
    }
}

async fn create_provider(
    provider_name: &str,
    model_name: Option<&str>,
) -> Result<BoxedLanguageModel> {
    log::debug!(
        "Creating provider: {} with model: {:?}",
        provider_name,
        model_name
    );

    let provider_info = get_provider(provider_name).ok_or_else(|| {
        WorkflowError::Provider(format!("Provider '{}' not found", provider_name))
    })?;

    let model_id = model_name
        .map(|m| m.to_string())
        .unwrap_or_else(|| provider_info.default_model.to_string());

    let model_info = get_model_info(&model_id, provider_name);

    let provider: BoxedLanguageModel = match provider_name {
        "anthropic" => {
            let api_key = env::var("ANTHROPIC_API_KEY")
                .map_err(|_| WorkflowError::Provider("ANTHROPIC_API_KEY not set".into()))?;
            Arc::new(
                AnthropicProvider::new(&api_key, model_info.clone())
                    .map_err(|e| WorkflowError::Provider(e.to_string()))?,
            )
        }
        "openai" => {
            let api_key = env::var("OPENAI_API_KEY")
                .map_err(|_| WorkflowError::Provider("OPENAI_API_KEY not set".into()))?;
            Arc::new(
                OpenAIProvider::new(&api_key, model_info.clone())
                    .map_err(|e| WorkflowError::Provider(e.to_string()))?,
            )
        }
        "openai-codex" | "codex" => {
            Arc::new(
                CodexProvider::new(model_info.clone())
                    .map_err(|e| WorkflowError::Provider(e.to_string()))?,
            )
        }
        "compoundcoders" => {
            let api_key = env::var("COMPOUNDCODER_API_KEY")
                .map_err(|_| WorkflowError::Provider("COMPOUNDCODER_API_KEY not set".into()))?;
            Arc::new(
                CompoundCodersProvider::new(&api_key, model_info.clone())
                    .map_err(|e| WorkflowError::Provider(e.to_string()))?,
            )
        }
        _ => {
            return Err(WorkflowError::Provider(format!(
                "Unsupported provider: {}",
                provider_name
            )))
        }
    };

    Ok(provider)
}

fn build_tool_definitions(config: &EngineConfig) -> Vec<ToolDefinition> {
    use wonopcode_tools::ToolRegistry;

    let registry = ToolRegistry::with_builtins();
    let tool_ids: Vec<String> = if config.enabled_tools.is_empty() {
        registry.list().iter().map(|s| s.to_string()).collect()
    } else {
        config.enabled_tools.clone()
    };

    tool_ids
        .iter()
        .filter_map(|name| registry.get(name))
        .map(|tool| ToolDefinition {
            name: tool.id().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters_schema(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retryable_error() {
        assert!(ProviderClient::is_retryable_error("Connection timeout"));
        assert!(ProviderClient::is_retryable_error("Rate limit exceeded"));
        assert!(ProviderClient::is_retryable_error("503 Service Unavailable"));
        assert!(!ProviderClient::is_retryable_error("Invalid API key"));
        assert!(!ProviderClient::is_retryable_error("401 Unauthorized"));
    }
}

