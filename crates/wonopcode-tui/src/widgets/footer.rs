//! Footer widget for status information.
//!
//! Shows: Status/Spinner | Mode + hints | Model | Tokens | Sandbox | Permissions | LSP | MCP

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::time::{Duration, Instant};

use crate::theme::Theme;

/// Status to display in the footer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FooterStatus {
    #[default]
    Idle,
    Thinking,
    Running(String),
    Error(String),
}

/// Sandbox display state for the footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxDisplayState {
    /// Sandbox is not configured/disabled.
    #[default]
    Disabled,
    /// Sandbox is stopped but available.
    Stopped,
    /// Sandbox is starting up.
    Starting,
    /// Sandbox is running and ready.
    Running,
    /// Sandbox encountered an error.
    Error,
}

/// Current mode for the footer display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FooterMode {
    #[default]
    Input,
    Scroll,
    Select,
    Search,
    Waiting,
    Leader,
}

impl FooterMode {
    /// Get the display name for the mode.
    pub fn name(&self) -> &'static str {
        match self {
            FooterMode::Input => "INPUT",
            FooterMode::Scroll => "SCROLL",
            FooterMode::Select => "SELECT",
            FooterMode::Search => "SEARCH",
            FooterMode::Waiting => "WAITING",
            FooterMode::Leader => "CTRL+X",
        }
    }

    /// Get contextual keybinding hints for the mode.
    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        match self {
            FooterMode::Input => &[
                ("Enter", "send"),
                ("Esc", "scroll"),
                ("^X", "leader"),
                ("^P", "commands"),
            ],
            FooterMode::Scroll => &[
                ("j/k", "scroll"),
                ("v", "select"),
                ("y", "copy"),
                ("i", "input"),
                ("^X", "leader"),
            ],
            FooterMode::Select => &[("j/k", "navigate"), ("y", "copy"), ("Esc", "cancel")],
            FooterMode::Search => &[("n/N", "next/prev"), ("Enter", "go to"), ("Esc", "cancel")],
            FooterMode::Waiting => &[("Esc", "cancel")],
            FooterMode::Leader => &[("N", "new"), ("L", "sessions"), ("M", "model")],
        }
    }
}

/// Footer widget showing directory and status.
#[derive(Debug, Clone)]
pub struct FooterWidget {
    /// Current mode.
    mode: FooterMode,
    /// Current directory.
    directory: String,
    /// Current model.
    model: String,
    /// Provider name.
    provider: String,
    /// Whether connected.
    connected: bool,
    /// Status (Ready/Thinking/Running).
    status: FooterStatus,
    /// Token counts (input, output).
    tokens: Option<(u32, u32)>,
    /// Number of pending permissions.
    pending_permissions: usize,
    /// Number of connected LSP servers.
    lsp_count: usize,
    /// Number of connected MCP servers.
    mcp_count: usize,
    /// Whether any MCP server has an error.
    mcp_has_error: bool,
    /// Sandbox state.
    sandbox_state: SandboxDisplayState,
    /// Sandbox runtime name (e.g., "docker", "lima").
    sandbox_runtime: Option<String>,
    /// Spinner animation frame.
    spinner_frame: usize,
    /// Last spinner update time.
    spinner_last_update: Instant,
    /// Spinner animation frames (braille spinner).
    spinner_frames: Vec<&'static str>,
}

impl Default for FooterWidget {
    fn default() -> Self {
        Self {
            mode: FooterMode::default(),
            directory: String::new(),
            model: String::new(),
            provider: String::new(),
            connected: true,
            status: FooterStatus::default(),
            tokens: None,
            pending_permissions: 0,
            lsp_count: 0,
            mcp_count: 0,
            mcp_has_error: false,
            sandbox_state: SandboxDisplayState::default(),
            sandbox_runtime: None,
            spinner_frame: 0,
            spinner_last_update: Instant::now(),
            spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
        }
    }
}

impl FooterWidget {
    /// Create a new footer widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the directory.
    pub fn set_directory(&mut self, dir: impl Into<String>) {
        self.directory = dir.into();
    }

    /// Set the model.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Set the provider.
    pub fn set_provider(&mut self, provider: impl Into<String>) {
        self.provider = provider.into();
    }

    /// Set connection status.
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    /// Set status (Ready/Thinking/Running).
    pub fn set_status(&mut self, status: FooterStatus) {
        self.status = status;
    }

    /// Check if the footer shows a busy state (thinking or running).
    pub fn is_busy(&self) -> bool {
        matches!(
            self.status,
            FooterStatus::Thinking | FooterStatus::Running(_)
        )
    }

    /// Set the token counts.
    pub fn set_tokens(&mut self, input: u32, output: u32) {
        self.tokens = Some((input, output));
    }

    /// Tick the spinner animation.
    pub fn tick(&mut self) {
        if matches!(
            self.status,
            FooterStatus::Thinking | FooterStatus::Running(_)
        ) {
            let speed = Duration::from_millis(80);
            if self.spinner_last_update.elapsed() >= speed {
                self.spinner_frame = (self.spinner_frame + 1) % self.spinner_frames.len();
                self.spinner_last_update = Instant::now();
            }
        }
    }

    /// Get the current spinner character.
    fn spinner_char(&self) -> &'static str {
        self.spinner_frames[self.spinner_frame]
    }

    /// Set the number of pending permissions.
    pub fn set_pending_permissions(&mut self, count: usize) {
        self.pending_permissions = count;
    }

    /// Set LSP server count.
    pub fn set_lsp_count(&mut self, count: usize) {
        self.lsp_count = count;
    }

    /// Set MCP server status.
    pub fn set_mcp_status(&mut self, connected_count: usize, has_error: bool) {
        self.mcp_count = connected_count;
        self.mcp_has_error = has_error;
    }

    /// Set sandbox status.
    pub fn set_sandbox_status(&mut self, state: SandboxDisplayState, runtime: Option<String>) {
        self.sandbox_state = state;
        self.sandbox_runtime = runtime;
    }

    /// Get the sandbox state.
    pub fn get_sandbox_state(&self) -> SandboxDisplayState {
        self.sandbox_state
    }

    /// Get the sandbox runtime name.
    pub fn get_sandbox_runtime(&self) -> Option<&str> {
        self.sandbox_runtime.as_deref()
    }

    /// Get the number of pending permissions.
    pub fn get_permissions_pending(&self) -> usize {
        self.pending_permissions
    }

    /// Set the current mode.
    pub fn set_mode(&mut self, mode: FooterMode) {
        self.mode = mode;
    }

    /// Render the footer.
    /// Layout: Status/Spinner | MODE hints | Model | Tokens | Sandbox | Permissions | LSP | MCP
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut spans = vec![Span::styled(" ", theme.text_style())];

        // Status indicator (Ready/Thinking/Running with spinner)
        match &self.status {
            FooterStatus::Idle => {
                spans.push(Span::styled("Ready", theme.success_style()));
            }
            FooterStatus::Thinking => {
                spans.push(Span::styled(self.spinner_char(), theme.warning_style()));
                spans.push(Span::styled(" Thinking", theme.warning_style()));
            }
            FooterStatus::Running(action) => {
                spans.push(Span::styled(self.spinner_char(), theme.warning_style()));
                spans.push(Span::styled(format!(" {action}"), theme.warning_style()));
            }
            FooterStatus::Error(err) => {
                spans.push(Span::styled(err.as_str(), theme.error_style()));
            }
        }

        spans.push(Span::styled(" │ ", theme.muted_style()));

        // Mode indicator with colored background
        let mode_style = match self.mode {
            FooterMode::Input => theme.success_style().add_modifier(Modifier::BOLD),
            FooterMode::Scroll => theme.info_style().add_modifier(Modifier::BOLD),
            FooterMode::Select => theme.warning_style().add_modifier(Modifier::BOLD),
            FooterMode::Search => theme.accent_style().add_modifier(Modifier::BOLD),
            FooterMode::Waiting => theme.warning_style().add_modifier(Modifier::BOLD),
            FooterMode::Leader => theme.accent_style().add_modifier(Modifier::BOLD),
        };
        spans.push(Span::styled(self.mode.name(), mode_style));
        spans.push(Span::styled(" ", theme.text_style()));

        // Key hints for current mode (keys in bold white)
        for (key, action) in self.mode.hints() {
            spans.push(Span::styled(
                *key,
                theme.text_style().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(":", theme.muted_style()));
            spans.push(Span::styled(*action, theme.muted_style()));
            spans.push(Span::styled(" ", theme.text_style()));
        }

        spans.push(Span::styled("│ ", theme.muted_style()));

        // Sandbox status
        match self.sandbox_state {
            SandboxDisplayState::Running => {
                spans.push(Span::styled("⬡ ", theme.success_style()));
                let label = self
                    .sandbox_runtime
                    .as_ref()
                    .map(|r| r.to_lowercase())
                    .unwrap_or_else(|| "sandbox".to_string());
                spans.push(Span::styled(label, theme.success_style()));
            }
            SandboxDisplayState::Starting => {
                spans.push(Span::styled("⬡ ", theme.warning_style()));
                spans.push(Span::styled("starting...", theme.warning_style()));
            }
            SandboxDisplayState::Stopped => {
                spans.push(Span::styled("⬡ ", theme.muted_style()));
                let label = self
                    .sandbox_runtime
                    .as_ref()
                    .map(|r| format!("{} (stopped)", r.to_lowercase()))
                    .unwrap_or_else(|| "sandbox (stopped)".to_string());
                spans.push(Span::styled(label, theme.muted_style()));
            }
            SandboxDisplayState::Error => {
                spans.push(Span::styled("⬡ ", theme.error_style()));
                spans.push(Span::styled("sandbox error", theme.error_style()));
            }
            SandboxDisplayState::Disabled => {
                spans.push(Span::styled("◇ ", theme.muted_style()));
                spans.push(Span::styled("host", theme.muted_style()));
            }
        }

        // Build right side
        let mut right_parts = vec![];

        // Pending permissions (warning style, prominent)
        if self.pending_permissions > 0 {
            right_parts.push(Span::styled("◉ ", theme.warning_style()));
            let label = if self.pending_permissions == 1 {
                "1 permission".to_string()
            } else {
                format!("{} permissions", self.pending_permissions)
            };
            right_parts.push(Span::styled(label, theme.warning_style()));
            right_parts.push(Span::styled("  ", theme.text_style()));
        }

        // LSP count (only if any connected)
        if self.lsp_count > 0 {
            right_parts.push(Span::styled("• ", theme.success_style()));
            right_parts.push(Span::styled(
                format!("{} LSP", self.lsp_count),
                theme.muted_style(),
            ));
            right_parts.push(Span::styled("  ", theme.text_style()));
        }

        // MCP count (only if any connected)
        if self.mcp_count > 0 {
            let icon_style = if self.mcp_has_error {
                theme.error_style()
            } else {
                theme.success_style()
            };
            right_parts.push(Span::styled("⊙ ", icon_style));
            right_parts.push(Span::styled(
                format!("{} MCP", self.mcp_count),
                theme.muted_style(),
            ));
            right_parts.push(Span::styled("  ", theme.text_style()));
        }

        // Model name
        if !self.model.is_empty() {
            right_parts.push(Span::styled(&self.model, theme.dim_style()));
            right_parts.push(Span::styled("  ", theme.text_style()));
        }

        // Token counts
        if let Some((input, output)) = self.tokens {
            right_parts.push(Span::styled(
                format!("{input}↓ {output}↑"),
                theme.dim_style(),
            ));
        }

        // Calculate spacing
        let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
        let right_len: usize = right_parts.iter().map(|s| s.content.len()).sum::<usize>() + 1;
        let available = area.width as usize;
        let spacing = available.saturating_sub(left_len + right_len);

        if spacing > 0 {
            spans.push(Span::styled(" ".repeat(spacing), theme.text_style()));
        }

        spans.extend(right_parts);
        spans.push(Span::styled(" ", theme.text_style()));

        let line = Line::from(spans);
        let para = Paragraph::new(line);
        frame.render_widget(para, area);
    }
}
