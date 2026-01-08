//! Backend abstraction for TUI communication.
//!
//! This module provides a trait for backend communication, allowing the TUI
//! to work with either a local runner (direct channels) or a remote server (HTTP/SSE).

use crate::{AppAction, AppUpdate};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Error type for backend operations.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Result type for backend operations.
pub type BackendResult<T> = Result<T, BackendError>;

/// Trait for backend communication.
///
/// Implementations provide either local (channel-based) or remote (HTTP/SSE) communication.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Send an action to the agent.
    async fn send_action(&self, action: AppAction) -> BackendResult<()>;

    /// Check if the backend is connected.
    fn is_connected(&self) -> bool;

    /// Get the backend type name (for display).
    fn backend_type(&self) -> &'static str;
}

/// Local backend using direct tokio channels.
///
/// This is used when the TUI and runner are in the same process.
pub struct LocalBackend {
    action_tx: mpsc::UnboundedSender<AppAction>,
}

impl LocalBackend {
    /// Create a new local backend with the given action sender.
    pub fn new(action_tx: mpsc::UnboundedSender<AppAction>) -> Self {
        Self { action_tx }
    }
}

#[async_trait]
impl Backend for LocalBackend {
    async fn send_action(&self, action: AppAction) -> BackendResult<()> {
        self.action_tx
            .send(action)
            .map_err(|_| BackendError::ChannelClosed)
    }

    fn is_connected(&self) -> bool {
        !self.action_tx.is_closed()
    }

    fn backend_type(&self) -> &'static str {
        "local"
    }
}

/// Remote backend using HTTP for actions and SSE for updates.
///
/// This is used when connecting to a remote headless agent server.
pub struct RemoteBackend {
    client: reqwest::Client,
    base_url: String,
    connected: std::sync::atomic::AtomicBool,
}

impl RemoteBackend {
    /// Create a new remote backend connecting to the given address.
    pub fn new(address: &str) -> BackendResult<Self> {
        let base_url = if address.starts_with("http://") || address.starts_with("https://") {
            address.to_string()
        } else {
            format!("http://{}", address)
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| BackendError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            connected: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check server health and mark as connected.
    pub async fn connect(&self) -> BackendResult<()> {
        let url = format!("{}/health", self.base_url);
        self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| BackendError::ConnectionFailed(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::ConnectionFailed(e.to_string()))?;

        self.connected
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// Get the full state from the server.
    pub async fn get_state(&self) -> BackendResult<wonopcode_protocol::State> {
        let url = format!("{}/state", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| BackendError::SerializationError(e.to_string()))
    }

    /// Subscribe to SSE events and forward them to the given sender.
    ///
    /// This spawns a background task that reads SSE events and sends them
    /// as AppUpdate messages.
    pub fn subscribe_updates(
        &self,
        update_tx: mpsc::UnboundedSender<AppUpdate>,
    ) -> tokio::task::JoinHandle<()> {
        let url = format!("{}/events", self.base_url);
        let client = self.client.clone();

        tokio::spawn(async move {
            use futures::StreamExt;

            loop {
                match client.get(&url).send().await {
                    Ok(response) => {
                        let mut stream = response.bytes_stream();
                        let mut buffer = String::new();

                        while let Some(chunk) = stream.next().await {
                            match chunk {
                                Ok(bytes) => {
                                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                                    // Process complete SSE events
                                    while let Some(pos) = buffer.find("\n\n") {
                                        let event_str = buffer[..pos].to_string();
                                        buffer = buffer[pos + 2..].to_string();

                                        if let Some(update) = parse_sse_event(&event_str) {
                                            if update_tx.send(update).is_err() {
                                                return; // Channel closed
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("SSE stream error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to connect to SSE stream: {}", e);
                    }
                }

                // Reconnect after a delay
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        })
    }

    /// Convert AppAction to protocol Action and send via HTTP.
    async fn send_protocol_action(&self, action: wonopcode_protocol::Action) -> BackendResult<()> {
        let endpoint = action.endpoint();
        let url = format!("{}{}", self.base_url, endpoint);

        self.client
            .post(&url)
            .json(&action)
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl Backend for RemoteBackend {
    async fn send_action(&self, action: AppAction) -> BackendResult<()> {
        let protocol_action = app_action_to_protocol(action)?;
        self.send_protocol_action(protocol_action).await
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn backend_type(&self) -> &'static str {
        "remote"
    }
}

/// Convert AppAction to protocol Action.
fn app_action_to_protocol(action: AppAction) -> BackendResult<wonopcode_protocol::Action> {
    use wonopcode_protocol::Action;

    Ok(match action {
        AppAction::SendPrompt(prompt) => Action::SendPrompt { prompt },
        AppAction::Cancel => Action::Cancel,
        AppAction::Quit => Action::Quit,
        AppAction::SwitchSession(session_id) => Action::SwitchSession { session_id },
        AppAction::ChangeModel(model) => Action::ChangeModel { model },
        AppAction::ChangeAgent(agent) => Action::ChangeAgent { agent },
        AppAction::NewSession => Action::NewSession,
        AppAction::Undo => Action::Undo,
        AppAction::Redo => Action::Redo,
        AppAction::Revert { message_id } => Action::Revert { message_id },
        AppAction::Unrevert => Action::Unrevert,
        AppAction::Compact => Action::Compact,
        AppAction::RenameSession { title } => Action::RenameSession { title },
        AppAction::McpToggle { name } => Action::McpToggle { name },
        AppAction::McpReconnect { name } => Action::McpReconnect { name },
        AppAction::ForkSession { message_id } => Action::ForkSession { message_id },
        AppAction::ShareSession => Action::ShareSession,
        AppAction::UnshareSession => Action::UnshareSession,
        AppAction::GotoMessage { message_id } => Action::GotoMessage { message_id },
        AppAction::SandboxStart => Action::SandboxStart,
        AppAction::SandboxStop => Action::SandboxStop,
        AppAction::SandboxRestart => Action::SandboxRestart,
        AppAction::SaveSettings { scope, config } => {
            let protocol_scope = match scope {
                crate::SaveScope::Project => wonopcode_protocol::SaveScope::Project,
                crate::SaveScope::Global => wonopcode_protocol::SaveScope::Global,
            };
            Action::SaveSettings {
                scope: protocol_scope,
                config: serde_json::to_value(&*config)
                    .map_err(|e| BackendError::SerializationError(e.to_string()))?,
            }
        }
        AppAction::UpdateTestProviderSettings {
            emulate_thinking,
            emulate_tool_calls,
            emulate_tool_observed,
            emulate_streaming,
        } => Action::UpdateTestProviderSettings {
            emulate_thinking,
            emulate_tool_calls,
            emulate_tool_observed,
            emulate_streaming,
        },
        AppAction::PermissionResponse {
            request_id,
            allow,
            remember,
        } => Action::PermissionResponse {
            request_id,
            allow,
            remember,
        },
        // OpenEditor is handled locally, not sent to server
        AppAction::OpenEditor { .. } => {
            return Err(BackendError::RequestFailed(
                "OpenEditor is not supported for remote backend".to_string(),
            ));
        }
    })
}

/// Parse an SSE event string into an AppUpdate.
fn parse_sse_event(event_str: &str) -> Option<AppUpdate> {
    let mut data = None;

    for line in event_str.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data = Some(rest.trim().to_string());
        }
        // We ignore the event type since we parse the full Update which has the type embedded
    }

    let data = data?;
    let update: wonopcode_protocol::Update = serde_json::from_str(&data).ok()?;

    Some(protocol_update_to_app(update))
}

/// Convert protocol Update to AppUpdate.
fn protocol_update_to_app(update: wonopcode_protocol::Update) -> AppUpdate {
    use wonopcode_protocol::Update;

    match update {
        Update::Started => AppUpdate::Started,
        Update::TextDelta { delta } => AppUpdate::TextDelta(delta),
        Update::ToolStarted { id, name, input } => AppUpdate::ToolStarted { name, id, input },
        Update::ToolCompleted {
            id,
            success,
            output,
            metadata,
        } => AppUpdate::ToolCompleted {
            id,
            success,
            output,
            metadata,
        },
        Update::Completed { text } => AppUpdate::Completed { text },
        Update::Error { error } => AppUpdate::Error(error),
        Update::Status { message } => AppUpdate::Status(message),
        Update::TokenUsage {
            input,
            output,
            cost,
            context_limit,
        } => AppUpdate::TokenUsage {
            input,
            output,
            cost,
            context_limit,
        },
        Update::ModelInfo { context_limit } => AppUpdate::ModelInfo { context_limit },
        Update::Sessions { sessions } => AppUpdate::Sessions(
            sessions
                .into_iter()
                .map(|s| (s.id, s.title, s.timestamp))
                .collect(),
        ),
        Update::TodosUpdated { todos } => AppUpdate::TodosUpdated(
            todos
                .into_iter()
                .map(|t| crate::TodoUpdate {
                    id: t.id,
                    content: t.content,
                    status: t.status,
                    priority: t.priority,
                })
                .collect(),
        ),
        Update::LspUpdated { servers } => AppUpdate::LspUpdated(
            servers
                .into_iter()
                .map(|s| crate::LspStatusUpdate {
                    id: s.id,
                    name: s.name,
                    root: s.root,
                    connected: s.connected,
                })
                .collect(),
        ),
        Update::McpUpdated { servers } => AppUpdate::McpUpdated(
            servers
                .into_iter()
                .map(|s| crate::McpStatusUpdate {
                    name: s.name,
                    connected: s.connected,
                    error: s.error,
                })
                .collect(),
        ),
        Update::ModifiedFilesUpdated { files } => AppUpdate::ModifiedFilesUpdated(
            files
                .into_iter()
                .map(|f| crate::ModifiedFileUpdate {
                    path: f.path,
                    added: f.added,
                    removed: f.removed,
                })
                .collect(),
        ),
        Update::PermissionsPending { count } => AppUpdate::PermissionsPending(count),
        Update::SandboxUpdated {
            state,
            runtime_type,
            error,
        } => AppUpdate::SandboxUpdated(crate::SandboxStatusUpdate {
            state,
            runtime_type,
            error,
        }),
        Update::SystemMessage { message } => AppUpdate::SystemMessage(message),
        Update::AgentChanged { agent } => AppUpdate::AgentChanged(agent),
        Update::PermissionRequest {
            id,
            tool,
            action,
            description,
            path,
        } => AppUpdate::PermissionRequest(crate::PermissionRequestUpdate {
            id,
            tool,
            action,
            description,
            path,
        }),
    }
}
