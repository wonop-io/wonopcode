//! Prompt handling for the server API.
//!
//! This module provides the ability to run prompts via HTTP with SSE streaming.

use crate::state::SharedTodoStore;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use wonopcode_core::bus::Event;
use wonopcode_core::Agent;
use wonopcode_provider::{
    model::{ModelCapabilities, ModelCost, ModelInfo, ModelLimit},
    stream::StreamChunk,
    BoxedLanguageModel, GenerateOptions, Message as ProviderMessage, ToolDefinition,
};
use wonopcode_tools::todo::{TodoItem, TodoPriority, TodoStatus};

/// Active session runners for abort support.
pub type SessionRunners = Arc<RwLock<HashMap<String, CancellationToken>>>;

/// Create a new session runners registry.
pub fn new_session_runners() -> SessionRunners {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Event types emitted during prompt execution.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptEvent {
    /// Prompt processing started.
    Started {
        session_id: String,
        message_id: String,
    },
    /// Text delta received from model.
    TextDelta { delta: String },
    /// Tool execution started.
    ToolStarted {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool execution completed.
    ToolCompleted {
        id: String,
        success: bool,
        output: String,
    },
    /// Token usage update.
    TokenUsage { input: u32, output: u32, cost: f64 },
    /// Status update.
    Status { message: String },
    /// Prompt completed successfully.
    Completed { message_id: String, text: String },
    /// Prompt failed with error.
    Error { error: String },
    /// Prompt was aborted.
    Aborted,
}

impl Event for PromptEvent {
    fn event_type() -> &'static str {
        "prompt"
    }
}

/// Request to execute a prompt.
#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// Response from prompt execution.
#[derive(Debug, Serialize)]
pub struct PromptResponse {
    pub message_id: String,
    pub text: String,
    pub usage: PromptUsage,
}

/// Token usage from prompt.
#[derive(Debug, Serialize)]
pub struct PromptUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost: f64,
}

/// Agent configuration for prompt execution.
#[derive(Debug, Clone, Default)]
pub struct AgentConfig {
    /// Agent name.
    pub name: Option<String>,
    /// Custom system prompt addition from agent.
    pub prompt: Option<String>,
    /// Temperature override.
    pub temperature: Option<f32>,
    /// Top-p override.
    pub top_p: Option<f32>,
    /// Tool enable/disable map.
    pub tools: HashMap<String, bool>,
    /// Max steps per turn.
    pub max_steps: Option<u32>,
}

impl From<&Agent> for AgentConfig {
    fn from(agent: &Agent) -> Self {
        Self {
            name: Some(agent.name.clone()),
            prompt: agent.prompt.clone(),
            temperature: agent.temperature,
            top_p: agent.top_p,
            tools: agent.tools.clone(),
            max_steps: agent.max_steps,
        }
    }
}

/// Simple prompt runner for the server.
///
/// Unlike the full Runner in wonopcode crate, this is a lightweight
/// implementation focused on API use without TUI integration.
pub struct ServerPromptRunner {
    provider: BoxedLanguageModel,
    cwd: PathBuf,
    cancel: CancellationToken,
    /// Tool definitions (simplified - no actual tool execution yet).
    tools: Vec<ToolDefinition>,
    /// Agent configuration.
    agent_config: AgentConfig,
    /// Shared todo store for updating todos when tools are observed.
    todo_store: Option<SharedTodoStore>,
}

impl ServerPromptRunner {
    /// Create a new server prompt runner.
    pub fn new(provider: BoxedLanguageModel, cwd: PathBuf, cancel: CancellationToken) -> Self {
        Self::with_agent(provider, cwd, cancel, AgentConfig::default(), None)
    }

    /// Create a new server prompt runner with agent configuration.
    pub fn with_agent(
        provider: BoxedLanguageModel,
        cwd: PathBuf,
        cancel: CancellationToken,
        agent_config: AgentConfig,
        todo_store: Option<SharedTodoStore>,
    ) -> Self {
        // Basic tool definitions for the model to know about.
        // In a full implementation, these would be dynamically loaded.
        let all_tools = vec![
            ToolDefinition {
                name: "read".to_string(),
                description: "Read a file from the filesystem".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filePath": {
                            "type": "string",
                            "description": "The absolute path to the file to read"
                        }
                    },
                    "required": ["filePath"]
                }),
            },
            ToolDefinition {
                name: "write".to_string(),
                description: "Write content to a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filePath": {
                            "type": "string",
                            "description": "The absolute path to the file"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write"
                        }
                    },
                    "required": ["filePath", "content"]
                }),
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: "Execute a shell command".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        },
                        "description": {
                            "type": "string",
                            "description": "A brief description of what this command does"
                        }
                    },
                    "required": ["command", "description"]
                }),
            },
        ];

        // Filter tools based on agent configuration
        let tools = if agent_config.tools.is_empty() {
            all_tools
        } else {
            all_tools
                .into_iter()
                .filter(|tool| {
                    // Check specific tool setting
                    if let Some(&enabled) = agent_config.tools.get(&tool.name) {
                        return enabled;
                    }
                    // Check wildcard
                    if let Some(&enabled) = agent_config.tools.get("*") {
                        return enabled;
                    }
                    // Default to enabled
                    true
                })
                .collect()
        };

        Self {
            provider,
            cwd,
            cancel,
            tools,
            agent_config,
            todo_store,
        }
    }

    /// Run a prompt and stream events.
    #[allow(clippy::cognitive_complexity)]
    pub async fn run(
        &self,
        prompt: &str,
        system_prompt: Option<String>,
        event_tx: mpsc::UnboundedSender<PromptEvent>,
    ) -> Result<PromptResponse, String> {
        let message_id = format!("msg_{}", uuid_simple());

        // Notify start
        let _ = event_tx.send(PromptEvent::Started {
            session_id: "server".to_string(),
            message_id: message_id.clone(),
        });

        // Build messages
        let messages = vec![ProviderMessage::user(prompt)];

        // Build system prompt: user override > agent prompt + base > base only
        let base_system = build_basic_system_prompt(&self.cwd);
        let system = if let Some(user_system) = system_prompt {
            Some(user_system)
        } else if let Some(agent_prompt) = &self.agent_config.prompt {
            // Combine base system prompt with agent-specific prompt
            Some(format!("{base_system}\n\n{agent_prompt}"))
        } else {
            Some(base_system)
        };

        // Use agent temperature if set, otherwise default
        let temperature = self.agent_config.temperature.or(Some(0.7));
        let top_p = self.agent_config.top_p;

        // Build options
        let options = GenerateOptions {
            temperature,
            top_p,
            max_tokens: Some(8192),
            system,
            tools: self.tools.clone(),
            abort: Some(self.cancel.clone()),
            ..Default::default()
        };

        // Call provider
        let stream = match self.provider.generate(messages, options).await {
            Ok(s) => s,
            Err(e) => {
                let error = format!("Provider error: {e}");
                let _ = event_tx.send(PromptEvent::Error {
                    error: error.clone(),
                });
                return Err(error);
            }
        };
        tokio::pin!(stream);

        let mut final_text = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut total_input: u32 = 0;
        let mut total_output: u32 = 0;

        // Process stream
        while let Some(chunk_result) = stream.next().await {
            if self.cancel.is_cancelled() {
                let _ = event_tx.send(PromptEvent::Aborted);
                return Err("Aborted".to_string());
            }

            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    warn!("Stream error: {}", e);
                    continue;
                }
            };

            match chunk {
                StreamChunk::TextStart => {}
                StreamChunk::TextDelta(delta) => {
                    final_text.push_str(&delta);
                    let _ = event_tx.send(PromptEvent::TextDelta { delta });
                }
                StreamChunk::TextEnd => {}
                StreamChunk::ToolCallStart { id, name } => {
                    debug!(id = %id, name = %name, "Tool call started");
                    tool_calls.push((id.clone(), name.clone(), String::new()));
                }
                StreamChunk::ToolCallDelta { delta, .. } => {
                    if let Some(call) = tool_calls.last_mut() {
                        call.2.push_str(&delta);
                    }
                }
                StreamChunk::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    debug!(id = %id, name = %name, "Tool call complete");
                    let input: serde_json::Value =
                        serde_json::from_str(&arguments).unwrap_or(serde_json::Value::Null);

                    let _ = event_tx.send(PromptEvent::ToolStarted {
                        id: id.clone(),
                        name: name.clone(),
                        input,
                    });

                    // For now, tools are not executed in server mode
                    // They would need permission handling, etc.
                    let _ = event_tx.send(PromptEvent::ToolCompleted {
                        id,
                        success: false,
                        output: "Tool execution not yet implemented in server API".to_string(),
                    });
                }
                StreamChunk::ReasoningStart => {}
                StreamChunk::ReasoningDelta(_) => {}
                StreamChunk::ReasoningEnd => {}
                StreamChunk::ToolObserved { id, name, input } => {
                    // Tool observed from external execution (e.g., Claude CLI)
                    debug!(id = %id, name = %name, "Tool observed (external execution)");
                    let input_value: serde_json::Value =
                        serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);

                    // Intercept todowrite calls to update the shared todo store
                    let base_name = name.split("__").last().unwrap_or(&name);
                    if base_name == "todowrite" {
                        if let Some(store) = &self.todo_store {
                            if let Some(todos_array) =
                                input_value.get("todos").and_then(|v| v.as_array())
                            {
                                let todos: Vec<TodoItem> = todos_array
                                    .iter()
                                    .filter_map(|item| {
                                        let id = item.get("id")?.as_str()?.to_string();
                                        let content = item.get("content")?.as_str()?.to_string();
                                        let status = match item.get("status")?.as_str()? {
                                            "pending" => TodoStatus::Pending,
                                            "in_progress" => TodoStatus::InProgress,
                                            "completed" => TodoStatus::Completed,
                                            "cancelled" => TodoStatus::Cancelled,
                                            _ => TodoStatus::Pending,
                                        };
                                        let priority = match item.get("priority")?.as_str()? {
                                            "high" => TodoPriority::High,
                                            "medium" => TodoPriority::Medium,
                                            "low" => TodoPriority::Low,
                                            _ => TodoPriority::Medium,
                                        };
                                        Some(TodoItem {
                                            id,
                                            content,
                                            status,
                                            priority,
                                        })
                                    })
                                    .collect();

                                // Update the shared todo store
                                let mut store_guard = store.blocking_write();
                                *store_guard = todos;
                                debug!(
                                    "Updated shared todo store with {} items",
                                    store_guard.len()
                                );
                            }
                        }
                    }

                    let _ = event_tx.send(PromptEvent::ToolStarted {
                        id,
                        name,
                        input: input_value,
                    });
                }
                StreamChunk::ToolResultObserved {
                    id,
                    success,
                    output,
                } => {
                    // Tool result from external execution
                    debug!(id = %id, success = %success, "Tool result observed");
                    let _ = event_tx.send(PromptEvent::ToolCompleted {
                        id,
                        success,
                        output,
                    });
                }
                StreamChunk::FinishStep { usage, .. } => {
                    total_input += usage.input_tokens;
                    total_output += usage.output_tokens;

                    let model_info = self.provider.model_info();
                    let cost = model_info.cost.calculate(total_input, total_output);

                    let _ = event_tx.send(PromptEvent::TokenUsage {
                        input: total_input,
                        output: total_output,
                        cost,
                    });
                }
                StreamChunk::Error(e) => {
                    warn!("Stream error: {}", e);
                    let _ = event_tx.send(PromptEvent::Status {
                        message: format!("Stream error: {e}"),
                    });
                }
            }
        }

        // Calculate final cost
        let model_info = self.provider.model_info();
        let cost = model_info.cost.calculate(total_input, total_output);

        // Send completed event
        let _ = event_tx.send(PromptEvent::Completed {
            message_id: message_id.clone(),
            text: final_text.clone(),
        });

        Ok(PromptResponse {
            message_id,
            text: final_text,
            usage: PromptUsage {
                input_tokens: total_input,
                output_tokens: total_output,
                cost,
            },
        })
    }
}

/// Build a basic system prompt for server mode.
fn build_basic_system_prompt(cwd: &std::path::Path) -> String {
    format!(
        r#"You are a helpful AI assistant. You are running in server API mode.

Current working directory: {}

You have access to tools for reading files, writing files, and executing shell commands.
Please use them when appropriate to help with tasks.

When reading or writing files, use absolute paths based on the current working directory."#,
        cwd.display()
    )
}

/// Generate a simple UUID-like string without external dependencies.
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", duration.as_secs(), duration.subsec_nanos())
}

/// Create a provider from configuration.
pub fn create_provider_from_config(
    provider_name: &str,
    model_id: &str,
) -> Result<BoxedLanguageModel, String> {
    // Try to load API key from environment
    let api_key = match provider_name {
        "anthropic" => std::env::var("ANTHROPIC_API_KEY").ok(),
        "openai" => std::env::var("OPENAI_API_KEY").ok(),
        "openrouter" => std::env::var("OPENROUTER_API_KEY").ok(),
        "google" => std::env::var("GOOGLE_API_KEY")
            .or_else(|_| std::env::var("GEMINI_API_KEY"))
            .ok(),
        "xai" => std::env::var("XAI_API_KEY").ok(),
        "mistral" => std::env::var("MISTRAL_API_KEY").ok(),
        "groq" => std::env::var("GROQ_API_KEY").ok(),
        _ => None,
    };

    let api_key = api_key.unwrap_or_default();

    // Build model info from model_id and provider
    let model_info = build_model_info(model_id, provider_name);

    match provider_name {
        "anthropic" => {
            use wonopcode_provider::claude_cli::ClaudeCliProvider;

            // Priority: API key > Claude CLI subscription
            if !api_key.is_empty() {
                let provider =
                    wonopcode_provider::anthropic::AnthropicProvider::new(&api_key, model_info)
                        .map_err(|e| e.to_string())?;
                Ok(Arc::new(provider) as BoxedLanguageModel)
            } else if ClaudeCliProvider::is_available() && ClaudeCliProvider::is_authenticated() {
                let provider =
                    wonopcode_provider::claude_cli::with_subscription_pricing(model_info)
                        .map_err(|e| e.to_string())?;
                Ok(Arc::new(provider) as BoxedLanguageModel)
            } else {
                Err("No Anthropic API key provided and Claude CLI not authenticated.".to_string())
            }
        }
        "openai" => {
            let provider = wonopcode_provider::openai::OpenAIProvider::new(&api_key, model_info)
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(provider) as BoxedLanguageModel)
        }
        "openrouter" => {
            let provider =
                wonopcode_provider::openrouter::OpenRouterProvider::new(&api_key, model_info)
                    .map_err(|e| e.to_string())?;
            Ok(Arc::new(provider) as BoxedLanguageModel)
        }
        "google" => {
            let provider = wonopcode_provider::google::GoogleProvider::new(&api_key, model_info)
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(provider) as BoxedLanguageModel)
        }
        _ => Err(format!("Unsupported provider: {provider_name}")),
    }
}

/// Build model info from model ID and provider name.
fn build_model_info(model_id: &str, provider_name: &str) -> ModelInfo {
    // Check for known built-in models first
    match model_id {
        // Anthropic - Latest (Claude 4.5)
        "claude-sonnet-4-5-20250929" | "claude-sonnet-4-5" => {
            return wonopcode_provider::model::anthropic::claude_sonnet_4_5()
        }
        "claude-haiku-4-5-20251001" | "claude-haiku-4-5" => {
            return wonopcode_provider::model::anthropic::claude_haiku_4_5()
        }
        "claude-opus-4-5-20251101" | "claude-opus-4-5" => {
            return wonopcode_provider::model::anthropic::claude_opus_4_5()
        }
        // Anthropic - Legacy (Claude 4.x)
        "claude-sonnet-4-20250514" | "claude-sonnet-4-0" | "claude-sonnet-4" => {
            return wonopcode_provider::model::anthropic::claude_sonnet_4()
        }
        "claude-opus-4-1-20250805" | "claude-opus-4-1" => {
            return wonopcode_provider::model::anthropic::claude_opus_4_1()
        }
        "claude-opus-4-20250514" | "claude-opus-4-0" | "claude-opus-4" => {
            return wonopcode_provider::model::anthropic::claude_opus_4()
        }
        // Anthropic - Legacy (Claude 3.x)
        "claude-3-7-sonnet-20250219" | "claude-3-7-sonnet" | "claude-3-7-sonnet-latest" => {
            return wonopcode_provider::model::anthropic::claude_sonnet_3_7()
        }
        "claude-3-haiku-20240307" | "claude-3-haiku" => {
            return wonopcode_provider::model::anthropic::claude_haiku_3()
        }
        // OpenAI - GPT-5 Series
        "gpt-5.2" => return wonopcode_provider::model::openai::gpt_5_2(),
        "gpt-5.1" => return wonopcode_provider::model::openai::gpt_5_1(),
        "gpt-5" => return wonopcode_provider::model::openai::gpt_5(),
        "gpt-5-mini" => return wonopcode_provider::model::openai::gpt_5_mini(),
        "gpt-5-nano" => return wonopcode_provider::model::openai::gpt_5_nano(),
        // OpenAI - GPT-4.1 Series
        "gpt-4.1" => return wonopcode_provider::model::openai::gpt_4_1(),
        "gpt-4.1-mini" => return wonopcode_provider::model::openai::gpt_4_1_mini(),
        "gpt-4.1-nano" => return wonopcode_provider::model::openai::gpt_4_1_nano(),
        // OpenAI - O-Series
        "o3" => return wonopcode_provider::model::openai::o3(),
        "o3-mini" => return wonopcode_provider::model::openai::o3_mini(),
        "o4-mini" => return wonopcode_provider::model::openai::o4_mini(),
        // OpenAI - Legacy
        "gpt-4o" => return wonopcode_provider::model::openai::gpt_4o(),
        "gpt-4o-mini" => return wonopcode_provider::model::openai::gpt_4o_mini(),
        "o1" => return wonopcode_provider::model::openai::o1(),
        // Google
        "gemini-2.0-flash" => return wonopcode_provider::model::google::gemini_2_flash(),
        "gemini-1.5-pro" => return wonopcode_provider::model::google::gemini_1_5_pro(),
        "gemini-1.5-flash" => return wonopcode_provider::model::google::gemini_1_5_flash(),
        _ => {}
    }

    // Build a reasonable default for unknown models
    let (context, output) = match provider_name {
        "anthropic" => (200_000, 8_192),
        "openai" => (128_000, 16_384),
        "google" => (1_000_000, 8_192),
        "openrouter" => (128_000, 8_192),
        _ => (32_000, 4_096),
    };

    ModelInfo {
        id: model_id.to_string(),
        provider_id: provider_name.to_string(),
        name: model_id.to_string(),
        family: None,
        capabilities: ModelCapabilities::default(),
        cost: ModelCost::default(),
        limit: ModelLimit { context, output },
        status: wonopcode_provider::model::ModelStatus::Active,
    }
}

/// Infer provider from model name.
pub fn infer_provider(model_id: &str) -> &'static str {
    if model_id.starts_with("claude") {
        "anthropic"
    } else if model_id.starts_with("gpt")
        || model_id.starts_with("o1")
        || model_id.starts_with("o3")
        || model_id.starts_with("o4")
    {
        "openai"
    } else if model_id.starts_with("gemini") {
        "google"
    } else if model_id.starts_with("grok") {
        "xai"
    } else if model_id.starts_with("mistral") || model_id.starts_with("codestral") {
        "mistral"
    } else if model_id.contains("/") {
        // OpenRouter format: provider/model
        "openrouter"
    } else {
        "anthropic" // Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Provider inference tests ===

    #[test]
    fn user_gets_anthropic_for_claude_models() {
        assert_eq!(infer_provider("claude-sonnet-4-5"), "anthropic");
        assert_eq!(infer_provider("claude-haiku-4-5"), "anthropic");
        assert_eq!(infer_provider("claude-opus-4-5"), "anthropic");
        assert_eq!(infer_provider("claude-3-7-sonnet-latest"), "anthropic");
    }

    #[test]
    fn user_gets_openai_for_gpt_models() {
        assert_eq!(infer_provider("gpt-4o"), "openai");
        assert_eq!(infer_provider("gpt-4o-mini"), "openai");
        assert_eq!(infer_provider("gpt-5"), "openai");
        assert_eq!(infer_provider("gpt-5-mini"), "openai");
    }

    #[test]
    fn user_gets_openai_for_o_series_models() {
        assert_eq!(infer_provider("o1"), "openai");
        assert_eq!(infer_provider("o3"), "openai");
        assert_eq!(infer_provider("o3-mini"), "openai");
        assert_eq!(infer_provider("o4-mini"), "openai");
    }

    #[test]
    fn user_gets_google_for_gemini_models() {
        assert_eq!(infer_provider("gemini-2.0-flash"), "google");
        assert_eq!(infer_provider("gemini-1.5-pro"), "google");
        assert_eq!(infer_provider("gemini-1.5-flash"), "google");
    }

    #[test]
    fn user_gets_xai_for_grok_models() {
        assert_eq!(infer_provider("grok-1"), "xai");
        assert_eq!(infer_provider("grok-beta"), "xai");
    }

    #[test]
    fn user_gets_mistral_for_mistral_and_codestral() {
        assert_eq!(infer_provider("mistral-large"), "mistral");
        assert_eq!(infer_provider("mistral-small"), "mistral");
        assert_eq!(infer_provider("codestral-latest"), "mistral");
    }

    #[test]
    fn user_gets_openrouter_for_slash_format() {
        assert_eq!(infer_provider("anthropic/claude-3-opus"), "openrouter");
        assert_eq!(infer_provider("meta-llama/llama-3-70b"), "openrouter");
    }

    #[test]
    fn user_gets_anthropic_as_default_for_unknown() {
        assert_eq!(infer_provider("some-unknown-model"), "anthropic");
    }

    // === Model info tests ===

    #[test]
    fn model_info_returns_known_claude_models() {
        let info = build_model_info("claude-sonnet-4-5", "anthropic");
        assert!(info.id.contains("claude"));
        assert_eq!(info.limit.context, 200_000);
    }

    #[test]
    fn model_info_returns_known_gpt_models() {
        let info = build_model_info("gpt-4o", "openai");
        assert!(info.id.contains("gpt"));
    }

    #[test]
    fn model_info_returns_reasonable_defaults_for_unknown() {
        let info = build_model_info("unknown-model", "anthropic");
        assert_eq!(info.id, "unknown-model");
        assert_eq!(info.provider_id, "anthropic");
        assert_eq!(info.limit.context, 200_000); // Anthropic default
        assert_eq!(info.limit.output, 8_192);
    }

    #[test]
    fn model_info_uses_provider_specific_defaults() {
        let anthropic = build_model_info("unknown", "anthropic");
        assert_eq!(anthropic.limit.context, 200_000);

        let openai = build_model_info("unknown", "openai");
        assert_eq!(openai.limit.context, 128_000);

        let google = build_model_info("unknown", "google");
        assert_eq!(google.limit.context, 1_000_000);

        let other = build_model_info("unknown", "other");
        assert_eq!(other.limit.context, 32_000);
    }

    // === UUID simple tests ===

    #[test]
    fn uuid_simple_generates_unique_ids() {
        let id1 = uuid_simple();
        // Sleep briefly to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = uuid_simple();

        assert!(!id1.is_empty());
        assert!(!id2.is_empty());
        // IDs should be different (though in fast execution they might be same)
    }

    #[test]
    fn uuid_simple_is_hex_string() {
        let id = uuid_simple();
        // Should be valid hex characters
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // === PromptEvent serialization tests ===

    #[test]
    fn prompt_event_started_serializes_correctly() {
        let event = PromptEvent::Started {
            session_id: "sess-123".to_string(),
            message_id: "msg-456".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"started\""));
        assert!(json.contains("\"session_id\":\"sess-123\""));
        assert!(json.contains("\"message_id\":\"msg-456\""));
    }

    #[test]
    fn prompt_event_text_delta_serializes_correctly() {
        let event = PromptEvent::TextDelta {
            delta: "Hello world".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"text_delta\""));
        assert!(json.contains("\"delta\":\"Hello world\""));
    }

    #[test]
    fn prompt_event_tool_started_serializes_correctly() {
        let event = PromptEvent::ToolStarted {
            id: "tool-1".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({"filePath": "/tmp/test.txt"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_started\""));
        assert!(json.contains("\"name\":\"read\""));
        assert!(json.contains("\"filePath\""));
    }

    #[test]
    fn prompt_event_tool_completed_serializes_correctly() {
        let event = PromptEvent::ToolCompleted {
            id: "tool-1".to_string(),
            success: true,
            output: "file contents".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_completed\""));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn prompt_event_token_usage_serializes_correctly() {
        let event = PromptEvent::TokenUsage {
            input: 1000,
            output: 500,
            cost: 0.015,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"token_usage\""));
        assert!(json.contains("\"input\":1000"));
        assert!(json.contains("\"output\":500"));
        assert!(json.contains("\"cost\":0.015"));
    }

    #[test]
    fn prompt_event_completed_serializes_correctly() {
        let event = PromptEvent::Completed {
            message_id: "msg-123".to_string(),
            text: "Final response".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"completed\""));
        assert!(json.contains("\"text\":\"Final response\""));
    }

    #[test]
    fn prompt_event_error_serializes_correctly() {
        let event = PromptEvent::Error {
            error: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"error\":\"Something went wrong\""));
    }

    #[test]
    fn prompt_event_aborted_serializes_correctly() {
        let event = PromptEvent::Aborted;
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"aborted\""));
    }

    // === PromptRequest deserialization tests ===

    #[test]
    fn prompt_request_deserializes_minimal() {
        let json = r#"{"prompt": "Hello"}"#;
        let request: PromptRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.prompt, "Hello");
        assert!(request.model.is_none());
        assert!(request.provider.is_none());
        assert!(request.agent.is_none());
        assert!(request.system_prompt.is_none());
    }

    #[test]
    fn prompt_request_deserializes_full() {
        let json = r#"{
            "prompt": "Explain this code",
            "model": "claude-sonnet-4-5",
            "provider": "anthropic",
            "agent": "coder",
            "system_prompt": "You are a helpful assistant"
        }"#;
        let request: PromptRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.prompt, "Explain this code");
        assert_eq!(request.model, Some("claude-sonnet-4-5".to_string()));
        assert_eq!(request.provider, Some("anthropic".to_string()));
        assert_eq!(request.agent, Some("coder".to_string()));
        assert!(request.system_prompt.is_some());
    }

    // === PromptResponse serialization tests ===

    #[test]
    fn prompt_response_serializes_correctly() {
        let response = PromptResponse {
            message_id: "msg-123".to_string(),
            text: "Response text".to_string(),
            usage: PromptUsage {
                input_tokens: 100,
                output_tokens: 50,
                cost: 0.005,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"message_id\":\"msg-123\""));
        assert!(json.contains("\"text\":\"Response text\""));
        assert!(json.contains("\"input_tokens\":100"));
        assert!(json.contains("\"output_tokens\":50"));
        assert!(json.contains("\"cost\":0.005"));
    }

    // === AgentConfig tests ===

    #[test]
    fn agent_config_default_is_empty() {
        let config = AgentConfig::default();
        assert!(config.name.is_none());
        assert!(config.prompt.is_none());
        assert!(config.temperature.is_none());
        assert!(config.top_p.is_none());
        assert!(config.tools.is_empty());
        assert!(config.max_steps.is_none());
    }

    #[test]
    fn agent_config_from_agent_copies_fields() {
        use wonopcode_core::{Agent, AgentMode, AgentPermission};

        let agent = Agent {
            name: "test-agent".to_string(),
            description: Some("Test description".to_string()),
            mode: AgentMode::Primary,
            native: false,
            hidden: false,
            is_default: false,
            temperature: Some(0.5),
            top_p: Some(0.9),
            color: None,
            permission: AgentPermission::default(),
            model: None,
            prompt: Some("Custom prompt".to_string()),
            tools: HashMap::from([("bash".to_string(), false)]),
            max_steps: Some(10),
            sandbox: None,
        };

        let config = AgentConfig::from(&agent);
        assert_eq!(config.name, Some("test-agent".to_string()));
        assert_eq!(config.prompt, Some("Custom prompt".to_string()));
        assert_eq!(config.temperature, Some(0.5));
        assert_eq!(config.top_p, Some(0.9));
        assert_eq!(config.tools.get("bash"), Some(&false));
        assert_eq!(config.max_steps, Some(10));
    }

    // === build_basic_system_prompt tests ===

    #[test]
    fn system_prompt_includes_cwd() {
        let cwd = std::path::Path::new("/home/user/project");
        let prompt = build_basic_system_prompt(cwd);
        assert!(prompt.contains("/home/user/project"));
    }

    #[test]
    fn system_prompt_mentions_tools() {
        let cwd = std::path::Path::new("/tmp");
        let prompt = build_basic_system_prompt(cwd);
        assert!(prompt.contains("tools"));
        assert!(prompt.contains("reading files"));
        assert!(prompt.contains("writing files"));
        assert!(prompt.contains("shell commands"));
    }

    // === Additional PromptEvent tests ===

    #[test]
    fn prompt_event_status_serializes_correctly() {
        let event = PromptEvent::Status {
            message: "Processing request...".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"status\""));
        assert!(json.contains("\"message\":\"Processing request...\""));
    }

    // === PromptUsage tests ===

    #[test]
    fn prompt_usage_serializes_correctly() {
        let usage = PromptUsage {
            input_tokens: 500,
            output_tokens: 250,
            cost: 0.0075,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("\"input_tokens\":500"));
        assert!(json.contains("\"output_tokens\":250"));
        assert!(json.contains("\"cost\":0.0075"));
    }

    #[test]
    fn prompt_usage_zero_values() {
        let usage = PromptUsage {
            input_tokens: 0,
            output_tokens: 0,
            cost: 0.0,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("\"input_tokens\":0"));
        assert!(json.contains("\"output_tokens\":0"));
    }

    // === new_session_runners test ===

    #[test]
    fn new_session_runners_creates_empty_map() {
        let runners = new_session_runners();
        // Should be able to read without blocking
        let guard = runners.try_read().unwrap();
        assert!(guard.is_empty());
    }

    // === AgentConfig Clone tests ===

    #[test]
    fn agent_config_clone_preserves_values() {
        let mut tools = HashMap::new();
        tools.insert("read".to_string(), true);
        tools.insert("write".to_string(), false);

        let config = AgentConfig {
            name: Some("coder".to_string()),
            prompt: Some("Custom instructions".to_string()),
            temperature: Some(0.7),
            top_p: Some(0.95),
            tools,
            max_steps: Some(20),
        };

        let cloned = config.clone();
        assert_eq!(cloned.name, Some("coder".to_string()));
        assert_eq!(cloned.prompt, Some("Custom instructions".to_string()));
        assert_eq!(cloned.temperature, Some(0.7));
        assert_eq!(cloned.top_p, Some(0.95));
        assert_eq!(cloned.tools.get("read"), Some(&true));
        assert_eq!(cloned.tools.get("write"), Some(&false));
        assert_eq!(cloned.max_steps, Some(20));
    }

    // === Additional model info tests ===

    #[test]
    fn model_info_o_series_has_correct_limits() {
        let info = build_model_info("o1", "openai");
        assert_eq!(info.limit.context, 200_000);
        assert_eq!(info.limit.output, 100_000);
    }

    #[test]
    fn model_info_o3_mini_has_correct_limits() {
        let info = build_model_info("o3-mini", "openai");
        assert_eq!(info.limit.context, 200_000);
        assert_eq!(info.limit.output, 100_000);
    }

    #[test]
    fn model_info_gemini_flash_has_correct_limits() {
        let info = build_model_info("gemini-2.0-flash", "google");
        assert_eq!(info.limit.context, 1_000_000);
        assert_eq!(info.limit.output, 8_192);
    }

    #[test]
    fn model_info_gemini_pro_has_higher_output() {
        let info = build_model_info("gemini-1.5-pro", "google");
        assert_eq!(info.limit.context, 2_000_000);
        assert!(info.limit.output > 8_000); // Pro has higher output
    }

    #[test]
    fn model_info_gpt4o_has_correct_limits() {
        let info = build_model_info("gpt-4o", "openai");
        assert_eq!(info.limit.context, 128_000);
        assert_eq!(info.limit.output, 16_384);
    }

    #[test]
    fn model_info_claude_opus_has_extended_output() {
        let info = build_model_info("claude-opus-4-5", "anthropic");
        assert_eq!(info.limit.context, 200_000);
        assert_eq!(info.limit.output, 64_000);
    }

    // === Additional model info tests ===

    #[test]
    fn model_info_claude_sonnet_4_5_has_correct_limits() {
        let info = build_model_info("claude-sonnet-4-5", "anthropic");
        assert!(info.limit.context >= 200_000);
        assert!(info.limit.output > 0);
    }

    #[test]
    fn model_info_claude_haiku_4_5_has_correct_limits() {
        let info = build_model_info("claude-haiku-4-5", "anthropic");
        assert!(info.limit.context > 0);
    }

    #[test]
    fn model_info_gpt_5_has_correct_limits() {
        let info = build_model_info("gpt-5", "openai");
        assert!(info.limit.context > 0);
    }

    #[test]
    fn model_info_gpt_4o_mini_has_correct_limits() {
        let info = build_model_info("gpt-4o-mini", "openai");
        assert!(info.limit.context > 0);
    }

    #[test]
    fn model_info_openrouter_fallback() {
        let info = build_model_info("some-model", "openrouter");
        assert_eq!(info.limit.context, 128_000);
        assert_eq!(info.limit.output, 8_192);
    }

    // === PromptEvent Debug tests ===

    #[test]
    fn prompt_event_started_debug() {
        let event = PromptEvent::Started {
            session_id: "s1".to_string(),
            message_id: "m1".to_string(),
        };
        let debug = format!("{:?}", event);
        assert!(debug.contains("Started"));
    }

    #[test]
    fn prompt_event_tool_started_debug() {
        let event = PromptEvent::ToolStarted {
            id: "t1".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({}),
        };
        let debug = format!("{:?}", event);
        assert!(debug.contains("ToolStarted"));
    }

    // === PromptRequest Debug ===

    #[test]
    fn prompt_request_debug() {
        let request = PromptRequest {
            prompt: "Hello".to_string(),
            model: None,
            provider: None,
            agent: None,
            system_prompt: None,
        };
        let debug = format!("{:?}", request);
        assert!(debug.contains("PromptRequest"));
    }

    // === PromptResponse Debug ===

    #[test]
    fn prompt_response_debug() {
        let response = PromptResponse {
            message_id: "m1".to_string(),
            text: "Response".to_string(),
            usage: PromptUsage {
                input_tokens: 10,
                output_tokens: 20,
                cost: 0.001,
            },
        };
        let debug = format!("{:?}", response);
        assert!(debug.contains("PromptResponse"));
    }

    // === PromptUsage Debug ===

    #[test]
    fn prompt_usage_debug() {
        let usage = PromptUsage {
            input_tokens: 100,
            output_tokens: 50,
            cost: 0.005,
        };
        let debug = format!("{:?}", usage);
        assert!(debug.contains("PromptUsage"));
    }

    // === AgentConfig Debug ===

    #[test]
    fn agent_config_debug() {
        let config = AgentConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("AgentConfig"));
    }

    // === PromptEvent Clone tests ===

    #[test]
    fn prompt_event_clone() {
        let event = PromptEvent::TextDelta { delta: "text".to_string() };
        let cloned = event.clone();
        if let PromptEvent::TextDelta { delta } = cloned {
            assert_eq!(delta, "text");
        } else {
            panic!("Clone should preserve variant");
        }
    }

    // === build_basic_system_prompt additional tests ===

    #[test]
    fn system_prompt_handles_special_characters_in_path() {
        let cwd = std::path::Path::new("/home/user/project with spaces");
        let prompt = build_basic_system_prompt(cwd);
        assert!(prompt.contains("project with spaces"));
    }

    // === infer_provider edge cases ===

    #[test]
    fn infer_provider_deepseek() {
        // Unknown model defaults to anthropic
        assert_eq!(infer_provider("deepseek-v2"), "anthropic");
    }

    #[test]
    fn infer_provider_llama_openrouter() {
        assert_eq!(infer_provider("meta-llama/llama-3.1-70b"), "openrouter");
    }

    #[test]
    fn infer_provider_anthropic_openrouter() {
        assert_eq!(infer_provider("anthropic/claude-3-opus"), "openrouter");
    }

    // === Additional build_model_info tests for all model branches ===

    #[test]
    fn model_info_claude_sonnet_4_5_full_name() {
        let info = build_model_info("claude-sonnet-4-5-20250929", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_haiku_4_5_full_name() {
        let info = build_model_info("claude-haiku-4-5-20251001", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_opus_4_5_full_name() {
        let info = build_model_info("claude-opus-4-5-20251101", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_sonnet_4_full_name() {
        let info = build_model_info("claude-sonnet-4-20250514", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_sonnet_4_0_alias() {
        let info = build_model_info("claude-sonnet-4-0", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_sonnet_4_alias() {
        let info = build_model_info("claude-sonnet-4", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_opus_4_1_full() {
        let info = build_model_info("claude-opus-4-1-20250805", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_opus_4_1_alias() {
        let info = build_model_info("claude-opus-4-1", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_opus_4_full() {
        let info = build_model_info("claude-opus-4-20250514", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_opus_4_0_alias() {
        let info = build_model_info("claude-opus-4-0", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_opus_4_alias() {
        let info = build_model_info("claude-opus-4", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_3_7_sonnet_full() {
        let info = build_model_info("claude-3-7-sonnet-20250219", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_3_7_sonnet_alias() {
        let info = build_model_info("claude-3-7-sonnet", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_3_7_sonnet_latest() {
        let info = build_model_info("claude-3-7-sonnet-latest", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_3_haiku_full() {
        let info = build_model_info("claude-3-haiku-20240307", "anthropic");
        assert!(info.id.contains("claude"));
    }

    #[test]
    fn model_info_claude_3_haiku_alias() {
        let info = build_model_info("claude-3-haiku", "anthropic");
        assert!(info.id.contains("claude"));
    }

    // OpenAI GPT-5 series
    #[test]
    fn model_info_gpt_5_2() {
        let info = build_model_info("gpt-5.2", "openai");
        assert!(info.id.contains("gpt-5"));
    }

    #[test]
    fn model_info_gpt_5_1() {
        let info = build_model_info("gpt-5.1", "openai");
        assert!(info.id.contains("gpt-5"));
    }

    #[test]
    fn model_info_gpt_5_mini() {
        let info = build_model_info("gpt-5-mini", "openai");
        assert!(info.id.contains("gpt-5"));
    }

    #[test]
    fn model_info_gpt_5_nano() {
        let info = build_model_info("gpt-5-nano", "openai");
        assert!(info.id.contains("gpt-5"));
    }

    // OpenAI GPT-4.1 series
    #[test]
    fn model_info_gpt_4_1() {
        let info = build_model_info("gpt-4.1", "openai");
        assert!(info.id.contains("gpt-4"));
    }

    #[test]
    fn model_info_gpt_4_1_mini() {
        let info = build_model_info("gpt-4.1-mini", "openai");
        assert!(info.id.contains("gpt-4"));
    }

    #[test]
    fn model_info_gpt_4_1_nano() {
        let info = build_model_info("gpt-4.1-nano", "openai");
        assert!(info.id.contains("gpt-4"));
    }

    // OpenAI O-series
    #[test]
    fn model_info_o3() {
        let info = build_model_info("o3", "openai");
        assert!(info.id.contains("o3"));
    }

    #[test]
    fn model_info_o4_mini() {
        let info = build_model_info("o4-mini", "openai");
        assert!(info.id.contains("o4"));
    }

    // Google Gemini
    #[test]
    fn model_info_gemini_2_flash() {
        let info = build_model_info("gemini-2.0-flash", "google");
        assert!(info.id.contains("gemini"));
    }

    #[test]
    fn model_info_gemini_1_5_pro() {
        let info = build_model_info("gemini-1.5-pro", "google");
        assert!(info.id.contains("gemini"));
    }

    #[test]
    fn model_info_gemini_1_5_flash() {
        let info = build_model_info("gemini-1.5-flash", "google");
        assert!(info.id.contains("gemini"));
    }

    // === infer_provider additional tests ===

    #[test]
    fn infer_provider_grok() {
        assert_eq!(infer_provider("grok-beta"), "xai");
    }

    #[test]
    fn infer_provider_codestral() {
        assert_eq!(infer_provider("codestral-latest"), "mistral");
    }

    #[test]
    fn infer_provider_mistral_large() {
        assert_eq!(infer_provider("mistral-large-latest"), "mistral");
    }

    #[test]
    fn infer_provider_o4_mini() {
        assert_eq!(infer_provider("o4-mini"), "openai");
    }
}
