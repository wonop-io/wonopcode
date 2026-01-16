//! Common dialog components and utilities.
//!
//! This module contains shared types and helpers used by various dialog widgets,
//! including selectable items, filterable selection dialogs, and layout utilities.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theme::Theme;

/// A selectable item in a dialog.
#[derive(Debug, Clone)]
pub struct DialogItem {
    /// Unique identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional keybind hint.
    pub keybind: Option<String>,
    /// Optional category.
    pub category: Option<String>,
}

impl DialogItem {
    /// Create a new dialog item.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            keybind: None,
            category: None,
        }
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a keybind hint.
    pub fn with_keybind(mut self, keybind: impl Into<String>) -> Self {
        self.keybind = Some(keybind.into());
        self
    }

    /// Add a category.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// A filterable selection dialog.
#[derive(Debug, Clone)]
pub struct SelectDialog {
    /// Title of the dialog.
    title: String,
    /// All items (unfiltered).
    items: Vec<DialogItem>,
    /// Filtered items (indices into items).
    filtered: Vec<usize>,
    /// Current filter text.
    filter: String,
    /// Selected index in filtered list.
    selected: usize,
    /// List state for rendering.
    list_state: ListState,
}

impl SelectDialog {
    /// Create a new select dialog.
    pub fn new(title: impl Into<String>, items: Vec<DialogItem>) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            title: title.into(),
            items,
            filtered,
            filter: String::new(),
            selected: 0,
            list_state,
        }
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&DialogItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.items.get(idx))
    }

    /// Handle a key event. Returns Some(id) if an item was selected.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Enter => {
                return self.selected_item().map(|item| item.id.clone());
            }
            KeyCode::Up | KeyCode::BackTab => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if self.selected < self.filtered.len().saturating_sub(1) {
                    self.selected += 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Home => {
                self.selected = 0;
                self.list_state.select(Some(0));
            }
            KeyCode::End => {
                self.selected = self.filtered.len().saturating_sub(1);
                self.list_state.select(Some(self.selected));
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match c {
                        'n' => {
                            if self.selected < self.filtered.len().saturating_sub(1) {
                                self.selected += 1;
                                self.list_state.select(Some(self.selected));
                            }
                        }
                        'p' => {
                            if self.selected > 0 {
                                self.selected -= 1;
                                self.list_state.select(Some(self.selected));
                            }
                        }
                        _ => {}
                    }
                } else {
                    self.filter.push(c);
                    self.update_filter();
                }
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.update_filter();
            }
            _ => {}
        }
        None
    }

    /// Update the filtered list based on current filter.
    fn update_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.label.to_lowercase().contains(&filter_lower)
                        || item
                            .description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&filter_lower))
                            .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Reset selection
        self.selected = 0;
        self.list_state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    /// Render the dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.render_with_sections(frame, area, theme, false);
    }

    /// Render the dialog with optional section headers.
    pub fn render_with_sections(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        show_sections: bool,
    ) {
        // Calculate dialog size (centered, 60% width, max 80 chars)
        let dialog_width = (area.width * 60 / 100).clamp(40, 80);
        let dialog_height = (area.height * 70 / 100).clamp(10, 30);

        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        // Clear the area behind the dialog
        frame.render_widget(Clear, dialog_area);

        // Dialog block
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into filter input and list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(inner);

        // Render filter input
        let filter_block = Block::default()
            .title(" Filter ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let filter_text = if self.filter.is_empty() {
            Line::from(Span::styled("Type to filter...", theme.dim_style()))
        } else {
            Line::from(Span::styled(&self.filter, theme.text_style()))
        };

        let filter_para = Paragraph::new(filter_text).block(filter_block);
        frame.render_widget(filter_para, chunks[0]);

        // Build list items with optional section headers
        let mut list_items: Vec<ListItem> = Vec::new();
        let mut current_category: Option<String> = None;
        let mut visual_to_filtered: Vec<Option<usize>> = Vec::new(); // Maps visual index to filtered index (None for headers)

        for (filtered_idx, &item_idx) in self.filtered.iter().enumerate() {
            let item = &self.items[item_idx];

            // Add section header if category changed and sections are enabled
            if show_sections {
                let item_category = item.category.clone();
                if item_category != current_category {
                    if let Some(ref cat) = item_category {
                        // Add section header
                        let header = ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("── {cat} "),
                                Style::default()
                                    .fg(theme.accent)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("─".repeat(30), Style::default().fg(theme.border_subtle)),
                        ]));
                        list_items.push(header);
                        visual_to_filtered.push(None); // Header, not selectable
                    }
                    current_category = item_category;
                }
            }

            // Add the actual item
            let mut spans = vec![Span::styled(&item.label, theme.text_style())];

            if let Some(desc) = &item.description {
                spans.push(Span::styled(" - ", theme.dim_style()));
                spans.push(Span::styled(desc, theme.dim_style()));
            }

            if let Some(kb) = &item.keybind {
                spans.push(Span::styled(format!("  [{kb}]"), theme.highlight_style()));
            }

            list_items.push(ListItem::new(Line::from(spans)));
            visual_to_filtered.push(Some(filtered_idx));
        }

        // Find the visual index for the current selection
        let visual_selected = visual_to_filtered
            .iter()
            .position(|&f| f == Some(self.selected))
            .unwrap_or(0);

        let mut visual_list_state = ListState::default();
        visual_list_state.select(Some(visual_selected));

        let list = List::new(list_items)
            .highlight_style(
                Style::default()
                    .bg(theme.border_active)
                    .fg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, chunks[1], &mut visual_list_state);
    }
}

/// Helper to create a centered rectangle.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
