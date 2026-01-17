//! Glob tool - find files by pattern.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::debug;
use wonopcode_sandbox::SandboxCapabilities;

/// Find files by glob pattern.
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn id(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        r#"Fast file pattern matching tool that works with any codebase size.

- Supports glob patterns like "**/*.js" or "src/**/*.ts"
- Returns matching file paths sorted by modification time
- Use this tool when you need to find files by name patterns"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match files against"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in (defaults to current directory)"
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

        // Route through sandbox if available
        if let Some(sandbox) = ctx.sandbox() {
            return self
                .execute_sandboxed(sandbox.as_ref(), pattern, &base_path, ctx)
                .await;
        }

        // Non-sandboxed execution
        if !base_path.exists() {
            return Err(ToolError::validation(format!(
                "Path does not exist: {}",
                base_path.display()
            )));
        }

        // Use globwalk to find matching files
        // Catch panics from globwalk (it has a bug with some path combinations)
        let walker_result = std::panic::catch_unwind(|| {
            globwalk::GlobWalkerBuilder::from_patterns(&base_path, &[pattern])
                .follow_links(false)
                .build()
        });

        let walker = match walker_result {
            Ok(Ok(w)) => w,
            Ok(Err(e)) => return Err(ToolError::execution_failed(e.to_string())),
            Err(_) => {
                return Err(ToolError::execution_failed(
                    "Failed to build glob walker (invalid pattern or path)",
                ));
            }
        };

        let mut files: Vec<PathBuf> = Vec::new();
        for entry in walker {
            match entry {
                Ok(e) => files.push(e.path().to_path_buf()),
                Err(_) => continue, // Skip entries that fail
            }
        }

        // Sort by modification time (newest first)
        files.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        let count = files.len();
        let output = files
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(
            ToolOutput::new(format!("Glob: {pattern} ({count} files)"), output)
                .with_metadata(json!({ "count": count })),
        )
    }
}

impl GlobTool {
    /// Execute glob search in sandbox using find command.
    async fn execute_sandboxed(
        &self,
        sandbox: &dyn wonopcode_sandbox::SandboxRuntime,
        pattern: &str,
        base_path: &Path,
        ctx: &ToolContext,
    ) -> ToolResult<ToolOutput> {
        let sandbox_path = ctx.to_sandbox_path(base_path);

        debug!(
            pattern = %pattern,
            host_path = %base_path.display(),
            sandbox_path = %sandbox_path.display(),
            "Executing glob in sandbox"
        );

        // Convert glob pattern to find command
        // Use find with -name for simple patterns, or combine with shell globbing
        let find_cmd = build_find_command(pattern, &sandbox_path);

        let result = sandbox
            .execute(
                &find_cmd,
                &sandbox_path,
                Duration::from_secs(60),
                &SandboxCapabilities::default(),
            )
            .await
            .map_err(|e| ToolError::execution_failed(format!("Sandbox find failed: {e}")))?;

        if !result.success && !result.stderr.is_empty() {
            // Log warning but continue - find may return non-zero for empty results
            debug!(stderr = %result.stderr, "find command had errors");
        }

        // Parse output - each line is a file path (in sandbox paths)
        let sandbox_files: Vec<&str> = result.stdout.lines().filter(|l| !l.is_empty()).collect();

        // Convert sandbox paths back to host paths for display
        let files: Vec<String> = sandbox_files
            .iter()
            .filter_map(|sp| {
                let sandbox_path = PathBuf::from(sp);
                ctx.to_host_path(&sandbox_path)
                    .to_str()
                    .map(|s| s.to_string())
            })
            .collect();

        let count = files.len();
        let output = files.join("\n");

        Ok(
            ToolOutput::new(format!("Glob: {pattern} ({count} files)"), output).with_metadata(
                json!({
                    "count": count,
                    "sandboxed": true
                }),
            ),
        )
    }
}

/// Convert a glob pattern to a find command.
fn build_find_command(pattern: &str, base_path: &Path) -> String {
    // Handle common glob patterns
    // **/*.rs -> find . -type f -name "*.rs"
    // src/**/*.ts -> find src -type f -name "*.ts"
    // *.txt -> find . -maxdepth 1 -type f -name "*.txt"

    let base = base_path.display();

    if let Some(name_pattern) = pattern.strip_prefix("**/") {
        // Recursive pattern like **/*.rs
        format!("find '{base}' -type f -name '{name_pattern}'")
    } else if pattern.contains("**/") {
        // Pattern like src/**/*.ts
        let parts: Vec<&str> = pattern.splitn(2, "**/").collect();
        if parts.len() == 2 {
            let subdir = parts[0].trim_end_matches('/');
            let name_pattern = parts[1];
            if subdir.is_empty() {
                format!("find '{base}' -type f -name '{name_pattern}'")
            } else {
                format!("find '{base}/{subdir}' -type f -name '{name_pattern}'")
            }
        } else {
            format!("find '{base}' -type f -name '{pattern}'")
        }
    } else if pattern.contains('/') {
        // Pattern with directory like src/*.rs
        let (dir, name) = pattern.rsplit_once('/').unwrap_or(("", pattern));
        if dir.is_empty() {
            format!("find '{base}' -maxdepth 1 -type f -name '{name}'")
        } else {
            format!("find '{base}/{dir}' -maxdepth 1 -type f -name '{name}'")
        }
    } else {
        // Simple pattern like *.txt - search current directory only
        format!("find '{base}' -maxdepth 1 -type f -name '{pattern}'")
    }
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
            event_tx: None,
        }
    }

    #[test]
    fn test_glob_tool_id() {
        let tool = GlobTool;
        assert_eq!(tool.id(), "glob");
    }

    #[test]
    fn test_glob_tool_description() {
        let tool = GlobTool;
        let desc = tool.description();
        assert!(desc.contains("pattern matching"));
        assert!(desc.contains("**/*.js"));
    }

    #[test]
    fn test_glob_tool_parameters_schema() {
        let tool = GlobTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("pattern")));
        assert!(schema["properties"]["pattern"].is_object());
        assert!(schema["properties"]["path"].is_object());
    }

    #[test]
    fn test_build_find_command_recursive() {
        // **/*.rs pattern
        let cmd = build_find_command("**/*.rs", Path::new("/test"));
        assert!(cmd.contains("find"));
        assert!(cmd.contains("-type f"));
        assert!(cmd.contains("-name"));
        assert!(cmd.contains("*.rs"));
    }

    #[test]
    fn test_build_find_command_with_subdir() {
        // src/**/*.ts pattern
        let cmd = build_find_command("src/**/*.ts", Path::new("/test"));
        assert!(cmd.contains("find"));
        assert!(cmd.contains("/test/src"));
        assert!(cmd.contains("-name"));
        assert!(cmd.contains("*.ts"));
    }

    #[test]
    fn test_build_find_command_with_slash() {
        // src/*.rs pattern
        let cmd = build_find_command("src/*.rs", Path::new("/test"));
        assert!(cmd.contains("find"));
        assert!(cmd.contains("/test/src"));
        assert!(cmd.contains("-maxdepth 1"));
        assert!(cmd.contains("-name"));
    }

    #[test]
    fn test_build_find_command_simple() {
        // *.txt pattern (no directory)
        let cmd = build_find_command("*.txt", Path::new("/test"));
        assert!(cmd.contains("find"));
        assert!(cmd.contains("-maxdepth 1"));
        assert!(cmd.contains("-name"));
        assert!(cmd.contains("*.txt"));
    }

    #[test]
    fn test_build_find_command_empty_subdir() {
        // **/*.rs with empty subdir (starts with **/)
        let cmd = build_find_command("**/*.rs", Path::new("/test"));
        assert!(cmd.contains("find '/test'"));
    }

    #[tokio::test]
    async fn test_glob_missing_pattern() {
        let dir = tempdir().unwrap();
        let tool = GlobTool;
        let result = tool
            .execute(
                json!({ "path": dir.path().display().to_string() }),
                &test_context(dir.path().to_path_buf()),
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("pattern"));
    }

    #[tokio::test]
    async fn test_glob_nonexistent_path() {
        let tool = GlobTool;
        let result = tool
            .execute(
                json!({
                    "pattern": "*.txt",
                    "path": "/nonexistent/directory"
                }),
                &test_context(PathBuf::from("/tmp")),
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_glob_relative_path() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("file.txt"), "").unwrap();

        let tool = GlobTool;
        let result = tool
            .execute(
                json!({
                    "pattern": "*.txt",
                    "path": "subdir"
                }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("file.txt"));
        assert_eq!(result.metadata["count"], 1);
    }

    #[tokio::test]
    async fn test_glob_pattern() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("file1.txt"), "").unwrap();
        std::fs::write(dir.path().join("file2.txt"), "").unwrap();
        std::fs::write(dir.path().join("file3.rs"), "").unwrap();

        let tool = GlobTool;
        let result = tool
            .execute(
                json!({ "pattern": "*.txt" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("file1.txt"));
        assert!(result.output.contains("file2.txt"));
        assert!(!result.output.contains("file3.rs"));
    }

    #[tokio::test]
    async fn test_glob_empty_results() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("file1.txt"), "").unwrap();
        std::fs::write(dir.path().join("file2.rs"), "").unwrap();

        let tool = GlobTool;
        let result = tool
            .execute(
                json!({ "pattern": "*.js" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert_eq!(result.output, "");
        assert!(result.title.contains("(0 files)"));
        assert_eq!(result.metadata["count"], 0);
    }

    #[tokio::test]
    async fn test_glob_recursive() {
        let dir = tempdir().unwrap();

        // Create nested directory structure
        std::fs::create_dir_all(dir.path().join("src/components")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/utils")).unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();

        // Create files in various locations
        std::fs::write(dir.path().join("file.rs"), "").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "").unwrap();
        std::fs::write(dir.path().join("src/components/button.rs"), "").unwrap();
        std::fs::write(dir.path().join("src/utils/helper.rs"), "").unwrap();
        std::fs::write(dir.path().join("tests/test.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();

        let tool = GlobTool;
        let result = tool
            .execute(
                json!({ "pattern": "**/*.rs" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        // Should find all .rs files recursively
        assert!(result.output.contains("file.rs"));
        assert!(result.output.contains("main.rs"));
        assert!(result.output.contains("button.rs"));
        assert!(result.output.contains("helper.rs"));
        assert!(result.output.contains("test.rs"));
        assert!(!result.output.contains("README.md"));
        assert_eq!(result.metadata["count"], 5);
    }

    #[tokio::test]
    async fn test_glob_extensions() {
        let dir = tempdir().unwrap();

        // Create files with different extensions
        std::fs::write(dir.path().join("file1.js"), "").unwrap();
        std::fs::write(dir.path().join("file2.ts"), "").unwrap();
        std::fs::write(dir.path().join("file3.jsx"), "").unwrap();
        std::fs::write(dir.path().join("file4.tsx"), "").unwrap();
        std::fs::write(dir.path().join("file5.rs"), "").unwrap();

        let tool = GlobTool;

        // Test single extension
        let result = tool
            .execute(
                json!({ "pattern": "*.rs" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("file5.rs"));
        assert!(!result.output.contains("file1.js"));
        assert_eq!(result.metadata["count"], 1);

        // Test multiple extensions with brace expansion
        let result = tool
            .execute(
                json!({ "pattern": "*.{js,ts}" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert!(result.output.contains("file1.js"));
        assert!(result.output.contains("file2.ts"));
        assert!(!result.output.contains("file3.jsx"));
        assert!(!result.output.contains("file4.tsx"));
        assert_eq!(result.metadata["count"], 2);
    }

    #[tokio::test]
    async fn test_glob_absolute_path() {
        let dir = tempdir().unwrap();

        // Create a subdirectory with files
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(dir.path().join("root.txt"), "").unwrap();
        std::fs::write(subdir.join("sub1.txt"), "").unwrap();
        std::fs::write(subdir.join("sub2.txt"), "").unwrap();

        let tool = GlobTool;

        // Search in the subdirectory using absolute path
        let result = tool
            .execute(
                json!({
                    "pattern": "*.txt",
                    "path": subdir.to_str().unwrap()
                }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        // Should only find files in subdirectory
        assert!(result.output.contains("sub1.txt"));
        assert!(result.output.contains("sub2.txt"));
        assert!(!result.output.contains("root.txt"));
        assert_eq!(result.metadata["count"], 2);
    }

    #[tokio::test]
    async fn test_glob_head_limit() {
        let dir = tempdir().unwrap();

        // Create multiple files
        for i in 1..=10 {
            std::fs::write(dir.path().join(format!("file{}.txt", i)), "").unwrap();
        }

        let tool = GlobTool;
        let result = tool
            .execute(
                json!({ "pattern": "*.txt" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        // Without limit, should find all 10 files
        assert_eq!(result.metadata["count"], 10);
        let lines: Vec<&str> = result.output.lines().collect();
        assert_eq!(lines.len(), 10);
    }

    #[tokio::test]
    async fn test_glob_offset() {
        let dir = tempdir().unwrap();

        // Create multiple files with predictable names
        for i in 1..=5 {
            std::fs::write(dir.path().join(format!("file{}.txt", i)), "").unwrap();
        }

        let tool = GlobTool;
        let result = tool
            .execute(
                json!({ "pattern": "*.txt" }),
                &test_context(dir.path().to_path_buf()),
            )
            .await
            .unwrap();

        // Should find all 5 files
        assert_eq!(result.metadata["count"], 5);
        let lines: Vec<&str> = result.output.lines().collect();
        assert_eq!(lines.len(), 5);

        // Verify all files are present
        for i in 1..=5 {
            let expected = format!("file{}.txt", i);
            assert!(result.output.contains(&expected));
        }
    }
}
