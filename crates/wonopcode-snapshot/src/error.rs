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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_error_not_found_formats() {
        let err = SnapshotError::not_found("snap_12345");
        assert_eq!(err.to_string(), "Snapshot not found: snap_12345");
    }

    #[test]
    fn snapshot_error_operation_failed_formats() {
        let err = SnapshotError::operation_failed("disk full");
        assert_eq!(err.to_string(), "Snapshot operation failed: disk full");
    }

    #[test]
    fn snapshot_error_file_not_found_formats() {
        let err = SnapshotError::FileNotFound("/tmp/test.txt".to_string());
        assert_eq!(err.to_string(), "File not found: /tmp/test.txt");
    }

    #[test]
    fn snapshot_error_invalid_id_formats() {
        let err = SnapshotError::InvalidId("bad-id".to_string());
        assert_eq!(err.to_string(), "Invalid snapshot ID: bad-id");
    }

    #[test]
    fn snapshot_error_corrupted_formats() {
        let err = SnapshotError::Corrupted("invalid json".to_string());
        assert_eq!(err.to_string(), "Snapshot storage corrupted: invalid json");
    }

    #[test]
    fn snapshot_error_io_wraps_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = SnapshotError::from(io_err);
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn snapshot_error_serialization_wraps_serde_error() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let err = SnapshotError::from(json_err);
        assert!(err.to_string().contains("Serialization error"));
    }
}
