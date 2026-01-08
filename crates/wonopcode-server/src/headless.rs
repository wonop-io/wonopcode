//! Headless server mode for remote TUI connections.
//!
//! This module provides an HTTP server that exposes the full agent functionality
//! via HTTP endpoints and SSE streaming, allowing remote TUI clients to connect.
//!
//! # MCP Support
//!
//! The headless server can optionally expose MCP (Model Context Protocol) endpoints
//! at `/mcp/sse` and `/mcp/message`, allowing Claude CLI to connect via HTTP.

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, RwLock};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{debug, info, Span};
use wonopcode_mcp::{create_mcp_router, McpHttpState};
use wonopcode_protocol::{Action, State as ProtocolState, Update};

/// State for the headless server.
#[derive(Clone)]
pub struct HeadlessState {
    /// Sender for actions to the runner.
    pub action_tx: mpsc::UnboundedSender<Action>,
    /// Broadcast sender for updates from the runner.
    pub update_tx: broadcast::Sender<Update>,
    /// Current state for initial sync.
    pub current_state: Arc<RwLock<ProtocolState>>,
    /// Flag to track if server should shutdown.
    pub shutdown: Arc<RwLock<bool>>,
}

impl HeadlessState {
    /// Create a new headless state.
    pub fn new(action_tx: mpsc::UnboundedSender<Action>) -> Self {
        let (update_tx, _) = broadcast::channel(256);
        Self {
            action_tx,
            update_tx,
            current_state: Arc::new(RwLock::new(ProtocolState::default())),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Send an update to all connected clients.
    pub fn send_update(&self, update: Update) {
        let _ = self.update_tx.send(update);
    }

    /// Update the current state.
    pub async fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut ProtocolState),
    {
        let mut state = self.current_state.write().await;
        f(&mut state);
    }
}

/// Create the headless server router.
pub fn create_headless_router(state: HeadlessState) -> Router {
    create_headless_router_with_mcp(state, None)
}

/// Create the headless server router with optional MCP support.
///
/// If `mcp_state` is provided, MCP endpoints will be available at `/mcp/sse` and `/mcp/message`.
pub fn create_headless_router_with_mcp(
    state: HeadlessState,
    mcp_state: Option<McpHttpState>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut router = Router::new()
        // Health check
        .route("/health", get(health))
        // State endpoint for initial sync
        .route("/state", get(get_state))
        // SSE events stream
        .route("/events", get(events))
        // Action endpoints
        .route("/action/prompt", post(action_prompt))
        .route("/action/cancel", post(action_cancel))
        .route("/action/model", post(action_model))
        .route("/action/agent", post(action_agent))
        .route("/action/session/new", post(action_session_new))
        .route("/action/session/switch", post(action_session_switch))
        .route("/action/session/rename", post(action_session_rename))
        .route("/action/session/fork", post(action_session_fork))
        .route("/action/session/share", post(action_session_share))
        .route("/action/session/unshare", post(action_session_unshare))
        .route("/action/undo", post(action_undo))
        .route("/action/redo", post(action_redo))
        .route("/action/revert", post(action_revert))
        .route("/action/unrevert", post(action_unrevert))
        .route("/action/compact", post(action_compact))
        .route("/action/sandbox/start", post(action_sandbox_start))
        .route("/action/sandbox/stop", post(action_sandbox_stop))
        .route("/action/sandbox/restart", post(action_sandbox_restart))
        .route("/action/mcp/toggle", post(action_mcp_toggle))
        .route("/action/mcp/reconnect", post(action_mcp_reconnect))
        .route("/action/goto", post(action_goto))
        .route("/action/settings", post(action_settings))
        .route("/action/permission", post(action_permission))
        .route("/action/quit", post(action_quit))
        .with_state(state);

    // Add MCP routes if state is provided
    if let Some(mcp) = mcp_state {
        let mcp_router = create_mcp_router(mcp);
        router = router.nest("/mcp", mcp_router);
        info!("MCP HTTP endpoints enabled at /mcp/sse and /mcp/message");
    }

    router.layer(cors).layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<_>| {
                tracing::info_span!(
                    "request",
                    method = %request.method(),
                    uri = %request.uri(),
                )
            })
            .on_request(|request: &axum::http::Request<_>, _span: &Span| {
                info!(
                    method = %request.method(),
                    path = %request.uri().path(),
                    "request"
                );
            })
            .on_response(
                |response: &axum::http::Response<_>, latency: Duration, _span: &Span| {
                    info!(
                        status = %response.status(),
                        latency = ?latency,
                        "response"
                    );
                },
            ),
    )
}

// ============================================================================
// Health & State Endpoints
// ============================================================================

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn get_state(State(state): State<HeadlessState>) -> impl IntoResponse {
    let current = state.current_state.read().await;
    Json(current.clone())
}

// ============================================================================
// SSE Events Stream
// ============================================================================

async fn events(
    State(state): State<HeadlessState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.update_tx.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(update) => {
                    let event_type = update.event_type();
                    if let Ok(data) = serde_json::to_string(&update) {
                        yield Ok(Event::default().event(event_type).data(data));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE stream lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

// ============================================================================
// Action Endpoints
// ============================================================================

#[derive(Deserialize)]
struct PromptRequest {
    prompt: String,
}

async fn action_prompt(
    State(state): State<HeadlessState>,
    Json(req): Json<PromptRequest>,
) -> impl IntoResponse {
    debug!(prompt = %req.prompt, "Received prompt action");
    match state
        .action_tx
        .send(Action::SendPrompt { prompt: req.prompt })
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_cancel(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received cancel action");
    match state.action_tx.send(Action::Cancel) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct ModelRequest {
    model: String,
}

async fn action_model(
    State(state): State<HeadlessState>,
    Json(req): Json<ModelRequest>,
) -> impl IntoResponse {
    debug!(model = %req.model, "Received model change action");
    match state
        .action_tx
        .send(Action::ChangeModel { model: req.model })
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct AgentRequest {
    agent: String,
}

async fn action_agent(
    State(state): State<HeadlessState>,
    Json(req): Json<AgentRequest>,
) -> impl IntoResponse {
    debug!(agent = %req.agent, "Received agent change action");
    match state
        .action_tx
        .send(Action::ChangeAgent { agent: req.agent })
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_session_new(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received new session action");
    match state.action_tx.send(Action::NewSession) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct SessionSwitchRequest {
    session_id: String,
}

async fn action_session_switch(
    State(state): State<HeadlessState>,
    Json(req): Json<SessionSwitchRequest>,
) -> impl IntoResponse {
    debug!(session_id = %req.session_id, "Received session switch action");
    match state.action_tx.send(Action::SwitchSession {
        session_id: req.session_id,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct SessionRenameRequest {
    title: String,
}

async fn action_session_rename(
    State(state): State<HeadlessState>,
    Json(req): Json<SessionRenameRequest>,
) -> impl IntoResponse {
    debug!(title = %req.title, "Received session rename action");
    match state
        .action_tx
        .send(Action::RenameSession { title: req.title })
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct SessionForkRequest {
    message_id: Option<String>,
}

async fn action_session_fork(
    State(state): State<HeadlessState>,
    Json(req): Json<SessionForkRequest>,
) -> impl IntoResponse {
    debug!(message_id = ?req.message_id, "Received session fork action");
    match state.action_tx.send(Action::ForkSession {
        message_id: req.message_id,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_session_share(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received session share action");
    match state.action_tx.send(Action::ShareSession) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_session_unshare(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received session unshare action");
    match state.action_tx.send(Action::UnshareSession) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_undo(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received undo action");
    match state.action_tx.send(Action::Undo) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_redo(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received redo action");
    match state.action_tx.send(Action::Redo) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct RevertRequest {
    message_id: String,
}

async fn action_revert(
    State(state): State<HeadlessState>,
    Json(req): Json<RevertRequest>,
) -> impl IntoResponse {
    debug!(message_id = %req.message_id, "Received revert action");
    match state.action_tx.send(Action::Revert {
        message_id: req.message_id,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_unrevert(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received unrevert action");
    match state.action_tx.send(Action::Unrevert) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_compact(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received compact action");
    match state.action_tx.send(Action::Compact) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_sandbox_start(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received sandbox start action");
    match state.action_tx.send(Action::SandboxStart) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_sandbox_stop(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received sandbox stop action");
    match state.action_tx.send(Action::SandboxStop) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_sandbox_restart(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received sandbox restart action");
    match state.action_tx.send(Action::SandboxRestart) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct McpToggleRequest {
    name: String,
}

async fn action_mcp_toggle(
    State(state): State<HeadlessState>,
    Json(req): Json<McpToggleRequest>,
) -> impl IntoResponse {
    debug!(name = %req.name, "Received MCP toggle action");
    match state.action_tx.send(Action::McpToggle { name: req.name }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct McpReconnectRequest {
    name: String,
}

async fn action_mcp_reconnect(
    State(state): State<HeadlessState>,
    Json(req): Json<McpReconnectRequest>,
) -> impl IntoResponse {
    debug!(name = %req.name, "Received MCP reconnect action");
    match state
        .action_tx
        .send(Action::McpReconnect { name: req.name })
    {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct GotoRequest {
    message_id: String,
}

async fn action_goto(
    State(state): State<HeadlessState>,
    Json(req): Json<GotoRequest>,
) -> impl IntoResponse {
    debug!(message_id = %req.message_id, "Received goto action");
    match state.action_tx.send(Action::GotoMessage {
        message_id: req.message_id,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct SettingsRequest {
    scope: wonopcode_protocol::SaveScope,
    config: serde_json::Value,
}

async fn action_settings(
    State(state): State<HeadlessState>,
    Json(req): Json<SettingsRequest>,
) -> impl IntoResponse {
    debug!(scope = ?req.scope, "Received settings action");
    match state.action_tx.send(Action::SaveSettings {
        scope: req.scope,
        config: req.config,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct PermissionRequest {
    request_id: String,
    allow: bool,
    remember: bool,
}

async fn action_permission(
    State(state): State<HeadlessState>,
    Json(req): Json<PermissionRequest>,
) -> impl IntoResponse {
    debug!(request_id = %req.request_id, allow = req.allow, "Received permission response");
    match state.action_tx.send(Action::PermissionResponse {
        request_id: req.request_id,
        allow: req.allow,
        remember: req.remember,
    }) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn action_quit(State(state): State<HeadlessState>) -> impl IntoResponse {
    debug!("Received quit action");
    *state.shutdown.write().await = true;
    match state.action_tx.send(Action::Quit) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
