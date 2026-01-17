//! MCP protocol types.
//!
//! Implements the JSON-RPC based MCP protocol.
//! See: <https://spec.modelcontextprotocol.io/>

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP protocol version.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC request (or notification if id is None).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// Request ID. None for notifications (which don't expect a response).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request.
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    /// Check if this is a notification (no response expected).
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC notification (no id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    /// Create a new JSON-RPC notification.
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

/// MCP initialization parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: ClientInfo::default(),
        }
    }
}

/// Client capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapability>,
}

/// Roots capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsCapability {
    pub list_changed: bool,
}

/// Sampling capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingCapability {}

/// Client info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self {
            name: "wonopcode".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// MCP initialization result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

/// Server capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
}

/// Tools capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// Resources capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesCapability {
    #[serde(default)]
    pub subscribe: bool,
    #[serde(default)]
    pub list_changed: bool,
}

/// Prompts capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// Server info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// MCP tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    /// Tool name.
    pub name: String,
    /// Tool description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the tool's input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

/// List tools result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    pub tools: Vec<McpTool>,
}

/// Tool call parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// Tool call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    /// Content returned by the tool.
    pub content: Vec<ToolContent>,
    /// Whether the tool call resulted in an error.
    #[serde(default)]
    pub is_error: bool,
}

/// Tool content item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContent },
}

/// Resource content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

// ============================================================================
// Permission Protocol Extension (wonopcode-specific)
// ============================================================================

/// Permission request parameters.
///
/// Sent from server to client when a tool execution requires user approval.
/// The client should present this to the user and respond with `PermissionResponseParams`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestParams {
    /// Unique identifier for this permission request.
    pub request_id: String,
    /// Name of the tool requesting permission.
    pub tool: String,
    /// Action being performed (e.g., "execute", "read", "write").
    pub action: String,
    /// Human-readable description of what the tool wants to do.
    pub description: String,
    /// File path involved (for file operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Additional details about the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Permission response parameters.
///
/// Sent from client to server in response to a permission request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponseParams {
    /// The request_id from the corresponding PermissionRequestParams.
    pub request_id: String,
    /// Whether the permission is granted.
    pub allow: bool,
    /// If true, remember this decision for future requests to the same tool.
    #[serde(default)]
    pub remember: bool,
}

/// Permission response result.
///
/// Returned by the server after processing a permission response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponseResult {
    /// Whether the response was processed successfully.
    pub success: bool,
    /// Optional message (e.g., error details).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Method name for permission request notifications.
pub const METHOD_PERMISSION_REQUEST: &str = "wonopcode/permissionRequest";

/// Method name for permission response requests.
pub const METHOD_PERMISSION_RESPONSE: &str = "wonopcode/permissionResponse";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_serialization() {
        let req = JsonRpcRequest::new(1, "initialize", Some(serde_json::json!({"test": true})));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_initialize_params() {
        let params = InitializeParams::default();
        assert_eq!(params.protocol_version, PROTOCOL_VERSION);
        assert_eq!(params.client_info.name, "wonopcode");
    }

    #[test]
    fn test_tool_content_deserialization() {
        let json = r#"{"type": "text", "text": "Hello"}"#;
        let content: ToolContent = serde_json::from_str(json).unwrap();
        match content {
            ToolContent::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Text content"),
        }
    }

    #[test]
    fn test_json_rpc_request_is_notification() {
        let req = JsonRpcRequest::new(1, "test", None);
        assert!(!req.is_notification());

        let notification = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notify".to_string(),
            params: None,
        };
        assert!(notification.is_notification());
    }

    #[test]
    fn test_json_rpc_notification() {
        let notif =
            JsonRpcNotification::new("notify/update", Some(serde_json::json!({"data": "test"})));
        assert_eq!(notif.jsonrpc, "2.0");
        assert_eq!(notif.method, "notify/update");
        assert!(notif.params.is_some());
    }

    #[test]
    fn test_json_rpc_response_serialization() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: 1,
            result: Some(serde_json::json!({"success": true})),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn test_json_rpc_error_response() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: 1,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"code\":-32600"));
        assert!(json.contains("Invalid Request"));
    }

    #[test]
    fn test_client_info_default() {
        let info = ClientInfo::default();
        assert_eq!(info.name, "wonopcode");
        assert!(!info.version.is_empty());
    }

    #[test]
    fn test_server_capabilities_default() {
        let caps = ServerCapabilities::default();
        assert!(caps.tools.is_none());
        assert!(caps.resources.is_none());
        assert!(caps.prompts.is_none());
    }

    #[test]
    fn test_client_capabilities_default() {
        let caps = ClientCapabilities::default();
        assert!(caps.roots.is_none());
        assert!(caps.sampling.is_none());
    }

    #[test]
    fn test_mcp_tool_serialization() {
        let tool = McpTool {
            name: "read".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            })),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"name\":\"read\""));
    }

    #[test]
    fn test_list_tools_result() {
        let result = ListToolsResult {
            tools: vec![McpTool {
                name: "tool1".to_string(),
                description: None,
                input_schema: None,
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"tools\""));
    }

    #[test]
    fn test_call_tool_params() {
        let params = CallToolParams {
            name: "bash".to_string(),
            arguments: Some(serde_json::json!({"command": "ls"})),
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"name\":\"bash\""));
    }

    #[test]
    fn test_tool_call_result() {
        let result = ToolCallResult {
            content: vec![ToolContent::Text {
                text: "output".to_string(),
            }],
            is_error: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"isError\":false"));
    }

    #[test]
    fn test_tool_content_image() {
        let content = ToolContent::Image {
            data: "base64data".to_string(),
            mime_type: "image/png".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        assert!(json.contains("\"mimeType\":\"image/png\""));
    }

    #[test]
    fn test_tool_content_resource() {
        let content = ToolContent::Resource {
            resource: ResourceContent {
                uri: "file:///test.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
                text: Some("content".to_string()),
                blob: None,
            },
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"resource\""));
        assert!(json.contains("file:///test.txt"));
    }

    #[test]
    fn test_permission_request_params() {
        let params = PermissionRequestParams {
            request_id: "req_123".to_string(),
            tool: "bash".to_string(),
            action: "execute".to_string(),
            description: "Run a shell command".to_string(),
            path: Some("/tmp".to_string()),
            details: Some(serde_json::json!({"command": "ls"})),
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"requestId\":\"req_123\""));
        assert!(json.contains("\"tool\":\"bash\""));
    }

    #[test]
    fn test_permission_response_params() {
        let params = PermissionResponseParams {
            request_id: "req_123".to_string(),
            allow: true,
            remember: true,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"requestId\":\"req_123\""));
        assert!(json.contains("\"allow\":true"));
        assert!(json.contains("\"remember\":true"));
    }

    #[test]
    fn test_permission_response_result() {
        let result = PermissionResponseResult {
            success: true,
            message: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn test_protocol_constants() {
        assert_eq!(PROTOCOL_VERSION, "2024-11-05");
        assert_eq!(METHOD_PERMISSION_REQUEST, "wonopcode/permissionRequest");
        assert_eq!(METHOD_PERMISSION_RESPONSE, "wonopcode/permissionResponse");
    }

    #[test]
    fn test_initialize_result() {
        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: true }),
                resources: None,
                prompts: None,
            },
            server_info: ServerInfo {
                name: "test-server".to_string(),
                version: Some("1.0.0".to_string()),
            },
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"protocolVersion\""));
        assert!(json.contains("\"listChanged\":true"));
    }
}
