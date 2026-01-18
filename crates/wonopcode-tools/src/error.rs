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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn tool_error_validation_formats() {
        let err = ToolError::validation("invalid input");
        assert_eq!(err.to_string(), "Validation error: invalid input");
    }

    #[test]
    fn tool_error_permission_denied_formats() {
        let err = ToolError::permission_denied("access denied");
        assert_eq!(err.to_string(), "Permission denied: access denied");
    }

    #[test]
    fn tool_error_execution_failed_formats() {
        let err = ToolError::execution_failed("command failed");
        assert_eq!(err.to_string(), "Execution failed: command failed");
    }

    #[test]
    fn tool_error_file_not_found_formats() {
        let err = ToolError::file_not_found("/path/to/file");
        assert_eq!(err.to_string(), "File not found: /path/to/file");
    }

    #[test]
    fn tool_error_timeout_formats() {
        let err = ToolError::Timeout(Duration::from_secs(30));
        assert!(err.to_string().contains("30"));
    }

    #[test]
    fn tool_error_cancelled_formats() {
        let err = ToolError::Cancelled;
        assert_eq!(err.to_string(), "Cancelled");
    }

    #[test]
    fn tool_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: ToolError = io_err.into();
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn tool_error_from_json_error() {
        let json_result: Result<i32, serde_json::Error> = serde_json::from_str("invalid");
        let json_err = json_result.unwrap_err();
        let err: ToolError = json_err.into();
        assert!(err.to_string().contains("JSON error"));
    }
}
