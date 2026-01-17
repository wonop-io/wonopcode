//! Status bar widget with integrated mode indicator.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use wonopcode_tui_core::Theme;

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

#[cfg(test)]
mod tests {
    use super::*;

    // Status enum tests

    #[test]
    fn test_status_default() {
        let status = Status::default();
        assert_eq!(status, Status::Idle);
    }

    #[test]
    fn test_status_variants() {
        assert_eq!(Status::Idle, Status::Idle);
        assert_eq!(Status::Thinking, Status::Thinking);
        assert_eq!(
            Status::Running("test".to_string()),
            Status::Running("test".to_string())
        );
        assert_eq!(
            Status::Error("error".to_string()),
            Status::Error("error".to_string())
        );
    }

    #[test]
    fn test_status_clone() {
        let status = Status::Running("Test".to_string());
        let cloned = status.clone();
        assert_eq!(cloned, Status::Running("Test".to_string()));
    }

    #[test]
    fn test_status_debug() {
        assert!(format!("{:?}", Status::Idle).contains("Idle"));
        assert!(format!("{:?}", Status::Thinking).contains("Thinking"));
        assert!(format!("{:?}", Status::Running("test".to_string())).contains("Running"));
        assert!(format!("{:?}", Status::Error("error".to_string())).contains("Error"));
    }

    // StatusMode tests

    #[test]
    fn test_status_mode_default() {
        let mode = StatusMode::default();
        assert_eq!(mode, StatusMode::Input);
    }

    #[test]
    fn test_status_mode_name() {
        assert_eq!(StatusMode::Input.name(), "INPUT");
        assert_eq!(StatusMode::Scroll.name(), "SCROLL");
        assert_eq!(StatusMode::Select.name(), "SELECT");
        assert_eq!(StatusMode::Search.name(), "SEARCH");
        assert_eq!(StatusMode::Waiting.name(), "WAIT");
        assert_eq!(StatusMode::Leader.name(), "CTRL+X");
    }

    #[test]
    fn test_status_mode_clone() {
        let mode = StatusMode::Search;
        let cloned = mode;
        assert_eq!(cloned, StatusMode::Search);
    }

    #[test]
    fn test_status_mode_debug() {
        assert!(format!("{:?}", StatusMode::Input).contains("Input"));
        assert!(format!("{:?}", StatusMode::Scroll).contains("Scroll"));
    }

    // StatusWidget tests

    #[test]
    fn test_status_widget_new() {
        let widget = StatusWidget::new();
        assert_eq!(widget.status, Status::Idle);
        assert_eq!(widget.mode, StatusMode::Input);
        assert!(widget.model.is_empty());
        assert!(widget.tokens.is_none());
        assert!(widget.project.is_empty());
    }

    #[test]
    fn test_status_widget_default() {
        let widget = StatusWidget::default();
        assert_eq!(widget.status, Status::Idle);
        assert_eq!(widget.mode, StatusMode::Input);
    }

    #[test]
    fn test_status_widget_set_status() {
        let mut widget = StatusWidget::new();
        widget.set_status(Status::Thinking);
        assert_eq!(widget.status, Status::Thinking);

        widget.set_status(Status::Error("Bad".to_string()));
        assert_eq!(widget.status, Status::Error("Bad".to_string()));
    }

    #[test]
    fn test_status_widget_set_mode() {
        let mut widget = StatusWidget::new();
        widget.set_mode(StatusMode::Scroll);
        assert_eq!(widget.mode, StatusMode::Scroll);
    }

    #[test]
    fn test_status_widget_set_model() {
        let mut widget = StatusWidget::new();
        widget.set_model("claude-sonnet-4");
        assert_eq!(widget.model, "claude-sonnet-4");

        widget.set_model(String::from("gpt-4"));
        assert_eq!(widget.model, "gpt-4");
    }

    #[test]
    fn test_status_widget_set_tokens() {
        let mut widget = StatusWidget::new();
        widget.set_tokens(1000, 500);
        assert_eq!(widget.tokens, Some((1000, 500)));
    }

    #[test]
    fn test_status_widget_set_project() {
        let mut widget = StatusWidget::new();
        widget.set_project("my-project");
        assert_eq!(widget.project, "my-project");
    }

    #[test]
    fn test_status_widget_clone() {
        let mut widget = StatusWidget::new();
        widget.set_model("test-model");
        widget.set_tokens(100, 50);
        let cloned = widget.clone();
        assert_eq!(cloned.model, "test-model");
        assert_eq!(cloned.tokens, Some((100, 50)));
    }

    #[test]
    fn test_status_widget_debug() {
        let widget = StatusWidget::new();
        let debug = format!("{widget:?}");
        assert!(debug.contains("StatusWidget"));
    }
}
