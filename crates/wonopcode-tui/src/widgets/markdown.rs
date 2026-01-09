//! Markdown rendering for terminal display.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};

use super::syntax::{highlight_code_with_settings, highlight_diff, is_diff};
use crate::theme::{RenderSettings, Theme};

/// Default width for code block backgrounds when width is not specified.
const DEFAULT_CODE_WIDTH: usize = 80;

/// Render markdown text to styled lines.
pub fn render_markdown(text: &str, theme: &Theme) -> Text<'static> {
    render_markdown_with_width(text, theme, DEFAULT_CODE_WIDTH)
}

/// Render markdown text to styled lines with a specific width for code blocks.
pub fn render_markdown_with_width(text: &str, theme: &Theme, width: usize) -> Text<'static> {
    render_markdown_with_settings(text, theme, width, &RenderSettings::default())
}

/// Render markdown text with custom render settings.
pub fn render_markdown_with_settings(
    text: &str,
    theme: &Theme,
    width: usize,
    settings: &RenderSettings,
) -> Text<'static> {
    // If markdown is disabled, return plain text
    if !settings.markdown_enabled {
        return Text::from(
            text.lines()
                .map(|line| Line::from(Span::styled(line.to_string(), theme.text_style())))
                .collect::<Vec<_>>(),
        );
    }

    render_markdown_internal(text, theme, width, settings)
}

/// Internal markdown rendering with settings support.
fn render_markdown_internal(
    text: &str,
    theme: &Theme,
    width: usize,
    settings: &RenderSettings,
) -> Text<'static> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_lines: Vec<String> = Vec::new();
    let mut in_table = false;
    let mut table_lines: Vec<String> = Vec::new();
    let mut last_was_blank = false;

    // Calculate the content width for code blocks (accounting for indent)
    let code_width = width.saturating_sub(4); // 2 for left indent, 2 for padding

    for line in text.lines() {
        if line.starts_with("```") {
            if in_code_block {
                // End code block - render accumulated code with syntax highlighting
                let code_content = code_lines.join("\n");
                let lang_display = if code_block_lang.is_empty() {
                    "code"
                } else {
                    &code_block_lang
                };

                // Code block header with background - pad to full width
                let header_text = format!(" {lang_display} ");
                let header_padding = code_width.saturating_sub(header_text.len());

                if settings.code_backgrounds_enabled {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default().bg(theme.background_element)),
                        Span::styled(
                            header_text,
                            Style::default()
                                .fg(theme.text_muted)
                                .bg(theme.background_element),
                        ),
                        Span::styled(
                            " ".repeat(header_padding),
                            Style::default().bg(theme.background_element),
                        ),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("  ", theme.text_style()),
                        Span::styled(header_text, theme.muted_style()),
                    ]));
                }

                // Check if it's a diff
                if is_diff(&code_content) || code_block_lang == "diff" {
                    let highlighted = highlight_diff(&code_content, theme);
                    for highlighted_line in highlighted {
                        render_code_line_with_settings(
                            &mut lines,
                            highlighted_line,
                            theme,
                            code_width,
                            settings,
                        );
                    }
                } else {
                    // Apply syntax highlighting with background
                    let highlighted = highlight_code_with_settings(
                        &code_content,
                        &code_block_lang,
                        theme,
                        settings,
                    );
                    for highlighted_line in highlighted {
                        render_code_line_with_settings(
                            &mut lines,
                            highlighted_line,
                            theme,
                            code_width,
                            settings,
                        );
                    }
                }

                code_lines.clear();
                code_block_lang.clear();
                in_code_block = false;
                last_was_blank = false;
            } else {
                // Start code block
                code_block_lang = line.strip_prefix("```").unwrap_or("").trim().to_string();
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        // Check for table start/continuation
        if line.contains('|') && !line.trim().is_empty() {
            // Flush any pending table before starting a new context
            if !in_table {
                in_table = true;
            }
            table_lines.push(line.to_string());
            last_was_blank = false;
            continue;
        } else if in_table {
            // End of table - render it
            if settings.tables_enabled {
                render_table(&mut lines, &table_lines, theme);
            } else {
                // Render table as plain text
                for table_line in &table_lines {
                    lines.push(Line::from(Span::styled(
                        table_line.clone(),
                        theme.text_style(),
                    )));
                }
            }
            table_lines.clear();
            in_table = false;
        }

        // Handle empty lines - collapse multiple blank lines into one
        if line.trim().is_empty() {
            if !last_was_blank && !lines.is_empty() {
                lines.push(Line::from(""));
                last_was_blank = true;
            }
            continue;
        }
        last_was_blank = false;

        // Handle headings
        if line.starts_with("# ") {
            lines.push(Line::from(Span::styled(
                line.strip_prefix("# ").unwrap_or(line).to_string(),
                theme.text_style().add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("## ") {
            lines.push(Line::from(Span::styled(
                line.strip_prefix("## ").unwrap_or(line).to_string(),
                theme.text_style().add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("### ") {
            lines.push(Line::from(Span::styled(
                line.strip_prefix("### ").unwrap_or(line).to_string(),
                theme.highlight_style().add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Handle bullet points
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = &line[2..];
            let mut spans = vec![
                Span::styled("  ", theme.text_style()),
                Span::styled("• ", theme.muted_style()),
            ];
            // Process inline markdown in bullet content
            let inline = render_inline_markdown(content, theme);
            spans.extend(inline.spans);
            lines.push(Line::from(spans));
            continue;
        }

        // Handle numbered lists (e.g., "1. item", "2. item")
        if let Some(idx) = line.find(". ") {
            let prefix = &line[..idx];
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                let content = &line[idx + 2..];
                let mut spans = vec![
                    Span::styled("  ", theme.text_style()),
                    Span::styled(format!("{prefix}. "), theme.muted_style()),
                ];
                // Process inline markdown in list content
                let inline = render_inline_markdown(content, theme);
                spans.extend(inline.spans);
                lines.push(Line::from(spans));
                continue;
            }
        }

        // Handle blockquotes
        if line.starts_with("> ") {
            lines.push(Line::from(vec![
                Span::styled("│ ", theme.muted_style()),
                Span::styled(
                    line.strip_prefix("> ").unwrap_or(line).to_string(),
                    theme.dim_style().add_modifier(Modifier::ITALIC),
                ),
            ]));
            continue;
        }

        // Handle horizontal rules
        if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
            lines.push(Line::from(Span::styled("─".repeat(40), theme.dim_style())));
            continue;
        }

        // Regular paragraph - handle inline formatting
        lines.push(render_inline_markdown(line, theme));
    }

    // Flush pending table at end
    if in_table && !table_lines.is_empty() {
        if settings.tables_enabled {
            render_table(&mut lines, &table_lines, theme);
        } else {
            // Render table as plain text
            for table_line in &table_lines {
                lines.push(Line::from(Span::styled(
                    table_line.clone(),
                    theme.text_style(),
                )));
            }
        }
    }

    // Handle unclosed code block (streaming scenario)
    if in_code_block && !code_lines.is_empty() {
        let code_content = code_lines.join("\n");
        let lang_display = if code_block_lang.is_empty() {
            "code"
        } else {
            &code_block_lang
        };

        // Code block header with background - pad to full width
        let header_text = format!(" {lang_display} ");
        let header_padding = code_width.saturating_sub(header_text.len());

        if settings.code_backgrounds_enabled {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().bg(theme.background_element)),
                Span::styled(
                    header_text,
                    Style::default()
                        .fg(theme.text_muted)
                        .bg(theme.background_element),
                ),
                Span::styled(
                    " ".repeat(header_padding),
                    Style::default().bg(theme.background_element),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  ", theme.text_style()),
                Span::styled(header_text, theme.muted_style()),
            ]));
        }

        // Apply syntax highlighting even to unclosed blocks with background
        let highlighted =
            highlight_code_with_settings(&code_content, &code_block_lang, theme, settings);
        for highlighted_line in highlighted {
            render_code_line_with_settings(
                &mut lines,
                highlighted_line,
                theme,
                code_width,
                settings,
            );
        }
    }

    Text::from(lines)
}

/// Render a markdown table.
fn render_table(lines: &mut Vec<Line<'static>>, table_lines: &[String], theme: &Theme) {
    if table_lines.is_empty() {
        return;
    }

    // Parse table structure
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut separator_idx: Option<usize> = None;

    for (idx, line) in table_lines.iter().enumerate() {
        let cells: Vec<String> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(|s| s.trim().to_string())
            .collect();

        // Check if this is a separator line (contains only -, :, and spaces)
        if cells
            .iter()
            .all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
        {
            separator_idx = Some(idx);
        } else {
            rows.push(cells);
        }
    }

    if rows.is_empty() {
        return;
    }

    // Calculate column widths based on display width (without markdown syntax)
    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![0; num_cols];

    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                // Calculate display width by stripping markdown syntax
                let display_width = calculate_display_width(cell);
                col_widths[i] = col_widths[i].max(display_width);
            }
        }
    }

    // Render table
    let is_header = separator_idx == Some(1);

    for (row_idx, row) in rows.iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::new();

        for (col_idx, cell) in row.iter().enumerate() {
            if col_idx < num_cols {
                let width = col_widths[col_idx];

                if col_idx > 0 {
                    spans.push(Span::styled(" │ ", theme.muted_style()));
                }

                // Render inline markdown for the cell content
                let cell_line = render_inline_markdown(cell, theme);
                let cell_display_width = calculate_display_width(cell);

                // Apply bold modifier to header row spans
                if is_header && row_idx == 0 {
                    for span in cell_line.spans {
                        spans.push(Span::styled(
                            span.content.to_string(),
                            span.style.add_modifier(Modifier::BOLD),
                        ));
                    }
                } else {
                    spans.extend(cell_line.spans);
                }

                // Add padding to reach the column width
                let padding = width.saturating_sub(cell_display_width);
                if padding > 0 {
                    spans.push(Span::styled(" ".repeat(padding), theme.text_style()));
                }
            }
        }

        lines.push(Line::from(spans));

        // Add separator after header
        if is_header && row_idx == 0 {
            let sep_spans: Vec<Span<'static>> = col_widths
                .iter()
                .enumerate()
                .flat_map(|(i, &w)| {
                    let mut s = vec![Span::styled("─".repeat(w), theme.muted_style())];
                    if i < col_widths.len() - 1 {
                        s.push(Span::styled("─┼─", theme.muted_style()));
                    }
                    s
                })
                .collect();
            lines.push(Line::from(sep_spans));
        }
    }
}

/// Helper function to render a single code line with settings support.
fn render_code_line_with_settings(
    lines: &mut Vec<Line<'static>>,
    highlighted_line: Line<'static>,
    theme: &Theme,
    code_width: usize,
    settings: &RenderSettings,
) {
    // If code backgrounds are disabled, render without background
    if !settings.code_backgrounds_enabled {
        let mut new_line = vec![Span::styled("  ", theme.text_style())];
        for span in highlighted_line.spans {
            new_line.push(span);
        }
        lines.push(Line::from(new_line));
        return;
    }

    // Use the regular render function with backgrounds
    let bg_style = Style::default().bg(theme.background_element);

    // Calculate the content length
    let mut content_len = 0;
    for span in &highlighted_line.spans {
        content_len += span.content.chars().count();
    }

    // For empty/blank lines, just render full-width background
    if content_len == 0 || highlighted_line.spans.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " ".repeat(code_width + 2), // +2 for left indent
            bg_style,
        )]));
        return;
    }

    let mut new_line = vec![Span::styled("  ", bg_style)];

    // Add background to each span
    for span in highlighted_line.spans {
        new_line.push(Span::styled(
            span.content.to_string(),
            span.style.bg(theme.background_element),
        ));
    }

    // Pad to fill the remaining width with background
    let padding = code_width.saturating_sub(content_len);
    if padding > 0 {
        new_line.push(Span::styled(" ".repeat(padding), bg_style));
    }

    lines.push(Line::from(new_line));
}

/// Calculate the display width of text after stripping markdown syntax.
/// This is used for table column alignment.
fn calculate_display_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                // Inline code - count content plus spaces for padding
                let mut code_len = 0;
                while let Some(&next) = chars.peek() {
                    if next == '`' {
                        chars.next();
                        break;
                    }
                    chars.next();
                    code_len += 1;
                }
                width += code_len + 2; // +2 for the spaces around code
            }
            '*' | '_' => {
                if chars.peek() == Some(&c) {
                    // Bold (**text**) - skip the markers
                    chars.next();
                    while let Some(&next) = chars.peek() {
                        if next == c {
                            chars.next();
                            if chars.peek() == Some(&c) {
                                chars.next();
                                break;
                            }
                        }
                        chars.next();
                        width += 1;
                    }
                } else {
                    // Italic (*text*) - skip the markers
                    while let Some(&next) = chars.peek() {
                        if next == c {
                            chars.next();
                            break;
                        }
                        chars.next();
                        width += 1;
                    }
                }
            }
            '[' => {
                // Link [text](url) - only count the text part
                let mut link_text_len = 0;
                while let Some(&next) = chars.peek() {
                    if next == ']' {
                        chars.next();
                        break;
                    }
                    chars.next();
                    link_text_len += 1;
                }

                if chars.peek() == Some(&'(') {
                    // Skip the URL part
                    chars.next();
                    while let Some(&next) = chars.peek() {
                        if next == ')' {
                            chars.next();
                            break;
                        }
                        chars.next();
                    }
                    width += link_text_len;
                } else {
                    // Not a link, count brackets and text
                    width += link_text_len + 2;
                }
            }
            _ => {
                width += 1;
            }
        }
    }

    width
}

/// Render inline markdown formatting (bold, italic, code, links).
fn render_inline_markdown(line: &str, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                // Inline code - use green (success) color with subtle background
                if !current.is_empty() {
                    spans.push(Span::styled(current.clone(), theme.text_style()));
                    current.clear();
                }

                let mut code = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '`' {
                        chars.next();
                        break;
                    }
                    if let Some(ch) = chars.next() {
                        code.push(ch);
                    }
                }

                spans.push(Span::styled(
                    format!(" {code} "),
                    Style::default()
                        .fg(theme.success)
                        .bg(theme.background_element),
                ));
            }
            '*' | '_' => {
                // Check for bold or italic
                if chars.peek() == Some(&c) {
                    // Bold (**) - use primary color for emphasis
                    chars.next();

                    if !current.is_empty() {
                        spans.push(Span::styled(current.clone(), theme.text_style()));
                        current.clear();
                    }

                    let mut bold = String::new();
                    while let Some(&next) = chars.peek() {
                        if next == c {
                            chars.next();
                            if chars.peek() == Some(&c) {
                                chars.next();
                                break;
                            }
                        }
                        if let Some(ch) = chars.next() {
                            bold.push(ch);
                        }
                    }

                    spans.push(Span::styled(
                        bold,
                        theme.primary_style().add_modifier(Modifier::BOLD),
                    ));
                } else {
                    // Italic (*) - use secondary color for emphasis
                    if !current.is_empty() {
                        spans.push(Span::styled(current.clone(), theme.text_style()));
                        current.clear();
                    }

                    let mut italic = String::new();
                    while let Some(&next) = chars.peek() {
                        if next == c {
                            chars.next();
                            break;
                        }
                        if let Some(ch) = chars.next() {
                            italic.push(ch);
                        }
                    }

                    spans.push(Span::styled(
                        italic,
                        theme.secondary_style().add_modifier(Modifier::ITALIC),
                    ));
                }
            }
            '[' => {
                // Link [text](url)
                if !current.is_empty() {
                    spans.push(Span::styled(current.clone(), theme.text_style()));
                    current.clear();
                }

                let mut link_text = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ']' {
                        chars.next();
                        break;
                    }
                    if let Some(ch) = chars.next() {
                        link_text.push(ch);
                    }
                }

                // Check for URL
                if chars.peek() == Some(&'(') {
                    chars.next();
                    let mut url = String::new();
                    while let Some(&next) = chars.peek() {
                        if next == ')' {
                            chars.next();
                            break;
                        }
                        if let Some(ch) = chars.next() {
                            url.push(ch);
                        }
                    }

                    spans.push(Span::styled(
                        link_text,
                        theme.highlight_style().add_modifier(Modifier::UNDERLINED),
                    ));
                } else {
                    // Not a link, just brackets
                    current.push('[');
                    current.push_str(&link_text);
                    current.push(']');
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, theme.text_style()));
    }

    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown() {
        let theme = Theme::wonopcode();

        let md = "# Heading\n\nSome **bold** and *italic* text.\n\n```rust\nfn main() {}\n```";
        let text = render_markdown(md, &theme);

        // Just verify it doesn't panic
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn test_calculate_display_width() {
        // Plain text
        assert_eq!(calculate_display_width("hello"), 5);

        // Bold
        assert_eq!(calculate_display_width("**bold**"), 4);

        // Italic
        assert_eq!(calculate_display_width("*italic*"), 6);

        // Inline code (adds 2 for spaces)
        assert_eq!(calculate_display_width("`code`"), 6);

        // Link
        assert_eq!(calculate_display_width("[text](url)"), 4);

        // Mixed
        assert_eq!(calculate_display_width("a **b** c"), 5);
    }

    #[test]
    fn test_render_table_with_markdown() {
        let theme = Theme::wonopcode();

        let md = "| Column | Value |\n|--------|-------|\n| **bold** | `code` |";
        let text = render_markdown(md, &theme);

        // Verify table rendered (should have 3 lines: header, separator, data)
        assert!(text.lines.len() >= 3);

        // Check that the table contains styled content (not raw markdown)
        let all_content: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();

        // Should not contain raw markdown syntax
        assert!(!all_content.contains("**bold**"));
        assert!(!all_content.contains("`code`"));

        // Should contain the actual text
        assert!(all_content.contains("bold"));
        assert!(all_content.contains("code"));
    }
}
