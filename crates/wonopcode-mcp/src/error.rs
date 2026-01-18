//! MCP error types.

use thiserror::Error;

/// Result type for MCP operations.
pub type McpResult<T> = Result<T, McpError>;

/// Errors that can occur during MCP operations.
#[derive(Debug, Error)]
pub enum McpError {
    /// Server not found.
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    /// Tool not found.
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Connection failed.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Server process failed.
    #[error("Server process error: {0}")]
    ProcessError(String),

    /// Protocol error.
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Tool execution failed.
    #[error("Tool execution failed: {0}")]
    ToolError(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// HTTP error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Server timeout.
    #[error("Server timeout")]
    Timeout,

    /// Server initialization failed.
    #[error("Server initialization failed: {0}")]
    InitializationFailed(String),

    /// Authentication required.
    #[error("Authentication required")]
    AuthRequired,

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
}

impl McpError {
    /// Create a connection failed error.
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::ConnectionFailed(message.into())
    }

    /// Create a protocol error.
    pub fn protocol_error(message: impl Into<String>) -> Self {
        Self::ProtocolError(message.into())
    }

    /// Create a tool error.
    pub fn tool_error(message: impl Into<String>) -> Self {
        Self::ToolError(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let errors = vec![
            (
                McpError::ServerNotFound("test".to_string()),
                "Server not found: test",
            ),
            (
                McpError::ToolNotFound("tool".to_string()),
                "Tool not found: tool",
            ),
            (
                McpError::ConnectionFailed("timeout".to_string()),
                "Connection failed: timeout",
            ),
            (
                McpError::ProcessError("exit 1".to_string()),
                "Server process error: exit 1",
            ),
            (
                McpError::ProtocolError("invalid".to_string()),
                "Protocol error: invalid",
            ),
            (
                McpError::ToolError("failed".to_string()),
                "Tool execution failed: failed",
            ),
            (McpError::Timeout, "Server timeout"),
            (
                McpError::InitializationFailed("init".to_string()),
                "Server initialization failed: init",
            ),
            (McpError::AuthRequired, "Authentication required"),
            (
                McpError::AuthFailed("bad token".to_string()),
                "Authentication failed: bad token",
            ),
        ];

        for (error, expected) in errors {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn test_error_constructors() {
        let conn_err = McpError::connection_failed("failed to connect");
        assert!(conn_err.to_string().contains("Connection failed"));

        let proto_err = McpError::protocol_error("invalid message");
        assert!(proto_err.to_string().contains("Protocol error"));

        let tool_err = McpError::tool_error("execution failed");
        assert!(tool_err.to_string().contains("Tool execution failed"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let mcp_err: McpError = io_err.into();
        assert!(mcp_err.to_string().contains("IO error"));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let mcp_err: McpError = json_err.into();
        assert!(mcp_err.to_string().contains("JSON error"));
    }
}
