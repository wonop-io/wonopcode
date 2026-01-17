//! File autocomplete widget.
//!
//! Provides autocomplete suggestions for file paths when typing '@'.

use wonopcode_tui_core::Theme;
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

    // AutocompleteAction tests

    #[test]
    fn test_autocomplete_action_debug() {
        let action = AutocompleteAction::None;
        let debug = format!("{:?}", action);
        assert!(debug.contains("None"));
    }

    #[test]
    fn test_autocomplete_action_clone() {
        let action = AutocompleteAction::Select("test.rs".to_string());
        let cloned = action.clone();
        assert_eq!(cloned, AutocompleteAction::Select("test.rs".to_string()));
    }

    #[test]
    fn test_autocomplete_action_eq() {
        assert_eq!(AutocompleteAction::None, AutocompleteAction::None);
        assert_eq!(AutocompleteAction::Handled, AutocompleteAction::Handled);
        assert_ne!(AutocompleteAction::None, AutocompleteAction::Handled);
        assert_eq!(
            AutocompleteAction::Select("a".to_string()),
            AutocompleteAction::Select("a".to_string())
        );
    }

    // FileAutocomplete tests

    #[test]
    fn test_file_autocomplete_new() {
        let ac = FileAutocomplete::new();
        assert!(!ac.is_visible());
        assert!(ac.filter().is_empty());
        assert!(ac.selected_suggestion().is_none());
    }

    #[test]
    fn test_file_autocomplete_default() {
        let ac = FileAutocomplete::default();
        assert!(!ac.is_visible());
    }

    #[test]
    fn test_file_autocomplete_set_cwd() {
        let mut ac = FileAutocomplete::new();
        ac.set_cwd(PathBuf::from("/home/user"));
        assert_eq!(ac.cwd, PathBuf::from("/home/user"));
    }

    #[test]
    fn test_file_autocomplete_show_hide() {
        let mut ac = FileAutocomplete::new();
        assert!(!ac.is_visible());

        ac.show(5, "test");
        assert!(ac.is_visible());
        assert_eq!(ac.trigger_pos(), 5);
        assert_eq!(ac.filter(), "test");

        ac.hide();
        assert!(!ac.is_visible());
        assert!(ac.filter().is_empty());
    }

    #[test]
    fn test_file_autocomplete_set_filter() {
        let mut ac = FileAutocomplete::new();
        ac.show(0, "initial");
        ac.set_filter("new_filter");
        assert_eq!(ac.filter(), "new_filter");
    }

    #[test]
    fn test_file_autocomplete_handle_key_escape() {
        let mut ac = FileAutocomplete::new();
        ac.show(0, "");
        assert!(ac.is_visible());

        let key = KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE);
        let action = ac.handle_key(key);
        assert_eq!(action, AutocompleteAction::Handled);
        assert!(!ac.is_visible());
    }

    #[test]
    fn test_file_autocomplete_handle_key_when_hidden() {
        let mut ac = FileAutocomplete::new();
        let key = KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::NONE);
        let action = ac.handle_key(key);
        assert_eq!(action, AutocompleteAction::None);
    }

    #[test]
    fn test_file_autocomplete_handle_key_navigation() {
        let mut ac = FileAutocomplete::new();
        ac.show(0, "");
        // Manually set some suggestions for testing
        ac.suggestions = vec!["file1.rs".to_string(), "file2.rs".to_string()];

        // Test down navigation
        let down = KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::NONE);
        ac.handle_key(down);
        assert_eq!(ac.selected, 1);

        // Test up navigation
        let up = KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::NONE);
        ac.handle_key(up);
        assert_eq!(ac.selected, 0);

        // Test wrap around up
        ac.handle_key(up);
        assert_eq!(ac.selected, 1); // Should wrap to last

        // Test wrap around down
        ac.selected = 1;
        ac.handle_key(down);
        assert_eq!(ac.selected, 0); // Should wrap to first
    }

    #[test]
    fn test_file_autocomplete_handle_key_select() {
        let mut ac = FileAutocomplete::new();
        ac.show(0, "");
        ac.suggestions = vec!["test.rs".to_string()];

        let enter = KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
        let action = ac.handle_key(enter);
        assert_eq!(action, AutocompleteAction::Select("test.rs".to_string()));
        assert!(!ac.is_visible());
    }

    #[test]
    fn test_file_autocomplete_handle_key_tab() {
        let mut ac = FileAutocomplete::new();
        ac.show(0, "");
        ac.suggestions = vec!["main.rs".to_string()];

        let tab = KeyEvent::new(KeyCode::Tab, crossterm::event::KeyModifiers::NONE);
        let action = ac.handle_key(tab);
        assert_eq!(action, AutocompleteAction::Select("main.rs".to_string()));
    }

    #[test]
    fn test_file_autocomplete_selected_suggestion() {
        let mut ac = FileAutocomplete::new();
        ac.suggestions = vec!["a.rs".to_string(), "b.rs".to_string()];
        ac.selected = 0;
        assert_eq!(ac.selected_suggestion(), Some("a.rs"));

        ac.selected = 1;
        assert_eq!(ac.selected_suggestion(), Some("b.rs"));

        ac.selected = 99;
        assert_eq!(ac.selected_suggestion(), None);
    }

    #[test]
    fn test_file_autocomplete_clone() {
        let mut ac = FileAutocomplete::new();
        ac.show(5, "filter");
        ac.suggestions = vec!["test.rs".to_string()];

        let cloned = ac.clone();
        assert!(cloned.is_visible());
        assert_eq!(cloned.trigger_pos(), 5);
        assert_eq!(cloned.filter(), "filter");
    }

    #[test]
    fn test_file_autocomplete_debug() {
        let ac = FileAutocomplete::new();
        let debug = format!("{:?}", ac);
        assert!(debug.contains("FileAutocomplete"));
    }
}
