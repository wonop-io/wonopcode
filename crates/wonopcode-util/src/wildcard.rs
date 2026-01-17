//! Wildcard pattern matching.
//!
//! This module provides simple wildcard matching for permission patterns.
//! Supports `*` as a wildcard that matches any sequence of characters.

/// Match a string against a wildcard pattern.
///
/// The pattern can contain:
/// - `*` - matches any sequence of characters (including empty)
/// - Any other character - matches itself literally
///
/// # Examples
///
/// ```
/// use wonopcode_util::wildcard::matches;
///
/// assert!(matches("echo *", "echo hello"));
/// assert!(matches("git *", "git status"));
/// assert!(matches("*", "anything"));
/// assert!(!matches("echo *", "cat file"));
/// ```
pub fn matches(pattern: &str, text: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();

    matches_recursive(&pattern_chars, &text_chars, 0, 0)
}

fn matches_recursive(pattern: &[char], text: &[char], pi: usize, ti: usize) -> bool {
    // Both exhausted - match!
    if pi == pattern.len() && ti == text.len() {
        return true;
    }

    // Pattern exhausted but text remains - no match
    if pi == pattern.len() {
        return false;
    }

    // Handle wildcard
    if pattern[pi] == '*' {
        // Try matching zero characters (skip the *)
        if matches_recursive(pattern, text, pi + 1, ti) {
            return true;
        }

        // Try matching one or more characters
        if ti < text.len() && matches_recursive(pattern, text, pi, ti + 1) {
            return true;
        }

        return false;
    }

    // Text exhausted but pattern has more non-* characters
    if ti == text.len() {
        return false;
    }

    // Match single character
    if pattern[pi] == text[ti] {
        return matches_recursive(pattern, text, pi + 1, ti + 1);
    }

    false
}

/// Check if a command matches any of the patterns.
///
/// Returns the first matching pattern, or None if no match.
pub fn find_matching_pattern<'a>(patterns: &'a [&str], text: &str) -> Option<&'a str> {
    patterns.iter().copied().find(|&p| matches(p, text))
}

/// Calculate the specificity of a pattern.
///
/// More specific patterns (fewer wildcards, longer literals) have higher specificity.
/// This is used to determine which pattern takes precedence when multiple match.
pub fn specificity(pattern: &str) -> u32 {
    let mut score = 0u32;

    // Count literal (non-wildcard) characters - this is the main specificity indicator
    let literal_chars = pattern.chars().filter(|&c| c != '*').count() as u32;
    score += literal_chars * 100;

    // Subtract for each wildcard (but less than the literal bonus)
    let wildcard_count = pattern.chars().filter(|&c| c == '*').count() as u32;
    score = score.saturating_sub(wildcard_count * 10);

    // Bonus for patterns that don't start with *
    if !pattern.starts_with('*') {
        score += 50;
    }

    // Bonus for patterns that don't end with *
    if !pattern.ends_with('*') {
        score += 50;
    }

    score
}

/// Find the most specific matching pattern.
///
/// When multiple patterns match, returns the one with highest specificity.
pub fn find_most_specific_match<'a>(patterns: &'a [&str], text: &str) -> Option<&'a str> {
    patterns
        .iter()
        .copied()
        .filter(|&p| matches(p, text))
        .max_by_key(|&p| specificity(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(matches("hello", "hello"));
        assert!(!matches("hello", "world"));
    }

    #[test]
    fn test_wildcard_end() {
        assert!(matches("echo *", "echo hello"));
        assert!(matches("echo *", "echo hello world"));
        // Note: "echo *" requires at least "echo " - the space is literal
        // "echo" without a trailing space doesn't match "echo " followed by *
        assert!(!matches("echo *", "echo")); // No trailing space in input
        assert!(matches("echo*", "echo")); // This works - no space before *
        assert!(!matches("echo *", "cat hello"));
    }

    #[test]
    fn test_wildcard_start() {
        assert!(matches("*.rs", "main.rs"));
        assert!(matches("*.rs", ".rs"));
        assert!(!matches("*.rs", "main.py"));
    }

    #[test]
    fn test_wildcard_middle() {
        assert!(matches("git * status", "git remote status"));
        assert!(matches("git * status", "git  status"));
        assert!(!matches("git * status", "git remote"));
    }

    #[test]
    fn test_multiple_wildcards() {
        assert!(matches("*hello*", "say hello world"));
        assert!(matches("*hello*", "hello"));
        assert!(matches("*hello*", "hello world"));
        assert!(matches("*hello*", "say hello"));
    }

    #[test]
    fn test_just_wildcard() {
        assert!(matches("*", "anything"));
        assert!(matches("*", ""));
    }

    #[test]
    fn test_empty_pattern() {
        assert!(matches("", ""));
        assert!(!matches("", "something"));
    }

    #[test]
    fn test_specificity() {
        assert!(specificity("echo hello") > specificity("echo *"));
        assert!(specificity("git status") > specificity("*"));
        assert!(specificity("echo *") > specificity("*"));
    }

    #[test]
    fn test_find_most_specific() {
        let patterns = &["*", "echo *", "echo hello"];
        assert_eq!(
            find_most_specific_match(patterns, "echo hello"),
            Some("echo hello")
        );
        assert_eq!(
            find_most_specific_match(patterns, "echo world"),
            Some("echo *")
        );
        assert_eq!(find_most_specific_match(patterns, "cat file"), Some("*"));
    }

    #[test]
    fn test_bash_permission_patterns() {
        let patterns = &[
            "git status*",
            "git log*",
            "git diff*",
            "ls*",
            "find * -delete*",
            "find *",
            "*",
        ];

        assert_eq!(
            find_most_specific_match(patterns, "git status"),
            Some("git status*")
        );
        assert_eq!(
            find_most_specific_match(patterns, "git log --oneline"),
            Some("git log*")
        );
        assert_eq!(
            find_most_specific_match(patterns, "find . -name '*.rs'"),
            Some("find *")
        );
        assert_eq!(
            find_most_specific_match(patterns, "find . -delete"),
            Some("find * -delete*")
        );
        assert_eq!(find_most_specific_match(patterns, "rm -rf /"), Some("*"));
    }

    #[test]
    fn test_find_matching_pattern_returns_first_match() {
        let patterns = &["echo *", "cat *", "ls *"];
        assert_eq!(
            find_matching_pattern(patterns, "echo hello"),
            Some("echo *")
        );
        assert_eq!(find_matching_pattern(patterns, "cat file"), Some("cat *"));
        assert_eq!(find_matching_pattern(patterns, "unknown"), None);
    }

    #[test]
    fn test_find_most_specific_match_returns_none_when_no_match() {
        let patterns = &["echo *", "cat *"];
        assert_eq!(find_most_specific_match(patterns, "rm -rf /"), None);
    }

    #[test]
    fn test_specificity_bonus_for_no_leading_wildcard() {
        // Pattern that doesn't start with * gets +50 bonus
        // "hello*" has 5 literal chars * 100 = 500, minus 1 wildcard * 10 = 490, plus 50 for not starting with * = 540
        // "*hello" has 5 literal chars * 100 = 500, minus 1 wildcard * 10 = 490, plus 50 for not ending with * = 540
        // They're equal because bonus is same (one doesn't start, other doesn't end)
        // Let's test a case where one has both bonuses
        assert!(specificity("hello") > specificity("*hello")); // no wildcards at all
    }

    #[test]
    fn test_specificity_prefers_no_trailing_wildcard() {
        // Pattern without trailing * should have higher specificity
        assert!(specificity("*hello") > specificity("*hello*"));
    }

    #[test]
    fn test_consecutive_wildcards() {
        assert!(matches("**", "anything"));
        assert!(matches("a**b", "ab"));
        assert!(matches("a**b", "aXXXb"));
    }

    #[test]
    fn test_wildcard_only_at_boundaries() {
        assert!(matches("*end", "the end"));
        assert!(matches("start*", "starting"));
        assert!(!matches("*middle*", "no match here"));
        assert!(matches("*middle*", "some middle text"));
    }
}
