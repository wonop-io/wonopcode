//! Transport error types.

use thiserror::Error;

/// Errors that can occur during transport operations.
#[derive(Debug, Error)]
pub enum TransportError {
    /// Failed to connect to Iggy server.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Failed to create stream or topic.
    #[error("Infrastructure setup failed: {0}")]
    InfrastructureSetup(String),

    /// Failed to send message.
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Failed to receive message.
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    /// Message serialization failed.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Channel was closed.
    #[error("Channel closed")]
    ChannelClosed,

    /// Transport not connected.
    #[error("Transport not connected")]
    NotConnected,

    /// Operation timed out.
    #[error("Operation timed out")]
    Timeout,

    /// Iggy client error.
    #[error("Iggy error: {0}")]
    Iggy(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

impl TransportError {
    /// Create an Iggy error from any error type.
    pub fn iggy<E: std::fmt::Display>(err: E) -> Self {
        Self::Iggy(err.to_string())
    }
}
