//! Timeline dialog for viewing message history and navigation.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theme::Theme;

use super::common::centered_rect;

/// Timeline item representing a message in the conversation.
#[derive(Debug, Clone)]
pub struct TimelineItem {
    /// Message ID.
    pub id: String,
    /// Role (user/assistant).
    pub role: String,
    /// Preview of the message content.
    pub preview: String,
    /// Timestamp or relative time.
    pub time: String,
    /// Whether this is a tool call.
    pub is_tool: bool,
}

impl TimelineItem {
    /// Create a new timeline item.
    pub fn new(id: impl Into<String>, role: impl Into<String>, preview: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: role.into(),
            preview: preview.into(),
            time: String::new(),
            is_tool: false,
        }
    }

    /// Set the timestamp.
    pub fn with_time(mut self, time: impl Into<String>) -> Self {
        self.time = time.into();
        self
    }

    /// Mark as a tool call.
    pub fn as_tool(mut self) -> Self {
        self.is_tool = true;
        self
    }
}

/// Timeline dialog for viewing message history and navigation.
#[derive(Debug, Clone)]
pub struct TimelineDialog {
    /// Timeline items.
    items: Vec<TimelineItem>,
    /// Selected index.
    selected: usize,
    /// List state for rendering.
    list_state: ListState,
}

impl TimelineDialog {
    /// Create a new timeline dialog with the given items.
    pub fn new(items: Vec<TimelineItem>) -> Self {
        let mut list_state = ListState::default();
        if !items.is_empty() {
            // Start at the bottom (most recent)
            list_state.select(Some(items.len().saturating_sub(1)));
        }

        Self {
            selected: items.len().saturating_sub(1),
            items,
            list_state,
        }
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&TimelineItem> {
        self.items.get(self.selected)
    }

    /// Handle a key event. Returns Some(action) if an action was triggered.
    /// Actions: `goto:<id>` for navigation, `fork:<id>` for forking.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Enter => {
                // Go to the selected message
                return self.selected_item().map(|item| format!("goto:{}", item.id));
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                // Fork from the selected message
                return self.selected_item().map(|item| format!("fork:{}", item.id));
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                if self.selected < self.items.len().saturating_sub(1) {
                    self.selected += 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
                self.list_state.select(Some(0));
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.items.len().saturating_sub(1);
                self.list_state.select(Some(self.selected));
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(10);
                self.list_state.select(Some(self.selected));
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + 10).min(self.items.len().saturating_sub(1));
                self.list_state.select(Some(self.selected));
            }
            _ => {}
        }
        None
    }

    /// Render the timeline dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 60 / 100).clamp(45, 70);
        let dialog_height = (area.height * 80 / 100).clamp(12, 30);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Message Timeline ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into list and help text
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        // Render timeline list
        let list_items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                // Role indicator
                let role_style = if item.role == "user" {
                    Style::default().fg(theme.primary)
                } else if item.is_tool {
                    Style::default().fg(theme.accent)
                } else {
                    theme.text_style()
                };

                let role_icon = if item.role == "user" {
                    "▸"
                } else if item.is_tool {
                    "◇"
                } else {
                    "◂"
                };

                // Message number
                let num = format!("{:3}", idx + 1);

                // Truncate preview if needed
                let max_preview = (dialog_width as usize).saturating_sub(20);
                let preview = if item.preview.chars().count() > max_preview {
                    let t: String = item
                        .preview
                        .chars()
                        .take(max_preview.saturating_sub(3))
                        .collect();
                    format!("{t}...")
                } else {
                    item.preview.clone()
                };

                let spans = vec![
                    Span::styled(num, theme.muted_style()),
                    Span::styled(" ", theme.text_style()),
                    Span::styled(role_icon, role_style),
                    Span::styled(" ", theme.text_style()),
                    Span::styled(preview, theme.text_style()),
                ];

                // Add time if present
                let line = if !item.time.is_empty() {
                    let mut s = spans;
                    s.push(Span::styled(
                        format!("  {}", item.time),
                        theme.muted_style(),
                    ));
                    Line::from(s)
                } else {
                    Line::from(spans)
                };

                ListItem::new(line)
            })
            .collect();

        let list = List::new(list_items)
            .highlight_style(
                Style::default()
                    .bg(theme.border_active)
                    .fg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

        // Render help text
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", theme.highlight_style()),
            Span::styled(" go to message  ", theme.dim_style()),
            Span::styled("f", theme.highlight_style()),
            Span::styled(" fork from here  ", theme.dim_style()),
            Span::styled("Esc", theme.highlight_style()),
            Span::styled(" close", theme.dim_style()),
        ])];
        let help_para = Paragraph::new(help_lines).alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[1]);
    }
}
