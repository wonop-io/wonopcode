//! Status bar widget with integrated mode indicator.

use wonopcode_tui_core::Theme;
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Status to display in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle,
    Thinking,
    Running(String),
    Error(String),
}

impl Default for Status {
    fn default() -> Self {
        Self::Idle
    }
}

/// Current application mode for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusMode {
    #[default]
    Input,
    Scroll,
    Select,
    Search,
    Waiting,
    Leader,
}

impl StatusMode {
    /// Get display name.
    pub fn name(&self) -> &'static str {
        match self {
            StatusMode::Input => "INPUT",
            StatusMode::Scroll => "SCROLL",
            StatusMode::Select => "SELECT",
            StatusMode::Search => "SEARCH",
            StatusMode::Waiting => "WAIT",
            StatusMode::Leader => "CTRL+X",
        }
    }
}

/// Status bar widget.
#[derive(Debug, Clone, Default)]
pub struct StatusWidget {
    /// Current status.
    status: Status,
    /// Current mode.
    mode: StatusMode,
    /// Model name.
    model: String,
    /// Token count.
    tokens: Option<(u32, u32)>,
    /// Project name.
    project: String,
}

impl StatusWidget {
    /// Create a new status widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the status.
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// Set the mode.
    pub fn set_mode(&mut self, mode: StatusMode) {
        self.mode = mode;
    }

    /// Set the model name.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Set the token count.
    pub fn set_tokens(&mut self, input: u32, output: u32) {
        self.tokens = Some((input, output));
    }

    /// Set the project name.
    pub fn set_project(&mut self, project: impl Into<String>) {
        self.project = project.into();
    }

    /// Render the status widget.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Status text (mode is now shown in the footer, not here)
        let (status_text, status_style) = match &self.status {
            Status::Idle => ("Ready", theme.success_style()),
            Status::Thinking => ("Thinking...", theme.warning_style()),
            Status::Running(action) => (action.as_str(), theme.warning_style()),
            Status::Error(err) => (err.as_str(), theme.error_style()),
        };

        let mut spans = vec![
            Span::styled(" ", theme.text_style()),
            Span::styled(status_text, status_style),
        ];

        // Build right side: model and tokens
        let mut right_parts = vec![];

        if !self.model.is_empty() {
            right_parts.push(Span::styled(&self.model, theme.dim_style()));
        }

        if let Some((input, output)) = self.tokens {
            if !right_parts.is_empty() {
                right_parts.push(Span::styled(" │ ", theme.dim_style()));
            }
            right_parts.push(Span::styled(
                format!("{input}↓ {output}↑"),
                theme.dim_style(),
            ));
        }

        // Calculate spacing
        let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
        let right_len: usize = right_parts.iter().map(|s| s.content.len()).sum();
        let total_width = area.width as usize;
        let spacing = total_width.saturating_sub(left_len + right_len + 2);

        if spacing > 0 {
            spans.push(Span::styled(" ".repeat(spacing), theme.text_style()));
        }

        spans.extend(right_parts);
        spans.push(Span::styled(" ", theme.text_style()));

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line);

        frame.render_widget(paragraph, area);
    }
}
