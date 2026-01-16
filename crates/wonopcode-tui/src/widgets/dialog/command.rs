//! Command palette and selection dialogs.
//!
//! This module contains dialogs that wrap the common `SelectDialog` for specific
//! purposes like command selection, model selection, session selection, theme selection,
//! and agent selection.

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};

use crate::theme::Theme;

use super::common::{DialogItem, SelectDialog};

/// Command palette dialog.
#[derive(Debug, Clone)]
pub struct CommandPalette {
    /// Inner select dialog.
    select: SelectDialog,
}

impl CommandPalette {
    /// Create a new command palette with default commands.
    pub fn new() -> Self {
        let items = vec![
            DialogItem::new("new_session", "New Session")
                .with_description("Start a new conversation")
                .with_keybind("Ctrl+X N")
                .with_category("Session"),
            DialogItem::new("session_list", "Session List")
                .with_description("Browse previous sessions")
                .with_keybind("Ctrl+X L")
                .with_category("Session"),
            DialogItem::new("model_select", "Select Model")
                .with_description("Change the AI model")
                .with_keybind("Ctrl+X M")
                .with_category("Model"),
            DialogItem::new("agent_select", "Select Agent")
                .with_description("Change the active agent")
                .with_keybind("Ctrl+X A")
                .with_category("Agent"),
            DialogItem::new("toggle_sidebar", "Toggle Sidebar")
                .with_description("Show/hide the sidebar")
                .with_keybind("Ctrl+X B")
                .with_category("View"),
            DialogItem::new("theme_select", "Select Theme")
                .with_description("Change color theme")
                .with_keybind("Ctrl+X T")
                .with_category("View"),
            DialogItem::new("copy_last", "Copy Last Response")
                .with_description("Copy assistant's last message")
                .with_keybind("Ctrl+X Y")
                .with_category("Edit"),
            DialogItem::new("edit_input", "Edit in External Editor")
                .with_description("Open input in $EDITOR")
                .with_keybind("Ctrl+X E")
                .with_category("Edit"),
            DialogItem::new("undo", "Undo Message")
                .with_description("Undo last message exchange")
                .with_keybind("Ctrl+X U")
                .with_category("Edit"),
            DialogItem::new("redo", "Redo Message")
                .with_description("Redo undone message")
                .with_keybind("Ctrl+X R")
                .with_category("Edit"),
            DialogItem::new("clear_history", "Clear History")
                .with_description("Clear conversation history")
                .with_category("Session"),
            DialogItem::new("export_session", "Export Session")
                .with_description("Export conversation to file")
                .with_keybind("Ctrl+X X")
                .with_category("Session"),
            DialogItem::new("sandbox", "Sandbox")
                .with_description("Start, stop, or restart sandbox")
                .with_keybind("/sandbox")
                .with_category("System"),
            DialogItem::new("mcp_servers", "MCP Servers")
                .with_description("Manage MCP server connections")
                .with_category("System"),
            DialogItem::new("help", "Help")
                .with_description("Show keybindings and help")
                .with_keybind("?")
                .with_category("Help"),
            DialogItem::new("quit", "Quit")
                .with_description("Exit wonopcode")
                .with_keybind("Ctrl+C")
                .with_category("System"),
        ];

        Self {
            select: SelectDialog::new("Command Palette", items),
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        self.select.handle_key(key)
    }

    /// Render the command palette.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.select.render(frame, area, theme);
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

/// Model selection dialog.
#[derive(Debug, Clone)]
pub struct ModelDialog {
    /// Inner select dialog.
    select: SelectDialog,
}

impl ModelDialog {
    /// Create a new model dialog.
    pub fn new() -> Self {
        Self::with_options(false)
    }

    /// Create a new model dialog with options.
    ///
    /// # Arguments
    /// * `show_test_models` - Whether to show test models (only when test_model_enabled is true in settings)
    pub fn with_options(show_test_models: bool) -> Self {
        let mut items = vec![
            // ══════════════════════════════════════════════════════════════
            // Anthropic
            // ══════════════════════════════════════════════════════════════
            // Claude 4.5 (Latest)
            DialogItem::new("anthropic/claude-sonnet-4-5-20250929", "Claude Sonnet 4.5")
                .with_description("Recommended - smart & fast")
                .with_category("Anthropic"),
            DialogItem::new("anthropic/claude-haiku-4-5-20251001", "Claude Haiku 4.5")
                .with_description("Fastest model")
                .with_category("Anthropic"),
            DialogItem::new("anthropic/claude-opus-4-5-20251101", "Claude Opus 4.5")
                .with_description("Most intelligent")
                .with_category("Anthropic"),
            // Claude 4.x (Legacy)
            DialogItem::new("anthropic/claude-sonnet-4-20250514", "Claude Sonnet 4")
                .with_description("Legacy Sonnet")
                .with_category("Anthropic"),
            DialogItem::new("anthropic/claude-opus-4-1-20250805", "Claude Opus 4.1")
                .with_description("Legacy Opus 4.1")
                .with_category("Anthropic"),
            DialogItem::new("anthropic/claude-opus-4-20250514", "Claude Opus 4")
                .with_description("Legacy Opus")
                .with_category("Anthropic"),
            // Claude 3.x (Legacy)
            DialogItem::new("anthropic/claude-3-7-sonnet-20250219", "Claude 3.7 Sonnet")
                .with_description("Extended thinking")
                .with_category("Anthropic"),
            DialogItem::new("anthropic/claude-3-haiku-20240307", "Claude 3 Haiku")
                .with_description("Fast, economical")
                .with_category("Anthropic"),
            // ══════════════════════════════════════════════════════════════
            // OpenAI
            // ══════════════════════════════════════════════════════════════
            // GPT-5 Series (Latest)
            DialogItem::new("openai/gpt-5.2", "GPT-5.2")
                .with_description("Best for coding & agents")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-5.1", "GPT-5.1")
                .with_description("Configurable reasoning")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-5", "GPT-5")
                .with_description("Intelligent reasoning")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-5-mini", "GPT-5 mini")
                .with_description("Fast, cost-efficient")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-5-nano", "GPT-5 nano")
                .with_description("Fastest, cheapest")
                .with_category("OpenAI"),
            // GPT-4.1 Series
            DialogItem::new("openai/gpt-4.1", "GPT-4.1")
                .with_description("Smartest non-reasoning")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-4.1-mini", "GPT-4.1 mini")
                .with_description("Fast, 1M context")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-4.1-nano", "GPT-4.1 nano")
                .with_description("Cheapest, 1M context")
                .with_category("OpenAI"),
            // O-Series (Reasoning)
            DialogItem::new("openai/o3", "o3")
                .with_description("Reasoning model")
                .with_category("OpenAI"),
            DialogItem::new("openai/o3-mini", "o3-mini")
                .with_description("Fast reasoning")
                .with_category("OpenAI"),
            DialogItem::new("openai/o4-mini", "o4-mini")
                .with_description("Cost-efficient reasoning")
                .with_category("OpenAI"),
            // Legacy
            DialogItem::new("openai/gpt-4o", "GPT-4o")
                .with_description("Previous flagship")
                .with_category("OpenAI"),
            DialogItem::new("openai/gpt-4o-mini", "GPT-4o mini")
                .with_description("Fast, affordable")
                .with_category("OpenAI"),
            DialogItem::new("openai/o1", "o1")
                .with_description("Legacy reasoning")
                .with_category("OpenAI"),
            // ══════════════════════════════════════════════════════════════
            // Google
            // ══════════════════════════════════════════════════════════════
            DialogItem::new("google/gemini-2.0-flash", "Gemini 2.0 Flash")
                .with_description("Latest, fast, multimodal")
                .with_category("Google"),
            DialogItem::new("google/gemini-1.5-pro", "Gemini 1.5 Pro")
                .with_description("2M context window")
                .with_category("Google"),
            DialogItem::new("google/gemini-1.5-flash", "Gemini 1.5 Flash")
                .with_description("Fast and affordable")
                .with_category("Google"),
            // ══════════════════════════════════════════════════════════════
            // xAI (Grok)
            // ══════════════════════════════════════════════════════════════
            DialogItem::new("xai/grok-3", "Grok 3")
                .with_description("Latest Grok model")
                .with_category("xAI"),
            DialogItem::new("xai/grok-3-mini", "Grok 3 Mini")
                .with_description("Compact Grok model")
                .with_category("xAI"),
            DialogItem::new("xai/grok-2", "Grok 2")
                .with_description("Previous generation")
                .with_category("xAI"),
            // ══════════════════════════════════════════════════════════════
            // Mistral
            // ══════════════════════════════════════════════════════════════
            DialogItem::new("mistral/mistral-large-latest", "Mistral Large")
                .with_description("Flagship model")
                .with_category("Mistral"),
            DialogItem::new("mistral/mistral-small-latest", "Mistral Small")
                .with_description("Fast and efficient")
                .with_category("Mistral"),
            DialogItem::new("mistral/codestral-latest", "Codestral")
                .with_description("Code-specialized")
                .with_category("Mistral"),
            DialogItem::new("mistral/pixtral-large-latest", "Pixtral Large")
                .with_description("Vision model")
                .with_category("Mistral"),
            // ══════════════════════════════════════════════════════════════
            // Groq (Fast inference)
            // ══════════════════════════════════════════════════════════════
            DialogItem::new("groq/llama-3.3-70b-versatile", "Llama 3.3 70B")
                .with_description("Fast Llama inference")
                .with_category("Groq"),
            DialogItem::new("groq/llama-3.1-8b-instant", "Llama 3.1 8B Instant")
                .with_description("Ultra-fast small model")
                .with_category("Groq"),
            DialogItem::new("groq/mixtral-8x7b-32768", "Mixtral 8x7B")
                .with_description("MoE model")
                .with_category("Groq"),
            DialogItem::new("groq/gemma2-9b-it", "Gemma 2 9B")
                .with_description("Google's Gemma")
                .with_category("Groq"),
            DialogItem::new("groq/deepseek-r1-distill-llama-70b", "DeepSeek R1 Distill")
                .with_description("Reasoning model")
                .with_category("Groq"),
            // ══════════════════════════════════════════════════════════════
            // DeepInfra
            // ══════════════════════════════════════════════════════════════
            DialogItem::new("deepinfra/deepseek-ai/DeepSeek-V3", "DeepSeek V3")
                .with_description("Latest DeepSeek")
                .with_category("DeepInfra"),
            DialogItem::new("deepinfra/deepseek-ai/DeepSeek-R1", "DeepSeek R1")
                .with_description("Reasoning model")
                .with_category("DeepInfra"),
            DialogItem::new("deepinfra/Qwen/Qwen2.5-72B-Instruct", "Qwen 2.5 72B")
                .with_description("Alibaba's flagship")
                .with_category("DeepInfra"),
            DialogItem::new(
                "deepinfra/meta-llama/Meta-Llama-3.1-405B-Instruct",
                "Llama 3.1 405B",
            )
            .with_description("Largest Llama")
            .with_category("DeepInfra"),
            // ══════════════════════════════════════════════════════════════
            // Together AI
            // ══════════════════════════════════════════════════════════════
            DialogItem::new("together/deepseek-ai/DeepSeek-V3", "DeepSeek V3")
                .with_description("Latest DeepSeek")
                .with_category("Together"),
            DialogItem::new("together/deepseek-ai/DeepSeek-R1", "DeepSeek R1")
                .with_description("Reasoning model")
                .with_category("Together"),
            DialogItem::new(
                "together/meta-llama/Llama-3.3-70B-Instruct-Turbo",
                "Llama 3.3 70B Turbo",
            )
            .with_description("Fast Llama")
            .with_category("Together"),
            DialogItem::new(
                "together/Qwen/Qwen2.5-72B-Instruct-Turbo",
                "Qwen 2.5 72B Turbo",
            )
            .with_description("Fast Qwen")
            .with_category("Together"),
            DialogItem::new(
                "together/Qwen/Qwen2.5-Coder-32B-Instruct",
                "Qwen 2.5 Coder 32B",
            )
            .with_description("Code-specialized")
            .with_category("Together"),
            // ══════════════════════════════════════════════════════════════
            // OpenRouter (Multi-provider gateway)
            // ══════════════════════════════════════════════════════════════
            DialogItem::new(
                "openrouter/anthropic/claude-3.5-sonnet",
                "Claude 3.5 Sonnet",
            )
            .with_description("Via OpenRouter")
            .with_category("OpenRouter"),
            DialogItem::new(
                "openrouter/meta-llama/llama-3.1-405b-instruct",
                "Llama 3.1 405B",
            )
            .with_description("Largest Llama")
            .with_category("OpenRouter"),
            DialogItem::new("openrouter/google/gemini-pro-1.5", "Gemini Pro 1.5")
                .with_description("Google via OR")
                .with_category("OpenRouter"),
        ];

        // Add test models if enabled
        if show_test_models {
            items.push(
                DialogItem::new("test/test-128b", "Test 128B")
                    .with_description("UI/UX testing - simulated responses")
                    .with_category("Test"),
            );
        }

        Self {
            select: SelectDialog::new("Select Model", items),
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        self.select.handle_key(key)
    }

    /// Render the dialog with section headers.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.select.render_with_sections(frame, area, theme, true);
    }
}

impl Default for ModelDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Session list dialog.
#[derive(Debug, Clone)]
pub struct SessionDialog {
    /// Inner select dialog.
    select: SelectDialog,
}

impl SessionDialog {
    /// Create a new session dialog.
    pub fn new(sessions: Vec<(String, String, String)>) -> Self {
        let items: Vec<DialogItem> = sessions
            .into_iter()
            .map(|(id, title, updated)| DialogItem::new(&id, &title).with_description(updated))
            .collect();

        Self {
            select: SelectDialog::new("Sessions", items),
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        self.select.handle_key(key)
    }

    /// Render the dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.select.render(frame, area, theme);
    }
}

/// Theme selection dialog.
#[derive(Debug, Clone)]
pub struct ThemeDialog {
    /// Inner select dialog.
    select: SelectDialog,
}

impl ThemeDialog {
    /// Create a new theme dialog.
    pub fn new() -> Self {
        let items = vec![
            DialogItem::new("dark", "Dark").with_description("Default dark theme"),
            DialogItem::new("light", "Light").with_description("Light theme"),
            DialogItem::new("catppuccin", "Catppuccin").with_description("Soothing pastel theme"),
            DialogItem::new("dracula", "Dracula").with_description("Dark purple theme"),
            DialogItem::new("gruvbox", "Gruvbox").with_description("Retro groove colors"),
            DialogItem::new("nord", "Nord").with_description("Arctic, bluish colors"),
            DialogItem::new("tokyo-night", "Tokyo Night").with_description("Dark Tokyo theme"),
            DialogItem::new("one-dark", "One Dark").with_description("Atom One Dark"),
            DialogItem::new("monokai", "Monokai").with_description("Sublime Text classic"),
            DialogItem::new("solarized-dark", "Solarized Dark")
                .with_description("Ethan Schoonover's theme"),
        ];

        Self {
            select: SelectDialog::new("Select Theme", items),
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        self.select.handle_key(key)
    }

    /// Render the dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.select.render(frame, area, theme);
    }
}

impl Default for ThemeDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent selection dialog.
#[derive(Debug, Clone)]
pub struct AgentDialog {
    /// Inner select dialog.
    select: SelectDialog,
}

impl AgentDialog {
    /// Create a new agent dialog with the given agents.
    pub fn new(agents: Vec<AgentInfo>) -> Self {
        let items: Vec<DialogItem> = agents
            .into_iter()
            .map(|agent| {
                let mut item = DialogItem::new(&agent.name, &agent.display_name);
                if let Some(desc) = agent.description {
                    item = item.with_description(desc);
                }
                if agent.is_default {
                    item = item.with_keybind("default");
                }
                item
            })
            .collect();

        Self {
            select: SelectDialog::new("Select Agent", items),
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        self.select.handle_key(key)
    }

    /// Render the dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.select.render(frame, area, theme);
    }
}

/// Agent information for the dialog.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// Agent identifier.
    pub name: String,
    /// Display name.
    pub display_name: String,
    /// Description.
    pub description: Option<String>,
    /// Whether this is the default agent.
    pub is_default: bool,
}

impl AgentInfo {
    /// Create a new agent info.
    pub fn new(name: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            description: None,
            is_default: false,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set as default.
    pub fn as_default(mut self) -> Self {
        self.is_default = true;
        self
    }
}
