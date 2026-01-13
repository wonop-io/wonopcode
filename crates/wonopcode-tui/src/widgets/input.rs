//! Input widget for the TUI with multi-line support and history.

use crate::metrics;
use crate::theme::{AgentMode, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use tui_textarea::TextArea;

/// Prompt history manager with optional file persistence.
#[derive(Debug, Clone, Default)]
pub struct PromptHistory {
    entries: Vec<String>,
    position: isize,
    max_size: usize,
    stashed: String,
    /// Path to the history file for persistence.
    file_path: Option<std::path::PathBuf>,
}

impl PromptHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            position: -1,
            max_size,
            stashed: String::new(),
            file_path: None,
        }
    }

    /// Create a new history manager with file persistence.
    pub fn with_file(max_size: usize, file_path: std::path::PathBuf) -> Self {
        let mut history = Self::new(max_size);
        history.file_path = Some(file_path.clone());

        // Try to load existing history
        if let Ok(content) = std::fs::read_to_string(&file_path) {
            for line in content.lines() {
                if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(input) = entry.get("input").and_then(|v| v.as_str()) {
                        if !input.trim().is_empty() {
                            history.entries.push(input.to_string());
                        }
                    }
                }
            }
            // Keep only max_size entries
            while history.entries.len() > max_size {
                history.entries.remove(0);
            }
        }

        history
    }

    /// Get the number of entries in history.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, entry: String) {
        if entry.trim().is_empty() {
            return;
        }
        // Don't add duplicate of the last entry
        if self.entries.last().map(|e| e.as_str()) == Some(&entry) {
            return;
        }
        self.entries.push(entry.clone());
        while self.entries.len() > self.max_size {
            self.entries.remove(0);
        }
        self.position = -1;
        self.stashed.clear();

        // Persist to file
        if let Some(ref path) = self.file_path {
            let json = serde_json::json!({ "input": entry });
            if let Ok(line) = serde_json::to_string(&json) {
                // Append to file
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(file, "{line}");
                }
            }
        }
    }

    pub fn previous(&mut self, current: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if self.position == -1 {
            self.stashed = current.to_string();
        }
        let max_pos = self.entries.len() as isize - 1;
        if self.position < max_pos {
            self.position += 1;
            let idx = self.entries.len() - 1 - self.position as usize;
            return Some(&self.entries[idx]);
        }
        None
    }

    /// Get the next (more recent) history entry.
    pub fn next_entry(&mut self) -> Option<&str> {
        match self.position.cmp(&0) {
            std::cmp::Ordering::Greater => {
                self.position -= 1;
                let idx = self.entries.len() - 1 - self.position as usize;
                Some(&self.entries[idx])
            }
            std::cmp::Ordering::Equal => {
                self.position = -1;
                Some(&self.stashed)
            }
            std::cmp::Ordering::Less => None,
        }
    }

    pub fn reset(&mut self) {
        self.position = -1;
        self.stashed.clear();
    }
}

/// Input widget for entering prompts using tui-textarea.
pub struct InputWidget {
    textarea: TextArea<'static>,
    focused: bool,
    placeholder: String,
    history: PromptHistory,
    agent: AgentMode,
    model: String,
    shell_mode: bool,
    /// Last known text area width for visual cursor movement calculations.
    last_text_width: usize,
    /// Counter for numbering pastes (reset on clear).
    paste_count: usize,
    /// Tracks ongoing paste for terminals that send line-by-line.
    paste_tracker: Option<PasteTracker>,
}

/// Tracks an ongoing paste operation for terminals that send line-by-line.
struct PasteTracker {
    /// Number of lines received in current paste batch.
    line_count: usize,
    /// When the first line of this paste was received.
    started: std::time::Instant,
}

impl PasteTracker {
    fn new() -> Self {
        Self {
            line_count: 1,
            started: std::time::Instant::now(),
        }
    }

    fn increment(&mut self) {
        self.line_count += 1;
    }

    fn is_expired(&self) -> bool {
        // If more than 100ms since start, consider paste complete
        self.started.elapsed() > std::time::Duration::from_millis(100)
    }

    fn line_count(&self) -> usize {
        self.line_count
    }
}

/// Minimum number of lines to trigger paste wrapping.
const PASTE_WRAP_MIN_LINES: usize = 2;

/// Opening tag for paste content.
const PASTE_TAG_OPEN: &str = "<wonopcode__paste>";
/// Closing tag for paste content.
const PASTE_TAG_CLOSE: &str = "</wonopcode__paste>";

impl Default for InputWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl InputWidget {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        Self {
            textarea,
            focused: false,
            placeholder: "Type a message...".to_string(),
            history: PromptHistory::new(100),
            agent: AgentMode::Build,
            model: String::new(),
            shell_mode: false,
            last_text_width: 80, // Default, will be updated on render
            paste_count: 0,
            paste_tracker: None,
        }
    }

    /// Create a new input widget with persistent history.
    pub fn with_history_file(history_file: std::path::PathBuf) -> Self {
        let mut widget = Self::new();
        widget.history = PromptHistory::with_file(100, history_file);
        widget
    }

    /// Set a custom history manager.
    pub fn set_history(&mut self, history: PromptHistory) {
        self.history = history;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn set_agent(&mut self, agent: AgentMode) {
        self.agent = agent;
    }

    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Get the raw text including paste tags (for internal use/rendering).
    #[cfg(test)]
    pub fn raw_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Get the raw text including paste tags (for internal use/rendering).
    #[cfg(not(test))]
    fn raw_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Get the text with paste tags removed (for submission).
    pub fn text(&self) -> String {
        strip_paste_tags(&self.raw_text())
    }

    /// Alias for text() - get the current content (with tags stripped).
    pub fn content(&self) -> String {
        self.text()
    }

    /// Alias for set_text() - set the content.
    pub fn set_content(&mut self, text: String) {
        self.set_text(&text);
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.lines().len() == 1
            && self
                .textarea
                .lines()
                .first()
                .map(|l| l.is_empty())
                .unwrap_or(true)
    }

    pub fn clear(&mut self) {
        self.textarea.select_all();
        self.textarea.delete_char();
        self.shell_mode = false;
        self.history.reset();
        self.paste_count = 0;
        self.paste_tracker = None;
    }

    pub fn take(&mut self) -> String {
        let raw = self.raw_text();
        // Store raw text with paste tags in history so it displays the same when recalled
        self.history.push(raw);
        // Return stripped text for submission (paste tags are for display only)
        let stripped = self.text();
        self.clear();
        stripped
    }

    pub fn set_text(&mut self, text: &str) {
        self.textarea.select_all();
        self.textarea.delete_char();
        self.textarea.insert_str(text);
        self.shell_mode = text.starts_with('!');
    }

    /// Insert text at the current cursor position, handling multi-line paste.
    pub fn insert_text(&mut self, text: &str) {
        self.textarea.insert_str(text);
        self.history.reset();

        // Update shell mode
        if self
            .textarea
            .lines()
            .first()
            .map(|l| l.starts_with('!'))
            .unwrap_or(false)
        {
            self.shell_mode = true;
        }
    }

    /// Insert pasted text, wrapping multi-line content in tags for display.
    pub fn insert_paste(&mut self, text: &str) {
        // Normalize line endings: \r\n -> \n, then \r -> \n
        // Some terminals (like iTerm2) send \r instead of \n
        let text = text.replace("\r\n", "\n").replace('\r', "\n");

        // Strip trailing newline if present
        let text = text.strip_suffix('\n').unwrap_or(&text);

        let line_count = text.lines().count().max(1);
        tracing::info!("insert_paste: {} lines, {} bytes", line_count, text.len());

        // Check if this is part of an ongoing paste (terminal sending line-by-line)
        if let Some(ref mut tracker) = self.paste_tracker {
            if !tracker.is_expired() {
                // Part of ongoing paste - insert newline then text
                self.textarea.insert_newline();
                self.textarea.insert_str(text);
                tracker.increment();
                self.history.reset();
                return;
            }
            // Expired - finalize previous paste if needed
            self.finalize_paste_tracking();
        }

        // Wrap multi-line pastes in tags
        if line_count >= PASTE_WRAP_MIN_LINES {
            self.paste_count += 1;
            let wrapped = format!("{PASTE_TAG_OPEN}{text}{PASTE_TAG_CLOSE}");
            tracing::info!(
                "insert_paste: wrapping {} lines in tags (paste #{})",
                line_count,
                self.paste_count
            );
            self.textarea.insert_str(&wrapped);
            self.paste_tracker = None; // Complete paste, no tracking needed
        } else {
            // Single line - insert and start tracking in case more lines come
            self.textarea.insert_str(text);
            self.paste_tracker = Some(PasteTracker::new());
        }

        self.history.reset();
        self.update_shell_mode();
    }

    /// Check if there's a tracked paste that should be finalized and wrapped.
    /// Call this on tick to wrap multi-line pastes after timeout.
    pub fn check_pending_paste(&mut self) -> bool {
        if let Some(ref tracker) = self.paste_tracker {
            if tracker.is_expired() {
                return self.finalize_paste_tracking();
            }
        }
        false
    }

    /// Finalize paste tracking - wrap content if it was multi-line.
    fn finalize_paste_tracking(&mut self) -> bool {
        if let Some(tracker) = self.paste_tracker.take() {
            if tracker.line_count() >= PASTE_WRAP_MIN_LINES {
                // Need to wrap the pasted content retroactively
                // Get all content and wrap the portion that was pasted
                let raw_text = self.textarea.lines().join("\n");

                // For simplicity, if we detected multiple lines were pasted,
                // wrap the entire current content (this works for empty-start pastes)
                // A more sophisticated approach would track exact positions
                if !raw_text.is_empty() && !raw_text.contains(PASTE_TAG_OPEN) {
                    self.paste_count += 1;
                    let wrapped = format!("{PASTE_TAG_OPEN}{raw_text}{PASTE_TAG_CLOSE}");
                    self.textarea.select_all();
                    self.textarea.delete_char();
                    self.textarea.insert_str(&wrapped);
                    return true;
                }
            }
        }
        false
    }

    /// Update shell mode based on first line content.
    fn update_shell_mode(&mut self) {
        if self
            .textarea
            .lines()
            .first()
            .map(|l| l.starts_with('!'))
            .unwrap_or(false)
        {
            self.shell_mode = true;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match c {
                        'a' => {
                            self.textarea.move_cursor(tui_textarea::CursorMove::Head);
                            // Snap outside paste regions after moving to head
                            self.snap_cursor_outside_paste_region();
                        }
                        'e' => {
                            self.textarea.move_cursor(tui_textarea::CursorMove::End);
                            // Snap outside paste regions after moving to end
                            self.snap_cursor_outside_paste_region();
                        }
                        'u' => {
                            self.textarea.delete_line_by_head();
                        }
                        'k' => {
                            self.textarea.delete_line_by_end();
                        }
                        'w' => {
                            self.textarea.delete_word();
                        }
                        'd' => {
                            self.textarea.delete_next_char();
                        }
                        'j' => {
                            self.textarea.insert_newline();
                        }
                        'p' => return InputAction::CommandPalette,
                        'c' => return InputAction::Cancel,
                        'x' => return InputAction::LeaderKey,
                        'v' => {
                            // Ctrl+V paste
                            return InputAction::Paste;
                        }
                        _ => {}
                    }
                } else if key.modifiers.contains(KeyModifiers::SUPER) {
                    // Handle Cmd+key on macOS
                    if c == 'v' {
                        // Cmd+V paste on macOS
                        return InputAction::Paste;
                    }
                } else {
                    if c == '!' && self.is_empty() {
                        self.shell_mode = true;
                    }
                    self.textarea.insert_char(c);
                    self.history.reset();
                }
            }
            KeyCode::Enter => {
                // Shift+Enter or Alt+Enter for new line
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.textarea.insert_newline();
                } else {
                    return InputAction::Submit;
                }
            }
            KeyCode::Backspace => {
                self.textarea.delete_char();
                self.history.reset();
                if self.is_empty() {
                    self.shell_mode = false;
                }
            }
            KeyCode::Delete => {
                self.textarea.delete_next_char();
            }
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.textarea
                        .move_cursor(tui_textarea::CursorMove::WordBack);
                } else {
                    // Skip over paste regions as atomic units
                    self.move_cursor_left_skip_paste();
                }
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.textarea
                        .move_cursor(tui_textarea::CursorMove::WordForward);
                } else {
                    // Skip over paste regions as atomic units
                    self.move_cursor_right_skip_paste();
                }
            }
            KeyCode::Up => {
                // Snap to start of paste region first (treat paste as single unit)
                self.snap_to_paste_start();
                // Try to move cursor up visually (handles wrapped lines)
                if self.move_cursor_up_visual() {
                    // Cursor was moved - snap outside paste regions
                    self.snap_cursor_outside_paste_region();
                } else {
                    // Already at top - navigate history
                    // Use raw_text() to preserve paste tags when stashing current content
                    let current_text = self.raw_text();
                    if let Some(prev) = self.history.previous(&current_text) {
                        let prev_owned = prev.to_string();
                        self.set_text(&prev_owned);
                    } else {
                        return InputAction::ScrollUp;
                    }
                }
            }
            KeyCode::Down => {
                // Snap to end of paste region first (treat paste as single unit)
                self.snap_to_paste_end();
                // Try to move cursor down visually (handles wrapped lines)
                if self.move_cursor_down_visual() {
                    // Cursor was moved - snap outside paste regions
                    self.snap_cursor_outside_paste_region();
                } else {
                    // Already at bottom - navigate history
                    if let Some(next) = self.history.next_entry() {
                        let next_owned = next.to_string();
                        self.set_text(&next_owned);
                    }
                }
            }
            KeyCode::Home => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.textarea.move_cursor(tui_textarea::CursorMove::Top);
                }
                self.textarea.move_cursor(tui_textarea::CursorMove::Head);
                // Snap outside paste regions after moving to head
                self.snap_cursor_outside_paste_region();
            }
            KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
                }
                self.textarea.move_cursor(tui_textarea::CursorMove::End);
                // Snap outside paste regions after moving to end
                self.snap_cursor_outside_paste_region();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // Cycle agent modes (Tab = forward, Shift+Tab/BackTab = backward)
                let reverse =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.agent = if reverse {
                    self.agent.prev()
                } else {
                    self.agent.next()
                };
                return InputAction::AgentChanged(self.agent);
            }
            KeyCode::Esc => return InputAction::Escape,
            _ => {}
        }
        InputAction::None
    }

    /// Get the number of lines in the input.
    pub fn line_count(&self) -> usize {
        self.textarea.lines().len()
    }

    /// Move cursor to a specific column on the current line.
    fn move_to_column(&mut self, col: usize) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Head);
        for _ in 0..col {
            self.textarea.move_cursor(tui_textarea::CursorMove::Forward);
        }
    }

    /// Move cursor to a specific offset in the text.
    fn move_to_offset(&mut self, offset: usize) {
        let raw_text = self.raw_text();
        let (row, col) = offset_to_cursor(&raw_text, offset);

        // Move to target row
        self.textarea.move_cursor(tui_textarea::CursorMove::Top);
        for _ in 0..row {
            self.textarea.move_cursor(tui_textarea::CursorMove::Down);
        }
        // Move to target column
        self.textarea.move_cursor(tui_textarea::CursorMove::Head);
        for _ in 0..col {
            self.textarea.move_cursor(tui_textarea::CursorMove::Forward);
        }
    }

    /// Get current cursor position as character offset.
    fn cursor_offset(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        let lines: Vec<&str> = self.textarea.lines().iter().map(|s| s.as_str()).collect();
        cursor_to_offset(&lines, row, col)
    }

    /// Move cursor right, skipping over paste regions as atomic units.
    fn move_cursor_right_skip_paste(&mut self) {
        let raw_text = self.raw_text();
        let current_offset = self.cursor_offset();

        // Check if we're at or entering a paste region
        if let Some(end_offset) = skip_paste_region_right(&raw_text, current_offset) {
            self.move_to_offset(end_offset);
        } else {
            // Normal move
            self.textarea.move_cursor(tui_textarea::CursorMove::Forward);
        }
    }

    /// Move cursor left, skipping over paste regions as atomic units.
    fn move_cursor_left_skip_paste(&mut self) {
        let raw_text = self.raw_text();
        let current_offset = self.cursor_offset();

        // First do a normal move left
        if current_offset == 0 {
            return;
        }

        // Check if we'd enter a paste region
        if let Some(start_offset) = skip_paste_region_left(&raw_text, current_offset - 1) {
            self.move_to_offset(start_offset);
        } else {
            self.textarea.move_cursor(tui_textarea::CursorMove::Back);
        }
    }

    /// Ensure cursor is not inside a paste region.
    /// If it is, snap to the nearest edge (start or end of region).
    fn snap_cursor_outside_paste_region(&mut self) {
        let raw_text = self.raw_text();
        let current_offset = self.cursor_offset();

        if let Some((start, end)) = find_containing_paste_region(&raw_text, current_offset) {
            // Cursor is inside a paste region, snap to nearest edge
            let dist_to_start = current_offset - start;
            let dist_to_end = end - current_offset;

            if dist_to_start <= dist_to_end {
                self.move_to_offset(start);
            } else {
                self.move_to_offset(end);
            }
        }
    }

    /// Snap cursor to start of paste region if inside or at end of one.
    /// Used before moving up to treat paste as single unit.
    fn snap_to_paste_start(&mut self) {
        let raw_text = self.raw_text();
        let current_offset = self.cursor_offset();

        // Check all paste regions
        for (start, end) in find_paste_regions(&raw_text) {
            // If cursor is inside or at the end of a paste region, snap to start
            // This treats the entire paste as a single unit when moving up
            if current_offset > start && current_offset <= end {
                self.move_to_offset(start);
                return;
            }
        }
    }

    /// Snap cursor to end of paste region if inside or at start of one.
    /// Used before moving down to treat paste as single unit.
    fn snap_to_paste_end(&mut self) {
        let raw_text = self.raw_text();
        let current_offset = self.cursor_offset();

        // Check all paste regions
        for (start, end) in find_paste_regions(&raw_text) {
            // If cursor is inside or at the start of a paste region, snap to end
            // This treats the entire paste as a single unit when moving down
            if current_offset >= start && current_offset < end {
                self.move_to_offset(end);
                return;
            }
        }
    }

    /// Get the wrap width used for visual row calculations.
    fn wrap_width(&self) -> usize {
        self.last_text_width.max(1)
    }

    /// Move cursor up one visual row, handling wrapped lines.
    /// Returns true if the cursor was moved, false if already at the top.
    fn move_cursor_up_visual(&mut self) -> bool {
        let (cursor_row, cursor_col) = self.textarea.cursor();
        let wrap_width = self.wrap_width();

        // Calculate position within the visual row
        let visual_col = cursor_col % wrap_width;

        // Check if we can move up within the current wrapped line
        if cursor_col >= wrap_width {
            // Move to the previous visual segment, same visual column
            let new_col = cursor_col - wrap_width;
            self.move_to_column(new_col);
            return true;
        }

        // We're on the first visual row of this logical line
        if cursor_row == 0 {
            // Already at the very top - can't move up
            return false;
        }

        // Move to the previous logical line
        self.textarea.move_cursor(tui_textarea::CursorMove::Up);
        let (new_row, _) = self.textarea.cursor();

        // Get the length of the previous line to position cursor on its last visual row
        let prev_line_len = self
            .textarea
            .lines()
            .get(new_row)
            .map(|l| l.len())
            .unwrap_or(0);

        // Calculate the start of the last visual segment
        let last_segment_start = (prev_line_len / wrap_width) * wrap_width;
        // Target column: last segment start + visual column, clamped to line length
        let target_col = (last_segment_start + visual_col).min(prev_line_len);
        self.move_to_column(target_col);
        true
    }

    /// Move cursor down one visual row, handling wrapped lines.
    /// Returns true if the cursor was moved, false if already at the bottom.
    fn move_cursor_down_visual(&mut self) -> bool {
        let (cursor_row, cursor_col) = self.textarea.cursor();
        let wrap_width = self.wrap_width();

        let current_line_len = self
            .textarea
            .lines()
            .get(cursor_row)
            .map(|l| l.len())
            .unwrap_or(0);
        let num_lines = self.textarea.lines().len();

        // Calculate which visual segment we're in
        let current_segment = cursor_col / wrap_width;
        let visual_col = cursor_col % wrap_width; // Position within the visual row

        // Calculate total visual segments for this line
        let total_segments = if current_line_len == 0 {
            1
        } else {
            current_line_len.div_ceil(wrap_width)
        };

        if current_segment + 1 < total_segments {
            // There's another visual row below in the same logical line
            let new_col = ((current_segment + 1) * wrap_width + visual_col).min(current_line_len);
            self.move_to_column(new_col);
            return true;
        }

        // We're on the last visual row of this logical line
        let last_row = num_lines.saturating_sub(1);
        if cursor_row >= last_row {
            // Already at the very bottom - can't move down
            return false;
        }

        // Move to the next logical line (first visual row)
        self.textarea.move_cursor(tui_textarea::CursorMove::Down);
        let (new_row, _) = self.textarea.cursor();

        // Position at the same visual column or end of line
        let next_line_len = self
            .textarea
            .lines()
            .get(new_row)
            .map(|l| l.len())
            .unwrap_or(0);
        let target_col = visual_col.min(next_line_len);
        self.move_to_column(target_col);
        true
    }

    /// Calculate the required height for rendering.
    /// Returns the height needed to display all lines plus the mode indicator and padding.
    pub fn height(&self) -> u16 {
        self.height_for_width(80) // Default width estimate
    }

    /// Calculate the required height for a given width, accounting for line wrapping.
    pub fn height_for_width(&self, width: u16) -> u16 {
        // Account for horizontal padding (2 cols each side) and border (1 col)
        let text_width = width.saturating_sub(5).max(1) as usize;

        // Calculate wrapped line count
        let wrapped_lines: u16 = self
            .textarea
            .lines()
            .iter()
            .map(|line| {
                if line.is_empty() {
                    1
                } else {
                    line.len().div_ceil(text_width).max(1) as u16
                }
            })
            .sum();

        let content_lines = wrapped_lines.max(1);
        // +1 for the mode indicator line, +1 for space between text and mode, +2 for vertical padding (1 top + 1 bottom)
        // Minimum height of 6, max of 15
        (content_lines + 4).clamp(6, 15)
    }

    /// Render the input widget
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let _timer = metrics::widget_timer("input");

        // Get agent color for the left border
        let agent_color = if self.shell_mode {
            theme.warning
        } else {
            theme.agent_color(self.agent)
        };

        // Main container with background
        let bg_style = Style::default().bg(theme.background_element);

        // Create the input area with left border only
        // We'll fake this by using a narrow column for the border
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(1), // Left border (just the vertical line)
                Constraint::Min(1),    // Content
            ])
            .split(area);

        // Content area with background
        let content_area = chunks[1];

        // Draw left border (limited to content area height)
        let border_area = Rect::new(
            chunks[0].x,
            chunks[0].y,
            chunks[0].width,
            content_area.height,
        );
        let border_line = "┃".repeat(content_area.height as usize);
        let border_para = Paragraph::new(border_line).style(Style::default().fg(agent_color));
        frame.render_widget(border_para, border_area);

        // Split content into text area and mode indicator (with vertical padding)
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Top padding
                Constraint::Min(1),    // Text input
                Constraint::Length(1), // Space between text and mode
                Constraint::Length(1), // Mode indicator
                Constraint::Length(1), // Bottom padding
            ])
            .split(content_area);

        // Text input area
        let text_area = content_chunks[1];

        // Fill background
        let bg_block = Block::default().style(bg_style);
        frame.render_widget(bg_block, content_area);

        // Configure textarea styling
        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.set_style(bg_style.fg(theme.text));

        if self.focused {
            self.textarea.set_cursor_style(
                Style::default()
                    .fg(theme.background)
                    .bg(theme.text)
                    .add_modifier(Modifier::BOLD),
            );
        } else {
            self.textarea.set_cursor_style(Style::default());
        }

        // Render placeholder or textarea with wrapping
        let inner_area = Rect::new(
            text_area.x + 2,
            text_area.y,
            text_area.width.saturating_sub(4),
            text_area.height,
        );

        // Store the text width for visual cursor movement calculations
        self.last_text_width = inner_area.width as usize;

        if self.is_empty() && !self.focused {
            let placeholder = Paragraph::new(Span::styled(&self.placeholder, theme.muted_style()))
                .style(bg_style);
            frame.render_widget(placeholder, inner_area);
        } else {
            // Custom wrapped rendering with cursor support
            self.render_wrapped_text(frame, inner_area, theme, bg_style);
        }

        // Mode indicator line
        let mode_area = content_chunks[3];

        // Check for paste tags to show indicator
        let raw_for_mode = self.textarea.lines().join("\n");
        let has_paste_tags = raw_for_mode.contains(PASTE_TAG_OPEN);

        let mode_name = if self.shell_mode {
            "Shell"
        } else if has_paste_tags {
            "Paste" // Show "Paste" mode when paste tags are present
        } else {
            self.agent.name()
        };

        let mode_color = if has_paste_tags {
            theme.secondary // Different color for paste mode
        } else {
            agent_color
        };

        let mut mode_spans = vec![
            Span::styled("  ", bg_style),
            Span::styled(mode_name, Style::default().fg(mode_color)),
        ];

        if !self.model.is_empty() {
            mode_spans.push(Span::styled(" · ", theme.muted_style()));
            mode_spans.push(Span::styled(&self.model, theme.muted_style()));
        }

        // Calculate character and line count from display text (not raw)
        let raw_text = self.textarea.lines().join("\n");
        let (display_text, paste_regions) = transform_for_display(&raw_text);
        let display_char_count = display_text.len();
        let display_line_count = display_text.lines().count().max(1);

        // Show character count on the right side (only if there's content)
        if display_char_count > 0 {
            // Calculate how much space we have
            let left_content_len: usize = mode_spans.iter().map(|s| s.content.len()).sum();
            let count_text = if !paste_regions.is_empty() {
                format!(
                    "{} chars | {} pastes",
                    display_char_count,
                    paste_regions.len()
                )
            } else if display_line_count > 1 {
                format!("{display_char_count} chars | {display_line_count} lines")
            } else {
                format!("{display_char_count} chars")
            };

            let available_width = mode_area.width as usize;
            let spacing = available_width.saturating_sub(left_content_len + count_text.len() + 2);

            if spacing > 0 {
                mode_spans.push(Span::styled(" ".repeat(spacing), bg_style));
                mode_spans.push(Span::styled(count_text, theme.dim_style()));
                mode_spans.push(Span::styled(" ", bg_style));
            }
        }

        let mode_line = Paragraph::new(Line::from(mode_spans)).style(bg_style);
        frame.render_widget(mode_line, mode_area);
    }

    /// Render text with wrapping and cursor support.
    fn render_wrapped_text(&self, frame: &mut Frame, area: Rect, theme: &Theme, bg_style: Style) {
        let width = area.width as usize;
        if width == 0 {
            return;
        }

        let (cursor_row, cursor_col) = self.textarea.cursor();
        let raw_lines = self.textarea.lines();
        let text_style = bg_style.fg(theme.text);
        let paste_style = bg_style.fg(theme.text_muted);
        let cursor_style = if self.focused {
            Style::default()
                .fg(theme.background)
                .bg(theme.text)
                .add_modifier(Modifier::BOLD)
        } else {
            text_style
        };

        // Transform raw text to display text with paste placeholders
        let raw_text = raw_lines.join("\n");
        let (display_text, paste_regions) = transform_for_display(&raw_text);

        // Debug: log when we have paste regions
        if !paste_regions.is_empty() {
            let preview: String = display_text.chars().take(100).collect();
            tracing::debug!(
                "render_wrapped_text: {} paste regions, display_text={:?}",
                paste_regions.len(),
                preview
            );
        }

        // Map cursor position from raw to display coordinates
        let raw_cursor_offset = cursor_to_offset(raw_lines, cursor_row, cursor_col);
        let display_cursor_offset = map_cursor_to_display(raw_cursor_offset, &paste_regions);
        let (display_cursor_row, display_cursor_col) =
            offset_to_cursor(&display_text, display_cursor_offset);

        // Split display text into lines
        let display_lines: Vec<&str> = display_text.split('\n').collect();

        // Build wrapped lines and track cursor position
        let mut wrapped_lines: Vec<Line> = Vec::new();
        let mut cursor_wrapped_row = 0usize;
        let mut cursor_wrapped_col = 0usize;
        let mut found_cursor = false;

        // Track character offset in display text for paste region detection
        let mut display_char_offset = 0usize;

        for (line_idx, line) in display_lines.iter().enumerate() {
            if line.is_empty() {
                // Empty line - check if cursor is here
                if line_idx == display_cursor_row && display_cursor_col == 0 {
                    cursor_wrapped_row = wrapped_lines.len();
                    cursor_wrapped_col = 0;
                    found_cursor = true;
                }
                wrapped_lines.push(Line::from(""));
                display_char_offset += 1; // newline
            } else {
                // Wrap the line
                let chars: Vec<char> = line.chars().collect();
                let mut char_idx = 0usize;

                while char_idx < chars.len() {
                    let wrap_start = char_idx;
                    let wrap_end = (char_idx + width).min(chars.len());
                    let segment: String = chars[wrap_start..wrap_end].iter().collect();

                    // Check if cursor is in this segment
                    if line_idx == display_cursor_row && !found_cursor {
                        if display_cursor_col >= wrap_start && display_cursor_col < wrap_end {
                            cursor_wrapped_row = wrapped_lines.len();
                            cursor_wrapped_col = display_cursor_col - wrap_start;
                            found_cursor = true;
                        } else if display_cursor_col == wrap_end && wrap_end == chars.len() {
                            // Cursor at end of line
                            cursor_wrapped_row = wrapped_lines.len();
                            cursor_wrapped_col = segment.chars().count();
                            found_cursor = true;
                        }
                    }

                    // Check if this segment contains a paste placeholder and style accordingly
                    let segment_start_offset = display_char_offset + wrap_start;
                    let segment_end_offset = display_char_offset + wrap_end;
                    let is_in_paste = paste_regions.iter().any(|r| {
                        segment_start_offset < r.display_end && segment_end_offset > r.display_start
                    });

                    let style = if is_in_paste { paste_style } else { text_style };
                    wrapped_lines.push(Line::from(Span::styled(segment, style)));
                    char_idx = wrap_end;
                }
                display_char_offset += line.len() + 1; // +1 for newline
            }
        }

        // Calculate scroll offset to keep cursor visible
        let visible_height = area.height as usize;
        let scroll_offset = if cursor_wrapped_row >= visible_height {
            cursor_wrapped_row - visible_height + 1
        } else {
            0
        };

        // Render wrapped lines with scroll offset
        let buffer = frame.buffer_mut();
        for (row_offset, wrapped_line) in wrapped_lines
            .iter()
            .skip(scroll_offset)
            .take(visible_height)
            .enumerate()
        {
            let y = area.y + row_offset as u16;
            if y >= area.y + area.height {
                break;
            }

            // Render the line content
            let mut x = area.x;
            for span in wrapped_line.spans.iter() {
                for ch in span.content.chars() {
                    if x < area.x + area.width {
                        buffer[(x, y)].set_char(ch).set_style(span.style);
                        x += 1;
                    }
                }
            }

            // Fill remaining space with background
            while x < area.x + area.width {
                buffer[(x, y)].set_char(' ').set_style(bg_style);
                x += 1;
            }
        }

        // Render cursor
        if self.focused {
            let cursor_screen_row = cursor_wrapped_row.saturating_sub(scroll_offset);
            if cursor_screen_row < visible_height {
                let cursor_y = area.y + cursor_screen_row as u16;
                let cursor_x = area.x + cursor_wrapped_col as u16;

                if cursor_x < area.x + area.width && cursor_y < area.y + area.height {
                    let cell = &mut buffer[(cursor_x, cursor_y)];
                    let ch = if cell.symbol() == " " || cell.symbol().is_empty() {
                        ' '
                    } else {
                        cell.symbol().chars().next().unwrap_or(' ')
                    };
                    cell.set_char(ch).set_style(cursor_style);
                }
            }
        }
    }
}

/// Actions that can result from input handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    None,
    Submit,
    CommandPalette,
    Cancel,
    Escape,
    LeaderKey,
    ScrollUp,
    Autocomplete,
    AgentChanged(AgentMode),
    /// Request to paste from clipboard.
    Paste,
}

/// Strip paste tags from text, keeping the content inside.
fn strip_paste_tags(text: &str) -> String {
    let mut result = text.to_string();
    // Remove all opening and closing tags
    result = result.replace(PASTE_TAG_OPEN, "");
    result = result.replace(PASTE_TAG_CLOSE, "");
    result
}

/// Information about a paste region for cursor mapping.
struct PasteRegion {
    /// Start offset in raw text (at opening tag).
    raw_start: usize,
    /// End offset in raw text (after closing tag).
    raw_end: usize,
    /// Start offset in display text.
    display_start: usize,
    /// End offset in display text (after placeholder).
    display_end: usize,
}

/// Transform text for display, replacing paste regions with placeholders.
/// Returns the display text and information for cursor mapping.
fn transform_for_display(text: &str) -> (String, Vec<PasteRegion>) {
    let mut result = String::new();
    let mut regions = Vec::new();
    let mut remaining = text;
    let mut paste_num = 1;
    let mut raw_offset = 0usize;

    while let Some(start_pos) = remaining.find(PASTE_TAG_OPEN) {
        // Add text before the tag
        result.push_str(&remaining[..start_pos]);
        raw_offset += start_pos;

        let display_start = result.len();
        let raw_start = raw_offset;

        // Find the closing tag
        let after_open = &remaining[start_pos + PASTE_TAG_OPEN.len()..];
        if let Some(end_pos) = after_open.find(PASTE_TAG_CLOSE) {
            // Extract the paste content to count lines
            let paste_content = &after_open[..end_pos];
            let line_count = paste_content.lines().count().max(1);

            // Add placeholder
            let placeholder = format!("[Paste #{paste_num} - {line_count} lines]");
            result.push_str(&placeholder);
            paste_num += 1;

            // Calculate raw end position (after closing tag)
            let raw_end = raw_start + PASTE_TAG_OPEN.len() + end_pos + PASTE_TAG_CLOSE.len();
            let display_end = result.len();

            regions.push(PasteRegion {
                raw_start,
                raw_end,
                display_start,
                display_end,
            });

            // Move past the closing tag
            remaining = &after_open[end_pos + PASTE_TAG_CLOSE.len()..];
            raw_offset = raw_end;
        } else {
            // No closing tag found, include the rest as-is
            result.push_str(&remaining[start_pos..]);
            break;
        }
    }

    // Add any remaining text
    result.push_str(remaining);
    (result, regions)
}

/// Map a cursor offset from raw text to display text.
fn map_cursor_to_display(raw_offset: usize, regions: &[PasteRegion]) -> usize {
    let mut display_offset = raw_offset;

    for region in regions {
        if raw_offset < region.raw_start {
            // Cursor is before this region, no adjustment needed for this region
            break;
        } else if raw_offset >= region.raw_start && raw_offset < region.raw_end {
            // Cursor is inside the paste region - show at end of placeholder
            return region.display_end;
        } else {
            // Cursor is after this region - adjust offset
            let raw_region_len = region.raw_end - region.raw_start;
            let display_region_len = region.display_end - region.display_start;
            display_offset = display_offset - raw_region_len + display_region_len;
        }
    }

    display_offset
}

/// Convert a line/column cursor position to a character offset.
fn cursor_to_offset(lines: &[impl AsRef<str>], row: usize, col: usize) -> usize {
    let mut offset = 0;
    for (i, line) in lines.iter().enumerate() {
        if i == row {
            return offset + col.min(line.as_ref().len());
        }
        offset += line.as_ref().len() + 1; // +1 for newline
    }
    offset
}

/// Convert a character offset to line/column position.
fn offset_to_cursor(text: &str, offset: usize) -> (usize, usize) {
    let mut row = 0;
    let mut col = 0;

    for (current_offset, ch) in text.chars().enumerate() {
        if current_offset >= offset {
            break;
        }
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (row, col)
}

/// Find all paste tag regions in the raw text.
/// Returns Vec of (start_offset, end_offset) for each paste region.
fn find_paste_regions(text: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut search_start = 0;

    while let Some(open_pos) = text[search_start..].find(PASTE_TAG_OPEN) {
        let abs_open = search_start + open_pos;
        let after_open = abs_open + PASTE_TAG_OPEN.len();

        if let Some(close_pos) = text[after_open..].find(PASTE_TAG_CLOSE) {
            let abs_close = after_open + close_pos + PASTE_TAG_CLOSE.len();
            regions.push((abs_open, abs_close));
            search_start = abs_close;
        } else {
            break;
        }
    }

    regions
}

/// Check if moving right from current offset would enter a paste region.
/// Returns Some(end_of_region) if so, None otherwise.
fn skip_paste_region_right(text: &str, current_offset: usize) -> Option<usize> {
    for (start, end) in find_paste_regions(text) {
        // If we're at or just before the start of a paste region, skip to end
        if current_offset >= start && current_offset < end {
            return Some(end);
        }
    }
    None
}

/// Check if moving left from current offset would enter a paste region.
/// Returns Some(start_of_region) if so, None otherwise.
fn skip_paste_region_left(text: &str, current_offset: usize) -> Option<usize> {
    for (start, end) in find_paste_regions(text) {
        // If we're at or just after the end of a paste region, skip to start
        if current_offset > start && current_offset <= end {
            return Some(start);
        }
    }
    None
}

/// Check if a cursor offset is inside a paste region (not at the edges).
/// Returns Some((start, end)) of the containing region if so.
fn find_containing_paste_region(text: &str, offset: usize) -> Option<(usize, usize)> {
    for (start, end) in find_paste_regions(text) {
        // Inside means strictly between start and end (not at edges)
        if offset > start && offset < end {
            return Some((start, end));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_newline() {
        let mut input = InputWidget::new();

        // Type some text
        input.set_text("hello world");
        assert_eq!(input.line_count(), 1);

        // Insert newline via Shift+Enter
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        input.handle_key(key);

        assert_eq!(input.line_count(), 2);
    }

    #[test]
    fn test_multiline_height() {
        let mut input = InputWidget::new();

        // Single line
        input.set_text("line 1");
        assert_eq!(input.height(), 6); // minimum

        // Multiple lines
        input.set_text("line 1\nline 2\nline 3\nline 4\nline 5");
        assert_eq!(input.line_count(), 5);
        assert_eq!(input.height(), 9); // 5 lines + space + mode + 2 padding

        // Many lines (should cap)
        input.set_text("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15");
        assert_eq!(input.line_count(), 15);
        assert_eq!(input.height(), 15); // capped at 15
    }

    #[test]
    fn test_ctrl_j_newline() {
        let mut input = InputWidget::new();
        input.set_text("hello");

        // Simulate Ctrl+J
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        let action = input.handle_key(key);

        assert_eq!(action, InputAction::None);
        assert_eq!(input.line_count(), 2);
    }

    #[test]
    fn test_basic_typing() {
        let mut input = InputWidget::new();

        // Type a character
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        input.handle_key(key);
        assert_eq!(input.text(), "a");

        // Type another character
        let key = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE);
        input.handle_key(key);
        assert_eq!(input.text(), "ab");
    }

    #[test]
    fn test_clear() {
        let mut input = InputWidget::new();
        input.set_text("hello world");
        assert!(!input.is_empty());

        input.clear();
        assert!(input.is_empty());
        assert_eq!(input.text(), "");
    }

    #[test]
    fn test_strip_paste_tags() {
        // Simple case
        let text = "<wonopcode__paste>line1\nline2</wonopcode__paste>";
        assert_eq!(strip_paste_tags(text), "line1\nline2");

        // With surrounding text
        let text = "before <wonopcode__paste>paste</wonopcode__paste> after";
        assert_eq!(strip_paste_tags(text), "before paste after");

        // Multiple pastes
        let text =
            "<wonopcode__paste>p1</wonopcode__paste> mid <wonopcode__paste>p2</wonopcode__paste>";
        assert_eq!(strip_paste_tags(text), "p1 mid p2");

        // No tags
        let text = "no tags here";
        assert_eq!(strip_paste_tags(text), "no tags here");
    }

    #[test]
    fn test_transform_for_display() {
        // Simple paste
        let text = "<wonopcode__paste>line1\nline2</wonopcode__paste>";
        let (display, regions) = transform_for_display(text);
        assert_eq!(display, "[Paste #1 - 2 lines]");
        assert_eq!(regions.len(), 1);

        // With surrounding text
        let text = "before <wonopcode__paste>line1\nline2\nline3</wonopcode__paste> after";
        let (display, regions) = transform_for_display(text);
        assert_eq!(display, "before [Paste #1 - 3 lines] after");
        assert_eq!(regions.len(), 1);

        // Multiple pastes
        let text = "<wonopcode__paste>a\nb</wonopcode__paste> mid <wonopcode__paste>c\nd\ne</wonopcode__paste>";
        let (display, regions) = transform_for_display(text);
        assert_eq!(display, "[Paste #1 - 2 lines] mid [Paste #2 - 3 lines]");
        assert_eq!(regions.len(), 2);

        // No tags
        let text = "no tags here";
        let (display, regions) = transform_for_display(text);
        assert_eq!(display, "no tags here");
        assert_eq!(regions.len(), 0);
    }

    #[test]
    fn test_insert_paste_single_line() {
        let mut input = InputWidget::new();
        input.insert_paste("single line");
        // Single line should not be wrapped
        assert_eq!(input.text(), "single line");
        assert_eq!(input.raw_text(), "single line");
    }

    #[test]
    fn test_insert_paste_multi_line() {
        let mut input = InputWidget::new();
        input.insert_paste("line1\nline2\nline3");
        // Multi-line should be wrapped in tags
        assert_eq!(input.text(), "line1\nline2\nline3"); // text() strips tags
        let raw = input.raw_text();
        assert!(raw.contains("<wonopcode__paste>"));
        assert!(raw.contains("</wonopcode__paste>"));

        // Verify display transformation works
        let (display, regions) = transform_for_display(&raw);
        assert_eq!(display, "[Paste #1 - 3 lines]");
        assert_eq!(regions.len(), 1);
    }

    #[test]
    fn test_paste_count() {
        let mut input = InputWidget::new();
        input.insert_paste("line1\nline2");
        input.insert_text(" ");
        input.insert_paste("line3\nline4");

        let raw = input.raw_text();
        // Should have two paste regions
        assert_eq!(raw.matches("<wonopcode__paste>").count(), 2);

        // Clear should reset paste count
        input.clear();
        assert_eq!(input.paste_count, 0);
    }

    #[test]
    fn test_textarea_preserves_tags_across_lines() {
        let mut input = InputWidget::new();
        input.insert_paste("line1\nline2\nline3");

        // Get the raw text as textarea stores it
        let raw = input.raw_text();

        // The raw text should have the full tags
        assert!(
            raw.starts_with("<wonopcode__paste>"),
            "raw should start with open tag: {raw:?}"
        );
        assert!(
            raw.ends_with("</wonopcode__paste>"),
            "raw should end with close tag: {raw:?}"
        );

        // Transform should produce the placeholder
        let (display, _) = transform_for_display(&raw);
        assert_eq!(
            display, "[Paste #1 - 3 lines]",
            "display should be placeholder, got: {display:?}"
        );
    }

    #[test]
    fn test_render_flow_simulation() {
        // This test simulates exactly what render_wrapped_text does
        let mut input = InputWidget::new();

        // Simulate pasting 5 lines
        let paste_content = "line 1\nline 2\nline 3\nline 4\nline 5";
        input.insert_paste(paste_content);

        // Simulate what render_wrapped_text does
        let raw_lines: Vec<String> = input
            .textarea
            .lines()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let raw_text = raw_lines.join("\n");
        let (display_text, paste_regions) = transform_for_display(&raw_text);
        let display_lines: Vec<&str> = display_text.split('\n').collect();

        // Verify we have paste regions
        assert_eq!(paste_regions.len(), 1, "Should have 1 paste region");

        // Verify display text is the placeholder
        assert_eq!(display_text, "[Paste #1 - 5 lines]");

        // Verify display_lines is just one line with the placeholder
        assert_eq!(display_lines.len(), 1);
        assert_eq!(display_lines[0], "[Paste #1 - 5 lines]");

        // Verify submission strips tags
        let submitted = input.text();
        assert_eq!(submitted, paste_content);
    }

    #[test]
    fn test_line_by_line_paste_tracking() {
        let mut input = InputWidget::new();

        // Simulate terminal sending paste line-by-line (like iTerm2)
        // First line - starts tracking
        input.insert_paste("line1");
        assert_eq!(input.text(), "line1");
        assert!(input.paste_tracker.is_some());

        // Second line - should be tracked as part of same paste
        input.insert_paste("line2");
        assert_eq!(input.text(), "line1\nline2");

        // Third line
        input.insert_paste("line3");
        assert_eq!(input.text(), "line1\nline2\nline3");

        // Check pending paste - since tracker is not expired, shouldn't finalize
        assert!(!input.check_pending_paste());

        // Now simulate expiry by directly calling finalize
        // (In real code, this happens after 100ms timeout)
        let wrapped = input.finalize_paste_tracking();
        assert!(wrapped, "Should have wrapped the paste");

        // After wrapping, raw text should have tags
        let raw = input.raw_text();
        assert!(raw.contains("<wonopcode__paste>"));
        assert!(raw.contains("</wonopcode__paste>"));

        // But text() should strip tags
        assert_eq!(input.text(), "line1\nline2\nline3");

        // Display should show placeholder
        let (display, regions) = transform_for_display(&raw);
        assert_eq!(display, "[Paste #1 - 3 lines]");
        assert_eq!(regions.len(), 1);
    }

    #[test]
    fn test_history_stores_raw_text_with_tags() {
        let mut input = InputWidget::new();

        // Insert a multi-line paste (should be wrapped in tags)
        input.insert_paste("line1\nline2\nline3");

        // Verify raw text has tags
        let raw = input.raw_text();
        assert!(raw.contains("<wonopcode__paste>"));

        // Take the text (this should push raw text to history)
        let submitted = input.take();

        // Submitted text should have tags stripped
        assert_eq!(submitted, "line1\nline2\nline3");
        assert!(!submitted.contains("<wonopcode__paste>"));

        // History should contain the raw text with tags
        assert!(!input.history.is_empty());
        let history_entry = input.history.previous("").unwrap();
        assert!(
            history_entry.contains("<wonopcode__paste>"),
            "History should contain paste tags, got: {history_entry}"
        );
    }

    #[test]
    fn test_find_containing_paste_region() {
        // Text with a paste region
        let text = "before <wonopcode__paste>paste content</wonopcode__paste> after";

        // Find the region boundaries
        let regions = find_paste_regions(text);
        assert_eq!(regions.len(), 1);
        let (start, end) = regions[0];

        // Cursor before the region - not inside
        assert!(find_containing_paste_region(text, 0).is_none());
        assert!(find_containing_paste_region(text, 5).is_none());

        // Cursor at the start of region - not inside (at edge)
        assert!(find_containing_paste_region(text, start).is_none());

        // Cursor inside the region
        assert!(find_containing_paste_region(text, start + 5).is_some());
        assert!(find_containing_paste_region(text, start + 10).is_some());

        // Cursor at the end of region - not inside (at edge)
        assert!(find_containing_paste_region(text, end).is_none());

        // Cursor after the region - not inside
        assert!(find_containing_paste_region(text, end + 1).is_none());
    }

    #[test]
    fn test_snap_cursor_preserves_position_outside_paste() {
        let mut input = InputWidget::new();

        // Type some text with a paste in the middle
        input.set_text("before ");
        input.insert_paste("line1\nline2");
        input.insert_text(" after");

        let raw = input.raw_text();
        assert!(raw.contains("<wonopcode__paste>"));

        // Cursor should be at the end, outside paste region
        let offset_before = input.cursor_offset();
        input.snap_cursor_outside_paste_region();
        let offset_after = input.cursor_offset();

        // Should not have moved
        assert_eq!(offset_before, offset_after);
    }

    #[test]
    fn test_history_navigation_preserves_paste_tags() {
        let mut input = InputWidget::new();

        // First, add a history entry
        input.set_text("previous entry");
        input.take();

        // Now type new content with paste
        input.insert_paste("line1\nline2\nline3");
        let raw_before = input.raw_text();
        assert!(
            raw_before.contains("<wonopcode__paste>"),
            "Should have paste tags before history navigation"
        );

        // Navigate up (should stash current content with tags)
        let up_key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        input.handle_key(up_key);

        // Should now show "previous entry"
        assert_eq!(input.raw_text(), "previous entry");

        // Navigate back down (should restore stashed content with tags)
        let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        input.handle_key(down_key);

        // Should have paste tags preserved
        let raw_after = input.raw_text();
        assert!(
            raw_after.contains("<wonopcode__paste>"),
            "Paste tags should be preserved after history navigation, got: {raw_after}"
        );
        assert_eq!(raw_before, raw_after);
    }
}
