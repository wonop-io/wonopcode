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
