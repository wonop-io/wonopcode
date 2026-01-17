//! MultiEdit tool - apply multiple edits to multiple files atomically.
//!
//! This tool performs batch string replacements across multiple files:
//! - All edits are validated before any are applied
//! - Atomic per-file writes (write to temp, then rename)
//! - Snapshot support for undo
//! - Combined diff output

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tracing::debug;

/// MultiEdit tool for batch string replacements.
pub struct MultiEditTool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MultiEditArgs {
    /// List of edits to apply.
    edits: Vec<EditOperation>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EditOperation {
    /// Path to the file to edit.
    file_path: String,
    /// The text to replace.
    old_string: String,
    /// The replacement text.
    new_string: String,
    /// Replace all occurrences (default false).
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for MultiEditTool {
    fn id(&self) -> &str {
        "multiedit"
    }

    fn description(&self) -> &str {
        r#"Applies multiple edits to multiple files atomically.

Usage:
- Provide an array of edit operations, each with filePath, oldString, newString.
- All edits are validated before any are applied.
- If any edit fails validation, no files are modified.
- Each file is written atomically (temp file + rename).
- Useful for refactoring across multiple files."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["edits"],
            "properties": {
                "edits": {
                    "type": "array",
                    "description": "List of edit operations to apply",
                    "items": {
                        "type": "object",
                        "required": ["filePath", "oldString", "newString"],
                        "properties": {
                            "filePath": {
                                "type": "string",
                                "description": "The path to the file to modify"
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
                    }
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: MultiEditArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        if args.edits.is_empty() {
            return Err(ToolError::validation("No edits provided"));
        }

        // Phase 1: Validate all edits and prepare changes
        // Track original content (before any edits) and current content (after prior edits)
        let mut original_contents: HashMap<PathBuf, String> = HashMap::new();
        let mut file_contents: HashMap<PathBuf, String> = HashMap::new();

        for (idx, op) in args.edits.iter().enumerate() {
            // Resolve path
            let path = resolve_path(&op.file_path, &ctx.cwd, &ctx.root_dir)
                .map_err(|e| ToolError::validation(format!("Edit {}: {}", idx + 1, e)))?;

            // Validate oldString != newString
            if op.old_string == op.new_string {
                return Err(ToolError::validation(format!(
                    "Edit {}: oldString and newString must be different",
                    idx + 1
                )));
            }

            // Get or read file content (may have been modified by previous edit in batch)
            let content = if let Some(content) = file_contents.get(&path) {
                content.clone()
            } else {
                let content = fs::read_to_string(&path).await.map_err(|e| {
                    ToolError::validation(format!(
                        "Edit {}: Failed to read file {}: {}",
                        idx + 1,
                        path.display(),
                        e
                    ))
                })?;
                // Store original content before any modifications
                original_contents.insert(path.clone(), content.clone());
                file_contents.insert(path.clone(), content.clone());
                content
            };

            // Validate and apply edit
            let match_result = find_matches(&content, &op.old_string);
            let modified = match match_result {
                MatchResult::None => {
                    // Try fuzzy matching
                    if let Some(fuzzy_match) = try_fuzzy_match(&content, &op.old_string) {
                        debug!(
                            "Edit {}: Using fuzzy match: original={:?}, fuzzy={:?}",
                            idx + 1,
                            &op.old_string,
                            &fuzzy_match
                        );
                        content.replace(&fuzzy_match, &op.new_string)
                    } else {
                        return Err(ToolError::validation(format!(
                            "Edit {}: oldString not found in file {}",
                            idx + 1,
                            path.display()
                        )));
                    }
                }
                MatchResult::Single => content.replacen(&op.old_string, &op.new_string, 1),
                MatchResult::Multiple(count) => {
                    if op.replace_all {
                        content.replace(&op.old_string, &op.new_string)
                    } else {
                        return Err(ToolError::validation(format!(
                            "Edit {}: oldString found {} times in {}. Use replaceAll or provide more context.",
                            idx + 1,
                            count,
                            path.display()
                        )));
                    }
                }
            };

            // Update file_contents for subsequent edits to same file
            file_contents.insert(path.clone(), modified);
        }

        // Collect unique files and their final contents (original -> modified)
        let mut final_contents: HashMap<PathBuf, (String, String)> = HashMap::new();
        for (path, modified) in &file_contents {
            let original = original_contents.get(path).cloned().unwrap_or_default();
            final_contents.insert(path.clone(), (original, modified.clone()));
        }

        // Phase 2: Take snapshots
        let files_to_snapshot: Vec<PathBuf> = final_contents.keys().cloned().collect();
        if let Some(ref snapshot_store) = ctx.snapshot {
            if let Err(e) = snapshot_store
                .take(
                    &files_to_snapshot,
                    &ctx.session_id,
                    &ctx.message_id,
                    &format!("Before multiedit: {} files", files_to_snapshot.len()),
                )
                .await
            {
                debug!("Failed to take snapshot before multiedit: {}", e);
            }
        }

        // Phase 3: Write all files atomically
        for (path, (_, modified)) in &final_contents {
            let temp_path = path.with_extension("tmp");
            fs::write(&temp_path, modified).await.map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to write temp file {}: {}",
                    temp_path.display(),
                    e
                ))
            })?;

            fs::rename(&temp_path, path).await.map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to rename file {}: {}",
                    path.display(),
                    e
                ))
            })?;
        }

        // Phase 4: Generate output
        let mut diff_output = String::new();
        let mut total_additions = 0;
        let mut total_deletions = 0;

        for (path, (original, modified)) in &final_contents {
            let diff = generate_diff(original, modified, path);
            if !diff_output.is_empty() {
                diff_output.push('\n');
            }
            diff_output.push_str(&diff);

            // Calculate stats
            let old_lines = original.lines().count();
            let new_lines = modified.lines().count();
            total_additions += new_lines.saturating_sub(old_lines);
            total_deletions += old_lines.saturating_sub(new_lines);
        }

        let title = format!(
            "Edited {} file(s) with {} edit(s)",
            final_contents.len(),
            args.edits.len()
        );

        Ok(ToolOutput::new(title, diff_output).with_metadata(json!({
            "files": final_contents.len(),
            "edits": args.edits.len(),
            "additions": total_additions,
            "deletions": total_deletions,
            "paths": final_contents.keys().map(|p| p.display().to_string()).collect::<Vec<_>>()
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
fn try_fuzzy_match(content: &str, target: &str) -> Option<String> {
    // Strategy 1: Normalize line endings
    let normalized_target = target.replace("\r\n", "\n");
    if content.contains(&normalized_target) {
        return Some(normalized_target);
    }

    // Strategy 2: Trim trailing whitespace from each line
    let trimmed_target: String = target
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if content.contains(&trimmed_target) {
        return Some(trimmed_target);
    }

    // Strategy 3: Try with leading/trailing whitespace stripped
    let stripped_target = target.trim();
    if stripped_target != target && content.contains(stripped_target) {
        return Some(stripped_target.to_string());
    }

    // Strategy 4: Handle indentation differences
    if let Some(matched) = try_indentation_match(content, target) {
        return Some(matched);
    }

    None
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
fn resolve_path(
    path: &str,
    cwd: &std::path::Path,
    root_dir: &std::path::Path,
) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);

    if path.is_absolute() {
        if !path.exists() {
            return Err(format!("File not found: {}", path.display()));
        }
        Ok(path)
    } else {
        // Try relative to cwd first
        let cwd_path = cwd.join(&path);
        if cwd_path.exists() {
            return Ok(cwd_path);
        }

        // Try relative to root
        let root_path = root_dir.join(&path);
        if root_path.exists() {
            return Ok(root_path);
        }

        Err(format!(
            "File not found: {} (tried {} and {})",
            path.display(),
            cwd_path.display(),
            root_path.display()
        ))
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

    #[test]
    fn test_multiedit_tool_id() {
        let tool = MultiEditTool;
        assert_eq!(tool.id(), "multiedit");
    }

    #[test]
    fn test_multiedit_tool_description() {
        let tool = MultiEditTool;
        let desc = tool.description();
        assert!(desc.contains("multiple edits"));
        assert!(desc.contains("atomically"));
    }

    #[test]
    fn test_multiedit_tool_parameters_schema() {
        let tool = MultiEditTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("edits")));
        assert!(schema["properties"]["edits"].is_object());
    }

    #[test]
    fn test_find_matches_none() {
        let result = find_matches("hello world", "xyz");
        assert!(matches!(result, MatchResult::None));
    }

    #[test]
    fn test_find_matches_single() {
        let result = find_matches("hello world", "hello");
        assert!(matches!(result, MatchResult::Single));
    }

    #[test]
    fn test_find_matches_multiple() {
        let result = find_matches("hello hello hello", "hello");
        assert!(matches!(result, MatchResult::Multiple(3)));
    }

    #[test]
    fn test_try_fuzzy_match_line_endings() {
        let content = "line1\nline2\nline3";
        let target = "line1\r\nline2";
        let result = try_fuzzy_match(content, target);
        assert!(result.is_some());
    }

    #[test]
    fn test_try_fuzzy_match_trailing_whitespace() {
        let content = "line1\nline2";
        let target = "line1  \nline2  "; // trailing whitespace
        let result = try_fuzzy_match(content, target);
        assert!(result.is_some());
    }

    #[test]
    fn test_try_fuzzy_match_stripped() {
        let content = "prefix target suffix";
        let target = "  target  "; // leading/trailing whitespace
        let result = try_fuzzy_match(content, target);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "target");
    }

    #[test]
    fn test_try_fuzzy_match_no_match() {
        let content = "hello world";
        let target = "xyz";
        let result = try_fuzzy_match(content, target);
        assert!(result.is_none());
    }

    #[test]
    fn test_try_indentation_match() {
        let content = "    fn test() {\n        body\n    }";
        let target = "fn test() {\n    body\n}"; // different indentation
        let result = try_indentation_match(content, target);
        assert!(result.is_some());
    }

    #[test]
    fn test_try_indentation_match_empty_target() {
        let content = "hello world";
        let target = "";
        let result = try_indentation_match(content, target);
        assert!(result.is_none());
    }

    #[test]
    fn test_try_indentation_match_empty_first_line() {
        let content = "hello world";
        let target = "   \nmore";
        let result = try_indentation_match(content, target);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_path_absolute() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "content").unwrap();

        let result = resolve_path(file_path.to_str().unwrap(), dir.path(), dir.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path);
    }

    #[test]
    fn test_resolve_path_relative_to_cwd() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "content").unwrap();

        let result = resolve_path("test.txt", dir.path(), dir.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path);
    }

    #[test]
    fn test_resolve_path_not_found() {
        let dir = TempDir::new().unwrap();
        let result = resolve_path("nonexistent.txt", dir.path(), dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_generate_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";
        let path = PathBuf::from("test.txt");
        let diff = generate_diff(old, new, &path);
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+modified"));
        assert!(diff.contains("--- a/test.txt"));
        assert!(diff.contains("+++ b/test.txt"));
    }

    #[test]
    fn test_generate_diff_no_trailing_newline() {
        let old = "no newline at end";
        let new = "different content";
        let path = PathBuf::from("test.txt");
        let diff = generate_diff(old, new, &path);
        // Should still produce valid diff
        assert!(!diff.is_empty());
    }

    #[test]
    fn test_edit_operation_deserialization() {
        let op: EditOperation = serde_json::from_value(json!({
            "filePath": "/test/file.txt",
            "oldString": "old",
            "newString": "new"
        }))
        .unwrap();
        assert_eq!(op.file_path, "/test/file.txt");
        assert_eq!(op.old_string, "old");
        assert_eq!(op.new_string, "new");
        assert!(!op.replace_all);
    }

    #[test]
    fn test_edit_operation_with_replace_all() {
        let op: EditOperation = serde_json::from_value(json!({
            "filePath": "/test/file.txt",
            "oldString": "old",
            "newString": "new",
            "replaceAll": true
        }))
        .unwrap();
        assert!(op.replace_all);
    }

    #[tokio::test]
    async fn test_invalid_args() {
        let (_, ctx) = setup_test().await;
        let tool = MultiEditTool;
        let result = tool.execute(json!({ "not_edits": [] }), &ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid arguments"));
    }

    #[tokio::test]
    async fn test_same_old_new_string() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").await.unwrap();

        let tool = MultiEditTool;
        let result = tool
            .execute(
                json!({
                    "edits": [{
                        "filePath": file_path.to_str().unwrap(),
                        "oldString": "hello",
                        "newString": "hello"  // same as old
                    }]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("must be different"));
    }

    #[tokio::test]
    async fn test_multiple_matches_without_replace_all() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo foo foo").await.unwrap();

        let tool = MultiEditTool;
        let result = tool
            .execute(
                json!({
                    "edits": [{
                        "filePath": file_path.to_str().unwrap(),
                        "oldString": "foo",
                        "newString": "bar"
                        // replaceAll not set, should fail
                    }]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("found 3 times"));
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let (dir, ctx) = setup_test().await;
        let tool = MultiEditTool;
        let result = tool
            .execute(
                json!({
                    "edits": [{
                        "filePath": dir.path().join("nonexistent.txt").to_str().unwrap(),
                        "oldString": "foo",
                        "newString": "bar"
                    }]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_single_edit() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").await.unwrap();

        let tool = MultiEditTool;
        let result = tool
            .execute(
                json!({
                    "edits": [{
                        "filePath": file_path.to_str().unwrap(),
                        "oldString": "hello",
                        "newString": "goodbye"
                    }]
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
    async fn test_multiple_edits_same_file() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo bar baz").await.unwrap();

        let tool = MultiEditTool;
        let result = tool
            .execute(
                json!({
                    "edits": [
                        {
                            "filePath": file_path.to_str().unwrap(),
                            "oldString": "foo",
                            "newString": "AAA"
                        },
                        {
                            "filePath": file_path.to_str().unwrap(),
                            "oldString": "baz",
                            "newString": "CCC"
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "AAA bar CCC");
        assert_eq!(result.metadata["edits"], 2);
        assert_eq!(result.metadata["files"], 1);
    }

    #[tokio::test]
    async fn test_multiple_files() {
        let (dir, ctx) = setup_test().await;
        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        fs::write(&file1, "hello world").await.unwrap();
        fs::write(&file2, "goodbye moon").await.unwrap();

        let tool = MultiEditTool;
        let result = tool
            .execute(
                json!({
                    "edits": [
                        {
                            "filePath": file1.to_str().unwrap(),
                            "oldString": "world",
                            "newString": "universe"
                        },
                        {
                            "filePath": file2.to_str().unwrap(),
                            "oldString": "moon",
                            "newString": "sun"
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        let content1 = fs::read_to_string(&file1).await.unwrap();
        let content2 = fs::read_to_string(&file2).await.unwrap();
        assert_eq!(content1, "hello universe");
        assert_eq!(content2, "goodbye sun");
        assert_eq!(result.metadata["files"], 2);
        assert_eq!(result.metadata["edits"], 2);
    }

    #[tokio::test]
    async fn test_validation_failure_no_changes() {
        let (dir, ctx) = setup_test().await;
        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        fs::write(&file1, "hello world").await.unwrap();
        fs::write(&file2, "goodbye moon").await.unwrap();

        let tool = MultiEditTool;
        // Second edit should fail - file2 doesn't contain "xyz"
        let result = tool
            .execute(
                json!({
                    "edits": [
                        {
                            "filePath": file1.to_str().unwrap(),
                            "oldString": "world",
                            "newString": "universe"
                        },
                        {
                            "filePath": file2.to_str().unwrap(),
                            "oldString": "xyz",
                            "newString": "abc"
                        }
                    ]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        // First file should NOT have been modified due to atomic behavior
        let content1 = fs::read_to_string(&file1).await.unwrap();
        assert_eq!(content1, "hello world");
    }

    #[tokio::test]
    async fn test_empty_edits() {
        let (_, ctx) = setup_test().await;

        let tool = MultiEditTool;
        let result = tool.execute(json!({ "edits": [] }), &ctx).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No edits"));
    }

    #[tokio::test]
    async fn test_replace_all() {
        let (dir, ctx) = setup_test().await;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo bar foo baz foo").await.unwrap();

        let tool = MultiEditTool;
        let _result = tool
            .execute(
                json!({
                    "edits": [{
                        "filePath": file_path.to_str().unwrap(),
                        "oldString": "foo",
                        "newString": "qux",
                        "replaceAll": true
                    }]
                }),
                &ctx,
            )
            .await
            .unwrap();

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "qux bar qux baz qux");
    }
}
