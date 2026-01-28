//! Error types for embedded Iggy server.

use thiserror::Error;

/// Errors that can occur during embedded Iggy server operations.
#[derive(Debug, Error)]
pub enum EmbeddedIggyError {
    /// Failed to find iggy-server binary.
    #[error("Iggy server binary not found: {0}")]
    BinaryNotFound(String),

    /// Failed to start the server process.
    #[error("Failed to start server: {0}")]
    StartFailed(String),

    /// Failed to create configuration.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Server process exited unexpectedly.
    #[error("Server exited unexpectedly: {0}")]
    UnexpectedExit(String),

    /// Failed to create data directory.
    #[error("Failed to create data directory: {0}")]
    DataDirectoryError(String),

    /// Port binding failed.
    #[error("Port binding failed: {0}")]
    PortBindingFailed(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Server health check failed.
    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),

    /// Timeout waiting for server.
    #[error("Timeout waiting for server to start")]
    Timeout,
}
