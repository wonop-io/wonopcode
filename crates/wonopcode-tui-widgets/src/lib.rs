//! Basic UI widgets for wonopcode TUI.
//!
//! This crate provides reusable widget components:
//! - Input widget with history and multi-line support
//! - Footer and topbar widgets
//! - Sidebar with context info
//! - Toast notifications
//! - Spinner animations
//! - And more...

pub mod autocomplete;
pub mod footer;
pub mod help_overlay;
pub mod input;
pub mod logo;
pub mod mode_indicator;
pub mod onboarding;
pub mod search;
pub mod sidebar;
pub mod slash_commands;
pub mod spinner;
pub mod status;
pub mod timeline;
pub mod toast;
pub mod topbar;
pub mod which_key;

// Re-export commonly used types
pub use autocomplete::{AutocompleteAction, FileAutocomplete};
pub use footer::{FooterMode, FooterStatus, FooterWidget, SandboxDisplayState};
pub use help_overlay::{HelpContext, HelpEntry, HelpOverlay};
pub use input::{InputAction, InputWidget, PromptHistory};
pub use logo::LogoWidget;
pub use mode_indicator::{DisplayMode, ModeIndicator};
pub use onboarding::OnboardingOverlay;
pub use search::{extract_preview, fuzzy_match, SearchMatch, SearchWidget};
pub use sidebar::{
    ContextInfo, LspServerStatus, LspStatus, McpServerStatus, McpStatus, ModifiedFile,
    SidebarSection, SidebarWidget, TodoItem,
};
pub use slash_commands::{SlashCommand, SlashCommandAction, SlashCommandAutocomplete};
pub use spinner::DotsSpinner;
pub use status::StatusWidget;
pub use timeline::{TimelineAction, TimelineEntry, TimelineWidget};
pub use toast::{Toast, ToastManager, ToastType};
pub use topbar::TopBarWidget;
pub use which_key::{KeyBinding, WhichKeyOverlay};
