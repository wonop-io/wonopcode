//! Sandbox management dialog.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use wonopcode_tui_core::Theme;

use crate::common::centered_rect;

/// Sandbox action in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxAction {
    /// Start the sandbox.
    Start,
    /// Stop the sandbox.
    Stop,
    /// Restart the sandbox.
    Restart,
    /// Show status (cancel dialog).
    Status,
}

/// Sandbox state for the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxState {
    /// Sandbox is disabled in config.
    #[default]
    Disabled,
    /// Sandbox is stopped.
    Stopped,
    /// Sandbox is starting.
    Starting,
    /// Sandbox is running.
    Running,
    /// Sandbox has an error.
    Error,
}

/// Sandbox management dialog.
#[derive(Debug, Clone)]
pub struct SandboxDialog {
    /// Current sandbox state.
    state: SandboxState,
    /// Runtime name (e.g., "Docker", "Lima").
    runtime: Option<String>,
    /// Error message if state is Error.
    error: Option<String>,
    /// Selected option index.
    selected: usize,
    /// Available options based on state.
    options: Vec<(SandboxAction, &'static str, &'static str)>,
}

impl SandboxDialog {
    /// Create a new sandbox dialog.
    pub fn new(state: SandboxState, runtime: Option<String>, error: Option<String>) -> Self {
        let options = Self::options_for_state(state);
        Self {
            state,
            runtime,
            error,
            selected: 0,
            options,
        }
    }

    /// Get available options based on sandbox state.
    fn options_for_state(state: SandboxState) -> Vec<(SandboxAction, &'static str, &'static str)> {
        match state {
            SandboxState::Disabled => {
                vec![(SandboxAction::Status, "Status", "Sandbox is not configured")]
            }
            SandboxState::Stopped => {
                vec![
                    (
                        SandboxAction::Start,
                        "Start Sandbox",
                        "Start the sandbox container",
                    ),
                    (SandboxAction::Status, "Status", "Show current status"),
                ]
            }
            SandboxState::Starting => {
                vec![(
                    SandboxAction::Status,
                    "Starting...",
                    "Sandbox is starting up",
                )]
            }
            SandboxState::Running => {
                vec![
                    (
                        SandboxAction::Stop,
                        "Stop Sandbox",
                        "Stop the running sandbox",
                    ),
                    (
                        SandboxAction::Restart,
                        "Restart Sandbox",
                        "Restart the sandbox",
                    ),
                    (SandboxAction::Status, "Status", "Show current status"),
                ]
            }
            SandboxState::Error => {
                vec![
                    (SandboxAction::Start, "Start Sandbox", "Try starting again"),
                    (SandboxAction::Status, "Status", "Show error details"),
                ]
            }
        }
    }

    /// Handle a key event. Returns Some(action) if an action was selected.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<SandboxAction> {
        match key.code {
            KeyCode::Enter => {
                return self
                    .options
                    .get(self.selected)
                    .map(|(action, _, _)| *action);
            }
            KeyCode::Esc => {
                return Some(SandboxAction::Status); // Close dialog
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                if self.selected < self.options.len().saturating_sub(1) {
                    self.selected += 1;
                }
            }
            KeyCode::Home => {
                self.selected = 0;
            }
            KeyCode::End => {
                self.selected = self.options.len().saturating_sub(1);
            }
            // Quick keys
            KeyCode::Char('s') | KeyCode::Char('S') => {
                // Find Start action
                for (i, (action, _, _)) in self.options.iter().enumerate() {
                    if *action == SandboxAction::Start {
                        self.selected = i;
                        return Some(SandboxAction::Start);
                    }
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                // Find Stop action
                for (i, (action, _, _)) in self.options.iter().enumerate() {
                    if *action == SandboxAction::Stop {
                        self.selected = i;
                        return Some(SandboxAction::Stop);
                    }
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                // Find Restart action
                for (i, (action, _, _)) in self.options.iter().enumerate() {
                    if *action == SandboxAction::Restart {
                        self.selected = i;
                        return Some(SandboxAction::Restart);
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// Render the sandbox dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 50.min(area.width.saturating_sub(4));
        let dialog_height = 12.min(area.height.saturating_sub(4));
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Sandbox ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into status, options, and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Status info
                Constraint::Min(1),    // Options
                Constraint::Length(2), // Help
            ])
            .split(inner);

        // Status section
        let (status_icon, status_text, status_style) = match self.state {
            SandboxState::Disabled => ("◇", "Not configured", theme.muted_style()),
            SandboxState::Stopped => ("○", "Stopped", theme.warning_style()),
            SandboxState::Starting => ("⋯", "Starting...", theme.warning_style()),
            SandboxState::Running => ("●", "Running", theme.success_style()),
            SandboxState::Error => ("✗", "Error", theme.error_style()),
        };

        let runtime_text = self.runtime.as_deref().unwrap_or("sandbox");
        let mut status_lines = vec![Line::from(vec![
            Span::styled(format!("{status_icon} "), status_style),
            Span::styled(format!("{runtime_text} - {status_text}"), status_style),
        ])];

        // Add error message if present
        if let Some(ref error) = self.error {
            status_lines.push(Line::from(Span::styled(
                format!("  {error}"),
                theme.error_style(),
            )));
        }

        let status_para = Paragraph::new(status_lines);
        frame.render_widget(status_para, chunks[0]);

        // Options list
        let list_items: Vec<ListItem> = self
            .options
            .iter()
            .map(|(action, label, desc)| {
                let key_hint = match action {
                    SandboxAction::Start => "[s]",
                    SandboxAction::Stop => "[x]",
                    SandboxAction::Restart => "[r]",
                    SandboxAction::Status => "",
                };

                let spans = vec![
                    Span::styled(*label, theme.text_style()),
                    Span::styled(format!(" {key_hint} "), theme.highlight_style()),
                    Span::styled(format!("- {desc}"), theme.dim_style()),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));

        let list = List::new(list_items)
            .highlight_style(
                Style::default()
                    .bg(theme.border_active)
                    .fg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, chunks[1], &mut list_state);

        // Help text
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", theme.highlight_style()),
            Span::styled(" select  ", theme.dim_style()),
            Span::styled("s/x/r", theme.highlight_style()),
            Span::styled(" quick action  ", theme.dim_style()),
            Span::styled("Esc", theme.highlight_style()),
            Span::styled(" close", theme.dim_style()),
        ])];
        let help_para = Paragraph::new(help_lines).alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[2]);
    }
}
