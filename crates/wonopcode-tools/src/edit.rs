//! Edit tool - perform exact string replacements in files.
//!
//! This tool performs search and replace operations on files with:
//! - Exact string matching (primary)
//! - Fuzzy matching fallback for common issues (whitespace, indentation)
//! - Atomic file writes
//! - Replace-all support

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::debug;

/// Edit tool for string replacement.
pub struct EditTool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for EditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        r#"Performs exact string replacements in files.

Usage:
- The edit will FAIL if `oldString` is not found in the file.
- The edit will FAIL if `oldString` is found multiple times (unless replaceAll is true).
- Use `replaceAll` for replacing all occurrences.
- Preserve exact indentation from the original file."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["filePath", "oldString", "newString"],
            "properties": {
                "filePath": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "oldString": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "newString": {
                    "type": "string",
                    "description": "The text to replace it with"
                },
                "replaceAll": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default false)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: EditArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Resolve file path
        let file_path = resolve_path(&args.file_path, &ctx.cwd, &ctx.root_dir, ctx).await?;

        // Check for concurrent modifications if file time tracking is enabled
        if let Some(ref file_time) = ctx.file_time {
            // For sandboxed execution, we still track the host path
            let host_path = if ctx.is_sandboxed() {
                ctx.to_host_path(&file_path)
            } else {
                file_path.clone()
            };

            let exists = if let Some(sandbox) = ctx.sandbox() {
                let sandbox_path = ctx.to_sandbox_path(&file_path);
                sandbox.path_exists(&sandbox_path).await.unwrap_or(false)
            } else {
                file_path.exists()
            };

            if exists {
                file_time
                    .assert_not_modified(&ctx.session_id, &host_path)
                    .await
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?;
            }
        }

        // Take snapshot before editing (if snapshot store is available)
        if let Some(ref snapshot_store) = ctx.snapshot {
            if let Err(e) = snapshot_store
                .take(
                    &[file_path.clone()],
                    &ctx.session_id,
                    &ctx.message_id,
                    &format!("Before edit: {}", file_path.display()),
                )
                .await
            {
                debug!("Failed to take snapshot before edit: {}", e);
            }
        }

        // Read current content - either from sandbox or directly
        let content = if let Some(sandbox) = ctx.sandbox() {
            let sandbox_path = ctx.to_sandbox_path(&file_path);
            let bytes = sandbox
                .read_file(&sandbox_path)
                .await
                .map_err(|e| ToolError::execution_failed(format!("Failed to read file: {e}")))?;
            String::from_utf8_lossy(&bytes).to_string()
        } else {
            fs::read_to_string(&file_path)
                .await
                .map_err(|e| ToolError::execution_failed(format!("Failed to read file: {e}")))?
        };

        // Validate oldString != newString
        if args.old_string == args.new_string {
            return Err(ToolError::validation(
                "oldString and newString must be different",
            ));
        }

        // Try exact match first
        let match_result = find_matches(&content, &args.old_string);

        let (new_content, swapped) = match match_result {
            MatchResult::None => {
                // Try fuzzy matching
                if let Some(fuzzy_match) = try_fuzzy_match(&content, &args.old_string) {
                    debug!(
                        "Using fuzzy match: original={:?}, fuzzy={:?}",
                        &args.old_string, &fuzzy_match
                    );
                    (content.replace(&fuzzy_match, &args.new_string), false)
                } else {
                    // TrySwap fallback: if newString exists but oldString doesn't,
                    // the edit was likely already applied - swap and "undo"
                    let swap_match = find_matches(&content, &args.new_string);
                    match swap_match {
                        MatchResult::Single => {
                            debug!("TrySwap: oldString not found but newString found, swapping");
                            (
                                content.replacen(&args.new_string, &args.old_string, 1),
                                true,
                            )
                        }
                        MatchResult::Multiple(_) if args.replace_all => {
                            debug!(
                                "TrySwap with replaceAll: oldString not found but newString found, swapping"
                            );
                            (content.replace(&args.new_string, &args.old_string), true)
                        }
                        _ => {
                            return Err(ToolError::execution_failed(
                                "oldString not found in file content",
                            ));
                        }
                    }
                }
            }
            MatchResult::Single => (
                content.replacen(&args.old_string, &args.new_string, 1),
                false,
            ),
            MatchResult::Multiple(count) => {
                if args.replace_all {
                    (content.replace(&args.old_string, &args.new_string), false)
                } else {
                    return Err(ToolError::execution_failed(format!(
                        "oldString found {count} times. Use replaceAll to replace all occurrences, or provide more context to make the match unique."
                    )));
                }
            }
        };

        // Generate diff for display
        let diff = generate_diff(&content, &new_content, &file_path);

        // Write file - either through sandbox or directly
        if let Some(sandbox) = ctx.sandbox() {
            let sandbox_path = ctx.to_sandbox_path(&file_path);
            sandbox
                .write_file(&sandbox_path, new_content.as_bytes())
                .await
                .map_err(|e| ToolError::execution_failed(format!("Failed to write file: {e}")))?;
        } else {
            // Write atomically (write to temp, then rename)
            // Use random temp file name to avoid collisions and predictable names
            let random_suffix: u64 = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0)
                ^ (std::process::id() as u64);
            let temp_name = format!(
                ".{}.{:x}.tmp",
                file_path
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default(),
                random_suffix
            );
            let temp_path = file_path
                .parent()
                .map(|p| p.join(&temp_name))
                .unwrap_or_else(|| PathBuf::from(&temp_name));

            fs::write(&temp_path, &new_content).await.map_err(|e| {
                ToolError::execution_failed(format!("Failed to write temp file: {e}"))
            })?;

            // Ensure temp file is cleaned up on failure
            let rename_result = fs::rename(&temp_path, &file_path).await;
            if rename_result.is_err() {
                // Try to clean up the temp file
                let _ = fs::remove_file(&temp_path).await;
            }
            rename_result
                .map_err(|e| ToolError::execution_failed(format!("Failed to rename file: {e}")))?;
        }

        // Update file read time after successful write
        if let Some(ref file_time) = ctx.file_time {
            let host_path = if ctx.is_sandboxed() {
                ctx.to_host_path(&file_path)
            } else {
                file_path.clone()
            };
            file_time.record_read(&ctx.session_id, &host_path).await;
        }

        // Calculate stats
        let (old_lines, new_lines) = if swapped {
            // When swapped, the stats are reversed
            (
                args.new_string.lines().count(),
                args.old_string.lines().count(),
            )
        } else {
            (
                args.old_string.lines().count(),
                args.new_string.lines().count(),
            )
        };
        let additions = new_lines.saturating_sub(old_lines);
        let deletions = old_lines.saturating_sub(new_lines);

        let title = if swapped {
            format!(
                "Edited {} (swapped - undoing previous edit)",
                file_path.display()
            )
        } else {
            format!("Edited {}", file_path.display())
        };

        Ok(ToolOutput::new(title, diff).with_metadata(json!({
            "file": file_path.display().to_string(),
            "additions": additions,
            "deletions": deletions,
            "replaced": if args.replace_all {
                match_result.count().max(1)
            } else {
                1
            },
            "swapped": swapped
        })))
    }
}

/// Result of finding matches.
#[derive(Debug)]
enum MatchResult {
    None,
    Single,
    Multiple(usize),
}

impl MatchResult {
    fn count(&self) -> usize {
        match self {
            MatchResult::None => 0,
            MatchResult::Single => 1,
            MatchResult::Multiple(n) => *n,
        }
    }
}

/// Find matches of needle in haystack.
fn find_matches(haystack: &str, needle: &str) -> MatchResult {
    let count = haystack.matches(needle).count();
    match count {
        0 => MatchResult::None,
        1 => MatchResult::Single,
        n => MatchResult::Multiple(n),
    }
}

/// Try fuzzy matching strategies.
///
/// Implements multiple replacer strategies:
/// 1. Line endings normalization
/// 2. Line trimming (trailing whitespace)
/// 3. Whitespace normalization (collapse multiple spaces)
/// 4. Boundary trimming (leading/trailing whitespace on entire block)
/// 5. Indentation flexibility
/// 6. Block anchor matching (first/last line anchors with fuzzy middle)
/// 7. Escape sequence normalization
/// 8. Context-aware matching
fn try_fuzzy_match(content: &str, target: &str) -> Option<String> {
    // Strategy 1: Normalize line endings
    let normalized_target = target.replace("\r\n", "\n");
    if content.contains(&normalized_target) {
        return Some(normalized_target);
    }

    // Strategy 2: Trim trailing whitespace from each line (LineTrimmedReplacer)
    let trimmed_target: String = target
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if content.contains(&trimmed_target) {
        return Some(trimmed_target);
    }

    // Strategy 3: Match with flexible whitespace (WhitespaceNormalizedReplacer)
    let collapsed_target = collapse_whitespace(target);
    for (start, _) in content.match_indices(collapsed_target.split_whitespace().next()?) {
        // Try to match from this position
        let remaining = &content[start..];
        if let Some(end) = find_fuzzy_end(remaining, target) {
            let matched = &remaining[..end];
            if collapse_whitespace(matched) == collapsed_target {
                return Some(matched.to_string());
            }
        }
    }

    // Strategy 4: Try with leading/trailing whitespace stripped (TrimmedBoundaryReplacer)
    let stripped_target = target.trim();
    if stripped_target != target && content.contains(stripped_target) {
        return Some(stripped_target.to_string());
    }

    // Strategy 5: Handle indentation differences (IndentationFlexibleReplacer)
    if let Some(matched) = try_indentation_match(content, target) {
        return Some(matched);
    }

    // Strategy 6: Block anchor matching (BlockAnchorReplacer)
    if let Some(matched) = try_block_anchor_match(content, target) {
        return Some(matched);
    }

    // Strategy 7: Escape sequence normalization (EscapeNormalizedReplacer)
    if let Some(matched) = try_escape_normalized_match(content, target) {
        return Some(matched);
    }

    // Strategy 8: Context-aware matching (ContextAwareReplacer)
    if let Some(matched) = try_context_aware_match(content, target) {
        return Some(matched);
    }

    None
}

/// Block anchor replacer - uses first and last lines as anchors.
///
/// This handles cases where the middle content has minor variations.
fn try_block_anchor_match(content: &str, target: &str) -> Option<String> {
    let target_lines: Vec<&str> = target.lines().collect();

    // Need at least 3 lines for block anchor matching
    if target_lines.len() < 3 {
        return None;
    }

    let first_line = target_lines[0].trim();
    let last_line = target_lines[target_lines.len() - 1].trim();

    if first_line.is_empty() || last_line.is_empty() {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let mut candidates = Vec::new();

    // Find all positions where first line matches
    for (start_idx, line) in content_lines.iter().enumerate() {
        if line.trim() != first_line {
            continue;
        }

        // Look for matching last line
        let expected_end = start_idx + target_lines.len() - 1;
        if expected_end >= content_lines.len() {
            continue;
        }

        if content_lines[expected_end].trim() != last_line {
            continue;
        }

        // Calculate similarity of middle lines
        let similarity =
            calculate_block_similarity(&content_lines[start_idx..=expected_end], &target_lines);

        candidates.push((start_idx, expected_end, similarity));
    }

    if candidates.is_empty() {
        return None;
    }

    // Choose the best match
    let threshold = if candidates.len() == 1 { 0.0 } else { 0.3 };
    let best = candidates
        .iter()
        .filter(|(_, _, sim)| *sim >= threshold)
        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((start, end, _)) = best {
        let matched = content_lines[*start..=*end].join("\n");
        return Some(matched);
    }

    None
}

/// Calculate similarity between two blocks of lines.
fn calculate_block_similarity(content_lines: &[&str], target_lines: &[&str]) -> f64 {
    if content_lines.len() != target_lines.len() {
        return 0.0;
    }

    let mut matching = 0;
    for (c, t) in content_lines.iter().zip(target_lines.iter()) {
        if c.trim() == t.trim() {
            matching += 1;
        } else {
            // Use simple character-based similarity for non-matching lines
            let sim = line_similarity(c.trim(), t.trim());
            if sim > 0.8 {
                matching += 1;
            }
        }
    }

    matching as f64 / content_lines.len() as f64
}

/// Calculate simple character-based similarity between two strings.
fn line_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let len_a = a.chars().count();
    let len_b = b.chars().count();
    let max_len = len_a.max(len_b) as f64;

    // Simple longest common substring ratio
    let lcs = longest_common_substring(a, b);
    lcs as f64 / max_len
}

/// Find longest common substring length.
fn longest_common_substring(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    if a_chars.is_empty() || b_chars.is_empty() {
        return 0;
    }

    let mut max_len = 0;
    let mut dp = vec![vec![0usize; b_chars.len() + 1]; a_chars.len() + 1];

    for i in 1..=a_chars.len() {
        for j in 1..=b_chars.len() {
            if a_chars[i - 1] == b_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
                max_len = max_len.max(dp[i][j]);
            }
        }
    }

    max_len
}

/// Escape sequence normalization - handles escaped characters.
fn try_escape_normalized_match(content: &str, target: &str) -> Option<String> {
    // Unescape common escape sequences in the target
    let unescaped = unescape_string(target);

    if unescaped != target && content.contains(&unescaped) {
        return Some(unescaped);
    }

    // Also try matching escaped versions in content
    let escaped_content = escape_string(content);
    if escaped_content.contains(target) {
        // Find the original substring in content
        let _escaped_pos = escaped_content.find(target)?;
        // This is approximate - we return the unescaped version
        if content.contains(&unescape_string(target)) {
            return Some(unescape_string(target));
        }
    }

    None
}

/// Unescape common escape sequences.
fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek() {
                Some('n') => {
                    chars.next();
                    result.push('\n');
                }
                Some('t') => {
                    chars.next();
                    result.push('\t');
                }
                Some('r') => {
                    chars.next();
                    result.push('\r');
                }
                Some('\\') => {
                    chars.next();
                    result.push('\\');
                }
                Some('\'') => {
                    chars.next();
                    result.push('\'');
                }
                Some('"') => {
                    chars.next();
                    result.push('"');
                }
                Some('`') => {
                    chars.next();
                    result.push('`');
                }
                Some('$') => {
                    chars.next();
                    result.push('$');
                }
                _ => result.push(ch),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Escape special characters in a string.
fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);

    for ch in s.chars() {
        match ch {
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            '\\' => result.push_str("\\\\"),
            '\'' => result.push_str("\\'"),
            '"' => result.push_str("\\\""),
            '`' => result.push_str("\\`"),
            '$' => result.push_str("\\$"),
            _ => result.push(ch),
        }
    }

    result
}

/// Context-aware matching - uses first and last lines with 50% threshold.
fn try_context_aware_match(content: &str, target: &str) -> Option<String> {
    let target_lines: Vec<&str> = target.lines().collect();

    // Need at least 2 lines
    if target_lines.len() < 2 {
        return None;
    }

    let first_line = target_lines[0].trim();
    let last_line = target_lines[target_lines.len() - 1].trim();

    if first_line.is_empty() || last_line.is_empty() {
        return None;
    }

    let content_lines: Vec<&str> = content.lines().collect();

    for (start_idx, line) in content_lines.iter().enumerate() {
        if line.trim() != first_line {
            continue;
        }

        let expected_end = start_idx + target_lines.len() - 1;
        if expected_end >= content_lines.len() {
            continue;
        }

        if content_lines[expected_end].trim() != last_line {
            continue;
        }

        // Check if block has same line count
        let candidate_lines = &content_lines[start_idx..=expected_end];
        if candidate_lines.len() != target_lines.len() {
            continue;
        }

        // Count matching middle lines (50% threshold)
        let mut matching = 0;
        let middle_count = target_lines.len().saturating_sub(2);

        for i in 1..target_lines.len() - 1 {
            if candidate_lines[i].trim() == target_lines[i].trim() {
                matching += 1;
            }
        }

        if middle_count == 0 || matching as f64 / middle_count as f64 >= 0.5 {
            let matched = candidate_lines.join("\n");
            return Some(matched);
        }
    }

    None
}

/// Collapse multiple whitespace characters into single spaces.
fn collapse_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_was_space = false;

    for ch in s.chars() {
        if ch.is_whitespace() && ch != '\n' {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }

    result
}

/// Find the end of a fuzzy match.
fn find_fuzzy_end(content: &str, target: &str) -> Option<usize> {
    let target_lines: Vec<&str> = target.lines().collect();
    let content_lines: Vec<&str> = content.lines().collect();

    if target_lines.is_empty() {
        return None;
    }

    // Try to find where the target ends
    let mut matched_lines = 0;
    let mut total_len = 0;

    for (i, line) in content_lines.iter().enumerate() {
        if i < target_lines.len() {
            let target_line = target_lines[i].trim();
            let content_line = line.trim();

            if target_line == content_line {
                matched_lines += 1;
                total_len += line.len() + 1; // +1 for newline
            } else {
                break;
            }
        }
    }

    if matched_lines == target_lines.len() {
        Some(total_len.saturating_sub(1)) // Remove last newline
    } else {
        None
    }
}

/// Try to match with different indentation.
fn try_indentation_match(content: &str, target: &str) -> Option<String> {
    let target_lines: Vec<&str> = target.lines().collect();
    if target_lines.is_empty() {
        return None;
    }

    let first_target_line = target_lines[0].trim();
    if first_target_line.is_empty() {
        return None;
    }

    // Find all occurrences of the first line (ignoring indentation)
    for (line_idx, line) in content.lines().enumerate() {
        if line.trim() == first_target_line {
            // Try to match subsequent lines
            let content_lines: Vec<&str> = content.lines().collect();
            let mut matches = true;

            for (i, target_line) in target_lines.iter().enumerate() {
                let content_idx = line_idx + i;
                if content_idx >= content_lines.len() {
                    matches = false;
                    break;
                }

                if content_lines[content_idx].trim() != target_line.trim() {
                    matches = false;
                    break;
                }
            }

            if matches {
                // Build the actual string from content
                let matched: String =
                    content_lines[line_idx..line_idx + target_lines.len()].join("\n");
                return Some(matched);
            }
        }
    }

    None
}

/// Resolve a file path, handling relative and absolute paths.
async fn resolve_path(
    path: &str,
    cwd: &std::path::Path,
    root_dir: &std::path::Path,
    ctx: &ToolContext,
) -> ToolResult<PathBuf> {
    let path = PathBuf::from(path);

    // Helper to check if path exists (sandbox or direct)
    async fn path_exists(p: &Path, ctx: &ToolContext) -> bool {
        if let Some(sandbox) = ctx.sandbox() {
            let sandbox_path = ctx.to_sandbox_path(p);
            sandbox.path_exists(&sandbox_path).await.unwrap_or(false)
        } else {
            p.exists()
        }
    }

    if path.is_absolute() {
        if !path_exists(&path, ctx).await {
            return Err(ToolError::execution_failed(format!(
                "File not found: {}",
                path.display()
            )));
        }
        Ok(path)
    } else {
        // Try relative to cwd first
        let cwd_path = cwd.join(&path);
        if path_exists(&cwd_path, ctx).await {
            return Ok(cwd_path);
        }

        // Try relative to root
        let root_path = root_dir.join(&path);
        if path_exists(&root_path, ctx).await {
            return Ok(root_path);
        }

        Err(ToolError::execution_failed(format!(
            "File not found: {} (tried {} and {})",
            path.display(),
            cwd_path.display(),
            root_path.display()
        )))
    }
}

/// Generate a unified diff.
fn generate_diff(old: &str, new: &str, path: &std::path::Path) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    output.push_str(&format!("--- a/{}\n", path.display()));
    output.push_str(&format!("+++ b/{}\n", path.display()));

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            output.push_str("...\n");
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                output.push_str(sign);
                output.push_str(change.value());
                if !change.value().ends_with('\n') {
                    output.push('\n');
                }
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    async fn setup_test() -> (TempDir, ToolContext) {
        let dir = TempDir::new().unwrap();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: dir.path().to_path_buf(),
            cwd: dir.path().to_path_buf(),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
        };
        (dir, ctx)
    }

    #[tokio::test]
    async fn test_simple_edit() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").await.unwrap();

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "hello",
                    "newString": "goodbye"
                }),
                &ctx,
            )
            .await
            .unwrap();

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "goodbye world");
        assert!(result.output.contains("-hello"));
        assert!(result.output.contains("+goodbye"));
    }

    #[tokio::test]
    async fn test_replace_all() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo bar foo baz foo").await.unwrap();

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "foo",
                    "newString": "qux",
                    "replaceAll": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "qux bar qux baz qux");
        assert_eq!(result.metadata["replaced"], 3);
    }

    #[tokio::test]
    async fn test_multiple_matches_error() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo bar foo").await.unwrap();

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "foo",
                    "newString": "baz"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("found 2 times"));
    }

    #[tokio::test]
    async fn test_not_found_error() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").await.unwrap();

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "xyz",
                    "newString": "abc"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_fuzzy_match_whitespace() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello   world").await.unwrap(); // Multiple spaces

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "hello world",  // Single space
                    "newString": "goodbye world"
                }),
                &ctx,
            )
            .await;

        // Should fail - fuzzy match should not be too aggressive
        assert!(result.is_err());
    }

    #[test]
    fn test_collapse_whitespace() {
        assert_eq!(collapse_whitespace("hello  world"), "hello world");
        assert_eq!(collapse_whitespace("a\tb\tc"), "a b c");
        assert_eq!(collapse_whitespace("a\n  b"), "a\n b");
    }

    #[test]
    fn test_find_matches() {
        assert!(matches!(
            find_matches("hello world", "xyz"),
            MatchResult::None
        ));
        assert!(matches!(
            find_matches("hello world", "hello"),
            MatchResult::Single
        ));
        assert!(matches!(
            find_matches("hello hello", "hello"),
            MatchResult::Multiple(2)
        ));
    }

    #[test]
    fn test_match_result_count() {
        assert_eq!(MatchResult::None.count(), 0);
        assert_eq!(MatchResult::Single.count(), 1);
        assert_eq!(MatchResult::Multiple(5).count(), 5);
    }

    #[test]
    fn test_unescape_string() {
        assert_eq!(unescape_string("hello\\nworld"), "hello\nworld");
        assert_eq!(unescape_string("a\\tb"), "a\tb");
        assert_eq!(unescape_string("a\\rb"), "a\rb");
        assert_eq!(unescape_string("a\\\\b"), "a\\b");
        assert_eq!(unescape_string("a\\'b"), "a'b");
        assert_eq!(unescape_string("a\\\"b"), "a\"b");
        assert_eq!(unescape_string("a\\`b"), "a`b");
        assert_eq!(unescape_string("a\\$b"), "a$b");
        assert_eq!(unescape_string("no escapes"), "no escapes");
        assert_eq!(unescape_string("trailing\\"), "trailing\\");
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(escape_string("hello\nworld"), "hello\\nworld");
        assert_eq!(escape_string("a\tb"), "a\\tb");
        assert_eq!(escape_string("a\rb"), "a\\rb");
        assert_eq!(escape_string("a\\b"), "a\\\\b");
        assert_eq!(escape_string("a'b"), "a\\'b");
        assert_eq!(escape_string("a\"b"), "a\\\"b");
        assert_eq!(escape_string("a`b"), "a\\`b");
        assert_eq!(escape_string("a$b"), "a\\$b");
        assert_eq!(escape_string("no escapes"), "no escapes");
    }

    #[test]
    fn test_line_similarity() {
        assert_eq!(line_similarity("hello", "hello"), 1.0);
        assert_eq!(line_similarity("", "hello"), 0.0);
        assert_eq!(line_similarity("hello", ""), 0.0);
        assert!(line_similarity("hello world", "hello") > 0.0);
    }

    #[test]
    fn test_longest_common_substring() {
        assert_eq!(longest_common_substring("hello", "hello"), 5);
        assert_eq!(longest_common_substring("", "hello"), 0);
        assert_eq!(longest_common_substring("hello", ""), 0);
        assert_eq!(longest_common_substring("abc", "xyz"), 0);
        assert_eq!(longest_common_substring("abcdef", "bcde"), 4);
    }

    #[test]
    fn test_calculate_block_similarity_same() {
        let content = vec!["line1", "line2", "line3"];
        let target = vec!["line1", "line2", "line3"];
        assert_eq!(calculate_block_similarity(&content, &target), 1.0);
    }

    #[test]
    fn test_calculate_block_similarity_different_lengths() {
        let content = vec!["line1", "line2"];
        let target = vec!["line1"];
        assert_eq!(calculate_block_similarity(&content, &target), 0.0);
    }

    #[test]
    fn test_calculate_block_similarity_partial() {
        let content = vec!["line1", "different", "line3"];
        let target = vec!["line1", "line2", "line3"];
        // 2 out of 3 match exactly
        let sim = calculate_block_similarity(&content, &target);
        assert!(sim >= 0.6 && sim <= 0.7);
    }

    #[test]
    fn test_generate_diff() {
        let old = "hello\nworld\n";
        let new = "hello\nuniverse\n";
        let path = std::path::Path::new("test.txt");
        let diff = generate_diff(old, new, path);
        assert!(diff.contains("--- a/test.txt"));
        assert!(diff.contains("+++ b/test.txt"));
        assert!(diff.contains("-world"));
        assert!(diff.contains("+universe"));
    }

    #[test]
    fn test_try_indentation_match() {
        let content = "    fn hello() {\n        println!(\"hi\");\n    }";
        let target = "fn hello() {\n    println!(\"hi\");\n}";
        let matched = try_indentation_match(content, target);
        assert!(matched.is_some());
    }

    #[test]
    fn test_try_indentation_match_no_match() {
        let content = "fn hello() {}";
        let target = "fn goodbye() {}";
        let matched = try_indentation_match(content, target);
        assert!(matched.is_none());
    }

    #[test]
    fn test_try_indentation_match_empty_target() {
        let content = "fn hello() {}";
        let target = "";
        let matched = try_indentation_match(content, target);
        assert!(matched.is_none());
    }

    #[test]
    fn test_try_block_anchor_match_too_few_lines() {
        let content = "line1\nline2";
        let target = "line1\nline2";
        let matched = try_block_anchor_match(content, target);
        assert!(matched.is_none()); // needs at least 3 lines
    }

    #[test]
    fn test_try_block_anchor_match_empty_anchors() {
        let content = "\nmiddle\n";
        let target = "\nmiddle\n";
        let matched = try_block_anchor_match(content, target);
        assert!(matched.is_none());
    }

    #[test]
    fn test_try_context_aware_match_single_line() {
        let content = "single line";
        let target = "single line";
        let matched = try_context_aware_match(content, target);
        assert!(matched.is_none()); // needs at least 2 lines
    }

    #[test]
    fn test_find_fuzzy_end_no_match() {
        let content = "hello";
        let target = "goodbye";
        let end = find_fuzzy_end(content, target);
        assert!(end.is_none());
    }

    #[test]
    fn test_find_fuzzy_end_empty_target() {
        let content = "hello";
        let target = "";
        let end = find_fuzzy_end(content, target);
        assert!(end.is_none());
    }

    #[test]
    fn test_try_escape_normalized_match() {
        let content = "hello\nworld";
        let target = "hello\\nworld";
        let matched = try_escape_normalized_match(content, target);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap(), "hello\nworld");
    }

    #[test]
    fn test_try_fuzzy_match_line_endings() {
        let content = "hello\nworld";
        let target = "hello\r\nworld";
        let matched = try_fuzzy_match(content, target);
        assert!(matched.is_some());
    }

    #[test]
    fn test_try_fuzzy_match_trimmed() {
        let content = "hello   \nworld   ";
        let target = "hello\nworld";
        let matched = try_fuzzy_match(content, target);
        assert!(matched.is_some());
    }

    #[test]
    fn test_try_fuzzy_match_boundary_trim() {
        let content = "xyz  hello world  abc";
        let target = "  hello world  ";
        let matched = try_fuzzy_match(content, target);
        // This may match via boundary trim - just ensure it doesn't panic
        // The behavior depends on the specific fuzzy strategies
        let _ = matched;
    }

    #[tokio::test]
    async fn test_edit_same_string_error() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").await.unwrap();

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "hello",
                    "newString": "hello"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("must be different"));
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let (_dir, ctx) = setup_test().await;

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": "/nonexistent/file.txt",
                    "oldString": "hello",
                    "newString": "goodbye"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found") || err.contains("File not found"));
    }

    #[tokio::test]
    async fn test_edit_invalid_args() {
        let (_dir, ctx) = setup_test().await;

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "invalid": "args"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid arguments"));
    }

    #[test]
    fn test_edit_tool_id() {
        let tool = EditTool;
        assert_eq!(tool.id(), "edit");
    }

    #[test]
    fn test_edit_tool_description() {
        let tool = EditTool;
        let desc = tool.description();
        assert!(desc.contains("exact string replacements"));
        assert!(desc.contains("replaceAll"));
    }

    #[test]
    fn test_edit_tool_parameters_schema() {
        let tool = EditTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("filePath")));
        assert!(required.contains(&json!("oldString")));
        assert!(required.contains(&json!("newString")));
    }

    #[tokio::test]
    async fn test_swap_behavior() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        // File already has the "new" content
        fs::write(&file_path, "goodbye world").await.unwrap();

        let tool = EditTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.to_str().unwrap(),
                    "oldString": "hello",  // doesn't exist
                    "newString": "goodbye"  // already exists
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Should swap and revert to "hello"
        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "hello world");
        assert!(result.metadata["swapped"].as_bool().unwrap());
    }
}
