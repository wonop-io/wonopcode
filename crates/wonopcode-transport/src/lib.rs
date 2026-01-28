//! Transport layer for Wonopcode using Apache Iggy message streaming.
//!
//! This crate provides a transport abstraction over Apache Iggy for
//! reliable, high-performance message delivery between Wonopcode
//! clients and servers.
//!
//! ## Features
//!
//! - **Reliable delivery**: Messages are persisted in Iggy
//! - **Offset-based replay**: Reconnecting clients can resume from last offset
//! - **Workstream routing**: Messages routed by `workstream_id` for Pro edition
//! - **Multiple transports**: TCP (default) or QUIC
//!
//! ## Stream Topology
//!
//! ```text
//! Stream: wonopcode
//! ├── Topic: client-messages
//! │   └── Partition 0: All ClientMessage
//! │
//! └── Topic: server-messages
//!     └── Partition 0: All ServerMessage
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use wonopcode_transport::{Transport, TransportConfig};
//! use wonopcode_message::{ClientMessage, ClientPayload, WorkstreamId};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Connect to Iggy
//!     let config = TransportConfig::default();
//!     let transport = Transport::connect(config).await?;
//!
//!     // Send a message
//!     let msg = ClientMessage::for_default(ClientPayload::Ping);
//!     transport.send_client_message(msg).await?;
//!
//!     // Subscribe to server messages
//!     let mut rx = transport.subscribe_server_messages("my-client", None).await?;
//!     while let Some(msg) = rx.recv().await {
//!         println!("Received: {:?}", msg);
//!     }
//!
//!     Ok(())
//! }
//! ```

mod config;
mod error;
mod transport;

pub use config::{TransportConfig, TransportProtocol};
pub use error::TransportError;
pub use transport::Transport;

/// Re-export message types for convenience.
pub use wonopcode_message::{
    ClientMessage, ClientPayload, ServerMessage, ServerPayload, WorkstreamId,
};

/// Stream name used for Wonopcode messages.
pub const STREAM_NAME: &str = "wonopcode";

/// Topic for client-to-server messages.
pub const CLIENT_TOPIC: &str = "client-messages";

/// Topic for server-to-client messages.
pub const SERVER_TOPIC: &str = "server-messages";

/// Default consumer group for the agent server.
pub const AGENT_SERVER_CONSUMER_GROUP: &str = "agent-server";
