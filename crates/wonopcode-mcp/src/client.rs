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
}
