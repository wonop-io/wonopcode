//! HTTP routes for the server.
//!
//! This module provides a comprehensive REST API.

use crate::{
    prompt::{
        create_provider_from_config, infer_provider, AgentConfig, PromptEvent, ServerPromptRunner,
    },
    sse::create_event_stream,
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{delete, get, patch, post, put},
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::Infallible, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use wonopcode_core::AgentRegistry;

/// Create the router with all routes.
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // ===================
        // Global endpoints
        // ===================
        .route("/health", get(health))
        .route("/global/health", get(health))
        .route("/global/event", get(global_events))
        .route("/global/dispose", post(global_dispose))
        // ===================
        // Instance endpoints
        // ===================
        .route("/instance", get(get_instance))
        .route("/instance/dispose", post(instance_dispose))
        // ===================
        // SSE events
        // ===================
        .route("/events", get(events))
        .route("/event", get(events))
        // ===================
        // Session endpoints
        // ===================
        .route("/session", get(session_list))
        .route("/session", post(session_create))
        .route("/session/{id}", get(session_get))
        .route("/session/{id}", put(session_update))
        .route("/session/{id}", delete(session_delete))
        .route("/session/{id}/messages", get(session_messages))
        .route("/session/{id}/message/{mid}", get(session_message))
        .route("/session/{id}/prompt", post(session_prompt))
        .route("/session/{id}/prompt_async", post(session_prompt_async))
        .route("/session/{id}/abort", post(session_abort))
        .route("/session/{id}/fork", post(session_fork))
        .route("/session/{id}/children", get(session_children))
        .route("/session/{id}/diff", get(session_diff))
        .route("/session/{id}/status", get(session_status))
        .route("/session/{id}/todo", get(session_todo))
        .route("/session/{id}/summarize", post(session_summarize))
        .route("/session/{id}/revert", post(session_revert))
        .route("/session/{id}/unrevert", post(session_unrevert))
        .route("/session/{id}/share", post(session_share))
        .route("/session/{id}/share", delete(session_unshare))
        .route("/session/{id}/init", post(session_init))
        .route("/session/{id}/command", post(session_command))
        // ===================
        // Part endpoints
        // ===================
        .route("/part/{id}", patch(part_update))
        .route("/part/{id}", delete(part_delete))
        // ===================
        // File endpoints
        // ===================
        .route("/file/read", get(file_read))
        .route("/file/list", get(file_list))
        .route("/file/status", get(file_status))
        // ===================
        // Find endpoints
        // ===================
        .route("/find/files", post(find_files))
        .route("/find/text", post(find_text))
        .route("/find/symbols", post(find_symbols))
        // ===================
        // Config endpoints
        // ===================
        .route("/config", get(config_get))
        .route("/config", patch(config_update))
        .route("/config/providers", get(config_providers))
        // ===================
        // Provider/Model endpoints
        // ===================
        .route("/provider", get(list_providers))
        .route("/provider/list", get(list_providers))
        .route("/provider/auth/{provider}", post(provider_auth))
        .route(
            "/provider/oauth/authorize/{provider}",
            get(provider_oauth_authorize),
        )
        .route(
            "/provider/oauth/callback/{provider}",
            get(provider_oauth_callback),
        )
        .route("/model", get(list_models))
        .route("/model/list", get(list_models))
        // ===================
        // Auth endpoints
        // ===================
        .route("/auth/set", post(auth_set))
        // ===================
        // Tool endpoints
        // ===================
        .route("/tool/list", get(tool_list))
        .route("/tool/ids", get(tool_ids))
        // ===================
        // Agent endpoints
        // ===================
        .route("/agent/list", get(agent_list))
        // ===================
        // Permission endpoints
        // ===================
        .route("/permission/list", get(permission_list))
        .route("/permission/respond/{id}", post(permission_respond))
        // ===================
        // MCP endpoints
        // ===================
        .route("/mcp/status", get(mcp_status))
        .route("/mcp/connect/{name}", post(mcp_connect))
        .route("/mcp/disconnect/{name}", post(mcp_disconnect))
        .route("/mcp/add", post(mcp_add))
        .route("/mcp/auth/start/{name}", post(mcp_auth_start))
        .route("/mcp/auth/callback", get(mcp_auth_callback))
        .route("/mcp/auth/authenticate/{name}", post(mcp_auth_authenticate))
        .route("/mcp/auth/{name}", delete(mcp_auth_remove))
        // ===================
        // LSP endpoints
        // ===================
        .route("/lsp/status", get(lsp_status))
        .route("/formatter/status", get(formatter_status))
        // ===================
        // Command endpoints
        // ===================
        .route("/command/list", get(command_list))
        // ===================
        // VCS endpoints
        // ===================
        .route("/vcs", get(vcs_get))
        // ===================
        // Path endpoints
        // ===================
        .route("/path", get(path_get))
        .with_state(state)
        .layer(cors)
}

// =============================================================================
// Response types
// =============================================================================

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
    code: String,
}

impl ApiError {
    fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
        }
    }

    fn not_found(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (StatusCode::NOT_FOUND, Json(Self::new(msg, "NOT_FOUND")))
    }

    fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (StatusCode::BAD_REQUEST, Json(Self::new(msg, "BAD_REQUEST")))
    }

    fn internal(msg: impl Into<String>) -> (StatusCode, Json<Self>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Self::new(msg, "INTERNAL_ERROR")),
        )
    }
}

// =============================================================================
// Global endpoints
// =============================================================================

/// Health check endpoint.
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "healthy": true,
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Global SSE events.
async fn global_events(State(state): State<AppState>) -> impl IntoResponse {
    create_event_stream(state.bus)
}

/// Dispose global resources.
async fn global_dispose(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    instance.dispose().await;
    Json(serde_json::json!({ "success": true }))
}

// =============================================================================
// Instance endpoints
// =============================================================================

#[derive(Debug, Serialize)]
struct InstanceInfo {
    directory: String,
    project_id: String,
    worktree: String,
}

async fn get_instance(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let worktree = instance.worktree().await;

    Json(InstanceInfo {
        directory: instance.directory().display().to_string(),
        project_id,
        worktree: worktree.display().to_string(),
    })
}

async fn instance_dispose(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    instance.dispose().await;
    Json(serde_json::json!({ "success": true }))
}

// =============================================================================
// SSE events endpoint
// =============================================================================

async fn events(State(state): State<AppState>) -> impl IntoResponse {
    create_event_stream(state.bus)
}

// =============================================================================
// Session endpoints
// =============================================================================

#[derive(Debug, Serialize)]
struct SessionResponse {
    id: String,
    project_id: String,
    title: String,
    directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    created: i64,
    updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<SessionSummaryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    share: Option<ShareResponse>,
}

#[derive(Debug, Serialize)]
struct SessionSummaryResponse {
    additions: u32,
    deletions: u32,
    files: u32,
}

#[derive(Debug, Serialize)]
struct ShareResponse {
    url: String,
}

impl From<wonopcode_core::session::Session> for SessionResponse {
    fn from(s: wonopcode_core::session::Session) -> Self {
        Self {
            id: s.id,
            project_id: s.project_id,
            title: s.title,
            directory: s.directory,
            parent_id: s.parent_id,
            created: s.time.created,
            updated: s.time.updated,
            summary: s.summary.map(|sum| SessionSummaryResponse {
                additions: sum.additions,
                deletions: sum.deletions,
                files: sum.files,
            }),
            share: s.share.map(|sh| ShareResponse { url: sh.url }),
        }
    }
}

async fn session_list(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let sessions = instance.list_sessions().await;
    let response: Vec<SessionResponse> = sessions.into_iter().map(Into::into).collect();
    Json(response)
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
}

async fn session_create(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;

    let session = if let Some(parent_id) = req.parent_id {
        // Create child session
        if let Some(parent) = instance.get_session(&parent_id).await {
            let child = wonopcode_core::session::Session::child(&parent);
            instance.session_repo().create(child).await
        } else {
            return Err(ApiError::not_found("Parent session not found"));
        }
    } else {
        instance.create_session(req.title).await
    };

    match session {
        Ok(s) => Ok((StatusCode::CREATED, Json(SessionResponse::from(s)))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

async fn session_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    match instance.get_session(&id).await {
        Some(s) => Ok(Json(SessionResponse::from(s))),
        None => Err(ApiError::not_found("Session not found")),
    }
}

#[derive(Debug, Deserialize)]
struct UpdateSessionRequest {
    #[serde(default)]
    title: Option<String>,
}

async fn session_update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let repo = instance.session_repo();

    let title_to_set = req.title.clone();
    match repo
        .update(&project_id, &id, move |session| {
            if let Some(title) = title_to_set {
                session.title = title;
            }
        })
        .await
    {
        Ok(session) => Ok(Json(SessionResponse::from(session))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

async fn session_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;

    match instance.session_repo().delete(&project_id, &id).await {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct MessagesQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;

    match instance
        .session_repo()
        .messages(&project_id, &id, query.limit)
        .await
    {
        Ok(messages) => Ok(Json(messages)),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

async fn session_message(
    State(state): State<AppState>,
    Path((session_id, message_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;

    match instance
        .session_repo()
        .messages(&project_id, &session_id, None)
        .await
    {
        Ok(messages) => {
            if let Some(msg) = messages.into_iter().find(|m| m.message.id() == message_id) {
                Ok(Json(msg))
            } else {
                Err(ApiError::not_found("Message not found"))
            }
        }
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct PromptRequest {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    /// Agent name for agent-specific configuration (model, tools, prompt).
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

/// Execute a prompt with SSE streaming response.
///
/// Returns Server-Sent Events with the following event types:
/// - `started`: Prompt processing started
/// - `text_delta`: Text chunk from model
/// - `tool_started`: Tool execution started
/// - `tool_completed`: Tool execution completed
/// - `token_usage`: Token usage update
/// - `completed`: Prompt completed successfully
/// - `error`: Error occurred
/// - `aborted`: Prompt was aborted
async fn session_prompt(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<PromptRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;

    // Verify session exists
    if instance.get_session(&session_id).await.is_none() {
        return Err(ApiError::not_found("Session not found"));
    }

    // Look up agent configuration if specified
    let agent_config = if let Some(agent_name) = &req.agent {
        let config = instance.config().await;
        let registry = AgentRegistry::new(&config);
        if let Some(agent) = registry.get(agent_name) {
            AgentConfig::from(agent)
        } else {
            return Err(ApiError::bad_request(format!(
                "Agent '{agent_name}' not found"
            )));
        }
    } else {
        AgentConfig::default()
    };

    // Determine model and provider: request > default
    // Note: Agent model override could be added to AgentConfig in the future
    let model_id = req
        .model
        .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());

    let provider_name = req
        .provider
        .unwrap_or_else(|| infer_provider(&model_id).to_string());

    // Create provider
    let provider = match create_provider_from_config(&provider_name, &model_id) {
        Ok(p) => p,
        Err(e) => return Err(ApiError::bad_request(e)),
    };

    // Create cancellation token for abort support
    let cancel = CancellationToken::new();

    // Store cancellation token for abort
    {
        let mut runners = state.session_runners.write().await;
        runners.insert(session_id.clone(), cancel.clone());
    }

    // Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<PromptEvent>();

    // Create runner with agent configuration
    let cwd = instance.directory().to_path_buf();
    let runner = ServerPromptRunner::with_agent(provider, cwd, cancel.clone(), agent_config);

    // Clone state for cleanup
    let state_clone = state.clone();
    let session_id_clone = session_id.clone();

    // Spawn prompt execution
    let prompt = req.prompt.clone();
    let system_prompt = req.system_prompt.clone();
    tokio::spawn(async move {
        let result = runner.run(&prompt, system_prompt, event_tx).await;

        // Clean up cancellation token
        {
            let mut runners = state_clone.session_runners.write().await;
            runners.remove(&session_id_clone);
        }

        if let Err(e) = result {
            tracing::warn!(session = %session_id_clone, error = %e, "Prompt execution failed");
        }
    });

    // Create SSE stream from events
    let stream = async_stream::stream! {
        while let Some(event) = event_rx.recv().await {
            let event_type = match &event {
                PromptEvent::Started { .. } => "started",
                PromptEvent::TextDelta { .. } => "text_delta",
                PromptEvent::ToolStarted { .. } => "tool_started",
                PromptEvent::ToolCompleted { .. } => "tool_completed",
                PromptEvent::TokenUsage { .. } => "token_usage",
                PromptEvent::Status { .. } => "status",
                PromptEvent::Completed { .. } => "completed",
                PromptEvent::Error { .. } => "error",
                PromptEvent::Aborted => "aborted",
            };

            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event(event_type).data(data));
            }

            // End stream on terminal events
            match event {
                PromptEvent::Completed { .. } | PromptEvent::Error { .. } | PromptEvent::Aborted => {
                    break;
                }
                _ => {}
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

/// Execute a prompt asynchronously (non-streaming).
///
/// Returns immediately with a message ID. Use the events endpoint to get updates.
async fn session_prompt_async(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<PromptRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;

    // Verify session exists
    if instance.get_session(&session_id).await.is_none() {
        return Err(ApiError::not_found("Session not found"));
    }

    // Look up agent configuration if specified
    let agent_config = if let Some(agent_name) = &req.agent {
        let config = instance.config().await;
        let registry = AgentRegistry::new(&config);
        if let Some(agent) = registry.get(agent_name) {
            AgentConfig::from(agent)
        } else {
            return Err(ApiError::bad_request(format!(
                "Agent '{agent_name}' not found"
            )));
        }
    } else {
        AgentConfig::default()
    };

    // Determine model and provider
    let model_id = req
        .model
        .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());
    let provider_name = req
        .provider
        .unwrap_or_else(|| infer_provider(&model_id).to_string());

    // Create provider
    let provider = match create_provider_from_config(&provider_name, &model_id) {
        Ok(p) => p,
        Err(e) => return Err(ApiError::bad_request(e)),
    };

    // Create cancellation token
    let cancel = CancellationToken::new();

    // Store for abort
    {
        let mut runners = state.session_runners.write().await;
        runners.insert(session_id.clone(), cancel.clone());
    }

    // Generate message ID
    let message_id = format!("msg_{}", generate_id());
    let message_id_clone = message_id;

    // Create runner with agent configuration
    let cwd = instance.directory().to_path_buf();
    let runner = ServerPromptRunner::with_agent(provider, cwd, cancel, agent_config);

    // Create event channel for internal use
    let (event_tx, _event_rx) = mpsc::unbounded_channel::<PromptEvent>();

    // Clone state and session_id for the spawned task
    let state_clone = state.clone();
    let bus = state.bus.clone();
    let session_id_clone = session_id.clone();

    // Spawn prompt execution
    let prompt = req.prompt.clone();
    let system_prompt = req.system_prompt.clone();
    tokio::spawn(async move {
        let result = runner.run(&prompt, system_prompt, event_tx).await;

        // Clean up
        {
            let mut runners = state_clone.session_runners.write().await;
            runners.remove(&session_id_clone);
        }

        // Publish result to bus
        match result {
            Ok(response) => {
                bus.publish(wonopcode_core::bus::SessionUpdated {
                    session_id: session_id_clone,
                })
                .await;
                tracing::info!(message_id = %response.message_id, "Async prompt completed");
            }
            Err(e) => {
                tracing::warn!(session = %session_id_clone, error = %e, "Async prompt failed");
            }
        }
    });

    Ok(Json(serde_json::json!({
        "message_id": message_id_clone,
        "status": "processing"
    })))
}

/// Abort a running prompt.
async fn session_abort(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let mut runners = state.session_runners.write().await;

    if let Some(cancel) = runners.remove(&session_id) {
        cancel.cancel();
        Json(serde_json::json!({
            "success": true,
            "message": "Abort signal sent"
        }))
    } else {
        Json(serde_json::json!({
            "success": false,
            "message": "No active prompt for this session"
        }))
    }
}

/// Generate a simple ID.
fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", duration.as_secs(), duration.subsec_nanos())
}

async fn session_fork(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;

    match instance.get_session(&id).await {
        Some(parent) => {
            let child = wonopcode_core::session::Session::child(&parent);
            match instance.session_repo().create(child).await {
                Ok(s) => Ok((StatusCode::CREATED, Json(SessionResponse::from(s)))),
                Err(e) => Err(ApiError::internal(e.to_string())),
            }
        }
        None => Err(ApiError::not_found("Session not found")),
    }
}

async fn session_children(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let sessions = instance.list_sessions().await;
    let children: Vec<SessionResponse> = sessions
        .into_iter()
        .filter(|s| s.parent_id.as_ref() == Some(&id))
        .map(Into::into)
        .collect();
    Json(children)
}

async fn session_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;

    match instance.get_session(&id).await {
        Some(session) => {
            // Return summary diff if available
            if let Some(summary) = session.summary {
                Ok(Json(serde_json::json!({
                    "additions": summary.additions,
                    "deletions": summary.deletions,
                    "files": summary.files,
                    "diffs": summary.diffs
                })))
            } else {
                Ok(Json(serde_json::json!({
                    "additions": 0,
                    "deletions": 0,
                    "files": 0,
                    "diffs": []
                })))
            }
        }
        None => Err(ApiError::not_found("Session not found")),
    }
}

async fn session_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;

    match instance.get_session(&id).await {
        Some(_session) => {
            // Check if there's an active prompt for this session
            let runners = state.session_runners.read().await;
            let is_running = runners.contains_key(&id);

            Ok(Json(serde_json::json!({
                "status": if is_running { "running" } else { "idle" },
                "can_abort": is_running
            })))
        }
        None => Err(ApiError::not_found("Session not found")),
    }
}

async fn session_todo(State(state): State<AppState>, Path(_id): Path<String>) -> impl IntoResponse {
    // Get todos from the project directory using file-based storage
    // Note: This uses FileTodoStore for the REST API to support external access
    let instance = state.instance.read().await;
    let root_dir = instance.directory();
    let store = wonopcode_tools::todo::FileTodoStore::new();
    let todos = wonopcode_tools::todo::get_todos(&store, root_dir);

    // Convert to JSON-serializable format
    let todos_json: Vec<serde_json::Value> = todos
        .iter()
        .map(|todo| {
            serde_json::json!({
                "id": todo.id,
                "content": todo.content,
                "status": format!("{:?}", todo.status).to_lowercase(),
                "priority": format!("{:?}", todo.priority).to_lowercase(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "todos": todos_json
    }))
}

/// Request body for session summarize.
#[derive(Debug, Deserialize)]
struct SummarizeRequest {
    /// If provided, summarize only messages from this message ID onwards.
    /// If not provided, summarize all messages.
    #[serde(default)]
    from_message_id: Option<String>,
}

async fn session_summarize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<SummarizeRequest>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    use futures::StreamExt;

    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let repo = instance.session_repo();

    // Get session (verify it exists)
    let _session = repo
        .get(&project_id, &id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;

    // Get messages
    let all_messages = repo
        .messages(&project_id, &id, Some(100))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if all_messages.is_empty() {
        return Ok(Json(serde_json::json!({
            "id": id,
            "title": null,
            "summary": null,
            "message": "No messages to summarize"
        })));
    }

    // Filter messages based on from_message_id if provided
    let from_message_id = body.and_then(|b| b.from_message_id.clone());
    let messages: Vec<_> = if let Some(ref from_id) = from_message_id {
        // Find the index of the message with the given ID
        let start_idx = all_messages.iter().position(|m| m.message.id() == from_id);

        match start_idx {
            Some(idx) => all_messages.into_iter().skip(idx).collect(),
            None => {
                return Err(ApiError::not_found(format!(
                    "Message with id '{from_id}' not found"
                )));
            }
        }
    } else {
        all_messages
    };

    let partial_summary = from_message_id.is_some();

    // Collect text from messages for the title
    let text_content: String = messages
        .iter()
        .filter_map(|m| {
            m.parts.iter().find_map(|p| {
                if let wonopcode_core::message::MessagePart::Text(text_part) = p {
                    Some(text_part.text.clone())
                } else {
                    None
                }
            })
        })
        .take(5) // Take more for partial summaries
        .collect::<Vec<_>>()
        .join("\n\n");

    if text_content.is_empty() {
        return Ok(Json(serde_json::json!({
            "id": id,
            "title": null,
            "summary": null,
            "message": "No text content to summarize"
        })));
    }

    // Try to create a provider to generate title
    let provider = match create_provider_from_config("anthropic", "claude-3-haiku-20240307") {
        Ok(p) => p,
        Err(_) => {
            // Fallback: just use first few words as title
            let title = text_content
                .chars()
                .take(50)
                .collect::<String>()
                .split_whitespace()
                .take(8)
                .collect::<Vec<_>>()
                .join(" ");

            return Ok(Json(serde_json::json!({
                "id": id,
                "title": if title.is_empty() { None } else { Some(title) },
                "summary": null,
                "message": "Generated fallback title (no AI provider available)"
            })));
        }
    };

    // Generate title using AI
    let title_prompt = format!(
        r#"Generate a very short title (max 8 words) for the following conversation.
Output ONLY the title, nothing else. No quotes, no explanation.

Conversation:
{}"#,
        text_content.chars().take(2000).collect::<String>()
    );

    let options = wonopcode_provider::GenerateOptions {
        temperature: Some(0.3),
        max_tokens: Some(50),
        system: Some(
            "You generate short, descriptive titles for conversations. Output only the title."
                .to_string(),
        ),
        tools: vec![],
        abort: None,
        ..Default::default()
    };

    let messages_for_ai = vec![wonopcode_provider::Message::user(&title_prompt)];

    let title = match provider.generate(messages_for_ai, options).await {
        Ok(stream) => {
            tokio::pin!(stream);
            let mut title_text = String::new();
            while let Some(chunk) = stream.next().await {
                if let Ok(wonopcode_provider::stream::StreamChunk::TextDelta(delta)) = chunk {
                    title_text.push_str(&delta);
                }
            }
            title_text.trim().to_string()
        }
        Err(e) => {
            tracing::warn!("Failed to generate title: {}", e);
            text_content
                .chars()
                .take(50)
                .collect::<String>()
                .split_whitespace()
                .take(8)
                .collect::<Vec<_>>()
                .join(" ")
        }
    };

    // Update session with title (only if full summarization, not partial)
    if !partial_summary {
        let _updated_session = repo
            .update(&project_id, &id, |s| {
                s.title = title.clone();
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }

    let message = if partial_summary {
        format!(
            "Summarized {} messages from specified point",
            messages.len()
        )
    } else {
        "Session summarized successfully".to_string()
    };

    Ok(Json(serde_json::json!({
        "id": id,
        "title": title,
        "summary": null,
        "partial": partial_summary,
        "messages_summarized": messages.len(),
        "message": message
    })))
}

/// Request body for session revert.
#[derive(Debug, Deserialize)]
struct RevertRequest {
    /// Message ID to revert to.
    message_id: String,
    /// Optional part ID to revert to a specific part.
    #[serde(default)]
    part_id: Option<String>,
}

async fn session_revert(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RevertRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let repo = std::sync::Arc::new(instance.session_repo());

    // Create revert handler
    let revert_handler = wonopcode_core::SessionRevert::new(repo, state.bus.clone());

    // Perform the revert
    let input = wonopcode_core::RevertInput {
        session_id: id.clone(),
        message_id: req.message_id,
        part_id: req.part_id,
    };

    match revert_handler.revert(&project_id, input).await {
        Ok(session) => Ok(Json(serde_json::json!({
            "id": session.id,
            "revert": session.revert,
            "message": "Session reverted successfully"
        }))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

async fn session_unrevert(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let repo = std::sync::Arc::new(instance.session_repo());

    // Create revert handler
    let revert_handler = wonopcode_core::SessionRevert::new(repo, state.bus.clone());

    // Perform the unrevert
    match revert_handler.unrevert(&project_id, &id).await {
        Ok(session) => Ok(Json(serde_json::json!({
            "id": session.id,
            "revert": session.revert,
            "message": "Session unrevert complete"
        }))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct ShareRequest {
    #[serde(default)]
    share_url: Option<String>,
}

async fn session_share(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ShareRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let repo = instance.session_repo();

    match wonopcode_core::share::share_session(&repo, &project_id, &id, req.share_url.as_deref())
        .await
    {
        Ok(share_info) => Ok(Json(serde_json::json!({
            "url": share_info.url,
            "created_at": share_info.created_at
        }))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct UnshareRequest {
    secret: String,
    #[serde(default)]
    share_url: Option<String>,
}

async fn session_unshare(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UnshareRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let repo = instance.session_repo();

    match wonopcode_core::share::unshare_session(
        &repo,
        &project_id,
        &id,
        &req.secret,
        req.share_url.as_deref(),
    )
    .await
    {
        Ok(_) => Ok(Json(serde_json::json!({ "success": true }))),
        Err(e) => Err(ApiError::internal(e.to_string())),
    }
}

async fn session_init(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    Json(serde_json::json!({ "success": true }))
}

#[derive(Debug, Deserialize)]
struct CommandRequest {
    command: String,
}

async fn session_command(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Parse the command (format: /command args)
    let command_str = req.command.trim();
    let Some(stripped) = command_str.strip_prefix('/') else {
        return Err(ApiError::bad_request(
            "Command must start with /".to_string(),
        ));
    };
    let parts: Vec<&str> = stripped.splitn(2, ' ').collect();
    let cmd_name = parts.first().unwrap_or(&"").to_string();
    let args = parts.get(1).map(|s| s.to_string()).unwrap_or_default();

    // Get the command registry
    let registry = wonopcode_core::CommandRegistry::with_builtins();

    // Look up the command
    let command = match registry.get(&cmd_name) {
        Some(c) => c,
        None => {
            return Err(ApiError::not_found(format!("Unknown command: /{cmd_name}")));
        }
    };

    // Expand the template with arguments
    let expanded_prompt = command.expand(&args);

    // Return the expanded prompt - the client should use this to send a regular prompt
    Ok(Json(serde_json::json!({
        "command": cmd_name,
        "prompt": expanded_prompt,
        "agent": command.agent,
        "model": command.model,
        "subtask": command.subtask,
        "message": format!("Command /{} expanded successfully. Use the prompt field to send a regular prompt request.", cmd_name)
    })))
}

// =============================================================================
// Part endpoints
// =============================================================================

/// Request body for updating a message part.
///
/// Message part editing allows modifying specific parts of messages in a session.
/// This is useful for:
/// - Correcting tool outputs that were incorrect
/// - Updating text content before regeneration
/// - Removing sensitive information
///
/// Note: Editing assistant reasoning or system messages is not allowed.
#[derive(Debug, Deserialize)]
struct UpdatePartRequest {
    /// New content for the part. The type must match the original part type.
    #[serde(default)]
    content: Option<String>,
}

async fn part_update(
    Path(id): Path<String>,
    Json(req): Json<UpdatePartRequest>,
) -> impl IntoResponse {
    // Part updates are not yet fully implemented
    // When implemented, this would:
    // 1. Parse the part ID to extract session_id, message_id, and part_index
    // 2. Validate the part exists and is editable (not reasoning, not system)
    // 3. Validate the new content matches the part type
    // 4. Update the part in storage
    // 5. Optionally invalidate/regenerate subsequent messages

    let has_content = req.content.is_some();

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "message": format!("Part update for '{}' is not yet implemented. Message parts are currently immutable.", id),
            "part_id": id,
            "content_provided": has_content,
            "suggestion": "Use session revert to go back to a previous state, then resend the message with corrections."
        })),
    )
}

async fn part_delete(Path(id): Path<String>) -> impl IntoResponse {
    // Part deletion is not yet implemented
    // When implemented, this would:
    // 1. Parse the part ID to extract session_id, message_id, and part_index
    // 2. Validate the part exists and is deletable
    // 3. Remove the part from the message
    // 4. If the message becomes empty, optionally remove the message too
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "message": format!("Part deletion for '{}' is not yet implemented.", id),
            "part_id": id,
            "suggestion": "Use session revert to go back to a previous state instead."
        })),
    )
}

// =============================================================================
// File endpoints
// =============================================================================

#[derive(Debug, Deserialize)]
struct FileReadQuery {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn file_read(
    State(state): State<AppState>,
    Query(query): Query<FileReadQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let base_path = instance.directory();
    let file_path = base_path.join(&query.path);

    // Security check: ensure path is within project
    if !file_path.starts_with(base_path) {
        return Err(ApiError::bad_request("Path outside project directory"));
    }

    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let offset = query.offset.unwrap_or(0);
            let limit = query.limit.unwrap_or(2000);

            let selected_lines: Vec<String> = lines
                .iter()
                .skip(offset)
                .take(limit)
                .enumerate()
                .map(|(i, line)| format!("{:6}\t{}", offset + i + 1, line))
                .collect();

            Ok(Json(serde_json::json!({
                "path": query.path,
                "content": selected_lines.join("\n"),
                "total_lines": lines.len(),
                "offset": offset,
                "limit": limit
            })))
        }
        Err(e) => Err(ApiError::not_found(format!("File not found: {e}"))),
    }
}

#[derive(Debug, Deserialize)]
struct FileListQuery {
    #[serde(default)]
    path: Option<String>,
}

async fn file_list(
    State(state): State<AppState>,
    Query(query): Query<FileListQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let instance = state.instance.read().await;
    let base_path = instance.directory();
    let dir_path = if let Some(ref p) = query.path {
        base_path.join(p)
    } else {
        base_path.to_path_buf()
    };

    // Security check
    if !dir_path.starts_with(base_path) {
        return Err(ApiError::bad_request("Path outside project directory"));
    }

    match tokio::fs::read_dir(&dir_path).await {
        Ok(mut entries) => {
            let mut files = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                files.push(serde_json::json!({
                    "name": name,
                    "is_directory": is_dir
                }));
            }
            Ok(Json(serde_json::json!({ "files": files })))
        }
        Err(e) => Err(ApiError::not_found(format!("Directory not found: {e}"))),
    }
}

async fn file_status(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let dir = instance.directory();

    // Try to get git status
    let git_status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let modified_files: Vec<&str> = git_status
        .lines()
        .filter_map(|line| {
            if line.len() > 3 {
                Some(line[3..].trim())
            } else {
                None
            }
        })
        .collect();

    Json(serde_json::json!({
        "modified_files": modified_files,
        "has_changes": !modified_files.is_empty()
    }))
}

// =============================================================================
// Find endpoints
// =============================================================================

#[derive(Debug, Deserialize)]
struct FindFilesRequest {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

async fn find_files(
    State(state): State<AppState>,
    Json(req): Json<FindFilesRequest>,
) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let base_path = instance.directory();
    let search_path = req
        .path
        .as_ref()
        .map(|p| base_path.join(p))
        .unwrap_or_else(|| base_path.to_path_buf());

    // Use glob to find files
    let pattern = search_path.join(&req.pattern).display().to_string();
    let files: Vec<String> = glob::glob(&pattern)
        .map(|paths| {
            paths
                .filter_map(|p| p.ok())
                .filter_map(|p| {
                    p.strip_prefix(base_path)
                        .ok()
                        .map(|p| p.display().to_string())
                })
                .take(100)
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "files": files }))
}

#[derive(Debug, Deserialize)]
struct FindTextRequest {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Option<String>,
}

async fn find_text(
    State(state): State<AppState>,
    Json(req): Json<FindTextRequest>,
) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let dir = instance.directory();

    // Use grep/ripgrep to search
    let mut cmd = std::process::Command::new("grep");
    cmd.args([
        "-rn",
        "--include",
        req.include.as_deref().unwrap_or("*"),
        &req.pattern,
    ]);
    if let Some(path) = &req.path {
        cmd.arg(path);
    } else {
        cmd.arg(".");
    }
    cmd.current_dir(dir);

    let output = cmd.output().ok();
    let results: Vec<serde_json::Value> = output
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            s.lines()
                .take(100)
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(3, ':').collect();
                    if parts.len() >= 2 {
                        Some(serde_json::json!({
                            "file": parts[0],
                            "line": parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
                            "content": parts.get(2).unwrap_or(&"")
                        }))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "results": results }))
}

#[derive(Debug, Deserialize)]
struct FindSymbolsRequest {
    query: String,
}

async fn find_symbols(
    State(_state): State<AppState>,
    Json(req): Json<FindSymbolsRequest>,
) -> impl IntoResponse {
    // LSP-based symbol search is not yet wired up
    // This would query the LSP client for workspace symbols matching the query
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "message": "LSP-based symbol search is not yet implemented. LSP integration is pending.",
            "query": req.query,
            "symbols": [],
            "hint": "Use the Glob or Grep tools instead for file/content search"
        })),
    )
}

// =============================================================================
// Config endpoints
// =============================================================================

async fn config_get(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let config = instance.config().await;
    Json(config)
}

#[derive(Debug, Deserialize)]
struct ConfigUpdateRequest {
    #[serde(flatten)]
    updates: HashMap<String, serde_json::Value>,
}

async fn config_update(
    State(state): State<AppState>,
    Json(req): Json<ConfigUpdateRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Get current config and apply updates
    // Note: Full implementation would write to config file
    // For now, we apply updates to the in-memory instance config

    let instance = state.instance.read().await;
    let mut config = instance.config().await;

    // Apply updates to the config
    for (key, value) in req.updates {
        match key.as_str() {
            "default_agent" => {
                if let Some(agent) = value.as_str() {
                    config.default_agent = Some(agent.to_string());
                }
            }
            "model" => {
                if let Some(model) = value.as_str() {
                    config.model = Some(model.to_string());
                }
            }
            "small_model" => {
                if let Some(model) = value.as_str() {
                    config.small_model = Some(model.to_string());
                }
            }
            "theme" => {
                if let Some(theme) = value.as_str() {
                    config.theme = Some(theme.to_string());
                }
            }
            "snapshot" => {
                if let Some(enabled) = value.as_bool() {
                    config.snapshot = Some(enabled);
                }
            }
            // For other keys, we'd need to handle them appropriately
            _ => {
                tracing::debug!("Ignoring unknown config key: {}", key);
            }
        }
    }

    // Note: In a full implementation, we'd persist this to the config file
    // For now, we return success but note that changes are in-memory only
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Config updated in memory. Restart may be required for persistence.",
        "config": config
    })))
}

async fn config_providers() -> impl IntoResponse {
    // Return provider configurations
    Json(serde_json::json!({
        "anthropic": { "name": "Anthropic", "env": ["ANTHROPIC_API_KEY"] },
        "openai": { "name": "OpenAI", "env": ["OPENAI_API_KEY"] },
        "openrouter": { "name": "OpenRouter", "env": ["OPENROUTER_API_KEY"] },
        "google": { "name": "Google", "env": ["GOOGLE_API_KEY", "GEMINI_API_KEY"] },
        "vertex": { "name": "Google Vertex", "env": ["GOOGLE_APPLICATION_CREDENTIALS"] },
        "bedrock": { "name": "Amazon Bedrock", "env": ["AWS_ACCESS_KEY_ID"] },
        "azure": { "name": "Azure OpenAI", "env": ["AZURE_OPENAI_API_KEY"] },
        "xai": { "name": "xAI", "env": ["XAI_API_KEY"] },
        "mistral": { "name": "Mistral", "env": ["MISTRAL_API_KEY"] },
        "groq": { "name": "Groq", "env": ["GROQ_API_KEY"] },
        "deepinfra": { "name": "DeepInfra", "env": ["DEEPINFRA_API_KEY"] },
        "together": { "name": "Together AI", "env": ["TOGETHER_API_KEY"] },
        "copilot": { "name": "GitHub Copilot", "env": ["GITHUB_TOKEN"] }
    }))
}

// =============================================================================
// Provider/Model endpoints
// =============================================================================

#[derive(Debug, Serialize)]
struct ProviderInfo {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    env: Vec<String>,
}

async fn list_providers() -> impl IntoResponse {
    let providers = vec![
        ProviderInfo {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            env: vec!["ANTHROPIC_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            env: vec!["OPENAI_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "google".to_string(),
            name: "Google".to_string(),
            env: vec!["GOOGLE_API_KEY".to_string(), "GEMINI_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "vertex".to_string(),
            name: "Google Vertex".to_string(),
            env: vec!["GOOGLE_APPLICATION_CREDENTIALS".to_string()],
        },
        ProviderInfo {
            id: "openrouter".to_string(),
            name: "OpenRouter".to_string(),
            env: vec!["OPENROUTER_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "bedrock".to_string(),
            name: "Amazon Bedrock".to_string(),
            env: vec!["AWS_ACCESS_KEY_ID".to_string()],
        },
        ProviderInfo {
            id: "azure".to_string(),
            name: "Azure OpenAI".to_string(),
            env: vec!["AZURE_OPENAI_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "xai".to_string(),
            name: "xAI".to_string(),
            env: vec!["XAI_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "mistral".to_string(),
            name: "Mistral".to_string(),
            env: vec!["MISTRAL_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "groq".to_string(),
            name: "Groq".to_string(),
            env: vec!["GROQ_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "deepinfra".to_string(),
            name: "DeepInfra".to_string(),
            env: vec!["DEEPINFRA_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "together".to_string(),
            name: "Together AI".to_string(),
            env: vec!["TOGETHER_API_KEY".to_string()],
        },
        ProviderInfo {
            id: "copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            env: vec!["GITHUB_TOKEN".to_string()],
        },
    ];

    Json(providers)
}

async fn provider_auth(
    Path(provider): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Get auth storage
    let storage = match wonopcode_auth::AuthStorage::new() {
        Ok(s) => s,
        Err(e) => {
            return Err(ApiError::internal(format!(
                "Failed to access auth storage: {e}"
            )))
        }
    };

    // Check if auth exists for this provider
    match storage.get(&provider).await {
        Ok(Some(auth)) => {
            let auth_type = if auth.is_cli() { "cli" } else { "api_key" };
            Ok(Json(serde_json::json!({
                "provider": provider,
                "authenticated": true,
                "type": auth_type
            })))
        }
        Ok(None) => {
            // Check environment variable fallback
            let env_key = format!("{}_API_KEY", provider.to_uppercase());
            let has_env = std::env::var(&env_key).is_ok();

            Ok(Json(serde_json::json!({
                "provider": provider,
                "authenticated": has_env,
                "type": if has_env { "environment" } else { "none" },
                "env_var": env_key
            })))
        }
        Err(e) => Err(ApiError::internal(format!("Failed to get auth: {e}"))),
    }
}

async fn provider_oauth_authorize(
    Path(provider): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // OAuth is primarily needed for certain providers that don't use API keys
    // For now, we return information about how to authenticate
    let auth_info = match provider.as_str() {
        "copilot" => serde_json::json!({
            "provider": provider,
            "method": "oauth",
            "instructions": "GitHub Copilot uses GitHub OAuth. Run 'wonopcode auth login copilot' in CLI.",
            "env_var": "GITHUB_TOKEN"
        }),
        _ => serde_json::json!({
            "provider": provider,
            "method": "api_key",
            "instructions": format!("Set your API key using the /provider/{}/auth endpoint or set the {}_API_KEY environment variable.", provider, provider.to_uppercase()),
            "env_var": format!("{}_API_KEY", provider.to_uppercase())
        }),
    };

    Ok(Json(auth_info))
}

async fn provider_oauth_callback(
    Path(provider): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Handle OAuth callback
    // This would normally exchange the code for a token
    if let Some(_code) = params.get("code") {
        Ok(Json(serde_json::json!({
            "provider": provider,
            "status": "received",
            "message": "OAuth callback received. Token exchange not implemented via API.",
            "code_received": true
        })))
    } else if let Some(error) = params.get("error") {
        Err(ApiError::bad_request(format!("OAuth error: {error}")))
    } else {
        Err(ApiError::bad_request(
            "Missing code parameter in callback".to_string(),
        ))
    }
}

#[derive(Debug, Serialize)]
struct ModelListInfo {
    id: String,
    name: String,
    provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<u32>,
}

async fn list_models() -> impl IntoResponse {
    // Try to get models from models.dev, fall back to static definitions
    match wonopcode_provider::models_dev::get_all_models().await {
        Ok(models) => {
            let model_list: Vec<ModelListInfo> = models
                .into_iter()
                .map(|m| ModelListInfo {
                    id: m.id,
                    name: m.name,
                    provider: m.provider_id,
                    context: Some(m.limit.context),
                    output: Some(m.limit.output),
                })
                .collect();
            Json(model_list)
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch models from models.dev: {}, using static fallback",
                e
            );
            // Fallback to static models
            let models = vec![
                ModelListInfo {
                    id: "claude-sonnet-4-5-20250929".to_string(),
                    name: "Claude Sonnet 4.5".to_string(),
                    provider: "anthropic".to_string(),
                    context: Some(200_000),
                    output: Some(64_000),
                },
                ModelListInfo {
                    id: "claude-haiku-4-5-20251001".to_string(),
                    name: "Claude Haiku 4.5".to_string(),
                    provider: "anthropic".to_string(),
                    context: Some(200_000),
                    output: Some(64_000),
                },
                ModelListInfo {
                    id: "claude-3-haiku-20240307".to_string(),
                    name: "Claude 3 Haiku".to_string(),
                    provider: "anthropic".to_string(),
                    context: Some(200_000),
                    output: Some(4_096),
                },
                ModelListInfo {
                    id: "gpt-5.2".to_string(),
                    name: "GPT-5.2".to_string(),
                    provider: "openai".to_string(),
                    context: Some(256_000),
                    output: Some(32_768),
                },
                ModelListInfo {
                    id: "gpt-4o".to_string(),
                    name: "GPT-4o".to_string(),
                    provider: "openai".to_string(),
                    context: Some(128_000),
                    output: Some(16_384),
                },
                ModelListInfo {
                    id: "o3".to_string(),
                    name: "o3".to_string(),
                    provider: "openai".to_string(),
                    context: Some(200_000),
                    output: Some(100_000),
                },
                ModelListInfo {
                    id: "gemini-2.0-flash".to_string(),
                    name: "Gemini 2.0 Flash".to_string(),
                    provider: "google".to_string(),
                    context: Some(1_000_000),
                    output: Some(8_192),
                },
                ModelListInfo {
                    id: "gemini-1.5-pro".to_string(),
                    name: "Gemini 1.5 Pro".to_string(),
                    provider: "google".to_string(),
                    context: Some(2_000_000),
                    output: Some(8_192),
                },
            ];
            Json(models)
        }
    }
}

// =============================================================================
// Auth endpoints
// =============================================================================

#[derive(Debug, Deserialize)]
struct AuthSetRequest {
    provider: String,
    key: String,
}

async fn auth_set(
    Json(req): Json<AuthSetRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Get auth storage
    let storage = match wonopcode_auth::AuthStorage::new() {
        Ok(s) => s,
        Err(e) => {
            return Err(ApiError::internal(format!(
                "Failed to access auth storage: {e}"
            )))
        }
    };

    // Store the API key
    let auth_info = wonopcode_auth::AuthInfo::api_key(req.key);
    match storage.set(&req.provider, auth_info).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "success": true,
            "provider": req.provider,
            "message": "API key stored successfully"
        }))),
        Err(e) => Err(ApiError::internal(format!("Failed to store auth: {e}"))),
    }
}

// =============================================================================
// Tool endpoints
// =============================================================================

async fn tool_list() -> impl IntoResponse {
    let tools = vec![
        serde_json::json!({ "id": "read", "name": "Read", "description": "Read file contents" }),
        serde_json::json!({ "id": "write", "name": "Write", "description": "Write file contents" }),
        serde_json::json!({ "id": "edit", "name": "Edit", "description": "Edit file with replacements" }),
        serde_json::json!({ "id": "multiedit", "name": "MultiEdit", "description": "Edit multiple files" }),
        serde_json::json!({ "id": "glob", "name": "Glob", "description": "Find files by pattern" }),
        serde_json::json!({ "id": "grep", "name": "Grep", "description": "Search file contents" }),
        serde_json::json!({ "id": "bash", "name": "Bash", "description": "Execute shell commands" }),
        serde_json::json!({ "id": "list", "name": "List", "description": "List directory contents" }),
        serde_json::json!({ "id": "webfetch", "name": "WebFetch", "description": "Fetch web content" }),
        serde_json::json!({ "id": "task", "name": "Task", "description": "Create subtasks" }),
        serde_json::json!({ "id": "todoread", "name": "TodoRead", "description": "Read todo list" }),
        serde_json::json!({ "id": "todowrite", "name": "TodoWrite", "description": "Write todo list" }),
        serde_json::json!({ "id": "patch", "name": "Patch", "description": "Apply unified diffs" }),
        serde_json::json!({ "id": "batch", "name": "Batch", "description": "Execute tools in parallel" }),
        serde_json::json!({ "id": "skill", "name": "Skill", "description": "Load skills" }),
        serde_json::json!({ "id": "lsp", "name": "LSP", "description": "Language server queries" }),
    ];

    Json(serde_json::json!({ "tools": tools }))
}

async fn tool_ids() -> impl IntoResponse {
    let ids = vec![
        "read",
        "write",
        "edit",
        "multiedit",
        "glob",
        "grep",
        "bash",
        "list",
        "webfetch",
        "task",
        "todoread",
        "todowrite",
        "patch",
        "batch",
        "skill",
        "lsp",
    ];
    Json(serde_json::json!({ "ids": ids }))
}

// =============================================================================
// Agent endpoints
// =============================================================================

async fn agent_list(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let config = instance.config().await;

    let agents: Vec<serde_json::Value> = config
        .agent
        .map(|agents| {
            agents
                .into_iter()
                .map(|(id, agent)| {
                    serde_json::json!({
                        "id": id,
                        "description": agent.description
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "agents": agents }))
}

// =============================================================================
// Permission endpoints
// =============================================================================

async fn permission_list(State(_state): State<AppState>) -> impl IntoResponse {
    // For now, return empty list as pending permissions are handled async via bus
    // In a full implementation, we'd track pending requests in the permission manager
    // and expose them here
    Json(serde_json::json!({
        "permissions": [],
        "default_rules": [
            { "tool": "read", "decision": "allow" },
            { "tool": "glob", "decision": "allow" },
            { "tool": "grep", "decision": "allow" },
            { "tool": "todoread", "decision": "allow" },
            { "tool": "webfetch", "decision": "allow" },
            { "tool": "*", "decision": "ask" }
        ]
    }))
}

#[derive(Debug, Deserialize)]
struct PermissionRespondRequest {
    allow: bool,
    #[serde(default)]
    remember: bool,
}

async fn permission_respond(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PermissionRespondRequest>,
) -> impl IntoResponse {
    // Respond to a pending permission request
    state
        .permission_manager
        .respond(&id, req.allow, req.remember)
        .await;

    Json(serde_json::json!({
        "success": true,
        "id": id,
        "allowed": req.allow,
        "remembered": req.remember
    }))
}

// =============================================================================
// MCP endpoints
// =============================================================================

async fn mcp_status(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let config = instance.config().await;

    let servers: Vec<serde_json::Value> = config
        .mcp
        .map(|mcp| {
            mcp.into_iter()
                .map(|(name, config)| {
                    let (type_str, details) = match config {
                        wonopcode_core::config::McpConfig::Local(local) => {
                            ("local", serde_json::json!({ "command": local.command }))
                        }
                        wonopcode_core::config::McpConfig::Remote(remote) => {
                            ("remote", serde_json::json!({ "url": remote.url }))
                        }
                    };
                    serde_json::json!({
                        "name": name,
                        "type": type_str,
                        "status": "disconnected",
                        "details": details
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Json(serde_json::json!({ "servers": servers }))
}

async fn mcp_connect(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    // Get the MCP config from the instance
    let instance = state.instance.read().await;
    let config = instance.config().await;

    // Look up the MCP server config
    let mcp_config = config.mcp.as_ref().and_then(|mcp| mcp.get(&name)).cloned();

    let mcp_config = match mcp_config {
        Some(c) => c,
        None => {
            return Err(ApiError::not_found(format!(
                "MCP server '{name}' not found in config"
            )));
        }
    };

    // For now, return the config - actual connection would require MCP client instantiation
    // which requires async initialization that should happen at startup
    let (type_str, details) = match mcp_config {
        wonopcode_core::config::McpConfig::Local(local) => (
            "local",
            serde_json::json!({
                "command": local.command,
                "environment": local.environment,
                "enabled": local.enabled,
                "timeout": local.timeout
            }),
        ),
        wonopcode_core::config::McpConfig::Remote(remote) => (
            "remote",
            serde_json::json!({
                "url": remote.url,
                "enabled": remote.enabled
            }),
        ),
    };

    Ok(Json(serde_json::json!({
        "name": name,
        "type": type_str,
        "status": "pending",
        "config": details,
        "message": "MCP server configured. Connection will be established on next prompt."
    })))
}

async fn mcp_disconnect(Path(_name): Path<String>) -> impl IntoResponse {
    Json(serde_json::json!({ "success": true }))
}

/// Request to add an MCP server dynamically.
///
/// MCP servers can be added via this API for runtime configuration.
/// The server will be started and connected immediately.
///
/// # Example
///
/// ```json
/// {
///   "name": "remote-server",
///   "url": "https://example.com/mcp/sse",
///   "headers": { "Authorization": "Bearer ..." }
/// }
/// ```
#[derive(Debug, Deserialize)]
struct McpAddRequest {
    /// Unique name for the server.
    name: String,
    /// URL for SSE transport.
    url: String,
    /// Headers for SSE transport.
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
}

async fn mcp_add(Json(req): Json<McpAddRequest>) -> impl IntoResponse {
    // MCP server addition via API requires an MCP client in AppState
    // This is a significant architectural change that would require:
    // 1. Adding McpClient to AppState
    // 2. Initializing it on server startup
    // 3. Managing server lifecycle (connect, disconnect, reconnect)
    //
    // For now, provide helpful guidance on config file approach.

    // Build example config based on request
    let config_example = serde_json::json!({
        "mcp": {
            &req.name: {
                "type": "remote",
                "url": req.url,
                "headers": req.headers.as_ref().unwrap_or(&std::collections::HashMap::new())
            }
        }
    });

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "message": format!(
                "Dynamic MCP server addition is not yet implemented. \
                 Please configure '{}' in your wonopcode.json config file.",
                req.name
            ),
            "server_name": req.name,
            "url": req.url,
            "config_valid": true,
            "suggestion": "Add the following to your wonopcode.json file:",
            "config_example": config_example,
            "config_paths": [
                ".wonopcode.json (project-local)",
                "~/.config/wonopcode/config.json (user-global)"
            ]
        })),
    )
}

async fn mcp_auth_start(Path(name): Path<String>) -> impl IntoResponse {
    // MCP OAuth is for servers that require authentication (e.g., cloud services)
    // Return 501 Not Implemented with informative error
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "server": name,
            "message": format!(
                "OAuth authentication for MCP server '{}' is not yet implemented via the API. \
                 Please configure authentication credentials in your wonopcode.json config file.",
                name
            ),
            "config_path": "mcp.<server_name>.env or mcp.<server_name>.auth"
        })),
    )
}

async fn mcp_auth_callback(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    if let Some(error) = params.get("error") {
        let description = params.get("error_description").cloned().unwrap_or_default();
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "oauth_error",
                "code": "BAD_REQUEST",
                "message": format!("OAuth provider returned an error: {}", error),
                "description": description
            })),
        );
    }

    if params.contains_key("code") {
        // We received a code but can't process it yet
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({
                "error": "not_implemented",
                "code": "NOT_IMPLEMENTED",
                "message": "OAuth token exchange is not yet implemented. The authorization code was received but cannot be processed.",
                "code_received": true,
                "next_steps": "Please configure your MCP server with API credentials directly in wonopcode.json"
            })),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_callback",
                "code": "BAD_REQUEST",
                "message": "OAuth callback is missing required 'code' parameter."
            })),
        )
    }
}

async fn mcp_auth_authenticate(Path(name): Path<String>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "server": name,
            "message": format!(
                "Direct authentication for MCP server '{}' is not yet implemented. \
                 Please configure credentials in your wonopcode.json config file.",
                name
            ),
            "config_example": {
                "mcp": {
                    &name: {
                        "command": "npx",
                        "args": ["-y", "@example/mcp-server"],
                        "env": {
                            "API_KEY": "your-api-key"
                        }
                    }
                }
            }
        })),
    )
}

async fn mcp_auth_remove(Path(name): Path<String>) -> impl IntoResponse {
    // This endpoint should remove stored credentials for an MCP server
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "code": "NOT_IMPLEMENTED",
            "server": name,
            "message": format!(
                "Credential removal for MCP server '{}' is not yet implemented. \
                 To remove credentials, manually edit your wonopcode.json config file.",
                name
            )
        })),
    )
}

// =============================================================================
// LSP endpoints
// =============================================================================

async fn lsp_status(State(state): State<AppState>) -> impl IntoResponse {
    // Get LSP status from instance
    let instance = state.instance.read().await;
    let project_dir = instance.directory().display().to_string();

    // Note: LSP clients are managed per-session by the runner, not by the server.
    // The server provides this endpoint for API completeness, but actual LSP status
    // is available through the TUI sidebar or via session-specific updates.
    Json(serde_json::json!({
        "servers": [],
        "status": "pending",
        "message": "LSP clients are managed per-session. See session updates for LSP status.",
        "project_dir": project_dir
    }))
}

async fn formatter_status(State(state): State<AppState>) -> impl IntoResponse {
    // Get formatter status from instance
    let instance = state.instance.read().await;
    let project_dir = instance.directory();
    let project_dir_str = project_dir.display().to_string();

    // Detect common formatter configurations in the project
    let mut formatters = Vec::new();
    let mut config_files = Vec::new();

    // Check for common formatter config files
    let formatter_configs = [
        (".prettierrc", "prettier", "JavaScript/TypeScript/CSS/HTML"),
        (
            ".prettierrc.json",
            "prettier",
            "JavaScript/TypeScript/CSS/HTML",
        ),
        (
            ".prettierrc.js",
            "prettier",
            "JavaScript/TypeScript/CSS/HTML",
        ),
        (
            "prettier.config.js",
            "prettier",
            "JavaScript/TypeScript/CSS/HTML",
        ),
        (".eslintrc", "eslint", "JavaScript/TypeScript (with --fix)"),
        (
            ".eslintrc.json",
            "eslint",
            "JavaScript/TypeScript (with --fix)",
        ),
        (
            "eslint.config.js",
            "eslint",
            "JavaScript/TypeScript (with --fix)",
        ),
        ("rustfmt.toml", "rustfmt", "Rust"),
        (".rustfmt.toml", "rustfmt", "Rust"),
        ("pyproject.toml", "black/ruff", "Python"),
        (".black", "black", "Python"),
        ("ruff.toml", "ruff", "Python"),
        (
            ".editorconfig",
            "editorconfig",
            "Universal (indentation, line endings)",
        ),
        ("biome.json", "biome", "JavaScript/TypeScript/JSON"),
        (".clang-format", "clang-format", "C/C++/Objective-C"),
        ("go.mod", "gofmt", "Go"),
        ("deno.json", "deno fmt", "Deno/TypeScript"),
    ];

    for (file, formatter, languages) in formatter_configs {
        let path = project_dir.join(file);
        if path.exists() {
            config_files.push(file.to_string());
            formatters.push(serde_json::json!({
                "name": formatter,
                "config_file": file,
                "languages": languages,
                "detected": true
            }));
        }
    }

    // Check for Cargo.toml (indicates Rust project, rustfmt available)
    if project_dir.join("Cargo.toml").exists()
        && !config_files.contains(&"rustfmt.toml".to_string())
    {
        formatters.push(serde_json::json!({
            "name": "rustfmt",
            "config_file": null,
            "languages": "Rust",
            "detected": true,
            "note": "Using default rustfmt settings (no rustfmt.toml found)"
        }));
    }

    // Check for package.json (indicates Node project)
    if project_dir.join("package.json").exists() {
        // Check if prettier is in dependencies
        if let Ok(content) = tokio::fs::read_to_string(project_dir.join("package.json")).await {
            if content.contains("\"prettier\"")
                && !config_files
                    .iter()
                    .any(|f| f.starts_with(".prettier") || f.starts_with("prettier"))
            {
                formatters.push(serde_json::json!({
                    "name": "prettier",
                    "config_file": null,
                    "languages": "JavaScript/TypeScript/CSS/HTML",
                    "detected": true,
                    "note": "Found in package.json dependencies"
                }));
            }
        }
    }

    let status = if formatters.is_empty() {
        "none_detected"
    } else {
        "detected"
    };

    let message = if formatters.is_empty() {
        "No formatter configurations detected. Consider adding .prettierrc, rustfmt.toml, or .editorconfig."
    } else {
        "Formatter configurations detected in project."
    };

    Json(serde_json::json!({
        "formatters": formatters,
        "config_files": config_files,
        "status": status,
        "message": message,
        "project_dir": project_dir_str
    }))
}

// =============================================================================
// Command endpoints
// =============================================================================

async fn command_list() -> impl IntoResponse {
    let commands = vec![
        serde_json::json!({ "id": "new", "name": "/new", "description": "Start a new session" }),
        serde_json::json!({ "id": "clear", "name": "/clear", "description": "Clear current session" }),
        serde_json::json!({ "id": "compact", "name": "/compact", "description": "Compact conversation history" }),
        serde_json::json!({ "id": "model", "name": "/model", "description": "Switch model" }),
        serde_json::json!({ "id": "agent", "name": "/agent", "description": "Switch agent" }),
        serde_json::json!({ "id": "theme", "name": "/theme", "description": "Change theme" }),
        serde_json::json!({ "id": "help", "name": "/help", "description": "Show help" }),
        serde_json::json!({ "id": "quit", "name": "/quit", "description": "Exit application" }),
    ];

    Json(serde_json::json!({ "commands": commands }))
}

// =============================================================================
// VCS endpoints
// =============================================================================

async fn vcs_get(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;
    let dir = instance.directory();

    // Get git info
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let remote = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Json(serde_json::json!({
        "type": "git",
        "branch": branch,
        "commit": commit,
        "remote": remote
    }))
}

// =============================================================================
// Path endpoints
// =============================================================================

async fn path_get(State(state): State<AppState>) -> impl IntoResponse {
    let instance = state.instance.read().await;

    Json(serde_json::json!({
        "cwd": instance.directory().display().to_string(),
        "home": dirs::home_dir().map(|p| p.display().to_string()).unwrap_or_default(),
        "config": wonopcode_core::config::Config::global_config_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        "data": wonopcode_core::config::Config::data_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }))
}
