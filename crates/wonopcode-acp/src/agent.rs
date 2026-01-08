//! ACP Agent implementation.
//!
//! The Agent handles all ACP protocol methods and manages the interaction
//! between IDE clients and the wonopcode core.

use crate::processor::{load_api_key, Processor, ProcessorConfig};
use crate::session::SessionManager;
use crate::transport::{Connection, IncomingMessage, StdioTransport};
use crate::types::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// ACP Agent configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent name.
    pub name: String,
    /// Agent version.
    pub version: String,
    /// Default model.
    pub default_model: Option<ModelRef>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "Wonopcode".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            default_model: None,
        }
    }
}

/// ACP Agent.
pub struct Agent {
    config: AgentConfig,
    transport: Arc<StdioTransport>,
    connection: Connection,
    session_manager: SessionManager,
    /// Processors for each session.
    processors: Arc<RwLock<HashMap<String, Arc<Processor>>>>,
}

impl Agent {
    /// Create a new agent.
    pub fn new(config: AgentConfig) -> (Self, mpsc::Receiver<IncomingMessage>) {
        let (transport, incoming_rx) = StdioTransport::new();
        let transport = Arc::new(transport);
        let connection = Connection::new(transport.clone());

        let agent = Self {
            config,
            transport,
            connection,
            session_manager: SessionManager::new(),
            processors: Arc::new(RwLock::new(HashMap::new())),
        };

        (agent, incoming_rx)
    }

    /// Run the agent, processing incoming messages.
    pub async fn run(self, mut incoming_rx: mpsc::Receiver<IncomingMessage>) {
        info!("ACP agent started");

        while let Some(message) = incoming_rx.recv().await {
            match message {
                IncomingMessage::Request(request) => {
                    self.handle_request(request).await;
                }
                IncomingMessage::Notification(notification) => {
                    self.handle_notification(notification).await;
                }
            }
        }

        info!("ACP agent stopped");
    }

    /// Handle a JSON-RPC request.
    async fn handle_request(&self, request: JsonRpcRequest) {
        let id = match request.id {
            Some(id) => id,
            None => {
                warn!("Received request without ID");
                return;
            }
        };

        debug!("Handling request: {} (id={:?})", request.method, id);

        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(request.params).await,
            "authenticate" => self.handle_authenticate(request.params).await,
            "session/new" => self.handle_new_session(request.params).await,
            "session/load" => self.handle_load_session(request.params).await,
            "session/prompt" => self.handle_prompt(request.params).await,
            "session/setModel" => self.handle_set_model(request.params).await,
            "session/setMode" => self.handle_set_mode(request.params).await,
            _ => {
                warn!("Unknown method: {}", request.method);
                Err(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                })
            }
        };

        if let Err(e) = self.transport.send_response(id, result).await {
            error!("Failed to send response: {}", e);
        }
    }

    /// Handle a JSON-RPC notification.
    async fn handle_notification(&self, notification: JsonRpcNotification) {
        debug!("Handling notification: {}", notification.method);

        match notification.method.as_str() {
            "session/cancel" => {
                if let Err(e) = self.handle_cancel(notification.params).await {
                    error!("Failed to handle cancel: {:?}", e);
                }
            }
            _ => {
                debug!("Ignoring unknown notification: {}", notification.method);
            }
        }
    }

    // ========================================================================
    // Protocol Handlers
    // ========================================================================

    /// Handle initialize request.
    async fn handle_initialize(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let _request: InitializeRequest = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .unwrap_or(InitializeRequest {
                protocol_version: 1,
                client_capabilities: None,
            });

        info!("Initializing ACP agent");

        let response = InitializeResponse {
            protocol_version: 1,
            agent_capabilities: AgentCapabilities {
                load_session: true,
                mcp_capabilities: Some(McpCapabilities {
                    http: true,
                    sse: true,
                }),
                prompt_capabilities: Some(PromptCapabilities {
                    embedded_context: true,
                    image: true,
                }),
            },
            auth_methods: vec![AuthMethod {
                id: "wonopcode-login".to_string(),
                name: "Login with wonopcode".to_string(),
                description: "Run `wonopcode auth login` in the terminal".to_string(),
                _meta: None,
            }],
            agent_info: AgentInfo {
                name: self.config.name.clone(),
                version: self.config.version.clone(),
            },
        };

        serde_json::to_value(response).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Handle authenticate request.
    async fn handle_authenticate(
        &self,
        _params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        // Authentication is handled externally via CLI
        Err(JsonRpcError {
            code: -32000,
            message: "Authentication not implemented. Use `wonopcode auth login`".to_string(),
            data: None,
        })
    }

    /// Handle new session request.
    async fn handle_new_session(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let request: NewSessionRequest = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        info!("Creating new session in: {}", request.cwd);

        // Generate session ID
        let session_id = format!("ses_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));

        // Create session state
        let _state = self
            .session_manager
            .create(
                session_id.clone(),
                request.cwd.clone(),
                request.mcp_servers,
                self.config.default_model.clone(),
            )
            .await;

        // Build response
        let response = self.build_session_response(&session_id).await?;

        serde_json::to_value(response).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Handle load session request.
    async fn handle_load_session(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let request: LoadSessionRequest = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        info!("Loading session: {}", request.session_id);

        // Load/create session state
        // Note: We use the current time as the ACP session load time.
        // The actual wonopcode session creation time is stored in the session repository,
        // but ACP sessions track their own lifecycle independently.
        let _state = self
            .session_manager
            .load(
                request.session_id.clone(),
                request.cwd.clone(),
                request.mcp_servers,
                self.config.default_model.clone(),
                chrono::Utc::now(),
            )
            .await;

        // Build response
        let response = self.build_session_response(&request.session_id).await?;

        // Replay session history if there's an existing processor
        self.replay_session_history(&request.session_id).await;

        serde_json::to_value(response).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Replay session history to the client.
    async fn replay_session_history(&self, session_id: &str) {
        let processors = self.processors.read().await;
        if let Some(processor) = processors.get(session_id) {
            let history = processor.get_history().await;

            if history.is_empty() {
                debug!("No history to replay for session {}", session_id);
                return;
            }

            info!(
                "Replaying {} messages for session {}",
                history.len(),
                session_id
            );

            for (role, text) in history {
                let update = match role.as_str() {
                    "user" => SessionUpdate::UserMessageChunk {
                        content: TextContent::new(&text),
                    },
                    "assistant" => SessionUpdate::AgentMessageChunk {
                        content: TextContent::new(&text),
                    },
                    _ => continue,
                };

                let _ = self
                    .connection
                    .session_update(SessionUpdateNotification {
                        session_id: session_id.to_string(),
                        update,
                    })
                    .await;
            }
        } else {
            debug!(
                "No processor found for session {} - no history to replay",
                session_id
            );
        }
    }

    /// Handle prompt request.
    async fn handle_prompt(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let request: PromptRequest = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        let session = self.session_manager.get(&request.session_id).await?;

        info!("Processing prompt for session: {}", session.id);

        // Extract text from prompt parts
        let text: String = request
            .prompt
            .iter()
            .filter_map(|part| match part {
                PromptPart::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        // Check for slash command
        if text.trim().starts_with('/') {
            return self.handle_slash_command(&session.id, &text).await;
        }

        // Get or create processor for this session
        let processor = self.get_or_create_processor(&session).await?;

        // Process the prompt
        match processor
            .process_prompt(&session.id, &text, &self.connection)
            .await
        {
            Ok(()) => {
                debug!("Prompt processed successfully");
            }
            Err(e) => {
                error!("Failed to process prompt: {}", e);
                // Send error as text
                let _ = self
                    .connection
                    .session_update(SessionUpdateNotification {
                        session_id: session.id.clone(),
                        update: SessionUpdate::AgentMessageChunk {
                            content: TextContent::new(format!("Error: {}", e)),
                        },
                    })
                    .await;
            }
        }

        let response = PromptResponse {
            stop_reason: StopReason::EndTurn,
            _meta: None,
        };

        serde_json::to_value(response).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Get or create a processor for a session.
    async fn get_or_create_processor(
        &self,
        session: &AcpSessionState,
    ) -> Result<Arc<Processor>, JsonRpcError> {
        // Check if processor exists
        {
            let processors = self.processors.read().await;
            if let Some(processor) = processors.get(&session.id) {
                return Ok(processor.clone());
            }
        }

        // Determine provider and model
        let (provider, model_id) = session
            .model
            .as_ref()
            .map(|m| (m.provider_id.clone(), m.model_id.clone()))
            .or_else(|| {
                self.config
                    .default_model
                    .as_ref()
                    .map(|m| (m.provider_id.clone(), m.model_id.clone()))
            })
            .unwrap_or_else(|| {
                (
                    "anthropic".to_string(),
                    "claude-sonnet-4-5-20250929".to_string(),
                )
            });

        // Load API key
        let api_key = load_api_key(&provider).ok_or_else(JsonRpcError::auth_required)?;

        // Create processor config
        let config = ProcessorConfig {
            provider,
            model_id,
            api_key,
            max_tokens: Some(8192),
            temperature: Some(0.7),
        };

        // Create processor
        let cwd = PathBuf::from(&session.cwd);
        let processor = Processor::new(config, &cwd)
            .await
            .map_err(|e| JsonRpcError::internal_error(e.to_string()))?;

        let processor = Arc::new(processor);

        // Store processor
        {
            let mut processors = self.processors.write().await;
            processors.insert(session.id.clone(), processor.clone());
        }

        Ok(processor)
    }

    /// Handle set model request.
    async fn handle_set_model(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let request: SetSessionModelRequest = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        let model = ModelRef::parse(&request.model_id)
            .ok_or_else(|| JsonRpcError::invalid_params("Invalid model ID format"))?;

        self.session_manager
            .set_model(&request.session_id, model)
            .await?;

        Ok(serde_json::json!({"_meta": {}}))
    }

    /// Handle set mode request.
    async fn handle_set_mode(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let request: SetSessionModeRequest = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        self.session_manager
            .set_mode(&request.session_id, request.mode_id)
            .await?;

        Ok(serde_json::Value::Null)
    }

    /// Handle cancel notification.
    async fn handle_cancel(&self, params: Option<serde_json::Value>) -> Result<(), JsonRpcError> {
        let notification: CancelNotification = params
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        info!("Cancelling session: {}", notification.session_id);

        // Cancel the running operation if there is one
        let processors = self.processors.read().await;
        if let Some(processor) = processors.get(&notification.session_id) {
            processor.cancel().await;
            info!("Session {} cancelled successfully", notification.session_id);
        } else {
            warn!(
                "No active processor for session {} to cancel",
                notification.session_id
            );
        }

        Ok(())
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    /// Build a session response with models and modes.
    async fn build_session_response(
        &self,
        session_id: &str,
    ) -> Result<SessionResponse, JsonRpcError> {
        let session = self.session_manager.get(session_id).await?;

        // Get available models dynamically from models.dev
        let available_models = self.get_available_models().await;

        let current_model_id = session
            .model
            .as_ref()
            .map(|m| m.as_string())
            .unwrap_or_else(|| "anthropic/claude-sonnet-4-5-20250929".to_string());

        // Get available modes from agents configuration
        let available_modes = self.get_available_modes();

        let current_mode_id = session.mode_id.unwrap_or_else(|| "default".to_string());

        // Send available commands
        let _ = self
            .connection
            .session_update(SessionUpdateNotification {
                session_id: session_id.to_string(),
                update: SessionUpdate::AvailableCommandsUpdate {
                    available_commands: vec![
                        CommandInfo {
                            name: "compact".to_string(),
                            description: "Compact the session history".to_string(),
                        },
                        CommandInfo {
                            name: "clear".to_string(),
                            description: "Clear the session".to_string(),
                        },
                    ],
                },
            })
            .await;

        Ok(SessionResponse {
            session_id: session_id.to_string(),
            models: ModelsInfo {
                current_model_id,
                available_models,
            },
            modes: ModesInfo {
                current_mode_id,
                available_modes,
            },
            _meta: None,
        })
    }

    /// Handle slash commands.
    async fn handle_slash_command(
        &self,
        session_id: &str,
        text: &str,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let text = text.trim();
        let parts: Vec<&str> = text[1..].splitn(2, char::is_whitespace).collect();
        let command = parts.first().unwrap_or(&"");

        info!("Handling slash command: /{}", command);

        match *command {
            "compact" => {
                // Get processor for session
                let processors = self.processors.read().await;
                if let Some(processor) = processors.get(session_id) {
                    let result = processor.compact().await;
                    let message = format!(
                        "Session compacted. Messages: {} -> {}, Estimated tokens saved: {}",
                        result.messages_before, result.messages_after, result.tokens_saved_estimate
                    );
                    let _ = self
                        .connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::AgentMessageChunk {
                                content: TextContent::new(&message),
                            },
                        })
                        .await;
                } else {
                    let _ = self
                        .connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::AgentMessageChunk {
                                content: TextContent::new("No active session to compact."),
                            },
                        })
                        .await;
                }
            }
            "clear" => {
                // Get processor for session and clear history
                let processors = self.processors.read().await;
                if let Some(processor) = processors.get(session_id) {
                    processor.clear_history().await;
                    let _ = self
                        .connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::AgentMessageChunk {
                                content: TextContent::new("Session history cleared."),
                            },
                        })
                        .await;
                } else {
                    let _ = self
                        .connection
                        .session_update(SessionUpdateNotification {
                            session_id: session_id.to_string(),
                            update: SessionUpdate::AgentMessageChunk {
                                content: TextContent::new("No active session to clear."),
                            },
                        })
                        .await;
                }
            }
            _ => {
                let _ = self
                    .connection
                    .session_update(SessionUpdateNotification {
                        session_id: session_id.to_string(),
                        update: SessionUpdate::AgentMessageChunk {
                            content: TextContent::new(format!("Unknown command: /{}", command)),
                        },
                    })
                    .await;
            }
        }

        let response = PromptResponse {
            stop_reason: StopReason::EndTurn,
            _meta: None,
        };

        serde_json::to_value(response).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Get available models from models.dev with fallback to static list.
    async fn get_available_models(&self) -> Vec<ModelInfo> {
        // Try to get models dynamically from models.dev
        match wonopcode_provider::models_dev::get_all_models().await {
            Ok(models) => {
                // Filter to the most commonly used models and convert to ACP format
                let mut result: Vec<ModelInfo> = models
                    .into_iter()
                    .filter(|m| {
                        // Filter to models that support tool calls (most useful for agents)
                        m.capabilities.tool_call
                    })
                    .map(|m| ModelInfo {
                        model_id: format!("{}/{}", m.provider_id, m.id),
                        name: format!("{}/{}", capitalize_provider(&m.provider_id), m.name),
                    })
                    .collect();

                // Sort by provider then name
                result.sort_by(|a, b| a.model_id.cmp(&b.model_id));

                if result.is_empty() {
                    debug!("No models with tool_call support found, using fallback");
                    Self::fallback_models()
                } else {
                    debug!("Loaded {} models from models.dev", result.len());
                    result
                }
            }
            Err(e) => {
                warn!(
                    "Failed to fetch models from models.dev: {}, using fallback",
                    e
                );
                Self::fallback_models()
            }
        }
    }

    /// Fallback model list when models.dev is unavailable.
    fn fallback_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                model_id: "anthropic/claude-sonnet-4-5-20250929".to_string(),
                name: "Anthropic/Claude Sonnet 4.5".to_string(),
            },
            ModelInfo {
                model_id: "anthropic/claude-haiku-4-5-20251001".to_string(),
                name: "Anthropic/Claude Haiku 4.5".to_string(),
            },
            ModelInfo {
                model_id: "anthropic/claude-3-haiku-20240307".to_string(),
                name: "Anthropic/Claude 3 Haiku".to_string(),
            },
            ModelInfo {
                model_id: "openai/gpt-5.2".to_string(),
                name: "OpenAI/GPT-5.2".to_string(),
            },
            ModelInfo {
                model_id: "openai/gpt-4o".to_string(),
                name: "OpenAI/GPT-4o".to_string(),
            },
            ModelInfo {
                model_id: "openai/gpt-4o-mini".to_string(),
                name: "OpenAI/GPT-4o Mini".to_string(),
            },
            ModelInfo {
                model_id: "google/gemini-2.0-flash".to_string(),
                name: "Google/Gemini 2.0 Flash".to_string(),
            },
        ]
    }

    /// Get available agent modes.
    ///
    /// Returns the built-in agent modes. Custom agents defined in wonopcode.json
    /// are loaded separately via the agent registry in wonopcode-core.
    fn get_available_modes(&self) -> Vec<ModeInfo> {
        // Built-in modes - custom agents are handled via wonopcode-core::AgentRegistry
        vec![
            ModeInfo {
                id: "default".to_string(),
                name: "default".to_string(),
                description: Some("General-purpose coding assistant".to_string()),
            },
            ModeInfo {
                id: "plan".to_string(),
                name: "plan".to_string(),
                description: Some("Planning mode for complex tasks".to_string()),
            },
            ModeInfo {
                id: "code".to_string(),
                name: "code".to_string(),
                description: Some("Code-focused mode with minimal explanations".to_string()),
            },
        ]
    }
}

/// Capitalize the first letter of a provider name.
fn capitalize_provider(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Start the ACP server.
pub async fn serve(config: AgentConfig) {
    let (agent, incoming_rx) = Agent::new(config);
    agent.run(incoming_rx).await;
}
