//! String utilities for safe UTF-8 handling.

/// Truncates a string to at most `max_bytes` bytes, ensuring we don't cut in the middle
/// of a multi-byte UTF-8 character. Returns the truncated slice.
///
/// # Examples
///
/// ```
/// use wonopcode_util::truncate_to_char_boundary;
///
/// // ASCII string - truncates exactly at boundary
/// assert_eq!(truncate_to_char_boundary("hello world", 5), "hello");
///
/// // String with emoji (4-byte char) - truncates before the emoji
/// let s = "abc✅def";  // ✅ is bytes 3..6
/// assert_eq!(truncate_to_char_boundary(s, 5), "abc");  // Can't fit partial emoji
///
/// // String shorter than max_bytes - returns unchanged
/// assert_eq!(truncate_to_char_boundary("hi", 100), "hi");
/// ```
pub fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest valid char boundary <= max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_truncation() {
        assert_eq!(truncate_to_char_boundary("hello world", 5), "hello");
        assert_eq!(truncate_to_char_boundary("hello", 10), "hello");
        assert_eq!(truncate_to_char_boundary("", 5), "");
    }

    #[test]
    fn test_emoji_truncation() {
        // ✅ is 3 bytes (E2 9C 85)
        let s = "abc✅def";
        // Bytes: a(0) b(1) c(2) ✅(3,4,5) d(6) e(7) f(8)
        assert_eq!(truncate_to_char_boundary(s, 3), "abc");
        assert_eq!(truncate_to_char_boundary(s, 4), "abc"); // Can't fit partial emoji
        assert_eq!(truncate_to_char_boundary(s, 5), "abc"); // Can't fit partial emoji
        assert_eq!(truncate_to_char_boundary(s, 6), "abc✅");
        assert_eq!(truncate_to_char_boundary(s, 7), "abc✅d");
    }

    #[test]
    fn test_multibyte_chars() {
        // Japanese hiragana あ is 3 bytes
        let s = "aあb";
        assert_eq!(truncate_to_char_boundary(s, 1), "a");
        assert_eq!(truncate_to_char_boundary(s, 2), "a"); // Can't fit partial あ
        assert_eq!(truncate_to_char_boundary(s, 3), "a"); // Can't fit partial あ
        assert_eq!(truncate_to_char_boundary(s, 4), "aあ");
        assert_eq!(truncate_to_char_boundary(s, 5), "aあb");
    }

    #[test]
    fn test_empty_result() {
        // If max_bytes is 0, return empty
        assert_eq!(truncate_to_char_boundary("hello", 0), "");
        // If first char doesn't fit, return empty
        assert_eq!(truncate_to_char_boundary("✅hello", 2), "");
    }
}
