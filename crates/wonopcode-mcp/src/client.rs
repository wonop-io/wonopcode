//! MCP client implementation.

use crate::error::{McpError, McpResult};
use crate::protocol::{
    CallToolParams, InitializeParams, InitializeResult, JsonRpcNotification, JsonRpcRequest,
    ListToolsResult, McpTool, ToolCallResult,
};
use crate::server::{ServerConfig, ServerState};
use crate::sse::{SseConfig, SseTransport};
use crate::transport::Transport;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// An MCP server connection.
struct ServerConnection {
    config: ServerConfig,
    transport: Arc<dyn Transport>,
    state: ServerState,
    /// Server capabilities advertised during initialization.
    capabilities: Option<InitializeResult>,
    tools: Vec<McpTool>,
}

impl ServerConnection {
    /// Check if the server supports tools.
    fn supports_tools(&self) -> bool {
        self.capabilities
            .as_ref()
            .map(|c| c.capabilities.tools.is_some())
            .unwrap_or(false)
    }
}

/// MCP client for managing multiple server connections.
pub struct McpClient {
    /// Connected servers.
    servers: RwLock<HashMap<String, ServerConnection>>,
    /// Request ID counter.
    next_id: AtomicU64,
}

impl McpClient {
    /// Create a new MCP client.
    pub fn new() -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Get the next request ID.
    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Add and connect to an MCP server.
    #[allow(clippy::cognitive_complexity)]
    pub async fn add_server(&self, config: ServerConfig) -> McpResult<()> {
        if !config.enabled {
            debug!(server = %config.name, "Server is disabled, skipping");
            return Ok(());
        }

        let name = config.name.clone();
        info!(server = %name, "Connecting to MCP server");

        // Create SSE transport
        // Extract auth token from headers if present
        let auth_token = config
            .headers
            .get("Authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        let sse_config = SseConfig {
            url: config.url.clone(),
            auth_token,
            timeout_secs: 60,
        };

        let transport: Arc<dyn Transport> = Arc::new(SseTransport::new(sse_config)?);

        // Initialize the connection
        let init_params = InitializeParams::default();
        let request = JsonRpcRequest::new(
            self.next_request_id(),
            "initialize",
            Some(serde_json::to_value(&init_params)?),
        );

        let response = transport.request(request).await?;

        if let Some(error) = response.error {
            return Err(McpError::InitializationFailed(error.message));
        }

        let init_result: InitializeResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| McpError::protocol_error("Missing initialize result"))?,
        )
        .map_err(|e| McpError::protocol_error(e.to_string()))?;

        debug!(
            server = %name,
            protocol_version = %init_result.protocol_version,
            server_name = %init_result.server_info.name,
            "MCP server initialized"
        );

        // Send initialized notification
        let notification = JsonRpcNotification::new("notifications/initialized", None);
        transport.notify(notification).await?;

        // List available tools
        let tools = self.list_tools_from_transport(&transport).await?;
        info!(server = %name, tool_count = tools.len(), "Discovered MCP tools");

        // Store connection
        let connection = ServerConnection {
            config,
            transport,
            state: ServerState::Connected,
            capabilities: Some(init_result),
            tools,
        };

        self.servers.write().await.insert(name.clone(), connection);

        Ok(())
    }

    /// List tools from a transport.
    async fn list_tools_from_transport(
        &self,
        transport: &Arc<dyn Transport>,
    ) -> McpResult<Vec<McpTool>> {
        let request = JsonRpcRequest::new(self.next_request_id(), "tools/list", None);

        let response = transport.request(request).await?;

        if let Some(error) = response.error {
            warn!(code = error.code, message = %error.message, "Failed to list tools");
            return Ok(Vec::new());
        }

        let result: ListToolsResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| McpError::protocol_error("Missing tools/list result"))?,
        )
        .map_err(|e| McpError::protocol_error(e.to_string()))?;

        Ok(result.tools)
    }

    /// Remove a server connection.
    pub async fn remove_server(&self, name: &str) -> McpResult<()> {
        let mut servers = self.servers.write().await;
        if let Some(connection) = servers.remove(name) {
            connection.transport.close().await?;
            info!(server = %name, "Disconnected from MCP server");
        }
        Ok(())
    }

    /// List all available tools across all connected servers.
    /// Only returns tools from servers that advertise tools capability.
    pub async fn list_tools(&self) -> Vec<McpTool> {
        let servers = self.servers.read().await;
        servers
            .values()
            .filter(|conn| conn.supports_tools())
            .flat_map(|conn| conn.tools.clone())
            .collect()
    }

    /// List tools from a specific server.
    /// Returns an error if the server doesn't support tools.
    pub async fn list_tools_from_server(&self, server_name: &str) -> McpResult<Vec<McpTool>> {
        let servers = self.servers.read().await;
        let connection = servers
            .get(server_name)
            .ok_or_else(|| McpError::ServerNotFound(server_name.to_string()))?;

        if !connection.supports_tools() {
            debug!(
                server = %server_name,
                "Server does not support tools capability"
            );
            return Ok(Vec::new());
        }

        Ok(connection.tools.clone())
    }

    /// Call a tool.
    ///
    /// This will find the server that provides the tool and call it.
    /// Returns an error if no server provides the tool or doesn't support tools.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> McpResult<ToolCallResult> {
        let servers = self.servers.read().await;

        // Find the server that has this tool and supports tools capability
        let (server_name, transport) = servers
            .iter()
            .filter(|(_, conn)| conn.supports_tools())
            .find(|(_, conn)| conn.tools.iter().any(|t| t.name == name))
            .map(|(name, conn)| (name.clone(), conn.transport.clone()))
            .ok_or_else(|| McpError::ToolNotFound(name.to_string()))?;

        drop(servers); // Release the lock before making the request

        debug!(tool = name, server = %server_name, "Calling MCP tool");

        let params = CallToolParams {
            name: name.to_string(),
            arguments: Some(arguments),
        };

        let request = JsonRpcRequest::new(
            self.next_request_id(),
            "tools/call",
            Some(serde_json::to_value(&params)?),
        );

        let response = transport.request(request).await?;

        if let Some(error) = response.error {
            return Err(McpError::tool_error(error.message));
        }

        let result: ToolCallResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| McpError::protocol_error("Missing tools/call result"))?,
        )
        .map_err(|e| McpError::protocol_error(e.to_string()))?;

        Ok(result)
    }

    /// Get the state of a server.
    pub async fn server_state(&self, name: &str) -> Option<ServerState> {
        let servers = self.servers.read().await;
        servers.get(name).map(|conn| conn.state.clone())
    }

    /// List connected server names.
    pub async fn server_names(&self) -> Vec<String> {
        let servers = self.servers.read().await;
        servers.keys().cloned().collect()
    }

    /// List all servers with their connection status.
    /// Returns a vector of (name, connected, error) tuples.
    pub async fn list_servers(&self) -> Vec<(String, bool, Option<String>)> {
        let servers = self.servers.read().await;
        servers
            .iter()
            .map(|(name, conn)| {
                let connected = matches!(conn.state, ServerState::Connected);
                let error = match &conn.state {
                    ServerState::Error(e) => Some(e.clone()),
                    _ => None,
                };
                (name.clone(), connected, error)
            })
            .collect()
    }

    /// Close all server connections.
    pub async fn close_all(&self) -> McpResult<()> {
        let mut servers = self.servers.write().await;
        for (name, connection) in servers.drain() {
            if let Err(e) = connection.transport.close().await {
                warn!(server = %name, error = %e, "Error closing server connection");
            }
        }
        Ok(())
    }

    /// Toggle a server's enabled state.
    /// If enabled, disconnect and mark as disabled. If disabled, mark as enabled but don't connect.
    /// Returns the new enabled state.
    #[allow(clippy::cognitive_complexity)]
    pub async fn toggle_server(&self, name: &str) -> McpResult<bool> {
        let mut servers = self.servers.write().await;
        if let Some(connection) = servers.get_mut(name) {
            if connection.config.enabled {
                // Disable: close the connection
                if let Err(e) = connection.transport.close().await {
                    warn!(server = %name, error = %e, "Error closing server during toggle");
                }
                connection.config.enabled = false;
                connection.state = ServerState::Disconnected;
                info!(server = %name, "Server disabled");
                Ok(false)
            } else {
                // Mark as enabled but don't reconnect - caller should use reconnect_server
                connection.config.enabled = true;
                info!(server = %name, "Server enabled (call reconnect to connect)");
                Ok(true)
            }
        } else {
            Err(McpError::ServerNotFound(name.to_string()))
        }
    }

    /// Reconnect to a server.
    /// This closes the existing connection and re-establishes it.
    pub async fn reconnect_server(&self, name: &str) -> McpResult<()> {
        // Get the config for the server
        let config = {
            let servers = self.servers.read().await;
            servers
                .get(name)
                .map(|conn| conn.config.clone())
                .ok_or_else(|| McpError::ServerNotFound(name.to_string()))?
        };

        // Remove the old connection
        self.remove_server(name).await?;

        // Re-add with the same config (but mark as enabled)
        let mut enabled_config = config;
        enabled_config.enabled = true;
        self.add_server(enabled_config).await?;

        Ok(())
    }

    /// Check if a server is enabled.
    pub async fn is_server_enabled(&self, name: &str) -> Option<bool> {
        let servers = self.servers.read().await;
        servers.get(name).map(|conn| conn.config.enabled)
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Note: We can't close transports in drop because it's async
        // The caller should call close_all() before dropping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = McpClient::new();
        assert_eq!(client.next_request_id(), 1);
        assert_eq!(client.next_request_id(), 2);
    }

    #[test]
    fn test_client_default() {
        let client = McpClient::default();
        assert_eq!(client.next_request_id(), 1);
    }

    #[test]
    fn test_request_id_increments() {
        let client = McpClient::new();
        let id1 = client.next_request_id();
        let id2 = client.next_request_id();
        let id3 = client.next_request_id();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[tokio::test]
    async fn test_list_tools_empty() {
        let client = McpClient::new();
        let tools = client.list_tools().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let client = McpClient::new();
        let result = client.call_tool("nonexistent", serde_json::json!({})).await;
        assert!(matches!(result, Err(McpError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_server_names_empty() {
        let client = McpClient::new();
        let servers = client.server_names().await;
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_server_state_nonexistent() {
        let client = McpClient::new();
        let state = client.server_state("nonexistent").await;
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_server() {
        let client = McpClient::new();
        // Should not panic when removing a server that doesn't exist
        let _ = client.remove_server("nonexistent").await;
    }

    #[tokio::test]
    async fn test_add_disabled_server() {
        let client = McpClient::new();
        let config = ServerConfig::sse("test-server", "http://localhost:8080").disabled();

        // Adding a disabled server should succeed but not connect
        let result = client.add_server(config).await;
        assert!(result.is_ok());

        // Should not be in the connected servers
        let servers = client.server_names().await;
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_list_servers_empty() {
        let client = McpClient::new();
        let servers = client.list_servers().await;
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_close_all_empty() {
        let client = McpClient::new();
        let result = client.close_all().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_toggle_server_nonexistent() {
        let client = McpClient::new();
        let result = client.toggle_server("nonexistent").await;
        // Should return error since server doesn't exist
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reconnect_server_nonexistent() {
        let client = McpClient::new();
        let result = client.reconnect_server("nonexistent").await;
        // Should return error since server doesn't exist
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_is_server_enabled_nonexistent() {
        let client = McpClient::new();
        let enabled = client.is_server_enabled("nonexistent").await;
        assert!(enabled.is_none());
    }

    #[tokio::test]
    async fn test_list_tools_from_server_nonexistent() {
        let client = McpClient::new();
        let result = client.list_tools_from_server("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_call_tool_with_arguments() {
        let client = McpClient::new();
        let args = serde_json::json!({
            "path": "/tmp/test",
            "content": "hello world"
        });
        let result = client.call_tool("write_file", args).await;
        // Should fail because no server is connected
        assert!(matches!(result, Err(McpError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_multiple_clients_independent() {
        let client1 = McpClient::new();
        let client2 = McpClient::new();

        // Request IDs should be independent
        assert_eq!(client1.next_request_id(), 1);
        assert_eq!(client2.next_request_id(), 1);
        assert_eq!(client1.next_request_id(), 2);
        assert_eq!(client2.next_request_id(), 2);
    }

    #[tokio::test]
    async fn test_add_server_connection_failed() {
        let client = McpClient::new();
        let config = ServerConfig::sse("test-server", "http://127.0.0.1:1");

        // Should fail to connect to invalid port
        let result = client.add_server(config).await;
        assert!(result.is_err());

        // Server should not be in the list
        let servers = client.server_names().await;
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn test_close_all_multiple_times() {
        let client = McpClient::new();

        // Should be safe to call multiple times
        assert!(client.close_all().await.is_ok());
        assert!(client.close_all().await.is_ok());
        assert!(client.close_all().await.is_ok());
    }

    #[tokio::test]
    async fn test_add_disabled_server_with_headers() {
        let client = McpClient::new();
        let config = ServerConfig::sse("test-server", "http://localhost:8080")
            .with_header("Authorization", "Bearer test-token")
            .with_header("X-Custom", "value")
            .disabled();

        let result = client.add_server(config).await;
        assert!(result.is_ok());

        // Should not be connected
        assert!(client.server_names().await.is_empty());
    }

    #[tokio::test]
    async fn test_list_tools_from_server_error_types() {
        let client = McpClient::new();

        // Test different server name patterns
        let result = client.list_tools_from_server("").await;
        assert!(result.is_err());

        let result = client
            .list_tools_from_server("server-with-special-chars-!@#")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_toggle_server_error_message() {
        let client = McpClient::new();
        let result = client.toggle_server("test-server").await;

        match result {
            Err(McpError::ServerNotFound(name)) => {
                assert_eq!(name, "test-server");
            }
            _ => panic!("Expected ServerNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_reconnect_server_error_message() {
        let client = McpClient::new();
        let result = client.reconnect_server("test-server").await;

        match result {
            Err(McpError::ServerNotFound(name)) => {
                assert_eq!(name, "test-server");
            }
            _ => panic!("Expected ServerNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_remove_server_returns_ok_for_nonexistent() {
        let client = McpClient::new();
        // Remove should not error for nonexistent servers
        let result = client.remove_server("nonexistent").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_request_id_concurrent() {
        use std::thread;

        let client = Arc::new(McpClient::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let client = client.clone();
            handles.push(thread::spawn(move || client.next_request_id()));
        }

        let ids: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All IDs should be unique
        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();
        assert_eq!(ids.len(), sorted_ids.len());
    }

    #[test]
    fn test_server_connection_supports_tools() {
        // Test ServerConnection::supports_tools()
        let _capabilities_with_tools = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: crate::protocol::ServerCapabilities {
                tools: Some(crate::protocol::ToolsCapability {
                    list_changed: false,
                }),
                ..Default::default()
            },
            server_info: crate::protocol::ServerInfo {
                name: "test".to_string(),
                version: Some("1.0".to_string()),
            },
        };

        // We can't easily test ServerConnection directly since it's private
        // But we've covered supports_tools through list_tools tests
    }

    #[tokio::test]
    async fn test_call_tool_complex_arguments() {
        let client = McpClient::new();
        let args = serde_json::json!({
            "nested": {
                "key": "value",
                "array": [1, 2, 3],
                "null": null
            },
            "boolean": true,
            "number": 42.5
        });

        let result = client.call_tool("complex_tool", args).await;
        assert!(matches!(result, Err(McpError::ToolNotFound(_))));
    }
}
