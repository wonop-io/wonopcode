//! wonopcode-runner - Connects the TUI to the AI prompt loop.
//!
//! This crate provides the `Runner` which handles:
//! - AI model interaction via pluggable `AgentLoop` implementations
//! - Tool execution
//! - Session management
//! - Sandbox integration
//! - MCP server management
//!
//! # Agent Loop Architecture
//!
//! The runner delegates prompt execution to an `AgentLoop` implementation.
//! This allows for different loop strategies (standard, WASM-based, etc.)
//! while keeping all the orchestration logic in the Runner.

pub mod compaction;
mod runner;

pub use compaction::{CompactionConfig, CompactionResult};
pub use runner::{
    get_auth_method, get_provider_status, get_server_config, has_credentials, load_api_key,
    PermissionCheckerAdapter, Runner, RunnerConfig, SandboxRuntimeWrapper,
};

// Re-export agent loop types for convenience
pub use wonopcode_agent_loop::{
    AgentLoop, BoxedAgentLoop, LoopCapabilities, LoopConfig, LoopContext, LoopError, LoopResult,
    LoopUpdate, StandardLoop,
};
