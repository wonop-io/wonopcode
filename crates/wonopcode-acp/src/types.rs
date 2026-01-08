//! ACP (Agent Client Protocol) type definitions.
//!
//! This module defines all the types used in the ACP protocol for IDE integration.
//! The protocol enables communication between IDEs (Zed, VS Code, Cursor) and
//! the wonopcode agent over stdio using newline-delimited JSON (ndjson).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// JSON-RPC Types
// ============================================================================

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<JsonRpcId>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 notification (no id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC request/response ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
}

/// JSON-RPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }

    pub fn auth_required() -> Self {
        Self {
            code: -32001,
            message: "Authentication required".to_string(),
            data: None,
        }
    }
}

// ============================================================================
// Initialize Request/Response
// ============================================================================

/// Initialize request from client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<ClientCapabilities>,
}

/// Client capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

/// Initialize response from agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: u32,
    pub agent_capabilities: AgentCapabilities,
    pub auth_methods: Vec<AuthMethod>,
    pub agent_info: AgentInfo,
}

/// Agent capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub load_session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_capabilities: Option<McpCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_capabilities: Option<PromptCapabilities>,
}

/// MCP capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCapabilities {
    #[serde(default)]
    pub http: bool,
    #[serde(default)]
    pub sse: bool,
}

/// Prompt capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    #[serde(default)]
    pub embedded_context: bool,
    #[serde(default)]
    pub image: bool,
}

/// Authentication method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

/// Agent information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub version: String,
}

// ============================================================================
// Session Management
// ============================================================================

/// MCP server configuration (local).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerLocal {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<EnvVar>,
}

/// MCP server configuration (remote).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerRemote {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(rename = "type")]
    pub server_type: String,
}

/// Environment variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// HTTP header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// MCP server (either local or remote).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServer {
    Local(McpServerLocal),
    Remote(McpServerRemote),
}

/// New session request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionRequest {
    pub cwd: String,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

/// Load session request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionRequest {
    pub session_id: String,
    pub cwd: String,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

/// Session response (for new/load).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub session_id: String,
    pub models: ModelsInfo,
    pub modes: ModesInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

/// Available models info.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsInfo {
    pub current_model_id: String,
    pub available_models: Vec<ModelInfo>,
}

/// Model information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub model_id: String,
    pub name: String,
}

/// Available modes info.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModesInfo {
    pub current_mode_id: String,
    pub available_modes: Vec<ModeInfo>,
}

/// Mode (agent) information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ============================================================================
// Session Model/Mode
// ============================================================================

/// Set session model request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModelRequest {
    pub session_id: String,
    pub model_id: String,
}

/// Set session mode request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModeRequest {
    pub session_id: String,
    pub mode_id: String,
}

// ============================================================================
// Prompt
// ============================================================================

/// Prompt request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    pub session_id: String,
    pub prompt: Vec<PromptPart>,
}

/// Prompt part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptPart {
    Text {
        text: String,
    },
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        uri: Option<String>,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    ResourceLink {
        uri: String,
    },
    Resource {
        resource: ResourceContent,
    },
}

/// Resource content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContent {
    Text { text: String },
    Binary { data: String },
}

/// Prompt response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    pub stop_reason: StopReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

/// Stop reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    Error,
    Cancelled,
}

// ============================================================================
// Cancel
// ============================================================================

/// Cancel notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelNotification {
    pub session_id: String,
}

// ============================================================================
// Session Updates (notifications from agent to client)
// ============================================================================

/// Session update notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateNotification {
    pub session_id: String,
    pub update: SessionUpdate,
}

/// Session update types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum SessionUpdate {
    /// Agent text message chunk (streaming).
    AgentMessageChunk { content: TextContent },

    /// User message chunk (for replay).
    UserMessageChunk { content: TextContent },

    /// Agent thinking/reasoning chunk.
    AgentThoughtChunk { content: TextContent },

    /// Tool call started.
    #[serde(rename_all = "camelCase")]
    ToolCall {
        tool_call_id: String,
        title: String,
        kind: ToolKind,
        status: ToolStatus,
        locations: Vec<Location>,
        raw_input: serde_json::Value,
    },

    /// Tool call progress/completion.
    #[serde(rename_all = "camelCase")]
    ToolCallUpdate {
        tool_call_id: String,
        status: ToolStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<ToolKind>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        locations: Option<Vec<Location>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw_input: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw_output: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Vec<ToolCallContent>>,
    },

    /// Plan/todo update.
    Plan { entries: Vec<PlanEntry> },

    /// Available commands update.
    #[serde(rename_all = "camelCase")]
    AvailableCommandsUpdate {
        available_commands: Vec<CommandInfo>,
    },
}

/// Text content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl TextContent {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            content_type: "text".to_string(),
            text: text.into(),
        }
    }
}

/// Tool kind (category).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Execute,
    Fetch,
    Edit,
    Search,
    Read,
    Other,
}

impl ToolKind {
    /// Map tool name to kind.
    pub fn from_tool_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "bash" => ToolKind::Execute,
            "webfetch" => ToolKind::Fetch,
            "edit" | "patch" | "write" | "multiedit" => ToolKind::Edit,
            "grep" | "glob" => ToolKind::Search,
            "list" | "read" => ToolKind::Read,
            _ => ToolKind::Other,
        }
    }
}

/// Tool call status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// File/path location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
}

impl Location {
    /// Extract locations from tool input.
    pub fn from_tool_input(tool_name: &str, input: &serde_json::Value) -> Vec<Self> {
        match tool_name.to_lowercase().as_str() {
            "read" | "edit" | "write" => {
                if let Some(path) = input.get("filePath").and_then(|v| v.as_str()) {
                    vec![Location {
                        path: path.to_string(),
                    }]
                } else {
                    vec![]
                }
            }
            "glob" | "grep" | "list" => {
                if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                    vec![Location {
                        path: path.to_string(),
                    }]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }
}

/// Tool call content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolCallContent {
    Content {
        content: TextContent,
    },
    Diff {
        path: String,
        #[serde(rename = "oldText")]
        old_text: String,
        #[serde(rename = "newText")]
        new_text: String,
    },
}

/// Plan entry (todo item).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntry {
    pub content: String,
    pub status: PlanStatus,
    pub priority: String,
}

/// Plan entry status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

/// Command information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
}

// ============================================================================
// Permission
// ============================================================================

/// Permission request (from agent to client).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub session_id: String,
    pub tool_call: ToolCallInfo,
    pub options: Vec<PermissionOption>,
}

/// Tool call info for permission.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallInfo {
    pub tool_call_id: String,
    pub status: ToolStatus,
    pub title: String,
    pub kind: ToolKind,
    pub locations: Vec<Location>,
    pub raw_input: serde_json::Value,
}

/// Permission option.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub kind: PermissionKind,
    pub name: String,
}

/// Permission option kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
}

/// Permission response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub outcome: PermissionOutcome,
}

/// Permission outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOutcome {
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
}

// ============================================================================
// Authenticate
// ============================================================================

/// Authenticate request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateRequest {
    pub method_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

// ============================================================================
// ACP Session State
// ============================================================================

/// Internal ACP session state.
#[derive(Debug, Clone)]
pub struct AcpSessionState {
    pub id: String,
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub model: Option<ModelRef>,
    pub mode_id: Option<String>,
}

/// Model reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

impl ModelRef {
    /// Parse "provider/model" format.
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(2, '/').collect();
        if parts.len() == 2 {
            Some(Self {
                provider_id: parts[0].to_string(),
                model_id: parts[1].to_string(),
            })
        } else {
            None
        }
    }

    /// Format as "provider/model".
    pub fn as_string(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_ref_parse() {
        let model = ModelRef::parse("anthropic/claude-3-5-sonnet").unwrap();
        assert_eq!(model.provider_id, "anthropic");
        assert_eq!(model.model_id, "claude-3-5-sonnet");
    }

    #[test]
    fn test_tool_kind_from_name() {
        assert_eq!(ToolKind::from_tool_name("bash"), ToolKind::Execute);
        assert_eq!(ToolKind::from_tool_name("edit"), ToolKind::Edit);
        assert_eq!(ToolKind::from_tool_name("grep"), ToolKind::Search);
        assert_eq!(ToolKind::from_tool_name("read"), ToolKind::Read);
        assert_eq!(ToolKind::from_tool_name("unknown"), ToolKind::Other);
    }

    #[test]
    fn test_session_update_serialization() {
        let update = SessionUpdate::AgentMessageChunk {
            content: TextContent::new("Hello, world!"),
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("agent_message_chunk"));
        assert!(json.contains("Hello, world!"));
    }
}
