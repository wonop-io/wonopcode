//! Terminal UI for wonopcode.
//!
//! Built with ratatui, providing an interactive interface for AI-assisted coding.

pub mod app;
pub mod backend;
pub mod widgets;

// Re-export from wonop-tui-core
pub use wonopcode_tui_core::{
    event, keybind, metrics, model_state, theme,
    // Event types
    is_backspace, is_enter, is_escape, is_quit, Event, EventHandler, EventLoopHandle,
    // Keybind types
    KeyAction, Keybind, KeybindConfig, KeybindManager,
    // Metrics
    MetricsSummary, TuiMetrics, WidgetSummary,
    // Model state
    ModelState,
    // Theme
    AgentMode, RenderSettings, Theme,
};

pub use app::{
    install_panic_hook, restore_terminal, ActiveDialog, App, AppAction, AppState, AppUpdate,
    GitCommitUpdate, GitFileUpdate, GitStatusUpdate, LspStatusUpdate, McpStatusUpdate,
    ModifiedFileUpdate, PermissionRequestUpdate, Route, SandboxStatusUpdate, SaveScope,
    TerminalGuard, TodoUpdate,
};
pub use backend::{Backend, BackendError, BackendResult, LocalBackend, RemoteBackend};
pub use widgets::{
    highlight_code, highlight_diff, is_diff, render_markdown, render_markdown_with_width,
    CommandPalette, ContextInfo, DialogItem, DiffHunk, DiffLine, DiffWidget, DisplayMessage,
    DisplayToolCall, DotsSpinner, FileDiff, FooterStatus, FooterWidget, HelpDialog, InputAction,
    InputWidget, LogoWidget, MessageRole, MessagesWidget, ModelDialog, ModifiedFile, PromptHistory,
    SelectDialog, SessionDialog, SidebarWidget, ThemeDialog, Toast, ToastManager, ToastType,
    TodoItem, ToolStatus,
};
