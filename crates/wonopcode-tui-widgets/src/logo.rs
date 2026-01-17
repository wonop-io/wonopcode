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
