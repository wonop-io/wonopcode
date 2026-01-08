//! Core business logic for wonopcode.
//!
//! This crate provides the central coordination layer for wonopcode:
//! - Configuration management (multi-source, JSONC support)
//! - Event bus for inter-component communication
//! - Instance/project state management
//! - Session and message management
//! - Agent definitions and loading
//! - Formatter integration for auto-formatting
//! - Hooks system for automation
//! - Custom command system

pub mod agent;
pub mod bus;
pub mod command;
pub mod config;
pub mod error;
pub mod format;
pub mod hook;
pub mod instance;
pub mod message;
pub mod permission;
pub mod project;
pub mod prompt;
pub mod retry;
pub mod revert;
pub mod session;
pub mod share;
pub mod system_prompt;
pub mod version;

pub use agent::{Agent, AgentMode, AgentPermission, AgentRegistry};
// Re-export bash permission types from util to maintain backwards compatibility
pub use bus::{Bus, SandboxState, SandboxStatusChanged, SandboxToolExecution};
pub use command::{Command, CommandRegistry};
pub use config::Config;
pub use error::{CoreError, CoreResult};
pub use format::{Formatter, FormatterRegistry};
pub use hook::{Hook, HookContext, HookEvent, HookRegistry};
pub use instance::Instance;
pub use message::{Message, MessagePart};
pub use permission::{Decision, PermissionCheck, PermissionManager, PermissionRule};
pub use project::Project;
pub use prompt::{PromptConfig, PromptLoop, PromptResult};
pub use retry::{
    calculate_delay, classify_error, should_retry, RateLimitInfo, RetryHelper, RetryableError,
};
pub use revert::{RevertInput, SessionRevert};
pub use session::Session;
pub use share::{ShareClient, ShareError, ShareInfo};
pub use wonopcode_util::{BashPermission, BashPermissionConfig};
