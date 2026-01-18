//! File snapshot system for wonopcode.
//!
//! This crate provides file versioning that enables:
//! - Undo/redo of file changes
//! - Diff between versions
//! - Restore files to previous states
//! - Track changes across sessions
//!
//! # Example
//!
//! ```no_run
//! use wonopcode_snapshot::{SnapshotStore, SnapshotConfig};
//! use std::path::PathBuf;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = SnapshotStore::new(
//!     PathBuf::from(".wonopcode/snapshots"),
//!     PathBuf::from("/project/root"),
//!     SnapshotConfig::default(),
//! ).await?;
//!
//! // Take a snapshot before editing
//! let snapshot = store.take(
//!     &[PathBuf::from("src/main.rs")],
//!     "session_123",
//!     "msg_456",
//!     "Before edit",
//! ).await?;
//!
//! // ... edit the file ...
//!
//! // Restore if needed
//! store.restore(&snapshot.id).await?;
//! # Ok(())
//! # }
//! ```

mod error;
mod snapshot;
mod store;

pub use error::{SnapshotError, SnapshotResult};
pub use snapshot::{Snapshot, SnapshotId};
pub use store::{SnapshotConfig, SnapshotStore};
