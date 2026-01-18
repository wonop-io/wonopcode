//! Error types for the core crate.

use thiserror::Error;

/// Core error types.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Configuration error.
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    /// Session error.
    #[error("session error: {0}")]
    Session(#[from] SessionError),

    /// Storage error.
    #[error("storage error: {0}")]
    Storage(#[from] wonopcode_storage::StorageError),

    /// IO error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Instance not initialized.
    #[error("instance not initialized - call Instance::provide() first")]
    InstanceNotInitialized,

    /// Project not found.
    #[error("project not found: {0}")]
    ProjectNotFound(String),
}

/// Configuration-specific errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Invalid JSON/JSONC syntax.
    #[error("invalid config at {path}: {message}")]
    InvalidJson { path: String, message: String },

    /// Config validation failed.
    #[error("config validation failed: {message}")]
    Validation { message: String },

    /// Config file not found (not an error for optional configs).
    #[error("config file not found: {path}")]
    NotFound { path: String },

    /// Environment variable not found during substitution.
    #[error("environment variable not found: {name}")]
    EnvVarNotFound { name: String },

    /// File reference not found during substitution.
    #[error("file reference not found: {path}")]
    FileRefNotFound { path: String },

    /// Invalid path (e.g., could not determine config directory).
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

/// Session-specific errors.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Session not found.
    #[error("session not found: {id}")]
    NotFound { id: String },

    /// Message not found.
    #[error("message not found: {id}")]
    MessageNotFound { id: String },

    /// Part not found.
    #[error("part not found: {id}")]
    PartNotFound { id: String },

    /// Session is locked (being compacted, etc.).
    #[error("session is locked: {id}")]
    Locked { id: String },
}

/// Result type for core operations.
pub type CoreResult<T> = Result<T, CoreError>;
