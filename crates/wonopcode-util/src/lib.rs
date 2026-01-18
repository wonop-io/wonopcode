//! Shared utilities for wonopcode.
//!
//! This crate provides common utilities used across the wonopcode workspace:
//! - Error handling patterns
//! - ULID-based identifier generation
//! - Logging setup with tracing
//! - Path utilities
//! - Wildcard pattern matching
//! - File time tracking for concurrent edit detection
//! - Bash permission configuration
//! - RAII-based timing for operation measurement
//! - Performance monitoring and metrics

pub mod bash_permission;
pub mod error;
pub mod file_time;
pub mod id;
pub mod log;
pub mod path;
pub mod perf;
pub mod timing;
pub mod wildcard;

pub use bash_permission::{
    default_bash_permissions, extract_path_args, is_external_path, readonly_bash_permissions,
    BashPermission, BashPermissionConfig,
};
pub use error::{Error, Result};
pub use file_time::{shared_file_time_state, FileTimeError, FileTimeState, FileTimeTracker};
pub use id::Identifier;
pub use perf::{PerfEvent, PerfEventType};
pub use timing::TimingGuard;
