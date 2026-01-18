//! Text sanitization utilities for TUI display.
//!
//! This module provides functions to sanitize text containing control characters
//! that could interfere with terminal display or corrupt the TUI rendering.

/// Sanitize text for safe TUI display.
///
/// This function replaces control characters (ASCII 0-31 except tab, newline, carriage return)
/// with their visible Unicode Control Picture equivalents (U+2400 range) or escape sequences.
///
/// ANSI escape sequences (CSI sequences starting with ESC[) are also replaced with visible
/// representations to prevent them from affecting terminal state.
///
/// # Examples
///
/// ```
/// use wonopcode_tui_render::sanitize::sanitize_for_display;
///
/// // Bell character becomes visible
/// assert_eq!(sanitize_for_display("hello\x07world"), "hello␇world");
///
/// // Null bytes become visible
/// assert_eq!(sanitize_for_display("test\x00data"), "test␀data");
///
/// // Tab, newline, and carriage return are preserved
/// assert_eq!(sanitize_for_display("line1\nline2\ttab"), "line1\nline2\ttab");
/// ```
pub fn sanitize_for_display(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Preserve common whitespace
            '\t' | '\n' | '\r' => result.push(c),

            // ESC character - check for ANSI escape sequences
            '\x1B' => {
                // Check if this is a CSI sequence (ESC[)
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume the '['
                    result.push_str("␛[");
                    // Consume the rest of the CSI sequence until we hit a letter
                    while let Some(&next) = chars.peek() {
                        if next.is_ascii_alphabetic() {
                            result.push(chars.next().unwrap());
                            break;
                        } else if next.is_ascii_digit()
                            || next == ';'
                            || next == '?'
                            || next == '='
                        {
                            result.push(chars.next().unwrap());
                        } else {
                            // Unknown sequence, just show what we have
                            break;
                        }
                    }
                } else if chars.peek() == Some(&']') {
                    // OSC sequence (ESC])
                    chars.next(); // consume the ']'
                    result.push_str("␛]");
                    // Consume until ST (ESC\) or BEL (\x07)
                    while let Some(&next) = chars.peek() {
                        if next == '\x07' {
                            result.push_str("␇");
                            chars.next();
                            break;
                        } else if next == '\x1B' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                                result.push_str("␛\\");
                            } else {
                                result.push_str("␛");
                            }
                            break;
                        } else {
                            result.push(chars.next().unwrap());
                        }
                    }
                } else {
                    // Just an ESC character
                    result.push_str("␛");
                }
            }

            // Control characters (ASCII 0-31, 127)
            c if c.is_ascii_control() => {
                // Map to Unicode Control Pictures (U+2400 range)
                let code = c as u32;
                if code < 32 {
                    // U+2400 is ␀ (NULL), U+2401 is ␁ (SOH), etc.
                    if let Some(symbol) = char::from_u32(0x2400 + code) {
                        result.push(symbol);
                    } else {
                        // Fallback: show as ^X notation
                        result.push('^');
                        result.push(char::from_u32(64 + code).unwrap_or('?'));
                    }
                } else if code == 127 {
                    // DEL character
                    result.push_str("␡");
                } else {
                    // Other control chars (shouldn't happen in UTF-8)
                    result.push(c);
                }
            }

            // Regular characters pass through
            _ => result.push(c),
        }
    }

    result
}

/// Check if a string contains any control characters that would need sanitization.
///
/// This is useful for avoiding the allocation cost of sanitization when the string
/// is already clean.
pub fn needs_sanitization(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_ascii_control() && c != '\t' && c != '\n' && c != '\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_null() {
        assert_eq!(sanitize_for_display("hello\x00world"), "hello␀world");
    }

    #[test]
    fn test_sanitize_bell() {
        assert_eq!(sanitize_for_display("alert\x07!"), "alert␇!");
    }

    #[test]
    fn test_sanitize_backspace() {
        assert_eq!(sanitize_for_display("back\x08space"), "back␈space");
    }

    #[test]
    fn test_sanitize_form_feed() {
        assert_eq!(sanitize_for_display("page\x0Cbreak"), "page␌break");
    }

    #[test]
    fn test_preserve_tab() {
        assert_eq!(sanitize_for_display("col1\tcol2"), "col1\tcol2");
    }

    #[test]
    fn test_preserve_newline() {
        assert_eq!(sanitize_for_display("line1\nline2"), "line1\nline2");
    }

    #[test]
    fn test_preserve_carriage_return() {
        assert_eq!(sanitize_for_display("line1\r\nline2"), "line1\r\nline2");
    }

    #[test]
    fn test_sanitize_escape() {
        assert_eq!(sanitize_for_display("test\x1Bmore"), "test␛more");
    }

    #[test]
    fn test_sanitize_csi_sequence() {
        // CSI sequence for red text
        assert_eq!(sanitize_for_display("test\x1B[31mred"), "test␛[31mred");
    }

    #[test]
    fn test_sanitize_csi_reset() {
        assert_eq!(sanitize_for_display("text\x1B[0m"), "text␛[0m");
    }

    #[test]
    fn test_sanitize_osc_sequence() {
        // OSC sequence for window title
        assert_eq!(
            sanitize_for_display("test\x1B]0;Title\x07end"),
            "test␛]0;Title␇end"
        );
    }

    #[test]
    fn test_sanitize_del() {
        assert_eq!(sanitize_for_display("test\x7Fend"), "test␡end");
    }

    #[test]
    fn test_clean_string() {
        let clean = "Hello, world! This is normal text.";
        assert_eq!(sanitize_for_display(clean), clean);
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(sanitize_for_display(""), "");
    }

    #[test]
    fn test_unicode_preserved() {
        assert_eq!(sanitize_for_display("Hello 世界 🌍"), "Hello 世界 🌍");
    }

    #[test]
    fn test_needs_sanitization() {
        assert!(!needs_sanitization("clean text"));
        assert!(!needs_sanitization("with\ttabs\nand\nnewlines"));
        assert!(needs_sanitization("has\x00null"));
        assert!(needs_sanitization("has\x1Bescape"));
        assert!(needs_sanitization("has\x07bell"));
    }

    #[test]
    fn test_multiple_control_chars() {
        assert_eq!(sanitize_for_display("\x00\x01\x02\x03"), "␀␁␂␃");
    }

    #[test]
    fn test_mixed_content() {
        assert_eq!(
            sanitize_for_display("Hello\x00World\x07Test\nNew line"),
            "Hello␀World␇Test\nNew line"
        );
    }
}
