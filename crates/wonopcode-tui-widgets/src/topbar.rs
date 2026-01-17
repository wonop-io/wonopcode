//! Top bar widget showing project directory and session info.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use wonopcode_tui_core::Theme;

/// Top bar widget.
#[derive(Debug, Clone, Default)]
pub struct TopBarWidget {
    /// Current directory.
    directory: String,
    /// Session title (optional).
    session_title: Option<String>,
    /// Project name (optional).
    project_name: Option<String>,
}

impl TopBarWidget {
    /// Create a new top bar widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the directory.
    pub fn set_directory(&mut self, dir: impl Into<String>) {
        self.directory = dir.into();
    }

    /// Set the session title.
    pub fn set_session_title(&mut self, title: Option<String>) {
        self.session_title = title;
    }

    /// Set the project name.
    pub fn set_project_name(&mut self, name: Option<String>) {
        self.project_name = name;
    }

    /// Render the top bar.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.height == 0 {
            return;
        }

        // Shorten directory for display
        let dir_display = self.format_directory(area.width as usize);

        let mut spans = vec![];

        // Directory with folder icon
        spans.push(Span::styled(" ", theme.text_style()));
        spans.push(Span::styled(
            &dir_display,
            theme.text_style().add_modifier(Modifier::BOLD),
        ));

        // Session title if available
        if let Some(ref title) = self.session_title {
            if !title.is_empty() {
                spans.push(Span::styled(" │ ", theme.muted_style()));
                spans.push(Span::styled(title, theme.accent_style()));
            }
        }

        // Right side: project name if different from directory
        let mut right_parts = vec![];
        if let Some(ref project) = self.project_name {
            if !project.is_empty() {
                right_parts.push(Span::styled(project, theme.muted_style()));
                right_parts.push(Span::styled(" ", theme.text_style()));
            }
        }

        // Calculate spacing
        let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
        let right_len: usize = right_parts.iter().map(|s| s.content.len()).sum();
        let available = area.width as usize;
        let spacing = available.saturating_sub(left_len + right_len);

        if spacing > 0 && !right_parts.is_empty() {
            spans.push(Span::styled(" ".repeat(spacing), theme.text_style()));
            spans.extend(right_parts);
        }

        let line = Line::from(spans);
        let para = Paragraph::new(line).style(theme.element_style());
        frame.render_widget(para, area);
    }

    /// Format directory for display, shortening if needed.
    fn format_directory(&self, max_width: usize) -> String {
        if self.directory.is_empty() {
            return String::new();
        }

        // Try to use ~ for home directory
        let home = std::env::var("HOME").unwrap_or_default();
        let display = if !home.is_empty() && self.directory.starts_with(&home) {
            format!("~{}", &self.directory[home.len()..])
        } else {
            self.directory.clone()
        };

        // Shorten if too long
        let max_dir_len = max_width.saturating_sub(10).min(50);
        if display.len() > max_dir_len {
            format!(
                "...{}",
                &display[display.len().saturating_sub(max_dir_len - 3)..]
            )
        } else {
            display
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_top_bar_widget_new() {
        let widget = TopBarWidget::new();
        assert!(widget.directory.is_empty());
        assert!(widget.session_title.is_none());
        assert!(widget.project_name.is_none());
    }

    #[test]
    fn test_top_bar_widget_default() {
        let widget = TopBarWidget::default();
        assert!(widget.directory.is_empty());
        assert!(widget.session_title.is_none());
        assert!(widget.project_name.is_none());
    }

    #[test]
    fn test_top_bar_widget_set_directory() {
        let mut widget = TopBarWidget::new();
        widget.set_directory("/home/user/project");
        assert_eq!(widget.directory, "/home/user/project");
    }

    #[test]
    fn test_top_bar_widget_set_session_title() {
        let mut widget = TopBarWidget::new();
        widget.set_session_title(Some("My Session".to_string()));
        assert_eq!(widget.session_title, Some("My Session".to_string()));

        widget.set_session_title(None);
        assert!(widget.session_title.is_none());
    }

    #[test]
    fn test_top_bar_widget_set_project_name() {
        let mut widget = TopBarWidget::new();
        widget.set_project_name(Some("my-project".to_string()));
        assert_eq!(widget.project_name, Some("my-project".to_string()));

        widget.set_project_name(None);
        assert!(widget.project_name.is_none());
    }

    #[test]
    fn test_top_bar_widget_format_directory_empty() {
        let widget = TopBarWidget::new();
        assert_eq!(widget.format_directory(100), "");
    }

    #[test]
    fn test_top_bar_widget_format_directory_short() {
        let mut widget = TopBarWidget::new();
        widget.set_directory("/short/path");
        let formatted = widget.format_directory(100);
        assert_eq!(formatted, "/short/path");
    }

    #[test]
    fn test_top_bar_widget_format_directory_long() {
        let mut widget = TopBarWidget::new();
        widget.set_directory("/very/long/path/that/exceeds/the/maximum/allowed/width/for/display");
        let formatted = widget.format_directory(30);
        assert!(formatted.starts_with("..."));
        assert!(formatted.len() <= 20); // max_dir_len = 30 - 10 = 20
    }

    #[test]
    fn test_top_bar_widget_clone() {
        let mut widget = TopBarWidget::new();
        widget.set_directory("/test");
        widget.set_session_title(Some("Session".to_string()));
        widget.set_project_name(Some("Project".to_string()));

        let cloned = widget.clone();
        assert_eq!(cloned.directory, "/test");
        assert_eq!(cloned.session_title, Some("Session".to_string()));
        assert_eq!(cloned.project_name, Some("Project".to_string()));
    }

    #[test]
    fn test_top_bar_widget_debug() {
        let widget = TopBarWidget::new();
        let debug = format!("{:?}", widget);
        assert!(debug.contains("TopBarWidget"));
    }
}
