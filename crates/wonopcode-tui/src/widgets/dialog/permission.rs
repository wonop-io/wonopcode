//! Dialog for requesting permission for a tool action.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::theme::Theme;

use super::common::centered_rect;

/// Result of a permission dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionResult {
    /// Allow this action.
    Allow,
    /// Deny this action.
    Deny,
    /// Allow and remember for this session.
    AllowAlways,
    /// Deny and remember for this session.
    DenyAlways,
    /// Cancelled (escape pressed).
    Cancelled,
}

/// Dialog for requesting permission for a tool action.
#[derive(Debug, Clone)]
pub struct PermissionDialog {
    /// Request ID.
    pub request_id: String,
    /// Tool name.
    pub tool: String,
    /// Action being performed.
    pub action: String,
    /// Human-readable description.
    pub description: String,
    /// Path involved (for file operations).
    pub path: Option<String>,
    /// Currently selected option (0 = Allow, 1 = Deny, 2 = Always Allow, 3 = Always Deny).
    selected: usize,
}

impl PermissionDialog {
    /// Create a new permission dialog.
    pub fn new(
        request_id: String,
        tool: String,
        action: String,
        description: String,
        path: Option<String>,
    ) -> Self {
        Self {
            request_id,
            tool,
            action,
            description,
            path,
            selected: 0,
        }
    }

    /// Handle a key event. Returns Some(result) if a choice was made.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<PermissionResult> {
        match key.code {
            KeyCode::Enter => {
                return Some(match self.selected {
                    0 => PermissionResult::Allow,
                    1 => PermissionResult::Deny,
                    2 => PermissionResult::AllowAlways,
                    3 => PermissionResult::DenyAlways,
                    _ => PermissionResult::Allow,
                });
            }
            KeyCode::Esc => {
                return Some(PermissionResult::Cancelled);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.selected < 3 {
                    self.selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                // Move between rows (0,1) and (2,3)
                if self.selected >= 2 {
                    self.selected -= 2;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected < 2 {
                    self.selected += 2;
                }
            }
            // Quick keys
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                return Some(PermissionResult::Allow);
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                return Some(PermissionResult::Deny);
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                return Some(PermissionResult::AllowAlways);
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                return Some(PermissionResult::DenyAlways);
            }
            _ => {}
        }
        None
    }

    /// Render the permission dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 60.min(area.width.saturating_sub(4));
        let dialog_height = 14.min(area.height.saturating_sub(4));
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Permission Required ")
            .borders(Borders::ALL)
            .border_style(theme.accent_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout: description, path, buttons
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(2), // Tool info
                Constraint::Length(2), // Description
                Constraint::Length(2), // Path (if any)
                Constraint::Length(1), // Spacer
                Constraint::Length(2), // Buttons row 1
                Constraint::Length(1), // Buttons row 2
            ])
            .split(inner);

        // Tool info
        let tool_text = Paragraph::new(Line::from(vec![
            Span::styled("Tool: ", theme.muted_style()),
            Span::styled(&self.tool, theme.accent_style()),
            Span::raw("  "),
            Span::styled("Action: ", theme.muted_style()),
            Span::styled(&self.action, theme.text_style()),
        ]));
        frame.render_widget(tool_text, chunks[0]);

        // Description
        let desc_text = Paragraph::new(self.description.as_str())
            .style(theme.text_style())
            .wrap(Wrap { trim: true });
        frame.render_widget(desc_text, chunks[1]);

        // Path (if present)
        if let Some(ref path) = self.path {
            let path_text = Paragraph::new(Line::from(vec![
                Span::styled("Path: ", theme.muted_style()),
                Span::styled(path, theme.text_style()),
            ]));
            frame.render_widget(path_text, chunks[2]);
        }

        // Button styles
        let button_style = |idx: usize| {
            if self.selected == idx {
                theme.accent_style()
            } else {
                theme.muted_style()
            }
        };

        // Buttons row 1: Allow / Deny
        let row1 = Paragraph::new(Line::from(vec![
            Span::styled(" [Y] Allow ", button_style(0)),
            Span::raw("   "),
            Span::styled(" [N] Deny ", button_style(1)),
        ]));
        frame.render_widget(row1, chunks[4]);

        // Buttons row 2: Always Allow / Always Deny
        let row2 = Paragraph::new(Line::from(vec![
            Span::styled(" [A] Always Allow ", button_style(2)),
            Span::raw(" "),
            Span::styled(" [D] Always Deny ", button_style(3)),
        ]));
        frame.render_widget(row2, chunks[5]);
    }
}
