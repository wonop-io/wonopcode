//! Prompt processor for ACP.
//!
//! Handles the actual AI prompt processing using wonopcode's provider and tool systems.

use crate::transport::Connection;
use crate::types::*;
use futures::StreamExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use wonopcode_core::Instance;
use wonopcode_provider::{
    model::ModelInfo, stream::StreamChunk, BoxedLanguageModel, GenerateOptions,
    Message as ProviderMessage, ToolDefinition,
};
use wonopcode_tools::ToolRegistry;

/// Processor configuration.
#[derive(Debug, Clone)]
pub struct ProcessorConfig {
    pub provider: String,
    pub model_id: String,
    pub api_key: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model_id: "claude-sonnet-4-5-20250929".to_string(),
            api_key: String::new(),
            max_tokens: Some(8192),
            temperature: Some(0.7),
        }
    }
}

/// Prompt processor for ACP sessions.
pub struct Processor {
    config: ProcessorConfig,
    instance: Instance,
    provider: Arc<RwLock<BoxedLanguageModel>>,
    tools: Arc<ToolRegistry>,
    history: RwLock<Vec<ProviderMessage>>,
    /// Cancellation token for the current operation.
    cancel_token: RwLock<Option<CancellationToken>>,
}

impl Processor {
    /// Create a new processor.
    pub async fn new(
        config: ProcessorConfig,
        cwd: &Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create instance
        let instance = Instance::new(cwd).await?;

        // Create provider
        let provider = create_provider(&config)?;

        // Create in-memory todo store
        let todo_store = Arc::new(wonopcode_tools::todo::InMemoryTodoStore::new());

        // Create tool registry
        let mut tools = ToolRegistry::with_builtins();
        tools.register(Arc::new(wonopcode_tools::bash::BashTool));
        tools.register(Arc::new(wonopcode_tools::webfetch::WebFetchTool));
        tools.register(Arc::new(wonopcode_tools::todo::TodoWriteTool::new(
            todo_store.clone(),
        )));
        tools.register(Arc::new(wonopcode_tools::todo::TodoReadTool::new(
            todo_store,
        )));

        Ok(Self {
            config,
            instance,
            provider: Arc::new(RwLock::new(provider)),
            tools: Arc::new(tools),
            history: RwLock::new(Vec::new()),
            cancel_token: RwLock::new(None),
        })
    }

    /// Cancel the current operation if one is running.
    pub async fn cancel(&self) {
        let token = self.cancel_token.read().await;
        if let Some(ref cancel_token) = *token {
            info!("Cancelling current operation");
            cancel_token.cancel();
        }
    }

    /// Check if an operation is currently running.
    pub async fn is_running(&self) -> bool {
        let token = self.cancel_token.read().await;
        token.as_ref().map(|t| !t.is_cancelled()).unwrap_or(false)
    }

    /// Process a prompt and stream responses via the connection.
    #[allow(clippy::cognitive_complexity)]
    pub async fn process_prompt(
        &self,
        session_id: &str,
        prompt: &str,
        connection: &Connection,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cwd = self.instance.directory();

        // Create a new cancellation token for this operation
        let cancel_token = CancellationToken::new();
        {
            let mut token = self.cancel_token.write().await;
            *token = Some(cancel_token.clone());
        }

        // Build system prompt
        let environment = build_environment_info(cwd);
        let system_prompt = wonopcode_core::system_prompt::build_system_prompt(
            &self.config.provider,
            &self.config.model_id,
            None, // agent_prompt
            None, // custom_instructions
            &environment,
        );

        // Add user message to history
        {
            let mut history = self.history.write().await;
            history.push(ProviderMessage::user(prompt));
        }

        // Get tool definitions
        let tool_defs: Vec<ToolDefinition> = self
            .tools
            .all()
            .map(|t| ToolDefinition {
                name: t.id().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
            })
            .collect();

        // Build options with the stored cancellation token
        let options = GenerateOptions {
            system: Some(system_prompt),
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            tools: tool_defs,
            abort: Some(cancel_token.clone()),
            ..Default::default()
        };

        // Get messages for generation
        let messages = self.history.read().await.clone();

        // Start generation
        let provider = self.provider.read().await;
        let mut stream = provider.generate(messages, options).await?;

        let mut response_text = String::new();
        let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut current_tool_id: Option<String> = None;
        let mut current_tool_name: Option<String> = None;
        let mut current_tool_args = String::new();

        // Process stream
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::TextDelta(delta)) => {
                    response_text.push_str(&delta);

                    // Send text chunk
                    let _ = connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::AgentMessageChunk {
                                content: TextContent::new(&delta),
                            },
                        })
                        .await;
                }
                Ok(StreamChunk::ToolCallStart { id, name }) => {
                    current_tool_id = Some(id);
                    current_tool_name = Some(name);
                    current_tool_args.clear();
                }
                Ok(StreamChunk::ToolCallDelta { delta, .. }) => {
                    current_tool_args.push_str(&delta);
                }
                Ok(StreamChunk::ToolCall {
                    id,
                    name,
                    arguments,
                }) => {
                    // Parse arguments
                    let args: serde_json::Value =
                        serde_json::from_str(&arguments).unwrap_or(serde_json::json!({}));

                    // Send tool call started
                    let _ = connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::ToolCall {
                                tool_call_id: id.clone(),
                                title: name.clone(),
                                kind: ToolKind::from_tool_name(&name),
                                status: ToolStatus::Pending,
                                locations: Location::from_tool_input(&name, &args),
                                raw_input: args.clone(),
                            },
                        })
                        .await;

                    tool_calls.push((id, name, args));
                }
                Ok(StreamChunk::ReasoningDelta(text)) => {
                    // Send reasoning chunk
                    let _ = connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::AgentThoughtChunk {
                                content: TextContent::new(&text),
                            },
                        })
                        .await;
                }
                Ok(StreamChunk::FinishStep { .. }) => {
                    // If we have a pending tool call from streaming, finalize it
                    if let (Some(id), Some(name)) =
                        (current_tool_id.take(), current_tool_name.take())
                    {
                        let args: serde_json::Value = serde_json::from_str(&current_tool_args)
                            .unwrap_or(serde_json::json!({}));

                        let _ = connection
                            .session_update(SessionUpdateNotification {
                                session_id: session_id.to_string(),
                                update: SessionUpdate::ToolCall {
                                    tool_call_id: id.clone(),
                                    title: name.clone(),
                                    kind: ToolKind::from_tool_name(&name),
                                    status: ToolStatus::Pending,
                                    locations: Location::from_tool_input(&name, &args),
                                    raw_input: args.clone(),
                                },
                            })
                            .await;

                        tool_calls.push((id, name, args));
                        current_tool_args.clear();
                    }
                }
                Ok(StreamChunk::Error(e)) => {
                    error!("Stream error: {}", e);
                    break;
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        // Execute tool calls
        for (call_id, name, args) in tool_calls {
            // Update status to in_progress
            let _ = connection
                .session_update(SessionUpdateNotification {
                    session_id: session_id.to_string(),
                    update: SessionUpdate::ToolCallUpdate {
                        tool_call_id: call_id.clone(),
                        status: ToolStatus::InProgress,
                        kind: Some(ToolKind::from_tool_name(&name)),
                        title: None,
                        locations: Some(Location::from_tool_input(&name, &args)),
                        raw_input: Some(args.clone()),
                        raw_output: None,
                        content: None,
                    },
                })
                .await;

            // Execute tool
            let result = self.execute_tool(&name, &args, cwd).await;

            // Send completion
            let (status, content, raw_output) = match result {
                Ok(output) => (
                    ToolStatus::Completed,
                    vec![ToolCallContent::Content {
                        content: TextContent::new(&output),
                    }],
                    serde_json::json!({ "output": output }),
                ),
                Err(e) => (
                    ToolStatus::Failed,
                    vec![ToolCallContent::Content {
                        content: TextContent::new(e.to_string()),
                    }],
                    serde_json::json!({ "error": e.to_string() }),
                ),
            };

            let _ = connection
                .session_update(SessionUpdateNotification {
                    session_id: session_id.to_string(),
                    update: SessionUpdate::ToolCallUpdate {
                        tool_call_id: call_id,
                        status,
                        kind: Some(ToolKind::from_tool_name(&name)),
                        title: Some(name),
                        locations: None,
                        raw_input: None,
                        raw_output: Some(raw_output),
                        content: Some(content),
                    },
                })
                .await;
        }

        // Add assistant response to history
        if !response_text.is_empty() {
            let mut history = self.history.write().await;
            history.push(ProviderMessage::assistant(&response_text));
        }

        // Clear the cancellation token as we're done
        {
            let mut token = self.cancel_token.write().await;
            *token = None;
        }

        Ok(())
    }

    /// Execute a tool.
    async fn execute_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
        cwd: &Path,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("Tool not found: {name}"))?;

        let ctx = wonopcode_tools::ToolContext {
            session_id: "acp".to_string(),
            message_id: "acp".to_string(),
            agent: "acp".to_string(),
            abort: CancellationToken::new(),
            root_dir: cwd.to_path_buf(),
            cwd: cwd.to_path_buf(),
            snapshot: None,
            file_time: None,
            sandbox: None, // ACP tools run without sandbox for now
            event_tx: None,
        };

        let _timing = wonopcode_util::TimingGuard::tool(tool.id());
        let result = tool.execute(args.clone(), &ctx).await?;
        Ok(result.output)
    }

    /// Change the model.
    pub async fn change_model(
        &self,
        provider_name: &str,
        model_id: &str,
        api_key: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get the current CLI session ID before recreating the provider
        // This preserves Claude CLI session persistence when changing models within same provider
        let old_provider_id = {
            let provider = self.provider.read().await;
            provider.provider_id().to_string()
        };
        let cli_session_id = if old_provider_id == "anthropic-cli" && provider_name == "anthropic" {
            // Staying with Claude CLI provider - preserve session
            let provider = self.provider.read().await;
            provider.get_cli_session_id().await
        } else {
            // Changing providers - start fresh session
            None
        };

        let new_config = ProcessorConfig {
            provider: provider_name.to_string(),
            model_id: model_id.to_string(),
            api_key: api_key.to_string(),
            ..self.config.clone()
        };

        let new_provider = create_provider(&new_config)?;

        // Restore CLI session ID if we preserved it
        if cli_session_id.is_some() {
            new_provider
                .set_cli_session_id(cli_session_id.clone())
                .await;
            debug!(cli_session_id = ?cli_session_id, "Restored CLI session ID after model change");
        }

        let mut provider = self.provider.write().await;
        *provider = new_provider;

        info!(provider = %provider_name, model = %model_id, cli_session_id = ?cli_session_id, "Model changed");
        Ok(())
    }

    /// Clear history.
    pub async fn clear_history(&self) {
        let mut history = self.history.write().await;
        history.clear();
    }

    /// Compact history by pruning old tool outputs.
    /// This implements a simple compaction strategy:
    /// - Keep the last N user turns (default 4)
    /// - Prune tool outputs from older messages
    pub async fn compact(&self) -> CompactionResult {
        let mut history = self.history.write().await;

        if history.is_empty() {
            return CompactionResult {
                messages_before: 0,
                messages_after: 0,
                tokens_saved_estimate: 0,
            };
        }

        let messages_before = history.len();
        let preserve_turns = 4; // Keep last 4 user turns

        // Count user turns from the end
        let mut user_turn_count = 0;
        let mut prune_before_index = 0;

        for (i, msg) in history.iter().enumerate().rev() {
            if msg.role == wonopcode_provider::Role::User {
                user_turn_count += 1;
                if user_turn_count >= preserve_turns {
                    prune_before_index = i;
                    break;
                }
            }
        }

        let mut tokens_saved_estimate = 0u32;

        // Prune tool outputs from messages before prune_before_index
        for (i, msg) in history.iter_mut().enumerate() {
            if i >= prune_before_index {
                break;
            }

            // For assistant messages, check for tool results
            if msg.role == wonopcode_provider::Role::Assistant {
                for content in msg.content.iter_mut() {
                    if let wonopcode_provider::ContentPart::ToolResult {
                        content: tool_content,
                        ..
                    } = content
                    {
                        // Estimate tokens (rough: 4 chars per token)
                        let old_len = tool_content.len();
                        if old_len > 100 {
                            // Only prune if significant content
                            tokens_saved_estimate += (old_len / 4) as u32;
                            *tool_content = "[compacted]".to_string();
                        }
                    }
                }
            }
        }

        let messages_after = history.len();

        CompactionResult {
            messages_before,
            messages_after,
            tokens_saved_estimate,
        }
    }

    /// Get context stats.
    pub async fn context_stats(&self) -> ContextStats {
        let history = self.history.read().await;
        let message_count = history.len();

        // Rough token estimate (4 chars per token)
        let mut char_count = 0;
        for msg in history.iter() {
            for content in &msg.content {
                match content {
                    wonopcode_provider::ContentPart::Text { text } => char_count += text.len(),
                    wonopcode_provider::ContentPart::ToolResult {
                        content: tool_content,
                        ..
                    } => char_count += tool_content.len(),
                    wonopcode_provider::ContentPart::ToolUse { input, .. } => {
                        char_count += input.to_string().len()
                    }
                    _ => {}
                }
            }
        }

        let estimated_tokens = (char_count / 4) as u32;
        let provider = self.provider.read().await;
        let context_limit = provider.model_info().limit.context;

        ContextStats {
            message_count,
            estimated_tokens,
            context_limit,
        }
    }

    /// Get message history for replay.
    /// Returns a list of (role, text) pairs.
    pub async fn get_history(&self) -> Vec<(String, String)> {
        let history = self.history.read().await;
        history
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    wonopcode_provider::Role::User => "user".to_string(),
                    wonopcode_provider::Role::Assistant => "assistant".to_string(),
                    wonopcode_provider::Role::System => "system".to_string(),
                    wonopcode_provider::Role::Tool => "tool".to_string(),
                };
                let text = msg.text();
                (role, text)
            })
            .collect()
    }

    /// Restore history from a list of (role, text) pairs.
    pub async fn restore_history(&self, messages: Vec<(String, String)>) {
        let mut history = self.history.write().await;
        history.clear();

        for (role, text) in messages {
            let msg = match role.as_str() {
                "user" => ProviderMessage::user(&text),
                "assistant" => ProviderMessage::assistant(&text),
                "system" => ProviderMessage::system(&text),
                _ => continue, // Skip unknown roles
            };
            history.push(msg);
        }
    }
}

/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages_before: usize,
    pub messages_after: usize,
    pub tokens_saved_estimate: u32,
}

/// Context statistics.
#[derive(Debug, Clone)]
pub struct ContextStats {
    pub message_count: usize,
    pub estimated_tokens: u32,
    pub context_limit: u32,
}

/// Build environment info string for the system prompt.
fn build_environment_info(cwd: &Path) -> String {
    let platform = std::env::consts::OS;
    let today = chrono::Local::now().format("%a %b %d %Y").to_string();

    format!(
        r#"<env>
  Working directory: {}
  Is directory a git repo: {}
  Platform: {}
  Today's date: {}
</env>"#,
        cwd.display(),
        cwd.join(".git").exists(),
        platform,
        today
    )
}

/// Create a provider from config.
fn create_provider(
    config: &ProcessorConfig,
) -> Result<BoxedLanguageModel, Box<dyn std::error::Error + Send + Sync>> {
    use wonopcode_provider::anthropic::AnthropicProvider;
    use wonopcode_provider::claude_cli::ClaudeCliProvider;
    use wonopcode_provider::google::GoogleProvider;
    use wonopcode_provider::openai::OpenAIProvider;
    use wonopcode_provider::openrouter::OpenRouterProvider;
    use wonopcode_provider::{deepinfra, groq, mistral, together, xai};

    let model_info = get_model_info(&config.model_id, &config.provider);

    let provider: BoxedLanguageModel = match config.provider.as_str() {
        "anthropic" => {
            // Priority: API key > Claude CLI subscription
            if !config.api_key.is_empty() {
                Arc::new(AnthropicProvider::new(&config.api_key, model_info)?)
            } else if ClaudeCliProvider::is_available() && ClaudeCliProvider::is_authenticated() {
                Arc::new(wonopcode_provider::claude_cli::with_subscription_pricing(
                    model_info,
                )?)
            } else {
                return Err(
                    "No Anthropic API key provided and Claude CLI not authenticated.".into(),
                );
            }
        }
        "openai" => Arc::new(OpenAIProvider::new(&config.api_key, model_info)?),
        "google" | "gemini" => Arc::new(GoogleProvider::new(&config.api_key, model_info)?),
        "openrouter" => Arc::new(OpenRouterProvider::new(&config.api_key, model_info)?),
        "xai" => Arc::new(xai::XaiProvider::new(&config.api_key, model_info)?),
        "mistral" => Arc::new(mistral::MistralProvider::new(&config.api_key, model_info)?),
        "groq" => Arc::new(groq::GroqProvider::new(&config.api_key, model_info)?),
        "deepinfra" => Arc::new(deepinfra::DeepInfraProvider::new(
            &config.api_key,
            model_info,
        )?),
        "together" => Arc::new(together::TogetherProvider::new(
            &config.api_key,
            model_info,
        )?),
        _ => {
            return Err(format!("Unknown provider: {}", config.provider).into());
        }
    };

    Ok(provider)
}

/// Get model info for a model ID.
fn get_model_info(model_id: &str, provider: &str) -> ModelInfo {
    use wonopcode_provider::{deepinfra, groq, mistral, together, xai};

    // Check built-in models
    match model_id {
        // Anthropic - Latest (Claude 4.5)
        "claude-sonnet-4-5-20250929" | "claude-sonnet-4-5" => {
            wonopcode_provider::model::anthropic::claude_sonnet_4_5()
        }
        "claude-haiku-4-5-20251001" | "claude-haiku-4-5" => {
            wonopcode_provider::model::anthropic::claude_haiku_4_5()
        }
        "claude-opus-4-5-20251101" | "claude-opus-4-5" => {
            wonopcode_provider::model::anthropic::claude_opus_4_5()
        }
        // Anthropic - Legacy (Claude 4.x)
        "claude-sonnet-4-20250514" | "claude-sonnet-4-0" | "claude-sonnet-4" => {
            wonopcode_provider::model::anthropic::claude_sonnet_4()
        }
        "claude-opus-4-1-20250805" | "claude-opus-4-1" => {
            wonopcode_provider::model::anthropic::claude_opus_4_1()
        }
        "claude-opus-4-20250514" | "claude-opus-4-0" | "claude-opus-4" => {
            wonopcode_provider::model::anthropic::claude_opus_4()
        }
        // Anthropic - Legacy (Claude 3.x)
        "claude-3-7-sonnet-20250219" | "claude-3-7-sonnet" | "claude-3-7-sonnet-latest" => {
            wonopcode_provider::model::anthropic::claude_sonnet_3_7()
        }
        "claude-3-haiku-20240307" | "claude-3-haiku" => {
            wonopcode_provider::model::anthropic::claude_haiku_3()
        }
        // OpenAI - GPT-5 Series
        "gpt-5.2" => wonopcode_provider::model::openai::gpt_5_2(),
        "gpt-5.1" => wonopcode_provider::model::openai::gpt_5_1(),
        "gpt-5" => wonopcode_provider::model::openai::gpt_5(),
        "gpt-5-mini" => wonopcode_provider::model::openai::gpt_5_mini(),
        "gpt-5-nano" => wonopcode_provider::model::openai::gpt_5_nano(),
        // OpenAI - GPT-4.1 Series
        "gpt-4.1" => wonopcode_provider::model::openai::gpt_4_1(),
        "gpt-4.1-mini" => wonopcode_provider::model::openai::gpt_4_1_mini(),
        "gpt-4.1-nano" => wonopcode_provider::model::openai::gpt_4_1_nano(),
        // OpenAI - O-Series
        "o3" => wonopcode_provider::model::openai::o3(),
        "o3-mini" => wonopcode_provider::model::openai::o3_mini(),
        "o4-mini" => wonopcode_provider::model::openai::o4_mini(),
        // OpenAI - Legacy
        "gpt-4o" => wonopcode_provider::model::openai::gpt_4o(),
        "gpt-4o-mini" => wonopcode_provider::model::openai::gpt_4o_mini(),
        "o1" => wonopcode_provider::model::openai::o1(),
        // Google
        "gemini-2.0-flash" | "gemini-2.0-flash-exp" => {
            wonopcode_provider::model::google::gemini_2_flash()
        }
        "gemini-1.5-pro" | "gemini-1.5-pro-latest" => {
            wonopcode_provider::model::google::gemini_1_5_pro()
        }
        "gemini-1.5-flash" | "gemini-1.5-flash-latest" => {
            wonopcode_provider::model::google::gemini_1_5_flash()
        }
        // xAI (Grok)
        "grok-3" => xai::models::grok_3(),
        "grok-3-mini" => xai::models::grok_3_mini(),
        "grok-2" | "grok-2-1212" => xai::models::grok_2(),
        // Mistral
        "mistral-large" | "mistral-large-latest" => mistral::models::mistral_large(),
        "mistral-small" | "mistral-small-latest" => mistral::models::mistral_small(),
        "codestral" | "codestral-latest" => mistral::models::codestral(),
        "pixtral-large" | "pixtral-large-latest" => mistral::models::pixtral_large(),
        // Groq
        "llama-3.3-70b-versatile" => groq::models::llama_3_3_70b(),
        "llama-3.1-8b-instant" => groq::models::llama_3_1_8b(),
        "mixtral-8x7b-32768" => groq::models::mixtral_8x7b(),
        "gemma2-9b-it" => groq::models::gemma_2_9b(),
        "deepseek-r1-distill-llama-70b" => groq::models::deepseek_r1_distill(),
        // DeepInfra
        "deepseek-ai/DeepSeek-V3" if provider == "deepinfra" => deepinfra::models::deepseek_v3(),
        "deepseek-ai/DeepSeek-R1" if provider == "deepinfra" => deepinfra::models::deepseek_r1(),
        "Qwen/Qwen2.5-72B-Instruct" => deepinfra::models::qwen_2_5_72b(),
        "meta-llama/Meta-Llama-3.1-405B-Instruct" => deepinfra::models::llama_3_1_405b(),
        // Together
        "deepseek-ai/DeepSeek-V3" if provider == "together" => together::models::deepseek_v3(),
        "deepseek-ai/DeepSeek-R1" if provider == "together" => together::models::deepseek_r1(),
        "meta-llama/Llama-3.3-70B-Instruct-Turbo" => together::models::llama_3_3_70b(),
        "Qwen/Qwen2.5-72B-Instruct-Turbo" => together::models::qwen_2_5_72b(),
        "Qwen/Qwen2.5-Coder-32B-Instruct" => together::models::qwen_2_5_coder(),
        // Fallback for unknown models
        _ => ModelInfo::new(model_id, provider).with_name(model_id),
    }
}

/// Load API key from environment or credentials file.
pub fn load_api_key(provider: &str) -> Option<String> {
    // Try environment variable first
    let env_var = match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "google" | "gemini" => "GOOGLE_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "xai" | "grok" => "XAI_API_KEY",
        _ => return None,
    };

    if let Ok(key) = std::env::var(env_var) {
        if !key.is_empty() {
            return Some(key);
        }
    }

    // Try credentials file
    let path = dirs::data_dir()?.join("wonopcode").join("credentials.json");
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            // Try parsing as simple HashMap<String, String> first (legacy format)
            if let Ok(creds) =
                serde_json::from_str::<std::collections::HashMap<String, String>>(&content)
            {
                return creds.get(provider).cloned();
            }

            // Try parsing as HashMap<String, Value> for new nested format
            if let Ok(creds) = serde_json::from_str::<
                std::collections::HashMap<String, serde_json::Value>,
            >(&content)
            {
                if let Some(provider_config) = creds.get(provider) {
                    // For API key auth, look for "key" field
                    if let Some(key) = provider_config.get("key").and_then(|v| v.as_str()) {
                        return Some(key.to_string());
                    }
                    // For OAuth auth (anthropic subscription), return None - we use CLI instead
                    if provider_config.get("type").and_then(|v| v.as_str()) == Some("oauth") {
                        return None;
                    }
                }
            }
        }
    }

    None
}
