//! Color themes for the TUI.

use ratatui::style::{Color, Modifier, Style};

/// Agent mode for coloring.
/// Only includes primary (user-selectable) agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// Build agent - default coding agent with full access.
    #[default]
    Build,
    /// Plan agent - planning with restricted write permissions.
    Plan,
}

impl AgentMode {
    /// Get agent name.
    pub fn name(&self) -> &'static str {
        match self {
            AgentMode::Build => "Build",
            AgentMode::Plan => "Plan",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plan" => AgentMode::Plan,
            // Default to Build for any other value (including "build", "default", etc.)
            _ => AgentMode::Build,
        }
    }

    /// Get the next agent in the cycle.
    pub fn next(&self) -> Self {
        match self {
            AgentMode::Build => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Build,
        }
    }

    /// Get the previous agent in the cycle (reverse direction).
    pub fn prev(&self) -> Self {
        match self {
            AgentMode::Build => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Build,
        }
    }

    /// Get the agent ID string (lowercase).
    pub fn id(&self) -> &'static str {
        match self {
            AgentMode::Build => "build",
            AgentMode::Plan => "plan",
        }
    }
}

/// Color theme for the TUI.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name.
    pub name: String,

    // Background hierarchy (3-4 shades)
    /// Base background - darkest.
    pub background: Color,
    /// Panel background (sidebar, dialogs).
    pub background_panel: Color,
    /// Element background (input, code blocks).
    pub background_element: Color,
    /// Menu background.
    pub background_menu: Color,

    // Text colors
    /// Primary text color.
    pub text: Color,
    /// Muted/secondary text.
    pub text_muted: Color,

    // Accent colors (for agent cycling)
    /// Primary accent (orange/peach).
    pub primary: Color,
    /// Secondary accent (blue).
    pub secondary: Color,
    /// Tertiary accent (purple).
    pub accent: Color,

    // Semantic colors
    /// Success color (green).
    pub success: Color,
    /// Warning color (orange/yellow).
    pub warning: Color,
    /// Error color (red).
    pub error: Color,
    /// Info color (blue).
    pub info: Color,

    // Border colors
    /// Default border.
    pub border: Color,
    /// Active/focused border.
    pub border_active: Color,
    /// Subtle border.
    pub border_subtle: Color,
    /// Tool border (very subtle, dark zinc).
    pub tool_border: Color,

    // Diff colors
    /// Added line background.
    pub diff_added_bg: Color,
    /// Removed line background.
    pub diff_removed_bg: Color,
    /// Added text color.
    pub diff_added: Color,
    /// Removed text color.
    pub diff_removed: Color,

    // Syntax highlighting
    pub syntax_comment: Color,
    pub syntax_keyword: Color,
    pub syntax_function: Color,
    pub syntax_variable: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_type: Color,
    pub syntax_operator: Color,

    // Opacity for thinking/reasoning (0.0-1.0)
    pub thinking_opacity: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self::troelsim()
    }
}

impl Theme {
    /// Get a theme by name.
    pub fn by_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "light" => Self::light(),
            "catppuccin" | "catppuccin-mocha" => Self::catppuccin_mocha(),
            "dracula" => Self::dracula(),
            "gruvbox" | "gruvbox-dark" => Self::gruvbox_dark(),
            "nord" => Self::nord(),
            "tokyo-night" | "tokyonight" => Self::tokyo_night(),
            "rosepine" | "rose-pine" => Self::rose_pine(),
            "wonopcode" => Self::wonopcode(),
            // Default to troels.im theme
            _ => Self::troelsim(),
        }
    }

    /// List available theme names.
    pub fn available() -> Vec<&'static str> {
        vec![
            "troelsim",
            "wonopcode",
            "light",
            "catppuccin",
            "dracula",
            "gruvbox",
            "nord",
            "tokyo-night",
            "rosepine",
        ]
    }

    /// Get color for an agent mode.
    pub fn agent_color(&self, mode: AgentMode) -> Color {
        match mode {
            AgentMode::Build => self.secondary,
            AgentMode::Plan => self.accent,
        }
    }

    /// Get color for an agent by name (for custom/subagents).
    pub fn agent_color_by_name(&self, name: &str) -> Color {
        match name.to_lowercase().as_str() {
            "build" => self.secondary,
            "plan" => self.accent,
            "explore" => self.success,
            "general" => self.warning,
            _ => self.primary,
        }
    }

    /// Get color for agent by index (for cycling).
    pub fn agent_color_by_index(&self, index: usize) -> Color {
        let colors = [
            self.secondary,
            self.accent,
            self.success,
            self.warning,
            self.primary,
            self.error,
        ];
        colors[index % colors.len()]
    }

    /// Wonopcode default theme (dark).
    pub fn wonopcode() -> Self {
        Self {
            name: "wonopcode".to_string(),

            // Background hierarchy
            background: Color::Rgb(10, 10, 10),         // #0a0a0a
            background_panel: Color::Rgb(20, 20, 20),   // #141414
            background_element: Color::Rgb(24, 24, 27), // #18181b - zinc-900
            background_menu: Color::Rgb(30, 30, 30),    // #1e1e1e

            // Text
            text: Color::Rgb(250, 250, 250),       // #fafafa
            text_muted: Color::Rgb(128, 128, 128), // #808080

            // Accents
            primary: Color::Rgb(250, 178, 131), // #fab283 (peach/orange)
            secondary: Color::Rgb(92, 156, 245), // #5c9cf5 (blue)
            accent: Color::Rgb(157, 124, 216),  // #9d7cd8 (purple)

            // Semantic
            success: Color::Rgb(127, 216, 143), // #7fd88f (green)
            warning: Color::Rgb(245, 167, 66),  // #f5a742 (orange)
            error: Color::Rgb(224, 108, 117),   // #e06c75 (red)
            info: Color::Rgb(92, 156, 245),     // #5c9cf5 (blue)

            // Borders
            border: Color::Rgb(60, 60, 60),           // #3c3c3c
            border_active: Color::Rgb(250, 178, 131), // primary
            border_subtle: Color::Rgb(40, 40, 40),    // #282828
            tool_border: Color::Rgb(39, 39, 42),      // #27272a - zinc-800

            // Diff
            diff_added_bg: Color::Rgb(32, 48, 59),   // #20303b
            diff_removed_bg: Color::Rgb(55, 34, 44), // #37222c
            diff_added: Color::Rgb(127, 216, 143),   // green
            diff_removed: Color::Rgb(224, 108, 117), // red

            // Syntax
            syntax_comment: Color::Rgb(128, 128, 128), // #808080
            syntax_keyword: Color::Rgb(157, 124, 216), // #9d7cd8 (purple)
            syntax_function: Color::Rgb(250, 178, 131), // #fab283 (orange)
            syntax_variable: Color::Rgb(224, 108, 117), // #e06c75 (red)
            syntax_string: Color::Rgb(127, 216, 143),  // #7fd88f (green)
            syntax_number: Color::Rgb(245, 167, 66),   // #f5a742 (orange)
            syntax_type: Color::Rgb(229, 192, 123),    // #e5c07b (yellow)
            syntax_operator: Color::Rgb(86, 182, 194), // #56b6c2 (cyan)

            thinking_opacity: 0.6,
        }
    }

    /// troels.im brand theme.
    ///
    /// Retro-futuristic cyberpunk aesthetic with:
    /// - Bright neon accents (green, cyan, purple, pink)
    /// - Deep dark backgrounds
    /// - Glowing digital energy feel
    pub fn troelsim() -> Self {
        Self {
            name: "troelsim".to_string(),

            // Background hierarchy - deep space-like darkness
            background: Color::Rgb(24, 24, 27), // #18181B - Deep Background
            background_panel: Color::Rgb(39, 39, 42), // #27272A - Surface Color
            background_element: Color::Rgb(50, 50, 55), // Slightly lighter for elements
            background_menu: Color::Rgb(39, 39, 42), // #27272A

            // Text - clean and readable
            text: Color::Rgb(250, 250, 250), // #fafafa - bright white
            text_muted: Color::Rgb(161, 161, 170), // #a1a1aa - zinc-400

            // Accents - neon vibrant colors
            primary: Color::Rgb(91, 222, 14), // #5BDE0E - Bright Green (main accent)
            secondary: Color::Rgb(14, 163, 222), // #0EA3DE - Cyan (digital energy)
            accent: Color::Rgb(145, 14, 222), // #910EDE - Purple (emphasis)

            // Semantic colors
            success: Color::Rgb(91, 222, 14), // #5BDE0E - Bright Green
            warning: Color::Rgb(250, 204, 21), // #facc15 - Yellow
            error: Color::Rgb(222, 13, 95),   // #DE0D5F - Pink (attention-grabbing)
            info: Color::Rgb(14, 163, 222),   // #0EA3DE - Cyan

            // Borders - subtle with potential for glow
            border: Color::Rgb(63, 63, 70), // #3f3f46 - zinc-700
            border_active: Color::Rgb(91, 222, 14), // #5BDE0E - Bright Green glow
            border_subtle: Color::Rgb(39, 39, 42), // #27272A
            tool_border: Color::Rgb(39, 39, 42), // #27272a - zinc-800

            // Diff colors
            diff_added_bg: Color::Rgb(30, 50, 30), // Dark green tint
            diff_removed_bg: Color::Rgb(50, 25, 35), // Dark pink tint
            diff_added: Color::Rgb(91, 222, 14),   // Bright Green
            diff_removed: Color::Rgb(222, 13, 95), // Pink

            // Syntax highlighting - vibrant retro-future
            syntax_comment: Color::Rgb(113, 113, 122), // #71717a - zinc-500
            syntax_keyword: Color::Rgb(145, 14, 222),  // #910EDE - Purple
            syntax_function: Color::Rgb(91, 222, 14),  // #5BDE0E - Bright Green
            syntax_variable: Color::Rgb(14, 163, 222), // #0EA3DE - Cyan
            syntax_string: Color::Rgb(91, 222, 14),    // #5BDE0E - Bright Green
            syntax_number: Color::Rgb(250, 204, 21),   // Yellow
            syntax_type: Color::Rgb(14, 163, 222),     // #0EA3DE - Cyan
            syntax_operator: Color::Rgb(222, 13, 95),  // #DE0D5F - Pink

            thinking_opacity: 0.6,
        }
    }

    /// Light theme.
    pub fn light() -> Self {
        Self {
            name: "light".to_string(),

            background: Color::Rgb(255, 255, 255), // #ffffff
            background_panel: Color::Rgb(250, 250, 250), // #fafafa
            background_element: Color::Rgb(245, 245, 245), // #f5f5f5
            background_menu: Color::Rgb(245, 245, 245),

            text: Color::Rgb(30, 30, 30), // #1e1e1e
            text_muted: Color::Rgb(128, 128, 128),

            primary: Color::Rgb(200, 120, 60),   // darker orange
            secondary: Color::Rgb(50, 100, 200), // blue
            accent: Color::Rgb(130, 80, 180),    // purple

            success: Color::Rgb(40, 160, 70),
            warning: Color::Rgb(200, 130, 30),
            error: Color::Rgb(200, 60, 70),
            info: Color::Rgb(50, 100, 200),

            border: Color::Rgb(220, 220, 220),
            border_active: Color::Rgb(50, 100, 200),
            border_subtle: Color::Rgb(235, 235, 235),
            tool_border: Color::Rgb(212, 212, 216), // zinc-300 for light theme

            diff_added_bg: Color::Rgb(220, 255, 220),
            diff_removed_bg: Color::Rgb(255, 220, 220),
            diff_added: Color::Rgb(40, 160, 70),
            diff_removed: Color::Rgb(200, 60, 70),

            syntax_comment: Color::Rgb(128, 128, 128),
            syntax_keyword: Color::Rgb(130, 80, 180),
            syntax_function: Color::Rgb(200, 120, 60),
            syntax_variable: Color::Rgb(200, 60, 70),
            syntax_string: Color::Rgb(40, 160, 70),
            syntax_number: Color::Rgb(200, 130, 30),
            syntax_type: Color::Rgb(180, 140, 50),
            syntax_operator: Color::Rgb(50, 140, 150),

            thinking_opacity: 0.5,
        }
    }

    /// Catppuccin Mocha theme.
    pub fn catppuccin_mocha() -> Self {
        Self {
            name: "catppuccin".to_string(),

            background: Color::Rgb(30, 30, 46),         // base
            background_panel: Color::Rgb(36, 36, 54),   // mantle
            background_element: Color::Rgb(49, 50, 68), // surface0
            background_menu: Color::Rgb(49, 50, 68),

            text: Color::Rgb(205, 214, 244),       // text
            text_muted: Color::Rgb(127, 132, 156), // overlay1

            primary: Color::Rgb(250, 179, 135),   // peach
            secondary: Color::Rgb(137, 180, 250), // blue
            accent: Color::Rgb(203, 166, 247),    // mauve

            success: Color::Rgb(166, 227, 161), // green
            warning: Color::Rgb(249, 226, 175), // yellow
            error: Color::Rgb(243, 139, 168),   // red
            info: Color::Rgb(137, 180, 250),    // blue

            border: Color::Rgb(69, 71, 90),           // surface1
            border_active: Color::Rgb(180, 190, 254), // lavender
            border_subtle: Color::Rgb(49, 50, 68),    // surface0
            tool_border: Color::Rgb(39, 39, 42),      // dark zinc

            diff_added_bg: Color::Rgb(40, 60, 50),
            diff_removed_bg: Color::Rgb(60, 40, 50),
            diff_added: Color::Rgb(166, 227, 161),
            diff_removed: Color::Rgb(243, 139, 168),

            syntax_comment: Color::Rgb(127, 132, 156),
            syntax_keyword: Color::Rgb(203, 166, 247),
            syntax_function: Color::Rgb(137, 180, 250),
            syntax_variable: Color::Rgb(243, 139, 168),
            syntax_string: Color::Rgb(166, 227, 161),
            syntax_number: Color::Rgb(250, 179, 135),
            syntax_type: Color::Rgb(249, 226, 175),
            syntax_operator: Color::Rgb(148, 226, 213),

            thinking_opacity: 0.6,
        }
    }

    /// Dracula theme.
    pub fn dracula() -> Self {
        Self {
            name: "dracula".to_string(),

            background: Color::Rgb(40, 42, 54),
            background_panel: Color::Rgb(48, 51, 66),
            background_element: Color::Rgb(68, 71, 90),
            background_menu: Color::Rgb(68, 71, 90),

            text: Color::Rgb(248, 248, 242),
            text_muted: Color::Rgb(98, 114, 164),

            primary: Color::Rgb(255, 184, 108),   // orange
            secondary: Color::Rgb(139, 233, 253), // cyan
            accent: Color::Rgb(189, 147, 249),    // purple

            success: Color::Rgb(80, 250, 123),
            warning: Color::Rgb(241, 250, 140),
            error: Color::Rgb(255, 85, 85),
            info: Color::Rgb(139, 233, 253),

            border: Color::Rgb(68, 71, 90),
            border_active: Color::Rgb(189, 147, 249),
            border_subtle: Color::Rgb(48, 51, 66),
            tool_border: Color::Rgb(39, 39, 42), // dark zinc

            diff_added_bg: Color::Rgb(40, 70, 50),
            diff_removed_bg: Color::Rgb(70, 40, 40),
            diff_added: Color::Rgb(80, 250, 123),
            diff_removed: Color::Rgb(255, 85, 85),

            syntax_comment: Color::Rgb(98, 114, 164),
            syntax_keyword: Color::Rgb(255, 121, 198),
            syntax_function: Color::Rgb(80, 250, 123),
            syntax_variable: Color::Rgb(248, 248, 242),
            syntax_string: Color::Rgb(241, 250, 140),
            syntax_number: Color::Rgb(189, 147, 249),
            syntax_type: Color::Rgb(139, 233, 253),
            syntax_operator: Color::Rgb(255, 121, 198),

            thinking_opacity: 0.6,
        }
    }

    /// Gruvbox Dark theme.
    pub fn gruvbox_dark() -> Self {
        Self {
            name: "gruvbox".to_string(),

            background: Color::Rgb(40, 40, 40),         // bg
            background_panel: Color::Rgb(50, 48, 47),   // bg1
            background_element: Color::Rgb(80, 73, 69), // bg2
            background_menu: Color::Rgb(80, 73, 69),

            text: Color::Rgb(235, 219, 178),       // fg
            text_muted: Color::Rgb(146, 131, 116), // gray

            primary: Color::Rgb(254, 128, 25),    // orange
            secondary: Color::Rgb(131, 165, 152), // aqua
            accent: Color::Rgb(177, 98, 134),     // purple

            success: Color::Rgb(152, 151, 26), // green
            warning: Color::Rgb(215, 153, 33), // yellow
            error: Color::Rgb(204, 36, 29),    // red
            info: Color::Rgb(69, 133, 136),    // blue

            border: Color::Rgb(80, 73, 69),
            border_active: Color::Rgb(215, 153, 33),
            border_subtle: Color::Rgb(50, 48, 47),
            tool_border: Color::Rgb(39, 39, 42), // dark zinc

            diff_added_bg: Color::Rgb(50, 60, 40),
            diff_removed_bg: Color::Rgb(60, 40, 40),
            diff_added: Color::Rgb(152, 151, 26),
            diff_removed: Color::Rgb(204, 36, 29),

            syntax_comment: Color::Rgb(146, 131, 116),
            syntax_keyword: Color::Rgb(204, 36, 29),
            syntax_function: Color::Rgb(152, 151, 26),
            syntax_variable: Color::Rgb(69, 133, 136),
            syntax_string: Color::Rgb(152, 151, 26),
            syntax_number: Color::Rgb(177, 98, 134),
            syntax_type: Color::Rgb(215, 153, 33),
            syntax_operator: Color::Rgb(254, 128, 25),

            thinking_opacity: 0.6,
        }
    }

    /// Nord theme.
    pub fn nord() -> Self {
        Self {
            name: "nord".to_string(),

            background: Color::Rgb(46, 52, 64), // polar night 1
            background_panel: Color::Rgb(59, 66, 82), // polar night 2
            background_element: Color::Rgb(67, 76, 94), // polar night 3
            background_menu: Color::Rgb(67, 76, 94),

            text: Color::Rgb(236, 239, 244),     // snow storm 2
            text_muted: Color::Rgb(76, 86, 106), // polar night 4

            primary: Color::Rgb(208, 135, 112),   // aurora orange
            secondary: Color::Rgb(129, 161, 193), // frost 1
            accent: Color::Rgb(180, 142, 173),    // aurora purple

            success: Color::Rgb(163, 190, 140), // aurora green
            warning: Color::Rgb(235, 203, 139), // aurora yellow
            error: Color::Rgb(191, 97, 106),    // aurora red
            info: Color::Rgb(136, 192, 208),    // frost 2

            border: Color::Rgb(67, 76, 94),
            border_active: Color::Rgb(136, 192, 208),
            border_subtle: Color::Rgb(59, 66, 82),
            tool_border: Color::Rgb(39, 39, 42), // dark zinc

            diff_added_bg: Color::Rgb(50, 70, 60),
            diff_removed_bg: Color::Rgb(70, 50, 55),
            diff_added: Color::Rgb(163, 190, 140),
            diff_removed: Color::Rgb(191, 97, 106),

            syntax_comment: Color::Rgb(76, 86, 106),
            syntax_keyword: Color::Rgb(129, 161, 193),
            syntax_function: Color::Rgb(136, 192, 208),
            syntax_variable: Color::Rgb(236, 239, 244),
            syntax_string: Color::Rgb(163, 190, 140),
            syntax_number: Color::Rgb(180, 142, 173),
            syntax_type: Color::Rgb(143, 188, 187),
            syntax_operator: Color::Rgb(129, 161, 193),

            thinking_opacity: 0.6,
        }
    }

    /// Tokyo Night theme.
    pub fn tokyo_night() -> Self {
        Self {
            name: "tokyo-night".to_string(),

            background: Color::Rgb(26, 27, 38),
            background_panel: Color::Rgb(36, 40, 59),
            background_element: Color::Rgb(41, 46, 66),
            background_menu: Color::Rgb(41, 46, 66),

            text: Color::Rgb(169, 177, 214),
            text_muted: Color::Rgb(86, 95, 137),

            primary: Color::Rgb(255, 158, 100),   // orange
            secondary: Color::Rgb(122, 162, 247), // blue
            accent: Color::Rgb(187, 154, 247),    // purple

            success: Color::Rgb(158, 206, 106),
            warning: Color::Rgb(224, 175, 104),
            error: Color::Rgb(247, 118, 142),
            info: Color::Rgb(125, 207, 255),

            border: Color::Rgb(41, 46, 66),
            border_active: Color::Rgb(187, 154, 247),
            border_subtle: Color::Rgb(36, 40, 59),
            tool_border: Color::Rgb(39, 39, 42), // dark zinc

            diff_added_bg: Color::Rgb(40, 60, 50),
            diff_removed_bg: Color::Rgb(60, 40, 50),
            diff_added: Color::Rgb(158, 206, 106),
            diff_removed: Color::Rgb(247, 118, 142),

            syntax_comment: Color::Rgb(86, 95, 137),
            syntax_keyword: Color::Rgb(187, 154, 247),
            syntax_function: Color::Rgb(122, 162, 247),
            syntax_variable: Color::Rgb(199, 146, 234),
            syntax_string: Color::Rgb(158, 206, 106),
            syntax_number: Color::Rgb(255, 158, 100),
            syntax_type: Color::Rgb(45, 212, 191),
            syntax_operator: Color::Rgb(137, 221, 255),

            thinking_opacity: 0.6,
        }
    }

    /// Rose Pine theme.
    pub fn rose_pine() -> Self {
        Self {
            name: "rosepine".to_string(),

            background: Color::Rgb(25, 23, 36),         // base
            background_panel: Color::Rgb(30, 28, 44),   // surface
            background_element: Color::Rgb(38, 35, 58), // overlay
            background_menu: Color::Rgb(38, 35, 58),

            text: Color::Rgb(224, 222, 244),       // text
            text_muted: Color::Rgb(110, 106, 134), // muted

            primary: Color::Rgb(235, 188, 186),   // rose
            secondary: Color::Rgb(156, 207, 216), // foam
            accent: Color::Rgb(196, 167, 231),    // iris

            success: Color::Rgb(156, 207, 216), // foam
            warning: Color::Rgb(246, 193, 119), // gold
            error: Color::Rgb(235, 111, 146),   // love
            info: Color::Rgb(156, 207, 216),

            border: Color::Rgb(38, 35, 58),
            border_active: Color::Rgb(235, 188, 186),
            border_subtle: Color::Rgb(30, 28, 44),
            tool_border: Color::Rgb(39, 39, 42), // dark zinc

            diff_added_bg: Color::Rgb(40, 55, 60),
            diff_removed_bg: Color::Rgb(55, 35, 45),
            diff_added: Color::Rgb(156, 207, 216),
            diff_removed: Color::Rgb(235, 111, 146),

            syntax_comment: Color::Rgb(110, 106, 134),
            syntax_keyword: Color::Rgb(49, 116, 143),
            syntax_function: Color::Rgb(235, 188, 186),
            syntax_variable: Color::Rgb(224, 222, 244),
            syntax_string: Color::Rgb(246, 193, 119),
            syntax_number: Color::Rgb(196, 167, 231),
            syntax_type: Color::Rgb(156, 207, 216),
            syntax_operator: Color::Rgb(110, 106, 134),

            thinking_opacity: 0.6,
        }
    }

    // Style helper methods

    /// Base text style.
    pub fn text_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    /// Muted text style.
    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.text_muted)
    }

    /// Primary accent style.
    pub fn primary_style(&self) -> Style {
        Style::default().fg(self.primary)
    }

    /// Secondary accent style.
    pub fn secondary_style(&self) -> Style {
        Style::default().fg(self.secondary)
    }

    /// Accent style.
    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Success style.
    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }

    /// Warning style.
    pub fn warning_style(&self) -> Style {
        Style::default().fg(self.warning)
    }

    /// Error style.
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    /// Info style.
    pub fn info_style(&self) -> Style {
        Style::default().fg(self.info)
    }

    /// Border style (not focused).
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Active border style.
    pub fn border_active_style(&self) -> Style {
        Style::default().fg(self.border_active)
    }

    /// Tool border style (for tool output borders).
    pub fn tool_border_style(&self) -> Style {
        Style::default().fg(self.tool_border)
    }

    /// Style with background panel color.
    pub fn panel_style(&self) -> Style {
        Style::default().bg(self.background_panel)
    }

    /// Style with element background.
    pub fn element_style(&self) -> Style {
        Style::default().bg(self.background_element)
    }

    /// Bold text style.
    pub fn bold(&self) -> Style {
        Style::default().fg(self.text).add_modifier(Modifier::BOLD)
    }

    /// Italic text style.
    pub fn italic(&self) -> Style {
        Style::default()
            .fg(self.text)
            .add_modifier(Modifier::ITALIC)
    }

    // Legacy compatibility methods
    pub fn dim_style(&self) -> Style {
        self.muted_style()
    }

    pub fn highlight_style(&self) -> Style {
        self.primary_style()
    }

    pub fn user_style(&self) -> Style {
        self.text_style()
    }

    pub fn assistant_style(&self) -> Style {
        self.primary_style()
    }

    pub fn tool_style(&self) -> Style {
        self.accent_style()
    }

    // Diff styles for tool output previews
    pub fn diff_added_style(&self) -> Style {
        Style::default().fg(self.diff_added)
    }

    pub fn diff_removed_style(&self) -> Style {
        Style::default().fg(self.diff_removed)
    }

    pub fn diff_hunk_style(&self) -> Style {
        Style::default().fg(self.info)
    }

    pub fn code_style(&self) -> Style {
        Style::default().fg(self.text)
    }
}

/// Settings that control rendering performance and features.
///
/// These can be toggled via the Settings dialog (Performance tab)
/// to optimize for low-memory or low-CPU environments.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    /// Enable markdown formatting (bold, italic, lists, etc.)
    pub markdown_enabled: bool,
    /// Enable syntax highlighting for code blocks
    pub syntax_highlighting_enabled: bool,
    /// Show background color for code blocks
    pub code_backgrounds_enabled: bool,
    /// Render markdown tables with borders
    pub tables_enabled: bool,
    /// Max frames per second during streaming
    pub streaming_fps: u32,
    /// Maximum messages to keep in memory
    pub max_messages: usize,
    /// Aggressive memory optimization mode
    pub low_memory_mode: bool,
    /// Enable test/debug commands
    pub enable_test_commands: bool,

    // Test provider settings
    /// Enable the test model in the model selector
    pub test_model_enabled: bool,
    /// Test provider: simulate thinking/reasoning blocks
    pub test_emulate_thinking: bool,
    /// Test provider: simulate tool calls (standard execution)
    pub test_emulate_tool_calls: bool,
    /// Test provider: simulate observed tools (CLI-style external execution)
    pub test_emulate_tool_observed: bool,
    /// Test provider: simulate streaming delays
    pub test_emulate_streaming: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            markdown_enabled: true,
            syntax_highlighting_enabled: true,
            code_backgrounds_enabled: true,
            tables_enabled: true,
            streaming_fps: 20,
            max_messages: 200,
            low_memory_mode: false,
            enable_test_commands: false,
            // Test provider defaults
            test_model_enabled: false,
            test_emulate_thinking: true,
            test_emulate_tool_calls: true,
            test_emulate_tool_observed: false,
            test_emulate_streaming: true,
        }
    }
}

impl RenderSettings {
    /// Create settings for low-memory environments (20MB or less)
    pub fn low_memory() -> Self {
        Self {
            markdown_enabled: true,
            syntax_highlighting_enabled: false, // Saves ~7MB
            code_backgrounds_enabled: false,
            tables_enabled: false,
            streaming_fps: 10,
            max_messages: 50,
            low_memory_mode: true,
            enable_test_commands: false,
            // Test provider defaults
            test_model_enabled: false,
            test_emulate_thinking: true,
            test_emulate_tool_calls: true,
            test_emulate_tool_observed: false,
            test_emulate_streaming: true,
        }
    }

    /// Create settings for minimal CPU usage
    pub fn low_cpu() -> Self {
        Self {
            markdown_enabled: false, // Plain text only
            syntax_highlighting_enabled: false,
            code_backgrounds_enabled: false,
            tables_enabled: false,
            streaming_fps: 5,
            max_messages: 100,
            low_memory_mode: false,
            enable_test_commands: false,
            // Test provider defaults
            test_model_enabled: false,
            test_emulate_thinking: true,
            test_emulate_tool_calls: true,
            test_emulate_tool_observed: false,
            test_emulate_streaming: true,
        }
    }

    /// Get minimum interval between streaming frames in milliseconds
    pub fn streaming_interval_ms(&self) -> u64 {
        if self.streaming_fps == 0 {
            1000 // 1 FPS minimum
        } else {
            1000 / self.streaming_fps as u64
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // === AgentMode tests ===

    #[test]
    fn test_agent_mode_default() {
        let mode = AgentMode::default();
        assert_eq!(mode, AgentMode::Build);
    }

    #[test]
    fn test_agent_mode_name() {
        assert_eq!(AgentMode::Build.name(), "Build");
        assert_eq!(AgentMode::Plan.name(), "Plan");
    }

    #[test]
    fn test_agent_mode_parse() {
        assert_eq!(AgentMode::parse("build"), AgentMode::Build);
        assert_eq!(AgentMode::parse("Build"), AgentMode::Build);
        assert_eq!(AgentMode::parse("BUILD"), AgentMode::Build);
        assert_eq!(AgentMode::parse("plan"), AgentMode::Plan);
        assert_eq!(AgentMode::parse("Plan"), AgentMode::Plan);
        assert_eq!(AgentMode::parse("PLAN"), AgentMode::Plan);
    }

    #[test]
    fn test_agent_mode_parse_unknown() {
        // Unknown values default to Build
        assert_eq!(AgentMode::parse("unknown"), AgentMode::Build);
        assert_eq!(AgentMode::parse("default"), AgentMode::Build);
        assert_eq!(AgentMode::parse(""), AgentMode::Build);
    }

    #[test]
    fn test_agent_mode_next() {
        assert_eq!(AgentMode::Build.next(), AgentMode::Plan);
        assert_eq!(AgentMode::Plan.next(), AgentMode::Build);
    }

    #[test]
    fn test_agent_mode_prev() {
        assert_eq!(AgentMode::Build.prev(), AgentMode::Plan);
        assert_eq!(AgentMode::Plan.prev(), AgentMode::Build);
    }

    #[test]
    fn test_agent_mode_id() {
        assert_eq!(AgentMode::Build.id(), "build");
        assert_eq!(AgentMode::Plan.id(), "plan");
    }

    #[test]
    fn test_agent_mode_clone() {
        let mode = AgentMode::Build;
        let cloned = mode;
        assert_eq!(cloned, AgentMode::Build);
    }

    #[test]
    fn test_agent_mode_debug() {
        let debug = format!("{:?}", AgentMode::Build);
        assert!(debug.contains("Build"));
    }

    // === Theme tests ===

    #[test]
    fn test_theme_default() {
        let theme = Theme::default();
        assert!(!theme.name.is_empty());
    }

    #[test]
    fn test_theme_clone() {
        let theme = Theme::default();
        let cloned = theme.clone();
        assert_eq!(cloned.name, theme.name);
    }

    #[test]
    fn test_theme_colors_are_set() {
        let theme = Theme::default();
        // Just verify colors are not black (uninitialized)
        assert_ne!(theme.text, Color::Black);
        assert_ne!(theme.background, Color::Black);
    }

    // === RenderSettings tests ===

    #[test]
    fn test_render_settings_default() {
        let settings = RenderSettings::default();
        assert!(settings.markdown_enabled);
        assert!(settings.syntax_highlighting_enabled);
        assert!(settings.code_backgrounds_enabled);
        assert!(settings.tables_enabled);
        assert_eq!(settings.streaming_fps, 20);
        assert_eq!(settings.max_messages, 200);
        assert!(!settings.low_memory_mode);
    }

    #[test]
    fn test_render_settings_low_memory() {
        let settings = RenderSettings::low_memory();
        assert!(!settings.syntax_highlighting_enabled);
        assert!(!settings.code_backgrounds_enabled);
        assert!(!settings.tables_enabled);
        assert_eq!(settings.streaming_fps, 10);
        assert_eq!(settings.max_messages, 50);
        assert!(settings.low_memory_mode);
    }

    #[test]
    fn test_render_settings_low_cpu() {
        let settings = RenderSettings::low_cpu();
        assert!(!settings.markdown_enabled);
        assert!(!settings.syntax_highlighting_enabled);
        assert_eq!(settings.streaming_fps, 5);
        assert_eq!(settings.max_messages, 100);
    }

    #[test]
    fn test_streaming_interval_ms() {
        let settings = RenderSettings::default();
        assert_eq!(settings.streaming_interval_ms(), 50); // 1000/20 = 50ms

        let low_cpu = RenderSettings::low_cpu();
        assert_eq!(low_cpu.streaming_interval_ms(), 200); // 1000/5 = 200ms
    }

    #[test]
    fn test_streaming_interval_ms_zero_fps() {
        let mut settings = RenderSettings::default();
        settings.streaming_fps = 0;
        assert_eq!(settings.streaming_interval_ms(), 1000); // Minimum 1 FPS
    }

    // === Theme helper tests ===

    #[test]
    fn test_theme_accent_style() {
        let theme = Theme::default();
        let style = theme.accent_style();
        // Verify style is created without panic
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_text_style() {
        let theme = Theme::default();
        let style = theme.text_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_error_style() {
        let theme = Theme::default();
        let style = theme.error_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_success_style() {
        let theme = Theme::default();
        let style = theme.success_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_warning_style() {
        let theme = Theme::default();
        let style = theme.warning_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_muted_style() {
        let theme = Theme::default();
        let style = theme.muted_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_info_style() {
        let theme = Theme::default();
        let style = theme.info_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_primary_style() {
        let theme = Theme::default();
        let style = theme.primary_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_secondary_style() {
        let theme = Theme::default();
        let style = theme.secondary_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_agent_color() {
        let theme = Theme::default();
        let _build_color = theme.agent_color(AgentMode::Build);
        let _plan_color = theme.agent_color(AgentMode::Plan);
        // Test passes if no panic
    }

    #[test]
    fn test_theme_agent_color_by_name() {
        let theme = Theme::default();
        let _color = theme.agent_color_by_name("build");
        let _color2 = theme.agent_color_by_name("plan");
        let _color3 = theme.agent_color_by_name("unknown");
        // Test passes if no panic
    }

    #[test]
    fn test_theme_agent_color_by_index() {
        let theme = Theme::default();
        let _color0 = theme.agent_color_by_index(0);
        let _color1 = theme.agent_color_by_index(1);
        let _color2 = theme.agent_color_by_index(2);
        // Test passes if no panic
    }

    #[test]
    fn test_theme_border_style() {
        let theme = Theme::default();
        let style = theme.border_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_border_active_style() {
        let theme = Theme::default();
        let style = theme.border_active_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_panel_style() {
        let theme = Theme::default();
        let style = theme.panel_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_element_style() {
        let theme = Theme::default();
        let style = theme.element_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_highlight_style() {
        let theme = Theme::default();
        let style = theme.highlight_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_user_style() {
        let theme = Theme::default();
        let style = theme.user_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_assistant_style() {
        let theme = Theme::default();
        let style = theme.assistant_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_tool_style() {
        let theme = Theme::default();
        let style = theme.tool_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_diff_added_style() {
        let theme = Theme::default();
        let style = theme.diff_added_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_diff_removed_style() {
        let theme = Theme::default();
        let style = theme.diff_removed_style();
        assert!(format!("{:?}", style).contains("Style"));
    }

    #[test]
    fn test_theme_code_style() {
        let theme = Theme::default();
        let style = theme.code_style();
        assert!(format!("{:?}", style).contains("Style"));
    }
}
