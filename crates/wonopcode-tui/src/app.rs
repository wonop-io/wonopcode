//! Main application for the TUI.

use crate::{
    event::{is_escape, Event, EventHandler},
    metrics::{self, EventType},
    model_state::ModelState,
    theme::{AgentMode, RenderSettings, Theme},
    widgets::{
        autocomplete::{AutocompleteAction, FileAutocomplete},
        dialog::{
            AgentDialog, AgentInfo, CommandPalette, GitCommitDisplay, GitDialog, GitDialogResult,
            GitFileDisplay, HelpDialog, InputDialog, InputDialogResult, McpDialog, McpServerInfo,
            McpStatus as DialogMcpStatus, ModelDialog, PerfDialog, PermissionDialog,
            PermissionResult, SandboxAction, SandboxDialog, SandboxState as DialogSandboxState,
            SessionDialog, SettingsDialog, SettingsResult, StatusDialog, ThemeDialog,
            TimelineDialog, TimelineItem,
        },
        footer::{FooterStatus, FooterWidget},
        help_overlay::{HelpContext, HelpOverlay},
        input::{InputAction, InputWidget},
        logo::LogoWidget,
        messages::{DisplayMessage, DisplayToolCall, MessageSegment, MessagesWidget, ToolStatus},
        mode_indicator::{DisplayMode, ModeIndicator},
        onboarding::OnboardingOverlay,
        search::{extract_preview, fuzzy_match, SearchMatch, SearchWidget},
        sidebar::{LspStatus, McpServerStatus, McpStatus, ModifiedFile, SidebarWidget, TodoItem},
        slash_commands::{SlashCommandAction, SlashCommandAutocomplete},
        toast::{Toast, ToastManager},
        topbar::TopBarWidget,
        which_key::WhichKeyOverlay,
    },
};
use arboard::Clipboard;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyCode, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal,
};
use std::io::{self, Write};
use std::process::Command;
use tokio::sync::mpsc;

// Re-export SaveScope for use in runner
pub use crate::widgets::dialog::SaveScope;

/// Restore terminal to normal state.
///
/// This should be called on panic or normal exit to ensure the terminal
/// is left in a usable state (not in raw mode, not in alternate screen).
pub fn restore_terminal() {
    // Best effort - ignore errors since we may be in a panic
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste,
        crossterm::cursor::Show
    );
    let _ = io::stdout().flush();
}

/// Install a panic hook that restores the terminal before printing the panic.
///
/// This ensures that if the application panics, the terminal is restored
/// to a usable state before the panic message is printed.
pub fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Restore terminal first
        restore_terminal();
        // Then call the original panic hook to print the panic message
        original_hook(panic_info);
    }));
}

/// Current view/route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    /// Home screen with logo.
    #[default]
    Home,
    /// Active session view.
    Session,
}

/// Active dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveDialog {
    None,
    CommandPalette,
    ModelSelect,
    AgentSelect,
    SessionList,
    ThemeSelect,
    Help,
    Status,
    Perf,
    Rename,
    Mcp,
    Timeline,
    Sandbox,
    Settings,
    Permission,
    Git,
}

/// State of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppState {
    /// Normal input mode.
    #[default]
    Input,
    /// Viewing messages (scroll mode).
    Scrolling,
    /// Selecting text for copying.
    Selecting,
    /// Searching through messages.
    Searching,
    /// Waiting for AI response.
    Waiting,
    /// Leader key pressed (awaiting second key).
    Leader,
    /// Quit requested.
    Quit,
}

/// Actions that can be sent from the UI.
#[derive(Debug, Clone)]
pub enum AppAction {
    /// Send a prompt to the AI.
    SendPrompt(String),
    /// Cancel the current operation.
    Cancel,
    /// Quit the application.
    Quit,
    /// Switch to a session.
    SwitchSession(String),
    /// Change model.
    ChangeModel(String),
    /// Change agent.
    ChangeAgent(String),
    /// Create new session.
    NewSession,
    /// Open external editor with content, returns edited content.
    OpenEditor { content: String },
    /// Undo last message.
    Undo,
    /// Redo undone message.
    Redo,
    /// Revert to a specific message.
    Revert { message_id: String },
    /// Unrevert (cancel a pending revert).
    Unrevert,
    /// Compact the conversation (prune/summarize).
    Compact,
    /// Rename the current session.
    RenameSession { title: String },
    /// Toggle an MCP server on/off.
    McpToggle { name: String },
    /// Reconnect an MCP server.
    McpReconnect { name: String },
    /// Fork the session from a specific message.
    ForkSession { message_id: Option<String> },
    /// Share the current session.
    ShareSession,
    /// Unshare the current session.
    UnshareSession,
    /// Go to a specific message in the timeline.
    GotoMessage { message_id: String },
    /// Start the sandbox.
    SandboxStart,
    /// Stop the sandbox.
    SandboxStop,
    /// Restart the sandbox.
    SandboxRestart,
    /// Save settings to config file.
    SaveSettings {
        /// Where to save (project or global).
        scope: SaveScope,
        /// The config to save.
        config: Box<wonopcode_core::config::Config>,
    },
    /// Update test provider settings.
    UpdateTestProviderSettings {
        emulate_thinking: bool,
        emulate_tool_calls: bool,
        emulate_tool_observed: bool,
        emulate_streaming: bool,
    },
    /// Respond to a permission request.
    PermissionResponse {
        /// The request ID to respond to.
        request_id: String,
        /// Whether to allow the action.
        allow: bool,
        /// Remember this decision for future requests.
        remember: bool,
    },
    /// Git: Get repository status.
    GitStatus,
    /// Git: Stage files.
    GitStage { paths: Vec<String> },
    /// Git: Unstage files.
    GitUnstage { paths: Vec<String> },
    /// Git: Checkout (discard changes to) files.
    GitCheckout { paths: Vec<String> },
    /// Git: Create commit with message.
    GitCommit { message: String },
    /// Git: Get commit history.
    GitHistory,
    /// Git: Push to remote.
    GitPush,
    /// Git: Pull from remote.
    GitPull,
}

/// Result from opening external editor.
#[derive(Debug, Clone)]
pub enum EditorResult {
    /// Editor completed with new content.
    Content(String),
    /// Editor was cancelled or unavailable.
    Cancelled,
}

/// A todo item for the sidebar (from tool execution).
#[derive(Debug, Clone)]
pub struct TodoUpdate {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// Updates that can be received by the UI.
#[derive(Debug, Clone)]
pub enum AppUpdate {
    /// Started processing.
    Started,
    /// Text delta from streaming.
    TextDelta(String),
    /// Tool call started.
    ToolStarted {
        name: String,
        id: String,
        input: String,
    },
    /// Tool call completed.
    ToolCompleted {
        id: String,
        success: bool,
        output: String,
        metadata: Option<serde_json::Value>,
    },
    /// Response completed.
    Completed { text: String },
    /// Error occurred.
    Error(String),
    /// Status update.
    Status(String),
    /// Token usage update.
    TokenUsage {
        input: u32,
        output: u32,
        cost: f64,
        context_limit: u32,
    },
    /// Model info update (context limit).
    ModelInfo { context_limit: u32 },
    /// Session list update.
    Sessions(Vec<(String, String, String)>),
    /// Todos updated (from todowrite tool).
    TodosUpdated(Vec<TodoUpdate>),
    /// LSP servers updated.
    LspUpdated(Vec<LspStatusUpdate>),
    /// MCP servers updated.
    McpUpdated(Vec<McpStatusUpdate>),
    /// Modified files updated.
    ModifiedFilesUpdated(Vec<ModifiedFileUpdate>),
    /// Permission pending count updated.
    PermissionsPending(usize),
    /// Sandbox status updated.
    SandboxUpdated(SandboxStatusUpdate),
    /// System message to display in the conversation.
    SystemMessage(String),
    /// Agent changed (e.g., entering/exiting plan mode).
    AgentChanged(String),
    /// Permission request from the runner.
    PermissionRequest(PermissionRequestUpdate),
    /// Session loaded with messages (used when connecting to remote server).
    SessionLoaded {
        id: String,
        title: String,
        messages: Vec<DisplayMessage>,
    },
    /// Git status update.
    GitStatusUpdated(GitStatusUpdate),
    /// Git history update.
    GitHistoryUpdated(Vec<GitCommitUpdate>),
    /// Git operation result (success/error).
    GitOperationResult { success: bool, message: String },
}

/// Git status update from the runner.
#[derive(Debug, Clone)]
pub struct GitStatusUpdate {
    /// Current branch name.
    pub branch: String,
    /// Commits ahead of upstream.
    pub ahead: usize,
    /// Commits behind upstream.
    pub behind: usize,
    /// Files with changes.
    pub files: Vec<GitFileUpdate>,
}

/// Git file update.
#[derive(Debug, Clone)]
pub struct GitFileUpdate {
    /// File path.
    pub path: String,
    /// Status indicator (M, A, D, R, ?, C).
    pub status: String,
    /// Whether file is staged.
    pub staged: bool,
}

/// Git commit update.
#[derive(Debug, Clone)]
pub struct GitCommitUpdate {
    /// Short commit hash.
    pub id: String,
    /// Commit message.
    pub message: String,
    /// Author name.
    pub author: String,
    /// Formatted date.
    pub date: String,
}

/// Sandbox status update.
#[derive(Debug, Clone)]
pub struct SandboxStatusUpdate {
    /// Current state: "disabled", "stopped", "starting", "running", "error"
    pub state: String,
    /// Runtime type (e.g., "Docker", "Lima", "Podman")
    pub runtime_type: Option<String>,
    /// Error message if state is "error"
    pub error: Option<String>,
}

/// Permission request update from the runner.
#[derive(Debug, Clone)]
pub struct PermissionRequestUpdate {
    /// Unique request ID.
    pub id: String,
    /// Tool name requesting permission.
    pub tool: String,
    /// Action being performed.
    pub action: String,
    /// Human-readable description.
    pub description: String,
    /// Path involved (for file operations).
    pub path: Option<String>,
}

/// Modified file update.
#[derive(Debug, Clone)]
pub struct ModifiedFileUpdate {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// LSP server status update.
#[derive(Debug, Clone)]
pub struct LspStatusUpdate {
    pub id: String,
    pub name: String,
    pub root: String,
    /// True if connected successfully, false if failed.
    pub connected: bool,
}

/// MCP server status update.
#[derive(Debug, Clone)]
pub struct McpStatusUpdate {
    pub name: String,
    pub connected: bool,
    pub error: Option<String>,
}

/// The main TUI application.
pub struct App {
    /// Current state.
    state: AppState,
    /// Current route.
    route: Route,
    /// Active dialog.
    dialog: ActiveDialog,
    /// Theme.
    theme: Theme,
    /// Logo widget.
    logo: LogoWidget,
    /// Input widget.
    input: InputWidget,
    /// File autocomplete (for @ mentions).
    autocomplete: FileAutocomplete,
    /// Slash command autocomplete (for / commands).
    slash_autocomplete: SlashCommandAutocomplete,
    /// Messages widget.
    messages: MessagesWidget,
    /// Top bar widget.
    topbar: TopBarWidget,
    /// Footer widget.
    footer: FooterWidget,
    /// Sidebar widget.
    sidebar: SidebarWidget,
    /// Toast manager.
    toasts: ToastManager,
    /// Command palette.
    command_palette: CommandPalette,
    /// Model dialog.
    model_dialog: ModelDialog,
    /// Session dialog.
    session_dialog: Option<SessionDialog>,
    /// Theme dialog.
    theme_dialog: ThemeDialog,
    /// Agent dialog.
    agent_dialog: Option<AgentDialog>,
    /// Help dialog.
    help_dialog: HelpDialog,
    /// Status dialog.
    status_dialog: StatusDialog,
    /// Input dialog (for rename etc.).
    input_dialog: Option<InputDialog>,
    /// MCP dialog.
    mcp_dialog: Option<McpDialog>,
    /// Timeline dialog.
    timeline_dialog: Option<TimelineDialog>,
    /// Sandbox dialog.
    sandbox_dialog: Option<SandboxDialog>,
    /// Settings dialog.
    settings_dialog: Option<SettingsDialog>,
    /// Performance metrics dialog.
    perf_dialog: Option<PerfDialog>,
    /// Permission request dialog.
    permission_dialog: Option<PermissionDialog>,
    /// Queue of pending permission requests (when dialog is already showing).
    permission_queue: std::collections::VecDeque<PermissionRequestUpdate>,
    /// Git dialog.
    git_dialog: Option<GitDialog>,
    /// Mode indicator.
    mode_indicator: ModeIndicator,
    /// Which-key overlay.
    which_key: WhichKeyOverlay,
    /// Context-sensitive help overlay.
    help_overlay: HelpOverlay,
    /// Onboarding overlay for first-time users.
    onboarding: OnboardingOverlay,
    /// Search widget.
    search: SearchWidget,
    /// Whether to show thinking/reasoning blocks.
    show_thinking: bool,
    /// Current session title (for rename).
    session_title: String,
    /// Event handler.
    events: EventHandler,
    /// Action sender.
    action_tx: mpsc::UnboundedSender<AppAction>,
    /// Action receiver (for the runner).
    action_rx: Option<mpsc::UnboundedReceiver<AppAction>>,
    /// Update sender (for the runner).
    update_tx: mpsc::UnboundedSender<AppUpdate>,
    /// Update receiver.
    update_rx: mpsc::UnboundedReceiver<AppUpdate>,
    /// Session list for dialog.
    sessions: Vec<(String, String, String)>,
    /// Current directory.
    directory: String,
    /// Current model.
    model: String,
    /// Current provider.
    provider: String,
    /// Current agent name.
    agent: String,
    /// Available agents for selection.
    available_agents: Vec<AgentInfo>,
    /// Model state for persistence.
    model_state: ModelState,
    /// Clipboard for copy/paste operations.
    clipboard: Option<Clipboard>,
    /// Cached input area rect for click detection.
    input_area: Rect,
    /// Cached messages area rect for click detection.
    messages_area: Rect,
    /// Cached sidebar area rect for click detection.
    sidebar_area: Rect,
    /// Whether the UI needs to be redrawn.
    needs_redraw: bool,
    /// Render settings for performance optimization.
    render_settings: RenderSettings,
}

impl App {
    /// Create a new application.
    pub fn new() -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let (update_tx, update_rx) = mpsc::unbounded_channel();

        // Get history file path
        let history_file = Self::get_history_file_path();
        let input = if let Some(path) = history_file {
            InputWidget::with_history_file(path)
        } else {
            InputWidget::new()
        };

        Self {
            state: AppState::Input,
            route: Route::Home,
            dialog: ActiveDialog::None,
            theme: Theme::default(),
            logo: LogoWidget::new(),
            input,
            autocomplete: FileAutocomplete::new(),
            slash_autocomplete: SlashCommandAutocomplete::new(),
            messages: MessagesWidget::new(),
            topbar: TopBarWidget::new(),
            footer: FooterWidget::new(),
            sidebar: SidebarWidget::new(),
            toasts: ToastManager::new(),
            command_palette: CommandPalette::new(),
            model_dialog: ModelDialog::new(),
            session_dialog: None,
            theme_dialog: ThemeDialog::new(),
            agent_dialog: None,
            help_dialog: HelpDialog::new(),
            status_dialog: StatusDialog::new(),
            input_dialog: None,
            mcp_dialog: None,
            timeline_dialog: None,
            sandbox_dialog: None,
            settings_dialog: None,
            perf_dialog: None,
            permission_dialog: None,
            permission_queue: std::collections::VecDeque::new(),
            git_dialog: None,
            mode_indicator: ModeIndicator::new(),
            which_key: WhichKeyOverlay::new(),
            help_overlay: HelpOverlay::new(),
            onboarding: OnboardingOverlay::new(),
            search: SearchWidget::new(),
            show_thinking: true,
            session_title: String::new(),
            events: EventHandler::new(),
            action_tx,
            action_rx: Some(action_rx),
            update_tx,
            update_rx,
            sessions: Vec::new(),
            directory: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            model: String::new(),
            provider: String::new(),
            agent: "build".to_string(),
            available_agents: Self::default_agents(),
            model_state: ModelState::load(),
            clipboard: Clipboard::new().ok(),
            input_area: Rect::default(),
            messages_area: Rect::default(),
            sidebar_area: Rect::default(),
            needs_redraw: true,
            render_settings: RenderSettings::default(),
        }
    }

    /// Get the path for the history file.
    fn get_history_file_path() -> Option<std::path::PathBuf> {
        // Use state directory: ~/.local/state/wonopcode or platform equivalent
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .map(|h| h.join("Library/Application Support/wonopcode/prompt-history.jsonl"))
        }

        #[cfg(target_os = "linux")]
        {
            dirs::state_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
                .map(|d| d.join("wonopcode/prompt-history.jsonl"))
        }

        #[cfg(target_os = "windows")]
        {
            dirs::data_local_dir().map(|d| d.join("wonopcode/prompt-history.jsonl"))
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }

    /// Get the default agents list.
    fn default_agents() -> Vec<AgentInfo> {
        vec![
            AgentInfo::new("build", "Build")
                .with_description("Default agent for coding tasks with full access")
                .as_default(),
            AgentInfo::new("plan", "Plan")
                .with_description("Planning agent with read-only file access"),
            AgentInfo::new("explore", "Explore")
                .with_description("Fast exploration agent for searching codebases"),
        ]
    }

    /// Take the action receiver (for the runner).
    pub fn take_action_rx(&mut self) -> Option<mpsc::UnboundedReceiver<AppAction>> {
        self.action_rx.take()
    }

    /// Get an update sender (for the runner).
    pub fn update_sender(&self) -> mpsc::UnboundedSender<AppUpdate> {
        self.update_tx.clone()
    }

    /// Set the model name.
    pub fn set_model(&mut self, model: impl Into<String>) {
        let model = model.into();
        // Parse provider/model format
        if let Some((provider, model_name)) = model.split_once('/') {
            self.provider = provider.to_string();
            self.model = model_name.to_string();
        } else {
            self.model = model.clone();
        }

        // Update UI components
        let full_model = format!("{}/{}", self.provider, self.model);
        self.footer.set_model(self.model.clone());
        self.footer.set_provider(self.provider.clone());
        self.input.set_model(self.model.clone());

        // Persist to model state
        self.model_state.add_recent(full_model);
        self.model_state.save();
    }

    /// Get the most recently used model (for startup).
    pub fn recent_model(&self) -> Option<&str> {
        self.model_state.most_recent()
    }

    /// Set the current agent.
    pub fn set_agent(&mut self, agent: impl Into<String>) {
        let agent = agent.into();
        self.agent = agent;

        // Update UI components
        self.sidebar.set_agent(&self.agent);
        self.input.set_agent(AgentMode::parse(&self.agent));

        // Update messages widget for new messages
        self.messages
            .set_streaming_agent(AgentMode::parse(&self.agent));
    }

    /// Get the current agent.
    pub fn current_agent_name(&self) -> &str {
        &self.agent
    }

    /// Set the available agents (from the registry).
    pub fn set_available_agents(&mut self, agents: Vec<AgentInfo>) {
        self.available_agents = agents;
    }

    /// Set the project/directory.
    pub fn set_project(&mut self, project: impl Into<String>) {
        let project = project.into();
        self.directory = project.clone();
        self.topbar.set_directory(&project);
        self.autocomplete
            .set_cwd(std::path::PathBuf::from(&project));
    }

    /// Set the theme by name.
    pub fn set_theme(&mut self, name: &str) {
        self.theme = Theme::by_name(name);
        self.toasts
            .push(Toast::info(format!("Theme: {}", self.theme.name)));
    }

    /// Show the onboarding overlay (for first-time users).
    pub fn show_onboarding(&mut self) {
        self.onboarding.show();
    }

    /// Hide the onboarding overlay.
    pub fn hide_onboarding(&mut self) {
        self.onboarding.hide();
    }

    /// Show an info toast message.
    pub fn show_toast(&mut self, message: &str) {
        self.toasts.push(Toast::info(message));
    }

    /// Add a user message.
    pub fn add_user_message(&mut self, text: String) {
        let mut msg = DisplayMessage::user(text);
        msg.agent = self.current_agent();
        self.messages.add_message(msg);
        self.messages.scroll_to_bottom();
        self.route = Route::Session;
    }

    /// Add an assistant message with segments (preserves text/tool ordering).
    pub fn add_assistant_message_with_segments(&mut self, segments: Vec<MessageSegment>) {
        let mut msg = DisplayMessage::assistant_with_segments(segments);
        msg.agent = self.current_agent();
        msg.model = Some(format!("{}/{}", self.provider, self.model));
        self.messages.add_message(msg);
        self.messages.scroll_to_bottom();
    }

    /// Add an assistant message (legacy API - tools appear after all text).
    pub fn add_assistant_message(&mut self, text: String, tool_calls: Vec<DisplayToolCall>) {
        let mut msg = DisplayMessage::assistant(text);
        msg.tool_calls = tool_calls;
        msg.agent = self.current_agent();
        msg.model = Some(format!("{}/{}", self.provider, self.model));
        self.messages.add_message(msg);
        self.messages.scroll_to_bottom();
    }

    /// Get the current agent mode.
    fn current_agent(&self) -> AgentMode {
        AgentMode::parse(&self.agent)
    }

    /// Copy text to the system clipboard.
    fn copy_to_clipboard(&mut self, text: &str) -> bool {
        if let Some(clipboard) = &mut self.clipboard {
            match clipboard.set_text(text.to_string()) {
                Ok(_) => true,
                Err(e) => {
                    tracing::warn!("Failed to copy to clipboard: {}", e);
                    false
                }
            }
        } else {
            tracing::warn!("Clipboard not available");
            false
        }
    }

    /// Set application state and update mode indicators.
    fn set_state(&mut self, state: AppState) {
        use crate::widgets::footer::FooterMode;

        self.state = state;
        // Update mode indicator to match
        let display_mode = match state {
            AppState::Input => DisplayMode::Input,
            AppState::Scrolling => DisplayMode::Scroll,
            AppState::Selecting => DisplayMode::Select,
            AppState::Searching => DisplayMode::Search,
            AppState::Waiting => DisplayMode::Waiting,
            AppState::Leader => DisplayMode::Leader,
            AppState::Quit => DisplayMode::Input, // Doesn't matter for quit
        };
        self.mode_indicator.set_mode(display_mode);

        // Update footer mode
        let footer_mode = match state {
            AppState::Input => FooterMode::Input,
            AppState::Scrolling => FooterMode::Scroll,
            AppState::Selecting => FooterMode::Select,
            AppState::Searching => FooterMode::Search,
            AppState::Waiting => FooterMode::Waiting,
            AppState::Leader => FooterMode::Leader,
            AppState::Quit => FooterMode::Input,
        };
        self.footer.set_mode(footer_mode);

        // Hide which-key when leaving leader mode
        if state != AppState::Leader {
            self.which_key.hide();
        }

        // Deactivate search when leaving search mode
        if state != AppState::Searching {
            self.search.deactivate();
        }
    }

    /// Perform search across all messages.
    fn perform_search(&mut self) {
        let query = self.search.query().to_string();
        if query.is_empty() {
            self.search.set_matches(vec![]);
            return;
        }

        let mut matches = vec![];

        // Search through all visible messages
        let visible_count = self.messages.message_count();
        for (msg_idx, msg) in self
            .messages
            .get_messages()
            .iter()
            .take(visible_count)
            .enumerate()
        {
            // Search in message content
            if fuzzy_match(&query, &msg.content) {
                matches.push(SearchMatch {
                    message_index: msg_idx,
                    in_tool: false,
                    tool_index: None,
                    preview: extract_preview(&msg.content, &query, 60),
                });
            }

            // Search in tool outputs
            for (tool_idx, tool) in msg.tool_calls.iter().enumerate() {
                if let Some(ref output) = tool.output {
                    if fuzzy_match(&query, output) {
                        matches.push(SearchMatch {
                            message_index: msg_idx,
                            in_tool: true,
                            tool_index: Some(tool_idx),
                            preview: extract_preview(output, &query, 60),
                        });
                    }
                }
                // Also search tool name and input
                if fuzzy_match(&query, &tool.name) {
                    matches.push(SearchMatch {
                        message_index: msg_idx,
                        in_tool: true,
                        tool_index: Some(tool_idx),
                        preview: format!("Tool: {}", tool.name),
                    });
                }
            }
        }

        self.search.set_matches(matches);
    }

    /// Copy the last assistant response to clipboard.
    fn copy_last_response(&mut self) {
        if let Some(content) = self.messages.get_last_assistant_content() {
            let content = content.to_string();
            if self.copy_to_clipboard(&content) {
                self.toasts.push(Toast::success("Copied to clipboard"));
            } else {
                self.toasts
                    .push(Toast::error("Failed to copy to clipboard"));
            }
        } else {
            self.toasts.push(Toast::warning("No response to copy"));
        }
    }

    /// Update the status dialog with current state.
    fn update_status_dialog(&mut self) {
        self.status_dialog.provider = self.provider.clone();
        self.status_dialog.model = self.model.clone();
        self.status_dialog.agent = self.agent.clone();
        self.status_dialog.directory = self.directory.clone();
        self.status_dialog.message_count = self.messages.message_count();

        // Get token info from sidebar
        let (input, output) = self.sidebar.get_tokens();
        self.status_dialog.input_tokens = input;
        self.status_dialog.output_tokens = output;
        self.status_dialog.cost = self.sidebar.get_cost();
        self.status_dialog.context_limit = self.sidebar.get_max_tokens();

        // Get service counts from sidebar
        let (mcp_connected, mcp_total) = self.sidebar.get_mcp_counts();
        self.status_dialog.mcp_connected = mcp_connected;
        self.status_dialog.mcp_total = mcp_total;

        let (lsp_connected, lsp_total) = self.sidebar.get_lsp_counts();
        self.status_dialog.lsp_connected = lsp_connected;
        self.status_dialog.lsp_total = lsp_total;

        self.status_dialog.permissions_pending = self.footer.get_permissions_pending();
    }

    /// Show the MCP servers dialog.
    fn show_mcp_dialog(&mut self) {
        // Get MCP server info from sidebar and convert to dialog format
        let servers: Vec<McpServerInfo> = self
            .sidebar
            .get_mcp_servers()
            .iter()
            .map(|s| {
                let status = match s.status {
                    McpServerStatus::Connected => DialogMcpStatus::Connected,
                    McpServerStatus::Failed => DialogMcpStatus::Error,
                    McpServerStatus::Disabled => DialogMcpStatus::Disconnected,
                    McpServerStatus::NeedsAuth => DialogMcpStatus::Error,
                };
                McpServerInfo::new(s.name.clone())
                    .with_status(status)
                    .with_enabled(s.status != McpServerStatus::Disabled)
                    // Tool count is not tracked in sidebar; would need MCP client query
                    .with_tool_count(0)
            })
            .collect();

        self.mcp_dialog = Some(McpDialog::new(servers));
        self.dialog = ActiveDialog::Mcp;
    }

    /// Show the sandbox dialog.
    fn show_sandbox_dialog(&mut self) {
        use crate::widgets::footer::SandboxDisplayState;

        let footer_state = self.footer.get_sandbox_state();
        let runtime = self.footer.get_sandbox_runtime().map(|s| s.to_string());

        // Convert footer state to dialog state
        let dialog_state = match footer_state {
            SandboxDisplayState::Disabled => DialogSandboxState::Disabled,
            SandboxDisplayState::Stopped => DialogSandboxState::Stopped,
            SandboxDisplayState::Starting => DialogSandboxState::Starting,
            SandboxDisplayState::Running => DialogSandboxState::Running,
            SandboxDisplayState::Error => DialogSandboxState::Error,
        };

        self.sandbox_dialog = Some(SandboxDialog::new(dialog_state, runtime, None));
        self.dialog = ActiveDialog::Sandbox;
    }

    /// Handle a sandbox action from the dialog.
    fn handle_sandbox_action(&mut self, action: SandboxAction) {
        match action {
            SandboxAction::Start => {
                let _ = self.action_tx.send(AppAction::SandboxStart);
                self.toasts.push(Toast::info("Starting sandbox..."));
            }
            SandboxAction::Stop => {
                let _ = self.action_tx.send(AppAction::SandboxStop);
                self.toasts.push(Toast::info("Stopping sandbox..."));
            }
            SandboxAction::Restart => {
                let _ = self.action_tx.send(AppAction::SandboxRestart);
                self.toasts.push(Toast::info("Restarting sandbox..."));
            }
            SandboxAction::Status => {
                // Just close the dialog, status is shown in the dialog itself
            }
        }
        self.dialog = ActiveDialog::None;
        self.sandbox_dialog = None;
    }

    /// Handle a permission dialog result.
    fn handle_permission_result(&mut self, result: PermissionResult) {
        if let Some(dialog) = self.permission_dialog.take() {
            let (allow, remember) = match result {
                PermissionResult::Allow => (true, false),
                PermissionResult::Deny => (false, false),
                PermissionResult::AllowAlways => (true, true),
                PermissionResult::DenyAlways => (false, true),
                PermissionResult::Cancelled => (false, false),
            };

            // Send response back to the runner
            let _ = self.action_tx.send(AppAction::PermissionResponse {
                request_id: dialog.request_id,
                allow,
                remember,
            });

            // Show toast
            let action = if allow { "Allowed" } else { "Denied" };
            let msg = if remember {
                format!("{} {} (remembered)", action, dialog.tool)
            } else {
                format!("{} {}", action, dialog.tool)
            };
            if allow {
                self.toasts.push(Toast::success(msg));
            } else {
                self.toasts.push(Toast::info(msg));
            }
        }

        // Check if there are more queued permission requests
        if let Some(next_req) = self.permission_queue.pop_front() {
            // Show the next queued permission dialog
            self.permission_dialog = Some(PermissionDialog::new(
                next_req.id,
                next_req.tool,
                next_req.action,
                next_req.description,
                next_req.path,
            ));
            self.dialog = ActiveDialog::Permission;
            // Update pending count (current dialog + remaining queue)
            let pending_count = 1 + self.permission_queue.len();
            self.footer.set_pending_permissions(pending_count);
        } else {
            // No more pending permissions
            self.dialog = ActiveDialog::None;
            self.footer.set_pending_permissions(0);
        }
    }

    /// Handle git dialog result.
    fn handle_git_dialog_result(&mut self, result: GitDialogResult) {
        match result {
            GitDialogResult::None => {}
            GitDialogResult::RefreshStatus => {
                let _ = self.action_tx.send(AppAction::GitStatus);
            }
            GitDialogResult::RefreshHistory => {
                let _ = self.action_tx.send(AppAction::GitHistory);
            }
            GitDialogResult::Stage(paths) => {
                let _ = self.action_tx.send(AppAction::GitStage { paths });
            }
            GitDialogResult::Unstage(paths) => {
                let _ = self.action_tx.send(AppAction::GitUnstage { paths });
            }
            GitDialogResult::Checkout(paths) => {
                let _ = self.action_tx.send(AppAction::GitCheckout { paths });
            }
            GitDialogResult::Commit(message) => {
                let _ = self.action_tx.send(AppAction::GitCommit { message });
            }
            GitDialogResult::Push => {
                let _ = self.action_tx.send(AppAction::GitPush);
            }
            GitDialogResult::Pull => {
                let _ = self.action_tx.send(AppAction::GitPull);
            }
            GitDialogResult::Close => {
                self.dialog = ActiveDialog::None;
                self.git_dialog = None;
            }
        }
    }

    /// Show the git dialog.
    pub fn show_git_dialog(&mut self) {
        self.git_dialog = Some(GitDialog::new());
        self.dialog = ActiveDialog::Git;
        // Request initial status
        let _ = self.action_tx.send(AppAction::GitStatus);
    }

    /// Show the settings dialog.
    fn show_settings_dialog(&mut self) {
        // Create settings dialog with current render settings and theme so they show correctly
        self.settings_dialog = Some(SettingsDialog::with_render_settings(
            &self.render_settings,
            &self.theme.name,
        ));
        self.dialog = ActiveDialog::Settings;
    }

    /// Show the settings dialog with config loaded from the instance.
    pub fn show_settings_dialog_with_config(&mut self, config: &wonopcode_core::config::Config) {
        self.settings_dialog = Some(SettingsDialog::from_config(config));
        self.dialog = ActiveDialog::Settings;
    }

    /// Apply render settings from config on startup.
    ///
    /// This should be called after creating the App to restore saved render settings.
    pub fn apply_config(&mut self, config: &wonopcode_core::config::Config) {
        if let Some(tui_config) = &config.tui {
            // Build render settings from config
            let mut settings = RenderSettings::default();

            if let Some(v) = tui_config.markdown {
                settings.markdown_enabled = v;
            }
            if let Some(v) = tui_config.syntax_highlighting {
                settings.syntax_highlighting_enabled = v;
            }
            if let Some(v) = tui_config.code_backgrounds {
                settings.code_backgrounds_enabled = v;
            }
            if let Some(v) = tui_config.tables {
                settings.tables_enabled = v;
            }
            if let Some(fps) = tui_config.streaming_fps {
                settings.streaming_fps = fps;
            }
            if let Some(max) = tui_config.max_messages {
                settings.max_messages = max;
            }
            if let Some(v) = tui_config.low_memory_mode {
                settings.low_memory_mode = v;
                // Note: We don't override other settings here when loading from config.
                // The user's explicit settings take precedence. The low_memory preset
                // is only applied when the user toggles low_memory_mode ON in the UI.
            }
            if let Some(v) = tui_config.enable_test_commands {
                settings.enable_test_commands = v;
            }

            // Test provider settings
            if let Some(v) = tui_config.test_model_enabled {
                settings.test_model_enabled = v;
            }
            if let Some(v) = tui_config.test_emulate_thinking {
                settings.test_emulate_thinking = v;
            }
            if let Some(v) = tui_config.test_emulate_tool_calls {
                settings.test_emulate_tool_calls = v;
            }
            if let Some(v) = tui_config.test_emulate_tool_observed {
                settings.test_emulate_tool_observed = v;
            }
            if let Some(v) = tui_config.test_emulate_streaming {
                settings.test_emulate_streaming = v;
            }

            // Apply settings
            self.render_settings = settings.clone();
            self.messages.set_render_settings(settings.clone());

            // Sync test commands setting to slash autocomplete
            self.slash_autocomplete
                .set_test_commands_enabled(settings.enable_test_commands);

            // Sync test provider settings to runner
            let _ = self.action_tx.send(AppAction::UpdateTestProviderSettings {
                emulate_thinking: settings.test_emulate_thinking,
                emulate_tool_calls: settings.test_emulate_tool_calls,
                emulate_tool_observed: settings.test_emulate_tool_observed,
                emulate_streaming: settings.test_emulate_streaming,
            });
        }

        // Apply theme if set
        if let Some(ref theme_name) = config.theme {
            self.set_theme(theme_name);
        }
    }

    /// Handle a settings dialog result.
    fn handle_settings_result(&mut self, result: SettingsResult) {
        match result {
            SettingsResult::Save(scope) => {
                // Extract values from dialog before making mutable borrows
                let (config, theme_name, new_render_settings) =
                    if let Some(ref dialog) = self.settings_dialog {
                        let config = dialog.to_config();
                        let theme_name = config.theme.clone();
                        let render_settings = dialog.get_render_settings();
                        (Some(config), theme_name, Some(render_settings))
                    } else {
                        (None, None, None)
                    };

                if let Some(config) = config {
                    // Apply theme change immediately if present
                    if let Some(ref theme_name) = theme_name {
                        self.set_theme(theme_name);
                    }

                    // Apply render settings immediately
                    if let Some(new_render_settings) = new_render_settings {
                        let settings_changed = self.render_settings.markdown_enabled
                            != new_render_settings.markdown_enabled
                            || self.render_settings.syntax_highlighting_enabled
                                != new_render_settings.syntax_highlighting_enabled
                            || self.render_settings.code_backgrounds_enabled
                                != new_render_settings.code_backgrounds_enabled
                            || self.render_settings.tables_enabled
                                != new_render_settings.tables_enabled;

                        self.render_settings = new_render_settings.clone();

                        // Always update messages widget with settings
                        self.messages
                            .set_render_settings(new_render_settings.clone());

                        // Sync test commands setting to slash autocomplete
                        self.slash_autocomplete
                            .set_test_commands_enabled(new_render_settings.enable_test_commands);

                        // Sync test provider settings to runner
                        let _ = self.action_tx.send(AppAction::UpdateTestProviderSettings {
                            emulate_thinking: new_render_settings.test_emulate_thinking,
                            emulate_tool_calls: new_render_settings.test_emulate_tool_calls,
                            emulate_tool_observed: new_render_settings.test_emulate_tool_observed,
                            emulate_streaming: new_render_settings.test_emulate_streaming,
                        });

                        // Invalidate cache if visual settings changed
                        if settings_changed {
                            self.messages.invalidate_cache();
                            self.toasts.push(Toast::info("Render settings updated"));
                        }
                    }

                    // Send save action to runner
                    let _ = self.action_tx.send(AppAction::SaveSettings {
                        scope,
                        config: Box::new(config),
                    });

                    let scope_name = match scope {
                        SaveScope::Project => "project",
                        SaveScope::Global => "global",
                    };
                    self.toasts.push(Toast::success(format!(
                        "Settings saved to {scope_name} config"
                    )));
                }
                self.dialog = ActiveDialog::None;
                self.settings_dialog = None;
            }
            SettingsResult::Cancel => {
                self.dialog = ActiveDialog::None;
                self.settings_dialog = None;
            }
            SettingsResult::None => {
                // Dialog still open, check for theme preview
                if let Some(ref dialog) = self.settings_dialog {
                    if let Some(theme_name) = dialog.get_theme() {
                        // Live preview theme changes
                        if self.theme.name != theme_name {
                            self.theme = Theme::by_name(&theme_name);
                        }
                    }
                }
            }
        }
    }

    /// Create test messages for performance testing.
    /// This generates a large number of simulated messages with various content types.
    fn add_test_messages(&mut self) {
        use crate::widgets::messages::{
            DisplayMessage, DisplayToolCall, MessageSegment, ToolStatus,
        };

        // Switch to session view if not already there
        self.route = Route::Session;

        // Sample code blocks for realistic content
        let rust_code = r#"```rust
fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() {
    for i in 0..20 {
        println!("fib({}) = {}", i, fibonacci(i));
    }
}
```"#;

        let python_code = r#"```python
def quicksort(arr):
    if len(arr) <= 1:
        return arr
    pivot = arr[len(arr) // 2]
    left = [x for x in arr if x < pivot]
    middle = [x for x in arr if x == pivot]
    right = [x for x in arr if x > pivot]
    return quicksort(left) + middle + quicksort(right)

numbers = [3, 6, 8, 10, 1, 2, 1]
print(quicksort(numbers))
```"#;

        let js_code = r#"```javascript
async function fetchUserData(userId) {
    try {
        const response = await fetch(`/api/users/${userId}`);
        if (!response.ok) {
            throw new Error(`HTTP error! status: ${response.status}`);
        }
        const data = await response.json();
        return data;
    } catch (error) {
        console.error('Error fetching user:', error);
        throw error;
    }
}
```"#;

        let markdown_table = r#"
| Feature | Status | Notes |
|---------|--------|-------|
| Markdown | ✅ | Full support |
| Syntax | ✅ | Rust, Python, JS |
| Tables | ✅ | Basic support |
| Emoji | ⚠️ | Partial |
"#;

        let long_text = "This is a sample paragraph that demonstrates how the messages widget handles longer text content. It should wrap properly and display without any issues. The text continues to provide more content for testing purposes. **Bold text** and *italic text* and `inline code` should all render correctly.";

        // Generate 100 test messages
        for i in 0..100 {
            // Alternate between user and assistant messages
            if i % 2 == 0 {
                // User message
                let content = match i % 10 {
                    0 => "Can you help me with a Rust function?".to_string(),
                    2 => "How do I implement quicksort in Python?".to_string(),
                    4 => "Show me async/await in JavaScript".to_string(),
                    6 => "What are the performance characteristics?".to_string(),
                    8 => format!("Question #{}: Can you explain this code?", i / 2),
                    _ => format!("Test message #{}", i / 2),
                };
                let msg = DisplayMessage::user(content);
                self.messages.add_message(msg);
            } else {
                // Assistant message with varied content
                let content = match i % 10 {
                    1 => format!("Here's a Fibonacci implementation:\n\n{rust_code}"),
                    3 => format!("Here's quicksort in Python:\n\n{python_code}"),
                    5 => format!("Here's an async example:\n\n{js_code}"),
                    7 => format!("{long_text}\n\n{markdown_table}"),
                    9 => format!("Response #{}: {}\n\n{}", i / 2, long_text, rust_code),
                    _ => long_text.to_string(),
                };

                let mut msg = DisplayMessage::assistant(&content);
                msg.model = Some("test-model".to_string());
                msg.duration = Some(format!("{:.1}s", (i as f64) * 0.5));

                // Add some tool calls to some messages
                if i % 6 == 5 {
                    let mut tool = DisplayToolCall::new(format!("tool-{i}"), "read".to_string());
                    tool.status = ToolStatus::Success;
                    tool.input = Some("path/to/file.rs".to_string());
                    tool.output = Some("File contents here...".to_string());
                    msg.tool_calls.push(tool);

                    // Create segments for proper rendering
                    msg.segments.push(MessageSegment::Text(content.clone()));
                    msg.segments
                        .push(MessageSegment::Tool(msg.tool_calls[0].clone()));
                }

                self.messages.add_message(msg);
            }
        }

        self.toasts.push(Toast::info(
            "Created 100 test messages for performance testing",
        ));
        tracing::info!("Created 100 test messages for performance testing");
    }

    /// Show TUI performance metrics dialog.
    fn show_perf_metrics(&mut self) {
        if let Some(summary) = metrics::summary() {
            // Create dialog with metrics data
            let mut dialog = PerfDialog::new();
            dialog.uptime_secs = summary.uptime_secs;
            dialog.status = summary.status().to_string();
            dialog.total_frames = summary.total_frames;
            dialog.fps = summary.fps;
            dialog.avg_frame_ms = summary.avg_frame_ms;
            dialog.p50_frame_ms = summary.p50_frame_ms;
            dialog.p95_frame_ms = summary.p95_frame_ms;
            dialog.p99_frame_ms = summary.p99_frame_ms;
            dialog.max_frame_ms = summary.max_frame_ms;
            dialog.slow_frames = summary.slow_frames;
            dialog.slow_frame_pct = summary.slow_frame_pct;
            dialog.avg_key_event_ms = summary.avg_key_event_ms;
            dialog.avg_input_latency_ms = summary.avg_input_latency_ms;
            dialog.p99_input_latency_ms = summary.p99_input_latency_ms;
            dialog.avg_scroll_ms = summary.avg_scroll_ms;
            dialog.widget_stats = summary
                .widget_stats
                .iter()
                .map(|w| (w.name.clone(), w.avg_ms, w.max_ms, w.count))
                .collect();

            self.perf_dialog = Some(dialog);
            self.dialog = ActiveDialog::Perf;

            // Also log to tracing
            tracing::info!(
                status = %summary.status(),
                fps = %format!("{:.1}", summary.fps),
                avg_frame_ms = %format!("{:.2}", summary.avg_frame_ms),
                p99_frame_ms = %format!("{:.2}", summary.p99_frame_ms),
                slow_frames_pct = %format!("{:.1}%", summary.slow_frame_pct),
                "TUI performance metrics"
            );
        } else {
            self.toasts
                .push(Toast::warning("Performance metrics not available"));
        }
    }

    /// Show the message timeline dialog.
    fn show_timeline_dialog(&mut self) {
        use crate::widgets::messages::MessageRole;

        // Build timeline items from messages
        let items: Vec<TimelineItem> = self
            .messages
            .get_messages()
            .iter()
            .enumerate()
            .map(|(idx, msg)| {
                let role = match msg.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::System => "system",
                    MessageRole::Tool => "tool",
                };

                // Create a preview of the content
                let preview: String = msg
                    .content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(50)
                    .collect();

                let preview = if preview.len() < msg.content.len() && !msg.content.is_empty() {
                    format!("{preview}...")
                } else {
                    preview
                };

                // Use index as ID since we don't have actual message IDs in the display model
                TimelineItem::new(idx.to_string(), role, preview)
            })
            .collect();

        if items.is_empty() {
            self.toasts.push(Toast::warning("No messages in timeline"));
            return;
        }

        self.timeline_dialog = Some(TimelineDialog::new(items));
        self.dialog = ActiveDialog::Timeline;
    }

    /// Export the current session to a file.
    fn export_session(&mut self) {
        // Build export content
        let mut content = String::new();
        content.push_str(&format!(
            "# Session: {}\n\n",
            if self.session_title.is_empty() {
                "Untitled"
            } else {
                &self.session_title
            }
        ));
        content.push_str(&format!("Directory: {}\n", self.directory));
        content.push_str(&format!("Model: {}\n", self.model));
        content.push_str(&format!("Agent: {}\n\n", self.agent));
        content.push_str("---\n\n");

        // Get messages from the widget
        if let Some(transcript) = self.messages.get_transcript() {
            content.push_str(&transcript);
        } else {
            content.push_str("(No messages)\n");
        }

        // Determine export path
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("wonopcode_export_{timestamp}.md");
        let export_path = std::path::PathBuf::from(&self.directory).join(&filename);

        // Write to file
        match std::fs::write(&export_path, &content) {
            Ok(_) => {
                self.toasts
                    .push(Toast::success(format!("Exported to: {filename}")));
            }
            Err(e) => {
                self.toasts
                    .push(Toast::error(format!("Export failed: {e}")));
            }
        }
    }

    /// Get the external editor command from environment.
    fn get_editor() -> Option<String> {
        std::env::var("VISUAL")
            .ok()
            .or_else(|| std::env::var("EDITOR").ok())
    }

    /// Open content in external editor and return the edited content.
    /// Returns None if editor is not available or editing was cancelled.
    fn open_in_editor(content: &str) -> Option<String> {
        let editor = Self::get_editor()?;

        // Create a temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("wonopcode_{}.md", std::process::id()));

        // Write content to temp file
        if std::fs::write(&temp_file, content).is_err() {
            tracing::error!("Failed to write temp file for editor");
            return None;
        }

        // Parse editor command (may have arguments like "code --wait")
        let parts: Vec<&str> = editor.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        let (cmd, args) = (parts[0], &parts[1..]);

        // Suspend terminal (restore normal mode)
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);

        // Run the editor
        let result = Command::new(cmd)
            .args(args)
            .arg(&temp_file)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();

        // Resume terminal (enter raw mode again)
        let _ = enable_raw_mode();
        let _ = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture);

        match result {
            Ok(status) if status.success() => {
                // Read the edited content
                let edited = std::fs::read_to_string(&temp_file).ok();
                // Clean up temp file
                let _ = std::fs::remove_file(&temp_file);
                edited
            }
            _ => {
                // Clean up temp file
                let _ = std::fs::remove_file(&temp_file);
                None
            }
        }
    }

    /// Check if we should trigger autocomplete based on input content.
    fn check_autocomplete_trigger(&mut self) {
        let text = self.input.content();

        // Check for '/' at the beginning of input (slash commands)
        if let Some(filter) = text.strip_prefix('/') {
            // Don't trigger if there's a space (command complete) or multiple lines
            if !filter.contains(' ') && !filter.contains('\n') {
                if self.slash_autocomplete.is_visible() {
                    self.slash_autocomplete.set_filter(filter);
                } else {
                    self.slash_autocomplete.show(filter);
                }
                // Hide file autocomplete
                self.autocomplete.hide();
                return;
            }
        }

        // Hide slash autocomplete if not starting with /
        if self.slash_autocomplete.is_visible() {
            self.slash_autocomplete.hide();
        }

        // Find the last '@' that could be a trigger (for file mentions)
        if let Some(at_pos) = text.rfind('@') {
            // Check if '@' is at start or after whitespace
            let before_at = if at_pos == 0 {
                None
            } else {
                text.chars().nth(at_pos - 1)
            };
            let is_valid_trigger =
                before_at.is_none() || before_at == Some(' ') || before_at == Some('\n');

            if is_valid_trigger {
                // Get the filter (text after '@')
                let filter = &text[at_pos + 1..];

                // Don't trigger if there's a space in the filter (completed already)
                if !filter.contains(' ') {
                    if self.autocomplete.is_visible() {
                        self.autocomplete.set_filter(filter);
                    } else {
                        self.autocomplete.show(at_pos, filter);
                    }
                    return;
                }
            }
        }

        // No valid trigger found, hide file autocomplete
        if self.autocomplete.is_visible() {
            self.autocomplete.hide();
        }
    }

    /// Apply an autocomplete selection.
    fn apply_autocomplete(&mut self, path: &str) {
        let trigger_pos = self.autocomplete.trigger_pos();
        let current = self.input.content();

        // Build new content: text before '@' + '@path ' + text after filter
        let before = &current[..trigger_pos];

        // Find where the filter ends (next space or end of string)
        let after_at = &current[trigger_pos + 1..];
        let filter_end = after_at
            .find(' ')
            .map(|i| trigger_pos + 1 + i)
            .unwrap_or(current.len());
        let after = &current[filter_end..];

        let new_content = format!("{}@{} {}", before, path, after.trim_start());
        self.input.set_content(new_content);
    }

    /// Undo the last message exchange.
    fn undo_message(&mut self) {
        if self.state == AppState::Waiting {
            self.toasts
                .push(Toast::warning("Cannot undo while waiting for response"));
            return;
        }

        if let Some(user_content) = self.messages.undo() {
            // Restore user message to input
            self.input.set_content(user_content);
            self.toasts
                .push(Toast::info("Message undone - edit and resend"));
            let _ = self.action_tx.send(AppAction::Undo);
        } else {
            self.toasts.push(Toast::warning("Nothing to undo"));
        }
    }

    /// Redo an undone message.
    fn redo_message(&mut self) {
        if self.messages.redo() {
            // Clear input since we're restoring the message
            self.input.clear();
            self.toasts.push(Toast::info("Message restored"));
            let _ = self.action_tx.send(AppAction::Redo);
        } else {
            self.toasts.push(Toast::warning("Nothing to redo"));
        }
    }

    /// Open the current input in external editor.
    fn edit_input_in_editor(&mut self) {
        let editor = Self::get_editor();
        if editor.is_none() {
            self.toasts
                .push(Toast::warning("No $EDITOR or $VISUAL set"));
            return;
        }

        let current_content = self.input.content();

        if let Some(edited) = Self::open_in_editor(&current_content) {
            let trimmed = edited.trim();
            if !trimmed.is_empty() && trimmed != current_content.trim() {
                self.input.set_content(trimmed.to_string());
                self.toasts.push(Toast::success("Updated from editor"));
            }
        }
    }

    /// Update sidebar todos.
    pub fn set_todos(&mut self, todos: Vec<TodoItem>) {
        self.sidebar.set_todos(todos);
    }

    /// Update sidebar modified files.
    pub fn set_modified_files(&mut self, files: Vec<ModifiedFile>) {
        self.sidebar.set_modified_files(files);
    }

    /// Run the TUI.
    pub async fn run(&mut self) -> io::Result<()> {
        // Install panic hook to restore terminal on panic
        install_panic_hook();

        // Initialize performance metrics
        metrics::init();

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Start event loop
        let event_loop = self.events.start();

        // Focus input by default
        self.input.set_focused(true);

        // Main loop
        while self.state != AppState::Quit {
            // Only draw if needed (dirty flag set or animations active)
            let should_draw = self.needs_redraw || self.footer.is_busy();

            if should_draw {
                // Draw with timing
                {
                    let _frame_timer = metrics::frame_timer();
                    terminal.draw(|frame| self.draw(frame))?;
                }
                self.needs_redraw = false;
            }

            // Handle events with timeout for animations
            // Process all pending updates first, then handle one event
            while let Ok(update) = self.update_rx.try_recv() {
                let _timer = metrics::event_timer(EventType::Update);
                self.handle_update(update);
                self.needs_redraw = true;
            }

            // Then handle one event if available
            tokio::select! {
                biased;

                Some(event) = self.events.next() => {
                    // Track input start for latency measurement
                    let input_start = if matches!(event, Event::Key(_)) {
                        metrics::mark_input_start()
                    } else {
                        None
                    };

                    // Tick events don't need redraw unless animations are active
                    let is_tick = matches!(event, Event::Tick);
                    self.handle_event(event);

                    if !is_tick {
                        self.needs_redraw = true;
                    }

                    // Complete latency measurement after event + next frame
                    metrics::complete_input_latency(input_start);
                }
                // Also check for updates again in case more came in
                Some(update) = self.update_rx.recv() => {
                    let _timer = metrics::event_timer(EventType::Update);
                    self.handle_update(update);
                    self.needs_redraw = true;
                }
            }

            // Tick animations
            self.footer.tick();
        }

        // Log final metrics summary
        if let Some(summary) = metrics::summary() {
            tracing::info!(
                fps = %format!("{:.1}", summary.fps),
                avg_frame_ms = %format!("{:.2}", summary.avg_frame_ms),
                p99_frame_ms = %format!("{:.2}", summary.p99_frame_ms),
                slow_frames_pct = %format!("{:.1}", summary.slow_frame_pct),
                status = %summary.status(),
                "TUI performance summary"
            );
        }

        // Cleanup
        event_loop.abort();
        restore_terminal();

        Ok(())
    }

    /// Draw the UI.
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        match self.route {
            Route::Home => self.draw_home(frame, area),
            Route::Session => self.draw_session(frame, area),
        }

        // Draw toasts on top
        self.toasts.render(frame, area, &self.theme);

        // Draw active dialog on top
        self.draw_dialog(frame, area);

        // Draw onboarding overlay on top of everything
        if self.onboarding.is_visible() {
            self.onboarding.render(frame, area, &self.theme);
        }
    }

    /// Draw the home screen.
    fn draw_home(&mut self, frame: &mut Frame, area: Rect) {
        // Calculate input height based on content (same as session view)
        let content_width = (area.width * 70 / 100).clamp(50, 100);
        let input_height = self.input.height_for_width(content_width).min(10);

        // Vertical layout: flex space, logo, input, flex space, footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),               // Top spacing
                Constraint::Length(12),           // Logo (11 lines + 1 empty)
                Constraint::Length(1),            // Spacing
                Constraint::Length(input_height), // Input (dynamic)
                Constraint::Min(1),               // Bottom spacing
                Constraint::Length(1),            // Footer
            ])
            .split(area);

        // Center the content horizontally
        let center_input = centered_horizontal(content_width, chunks[3]);

        // Logo
        self.logo.render(frame, chunks[1], &self.theme);

        // Input
        self.input.render(frame, center_input, &self.theme);

        // Autocomplete popups (above input) - only show one at a time
        if self.slash_autocomplete.is_visible() {
            self.slash_autocomplete
                .render(frame, center_input, &self.theme);
        } else {
            self.autocomplete.render(frame, center_input, &self.theme);
        }

        // Tips/hints
        let tips = self.get_random_tip();
        let tip_line = Line::from(vec![
            Span::styled("Tip: ", self.theme.dim_style()),
            Span::styled(tips, self.theme.dim_style()),
        ]);
        let tip_para = Paragraph::new(tip_line).alignment(Alignment::Center);
        frame.render_widget(tip_para, chunks[4]);

        // Footer
        self.footer.render(frame, chunks[5], &self.theme);
    }

    /// Draw the session view.
    fn draw_session(&mut self, frame: &mut Frame, area: Rect) {
        // Main layout with optional sidebar (with margin between main and sidebar)
        let main_chunks = if self.sidebar.is_visible() {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(40),
                    Constraint::Length(1), // Margin/separator
                    Constraint::Length(self.sidebar.width()),
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(area)
        };

        // Main content area
        let main_area = main_chunks[0];

        // Calculate input height based on content
        let input_height = self.input.height().min(10);

        // Calculate search bar height
        let search_height = self.search.height();

        // Vertical layout: top padding, messages, search (optional), input, footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),             // Top padding
                Constraint::Min(5),                // Messages
                Constraint::Length(search_height), // Search bar (0 when inactive)
                Constraint::Length(input_height),  // Input
                Constraint::Length(1),             // Footer
            ])
            .split(main_area);

        // Cache areas for mouse click detection
        self.messages_area = chunks[1];
        self.input_area = chunks[3];

        // Messages (with horizontal padding)
        let messages_area = chunks[1];
        let padded_messages_area = Rect {
            x: messages_area.x + 2,
            y: messages_area.y,
            width: messages_area.width.saturating_sub(4),
            height: messages_area.height,
        };
        self.messages
            .render(frame, padded_messages_area, &self.theme);

        // Search bar (when active)
        if self.search.is_active() {
            self.search.render(frame, chunks[2], &self.theme);
        }

        // Input
        self.input.render(frame, chunks[3], &self.theme);

        // Autocomplete popups (above input) - only show one at a time
        if self.slash_autocomplete.is_visible() {
            self.slash_autocomplete
                .render(frame, chunks[3], &self.theme);
        } else {
            self.autocomplete.render(frame, chunks[3], &self.theme);
        }

        // Footer
        self.footer.render(frame, chunks[4], &self.theme);

        // Which-key overlay (on top of everything)
        if self.which_key.is_visible() {
            self.which_key.render(frame, main_area, &self.theme);
        }

        // Help overlay (on top of everything except dialogs)
        if self.help_overlay.is_visible() {
            self.help_overlay.render(frame, main_area, &self.theme);
        }

        // Sidebar (at index 2 because index 1 is the margin)
        if self.sidebar.is_visible() && main_chunks.len() > 2 {
            self.sidebar_area = main_chunks[2];
            self.sidebar.render(frame, main_chunks[2], &self.theme);
        } else {
            self.sidebar_area = Rect::default();
        }
    }

    /// Draw the active dialog.
    fn draw_dialog(&mut self, frame: &mut Frame, area: Rect) {
        match &self.dialog {
            ActiveDialog::None => {}
            ActiveDialog::CommandPalette => {
                self.command_palette.render(frame, area, &self.theme);
            }
            ActiveDialog::ModelSelect => {
                self.model_dialog.render(frame, area, &self.theme);
            }
            ActiveDialog::AgentSelect => {
                if let Some(dialog) = &mut self.agent_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::SessionList => {
                if let Some(dialog) = &mut self.session_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::ThemeSelect => {
                self.theme_dialog.render(frame, area, &self.theme);
            }
            ActiveDialog::Help => {
                self.help_dialog.render(frame, area, &self.theme);
            }
            ActiveDialog::Status => {
                self.status_dialog.render(frame, area, &self.theme);
            }
            ActiveDialog::Perf => {
                if let Some(dialog) = &self.perf_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Rename => {
                if let Some(dialog) = &self.input_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Mcp => {
                if let Some(dialog) = &mut self.mcp_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Timeline => {
                if let Some(dialog) = &mut self.timeline_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Sandbox => {
                if let Some(dialog) = &self.sandbox_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Settings => {
                if let Some(dialog) = &mut self.settings_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Permission => {
                if let Some(dialog) = &self.permission_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
            ActiveDialog::Git => {
                if let Some(dialog) = &mut self.git_dialog {
                    dialog.render(frame, area, &self.theme);
                }
            }
        }
    }

    /// Handle an event.
    fn handle_event(&mut self, event: Event) {
        // Time event handling by type
        let event_type = match &event {
            Event::Key(_) => Some(EventType::Key),
            Event::Mouse(_) => Some(EventType::Mouse),
            Event::Resize(_, _) => Some(EventType::Resize),
            Event::Tick => Some(EventType::Tick),
            _ => None,
        };
        let _event_timer = event_type.and_then(metrics::event_timer);

        match event {
            Event::Key(key) => {
                // Ctrl+C is now only used as part of Ctrl+X Ctrl+C sequence (handled in leader key)
                // Single Ctrl+C does nothing to prevent accidental exits

                // Dismiss onboarding overlay on any key
                if self.onboarding.is_visible() {
                    self.onboarding.hide();
                    return; // Don't process the key further
                }

                // Dismiss help overlay on any key except ?
                if self.help_overlay.is_visible() && key.code != KeyCode::Char('?') {
                    self.help_overlay.hide();
                    // Don't return - still process the key
                }

                // Handle dialog first
                if self.dialog != ActiveDialog::None {
                    self.handle_dialog_key(key);
                    return;
                }

                // Leader key handling
                if self.state == AppState::Leader {
                    self.handle_leader_key(key);
                    return;
                }

                match self.state {
                    AppState::Input => {
                        // Handle slash command autocomplete first if visible
                        if self.slash_autocomplete.is_visible() {
                            match self.slash_autocomplete.handle_key(key) {
                                SlashCommandAction::Execute(cmd) => {
                                    self.input.clear();
                                    self.execute_slash_command(&cmd);
                                    return;
                                }
                                SlashCommandAction::Handled => {
                                    return;
                                }
                                SlashCommandAction::None => {
                                    // Not handled, continue to normal input handling
                                }
                            }
                        }

                        // Handle file autocomplete if visible
                        if self.autocomplete.is_visible() {
                            match self.autocomplete.handle_key(key) {
                                AutocompleteAction::Select(path) => {
                                    // Replace @filter with @path
                                    self.apply_autocomplete(&path);
                                    return;
                                }
                                AutocompleteAction::Handled => {
                                    return;
                                }
                                AutocompleteAction::None => {
                                    // Not handled, continue to normal input handling
                                }
                            }
                        }

                        let action = self.input.handle_key(key);

                        // Check for autocomplete triggers after input handling
                        self.check_autocomplete_trigger();

                        match action {
                            InputAction::Submit => {
                                // Check if this is a slash command
                                let text = self.input.content();
                                if let Some(full_cmd) = text.strip_prefix('/') {
                                    let full_cmd = full_cmd.to_string();
                                    // Use take() instead of clear() to store command in history
                                    let _ = self.input.take();
                                    self.slash_autocomplete.hide();
                                    self.execute_slash_command(&full_cmd);
                                    return;
                                }

                                self.autocomplete.hide();
                                self.slash_autocomplete.hide();
                                let text = self.input.take();
                                if !text.is_empty() {
                                    // Commit any pending revert (discard undone messages)
                                    self.messages.commit_revert();

                                    self.add_user_message(text.clone());
                                    self.set_state(AppState::Waiting);
                                    self.footer.set_status(FooterStatus::Thinking);
                                    self.messages.start_streaming();
                                    let _ = self.action_tx.send(AppAction::SendPrompt(text));
                                }
                            }
                            InputAction::CommandPalette => {
                                self.autocomplete.hide();
                                self.slash_autocomplete.hide();
                                self.dialog = ActiveDialog::CommandPalette;
                                self.command_palette = CommandPalette::new();
                            }
                            InputAction::LeaderKey => {
                                self.autocomplete.hide();
                                self.slash_autocomplete.hide();
                                self.set_state(AppState::Leader);
                                self.which_key.show();
                            }
                            InputAction::Escape => {
                                if self.slash_autocomplete.is_visible() {
                                    self.slash_autocomplete.hide();
                                } else if self.autocomplete.is_visible() {
                                    self.autocomplete.hide();
                                } else if self.footer.is_busy() {
                                    // If LLM is running, cancel the operation
                                    let _ = self.action_tx.send(AppAction::Cancel);
                                    self.toasts.push(Toast::warning("Cancelling..."));
                                } else if self.route == Route::Session {
                                    self.set_state(AppState::Scrolling);
                                    self.input.set_focused(false);
                                    self.messages.set_focused(true);
                                }
                            }
                            InputAction::ScrollUp => {
                                self.autocomplete.hide();
                                self.slash_autocomplete.hide();
                                self.set_state(AppState::Scrolling);
                                self.input.set_focused(false);
                                self.messages.set_focused(true);
                            }
                            InputAction::Cancel => {
                                self.autocomplete.hide();
                                self.slash_autocomplete.hide();
                                if self.state == AppState::Waiting {
                                    let _ = self.action_tx.send(AppAction::Cancel);
                                }
                            }
                            InputAction::Paste => {
                                self.autocomplete.hide();
                                self.slash_autocomplete.hide();
                                // Try to paste from clipboard
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    if let Ok(text) = clipboard.get_text() {
                                        let line_count = text.lines().count();
                                        tracing::debug!(
                                            "Ctrl+V paste: {} lines, {} bytes",
                                            line_count,
                                            text.len()
                                        );
                                        if line_count >= 2 {
                                            self.toasts.push(Toast::info(format!(
                                                "Pasted {line_count} lines"
                                            )));
                                        }
                                        self.input.insert_paste(&text);
                                    }
                                }
                            }
                            _ => {}
                        }

                        // Handle ? for help - show context-sensitive help overlay
                        if key.code == KeyCode::Char('?')
                            && !key.modifiers.contains(KeyModifiers::SHIFT)
                            && self.input.is_empty()
                        {
                            self.autocomplete.hide();
                            self.help_overlay.toggle(HelpContext::Input);
                        }
                    }
                    AppState::Scrolling => {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                self.messages.scroll_up(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                self.messages.scroll_down(1);
                            }
                            KeyCode::PageUp => {
                                self.messages.scroll_up(10);
                            }
                            KeyCode::PageDown => {
                                self.messages.scroll_down(10);
                            }
                            KeyCode::Home | KeyCode::Char('g') => {
                                self.messages.scroll_up(usize::MAX);
                            }
                            KeyCode::End | KeyCode::Char('G') => {
                                self.messages.scroll_to_bottom();
                            }
                            KeyCode::Char('y') => {
                                // Copy last response while in scroll mode
                                self.copy_last_response();
                            }
                            KeyCode::Char('v') => {
                                // Enter selection mode
                                self.messages.enter_selection_mode();
                                self.set_state(AppState::Selecting);
                                self.toasts.push(Toast::info("Selection mode: j/k to navigate, y to copy, o to expand, Esc to exit"));
                            }
                            KeyCode::Char('o') => {
                                // Toggle tool output expansion - expand/collapse all tools in last message
                                let msg_count = self.messages.message_count();
                                if msg_count > 0 {
                                    self.messages.toggle_tool_expansion(msg_count - 1);
                                }
                            }
                            KeyCode::Char('/') => {
                                // Enter search mode
                                self.search.activate();
                                self.set_state(AppState::Searching);
                            }
                            KeyCode::Char('i') | KeyCode::Enter => {
                                self.set_state(AppState::Input);
                                self.input.set_focused(true);
                                self.messages.set_focused(false);
                            }
                            KeyCode::Char('?') => {
                                // Show context-sensitive help
                                self.help_overlay.toggle(HelpContext::Scroll);
                            }
                            _ if is_escape(&key) => {
                                // If LLM is running, cancel the operation
                                if self.footer.is_busy() {
                                    let _ = self.action_tx.send(AppAction::Cancel);
                                    self.toasts.push(Toast::warning("Cancelling..."));
                                } else {
                                    self.set_state(AppState::Input);
                                    self.input.set_focused(true);
                                    self.messages.set_focused(false);
                                }
                            }
                            _ => {}
                        }
                    }
                    AppState::Selecting => {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                self.messages.select_prev_message();
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                self.messages.select_next_message();
                            }
                            KeyCode::Char('y') => {
                                // Copy selected message
                                if let Some(content) = self.messages.get_selected_content() {
                                    if self.copy_to_clipboard(&content) {
                                        self.toasts.push(Toast::success("Copied to clipboard"));
                                    } else {
                                        self.toasts.push(Toast::error("Failed to copy"));
                                    }
                                }
                                // Exit selection mode after copy
                                self.messages.exit_selection_mode();
                                self.set_state(AppState::Scrolling);
                            }
                            KeyCode::Enter => {
                                // Copy and stay in selection mode
                                if let Some(content) = self.messages.get_selected_content() {
                                    if self.copy_to_clipboard(&content) {
                                        self.toasts.push(Toast::success("Copied to clipboard"));
                                    } else {
                                        self.toasts.push(Toast::error("Failed to copy"));
                                    }
                                }
                            }
                            KeyCode::Char('o') => {
                                // Toggle tool output expansion for selected message
                                self.messages.toggle_selected_tool_expansion();
                            }
                            _ if is_escape(&key) => {
                                // If LLM is running, cancel the operation
                                if self.footer.is_busy() {
                                    let _ = self.action_tx.send(AppAction::Cancel);
                                    self.toasts.push(Toast::warning("Cancelling..."));
                                }
                                // Exit selection mode
                                self.messages.exit_selection_mode();
                                self.set_state(AppState::Scrolling);
                            }
                            _ => {}
                        }
                    }
                    AppState::Searching => {
                        match key.code {
                            KeyCode::Esc => {
                                // If LLM is running, cancel the operation
                                if self.footer.is_busy() {
                                    let _ = self.action_tx.send(AppAction::Cancel);
                                    self.toasts.push(Toast::warning("Cancelling..."));
                                }
                                // Exit search mode
                                self.search.deactivate();
                                self.set_state(AppState::Scrolling);
                            }
                            KeyCode::Enter => {
                                // Go to current match and exit search
                                if let Some(m) = self.search.current_match() {
                                    // Scroll to the message
                                    self.messages.scroll_to_message(m.message_index);
                                }
                                self.search.deactivate();
                                self.set_state(AppState::Scrolling);
                            }
                            KeyCode::Char('n') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                                self.search.next_match();
                                // Also scroll to the match
                                if let Some(m) = self.search.current_match() {
                                    self.messages.scroll_to_message(m.message_index);
                                }
                            }
                            KeyCode::Char('N') | KeyCode::Char('n')
                                if key.modifiers.contains(KeyModifiers::SHIFT) =>
                            {
                                self.search.prev_match();
                                if let Some(m) = self.search.current_match() {
                                    self.messages.scroll_to_message(m.message_index);
                                }
                            }
                            KeyCode::Backspace => {
                                self.search.delete_char();
                                self.perform_search();
                            }
                            KeyCode::Delete => {
                                self.search.delete_char_forward();
                                self.perform_search();
                            }
                            KeyCode::Left => {
                                self.search.cursor_left();
                            }
                            KeyCode::Right => {
                                self.search.cursor_right();
                            }
                            KeyCode::Home => {
                                self.search.cursor_start();
                            }
                            KeyCode::End => {
                                self.search.cursor_end();
                            }
                            KeyCode::Char(c) => {
                                self.search.insert_char(c);
                                self.perform_search();
                            }
                            _ => {}
                        }
                    }
                    AppState::Waiting => {
                        if is_escape(&key) {
                            let _ = self.action_tx.send(AppAction::Cancel);
                            self.toasts.push(Toast::warning("Cancelling..."));
                        }
                    }
                    AppState::Leader | AppState::Quit => {}
                }
            }
            Event::Resize(_, _) => {
                // Terminal will handle resize
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
            }
            Event::Tick => {
                // Update animations
                self.footer.tick();
                // Check help overlay auto-dismiss
                self.help_overlay.tick();
                // Check for pending paste that needs to be finalized
                if self.input.check_pending_paste() {
                    self.needs_redraw = true;
                }
            }
            Event::Paste(text) => {
                // Handle bracketed paste - insert text into input
                // The input widget handles both single-line and multi-line pastes,
                // including tracking for terminals that send paste line-by-line
                let line_count = text.lines().count();
                tracing::info!(
                    "Event::Paste received: {} lines, {} bytes, state={:?}",
                    line_count,
                    text.len(),
                    self.state
                );

                if self.state == AppState::Input || self.state == AppState::Scrolling {
                    self.autocomplete.hide();
                    self.slash_autocomplete.hide();

                    // insert_paste handles both multi-line wrapping and single-line tracking
                    self.input.insert_paste(&text);

                    tracing::info!("After insert_paste: raw_len={}", self.input.content().len());

                    self.set_state(AppState::Input);
                    self.input.set_focused(true);
                    self.messages.set_focused(false);
                }
            }
            Event::Message(_) | Event::Status(_) | Event::Error(_) => {
                // Legacy events - handled via update channel now
            }
        }
    }

    /// Handle mouse events.
    /// Note: Hold Shift while clicking/dragging to use native terminal text selection.
    /// Most modern terminals (iTerm2, Alacritty, Kitty, etc.) support this.
    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        use crossterm::event::MouseEventKind;

        // When Shift is held, skip our mouse handling to allow native terminal
        // text selection to work. This is the standard way to enable text selection
        // in TUI applications that capture mouse events.
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.route == Route::Session {
                    let x = mouse.column;
                    let y = mouse.row;

                    // Check if scroll is in sidebar area
                    if self.sidebar.is_visible()
                        && x >= self.sidebar_area.x
                        && x < self.sidebar_area.x + self.sidebar_area.width
                        && y >= self.sidebar_area.y
                        && y < self.sidebar_area.y + self.sidebar_area.height
                    {
                        self.sidebar.handle_scroll(true, self.sidebar_area);
                    } else {
                        // Scroll messages up
                        self.messages.scroll_up(3);
                        // Switch to scrolling mode if in input mode
                        if self.state == AppState::Input {
                            self.set_state(AppState::Scrolling);
                            self.input.set_focused(false);
                            self.messages.set_focused(true);
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if self.route == Route::Session {
                    let x = mouse.column;
                    let y = mouse.row;

                    // Check if scroll is in sidebar area
                    if self.sidebar.is_visible()
                        && x >= self.sidebar_area.x
                        && x < self.sidebar_area.x + self.sidebar_area.width
                        && y >= self.sidebar_area.y
                        && y < self.sidebar_area.y + self.sidebar_area.height
                    {
                        self.sidebar.handle_scroll(false, self.sidebar_area);
                    } else {
                        // Scroll messages down
                        self.messages.scroll_down(3);
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                // Handle click to focus
                if self.route == Route::Session && self.dialog == ActiveDialog::None {
                    let x = mouse.column;
                    let y = mouse.row;

                    // Check if click is in sidebar (section header toggle)
                    if self.sidebar.is_visible()
                        && self.sidebar.handle_click(x, y, self.sidebar_area)
                    {
                        // Section was toggled, nothing else to do
                        return;
                    }

                    // Check if click is in input area
                    if x >= self.input_area.x
                        && x < self.input_area.x + self.input_area.width
                        && y >= self.input_area.y
                        && y < self.input_area.y + self.input_area.height
                    {
                        // Focus input
                        self.set_state(AppState::Input);
                        self.input.set_focused(true);
                        self.messages.set_focused(false);
                    }
                    // Check if click is in messages area
                    else if x >= self.messages_area.x
                        && x < self.messages_area.x + self.messages_area.width
                        && y >= self.messages_area.y
                        && y < self.messages_area.y + self.messages_area.height
                    {
                        // Check if clicking on a code block
                        if let Some(code_content) = self.messages.handle_click(x, y) {
                            // Copy code to clipboard
                            if self.copy_to_clipboard(&code_content) {
                                self.toasts.push(Toast::success("Code copied to clipboard"));
                            } else {
                                self.toasts
                                    .push(Toast::error("Failed to copy to clipboard"));
                            }
                        } else {
                            // Focus messages (scroll mode)
                            self.set_state(AppState::Scrolling);
                            self.input.set_focused(false);
                            self.messages.set_focused(true);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle dialog key events.
    fn handle_dialog_key(&mut self, key: crossterm::event::KeyEvent) {
        if is_escape(&key) {
            self.dialog = ActiveDialog::None;
            return;
        }

        match &mut self.dialog {
            ActiveDialog::CommandPalette => {
                if let Some(id) = self.command_palette.handle_key(key) {
                    self.dialog = ActiveDialog::None;
                    self.execute_command(&id);
                }
            }
            ActiveDialog::ModelSelect => {
                if let Some(id) = self.model_dialog.handle_key(key) {
                    self.dialog = ActiveDialog::None;
                    let _ = self.action_tx.send(AppAction::ChangeModel(id.clone()));
                    self.set_model(&id);
                    self.toasts.push(Toast::success(format!("Model: {id}")));
                }
            }
            ActiveDialog::AgentSelect => {
                if let Some(dialog) = &mut self.agent_dialog {
                    if let Some(id) = dialog.handle_key(key) {
                        self.dialog = ActiveDialog::None;
                        let _ = self.action_tx.send(AppAction::ChangeAgent(id.clone()));
                        self.set_agent(&id);
                        self.toasts.push(Toast::success(format!("Agent: {id}")));
                    }
                }
            }
            ActiveDialog::SessionList => {
                if let Some(dialog) = &mut self.session_dialog {
                    if let Some(id) = dialog.handle_key(key) {
                        self.dialog = ActiveDialog::None;
                        let _ = self.action_tx.send(AppAction::SwitchSession(id));
                    }
                }
            }
            ActiveDialog::ThemeSelect => {
                if let Some(id) = self.theme_dialog.handle_key(key) {
                    self.dialog = ActiveDialog::None;
                    self.set_theme(&id);
                }
            }
            ActiveDialog::Rename => {
                if let Some(dialog) = &mut self.input_dialog {
                    if let Some(result) = dialog.handle_key(key) {
                        self.dialog = ActiveDialog::None;
                        match result {
                            InputDialogResult::Submit(new_title) => {
                                if !new_title.is_empty() {
                                    self.session_title = new_title.clone();
                                    self.sidebar.set_session_title(&new_title);
                                    self.topbar.set_session_title(Some(new_title.clone()));
                                    self.toasts
                                        .push(Toast::success(format!("Renamed to: {new_title}")));
                                    // Send action to persist the rename
                                    let _ = self
                                        .action_tx
                                        .send(AppAction::RenameSession { title: new_title });
                                }
                            }
                            InputDialogResult::Cancel => {}
                        }
                        self.input_dialog = None;
                    }
                }
            }
            ActiveDialog::Mcp => {
                if let Some(dialog) = &mut self.mcp_dialog {
                    if let Some(action) = dialog.handle_key(key) {
                        if action == "close" {
                            self.dialog = ActiveDialog::None;
                            self.mcp_dialog = None;
                        } else if let Some(name) = action.strip_prefix("toggle:") {
                            self.toasts
                                .push(Toast::info(format!("Toggle MCP server: {name}")));
                            let _ = self.action_tx.send(AppAction::McpToggle {
                                name: name.to_string(),
                            });
                        } else if let Some(name) = action.strip_prefix("reconnect:") {
                            self.toasts
                                .push(Toast::info(format!("Reconnecting: {name}")));
                            let _ = self.action_tx.send(AppAction::McpReconnect {
                                name: name.to_string(),
                            });
                        }
                    }
                }
            }
            ActiveDialog::Timeline => {
                if let Some(dialog) = &mut self.timeline_dialog {
                    if let Some(action) = dialog.handle_key(key) {
                        self.dialog = ActiveDialog::None;
                        self.timeline_dialog = None;

                        if let Some(msg_id) = action.strip_prefix("goto:") {
                            // Navigate to the message (scroll to it)
                            if let Ok(idx) = msg_id.parse::<usize>() {
                                // Scroll to message index
                                self.messages.scroll_to_bottom();
                                self.toasts
                                    .push(Toast::info(format!("Jumped to message {}", idx + 1)));
                            }
                        } else if let Some(msg_id) = action.strip_prefix("fork:") {
                            self.toasts
                                .push(Toast::info(format!("Forking from message {msg_id}...")));
                            let _ = self.action_tx.send(AppAction::ForkSession {
                                message_id: Some(msg_id.to_string()),
                            });
                        }
                    }
                }
            }
            ActiveDialog::Sandbox => {
                if let Some(dialog) = &mut self.sandbox_dialog {
                    if let Some(action) = dialog.handle_key(key) {
                        self.handle_sandbox_action(action);
                    }
                }
            }
            ActiveDialog::Settings => {
                if let Some(dialog) = &mut self.settings_dialog {
                    let result = dialog.handle_key(key);
                    self.handle_settings_result(result);
                }
            }
            ActiveDialog::Perf => {
                if let Some(dialog) = &mut self.perf_dialog {
                    if dialog.handle_key(key) {
                        self.dialog = ActiveDialog::None;
                        self.perf_dialog = None;
                    }
                }
            }
            ActiveDialog::Permission => {
                if let Some(dialog) = &mut self.permission_dialog {
                    if let Some(result) = dialog.handle_key(key) {
                        self.handle_permission_result(result);
                    }
                }
            }
            ActiveDialog::Git => {
                if let Some(dialog) = &mut self.git_dialog {
                    let result = dialog.handle_key(key);
                    self.handle_git_dialog_result(result);
                }
            }
            ActiveDialog::Help | ActiveDialog::Status | ActiveDialog::None => {
                // These dialogs close on any key press (already handled escape above)
            }
        }
    }

    /// Handle leader key sequences (Ctrl+X followed by another key).
    fn handle_leader_key(&mut self, key: crossterm::event::KeyEvent) {
        self.set_state(AppState::Input);

        match key.code {
            KeyCode::Char('n') | KeyCode::Char('N') => {
                // New session
                let _ = self.action_tx.send(AppAction::NewSession);
                self.messages = MessagesWidget::with_render_settings(self.render_settings.clone());
                self.route = Route::Home;
                self.toasts.push(Toast::info("New session"));
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                // Session list
                self.session_dialog = Some(SessionDialog::new(self.sessions.clone()));
                self.dialog = ActiveDialog::SessionList;
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                // Model select
                self.model_dialog =
                    ModelDialog::with_options(self.render_settings.test_model_enabled);
                self.dialog = ActiveDialog::ModelSelect;
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                // Agent select
                self.agent_dialog = Some(AgentDialog::new(self.available_agents.clone()));
                self.dialog = ActiveDialog::AgentSelect;
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                // Toggle sidebar
                self.sidebar.toggle();
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                // Git dialog
                self.show_git_dialog();
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                // Theme select
                self.theme_dialog = ThemeDialog::new();
                self.dialog = ActiveDialog::ThemeSelect;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Copy last response
                self.copy_last_response();
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                // Open input in external editor
                self.edit_input_in_editor();
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                // Export session
                self.export_session();
            }
            KeyCode::Char('u') | KeyCode::Char('U') => {
                // Undo last message
                self.undo_message();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                // Redo undone message
                self.redo_message();
            }
            KeyCode::Char('?') => {
                // Help
                self.dialog = ActiveDialog::Help;
            }
            KeyCode::Char('s') | KeyCode::Char('S')
                if !key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                // Settings
                self.show_settings_dialog();
            }
            KeyCode::Char('c') | KeyCode::Char('C')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                // Ctrl+X Ctrl+C: Exit application
                self.set_state(AppState::Quit);
                let _ = self.action_tx.send(AppAction::Quit);
            }
            _ => {
                // Unknown leader key
            }
        }
    }

    /// Execute a slash command by name.
    fn execute_slash_command(&mut self, full_command: &str) {
        // Parse command (arguments are not used for most commands anymore)
        let mut parts = full_command.split_whitespace();
        let command = parts.next().unwrap_or("");

        // Map slash command names to internal command IDs
        let internal_command = match command {
            // Session commands
            "new" | "clear" => "new_session",
            "sessions" | "session" | "resume" | "continue" => "session_list",
            "undo" => "undo",
            "redo" => "redo",
            "copy" => "copy_last",
            // Navigation commands
            "models" => "model_select",
            "agents" | "agent" => "agent_select",
            "theme" => "theme_select",
            // UI commands
            "editor" => "edit_input",
            "sidebar" => "toggle_sidebar",
            "help" | "commands" => "help",
            "quit" | "exit" | "q" => "quit",
            // Not yet implemented commands
            "compact" | "summarize" => {
                let _ = self.action_tx.send(AppAction::Compact);
                self.toasts.push(Toast::info("Compacting conversation..."));
                return;
            }
            "rename" => {
                let current_title = if self.session_title.is_empty() {
                    "Untitled".to_string()
                } else {
                    self.session_title.clone()
                };
                self.input_dialog = Some(
                    InputDialog::new("Rename Session", "New session name:")
                        .with_value(current_title),
                );
                self.dialog = ActiveDialog::Rename;
                return;
            }
            "export" => {
                self.export_session();
                return;
            }
            "timeline" => {
                self.show_timeline_dialog();
                return;
            }
            "settings" | "config" | "preferences" => {
                self.show_settings_dialog();
                return;
            }
            "fork" => {
                // Fork from current position (no specific message)
                let _ = self
                    .action_tx
                    .send(AppAction::ForkSession { message_id: None });
                self.toasts.push(Toast::info("Forking session..."));
                return;
            }
            "thinking" => {
                self.show_thinking = !self.show_thinking;
                self.messages.set_show_thinking(self.show_thinking);
                let status = if self.show_thinking {
                    "visible"
                } else {
                    "hidden"
                };
                self.toasts
                    .push(Toast::info(format!("Thinking blocks: {status}")));
                return;
            }
            "share" => {
                let _ = self.action_tx.send(AppAction::ShareSession);
                self.toasts.push(Toast::info("Sharing session..."));
                return;
            }
            "unshare" => {
                let _ = self.action_tx.send(AppAction::UnshareSession);
                self.toasts.push(Toast::info("Unsharing session..."));
                return;
            }
            "status" => {
                self.update_status_dialog();
                self.dialog = ActiveDialog::Status;
                return;
            }
            "perf" => {
                if self.render_settings.enable_test_commands {
                    self.show_perf_metrics();
                } else {
                    self.toasts.push(Toast::warning(
                        "Test commands are disabled. Enable in Settings > Performance",
                    ));
                }
                return;
            }
            "mcp" => {
                self.show_mcp_dialog();
                return;
            }
            "connect" => {
                // Show a simple dialog to select provider for connection
                // For now, just show the model dialog as a starting point
                self.model_dialog =
                    ModelDialog::with_options(self.render_settings.test_model_enabled);
                self.dialog = ActiveDialog::ModelSelect;
                self.toasts
                    .push(Toast::info("Select a model to connect to a provider"));
                return;
            }
            "sandbox" => {
                self.show_sandbox_dialog();
                return;
            }
            "git" => {
                self.show_git_dialog();
                return;
            }
            "add_test_messages" => {
                if self.render_settings.enable_test_commands {
                    self.add_test_messages();
                } else {
                    self.toasts.push(Toast::warning(
                        "Test commands are disabled. Enable in Settings > Performance",
                    ));
                }
                return;
            }
            _ => {
                self.toasts
                    .push(Toast::warning(format!("Unknown command: /{command}")));
                return;
            }
        };
        self.execute_command(internal_command);
    }

    /// Execute a command from the palette.
    fn execute_command(&mut self, command: &str) {
        match command {
            "new_session" => {
                let _ = self.action_tx.send(AppAction::NewSession);
                self.messages = MessagesWidget::with_render_settings(self.render_settings.clone());
                self.route = Route::Home;
            }
            "session_list" => {
                self.session_dialog = Some(SessionDialog::new(self.sessions.clone()));
                self.dialog = ActiveDialog::SessionList;
            }
            "model_select" => {
                self.model_dialog =
                    ModelDialog::with_options(self.render_settings.test_model_enabled);
                self.dialog = ActiveDialog::ModelSelect;
            }
            "agent_select" => {
                self.agent_dialog = Some(AgentDialog::new(self.available_agents.clone()));
                self.dialog = ActiveDialog::AgentSelect;
            }
            "toggle_sidebar" => {
                self.sidebar.toggle();
            }
            "theme_select" => {
                self.theme_dialog = ThemeDialog::new();
                self.dialog = ActiveDialog::ThemeSelect;
            }
            "copy_last" => {
                self.copy_last_response();
            }
            "edit_input" => {
                self.edit_input_in_editor();
            }
            "undo" => {
                self.undo_message();
            }
            "redo" => {
                self.redo_message();
            }
            "clear_history" => {
                self.messages = MessagesWidget::with_render_settings(self.render_settings.clone());
                self.route = Route::Home;
                self.toasts.push(Toast::info("History cleared"));
            }
            "export_session" => {
                self.export_session();
            }
            "sandbox" => {
                self.show_sandbox_dialog();
            }
            "mcp_servers" => {
                self.dialog = ActiveDialog::Mcp;
            }
            "help" => {
                self.dialog = ActiveDialog::Help;
            }
            "quit" => {
                self.set_state(AppState::Quit);
                let _ = self.action_tx.send(AppAction::Quit);
            }
            _ => {}
        }
    }

    /// Handle an update from the runner.
    fn handle_update(&mut self, update: AppUpdate) {
        match update {
            AppUpdate::Started => {
                self.footer.set_status(FooterStatus::Thinking);
                self.messages.start_streaming();
            }
            AppUpdate::TextDelta(text) => {
                self.messages.append_streaming(&text);
            }
            AppUpdate::ToolStarted { name, id, input } => {
                // Normalize MCP tool names: mcp__wonopcode-tools__bash -> bash
                let display_name = normalize_tool_name(&name);
                self.footer
                    .set_status(FooterStatus::Running(format!("Running: {display_name}")));
                // Add tool call to messages for rendering
                self.messages
                    .add_tool_call_with_input(id, display_name, input);
            }
            AppUpdate::ToolCompleted {
                id,
                success,
                output,
                metadata,
            } => {
                // Update tool status in messages
                let status = if success {
                    ToolStatus::Success
                } else {
                    ToolStatus::Error
                };
                self.messages
                    .update_tool_status_with_metadata(&id, status, Some(output), metadata);

                if success {
                    self.footer.set_status(FooterStatus::Thinking);
                }
            }
            AppUpdate::Completed { text: _ } => {
                // Use atomic end_streaming_and_add_message to avoid flicker
                let mut msg = DisplayMessage::assistant("");
                msg.agent = self.current_agent();
                msg.model = Some(format!("{}/{}", self.provider, self.model));
                self.messages.end_streaming_and_add_message(msg);

                self.set_state(AppState::Input);
                self.footer.set_status(FooterStatus::Idle);
                self.input.set_focused(true);
            }
            AppUpdate::Error(err) => {
                let _ = self.messages.end_streaming_legacy();
                self.set_state(AppState::Input);
                self.footer.set_status(FooterStatus::Error(err.clone()));
                self.input.set_focused(true);
                self.toasts.push(Toast::error("Error").with_message(err));
            }
            AppUpdate::Status(status) => {
                self.footer.set_status(FooterStatus::Running(status));
            }
            AppUpdate::TokenUsage {
                input,
                output,
                cost,
                context_limit,
            } => {
                self.footer.set_tokens(input, output);
                self.sidebar.update_tokens(input, output);
                self.sidebar.set_cost(cost);
                if context_limit > 0 {
                    self.sidebar.set_max_tokens(context_limit);
                }
            }
            AppUpdate::ModelInfo { context_limit } => {
                self.sidebar.set_max_tokens(context_limit);
            }
            AppUpdate::Sessions(sessions) => {
                self.sessions = sessions;
            }
            AppUpdate::TodosUpdated(todos) => {
                // Convert TodoUpdate to sidebar::TodoItem
                let sidebar_todos: Vec<TodoItem> = todos
                    .into_iter()
                    .map(|t| TodoItem {
                        content: t.content,
                        completed: t.status == "completed",
                        in_progress: t.status == "in_progress",
                    })
                    .collect();
                self.sidebar.set_todos(sidebar_todos);
            }
            AppUpdate::LspUpdated(servers) => {
                use crate::widgets::sidebar::LspServerStatus;
                let lsp_statuses: Vec<LspStatus> = servers
                    .into_iter()
                    .map(|s| LspStatus {
                        id: s.id,
                        name: s.name,
                        root: s.root,
                        status: if s.connected {
                            LspServerStatus::Connected
                        } else {
                            LspServerStatus::Failed
                        },
                    })
                    .collect();
                // Update footer with LSP count
                self.footer.set_lsp_count(
                    lsp_statuses
                        .iter()
                        .filter(|s| s.status == LspServerStatus::Connected)
                        .count(),
                );
                self.sidebar.set_lsp_servers(lsp_statuses);
            }
            AppUpdate::McpUpdated(servers) => {
                let mcp_statuses: Vec<McpStatus> = servers
                    .into_iter()
                    .map(|s| McpStatus {
                        name: s.name,
                        status: if s.connected {
                            McpServerStatus::Connected
                        } else if s.error.is_some() {
                            McpServerStatus::Failed
                        } else {
                            McpServerStatus::Disabled
                        },
                        error: s.error,
                    })
                    .collect();
                // Update footer with MCP status
                let connected_count = mcp_statuses
                    .iter()
                    .filter(|s| matches!(s.status, McpServerStatus::Connected))
                    .count();
                let has_error = mcp_statuses
                    .iter()
                    .any(|s| matches!(s.status, McpServerStatus::Failed));
                self.footer.set_mcp_status(connected_count, has_error);
                self.sidebar.set_mcp_servers(mcp_statuses);
            }
            AppUpdate::ModifiedFilesUpdated(files) => {
                // Merge incremental updates instead of replacing
                for f in files {
                    self.sidebar.add_modified_file(f.path, f.added, f.removed);
                }
            }
            AppUpdate::PermissionsPending(count) => {
                self.footer.set_pending_permissions(count);
            }
            AppUpdate::SandboxUpdated(status) => {
                use crate::widgets::footer::SandboxDisplayState;
                let state = match status.state.as_str() {
                    "running" => SandboxDisplayState::Running,
                    "starting" => SandboxDisplayState::Starting,
                    "error" => SandboxDisplayState::Error,
                    "stopped" => SandboxDisplayState::Stopped,
                    "disabled" => SandboxDisplayState::Disabled,
                    _ => SandboxDisplayState::Disabled,
                };
                self.footer.set_sandbox_status(state, status.runtime_type);

                // Show toast for state changes
                match state {
                    SandboxDisplayState::Running => {
                        self.toasts.push(Toast::success("Sandbox is running"));
                    }
                    SandboxDisplayState::Error => {
                        let msg = status.error.unwrap_or_else(|| "Unknown error".to_string());
                        self.toasts
                            .push(Toast::error(format!("Sandbox error: {msg}")));
                    }
                    _ => {}
                }
            }
            AppUpdate::SystemMessage(msg) => {
                use crate::widgets::messages::DisplayMessage;
                self.messages.add_message(DisplayMessage::system(msg));
            }
            AppUpdate::AgentChanged(agent) => {
                self.set_agent(&agent);
                // Show toast for agent change
                let mode_name = if agent == "plan" {
                    "Plan Mode (read-only)"
                } else {
                    "Build Mode (full access)"
                };
                self.toasts
                    .push(Toast::info(format!("Switched to {mode_name}")));
            }
            AppUpdate::PermissionRequest(req) => {
                // If a permission dialog is already showing, queue this request
                if self.permission_dialog.is_some() {
                    self.permission_queue.push_back(req);
                } else {
                    // Show permission dialog
                    self.permission_dialog = Some(PermissionDialog::new(
                        req.id,
                        req.tool,
                        req.action,
                        req.description,
                        req.path,
                    ));
                    self.dialog = ActiveDialog::Permission;
                }
                // Update pending count (current dialog + queue size)
                let pending_count = 1 + self.permission_queue.len();
                self.footer.set_pending_permissions(pending_count);
            }
            AppUpdate::SessionLoaded {
                id,
                title,
                messages,
            } => {
                // Load a session with its messages (from remote server)
                tracing::info!(session_id = %id, title = %title, message_count = messages.len(), "Loading session");
                self.messages.set_messages(messages);
                self.session_title = title;
                // Move to session view if we have messages
                if self.messages.message_count() > 0 {
                    self.route = Route::Session;
                }
            }
            AppUpdate::GitStatusUpdated(status) => {
                if let Some(dialog) = &mut self.git_dialog {
                    let files: Vec<GitFileDisplay> = status
                        .files
                        .iter()
                        .map(|f| GitFileDisplay {
                            path: f.path.clone(),
                            status: f.status.clone(),
                            staged: f.staged,
                        })
                        .collect();
                    dialog.set_status(status.branch, status.ahead, status.behind, files);
                }
            }
            AppUpdate::GitHistoryUpdated(commits) => {
                if let Some(dialog) = &mut self.git_dialog {
                    let commits: Vec<GitCommitDisplay> = commits
                        .iter()
                        .map(|c| GitCommitDisplay {
                            id: c.id.clone(),
                            message: c.message.clone(),
                            author: c.author.clone(),
                            date: c.date.clone(),
                        })
                        .collect();
                    dialog.set_history(commits);
                }
            }
            AppUpdate::GitOperationResult { success, message } => {
                if success {
                    self.toasts.push(Toast::success(&message));
                    // Refresh status after successful operation
                    let _ = self.action_tx.send(AppAction::GitStatus);
                } else {
                    self.toasts.push(Toast::error(&message));
                }
                if let Some(dialog) = &mut self.git_dialog {
                    dialog.set_message(&message);
                }
            }
        }
    }

    /// Get a random tip for the home screen.
    fn get_random_tip(&self) -> &'static str {
        let tips = [
            "Press Ctrl+P to open the command palette",
            "Use Ctrl+J for multi-line input",
            "Press Up/Down to navigate prompt history",
            "Use Ctrl+X B to toggle the sidebar",
            "Type @ to autocomplete file names",
            "Start with ! for shell mode",
            "Press Ctrl+X M to change models",
            "Use Ctrl+X L to browse sessions",
            "Press ? for help and keybindings",
            "Escape cancels the current operation",
        ];

        // Simple pseudo-random based on time
        let idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as usize % tips.len())
            .unwrap_or(0);

        tips[idx]
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to create a horizontally centered rectangle.
fn centered_horizontal(width: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect::new(x, area.y, width.min(area.width), area.height)
}

/// Normalize MCP tool names to their base tool name.
/// e.g., "mcp__wonopcode-tools__bash" -> "bash"
fn normalize_tool_name(name: &str) -> String {
    // Handle MCP server prefixed names: mcp__<server>__<tool>
    if name.starts_with("mcp__") {
        if let Some(tool_name) = name.rsplit("__").next() {
            return tool_name.to_string();
        }
    }
    name.to_string()
}
