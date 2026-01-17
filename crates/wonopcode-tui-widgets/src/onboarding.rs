//! Onboarding overlay widget for first-time users.
//!
//! Shows a welcome message and key hints on first run,
//! dismissible with any key press.

use ratatui::{
    layout::{Alignment, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use wonopcode_tui_core::Theme;

/// Onboarding overlay state.
#[derive(Debug, Clone, Default)]
pub struct OnboardingOverlay {
    /// Whether the overlay is visible.
    visible: bool,
    /// Whether this is the first time showing (for persistence).
    is_first_run: bool,
}

impl OnboardingOverlay {
    /// Create a new onboarding overlay.
    pub fn new() -> Self {
        Self::default()
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

    /// Set whether this is first run.
    pub fn set_first_run(&mut self, first_run: bool) {
        self.is_first_run = first_run;
        if first_run {
            self.visible = true;
        }
    }

    /// Render the onboarding overlay.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        // Calculate overlay size (centered, reasonable size)
        let overlay_width = 55u16.min(area.width.saturating_sub(4));
        let overlay_height = 16u16.min(area.height.saturating_sub(4));

        // Center the overlay
        let x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
        let y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
        let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

        // Clear background
        frame.render_widget(Clear, overlay_area);

        // Build content
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Welcome to Wonopcode!",
                theme.accent_style().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "A powerful AI coding assistant in your terminal.",
                theme.text_style(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Quick Start:",
                theme.text_style().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  1. ", theme.muted_style()),
                Span::styled("Type your question and press ", theme.text_style()),
                Span::styled("Enter", theme.accent_style()),
            ]),
            Line::from(vec![
                Span::styled("  2. ", theme.muted_style()),
                Span::styled("Press ", theme.text_style()),
                Span::styled("Ctrl+P", theme.accent_style()),
                Span::styled(" for commands", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("  3. ", theme.muted_style()),
                Span::styled("Press ", theme.text_style()),
                Span::styled("?", theme.accent_style()),
                Span::styled(" anytime for help", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("  4. ", theme.muted_style()),
                Span::styled("Press ", theme.text_style()),
                Span::styled("Ctrl+X", theme.accent_style()),
                Span::styled(" for quick actions", theme.text_style()),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled("Press any key to start...", theme.dim_style())),
        ];

        let block = Block::default()
            .title(Span::styled(
                " Getting Started ",
                theme.accent_style().add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(theme.border_style())
            .style(theme.panel_style());

        let para = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center);

        frame.render_widget(para, overlay_area);
    }
}
