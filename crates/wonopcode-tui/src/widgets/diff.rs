//! Diff viewer widget for displaying file changes.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::theme::Theme;

/// Diff display style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffStyle {
    /// Unified (stacked) diff view - default.
    #[default]
    Unified,
    /// Side-by-side split view.
    SideBySide,
}

/// A line in a diff.
#[derive(Debug, Clone)]
pub enum DiffLine {
    /// Context line (unchanged).
    Context(String),
    /// Added line.
    Added(String),
    /// Removed line.
    Removed(String),
    /// Hunk header.
    Hunk(String),
}

/// A diff hunk.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Starting line in old file.
    pub old_start: usize,
    /// Number of lines in old file.
    pub old_count: usize,
    /// Starting line in new file.
    pub new_start: usize,
    /// Number of lines in new file.
    pub new_count: usize,
    /// Lines in this hunk.
    pub lines: Vec<DiffLine>,
}

/// A file diff.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// File path.
    pub path: String,
    /// Old file path (for renames).
    pub old_path: Option<String>,
    /// Diff hunks.
    pub hunks: Vec<DiffHunk>,
}

impl FileDiff {
    /// Create a new file diff.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            old_path: None,
            hunks: Vec::new(),
        }
    }

    /// Parse a unified diff string.
    pub fn parse_unified(diff: &str) -> Vec<FileDiff> {
        let mut diffs = Vec::new();
        let mut current_diff: Option<FileDiff> = None;
        let mut current_hunk: Option<DiffHunk> = None;

        for line in diff.lines() {
            if line.starts_with("--- ") {
                // Save previous diff
                if let Some(mut d) = current_diff.take() {
                    if let Some(h) = current_hunk.take() {
                        d.hunks.push(h);
                    }
                    diffs.push(d);
                }
                // Start new diff
                let path = line.strip_prefix("--- ").unwrap_or("");
                let path = path.strip_prefix("a/").unwrap_or(path);
                current_diff = Some(FileDiff::new(path));
            } else if line.starts_with("+++ ") {
                // Update path from +++ line
                if let Some(ref mut d) = current_diff {
                    let path = line.strip_prefix("+++ ").unwrap_or("");
                    let path = path.strip_prefix("b/").unwrap_or(path);
                    if d.path != path {
                        d.old_path = Some(d.path.clone());
                        d.path = path.to_string();
                    }
                }
            } else if line.starts_with("@@ ") {
                // Hunk header
                if let Some(ref mut d) = current_diff {
                    if let Some(h) = current_hunk.take() {
                        d.hunks.push(h);
                    }
                }

                // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
                let mut hunk = DiffHunk {
                    old_start: 1,
                    old_count: 0,
                    new_start: 1,
                    new_count: 0,
                    lines: vec![DiffLine::Hunk(line.to_string())],
                };

                // Simple parse of @@ -x,y +a,b @@
                if let Some(header) = line.strip_prefix("@@ ") {
                    if let Some(end) = header.find(" @@") {
                        let parts: Vec<&str> = header[..end].split_whitespace().collect();
                        for part in parts {
                            if let Some(old) = part.strip_prefix('-') {
                                let nums: Vec<&str> = old.split(',').collect();
                                if let Ok(n) = nums[0].parse() {
                                    hunk.old_start = n;
                                }
                                if nums.len() > 1 {
                                    if let Ok(n) = nums[1].parse() {
                                        hunk.old_count = n;
                                    }
                                }
                            } else if let Some(new) = part.strip_prefix('+') {
                                let nums: Vec<&str> = new.split(',').collect();
                                if let Ok(n) = nums[0].parse() {
                                    hunk.new_start = n;
                                }
                                if nums.len() > 1 {
                                    if let Ok(n) = nums[1].parse() {
                                        hunk.new_count = n;
                                    }
                                }
                            }
                        }
                    }
                }

                current_hunk = Some(hunk);
            } else if let Some(ref mut hunk) = current_hunk {
                if line.starts_with('+') {
                    hunk.lines.push(DiffLine::Added(
                        line.strip_prefix('+').unwrap_or("").to_string(),
                    ));
                } else if line.starts_with('-') {
                    hunk.lines.push(DiffLine::Removed(
                        line.strip_prefix('-').unwrap_or("").to_string(),
                    ));
                } else if line.starts_with(' ') || line.is_empty() {
                    hunk.lines.push(DiffLine::Context(
                        line.strip_prefix(' ').unwrap_or(line).to_string(),
                    ));
                }
            }
        }

        // Save last diff
        if let Some(mut d) = current_diff {
            if let Some(h) = current_hunk {
                d.hunks.push(h);
            }
            diffs.push(d);
        }

        diffs
    }
}

/// Diff viewer widget with navigation.
#[derive(Debug, Clone, Default)]
pub struct DiffWidget {
    /// Diffs to display.
    diffs: Vec<FileDiff>,
    /// Scroll offset (line).
    scroll: usize,
    /// Whether focused.
    focused: bool,
    /// Whether collapsed.
    collapsed: bool,
    /// Current file index.
    current_file: usize,
    /// Current hunk index within the file.
    current_hunk: usize,
    /// Line positions of each hunk for navigation.
    hunk_positions: Vec<(usize, usize, usize)>, // (file_idx, hunk_idx, line_pos)
    /// Display style (unified or side-by-side).
    style: DiffStyle,
}

impl DiffWidget {
    /// Create a new diff widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the diffs.
    pub fn set_diffs(&mut self, diffs: Vec<FileDiff>) {
        self.diffs = diffs;
        self.update_hunk_positions();
        self.current_file = 0;
        self.current_hunk = 0;
    }

    /// Parse and set from unified diff string.
    pub fn set_unified_diff(&mut self, diff: &str) {
        self.diffs = FileDiff::parse_unified(diff);
        self.update_hunk_positions();
        self.current_file = 0;
        self.current_hunk = 0;
    }

    /// Update hunk positions for navigation.
    fn update_hunk_positions(&mut self) {
        self.hunk_positions.clear();
        let mut line_pos = 0;

        for (file_idx, diff) in self.diffs.iter().enumerate() {
            // Account for file header line
            line_pos += 1;

            for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
                self.hunk_positions.push((file_idx, hunk_idx, line_pos));
                line_pos += hunk.lines.len();
            }

            // Account for separator line
            line_pos += 1;
        }
    }

    /// Set whether focused.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Toggle collapsed state.
    pub fn toggle_collapsed(&mut self) {
        self.collapsed = !self.collapsed;
    }

    /// Set the display style.
    pub fn set_style(&mut self, style: DiffStyle) {
        self.style = style;
    }

    /// Get the current display style.
    pub fn style(&self) -> DiffStyle {
        self.style
    }

    /// Toggle between unified and side-by-side view.
    pub fn toggle_style(&mut self) {
        self.style = match self.style {
            DiffStyle::Unified => DiffStyle::SideBySide,
            DiffStyle::SideBySide => DiffStyle::Unified,
        };
    }

    /// Scroll up.
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    /// Scroll down.
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    /// Get the number of hunks across all files.
    pub fn hunk_count(&self) -> usize {
        self.hunk_positions.len()
    }

    /// Get the current hunk index (global).
    pub fn current_hunk_index(&self) -> usize {
        self.hunk_positions
            .iter()
            .position(|(f, h, _)| *f == self.current_file && *h == self.current_hunk)
            .unwrap_or(0)
    }

    /// Jump to the next hunk.
    pub fn next_hunk(&mut self) {
        let current_idx = self.current_hunk_index();
        if current_idx + 1 < self.hunk_positions.len() {
            let (file_idx, hunk_idx, line_pos) = self.hunk_positions[current_idx + 1];
            self.current_file = file_idx;
            self.current_hunk = hunk_idx;
            self.scroll = line_pos;
        }
    }

    /// Jump to the previous hunk.
    pub fn prev_hunk(&mut self) {
        let current_idx = self.current_hunk_index();
        if current_idx > 0 {
            let (file_idx, hunk_idx, line_pos) = self.hunk_positions[current_idx - 1];
            self.current_file = file_idx;
            self.current_hunk = hunk_idx;
            self.scroll = line_pos;
        }
    }

    /// Jump to the first hunk.
    pub fn first_hunk(&mut self) {
        if !self.hunk_positions.is_empty() {
            let (file_idx, hunk_idx, line_pos) = self.hunk_positions[0];
            self.current_file = file_idx;
            self.current_hunk = hunk_idx;
            self.scroll = line_pos;
        } else {
            self.scroll = 0;
        }
    }

    /// Jump to the last hunk.
    pub fn last_hunk(&mut self) {
        if !self.hunk_positions.is_empty() {
            let (file_idx, hunk_idx, line_pos) = self.hunk_positions[self.hunk_positions.len() - 1];
            self.current_file = file_idx;
            self.current_hunk = hunk_idx;
            self.scroll = line_pos;
        }
    }

    /// Jump to the next file.
    pub fn next_file(&mut self) {
        if self.current_file + 1 < self.diffs.len() {
            self.current_file += 1;
            self.current_hunk = 0;
            // Find the line position for this file's first hunk
            if let Some((_, _, line_pos)) = self
                .hunk_positions
                .iter()
                .find(|(f, h, _)| *f == self.current_file && *h == 0)
            {
                self.scroll = *line_pos;
            }
        }
    }

    /// Jump to the previous file.
    pub fn prev_file(&mut self) {
        if self.current_file > 0 {
            self.current_file -= 1;
            self.current_hunk = 0;
            // Find the line position for this file's first hunk
            if let Some((_, _, line_pos)) = self
                .hunk_positions
                .iter()
                .find(|(f, h, _)| *f == self.current_file && *h == 0)
            {
                self.scroll = *line_pos;
            }
        }
    }

    /// Get summary stats.
    pub fn stats(&self) -> (usize, usize, usize) {
        let mut additions = 0;
        let mut deletions = 0;
        let files = self.diffs.len();

        for diff in &self.diffs {
            for hunk in &diff.hunks {
                for line in &hunk.lines {
                    match line {
                        DiffLine::Added(_) => additions += 1,
                        DiffLine::Removed(_) => deletions += 1,
                        _ => {}
                    }
                }
            }
        }

        (files, additions, deletions)
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.diffs.is_empty()
    }

    /// Render the diff widget.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if self.diffs.is_empty() {
            return;
        }

        let style_indicator = match self.style {
            DiffStyle::Unified => "unified",
            DiffStyle::SideBySide => "split",
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(if self.focused {
                theme.border_active_style()
            } else {
                theme.border_style()
            })
            .title(format!(" Diff ({}) ", style_indicator));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.collapsed {
            // Just show summary
            let summary = format!("{} file(s) changed", self.diffs.len());
            let para = Paragraph::new(Span::styled(summary, theme.dim_style()));
            frame.render_widget(para, inner);
            return;
        }

        match self.style {
            DiffStyle::Unified => self.render_unified(frame, inner, theme),
            DiffStyle::SideBySide => self.render_side_by_side(frame, inner, theme),
        }
    }

    /// Render unified (stacked) diff view.
    fn render_unified(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut lines: Vec<Line> = Vec::new();

        for (file_idx, diff) in self.diffs.iter().enumerate() {
            // File header
            let file_header = if let Some(old) = &diff.old_path {
                format!("{} -> {}", old, diff.path)
            } else {
                diff.path.clone()
            };
            let file_style = if file_idx == self.current_file {
                theme.primary_style()
            } else {
                theme.highlight_style()
            };
            lines.push(Line::from(Span::styled(
                format!(" {}", file_header),
                file_style,
            )));

            for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
                let is_current_hunk =
                    file_idx == self.current_file && hunk_idx == self.current_hunk;

                for diff_line in &hunk.lines {
                    let (prefix, content, style) = match diff_line {
                        DiffLine::Hunk(s) => ("", s.as_str(), theme.dim_style()),
                        DiffLine::Context(s) => ("  ", s.as_str(), theme.text_style()),
                        DiffLine::Added(s) => (
                            "+ ",
                            s.as_str(),
                            ratatui::style::Style::default()
                                .fg(theme.diff_added)
                                .bg(theme.diff_added_bg),
                        ),
                        DiffLine::Removed(s) => (
                            "- ",
                            s.as_str(),
                            ratatui::style::Style::default()
                                .fg(theme.diff_removed)
                                .bg(theme.diff_removed_bg),
                        ),
                    };

                    // Highlight current hunk with a marker
                    let marker = if is_current_hunk && matches!(diff_line, DiffLine::Hunk(_)) {
                        ">"
                    } else {
                        " "
                    };

                    lines.push(Line::from(vec![
                        Span::styled(marker, theme.primary_style()),
                        Span::styled(prefix, style),
                        Span::styled(content.to_string(), style),
                    ]));
                }
            }

            // Separator between files
            lines.push(Line::from(""));
        }

        // Calculate scroll
        let total_lines = lines.len();
        let visible_lines = area.height as usize;
        let max_scroll = total_lines.saturating_sub(visible_lines);
        self.scroll = self.scroll.min(max_scroll);

        let paragraph = Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll as u16, 0));

        frame.render_widget(paragraph, area);
    }

    /// Render side-by-side diff view.
    fn render_side_by_side(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Split into left (old) and right (new) panels
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let left_area = chunks[0];
        let right_area = chunks[1];

        // Build paired lines for side-by-side view
        let mut left_lines: Vec<Line> = Vec::new();
        let mut right_lines: Vec<Line> = Vec::new();

        for (file_idx, diff) in self.diffs.iter().enumerate() {
            // File header on both sides
            let file_header = if let Some(old) = &diff.old_path {
                format!("{} -> {}", old, diff.path)
            } else {
                diff.path.clone()
            };
            let file_style = if file_idx == self.current_file {
                theme.primary_style()
            } else {
                theme.highlight_style()
            };

            left_lines.push(Line::from(Span::styled(
                format!(" {} (old)", file_header),
                file_style,
            )));
            right_lines.push(Line::from(Span::styled(
                format!(" {} (new)", file_header),
                file_style,
            )));

            for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
                let is_current_hunk =
                    file_idx == self.current_file && hunk_idx == self.current_hunk;

                // Add hunk header to both sides
                if let Some(DiffLine::Hunk(header)) = hunk.lines.first() {
                    let marker = if is_current_hunk { ">" } else { " " };
                    left_lines.push(Line::from(vec![
                        Span::styled(marker, theme.primary_style()),
                        Span::styled(header.clone(), theme.dim_style()),
                    ]));
                    right_lines.push(Line::from(vec![
                        Span::styled(marker, theme.primary_style()),
                        Span::styled(header.clone(), theme.dim_style()),
                    ]));
                }

                // Collect removed and added lines, pair them with context
                let mut old_lines_in_hunk: Vec<&DiffLine> = Vec::new();
                let mut new_lines_in_hunk: Vec<&DiffLine> = Vec::new();

                for diff_line in hunk.lines.iter().skip(1) {
                    // Skip hunk header
                    match diff_line {
                        DiffLine::Context(_) => {
                            old_lines_in_hunk.push(diff_line);
                            new_lines_in_hunk.push(diff_line);
                        }
                        DiffLine::Removed(_) => {
                            old_lines_in_hunk.push(diff_line);
                        }
                        DiffLine::Added(_) => {
                            new_lines_in_hunk.push(diff_line);
                        }
                        DiffLine::Hunk(_) => {} // Already handled
                    }
                }

                // Now pair them line by line
                let _max_lines = old_lines_in_hunk.len().max(new_lines_in_hunk.len());
                let mut old_idx = 0;
                let mut new_idx = 0;

                while old_idx < old_lines_in_hunk.len() || new_idx < new_lines_in_hunk.len() {
                    let old_line = old_lines_in_hunk.get(old_idx);
                    let new_line = new_lines_in_hunk.get(new_idx);

                    match (old_line, new_line) {
                        (Some(DiffLine::Context(s)), Some(DiffLine::Context(_))) => {
                            // Context line - same on both sides
                            left_lines.push(Line::from(vec![
                                Span::styled("  ", theme.text_style()),
                                Span::styled(s.clone(), theme.text_style()),
                            ]));
                            right_lines.push(Line::from(vec![
                                Span::styled("  ", theme.text_style()),
                                Span::styled(s.clone(), theme.text_style()),
                            ]));
                            old_idx += 1;
                            new_idx += 1;
                        }
                        (Some(DiffLine::Removed(s)), Some(DiffLine::Added(t))) => {
                            // Changed line - show old on left, new on right
                            left_lines.push(Line::from(vec![
                                Span::styled(
                                    "- ",
                                    ratatui::style::Style::default().fg(theme.diff_removed),
                                ),
                                Span::styled(
                                    s.clone(),
                                    ratatui::style::Style::default()
                                        .fg(theme.diff_removed)
                                        .bg(theme.diff_removed_bg),
                                ),
                            ]));
                            right_lines.push(Line::from(vec![
                                Span::styled(
                                    "+ ",
                                    ratatui::style::Style::default().fg(theme.diff_added),
                                ),
                                Span::styled(
                                    t.clone(),
                                    ratatui::style::Style::default()
                                        .fg(theme.diff_added)
                                        .bg(theme.diff_added_bg),
                                ),
                            ]));
                            old_idx += 1;
                            new_idx += 1;
                        }
                        (Some(DiffLine::Removed(s)), _) => {
                            // Removed line with no corresponding add
                            left_lines.push(Line::from(vec![
                                Span::styled(
                                    "- ",
                                    ratatui::style::Style::default().fg(theme.diff_removed),
                                ),
                                Span::styled(
                                    s.clone(),
                                    ratatui::style::Style::default()
                                        .fg(theme.diff_removed)
                                        .bg(theme.diff_removed_bg),
                                ),
                            ]));
                            right_lines.push(Line::from(Span::styled("", theme.dim_style())));
                            old_idx += 1;
                        }
                        (_, Some(DiffLine::Added(s))) => {
                            // Added line with no corresponding remove
                            left_lines.push(Line::from(Span::styled("", theme.dim_style())));
                            right_lines.push(Line::from(vec![
                                Span::styled(
                                    "+ ",
                                    ratatui::style::Style::default().fg(theme.diff_added),
                                ),
                                Span::styled(
                                    s.clone(),
                                    ratatui::style::Style::default()
                                        .fg(theme.diff_added)
                                        .bg(theme.diff_added_bg),
                                ),
                            ]));
                            new_idx += 1;
                        }
                        (Some(DiffLine::Context(s)), None) => {
                            // Trailing context on old side only
                            left_lines.push(Line::from(vec![
                                Span::styled("  ", theme.text_style()),
                                Span::styled(s.clone(), theme.text_style()),
                            ]));
                            right_lines.push(Line::from(Span::styled("", theme.dim_style())));
                            old_idx += 1;
                        }
                        (None, Some(DiffLine::Context(s))) => {
                            // Trailing context on new side only
                            left_lines.push(Line::from(Span::styled("", theme.dim_style())));
                            right_lines.push(Line::from(vec![
                                Span::styled("  ", theme.text_style()),
                                Span::styled(s.clone(), theme.text_style()),
                            ]));
                            new_idx += 1;
                        }
                        _ => {
                            // Move forward in any case to prevent infinite loop
                            if old_idx < old_lines_in_hunk.len() {
                                old_idx += 1;
                            }
                            if new_idx < new_lines_in_hunk.len() {
                                new_idx += 1;
                            }
                        }
                    }
                }
            }

            // Separator between files
            left_lines.push(Line::from(""));
            right_lines.push(Line::from(""));
        }

        // Calculate scroll
        let total_lines = left_lines.len().max(right_lines.len());
        let visible_lines = area.height as usize;
        let max_scroll = total_lines.saturating_sub(visible_lines);
        self.scroll = self.scroll.min(max_scroll);

        // Render left panel
        let left_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(theme.border_style());
        let left_inner = left_block.inner(left_area);
        frame.render_widget(left_block, left_area);

        let left_para = Paragraph::new(Text::from(left_lines))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll as u16, 0));
        frame.render_widget(left_para, left_inner);

        // Render right panel
        let right_para = Paragraph::new(Text::from(right_lines))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll as u16, 0));
        frame.render_widget(right_para, right_area);
    }
}

/// Navigation action for diff viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffNavAction {
    /// Navigate to next hunk.
    NextHunk,
    /// Navigate to previous hunk.
    PrevHunk,
    /// Navigate to next file.
    NextFile,
    /// Navigate to previous file.
    PrevFile,
    /// Navigate to first hunk.
    FirstHunk,
    /// Navigate to last hunk.
    LastHunk,
    /// Scroll up.
    ScrollUp(usize),
    /// Scroll down.
    ScrollDown(usize),
    /// Toggle collapsed.
    ToggleCollapsed,
    /// Toggle between unified and side-by-side view.
    ToggleStyle,
}

impl DiffWidget {
    /// Handle a navigation action.
    pub fn handle_nav(&mut self, action: DiffNavAction) {
        match action {
            DiffNavAction::NextHunk => self.next_hunk(),
            DiffNavAction::PrevHunk => self.prev_hunk(),
            DiffNavAction::NextFile => self.next_file(),
            DiffNavAction::PrevFile => self.prev_file(),
            DiffNavAction::FirstHunk => self.first_hunk(),
            DiffNavAction::LastHunk => self.last_hunk(),
            DiffNavAction::ScrollUp(n) => self.scroll_up(n),
            DiffNavAction::ScrollDown(n) => self.scroll_down(n),
            DiffNavAction::ToggleCollapsed => self.toggle_collapsed(),
            DiffNavAction::ToggleStyle => self.toggle_style(),
        }
    }
}

/// Create a simple before/after diff display.
pub fn simple_diff(old: &str, new: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Show removed lines (old)
    for line in old.lines() {
        lines.push(Line::from(vec![
            Span::styled(
                "- ",
                ratatui::style::Style::default().fg(theme.diff_removed),
            ),
            Span::styled(
                line.to_string(),
                ratatui::style::Style::default().fg(theme.diff_removed),
            ),
        ]));
    }

    // Show added lines (new)
    for line in new.lines() {
        lines.push(Line::from(vec![
            Span::styled("+ ", ratatui::style::Style::default().fg(theme.diff_added)),
            Span::styled(
                line.to_string(),
                ratatui::style::Style::default().fg(theme.diff_added),
            ),
        ]));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"--- a/file1.rs
+++ b/file1.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!("Hello");
     let x = 1;
 }
--- a/file2.rs
+++ b/file2.rs
@@ -10,5 +10,6 @@
 impl Foo {
-    fn old() {}
+    fn new() {}
+    fn extra() {}
 }
"#;

    #[test]
    fn test_parse_unified_diff() {
        let diffs = FileDiff::parse_unified(SAMPLE_DIFF);
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].path, "file1.rs");
        assert_eq!(diffs[1].path, "file2.rs");
        assert_eq!(diffs[0].hunks.len(), 1);
        assert_eq!(diffs[1].hunks.len(), 1);
    }

    #[test]
    fn test_navigation() {
        let mut widget = DiffWidget::new();
        widget.set_unified_diff(SAMPLE_DIFF);

        assert_eq!(widget.hunk_count(), 2);
        assert_eq!(widget.current_file, 0);
        assert_eq!(widget.current_hunk, 0);

        widget.next_hunk();
        assert_eq!(widget.current_file, 1);
        assert_eq!(widget.current_hunk, 0);

        widget.prev_hunk();
        assert_eq!(widget.current_file, 0);
        assert_eq!(widget.current_hunk, 0);
    }

    #[test]
    fn test_file_navigation() {
        let mut widget = DiffWidget::new();
        widget.set_unified_diff(SAMPLE_DIFF);

        widget.next_file();
        assert_eq!(widget.current_file, 1);

        widget.prev_file();
        assert_eq!(widget.current_file, 0);
    }

    #[test]
    fn test_stats() {
        let mut widget = DiffWidget::new();
        widget.set_unified_diff(SAMPLE_DIFF);

        let (files, additions, deletions) = widget.stats();
        assert_eq!(files, 2);
        assert_eq!(additions, 3); // +println, +fn new, +fn extra
        assert_eq!(deletions, 1); // -fn old
    }
}
