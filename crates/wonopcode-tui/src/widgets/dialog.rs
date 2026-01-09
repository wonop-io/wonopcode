//! Dialog widgets for modal interfaces.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::theme::{RenderSettings, Theme};

/// A selectable item in a dialog.
#[derive(Debug, Clone)]
pub struct DialogItem {
    /// Unique identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Optional description.
    pub description: Option<String>,
    /// Optional keybind hint.
    pub keybind: Option<String>,
    /// Optional category.
    pub category: Option<String>,
}

impl DialogItem {
    /// Create a new dialog item.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            keybind: None,
            category: None,
        }
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a keybind hint.
    pub fn with_keybind(mut self, keybind: impl Into<String>) -> Self {
        self.keybind = Some(keybind.into());
        self
    }

    /// Add a category.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// A filterable selection dialog.
#[derive(Debug, Clone)]
pub struct SelectDialog {
    /// Title of the dialog.
    title: String,
    /// All items (unfiltered).
    items: Vec<DialogItem>,
    /// Filtered items (indices into items).
    filtered: Vec<usize>,
    /// Current filter text.
    filter: String,
    /// Selected index in filtered list.
    selected: usize,
    /// List state for rendering.
    list_state: ListState,
}

impl SelectDialog {
    /// Create a new select dialog.
    pub fn new(title: impl Into<String>, items: Vec<DialogItem>) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            title: title.into(),
            items,
            filtered,
            filter: String::new(),
            selected: 0,
            list_state,
        }
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&DialogItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.items.get(idx))
    }

    /// Handle a key event. Returns Some(id) if an item was selected.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Enter => {
                return self.selected_item().map(|item| item.id.clone());
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

    /// Update the filtered list based on current filter.
    fn update_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.label.to_lowercase().contains(&filter_lower)
                        || item
                            .description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&filter_lower))
                            .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Reset selection
        self.selected = 0;
        self.list_state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    /// Render the dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.render_with_sections(frame, area, theme, false);
    }

    /// Render the dialog with optional section headers.
    pub fn render_with_sections(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        show_sections: bool,
    ) {
        // Calculate dialog size (centered, 60% width, max 80 chars)
        let dialog_width = (area.width * 60 / 100).clamp(40, 80);
        let dialog_height = (area.height * 70 / 100).clamp(10, 30);

        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        // Clear the area behind the dialog
        frame.render_widget(Clear, dialog_area);

        // Dialog block
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into filter input and list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
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

        // Build list items with optional section headers
        let mut list_items: Vec<ListItem> = Vec::new();
        let mut current_category: Option<String> = None;
        let mut visual_to_filtered: Vec<Option<usize>> = Vec::new(); // Maps visual index to filtered index (None for headers)

        for (filtered_idx, &item_idx) in self.filtered.iter().enumerate() {
            let item = &self.items[item_idx];

            // Add section header if category changed and sections are enabled
            if show_sections {
                let item_category = item.category.clone();
                if item_category != current_category {
                    if let Some(ref cat) = item_category {
                        // Add section header
                        let header = ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("── {cat} "),
                                Style::default()
                                    .fg(theme.accent)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled("─".repeat(30), Style::default().fg(theme.border_subtle)),
                        ]));
                        list_items.push(header);
                        visual_to_filtered.push(None); // Header, not selectable
                    }
                    current_category = item_category;
                }
            }

            // Add the actual item
            let mut spans = vec![Span::styled(&item.label, theme.text_style())];

            if let Some(desc) = &item.description {
                spans.push(Span::styled(" - ", theme.dim_style()));
                spans.push(Span::styled(desc, theme.dim_style()));
            }

            if let Some(kb) = &item.keybind {
                spans.push(Span::styled(format!("  [{kb}]"), theme.highlight_style()));
            }

            list_items.push(ListItem::new(Line::from(spans)));
            visual_to_filtered.push(Some(filtered_idx));
        }

        // Find the visual index for the current selection
        let visual_selected = visual_to_filtered
            .iter()
            .position(|&f| f == Some(self.selected))
            .unwrap_or(0);

        let mut visual_list_state = ListState::default();
        visual_list_state.select(Some(visual_selected));

        let list = List::new(list_items)
            .highlight_style(
                Style::default()
                    .bg(theme.border_active)
                    .fg(theme.background)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, chunks[1], &mut visual_list_state);
    }
}

/// Helper to create a centered rectangle.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

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

/// MCP server status.
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

/// Simple text input dialog for things like rename.
#[derive(Debug, Clone, Default)]
pub struct InputDialog {
    /// Dialog title.
    pub title: String,
    /// Input prompt/label.
    pub prompt: String,
    /// Current input value.
    pub value: String,
    /// Cursor position.
    cursor: usize,
}

impl InputDialog {
    /// Create a new input dialog.
    pub fn new(title: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            value: String::new(),
            cursor: 0,
        }
    }

    /// Create with an initial value.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.len();
        self
    }

    /// Get the current value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Handle a key event. Returns Some(value) on Enter, None on Escape.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<InputDialogResult> {
        match key.code {
            KeyCode::Enter => {
                return Some(InputDialogResult::Submit(self.value.clone()));
            }
            KeyCode::Esc => {
                return Some(InputDialogResult::Cancel);
            }
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.value.len();
            }
            _ => {}
        }
        None
    }

    /// Render the input dialog.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 50.min(area.width.saturating_sub(4));
        let dialog_height = 7;
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout: prompt, input field, help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Prompt
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Input
                Constraint::Length(1), // Spacing
                Constraint::Length(1), // Help
            ])
            .split(inner);

        // Prompt
        let prompt = Paragraph::new(Span::styled(&self.prompt, theme.text_style()));
        frame.render_widget(prompt, chunks[0]);

        // Input field with cursor
        let display_value = if self.cursor < self.value.len() {
            let (before, after) = self.value.split_at(self.cursor);
            let (cursor_char, rest) = after.split_at(1);
            Line::from(vec![
                Span::styled(before, theme.text_style()),
                Span::styled(
                    cursor_char,
                    Style::default().bg(theme.primary).fg(theme.background),
                ),
                Span::styled(rest, theme.text_style()),
            ])
        } else {
            Line::from(vec![
                Span::styled(&self.value, theme.text_style()),
                Span::styled(" ", Style::default().bg(theme.primary)),
            ])
        };
        let input = Paragraph::new(display_value);
        frame.render_widget(input, chunks[2]);

        // Help text
        let help = Paragraph::new(Line::from(vec![
            Span::styled("Enter", theme.highlight_style()),
            Span::styled(" confirm  ", theme.dim_style()),
            Span::styled("Esc", theme.highlight_style()),
            Span::styled(" cancel", theme.dim_style()),
        ]));
        frame.render_widget(help, chunks[4]);
    }
}

/// Result from input dialog.
#[derive(Debug, Clone)]
pub enum InputDialogResult {
    /// User submitted a value.
    Submit(String),
    /// User cancelled.
    Cancel,
}

/// Timeline item representing a message in the conversation.
#[derive(Debug, Clone)]
pub struct TimelineItem {
    /// Message ID.
    pub id: String,
    /// Role (user/assistant).
    pub role: String,
    /// Preview of the message content.
    pub preview: String,
    /// Timestamp or relative time.
    pub time: String,
    /// Whether this is a tool call.
    pub is_tool: bool,
}

impl TimelineItem {
    /// Create a new timeline item.
    pub fn new(id: impl Into<String>, role: impl Into<String>, preview: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: role.into(),
            preview: preview.into(),
            time: String::new(),
            is_tool: false,
        }
    }

    /// Set the timestamp.
    pub fn with_time(mut self, time: impl Into<String>) -> Self {
        self.time = time.into();
        self
    }

    /// Mark as a tool call.
    pub fn as_tool(mut self) -> Self {
        self.is_tool = true;
        self
    }
}

/// Timeline dialog for viewing message history and navigation.
#[derive(Debug, Clone)]
pub struct TimelineDialog {
    /// Timeline items.
    items: Vec<TimelineItem>,
    /// Selected index.
    selected: usize,
    /// List state for rendering.
    list_state: ListState,
}

impl TimelineDialog {
    /// Create a new timeline dialog with the given items.
    pub fn new(items: Vec<TimelineItem>) -> Self {
        let mut list_state = ListState::default();
        if !items.is_empty() {
            // Start at the bottom (most recent)
            list_state.select(Some(items.len().saturating_sub(1)));
        }

        Self {
            selected: items.len().saturating_sub(1),
            items,
            list_state,
        }
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&TimelineItem> {
        self.items.get(self.selected)
    }

    /// Handle a key event. Returns Some(action) if an action was triggered.
    /// Actions: `goto:<id>` for navigation, `fork:<id>` for forking.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match key.code {
            KeyCode::Enter => {
                // Go to the selected message
                return self.selected_item().map(|item| format!("goto:{}", item.id));
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                // Fork from the selected message
                return self.selected_item().map(|item| format!("fork:{}", item.id));
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                if self.selected < self.items.len().saturating_sub(1) {
                    self.selected += 1;
                    self.list_state.select(Some(self.selected));
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
                self.list_state.select(Some(0));
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.items.len().saturating_sub(1);
                self.list_state.select(Some(self.selected));
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(10);
                self.list_state.select(Some(self.selected));
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + 10).min(self.items.len().saturating_sub(1));
                self.list_state.select(Some(self.selected));
            }
            _ => {}
        }
        None
    }

    /// Render the timeline dialog.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = (area.width * 60 / 100).clamp(45, 70);
        let dialog_height = (area.height * 80 / 100).clamp(12, 30);
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Message Timeline ")
            .borders(Borders::ALL)
            .border_style(theme.border_active_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split into list and help text
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        // Render timeline list
        let list_items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                // Role indicator
                let role_style = if item.role == "user" {
                    Style::default().fg(theme.primary)
                } else if item.is_tool {
                    Style::default().fg(theme.accent)
                } else {
                    theme.text_style()
                };

                let role_icon = if item.role == "user" {
                    "▸"
                } else if item.is_tool {
                    "◇"
                } else {
                    "◂"
                };

                // Message number
                let num = format!("{:3}", idx + 1);

                // Truncate preview if needed
                let max_preview = (dialog_width as usize).saturating_sub(20);
                let preview = if item.preview.chars().count() > max_preview {
                    let t: String = item.preview.chars().take(max_preview.saturating_sub(3)).collect();
                    format!("{}...", t)
                } else {
                    item.preview.clone()
                };

                let spans = vec![
                    Span::styled(num, theme.muted_style()),
                    Span::styled(" ", theme.text_style()),
                    Span::styled(role_icon, role_style),
                    Span::styled(" ", theme.text_style()),
                    Span::styled(preview, theme.text_style()),
                ];

                // Add time if present
                let line = if !item.time.is_empty() {
                    let mut s = spans;
                    s.push(Span::styled(
                        format!("  {}", item.time),
                        theme.muted_style(),
                    ));
                    Line::from(s)
                } else {
                    Line::from(spans)
                };

                ListItem::new(line)
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

        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

        // Render help text
        let help_lines = vec![Line::from(vec![
            Span::styled("Enter", theme.highlight_style()),
            Span::styled(" go to message  ", theme.dim_style()),
            Span::styled("f", theme.highlight_style()),
            Span::styled(" fork from here  ", theme.dim_style()),
            Span::styled("Esc", theme.highlight_style()),
            Span::styled(" close", theme.dim_style()),
        ])];
        let help_para = Paragraph::new(help_lines).alignment(Alignment::Center);
        frame.render_widget(help_para, chunks[1]);
    }
}

/// Help dialog showing keybindings.
#[derive(Debug, Clone, Default)]
pub struct HelpDialog;

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

// ============================================================================
// Settings Dialog
// ============================================================================

/// Settings category tabs.
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
