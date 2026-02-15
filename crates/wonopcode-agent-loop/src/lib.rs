//! Agent loop trait and standard implementation for wonopcode.
//!
//! This crate provides the core abstraction for agentic loops - the component
//! that orchestrates the interaction between user prompts, LLM providers,
//! and tool execution.
//!
//! # Overview
//!
//! An agentic loop handles:
//! - Processing user prompts
//! - Streaming responses from the LLM
//! - Executing tool calls
//! - Managing conversation history
//! - Compacting context when needed
//!
//! # Usage
//!
//! The main trait is [`AgentLoop`], which can be implemented to customize
//! the agentic behavior. The crate provides [`StandardLoop`] as the default
//! implementation that replicates the current behavior.
//!
//! ```rust,ignore
//! use wonopcode_agent_loop::{AgentLoop, StandardLoop};
//!
//! // Use the standard loop
//! let loop_impl = StandardLoop::new();
//!
//! // Or create a custom implementation
//! struct MyCustomLoop;
//!
//! impl AgentLoop for MyCustomLoop {
//!     // ...
//! }
//! ```
//!
//! # Crate Organization
//!
//! - [`AgentLoop`] - The core trait for agentic loops
//! - [`LoopContext`] - Context passed to loop implementations
//! - [`LoopCapabilities`] - Declares what features a loop supports
//! - [`LoopError`] - Error types for loop execution
//! - [`StandardLoop`] - Default implementation matching current behavior

pub mod capabilities;
pub mod context;
pub mod error;
pub mod standard;
pub mod traits;

// Re-exports for convenience
pub use capabilities::LoopCapabilities;
pub use context::{
    CompactionConfig, LoopConfig, LoopContext, LoopUpdate, PermissionCheckRequest,
    PermissionChecker,
};
pub use error::{LoopError, LoopResult};
pub use standard::StandardLoop;
pub use traits::{AgentLoop, BoxedAgentLoop};
