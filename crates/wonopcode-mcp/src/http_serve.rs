//! HTTP/SSE transport for MCP server.
//!
//! This module implements the MCP protocol over HTTP using Server-Sent Events (SSE),
//! allowing clients (like Claude CLI) to connect to the MCP server.
//!
//! # Protocol
//!
//! ```text
//! Client                             Server
//!   │                                  │
//!   │── GET /mcp/sse ─────────────────►│ (establish SSE connection)
//!   │◄── SSE: endpoint event ──────────│ (server sends message URL)
//!   │                                  │
//!   │── POST /mcp/message?sessionId=x─►│ (JSON-RPC requests)
//!   │◄── SSE: message event ───────────│ (responses via SSE)
//!   │                                  │
//! ```
//!
//! # Endpoints
//!
//! - `GET /mcp/sse` - Establish SSE connection, receive server events
//! - `POST /mcp/message` - Send JSON-RPC requests to server

use crate::protocol::{
    CallToolParams, InitializeResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    ListToolsResult, McpTool, ServerCapabilities, ServerInfo, ToolCallResult, ToolContent,
    ToolsCapability, PROTOCOL_VERSION,
};
use crate::serve::{McpServerTool, McpToolContext};
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
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::sync::{mpsc, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};

/// State for MCP HTTP server.
#[derive(Clone)]
pub struct McpHttpState {
    /// Server name.
    pub name: String,
    /// Server version.
    pub version: String,
    /// Registered tools.
    pub tools: Arc<HashMap<String, McpServerTool>>,
    /// Active sessions (session_id -> session).
    sessions: Arc<RwLock<HashMap<String, McpSession>>>,
    /// Tool execution context.
    pub context: McpToolContext,
    /// Base URL for message endpoint (used in endpoint event).
    pub message_url: String,
    /// Optional API key for authentication.
    /// If set, clients must provide this key in the `X-API-Key` header
    /// or `Authorization: Bearer <key>` header.
    api_key: Option<String>,
}

/// An active MCP session.
struct McpSession {
    /// Channel to send responses to SSE stream.
    response_tx: mpsc::UnboundedSender<JsonRpcResponse>,
    /// Session creation time (for cleanup).
    #[allow(dead_code)]
    created_at: Instant,
}

impl McpHttpState {
    /// Create a new MCP HTTP state.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        tools: HashMap<String, McpServerTool>,
        context: McpToolContext,
        message_url: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            tools: Arc::new(tools),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            context,
            message_url: message_url.into(),
            api_key: None,
        }
    }

    /// Set the API key for authentication.
    ///
    /// When set, all requests must include a valid API key in either:
    /// - `X-API-Key` header
    /// - `Authorization: Bearer <key>` header
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Check if authentication is enabled.
    pub fn has_auth(&self) -> bool {
        self.api_key.is_some()
    }

    /// Register a new session.
    async fn register_session(
        &self,
        session_id: String,
        tx: mpsc::UnboundedSender<JsonRpcResponse>,
    ) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            session_id.clone(),
            McpSession {
                response_tx: tx,
                created_at: Instant::now(),
            },
        );
        info!(session_id = %session_id, "MCP session registered");
    }

    /// Unregister a session.
    async fn unregister_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(session_id).is_some() {
            info!(session_id = %session_id, "MCP session unregistered");
        }
    }

    /// Send a response to a specific session.
    async fn send_response(&self, session_id: &str, response: JsonRpcResponse) -> Result<(), ()> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            session.response_tx.send(response).map_err(|_| ())
        } else {
            Err(())
        }
    }

    /// Handle a JSON-RPC request.
    async fn handle_request(&self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        debug!(method = %request.method, id = ?request.id, "Handling MCP request");

        // Notifications (no id) don't expect a response
        let id = match request.id {
            Some(id) => id,
            None => {
                // This is a notification - handle it but don't respond
                match request.method.as_str() {
                    "notifications/initialized" => {
                        debug!("Received initialized notification");
                    }
                    _ => {
                        debug!(method = %request.method, "Received unknown notification");
                    }
                }
                return None;
            }
        };

        match request.method.as_str() {
            "initialize" => Some(self.handle_initialize(id)),
            "tools/list" => Some(self.handle_list_tools(id)),
            "tools/call" => Some(self.handle_call_tool(id, request.params).await),
            _ => Some(self.error_response(id, -32601, "Method not found")),
        }
    }

    /// Handle the initialize request.
    fn handle_initialize(&self, id: u64) -> JsonRpcResponse {
        info!(name = %self.name, version = %self.version, "Initializing MCP HTTP server");

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: false,
                }),
                resources: None,
                prompts: None,
            },
            server_info: ServerInfo {
                name: self.name.clone(),
                version: Some(self.version.clone()),
            },
        };

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: serde_json::to_value(result).ok(),
            error: None,
        }
    }

    /// Handle the tools/list request.
    fn handle_list_tools(&self, id: u64) -> JsonRpcResponse {
        debug!(count = self.tools.len(), "Listing MCP tools");

        let tools: Vec<McpTool> = self
            .tools
            .values()
            .map(|tool| McpTool {
                name: tool.name.clone(),
                description: Some(tool.description.clone()),
                input_schema: Some(tool.parameters.clone()),
            })
            .collect();

        let result = ListToolsResult { tools };

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: serde_json::to_value(result).ok(),
            error: None,
        }
    }

    /// Handle the tools/call request.
    async fn handle_call_tool(&self, id: u64, params: Option<Value>) -> JsonRpcResponse {
        // Parse parameters
        let params: CallToolParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(params) => params,
                Err(e) => {
                    return self.error_response(id, -32602, &format!("Invalid params: {e}"));
                }
            },
            None => {
                return self.error_response(id, -32602, "Missing params");
            }
        };

        debug!(tool = %params.name, "Calling MCP tool");

        // Find tool
        let tool = match self.tools.get(&params.name) {
            Some(t) => t,
            None => {
                return self.error_response(id, -32602, &format!("Unknown tool: {}", params.name));
            }
        };

        // Execute tool
        let args = params
            .arguments
            .unwrap_or(Value::Object(serde_json::Map::new()));
        let result = tool.executor.execute(args, &self.context).await;

        let tool_result = match result {
            Ok(output) => {
                debug!(tool = %params.name, output_len = output.len(), "Tool completed successfully");
                ToolCallResult {
                    content: vec![ToolContent::Text { text: output }],
                    is_error: false,
                }
            }
            Err(e) => {
                warn!(tool = %params.name, error = %e, "Tool failed");
                ToolCallResult {
                    content: vec![ToolContent::Text { text: e }],
                    is_error: true,
                }
            }
        };

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: serde_json::to_value(tool_result).ok(),
            error: None,
        }
    }

    /// Create an error response.
    fn error_response(&self, id: u64, code: i64, message: &str) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }
}

// ============================================================================
// Authentication
// ============================================================================

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
    State(state): State<McpHttpState>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // If no API key is configured, allow all requests
    let Some(ref expected_key) = state.api_key else {
        return Ok(next.run(request).await);
    };

    // Extract and validate API key
    let provided_key = extract_api_key(request.headers());

    match provided_key {
        Some(key) if constant_time_eq(key.as_bytes(), expected_key.as_bytes()) => {
            Ok(next.run(request).await)
        }
        Some(_) => {
            warn!("Invalid API key provided for MCP endpoint");
            Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Invalid API key" })),
            ))
        }
        None => {
            warn!("Missing API key for MCP endpoint");
            Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Authentication required" })),
            ))
        }
    }
}

// ============================================================================
// Router
// ============================================================================

/// Create the MCP HTTP router.
///
/// If the state has an API key configured, all requests will require authentication
/// via `X-API-Key` header or `Authorization: Bearer <key>` header.
pub fn create_mcp_router(state: McpHttpState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let has_auth = state.has_auth();

    let router = Router::new()
        .route("/sse", get(mcp_sse))
        .route("/message", post(mcp_message));

    // Only add auth middleware if API key is configured
    let router = if has_auth {
        info!("MCP API key authentication enabled");
        router.layer(axum::middleware::from_fn_with_state(
            state.clone(),
            api_key_auth,
        ))
    } else {
        router
    };

    router.layer(cors).with_state(state)
}

/// SSE connection handler.
async fn mcp_sse(
    State(state): State<McpHttpState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Generate unique session ID
    let session_id = uuid::Uuid::new_v4().to_string();

    // Create channel for responses
    let (tx, mut rx) = mpsc::unbounded_channel::<JsonRpcResponse>();

    // Register session
    state.register_session(session_id.clone(), tx).await;

    // Build message URL with session ID
    let message_url = format!("{}?sessionId={}", state.message_url, session_id);

    info!(session_id = %session_id, message_url = %message_url, "MCP SSE connection established");

    let state_clone = state.clone();
    let session_id_clone = session_id.clone();

    let stream = async_stream::stream! {
        // Send endpoint event first - just the URL string, not JSON
        // The old HTTP+SSE transport (2024-11-05) expects just the URL as the data
        yield Ok(Event::default().event("endpoint").data(message_url));

        // Then stream responses
        loop {
            tokio::select! {
                Some(response) = rx.recv() => {
                    if let Ok(data) = serde_json::to_string(&response) {
                        yield Ok(Event::default().event("message").data(data));
                    }
                }
                else => {
                    // Channel closed
                    break;
                }
            }
        }

        // Cleanup on disconnect
        state_clone.unregister_session(&session_id_clone).await;
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// Query parameters for message endpoint.
#[derive(Deserialize)]
struct MessageQuery {
    #[serde(rename = "sessionId")]
    session_id: String,
}

/// Message endpoint handler.
async fn mcp_message(
    State(state): State<McpHttpState>,
    Query(query): Query<MessageQuery>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    debug!(
        session_id = %query.session_id,
        method = %request.method,
        "Received MCP message"
    );

    // Handle the request
    if let Some(response) = state.handle_request(request).await {
        // Send response via SSE
        if state
            .send_response(&query.session_id, response.clone())
            .await
            .is_err()
        {
            warn!(session_id = %query.session_id, "Failed to send response - session not found");
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Session not found" })),
            );
        }
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": "ok" })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::ClosureExecutor;

    fn create_test_state() -> McpHttpState {
        let mut tools = HashMap::new();
        tools.insert(
            "echo".to_string(),
            McpServerTool {
                name: "echo".to_string(),
                description: "Echo the input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    }
                }),
                executor: Arc::new(ClosureExecutor::new(|args, _ctx| {
                    let msg = args
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("no message");
                    Ok(msg.to_string())
                })),
            },
        );

        McpHttpState::new(
            "test-server",
            "1.0.0",
            tools,
            McpToolContext::default(),
            "/mcp/message",
        )
    }

    #[test]
    fn test_state_creation() {
        let state = create_test_state();
        assert_eq!(state.name, "test-server");
        assert_eq!(state.version, "1.0.0");
        assert_eq!(state.tools.len(), 1);
    }

    #[test]
    fn test_initialize_response() {
        let state = create_test_state();
        let response = state.handle_initialize(1);

        assert_eq!(response.id, 1);
        assert!(response.error.is_none());

        let result: InitializeResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.server_info.name, "test-server");
    }

    #[test]
    fn test_list_tools_response() {
        let state = create_test_state();
        let response = state.handle_list_tools(2);

        assert_eq!(response.id, 2);
        assert!(response.error.is_none());

        let result: ListToolsResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "echo");
    }

    #[tokio::test]
    async fn test_call_tool() {
        let state = create_test_state();
        let params = serde_json::json!({
            "name": "echo",
            "arguments": { "message": "hello" }
        });

        let response = state.handle_call_tool(3, Some(params)).await;
        assert_eq!(response.id, 3);
        assert!(response.error.is_none());

        let result: ToolCallResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(!result.is_error);
        match &result.content[0] {
            ToolContent::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("Expected text content"),
        }
    }

    #[tokio::test]
    async fn test_unknown_tool() {
        let state = create_test_state();
        let params = serde_json::json!({
            "name": "unknown_tool",
            "arguments": {}
        });

        let response = state.handle_call_tool(4, Some(params)).await;
        assert_eq!(response.id, 4);
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32602);
    }

    // ========================================================================
    // Authentication Tests
    // ========================================================================

    #[test]
    fn test_state_has_auth() {
        let state = create_test_state();
        assert!(!state.has_auth());

        let state_with_key = state.with_api_key("test-secret");
        assert!(state_with_key.has_auth());
    }

    #[test]
    fn test_extract_api_key_from_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "test-key".parse().unwrap());
        assert_eq!(extract_api_key(&headers), Some("test-key"));
    }

    #[test]
    fn test_extract_api_key_from_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer my-token".parse().unwrap());
        assert_eq!(extract_api_key(&headers), Some("my-token"));
    }

    #[test]
    fn test_extract_api_key_bearer_with_whitespace() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer   my-token  ".parse().unwrap());
        assert_eq!(extract_api_key(&headers), Some("my-token"));
    }

    #[test]
    fn test_extract_api_key_missing() {
        let headers = HeaderMap::new();
        assert_eq!(extract_api_key(&headers), None);
    }

    #[test]
    fn test_extract_api_key_invalid_auth_format() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic abc123".parse().unwrap());
        assert_eq!(extract_api_key(&headers), None);
    }

    #[test]
    fn test_extract_api_key_x_api_key_takes_precedence() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "key-from-header".parse().unwrap());
        headers.insert("authorization", "Bearer key-from-bearer".parse().unwrap());
        // X-API-Key should be checked first
        assert_eq!(extract_api_key(&headers), Some("key-from-header"));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"test", b"test"));
        assert!(!constant_time_eq(b"test", b"wrong"));
        assert!(!constant_time_eq(b"test", b"test-longer"));
        assert!(!constant_time_eq(b"test-longer", b"test"));
        assert!(constant_time_eq(b"", b""));
    }
}
