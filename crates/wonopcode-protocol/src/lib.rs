//! Shared protocol types for wonopcode client-server communication.
//!
//! This crate defines the types used for communication between:
//! - TUI client (`wonopcode --connect`)
//! - Agent server (`wonopcode --headless`)
//!
//! Communication uses HTTP for actions and SSE for updates.

mod action;
mod state;
mod update;

pub use action::{Action, SaveScope};
pub use state::*;
pub use update::*;
