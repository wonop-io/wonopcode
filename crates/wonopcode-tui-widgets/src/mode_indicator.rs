//! Mode indicator widget showing current mode and contextual keybindings.
//!
//! Displays the current application mode (Input, Scroll, Select, Waiting)
//! with contextual keyboard shortcuts to improve discoverability.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use wonopcode_tui_core::Theme;

/// Application mode for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// Normal input mode.
    #[default]
    Input,
    /// Scrolling through messages.
    Scroll,
    /// Selecting text for copying.
    Select,
    /// Searching through messages.
    Search,
    /// Waiting for AI response.
    Waiting,
    /// Leader key pressed.
    Leader,
}

impl DisplayMode {
    /// Get the display name for the mode.
    pub fn name(&self) -> &'static str {
        match self {
            DisplayMode::Input => "INPUT",
            DisplayMode::Scroll => "SCROLL",
            DisplayMode::Select => "SELECT",
            DisplayMode::Search => "SEARCH",
            DisplayMode::Waiting => "WAITING",
            DisplayMode::Leader => "CTRL+X",
        }
    }

    /// Get contextual keybinding hints for the mode.
    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            DisplayMode::Input => vec![
                ("Enter", "send"),
                ("Esc", "scroll"),
                ("Ctrl+P", "commands"),
                ("Tab", "agent"),
                ("?", "help"),
            ],
            DisplayMode::Scroll => vec![
                ("j/k", "scroll"),
                ("v", "select"),
                ("y", "copy"),
                ("o", "expand"),
                ("i", "input"),
            ],
            DisplayMode::Select => vec![
                ("j/k", "navigate"),
                ("y", "copy"),
                ("o", "expand"),
                ("Esc", "cancel"),
            ],
            DisplayMode::Search => vec![
                ("n", "next"),
                ("N", "prev"),
                ("Enter", "go to"),
                ("Esc", "cancel"),
            ],
            DisplayMode::Waiting => vec![("Esc", "cancel")],
            DisplayMode::Leader => vec![
                ("N", "new"),
                ("L", "sessions"),
                ("M", "model"),
                ("A", "agent"),
                ("T", "theme"),
                ("U", "undo"),
            ],
        }
    }
}

/// Mode indicator widget.
#[derive(Debug, Clone, Default)]
pub struct ModeIndicator {
    /// Current mode.
    mode: DisplayMode,
    /// Whether to show the indicator (hidden in some states).
    visible: bool,
}

impl ModeIndicator {
    /// Create a new mode indicator.
    pub fn new() -> Self {
        Self {
            mode: DisplayMode::Input,
            visible: true,
        }
    }

    /// Set the current mode.
    pub fn set_mode(&mut self, mode: DisplayMode) {
        self.mode = mode;
    }

    /// Set visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Render the mode indicator.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible || area.height == 0 {
            return;
        }

        // Mode-specific colors
        let (_mode_style, mode_bg) = match self.mode {
            DisplayMode::Input => (theme.success_style(), theme.success_style()),
            DisplayMode::Scroll => (theme.info_style(), theme.info_style()),
            DisplayMode::Select => (theme.warning_style(), theme.warning_style()),
            DisplayMode::Search => (theme.accent_style(), theme.accent_style()),
            DisplayMode::Waiting => (theme.warning_style(), theme.warning_style()),
            DisplayMode::Leader => (theme.accent_style(), theme.accent_style()),
        };

        let mut spans = vec![];

        // Mode name with background
        spans.push(Span::styled(
            format!(" {} ", self.mode.name()),
            mode_bg.add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" ", theme.text_style()));

        // Contextual hints
        let hints = self.mode.hints();
        for (i, (key, action)) in hints.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  ", theme.muted_style()));
            }
            spans.push(Span::styled(*key, theme.accent_style()));
            spans.push(Span::styled(":", theme.muted_style()));
            spans.push(Span::styled(*action, theme.muted_style()));
        }

        let line = Line::from(spans);
        let para = Paragraph::new(line);
        frame.render_widget(para, area);
    }

    /// Get the height needed for this widget.
    pub fn height(&self) -> u16 {
        if self.visible {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // DisplayMode tests

    #[test]
    fn test_display_mode_default() {
        let mode = DisplayMode::default();
        assert_eq!(mode, DisplayMode::Input);
    }

    #[test]
    fn test_display_mode_name() {
        assert_eq!(DisplayMode::Input.name(), "INPUT");
        assert_eq!(DisplayMode::Scroll.name(), "SCROLL");
        assert_eq!(DisplayMode::Select.name(), "SELECT");
        assert_eq!(DisplayMode::Search.name(), "SEARCH");
        assert_eq!(DisplayMode::Waiting.name(), "WAITING");
        assert_eq!(DisplayMode::Leader.name(), "CTRL+X");
    }

    #[test]
    fn test_display_mode_hints_input() {
        let hints = DisplayMode::Input.hints();
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|(k, _)| *k == "Enter"));
        assert!(hints.iter().any(|(k, _)| *k == "Esc"));
    }

    #[test]
    fn test_display_mode_hints_scroll() {
        let hints = DisplayMode::Scroll.hints();
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|(k, _)| *k == "j/k"));
        assert!(hints.iter().any(|(k, _)| *k == "y"));
    }

    #[test]
    fn test_display_mode_hints_select() {
        let hints = DisplayMode::Select.hints();
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|(k, _)| *k == "y"));
        assert!(hints.iter().any(|(k, _)| *k == "Esc"));
    }

    #[test]
    fn test_display_mode_hints_search() {
        let hints = DisplayMode::Search.hints();
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|(k, _)| *k == "n"));
        assert!(hints.iter().any(|(k, _)| *k == "N"));
    }

    #[test]
    fn test_display_mode_hints_waiting() {
        let hints = DisplayMode::Waiting.hints();
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0], ("Esc", "cancel"));
    }

    #[test]
    fn test_display_mode_hints_leader() {
        let hints = DisplayMode::Leader.hints();
        assert!(!hints.is_empty());
        assert!(hints.iter().any(|(k, _)| *k == "N"));
        assert!(hints.iter().any(|(k, _)| *k == "M"));
    }

    #[test]
    fn test_display_mode_clone() {
        let mode = DisplayMode::Search;
        let cloned = mode.clone();
        assert_eq!(cloned, DisplayMode::Search);
    }

    #[test]
    fn test_display_mode_debug() {
        assert!(format!("{:?}", DisplayMode::Input).contains("Input"));
        assert!(format!("{:?}", DisplayMode::Scroll).contains("Scroll"));
    }

    // ModeIndicator tests

    #[test]
    fn test_mode_indicator_new() {
        let indicator = ModeIndicator::new();
        assert_eq!(indicator.mode, DisplayMode::Input);
        assert!(indicator.visible);
    }

    #[test]
    fn test_mode_indicator_default() {
        let indicator = ModeIndicator::default();
        assert_eq!(indicator.mode, DisplayMode::Input);
        assert!(!indicator.visible); // default is false
    }

    #[test]
    fn test_mode_indicator_set_mode() {
        let mut indicator = ModeIndicator::new();
        indicator.set_mode(DisplayMode::Scroll);
        assert_eq!(indicator.mode, DisplayMode::Scroll);
    }

    #[test]
    fn test_mode_indicator_set_visible() {
        let mut indicator = ModeIndicator::new();
        assert!(indicator.visible);

        indicator.set_visible(false);
        assert!(!indicator.visible);

        indicator.set_visible(true);
        assert!(indicator.visible);
    }

    #[test]
    fn test_mode_indicator_height_visible() {
        let mut indicator = ModeIndicator::new();
        indicator.set_visible(true);
        assert_eq!(indicator.height(), 1);
    }

    #[test]
    fn test_mode_indicator_height_hidden() {
        let mut indicator = ModeIndicator::new();
        indicator.set_visible(false);
        assert_eq!(indicator.height(), 0);
    }

    #[test]
    fn test_mode_indicator_clone() {
        let mut indicator = ModeIndicator::new();
        indicator.set_mode(DisplayMode::Select);
        indicator.set_visible(false);
        let cloned = indicator.clone();
        assert_eq!(cloned.mode, DisplayMode::Select);
        assert!(!cloned.visible);
    }

    #[test]
    fn test_mode_indicator_debug() {
        let indicator = ModeIndicator::new();
        let debug = format!("{:?}", indicator);
        assert!(debug.contains("ModeIndicator"));
    }
}
