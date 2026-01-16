//! Storage error types.

use thiserror::Error;

/// Result type for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Errors that can occur during storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// IO error (file not found, permission denied, etc.)
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Key not found
    #[error("Key not found: {0}")]
    NotFound(String),

    /// Invalid key format
    #[error("Invalid key: {0}")]
    InvalidKey(String),

    /// Concurrent modification detected
    #[error("Concurrent modification detected")]
    ConcurrentModification,

    /// Storage is read-only
    #[error("Storage is read-only")]
    ReadOnly,

    /// Lock was poisoned (another thread panicked while holding the lock)
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),
}

impl StorageError {
    /// Create a not found error with the given key.
    pub fn not_found(key: &[&str]) -> Self {
        Self::NotFound(key.join("/"))
    }

    /// Create an invalid key error.
    pub fn invalid_key(message: impl Into<String>) -> Self {
        Self::InvalidKey(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_error_not_found_formats_key() {
        let err = StorageError::not_found(&["session", "proj_123", "ses_456"]);
        assert_eq!(err.to_string(), "Key not found: session/proj_123/ses_456");
    }

    #[test]
    fn storage_error_invalid_key_formats_message() {
        let err = StorageError::invalid_key("empty key component");
        assert_eq!(err.to_string(), "Invalid key: empty key component");
    }

    #[test]
    fn storage_error_io_wraps_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = StorageError::from(io_err);
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn storage_error_json_wraps_serde_error() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let err = StorageError::from(json_err);
        assert!(err.to_string().contains("JSON error"));
    }

    #[test]
    fn storage_error_concurrent_modification_displays() {
        let err = StorageError::ConcurrentModification;
        assert_eq!(err.to_string(), "Concurrent modification detected");
    }

    #[test]
    fn storage_error_read_only_displays() {
        let err = StorageError::ReadOnly;
        assert_eq!(err.to_string(), "Storage is read-only");
    }

    #[test]
    fn storage_error_lock_poisoned_displays() {
        let err = StorageError::LockPoisoned("mutex poisoned".to_string());
        assert_eq!(err.to_string(), "Lock poisoned: mutex poisoned");
    }
}
