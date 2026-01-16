//! Simple text input dialog for things like rename.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::theme::Theme;

use super::common::centered_rect;

/// Simple text input dialog for things like rename.
#[derive(Debug, Clone, Default)]
pub struct InputDialog {
    /// Dialog title.
    pub title: String,
    /// Input prompt/label.
    pub prompt: String,
    /// Current input value.
    pub value: String,
    /// Cursor position.
    cursor: usize,
}

impl InputDialog {
    /// Create a new input dialog.
    pub fn new(title: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            value: String::new(),
            cursor: 0,
        }
    }

    /// Create with an initial value.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.len();
        self
    }

    /// Get the current value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Handle a key event. Returns Some(value) on Enter, None on Escape.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<InputDialogResult> {
        match key.code {
            KeyCode::Enter => {
                return Some(InputDialogResult::Submit(self.value.clone()));
            }
            KeyCode::Esc => {
                return Some(InputDialogResult::Cancel);
            }
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.value.len();
            }
            _ => {}
        }
        None
    }

    /// Render the input dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 50.min(area.width.saturating_sub(4));
        let dialog_height = 7;
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout: prompt, input field, help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Prompt
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Input
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Help
            ])
            .split(inner);

        // Prompt
        let prompt = Paragraph::new(Span::styled(&self.prompt, theme.text_style()));
        frame.render_widget(prompt, chunks[0]);

        // Input field with cursor
        let display_value = if self.cursor < self.value.len() {
            let (before, after) = self.value.split_at(self.cursor);
            let (cursor_char, rest) = after.split_at(1);
            Line::from(vec![
                Span::styled(before, theme.text_style()),
                Span::styled(
                    cursor_char,
                    Style::default().bg(theme.primary).fg(theme.background),
                ),
                Span::styled(rest, theme.text_style()),
            ])
        } else {
            Line::from(vec![
                Span::styled(&self.value, theme.text_style()),
                Span::styled(" ", Style::default().bg(theme.primary)),
            ])
        };
        let input = Paragraph::new(display_value);
        frame.render_widget(input, chunks[2]);

        // Help text
        let help = Paragraph::new(Line::from(vec![
            Span::styled("Enter", theme.highlight_style()),
            Span::styled(" confirm  ", theme.dim_style()),
            Span::styled("Esc", theme.highlight_style()),
            Span::styled(" cancel", theme.dim_style()),
        ]));
        frame.render_widget(help, chunks[4]);
    }
}

/// Result from input dialog.
#[derive(Debug, Clone)]
pub enum InputDialogResult {
    /// User submitted a value.
    Submit(String),
    /// User cancelled.
    Cancel,
}
