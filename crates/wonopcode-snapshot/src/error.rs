//! Snapshot error types.

use thiserror::Error;

/// Result type for snapshot operations.
pub type SnapshotResult<T> = Result<T, SnapshotError>;

/// Errors that can occur during snapshot operations.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// Snapshot not found.
    #[error("Snapshot not found: {0}")]
    NotFound(String),

    /// File not found.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid snapshot ID.
    #[error("Invalid snapshot ID: {0}")]
    InvalidId(String),

    /// Snapshot storage is corrupted.
    #[error("Snapshot storage corrupted: {0}")]
    Corrupted(String),

    /// Operation failed.
    #[error("Snapshot operation failed: {0}")]
    OperationFailed(String),
}

impl SnapshotError {
    /// Create a not found error.
    pub fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound(id.into())
    }

    /// Create an operation failed error.
    pub fn operation_failed(message: impl Into<String>) -> Self {
        Self::OperationFailed(message.into())
    }
}
