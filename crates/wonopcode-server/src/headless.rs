//! Headless server mode for remote TUI connections.
//!
//! This module provides an HTTP server that exposes the full agent functionality
//! via HTTP endpoints and SSE streaming, allowing remote TUI clients to connect.
//!
//! # MCP Support
//!
//! The headless server can optionally expose MCP (Model Context Protocol) endpoints
//! at `/mcp/sse` and `/mcp/message`, allowing Claude CLI to connect via HTTP.
//!
//! # Authentication
//!
//! The server can be protected with an API key. When configured, clients must provide
//! the key via `X-API-Key` header or `Authorization: Bearer <key>` header.

use axum::{
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use futures::stream::Stream;
use serde::Deserialize;

use crate::git::GitOperations;
use std::{convert::Infallible, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::sync::{broadcast, mpsc, RwLock};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{debug, info, warn, Span};
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
    /// Sender to trigger server shutdown.
    pub shutdown_tx: Option<mpsc::Sender<()>>,
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
            shutdown_tx: None,
        }
    }

    /// Set the shutdown sender for graceful server shutdown.
    pub fn with_shutdown_tx(mut self, tx: mpsc::Sender<()>) -> Self {
        self.shutdown_tx = Some(tx);
        self
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

// ============================================================================
// Authentication
// ============================================================================

/// State for API key authentication middleware.
#[derive(Clone)]
struct AuthState {
    api_key: Option<String>,
}

/// Extract API key from request headers.
///
/// Supports both `X-API-Key` header and `Authorization: Bearer <key>` format.
fn extract_api_key(headers: &HeaderMap) -> Option<&str> {
    // Check X-API-Key header first (case-insensitive in HTTP)
    if let Some(key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(key);
    }

    // Check Authorization header for Bearer token
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(key) = auth.strip_prefix("Bearer ") {
            return Some(key.trim());
        }
    }

    None
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// Middleware to validate API key.
async fn api_key_auth(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // If no API key is configured, allow all requests
    let Some(ref expected_key) = auth.api_key else {
        return Ok(next.run(request).await);
    };

    // Extract and validate API key
    let provided_key = extract_api_key(request.headers());

    match provided_key {
        Some(key) if constant_time_eq(key.as_bytes(), expected_key.as_bytes()) => {
            Ok(next.run(request).await)
        }
        Some(_) => {
            warn!("Invalid API key provided");
            Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Invalid API key" })),
            ))
        }
        None => {
            warn!("Missing API key");
            Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Authentication required" })),
            ))
        }
    }
}

// ============================================================================
// Router Creation
// ============================================================================

/// Create the headless server router.
pub fn create_headless_router(state: HeadlessState) -> Router {
    create_headless_router_with_options(state, None, None)
}

/// Create the headless server router with optional MCP support.
///
/// If `mcp_state` is provided, MCP endpoints will be available at `/mcp/sse` and `/mcp/message`.
pub fn create_headless_router_with_mcp(
    state: HeadlessState,
    mcp_state: Option<McpHttpState>,
) -> Router {
    create_headless_router_with_options(state, mcp_state, None)
}

/// Create the headless server router with optional MCP support and API key authentication.
///
/// If `mcp_state` is provided, MCP endpoints will be available at `/mcp/sse` and `/mcp/message`.
/// If `api_key` is provided, all endpoints (except /health) will require authentication.
pub fn create_headless_router_with_options(
    state: HeadlessState,
    mcp_state: Option<McpHttpState>,
    api_key: Option<String>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let has_auth = api_key.is_some();
    let auth_state = AuthState {
        api_key: api_key.clone(),
    };

    // Protected routes that require authentication
    let mut protected_router = Router::new()
        // Info endpoint for quick agent identification
        .route("/info", get(get_info))
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
        .route("/action/shutdown", post(action_shutdown))
        // Git operations
        .route("/git/status", get(git_status))
        .route("/git/stage", post(git_stage))
        .route("/git/unstage", post(git_unstage))
        .route("/git/checkout", post(git_checkout))
        .route("/git/commit", post(git_commit))
        .route("/git/history", get(git_history))
        .route("/git/push", post(git_push))
        .route("/git/pull", post(git_pull))
        .with_state(state);

    // Add MCP routes if state is provided (with API key auth applied via MCP's own middleware)
    if let Some(mut mcp) = mcp_state {
        // Apply the same API key to MCP state if configured
        if let Some(ref key) = api_key {
            mcp = mcp.with_api_key(key.clone());
        }
        let mcp_router = create_mcp_router(mcp);
        protected_router = protected_router.nest("/mcp", mcp_router);
        info!("MCP HTTP endpoints enabled at /mcp/sse and /mcp/message");
    }

    // Apply auth middleware to protected routes if API key is configured
    let protected_router = if has_auth {
        info!("API key authentication enabled for all endpoints");
        protected_router.layer(axum::middleware::from_fn_with_state(
            auth_state,
            api_key_auth,
        ))
    } else {
        protected_router
    };

    // Combine with public routes (health check remains accessible for monitoring)
    let router = Router::new()
        .route("/health", get(health))
        .merge(protected_router);

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

/// Get basic agent info (name, working directory, project_id, work_id).
/// This is a lightweight endpoint for quick identification.
async fn get_info(State(state): State<HeadlessState>) -> impl IntoResponse {
    let current = state.current_state.read().await;
    Json(serde_json::json!({
        "name": current.agent,
        "project": current.project,
        "model": current.model,
        "project_id": current.project_id,
        "work_id": current.work_id,
    }))
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

/// Shutdown endpoint - triggers graceful server shutdown.
/// This is used by WARP to stop agents it has spawned.
async fn action_shutdown(State(state): State<HeadlessState>) -> impl IntoResponse {
    info!("Received shutdown request");
    *state.shutdown.write().await = true;

    // Try to send quit action to runner
    let _ = state.action_tx.send(Action::Quit);

    // Trigger server shutdown if channel is configured
    if let Some(ref tx) = state.shutdown_tx {
        if tx.send(()).await.is_ok() {
            info!("Server shutdown initiated");
            return Json(serde_json::json!({ "status": "shutting_down" }));
        }
    }

    // Even without shutdown channel, mark as shutting down
    Json(serde_json::json!({ "status": "shutdown_requested" }))
}

// ============================================================================
// Git Operations Endpoints
// ============================================================================

/// Get git repository status.
async fn git_status(
    State(state): State<HeadlessState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.status() {
        Ok(status) => Ok(Json(status)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize)]
struct GitStageRequest {
    /// Paths to stage. If empty, stages all modified files.
    #[serde(default)]
    paths: Vec<String>,
}

/// Stage files in the git index.
async fn git_stage(
    State(state): State<HeadlessState>,
    Json(req): Json<GitStageRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.stage(&req.paths) {
        Ok(()) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize)]
struct GitUnstageRequest {
    /// Paths to unstage. If empty, unstages all staged files.
    #[serde(default)]
    paths: Vec<String>,
}

/// Unstage files from the git index.
async fn git_unstage(
    State(state): State<HeadlessState>,
    Json(req): Json<GitUnstageRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.unstage(&req.paths) {
        Ok(()) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize)]
struct GitCheckoutRequest {
    /// Paths to checkout (discard changes). Required - must specify files.
    paths: Vec<String>,
}

/// Checkout (discard changes to) files.
async fn git_checkout(
    State(state): State<HeadlessState>,
    Json(req): Json<GitCheckoutRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    if req.paths.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Must specify files to checkout" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.checkout(&req.paths) {
        Ok(()) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize)]
struct GitCommitRequest {
    /// Commit message.
    message: String,
}

/// Create a git commit.
async fn git_commit(
    State(state): State<HeadlessState>,
    Json(req): Json<GitCommitRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    if req.message.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Commit message cannot be empty" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.commit(&req.message) {
        Ok(commit) => Ok(Json(serde_json::json!({
            "success": true,
            "commit": commit,
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize, Default)]
struct GitHistoryQuery {
    /// Maximum number of commits to return (default: 50).
    #[serde(default = "default_history_limit")]
    limit: usize,
}

fn default_history_limit() -> usize {
    50
}

/// Get git commit history.
async fn git_history(
    State(state): State<HeadlessState>,
    Query(query): Query<GitHistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.history(query.limit) {
        Ok(history) => Ok(Json(history)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize, Default)]
struct GitPushRequest {
    /// Remote name (default: "origin").
    remote: Option<String>,
    /// Branch name (default: current branch).
    branch: Option<String>,
}

/// Push to remote.
async fn git_push(
    State(state): State<HeadlessState>,
    Json(req): Json<GitPushRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.push(req.remote.as_deref(), req.branch.as_deref()) {
        Ok(()) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize, Default)]
struct GitPullRequest {
    /// Remote name (default: "origin").
    remote: Option<String>,
    /// Branch name (default: current branch).
    branch: Option<String>,
}

/// Pull from remote.
async fn git_pull(
    State(state): State<HeadlessState>,
    Json(req): Json<GitPullRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let current = state.current_state.read().await;
    let working_dir = &current.project;

    if working_dir.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No working directory set" })),
        ));
    }

    let ops = GitOperations::new(working_dir);
    match ops.pull(req.remote.as_deref(), req.branch.as_deref()) {
        Ok(()) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    // === HeadlessState tests ===

    #[tokio::test]
    async fn test_headless_state_new() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = HeadlessState::new(tx);

        assert!(state.shutdown_tx.is_none());
        let shutdown = state.shutdown.read().await;
        assert!(!*shutdown);
    }

    #[tokio::test]
    async fn test_headless_state_with_shutdown_tx() {
        let (action_tx, _action_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, _shutdown_rx) = mpsc::channel(1);

        let state = HeadlessState::new(action_tx).with_shutdown_tx(shutdown_tx);
        assert!(state.shutdown_tx.is_some());
    }

    #[tokio::test]
    async fn test_headless_state_send_update() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = HeadlessState::new(tx);

        // Subscribe before sending
        let mut rx = state.update_tx.subscribe();

        let update = Update::Started;
        state.send_update(update);

        // Should receive the update
        let received = rx.try_recv();
        assert!(received.is_ok());
    }

    #[tokio::test]
    async fn test_headless_state_update_state() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = HeadlessState::new(tx);

        state
            .update_state(|s| {
                s.project = "/test/path".to_string();
            })
            .await;

        let current = state.current_state.read().await;
        assert_eq!(current.project, "/test/path");
    }

    #[tokio::test]
    async fn test_headless_state_clone() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = HeadlessState::new(tx);

        state
            .update_state(|s| {
                s.project = "/original".to_string();
            })
            .await;

        let cloned = state.clone();

        // Both should share the same state
        let original_state = state.current_state.read().await;
        let cloned_state = cloned.current_state.read().await;
        assert_eq!(original_state.project, cloned_state.project);
    }

    // === extract_api_key tests ===

    #[test]
    fn test_extract_api_key_from_x_api_key_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("test-key-123"));

        let key = extract_api_key(&headers);
        assert_eq!(key, Some("test-key-123"));
    }

    #[test]
    fn test_extract_api_key_from_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer my-bearer-token"),
        );

        let key = extract_api_key(&headers);
        assert_eq!(key, Some("my-bearer-token"));
    }

    #[test]
    fn test_extract_api_key_bearer_with_whitespace() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer   spaced-token  "),
        );

        let key = extract_api_key(&headers);
        assert_eq!(key, Some("spaced-token"));
    }

    #[test]
    fn test_extract_api_key_prefers_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("x-api-key-value"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer bearer-value"),
        );

        let key = extract_api_key(&headers);
        assert_eq!(key, Some("x-api-key-value"));
    }

    #[test]
    fn test_extract_api_key_missing() {
        let headers = HeaderMap::new();
        let key = extract_api_key(&headers);
        assert!(key.is_none());
    }

    #[test]
    fn test_extract_api_key_non_bearer_auth() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Basic dXNlcjpwYXNz"));

        let key = extract_api_key(&headers);
        assert!(key.is_none());
    }

    // === constant_time_eq tests ===

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq(b"test", b"test"));
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"longer string here", b"longer string here"));
    }

    #[test]
    fn test_constant_time_eq_not_equal() {
        assert!(!constant_time_eq(b"test", b"tset"));
        assert!(!constant_time_eq(b"test", b"test1"));
        assert!(!constant_time_eq(b"a", b"b"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    // === Request types tests ===

    #[test]
    fn test_git_stage_request_deserialize() {
        let json = r#"{"paths": ["file1.txt", "file2.txt"]}"#;
        let req: GitStageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.paths, vec!["file1.txt", "file2.txt"]);
    }

    #[test]
    fn test_git_stage_request_deserialize_empty() {
        let json = r#"{"paths": []}"#;
        let req: GitStageRequest = serde_json::from_str(json).unwrap();
        assert!(req.paths.is_empty());
    }

    #[test]
    fn test_git_commit_request_deserialize() {
        let json = r#"{"message": "Initial commit"}"#;
        let req: GitCommitRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message, "Initial commit");
    }

    #[test]
    fn test_git_checkout_request_deserialize_with_paths() {
        let json = r#"{"paths": ["src/main.rs"]}"#;
        let req: GitCheckoutRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.paths, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn test_git_checkout_request_deserialize_empty() {
        let json = r#"{"paths": []}"#;
        let req: GitCheckoutRequest = serde_json::from_str(json).unwrap();
        assert!(req.paths.is_empty());
    }

    #[test]
    fn test_git_push_request_default() {
        let req = GitPushRequest::default();
        assert!(req.remote.is_none());
        assert!(req.branch.is_none());
    }

    #[test]
    fn test_git_push_request_deserialize_full() {
        let json = r#"{"remote": "upstream", "branch": "feature"}"#;
        let req: GitPushRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.remote, Some("upstream".to_string()));
        assert_eq!(req.branch, Some("feature".to_string()));
    }

    #[test]
    fn test_git_pull_request_default() {
        let req = GitPullRequest::default();
        assert!(req.remote.is_none());
        assert!(req.branch.is_none());
    }

    #[test]
    fn test_git_pull_request_deserialize_partial() {
        let json = r#"{"remote": "origin"}"#;
        let req: GitPullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.remote, Some("origin".to_string()));
        assert!(req.branch.is_none());
    }

    #[test]
    fn test_history_query_default() {
        let query = GitHistoryQuery::default();
        // Using #[derive(Default)] uses usize::default() which is 0
        // The serde default of 50 only applies during deserialization
        assert_eq!(query.limit, 0);
    }

    #[test]
    fn test_history_query_deserialize() {
        let json = r#"{"limit": 100}"#;
        let query: GitHistoryQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.limit, 100);
    }

    #[test]
    fn test_history_query_deserialize_uses_default() {
        let json = r#"{}"#;
        let query: GitHistoryQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.limit, 50); // default_history_limit
    }

    // === AuthState tests ===

    #[test]
    fn test_auth_state_clone() {
        let state = AuthState {
            api_key: Some("secret".to_string()),
        };
        let cloned = state.clone();
        assert_eq!(cloned.api_key, Some("secret".to_string()));
    }

    #[test]
    fn test_auth_state_no_key() {
        let state = AuthState { api_key: None };
        assert!(state.api_key.is_none());
    }

    // === Additional Request types tests ===

    #[test]
    fn test_prompt_request_deserialize() {
        let json = r#"{"prompt": "Hello, world!"}"#;
        let req: PromptRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.prompt, "Hello, world!");
    }

    #[test]
    fn test_model_request_deserialize() {
        let json = r#"{"model": "claude-3-opus"}"#;
        let req: ModelRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "claude-3-opus");
    }

    #[test]
    fn test_agent_request_deserialize() {
        let json = r#"{"agent": "coder"}"#;
        let req: AgentRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.agent, "coder");
    }

    #[test]
    fn test_session_switch_request_deserialize() {
        let json = r#"{"session_id": "sess-123"}"#;
        let req: SessionSwitchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.session_id, "sess-123");
    }

    #[test]
    fn test_session_rename_request_deserialize() {
        let json = r#"{"title": "New Session Title"}"#;
        let req: SessionRenameRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, "New Session Title");
    }

    #[test]
    fn test_session_fork_request_deserialize_with_message() {
        let json = r#"{"message_id": "msg-456"}"#;
        let req: SessionForkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message_id, Some("msg-456".to_string()));
    }

    #[test]
    fn test_session_fork_request_deserialize_without_message() {
        let json = r#"{}"#;
        let req: SessionForkRequest = serde_json::from_str(json).unwrap();
        assert!(req.message_id.is_none());
    }

    #[test]
    fn test_revert_request_deserialize() {
        let json = r#"{"message_id": "msg-789"}"#;
        let req: RevertRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message_id, "msg-789");
    }

    #[test]
    fn test_mcp_toggle_request_deserialize() {
        let json = r#"{"name": "my-mcp-server"}"#;
        let req: McpToggleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-mcp-server");
    }

    #[test]
    fn test_mcp_reconnect_request_deserialize() {
        let json = r#"{"name": "filesystem"}"#;
        let req: McpReconnectRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "filesystem");
    }

    #[test]
    fn test_goto_request_deserialize() {
        let json = r#"{"message_id": "msg-navigate-to"}"#;
        let req: GotoRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message_id, "msg-navigate-to");
    }

    #[test]
    fn test_settings_request_deserialize() {
        let json = r#"{"scope": "project", "config": {"key": "value"}}"#;
        let req: SettingsRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.config.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn test_permission_request_deserialize() {
        let json = r#"{"request_id": "perm-123", "allow": true, "remember": false}"#;
        let req: PermissionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.request_id, "perm-123");
        assert!(req.allow);
        assert!(!req.remember);
    }

    #[test]
    fn test_permission_request_deserialize_denied() {
        let json = r#"{"request_id": "perm-456", "allow": false, "remember": true}"#;
        let req: PermissionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.request_id, "perm-456");
        assert!(!req.allow);
        assert!(req.remember);
    }

    #[test]
    fn test_git_unstage_request_deserialize() {
        let json = r#"{"paths": ["src/lib.rs", "Cargo.toml"]}"#;
        let req: GitUnstageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.paths.len(), 2);
        assert_eq!(req.paths[0], "src/lib.rs");
        assert_eq!(req.paths[1], "Cargo.toml");
    }

    #[test]
    fn test_git_unstage_request_deserialize_empty() {
        let json = r#"{"paths": []}"#;
        let req: GitUnstageRequest = serde_json::from_str(json).unwrap();
        assert!(req.paths.is_empty());
    }
}
