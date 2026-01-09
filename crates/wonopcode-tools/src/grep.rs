//! Grep tool - search file contents using native Rust.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::debug;
use wonopcode_sandbox::SandboxCapabilities;

/// Maximum number of matches per file to prevent excessive output.
const MAX_MATCHES_PER_FILE: usize = 100;

/// Maximum total matches across all files.
const MAX_TOTAL_MATCHES: usize = 1000;

/// Search file contents using regex.
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn id(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        r#"Fast content search tool that works with any codebase size.

- Searches file contents using regular expressions
- Supports full regex syntax (eg. "log.*Error", "function\s+\w+")
- Filter files by pattern with the include parameter (eg. "*.js", "*.{ts,tsx}")
- Returns file paths and line numbers with at least one match sorted by modification time
- IMPORTANT: Automatically respects .gitignore, so it won't search in ignored directories like target/, node_modules/, .git/, etc.
- Use this tool instead of bash grep commands for better performance and gitignore support"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in (defaults to current directory)"
                },
                "include": {
                    "type": "string",
                    "description": "File pattern to include in the search (e.g. \"*.js\")"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::validation("pattern is required"))?;

        let base_path = args["path"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Ensure base path is absolute
        let base_path = if base_path.is_absolute() {
            base_path
        } else {
            ctx.cwd.join(&base_path)
        };

        let include = args["include"].as_str();

        // Route through sandbox if available
        if let Some(sandbox) = ctx.sandbox() {
            return self
                .execute_sandboxed(sandbox.as_ref(), pattern, &base_path, include, ctx)
                .await;
        }

        // Non-sandboxed execution using native Rust

        // Compile the regex pattern
        let regex = Regex::new(pattern)
            .map_err(|e| ToolError::validation(format!("Invalid regex pattern: {e}")))?;

        // Parse include pattern for glob matching
        let include_matcher = include.map(build_glob_matcher);

        // Validate base path exists
        if !base_path.exists() {
            return Err(ToolError::validation(format!(
                "Path does not exist: {}",
                base_path.display()
            )));
        }

        debug!(
            pattern = %pattern,
            base_path = %base_path.display(),
            include = ?include,
            "Executing native grep search"
        );

        // Use ignore crate's WalkBuilder to respect .gitignore
        let walker = WalkBuilder::new(&base_path)
            .hidden(true) // Skip hidden files by default
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .follow_links(false)
            .build();

        let mut results: Vec<String> = Vec::new();
        let mut total_matches = 0;

        for entry in walker {
            // Check for cancellation
            if ctx.abort.is_cancelled() {
                return Err(ToolError::Cancelled);
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Apply include filter if specified
            if let Some(ref matcher) = include_matcher {
                if !matches_glob(path, matcher) {
                    continue;
                }
            }

            // Search file contents
            match search_file(path, &regex, MAX_MATCHES_PER_FILE) {
                Ok(matches) => {
                    for (line_num, line_content) in matches {
                        if total_matches >= MAX_TOTAL_MATCHES {
                            results.push(format!(
                                "... truncated (reached {MAX_TOTAL_MATCHES} matches)"
                            ));
                            break;
                        }
                        results.push(format!("{}:{}:{}", path.display(), line_num, line_content));
                        total_matches += 1;
                    }
                }
                Err(_) => {
                    // Skip files that can't be read (binary, permission denied, etc.)
                    continue;
                }
            }

            if total_matches >= MAX_TOTAL_MATCHES {
                break;
            }
        }

        // Sort results by file modification time (newest first)
        // We do this by grouping by file and sorting files
        let output = sort_results_by_mtime(results);
        let match_count = output.lines().count();

        Ok(
            ToolOutput::new(format!("Grep: {pattern} ({match_count} matches)"), output)
                .with_metadata(json!({ "count": match_count })),
        )
    }
}

impl GrepTool {
    /// Execute grep search in sandbox using rg (ripgrep) if available, or grep.
    async fn execute_sandboxed(
        &self,
        sandbox: &dyn wonopcode_sandbox::SandboxRuntime,
        pattern: &str,
        base_path: &Path,
        include: Option<&str>,
        ctx: &ToolContext,
    ) -> ToolResult<ToolOutput> {
        let sandbox_path = ctx.to_sandbox_path(base_path);

        debug!(
            pattern = %pattern,
            host_path = %base_path.display(),
            sandbox_path = %sandbox_path.display(),
            include = ?include,
            "Executing grep in sandbox"
        );

        // Try rg first, fall back to grep if not available
        let cmd = if let Some(glob) = include {
            format!(
                "rg --line-number --no-heading --color=never --max-count=100 '{}' '{}' --glob '{}' 2>/dev/null || \
                 grep -rn '{}' '{}' --include='{}' 2>/dev/null || true",
                escape_shell_arg(pattern),
                sandbox_path.display(),
                escape_shell_arg(glob),
                escape_shell_arg(pattern),
                sandbox_path.display(),
                escape_shell_arg(glob)
            )
        } else {
            format!(
                "rg --line-number --no-heading --color=never --max-count=100 '{}' '{}' 2>/dev/null || \
                 grep -rn '{}' '{}' 2>/dev/null || true",
                escape_shell_arg(pattern),
                sandbox_path.display(),
                escape_shell_arg(pattern),
                sandbox_path.display()
            )
        };

        let result = sandbox
            .execute(
                &cmd,
                &sandbox_path,
                Duration::from_secs(60),
                &SandboxCapabilities::default(),
            )
            .await
            .map_err(|e| ToolError::execution_failed(format!("Sandbox grep failed: {e}")))?;

        // Convert sandbox paths in output to host paths
        let output = convert_sandbox_paths_to_host(&result.stdout, ctx);
        let match_count = output.lines().filter(|l| !l.is_empty()).count();

        Ok(
            ToolOutput::new(format!("Grep: {pattern} ({match_count} matches)"), output)
                .with_metadata(json!({
                    "count": match_count,
                    "sandboxed": true
                })),
        )
    }
}

/// Search a single file for lines matching the regex pattern.
fn search_file(
    path: &Path,
    regex: &Regex,
    max_matches: usize,
) -> std::io::Result<Vec<(usize, String)>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut matches = Vec::new();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue, // Skip lines with encoding issues
        };

        if regex.is_match(&line) {
            matches.push((line_num + 1, line));
            if matches.len() >= max_matches {
                break;
            }
        }
    }

    Ok(matches)
}

/// Build a glob matcher from a pattern string.
/// Supports patterns like "*.js", "*.{ts,tsx}", etc.
fn build_glob_matcher(pattern: &str) -> GlobMatcher {
    GlobMatcher {
        pattern: pattern.to_string(),
    }
}

struct GlobMatcher {
    pattern: String,
}

/// Check if a path matches a glob pattern.
fn matches_glob(path: &Path, matcher: &GlobMatcher) -> bool {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    let pattern = &matcher.pattern;

    // Handle brace expansion like *.{ts,tsx}
    if pattern.contains('{') && pattern.contains('}') {
        if let Some(start) = pattern.find('{') {
            if let Some(end) = pattern.find('}') {
                let prefix = &pattern[..start];
                let suffix = &pattern[end + 1..];
                let alternatives = &pattern[start + 1..end];

                for alt in alternatives.split(',') {
                    let expanded = format!("{}{}{}", prefix, alt.trim(), suffix);
                    if glob_match(&expanded, file_name) {
                        return true;
                    }
                }
                return false;
            }
        }
    }

    glob_match(pattern, file_name)
}

/// Simple glob matching supporting * and ? wildcards.
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut pattern_chars = pattern.chars().peekable();
    let mut text_chars = text.chars().peekable();

    while let Some(p) = pattern_chars.next() {
        match p {
            '*' => {
                // Handle ** as single *
                while pattern_chars.peek() == Some(&'*') {
                    pattern_chars.next();
                }

                // If * is at end, match everything
                if pattern_chars.peek().is_none() {
                    return true;
                }

                // Try to match remaining pattern at each position
                let remaining_pattern: String = pattern_chars.collect();
                while text_chars.peek().is_some() {
                    let remaining_text: String = text_chars.clone().collect();
                    if glob_match(&remaining_pattern, &remaining_text) {
                        return true;
                    }
                    text_chars.next();
                }
                return glob_match(&remaining_pattern, "");
            }
            '?' => {
                // Match exactly one character
                if text_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                // Match literal character (case-insensitive on Windows)
                match text_chars.next() {
                    Some(t) if t == c => {}
                    #[cfg(windows)]
                    Some(t) if t.to_ascii_lowercase() == c.to_ascii_lowercase() => {}
                    _ => return false,
                }
            }
        }
    }

    // Pattern exhausted - check if text is also exhausted
    text_chars.peek().is_none()
}

/// Sort results by file modification time (newest first).
fn sort_results_by_mtime(results: Vec<String>) -> String {
    use std::collections::HashMap;

    // Group results by file
    let mut file_results: HashMap<String, Vec<String>> = HashMap::new();

    for result in results {
        if let Some(colon_pos) = result.find(':') {
            let file_path = result[..colon_pos].to_string();
            file_results.entry(file_path).or_default().push(result);
        }
    }

    // Sort files by modification time
    let mut files: Vec<_> = file_results.keys().cloned().collect();
    files.sort_by(|a, b| {
        let a_time = PathBuf::from(a).metadata().and_then(|m| m.modified()).ok();
        let b_time = PathBuf::from(b).metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    // Collect results in sorted order
    let mut sorted_results = Vec::new();
    for file in files {
        if let Some(results) = file_results.get(&file) {
            sorted_results.extend(results.iter().cloned());
        }
    }

    sorted_results.join("\n")
}

/// Escape a string for use in shell command.
fn escape_shell_arg(s: &str) -> String {
    // Basic escaping - replace single quotes with '\''
    s.replace('\'', "'\\''")
}

/// Convert sandbox paths in output to host paths.
fn convert_sandbox_paths_to_host(output: &str, ctx: &ToolContext) -> String {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            // Output format: filename:lineno:content
            if let Some(colon_pos) = line.find(':') {
                let path_str = &line[..colon_pos];
                let rest = &line[colon_pos..];

                let sandbox_path = PathBuf::from(path_str);
                let host_path = ctx.to_host_path(&sandbox_path);

                format!("{}{}", host_path.display(), rest)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn test_context(cwd: PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: cwd.clone(),
            cwd,
            snapshot: None,
            file_time: None,
            sandbox: None,
        }
    }

    #[test]
    fn test_glob_match_simple() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.txt"));
        assert!(!glob_match("*.rs", "main.rs.bak"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("?.rs", "a.rs"));
        assert!(!glob_match("?.rs", "ab.rs"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("test*", "test"));
        assert!(glob_match("test*", "testing"));
        assert!(glob_match("*test", "test"));
        assert!(glob_match("*test", "mytest"));
        assert!(glob_match("*test*", "testing"));
    }

    #[test]
    fn test_matches_glob_brace() {
        let matcher = build_glob_matcher("*.{ts,tsx}");
        assert!(matches_glob(Path::new("file.ts"), &matcher));
        assert!(matches_glob(Path::new("file.tsx"), &matcher));
        assert!(!matches_glob(Path::new("file.js"), &matcher));
    }

    #[tokio::test]
    async fn test_grep_basic() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("test.txt"),
            "hello world\nfoo bar\nhello again",
        )
        .unwrap();

        let tool = GrepTool;
        let result = tool
            .execute(
                json!({ "pattern": "hello" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("test.txt:1:hello world"));
        assert!(result.output.contains("test.txt:3:hello again"));
        assert!(!result.output.contains("foo bar"));
    }

    #[tokio::test]
    async fn test_grep_with_include() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("test.txt"), "fn main() {}\n").unwrap();

        let tool = GrepTool;
        let result = tool
            .execute(
                json!({ "pattern": "fn main", "include": "*.rs" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("test.rs"));
        assert!(!result.output.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_grep_regex() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "error123\nERROR456\nwarning").unwrap();

        let tool = GrepTool;
        let result = tool
            .execute(
                json!({ "pattern": r"error\d+" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("error123"));
        assert!(!result.output.contains("ERROR456")); // Case sensitive
        assert!(!result.output.contains("warning"));
    }

    #[tokio::test]
    async fn test_grep_invalid_regex() {
        let dir = tempdir().unwrap();
        let tool = GrepTool;
        let result = tool
            .execute(
                json!({ "pattern": "[invalid" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await;

        assert!(result.is_err());
    }
}
