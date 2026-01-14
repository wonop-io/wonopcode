//! UI widgets for the TUI.

pub mod autocomplete;
pub mod dialog;
pub mod diff;
pub mod footer;
pub mod help_overlay;
pub mod input;
pub mod logo;
pub mod markdown;
pub mod messages;
pub mod mode_indicator;
pub mod onboarding;
pub mod search;
pub mod sidebar;
pub mod slash_commands;
pub mod spinner;
pub mod status;
pub mod syntax;
pub mod timeline;
pub mod toast;
pub mod topbar;
pub mod which_key;

pub use autocomplete::{AutocompleteAction, FileAutocomplete};
pub use dialog::{
    CommandPalette, DialogItem, GitCommitDisplay, GitDialog, GitDialogResult, GitFileDisplay,
    GitView, HelpDialog, ModelDialog, PerfDialog, SelectDialog, SessionDialog, ThemeDialog,
    TimelineDialog, TimelineItem,
};
pub use diff::{simple_diff, DiffHunk, DiffLine, DiffNavAction, DiffWidget, FileDiff};
pub use footer::{FooterStatus, FooterWidget};
pub use help_overlay::{HelpContext, HelpEntry, HelpOverlay};
pub use input::{InputAction, InputWidget, PromptHistory};
pub use logo::LogoWidget;
pub use markdown::{
    render_markdown, render_markdown_with_regions, render_markdown_with_width, CodeRegion,
    RenderedMarkdown,
};
pub use messages::{
    ClickableCodeRegion, DisplayMessage, DisplayToolCall, MessageRole, MessageSegment,
    MessagesWidget, ToolStatus,
};
pub use mode_indicator::{DisplayMode, ModeIndicator};
pub use onboarding::OnboardingOverlay;
pub use search::{extract_preview, fuzzy_match, SearchMatch, SearchWidget};
pub use sidebar::{ContextInfo, ModifiedFile, SidebarSection, SidebarWidget, TodoItem};
pub use slash_commands::{SlashCommand, SlashCommandAction, SlashCommandAutocomplete};
pub use spinner::DotsSpinner;
pub use syntax::{highlight_code, highlight_diff, is_diff};
pub use timeline::{TimelineAction, TimelineEntry, TimelineWidget};
pub use toast::{Toast, ToastManager, ToastType};
pub use topbar::TopBarWidget;
pub use which_key::{KeyBinding, WhichKeyOverlay};
