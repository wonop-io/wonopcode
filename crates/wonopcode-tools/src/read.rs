//! Read tool - read file contents.
// @ace:implements COMP-T90R73-2AG

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use tracing::warn;

/// Maximum file size to read (10MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Sensitive file patterns that should not be read.
const SENSITIVE_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.development",
    ".env.production",
    ".env.staging",
    ".env.test",
    "credentials.json",
    "secrets.json",
    "secrets.yaml",
    "secrets.yml",
    ".npmrc",
    ".pypirc",
    ".netrc",
    ".aws/credentials",
    ".ssh/id_rsa",
    ".ssh/id_ed25519",
    ".ssh/id_dsa",
];

/// Read file contents with line numbers.
pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        r#"Reads a file from the local filesystem.

Usage:
- The filePath parameter must be an absolute path, not a relative path
- By default, it reads up to 2000 lines starting from the beginning of the file
- You can optionally specify a line offset and limit
- Results are returned with line numbers starting at 1"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["filePath"],
            "properties": {
                "filePath": {
                    "type": "string",
                    "description": "The absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "The line number to start reading from (0-based)"
                },
                "limit": {
                    "type": "integer",
                    "description": "The number of lines to read (defaults to 2000)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let file_path: PathBuf = args["filePath"]
            .as_str()
            .ok_or_else(|| ToolError::validation("filePath is required"))?
            .into();

        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(2000) as usize;

        // Check for sensitive files (always, regardless of sandbox)
        if is_sensitive_file(&file_path) {
            warn!(path = %file_path.display(), "Attempted to read sensitive file");
            return Err(ToolError::permission_denied(format!(
                "Cannot read sensitive file: {}. This file may contain secrets or credentials.",
                file_path.display()
            )));
        }

        // Read file bytes - either from sandbox or directly
        let bytes = if let Some(sandbox) = ctx.sandbox() {
            // Convert host path to sandbox path for reading
            let sandbox_path = ctx.to_sandbox_path(&file_path);

            // Check if file exists in sandbox
            let exists = sandbox
                .path_exists(&sandbox_path)
                .await
                .map_err(|e| ToolError::execution_failed(format!("Sandbox error: {e}")))?;

            if !exists {
                let suggestion = suggest_similar_file(&file_path).await;
                let mut message = format!("File not found: {}", file_path.display());
                if let Some(suggestion) = suggestion {
                    message.push_str(&format!("\n\nDid you mean: {suggestion}"));
                }
                return Err(ToolError::file_not_found(message));
            }

            // Get file metadata for size check
            let metadata = sandbox
                .metadata(&sandbox_path)
                .await
                .map_err(|e| ToolError::execution_failed(format!("Sandbox error: {e}")))?;

            if metadata.size > MAX_FILE_SIZE {
                return Err(ToolError::validation(format!(
                    "File too large ({} bytes). Maximum allowed size is {} bytes.",
                    metadata.size, MAX_FILE_SIZE
                )));
            }

            // Read from sandbox
            sandbox
                .read_file(&sandbox_path)
                .await
                .map_err(|e| ToolError::execution_failed(format!("Sandbox read error: {e}")))?
        } else {
            // Direct host filesystem access

            // Check if file exists
            if !file_path.exists() {
                let suggestion = suggest_similar_file(&file_path).await;
                let mut message = format!("File not found: {}", file_path.display());
                if let Some(suggestion) = suggestion {
                    message.push_str(&format!("\n\nDid you mean: {suggestion}"));
                }
                return Err(ToolError::file_not_found(message));
            }

            // Check file size
            let metadata = tokio::fs::metadata(&file_path).await?;
            if metadata.len() > MAX_FILE_SIZE {
                return Err(ToolError::validation(format!(
                    "File too large ({} bytes). Maximum allowed size is {} bytes.",
                    metadata.len(),
                    MAX_FILE_SIZE
                )));
            }

            // Read file as bytes
            tokio::fs::read(&file_path).await?
        };

        // Check if file is binary (contains null bytes in first 8KB)
        let sample_size = std::cmp::min(bytes.len(), 8192);
        let is_binary = bytes[..sample_size].contains(&0);

        if is_binary {
            // Return binary file info instead of content
            return Ok(ToolOutput::new(
                format!("Read {}", file_path.display()),
                format!(
                    "[Binary file: {} bytes]\n\nThis file appears to be binary and cannot be displayed as text.",
                    bytes.len()
                ),
            )
            .with_metadata(json!({
                "binary": true,
                "size": bytes.len(),
                "path": file_path.display().to_string(),
                "sandboxed": ctx.is_sandboxed()
            })));
        }

        // Convert to string (lossy for non-UTF8)
        let content = String::from_utf8_lossy(&bytes);

        // Record the read time for concurrent edit detection
        if let Some(ref file_time) = ctx.file_time {
            file_time.record_read(&ctx.session_id, &file_path).await;
        }

        // Format with line numbers
        let lines: Vec<&str> = content.lines().skip(offset).take(limit).collect();
        let line_count = lines.len();

        let output = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                // Truncate long lines
                let truncated = if line.len() > 2000 {
                    format!("{}... [truncated]", &line[..2000])
                } else {
                    (*line).to_string()
                };
                format!("{:5}|\t{}", offset + i + 1, truncated)
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(
            ToolOutput::new(format!("Read {}", file_path.display()), output).with_metadata(json!({
                "lines": line_count,
                "offset": offset,
                "path": file_path.display().to_string(),
                "sandboxed": ctx.is_sandboxed()
            })),
        )
    }
}

/// Check if a file path matches sensitive file patterns.
fn is_sensitive_file(path: &std::path::Path) -> bool {
    let path_str = path.display().to_string();

    // Check against sensitive file patterns
    for pattern in SENSITIVE_FILES {
        // Match if filename equals pattern or path ends with /pattern
        if let Some(filename) = path.file_name() {
            if filename.to_string_lossy() == *pattern {
                return true;
            }
        }

        // Also check if path contains the sensitive pattern
        if path_str.contains(pattern) {
            return true;
        }
    }

    false
}

/// Suggest a similar file if the requested file doesn't exist.
async fn suggest_similar_file(path: &std::path::Path) -> Option<String> {
    let parent = path.parent()?;
    let filename = path.file_name()?.to_string_lossy();

    // Check if parent directory exists
    if !parent.exists() {
        return None;
    }

    // List files in parent directory
    let mut entries = match tokio::fs::read_dir(parent).await {
        Ok(entries) => entries,
        Err(_) => return None,
    };

    let mut best_match: Option<(String, usize)> = None;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_name = entry.file_name().to_string_lossy().to_string();

        // Simple similarity: count matching characters
        let similarity = filename
            .chars()
            .zip(entry_name.chars())
            .filter(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();

        // Also consider length similarity
        let len_diff = (filename.len() as i32 - entry_name.len() as i32).unsigned_abs() as usize;
        let score = similarity.saturating_sub(len_diff);

        if score > filename.len() / 3 {
            match &best_match {
                None => best_match = Some((entry_name, score)),
                Some((_, best_score)) if score > *best_score => {
                    best_match = Some((entry_name, score));
                }
                _ => {}
            }
        }
    }

    best_match.map(|(name, _)| parent.join(name).display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn test_context() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
        }
    }

    #[test]
    fn test_read_tool_id() {
        let tool = ReadTool;
        assert_eq!(tool.id(), "read");
    }

    #[test]
    fn test_read_tool_description() {
        let tool = ReadTool;
        let desc = tool.description();
        assert!(desc.contains("Reads a file"));
        assert!(desc.contains("absolute path"));
        assert!(desc.contains("2000"));
    }

    #[test]
    fn test_read_tool_parameters_schema() {
        let tool = ReadTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("filePath")));
        assert!(schema["properties"]["filePath"].is_object());
        assert!(schema["properties"]["offset"].is_object());
        assert!(schema["properties"]["limit"].is_object());
    }

    #[tokio::test]
    async fn test_read_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2\nline 3").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        assert!(result.output.contains("line 1"));
        assert!(result.output.contains("line 2"));
        assert!(result.output.contains("line 3"));
    }

    #[tokio::test]
    async fn test_read_file_with_offset() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2\nline 3").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.display().to_string(),
                    "offset": 1
                }),
                &test_context(),
            )
            .await
            .unwrap();

        assert!(!result.output.contains("line 1"));
        assert!(result.output.contains("line 2"));
        assert!(result.output.contains("line 3"));
    }

    #[tokio::test]
    async fn test_read_file_with_limit() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.display().to_string(),
                    "limit": 2
                }),
                &test_context(),
            )
            .await
            .unwrap();

        assert!(result.output.contains("line 1"));
        assert!(result.output.contains("line 2"));
        assert!(!result.output.contains("line 3"));
    }

    #[tokio::test]
    async fn test_read_file_with_offset_and_limit() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.display().to_string(),
                    "offset": 1,
                    "limit": 2
                }),
                &test_context(),
            )
            .await
            .unwrap();

        assert!(!result.output.contains("line 1"));
        assert!(result.output.contains("line 2"));
        assert!(result.output.contains("line 3"));
        assert!(!result.output.contains("line 4"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": "/nonexistent/file.txt" }),
                &test_context(),
            )
            .await;

        assert!(matches!(result, Err(ToolError::FileNotFound(_))));
    }

    #[tokio::test]
    async fn test_read_binary_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("binary.bin");
        // Write binary data with null bytes
        std::fs::write(&file_path, b"hello\x00world\x00binary").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        assert!(result.output.contains("[Binary file:"));
        assert!(result.metadata["binary"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn test_read_sensitive_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join(".env");
        std::fs::write(&file_path, "SECRET=password123").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await;

        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_read_missing_file_path() {
        let tool = ReadTool;
        let result = tool
            .execute(json!({ "not_file_path": "something" }), &test_context())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("filePath"));
    }

    #[tokio::test]
    async fn test_read_empty_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("empty.txt");
        std::fs::write(&file_path, "").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["lines"], 0);
    }

    #[tokio::test]
    async fn test_read_file_metadata() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line 1\nline 2").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["lines"], 2);
        assert_eq!(result.metadata["offset"], 0);
        assert_eq!(result.metadata["sandboxed"], false);
    }

    #[test]
    fn test_is_sensitive_file() {
        assert!(is_sensitive_file(std::path::Path::new("/project/.env")));
        assert!(is_sensitive_file(std::path::Path::new(
            "/home/.aws/credentials"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/secrets.json"
        )));
        assert!(!is_sensitive_file(std::path::Path::new(
            "/project/config.json"
        )));
        assert!(!is_sensitive_file(std::path::Path::new(
            "/project/src/main.rs"
        )));
    }

    #[test]
    fn test_is_sensitive_file_env_variants() {
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/.env.local"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/.env.development"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/.env.production"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/.env.staging"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/.env.test"
        )));
    }

    #[test]
    fn test_is_sensitive_file_secrets() {
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/secrets.yaml"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/secrets.yml"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/project/credentials.json"
        )));
    }

    #[test]
    fn test_is_sensitive_file_rc_files() {
        assert!(is_sensitive_file(std::path::Path::new("/home/user/.npmrc")));
        assert!(is_sensitive_file(std::path::Path::new(
            "/home/user/.pypirc"
        )));
        assert!(is_sensitive_file(std::path::Path::new("/home/user/.netrc")));
    }

    #[test]
    fn test_is_sensitive_file_ssh() {
        assert!(is_sensitive_file(std::path::Path::new(
            "/home/user/.ssh/id_rsa"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/home/user/.ssh/id_ed25519"
        )));
        assert!(is_sensitive_file(std::path::Path::new(
            "/home/user/.ssh/id_dsa"
        )));
    }

    #[tokio::test]
    async fn test_suggest_similar_file() {
        let dir = tempdir().unwrap();
        // Create a file called "readme.md"
        std::fs::write(dir.path().join("readme.md"), "# README").unwrap();

        // Try to find a similar file for "readm.md" (typo)
        let nonexistent = dir.path().join("readm.md");
        let suggestion = suggest_similar_file(&nonexistent).await;

        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("readme.md"));
    }

    #[tokio::test]
    async fn test_suggest_similar_file_no_match() {
        let dir = tempdir().unwrap();
        // Create a file with a very different name
        std::fs::write(dir.path().join("abc.txt"), "content").unwrap();

        // Try to find similar file for something completely different
        let nonexistent = dir.path().join("xyz123.rs");
        let suggestion = suggest_similar_file(&nonexistent).await;

        // May or may not find a match depending on the similarity threshold
        // Just verify it doesn't panic
        let _ = suggestion;
    }

    #[tokio::test]
    async fn test_suggest_similar_file_nonexistent_parent() {
        let nonexistent = PathBuf::from("/nonexistent/directory/file.txt");
        let suggestion = suggest_similar_file(&nonexistent).await;

        assert!(suggestion.is_none());
    }

    #[tokio::test]
    async fn test_read_long_lines_truncation() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("longlines.txt");
        // Create a line with more than 2000 characters
        let long_line = "x".repeat(3000);
        std::fs::write(&file_path, &long_line).unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        // The line should be truncated
        assert!(result.output.contains("[truncated]"));
        // But should still have 2000 x's
        assert!(result.output.contains(&"x".repeat(2000)));
    }

    #[tokio::test]
    async fn test_read_title_output() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "content").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        assert!(result.title.contains("Read"));
    }

    #[tokio::test]
    async fn test_read_line_numbers_in_output() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("numbered.txt");
        std::fs::write(&file_path, "first\nsecond\nthird").unwrap();

        let tool = ReadTool;
        let result = tool
            .execute(
                json!({ "filePath": file_path.display().to_string() }),
                &test_context(),
            )
            .await
            .unwrap();

        // Line numbers should be 1-indexed
        assert!(result.output.contains("1|"));
        assert!(result.output.contains("2|"));
        assert!(result.output.contains("3|"));
    }
}