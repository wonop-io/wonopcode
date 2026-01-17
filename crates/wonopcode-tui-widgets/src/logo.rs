//! Logo widget for the home screen.

use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use wonopcode_tui_core::Theme;

/// ASCII art logo for wonopcode.
const LOGO: &str = r"
 /$$      /$$                                                /$$$$$$                  /$$          
| $$  /$ | $$                                               /$$__  $$                | $$          
| $$ /$$$| $$  /$$$$$$  /$$$$$$$   /$$$$$$   /$$$$$$       | $$  \__/  /$$$$$$   /$$$$$$$  /$$$$$$ 
| $$/$$ $$ $$ /$$__  $$| $$__  $$ /$$__  $$ /$$__  $$      | $$       /$$__  $$ /$$__  $$ /$$__  $$
| $$$$_  $$$$| $$  \ $$| $$  \ $$| $$  \ $$| $$  \ $$      | $$      | $$  \ $$| $$  | $$| $$$$$$$$
| $$$/ \  $$$| $$  | $$| $$  | $$| $$  | $$| $$  | $$      | $$    $$| $$  | $$| $$  | $$| $$_____/
| $$/   \  $$|  $$$$$$/| $$  | $$|  $$$$$$/| $$$$$$$/      |  $$$$$$/|  $$$$$$/|  $$$$$$$|  $$$$$$$
|__/     \__/ \______/ |__/  |__/ \______/ | $$____/        \______/  \______/  \_______/ \_______/
                                           | $$                                                    
                                           | $$                                                    
                                           |__/                                                    
";

/// Small logo for narrow terminals.
const LOGO_SMALL: &str = r#"
 Wonop Code
"#;

/// Logo widget.
#[derive(Debug, Clone, Default)]
pub struct LogoWidget {
    /// Whether to show the small version.
    small: bool,
}

impl LogoWidget {
    /// Create a new logo widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to use the small logo.
    pub fn small(mut self, small: bool) -> Self {
        self.small = small;
        self
    }

    /// Get the height needed for the logo.
    pub fn height(&self) -> u16 {
        if self.small {
            3
        } else {
            13 // New logo is 11 lines + 2 padding
        }
    }

    /// Render the logo widget.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // The large logo needs ~100 columns to display properly
        let logo_text = if self.small || area.width < 100 {
            LOGO_SMALL
        } else {
            LOGO
        };

        let lines: Vec<Line> = logo_text
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), theme.highlight_style())))
            .collect();

        let paragraph = Paragraph::new(lines).alignment(Alignment::Center);

        frame.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logo_widget_new() {
        let widget = LogoWidget::new();
        assert!(!widget.small);
    }

    #[test]
    fn test_logo_widget_default() {
        let widget = LogoWidget::default();
        assert!(!widget.small);
    }

    #[test]
    fn test_logo_widget_small() {
        let widget = LogoWidget::new().small(true);
        assert!(widget.small);

        let widget = LogoWidget::new().small(false);
        assert!(!widget.small);
    }

    #[test]
    fn test_logo_widget_height_large() {
        let widget = LogoWidget::new();
        assert_eq!(widget.height(), 13);
    }

    #[test]
    fn test_logo_widget_height_small() {
        let widget = LogoWidget::new().small(true);
        assert_eq!(widget.height(), 3);
    }

    #[test]
    fn test_logo_widget_clone() {
        let widget = LogoWidget::new().small(true);
        let cloned = widget.clone();
        assert!(cloned.small);
    }

    #[test]
    fn test_logo_widget_debug() {
        let widget = LogoWidget::new();
        let debug = format!("{:?}", widget);
        assert!(debug.contains("LogoWidget"));
    }

    #[test]
    fn test_logo_constants() {
        // Verify the logo constants exist and have content
        assert!(!LOGO.is_empty());
        assert!(!LOGO_SMALL.is_empty());
        assert!(LOGO.contains("$$"));
        assert!(LOGO_SMALL.contains("Wonop"));
    }
}
