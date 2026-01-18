//! Configurable keybind system.
//!
//! Supports user-customizable keyboard shortcuts with:
//! - Leader key sequences (e.g., `<leader>n` for new session)
//! - Modifier combinations (e.g., `ctrl+c`, `alt+enter`)
//! - Multiple bindings per action (e.g., `ctrl+c,ctrl+d`)

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A keybind action identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    // Application
    AppExit,

    // Editor
    EditorOpen,

    // Theme
    ThemeList,

    // Sidebar
    SidebarToggle,

    // Session
    SessionNew,
    SessionList,
    SessionExport,
    SessionInterrupt,
    SessionCompact,
    SessionTimeline,

    // Messages
    MessagesPageUp,
    MessagesPageDown,
    MessagesHalfPageUp,
    MessagesHalfPageDown,
    MessagesFirst,
    MessagesLast,
    MessagesNext,
    MessagesPrev,

    // Input
    InputSubmit,
    InputNewline,
    InputHistory,
    InputCancel,

    // Models/Agents
    ModelList,
    AgentList,

    // Edit
    EditUndo,
    EditRedo,
    EditCopy,

    // Revert
    SessionRevert,
    SessionUnrevert,

    // Command palette
    CommandPalette,

    // Help
    HelpToggle,

    // Settings
    SettingsOpen,
}

impl KeyAction {
    /// Get all actions.
    pub fn all() -> &'static [KeyAction] {
        use KeyAction::*;
        &[
            AppExit,
            EditorOpen,
            ThemeList,
            SidebarToggle,
            SessionNew,
            SessionList,
            SessionExport,
            SessionInterrupt,
            SessionCompact,
            SessionTimeline,
            MessagesPageUp,
            MessagesPageDown,
            MessagesHalfPageUp,
            MessagesHalfPageDown,
            MessagesFirst,
            MessagesLast,
            MessagesNext,
            MessagesPrev,
            InputSubmit,
            InputNewline,
            InputHistory,
            InputCancel,
            ModelList,
            AgentList,
            EditUndo,
            EditRedo,
            EditCopy,
            SessionRevert,
            SessionUnrevert,
            CommandPalette,
            HelpToggle,
            SettingsOpen,
        ]
    }

    /// Get the default keybind string for this action.
    pub fn default_binding(&self) -> &'static str {
        use KeyAction::*;
        match self {
            AppExit => "ctrl+x ctrl+c",
            EditorOpen => "<leader>e",
            ThemeList => "<leader>t",
            SidebarToggle => "<leader>b",
            SessionNew => "<leader>n",
            SessionList => "<leader>l",
            SessionExport => "<leader>x",
            SessionInterrupt => "escape",
            SessionCompact => "<leader>c",
            SessionTimeline => "<leader>g",
            MessagesPageUp => "pageup",
            MessagesPageDown => "pagedown",
            MessagesHalfPageUp => "ctrl+u",
            MessagesHalfPageDown => "ctrl+d",
            MessagesFirst => "home",
            MessagesLast => "end",
            MessagesNext => "j,down",
            MessagesPrev => "k,up",
            InputSubmit => "enter",
            InputNewline => "ctrl+j,shift+enter",
            InputHistory => "up,down",
            InputCancel => "ctrl+c",
            ModelList => "<leader>m",
            AgentList => "<leader>a",
            EditUndo => "<leader>u",
            EditRedo => "<leader>r",
            EditCopy => "<leader>y",
            SessionRevert => "<leader>z",
            SessionUnrevert => "<leader>Z",
            CommandPalette => "ctrl+p",
            HelpToggle => "?",
            SettingsOpen => "<leader>s",
        }
    }

    /// Get a human-readable description of this action.
    pub fn description(&self) -> &'static str {
        use KeyAction::*;
        match self {
            AppExit => "Exit the application",
            EditorOpen => "Open external editor",
            ThemeList => "List available themes",
            SidebarToggle => "Toggle sidebar",
            SessionNew => "Create a new session",
            SessionList => "List all sessions",
            SessionExport => "Export session",
            SessionInterrupt => "Interrupt current operation",
            SessionCompact => "Compact the session",
            SessionTimeline => "Show session timeline",
            MessagesPageUp => "Scroll messages up by one page",
            MessagesPageDown => "Scroll messages down by one page",
            MessagesHalfPageUp => "Scroll messages up by half page",
            MessagesHalfPageDown => "Scroll messages down by half page",
            MessagesFirst => "Navigate to first message",
            MessagesLast => "Navigate to last message",
            MessagesNext => "Navigate to next message",
            MessagesPrev => "Navigate to previous message",
            InputSubmit => "Submit input",
            InputNewline => "Insert newline",
            InputHistory => "Navigate input history",
            InputCancel => "Cancel input",
            ModelList => "List available models",
            AgentList => "List available agents",
            EditUndo => "Undo last message",
            EditRedo => "Redo undone message",
            EditCopy => "Copy last response",
            SessionRevert => "Revert to previous message",
            SessionUnrevert => "Cancel revert",
            CommandPalette => "Open command palette",
            HelpToggle => "Toggle help",
            SettingsOpen => "Open settings",
        }
    }
}

/// A parsed keybind.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keybind {
    /// The key name (e.g., "a", "enter", "escape").
    pub key: String,
    /// Control modifier.
    pub ctrl: bool,
    /// Alt/Meta modifier.
    pub alt: bool,
    /// Shift modifier.
    pub shift: bool,
    /// Whether this is after a leader key.
    pub leader: bool,
}

impl Keybind {
    /// Create a new keybind.
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            ctrl: false,
            alt: false,
            shift: false,
            leader: false,
        }
    }

    /// Add control modifier.
    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// Add alt modifier.
    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Add shift modifier.
    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Add leader prefix.
    pub fn leader(mut self) -> Self {
        self.leader = true;
        self
    }

    /// Parse a keybind string (e.g., "ctrl+c", "`<leader>n`").
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        if s == "none" || s.is_empty() {
            return None;
        }

        // Handle <leader> syntax
        let (leader, rest) = if s.starts_with("<leader>") {
            (true, s.strip_prefix("<leader>").unwrap_or(&s).trim())
        } else if s.starts_with("leader+") {
            (true, s.strip_prefix("leader+").unwrap_or(&s).trim())
        } else {
            (false, s.as_str())
        };

        let parts: Vec<&str> = rest.split('+').collect();
        let mut keybind = Keybind {
            key: String::new(),
            ctrl: false,
            alt: false,
            shift: false,
            leader,
        };

        for part in parts {
            match part {
                "ctrl" | "control" => keybind.ctrl = true,
                "alt" | "meta" | "option" => keybind.alt = true,
                "shift" => keybind.shift = true,
                "esc" => keybind.key = "escape".to_string(),
                "del" => keybind.key = "delete".to_string(),
                "ins" => keybind.key = "insert".to_string(),
                "pgup" => keybind.key = "pageup".to_string(),
                "pgdn" | "pgdown" => keybind.key = "pagedown".to_string(),
                other if !other.is_empty() => keybind.key = other.to_string(),
                _ => {}
            }
        }

        if keybind.key.is_empty() {
            None
        } else {
            Some(keybind)
        }
    }

    /// Parse multiple keybinds separated by commas.
    pub fn parse_multi(s: &str) -> Vec<Self> {
        s.split(',')
            .filter_map(|part| Self::parse(part.trim()))
            .collect()
    }

    /// Check if this keybind matches a key event.
    pub fn matches(&self, key: &KeyEvent, leader_active: bool) -> bool {
        // Check leader state
        if self.leader != leader_active {
            return false;
        }

        // Check modifiers
        let ctrl_match = self.ctrl == key.modifiers.contains(KeyModifiers::CONTROL);
        let alt_match = self.alt == key.modifiers.contains(KeyModifiers::ALT);
        let shift_match = self.shift == key.modifiers.contains(KeyModifiers::SHIFT);

        if !ctrl_match || !alt_match || !shift_match {
            return false;
        }

        // Check key
        match &key.code {
            KeyCode::Char(c) => self.key == c.to_lowercase().to_string(),
            KeyCode::Enter => self.key == "enter",
            KeyCode::Esc => self.key == "escape",
            KeyCode::Tab => self.key == "tab",
            KeyCode::Backspace => self.key == "backspace",
            KeyCode::Delete => self.key == "delete",
            KeyCode::Insert => self.key == "insert",
            KeyCode::Home => self.key == "home",
            KeyCode::End => self.key == "end",
            KeyCode::PageUp => self.key == "pageup",
            KeyCode::PageDown => self.key == "pagedown",
            KeyCode::Up => self.key == "up",
            KeyCode::Down => self.key == "down",
            KeyCode::Left => self.key == "left",
            KeyCode::Right => self.key == "right",
            KeyCode::F(n) => self.key == format!("f{n}"),
            _ => false,
        }
    }

    /// Convert to display string.
    pub fn to_display(&self) -> String {
        let mut parts = Vec::new();

        if self.leader {
            parts.push("<leader>".to_string());
        }
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }

        // Capitalize key name for display
        let key_display = match self.key.as_str() {
            "escape" => "Esc",
            "enter" => "Enter",
            "tab" => "Tab",
            "backspace" => "Backspace",
            "delete" => "Del",
            "insert" => "Ins",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PgUp",
            "pagedown" => "PgDn",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            k => k,
        };

        if self.leader {
            format!("{} {}", parts.join("+"), key_display.to_uppercase())
        } else {
            parts.push(key_display.to_uppercase());
            parts.join("+")
        }
    }
}

/// Keybind configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    /// The leader key binding.
    pub leader: String,
    /// Action to keybind mappings.
    #[serde(flatten)]
    pub bindings: HashMap<String, String>,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        for action in KeyAction::all() {
            let key = format!("{action:?}").to_lowercase();
            bindings.insert(key, action.default_binding().to_string());
        }

        Self {
            leader: "ctrl+x".to_string(),
            bindings,
        }
    }
}

/// Keybind manager that handles matching and lookup.
#[derive(Debug, Clone)]
pub struct KeybindManager {
    /// Leader key binding.
    leader: Vec<Keybind>,
    /// Action to keybinds mapping.
    actions: HashMap<KeyAction, Vec<Keybind>>,
    /// Whether leader is currently active.
    leader_active: bool,
}

impl Default for KeybindManager {
    fn default() -> Self {
        Self::new(&KeybindConfig::default())
    }
}

impl KeybindManager {
    /// Create a new keybind manager from configuration.
    pub fn new(config: &KeybindConfig) -> Self {
        let leader = Keybind::parse_multi(&config.leader);

        let mut actions = HashMap::new();
        for action in KeyAction::all() {
            let key = format!("{action:?}").to_lowercase();
            let binding = config
                .bindings
                .get(&key)
                .map(|s| s.as_str())
                .unwrap_or(action.default_binding());
            actions.insert(*action, Keybind::parse_multi(binding));
        }

        Self {
            leader,
            actions,
            leader_active: false,
        }
    }

    /// Check if the leader key was pressed.
    pub fn is_leader(&self, key: &KeyEvent) -> bool {
        self.leader.iter().any(|kb| kb.matches(key, false))
    }

    /// Set leader state.
    pub fn set_leader_active(&mut self, active: bool) {
        self.leader_active = active;
    }

    /// Check if leader is active.
    pub fn leader_active(&self) -> bool {
        self.leader_active
    }

    /// Find the action for a key event.
    pub fn find_action(&self, key: &KeyEvent) -> Option<KeyAction> {
        for (action, keybinds) in &self.actions {
            for kb in keybinds {
                if kb.matches(key, self.leader_active) {
                    return Some(*action);
                }
            }
        }
        None
    }

    /// Get the keybinds for an action.
    pub fn get_bindings(&self, action: KeyAction) -> &[Keybind] {
        self.actions
            .get(&action)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the display string for an action's primary binding.
    pub fn get_display(&self, action: KeyAction) -> String {
        self.get_bindings(action)
            .first()
            .map(|kb| kb.to_display())
            .unwrap_or_default()
    }

    /// Reset leader state (call after handling leader sequence or timeout).
    pub fn reset_leader(&mut self) {
        self.leader_active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let kb = Keybind::parse("ctrl+c").unwrap();
        assert_eq!(kb.key, "c");
        assert!(kb.ctrl);
        assert!(!kb.alt);
        assert!(!kb.shift);
        assert!(!kb.leader);
    }

    #[test]
    fn test_parse_leader() {
        let kb = Keybind::parse("<leader>n").unwrap();
        assert_eq!(kb.key, "n");
        assert!(kb.leader);
        assert!(!kb.ctrl);
    }

    #[test]
    fn test_parse_multi() {
        let kbs = Keybind::parse_multi("ctrl+c,ctrl+d");
        assert_eq!(kbs.len(), 2);
        assert_eq!(kbs[0].key, "c");
        assert_eq!(kbs[1].key, "d");
    }

    #[test]
    fn test_parse_none() {
        let kb = Keybind::parse("none");
        assert!(kb.is_none());
    }

    #[test]
    fn test_matches() {
        let kb = Keybind::new("c").ctrl();
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(kb.matches(&event, false));
    }

    #[test]
    fn test_matches_leader() {
        let kb = Keybind::new("n").leader();
        let event = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        assert!(!kb.matches(&event, false)); // Leader not active
        assert!(kb.matches(&event, true)); // Leader active
    }

    #[test]
    fn test_manager_find_action() {
        let config = KeybindConfig::default();
        let mut manager = KeybindManager::new(&config);

        // Test ctrl+p -> CommandPalette
        let event = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(manager.find_action(&event), Some(KeyAction::CommandPalette));

        // Test leader sequence for new session
        let leader_event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert!(manager.is_leader(&leader_event));
        manager.set_leader_active(true);

        let n_event = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(manager.find_action(&n_event), Some(KeyAction::SessionNew));
    }

    #[test]
    fn test_display() {
        let kb = Keybind::new("c").ctrl();
        assert_eq!(kb.to_display(), "Ctrl+C");

        let kb2 = Keybind::new("n").leader();
        assert_eq!(kb2.to_display(), "<leader> N");
    }
}
