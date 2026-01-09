//! File autocomplete widget.
//!
//! Provides autocomplete suggestions for file paths when typing '@'.

use crate::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};
use std::path::PathBuf;

/// Maximum number of suggestions to show.
const MAX_SUGGESTIONS: usize = 10;

/// File autocomplete state and logic.
#[derive(Debug, Clone, Default)]
pub struct FileAutocomplete {
    /// Whether autocomplete is visible.
    visible: bool,
    /// The filter text after '@'.
    filter: String,
    /// Position in the input where '@' was typed.
    trigger_pos: usize,
    /// Current suggestions.
    suggestions: Vec<String>,
    /// Selected index.
    selected: usize,
    /// Working directory for file search.
    cwd: PathBuf,
}

impl FileAutocomplete {
    /// Create a new autocomplete.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the working directory.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    /// Check if autocomplete is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show autocomplete at the given position with initial filter.
    pub fn show(&mut self, trigger_pos: usize, filter: &str) {
        self.visible = true;
        self.trigger_pos = trigger_pos;
        self.filter = filter.to_string();
        self.selected = 0;
        self.update_suggestions();
    }

    /// Hide autocomplete.
    pub fn hide(&mut self) {
        self.visible = false;
        self.filter.clear();
        self.suggestions.clear();
        self.selected = 0;
    }

    /// Update the filter text.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.selected = 0;
        self.update_suggestions();
    }

    /// Get the trigger position (where '@' is).
    pub fn trigger_pos(&self) -> usize {
        self.trigger_pos
    }

    /// Get the current filter.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Get the selected suggestion, if any.
    pub fn selected_suggestion(&self) -> Option<&str> {
        self.suggestions.get(self.selected).map(|s| s.as_str())
    }

    /// Update suggestions based on current filter.
    fn update_suggestions(&mut self) {
        self.suggestions.clear();

        if self.cwd.as_os_str().is_empty() {
            return;
        }

        // Use ignore crate to walk files (respects .gitignore)
        let walker = ignore::WalkBuilder::new(&self.cwd)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .max_depth(Some(5)) // Limit depth for performance
            .build();

        let filter_lower = self.filter.to_lowercase();

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip the root directory itself
            if path == self.cwd {
                continue;
            }

            // Get relative path
            let rel_path = match path.strip_prefix(&self.cwd) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Skip hidden files that start with .
            if rel_path.starts_with('.') {
                continue;
            }

            // Apply fuzzy filter
            if !filter_lower.is_empty() {
                let rel_lower = rel_path.to_lowercase();
                if !fuzzy_match(&rel_lower, &filter_lower) {
                    continue;
                }
            }

            // Add directory marker
            let display = if path.is_dir() {
                format!("{rel_path}/")
            } else {
                rel_path
            };

            self.suggestions.push(display);

            if self.suggestions.len() >= MAX_SUGGESTIONS {
                break;
            }
        }

        // Sort suggestions - directories first, then alphabetically
        self.suggestions.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });
    }

    /// Handle a key event. Returns the selected suggestion if Enter is pressed.
    pub fn handle_key(&mut self, key: KeyEvent) -> AutocompleteAction {
        if !self.visible {
            return AutocompleteAction::None;
        }

        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                } else if !self.suggestions.is_empty() {
                    self.selected = self.suggestions.len() - 1;
                }
                AutocompleteAction::Handled
            }
            KeyCode::Down => {
                if self.selected < self.suggestions.len().saturating_sub(1) {
                    self.selected += 1;
                } else {
                    self.selected = 0;
                }
                AutocompleteAction::Handled
            }
            KeyCode::Tab | KeyCode::Enter => {
                if let Some(suggestion) = self.selected_suggestion() {
                    let result = suggestion.to_string();
                    self.hide();
                    AutocompleteAction::Select(result)
                } else {
                    self.hide();
                    AutocompleteAction::Handled
                }
            }
            KeyCode::Esc => {
                self.hide();
                AutocompleteAction::Handled
            }
            _ => AutocompleteAction::None,
        }
    }

    /// Render the autocomplete popup.
    pub fn render(&self, frame: &mut Frame, input_area: Rect, theme: &Theme) {
        if !self.visible || self.suggestions.is_empty() {
            return;
        }

        // Position above the input
        let height = (self.suggestions.len() as u16 + 2).min(12);
        let width = input_area.width.min(60);

        let popup_area = Rect::new(
            input_area.x,
            input_area.y.saturating_sub(height),
            width,
            height,
        );

        // Clear the area first
        frame.render_widget(Clear, popup_area);

        // Create list items
        let items: Vec<ListItem> = self
            .suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let style = if i == self.selected {
                    Style::default().fg(theme.background).bg(theme.primary)
                } else {
                    theme.text_style()
                };

                // Show icon based on type (folder vs file)
                let icon = if s.ends_with('/') { "📁 " } else { "📄 " };
                ListItem::new(Line::from(vec![
                    Span::styled(icon, style),
                    Span::styled(s.clone(), style),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background_element))
            .title(" Files ");

        let list = List::new(items).block(block);

        frame.render_widget(list, popup_area);
    }
}

/// Simple fuzzy matching - checks if all chars of needle appear in haystack in order.
fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }

    let mut needle_chars = needle.chars().peekable();

    for h in haystack.chars() {
        if let Some(&n) = needle_chars.peek() {
            if h == n {
                needle_chars.next();
            }
        }
    }

    needle_chars.peek().is_none()
}

/// Action returned from autocomplete key handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutocompleteAction {
    /// No action taken.
    None,
    /// Key was handled, no selection made.
    Handled,
    /// A suggestion was selected.
    Select(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match() {
        assert!(fuzzy_match("src/main.rs", "smr"));
        assert!(fuzzy_match("src/main.rs", "main"));
        assert!(fuzzy_match("package.json", "pj"));
        assert!(!fuzzy_match("src/main.rs", "xyz"));
        assert!(fuzzy_match("anything", ""));
    }
}
