//! Unified message types for Wonopcode agent communication.
//!
//! This crate defines the canonical message types used for all communication
//! between Wonopcode clients and servers, via Apache Iggy transport.
//!
//! ## Protocol Design
//!
//! All communication uses a unified envelope format:
//! - [`ClientMessage`]: Client-to-server messages (actions, requests)
//! - [`ServerMessage`]: Server-to-client messages (updates, responses)
//!
//! ## Workstream Support
//!
//! Messages include a [`WorkstreamId`] field for routing:
//! - **Community Edition**: Always uses `WorkstreamId::default()` ("default")
//! - **Pro Edition**: Routes messages to specific workstreams
//!
//! ## Example
//!
//! ```rust
//! use wonopcode_message::{ClientMessage, ClientPayload, WorkstreamId};
//!
//! let message = ClientMessage::new(
//!     WorkstreamId::default(),
//!     ClientPayload::SendPrompt {
//!         prompt: "Hello, world!".to_string(),
//!     },
//! );
//!
//! // Serialize for transmission
//! let json = serde_json::to_string(&message).unwrap();
//! ```

mod client;
mod info;
mod server;
mod state;
mod workstream;

pub use client::{ClientMessage, ClientPayload, SaveScope};
pub use info::*;
pub use server::{ServerMessage, ServerPayload};
pub use state::*;
pub use workstream::{WorkstreamId, WorkstreamInfo, WorkstreamStatus, WorkstreamEvent};

/// Re-export common types for convenience.
pub mod prelude {
    pub use crate::client::{ClientMessage, ClientPayload, SaveScope};
    pub use crate::info::*;
    pub use crate::server::{ServerMessage, ServerPayload};
    pub use crate::state::*;
    pub use crate::workstream::{WorkstreamId, WorkstreamInfo, WorkstreamStatus, WorkstreamEvent};
}

/// Error types for message handling.
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid message format.
    #[error("Invalid message format: {0}")]
    InvalidFormat(String),

    /// Unknown message type.
    #[error("Unknown message type: {0}")]
    UnknownType(String),
}

/// Generate a unique message ID.
pub fn generate_message_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Get current timestamp in milliseconds since Unix epoch.
pub fn timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_message_id() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();
        assert_ne!(id1, id2);
        assert!(!id1.is_empty());
    }

    #[test]
    fn test_timestamp_millis() {
        let ts = timestamp_millis();
        assert!(ts > 0);
    }
}
