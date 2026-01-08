//! MCP server configuration and management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Transport type for MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Local server via stdio.
    Stdio,
    /// Remote server via SSE.
    Sse,
}

/// Configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    /// Server name (unique identifier).
    pub name: String,

    /// Transport type.
    #[serde(default = "default_transport")]
    pub transport: TransportType,

    /// Command to run (for stdio transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Arguments for the command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,

    /// URL for SSE transport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Headers for SSE transport.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,

    /// Whether the server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_transport() -> TransportType {
    TransportType::Stdio
}

fn default_enabled() -> bool {
    true
}

impl ServerConfig {
    /// Create a stdio server configuration.
    pub fn stdio(
        name: impl Into<String>,
        command: impl Into<String>,
        args: Vec<impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            transport: TransportType::Stdio,
            command: Some(command.into()),
            args: args.into_iter().map(|a| a.into()).collect(),
            env: HashMap::new(),
            cwd: None,
            url: None,
            headers: HashMap::new(),
            enabled: true,
        }
    }

    /// Create an SSE server configuration.
    pub fn sse(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            transport: TransportType::Sse,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: Some(url.into()),
            headers: HashMap::new(),
            enabled: true,
        }
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
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
    fn test_stdio_config() {
        let config = ServerConfig::stdio("test", "echo", vec!["hello"]);
        assert_eq!(config.name, "test");
        assert!(matches!(config.transport, TransportType::Stdio));
        assert_eq!(config.command, Some("echo".to_string()));
        assert_eq!(config.args, vec!["hello"]);
    }

    #[test]
    fn test_sse_config() {
        let config = ServerConfig::sse("test", "https://example.com/sse")
            .with_header("Authorization", "Bearer token");
        assert_eq!(config.name, "test");
        assert!(matches!(config.transport, TransportType::Sse));
        assert_eq!(config.url, Some("https://example.com/sse".to_string()));
        assert_eq!(
            config.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }
}
