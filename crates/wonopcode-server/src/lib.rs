//! HTTP server for wonopcode.
//!
//! Provides REST API and SSE endpoints for the TUI and external clients.

pub mod git;
pub mod headless;
pub mod prompt;
pub mod routes;
pub mod sse;
pub mod state;

pub use git::{GitCommitInfo, GitError, GitFileState, GitFileStatus, GitOperations, GitStatus};
pub use headless::{
    create_headless_router, create_headless_router_with_mcp, create_headless_router_with_options,
    HeadlessState,
};
pub use prompt::{PromptEvent, PromptRequest, PromptResponse, ServerPromptRunner};
pub use routes::create_router;
pub use state::AppState;
