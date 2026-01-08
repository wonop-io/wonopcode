//! LSP error types.

use thiserror::Error;

/// Result type for LSP operations.
pub type LspResult<T> = Result<T, LspError>;

/// Errors that can occur during LSP operations.
#[derive(Debug, Error)]
pub enum LspError {
    /// Server not found.
    #[error("Server not found for language: {0}")]
    ServerNotFound(String),

    /// Server not configured.
    #[error("No server configured for file: {0}")]
    NoServerForFile(String),

    /// Connection failed.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Server process failed.
    #[error("Server process error: {0}")]
    ProcessError(String),

    /// Protocol error.
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Request failed.
    #[error("Request failed: {0}")]
    RequestFailed(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Server timeout.
    #[error("Server timeout")]
    Timeout,

    /// Server initialization failed.
    #[error("Server initialization failed: {0}")]
    InitializationFailed(String),

    /// Invalid URI.
    #[error("Invalid URI: {0}")]
    InvalidUri(String),
}

impl LspError {
    /// Create a connection failed error.
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::ConnectionFailed(message.into())
    }

    /// Create a protocol error.
    pub fn protocol_error(message: impl Into<String>) -> Self {
        Self::ProtocolError(message.into())
    }

    /// Create a request failed error.
    pub fn request_failed(message: impl Into<String>) -> Self {
        Self::RequestFailed(message.into())
    }
}
