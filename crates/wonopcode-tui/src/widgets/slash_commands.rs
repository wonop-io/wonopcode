//! Slash command autocomplete widget.
//!
//! Provides autocomplete suggestions for slash commands when typing '/'.

use crate::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

/// Maximum number of suggestions to show.
const MAX_SUGGESTIONS: usize = 15;

/// A slash command definition.
#[derive(Debug, Clone)]
pub struct SlashCommand {
    /// Command name (without the leading /).
    pub name: String,
    /// Short description.
    pub description: String,
    /// Optional aliases.
    pub aliases: Vec<String>,
    /// Whether this is a test/debug command.
    pub is_test_command: bool,
}

impl SlashCommand {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            aliases: vec![],
            is_test_command: false,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn test_command(mut self) -> Self {
        self.is_test_command = true;
        self
    }
}

/// Slash command autocomplete state and logic.
#[derive(Debug, Clone)]
pub struct SlashCommandAutocomplete {
    /// Whether autocomplete is visible.
    visible: bool,
    /// The filter text after '/'.
    filter: String,
    /// Available commands.
    commands: Vec<SlashCommand>,
    /// Filtered suggestions.
    suggestions: Vec<usize>,
    /// Selected index.
    selected: usize,
    /// Whether test commands are enabled.
    test_commands_enabled: bool,
}

impl Default for SlashCommandAutocomplete {
    fn default() -> Self {
        Self::new()
    }
}

impl SlashCommandAutocomplete {
    /// Create a new slash command autocomplete with built-in commands.
    pub fn new() -> Self {
        let commands = vec![
            // Session commands
            SlashCommand::new("new", "Create a new session").with_alias("clear"),
            SlashCommand::new("undo", "Undo the last message"),
            SlashCommand::new("redo", "Redo an undone message"),
            SlashCommand::new("compact", "Compact conversation history").with_alias("summarize"),
            SlashCommand::new("rename", "Rename the current session"),
            SlashCommand::new("copy", "Copy session transcript to clipboard"),
            SlashCommand::new("export", "Export session transcript to file"),
            SlashCommand::new("timeline", "Jump to a specific message"),
            SlashCommand::new("fork", "Fork from a message"),
            SlashCommand::new("thinking", "Toggle thinking visibility"),
            SlashCommand::new("share", "Share the current session"),
            SlashCommand::new("unshare", "Unshare a session"),
            // Navigation commands
            SlashCommand::new("sessions", "List all sessions")
                .with_alias("session")
                .with_alias("resume")
                .with_alias("continue"),
            SlashCommand::new("models", "List and select a model"),
            SlashCommand::new("agents", "List and select an agent").with_alias("agent"),
            SlashCommand::new("theme", "Change the theme"),
            SlashCommand::new("status", "Show configuration status"),
            SlashCommand::new("settings", "Open settings dialog")
                .with_alias("config")
                .with_alias("preferences"),
            SlashCommand::new("mcp", "Toggle MCP servers"),
            SlashCommand::new("sandbox", "Manage sandbox"),
            SlashCommand::new("connect", "Connect to a provider"),
            // UI commands
            SlashCommand::new("editor", "Open input in external editor"),
            SlashCommand::new("sidebar", "Toggle the sidebar"),
            SlashCommand::new("commands", "Show all commands"),
            SlashCommand::new("help", "Show help"),
            // Debug/testing commands (hidden by default)
            SlashCommand::new("perf", "Show TUI performance metrics").test_command(),
            SlashCommand::new(
                "add_test_messages",
                "Add 100 test messages for performance testing",
            )
            .test_command(),
            SlashCommand::new("quit", "Quit the application")
                .with_alias("exit")
                .with_alias("q"),
        ];

        Self {
            visible: false,
            filter: String::new(),
            commands,
            suggestions: vec![],
            selected: 0,
            test_commands_enabled: false,
        }
    }

    /// Add a custom command.
    pub fn add_command(&mut self, command: SlashCommand) {
        self.commands.push(command);
    }

    /// Set whether test commands are enabled.
    pub fn set_test_commands_enabled(&mut self, enabled: bool) {
        self.test_commands_enabled = enabled;
        // Re-filter if visible
        if self.visible {
            self.update_suggestions();
        }
    }

    /// Check if autocomplete is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show autocomplete with initial filter.
    pub fn show(&mut self, filter: &str) {
        self.visible = true;
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

    /// Get the current filter.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Get the selected command, if any.
    pub fn selected_command(&self) -> Option<&SlashCommand> {
        self.suggestions
            .get(self.selected)
            .and_then(|&idx| self.commands.get(idx))
    }

    /// Update suggestions based on current filter.
    fn update_suggestions(&mut self) {
        self.suggestions.clear();

        let filter_lower = self.filter.to_lowercase();

        for (idx, cmd) in self.commands.iter().enumerate() {
            // Skip test commands if not enabled
            if cmd.is_test_command && !self.test_commands_enabled {
                continue;
            }

            // Match against name
            if cmd.name.to_lowercase().contains(&filter_lower) {
                self.suggestions.push(idx);
                continue;
            }

            // Match against aliases
            if cmd
                .aliases
                .iter()
                .any(|a| a.to_lowercase().contains(&filter_lower))
            {
                self.suggestions.push(idx);
                continue;
            }

            // Match against description
            if cmd.description.to_lowercase().contains(&filter_lower) {
                self.suggestions.push(idx);
            }

            if self.suggestions.len() >= MAX_SUGGESTIONS {
                break;
            }
        }

        // If filter is empty, show all visible commands (up to limit)
        if filter_lower.is_empty() {
            self.suggestions = self
                .commands
                .iter()
                .enumerate()
                .filter(|(_, cmd)| !cmd.is_test_command || self.test_commands_enabled)
                .map(|(idx, _)| idx)
                .take(MAX_SUGGESTIONS)
                .collect();
        }

        // Ensure selected is in bounds
        if self.selected >= self.suggestions.len() {
            self.selected = 0;
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> SlashCommandAction {
        if !self.visible {
            return SlashCommandAction::None;
        }

        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                } else if !self.suggestions.is_empty() {
                    self.selected = self.suggestions.len() - 1;
                }
                SlashCommandAction::Handled
            }
            KeyCode::Down => {
                if self.selected < self.suggestions.len().saturating_sub(1) {
                    self.selected += 1;
                } else {
                    self.selected = 0;
                }
                SlashCommandAction::Handled
            }
            KeyCode::Tab | KeyCode::Enter => {
                if let Some(cmd) = self.selected_command() {
                    let name = cmd.name.clone();
                    self.hide();
                    SlashCommandAction::Execute(name)
                } else {
                    self.hide();
                    SlashCommandAction::Handled
                }
            }
            KeyCode::Esc => {
                self.hide();
                SlashCommandAction::Handled
            }
            _ => SlashCommandAction::None,
        }
    }

    /// Render the autocomplete popup.
    pub fn render(&self, frame: &mut Frame, input_area: Rect, theme: &Theme) {
        if !self.visible || self.suggestions.is_empty() {
            return;
        }

        // Position above the input
        let height = (self.suggestions.len() as u16 + 2).min(17);
        let width = input_area.width.min(50);

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
            .filter_map(|(i, &cmd_idx)| {
                let cmd = self.commands.get(cmd_idx)?;
                let is_selected = i == self.selected;

                let style = if is_selected {
                    Style::default().fg(theme.background).bg(theme.primary)
                } else {
                    theme.text_style()
                };

                let desc_style = if is_selected {
                    Style::default().fg(theme.background).bg(theme.primary)
                } else {
                    theme.muted_style()
                };

                Some(ListItem::new(Line::from(vec![
                    Span::styled(format!("/{}", cmd.name), style),
                    Span::styled("  ", style),
                    Span::styled(&cmd.description, desc_style),
                ])))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background_element))
            .title(" Commands ");

        let list = List::new(items).block(block);

        frame.render_widget(list, popup_area);
    }
}

/// Action returned from slash command key handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandAction {
    /// No action taken.
    None,
    /// Key was handled, no selection made.
    Handled,
    /// A command was selected for execution.
    Execute(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slash_commands() {
        let mut ac = SlashCommandAutocomplete::new();
        assert!(!ac.is_visible());

        ac.show("");
        assert!(ac.is_visible());
        assert!(!ac.suggestions.is_empty());

        ac.set_filter("new");
        assert!(!ac.suggestions.is_empty());
    }
}
