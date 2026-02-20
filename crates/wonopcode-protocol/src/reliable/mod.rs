//! Reliable communication protocol for Wonop Code.
//!
//! This module implements a bidirectional WebSocket protocol with:
//! - Message acknowledgments (guaranteed delivery or failure notification)
//! - Sequence numbers (gap detection and replay)
//! - State snapshots (full state reconstruction on connect/reconnect)
//!
//! # Protocol Overview
//!
//! ```text
//! Client                              Server
//!   │                                   │
//!   │── Authenticate ─────────────────▶│
//!   │◀─ Authenticated ────────────────│
//!   │                                   │
//!   │── ConnectWorkstream ────────────▶│
//!   │◀─ WorkstreamSnapshot(seq=N) ────│
//!   │── Subscribe(from_seq=N) ────────▶│
//!   │◀─ Events (seq=N+1, N+2, ...) ───│
//!   │── Ack ──────────────────────────▶│
//!   │                                   │
//! ```
//!
//! # Delivery Modes
//!
//! - `Reliable`: Message must be acknowledged, will retry on timeout
//! - `BestEffort`: Fire-and-forget, no ack required

mod client;
mod envelope;
mod events;
mod input;
mod server;
mod snapshot;

pub use client::*;
pub use envelope::*;
pub use events::*;
pub use input::*;
pub use server::*;
pub use snapshot::*;

/// Protocol version for compatibility checks
pub const PROTOCOL_VERSION: u32 = 2;

/// Default ack timeout in milliseconds
pub const DEFAULT_ACK_TIMEOUT_MS: u32 = 5000;

/// Default max retries for reliable messages
pub const DEFAULT_MAX_RETRIES: u8 = 3;

/// Default event buffer capacity per topic
pub const DEFAULT_EVENT_BUFFER_CAPACITY: usize = 1000;

/// Default max pending acks before backpressure
pub const DEFAULT_MAX_PENDING_ACKS: usize = 256;
