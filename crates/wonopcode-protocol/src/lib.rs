//! Shared protocol types for wonopcode client-server communication.
//!
//! This crate defines the types used for communication between:
//! - TUI client (`wonopcode --connect`)
//! - Agent server (`wonopcode --headless`)
//! - Desktop client (Tauri app)
//! - Pro server (workstream server)
//!
//! # Protocol Versions
//!
//! ## V1 (Legacy)
//!
//! The original protocol uses HTTP for actions and SSE for updates.
//! This is still supported for backward compatibility.
//!
//! - Actions: `Action` enum sent via HTTP POST
//! - Updates: `Update` enum sent via SSE
//! - State: `State` struct for initial sync
//!
//! ## V2 (Reliable)
//!
//! The new protocol uses bidirectional WebSocket with:
//! - Message acknowledgments (guaranteed delivery or failure notification)
//! - Sequence numbers (gap detection and replay)
//! - State snapshots (full state reconstruction on connect/reconnect)
//!
//! Enable with the `reliable` feature:
//!
//! ```toml
//! [dependencies]
//! wonopcode-protocol = { version = "0.1", features = ["reliable"] }
//! ```
//!
//! See the [`reliable`] module for details.

// V1 (Legacy) protocol
mod action;
mod state;
mod update;

pub use action::{
    Action, ImageData, SaveScope, MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_SIZE_BYTES,
    SUPPORTED_IMAGE_TYPES,
};
pub use state::*;
pub use update::*;

// V2 (Reliable) protocol
pub mod reliable;
