//! MCP server for exposing wonopcode tools.
//!
//! This module implements an MCP server that exposes wonopcode's tool registry
//! to MCP clients (such as Claude CLI). This enables Claude CLI to use our
//! custom tools instead of its built-in ones.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     JSON-RPC/stdio     ┌──────────────────┐
//! │   Claude CLI    │ ◄────────────────────► │   McpServer      │
//! │   (MCP client)  │                        │ (wonopcode tools)│
//! └─────────────────┘                        └──────────────────┘
//!                                                      │
//!                                                      ▼
//!                                            ┌──────────────────┐
//!                                            │   ToolRegistry   │
//!                                            │ (bash, read, etc)│
//!                                            └──────────────────┘
//! ```
//!
//! # Protocol
//!
//! The server implements the MCP protocol over stdio:
//! - `initialize`: Returns server capabilities
//! - `notifications/initialized`: Acknowledgment (no response)
//! - `tools/list`: Returns available tools
//! - `tools/call`: Executes a tool and returns the result

use crate::error::{McpError, McpResult};
use crate::protocol::{
    CallToolParams, InitializeResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    ListToolsResult, McpTool, ServerCapabilities, ServerInfo, ToolCallResult, ToolContent,
    ToolsCapability, PROTOCOL_VERSION,
};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::{debug, error, info, warn};

/// Tool definition for the MCP server.
/// This is a simplified interface that doesn't require the full wonopcode-tools crate.
#[derive(Clone)]
pub struct McpServerTool {
    /// Tool name/ID.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON Schema for parameters.
    pub parameters: Value,
    /// Tool executor function.
    pub executor: Arc<dyn McpToolExecutor>,
}

impl std::fmt::Debug for McpServerTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

/// Trait for tool execution.
#[async_trait::async_trait]
pub trait McpToolExecutor: Send + Sync {
    /// Execute the tool with given arguments.
    async fn execute(&self, args: Value, ctx: &McpToolContext) -> Result<String, String>;
}

/// Context provided to tools during execution.
#[derive(Debug, Clone)]
pub struct McpToolContext {
    /// Session ID.
    pub session_id: String,
    /// Working directory.
    pub cwd: PathBuf,
    /// Root directory (project root).
    pub root_dir: PathBuf,
}

impl Default for McpToolContext {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        Self {
            session_id: "mcp-default".to_string(),
            cwd: cwd.clone(),
            root_dir: cwd,
        }
    }
}

/// MCP server that exposes tools via the MCP protocol.
pub struct McpServer {
    /// Server name.
    name: String,
    /// Server version.
    version: String,
    /// Registered tools.
    tools: HashMap<String, McpServerTool>,
    /// Tool execution context.
    context: McpToolContext,
}

impl McpServer {
    /// Create a new MCP server.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            tools: HashMap::new(),
            context: McpToolContext::default(),
        }
    }

    /// Set the tool execution context.
    pub fn with_context(mut self, context: McpToolContext) -> Self {
        self.context = context;
        self
    }

    /// Register a tool.
    pub fn register_tool(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        executor: impl McpToolExecutor + 'static,
    ) {
        let name = name.into();
        self.tools.insert(
            name.clone(),
            McpServerTool {
                name,
                description: description.into(),
                parameters,
                executor: Arc::new(executor),
            },
        );
    }

    /// Register a tool from a McpServerTool struct.
    pub fn register(&mut self, tool: McpServerTool) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Get the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Run the MCP server on stdio (blocking, synchronous version).
    ///
    /// This is useful for running as a child process where async might
    /// interfere with stdio buffering.
    pub fn serve_stdio_sync(&mut self) -> McpResult<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();

        info!(name = %self.name, tools = self.tools.len(), "MCP server started (sync)");

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    error!(error = %e, "Error reading stdin");
                    break;
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            debug!(line_len = trimmed.len(), "Received line from Claude CLI");

            // Parse request
            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    // Log the actual content for debugging
                    warn!(error = %e, line_preview = %trimmed.chars().take(100).collect::<String>(), "Invalid JSON-RPC request");
                    continue;
                }
            };

            debug!(method = %request.method, id = ?request.id, "Parsed request");

            // Handle request
            let response = self.handle_request_sync(request);

            // Send response (skip for notifications)
            if let Some(response) = response {
                let response_json = serde_json::to_string(&response).map_err(|e| {
                    McpError::protocol_error(format!("Failed to serialize response: {}", e))
                })?;

                writeln!(stdout, "{}", response_json).map_err(|e| {
                    McpError::ProcessError(format!("Failed to write response: {}", e))
                })?;
                stdout.flush().map_err(|e| {
                    McpError::ProcessError(format!("Failed to flush stdout: {}", e))
                })?;
            }
        }

        info!("MCP server shutting down");
        Ok(())
    }

    /// Run the MCP server on stdio (async version).
    pub async fn serve_stdio(&mut self) -> McpResult<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let mut reader = BufReader::new(stdin);
        let mut writer = BufWriter::new(stdout);

        info!(name = %self.name, tools = self.tools.len(), "MCP server started");

        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                // EOF - client disconnected
                info!("Client disconnected");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            debug!(line = trimmed, "Received request");

            // Parse request
            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Invalid JSON-RPC request");
                    continue;
                }
            };

            // Handle request
            let response = self.handle_request(request).await;

            // Send response (skip for notifications)
            if let Some(response) = response {
                let response_json = serde_json::to_string(&response).map_err(|e| {
                    McpError::protocol_error(format!("Failed to serialize response: {}", e))
                })?;

                debug!(response = %response_json, "Sending response");

                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
        }

        info!("MCP server shutting down");
        Ok(())
    }

    /// Handle a JSON-RPC request synchronously.
    fn handle_request_sync(&mut self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        debug!(method = %request.method, id = ?request.id, "Handling request (sync)");

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
            "tools/call" => {
                // For sync mode, we need to block on the async executor.
                // Create a new runtime for each tool call to avoid deadlocks.
                // This is safe because serve_stdio_sync is designed to be called
                // from a non-async context, even though run_mcp_server is async.
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok()?;
                Some(runtime.block_on(self.handle_call_tool(id, request.params)))
            }
            _ => Some(self.error_response(id, -32601, "Method not found")),
        }
    }

    /// Handle a JSON-RPC request asynchronously.
    async fn handle_request(&mut self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        debug!(method = %request.method, id = ?request.id, "Handling request");

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
        info!(name = %self.name, version = %self.version, "Initializing MCP server");

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
        debug!(count = self.tools.len(), "Listing tools");

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
        Self::call_tool_static(&self.tools, &self.context, id, params).await
    }

    /// Static version of handle_call_tool that doesn't require &self.
    /// This is needed for the sync mode to avoid borrowing issues when using thread::scope.
    async fn call_tool_static(
        tools: &HashMap<String, McpServerTool>,
        context: &McpToolContext,
        id: u64,
        params: Option<Value>,
    ) -> JsonRpcResponse {
        // Parse parameters
        let params: CallToolParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(params) => params,
                Err(e) => {
                    return Self::error_response_static(
                        id,
                        -32602,
                        &format!("Invalid params: {}", e),
                    );
                }
            },
            None => {
                return Self::error_response_static(id, -32602, "Missing params");
            }
        };

        debug!(tool = %params.name, "Calling tool");

        // Find tool
        let tool = match tools.get(&params.name) {
            Some(t) => t,
            None => {
                return Self::error_response_static(
                    id,
                    -32602,
                    &format!("Unknown tool: {}", params.name),
                );
            }
        };

        // Execute tool
        let args = params
            .arguments
            .unwrap_or(Value::Object(serde_json::Map::new()));
        let result = tool.executor.execute(args, context).await;

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
        Self::error_response_static(id, code, message)
    }

    /// Static version of error_response that doesn't require &self.
    fn error_response_static(id: u64, code: i64, message: &str) -> JsonRpcResponse {
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

/// Builder for McpServerTool.
pub struct McpServerToolBuilder {
    name: String,
    description: String,
    parameters: Value,
}

impl McpServerToolBuilder {
    /// Create a new tool builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    /// Set the tool description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the parameters schema.
    pub fn parameters(mut self, parameters: Value) -> Self {
        self.parameters = parameters;
        self
    }

    /// Build the tool with an executor.
    pub fn build(self, executor: impl McpToolExecutor + 'static) -> McpServerTool {
        McpServerTool {
            name: self.name,
            description: self.description,
            parameters: self.parameters,
            executor: Arc::new(executor),
        }
    }
}

/// Simple executor that wraps a closure.
pub struct ClosureExecutor<F>
where
    F: Fn(Value, &McpToolContext) -> Result<String, String> + Send + Sync,
{
    f: F,
}

impl<F> ClosureExecutor<F>
where
    F: Fn(Value, &McpToolContext) -> Result<String, String> + Send + Sync,
{
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

#[async_trait::async_trait]
impl<F> McpToolExecutor for ClosureExecutor<F>
where
    F: Fn(Value, &McpToolContext) -> Result<String, String> + Send + Sync,
{
    async fn execute(&self, args: Value, ctx: &McpToolContext) -> Result<String, String> {
        (self.f)(args, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = McpServer::new("test-server", "1.0.0");
        assert_eq!(server.name, "test-server");
        assert_eq!(server.version, "1.0.0");
        assert_eq!(server.tool_count(), 0);
    }

    #[test]
    fn test_tool_registration() {
        let mut server = McpServer::new("test", "1.0");

        server.register_tool(
            "echo",
            "Echo the input",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            }),
            ClosureExecutor::new(|args, _ctx| {
                let msg = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("no message");
                Ok(msg.to_string())
            }),
        );

        assert_eq!(server.tool_count(), 1);
    }

    #[test]
    fn test_initialize_response() {
        let server = McpServer::new("test", "1.0");
        let response = server.handle_initialize(1);

        assert_eq!(response.id, 1);
        assert!(response.error.is_none());

        let result: InitializeResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.server_info.name, "test");
    }

    #[test]
    fn test_list_tools_response() {
        let mut server = McpServer::new("test", "1.0");
        server.register_tool(
            "test_tool",
            "A test tool",
            serde_json::json!({}),
            ClosureExecutor::new(|_, _| Ok("ok".to_string())),
        );

        let response = server.handle_list_tools(2);
        assert_eq!(response.id, 2);
        assert!(response.error.is_none());

        let result: ListToolsResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "test_tool");
    }

    #[tokio::test]
    async fn test_call_tool() {
        let mut server = McpServer::new("test", "1.0");
        server.register_tool(
            "add",
            "Add two numbers",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                }
            }),
            ClosureExecutor::new(|args, _ctx| {
                let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                Ok(format!("{}", a + b))
            }),
        );

        let params = serde_json::json!({
            "name": "add",
            "arguments": { "a": 1, "b": 2 }
        });

        let response = server.handle_call_tool(3, Some(params)).await;
        assert_eq!(response.id, 3);
        assert!(response.error.is_none());

        let result: ToolCallResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(!result.is_error);
        match &result.content[0] {
            ToolContent::Text { text } => assert_eq!(text, "3"),
            _ => panic!("Expected text content"),
        }
    }
}
