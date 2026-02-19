//! Backend abstraction for TUI communication.
//!
//! This module provides a trait for backend communication, allowing the TUI
//! to work with either a local runner (direct channels) or a remote server (HTTP/SSE).

use crate::{AppAction, AppUpdate, GitCommitUpdate, GitFileUpdate, GitStatusUpdate};
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

    /// Convenience method: Change the AI model
    async fn change_model(&self, model_id: String) -> BackendResult<()> {
        self.send_action(AppAction::ChangeModel(model_id)).await
    }

    /// Convenience method: Start the sandbox (Docker)
    ///
    /// This sends a start command and waits for the sandbox to report running status.
    /// Returns the container ID once available, or an error if start fails or times out.
    async fn start_sandbox(&self) -> BackendResult<String>;

    /// Convenience method: Stop the sandbox (Docker)
    async fn stop_sandbox(&self) -> BackendResult<()> {
        self.send_action(AppAction::SandboxStop).await
    }

    /// Convenience method: Restart the sandbox (Docker)
    async fn restart_sandbox(&self) -> BackendResult<String> {
        self.send_action(AppAction::SandboxRestart).await?;
        // TODO: Return actual container ID from backend response
        Ok("sandbox-container".to_string())
    }

    /// Convenience method: Stop the agent
    async fn stop_agent(&self) -> BackendResult<()> {
        self.send_action(AppAction::Quit).await
    }
}

/// Local backend using direct tokio channels.
///
/// This is used when the TUI and runner are in the same process.
pub struct LocalBackend {
    action_tx: mpsc::UnboundedSender<AppAction>,
    update_rx: tokio::sync::broadcast::Sender<AppUpdate>,
}

impl LocalBackend {
    /// Create a new local backend with the given action sender and update receiver.
    pub fn new(
        action_tx: mpsc::UnboundedSender<AppAction>,
        update_rx: tokio::sync::broadcast::Sender<AppUpdate>,
    ) -> Self {
        Self {
            action_tx,
            update_rx,
        }
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

    async fn start_sandbox(&self) -> BackendResult<String> {
        // Send the start action
        self.send_action(AppAction::SandboxStart).await?;

        // Subscribe to updates to wait for the sandbox to start
        let mut rx = self.update_rx.subscribe();
        let timeout = tokio::time::Duration::from_secs(30);

        match tokio::time::timeout(timeout, async {
            loop {
                match rx.recv().await {
                    Ok(AppUpdate::SandboxUpdated(info)) => {
                        match info.state.as_str() {
                            "running" => {
                                if let Some(container_id) = info.container_id {
                                    return Ok(container_id);
                                } else {
                                    return Err(BackendError::RequestFailed(
                                        "Sandbox started but no container ID received".to_string(),
                                    ));
                                }
                            }
                            "error" => {
                                return Err(BackendError::RequestFailed(
                                    info.error.unwrap_or_else(|| "Unknown error".to_string()),
                                ));
                            }
                            _ => continue, // Keep waiting for "running" or "error"
                        }
                    }
                    Err(e) => {
                        return Err(BackendError::RequestFailed(format!(
                            "Failed to receive sandbox update: {}",
                            e
                        )));
                    }
                    _ => continue, // Ignore other update types
                }
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Err(BackendError::RequestFailed(
                "Timeout waiting for sandbox to start".to_string(),
            )),
        }
    }

    async fn stop_sandbox(&self) -> BackendResult<()> {
        // Send the stop action
        self.send_action(AppAction::SandboxStop).await?;

        // Subscribe to updates to wait for the sandbox to stop
        // This is CRITICAL for multi-workstream setups - we must wait for the
        // sandbox to fully stop and the PermissionManager to be cleared before
        // switching to another workstream, otherwise MCP tools will use stale runtime.
        let mut rx = self.update_rx.subscribe();
        let timeout = tokio::time::Duration::from_secs(10);

        match tokio::time::timeout(timeout, async {
            loop {
                match rx.recv().await {
                    Ok(AppUpdate::SandboxUpdated(info)) => {
                        match info.state.as_str() {
                            "stopped" => {
                                return Ok(());
                            }
                            "error" => {
                                // Even on error, consider it stopped
                                return Ok(());
                            }
                            _ => continue, // Keep waiting for "stopped" or "error"
                        }
                    }
                    Err(e) => {
                        return Err(BackendError::RequestFailed(format!(
                            "Failed to receive sandbox update: {}",
                            e
                        )));
                    }
                    _ => continue, // Ignore other update types
                }
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                // Timeout is not fatal for stop - log and continue
                tracing::warn!("Timeout waiting for sandbox to stop, continuing anyway");
                Ok(())
            }
        }
    }
}

/// Remote backend using HTTP for actions and SSE for updates.
///
/// This is used when connecting to a remote headless agent server.
pub struct RemoteBackend {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    connected: std::sync::atomic::AtomicBool,
    /// Sender for updates (used for git operations that need to send updates back).
    update_tx: Option<mpsc::UnboundedSender<AppUpdate>>,
}

impl RemoteBackend {
    /// Create a new remote backend connecting to the given address.
    pub fn new(address: &str) -> BackendResult<Self> {
        Self::with_api_key(address, None)
    }

    /// Create a new remote backend with optional API key authentication.
    pub fn with_api_key(address: &str, api_key: Option<String>) -> BackendResult<Self> {
        let base_url = if address.starts_with("http://") || address.starts_with("https://") {
            address.to_string()
        } else {
            format!("http://{address}")
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| BackendError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            api_key,
            connected: std::sync::atomic::AtomicBool::new(false),
            update_tx: None,
        })
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Add API key header to a request if configured.
    fn add_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref key) = self.api_key {
            request.header("X-API-Key", key)
        } else {
            request
        }
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
            .add_auth(self.client.get(&url))
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
        let api_key = self.api_key.clone();

        tokio::spawn(async move {
            use futures::StreamExt;

            loop {
                let mut request = client.get(&url);
                if let Some(ref key) = api_key {
                    request = request.header("X-API-Key", key);
                }

                match request.send().await {
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

        self.add_auth(self.client.post(&url))
            .json(&action)
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        Ok(())
    }

    /// Set the update sender for git operations.
    pub fn set_update_sender(&mut self, update_tx: mpsc::UnboundedSender<AppUpdate>) {
        self.update_tx = Some(update_tx);
    }

    /// Send an update via the update channel.
    fn send_update(&self, update: AppUpdate) {
        if let Some(ref tx) = self.update_tx {
            let _ = tx.send(update);
        }
    }

    /// Handle git status request.
    async fn handle_git_status(&self) -> BackendResult<()> {
        let url = format!("{}/git/status", self.base_url);
        let resp = self
            .add_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        if resp.status().is_success() {
            let status: GitStatusResponse = resp
                .json()
                .await
                .map_err(|e| BackendError::SerializationError(e.to_string()))?;

            self.send_update(AppUpdate::GitStatusUpdated(GitStatusUpdate {
                branch: status.branch,
                ahead: status.ahead,
                behind: status.behind,
                files: status
                    .files
                    .into_iter()
                    .map(|f| GitFileUpdate {
                        path: f.path,
                        status: f.status,
                        staged: f.staged,
                    })
                    .collect(),
            }));
        } else {
            let error = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            self.send_update(AppUpdate::GitOperationResult {
                success: false,
                message: error,
            });
        }
        Ok(())
    }

    /// Handle git stage request.
    async fn handle_git_stage(&self, paths: Vec<String>) -> BackendResult<()> {
        let url = format!("{}/git/stage", self.base_url);
        let resp = self
            .add_auth(self.client.post(&url))
            .json(&serde_json::json!({ "paths": paths }))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        let success = resp.status().is_success();
        let message = resp.text().await.unwrap_or_default();

        self.send_update(AppUpdate::GitOperationResult { success, message });

        // Refresh status after staging
        if success {
            let _ = self.handle_git_status().await;
        }
        Ok(())
    }

    /// Handle git unstage request.
    async fn handle_git_unstage(&self, paths: Vec<String>) -> BackendResult<()> {
        let url = format!("{}/git/unstage", self.base_url);
        let resp = self
            .add_auth(self.client.post(&url))
            .json(&serde_json::json!({ "paths": paths }))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        let success = resp.status().is_success();
        let message = resp.text().await.unwrap_or_default();

        self.send_update(AppUpdate::GitOperationResult { success, message });

        // Refresh status after unstaging
        if success {
            let _ = self.handle_git_status().await;
        }
        Ok(())
    }

    /// Handle git checkout request.
    async fn handle_git_checkout(&self, paths: Vec<String>) -> BackendResult<()> {
        let url = format!("{}/git/checkout", self.base_url);
        let resp = self
            .add_auth(self.client.post(&url))
            .json(&serde_json::json!({ "paths": paths }))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        let success = resp.status().is_success();
        let message = resp.text().await.unwrap_or_default();

        self.send_update(AppUpdate::GitOperationResult { success, message });

        // Refresh status after checkout
        if success {
            let _ = self.handle_git_status().await;
        }
        Ok(())
    }

    /// Handle git commit request.
    async fn handle_git_commit(&self, message: String) -> BackendResult<()> {
        let url = format!("{}/git/commit", self.base_url);
        let resp = self
            .add_auth(self.client.post(&url))
            .json(&serde_json::json!({ "message": message }))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        let success = resp.status().is_success();
        let result_message = resp.text().await.unwrap_or_default();

        self.send_update(AppUpdate::GitOperationResult {
            success,
            message: result_message,
        });

        // Refresh status after commit
        if success {
            let _ = self.handle_git_status().await;
        }
        Ok(())
    }

    /// Handle git history request.
    async fn handle_git_history(&self) -> BackendResult<()> {
        let url = format!("{}/git/history", self.base_url);
        let resp = self
            .add_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        if resp.status().is_success() {
            let commits: Vec<GitCommitResponse> = resp
                .json()
                .await
                .map_err(|e| BackendError::SerializationError(e.to_string()))?;

            self.send_update(AppUpdate::GitHistoryUpdated(
                commits
                    .into_iter()
                    .map(|c| GitCommitUpdate {
                        id: c.id,
                        message: c.message,
                        author: c.author,
                        date: c.date,
                    })
                    .collect(),
            ));
        } else {
            let error = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            self.send_update(AppUpdate::GitOperationResult {
                success: false,
                message: error,
            });
        }
        Ok(())
    }

    /// Handle git push request.
    async fn handle_git_push(&self) -> BackendResult<()> {
        let url = format!("{}/git/push", self.base_url);
        let resp = self
            .add_auth(self.client.post(&url))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        let success = resp.status().is_success();
        let message = resp.text().await.unwrap_or_default();

        self.send_update(AppUpdate::GitOperationResult { success, message });

        // Refresh status after push
        if success {
            let _ = self.handle_git_status().await;
        }
        Ok(())
    }

    /// Handle git pull request.
    async fn handle_git_pull(&self) -> BackendResult<()> {
        let url = format!("{}/git/pull", self.base_url);
        let resp = self
            .add_auth(self.client.post(&url))
            .send()
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        let success = resp.status().is_success();
        let message = resp.text().await.unwrap_or_default();

        self.send_update(AppUpdate::GitOperationResult { success, message });

        // Refresh status after pull
        if success {
            let _ = self.handle_git_status().await;
        }
        Ok(())
    }
}

/// Response structure for git status endpoint.
#[derive(Debug, serde::Deserialize)]
struct GitStatusResponse {
    branch: String,
    ahead: usize,
    behind: usize,
    files: Vec<GitFileResponse>,
}

/// Response structure for individual git file.
#[derive(Debug, serde::Deserialize)]
struct GitFileResponse {
    path: String,
    status: String,
    staged: bool,
}

/// Response structure for git commit info.
#[derive(Debug, serde::Deserialize)]
struct GitCommitResponse {
    id: String,
    message: String,
    author: String,
    date: String,
}

#[async_trait]
impl Backend for RemoteBackend {
    async fn send_action(&self, action: AppAction) -> BackendResult<()> {
        // Handle git actions specially via direct HTTP calls
        match action {
            AppAction::GitStatus => return self.handle_git_status().await,
            AppAction::GitStage { paths } => return self.handle_git_stage(paths).await,
            AppAction::GitUnstage { paths } => return self.handle_git_unstage(paths).await,
            AppAction::GitCheckout { paths } => return self.handle_git_checkout(paths).await,
            AppAction::GitCommit { message } => return self.handle_git_commit(message).await,
            AppAction::GitHistory => return self.handle_git_history().await,
            AppAction::GitPush => return self.handle_git_push().await,
            AppAction::GitPull => return self.handle_git_pull().await,
            _ => {}
        }

        // Handle other actions via protocol conversion
        let protocol_action = app_action_to_protocol(action)?;
        self.send_protocol_action(protocol_action).await
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn backend_type(&self) -> &'static str {
        "remote"
    }

    async fn start_sandbox(&self) -> BackendResult<String> {
        // For RemoteBackend, send the action and return a placeholder.
        // The actual status updates come through SSE.
        self.send_action(AppAction::SandboxStart).await?;
        Ok("remote-sandbox".to_string())
    }
}

/// Convert AppAction to protocol Action.
fn app_action_to_protocol(action: AppAction) -> BackendResult<wonopcode_protocol::Action> {
    use wonopcode_protocol::Action;

    Ok(match action {
        AppAction::SendPrompt(prompt) => Action::SendPrompt {
            prompt,
            images: vec![],
        },
        AppAction::SendPromptWithImages { prompt, images } => Action::SendPrompt { prompt, images },
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
        AppAction::SetAllowAll { enabled } => Action::SetAllowAll { enabled },
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
        AppAction::EnableAllowAll { request_id } => Action::EnableAllowAll { request_id },
        // OpenEditor is handled locally, not sent to server
        AppAction::OpenEditor { .. } => {
            return Err(BackendError::RequestFailed(
                "OpenEditor is not supported for remote backend".to_string(),
            ));
        }
        // Git actions are handled specially via HTTP, should not reach here
        AppAction::GitStatus
        | AppAction::GitStage { .. }
        | AppAction::GitUnstage { .. }
        | AppAction::GitCheckout { .. }
        | AppAction::GitCommit { .. }
        | AppAction::GitHistory
        | AppAction::GitPush
        | AppAction::GitPull => {
            return Err(BackendError::RequestFailed(
                "Git actions should be handled via HTTP endpoints".to_string(),
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
            accumulated_input,
            accumulated_output,
            accumulated_cost,
            last_request_input,
            last_request_output,
            last_request_cache_read,
        } => AppUpdate::TokenUsage {
            input,
            output,
            cost,
            context_limit,
            accumulated_input,
            accumulated_output,
            accumulated_cost,
            last_request_input,
            last_request_output,
            last_request_cache_read,
        },
        Update::ModelInfo { context_limit } => AppUpdate::ModelInfo { context_limit },
        Update::Sessions { sessions } => AppUpdate::Sessions(
            sessions
                .into_iter()
                .map(|s| (s.id, s.title, s.timestamp))
                .collect(),
        ),
        Update::TodosUpdated { phases, todos } => AppUpdate::TodosUpdated {
            phases: phases
                .into_iter()
                .map(|p| crate::PhaseUpdate {
                    id: p.id,
                    name: p.name,
                    status: p.status,
                    todos: p
                        .todos
                        .into_iter()
                        .map(|t| crate::TodoUpdate {
                            id: t.id,
                            content: t.content,
                            status: t.status,
                            priority: t.priority,
                            phase_id: t.phase_id,
                            parents: t.parents,
                        })
                        .collect(),
                })
                .collect(),
            todos: todos
                .into_iter()
                .map(|t| crate::TodoUpdate {
                    id: t.id,
                    content: t.content,
                    status: t.status,
                    priority: t.priority,
                    phase_id: t.phase_id,
                    parents: t.parents,
                })
                .collect(),
        },
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
            container_id,
        } => AppUpdate::SandboxUpdated(crate::SandboxStatusUpdate {
            state,
            runtime_type,
            error,
            container_id,
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
        Update::PermissionResolved {
            request_id,
            allowed,
        } => AppUpdate::PermissionResolved {
            request_id,
            allowed,
        },
    }
}

// ============================================================================
// IggyBackend - Apache Iggy-based transport for client-server communication
// ============================================================================

use std::sync::Arc;
use tokio::sync::RwLock;
use wonopcode_message::{ClientMessage, ClientPayload, ServerPayload, WorkstreamId};
use wonopcode_transport::{Transport, TransportConfig};

/// Iggy-based backend for TUI communication via Apache Iggy message streaming.
///
/// This backend connects to an Iggy server and communicates with the agent
/// server through message topics. It supports:
/// - Reliable message delivery with persistence
/// - Offset-based replay for reconnecting clients
/// - Workstream routing for Pro edition
pub struct IggyBackend {
    /// The transport layer for Iggy communication.
    transport: Arc<Transport>,
    /// The workstream to communicate with (default for Community Edition).
    workstream_id: WorkstreamId,
    /// Update sender for forwarding server messages to the TUI.
    update_tx: Option<mpsc::UnboundedSender<AppUpdate>>,
    /// Last seen message offset for replay on reconnect.
    last_offset: Arc<RwLock<u64>>,
    /// Consumer ID for this client.
    consumer_id: String,
}

impl IggyBackend {
    /// Create a new Iggy backend connecting to the specified server.
    ///
    /// # Arguments
    /// * `iggy_address` - The Iggy server address (e.g., "127.0.0.1:8090")
    /// * `workstream_id` - Optional workstream ID (defaults to "default")
    pub async fn connect(
        iggy_address: &str,
        workstream_id: Option<WorkstreamId>,
    ) -> BackendResult<Self> {
        let config = TransportConfig {
            server_address: iggy_address.to_string(),
            auto_create: false, // Server should create infrastructure
            ..Default::default()
        };

        let transport = Transport::connect(config)
            .await
            .map_err(|e| BackendError::ConnectionFailed(e.to_string()))?;

        let consumer_id = format!("tui-client-{}", uuid::Uuid::new_v4());

        Ok(Self {
            transport: Arc::new(transport),
            workstream_id: workstream_id.unwrap_or_default(),
            update_tx: None,
            last_offset: Arc::new(RwLock::new(0)),
            consumer_id,
        })
    }

    /// Subscribe to server messages and forward them to the given sender.
    ///
    /// This spawns a background task that receives server messages from Iggy
    /// and converts them to AppUpdate messages for the TUI.
    pub fn subscribe_updates(
        &mut self,
        update_tx: mpsc::UnboundedSender<AppUpdate>,
    ) -> tokio::task::JoinHandle<()> {
        self.update_tx = Some(update_tx.clone());

        let transport = Arc::clone(&self.transport);
        let workstream_id = self.workstream_id.clone();
        let last_offset = Arc::clone(&self.last_offset);
        let consumer_id = self.consumer_id.clone();

        tokio::spawn(async move {
            // Get current offset for replay
            let from_offset = {
                let offset = last_offset.read().await;
                if *offset > 0 {
                    Some(*offset)
                } else {
                    None
                }
            };

            // Subscribe to server messages
            let rx = match transport
                .subscribe_server_messages(&consumer_id, Some(workstream_id), from_offset)
                .await
            {
                Ok(rx) => rx,
                Err(e) => {
                    tracing::error!("Failed to subscribe to server messages: {}", e);
                    let _ = update_tx.send(AppUpdate::Error(format!(
                        "Failed to connect to message stream: {}",
                        e
                    )));
                    return;
                }
            };

            Self::process_server_messages(rx, update_tx, last_offset).await;
        })
    }

    /// Process incoming server messages and convert to AppUpdate.
    async fn process_server_messages(
        mut rx: mpsc::UnboundedReceiver<wonopcode_message::ServerMessage>,
        update_tx: mpsc::UnboundedSender<AppUpdate>,
        last_offset: Arc<RwLock<u64>>,
    ) {
        while let Some(server_msg) = rx.recv().await {
            // Update last seen offset
            {
                let mut offset = last_offset.write().await;
                if server_msg.offset > *offset {
                    *offset = server_msg.offset;
                }
            }

            // Convert ServerPayload to AppUpdate(s)
            for update in server_payload_to_app_updates(server_msg.payload) {
                if update_tx.send(update).is_err() {
                    tracing::info!("Update channel closed, stopping message processor");
                    return;
                }
            }
        }

        tracing::info!("Server message stream ended");
    }

    /// Set the update sender for direct use.
    pub fn set_update_sender(&mut self, update_tx: mpsc::UnboundedSender<AppUpdate>) {
        self.update_tx = Some(update_tx);
    }

    /// Get the last seen message offset (for reconnection).
    pub async fn last_offset(&self) -> u64 {
        *self.last_offset.read().await
    }
}

#[async_trait]
impl Backend for IggyBackend {
    async fn send_action(&self, action: AppAction) -> BackendResult<()> {
        // Convert AppAction to ClientPayload
        let payload = app_action_to_client_payload(action)?;

        // Wrap in ClientMessage envelope
        let message = ClientMessage::new(self.workstream_id.clone(), payload);

        // Send via transport
        self.transport
            .send_client_message(message)
            .await
            .map_err(|e| BackendError::RequestFailed(e.to_string()))?;

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    fn backend_type(&self) -> &'static str {
        "iggy"
    }

    async fn start_sandbox(&self) -> BackendResult<String> {
        // For IggyBackend, send the action and return a placeholder.
        // The actual status updates come through Iggy message stream.
        self.send_action(AppAction::SandboxStart).await?;
        Ok("iggy-sandbox".to_string())
    }
}

/// Convert AppAction to ClientPayload for Iggy transport.
fn app_action_to_client_payload(action: AppAction) -> BackendResult<ClientPayload> {
    Ok(match action {
        AppAction::SendPrompt(prompt) => ClientPayload::SendPrompt { prompt },
        AppAction::SendPromptWithImages { prompt, images: _ } => {
            // TODO: Add image support to Iggy transport
            // For now, just send the text prompt
            ClientPayload::SendPrompt { prompt }
        }
        AppAction::Cancel => ClientPayload::Cancel,
        AppAction::Quit => ClientPayload::Quit,
        AppAction::SwitchSession(session_id) => ClientPayload::SwitchSession { session_id },
        AppAction::ChangeModel(model) => ClientPayload::ChangeModel { model },
        AppAction::ChangeAgent(agent) => ClientPayload::ChangeAgent { agent },
        AppAction::NewSession => ClientPayload::NewSession,
        AppAction::Undo => ClientPayload::Undo,
        AppAction::Redo => ClientPayload::Redo,
        AppAction::Revert { message_id } => ClientPayload::Revert { message_id },
        AppAction::Unrevert => ClientPayload::Unrevert,
        AppAction::Compact => ClientPayload::Compact,
        AppAction::RenameSession { title } => ClientPayload::RenameSession { title },
        AppAction::McpToggle { name } => ClientPayload::McpToggle { name },
        AppAction::McpReconnect { name } => ClientPayload::McpReconnect { name },
        AppAction::ForkSession { message_id } => ClientPayload::ForkSession { message_id },
        AppAction::ShareSession => ClientPayload::ShareSession,
        AppAction::UnshareSession => ClientPayload::UnshareSession,
        AppAction::GotoMessage { message_id } => ClientPayload::GotoMessage { message_id },
        AppAction::SandboxStart => ClientPayload::SandboxStart,
        AppAction::SandboxStop => ClientPayload::SandboxStop,
        AppAction::SandboxRestart => ClientPayload::SandboxRestart,
        AppAction::SetAllowAll { enabled } => ClientPayload::SetAllowAll { enabled },
        AppAction::SaveSettings { scope, config } => {
            let message_scope = match scope {
                crate::SaveScope::Project => wonopcode_message::SaveScope::Project,
                crate::SaveScope::Global => wonopcode_message::SaveScope::Global,
            };
            ClientPayload::SaveSettings {
                scope: message_scope,
                config: serde_json::to_value(&*config)
                    .map_err(|e| BackendError::SerializationError(e.to_string()))?,
            }
        }
        AppAction::PermissionResponse {
            request_id,
            allow,
            remember,
        } => ClientPayload::PermissionResponse {
            request_id,
            allow,
            remember,
        },
        AppAction::EnableAllowAll { request_id } => ClientPayload::EnableAllowAll { request_id },
        // OpenEditor is handled locally, not sent to server
        AppAction::OpenEditor { .. } => {
            return Err(BackendError::RequestFailed(
                "OpenEditor is not supported for Iggy backend".to_string(),
            ));
        }
        // UpdateTestProviderSettings not in the unified protocol yet
        AppAction::UpdateTestProviderSettings { .. } => {
            return Err(BackendError::RequestFailed(
                "UpdateTestProviderSettings is not yet supported via Iggy".to_string(),
            ));
        }
        // Git operations - not in the unified protocol yet, handle via Ping for now
        // TODO: Add Git payloads to ClientPayload in wonopcode-message
        AppAction::GitStatus
        | AppAction::GitStage { .. }
        | AppAction::GitUnstage { .. }
        | AppAction::GitCheckout { .. }
        | AppAction::GitCommit { .. }
        | AppAction::GitHistory
        | AppAction::GitPush
        | AppAction::GitPull => {
            return Err(BackendError::RequestFailed(
                "Git operations are not yet supported via Iggy backend".to_string(),
            ));
        }
    })
}

/// Convert ServerPayload to AppUpdate(s) for the TUI.
///
/// Most payloads map to a single AppUpdate, but State payloads can generate
/// multiple updates to properly sync all UI state (tokens, sessions, etc.).
fn server_payload_to_app_updates(payload: ServerPayload) -> Vec<AppUpdate> {
    match payload {
        ServerPayload::Started => vec![AppUpdate::Started],
        ServerPayload::TextDelta { delta } => vec![AppUpdate::TextDelta(delta)],
        ServerPayload::ToolStarted { id, name, input } => {
            vec![AppUpdate::ToolStarted { name, id, input }]
        }
        ServerPayload::ToolCompleted {
            id,
            success,
            output,
            metadata,
        } => vec![AppUpdate::ToolCompleted {
            id,
            success,
            output,
            metadata,
        }],
        ServerPayload::Completed { text } => vec![AppUpdate::Completed { text }],
        ServerPayload::Error { error } => vec![AppUpdate::Error(error)],
        ServerPayload::Status { message } => vec![AppUpdate::Status(message)],
        ServerPayload::TokenUsage {
            input,
            output,
            cost,
            context_limit,
            accumulated_input,
            accumulated_output,
            accumulated_cost,
            last_request_input,
            last_request_output,
            last_request_cache_read,
        } => vec![AppUpdate::TokenUsage {
            input,
            output,
            cost,
            context_limit,
            accumulated_input,
            accumulated_output,
            accumulated_cost,
            last_request_input,
            last_request_output,
            last_request_cache_read,
        }],
        ServerPayload::ModelInfo { context_limit } => vec![AppUpdate::ModelInfo { context_limit }],
        ServerPayload::Sessions { sessions } => vec![AppUpdate::Sessions(
            sessions
                .into_iter()
                .map(|s| (s.id, s.title, s.timestamp))
                .collect(),
        )],
        ServerPayload::TodosUpdated { phases, todos } => vec![AppUpdate::TodosUpdated {
            phases: phases
                .into_iter()
                .map(|p| crate::PhaseUpdate {
                    id: p.id,
                    name: p.name,
                    status: p.status,
                    todos: p
                        .todos
                        .into_iter()
                        .map(|t| crate::TodoUpdate {
                            id: t.id,
                            content: t.content,
                            status: t.status,
                            priority: t.priority,
                            phase_id: t.phase_id,
                            parents: t.parents,
                        })
                        .collect(),
                })
                .collect(),
            todos: todos
                .into_iter()
                .map(|t| crate::TodoUpdate {
                    id: t.id,
                    content: t.content,
                    status: t.status,
                    priority: t.priority,
                    phase_id: t.phase_id,
                    parents: t.parents,
                })
                .collect(),
        }],
        ServerPayload::LspUpdated { servers } => vec![AppUpdate::LspUpdated(
            servers
                .into_iter()
                .map(|s| crate::LspStatusUpdate {
                    id: s.id,
                    name: s.name,
                    root: s.root,
                    connected: s.connected,
                })
                .collect(),
        )],
        ServerPayload::McpUpdated { servers } => vec![AppUpdate::McpUpdated(
            servers
                .into_iter()
                .map(|s| crate::McpStatusUpdate {
                    name: s.name,
                    connected: s.connected,
                    error: s.error,
                })
                .collect(),
        )],
        ServerPayload::ModifiedFilesUpdated { files } => vec![AppUpdate::ModifiedFilesUpdated(
            files
                .into_iter()
                .map(|f| crate::ModifiedFileUpdate {
                    path: f.path,
                    added: f.added,
                    removed: f.removed,
                })
                .collect(),
        )],
        ServerPayload::PermissionsPending { count } => vec![AppUpdate::PermissionsPending(count)],
        ServerPayload::SandboxUpdated {
            state,
            runtime_type,
            error,
            container_id,
        } => vec![AppUpdate::SandboxUpdated(crate::SandboxStatusUpdate {
            state,
            runtime_type,
            error,
            container_id,
        })],
        ServerPayload::SystemMessage { message } => vec![AppUpdate::SystemMessage(message)],
        ServerPayload::AgentChanged { agent } => vec![AppUpdate::AgentChanged(agent)],
        ServerPayload::PermissionRequest {
            id,
            tool,
            action,
            description,
            path,
        } => vec![AppUpdate::PermissionRequest(crate::PermissionRequestUpdate {
            id,
            tool,
            action,
            description,
            path,
        })],
        ServerPayload::PermissionResolved {
            request_id,
            allowed,
        } => vec![AppUpdate::PermissionResolved {
            request_id,
            allowed,
        }],
        // State payload contains full state - extract relevant updates
        ServerPayload::State(state) => {
            let mut updates = Vec::new();

            // Send token usage if we have any
            if state.token_usage.input > 0 || state.token_usage.output > 0 {
                updates.push(AppUpdate::TokenUsage {
                    input: state.token_usage.input,
                    output: state.token_usage.output,
                    cost: state.token_usage.cost,
                    context_limit: state.context_limit,
                    // State doesn't track accumulated values separately
                    accumulated_input: None,
                    accumulated_output: None,
                    accumulated_cost: None,
                    last_request_input: None,
                    last_request_output: None,
                    last_request_cache_read: None,
                });
            }

            // Send model info for context limit
            updates.push(AppUpdate::ModelInfo {
                context_limit: state.context_limit,
            });

            // Send sessions list
            if !state.sessions.is_empty() {
                updates.push(AppUpdate::Sessions(
                    state
                        .sessions
                        .into_iter()
                        .map(|s| (s.id, s.title, s.timestamp))
                        .collect(),
                ));
            }

            // Send todos if any
            if !state.phases.is_empty() || !state.todos.is_empty() {
                updates.push(AppUpdate::TodosUpdated {
                    phases: state
                        .phases
                        .into_iter()
                        .map(|p| crate::PhaseUpdate {
                            id: p.id,
                            name: p.name,
                            status: p.status,
                            todos: p
                                .todos
                                .into_iter()
                                .map(|t| crate::TodoUpdate {
                                    id: t.id,
                                    content: t.content,
                                    status: t.status,
                                    priority: t.priority,
                                    phase_id: t.phase_id,
                                    parents: t.parents,
                                })
                                .collect(),
                        })
                        .collect(),
                    todos: state
                        .todos
                        .into_iter()
                        .map(|t| crate::TodoUpdate {
                            id: t.id,
                            content: t.content,
                            status: t.status,
                            priority: t.priority,
                            phase_id: t.phase_id,
                            parents: t.parents,
                        })
                        .collect(),
                });
            }

            // Send LSP status if any
            if !state.lsp_servers.is_empty() {
                updates.push(AppUpdate::LspUpdated(
                    state
                        .lsp_servers
                        .into_iter()
                        .map(|s| crate::LspStatusUpdate {
                            id: s.id,
                            name: s.name,
                            root: s.root,
                            connected: s.connected,
                        })
                        .collect(),
                ));
            }

            // Send MCP status if any
            if !state.mcp_servers.is_empty() {
                updates.push(AppUpdate::McpUpdated(
                    state
                        .mcp_servers
                        .into_iter()
                        .map(|s| crate::McpStatusUpdate {
                            name: s.name,
                            connected: s.connected,
                            error: s.error,
                        })
                        .collect(),
                ));
            }

            // Send sandbox status
            updates.push(AppUpdate::SandboxUpdated(crate::SandboxStatusUpdate {
                state: state.sandbox.state,
                runtime_type: state.sandbox.runtime_type,
                error: state.sandbox.error,
                container_id: None,
            }));

            // Send modified files if any
            if !state.modified_files.is_empty() {
                updates.push(AppUpdate::ModifiedFilesUpdated(
                    state
                        .modified_files
                        .into_iter()
                        .map(|f| crate::ModifiedFileUpdate {
                            path: f.path,
                            added: f.added,
                            removed: f.removed,
                        })
                        .collect(),
                ));
            }

            // Send agent mode
            if !state.agent.is_empty() {
                updates.push(AppUpdate::AgentChanged(state.agent));
            }

            updates
        }
        // Workstream payloads - Pro edition only, ignore for now
        ServerPayload::WorkstreamList { .. }
        | ServerPayload::WorkstreamCreated { .. }
        | ServerPayload::WorkstreamConnected { .. }
        | ServerPayload::WorkstreamDisconnected
        | ServerPayload::WorkstreamRemoved
        | ServerPayload::WorkstreamActivated { .. }
        | ServerPayload::WorkstreamDeactivated
        | ServerPayload::WorktreeCreated { .. }
        | ServerPayload::WorktreeDeleted
        | ServerPayload::WorkstreamsRefreshed { .. }
        | ServerPayload::WorkstreamEvent { .. }
        | ServerPayload::ConversationHistory { .. } => vec![],
        // Control payloads
        ServerPayload::Pong => vec![], // Internal ping/pong, don't expose to UI
        ServerPayload::ServerStatus { .. } => vec![], // Internal status
        // Agent control payloads - handled by Pro edition UI, not TUI
        ServerPayload::ModelChanged { .. }
        | ServerPayload::SandboxStarted { .. }
        | ServerPayload::SandboxStopped
        | ServerPayload::SandboxRestarted { .. }
        | ServerPayload::SandboxError { .. }
        | ServerPayload::AgentStopped
        | ServerPayload::SessionStats { .. }
        | ServerPayload::AvailableModels { .. }
        | ServerPayload::AllowAllChanged { .. } => vec![],
    }
}
