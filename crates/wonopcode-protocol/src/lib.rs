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

pub use action::{
    Action, ImageData, SaveScope, MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_SIZE_BYTES,
    SUPPORTED_IMAGE_TYPES,
};
pub use state::*;
pub use update::*;
