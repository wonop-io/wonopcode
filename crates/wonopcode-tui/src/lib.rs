//! Terminal UI for wonopcode.
//!
//! Built with ratatui, providing an interactive interface for AI-assisted coding.

pub mod app;
pub mod backend;
pub mod widgets;

// Re-export from wonop-tui-core
pub use wonopcode_tui_core::{
    event,
    // Event types
    is_backspace,
    is_enter,
    is_escape,
    is_quit,
    keybind,
    metrics,
    model_state,
    theme,
    // Theme
    AgentMode,
    Event,
    EventHandler,
    EventLoopHandle,
    // Keybind types
    KeyAction,
    Keybind,
    KeybindConfig,
    KeybindManager,
    // Metrics
    MetricsSummary,
    // Model state
    ModelState,
    RenderSettings,
    Theme,
    TuiMetrics,
    WidgetSummary,
};

pub use app::{
    install_panic_hook, restore_terminal, ActiveDialog, App, AppAction, AppState, AppUpdate,
    CompactionType, GitCommitUpdate, GitFileUpdate, GitStatusUpdate, LspStatusUpdate,
    McpStatusUpdate, ModifiedFileUpdate, ObservationalMemoryStateUpdate, ObservationUpdate,
    PermissionRequestUpdate, PhaseUpdate, Route, SandboxStatusUpdate, SaveScope, TerminalGuard,
    TodoUpdate,
};
pub use backend::{Backend, BackendError, BackendResult, IggyBackend, LocalBackend, RemoteBackend};
pub use widgets::{
    highlight_code, highlight_diff, is_diff, render_markdown, render_markdown_with_width,
    AuthMethodDisplay, CommandPalette, ContextInfo, DialogItem, DiffHunk, DiffLine, DiffWidget,
    DisplayMessage, DisplayToolCall, DotsSpinner, FileDiff, FooterStatus, FooterWidget, HelpDialog,
    InputAction, InputWidget, LogoWidget, LspServerStatus, LspStatus, McpServerStatus, McpStatus,
    MessageRole, MessagesWidget, ModelDialog, ModifiedFile, PhaseItem, PromptHistory, SelectDialog,
    SessionDialog, SidebarSection, SidebarTrait, SidebarWidget, ThemeDialog, Toast, ToastManager,
    ToastType, TodoItem, ToolStatus,
};
