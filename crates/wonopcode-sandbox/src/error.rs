//! Error types for sandbox operations.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during sandbox operations.
#[derive(Debug, Error)]
pub enum SandboxError {
    /// Failed to connect to Docker daemon
    #[error("failed to connect to Docker daemon: {0}")]
    ConnectionFailed(String),

    /// Failed to pull container image
    #[error("failed to pull image '{image}': {message}")]
    ImagePullFailed { image: String, message: String },

    /// Container image not found
    #[error("image not found: {0}")]
    ImageNotFound(String),

    /// Failed to create container
    #[error("failed to create container: {0}")]
    CreateFailed(String),

    /// Failed to start container
    #[error("failed to start container: {0}")]
    StartFailed(String),

    /// Failed to stop container
    #[error("failed to stop container: {0}")]
    StopFailed(String),

    /// Failed to remove container
    #[error("failed to remove container: {0}")]
    RemoveFailed(String),

    /// Sandbox is not running
    #[error("sandbox is not running")]
    NotRunning,

    /// Sandbox is already running
    #[error("sandbox is already running")]
    AlreadyRunning,

    /// Command execution failed
    #[error("command execution failed: {0}")]
    ExecFailed(String),

    /// Command timed out
    #[error("command timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// File not found in sandbox
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),

    /// Failed to read file
    #[error("failed to read file '{path}': {message}")]
    ReadFailed { path: PathBuf, message: String },

    /// Failed to write file
    #[error("failed to write file '{path}': {message}")]
    WriteFailed { path: PathBuf, message: String },

    /// Path is outside the workspace
    #[error("path '{path}' is outside the workspace")]
    PathOutsideWorkspace { path: PathBuf },

    /// Invalid path
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// Lima-specific error
    #[error("Lima error: {0}")]
    LimaError(String),

    /// Lima VM not found
    #[error("Lima VM '{0}' not found")]
    LimaVmNotFound(String),

    /// Configuration error
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// Runtime not available
    #[error("sandbox runtime '{0}' is not available on this system")]
    RuntimeNotAvailable(String),

    /// Operation not supported by this runtime
    #[error("operation '{0}' is not supported by this sandbox runtime")]
    OperationNotSupported(String),

    /// Snapshot not found
    #[error("snapshot '{0}' not found")]
    SnapshotNotFound(String),

    /// Snapshot operation failed
    #[error("snapshot operation failed: {0}")]
    SnapshotFailed(String),

    /// Generic I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl SandboxError {
    /// Create a connection failed error
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::ConnectionFailed(message.into())
    }

    /// Create an image pull failed error
    pub fn image_pull_failed(image: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ImagePullFailed {
            image: image.into(),
            message: message.into(),
        }
    }

    /// Create a read failed error
    pub fn read_failed(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::ReadFailed {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create a write failed error
    pub fn write_failed(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::WriteFailed {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Check if this error indicates the sandbox is not running
    pub fn is_not_running(&self) -> bool {
        matches!(self, Self::NotRunning)
    }

    /// Check if this error is a timeout
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout(_))
    }
}

/// Result type for sandbox operations.
pub type SandboxResult<T> = Result<T, SandboxError>;
