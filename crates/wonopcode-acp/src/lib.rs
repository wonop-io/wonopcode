//! ACP (Agent Client Protocol) implementation for wonopcode.
//!
//! This crate provides the ACP server that enables IDE integration for editors
//! like Zed, VS Code, and Cursor. Communication happens over stdio using
//! newline-delimited JSON (ndjson).
//!
//! # Protocol Overview
//!
//! The ACP protocol uses JSON-RPC 2.0 over stdio:
//!
//! 1. **Initialization**: Client sends `initialize` request, agent responds with capabilities
//! 2. **Session Management**: `session/new` or `session/load` to start working
//! 3. **Prompting**: `session/prompt` to send messages, agent streams updates
//! 4. **Permissions**: Agent requests permissions for tool execution
//!
//! # Example
//!
//! ```no_run
//! use wonopcode_acp::{Agent, AgentConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = AgentConfig::default();
//!     wonopcode_acp::serve(config).await;
//! }
//! ```
//!
//! # IDE Configuration
//!
//! ## Zed
//!
//! Add to `settings.json`:
//! ```json
//! {
//!   "agent_servers": {
//!     "Wonopcode": {
//!       "command": "wonopcode",
//!       "args": ["acp"]
//!     }
//!   }
//! }
//! ```
//!
//! ## VS Code / Cursor
//!
//! Configure in the AI assistant settings to use the wonopcode ACP server.

pub mod agent;
pub mod processor;
pub mod session;
pub mod transport;
pub mod types;

pub use agent::{serve, Agent, AgentConfig};
pub use processor::{load_api_key, Processor, ProcessorConfig};
pub use session::SessionManager;
pub use transport::{Connection, StdioTransport};
pub use types::*;
