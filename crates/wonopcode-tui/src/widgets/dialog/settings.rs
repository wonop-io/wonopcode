//! Settings dialog for configuration management.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theme::{RenderSettings, Theme};

/// Helper function to create a centered rectangle.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

/// Settings tab categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Model,
    Permissions,
    Sandbox,
    Tools,
    Performance,
    Advanced,
}

impl SettingsTab {
    /// Get all tabs in order.
    pub fn all() -> &'static [SettingsTab] {
        &[
            SettingsTab::General,
            SettingsTab::Model,
            SettingsTab::Permissions,
            SettingsTab::Sandbox,
            SettingsTab::Tools,
            SettingsTab::Performance,
            SettingsTab::Advanced,
        ]
    }

    /// Get the display name for this tab.
    pub fn name(&self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Model => "Model",
            SettingsTab::Permissions => "Permissions",
            SettingsTab::Sandbox => "Sandbox",
            SettingsTab::Tools => "Tools",
            SettingsTab::Performance => "Performance",
            SettingsTab::Advanced => "Advanced",
        }
    }

    /// Get the next tab.
    pub fn next(&self) -> Self {
        match self {
            SettingsTab::General => SettingsTab::Model,
            SettingsTab::Model => SettingsTab::Permissions,
            SettingsTab::Permissions => SettingsTab::Sandbox,
            SettingsTab::Sandbox => SettingsTab::Tools,
            SettingsTab::Tools => SettingsTab::Performance,
            SettingsTab::Performance => SettingsTab::Advanced,
            SettingsTab::Advanced => SettingsTab::General,
        }
    }

    /// Get the previous tab.
    pub fn prev(&self) -> Self {
        match self {
            SettingsTab::General => SettingsTab::Advanced,
            SettingsTab::Model => SettingsTab::General,
            SettingsTab::Permissions => SettingsTab::Model,
            SettingsTab::Sandbox => SettingsTab::Permissions,
            SettingsTab::Tools => SettingsTab::Sandbox,
            SettingsTab::Performance => SettingsTab::Tools,
            SettingsTab::Advanced => SettingsTab::Performance,
        }
    }
}

/// Setting value types.
#[derive(Debug, Clone)]
pub enum SettingValue {
    /// Boolean toggle.
    Bool(bool),
    /// String input.
    String(String),
    /// Selection from options.
    Select { value: String, options: Vec<String> },
    /// Integer number.
    Number {
        value: i64,
        min: Option<i64>,
        max: Option<i64>,
    },
    /// Floating point number.
    Float {
        value: f64,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// List of strings.
    List(Vec<String>),
    /// Keybind string.
    KeyBind(String),
}

impl SettingValue {
    /// Get a display string for the value.
    pub fn display(&self) -> String {
        match self {
            SettingValue::Bool(b) => {
                if *b {
                    "✓ enabled".to_string()
                } else {
                    "○ disabled".to_string()
                }
            }
            SettingValue::String(s) => {
                if s.is_empty() {
                    "(not set)".to_string()
                } else {
                    s.clone()
                }
            }
            SettingValue::Select { value, .. } => {
                if value.is_empty() {
                    "(not set)".to_string()
                } else {
                    value.clone()
                }
            }
            SettingValue::Number { value, .. } => value.to_string(),
            SettingValue::Float { value, .. } => format!("{value:.2}"),
            SettingValue::List(items) => {
                if items.is_empty() {
                    "(empty)".to_string()
                } else {
                    format!("{} items", items.len())
                }
            }
            SettingValue::KeyBind(kb) => {
                if kb.is_empty() {
                    "(not set)".to_string()
                } else {
                    kb.clone()
                }
            }
        }
    }

    /// Check if this is a boolean value.
    pub fn is_bool(&self) -> bool {
        matches!(self, SettingValue::Bool(_))
    }

    /// Toggle a boolean value.
    pub fn toggle(&mut self) {
        if let SettingValue::Bool(b) = self {
            *b = !*b;
        }
    }

    /// Cycle through select options.
    pub fn cycle_next(&mut self) {
        if let SettingValue::Select { value, options } = self {
            if let Some(idx) = options.iter().position(|o| o == value) {
                let next_idx = (idx + 1) % options.len();
                *value = options[next_idx].clone();
            } else if !options.is_empty() {
                *value = options[0].clone();
            }
        }
    }

    /// Cycle through select options backwards.
    pub fn cycle_prev(&mut self) {
        if let SettingValue::Select { value, options } = self {
            if let Some(idx) = options.iter().position(|o| o == value) {
                let prev_idx = if idx == 0 { options.len() - 1 } else { idx - 1 };
                *value = options[prev_idx].clone();
            } else if !options.is_empty() {
                *value = options[options.len() - 1].clone();
            }
        }
    }
}

/// A setting item that can be edited.
#[derive(Debug, Clone)]
pub struct SettingItem {
    /// Configuration key (e.g., "theme", "sandbox.enabled").
    pub key: String,
    /// Display label.
    pub label: String,
    /// Description/help text.
    pub description: String,
    /// Current value.
    pub value: SettingValue,
    /// Original value (for dirty checking).
    pub original: SettingValue,
    /// Whether this setting has been modified.
    pub dirty: bool,
    /// Whether this setting is disabled (greyed out, not editable).
    pub disabled: bool,
}

impl SettingItem {
    /// Create a new setting item.
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        value: SettingValue,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            original: value.clone(),
            value,
            dirty: false,
            disabled: false,
        }
    }

    /// Mark as dirty if value changed.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Reset to original value.
    pub fn reset(&mut self) {
        self.value = self.original.clone();
        self.dirty = false;
    }
}

/// Save scope for settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveScope {
    /// Save to project config (wonopcode.json in current directory).
    Project,
    /// Save to global config (~/.config/wonopcode/config.json).
    Global,
}

/// Result from settings dialog.
#[derive(Debug, Clone)]
pub enum SettingsResult {
    /// Save changes.
    Save(SaveScope),
    /// Cancel and discard changes.
    Cancel,
    /// No action (dialog still open).
    None,
}

/// Internal action for starting an edit (to avoid borrow checker issues).
enum EditAction {
    Toggle,
    StartSelect(usize),
    StartString(String, bool), // (value, is_keybind)
    StartList,
}

/// Settings dialog for editing configuration.
#[derive(Debug, Clone)]
pub struct SettingsDialog {
    /// Current tab.
    tab: SettingsTab,
    /// Settings items organized by tab.
    items: std::collections::HashMap<SettingsTab, Vec<SettingItem>>,
    /// Selected item index within current tab.
    selected: usize,
    /// Whether in edit mode for current item.
    editing: bool,
    /// Edit buffer for string/keybind values.
    edit_buffer: String,
    /// Cursor position in edit buffer.
    edit_cursor: usize,
    /// Select dropdown index (for Select values).
    select_index: usize,
    /// List state for rendering.
    list_state: ListState,
    /// Whether any changes were made.
    has_changes: bool,
    /// Capture mode for keybinds.
    capturing_keybind: bool,
}

impl Default for SettingsDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsDialog {
    /// Create a new settings dialog with default settings.
    pub fn new() -> Self {
        let mut items = std::collections::HashMap::new();

        // General tab
        items.insert(
            SettingsTab::General,
            vec![
                SettingItem::new(
                    "theme",
                    "Theme",
                    "Color theme for the interface",
                    SettingValue::Select {
                        value: "troelsim".to_string(),
                        options: vec![
                            "troelsim".to_string(),
                            "wonopcode".to_string(),
                            "light".to_string(),
                            "catppuccin".to_string(),
                            "dracula".to_string(),
                            "gruvbox".to_string(),
                            "nord".to_string(),
                            "tokyo-night".to_string(),
                            "rosepine".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "log_level",
                    "Log Level",
                    "Logging verbosity level",
                    SettingValue::Select {
                        value: "info".to_string(),
                        options: vec![
                            "debug".to_string(),
                            "info".to_string(),
                            "warn".to_string(),
                            "error".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "username",
                    "Username",
                    "Display name for the user",
                    SettingValue::String(String::new()),
                ),
                SettingItem::new(
                    "update.auto",
                    "Auto Update",
                    "Update behavior on startup",
                    SettingValue::Select {
                        value: "notify".to_string(),
                        options: vec![
                            "auto".to_string(),
                            "notify".to_string(),
                            "disabled".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "update.channel",
                    "Update Channel",
                    "Release channel for updates",
                    SettingValue::Select {
                        value: "stable".to_string(),
                        options: vec![
                            "stable".to_string(),
                            "beta".to_string(),
                            "nightly".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "snapshot",
                    "Snapshots",
                    "Enable file snapshot tracking for undo",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "share",
                    "Share Mode",
                    "Session sharing behavior",
                    SettingValue::Select {
                        value: "manual".to_string(),
                        options: vec![
                            "manual".to_string(),
                            "auto".to_string(),
                            "disabled".to_string(),
                        ],
                    },
                ),
            ],
        );

        // Model tab
        items.insert(
            SettingsTab::Model,
            vec![
                SettingItem::new(
                    "model",
                    "Primary Model",
                    "Default model for conversations (provider/model)",
                    SettingValue::String("anthropic/claude-sonnet-4-5-20250929".to_string()),
                ),
                SettingItem::new(
                    "small_model",
                    "Small Model",
                    "Fast model for quick tasks",
                    SettingValue::String("anthropic/claude-3-haiku-20240307".to_string()),
                ),
                SettingItem::new(
                    "default_agent",
                    "Default Agent",
                    "Agent to use by default",
                    SettingValue::Select {
                        value: "build".to_string(),
                        options: vec![
                            "build".to_string(),
                            "plan".to_string(),
                            "explore".to_string(),
                        ],
                    },
                ),
            ],
        );

        // Permissions tab
        items.insert(
            SettingsTab::Permissions,
            vec![
                SettingItem::new(
                    "permission.edit",
                    "File Edit",
                    "Permission for editing files",
                    SettingValue::Select {
                        value: "ask".to_string(),
                        options: vec!["ask".to_string(), "allow".to_string(), "deny".to_string()],
                    },
                ),
                SettingItem::new(
                    "permission.bash",
                    "Bash Commands",
                    "Permission for running shell commands",
                    SettingValue::Select {
                        value: "ask".to_string(),
                        options: vec!["ask".to_string(), "allow".to_string(), "deny".to_string()],
                    },
                ),
                SettingItem::new(
                    "permission.webfetch",
                    "Web Fetch",
                    "Permission for fetching web content",
                    SettingValue::Select {
                        value: "ask".to_string(),
                        options: vec!["ask".to_string(), "allow".to_string(), "deny".to_string()],
                    },
                ),
                SettingItem::new(
                    "permission.external_directory",
                    "External Directory",
                    "Permission for accessing files outside project",
                    SettingValue::Select {
                        value: "ask".to_string(),
                        options: vec!["ask".to_string(), "allow".to_string(), "deny".to_string()],
                    },
                ),
            ],
        );

        // Sandbox tab
        items.insert(
            SettingsTab::Sandbox,
            vec![
                SettingItem::new(
                    "sandbox.enabled",
                    "Enable Sandbox",
                    "Run tools in isolated container",
                    SettingValue::Bool(false),
                ),
                SettingItem::new(
                    "sandbox.runtime",
                    "Runtime",
                    "Container runtime to use",
                    SettingValue::Select {
                        value: "auto".to_string(),
                        options: vec![
                            "auto".to_string(),
                            "docker".to_string(),
                            "podman".to_string(),
                            "lima".to_string(),
                            "none".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "sandbox.network",
                    "Network",
                    "Network access policy for sandbox",
                    SettingValue::Select {
                        value: "limited".to_string(),
                        options: vec![
                            "limited".to_string(),
                            "full".to_string(),
                            "none".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "sandbox.image",
                    "Container Image",
                    "Docker/OCI image for sandbox",
                    SettingValue::String(String::new()),
                ),
                SettingItem::new(
                    "sandbox.keep_alive",
                    "Keep Alive",
                    "Keep sandbox running between commands",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "sandbox.resources.memory",
                    "Memory Limit",
                    "Memory limit (e.g., 2G, 512M)",
                    SettingValue::String("2G".to_string()),
                ),
                SettingItem::new(
                    "sandbox.resources.cpus",
                    "CPU Limit",
                    "Number of CPUs (e.g., 2.0)",
                    SettingValue::Float {
                        value: 2.0,
                        min: Some(0.5),
                        max: Some(16.0),
                    },
                ),
                SettingItem::new(
                    "sandbox.mounts.workspace_writable",
                    "Writable Workspace",
                    "Allow writing to workspace in sandbox",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "sandbox.mounts.persist_caches",
                    "Persist Caches",
                    "Persist package caches across sessions",
                    SettingValue::Bool(true),
                ),
            ],
        );

        // Tools tab
        items.insert(
            SettingsTab::Tools,
            vec![
                SettingItem::new(
                    "tools.bash",
                    "Bash",
                    "Enable bash/shell tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.edit",
                    "Edit",
                    "Enable file editing tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.read",
                    "Read",
                    "Enable file reading tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.write",
                    "Write",
                    "Enable file writing tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.glob",
                    "Glob",
                    "Enable glob/file search tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.grep",
                    "Grep",
                    "Enable grep/content search tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.list",
                    "List",
                    "Enable directory listing tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.patch",
                    "Patch",
                    "Enable patch/diff tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.webfetch",
                    "Web Fetch",
                    "Enable web fetching tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.websearch",
                    "Web Search",
                    "Enable web search tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.task",
                    "Task/Subagent",
                    "Enable task/subagent tool",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tools.lsp",
                    "LSP",
                    "Enable LSP code intelligence tool",
                    SettingValue::Bool(true),
                ),
            ],
        );

        // Performance tab - rendering feature toggles
        items.insert(
            SettingsTab::Performance,
            vec![
                SettingItem::new(
                    "perf.markdown",
                    "Markdown Rendering",
                    "Render markdown formatting (bold, italic, lists, etc.)",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "perf.syntax_highlighting",
                    "Syntax Highlighting",
                    "Enable syntax highlighting for code blocks",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "perf.code_backgrounds",
                    "Code Block Backgrounds",
                    "Show background color for code blocks",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "perf.tables",
                    "Table Rendering",
                    "Render markdown tables with borders",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "perf.streaming_fps",
                    "Streaming FPS",
                    "Max frames per second during streaming (lower = less CPU)",
                    SettingValue::Select {
                        value: "20".to_string(),
                        options: vec![
                            "5".to_string(),
                            "10".to_string(),
                            "15".to_string(),
                            "20".to_string(),
                            "30".to_string(),
                            "60".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "perf.max_messages",
                    "Max Messages",
                    "Maximum messages to keep in memory",
                    SettingValue::Select {
                        value: "200".to_string(),
                        options: vec![
                            "25".to_string(),
                            "50".to_string(),
                            "100".to_string(),
                            "200".to_string(),
                            "500".to_string(),
                        ],
                    },
                ),
                SettingItem::new(
                    "perf.low_memory_mode",
                    "Low Memory Mode",
                    "Aggressive memory optimization (disables some features)",
                    SettingValue::Bool(false),
                ),
                SettingItem::new(
                    "perf.enable_test_commands",
                    "Enable Test Commands",
                    "Enable debug/test commands like /add_test_messages",
                    SettingValue::Bool(false),
                ),
                // Test Provider Settings (subsection)
                SettingItem::new(
                    "test.model_enabled",
                    "Enable Test Model",
                    "Show test/test-128b in model selector",
                    SettingValue::Bool(false),
                ),
                SettingItem::new(
                    "test.emulate_thinking",
                    "Emulate Thinking",
                    "Simulate reasoning/thinking blocks",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "test.emulate_tool_calls",
                    "Emulate Tool Calls",
                    "Simulate standard tool execution",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "test.emulate_tool_observed",
                    "Emulate Tool Observed",
                    "Simulate CLI-style external tool execution",
                    SettingValue::Bool(false),
                ),
                SettingItem::new(
                    "test.emulate_streaming",
                    "Emulate Streaming Delays",
                    "Add realistic delays between chunks",
                    SettingValue::Bool(true),
                ),
            ],
        );

        // Advanced tab
        items.insert(
            SettingsTab::Advanced,
            vec![
                SettingItem::new(
                    "tui.mouse",
                    "Mouse Support",
                    "Enable mouse interactions in TUI",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "tui.paste",
                    "Paste Mode",
                    "How to handle pasted text",
                    SettingValue::Select {
                        value: "bracketed".to_string(),
                        options: vec!["bracketed".to_string(), "direct".to_string()],
                    },
                ),
                SettingItem::new(
                    "compaction.auto",
                    "Auto Compaction",
                    "Automatically compact long conversations",
                    SettingValue::Bool(true),
                ),
                SettingItem::new(
                    "compaction.prune",
                    "Prune Messages",
                    "Remove old messages during compaction",
                    SettingValue::Bool(false),
                ),
                SettingItem::new(
                    "server.disabled",
                    "Disable Server",
                    "Disable the HTTP API server",
                    SettingValue::Bool(false),
                ),
                SettingItem::new(
                    "server.port",
                    "Server Port",
                    "Port for the HTTP API server",
                    SettingValue::Number {
                        value: 8080,
                        min: Some(1024),
                        max: Some(65535),
                    },
                ),
            ],
        );

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            tab: SettingsTab::General,
            items,
            selected: 0,
            editing: false,
            edit_buffer: String::new(),
            edit_cursor: 0,
            select_index: 0,
            list_state,
            has_changes: false,
            capturing_keybind: false,
        }
    }

    /// Create a new settings dialog with the given render settings and theme applied.
    /// This is used when opening settings to show the current runtime values.
    pub fn with_render_settings(
        render_settings: &crate::theme::RenderSettings,
        theme_name: &str,
    ) -> Self {
        let mut dialog = Self::new();

        // Helper to update a setting item
        fn update_item(item: &mut SettingItem, new_value: SettingValue) {
            item.value = new_value.clone();
            item.original = new_value;
        }

        // Update General tab with current theme
        if let Some(items) = dialog.items.get_mut(&SettingsTab::General) {
            for item in items.iter_mut() {
                if item.key == "theme" {
                    if let SettingValue::Select { options, .. } = &item.value {
                        update_item(
                            item,
                            SettingValue::Select {
                                value: theme_name.to_string(),
                                options: options.clone(),
                            },
                        );
                    }
                }
            }
        }

        // Update Performance tab from render settings
        if let Some(items) = dialog.items.get_mut(&SettingsTab::Performance) {
            for item in items.iter_mut() {
                match item.key.as_str() {
                    "perf.markdown" => {
                        update_item(item, SettingValue::Bool(render_settings.markdown_enabled));
                    }
                    "perf.syntax_highlighting" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.syntax_highlighting_enabled),
                        );
                    }
                    "perf.code_backgrounds" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.code_backgrounds_enabled),
                        );
                    }
                    "perf.tables" => {
                        update_item(item, SettingValue::Bool(render_settings.tables_enabled));
                    }
                    "perf.streaming_fps" => {
                        if let SettingValue::Select { options, .. } = &item.value {
                            update_item(
                                item,
                                SettingValue::Select {
                                    value: render_settings.streaming_fps.to_string(),
                                    options: options.clone(),
                                },
                            );
                        }
                    }
                    "perf.max_messages" => {
                        if let SettingValue::Select { options, .. } = &item.value {
                            update_item(
                                item,
                                SettingValue::Select {
                                    value: render_settings.max_messages.to_string(),
                                    options: options.clone(),
                                },
                            );
                        }
                    }
                    "perf.low_memory_mode" => {
                        update_item(item, SettingValue::Bool(render_settings.low_memory_mode));
                    }
                    "perf.enable_test_commands" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.enable_test_commands),
                        );
                    }
                    // Test provider settings
                    "test.model_enabled" => {
                        update_item(item, SettingValue::Bool(render_settings.test_model_enabled));
                    }
                    "test.emulate_thinking" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.test_emulate_thinking),
                        );
                    }
                    "test.emulate_tool_calls" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.test_emulate_tool_calls),
                        );
                    }
                    "test.emulate_tool_observed" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.test_emulate_tool_observed),
                        );
                    }
                    "test.emulate_streaming" => {
                        update_item(
                            item,
                            SettingValue::Bool(render_settings.test_emulate_streaming),
                        );
                    }
                    _ => {}
                }
            }
        }

        // Update disabled state based on low_memory_mode
        dialog.update_low_memory_disabled_state();

        dialog
    }

    /// Load settings from a config.
    #[allow(clippy::cognitive_complexity)]
    pub fn from_config(config: &wonopcode_core::config::Config) -> Self {
        let mut dialog = Self::new();

        // Helper to update a setting item
        fn update_item(item: &mut SettingItem, new_value: SettingValue) {
            item.value = new_value.clone();
            item.original = new_value;
        }

        // Update General tab from config
        if let Some(items) = dialog.items.get_mut(&SettingsTab::General) {
            for item in items.iter_mut() {
                match item.key.as_str() {
                    "theme" => {
                        if let Some(theme) = &config.theme {
                            if let SettingValue::Select { options, .. } = &item.value {
                                update_item(
                                    item,
                                    SettingValue::Select {
                                        value: theme.clone(),
                                        options: options.clone(),
                                    },
                                );
                            }
                        }
                    }
                    "log_level" => {
                        if let Some(level) = &config.log_level {
                            if let SettingValue::Select { options, .. } = &item.value {
                                update_item(
                                    item,
                                    SettingValue::Select {
                                        value: format!("{level:?}").to_lowercase(),
                                        options: options.clone(),
                                    },
                                );
                            }
                        }
                    }
                    "username" => {
                        if let Some(username) = &config.username {
                            update_item(item, SettingValue::String(username.clone()));
                        }
                    }
                    "snapshot" => {
                        if let Some(snap) = config.snapshot {
                            update_item(item, SettingValue::Bool(snap));
                        }
                    }
                    "share" => {
                        if let Some(share) = &config.share {
                            if let SettingValue::Select { options, .. } = &item.value {
                                update_item(
                                    item,
                                    SettingValue::Select {
                                        value: format!("{share:?}").to_lowercase(),
                                        options: options.clone(),
                                    },
                                );
                            }
                        }
                    }
                    "update.auto" => {
                        if let Some(ref update) = config.update {
                            if let Some(mode) = update.auto {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    let value = match mode {
                                        wonopcode_core::config::AutoUpdateMode::Auto => "auto",
                                        wonopcode_core::config::AutoUpdateMode::Notify => "notify",
                                        wonopcode_core::config::AutoUpdateMode::Disabled => {
                                            "disabled"
                                        }
                                    };
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: value.to_string(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        } else if let Some(autoupdate) = &config.autoupdate {
                            // Legacy fallback
                            if let SettingValue::Select { options, .. } = &item.value {
                                let value = match autoupdate {
                                    wonopcode_core::config::AutoUpdate::Bool(true) => "auto",
                                    wonopcode_core::config::AutoUpdate::Bool(false) => "disabled",
                                    wonopcode_core::config::AutoUpdate::Notify => "notify",
                                };
                                update_item(
                                    item,
                                    SettingValue::Select {
                                        value: value.to_string(),
                                        options: options.clone(),
                                    },
                                );
                            }
                        }
                    }
                    "update.channel" => {
                        if let Some(ref update) = config.update {
                            if let Some(channel) = update.channel {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    let value = match channel {
                                        wonopcode_core::version::ReleaseChannel::Stable => "stable",
                                        wonopcode_core::version::ReleaseChannel::Beta => "beta",
                                        wonopcode_core::version::ReleaseChannel::Nightly => {
                                            "nightly"
                                        }
                                    };
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: value.to_string(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Update Model tab from config
        if let Some(items) = dialog.items.get_mut(&SettingsTab::Model) {
            for item in items.iter_mut() {
                match item.key.as_str() {
                    "model" => {
                        if let Some(model) = &config.model {
                            update_item(item, SettingValue::String(model.clone()));
                        }
                    }
                    "small_model" => {
                        if let Some(model) = &config.small_model {
                            update_item(item, SettingValue::String(model.clone()));
                        }
                    }
                    "default_agent" => {
                        if let Some(agent) = &config.default_agent {
                            if let SettingValue::Select { options, .. } = &item.value {
                                update_item(
                                    item,
                                    SettingValue::Select {
                                        value: agent.clone(),
                                        options: options.clone(),
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Update Permissions tab from config
        if let Some(perm_config) = &config.permission {
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Permissions) {
                for item in items.iter_mut() {
                    let perm_value = match item.key.as_str() {
                        "permission.edit" => perm_config.edit.as_ref(),
                        "permission.webfetch" => perm_config.webfetch.as_ref(),
                        "permission.external_directory" => perm_config.external_directory.as_ref(),
                        _ => None,
                    };
                    if let Some(perm) = perm_value {
                        if let SettingValue::Select { options, .. } = &item.value {
                            update_item(
                                item,
                                SettingValue::Select {
                                    value: format!("{perm:?}").to_lowercase(),
                                    options: options.clone(),
                                },
                            );
                        }
                    }
                }
            }
        }

        // Update Sandbox tab from config
        if let Some(sandbox_config) = &config.sandbox {
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Sandbox) {
                for item in items.iter_mut() {
                    match item.key.as_str() {
                        "sandbox.enabled" => {
                            if let Some(enabled) = sandbox_config.enabled {
                                update_item(item, SettingValue::Bool(enabled));
                            }
                        }
                        "sandbox.runtime" => {
                            if let Some(runtime) = &sandbox_config.runtime {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: runtime.clone(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        }
                        "sandbox.network" => {
                            if let Some(network) = &sandbox_config.network {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: network.clone(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        }
                        "sandbox.image" => {
                            if let Some(image) = &sandbox_config.image {
                                update_item(item, SettingValue::String(image.clone()));
                            }
                        }
                        "sandbox.keep_alive" => {
                            if let Some(keep) = sandbox_config.keep_alive {
                                update_item(item, SettingValue::Bool(keep));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Update Tools tab from config
        if let Some(tools_config) = &config.tools {
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Tools) {
                for item in items.iter_mut() {
                    if let Some(tool_name) = item.key.strip_prefix("tools.") {
                        if let Some(&enabled) = tools_config.get(tool_name) {
                            update_item(item, SettingValue::Bool(enabled));
                        }
                    }
                }
            }
        }

        // Update TUI settings from config
        if let Some(tui_config) = &config.tui {
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Advanced) {
                for item in items.iter_mut() {
                    match item.key.as_str() {
                        "tui.mouse" => {
                            if let Some(mouse) = tui_config.mouse {
                                update_item(item, SettingValue::Bool(mouse));
                            }
                        }
                        "tui.paste" => {
                            if let Some(paste) = &tui_config.paste {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: format!("{paste:?}").to_lowercase(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Update Performance tab settings from tui config
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Performance) {
                for item in items.iter_mut() {
                    match item.key.as_str() {
                        "perf.markdown" => {
                            if let Some(v) = tui_config.markdown {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "perf.syntax_highlighting" => {
                            if let Some(v) = tui_config.syntax_highlighting {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "perf.code_backgrounds" => {
                            if let Some(v) = tui_config.code_backgrounds {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "perf.tables" => {
                            if let Some(v) = tui_config.tables {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "perf.streaming_fps" => {
                            if let Some(fps) = tui_config.streaming_fps {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: fps.to_string(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        }
                        "perf.max_messages" => {
                            if let Some(max) = tui_config.max_messages {
                                if let SettingValue::Select { options, .. } = &item.value {
                                    update_item(
                                        item,
                                        SettingValue::Select {
                                            value: max.to_string(),
                                            options: options.clone(),
                                        },
                                    );
                                }
                            }
                        }
                        "perf.low_memory_mode" => {
                            if let Some(v) = tui_config.low_memory_mode {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "perf.enable_test_commands" => {
                            if let Some(v) = tui_config.enable_test_commands {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        // Test provider settings
                        "test.model_enabled" => {
                            if let Some(v) = tui_config.test_model_enabled {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "test.emulate_thinking" => {
                            if let Some(v) = tui_config.test_emulate_thinking {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "test.emulate_tool_calls" => {
                            if let Some(v) = tui_config.test_emulate_tool_calls {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "test.emulate_tool_observed" => {
                            if let Some(v) = tui_config.test_emulate_tool_observed {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        "test.emulate_streaming" => {
                            if let Some(v) = tui_config.test_emulate_streaming {
                                update_item(item, SettingValue::Bool(v));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Update compaction settings from config
        if let Some(compaction_config) = &config.compaction {
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Advanced) {
                for item in items.iter_mut() {
                    match item.key.as_str() {
                        "compaction.auto" => {
                            if let Some(auto) = compaction_config.auto {
                                update_item(item, SettingValue::Bool(auto));
                            }
                        }
                        "compaction.prune" => {
                            if let Some(prune) = compaction_config.prune {
                                update_item(item, SettingValue::Bool(prune));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Update server settings from config
        if let Some(server_config) = &config.server {
            if let Some(items) = dialog.items.get_mut(&SettingsTab::Advanced) {
                for item in items.iter_mut() {
                    match item.key.as_str() {
                        "server.disabled" => {
                            if let Some(disabled) = server_config.disabled {
                                update_item(item, SettingValue::Bool(disabled));
                            }
                        }
                        "server.port" => {
                            if let Some(port) = server_config.port {
                                if let SettingValue::Number { min, max, .. } = &item.value {
                                    update_item(
                                        item,
                                        SettingValue::Number {
                                            value: port as i64,
                                            min: *min,
                                            max: *max,
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Update disabled state based on low_memory_mode
        dialog.update_low_memory_disabled_state();

        dialog
    }

    /// Convert current settings to a Config struct.
    #[allow(clippy::cognitive_complexity)]
    pub fn to_config(&self) -> wonopcode_core::config::Config {
        use wonopcode_core::config::*;

        let mut config = Config::default();

        // Only include dirty items
        for (tab, items) in &self.items {
            for item in items {
                if !item.dirty {
                    continue;
                }

                match (tab, item.key.as_str()) {
                    // General settings
                    (SettingsTab::General, "theme") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            config.theme = Some(value.clone());
                        }
                    }
                    (SettingsTab::General, "log_level") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            config.log_level = match value.as_str() {
                                "debug" => Some(LogLevel::Debug),
                                "info" => Some(LogLevel::Info),
                                "warn" => Some(LogLevel::Warn),
                                "error" => Some(LogLevel::Error),
                                _ => None,
                            };
                        }
                    }
                    (SettingsTab::General, "username") => {
                        if let SettingValue::String(s) = &item.value {
                            if !s.is_empty() {
                                config.username = Some(s.clone());
                            }
                        }
                    }
                    (SettingsTab::General, "snapshot") => {
                        if let SettingValue::Bool(b) = &item.value {
                            config.snapshot = Some(*b);
                        }
                    }
                    (SettingsTab::General, "share") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            config.share = match value.as_str() {
                                "manual" => Some(ShareMode::Manual),
                                "auto" => Some(ShareMode::Auto),
                                "disabled" => Some(ShareMode::Disabled),
                                _ => None,
                            };
                        }
                    }
                    (SettingsTab::General, "update.auto") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            let update_config = config.update.get_or_insert_with(Default::default);
                            update_config.auto = match value.as_str() {
                                "auto" => Some(AutoUpdateMode::Auto),
                                "notify" => Some(AutoUpdateMode::Notify),
                                "disabled" => Some(AutoUpdateMode::Disabled),
                                _ => None,
                            };
                        }
                    }
                    (SettingsTab::General, "update.channel") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            let update_config = config.update.get_or_insert_with(Default::default);
                            update_config.channel = match value.as_str() {
                                "stable" => Some(wonopcode_core::version::ReleaseChannel::Stable),
                                "beta" => Some(wonopcode_core::version::ReleaseChannel::Beta),
                                "nightly" => Some(wonopcode_core::version::ReleaseChannel::Nightly),
                                _ => None,
                            };
                        }
                    }

                    // Model settings
                    (SettingsTab::Model, "model") => {
                        if let SettingValue::String(s) = &item.value {
                            if !s.is_empty() {
                                config.model = Some(s.clone());
                            }
                        }
                    }
                    (SettingsTab::Model, "small_model") => {
                        if let SettingValue::String(s) = &item.value {
                            if !s.is_empty() {
                                config.small_model = Some(s.clone());
                            }
                        }
                    }
                    (SettingsTab::Model, "default_agent") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            config.default_agent = Some(value.clone());
                        }
                    }

                    // Permission settings
                    (SettingsTab::Permissions, key) if key.starts_with("permission.") => {
                        let perm_config = config.permission.get_or_insert_with(Default::default);
                        if let SettingValue::Select { value, .. } = &item.value {
                            let perm = match value.as_str() {
                                "ask" => Some(Permission::Ask),
                                "allow" => Some(Permission::Allow),
                                "deny" => Some(Permission::Deny),
                                _ => None,
                            };
                            match key {
                                "permission.edit" => perm_config.edit = perm,
                                "permission.webfetch" => perm_config.webfetch = perm,
                                "permission.external_directory" => {
                                    perm_config.external_directory = perm
                                }
                                _ => {}
                            }
                        }
                    }

                    // Sandbox settings
                    (SettingsTab::Sandbox, key) if key.starts_with("sandbox.") => {
                        let sandbox = config.sandbox.get_or_insert_with(Default::default);
                        match key {
                            "sandbox.enabled" => {
                                if let SettingValue::Bool(b) = &item.value {
                                    sandbox.enabled = Some(*b);
                                }
                            }
                            "sandbox.runtime" => {
                                if let SettingValue::Select { value, .. } = &item.value {
                                    sandbox.runtime = Some(value.clone());
                                }
                            }
                            "sandbox.network" => {
                                if let SettingValue::Select { value, .. } = &item.value {
                                    sandbox.network = Some(value.clone());
                                }
                            }
                            "sandbox.image" => {
                                if let SettingValue::String(s) = &item.value {
                                    if !s.is_empty() {
                                        sandbox.image = Some(s.clone());
                                    }
                                }
                            }
                            "sandbox.keep_alive" => {
                                if let SettingValue::Bool(b) = &item.value {
                                    sandbox.keep_alive = Some(*b);
                                }
                            }
                            "sandbox.resources.memory" => {
                                if let SettingValue::String(s) = &item.value {
                                    let res =
                                        sandbox.resources.get_or_insert_with(Default::default);
                                    if !s.is_empty() {
                                        res.memory = Some(s.clone());
                                    }
                                }
                            }
                            "sandbox.resources.cpus" => {
                                if let SettingValue::Float { value, .. } = &item.value {
                                    let res =
                                        sandbox.resources.get_or_insert_with(Default::default);
                                    res.cpus = Some(*value as f32);
                                }
                            }
                            "sandbox.mounts.workspace_writable" => {
                                if let SettingValue::Bool(b) = &item.value {
                                    let mounts =
                                        sandbox.mounts.get_or_insert_with(Default::default);
                                    mounts.workspace_writable = Some(*b);
                                }
                            }
                            "sandbox.mounts.persist_caches" => {
                                if let SettingValue::Bool(b) = &item.value {
                                    let mounts =
                                        sandbox.mounts.get_or_insert_with(Default::default);
                                    mounts.persist_caches = Some(*b);
                                }
                            }
                            _ => {}
                        }
                    }

                    // Tools settings
                    (SettingsTab::Tools, key) if key.starts_with("tools.") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tools = config.tools.get_or_insert_with(Default::default);
                            if let Some(tool_name) = key.strip_prefix("tools.") {
                                tools.insert(tool_name.to_string(), *b);
                            }
                        }
                    }

                    // Performance/Render settings
                    (SettingsTab::Performance, "perf.markdown") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.markdown = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "perf.syntax_highlighting") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.syntax_highlighting = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "perf.code_backgrounds") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.code_backgrounds = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "perf.tables") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.tables = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "perf.streaming_fps") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.streaming_fps = value.parse().ok();
                        }
                    }
                    (SettingsTab::Performance, "perf.max_messages") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.max_messages = value.parse().ok();
                        }
                    }
                    (SettingsTab::Performance, "perf.low_memory_mode") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.low_memory_mode = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "perf.enable_test_commands") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.enable_test_commands = Some(*b);
                        }
                    }
                    // Test provider settings
                    (SettingsTab::Performance, "test.model_enabled") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.test_model_enabled = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "test.emulate_thinking") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.test_emulate_thinking = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "test.emulate_tool_calls") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.test_emulate_tool_calls = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "test.emulate_tool_observed") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.test_emulate_tool_observed = Some(*b);
                        }
                    }
                    (SettingsTab::Performance, "test.emulate_streaming") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.test_emulate_streaming = Some(*b);
                        }
                    }

                    // Advanced/TUI settings
                    (SettingsTab::Advanced, "tui.mouse") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.mouse = Some(*b);
                        }
                    }
                    (SettingsTab::Advanced, "tui.paste") => {
                        if let SettingValue::Select { value, .. } = &item.value {
                            let tui = config.tui.get_or_insert_with(Default::default);
                            tui.paste = match value.as_str() {
                                "bracketed" => Some(PasteMode::Bracketed),
                                "direct" => Some(PasteMode::Direct),
                                _ => None,
                            };
                        }
                    }
                    (SettingsTab::Advanced, "compaction.auto") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let comp = config.compaction.get_or_insert_with(Default::default);
                            comp.auto = Some(*b);
                        }
                    }
                    (SettingsTab::Advanced, "compaction.prune") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let comp = config.compaction.get_or_insert_with(Default::default);
                            comp.prune = Some(*b);
                        }
                    }
                    (SettingsTab::Advanced, "server.disabled") => {
                        if let SettingValue::Bool(b) = &item.value {
                            let server = config.server.get_or_insert_with(Default::default);
                            server.disabled = Some(*b);
                        }
                    }
                    (SettingsTab::Advanced, "server.port") => {
                        if let SettingValue::Number { value, .. } = &item.value {
                            let server = config.server.get_or_insert_with(Default::default);
                            server.port = Some(*value as u16);
                        }
                    }

                    _ => {}
                }
            }
        }

        config
    }

    /// Check if there are unsaved changes.
    pub fn has_changes(&self) -> bool {
        self.has_changes
    }

    /// Get the currently selected item.
    fn current_item(&self) -> Option<&SettingItem> {
        self.items
            .get(&self.tab)
            .and_then(|items| items.get(self.selected))
    }

    /// Get the currently selected item mutably.
    fn current_item_mut(&mut self) -> Option<&mut SettingItem> {
        self.items
            .get_mut(&self.tab)
            .and_then(|items| items.get_mut(self.selected))
    }

    /// Get item count for current tab.
    fn item_count(&self) -> usize {
        self.items.get(&self.tab).map(|i| i.len()).unwrap_or(0)
    }

    /// Update the disabled state of performance settings based on low_memory_mode.
    fn update_low_memory_disabled_state(&mut self) {
        // First, get the low_memory_mode value
        let low_memory_enabled = self
            .items
            .get(&SettingsTab::Performance)
            .and_then(|items| {
                items
                    .iter()
                    .find(|i| i.key == "perf.low_memory_mode")
                    .and_then(|i| {
                        if let SettingValue::Bool(v) = &i.value {
                            Some(*v)
                        } else {
                            None
                        }
                    })
            })
            .unwrap_or(false);

        // Then update the disabled state of other performance items
        if let Some(items) = self.items.get_mut(&SettingsTab::Performance) {
            for item in items.iter_mut() {
                match item.key.as_str() {
                    "perf.syntax_highlighting"
                    | "perf.code_backgrounds"
                    | "perf.tables"
                    | "perf.streaming_fps"
                    | "perf.max_messages" => {
                        item.disabled = low_memory_enabled;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Start editing the current item.
    fn start_edit(&mut self) {
        // First, gather information we need from the current item
        let action = if let Some(item) = self.current_item() {
            // Don't allow editing disabled items
            if item.disabled {
                return;
            }
            match &item.value {
                SettingValue::Bool(_) => Some(EditAction::Toggle),
                SettingValue::Select { value, options } => {
                    let idx = options.iter().position(|o| o == value).unwrap_or(0);
                    Some(EditAction::StartSelect(idx))
                }
                SettingValue::String(s) => Some(EditAction::StartString(s.clone(), false)),
                SettingValue::KeyBind(s) => Some(EditAction::StartString(s.clone(), true)),
                SettingValue::Number { value, .. } => {
                    Some(EditAction::StartString(value.to_string(), false))
                }
                SettingValue::Float { value, .. } => {
                    Some(EditAction::StartString(format!("{value:.2}"), false))
                }
                SettingValue::List(_) => Some(EditAction::StartList),
            }
        } else {
            None
        };

        // Now apply the action
        if let Some(action) = action {
            match action {
                EditAction::Toggle => {
                    let is_low_memory_toggle = self
                        .current_item()
                        .map(|i| i.key == "perf.low_memory_mode")
                        .unwrap_or(false);

                    if let Some(item) = self.current_item_mut() {
                        item.value.toggle();
                        item.mark_dirty();
                        self.has_changes = true;
                    }

                    // Update disabled state if low_memory_mode was toggled
                    if is_low_memory_toggle {
                        self.update_low_memory_disabled_state();
                    }
                }
                EditAction::StartSelect(idx) => {
                    self.select_index = idx;
                    self.editing = true;
                }
                EditAction::StartString(s, is_keybind) => {
                    let len = s.len();
                    self.edit_buffer = s;
                    self.edit_cursor = len;
                    self.editing = true;
                    self.capturing_keybind = is_keybind;
                }
                EditAction::StartList => {
                    self.editing = true;
                }
            }
        }
    }

    /// Confirm the current edit.
    fn confirm_edit(&mut self) {
        // Gather values we need before borrowing mutably
        let select_index = self.select_index;
        let edit_buffer = self.edit_buffer.clone();

        if let Some(item) = self.current_item_mut() {
            match &mut item.value {
                SettingValue::Select { value, options } => {
                    if let Some(new_val) = options.get(select_index) {
                        *value = new_val.clone();
                        item.mark_dirty();
                        self.has_changes = true;
                    }
                }
                SettingValue::String(s) | SettingValue::KeyBind(s) => {
                    *s = edit_buffer;
                    item.mark_dirty();
                    self.has_changes = true;
                }
                SettingValue::Number { value, min, max } => {
                    if let Ok(n) = edit_buffer.parse::<i64>() {
                        let n = min.map(|m| n.max(m)).unwrap_or(n);
                        let n = max.map(|m| n.min(m)).unwrap_or(n);
                        *value = n;
                        item.mark_dirty();
                        self.has_changes = true;
                    }
                }
                SettingValue::Float { value, min, max } => {
                    if let Ok(f) = edit_buffer.parse::<f64>() {
                        let f = min.map(|m| f.max(m)).unwrap_or(f);
                        let f = max.map(|m| f.min(m)).unwrap_or(f);
                        *value = f;
                        item.mark_dirty();
                        self.has_changes = true;
                    }
                }
                _ => {}
            }
        }
        self.editing = false;
        self.capturing_keybind = false;
        self.edit_buffer.clear();
    }

    /// Cancel the current edit.
    fn cancel_edit(&mut self) {
        self.editing = false;
        self.capturing_keybind = false;
        self.edit_buffer.clear();
    }

    /// Handle a key event. Returns a SettingsResult.
    pub fn handle_key(&mut self, key: KeyEvent) -> SettingsResult {
        // Handle keybind capture mode
        if self.capturing_keybind {
            // Escape cancels capture
            if key.code == KeyCode::Esc {
                self.cancel_edit();
                return SettingsResult::None;
            }

            // Build keybind string from the key event
            let mut parts = Vec::new();
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                parts.push("ctrl");
            }
            if key.modifiers.contains(KeyModifiers::ALT) {
                parts.push("alt");
            }
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                parts.push("shift");
            }

            let key_name = match key.code {
                KeyCode::Char(c) => c.to_string(),
                KeyCode::Enter => "enter".to_string(),
                KeyCode::Tab => "tab".to_string(),
                KeyCode::Backspace => "backspace".to_string(),
                KeyCode::Delete => "delete".to_string(),
                KeyCode::Home => "home".to_string(),
                KeyCode::End => "end".to_string(),
                KeyCode::PageUp => "pageup".to_string(),
                KeyCode::PageDown => "pagedown".to_string(),
                KeyCode::Up => "up".to_string(),
                KeyCode::Down => "down".to_string(),
                KeyCode::Left => "left".to_string(),
                KeyCode::Right => "right".to_string(),
                KeyCode::F(n) => format!("f{n}"),
                _ => return SettingsResult::None,
            };

            parts.push(&key_name);
            self.edit_buffer = parts.join("+");
            self.confirm_edit();
            return SettingsResult::None;
        }

        // Handle edit mode for non-keybind values
        if self.editing {
            if let Some(item) = self.current_item() {
                match &item.value {
                    SettingValue::Select { options, .. } => {
                        match key.code {
                            KeyCode::Esc => self.cancel_edit(),
                            KeyCode::Enter => self.confirm_edit(),
                            KeyCode::Up | KeyCode::Char('k') => {
                                if self.select_index > 0 {
                                    self.select_index -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if self.select_index < options.len().saturating_sub(1) {
                                    self.select_index += 1;
                                }
                            }
                            _ => {}
                        }
                        return SettingsResult::None;
                    }
                    _ => {
                        // String/Number/Float editing
                        match key.code {
                            KeyCode::Esc => {
                                self.cancel_edit();
                                return SettingsResult::None;
                            }
                            KeyCode::Enter => {
                                self.confirm_edit();
                                return SettingsResult::None;
                            }
                            KeyCode::Char(c) => {
                                self.edit_buffer.insert(self.edit_cursor, c);
                                self.edit_cursor += 1;
                            }
                            KeyCode::Backspace => {
                                if self.edit_cursor > 0 {
                                    self.edit_cursor -= 1;
                                    self.edit_buffer.remove(self.edit_cursor);
                                }
                            }
                            KeyCode::Delete => {
                                if self.edit_cursor < self.edit_buffer.len() {
                                    self.edit_buffer.remove(self.edit_cursor);
                                }
                            }
                            KeyCode::Left => {
                                if self.edit_cursor > 0 {
                                    self.edit_cursor -= 1;
                                }
                            }
                            KeyCode::Right => {
                                if self.edit_cursor < self.edit_buffer.len() {
                                    self.edit_cursor += 1;
                                }
                            }
                            KeyCode::Home => {
                                self.edit_cursor = 0;
                            }
                            KeyCode::End => {
                                self.edit_cursor = self.edit_buffer.len();
                            }
                            _ => {}
                        }
                        return SettingsResult::None;
                    }
                }
            }
        }

        // Normal navigation mode
        match key.code {
            KeyCode::Esc => {
                return SettingsResult::Cancel;
            }
            KeyCode::Tab => {
                self.tab = self.tab.next();
                self.selected = 0;
                self.list_state.select(Some(0));
            }
            KeyCode::BackTab => {
                self.tab = self.tab.prev();
                self.selected = 0;
                self.list_state.select(Some(0));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let count = self.item_count();
                if self.selected < count.saturating_sub(1) {
                    self.selected += 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
                self.list_state.select(Some(0));
            }
            KeyCode::End | KeyCode::Char('G') => {
                let count = self.item_count();
                self.selected = count.saturating_sub(1);
                self.list_state.select(Some(self.selected));
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.start_edit();
            }
            KeyCode::Char('s') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    return SettingsResult::Save(SaveScope::Global);
                } else {
                    return SettingsResult::Save(SaveScope::Project);
                }
            }
            KeyCode::Char('r') => {
                // Reset current item
                if let Some(item) = self.current_item_mut() {
                    item.reset();
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                // Quick cycle forward for Select values
                if let Some(item) = self.current_item_mut() {
                    if matches!(item.value, SettingValue::Select { .. }) {
                        item.value.cycle_next();
                        item.mark_dirty();
                        self.has_changes = true;
                    }
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                // Quick cycle backward for Select values
                if let Some(item) = self.current_item_mut() {
                    if matches!(item.value, SettingValue::Select { .. }) {
                        item.value.cycle_prev();
                        item.mark_dirty();
                        self.has_changes = true;
                    }
                }
            }
            _ => {}
        }

        SettingsResult::None
    }

    /// Render the settings dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Calculate dialog size
        let dialog_width = (area.width * 80 / 100).clamp(60, 100);
        let dialog_height = (area.height * 85 / 100).clamp(20, 40);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        // Clear background
        frame.render_widget(Clear, dialog_area);

        // Main block with title
        let title = if self.has_changes {
            " Settings * "
        } else {
            " Settings "
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into tabs, content, description, and help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tabs
                Constraint::Min(8),    // Content
                Constraint::Length(3), // Description
                Constraint::Length(1), // Help
            ])
            .split(inner);

        // Render tabs
        self.render_tabs(frame, chunks[0], theme);

        // Render settings list
        self.render_items(frame, chunks[1], theme);

        // Render description
        self.render_description(frame, chunks[2], theme);

        // Render help
        self.render_help(frame, chunks[3], theme);
    }

    /// Render the tab bar.
    fn render_tabs(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let tabs: Vec<Span> = SettingsTab::all()
            .iter()
            .map(|t| {
                let style = if *t == self.tab {
                    Style::default()
                        .fg(theme.background)
                        .bg(theme.border_active)
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme.muted_style()
                };
                Span::styled(format!(" {} ", t.name()), style)
            })
            .collect();

        let mut line_spans = Vec::new();
        for (i, span) in tabs.into_iter().enumerate() {
            line_spans.push(span);
            if i < SettingsTab::all().len() - 1 {
                line_spans.push(Span::styled(" ", theme.text_style()));
            }
        }

        let tabs_line = Line::from(line_spans);
        let tabs_block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme.border_style());

        let tabs_para = Paragraph::new(tabs_line)
            .block(tabs_block)
            .alignment(Alignment::Center);

        frame.render_widget(tabs_para, area);
    }

    /// Render the settings items list.
    fn render_items(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let items = match self.items.get(&self.tab) {
            Some(items) => items,
            None => return,
        };

        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_selected = idx == self.selected;

                // Build the line - use dim style for disabled items
                let label_style = if item.disabled {
                    theme.dim_style()
                } else if item.dirty {
                    Style::default().fg(theme.warning)
                } else {
                    theme.text_style()
                };

                let value_display = item.value.display();
                let value_style = if item.disabled {
                    theme.dim_style()
                } else if is_selected && self.editing {
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme.muted_style()
                };

                // Create spans
                let mut spans = vec![Span::styled(&item.label, label_style), Span::raw("  ")];

                // Special rendering for editing mode
                if is_selected && self.editing {
                    match &item.value {
                        SettingValue::Select { options, .. } => {
                            // Show dropdown
                            let display =
                                options.get(self.select_index).cloned().unwrap_or_default();
                            spans.push(Span::styled(
                                format!("▼ {display}"),
                                Style::default()
                                    .fg(theme.primary)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        }
                        _ => {
                            // Show edit buffer with cursor
                            let before = &self.edit_buffer[..self.edit_cursor];
                            let after = &self.edit_buffer[self.edit_cursor..];
                            spans.push(Span::styled(before, value_style));
                            spans.push(Span::styled(
                                "│",
                                Style::default()
                                    .fg(theme.primary)
                                    .add_modifier(Modifier::RAPID_BLINK),
                            ));
                            spans.push(Span::styled(after, value_style));
                        }
                    }
                } else {
                    spans.push(Span::styled(value_display, value_style));
                }

                // Dirty indicator
                if item.dirty {
                    spans.push(Span::styled(" *", Style::default().fg(theme.warning)));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(list_items)
            .highlight_style(
                Style::default()
                    .bg(theme.background_element)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    /// Render the description area.
    fn render_description(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let description = self
            .current_item()
            .map(|i| i.description.as_str())
            .unwrap_or("");

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(theme.border_style());

        let para = Paragraph::new(Span::styled(description, theme.muted_style()))
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(para, area);
    }

    /// Render the help line.
    fn render_help(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let help_spans = if self.editing {
            if self.capturing_keybind {
                vec![
                    Span::styled("Press key", theme.highlight_style()),
                    Span::styled(" to capture  ", theme.dim_style()),
                    Span::styled("Esc", theme.highlight_style()),
                    Span::styled(" cancel", theme.dim_style()),
                ]
            } else {
                vec![
                    Span::styled("Enter", theme.highlight_style()),
                    Span::styled(" confirm  ", theme.dim_style()),
                    Span::styled("Esc", theme.highlight_style()),
                    Span::styled(" cancel", theme.dim_style()),
                ]
            }
        } else {
            vec![
                Span::styled("Tab", theme.highlight_style()),
                Span::styled(" tabs  ", theme.dim_style()),
                Span::styled("j/k", theme.highlight_style()),
                Span::styled(" nav  ", theme.dim_style()),
                Span::styled("Enter", theme.highlight_style()),
                Span::styled(" edit  ", theme.dim_style()),
                Span::styled("s", theme.highlight_style()),
                Span::styled(" save project  ", theme.dim_style()),
                Span::styled("S", theme.highlight_style()),
                Span::styled(" save global  ", theme.dim_style()),
                Span::styled("Esc", theme.highlight_style()),
                Span::styled(" close", theme.dim_style()),
            ]
        };

        let help = Paragraph::new(Line::from(help_spans)).alignment(Alignment::Center);

        frame.render_widget(help, area);
    }

    /// Get the current theme value (for live preview).
    pub fn get_theme(&self) -> Option<String> {
        self.items.get(&SettingsTab::General).and_then(|items| {
            items.iter().find(|i| i.key == "theme").and_then(|i| {
                if let SettingValue::Select { value, .. } = &i.value {
                    Some(value.clone())
                } else {
                    None
                }
            })
        })
    }

    /// Get the current render settings from Performance tab.
    pub fn get_render_settings(&self) -> RenderSettings {
        let items = match self.items.get(&SettingsTab::Performance) {
            Some(items) => items,
            None => return RenderSettings::default(),
        };

        let mut settings = RenderSettings::default();

        for item in items {
            match item.key.as_str() {
                "perf.markdown" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.markdown_enabled = *v;
                    }
                }
                "perf.syntax_highlighting" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.syntax_highlighting_enabled = *v;
                    }
                }
                "perf.code_backgrounds" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.code_backgrounds_enabled = *v;
                    }
                }
                "perf.tables" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.tables_enabled = *v;
                    }
                }
                "perf.streaming_fps" => {
                    if let SettingValue::Select { value, .. } = &item.value {
                        settings.streaming_fps = value.parse().unwrap_or(20);
                    }
                }
                "perf.max_messages" => {
                    if let SettingValue::Select { value, .. } = &item.value {
                        settings.max_messages = value.parse().unwrap_or(200);
                    }
                }
                "perf.low_memory_mode" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.low_memory_mode = *v;
                        // Note: We don't override other settings here. The user's
                        // explicit settings in the dialog take precedence.
                    }
                }
                "perf.enable_test_commands" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.enable_test_commands = *v;
                    }
                }
                // Test provider settings
                "test.model_enabled" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.test_model_enabled = *v;
                    }
                }
                "test.emulate_thinking" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.test_emulate_thinking = *v;
                    }
                }
                "test.emulate_tool_calls" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.test_emulate_tool_calls = *v;
                    }
                }
                "test.emulate_tool_observed" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.test_emulate_tool_observed = *v;
                    }
                }
                "test.emulate_streaming" => {
                    if let SettingValue::Bool(v) = &item.value {
                        settings.test_emulate_streaming = *v;
                    }
                }
                _ => {}
            }
        }

        settings
    }
}
