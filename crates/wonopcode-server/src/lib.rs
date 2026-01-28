//! HTTP server for wonopcode.
//!
//! Provides REST API, SSE, WebSocket, and Iggy-based transport for the TUI and external clients.
//!
//! ## Transport Options
//!
//! - **HTTP/SSE** (legacy): `create_headless_router` - Used by `--headless` mode
//! - **Iggy** (new): `AgentServer` - High-performance message streaming

pub mod git;
pub mod headless;
pub mod iggy;
pub mod prompt;
pub mod routes;
pub mod sse;
pub mod state;
pub mod ws;

pub use git::{GitCommitInfo, GitError, GitFileState, GitFileStatus, GitOperations, GitStatus};
pub use headless::{
    create_headless_router, create_headless_router_with_mcp, create_headless_router_with_options,
    HeadlessState,
};
pub use iggy::{
    app_update_to_server_payload, client_payload_to_app_action, client_payload_to_legacy_action,
    legacy_update_to_server_payload, AgentServer, AgentServerError,
};
pub use prompt::{PromptEvent, PromptRequest, PromptResponse, ServerPromptRunner};
pub use routes::create_router;
pub use state::AppState;
