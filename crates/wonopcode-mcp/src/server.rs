//! MCP server configuration and management.
// @ace:implements COMP-T90R6K-1500

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for an MCP server.
///
/// MCP servers are connected via HTTP/SSE transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    /// Server name (unique identifier).
    pub name: String,

    /// URL for SSE transport.
    pub url: String,

    /// Headers for SSE transport.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,

    /// Whether the server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl ServerConfig {
    /// Create an SSE server configuration.
    pub fn sse(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            headers: HashMap::new(),
            enabled: true,
        }
    }

    /// Add a header (for SSE transport).
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Disable the server.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// State of an MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Server is not connected.
    Disconnected,
    /// Server is connecting.
    Connecting,
    /// Server is connected and ready.
    Connected,
    /// Server encountered an error.
    Error(String),
}

impl Default for ServerState {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_config() {
        let config = ServerConfig::sse("test", "https://example.com/sse")
            .with_header("Authorization", "Bearer token");
        assert_eq!(config.name, "test");
        assert_eq!(config.url, "https://example.com/sse".to_string());
        assert_eq!(
            config.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }

    #[test]
    fn test_server_config_new() {
        let config = ServerConfig::sse("my-server", "http://localhost:8080");
        assert_eq!(config.name, "my-server");
        assert_eq!(config.url, "http://localhost:8080");
        assert!(config.headers.is_empty());
        assert!(config.enabled);
    }

    #[test]
    fn test_server_config_disabled() {
        let config = ServerConfig::sse("test", "http://example.com").disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_server_config_multiple_headers() {
        let config = ServerConfig::sse("test", "http://example.com")
            .with_header("Authorization", "Bearer token")
            .with_header("X-Custom-Header", "value")
            .with_header("Content-Type", "application/json");

        assert_eq!(config.headers.len(), 3);
        assert_eq!(
            config.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
        assert_eq!(
            config.headers.get("X-Custom-Header"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_server_config_serialization() {
        let config = ServerConfig::sse("test-server", "https://api.example.com/mcp")
            .with_header("Authorization", "Bearer token123");

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"name\":\"test-server\""));
        assert!(json.contains("\"url\":\"https://api.example.com/mcp\""));

        let deserialized: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test-server");
        assert_eq!(deserialized.url, "https://api.example.com/mcp");
        assert!(deserialized.enabled);
    }

    #[test]
    fn test_server_config_deserialization_defaults() {
        let json = r#"{"name": "test", "url": "http://localhost"}"#;
        let config: ServerConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.name, "test");
        assert_eq!(config.url, "http://localhost");
        assert!(config.headers.is_empty());
        assert!(config.enabled); // default
    }

    #[test]
    fn test_server_config_clone() {
        let config =
            ServerConfig::sse("original", "http://example.com").with_header("Key", "Value");

        let cloned = config.clone();
        assert_eq!(cloned.name, config.name);
        assert_eq!(cloned.url, config.url);
        assert_eq!(cloned.headers, config.headers);
    }

    #[test]
    fn test_server_state_default() {
        let state: ServerState = Default::default();
        assert_eq!(state, ServerState::Disconnected);
    }

    #[test]
    fn test_server_state_variants() {
        assert_eq!(ServerState::Disconnected, ServerState::Disconnected);
        assert_eq!(ServerState::Connecting, ServerState::Connecting);
        assert_eq!(ServerState::Connected, ServerState::Connected);
        assert_eq!(
            ServerState::Error("test".to_string()),
            ServerState::Error("test".to_string())
        );

        assert_ne!(ServerState::Disconnected, ServerState::Connected);
        assert_ne!(
            ServerState::Error("a".to_string()),
            ServerState::Error("b".to_string())
        );
    }

    #[test]
    fn test_server_state_clone() {
        let state = ServerState::Error("connection failed".to_string());
        let cloned = state.clone();
        assert_eq!(cloned, state);
    }

    #[test]
    fn test_server_state_debug() {
        let state = ServerState::Connected;
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Connected"));
    }

    #[test]
    fn test_server_config_debug() {
        let config = ServerConfig::sse("debug-test", "http://test.com");
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("debug-test"));
    }
}