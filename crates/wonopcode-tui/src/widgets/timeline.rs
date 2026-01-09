//! Session timeline widget.
//!
//! Displays a git-like timeline of conversation messages that users
//! can navigate through to jump to specific points in the conversation.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theme::Theme;

/// A point in the conversation timeline.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    /// Unique message ID.
    pub id: String,
    /// Message index in the conversation.
    pub index: usize,
    /// Preview of the message content.
    pub preview: String,
    /// Timestamp string.
    pub timestamp: String,
    /// Whether this is a user or assistant message.
    pub is_user: bool,
    /// Optional tool summary (e.g., "3 tool calls").
    pub tool_summary: Option<String>,
}

impl TimelineEntry {
    /// Create a new user timeline entry.
    pub fn user(
        id: impl Into<String>,
        index: usize,
        preview: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            index,
            preview: preview.into(),
            timestamp: timestamp.into(),
            is_user: true,
            tool_summary: None,
        }
    }

    /// Create a new assistant timeline entry.
    pub fn assistant(
        id: impl Into<String>,
        index: usize,
        preview: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            index,
            preview: preview.into(),
            timestamp: timestamp.into(),
            is_user: false,
            tool_summary: None,
        }
    }

    /// Add tool summary.
    pub fn with_tools(mut self, summary: impl Into<String>) -> Self {
        self.tool_summary = Some(summary.into());
        self
    }

    /// Truncate preview to max length.
    fn truncated_preview(&self, max_len: usize) -> String {
        let preview = self.preview.replace('\n', " ");
        if preview.len() > max_len {
            format!("{}...", &preview[..max_len.saturating_sub(3)])
        } else {
            preview
        }
    }
}

/// Timeline widget for session navigation.
#[derive(Debug, Clone, Default)]
pub struct TimelineWidget {
    /// Timeline entries.
    entries: Vec<TimelineEntry>,
    /// Selected index.
    selected: usize,
    /// List state for rendering.
    list_state: ListState,
    /// Filter text.
    filter: String,
    /// Filtered indices.
    filtered: Vec<usize>,
    /// Whether the widget is visible.
    visible: bool,
}

impl TimelineWidget {
    /// Create a new timeline widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the timeline entries.
    pub fn set_entries(&mut self, entries: Vec<TimelineEntry>) {
        self.entries = entries;
        self.update_filtered();
        self.selected = 0;
        if !self.filtered.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    /// Add an entry to the timeline.
    pub fn add_entry(&mut self, entry: TimelineEntry) {
        self.entries.push(entry);
        self.update_filtered();
    }

    /// Clear the timeline.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.filtered.clear();
        self.selected = 0;
        self.filter.clear();
        self.list_state.select(None);
    }

    /// Show the timeline.
    pub fn show(&mut self) {
        self.visible = true;
        self.filter.clear();
        self.update_filtered();
        self.selected = 0;
        if !self.filtered.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    /// Hide the timeline.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Check if visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Get the selected entry.
    pub fn selected_entry(&self) -> Option<&TimelineEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.entries.get(idx))
    }

    /// Get the selected entry ID.
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_entry().map(|e| e.id.as_str())
    }

    /// Get the selected message index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_entry().map(|e| e.index)
    }

    /// Update filtered list based on current filter.
    fn update_filtered(&mut self) {
        if self.filter.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.preview.to_lowercase().contains(&filter_lower))
                .map(|(i, _)| i)
                .collect();
        }

        // Reset selection
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
        self.list_state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(self.selected)
        });
    }

    /// Handle a key event. Returns Some(message_index) if an entry was selected.
    pub fn handle_key(&mut self, key: KeyEvent) -> TimelineAction {
        if !self.visible {
            return TimelineAction::None;
        }

        match key.code {
            KeyCode::Enter => {
                if let Some(idx) = self.selected_index() {
                    self.hide();
                    return TimelineAction::Jump(idx);
                }
                TimelineAction::Handled
            }
            KeyCode::Esc => {
                self.hide();
                TimelineAction::Handled
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.list_state.select(Some(self.selected));
                }
                TimelineAction::Handled
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected < self.filtered.len().saturating_sub(1) {
                    self.selected += 1;
                    self.list_state.select(Some(self.selected));
                }
                TimelineAction::Handled
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
                self.list_state.select(Some(0));
                TimelineAction::Handled
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.filtered.len().saturating_sub(1);
                self.list_state.select(Some(self.selected));
                TimelineAction::Handled
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.update_filtered();
                TimelineAction::Handled
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.update_filtered();
                TimelineAction::Handled
            }
            _ => TimelineAction::None,
        }
    }

    /// Render the timeline widget as a dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        // Calculate dialog size (centered, 70% width, 60% height)
        let dialog_width = (area.width * 70 / 100).clamp(40, 80);
        let dialog_height = (area.height * 60 / 100).clamp(10, 30);

        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        // Clear the area behind the dialog
        frame.render_widget(Clear, dialog_area);

        // Dialog block
        let title = if self.filter.is_empty() {
            " Timeline ".to_string()
        } else {
            format!(" Timeline [{}] ", self.filter)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme.border_active_style())
            .style(Style::default().bg(theme.background_panel));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        if self.filtered.is_empty() {
            let empty_msg = if self.filter.is_empty() {
                "No messages in session"
            } else {
                "No matching messages"
            };
            let para = Paragraph::new(Span::styled(empty_msg, theme.muted_style()));
            frame.render_widget(para, inner);
            return;
        }

        // Build list items
        let max_preview_len = (inner.width as usize).saturating_sub(20);
        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .map(|&idx| {
                let entry = &self.entries[idx];
                let role_icon = if entry.is_user { ">" } else { "<" };
                let role_style = if entry.is_user {
                    theme.primary_style()
                } else {
                    theme.secondary_style()
                };

                let mut spans = vec![
                    Span::styled(role_icon, role_style),
                    Span::styled(" ", theme.text_style()),
                    Span::styled(entry.truncated_preview(max_preview_len), theme.text_style()),
                ];

                // Add tool summary if present
                if let Some(ref tools) = entry.tool_summary {
                    spans.push(Span::styled(format!(" [{tools}]"), theme.muted_style()));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, inner, &mut self.list_state);
    }
}

/// Action returned from timeline key handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineAction {
    /// No action taken.
    None,
    /// Key was handled, no selection.
    Handled,
    /// Jump to message at index.
    Jump(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeline_entries() {
        let mut timeline = TimelineWidget::new();
        timeline.set_entries(vec![
            TimelineEntry::user("msg1", 0, "Hello, world!", "10:00"),
            TimelineEntry::assistant("msg2", 1, "Hi there!", "10:01").with_tools("2 tools"),
            TimelineEntry::user("msg3", 2, "Fix the bug", "10:02"),
        ]);

        assert_eq!(timeline.entries.len(), 3);
        assert_eq!(timeline.filtered.len(), 3);
    }

    #[test]
    fn test_timeline_filter() {
        let mut timeline = TimelineWidget::new();
        timeline.set_entries(vec![
            TimelineEntry::user("msg1", 0, "Hello, world!", "10:00"),
            TimelineEntry::assistant("msg2", 1, "Hi there!", "10:01"),
            TimelineEntry::user("msg3", 2, "Fix the bug", "10:02"),
        ]);

        timeline.filter = "bug".to_string();
        timeline.update_filtered();

        assert_eq!(timeline.filtered.len(), 1);
        assert_eq!(timeline.filtered[0], 2);
    }

    #[test]
    fn test_timeline_selection() {
        let mut timeline = TimelineWidget::new();
        timeline.set_entries(vec![
            TimelineEntry::user("msg1", 0, "Hello", "10:00"),
            TimelineEntry::user("msg2", 1, "World", "10:01"),
        ]);

        assert_eq!(timeline.selected_index(), Some(0));

        timeline.selected = 1;
        assert_eq!(timeline.selected_index(), Some(1));
    }

    #[test]
    fn test_truncated_preview() {
        let entry = TimelineEntry::user(
            "id",
            0,
            "This is a very long message that should be truncated",
            "10:00",
        );
        let truncated = entry.truncated_preview(20);
        assert!(truncated.len() <= 20);
        assert!(truncated.ends_with("..."));
    }
}
