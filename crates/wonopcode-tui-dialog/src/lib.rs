//! Dialog widgets for modal interfaces.
//!
//! This module provides various dialog widgets for the TUI:
//! - [`SelectDialog`] - A filterable selection dialog
//! - [`CommandPalette`] - Quick command search and execution
//! - [`InputDialog`] - Text input with validation
//! - [`SettingsDialog`] - Configuration management
//! - [`GitDialog`] - Git operations (status, commit, diff)
//! - [`McpDialog`] - MCP server management
//! - [`PermissionDialog`] - Permission requests
//! - [`SandboxDialog`] - Sandbox file management
//! - [`StatusDialog`] - Session status display
//! - [`HelpDialog`] - Keyboard shortcuts reference
//! - [`PerfDialog`] - Performance metrics
//! - [`TimelineDialog`] - Message timeline navigation

mod command;
mod common;
mod git;
mod input;
mod mcp;
mod permission;
mod sandbox;
mod settings;
mod status;
mod timeline;

// Re-export all public types
pub use command::{
    AgentDialog, AgentInfo, CommandPalette, ModelDialog, SessionDialog, ThemeDialog,
};
pub use common::{centered_rect, DialogItem, SelectDialog};
pub use git::{GitCommitDisplay, GitDialog, GitDialogResult, GitFileDisplay, GitView};
pub use input::{InputDialog, InputDialogResult};
pub use mcp::{McpDialog, McpServerInfo, McpStatus};
pub use permission::{PermissionDialog, PermissionResult};
pub use sandbox::{SandboxAction, SandboxDialog, SandboxState};
pub use settings::{
    SaveScope, SettingItem, SettingValue, SettingsDialog, SettingsResult, SettingsTab,
};
pub use status::{HelpDialog, PerfDialog, StatusDialog};
pub use timeline::{TimelineDialog, TimelineItem};
