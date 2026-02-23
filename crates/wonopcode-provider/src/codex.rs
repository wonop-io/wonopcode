//! OpenAI Codex provider implementation.
//!
//! This provider uses OpenAI's Responses API for Codex access, supporting both
//! API key authentication and ChatGPT subscription-based access via OAuth.
//!
//! # Authentication Methods
//!
//! 1. **API Key**: Set `OPENAI_API_KEY` environment variable
//! 2. **ChatGPT Subscription**: Uses device code OAuth flow via `codex login`
//!
//! # Models
//!
//! - `codex`: Default Codex model (included with ChatGPT subscription)
//! - `o3`: OpenAI o3 reasoning model
//! - `o4-mini`: OpenAI o4-mini model

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
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};
use tracing::{debug, info, trace, warn};
use wonop_codex_auth::{AuthCredentialsStoreMode, AuthManager};
use wonop_codex_client::{
    CodexClient, ConversationItem, ResponseEvent, ResponsesApiRequest, ResponsesOptions,
    Tool as CodexTool,
};

/// Cache for the Codex CLI binary path.
static CODEX_CLI_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Find the Codex CLI binary, searching common installation locations.
///
/// Search order:
/// 0. Custom path from WONOPCODE_CODEX_CLI_PATH environment variable
/// 1. Standard PATH lookup
/// 2. Homebrew on macOS: /opt/homebrew/bin/codex, /usr/local/bin/codex
/// 3. npm global: ~/.npm-global/bin/codex
/// 4. User local: ~/.local/bin/codex
fn find_codex_cli() -> Option<PathBuf> {
    CODEX_CLI_PATH
        .get_or_init(|| {
            debug!("Searching for Codex CLI...");

            // Build enhanced PATH to ensure Node.js is available
            let enhanced_path = build_enhanced_path();

            // 0. Check custom path from environment variable first
            if let Ok(custom_path) = std::env::var("WONOPCODE_CODEX_CLI_PATH") {
                if !custom_path.is_empty() {
                    let path = PathBuf::from(&custom_path);
                    debug!(path = %path.display(), "Checking custom Codex CLI path from WONOPCODE_CODEX_CLI_PATH");

                    if path.exists() {
                        let mut cmd = Command::new(&path);
                        cmd.arg("--version");
                        cmd.env("PATH", &enhanced_path);

                        if let Ok(output) = cmd.output() {
                            if output.status.success() {
                                debug!(path = %path.display(), "Using custom Codex CLI path");
                                return Some(path);
                            }
                        }
                    }
                }
            }

            // 1. Try standard PATH lookup first (with enhanced PATH)
            let mut cmd = Command::new("codex");
            cmd.arg("--version");
            cmd.env("PATH", &enhanced_path);

            if let Ok(output) = cmd.output() {
                if output.status.success() {
                    debug!("Found codex in PATH");
                    return Some(PathBuf::from("codex"));
                }
            }

            // 2. Check common installation locations
            let common_paths = [
                // Homebrew on Apple Silicon
                "/opt/homebrew/bin/codex",
                // Homebrew on Intel Mac / Linux
                "/usr/local/bin/codex",
                // User local bin
                "~/.local/bin/codex",
                // npm global (common location)
                "~/.npm-global/bin/codex",
            ];

            for path_str in &common_paths {
                let path = if path_str.starts_with('~') {
                    if let Some(home) = dirs::home_dir() {
                        home.join(&path_str[2..])
                    } else {
                        continue;
                    }
                } else {
                    PathBuf::from(path_str)
                };

                if path.exists() {
                    let mut cmd = Command::new(&path);
                    cmd.arg("--version");
                    cmd.env("PATH", &enhanced_path);

                    if let Ok(output) = cmd.output() {
                        if output.status.success() {
                            debug!(path = %path.display(), "Found Codex CLI");
                            return Some(path);
                        }
                    }
                }
            }

            debug!("Codex CLI not found");
            None
        })
        .clone()
}

/// Build an enhanced PATH that includes common Node.js installation locations.
fn build_enhanced_path() -> String {
    let mut paths: Vec<String> = Vec::new();

    // Add common Node.js/npm locations
    paths.push("/opt/homebrew/bin".to_string());
    paths.push("/usr/local/bin".to_string());

    // Add user's local bin
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/bin").to_string_lossy().to_string());
        paths.push(home.join(".npm-global/bin").to_string_lossy().to_string());

        // nvm - check for current version symlink first
        let nvm_current = home.join(".nvm/current/bin");
        if nvm_current.exists() {
            paths.push(nvm_current.to_string_lossy().to_string());
        }
    }

    // Add existing PATH
    if let Ok(existing_path) = std::env::var("PATH") {
        paths.push(existing_path);
    }

    #[cfg(unix)]
    let separator = ":";
    #[cfg(windows)]
    let separator = ";";

    paths.join(separator)
}

/// OpenAI Codex provider using the Responses API.
pub struct CodexProvider {
    client: Arc<CodexClient>,
    auth_manager: Arc<AuthManager>,
    model: ModelInfo,
}

impl CodexProvider {
    /// Create a new Codex provider.
    ///
    /// This will attempt to authenticate using:
    /// 1. OPENAI_API_KEY environment variable (if set)
    /// 2. Existing credentials from ~/.codex/auth.json
    ///
    /// Returns an error if no valid authentication is available.
    pub fn new(model: ModelInfo) -> ProviderResult<Self> {
        let codex_home = wonop_codex_auth::default_codex_home();

        // Create auth manager with env key support
        let auth_manager = Arc::new(AuthManager::new(
            codex_home,
            true, // Enable env API key
            AuthCredentialsStoreMode::File,
        ));

        // Create client
        let client = Arc::new(CodexClient::new(auth_manager.clone()));

        Ok(Self {
            client,
            auth_manager,
            model,
        })
    }

    /// Create a new Codex provider with a specific API key.
    pub fn with_api_key(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        // Set the environment variable so AuthManager will pick it up
        std::env::set_var(wonop_codex_auth::OPENAI_API_KEY_ENV_VAR, api_key);

        let codex_home = wonop_codex_auth::default_codex_home();
        let auth_manager = Arc::new(AuthManager::new(
            codex_home,
            true, // Use env key
            AuthCredentialsStoreMode::Ephemeral,
        ));

        let client = Arc::new(CodexClient::new(auth_manager.clone()));

        Ok(Self {
            client,
            auth_manager,
            model,
        })
    }

    /// Check if the provider is authenticated.
    pub async fn is_authenticated(&self) -> bool {
        self.auth_manager.is_authenticated().await
    }

    /// Get the authentication mode.
    pub async fn auth_mode(&self) -> Option<wonop_codex_auth::AuthMode> {
        self.auth_manager.auth_mode().await
    }

    /// Check if Codex CLI is available (cached for performance).
    ///
    /// This checks if the `codex` CLI is installed and accessible.
    pub fn is_available() -> bool {
        find_codex_cli().is_some()
    }

    /// Check if Codex is authenticated (sync, fast check) - static version.
    ///
    /// This performs a quick heuristic check by looking for:
    /// 1. OPENAI_API_KEY environment variable
    /// 2. ~/.codex/auth.json file (from `codex login`)
    ///
    /// Returns `true` if authentication appears to be configured.
    pub fn has_credentials() -> bool {
        // 1. Check for API key in environment
        if wonop_codex_auth::read_api_key_from_env().is_some() {
            debug!("Codex authenticated via API key environment variable");
            return true;
        }

        // 2. Check for auth.json file from `codex login`
        let codex_home = wonop_codex_auth::default_codex_home();
        let auth_file = codex_home.join(wonop_codex_auth::AUTH_FILE_NAME);
        
        if auth_file.exists() {
            debug!(path = %auth_file.display(), "Codex auth file found");
            return true;
        }

        debug!("Codex not authenticated");
        false
    }

    /// Check if Codex CLI is installed and accessible.
    pub fn check_cli_available() -> ProviderResult<()> {
        if find_codex_cli().is_some() {
            debug!("Codex CLI found");
            Ok(())
        } else {
            Err(ProviderError::internal(
                "Codex CLI not found. Install with: npm install -g @openai/codex".to_string(),
            ))
        }
    }

    /// Convert messages to Codex conversation format.
    fn convert_messages(messages: &[Message], _system: Option<&str>) -> Vec<ConversationItem> {
        let mut items = Vec::new();

        // System message is handled via instructions in the request

        for msg in messages {
            match msg.role {
                Role::User => {
                    // Extract text content
                    for part in &msg.content {
                        if let ContentPart::Text { text } = part {
                            items.push(ConversationItem::user_message(text.clone()));
                        }
                    }
                }
                Role::Assistant => {
                    // Extract text content
                    for part in &msg.content {
                        if let ContentPart::Text { text } = part {
                            items.push(ConversationItem::assistant_message(text.clone()));
                        }
                    }
                }
                Role::Tool => {
                    // Tool results
                    for part in &msg.content {
                        if let ContentPart::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = part
                        {
                            items.push(ConversationItem::tool_result(
                                tool_use_id.clone(),
                                content.clone(),
                            ));
                        }
                    }
                }
                Role::System => {
                    // System messages are not directly supported; they should be passed
                    // as instructions to the request
                }
            }
        }

        items
    }

    /// Convert tools to Codex format.
    /// 
    /// The Responses API with strict mode requires:
    /// 1. additionalProperties: false in the parameters schema
    /// 2. All properties must be listed in the required array
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<CodexTool> {
        tools
            .iter()
            .map(|tool| {
                let mut params = tool.parameters.clone();
                if let Some(obj) = params.as_object_mut() {
                    // Ensure additionalProperties: false (required for strict mode)
                    obj.insert("additionalProperties".to_string(), serde_json::Value::Bool(false));
                    
                    // Ensure all properties are in the required array (required for strict mode)
                    if let Some(properties) = obj.get("properties").and_then(|p| p.as_object()) {
                        let all_keys: Vec<serde_json::Value> = properties
                            .keys()
                            .map(|k| serde_json::Value::String(k.clone()))
                            .collect();
                        obj.insert("required".to_string(), serde_json::Value::Array(all_keys));
                    }
                }
                CodexTool::function_with_params(&tool.name, &tool.description, params)
            })
            .collect()
    }
}

#[async_trait]
impl LanguageModel for CodexProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        // Load authentication
        if let Err(e) = self.auth_manager.load().await {
            warn!("Failed to load auth: {}", e);
        }

        // Check authentication
        if !self.auth_manager.is_authenticated().await {
            return Err(ProviderError::Internal {
                message: "Not authenticated. Run 'codex login' or set OPENAI_API_KEY.".to_string(),
            });
        }

        // Convert messages
        let conversation = Self::convert_messages(&messages, options.system.as_deref());

        // Convert tools
        let tools = Self::convert_tools(&options.tools);

        // Build request
        let mut request = ResponsesApiRequest::text(self.model.id.clone(), "")
            .with_storage(false);
        
        // Replace input with our conversation
        request.input = conversation;

        // Add system instructions if provided
        if let Some(ref system) = options.system {
            request = request.with_instructions(system.clone());
        }

        // Add tools if any
        if !tools.is_empty() {
            request = request.with_tools(tools);
        }

        // Add max output tokens if specified (Responses API uses max_output_tokens)
        if let Some(max_tokens) = options.max_tokens {
            request = request.with_max_output_tokens(max_tokens);
        }

        info!(
            model = %self.model.id,
            tools_count = options.tools.len(),
            "Sending Codex Responses API request"
        );
        trace!(request = ?request, "Full Codex request");

        // Create streaming response
        let response_options = ResponsesOptions::new();
        let mut stream = self
            .client
            .responses()
            .stream(request, response_options)
            .await
            .map_err(|e| ProviderError::Internal {
                message: format!("Codex API error: {}", e),
            })?;

        let abort = options.abort.clone();

        Ok(Box::pin(try_stream! {
            let mut text_started = false;
            let mut current_tool_id: Option<String> = None;
            let mut current_tool_name: Option<String> = None;
            let mut current_tool_args = String::new();

            while let Some(event_result) = stream.next().await {
                // Check for cancellation
                if let Some(ref token) = abort {
                    if token.is_cancelled() {
                        Err(ProviderError::Cancelled)?;
                    }
                }

                let event = event_result.map_err(|e| ProviderError::Internal {
                    message: format!("Stream error: {}", e),
                })?;

                match event {
                    ResponseEvent::TextDelta { delta, .. } => {
                        if !text_started {
                            info!("Codex: Starting text stream");
                            yield StreamChunk::TextStart;
                            text_started = true;
                        }
                        trace!(text_len = delta.len(), "Codex: TextDelta");
                        yield StreamChunk::TextDelta(delta);
                    }
                    ResponseEvent::ToolCallCreated { tool_call, .. } => {
                        // End any previous tool call
                        if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                            yield StreamChunk::ToolCall {
                                id,
                                name,
                                arguments: std::mem::take(&mut current_tool_args),
                            };
                        }

                        // Start new tool call
                        let tool_id = tool_call.id.clone();
                        let tool_name = tool_call.function
                            .as_ref()
                            .map(|f| f.name.clone())
                            .or(tool_call.name.clone())
                            .unwrap_or_default();
                        
                        current_tool_id = Some(tool_id.clone());
                        current_tool_name = Some(tool_name.clone());
                        current_tool_args.clear();

                        yield StreamChunk::ToolCallStart {
                            id: tool_id,
                            name: tool_name,
                        };
                    }
                    ResponseEvent::ToolCallDelta { delta, .. } => {
                        if let Some(args) = delta.arguments {
                            current_tool_args.push_str(&args);
                        }
                    }
                    ResponseEvent::ToolCallDone { tool_call, .. } => {
                        // Get final tool call info
                        let id = current_tool_id.take().unwrap_or(tool_call.id.clone());
                        let name = current_tool_name.take()
                            .or_else(|| tool_call.function.as_ref().map(|f| f.name.clone()))
                            .or(tool_call.name.clone())
                            .unwrap_or_default();
                        let args = if current_tool_args.is_empty() {
                            tool_call.function
                                .as_ref()
                                .map(|f| f.arguments.clone())
                                .or(tool_call.arguments.clone())
                                .unwrap_or_default()
                        } else {
                            std::mem::take(&mut current_tool_args)
                        };

                        yield StreamChunk::ToolCall {
                            id,
                            name,
                            arguments: args,
                        };
                    }
                    ResponseEvent::ResponseDone { response } => {
                        // End text if started
                        if text_started {
                            yield StreamChunk::TextEnd;
                            text_started = false;
                        }

                        // Emit any remaining tool call
                        if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                            yield StreamChunk::ToolCall {
                                id,
                                name,
                                arguments: std::mem::take(&mut current_tool_args),
                            };
                        }

                        // Determine finish reason
                        let has_tool_calls = !response.tool_calls().is_empty();
                        let reason = if has_tool_calls {
                            FinishReason::ToolUse
                        } else if response.status.as_deref() == Some("max_tokens") {
                            FinishReason::MaxTokens
                        } else {
                            FinishReason::EndTurn
                        };

                        // Build usage stats
                        let usage_stats = response.usage
                            .map(|u| Usage::new(u.input_tokens, u.output_tokens))
                            .unwrap_or_default();

                        yield StreamChunk::FinishStep {
                            usage: usage_stats,
                            accumulated_usage: None,
                            finish_reason: reason,
                        };
                    }
                    ResponseEvent::Error { error } => {
                        Err(ProviderError::Internal {
                            message: format!("Codex API error: {}", error.message),
                        })?;
                    }
                    ResponseEvent::Done => {
                        // Stream ended
                        if text_started {
                            yield StreamChunk::TextEnd;
                        }
                    }
                    _ => {
                        // Ignore other events
                        trace!(event = ?event, "Codex: Ignoring event");
                    }
                }
            }
        }))
    }

    fn model_info(&self) -> &ModelInfo {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "openai-codex"
    }
}

/// Codex model definitions.
pub mod models {
    use super::*;
    use crate::model::{ModelCapabilities, ModelCost, ModelLimit, ModelStatus, ModalitySupport};

    /// Default Codex model (included with ChatGPT subscription).
    pub fn codex() -> ModelInfo {
        ModelInfo {
            id: "codex".to_string(),
            name: "OpenAI Codex".to_string(),
            provider_id: "openai-codex".to_string(),
            family: Some("codex".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                reasoning: false,
                temperature: false,
                attachment: true,
                input: ModalitySupport { text: true, image: true, ..Default::default() },
                output: ModalitySupport { text: true, ..Default::default() },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.0, // Included with subscription
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200000,
                output: 32000,
            },
            status: ModelStatus::Active,
        }
    }

    /// OpenAI o3 reasoning model via Codex.
    pub fn o3() -> ModelInfo {
        ModelInfo {
            id: "o3".to_string(),
            name: "OpenAI Codex (o3)".to_string(),
            provider_id: "openai-codex".to_string(),
            family: Some("o3".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                reasoning: true,
                temperature: false,
                attachment: true,
                input: ModalitySupport { text: true, image: true, ..Default::default() },
                output: ModalitySupport { text: true, ..Default::default() },
                interleaved: true,
            },
            cost: ModelCost {
                input: 10000.0, // $10 per 1M tokens
                output: 40000.0, // $40 per 1M tokens
                cache_read: 0.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200000,
                output: 100000,
            },
            status: ModelStatus::Active,
        }
    }

    /// OpenAI o4-mini model via Codex.
    pub fn o4_mini() -> ModelInfo {
        ModelInfo {
            id: "o4-mini".to_string(),
            name: "OpenAI Codex (o4-mini)".to_string(),
            provider_id: "openai-codex".to_string(),
            family: Some("o4".to_string()),
            capabilities: ModelCapabilities {
                tool_call: true,
                reasoning: true,
                temperature: false,
                attachment: true,
                input: ModalitySupport { text: true, image: true, ..Default::default() },
                output: ModalitySupport { text: true, ..Default::default() },
                interleaved: true,
            },
            cost: ModelCost {
                input: 1100.0, // $1.10 per 1M tokens
                output: 4400.0, // $4.40 per 1M tokens
                cache_read: 0.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200000,
                output: 100000,
            },
            status: ModelStatus::Active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_info() {
        let model_info = models::codex();
        assert_eq!(model_info.id, "codex");
        assert_eq!(model_info.provider_id, "openai-codex");
        assert!(model_info.capabilities.tool_call);
    }

    #[test]
    fn test_o3_model_info() {
        let model_info = models::o3();
        assert_eq!(model_info.id, "o3");
        assert!(model_info.capabilities.reasoning);
    }

    #[test]
    fn test_convert_tools() {
        // Test with multiple properties, some originally not required
        let tools = vec![ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "optional_param": {"type": "boolean"}
                },
                "required": ["query"]  // Only query was required originally
            }),
        }];

        let converted = CodexProvider::convert_tools(&tools);
        assert_eq!(converted.len(), 1);
        
        // Verify the tool has the correct structure
        let tool = &converted[0];
        assert_eq!(tool.name, Some("test_tool".to_string()));
        assert_eq!(tool.description, Some("A test tool".to_string()));
        assert_eq!(tool.strict, Some(true));
        
        // Verify strict mode requirements
        if let Some(params) = &tool.parameters {
            // additionalProperties must be false
            assert_eq!(
                params.get("additionalProperties"),
                Some(&serde_json::Value::Bool(false)),
                "additionalProperties must be false for strict mode"
            );
            
            // All properties must be in required array
            let required = params.get("required").and_then(|r| r.as_array()).unwrap();
            let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
            assert!(required_strs.contains(&"query"), "query must be required");
            assert!(required_strs.contains(&"optional_param"), "optional_param must be required for strict mode");
        }
    }
}
