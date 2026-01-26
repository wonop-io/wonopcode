//! Workstream abstraction for managing git worktrees with optional AI runners.
//!
//! A workstream represents a unit of work tied to a git branch/worktree.
//! Workstreams can be in two states:
//!
//! - **Passive**: Just a git worktree reference (branch + path). No compute resources.
//! - **Active**: Passive workstream + Runner + SessionService + Instance + connected clients.
//!
//! The "direct" workstream is a special passive workstream representing the main
//! working tree (where `.git/` lives).

mod types;
mod discovery;
mod service;

pub use types::{
    WorkstreamId, PassiveWorkstream, ActiveWorkstream, WorkstreamInfo,
    WorkstreamStatus, WorkstreamEvent,
};
pub use discovery::{discover_worktrees, get_direct_workstream, get_repo_root};
pub use service::{WorkstreamService, WorkstreamServiceConfig};

#[cfg(test)]
mod tests;
