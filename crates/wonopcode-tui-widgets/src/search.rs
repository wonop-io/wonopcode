//! Search widget for searching conversation history.
//!
//! Provides fuzzy search across messages and tool outputs with
//! navigation between matches.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

use wonopcode_tui_core::Theme;

/// A search match result.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Index of the message containing the match.
    pub message_index: usize,
    /// Whether the match is in tool output (vs message content).
    pub in_tool: bool,
    /// Tool index if in_tool is true.
    pub tool_index: Option<usize>,
    /// Preview of the matched text (with context).
    pub preview: String,
}

/// Search widget state.
#[derive(Debug, Clone, Default)]
pub struct SearchWidget {
    /// Whether search is active.
    active: bool,
    /// Current search query.
    query: String,
    /// Search results.
    matches: Vec<SearchMatch>,
    /// Currently selected match index.
    current_match: usize,
    /// Cursor position in query.
    cursor: usize,
}

impl SearchWidget {
    /// Create a new search widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Activate search mode.
    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
        self.cursor = 0;
    }

    /// Deactivate search mode.
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Check if search is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the current query.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Get current match index.
    pub fn current_match_index(&self) -> usize {
        self.current_match
    }

    /// Get total match count.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Get the current match if any.
    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current_match)
    }

    /// Get all matches.
    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    /// Insert a character at cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.query.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete character before cursor.
    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            let prev = self.prev_char_boundary(self.cursor);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    /// Delete character at cursor.
    pub fn delete_char_forward(&mut self) {
        if self.cursor < self.query.len() {
            let next = self.next_char_boundary(self.cursor);
            self.query.drain(self.cursor..next);
        }
    }

    /// Move cursor left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary(self.cursor);
        }
    }

    /// Move cursor right.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor = self.next_char_boundary(self.cursor);
        }
    }

    /// Get the byte index of the previous character boundary.
    fn prev_char_boundary(&self, byte_idx: usize) -> usize {
        if byte_idx == 0 {
            return 0;
        }
        let mut idx = byte_idx - 1;
        while idx > 0 && !self.query.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    /// Get the byte index of the next character boundary.
    fn next_char_boundary(&self, byte_idx: usize) -> usize {
        if byte_idx >= self.query.len() {
            return self.query.len();
        }
        let mut idx = byte_idx + 1;
        while idx < self.query.len() && !self.query.is_char_boundary(idx) {
            idx += 1;
        }
        idx
    }

    /// Get the character at the given byte index.
    fn char_at(&self, byte_idx: usize) -> Option<char> {
        if byte_idx >= self.query.len() {
            return None;
        }
        self.query[byte_idx..].chars().next()
    }

    /// Move to start of query.
    pub fn cursor_start(&mut self) {
        self.cursor = 0;
    }

    /// Move to end of query.
    pub fn cursor_end(&mut self) {
        self.cursor = self.query.len();
    }

    /// Clear the query.
    pub fn clear(&mut self) {
        self.query.clear();
        self.cursor = 0;
        self.matches.clear();
        self.current_match = 0;
    }

    /// Go to next match.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    /// Go to previous match.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }

    /// Update search results.
    pub fn set_matches(&mut self, matches: Vec<SearchMatch>) {
        self.matches = matches;
        self.current_match = 0;
    }

    /// Render the search bar.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.active || area.height == 0 {
            return;
        }

        // Clear background
        frame.render_widget(Clear, area);

        // Build the search line
        let mut spans = vec![];

        // Search icon
        spans.push(Span::styled(
            " / ",
            theme.accent_style().add_modifier(Modifier::BOLD),
        ));

        // Query with cursor (cursor is a byte index)
        let cursor_pos = self.cursor.min(self.query.len());
        let query_before = &self.query[..cursor_pos];
        let cursor_char = self
            .char_at(cursor_pos)
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let query_after = if cursor_pos < self.query.len() {
            let next_pos = self.next_char_boundary(cursor_pos);
            &self.query[next_pos..]
        } else {
            ""
        };

        spans.push(Span::styled(query_before, theme.text_style()));
        spans.push(Span::styled(
            cursor_char,
            theme.text_style().add_modifier(Modifier::REVERSED),
        ));
        spans.push(Span::styled(query_after, theme.text_style()));

        // Match count
        if !self.query.is_empty() {
            spans.push(Span::styled("  ", theme.text_style()));
            if self.matches.is_empty() {
                spans.push(Span::styled("No matches", theme.error_style()));
            } else {
                spans.push(Span::styled(
                    format!("{}/{}", self.current_match + 1, self.matches.len()),
                    theme.muted_style(),
                ));
            }
        }

        // Hints
        let hints_text = " │ n:next  N:prev  Enter:go  Esc:close";
        let available_width = area.width as usize;
        let current_width: usize = spans.iter().map(|s| s.content.len()).sum();

        if current_width + hints_text.len() < available_width {
            let padding = available_width - current_width - hints_text.len();
            spans.push(Span::styled(" ".repeat(padding), theme.text_style()));
            spans.push(Span::styled(hints_text, theme.muted_style()));
        }

        let line = Line::from(spans);
        let para = Paragraph::new(line).style(theme.element_style());

        frame.render_widget(para, area);
    }

    /// Get the height needed for this widget.
    pub fn height(&self) -> u16 {
        if self.active {
            1
        } else {
            0
        }
    }
}

/// Perform fuzzy search on a string.
pub fn fuzzy_match(query: &str, text: &str) -> bool {
    if query.is_empty() {
        return false;
    }

    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    // Simple substring match for now
    text_lower.contains(&query_lower)
}

/// Extract a preview snippet around a match.
pub fn extract_preview(text: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    if let Some(pos) = text_lower.find(&query_lower) {
        let start = pos.saturating_sub(max_len / 4);
        let end = (pos + query.len() + max_len / 2).min(text.len());

        let mut preview = String::new();
        if start > 0 {
            preview.push_str("...");
        }
        preview.push_str(&text[start..end]);
        if end < text.len() {
            preview.push_str("...");
        }
        preview
    } else {
        text.chars().take(max_len).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SearchMatch tests

    #[test]
    fn test_search_match_clone() {
        let m = SearchMatch {
            message_index: 5,
            in_tool: true,
            tool_index: Some(2),
            preview: "test preview".to_string(),
        };
        let cloned = m.clone();
        assert_eq!(cloned.message_index, 5);
        assert!(cloned.in_tool);
        assert_eq!(cloned.tool_index, Some(2));
        assert_eq!(cloned.preview, "test preview");
    }

    #[test]
    fn test_search_match_debug() {
        let m = SearchMatch {
            message_index: 0,
            in_tool: false,
            tool_index: None,
            preview: "test".to_string(),
        };
        let debug = format!("{:?}", m);
        assert!(debug.contains("SearchMatch"));
    }

    // SearchWidget tests

    #[test]
    fn test_search_widget_new() {
        let widget = SearchWidget::new();
        assert!(!widget.is_active());
        assert!(widget.query().is_empty());
        assert_eq!(widget.match_count(), 0);
    }

    #[test]
    fn test_search_widget_default() {
        let widget = SearchWidget::default();
        assert!(!widget.is_active());
    }

    #[test]
    fn test_search_widget_activate_deactivate() {
        let mut widget = SearchWidget::new();
        widget.insert_char('t');
        widget.insert_char('e');

        widget.activate();
        assert!(widget.is_active());
        assert!(widget.query().is_empty()); // Activation clears query
        assert_eq!(widget.cursor, 0);

        widget.deactivate();
        assert!(!widget.is_active());
    }

    #[test]
    fn test_search_widget_insert_char() {
        let mut widget = SearchWidget::new();
        widget.activate();
        widget.insert_char('h');
        widget.insert_char('e');
        widget.insert_char('l');
        widget.insert_char('l');
        widget.insert_char('o');
        assert_eq!(widget.query(), "hello");
        assert_eq!(widget.cursor, 5);
    }

    #[test]
    fn test_search_widget_delete_char() {
        let mut widget = SearchWidget::new();
        widget.activate();
        widget.insert_char('a');
        widget.insert_char('b');
        widget.insert_char('c');
        assert_eq!(widget.query(), "abc");

        widget.delete_char();
        assert_eq!(widget.query(), "ab");
        assert_eq!(widget.cursor, 2);
    }

    #[test]
    fn test_search_widget_delete_char_forward() {
        let mut widget = SearchWidget::new();
        widget.activate();
        widget.insert_char('a');
        widget.insert_char('b');
        widget.insert_char('c');
        widget.cursor_start();
        widget.delete_char_forward();
        assert_eq!(widget.query(), "bc");
    }

    #[test]
    fn test_search_widget_cursor_movement() {
        let mut widget = SearchWidget::new();
        widget.activate();
        widget.insert_char('a');
        widget.insert_char('b');
        widget.insert_char('c');
        assert_eq!(widget.cursor, 3);

        widget.cursor_left();
        assert_eq!(widget.cursor, 2);

        widget.cursor_left();
        assert_eq!(widget.cursor, 1);

        widget.cursor_right();
        assert_eq!(widget.cursor, 2);

        widget.cursor_start();
        assert_eq!(widget.cursor, 0);

        widget.cursor_end();
        assert_eq!(widget.cursor, 3);
    }

    #[test]
    fn test_search_widget_clear() {
        let mut widget = SearchWidget::new();
        widget.activate();
        widget.insert_char('x');
        widget.insert_char('y');
        widget.clear();
        assert!(widget.query().is_empty());
        assert_eq!(widget.cursor, 0);
    }

    #[test]
    fn test_search_widget_matches() {
        let mut widget = SearchWidget::new();
        widget.activate();

        let matches = vec![
            SearchMatch {
                message_index: 0,
                in_tool: false,
                tool_index: None,
                preview: "match 1".to_string(),
            },
            SearchMatch {
                message_index: 1,
                in_tool: true,
                tool_index: Some(0),
                preview: "match 2".to_string(),
            },
        ];
        widget.set_matches(matches);

        assert_eq!(widget.match_count(), 2);
        assert_eq!(widget.current_match_index(), 0);
        assert!(widget.current_match().is_some());
        assert_eq!(widget.matches().len(), 2);
    }

    #[test]
    fn test_search_widget_next_prev_match() {
        let mut widget = SearchWidget::new();
        let matches = vec![
            SearchMatch {
                message_index: 0,
                in_tool: false,
                tool_index: None,
                preview: "1".to_string(),
            },
            SearchMatch {
                message_index: 1,
                in_tool: false,
                tool_index: None,
                preview: "2".to_string(),
            },
            SearchMatch {
                message_index: 2,
                in_tool: false,
                tool_index: None,
                preview: "3".to_string(),
            },
        ];
        widget.set_matches(matches);

        assert_eq!(widget.current_match_index(), 0);
        widget.next_match();
        assert_eq!(widget.current_match_index(), 1);
        widget.next_match();
        assert_eq!(widget.current_match_index(), 2);
        widget.next_match(); // Should wrap
        assert_eq!(widget.current_match_index(), 0);

        widget.prev_match(); // Should wrap backward
        assert_eq!(widget.current_match_index(), 2);
        widget.prev_match();
        assert_eq!(widget.current_match_index(), 1);
    }

    #[test]
    fn test_search_widget_height() {
        let mut widget = SearchWidget::new();
        assert_eq!(widget.height(), 0);

        widget.activate();
        assert_eq!(widget.height(), 1);

        widget.deactivate();
        assert_eq!(widget.height(), 0);
    }

    #[test]
    fn test_search_widget_clone() {
        let mut widget = SearchWidget::new();
        widget.activate();
        widget.insert_char('t');
        let cloned = widget.clone();
        assert!(cloned.is_active());
        assert_eq!(cloned.query(), "t");
    }

    #[test]
    fn test_search_widget_debug() {
        let widget = SearchWidget::new();
        let debug = format!("{:?}", widget);
        assert!(debug.contains("SearchWidget"));
    }

    // fuzzy_match tests

    #[test]
    fn test_fuzzy_match_empty_query() {
        assert!(!fuzzy_match("", "hello world"));
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        assert!(fuzzy_match("hello", "Hello World"));
        assert!(fuzzy_match("HELLO", "hello world"));
        assert!(fuzzy_match("HeLLo", "hElLo WoRlD"));
    }

    #[test]
    fn test_fuzzy_match_substring() {
        assert!(fuzzy_match("world", "hello world"));
        assert!(fuzzy_match("lo wo", "hello world"));
        assert!(!fuzzy_match("xyz", "hello world"));
    }

    // extract_preview tests

    #[test]
    fn test_extract_preview_with_match() {
        let preview = extract_preview("the quick brown fox jumps over the lazy dog", "fox", 30);
        assert!(preview.contains("fox"));
    }

    #[test]
    fn test_extract_preview_no_match() {
        let preview = extract_preview("hello world", "xyz", 20);
        // Should return truncated original
        assert!(!preview.is_empty());
    }

    #[test]
    fn test_extract_preview_short_text() {
        let preview = extract_preview("short", "short", 100);
        assert_eq!(preview, "short");
    }
}
