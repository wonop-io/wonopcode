//! wonopcode-runner - Connects the TUI to the AI prompt loop.
//!
//! This crate provides the `Runner` which handles:
//! - AI model interaction
//! - Tool execution
//! - Session management
//! - Sandbox integration
//! - MCP server management

pub mod compaction;
mod runner;

pub use compaction::{CompactionConfig, CompactionResult};
pub use runner::{load_api_key, Runner, RunnerConfig, SandboxRuntimeWrapper};
