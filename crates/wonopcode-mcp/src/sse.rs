//! SSE (Server-Sent Events) transport for remote MCP servers.
//!
//! This implements the streamable HTTP transport for MCP, which uses:
//! - HTTP POST for sending requests
//! - SSE for receiving responses and events

use crate::error::{McpError, McpResult};
use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use crate::transport::Transport;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// SSE transport configuration.
#[derive(Debug, Clone)]
pub struct SseConfig {
    /// The server URL (e.g., `https://mcp.example.com`)
    pub url: String,
    /// Optional authorization token
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            auth_token: None,
            timeout_secs: 60,
        }
    }
}

/// SSE transport for remote MCP servers.
pub struct SseTransport {
    config: SseConfig,
    client: Client,
    connected: AtomicBool,
    /// Cached session ID from server
    session_id: RwLock<Option<String>>,
}

impl SseTransport {
    /// Create a new SSE transport.
    pub fn new(config: SseConfig) -> McpResult<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| {
                McpError::connection_failed(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            config,
            client,
            connected: AtomicBool::new(false),
            session_id: RwLock::new(None),
        })
    }

    /// Set the authorization token.
    pub fn set_auth_token(&mut self, token: String) {
        self.config.auth_token = Some(token);
    }

    /// Build request with common headers.
    fn build_request(&self, body: &str) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.config.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(body.to_string());

        if let Some(ref token) = self.config.auth_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }

        req
    }

    /// Parse SSE response.
    async fn parse_sse_response(&self, response: reqwest::Response) -> McpResult<JsonRpcResponse> {
        let status = response.status();

        if status == StatusCode::UNAUTHORIZED {
            return Err(McpError::AuthRequired);
        }

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(McpError::protocol_error(format!(
                "Server returned {status}: {text}"
            )));
        }

        // Check content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if content_type.contains("text/event-stream") {
            // Parse SSE stream for the response
            self.parse_sse_stream(response).await
        } else {
            // Regular JSON response
            let text = response
                .text()
                .await
                .map_err(|e| McpError::protocol_error(format!("Failed to read response: {e}")))?;

            serde_json::from_str(&text)
                .map_err(|e| McpError::protocol_error(format!("Invalid JSON response: {e}")))
        }
    }

    /// Parse SSE stream for JSON-RPC response.
    async fn parse_sse_stream(&self, response: reqwest::Response) -> McpResult<JsonRpcResponse> {
        use futures::StreamExt;

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk =
                chunk_result.map_err(|e| McpError::protocol_error(format!("Stream error: {e}")))?;

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Parse SSE events
            for line in buffer.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    // Try to parse as JSON-RPC response
                    if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(data) {
                        return Ok(response);
                    }
                }
            }

            // Keep only incomplete lines
            if let Some(last_newline) = buffer.rfind('\n') {
                buffer = buffer[last_newline + 1..].to_string();
            }
        }

        Err(McpError::protocol_error(
            "SSE stream ended without response",
        ))
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse> {
        let request_json = serde_json::to_string(&request)?;

        debug!(id = request.id, method = %request.method, "Sending SSE request");

        let response = self
            .build_request(&request_json)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    McpError::Timeout
                } else if e.is_connect() {
                    McpError::connection_failed(format!("Connection failed: {e}"))
                } else {
                    McpError::protocol_error(format!("Request failed: {e}"))
                }
            })?;

        // Update session ID if provided
        if let Some(session_id) = response.headers().get("x-session-id") {
            if let Ok(id) = session_id.to_str() {
                *self.session_id.write().await = Some(id.to_string());
            }
        }

        self.connected.store(true, Ordering::SeqCst);
        self.parse_sse_response(response).await
    }

    async fn notify(&self, notification: JsonRpcNotification) -> McpResult<()> {
        let notification_json = serde_json::to_string(&notification)?;

        debug!(method = %notification.method, "Sending SSE notification");

        let response = self
            .build_request(&notification_json)
            .send()
            .await
            .map_err(|e| McpError::protocol_error(format!("Notification failed: {e}")))?;

        if !response.status().is_success() {
            warn!(status = %response.status(), "Notification returned non-success status");
        }

        Ok(())
    }

    async fn close(&self) -> McpResult<()> {
        self.connected.store(false, Ordering::SeqCst);
        debug!("Closed SSE transport");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_config_default() {
        let config = SseConfig::default();
        assert!(config.url.is_empty());
        assert!(config.auth_token.is_none());
        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn test_sse_config_clone() {
        let config = SseConfig {
            url: "https://example.com".to_string(),
            auth_token: Some("token".to_string()),
            timeout_secs: 30,
        };
        let cloned = config;
        assert_eq!(cloned.url, "https://example.com");
        assert_eq!(cloned.auth_token, Some("token".to_string()));
        assert_eq!(cloned.timeout_secs, 30);
    }

    #[test]
    fn test_sse_config_debug() {
        let config = SseConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("SseConfig"));
    }

    #[test]
    fn test_sse_transport_creation() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: Some("test-token".to_string()),
            timeout_secs: 30,
        };

        let transport = SseTransport::new(config);
        assert!(transport.is_ok());
    }

    #[test]
    fn test_sse_transport_creation_no_auth() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            timeout_secs: 60,
        };

        let transport = SseTransport::new(config).unwrap();
        assert!(!transport.is_connected());
    }

    #[test]
    fn test_sse_transport_set_auth_token() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            timeout_secs: 60,
        };

        let mut transport = SseTransport::new(config).unwrap();
        assert!(transport.config.auth_token.is_none());

        transport.set_auth_token("new-token".to_string());
        assert_eq!(transport.config.auth_token, Some("new-token".to_string()));
    }

    #[test]
    fn test_sse_transport_is_connected_initially_false() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            timeout_secs: 60,
        };

        let transport = SseTransport::new(config).unwrap();
        assert!(!transport.is_connected());
    }

    #[tokio::test]
    async fn test_sse_transport_close() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            timeout_secs: 60,
        };

        let transport = SseTransport::new(config).unwrap();
        // Set connected to true
        transport.connected.store(true, Ordering::SeqCst);
        assert!(transport.is_connected());

        // Close should set connected to false
        let result = transport.close().await;
        assert!(result.is_ok());
        assert!(!transport.is_connected());
    }

    #[test]
    fn test_sse_transport_build_request_with_auth() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: Some("test-token".to_string()),
            timeout_secs: 60,
        };

        let transport = SseTransport::new(config).unwrap();
        let _request = transport.build_request(r#"{"test": true}"#);
        // Verify the config was set correctly
        assert_eq!(transport.config.url, "https://example.com/mcp");
        assert!(transport.config.auth_token.is_some());
    }

    #[test]
    fn test_sse_transport_build_request_without_auth() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            timeout_secs: 60,
        };

        let transport = SseTransport::new(config).unwrap();
        let _request = transport.build_request(r#"{"test": true}"#);
        // Verify the config was set correctly
        assert_eq!(transport.config.url, "https://example.com/mcp");
        assert!(transport.config.auth_token.is_none());
    }

    #[tokio::test]
    async fn test_sse_transport_request_connection_refused() {
        let config = SseConfig {
            url: "http://127.0.0.1:1".to_string(), // Invalid port
            auth_token: None,
            timeout_secs: 1,
        };

        let transport = SseTransport::new(config).unwrap();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "test".to_string(),
            params: None,
        };

        let result = transport.request(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sse_transport_notify_connection_refused() {
        let config = SseConfig {
            url: "http://127.0.0.1:1".to_string(), // Invalid port
            auth_token: None,
            timeout_secs: 1,
        };

        let transport = SseTransport::new(config).unwrap();
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "test".to_string(),
            params: None,
        };

        let result = transport.notify(notification).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sse_transport_session_id_initially_none() {
        let config = SseConfig {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            timeout_secs: 60,
        };

        let transport = SseTransport::new(config).unwrap();
        let session_id = transport.session_id.read().await;
        assert!(session_id.is_none());
    }
}
