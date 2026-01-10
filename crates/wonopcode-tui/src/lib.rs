//! Terminal UI for wonopcode.
//!
//! Built with ratatui, providing an interactive interface for AI-assisted coding.

pub mod app;
pub mod backend;
pub mod event;
pub mod keybind;
pub mod metrics;
pub mod model_state;
pub mod theme;
pub mod widgets;

pub use app::{
    install_panic_hook, restore_terminal, ActiveDialog, App, AppAction, AppState, AppUpdate,
    GitCommitUpdate, GitFileUpdate, GitStatusUpdate, LspStatusUpdate, McpStatusUpdate,
    ModifiedFileUpdate, PermissionRequestUpdate, Route, SandboxStatusUpdate, SaveScope, TodoUpdate,
};
pub use backend::{Backend, BackendError, BackendResult, LocalBackend, RemoteBackend};
pub use event::{Event, EventHandler};
pub use keybind::{KeyAction, Keybind, KeybindConfig, KeybindManager};
pub use model_state::ModelState;
pub use theme::{AgentMode, RenderSettings, Theme};
pub use widgets::{
    highlight_code, highlight_diff, is_diff, render_markdown, render_markdown_with_width,
    CommandPalette, ContextInfo, DialogItem, DiffHunk, DiffLine, DiffWidget, DisplayMessage,
    DisplayToolCall, DotsSpinner, FileDiff, FooterStatus, FooterWidget, HelpDialog, InputAction,
    InputWidget, LogoWidget, MessageRole, MessagesWidget, ModelDialog, ModifiedFile, PromptHistory,
    SelectDialog, SessionDialog, SidebarWidget, ThemeDialog, Toast, ToastManager, ToastType,
    TodoItem, ToolStatus,
};
