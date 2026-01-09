//! MCP server configuration and management.

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
}
