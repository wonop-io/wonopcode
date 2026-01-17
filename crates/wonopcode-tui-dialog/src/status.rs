//! Status and monitoring dialog widgets.
//!
//! This module contains read-only dialogs that display system status, performance metrics,
//! and help information to the user.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use wonopcode_tui_core::Theme;

use crate::common::centered_rect;

/// Status dialog showing current configuration and state.
#[derive(Debug, Clone, Default)]
pub struct StatusDialog {
    /// Current provider.
    pub provider: String,
    /// Current model.
    pub model: String,
    /// Current agent.
    pub agent: String,
    /// Current directory.
    pub directory: String,
    /// Session ID.
    pub session_id: Option<String>,
    /// Message count in current session.
    pub message_count: usize,
    /// Input tokens used.
    pub input_tokens: u32,
    /// Output tokens used.
    pub output_tokens: u32,
    /// Total cost.
    pub cost: f64,
    /// Context limit.
    pub context_limit: u32,
    /// MCP servers connected.
    pub mcp_connected: usize,
    /// MCP servers total.
    pub mcp_total: usize,
    /// LSP servers connected.
    pub lsp_connected: usize,
    /// LSP servers total.
    pub lsp_total: usize,
    /// Permissions pending.
    pub permissions_pending: usize,
}

impl StatusDialog {
    /// Create a new status dialog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Render the status dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 60 / 100).clamp(45, 60);
        let dialog_height = (area.height * 70 / 100).clamp(16, 22);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Status ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Format cost
        let cost_str = if self.cost > 0.0 {
            format!("${:.4}", self.cost)
        } else {
            "-".to_string()
        };

        // Format context usage
        let context_str = if self.context_limit > 0 {
            let total = self.input_tokens + self.output_tokens;
            let pct = (total as f64 / self.context_limit as f64 * 100.0) as u32;
            format!("{} / {} ({}%)", total, self.context_limit, pct)
        } else {
            format!("{}", self.input_tokens + self.output_tokens)
        };

        let status_lines = vec![
            Line::from(Span::styled("-- Provider --", theme.dim_style())),
            Line::from(vec![
                Span::styled("Provider:    ", theme.muted_style()),
                Span::styled(&self.provider, theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Model:       ", theme.muted_style()),
                Span::styled(&self.model, theme.highlight_style()),
            ]),
            Line::from(vec![
                Span::styled("Agent:       ", theme.muted_style()),
                Span::styled(&self.agent, theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled("-- Session --", theme.dim_style())),
            Line::from(vec![
                Span::styled("Directory:   ", theme.muted_style()),
                Span::styled(&self.directory, theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Session:     ", theme.muted_style()),
                Span::styled(
                    self.session_id.as_deref().unwrap_or("-"),
                    theme.text_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Messages:    ", theme.muted_style()),
                Span::styled(format!("{}", self.message_count), theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled("-- Usage --", theme.dim_style())),
            Line::from(vec![
                Span::styled("Tokens:      ", theme.muted_style()),
                Span::styled(
                    format!("{} in / {} out", self.input_tokens, self.output_tokens),
                    theme.text_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Context:     ", theme.muted_style()),
                Span::styled(context_str, theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Cost:        ", theme.muted_style()),
                Span::styled(cost_str, theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled("-- Services --", theme.dim_style())),
            Line::from(vec![
                Span::styled("MCP:         ", theme.muted_style()),
                Span::styled(
                    format!("{}/{} connected", self.mcp_connected, self.mcp_total),
                    if self.mcp_connected > 0 {
                        theme.success_style()
                    } else {
                        theme.muted_style()
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("LSP:         ", theme.muted_style()),
                Span::styled(
                    format!("{}/{} connected", self.lsp_connected, self.lsp_total),
                    if self.lsp_connected > 0 {
                        theme.success_style()
                    } else {
                        theme.muted_style()
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Permissions: ", theme.muted_style()),
                Span::styled(
                    format!("{} pending", self.permissions_pending),
                    if self.permissions_pending > 0 {
                        theme.warning_style()
                    } else {
                        theme.muted_style()
                    },
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled("Press Escape to close", theme.dim_style())),
        ];

        let paragraph = Paragraph::new(status_lines);
        frame.render_widget(paragraph, inner);
    }
}

/// Performance metrics dialog.
#[derive(Debug, Clone, Default)]
pub struct PerfDialog {
    /// Uptime in seconds.
    pub uptime_secs: f64,
    /// Status string (excellent/good/degraded/poor).
    pub status: String,
    /// Total frames rendered.
    pub total_frames: u64,
    /// Average FPS.
    pub fps: f64,
    /// Average frame time in ms.
    pub avg_frame_ms: f64,
    /// P50 frame time in ms.
    pub p50_frame_ms: f64,
    /// P95 frame time in ms.
    pub p95_frame_ms: f64,
    /// P99 frame time in ms.
    pub p99_frame_ms: f64,
    /// Max frame time in ms.
    pub max_frame_ms: f64,
    /// Slow frames count.
    pub slow_frames: u64,
    /// Slow frame percentage.
    pub slow_frame_pct: f64,
    /// Average key event time in ms.
    pub avg_key_event_ms: f64,
    /// Average input latency in ms.
    pub avg_input_latency_ms: f64,
    /// P99 input latency in ms.
    pub p99_input_latency_ms: f64,
    /// Average scroll time in ms.
    pub avg_scroll_ms: f64,
    /// Widget stats: (name, avg_ms, max_ms, calls).
    pub widget_stats: Vec<(String, f64, f64, u64)>,
    /// Scroll offset for widget list.
    scroll_offset: usize,
}

impl PerfDialog {
    /// Create a new performance dialog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle key events. Returns true if dialog should close.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => true,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.scroll_offset < self.widget_stats.len().saturating_sub(1) {
                    self.scroll_offset += 1;
                }
                false
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                false
            }
            _ => false,
        }
    }

    /// Render the performance dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 70 / 100).clamp(50, 80);
        let dialog_height = (area.height * 80 / 100).clamp(20, 30);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Performance Metrics ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Status header
                Constraint::Length(9), // Frame stats
                Constraint::Length(5), // Input stats
                Constraint::Min(3),    // Widget stats
                Constraint::Length(1), // Footer
            ])
            .split(inner);

        // Status header
        let status_style = match self.status.as_str() {
            "excellent" => theme.success_style(),
            "good" => Style::default().fg(theme.info),
            "degraded" => theme.warning_style(),
            _ => theme.error_style(),
        };
        let status_lines = vec![Line::from(vec![
            Span::styled("Status: ", theme.muted_style()),
            Span::styled(self.status.to_uppercase(), status_style),
            Span::raw("  "),
            Span::styled(
                format!("Uptime: {:.1}s", self.uptime_secs),
                theme.dim_style(),
            ),
        ])];
        frame.render_widget(Paragraph::new(status_lines), chunks[0]);

        // Frame statistics
        let frame_lines = vec![
            Line::from(Span::styled("── Frame Statistics ──", theme.dim_style())),
            Line::from(vec![
                Span::styled("Total frames:  ", theme.muted_style()),
                Span::styled(format!("{}", self.total_frames), theme.text_style()),
                Span::raw("    "),
                Span::styled("FPS: ", theme.muted_style()),
                Span::styled(format!("{:.1}", self.fps), theme.highlight_style()),
            ]),
            Line::from(vec![
                Span::styled("Avg frame:     ", theme.muted_style()),
                Span::styled(format!("{:.2}ms", self.avg_frame_ms), theme.text_style()),
                Span::raw("    "),
                Span::styled("P50: ", theme.muted_style()),
                Span::styled(format!("{:.2}ms", self.p50_frame_ms), theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("P95 frame:     ", theme.muted_style()),
                Span::styled(format!("{:.2}ms", self.p95_frame_ms), theme.text_style()),
                Span::raw("    "),
                Span::styled("P99: ", theme.muted_style()),
                Span::styled(
                    format!("{:.2}ms", self.p99_frame_ms),
                    self.latency_style(self.p99_frame_ms, theme),
                ),
            ]),
            Line::from(vec![
                Span::styled("Max frame:     ", theme.muted_style()),
                Span::styled(
                    format!("{:.2}ms", self.max_frame_ms),
                    self.latency_style(self.max_frame_ms, theme),
                ),
            ]),
            Line::from(vec![
                Span::styled("Slow frames:   ", theme.muted_style()),
                Span::styled(
                    format!("{} ({:.1}%)", self.slow_frames, self.slow_frame_pct),
                    if self.slow_frame_pct > 5.0 {
                        theme.warning_style()
                    } else {
                        theme.text_style()
                    },
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(frame_lines), chunks[1]);

        // Input statistics
        let input_lines = vec![
            Line::from(Span::styled("── Input Latency ──", theme.dim_style())),
            Line::from(vec![
                Span::styled("Avg key event: ", theme.muted_style()),
                Span::styled(
                    format!("{:.2}ms", self.avg_key_event_ms),
                    theme.text_style(),
                ),
                Span::raw("    "),
                Span::styled("Avg scroll: ", theme.muted_style()),
                Span::styled(format!("{:.2}ms", self.avg_scroll_ms), theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Avg latency:   ", theme.muted_style()),
                Span::styled(
                    format!("{:.2}ms", self.avg_input_latency_ms),
                    theme.text_style(),
                ),
                Span::raw("    "),
                Span::styled("P99: ", theme.muted_style()),
                Span::styled(
                    format!("{:.2}ms", self.p99_input_latency_ms),
                    self.latency_style(self.p99_input_latency_ms, theme),
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(input_lines), chunks[2]);

        // Widget statistics
        let mut widget_lines = vec![Line::from(Span::styled(
            "── Widget Render Times ──",
            theme.dim_style(),
        ))];

        if self.widget_stats.is_empty() {
            widget_lines.push(Line::from(Span::styled(
                "  No widget data yet",
                theme.dim_style(),
            )));
        } else {
            let visible_count = chunks[3].height.saturating_sub(2) as usize;
            for (name, avg, max, calls) in self
                .widget_stats
                .iter()
                .skip(self.scroll_offset)
                .take(visible_count)
            {
                widget_lines.push(Line::from(vec![
                    Span::styled(format!("  {name:12}"), theme.muted_style()),
                    Span::styled(format!("avg: {avg:6.2}ms"), theme.text_style()),
                    Span::raw("  "),
                    Span::styled(format!("max: {max:6.2}ms"), self.latency_style(*max, theme)),
                    Span::raw("  "),
                    Span::styled(format!("({calls} calls)"), theme.dim_style()),
                ]));
            }
            if self.widget_stats.len() > visible_count {
                widget_lines.push(Line::from(Span::styled(
                    format!(
                        "  ... {} more (↑/↓ to scroll)",
                        self.widget_stats.len() - visible_count - self.scroll_offset
                    ),
                    theme.dim_style(),
                )));
            }
        }
        frame.render_widget(Paragraph::new(widget_lines), chunks[3]);

        // Footer
        let footer = Line::from(Span::styled("Press Escape to close", theme.dim_style()));
        frame.render_widget(Paragraph::new(vec![footer]), chunks[4]);
    }

    /// Get style based on latency value.
    fn latency_style(&self, ms: f64, theme: &Theme) -> Style {
        if ms < 16.67 {
            theme.success_style()
        } else if ms < 50.0 {
            theme.warning_style()
        } else {
            theme.error_style()
        }
    }
}

/// Help dialog showing keybindings.
#[derive(Debug, Clone, Copy)]
pub struct HelpDialog;

impl Default for HelpDialog {
    fn default() -> Self {
        Self::new()
    }
}
impl HelpDialog {
    /// Create a new help dialog.
    pub fn new() -> Self {
        Self
    }

    /// Render the help dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 70 / 100).clamp(50, 70);
        let dialog_height = (area.height * 80 / 100).clamp(15, 25);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Help - Keybindings ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let help_text = vec![
            Line::from(vec![
                Span::styled("Ctrl+P", theme.highlight_style()),
                Span::styled("        Command palette", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+C", theme.highlight_style()),
                Span::styled("        Quit / Cancel", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Escape", theme.highlight_style()),
                Span::styled("        Cancel / Close dialog", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Enter", theme.highlight_style()),
                Span::styled("         Send message / Confirm", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+J", theme.highlight_style()),
                Span::styled("        New line in input", theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled("-- Navigation --", theme.dim_style())),
            Line::from(vec![
                Span::styled("Up/Down", theme.highlight_style()),
                Span::styled("       Scroll messages / History", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("PageUp/Down", theme.highlight_style()),
                Span::styled("   Scroll page", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Home/End", theme.highlight_style()),
                Span::styled("      First/Last message", theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "-- Leader Commands (Ctrl+X) --",
                theme.dim_style(),
            )),
            Line::from(vec![
                Span::styled("Ctrl+X N", theme.highlight_style()),
                Span::styled("      New session", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+X L", theme.highlight_style()),
                Span::styled("      Session list", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+X M", theme.highlight_style()),
                Span::styled("      Model selection", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+X B", theme.highlight_style()),
                Span::styled("      Toggle sidebar", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+X T", theme.highlight_style()),
                Span::styled("      Theme selection", theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "-- Selection Mode (in scroll mode) --",
                theme.dim_style(),
            )),
            Line::from(vec![
                Span::styled("v", theme.highlight_style()),
                Span::styled("             Enter selection mode", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("j/k", theme.highlight_style()),
                Span::styled("           Select message up/down", theme.text_style()),
            ]),
            Line::from(vec![
                Span::styled("y", theme.highlight_style()),
                Span::styled("             Copy selected message", theme.text_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled("Press Escape to close", theme.dim_style())),
        ];

        let paragraph = Paragraph::new(help_text);
        frame.render_widget(paragraph, inner);
    }
}
