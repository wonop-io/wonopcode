//! Syntax highlighting for code blocks.
//!
//! Uses syntect for language-aware syntax highlighting.

use once_cell::sync::Lazy;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use crate::theme::{RenderSettings, Theme};

/// Lazily loaded syntax set.
static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

/// Lazily loaded theme set.
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);

/// Languages that need custom highlighting (not in syntect defaults).
const CUSTOM_HIGHLIGHT_LANGS: &[&str] = &["toml", "ini", "cfg", "conf", "env", "lock"];

/// Highlight code with syntax highlighting.
///
/// Returns styled lines for the given code and language.
pub fn highlight_code(code: &str, language: &str, theme: &Theme) -> Vec<Line<'static>> {
    let lang_lower = language.to_lowercase();

    // Check if this language needs custom highlighting
    if CUSTOM_HIGHLIGHT_LANGS.contains(&lang_lower.as_str()) {
        return highlight_config_file(code, theme);
    }

    // Try to find the syntax for the language
    let syntax = SYNTAX_SET
        .find_syntax_by_token(language)
        .or_else(|| SYNTAX_SET.find_syntax_by_extension(language))
        .or_else(|| {
            // Try common aliases and map to syntect names
            let lang = match lang_lower.as_str() {
                "js" | "mjs" | "cjs" => "JavaScript",
                "ts" | "mts" | "cts" => "JavaScript", // syntect doesn't have TS, use JS
                "py" | "python3" | "pyw" => "Python",
                "rb" => "Ruby",
                "rs" => "Rust",
                "sh" | "bash" | "shell" | "zsh" | "fish" => "Bourne Again Shell (bash)",
                "yml" => "YAML",
                "md" | "markdown" => "Markdown",
                "dockerfile" => "Dockerfile",
                "makefile" | "make" | "mk" => "Makefile",
                "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "C++",
                "c#" | "csharp" | "cs" => "C#",
                "objc" | "objective-c" | "m" => "Objective-C",
                "jsx" | "tsx" => "JavaScript",
                "htm" => "HTML",
                "json5" | "jsonc" => "JSON",
                "scss" | "sass" => "CSS",
                "sql" | "mysql" | "postgresql" | "sqlite" => "SQL",
                "pl" | "pm" => "Perl",
                "hs" => "Haskell",
                "ex" | "exs" => "Ruby",   // Elixir looks similar to Ruby
                "kt" | "kts" => "Java",   // Kotlin similar to Java
                "swift" => "Objective-C", // Swift similar to ObjC
                "clj" | "cljs" | "cljc" => "Clojure",
                "erl" | "hrl" => "Erlang",
                "elm" => "Haskell", // Elm similar to Haskell
                "vue" | "svelte" => "HTML",
                "graphql" | "gql" => "JavaScript",
                _ => language,
            };
            SYNTAX_SET
                .find_syntax_by_name(lang)
                .or_else(|| SYNTAX_SET.find_syntax_by_token(lang))
        })
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    // Use base16-eighties for parsing - it has good scope coverage
    // We'll map the colors to our theme colors afterward
    let syntect_theme = THEME_SET
        .themes
        .get("base16-eighties.dark")
        .unwrap_or(&THEME_SET.themes["base16-ocean.dark"]);

    let mut highlighter = HighlightLines::new(syntax, syntect_theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, &SYNTAX_SET);

        match ranges {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        // Map syntect colors to our theme's syntax colors
                        let fg = map_syntect_to_theme(style.foreground, theme);
                        let mut ratatui_style = Style::default().fg(fg);

                        if style.font_style.contains(FontStyle::BOLD) {
                            ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
                        }
                        if style.font_style.contains(FontStyle::ITALIC) {
                            ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
                        }
                        if style.font_style.contains(FontStyle::UNDERLINE) {
                            ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
                        }

                        // Remove trailing newline for clean display
                        let text = text.trim_end_matches('\n').to_string();
                        Span::styled(text, ratatui_style)
                    })
                    .collect();

                lines.push(Line::from(spans));
            }
            Err(_) => {
                // Fallback to plain text on error
                let text = line.trim_end_matches('\n').to_string();
                lines.push(Line::from(Span::styled(
                    text,
                    Style::default().fg(theme.text_muted),
                )));
            }
        }
    }

    lines
}

/// Highlight code with syntax highlighting and settings support.
///
/// If syntax highlighting is disabled in settings, returns plain text.
pub fn highlight_code_with_settings(
    code: &str,
    language: &str,
    theme: &Theme,
    settings: &RenderSettings,
) -> Vec<Line<'static>> {
    // If syntax highlighting is disabled, return plain text
    if !settings.syntax_highlighting_enabled {
        return code
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), theme.text_style())))
            .collect();
    }

    // Use the regular highlighting function
    highlight_code(code, language, theme)
}

/// Map syntect theme colors to our app theme colors.
///
/// base16-eighties palette (used for semantic detection):
/// - Gray tones (03-04): comments, muted text
/// - Light tones (05-07): regular text  
/// - Red (08): variables, tags
/// - Orange (09): numbers, constants
/// - Yellow (0A): classes, types
/// - Green (0B): strings
/// - Cyan (0C): regex, escape sequences
/// - Blue (0D): functions, methods
/// - Purple (0E): keywords, storage
/// - Brown (0F): deprecated
fn map_syntect_to_theme(color: syntect::highlighting::Color, theme: &Theme) -> Color {
    let (r, g, b) = (color.r, color.g, color.b);

    // Gray tones (comments) - low saturation
    if is_gray(r, g, b) && r < 180 {
        return theme.syntax_comment;
    }

    // Very light (near white) - regular text
    if r > 200 && g > 200 && b > 200 {
        return theme.text;
    }

    // Red tones (variables, tags) - #f2777a
    if r > 200 && g < 140 && b < 160 {
        return theme.syntax_variable;
    }

    // Orange tones (numbers, constants) - #f99157
    if r > 220 && g > 120 && g < 180 && b < 120 {
        return theme.syntax_number;
    }

    // Yellow tones (types, classes) - #ffcc66
    if r > 220 && g > 180 && b < 140 {
        return theme.syntax_type;
    }

    // Green tones (strings) - #99cc99
    if g > 170 && r < 180 && b < 180 {
        return theme.syntax_string;
    }

    // Cyan tones (regex, escape) - #66cccc
    if g > 180 && b > 180 && r < 140 {
        return theme.syntax_operator;
    }

    // Blue tones (functions) - #6699cc
    if b > 170 && r < 140 && g > 120 && g < 180 {
        return theme.syntax_function;
    }

    // Purple/magenta tones (keywords) - #cc99cc
    if r > 170 && b > 170 && g < 170 {
        return theme.syntax_keyword;
    }

    // Default to regular text
    theme.text
}

/// Check if a color is a shade of gray.
fn is_gray(r: u8, g: u8, b: u8) -> bool {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    (max - min) < 25
}

/// Custom syntax highlighting for TOML, INI, and config files.
/// Provides rich, colorful highlighting for these common formats.
fn highlight_config_file(code: &str, theme: &Theme) -> Vec<Line<'static>> {
    // Vibrant color palette for config files
    let colors = ConfigColors::for_theme(theme);

    let mut lines = Vec::new();

    for line in code.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Comment lines
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default()
                    .fg(colors.comment)
                    .add_modifier(Modifier::ITALIC),
            )));
            continue;
        }

        // Section headers [section] or [[array]]
        if trimmed.starts_with('[') {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default()
                    .fg(colors.section)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Key = value pairs
        if let Some(eq_pos) = line.find('=') {
            let (key_part, rest) = line.split_at(eq_pos);
            let value_part = &rest[1..]; // Skip the '='

            let mut spans = Vec::new();

            // Key (before =)
            spans.push(Span::styled(
                key_part.to_string(),
                Style::default().fg(colors.key),
            ));

            // Equals sign
            spans.push(Span::styled(
                "=".to_string(),
                Style::default().fg(colors.operator),
            ));

            // Value - determine type and color accordingly
            let value_trimmed = value_part.trim();
            let value_spans = highlight_config_value(value_part, value_trimmed, &colors);
            spans.extend(value_spans);

            lines.push(Line::from(spans));
            continue;
        }

        // Fallback - just show as plain text
        lines.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(colors.text),
        )));
    }

    lines
}

/// Color palette for config file highlighting.
/// Uses the theme's syntax colors for consistency.
struct ConfigColors {
    comment: Color,
    section: Color,
    key: Color,
    operator: Color,
    string: Color,
    number: Color,
    boolean: Color,
    array_bracket: Color,
    text: Color,
}

impl ConfigColors {
    /// Create config colors from the app theme.
    fn for_theme(theme: &Theme) -> Self {
        Self {
            comment: theme.syntax_comment,
            section: theme.syntax_keyword, // Sections are like keywords
            key: theme.syntax_variable,    // Keys are like variables
            operator: theme.syntax_operator,
            string: theme.syntax_string,
            number: theme.syntax_number,
            boolean: theme.syntax_keyword, // Booleans are keyword-like
            array_bracket: theme.syntax_type, // Brackets like type delimiters
            text: theme.text,
        }
    }
}

/// Highlight a config file value with appropriate colors
fn highlight_config_value(
    full_value: &str,
    trimmed: &str,
    colors: &ConfigColors,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    // Preserve leading whitespace
    let leading_ws = &full_value[..full_value.len() - full_value.trim_start().len()];
    if !leading_ws.is_empty() {
        spans.push(Span::raw(leading_ws.to_string()));
    }

    // String values (quoted)
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        spans.push(Span::styled(
            trimmed.to_string(),
            Style::default().fg(colors.string),
        ));
        return spans;
    }

    // Multi-line string start
    if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
        spans.push(Span::styled(
            trimmed.to_string(),
            Style::default().fg(colors.string),
        ));
        return spans;
    }

    // Boolean values
    if trimmed == "true" || trimmed == "false" {
        spans.push(Span::styled(
            trimmed.to_string(),
            Style::default()
                .fg(colors.boolean)
                .add_modifier(Modifier::BOLD),
        ));
        return spans;
    }

    // Number values (integers and floats)
    if trimmed.parse::<f64>().is_ok()
        || trimmed.starts_with("0x")
        || trimmed.starts_with("0o")
        || trimmed.starts_with("0b")
    {
        spans.push(Span::styled(
            trimmed.to_string(),
            Style::default().fg(colors.number),
        ));
        return spans;
    }

    // Array values [...] - highlight brackets and contents
    if trimmed.starts_with('[') {
        // For simplicity, just color the whole array with mixed styling
        let mut in_string = false;
        let mut current = String::new();
        let mut current_style = Style::default().fg(colors.array_bracket);

        for ch in trimmed.chars() {
            match ch {
                '"' | '\'' => {
                    if !current.is_empty() {
                        spans.push(Span::styled(current.clone(), current_style));
                        current.clear();
                    }
                    in_string = !in_string;
                    current_style = Style::default().fg(colors.string);
                    current.push(ch);
                    if !in_string {
                        spans.push(Span::styled(current.clone(), current_style));
                        current.clear();
                        current_style = Style::default().fg(colors.text);
                    }
                }
                '[' | ']' if !in_string => {
                    if !current.is_empty() {
                        spans.push(Span::styled(current.clone(), current_style));
                        current.clear();
                    }
                    spans.push(Span::styled(
                        ch.to_string(),
                        Style::default()
                            .fg(colors.array_bracket)
                            .add_modifier(Modifier::BOLD),
                    ));
                    current_style = Style::default().fg(colors.text);
                }
                ',' if !in_string => {
                    if !current.is_empty() {
                        // Try to detect if current is a number
                        let style = if current.trim().parse::<f64>().is_ok() {
                            Style::default().fg(colors.number)
                        } else if current.trim() == "true" || current.trim() == "false" {
                            Style::default().fg(colors.boolean)
                        } else {
                            current_style
                        };
                        spans.push(Span::styled(current.clone(), style));
                        current.clear();
                    }
                    spans.push(Span::styled(
                        ",".to_string(),
                        Style::default().fg(colors.operator),
                    ));
                    current_style = Style::default().fg(colors.text);
                }
                _ => {
                    current.push(ch);
                }
            }
        }

        if !current.is_empty() {
            let style = if current.trim().parse::<f64>().is_ok() {
                Style::default().fg(colors.number)
            } else if current.trim() == "true" || current.trim() == "false" {
                Style::default().fg(colors.boolean)
            } else {
                current_style
            };
            spans.push(Span::styled(current, style));
        }

        return spans;
    }

    // Inline table {...}
    if trimmed.starts_with('{') {
        spans.push(Span::styled(
            trimmed.to_string(),
            Style::default().fg(colors.text),
        ));
        return spans;
    }

    // Fallback - plain text
    spans.push(Span::styled(
        trimmed.to_string(),
        Style::default().fg(colors.text),
    ));

    spans
}

/// Highlight a diff with appropriate colors and syntax highlighting for code content.
pub fn highlight_diff(diff: &str, theme: &Theme) -> Vec<Line<'static>> {
    // Try to detect the language from file headers
    let language = detect_diff_language(diff);
    highlight_diff_with_language(diff, theme, language.as_deref())
}

/// Highlight a diff with a specific language for syntax highlighting.
pub fn highlight_diff_with_language(
    diff: &str,
    theme: &Theme,
    language: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            // File headers - muted style
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.text_muted),
            )));
        } else if line.starts_with("@@") {
            // Hunk headers - info style
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.info),
            )));
        } else if let Some(content) = line.strip_prefix('+') {
            // Added lines - syntax highlight the content after the prefix
            let highlighted = highlight_diff_line_content(content, language, theme);
            let mut spans = vec![Span::styled(
                "+".to_string(),
                Style::default()
                    .fg(theme.diff_added)
                    .bg(theme.diff_added_bg),
            )];
            // Apply diff background to highlighted spans
            for span in highlighted {
                spans.push(Span::styled(
                    span.content.to_string(),
                    span.style.bg(theme.diff_added_bg),
                ));
            }
            lines.push(Line::from(spans));
        } else if let Some(content) = line.strip_prefix('-') {
            // Removed lines - syntax highlight the content after the prefix
            let highlighted = highlight_diff_line_content(content, language, theme);
            let mut spans = vec![Span::styled(
                "-".to_string(),
                Style::default()
                    .fg(theme.diff_removed)
                    .bg(theme.diff_removed_bg),
            )];
            // Apply diff background to highlighted spans
            for span in highlighted {
                spans.push(Span::styled(
                    span.content.to_string(),
                    span.style.bg(theme.diff_removed_bg),
                ));
            }
            lines.push(Line::from(spans));
        } else if let Some(content) = line.strip_prefix(' ') {
            // Context lines - syntax highlight but keep muted
            let highlighted = highlight_diff_line_content(content, language, theme);
            let mut spans = vec![Span::styled(" ".to_string(), Style::default())];
            spans.extend(highlighted);
            lines.push(Line::from(spans));
        } else {
            // Other lines (like "...", "\ No newline", etc.)
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.text_muted),
            )));
        }
    }

    lines
}

/// Detect the programming language from diff file headers.
fn detect_diff_language(diff: &str) -> Option<String> {
    for line in diff.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            // Extract filename from header like "+++ b/src/main.rs"
            let path = line
                .strip_prefix("+++ ")
                .or_else(|| line.strip_prefix("--- "))
                .unwrap_or("");

            // Remove common prefixes like "a/" or "b/"
            let path = path
                .strip_prefix("a/")
                .or_else(|| path.strip_prefix("b/"))
                .unwrap_or(path);

            // Get extension
            if let Some(ext) = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
            {
                return Some(ext.to_lowercase());
            }
        }
    }
    None
}

/// Highlight a single line of code content for use in diffs.
/// Preserves all whitespace.
fn highlight_diff_line_content(
    content: &str,
    language: Option<&str>,
    theme: &Theme,
) -> Vec<Span<'static>> {
    // If no language or empty content, return as plain text preserving whitespace
    if content.is_empty() {
        return vec![Span::styled(String::new(), Style::default())];
    }

    let lang = match language {
        Some(l) => l,
        None => {
            // No language detected, return plain text
            return vec![Span::styled(
                content.to_string(),
                Style::default().fg(theme.text),
            )];
        }
    };

    // Try to find the syntax for the language
    let syntax = SYNTAX_SET
        .find_syntax_by_token(lang)
        .or_else(|| SYNTAX_SET.find_syntax_by_extension(lang))
        .or_else(|| {
            // Try common aliases
            let mapped = match lang {
                "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
                "ts" | "mts" | "cts" | "tsx" => "JavaScript",
                "py" | "python3" | "pyw" => "Python",
                "rb" => "Ruby",
                "rs" => "Rust",
                "sh" | "bash" | "shell" | "zsh" | "fish" => "Bourne Again Shell (bash)",
                "yml" => "YAML",
                "md" | "markdown" => "Markdown",
                "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "h" => "C++",
                "c" => "C",
                "cs" | "csharp" => "C#",
                "go" => "Go",
                "java" => "Java",
                "kt" | "kts" => "Kotlin",
                "swift" => "Swift",
                "php" => "PHP",
                "sql" => "SQL",
                "html" | "htm" => "HTML",
                "css" | "scss" | "sass" => "CSS",
                "json" | "jsonc" => "JSON",
                "xml" => "XML",
                _ => lang,
            };
            SYNTAX_SET.find_syntax_by_name(mapped)
        });

    let syntax = match syntax {
        Some(s) => s,
        None => {
            // Unknown language, return plain text
            return vec![Span::styled(
                content.to_string(),
                Style::default().fg(theme.text),
            )];
        }
    };

    // Use base16-eighties theme for syntax detection
    let syntect_theme = THEME_SET
        .themes
        .get("base16-eighties.dark")
        .unwrap_or(&THEME_SET.themes["base16-ocean.dark"]);

    let mut highlighter = HighlightLines::new(syntax, syntect_theme);

    // Highlight the single line (add newline for syntect)
    let line_with_newline = format!("{}\n", content);
    let ranges = highlighter.highlight_line(&line_with_newline, &SYNTAX_SET);

    match ranges {
        Ok(ranges) => {
            ranges
                .into_iter()
                .map(|(style, text)| {
                    let fg = map_syntect_to_theme(style.foreground, theme);
                    let mut ratatui_style = Style::default().fg(fg);

                    if style.font_style.contains(FontStyle::BOLD) {
                        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
                    }
                    if style.font_style.contains(FontStyle::ITALIC) {
                        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
                    }

                    // Remove trailing newline but preserve all other whitespace
                    let text = text.strip_suffix('\n').unwrap_or(text).to_string();
                    Span::styled(text, ratatui_style)
                })
                .collect()
        }
        Err(_) => {
            // Fallback to plain text on error
            vec![Span::styled(
                content.to_string(),
                Style::default().fg(theme.text),
            )]
        }
    }
}

/// Detect if content looks like a diff.
pub fn is_diff(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().take(5).collect();

    // Check for diff-like patterns
    lines
        .iter()
        .any(|l| l.starts_with("---") || l.starts_with("+++"))
        || lines.iter().any(|l| l.starts_with("@@"))
        || (lines.iter().any(|l| l.starts_with('+')) && lines.iter().any(|l| l.starts_with('-')))
}

/// Get the language/extension from a file path for syntax highlighting.
pub fn language_from_path(path: &str) -> &str {
    std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
}

/// Get a list of supported languages.
pub fn supported_languages() -> Vec<&'static str> {
    vec![
        "rust",
        "python",
        "javascript",
        "typescript",
        "go",
        "java",
        "c",
        "c++",
        "ruby",
        "php",
        "swift",
        "kotlin",
        "scala",
        "haskell",
        "lua",
        "perl",
        "bash",
        "shell",
        "fish",
        "powershell",
        "sql",
        "html",
        "css",
        "scss",
        "json",
        "yaml",
        "toml",
        "xml",
        "markdown",
        "dockerfile",
        "makefile",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust() {
        let theme = Theme::wonopcode();
        let code = r#"fn main() {
    println!("Hello, world!");
}"#;
        let lines = highlight_code(code, "rust", &theme);
        assert!(!lines.is_empty());
        assert!(lines.len() >= 3);
    }

    #[test]
    fn test_highlight_python() {
        let theme = Theme::wonopcode();
        let code = r#"def hello():
    print("Hello, world!")
"#;
        let lines = highlight_code(code, "python", &theme);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_highlight_unknown_language() {
        let theme = Theme::wonopcode();
        let code = "some random text";
        let lines = highlight_code(code, "unknown_lang", &theme);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_highlight_diff() {
        let theme = Theme::wonopcode();
        let diff = r#"--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,3 @@
 context
-removed
+added
"#;
        let lines = highlight_diff(diff, &theme);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_is_diff() {
        assert!(is_diff("--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new"));
        assert!(!is_diff("fn main() { }"));
    }

    #[test]
    fn test_detect_diff_language() {
        // Rust file
        let diff = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new";
        assert_eq!(detect_diff_language(diff), Some("rs".to_string()));

        // Python file
        let diff = "--- a/script.py\n+++ b/script.py\n@@ -1 +1 @@\n-old\n+new";
        assert_eq!(detect_diff_language(diff), Some("py".to_string()));

        // No extension
        let diff = "--- a/Makefile\n+++ b/Makefile\n@@ -1 +1 @@\n-old\n+new";
        assert_eq!(detect_diff_language(diff), None);
    }

    #[test]
    fn test_highlight_diff_preserves_whitespace() {
        let theme = Theme::wonopcode();
        let diff = "--- a/test.rs\n+++ b/test.rs\n@@ -1 +1 @@\n-    let x = 1;\n+    let x = 2;";
        let lines = highlight_diff(diff, &theme);

        // Check that the added/removed lines preserve leading whitespace
        // Line 4 is "-    let x = 1;"
        // Line 5 is "+    let x = 2;"
        assert!(lines.len() >= 5);

        // Get the content of the removed line (index 3)
        let removed_content: String = lines[3]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(
            removed_content.contains("    let"),
            "Should preserve 4 spaces: {}",
            removed_content
        );

        // Get the content of the added line (index 4)
        let added_content: String = lines[4]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(
            added_content.contains("    let"),
            "Should preserve 4 spaces: {}",
            added_content
        );
    }

    #[test]
    fn test_highlight_diff_with_syntax() {
        let theme = Theme::wonopcode();
        let diff = "--- a/test.rs\n+++ b/test.rs\n@@ -1 +1 @@\n+fn main() {}";
        let lines = highlight_diff(diff, &theme);

        // The added line should have multiple spans (syntax highlighted)
        // Not just a single span for the whole line
        let added_line = &lines[3];
        assert!(
            added_line.spans.len() > 1,
            "Should have syntax highlighting spans"
        );
    }
}
