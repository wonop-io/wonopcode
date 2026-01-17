//! Which-key overlay widget for displaying available key sequences.
//!
//! Shows available keyboard shortcuts when the leader key (Ctrl+X) is pressed,
//! similar to vim's which-key plugin.

use ratatui::{
    layout::{Alignment, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use wonopcode_tui_core::Theme;

/// A key binding entry for the which-key display.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    /// The key to press.
    pub key: &'static str,
    /// Description of what the key does.
    pub description: &'static str,
}

/// Which-key overlay widget.
#[derive(Debug, Clone, Default)]
pub struct WhichKeyOverlay {
    /// Whether the overlay is visible.
    visible: bool,
    /// Title for the overlay.
    title: String,
    /// Key bindings to display.
    bindings: Vec<KeyBinding>,
}

impl WhichKeyOverlay {
    /// Create a new which-key overlay.
    pub fn new() -> Self {
        Self {
            visible: false,
            title: "Ctrl+X".to_string(),
            bindings: Self::default_bindings(),
        }
    }

    /// Get the default Ctrl+X key bindings.
    fn default_bindings() -> Vec<KeyBinding> {
        vec![
            KeyBinding {
                key: "N",
                description: "New session",
            },
            KeyBinding {
                key: "L",
                description: "Session list",
            },
            KeyBinding {
                key: "M",
                description: "Model selection",
            },
            KeyBinding {
                key: "A",
                description: "Agent selection",
            },
            KeyBinding {
                key: "B",
                description: "Toggle sidebar",
            },
            KeyBinding {
                key: "T",
                description: "Theme selection",
            },
            KeyBinding {
                key: "Y",
                description: "Copy response",
            },
            KeyBinding {
                key: "E",
                description: "Edit in $EDITOR",
            },
            KeyBinding {
                key: "X",
                description: "Export session",
            },
            KeyBinding {
                key: "U",
                description: "Undo message",
            },
            KeyBinding {
                key: "R",
                description: "Redo message",
            },
            KeyBinding {
                key: "S",
                description: "Settings",
            },
        ]
    }

    /// Show the overlay.
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide the overlay.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Check if visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Render the overlay centered on screen.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        // Calculate overlay dimensions
        let max_key_len = self.bindings.iter().map(|b| b.key.len()).max().unwrap_or(1);
        let max_desc_len = self
            .bindings
            .iter()
            .map(|b| b.description.len())
            .max()
            .unwrap_or(10);
        let content_width = max_key_len + 3 + max_desc_len + 4; // key + " - " + desc + padding
        let content_height = self.bindings.len() as u16 + 2; // bindings + borders

        let overlay_width = (content_width as u16)
            .min(area.width.saturating_sub(4))
            .max(30);
        let overlay_height = content_height.min(area.height.saturating_sub(4)).max(5);

        // Center the overlay
        let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
        let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
        let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

        // Clear the background
        frame.render_widget(Clear, overlay_area);

        // Build the content
        let mut lines: Vec<Line> = vec![];

        for binding in &self.bindings {
            let key_span = Span::styled(
                format!(" {:>width$}", binding.key, width = max_key_len),
                theme.accent_style().add_modifier(Modifier::BOLD),
            );
            let sep_span = Span::styled(" → ", theme.muted_style());
            let desc_span = Span::styled(binding.description, theme.text_style());

            lines.push(Line::from(vec![key_span, sep_span, desc_span]));
        }

        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", self.title),
                theme.accent_style().add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(theme.border_style())
            .style(theme.panel_style());

        let para = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left);

        frame.render_widget(para, overlay_area);
    }
}
