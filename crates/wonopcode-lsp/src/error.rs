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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let errors = vec![
            (
                LspError::ServerNotFound("rust".to_string()),
                "Server not found for language: rust",
            ),
            (
                LspError::NoServerForFile("test.rs".to_string()),
                "No server configured for file: test.rs",
            ),
            (
                LspError::ConnectionFailed("timeout".to_string()),
                "Connection failed: timeout",
            ),
            (
                LspError::ProcessError("exit 1".to_string()),
                "Server process error: exit 1",
            ),
            (
                LspError::ProtocolError("invalid".to_string()),
                "Protocol error: invalid",
            ),
            (
                LspError::RequestFailed("not found".to_string()),
                "Request failed: not found",
            ),
            (LspError::Timeout, "Server timeout"),
            (
                LspError::InitializationFailed("init".to_string()),
                "Server initialization failed: init",
            ),
            (
                LspError::InvalidUri("bad://uri".to_string()),
                "Invalid URI: bad://uri",
            ),
        ];

        for (error, expected) in errors {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn test_error_constructors() {
        let conn_err = LspError::connection_failed("failed to connect");
        assert!(conn_err.to_string().contains("Connection failed"));

        let proto_err = LspError::protocol_error("invalid message");
        assert!(proto_err.to_string().contains("Protocol error"));

        let req_err = LspError::request_failed("not found");
        assert!(req_err.to_string().contains("Request failed"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let lsp_err: LspError = io_err.into();
        assert!(lsp_err.to_string().contains("IO error"));
    }

    #[test]
    fn test_error_from_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let lsp_err: LspError = json_err.into();
        assert!(lsp_err.to_string().contains("JSON error"));
    }
}
