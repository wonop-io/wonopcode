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
}
