//! Sidebar widget showing context information.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::metrics;
use crate::theme::Theme;

/// Format a number with comma separators (e.g., 67360 -> "67,360").
fn format_number(n: u32) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[derive(Debug, Clone, Default)]
pub struct ContextInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub max_tokens: u32,
    pub cost: f64,
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub content: String,
    pub completed: bool,
    pub in_progress: bool,
}

#[derive(Debug, Clone)]
pub struct ModifiedFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// LSP server status.
#[derive(Debug, Clone)]
pub struct LspStatus {
    pub id: String,
    pub name: String,
    pub root: String,
    pub status: LspServerStatus,
}

/// LSP server connection status.
#[derive(Debug, Clone, PartialEq)]
pub enum LspServerStatus {
    /// Server is connected and working.
    Connected,
    /// Server failed to start or crashed.
    Failed,
}

/// MCP server status.
#[derive(Debug, Clone)]
pub struct McpStatus {
    pub name: String,
    pub status: McpServerStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpServerStatus {
    Connected,
    Failed,
    Disabled,
    NeedsAuth,
}

/// Which sidebar section is collapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Lsp,
    Mcp,
    Todos,
    Modified,
}

#[derive(Debug, Clone, Default)]
pub struct SidebarWidget {
    visible: bool,
    session_title: String,
    context: ContextInfo,
    todos: Vec<TodoItem>,
    modified_files: Vec<ModifiedFile>,
    lsp_servers: Vec<LspStatus>,
    mcp_servers: Vec<McpStatus>,
    agent: String,
    model: String,
    version: String,
    /// Which sections are explicitly collapsed by user.
    collapsed: std::collections::HashSet<u8>,
    /// Whether to auto-collapse empty sections.
    auto_collapse_empty: bool,
    /// Current scroll offset for the sidebar content.
    scroll_offset: u16,
    /// Total content height (calculated during render).
    total_height: u16,
    /// Whether the sidebar is focused for scrolling.
    focused: bool,
}

impl SidebarWidget {
    pub fn new() -> Self {
        Self {
            visible: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
            auto_collapse_empty: true, // Default to smart collapse
            ..Default::default()
        }
    }

    pub fn width(&self) -> u16 {
        if self.visible {
            42
        } else {
            0
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_session_title(&mut self, title: impl Into<String>) {
        self.session_title = title.into();
    }

    pub fn set_context(&mut self, context: ContextInfo) {
        self.context = context;
    }

    pub fn update_tokens(&mut self, input: u32, output: u32) {
        self.context.input_tokens = input;
        self.context.output_tokens = output;
    }

    pub fn set_cost(&mut self, cost: f64) {
        self.context.cost = cost;
    }

    pub fn set_max_tokens(&mut self, max: u32) {
        self.context.max_tokens = max;
    }

    /// Get current token counts.
    pub fn get_tokens(&self) -> (u32, u32) {
        (self.context.input_tokens, self.context.output_tokens)
    }

    /// Get current cost.
    pub fn get_cost(&self) -> f64 {
        self.context.cost
    }

    /// Get max tokens (context limit).
    pub fn get_max_tokens(&self) -> u32 {
        self.context.max_tokens
    }

    /// Get MCP server counts (connected, total).
    pub fn get_mcp_counts(&self) -> (usize, usize) {
        let connected = self
            .mcp_servers
            .iter()
            .filter(|s| s.status == McpServerStatus::Connected)
            .count();
        (connected, self.mcp_servers.len())
    }

    /// Get LSP server counts (connected, total).
    pub fn get_lsp_counts(&self) -> (usize, usize) {
        let connected = self
            .lsp_servers
            .iter()
            .filter(|s| s.status == LspServerStatus::Connected)
            .count();
        (connected, self.lsp_servers.len())
    }

    /// Get MCP servers list.
    pub fn get_mcp_servers(&self) -> &[McpStatus] {
        &self.mcp_servers
    }

    pub fn set_todos(&mut self, todos: Vec<TodoItem>) {
        self.todos = todos;
    }

    pub fn set_modified_files(&mut self, files: Vec<ModifiedFile>) {
        self.modified_files = files;
    }

    pub fn set_lsp_servers(&mut self, servers: Vec<LspStatus>) {
        self.lsp_servers = servers;
    }

    pub fn set_mcp_servers(&mut self, servers: Vec<McpStatus>) {
        self.mcp_servers = servers;
    }

    pub fn set_agent(&mut self, agent: impl Into<String>) {
        self.agent = agent.into();
    }

    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Toggle a section's collapsed state.
    pub fn toggle_section(&mut self, section: SidebarSection) {
        let key = section as u8;
        if self.collapsed.contains(&key) {
            self.collapsed.remove(&key);
        } else {
            self.collapsed.insert(key);
        }
    }

    /// Check if a section is collapsed (either explicitly or auto-collapsed when empty).
    pub fn is_collapsed(&self, section: SidebarSection) -> bool {
        // If explicitly collapsed, return true
        if self.collapsed.contains(&(section as u8)) {
            return true;
        }

        // If auto-collapse is enabled and section is empty, collapse it
        if self.auto_collapse_empty {
            match section {
                SidebarSection::Lsp => self.lsp_servers.is_empty(),
                SidebarSection::Mcp => self.mcp_servers.is_empty(),
                SidebarSection::Todos => self.todos.is_empty(),
                SidebarSection::Modified => self.modified_files.is_empty(),
            }
        } else {
            false
        }
    }

    /// Check if a section is empty.
    pub fn is_section_empty(&self, section: SidebarSection) -> bool {
        match section {
            SidebarSection::Lsp => self.lsp_servers.is_empty(),
            SidebarSection::Mcp => self.mcp_servers.is_empty(),
            SidebarSection::Todos => self.todos.is_empty(),
            SidebarSection::Modified => self.modified_files.is_empty(),
        }
    }

    /// Toggle auto-collapse for empty sections.
    pub fn set_auto_collapse(&mut self, enabled: bool) {
        self.auto_collapse_empty = enabled;
    }

    /// Set whether the sidebar is focused for scrolling.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Check if the sidebar is focused.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Scroll up by the given amount.
    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll down by the given amount.
    pub fn scroll_down(&mut self, amount: u16, visible_height: u16) {
        let max_scroll = self.total_height.saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    /// Reset scroll to top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Check if content can scroll (has overflow).
    pub fn can_scroll(&self, visible_height: u16) -> bool {
        self.total_height > visible_height
    }

    /// Maximum number of modified files to track.
    const MAX_MODIFIED_FILES: usize = 50;

    /// Add a modified file.
    pub fn add_modified_file(&mut self, path: String, added: u32, removed: u32) {
        // Check if file already exists, update if so
        if let Some(existing) = self.modified_files.iter_mut().find(|f| f.path == path) {
            existing.added = added;
            existing.removed = removed;
        } else {
            self.modified_files.push(ModifiedFile {
                path,
                added,
                removed,
            });
            // Remove oldest entries if we exceed the limit
            while self.modified_files.len() > Self::MAX_MODIFIED_FILES {
                self.modified_files.remove(0);
            }
        }
    }

    /// Clear all modified files.
    pub fn clear_modified_files(&mut self) {
        self.modified_files.clear();
    }

    /// Handle a mouse click at the given position.
    /// Returns true if a section header was clicked and toggled, or if a link was opened.
    pub fn handle_click(&mut self, x: u16, y: u16, area: Rect) -> bool {
        if !self.visible || area.width < 20 {
            return false;
        }

        // Check if click is within sidebar bounds
        if x < area.x || x >= area.x + area.width || y < area.y || y >= area.y + area.height {
            return false;
        }

        // Check if click is on the "troels.im" link in the footer
        let footer_height: u16 = 3;
        let footer_area = Rect::new(
            area.x + 2,
            area.y + area.height - footer_height - 1,
            area.width.saturating_sub(4),
            footer_height,
        );
        // "Made with ❤️ by " = 16 display cells
        let prefix_width: u16 = 16;
        let hyperlink_y = footer_area.y + 2; // Line 0 is spacer, line 1 is version, line 2 is "Made with..."
        let hyperlink_x = footer_area.x + prefix_width;
        let hyperlink_len: u16 = 9; // "troels.im"

        if y == hyperlink_y && x >= hyperlink_x && x < hyperlink_x + hyperlink_len {
            // Open the URL in the default browser
            let _ = open_url("https://troels.im");
            return true;
        }

        // Content area with padding (same as in render: 2 cols horizontal, 1 row vertical, plus 1 row for status bar)
        let content_area = Rect::new(
            area.x + 2,
            area.y + 2, // 1 row for status bar + 1 row padding
            area.width.saturating_sub(4),
            area.height.saturating_sub(footer_height + 3),
        );

        // Calculate the actual line being clicked (accounting for scroll)
        // Guard against clicks above the content area (e.g., on status bar)
        if y < content_area.y {
            return false;
        }
        let clicked_line = (y - content_area.y) + self.scroll_offset;

        // Calculate line positions for each section header
        // Session: lines 0-1, then spacer
        // Context: lines 3-7, then spacer
        // LSP header is after context section
        let mut current_line: u16 = 0;

        // Session (2 lines + spacer)
        current_line += 3;

        // Context (4 lines + spacer)
        current_line += 5;

        // LSP header
        let lsp_header_line = current_line;
        current_line += 1; // header
        if !self.is_collapsed(SidebarSection::Lsp) {
            current_line += if self.lsp_servers.is_empty() {
                1
            } else {
                self.lsp_servers.len() as u16
            };
        }
        current_line += 1; // spacer

        // MCP header
        let mcp_header_line = current_line;
        current_line += 1; // header
        if !self.is_collapsed(SidebarSection::Mcp) {
            current_line += if self.mcp_servers.is_empty() {
                1
            } else {
                self.mcp_servers.len() as u16
            };
        }
        current_line += 1; // spacer

        // Todos header
        let todos_header_line = current_line;
        current_line += 1; // header
        if !self.is_collapsed(SidebarSection::Todos) {
            current_line += if self.todos.is_empty() {
                1
            } else {
                self.todos.len() as u16
            };
        }
        current_line += 1; // spacer

        // Modified header
        let modified_header_line = current_line;

        // Check which header was clicked
        if clicked_line == lsp_header_line {
            self.toggle_section(SidebarSection::Lsp);
            return true;
        }
        if clicked_line == mcp_header_line {
            self.toggle_section(SidebarSection::Mcp);
            return true;
        }
        if clicked_line == todos_header_line {
            self.toggle_section(SidebarSection::Todos);
            return true;
        }
        if clicked_line == modified_header_line {
            self.toggle_section(SidebarSection::Modified);
            return true;
        }

        false
    }

    /// Handle mouse scroll events.
    pub fn handle_scroll(&mut self, up: bool, area: Rect) {
        let visible_height = area.height.saturating_sub(2);
        if up {
            self.scroll_up(3);
        } else {
            self.scroll_down(3, visible_height);
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let _timer = metrics::widget_timer("sidebar");

        if !self.visible || area.width < 20 {
            return;
        }

        // Background fill with panel color (leaving 1 row at top for status bar)
        let bg_area = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        let bg_style = Style::default().bg(theme.background_panel);
        let block = Block::default().style(bg_style);
        frame.render_widget(block, bg_area);

        // Footer area for version info (fixed at bottom, 2 lines + 1 padding)
        let footer_height: u16 = 3;

        // Content area with padding (2 cols horizontal, 1 row vertical, plus 1 row at top for status bar)
        let content_area = Rect::new(
            area.x + 2,
            area.y + 2, // 1 row for status bar + 1 row padding
            area.width.saturating_sub(4),
            area.height.saturating_sub(footer_height + 3), // footer + 1 top status + 1 top padding + 1 bottom padding
        );

        // Footer area (with 1 row bottom padding)
        let footer_area = Rect::new(
            area.x + 2,
            area.y + area.height - footer_height - 1,
            area.width.saturating_sub(4),
            footer_height,
        );

        // Build all scrollable content lines
        let mut lines: Vec<Line<'static>> = Vec::new();
        let width = content_area.width as usize;

        // Session info
        self.build_session_lines(&mut lines, width, theme);
        lines.push(Line::from("")); // Spacer

        // Context stats
        self.build_context_lines(&mut lines, theme);
        lines.push(Line::from("")); // Spacer

        // Todos
        self.build_todo_lines(&mut lines, width, theme);
        lines.push(Line::from("")); // Spacer

        // Modified files
        self.build_modified_lines(&mut lines, width, theme);
        lines.push(Line::from("")); // Spacer

        // LSP servers
        self.build_lsp_lines(&mut lines, width, theme);
        lines.push(Line::from("")); // Spacer

        // MCP servers
        self.build_mcp_lines(&mut lines, width, theme);

        // Store total height for scroll calculations
        self.total_height = lines.len() as u16;

        // Clamp scroll offset to valid range
        let visible_height = content_area.height;
        let max_scroll = self.total_height.saturating_sub(visible_height);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }

        // Render scrollable content with scroll offset
        let para = Paragraph::new(lines.clone()).scroll((self.scroll_offset, 0));
        frame.render_widget(para, content_area);

        // Render fixed footer (version info)
        let mut footer_lines: Vec<Line<'static>> = Vec::new();
        footer_lines.push(Line::from("")); // Spacer before footer
        self.build_version_lines(&mut footer_lines, theme);
        let footer_para = Paragraph::new(footer_lines);
        frame.render_widget(footer_para, footer_area);

        // Apply OSC 8 hyperlink to "troels.im" in the footer
        // "Made with ❤️ by " = 16 display cells
        // Footer line 2 (index 1) contains the "Made with..." text
        let prefix_width: u16 = 16;
        let hyperlink_y = footer_area.y + 2; // Line 0 is spacer, line 1 is version, line 2 is "Made with..."
        let hyperlink_x = footer_area.x + prefix_width;
        render_hyperlink(
            frame.buffer_mut(),
            hyperlink_x,
            hyperlink_y,
            "troels.im",
            "https://troels.im",
        );

        // Show scroll indicator if content overflows
        if self.total_height > visible_height {
            // Draw scroll indicator on the right edge
            let indicator_height = (visible_height as f32 * visible_height as f32
                / self.total_height as f32)
                .max(1.0) as u16;
            let indicator_pos = if max_scroll > 0 {
                (self.scroll_offset as f32 / max_scroll as f32
                    * (visible_height - indicator_height) as f32) as u16
            } else {
                0
            };

            for i in 0..visible_height {
                let x = area.x + area.width - 1;
                let y = content_area.y + i;
                let char = if i >= indicator_pos && i < indicator_pos + indicator_height {
                    "┃"
                } else {
                    "│"
                };
                let style = if i >= indicator_pos && i < indicator_pos + indicator_height {
                    Style::default().fg(theme.text)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                frame.buffer_mut().set_string(x, y, char, style);
            }
        }
    }

    /// Build session info lines.
    fn build_session_lines(&self, lines: &mut Vec<Line<'static>>, width: usize, theme: &Theme) {
        let title_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

        lines.push(Line::from(Span::styled("Session", title_style)));

        let title = if self.session_title.is_empty() {
            "New Session"
        } else {
            &self.session_title
        };
        lines.push(Line::from(Span::styled(
            truncate(title, width),
            theme.text_style(),
        )));
    }

    /// Build context stats lines.
    fn build_context_lines(&self, lines: &mut Vec<Line<'static>>, theme: &Theme) {
        let title_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

        lines.push(Line::from(Span::styled("Context", title_style)));

        let total_tokens = self.context.input_tokens + self.context.output_tokens;
        let usage_pct = if self.context.max_tokens > 0 {
            (total_tokens as f64 / self.context.max_tokens as f64 * 100.0) as u32
        } else {
            0
        };

        let usage_style = if usage_pct > 80 {
            theme.warning_style()
        } else {
            theme.text_style()
        };

        // Format: "67,360 tokens"
        lines.push(Line::from(vec![
            Span::styled(format_number(total_tokens), theme.text_style()),
            Span::styled(" tokens", theme.muted_style()),
        ]));
        // Format: "34% used"
        lines.push(Line::from(vec![
            Span::styled(format!("{usage_pct}%"), usage_style),
            Span::styled(" used", theme.muted_style()),
        ]));
        // Format: "$0.0000 spent"
        lines.push(Line::from(vec![
            Span::styled(format!("${:.4}", self.context.cost), theme.text_style()),
            Span::styled(" spent", theme.muted_style()),
        ]));
    }

    /// Build LSP server lines.
    fn build_lsp_lines(&self, lines: &mut Vec<Line<'static>>, width: usize, theme: &Theme) {
        let title_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

        let collapsed = self.is_collapsed(SidebarSection::Lsp);
        let arrow = if collapsed { "▶" } else { "▼" };

        let mut header_spans = vec![
            Span::styled(format!("{arrow} "), theme.muted_style()),
            Span::styled("LSP", title_style),
        ];
        if !self.lsp_servers.is_empty() {
            header_spans.push(Span::styled(
                format!(" ({})", self.lsp_servers.len()),
                theme.muted_style(),
            ));
        }
        lines.push(Line::from(header_spans));

        if !collapsed {
            if self.lsp_servers.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No active servers",
                    theme.muted_style(),
                )));
            } else {
                for server in &self.lsp_servers {
                    let (circle, status_style) = match server.status {
                        LspServerStatus::Connected => ("●", theme.success_style()),
                        LspServerStatus::Failed => ("●", theme.error_style()),
                    };

                    lines.push(Line::from(vec![
                        Span::styled(format!("  {circle} "), status_style),
                        Span::styled(
                            truncate(&server.name, width.saturating_sub(6)),
                            theme.text_style(),
                        ),
                    ]));
                }
            }
        }
    }

    /// Build MCP server lines.
    fn build_mcp_lines(&self, lines: &mut Vec<Line<'static>>, width: usize, theme: &Theme) {
        let title_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

        let collapsed = self.is_collapsed(SidebarSection::Mcp);
        let arrow = if collapsed { "▶" } else { "▼" };

        let mut header_spans = vec![
            Span::styled(format!("{arrow} "), theme.muted_style()),
            Span::styled("MCP", title_style),
        ];
        if !self.mcp_servers.is_empty() {
            header_spans.push(Span::styled(
                format!(" ({})", self.mcp_servers.len()),
                theme.muted_style(),
            ));
        }
        lines.push(Line::from(header_spans));

        if !collapsed {
            if self.mcp_servers.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No MCP servers",
                    theme.muted_style(),
                )));
            } else {
                for server in &self.mcp_servers {
                    let (status_style, status_text) = match server.status {
                        McpServerStatus::Connected => (theme.success_style(), ""),
                        McpServerStatus::Failed => (theme.error_style(), " (failed)"),
                        McpServerStatus::Disabled => (theme.muted_style(), " (disabled)"),
                        McpServerStatus::NeedsAuth => (theme.warning_style(), " (auth)"),
                    };

                    lines.push(Line::from(vec![
                        Span::styled("  • ", status_style),
                        Span::styled(
                            truncate(&server.name, width.saturating_sub(12)),
                            theme.text_style(),
                        ),
                        Span::styled(status_text, theme.muted_style()),
                    ]));
                }
            }
        }
    }

    /// Build todo lines.
    fn build_todo_lines(&self, lines: &mut Vec<Line<'static>>, width: usize, theme: &Theme) {
        let title_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

        let collapsed = self.is_collapsed(SidebarSection::Todos);
        let arrow = if collapsed { "▶" } else { "▼" };

        let mut header_spans = vec![
            Span::styled(format!("{arrow} "), theme.muted_style()),
            Span::styled("Todos", title_style),
        ];
        if !self.todos.is_empty() {
            let completed = self.todos.iter().filter(|t| t.completed).count();
            header_spans.push(Span::styled(
                format!(" ({}/{})", completed, self.todos.len()),
                theme.muted_style(),
            ));
        }
        lines.push(Line::from(header_spans));

        if !collapsed {
            if self.todos.is_empty() {
                lines.push(Line::from(Span::styled("  No todos", theme.muted_style())));
            } else {
                for todo in &self.todos {
                    let (icon, style) = if todo.completed {
                        ("[✓]", theme.success_style())
                    } else if todo.in_progress {
                        ("[•]", theme.warning_style())
                    } else {
                        ("[ ]", theme.text_style())
                    };

                    lines.push(Line::from(vec![
                        Span::styled(format!("  {icon} "), style),
                        Span::styled(
                            truncate(&todo.content, width.saturating_sub(8)),
                            theme.text_style(),
                        ),
                    ]));
                }
            }
        }
    }

    /// Build modified files lines.
    fn build_modified_lines(&self, lines: &mut Vec<Line<'static>>, width: usize, theme: &Theme) {
        let title_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

        let collapsed = self.is_collapsed(SidebarSection::Modified);
        let arrow = if collapsed { "▶" } else { "▼" };

        // Calculate total stats
        let total_added: u32 = self.modified_files.iter().map(|f| f.added).sum();
        let total_removed: u32 = self.modified_files.iter().map(|f| f.removed).sum();

        let mut header_spans = vec![
            Span::styled(format!("{arrow} "), theme.muted_style()),
            Span::styled("Modified", title_style),
        ];
        if !self.modified_files.is_empty() {
            header_spans.push(Span::styled(
                format!(
                    " ({} +{} -{})",
                    self.modified_files.len(),
                    total_added,
                    total_removed
                ),
                theme.muted_style(),
            ));
        }
        lines.push(Line::from(header_spans));

        if !collapsed {
            if self.modified_files.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No changes",
                    theme.muted_style(),
                )));
            } else {
                for file in &self.modified_files {
                    // Get just the filename, not full path
                    let filename = std::path::Path::new(&file.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&file.path);

                    lines.push(Line::from(vec![
                        Span::styled("  ", theme.text_style()),
                        Span::styled(
                            truncate(filename, width.saturating_sub(14)),
                            theme.text_style(),
                        ),
                        Span::styled(format!(" +{}", file.added), theme.success_style()),
                        Span::styled(format!(" -{}", file.removed), theme.error_style()),
                    ]));
                }
            }
        }
    }

    /// Build version line.
    fn build_version_lines(&self, lines: &mut Vec<Line<'static>>, theme: &Theme) {
        lines.push(Line::from(vec![
            Span::styled("v", theme.muted_style()),
            Span::styled(self.version.clone(), theme.muted_style()),
        ]));
        // Render the text normally - hyperlink will be applied in render_hyperlink
        lines.push(Line::from(vec![
            Span::styled("Made with ❤️ by ", theme.muted_style()),
            Span::styled(
                "troels.im",
                Style::default()
                    .fg(theme.text_muted)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]));
    }
}

/// Render OSC 8 hyperlink by directly manipulating buffer cells.
/// This is necessary because ratatui doesn't natively support hyperlinks in Span/Text.
///
/// Uses 2-character chunks as a workaround for ratatui issue #902 which incorrectly
/// calculates the width of ANSI escape sequences.
/// See: https://github.com/ratatui/ratatui/issues/902
fn render_hyperlink(buffer: &mut Buffer, x: u16, y: u16, text: &str, url: &str) {
    // Apply OSC 8 escape sequence using 2-character chunks
    // OSC 8 format: \x1B]8;;URL\x07 text \x1B]8;;\x07
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut cell_offset = 0u16;

    while i < chars.len() {
        let chunk: String = if i + 1 < chars.len() {
            chars[i..=i + 1].iter().collect()
        } else {
            chars[i..].iter().collect()
        };
        let chunk_len = chunk.chars().count() as u16;

        let cell_x = x + cell_offset;
        if let Some(cell) = buffer.cell_mut((cell_x, y)) {
            let hyperlink = format!("\x1B]8;;{url}\x07{chunk}\x1B]8;;\x07");
            cell.set_symbol(&hyperlink);
        }

        // For a 2-char chunk, clear the second cell to prevent artifacts
        if chunk_len == 2 {
            if let Some(cell) = buffer.cell_mut((cell_x + 1, y)) {
                cell.set_symbol("");
            }
        }

        cell_offset += chunk_len;
        i += 2;
    }

    // Clear the cell immediately after the hyperlink text to prevent artifacts
    if let Some(cell) = buffer.cell_mut((x + chars.len() as u16, y)) {
        cell.set_symbol(" ");
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        ".".repeat(max_len)
    } else {
        let t: String = s.chars().take(max_len - 3).collect();
        format!("{}...", t)
    }
}

/// Open a URL in the default browser.
fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    Ok(())
}
