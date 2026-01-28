//! Embedded Apache Iggy server for standalone Wonopcode operation.
//!
//! This crate provides functionality to run an Iggy server embedded within
//! the Wonopcode process for standalone/local operation. It manages:
//!
//! - Server lifecycle (start, stop)
//! - Configuration generation
//! - Port allocation
//! - Data directory management
//!
//! ## Usage
//!
//! ```rust,ignore
//! use wonopcode_iggy_embedded::{EmbeddedIggy, EmbeddedIggyConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Start embedded server with default config (in-memory, random port)
//!     let config = EmbeddedIggyConfig::default();
//!     let server = EmbeddedIggy::start(config).await?;
//!
//!     println!("Iggy server running at {}", server.address());
//!
//!     // Server will be stopped when dropped
//!     // Or explicitly: server.stop().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Modes
//!
//! - **In-Memory**: No persistence, data lost on restart (default for standalone)
//! - **Persistent**: Data stored in specified directory (for headless/server mode)

mod config;
mod error;
mod server;

pub use config::EmbeddedIggyConfig;
pub use error::EmbeddedIggyError;
pub use server::EmbeddedIggy;

/// Re-export transport config for convenience.
pub use wonopcode_transport::TransportConfig;
