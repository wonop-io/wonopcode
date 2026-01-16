//! MCP (Model Context Protocol) server management dialog.
//!
//! This module provides the dialog interface for managing MCP servers,
//! including viewing their status, enabling/disabling servers, and
//! inspecting their available tools.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theme::Theme;

use super::common::centered_rect;

/// Status of an MCP server connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpStatus {
    /// Server is connected and ready.
    Connected,
    /// Server is disconnected.
    Disconnected,
    /// Server is connecting.
    Connecting,
    /// Server has an error.
    Error,
}

impl McpStatus {
    /// Get a display string for the status.
    pub fn as_str(&self) -> &'static str {
        match self {
            McpStatus::Connected => "connected",
            McpStatus::Disconnected => "disconnected",
            McpStatus::Connecting => "connecting",
            McpStatus::Error => "error",
        }
    }

    /// Get a symbol for the status.
    pub fn symbol(&self) -> &'static str {
        match self {
            McpStatus::Connected => "✓",
            McpStatus::Disconnected => "○",
            McpStatus::Connecting => "⋯",
            McpStatus::Error => "✗",
        }
    }
}

/// Information about an MCP server.
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    /// Server name.
    pub name: String,
    /// Current status.
    pub status: McpStatus,
    /// Number of tools provided.
    pub tool_count: usize,
    /// Whether the server is enabled.
    pub enabled: bool,
    /// Optional error message.
    pub error: Option<String>,
}

impl McpServerInfo {
    /// Create a new MCP server info.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: McpStatus::Disconnected,
            tool_count: 0,
            enabled: false,
            error: None,
        }
    }

    /// Set the status.
    pub fn with_status(mut self, status: McpStatus) -> Self {
        self.status = status;
        self
    }

    /// Set the tool count.
    pub fn with_tool_count(mut self, count: usize) -> Self {
        self.tool_count = count;
        self
    }

    /// Set as enabled.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set error message.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self.status = McpStatus::Error;
        self
    }
}

/// MCP server management dialog.
#[derive(Debug, Clone)]
pub struct McpDialog {
    /// Server information.
    servers: Vec<McpServerInfo>,
    /// Selected index.
    selected: usize,
    /// List state for rendering.
    list_state: ListState,
    /// Filter text.
    filter: String,
    /// Filtered indices.
    filtered: Vec<usize>,
}

impl McpDialog {
    /// Create a new MCP dialog with the given servers.
    pub fn new(servers: Vec<McpServerInfo>) -> Self {
        let filtered: Vec<usize> = (0..servers.len()).collect();
        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            servers,
            selected: 0,
            list_state,
            filter: String::new(),
            filtered,
        }
    }

    /// Get the currently selected server.
    pub fn selected_server(&self) -> Option<&McpServerInfo> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.servers.get(idx))
    }

    /// Get the currently selected server name.
    pub fn selected_name(&self) -> Option<&str> {
        self.selected_server().map(|s| s.name.as_str())
    }

    /// Update the filter.
    fn update_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered = (0..self.servers.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered = self
                .servers
                .iter()
                .enumerate()
                .filter(|(_, server)| server.name.to_lowercase().contains(&filter_lower))
                .map(|(i, _)| i)
                .collect();
        }

        self.selected = 0;
        self.list_state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    /// Handle a key event. Returns Some(action) if an action was triggered.
    /// Actions: `toggle:<name>` for toggling, `select:<name>` for selection.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Enter => {
                return self.selected_server().map(|s| format!("select:{}", s.name));
            }
            KeyCode::Char(' ') => {
                // Space toggles the server
                return self.selected_server().map(|s| format!("toggle:{}", s.name));
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

    /// Render the MCP dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 60 / 100).clamp(40, 70);
        let dialog_height = (area.height * 70 / 100).clamp(10, 25);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" MCP Servers ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into filter, list, and help text
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
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

        // Render server list
        let list_items: Vec<ListItem> = self
            .filtered
            .iter()
            .map(|&idx| {
                let server = &self.servers[idx];

                // Status indicator
                let (status_symbol, status_style) = match server.status {
                    McpStatus::Connected => ("✓", Style::default().fg(theme.success)),
                    McpStatus::Disconnected => ("○", theme.dim_style()),
                    McpStatus::Connecting => ("⋯", Style::default().fg(theme.warning)),
                    McpStatus::Error => ("✗", Style::default().fg(theme.error)),
                };

                // Enabled indicator
                let enabled_text = if server.enabled {
                    Span::styled(" [enabled]", Style::default().fg(theme.success))
                } else {
                    Span::styled(" [disabled]", theme.dim_style())
                };

                // Tool count
                let tool_text = if server.tool_count > 0 {
                    Span::styled(format!(" ({} tools)", server.tool_count), theme.dim_style())
                } else {
                    Span::raw("")
                };

                let mut spans = vec![
                    Span::styled(format!("{status_symbol} "), status_style),
                    Span::styled(&server.name, theme.text_style()),
                    enabled_text,
                    tool_text,
                ];

                // Add error message if present
                if let Some(error) = &server.error {
                    spans.push(Span::styled(
                        format!(" - {error}"),
                        Style::default().fg(theme.error),
                    ));
                }

                ListItem::new(Line::from(spans))
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

        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);

        // Render help text
        let help_text = Line::from(vec![
            Span::styled("Space", theme.highlight_style()),
            Span::styled(" toggle  ", theme.dim_style()),
            Span::styled("Enter", theme.highlight_style()),
            Span::styled(" select  ", theme.dim_style()),
            Span::styled("Esc", theme.highlight_style()),
            Span::styled(" close", theme.dim_style()),
        ]);
        let help_para = Paragraph::new(help_text).alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[2]);
    }
}
