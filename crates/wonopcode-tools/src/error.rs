//! Tool error types.

use thiserror::Error;

/// Result type for tool operations.
pub type ToolResult<T> = Result<T, ToolError>;

/// Errors that can occur during tool execution.
#[derive(Debug, Error)]
pub enum ToolError {
    /// Invalid parameters.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Permission denied.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Execution failed.
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// Operation timed out.
    #[error("Timeout after {0:?}")]
    Timeout(std::time::Duration),

    /// Operation was cancelled.
    #[error("Cancelled")]
    Cancelled,

    /// File not found.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ToolError {
    /// Create a validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Create a permission denied error.
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied(message.into())
    }

    /// Create an execution failed error.
    pub fn execution_failed(message: impl Into<String>) -> Self {
        Self::ExecutionFailed(message.into())
    }

    /// Create a file not found error.
    pub fn file_not_found(path: impl Into<String>) -> Self {
        Self::FileNotFound(path.into())
    }
}
