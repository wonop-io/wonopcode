//! Rendering utilities for wonopcode TUI.
//!
//! This crate provides:
//! - Markdown rendering with syntax highlighting
//! - Syntax highlighting for code blocks
//! - Diff display widgets
//! - Text sanitization for safe TUI display

pub mod diff;
pub mod markdown;
pub mod sanitize;
pub mod syntax;

// Re-export commonly used types
pub use diff::{DiffHunk, DiffLine, DiffStyle, DiffWidget, FileDiff};
pub use markdown::{
    render_markdown, render_markdown_with_settings, render_markdown_with_width, wrap_line,
    CodeRegion, RenderedMarkdown,
};
pub use sanitize::{needs_sanitization, sanitize_for_display};
pub use syntax::{highlight_code, highlight_code_with_settings, highlight_diff, is_diff};
