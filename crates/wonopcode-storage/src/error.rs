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
