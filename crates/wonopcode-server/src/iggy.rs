//! Iggy-based agent server for Wonopcode.
//!
//! This module provides an agent server that uses Apache Iggy for message transport
//! instead of HTTP/SSE. This enables:
//! - Higher throughput for streaming responses
//! - Offset-based replay for reconnecting clients
//! - Unified protocol with Pro edition workstreams
//!
//! # Architecture
//!
//! ```text
//! TUI Client                    AgentServer
//! +---------+                   +-----------+
//! |         |  ClientMessage    |           |
//! |  Iggy   | ----------------> |   Iggy    |
//! | Client  |                   |  Server   |
//! |         | <---------------- |           |
//! +---------+  ServerMessage    +-----------+
//!              (via Iggy)           |
//!                                   v
//!                               +-------+
//!                               | Agent |
//!                               | Runner|
//!                               +-------+
//! ```

use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use wonopcode_message::{
    AgentState, ClientMessage, ClientPayload, ServerMessage, ServerPayload, WorkstreamId,
};
use wonopcode_transport::{Transport, TransportConfig, TransportError};

/// Errors from the AgentServer.
#[derive(Debug, thiserror::Error)]
pub enum AgentServerError {
    /// Transport error.
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    /// Channel closed.
    #[error("Channel closed")]
    ChannelClosed,

    /// Server not running.
    #[error("Server not running")]
    NotRunning,
}

/// Agent server using Iggy transport.
///
/// This is the server-side component that:
/// - Subscribes to client messages from Iggy
/// - Dispatches them to the agent runner
/// - Publishes agent responses back to Iggy
pub struct AgentServer {
    /// Transport for Iggy communication.
    transport: Transport,
    /// Current state for synchronization.
    current_state: Arc<RwLock<AgentState>>,
    /// Workstream ID (default for Community Edition).
    workstream_id: WorkstreamId,
    /// Shutdown flag.
    shutdown: Arc<RwLock<bool>>,
}

impl AgentServer {
    /// Create a new agent server.
    pub async fn new(config: TransportConfig) -> Result<Self, AgentServerError> {
        info!("Starting AgentServer with Iggy transport");
        let transport = Transport::connect(config).await?;

        Ok(Self {
            transport,
            current_state: Arc::new(RwLock::new(AgentState::default())),
            workstream_id: WorkstreamId::default(),
            shutdown: Arc::new(RwLock::new(false)),
        })
    }

    /// Create a new agent server with a specific workstream ID (for Pro edition).
    pub async fn new_with_workstream(
        config: TransportConfig,
        workstream_id: WorkstreamId,
    ) -> Result<Self, AgentServerError> {
        info!(
            "Starting AgentServer for workstream '{}' with Iggy transport",
            workstream_id
        );
        let transport = Transport::connect(config).await?;

        Ok(Self {
            transport,
            current_state: Arc::new(RwLock::new(AgentState::default())),
            workstream_id,
            shutdown: Arc::new(RwLock::new(false)),
        })
    }

    /// Get the workstream ID.
    pub fn workstream_id(&self) -> &WorkstreamId {
        &self.workstream_id
    }

    /// Get the current state.
    pub async fn current_state(&self) -> AgentState {
        self.current_state.read().await.clone()
    }

    /// Update the current state.
    pub async fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut AgentState),
    {
        let mut state = self.current_state.write().await;
        f(&mut state);
    }

    /// Send a server update to clients via Iggy.
    pub async fn send_update(&self, payload: ServerPayload) -> Result<(), AgentServerError> {
        let message = ServerMessage::new(self.workstream_id.clone(), payload);
        self.transport.send_server_message(message).await?;
        Ok(())
    }

    /// Run the server, processing client messages.
    ///
    /// This method:
    /// 1. Subscribes to client messages from Iggy
    /// 2. Forwards them to the provided action handler
    /// 3. Runs until shutdown is requested
    ///
    /// # Arguments
    ///
    /// * `action_tx` - Channel to send actions to the agent runner
    pub async fn run(
        &self,
        action_tx: mpsc::UnboundedSender<ClientPayload>,
    ) -> Result<(), AgentServerError> {
        info!("AgentServer starting message processing loop");

        // Subscribe to client messages
        let mut client_rx = self
            .transport
            .subscribe_client_messages(wonopcode_transport::AGENT_SERVER_CONSUMER_GROUP)
            .await?;

        // Send initial state to any connected clients
        let initial_state = self.current_state.read().await.clone();
        self.send_update(ServerPayload::State(Box::new(initial_state)))
            .await?;

        loop {
            // Check shutdown flag
            if *self.shutdown.read().await {
                info!("AgentServer shutdown requested");
                break;
            }

            // Process client messages with timeout
            tokio::select! {
                Some(msg) = client_rx.recv() => {
                    self.handle_client_message(msg, &action_tx).await;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    // Periodic check for shutdown
                }
            }
        }

        Ok(())
    }

    /// Handle a client message.
    async fn handle_client_message(
        &self,
        msg: ClientMessage,
        action_tx: &mpsc::UnboundedSender<ClientPayload>,
    ) {
        debug!(
            "Received client message: {:?} for workstream '{}'",
            msg.id, msg.workstream_id
        );

        // Filter by workstream ID (for Pro edition)
        if msg.workstream_id != self.workstream_id {
            debug!(
                "Ignoring message for different workstream: {} (expected {})",
                msg.workstream_id, self.workstream_id
            );
            return;
        }

        // Handle control messages
        match &msg.payload {
            ClientPayload::Ping => {
                debug!("Received ping, sending pong");
                if let Err(e) = self.send_update(ServerPayload::Pong).await {
                    warn!("Failed to send pong: {}", e);
                }
                return;
            }
            ClientPayload::RequestState => {
                debug!("Received state request, sending current state");
                let state = self.current_state.read().await.clone();
                if let Err(e) = self
                    .send_update(ServerPayload::State(Box::new(state)))
                    .await
                {
                    warn!("Failed to send state: {}", e);
                }
                return;
            }
            ClientPayload::Quit => {
                info!("Received quit request");
                *self.shutdown.write().await = true;
                return;
            }
            _ => {}
        }

        // Forward action to runner
        if action_tx.send(msg.payload).is_err() {
            error!("Failed to forward action to runner - channel closed");
        }
    }

    /// Request shutdown of the server.
    pub async fn shutdown(&self) {
        info!("Requesting AgentServer shutdown");
        *self.shutdown.write().await = true;
    }

    /// Check if the server is running.
    pub async fn is_running(&self) -> bool {
        !*self.shutdown.read().await
    }

    /// Run the server as a bridge between Iggy transport and a local Runner.
    ///
    /// This is an alternative to `run()` that bridges between:
    /// - Iggy client messages -> local AppAction channel -> Runner
    /// - Runner -> local AppUpdate channel -> Iggy server messages
    ///
    /// Use this when the Runner is in the same process and communicates via channels.
    pub async fn run_bridge(
        self,
        action_tx: mpsc::UnboundedSender<wonopcode_tui::AppAction>,
        mut update_rx: mpsc::UnboundedReceiver<wonopcode_tui::AppUpdate>,
    ) -> Result<(), AgentServerError> {
        info!("AgentServer starting bridge mode");

        // Subscribe to client messages
        let mut client_rx = self
            .transport
            .subscribe_client_messages(wonopcode_transport::AGENT_SERVER_CONSUMER_GROUP)
            .await?;

        // Send initial state
        let initial_state = self.current_state.read().await.clone();
        self.send_update(ServerPayload::State(Box::new(initial_state)))
            .await?;

        loop {
            // Check shutdown flag
            if *self.shutdown.read().await {
                info!("AgentServer bridge shutdown requested");
                break;
            }

            tokio::select! {
                // Handle incoming client messages from Iggy
                Some(msg) = client_rx.recv() => {
                    self.handle_client_message_for_bridge(msg, &action_tx).await;
                }
                // Handle outgoing updates from Runner
                Some(update) = update_rx.recv() => {
                    self.handle_runner_update(update).await;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    // Periodic check for shutdown
                }
            }
        }

        Ok(())
    }

    /// Handle a client message in bridge mode - convert to AppAction.
    async fn handle_client_message_for_bridge(
        &self,
        msg: ClientMessage,
        action_tx: &mpsc::UnboundedSender<wonopcode_tui::AppAction>,
    ) {
        debug!(
            "Bridge received client message: {:?} for workstream '{}'",
            msg.id, msg.workstream_id
        );

        // Filter by workstream ID
        if msg.workstream_id != self.workstream_id {
            return;
        }

        // Handle control messages
        match &msg.payload {
            ClientPayload::Ping => {
                if let Err(e) = self.send_update(ServerPayload::Pong).await {
                    warn!("Failed to send pong: {}", e);
                }
                return;
            }
            ClientPayload::RequestState => {
                let state = self.current_state.read().await.clone();
                if let Err(e) = self
                    .send_update(ServerPayload::State(Box::new(state)))
                    .await
                {
                    warn!("Failed to send state: {}", e);
                }
                return;
            }
            ClientPayload::Quit => {
                *self.shutdown.write().await = true;
                if action_tx.send(wonopcode_tui::AppAction::Quit).is_err() {
                    error!("Failed to forward quit action");
                }
                return;
            }
            _ => {}
        }

        // Convert ClientPayload to AppAction
        if let Some(app_action) = client_payload_to_app_action(&msg.payload) {
            if action_tx.send(app_action).is_err() {
                error!("Failed to forward action to runner - channel closed");
            }
        }
    }

    /// Handle a runner update - convert to ServerPayload and send via Iggy.
    ///
    /// Also updates internal state for certain updates so that new clients
    /// receive current values when they connect.
    async fn handle_runner_update(&self, update: wonopcode_tui::AppUpdate) {
        // Update internal state for persistent values
        match &update {
            wonopcode_tui::AppUpdate::TokenUsage {
                input,
                output,
                cost,
                context_limit,
                accumulated_input,
                accumulated_output,
                accumulated_cost,
                last_request_input: _,
                last_request_output: _,
                last_request_cache_read: _,
            } => {
                // If provider gives us accumulated values, use those; otherwise use deltas
                let final_input = accumulated_input.map(|v| v as u32).unwrap_or(*input);
                let final_output = accumulated_output.map(|v| v as u32).unwrap_or(*output);
                let final_cost = accumulated_cost.unwrap_or(*cost);

                self.update_state(|state| {
                    state.token_usage.input = final_input;
                    state.token_usage.output = final_output;
                    state.token_usage.cost = final_cost;
                    state.context_limit = *context_limit;
                })
                .await;
            }
            wonopcode_tui::AppUpdate::ContextStatus {
                estimated_tokens,
                context_limit,
                ..
            } => {
                // ContextStatus provides estimated tokens from history
                // Use these to populate token_usage.input for initial state
                self.update_state(|state| {
                    // Only update if we don't have actual usage yet
                    // (estimated tokens are less accurate than actual usage)
                    if state.token_usage.input == 0 {
                        state.token_usage.input = *estimated_tokens;
                    }
                    state.context_limit = *context_limit;
                })
                .await;
            }
            wonopcode_tui::AppUpdate::ModelInfo { context_limit } => {
                self.update_state(|state| {
                    state.context_limit = *context_limit;
                })
                .await;
            }
            wonopcode_tui::AppUpdate::AgentChanged(agent) => {
                self.update_state(|state| {
                    state.agent = agent.clone();
                })
                .await;
            }
            wonopcode_tui::AppUpdate::TodosUpdated { phases, todos } => {
                self.update_state(|state| {
                    state.phases = phases
                        .iter()
                        .map(|p| wonopcode_message::PhaseInfo {
                            id: p.id.clone(),
                            name: p.name.clone(),
                            status: p.status.clone(),
                            todos: p
                                .todos
                                .iter()
                                .map(|t| wonopcode_message::TodoInfo {
                                    id: t.id.clone(),
                                    content: t.content.clone(),
                                    status: t.status.clone(),
                                    priority: t.priority.clone(),
                                    phase_id: t.phase_id.clone(),
                                    parents: t.parents.clone(),
                                })
                                .collect(),
                        })
                        .collect();
                    state.todos = todos
                        .iter()
                        .map(|t| wonopcode_message::TodoInfo {
                            id: t.id.clone(),
                            content: t.content.clone(),
                            status: t.status.clone(),
                            priority: t.priority.clone(),
                            phase_id: t.phase_id.clone(),
                            parents: t.parents.clone(),
                        })
                        .collect();
                })
                .await;
            }
            wonopcode_tui::AppUpdate::LspUpdated(servers) => {
                self.update_state(|state| {
                    state.lsp_servers = servers
                        .iter()
                        .map(|s| wonopcode_message::LspInfo {
                            id: s.id.clone(),
                            name: s.name.clone(),
                            root: s.root.clone(),
                            connected: s.connected,
                        })
                        .collect();
                })
                .await;
            }
            wonopcode_tui::AppUpdate::McpUpdated(servers) => {
                self.update_state(|state| {
                    state.mcp_servers = servers
                        .iter()
                        .map(|s| wonopcode_message::McpInfo {
                            name: s.name.clone(),
                            connected: s.connected,
                            error: s.error.clone(),
                        })
                        .collect();
                })
                .await;
            }
            wonopcode_tui::AppUpdate::ModifiedFilesUpdated(files) => {
                self.update_state(|state| {
                    state.modified_files = files
                        .iter()
                        .map(|f| wonopcode_message::ModifiedFileInfo {
                            path: f.path.clone(),
                            added: f.added,
                            removed: f.removed,
                        })
                        .collect();
                })
                .await;
            }
            wonopcode_tui::AppUpdate::SandboxUpdated(status) => {
                self.update_state(|state| {
                    state.sandbox = wonopcode_message::SandboxState {
                        state: status.state.clone(),
                        runtime_type: status.runtime_type.clone(),
                        error: status.error.clone(),
                    };
                })
                .await;
            }
            wonopcode_tui::AppUpdate::Sessions(sessions) => {
                self.update_state(|state| {
                    state.sessions = sessions
                        .iter()
                        .map(|(id, title, timestamp)| wonopcode_message::SessionInfo {
                            id: id.clone(),
                            title: title.clone(),
                            timestamp: timestamp.clone(),
                        })
                        .collect();
                })
                .await;
            }
            _ => {
                // Other updates don't need to be persisted in state
            }
        }

        // Convert and send to clients
        let payload = app_update_to_server_payload(&update);
        if let Err(e) = self.send_update(payload).await {
            warn!("Failed to send update via Iggy: {}", e);
        }
    }
}

/// Convert a `ClientPayload` to `wonopcode_tui::AppAction`.
///
/// This is used by the bridge mode to forward client messages to the local Runner.
pub fn client_payload_to_app_action(payload: &ClientPayload) -> Option<wonopcode_tui::AppAction> {
    match payload {
        ClientPayload::SendPrompt { prompt } => {
            Some(wonopcode_tui::AppAction::SendPrompt(prompt.clone()))
        }
        ClientPayload::Cancel => Some(wonopcode_tui::AppAction::Cancel),
        ClientPayload::ChangeModel { model } => {
            Some(wonopcode_tui::AppAction::ChangeModel(model.clone()))
        }
        ClientPayload::ChangeAgent { agent } => {
            Some(wonopcode_tui::AppAction::ChangeAgent(agent.clone()))
        }
        ClientPayload::NewSession => Some(wonopcode_tui::AppAction::NewSession),
        ClientPayload::SwitchSession { session_id } => {
            Some(wonopcode_tui::AppAction::SwitchSession(session_id.clone()))
        }
        ClientPayload::RenameSession { title } => Some(wonopcode_tui::AppAction::RenameSession {
            title: title.clone(),
        }),
        ClientPayload::ForkSession { message_id } => Some(wonopcode_tui::AppAction::ForkSession {
            message_id: message_id.clone(),
        }),
        ClientPayload::ShareSession => Some(wonopcode_tui::AppAction::ShareSession),
        ClientPayload::UnshareSession => Some(wonopcode_tui::AppAction::UnshareSession),
        ClientPayload::Undo => Some(wonopcode_tui::AppAction::Undo),
        ClientPayload::Redo => Some(wonopcode_tui::AppAction::Redo),
        ClientPayload::Revert { message_id } => Some(wonopcode_tui::AppAction::Revert {
            message_id: message_id.clone(),
        }),
        ClientPayload::Unrevert => Some(wonopcode_tui::AppAction::Unrevert),
        ClientPayload::Compact => Some(wonopcode_tui::AppAction::Compact),
        ClientPayload::GotoMessage { message_id } => Some(wonopcode_tui::AppAction::GotoMessage {
            message_id: message_id.clone(),
        }),
        ClientPayload::SandboxStart => Some(wonopcode_tui::AppAction::SandboxStart),
        ClientPayload::SandboxStop => Some(wonopcode_tui::AppAction::SandboxStop),
        ClientPayload::SandboxRestart => Some(wonopcode_tui::AppAction::SandboxRestart),
        ClientPayload::SetAllowAll { enabled } => {
            Some(wonopcode_tui::AppAction::SetAllowAll { enabled: *enabled })
        }
        ClientPayload::McpToggle { name } => {
            Some(wonopcode_tui::AppAction::McpToggle { name: name.clone() })
        }
        ClientPayload::McpReconnect { name } => {
            Some(wonopcode_tui::AppAction::McpReconnect { name: name.clone() })
        }
        ClientPayload::SaveSettings { scope, config } => {
            // Need to deserialize config to wonopcode_core::config::Config
            let app_scope = match scope {
                wonopcode_message::SaveScope::Project => wonopcode_tui::SaveScope::Project,
                wonopcode_message::SaveScope::Global => wonopcode_tui::SaveScope::Global,
            };
            if let Ok(parsed_config) =
                serde_json::from_value::<wonopcode_core::config::Config>(config.clone())
            {
                Some(wonopcode_tui::AppAction::SaveSettings {
                    scope: app_scope,
                    config: Box::new(parsed_config),
                })
            } else {
                warn!("Failed to deserialize config for SaveSettings");
                None
            }
        }
        ClientPayload::PermissionResponse {
            request_id,
            allow,
            remember,
        } => Some(wonopcode_tui::AppAction::PermissionResponse {
            request_id: request_id.clone(),
            allow: *allow,
            remember: *remember,
        }),
        ClientPayload::EnableAllowAll { request_id } => {
            Some(wonopcode_tui::AppAction::EnableAllowAll {
                request_id: request_id.clone(),
            })
        }
        ClientPayload::Quit => Some(wonopcode_tui::AppAction::Quit),
        // Control actions handled separately
        ClientPayload::Ping | ClientPayload::RequestState | ClientPayload::GetAvailableModels => {
            None
        }
        // Workstream actions are Pro-only (not supported in Community Edition AppAction)
        ClientPayload::ListWorkstreams
        | ClientPayload::CreateWorktree { .. }
        | ClientPayload::DeleteWorktree
        | ClientPayload::ActivateWorkstream
        | ClientPayload::DeactivateWorkstream
        | ClientPayload::ConnectWorkstream
        | ClientPayload::DisconnectWorkstream
        | ClientPayload::RefreshWorkstreams => None,
    }
}

/// Convert a `wonopcode_tui::AppUpdate` to `ServerPayload`.
///
/// This is used by the bridge mode to forward Runner updates to Iggy clients.
pub fn app_update_to_server_payload(update: &wonopcode_tui::AppUpdate) -> ServerPayload {
    match update {
        wonopcode_tui::AppUpdate::Started => ServerPayload::Started,
        wonopcode_tui::AppUpdate::TextDelta(delta) => ServerPayload::TextDelta {
            delta: delta.clone(),
        },
        wonopcode_tui::AppUpdate::ToolStarted { name, id, input } => ServerPayload::ToolStarted {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        wonopcode_tui::AppUpdate::ToolCompleted {
            id,
            success,
            output,
            metadata,
        } => ServerPayload::ToolCompleted {
            id: id.clone(),
            success: *success,
            output: output.clone(),
            metadata: metadata.clone(),
        },
        wonopcode_tui::AppUpdate::Completed { text } => {
            ServerPayload::Completed { text: text.clone() }
        }
        wonopcode_tui::AppUpdate::Error(error) => ServerPayload::Error {
            error: error.clone(),
        },
        wonopcode_tui::AppUpdate::Status(message) => ServerPayload::Status {
            message: message.clone(),
        },
        wonopcode_tui::AppUpdate::TokenUsage {
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
        } => ServerPayload::TokenUsage {
            input: *input,
            output: *output,
            cost: *cost,
            context_limit: *context_limit,
            accumulated_input: *accumulated_input,
            accumulated_output: *accumulated_output,
            accumulated_cost: *accumulated_cost,
            last_request_input: *last_request_input,
            last_request_output: *last_request_output,
            last_request_cache_read: *last_request_cache_read,
        },
        wonopcode_tui::AppUpdate::ModelInfo { context_limit } => ServerPayload::ModelInfo {
            context_limit: *context_limit,
        },
        wonopcode_tui::AppUpdate::Sessions(sessions) => ServerPayload::Sessions {
            sessions: sessions
                .iter()
                .map(|(id, title, timestamp)| wonopcode_message::SessionInfo {
                    id: id.clone(),
                    title: title.clone(),
                    timestamp: timestamp.clone(),
                })
                .collect(),
        },
        wonopcode_tui::AppUpdate::TodosUpdated { phases, todos } => ServerPayload::TodosUpdated {
            phases: phases
                .iter()
                .map(|p| wonopcode_message::PhaseInfo {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    status: p.status.clone(),
                    todos: p
                        .todos
                        .iter()
                        .map(|t| wonopcode_message::TodoInfo {
                            id: t.id.clone(),
                            content: t.content.clone(),
                            status: t.status.clone(),
                            priority: t.priority.clone(),
                            phase_id: t.phase_id.clone(),
                            parents: t.parents.clone(),
                        })
                        .collect(),
                })
                .collect(),
            todos: todos
                .iter()
                .map(|t| wonopcode_message::TodoInfo {
                    id: t.id.clone(),
                    content: t.content.clone(),
                    status: t.status.clone(),
                    priority: t.priority.clone(),
                    phase_id: t.phase_id.clone(),
                    parents: t.parents.clone(),
                })
                .collect(),
        },
        wonopcode_tui::AppUpdate::LspUpdated(servers) => ServerPayload::LspUpdated {
            servers: servers
                .iter()
                .map(|s| wonopcode_message::LspInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    root: s.root.clone(),
                    connected: s.connected,
                })
                .collect(),
        },
        wonopcode_tui::AppUpdate::McpUpdated(servers) => ServerPayload::McpUpdated {
            servers: servers
                .iter()
                .map(|s| wonopcode_message::McpInfo {
                    name: s.name.clone(),
                    connected: s.connected,
                    error: s.error.clone(),
                })
                .collect(),
        },
        wonopcode_tui::AppUpdate::ModifiedFilesUpdated(files) => {
            ServerPayload::ModifiedFilesUpdated {
                files: files
                    .iter()
                    .map(|f| wonopcode_message::ModifiedFileInfo {
                        path: f.path.clone(),
                        added: f.added,
                        removed: f.removed,
                    })
                    .collect(),
            }
        }
        wonopcode_tui::AppUpdate::PermissionsPending(count) => {
            ServerPayload::PermissionsPending { count: *count }
        }
        wonopcode_tui::AppUpdate::SandboxUpdated(status) => ServerPayload::SandboxUpdated {
            state: status.state.clone(),
            runtime_type: status.runtime_type.clone(),
            error: status.error.clone(),
            container_id: status.container_id.clone(),
        },
        wonopcode_tui::AppUpdate::SystemMessage(message) => ServerPayload::SystemMessage {
            message: message.clone(),
        },
        wonopcode_tui::AppUpdate::AgentChanged(agent) => ServerPayload::AgentChanged {
            agent: agent.clone(),
        },
        wonopcode_tui::AppUpdate::PermissionRequest(req) => ServerPayload::PermissionRequest {
            id: req.id.clone(),
            tool: req.tool.clone(),
            action: req.action.clone(),
            description: req.description.clone(),
            path: req.path.clone(),
        },
        wonopcode_tui::AppUpdate::PermissionResolved {
            request_id,
            allowed,
        } => ServerPayload::PermissionResolved {
            request_id: request_id.clone(),
            allowed: *allowed,
        },
        // Git updates - not in unified protocol yet, convert to Status
        wonopcode_tui::AppUpdate::GitStatusUpdated(_) => ServerPayload::Status {
            message: "Git status updated".to_string(),
        },
        wonopcode_tui::AppUpdate::GitHistoryUpdated(_) => ServerPayload::Status {
            message: "Git history updated".to_string(),
        },
        wonopcode_tui::AppUpdate::GitOperationResult { success, message } => {
            if *success {
                ServerPayload::Status {
                    message: message.clone(),
                }
            } else {
                ServerPayload::Error {
                    error: message.clone(),
                }
            }
        }
        // Session loaded - convert to Status for now
        wonopcode_tui::AppUpdate::SessionLoaded { id, .. } => ServerPayload::Status {
            message: format!("Session {} loaded", id),
        },
        // Busy state change - convert to Status for now
        // The actual busy state is already reflected through Started/Completed
        wonopcode_tui::AppUpdate::BusyStateChanged(busy) => ServerPayload::Status {
            message: if *busy {
                "Agent is busy".to_string()
            } else {
                "Agent is idle".to_string()
            },
        },
        // Allow-all mode changed - send as status for now
        // The desktop frontend uses its own state tracking
        wonopcode_tui::AppUpdate::AllowAllChanged { enabled } => ServerPayload::Status {
            message: if *enabled {
                "Allow-all mode enabled".to_string()
            } else {
                "Allow-all mode disabled".to_string()
            },
        },
        // TurnPersisted - internal signal that messages have been saved to session.
        // Convert to status for external clients.
        wonopcode_tui::AppUpdate::TurnPersisted => ServerPayload::Status {
            message: "Turn persisted".to_string(),
        },
        // ContextStatus - token usage and compaction status.
        // Convert to status message for now; could be expanded to a proper ServerPayload variant.
        wonopcode_tui::AppUpdate::ContextStatus {
            estimated_tokens,
            context_limit,
            usage_percent,
            needs_compaction,
        } => ServerPayload::Status {
            message: if *needs_compaction {
                format!(
                    "Context: {}% ({}/{} tokens) - compaction may occur",
                    usage_percent, estimated_tokens, context_limit
                )
            } else {
                format!(
                    "Context: {}% ({}/{} tokens)",
                    usage_percent, estimated_tokens, context_limit
                )
            },
        },
        // CompactionStarted - forward to clients
        wonopcode_tui::AppUpdate::CompactionStarted {
            compaction_type,
            messages_before,
        } => ServerPayload::CompactionStarted {
            compaction_type: format!("{:?}", compaction_type).to_lowercase(),
            messages_before: *messages_before,
        },
        // CompactionPerformed - forward to clients
        wonopcode_tui::AppUpdate::CompactionPerformed {
            compaction_type,
            messages_before,
            messages_after,
            tokens_before,
            tokens_after,
            summary,
        } => ServerPayload::CompactionPerformed {
            compaction_type: format!("{:?}", compaction_type).to_lowercase(),
            messages_before: *messages_before,
            messages_after: *messages_after,
            tokens_before: *tokens_before,
            tokens_after: *tokens_after,
            summary: summary.clone(),
        },
        wonopcode_tui::AppUpdate::CompactionNotNeeded => ServerPayload::CompactionNotNeeded,
        wonopcode_tui::AppUpdate::CompactionProgress {
            current_chunk,
            total_chunks,
            phase,
        } => ServerPayload::CompactionProgress {
            current_chunk: *current_chunk,
            total_chunks: *total_chunks,
            phase: phase.clone(),
        },
    }
}

/// Convert a `ClientPayload` to the legacy `wonopcode_protocol::Action`.
///
/// This is a transitional function to bridge between the new unified protocol
/// and the existing runner infrastructure.
pub fn client_payload_to_legacy_action(
    payload: &ClientPayload,
) -> Option<wonopcode_protocol::Action> {
    match payload {
        ClientPayload::SendPrompt { prompt } => Some(wonopcode_protocol::Action::SendPrompt {
            prompt: prompt.clone(),
            images: vec![],
        }),
        ClientPayload::Cancel => Some(wonopcode_protocol::Action::Cancel),
        ClientPayload::ChangeModel { model } => Some(wonopcode_protocol::Action::ChangeModel {
            model: model.clone(),
        }),
        ClientPayload::ChangeAgent { agent } => Some(wonopcode_protocol::Action::ChangeAgent {
            agent: agent.clone(),
        }),
        ClientPayload::NewSession => Some(wonopcode_protocol::Action::NewSession),
        ClientPayload::SwitchSession { session_id } => {
            Some(wonopcode_protocol::Action::SwitchSession {
                session_id: session_id.clone(),
            })
        }
        ClientPayload::RenameSession { title } => Some(wonopcode_protocol::Action::RenameSession {
            title: title.clone(),
        }),
        ClientPayload::ForkSession { message_id } => {
            Some(wonopcode_protocol::Action::ForkSession {
                message_id: message_id.clone(),
            })
        }
        ClientPayload::ShareSession => Some(wonopcode_protocol::Action::ShareSession),
        ClientPayload::UnshareSession => Some(wonopcode_protocol::Action::UnshareSession),
        ClientPayload::Undo => Some(wonopcode_protocol::Action::Undo),
        ClientPayload::Redo => Some(wonopcode_protocol::Action::Redo),
        ClientPayload::Revert { message_id } => Some(wonopcode_protocol::Action::Revert {
            message_id: message_id.clone(),
        }),
        ClientPayload::Unrevert => Some(wonopcode_protocol::Action::Unrevert),
        ClientPayload::Compact => Some(wonopcode_protocol::Action::Compact),
        ClientPayload::GotoMessage { message_id } => {
            Some(wonopcode_protocol::Action::GotoMessage {
                message_id: message_id.clone(),
            })
        }
        ClientPayload::SandboxStart => Some(wonopcode_protocol::Action::SandboxStart),
        ClientPayload::SandboxStop => Some(wonopcode_protocol::Action::SandboxStop),
        ClientPayload::SandboxRestart => Some(wonopcode_protocol::Action::SandboxRestart),
        ClientPayload::SetAllowAll { enabled } => {
            Some(wonopcode_protocol::Action::SetAllowAll { enabled: *enabled })
        }
        ClientPayload::McpToggle { name } => {
            Some(wonopcode_protocol::Action::McpToggle { name: name.clone() })
        }
        ClientPayload::McpReconnect { name } => {
            Some(wonopcode_protocol::Action::McpReconnect { name: name.clone() })
        }
        ClientPayload::SaveSettings { scope, config } => {
            let legacy_scope = match scope {
                wonopcode_message::SaveScope::Project => wonopcode_protocol::SaveScope::Project,
                wonopcode_message::SaveScope::Global => wonopcode_protocol::SaveScope::Global,
            };
            Some(wonopcode_protocol::Action::SaveSettings {
                scope: legacy_scope,
                config: config.clone(),
            })
        }
        ClientPayload::PermissionResponse {
            request_id,
            allow,
            remember,
        } => Some(wonopcode_protocol::Action::PermissionResponse {
            request_id: request_id.clone(),
            allow: *allow,
            remember: *remember,
        }),
        ClientPayload::EnableAllowAll { request_id } => {
            Some(wonopcode_protocol::Action::EnableAllowAll {
                request_id: request_id.clone(),
            })
        }
        // Control actions are handled separately
        ClientPayload::Ping
        | ClientPayload::RequestState
        | ClientPayload::Quit
        | ClientPayload::GetAvailableModels => None,
        // Workstream actions are Pro-only
        ClientPayload::ListWorkstreams
        | ClientPayload::CreateWorktree { .. }
        | ClientPayload::DeleteWorktree
        | ClientPayload::ActivateWorkstream
        | ClientPayload::DeactivateWorkstream
        | ClientPayload::ConnectWorkstream
        | ClientPayload::DisconnectWorkstream
        | ClientPayload::RefreshWorkstreams => None,
    }
}

/// Convert a legacy `wonopcode_protocol::Update` to `ServerPayload`.
///
/// This is a transitional function to bridge between the existing runner
/// infrastructure and the new unified protocol.
pub fn legacy_update_to_server_payload(update: &wonopcode_protocol::Update) -> ServerPayload {
    match update {
        wonopcode_protocol::Update::Started => ServerPayload::Started,
        wonopcode_protocol::Update::TextDelta { delta } => ServerPayload::TextDelta {
            delta: delta.clone(),
        },
        wonopcode_protocol::Update::ToolStarted { id, name, input } => ServerPayload::ToolStarted {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        wonopcode_protocol::Update::ToolCompleted {
            id,
            success,
            output,
            metadata,
        } => ServerPayload::ToolCompleted {
            id: id.clone(),
            success: *success,
            output: output.clone(),
            metadata: metadata.clone(),
        },
        wonopcode_protocol::Update::Completed { text } => {
            ServerPayload::Completed { text: text.clone() }
        }
        wonopcode_protocol::Update::Error { error } => ServerPayload::Error {
            error: error.clone(),
        },
        wonopcode_protocol::Update::Status { message } => ServerPayload::Status {
            message: message.clone(),
        },
        wonopcode_protocol::Update::TokenUsage {
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
        } => ServerPayload::TokenUsage {
            input: *input,
            output: *output,
            cost: *cost,
            context_limit: *context_limit,
            accumulated_input: *accumulated_input,
            accumulated_output: *accumulated_output,
            accumulated_cost: *accumulated_cost,
            last_request_input: *last_request_input,
            last_request_output: *last_request_output,
            last_request_cache_read: *last_request_cache_read,
        },
        wonopcode_protocol::Update::ModelInfo { context_limit } => ServerPayload::ModelInfo {
            context_limit: *context_limit,
        },
        wonopcode_protocol::Update::Sessions { sessions } => ServerPayload::Sessions {
            sessions: sessions
                .iter()
                .map(|s| wonopcode_message::SessionInfo {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    timestamp: s.timestamp.clone(),
                })
                .collect(),
        },
        wonopcode_protocol::Update::TodosUpdated { phases, todos } => ServerPayload::TodosUpdated {
            phases: phases
                .iter()
                .map(|p| wonopcode_message::PhaseInfo {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    status: p.status.clone(),
                    todos: p
                        .todos
                        .iter()
                        .map(|t| wonopcode_message::TodoInfo {
                            id: t.id.clone(),
                            content: t.content.clone(),
                            status: t.status.clone(),
                            priority: t.priority.clone(),
                            phase_id: None,
                            parents: t.parents.clone(),
                        })
                        .collect(),
                })
                .collect(),
            todos: todos
                .iter()
                .map(|t| wonopcode_message::TodoInfo {
                    id: t.id.clone(),
                    content: t.content.clone(),
                    status: t.status.clone(),
                    priority: t.priority.clone(),
                    phase_id: None,
                    parents: t.parents.clone(),
                })
                .collect(),
        },
        wonopcode_protocol::Update::LspUpdated { servers } => ServerPayload::LspUpdated {
            servers: servers
                .iter()
                .map(|s| wonopcode_message::LspInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    root: s.root.clone(),
                    connected: s.connected,
                })
                .collect(),
        },
        wonopcode_protocol::Update::McpUpdated { servers } => ServerPayload::McpUpdated {
            servers: servers
                .iter()
                .map(|s| wonopcode_message::McpInfo {
                    name: s.name.clone(),
                    connected: s.connected,
                    error: s.error.clone(),
                })
                .collect(),
        },
        wonopcode_protocol::Update::ModifiedFilesUpdated { files } => {
            ServerPayload::ModifiedFilesUpdated {
                files: files
                    .iter()
                    .map(|f| wonopcode_message::ModifiedFileInfo {
                        path: f.path.clone(),
                        added: f.added,
                        removed: f.removed,
                    })
                    .collect(),
            }
        }
        wonopcode_protocol::Update::PermissionsPending { count } => {
            ServerPayload::PermissionsPending { count: *count }
        }
        wonopcode_protocol::Update::SandboxUpdated {
            state,
            runtime_type,
            error,
            container_id,
        } => ServerPayload::SandboxUpdated {
            state: state.clone(),
            runtime_type: runtime_type.clone(),
            error: error.clone(),
            container_id: container_id.clone(),
        },
        wonopcode_protocol::Update::SystemMessage { message } => ServerPayload::SystemMessage {
            message: message.clone(),
        },
        wonopcode_protocol::Update::AgentChanged { agent } => ServerPayload::AgentChanged {
            agent: agent.clone(),
        },
        wonopcode_protocol::Update::PermissionRequest {
            id,
            tool,
            action,
            description,
            path,
        } => ServerPayload::PermissionRequest {
            id: id.clone(),
            tool: tool.clone(),
            action: action.clone(),
            description: description.clone(),
            path: path.clone(),
        },
        wonopcode_protocol::Update::PermissionResolved {
            request_id,
            allowed,
        } => ServerPayload::PermissionResolved {
            request_id: request_id.clone(),
            allowed: *allowed,
        },
        wonopcode_protocol::Update::CompactionStarted {
            compaction_type,
            messages_before,
        } => ServerPayload::CompactionStarted {
            compaction_type: compaction_type.clone(),
            messages_before: *messages_before,
        },
        wonopcode_protocol::Update::CompactionPerformed {
            compaction_type,
            messages_before,
            messages_after,
            tokens_before,
            tokens_after,
            summary,
        } => ServerPayload::CompactionPerformed {
            compaction_type: compaction_type.clone(),
            messages_before: *messages_before,
            messages_after: *messages_after,
            tokens_before: *tokens_before,
            tokens_after: *tokens_after,
            summary: summary.clone(),
        },
        wonopcode_protocol::Update::CompactionNotNeeded => ServerPayload::CompactionNotNeeded,
        wonopcode_protocol::Update::CompactionProgress {
            current_chunk,
            total_chunks,
            phase,
        } => ServerPayload::CompactionProgress {
            current_chunk: *current_chunk,
            total_chunks: *total_chunks,
            phase: phase.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_payload_to_legacy_send_prompt() {
        let payload = ClientPayload::SendPrompt {
            prompt: "Hello".to_string(),
        };
        let action = client_payload_to_legacy_action(&payload);
        assert!(matches!(
            action,
            Some(wonopcode_protocol::Action::SendPrompt { .. })
        ));
    }

    #[test]
    fn test_client_payload_to_legacy_cancel() {
        let payload = ClientPayload::Cancel;
        let action = client_payload_to_legacy_action(&payload);
        assert!(matches!(action, Some(wonopcode_protocol::Action::Cancel)));
    }

    #[test]
    fn test_client_payload_to_legacy_ping_returns_none() {
        let payload = ClientPayload::Ping;
        let action = client_payload_to_legacy_action(&payload);
        assert!(action.is_none());
    }

    #[test]
    fn test_client_payload_to_legacy_workstream_actions_return_none() {
        let payload = ClientPayload::ListWorkstreams;
        let action = client_payload_to_legacy_action(&payload);
        assert!(action.is_none());
    }

    #[test]
    fn test_legacy_update_to_server_payload_started() {
        let update = wonopcode_protocol::Update::Started;
        let payload = legacy_update_to_server_payload(&update);
        assert!(matches!(payload, ServerPayload::Started));
    }

    #[test]
    fn test_legacy_update_to_server_payload_text_delta() {
        let update = wonopcode_protocol::Update::TextDelta {
            delta: "Hello".to_string(),
        };
        let payload = legacy_update_to_server_payload(&update);
        assert!(matches!(payload, ServerPayload::TextDelta { delta } if delta == "Hello"));
    }

    #[test]
    fn test_legacy_update_to_server_payload_error() {
        let update = wonopcode_protocol::Update::Error {
            error: "Test error".to_string(),
        };
        let payload = legacy_update_to_server_payload(&update);
        assert!(matches!(payload, ServerPayload::Error { error } if error == "Test error"));
    }
}
