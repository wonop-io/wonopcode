//! Git operations dialog for staging, committing, and managing repository changes.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::collections::HashSet;

use crate::theme::Theme;

use super::common::centered_rect;

/// Git file display information.
#[derive(Debug, Clone)]
pub struct GitFileDisplay {
    /// File path relative to repo root.
    pub path: String,
    /// Status indicator (M, A, D, R, ?, C).
    pub status: String,
    /// Whether file is staged.
    pub staged: bool,
}

/// Git commit display information.
#[derive(Debug, Clone)]
pub struct GitCommitDisplay {
    /// Short commit hash.
    pub id: String,
    /// Commit message (first line).
    pub message: String,
    /// Author name.
    pub author: String,
    /// Formatted date.
    pub date: String,
}

/// Git dialog view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitView {
    /// Main menu.
    #[default]
    Menu,
    /// Stage files view.
    Stage,
    /// Unstage files view.
    Unstage,
    /// Commit view (message input).
    Commit,
    /// History view.
    History,
}

/// Result from git dialog interactions.
#[derive(Debug, Clone)]
pub enum GitDialogResult {
    /// No action yet.
    None,
    /// Request status update.
    RefreshStatus,
    /// Request history update.
    RefreshHistory,
    /// Stage selected files.
    Stage(Vec<String>),
    /// Unstage selected files.
    Unstage(Vec<String>),
    /// Checkout (discard) selected files.
    Checkout(Vec<String>),
    /// Create commit with message.
    Commit(String),
    /// Push to remote.
    Push,
    /// Pull from remote.
    Pull,
    /// Close dialog.
    Close,
}

/// Git operations dialog.
#[derive(Debug, Clone)]
pub struct GitDialog {
    /// Current view.
    view: GitView,
    /// File list state (for stage/unstage).
    file_list_state: ListState,
    /// Files to display.
    files: Vec<GitFileDisplay>,
    /// Selected files (indices).
    selected_files: HashSet<usize>,
    /// Commit message input.
    commit_message: String,
    /// History entries.
    history: Vec<GitCommitDisplay>,
    /// History scroll state.
    history_state: ListState,
    /// Status message.
    status_message: Option<String>,
    /// Current branch.
    branch: String,
    /// Ahead/behind counts.
    ahead: usize,
    behind: usize,
    /// Loading state.
    loading: bool,
}

impl Default for GitDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl GitDialog {
    /// Create a new git dialog.
    pub fn new() -> Self {
        Self {
            view: GitView::Menu,
            file_list_state: ListState::default(),
            files: Vec::new(),
            selected_files: HashSet::new(),
            commit_message: String::new(),
            history: Vec::new(),
            history_state: ListState::default(),
            status_message: None,
            branch: String::new(),
            ahead: 0,
            behind: 0,
            loading: false,
        }
    }

    /// Update with status from server.
    pub fn set_status(
        &mut self,
        branch: String,
        ahead: usize,
        behind: usize,
        files: Vec<GitFileDisplay>,
    ) {
        self.branch = branch;
        self.ahead = ahead;
        self.behind = behind;
        self.files = files;
        self.selected_files.clear();
        if !self.files.is_empty() {
            self.file_list_state.select(Some(0));
        }
        self.loading = false;
    }

    /// Update with history from server.
    pub fn set_history(&mut self, commits: Vec<GitCommitDisplay>) {
        self.history = commits;
        if !self.history.is_empty() {
            self.history_state.select(Some(0));
        }
        self.loading = false;
    }

    /// Set a status message (shown at bottom of dialog).
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Clear the status message.
    pub fn clear_message(&mut self) {
        self.status_message = None;
    }

    /// Set loading state.
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Get current view.
    pub fn view(&self) -> GitView {
        self.view
    }

    /// Handle a key event. Returns the result action.
    pub fn handle_key(&mut self, key: KeyEvent) -> GitDialogResult {
        match self.view {
            GitView::Menu => self.handle_menu_key(key),
            GitView::Stage | GitView::Unstage => self.handle_file_list_key(key),
            GitView::Commit => self.handle_commit_key(key),
            GitView::History => self.handle_history_key(key),
        }
    }

    fn handle_menu_key(&mut self, key: KeyEvent) -> GitDialogResult {
        match key.code {
            KeyCode::Char('s') | KeyCode::Char('1') => {
                self.view = GitView::Stage;
                GitDialogResult::RefreshStatus
            }
            KeyCode::Char('u') | KeyCode::Char('2') => {
                self.view = GitView::Unstage;
                GitDialogResult::RefreshStatus
            }
            KeyCode::Char('c') | KeyCode::Char('3') => {
                self.view = GitView::Commit;
                self.commit_message.clear();
                GitDialogResult::None
            }
            KeyCode::Char('h') | KeyCode::Char('4') => {
                self.view = GitView::History;
                GitDialogResult::RefreshHistory
            }
            KeyCode::Char('p') | KeyCode::Char('5') => GitDialogResult::Push,
            KeyCode::Char('l') | KeyCode::Char('6') => GitDialogResult::Pull,
            KeyCode::Esc | KeyCode::Char('q') => GitDialogResult::Close,
            _ => GitDialogResult::None,
        }
    }

    fn handle_file_list_key(&mut self, key: KeyEvent) -> GitDialogResult {
        let is_stage_view = self.view == GitView::Stage;

        // Filter files based on view
        let filtered_indices: Vec<usize> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| if is_stage_view { !f.staged } else { f.staged })
            .map(|(i, _)| i)
            .collect();

        match key.code {
            KeyCode::Esc => {
                self.view = GitView::Menu;
                self.selected_files.clear();
                GitDialogResult::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(current) = self.file_list_state.selected() {
                    if current > 0 {
                        self.file_list_state.select(Some(current - 1));
                    }
                }
                GitDialogResult::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(current) = self.file_list_state.selected() {
                    if current < filtered_indices.len().saturating_sub(1) {
                        self.file_list_state.select(Some(current + 1));
                    }
                } else if !filtered_indices.is_empty() {
                    self.file_list_state.select(Some(0));
                }
                GitDialogResult::None
            }
            KeyCode::Char(' ') => {
                // Toggle selection
                if let Some(visual_idx) = self.file_list_state.selected() {
                    if let Some(&file_idx) = filtered_indices.get(visual_idx) {
                        if self.selected_files.contains(&file_idx) {
                            self.selected_files.remove(&file_idx);
                        } else {
                            self.selected_files.insert(file_idx);
                        }
                    }
                }
                GitDialogResult::None
            }
            KeyCode::Char('a') => {
                // Select/deselect all
                if self.selected_files.len() == filtered_indices.len() {
                    self.selected_files.clear();
                } else {
                    self.selected_files = filtered_indices.iter().copied().collect();
                }
                GitDialogResult::None
            }
            KeyCode::Enter => {
                // Apply action to selected files (or current if none selected)
                let paths: Vec<String> = if self.selected_files.is_empty() {
                    // Use current selection
                    if let Some(visual_idx) = self.file_list_state.selected() {
                        filtered_indices
                            .get(visual_idx)
                            .map(|&i| vec![self.files[i].path.clone()])
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                } else {
                    self.selected_files
                        .iter()
                        .map(|&i| self.files[i].path.clone())
                        .collect()
                };

                if paths.is_empty() {
                    return GitDialogResult::None;
                }

                self.selected_files.clear();
                if is_stage_view {
                    GitDialogResult::Stage(paths)
                } else {
                    GitDialogResult::Unstage(paths)
                }
            }
            KeyCode::Char('d') if !is_stage_view => {
                // Checkout (discard) in unstage view
                let paths: Vec<String> = if self.selected_files.is_empty() {
                    if let Some(visual_idx) = self.file_list_state.selected() {
                        filtered_indices
                            .get(visual_idx)
                            .map(|&i| vec![self.files[i].path.clone()])
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                } else {
                    self.selected_files
                        .iter()
                        .map(|&i| self.files[i].path.clone())
                        .collect()
                };

                if paths.is_empty() {
                    return GitDialogResult::None;
                }

                self.selected_files.clear();
                GitDialogResult::Checkout(paths)
            }
            _ => GitDialogResult::None,
        }
    }

    fn handle_commit_key(&mut self, key: KeyEvent) -> GitDialogResult {
        match key.code {
            KeyCode::Esc => {
                self.view = GitView::Menu;
                self.commit_message.clear();
                GitDialogResult::None
            }
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    // Ctrl+Enter or Alt+Enter submits
                    if !self.commit_message.trim().is_empty() {
                        let msg = std::mem::take(&mut self.commit_message);
                        self.view = GitView::Menu;
                        return GitDialogResult::Commit(msg);
                    }
                } else {
                    // Regular enter adds newline
                    self.commit_message.push('\n');
                }
                GitDialogResult::None
            }
            KeyCode::Char(c) => {
                self.commit_message.push(c);
                GitDialogResult::None
            }
            KeyCode::Backspace => {
                self.commit_message.pop();
                GitDialogResult::None
            }
            _ => GitDialogResult::None,
        }
    }

    fn handle_history_key(&mut self, key: KeyEvent) -> GitDialogResult {
        match key.code {
            KeyCode::Esc => {
                self.view = GitView::Menu;
                GitDialogResult::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(current) = self.history_state.selected() {
                    if current > 0 {
                        self.history_state.select(Some(current - 1));
                    }
                }
                GitDialogResult::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(current) = self.history_state.selected() {
                    if current < self.history.len().saturating_sub(1) {
                        self.history_state.select(Some(current + 1));
                    }
                } else if !self.history.is_empty() {
                    self.history_state.select(Some(0));
                }
                GitDialogResult::None
            }
            _ => GitDialogResult::None,
        }
    }

    /// Render the git dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 70 / 100).clamp(50, 90);
        let dialog_height = (area.height * 80 / 100).clamp(15, 35);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        match self.view {
            GitView::Menu => self.render_menu(frame, dialog_area, theme),
            GitView::Stage => self.render_file_list(frame, dialog_area, theme, true),
            GitView::Unstage => self.render_file_list(frame, dialog_area, theme, false),
            GitView::Commit => self.render_commit(frame, dialog_area, theme),
            GitView::History => self.render_history(frame, dialog_area, theme),
        }
    }

    fn render_menu(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .title(" Git ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Branch info at top
        let branch_info = if !self.branch.is_empty() {
            let mut info = format!("Branch: {}", self.branch);
            if self.ahead > 0 || self.behind > 0 {
                info.push_str(&format!(" (↑{} ↓{})", self.ahead, self.behind));
            }
            info
        } else {
            "Loading...".to_string()
        };

        let menu_items = vec![
            ("s", "Stage files", "Add files to index"),
            ("u", "Unstage files", "Remove files from index"),
            ("c", "Commit", "Create a commit"),
            ("h", "History", "View commit history"),
            ("p", "Push", "Push to remote"),
            ("l", "Pull", "Pull from remote"),
        ];

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(branch_info, theme.highlight_style())),
            Line::from(""),
        ];

        for (key, label, desc) in menu_items {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" [{key}] "),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{label:16}"), theme.text_style()),
                Span::styled(desc, theme.dim_style()),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(" [Esc] ", theme.dim_style()),
            Span::styled("Close", theme.dim_style()),
        ]));

        // Show status message if any
        if let Some(ref msg) = self.status_message {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                msg.clone(),
                theme.highlight_style(),
            )));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    fn render_file_list(&mut self, frame: &mut Frame, area: Rect, theme: &Theme, is_stage: bool) {
        let title = if is_stage {
            " Stage Files (unstaged) "
        } else {
            " Unstage Files (staged) "
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into list and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        // Filter files based on view
        let filtered: Vec<(usize, &GitFileDisplay)> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| if is_stage { !f.staged } else { f.staged })
            .collect();

        if filtered.is_empty() {
            let msg = if is_stage {
                "No unstaged changes"
            } else {
                "No staged changes"
            };
            let para = Paragraph::new(Span::styled(msg, theme.dim_style()));
            frame.render_widget(para, chunks[0]);
        } else {
            let list_items: Vec<ListItem> = filtered
                .iter()
                .map(|(idx, file)| {
                    let selected = self.selected_files.contains(idx);
                    let checkbox = if selected { "[x]" } else { "[ ]" };

                    let status_style = match file.status.as_str() {
                        "M" => Style::default().fg(theme.warning),
                        "A" => Style::default().fg(theme.success),
                        "D" => Style::default().fg(theme.error),
                        "?" => Style::default().fg(theme.text_muted),
                        _ => theme.text_style(),
                    };

                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{checkbox} "), theme.text_style()),
                        Span::styled(format!("{:2} ", file.status), status_style),
                        Span::styled(&file.path, theme.text_style()),
                    ]))
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

            frame.render_stateful_widget(list, chunks[0], &mut self.file_list_state);
        }

        // Help text
        let help = if is_stage {
            "Space: toggle  a: all  Enter: stage  Esc: back"
        } else {
            "Space: toggle  a: all  Enter: unstage  d: discard  Esc: back"
        };
        let help_para =
            Paragraph::new(Span::styled(help, theme.dim_style())).alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[1]);
    }

    fn render_commit(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .title(" Commit ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into message area and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        // Message input
        let msg_block = Block::default()
            .title(" Message ")
            .borders(Borders::ALL)
            .border_style(theme.border_style());

        let msg_text = if self.commit_message.is_empty() {
            Span::styled("Enter commit message...", theme.dim_style())
        } else {
            Span::styled(&self.commit_message, theme.text_style())
        };

        let msg_para = Paragraph::new(msg_text)
            .block(msg_block)
            .wrap(Wrap { trim: false });
        frame.render_widget(msg_para, chunks[0]);

        // Help
        let help = "Ctrl+Enter: commit  Esc: cancel";
        let help_para =
            Paragraph::new(Span::styled(help, theme.dim_style())).alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[1]);
    }

    fn render_history(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .title(" History ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into list and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        if self.history.is_empty() {
            let para = Paragraph::new(Span::styled("No commits", theme.dim_style()));
            frame.render_widget(para, chunks[0]);
        } else {
            let list_items: Vec<ListItem> = self
                .history
                .iter()
                .map(|commit| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", commit.id),
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(&commit.message, theme.text_style()),
                        Span::styled(format!("  ({})", commit.author), theme.dim_style()),
                    ]))
                })
                .collect();

            let list = List::new(list_items)
                .highlight_style(
                    Style::default()
                        .bg(theme.border_active)
                        .fg(theme.background),
                )
                .highlight_symbol("> ");

            frame.render_stateful_widget(list, chunks[0], &mut self.history_state);
        }

        // Help
        let help_para = Paragraph::new(Span::styled("j/k: navigate  Esc: back", theme.dim_style()))
            .alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[1]);
    }
}
