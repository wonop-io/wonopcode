//! Prompt handling for the server API.
//!
//! This module provides the ability to run prompts via HTTP with SSE streaming.

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
}

impl ServerPromptRunner {
    /// Create a new server prompt runner.
    pub fn new(provider: BoxedLanguageModel, cwd: PathBuf, cancel: CancellationToken) -> Self {
        Self::with_agent(provider, cwd, cancel, AgentConfig::default())
    }

    /// Create a new server prompt runner with agent configuration.
    pub fn with_agent(
        provider: BoxedLanguageModel,
        cwd: PathBuf,
        cancel: CancellationToken,
        agent_config: AgentConfig,
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
        }
    }

    /// Run a prompt and stream events.
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
            Some(format!("{}\n\n{}", base_system, agent_prompt))
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
                let error = format!("Provider error: {}", e);
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
                        message: format!("Stream error: {}", e),
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
        _ => Err(format!("Unsupported provider: {}", provider_name)),
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
