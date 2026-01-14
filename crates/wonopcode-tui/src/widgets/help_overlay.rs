//! Context-sensitive help overlay widget.
//!
//! Shows contextual keyboard shortcuts when `?` is pressed,
//! with hints that fade after a timeout or on any key press.

use ratatui::{
    layout::{Alignment, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};

use crate::theme::Theme;

/// Context for the help overlay - determines what hints to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpContext {
    /// General help for input mode.
    #[default]
    Input,
    /// Help for scroll mode.
    Scroll,
    /// Help for selection mode.
    Select,
    /// Help for search mode.
    Search,
    /// Help for when waiting for AI.
    Waiting,
}

/// A help entry to display.
#[derive(Debug, Clone)]
pub struct HelpEntry {
    /// Keyboard shortcut.
    pub key: &'static str,
    /// Description of what it does.
    pub description: &'static str,
    /// Category/group for organization.
    pub category: &'static str,
}

/// Context-sensitive help overlay.
#[derive(Debug, Clone)]
pub struct HelpOverlay {
    /// Whether the overlay is visible.
    visible: bool,
    /// Current context.
    context: HelpContext,
    /// When the overlay was shown (for auto-dismiss).
    shown_at: Option<Instant>,
    /// Auto-dismiss timeout.
    timeout: Duration,
}

impl Default for HelpOverlay {
    fn default() -> Self {
        Self {
            visible: false,
            context: HelpContext::Input,
            shown_at: None,
            timeout: Duration::from_secs(5),
        }
    }
}

impl HelpOverlay {
    /// Create a new help overlay.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the overlay with the given context.
    pub fn show(&mut self, context: HelpContext) {
        self.visible = true;
        self.context = context;
        self.shown_at = Some(Instant::now());
    }

    /// Hide the overlay.
    pub fn hide(&mut self) {
        self.visible = false;
        self.shown_at = None;
    }

    /// Toggle visibility.
    pub fn toggle(&mut self, context: HelpContext) {
        if self.visible {
            self.hide();
        } else {
            self.show(context);
        }
    }

    /// Check if visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check if should auto-dismiss (timeout expired).
    pub fn should_dismiss(&self) -> bool {
        if let Some(shown_at) = self.shown_at {
            shown_at.elapsed() >= self.timeout
        } else {
            false
        }
    }

    /// Tick for auto-dismiss check.
    pub fn tick(&mut self) {
        if self.should_dismiss() {
            self.hide();
        }
    }

    /// Get help entries for the current context.
    fn get_entries(&self) -> Vec<HelpEntry> {
        match self.context {
            HelpContext::Input => vec![
                HelpEntry {
                    key: "Enter",
                    description: "Send message",
                    category: "Input",
                },
                HelpEntry {
                    key: "Ctrl+X Ctrl+C",
                    description: "Exit application",
                    category: "Application",
                },
                HelpEntry {
                    key: "Esc",
                    description: "Switch to scroll mode",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "Ctrl+P",
                    description: "Open command palette",
                    category: "Commands",
                },
                HelpEntry {
                    key: "Ctrl+X",
                    description: "Leader key (show more)",
                    category: "Commands",
                },
                HelpEntry {
                    key: "/cmd",
                    description: "Run slash command",
                    category: "Commands",
                },
                HelpEntry {
                    key: "@file",
                    description: "Attach file context",
                    category: "Input",
                },
                HelpEntry {
                    key: "Tab",
                    description: "Agent autocomplete",
                    category: "Input",
                },
                HelpEntry {
                    key: "Ctrl+E",
                    description: "Edit in $EDITOR",
                    category: "Input",
                },
                HelpEntry {
                    key: "Ctrl+V",
                    description: "Paste from clipboard",
                    category: "Input",
                },
            ],
            HelpContext::Scroll => vec![
                HelpEntry {
                    key: "j/k",
                    description: "Scroll up/down",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "g/G",
                    description: "Go to top/bottom",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "PgUp/PgDn",
                    description: "Page up/down",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "v",
                    description: "Enter selection mode",
                    category: "Selection",
                },
                HelpEntry {
                    key: "y",
                    description: "Copy last response",
                    category: "Clipboard",
                },
                HelpEntry {
                    key: "Click",
                    description: "Click code block to copy",
                    category: "Clipboard",
                },
                HelpEntry {
                    key: "o",
                    description: "Expand/collapse tool output",
                    category: "View",
                },
                HelpEntry {
                    key: "/",
                    description: "Search messages",
                    category: "Search",
                },
                HelpEntry {
                    key: "i",
                    description: "Return to input mode",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "Esc",
                    description: "Return to input mode",
                    category: "Navigation",
                },
            ],
            HelpContext::Select => vec![
                HelpEntry {
                    key: "j/k",
                    description: "Select prev/next message",
                    category: "Selection",
                },
                HelpEntry {
                    key: "y",
                    description: "Copy and exit",
                    category: "Clipboard",
                },
                HelpEntry {
                    key: "Enter",
                    description: "Copy and stay",
                    category: "Clipboard",
                },
                HelpEntry {
                    key: "o",
                    description: "Expand/collapse tools",
                    category: "View",
                },
                HelpEntry {
                    key: "Esc",
                    description: "Exit selection mode",
                    category: "Navigation",
                },
            ],
            HelpContext::Search => vec![
                HelpEntry {
                    key: "Type",
                    description: "Enter search query",
                    category: "Search",
                },
                HelpEntry {
                    key: "n",
                    description: "Next match",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "N",
                    description: "Previous match",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "Enter",
                    description: "Go to match and close",
                    category: "Navigation",
                },
                HelpEntry {
                    key: "Esc",
                    description: "Cancel search",
                    category: "Navigation",
                },
            ],
            HelpContext::Waiting => vec![HelpEntry {
                key: "Esc",
                description: "Cancel request",
                category: "Control",
            }],
        }
    }

    /// Get title for current context.
    fn get_title(&self) -> &'static str {
        match self.context {
            HelpContext::Input => "Input Mode",
            HelpContext::Scroll => "Scroll Mode",
            HelpContext::Select => "Selection Mode",
            HelpContext::Search => "Search Mode",
            HelpContext::Waiting => "Waiting",
        }
    }

    /// Render the help overlay.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        let entries = self.get_entries();
        if entries.is_empty() {
            return;
        }

        // Calculate overlay size
        let max_key_len = entries.iter().map(|e| e.key.len()).max().unwrap_or(5);
        let max_desc_len = entries
            .iter()
            .map(|e| e.description.len())
            .max()
            .unwrap_or(20);
        let content_width = max_key_len + 3 + max_desc_len + 6; // padding
        let content_height = entries.len() as u16 + 4; // entries + title + borders + hint

        let overlay_width = (content_width as u16)
            .min(area.width.saturating_sub(4))
            .max(35);
        let overlay_height = content_height.min(area.height.saturating_sub(4)).max(6);

        // Position in bottom-right corner
        let x = area.x + area.width.saturating_sub(overlay_width + 2);
        let y = area.y + area.height.saturating_sub(overlay_height + 2);
        let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        // Build content
        let mut lines: Vec<Line> = vec![];

        // Group entries by category
        let mut current_category = "";
        for entry in &entries {
            if entry.category != current_category {
                if !current_category.is_empty() {
                    lines.push(Line::from("")); // Separator
                }
                current_category = entry.category;
            }

            let key_span = Span::styled(
                format!(" {:>width$}", entry.key, width = max_key_len),
                theme.accent_style().add_modifier(Modifier::BOLD),
            );
            let sep_span = Span::styled("  ", theme.muted_style());
            let desc_span = Span::styled(entry.description, theme.text_style());

            lines.push(Line::from(vec![key_span, sep_span, desc_span]));
        }

        // Add dismiss hint at bottom
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Press any key to dismiss",
            theme.dim_style(),
        )));

        let block = Block::default()
            .title(Span::styled(
                format!(" {} Help ", self.get_title()),
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
