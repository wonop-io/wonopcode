//! Write tool - write file contents.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use tracing::{debug, warn};

/// Write file contents.
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn id(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        r#"Writes a file to the local filesystem.

Usage:
- This tool will overwrite the existing file if there is one at the provided path.
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless explicitly required."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["filePath", "content"],
            "properties": {
                "filePath": {
                    "type": "string",
                    "description": "The absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let file_path: PathBuf = args["filePath"]
            .as_str()
            .ok_or_else(|| ToolError::validation("filePath is required"))?
            .into();

        let content = args["content"]
            .as_str()
            .ok_or_else(|| ToolError::validation("content is required"))?;

        // Route through sandbox if available
        if let Some(sandbox) = ctx.sandbox() {
            return self
                .write_sandboxed(sandbox.as_ref(), &file_path, content, ctx)
                .await;
        }

        // Non-sandboxed execution: additional security checks required

        // Validate path is within project root
        let canonical_root = ctx
            .root_dir
            .canonicalize()
            .unwrap_or_else(|_| ctx.root_dir.clone());

        // For new files, check the parent directory. For existing files, check the file itself.
        let path_to_check = if file_path.exists() {
            file_path
                .canonicalize()
                .unwrap_or_else(|_| file_path.clone())
        } else {
            // For new files, normalize the path by resolving .. and . components

            normalize_path(&file_path)
        };

        // Ensure path is within project root
        if !path_to_check.starts_with(&canonical_root) {
            warn!(
                path = %file_path.display(),
                root = %ctx.root_dir.display(),
                "Attempted to write file outside project root"
            );
            return Err(ToolError::permission_denied(format!(
                "Cannot write to '{}' - path is outside the project root. \
                All file operations must be within: {}",
                file_path.display(),
                ctx.root_dir.display()
            )));
        }

        // Check for concurrent modifications if file exists and file time tracking is enabled
        if file_path.exists() {
            if let Some(ref file_time) = ctx.file_time {
                file_time
                    .assert_not_modified(&ctx.session_id, &file_path)
                    .await
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?;
            }

            // Take snapshot before writing (if snapshot store is available)
            if let Some(ref snapshot_store) = ctx.snapshot {
                if let Err(e) = snapshot_store
                    .take(
                        &[file_path.clone()],
                        &ctx.session_id,
                        &ctx.message_id,
                        &format!("Before write: {}", file_path.display()),
                    )
                    .await
                {
                    debug!("Failed to take snapshot before write: {}", e);
                }
            }
        }

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write file
        tokio::fs::write(&file_path, content).await?;

        // Update file read time after successful write
        if let Some(ref file_time) = ctx.file_time {
            file_time.record_read(&ctx.session_id, &file_path).await;
        }

        // Generate preview (first 10 lines)
        let preview: String = content.lines().take(10).collect::<Vec<_>>().join("\n");

        Ok(ToolOutput::new(
            format!("Wrote {}", file_path.display()),
            format!("Successfully wrote {} bytes", content.len()),
        )
        .with_metadata(json!({
            "bytes": content.len(),
            "path": file_path.display().to_string(),
            "preview": preview
        })))
    }
}

impl WriteTool {
    /// Write file through sandbox runtime.
    #[allow(clippy::cognitive_complexity)]
    async fn write_sandboxed(
        &self,
        sandbox: &dyn wonopcode_sandbox::SandboxRuntime,
        file_path: &PathBuf,
        content: &str,
        ctx: &ToolContext,
    ) -> ToolResult<ToolOutput> {
        // Convert host path to sandbox path
        let sandbox_path = ctx.to_sandbox_path(file_path);

        debug!(
            host_path = %file_path.display(),
            sandbox_path = %sandbox_path.display(),
            bytes = content.len(),
            "Writing file through sandbox"
        );

        // Check for concurrent modifications if file exists
        let file_exists = sandbox
            .path_exists(&sandbox_path)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Sandbox error: {e}")))?;

        if file_exists {
            if let Some(ref file_time) = ctx.file_time {
                file_time
                    .assert_not_modified(&ctx.session_id, file_path)
                    .await
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?;
            }

            // Take snapshot before writing (if snapshot store is available)
            // Note: Snapshot reads from host path, which is synced with sandbox
            if let Some(ref snapshot_store) = ctx.snapshot {
                if let Err(e) = snapshot_store
                    .take(
                        &[file_path.clone()],
                        &ctx.session_id,
                        &ctx.message_id,
                        &format!("Before write: {}", file_path.display()),
                    )
                    .await
                {
                    debug!("Failed to take snapshot before write: {}", e);
                }
            }
        }

        // Create parent directories in sandbox if needed
        if let Some(parent) = sandbox_path.parent() {
            sandbox
                .create_dir_all(parent)
                .await
                .map_err(|e| ToolError::execution_failed(format!("Sandbox mkdir error: {e}")))?;
        }

        // Write file through sandbox
        sandbox
            .write_file(&sandbox_path, content.as_bytes())
            .await
            .map_err(|e| ToolError::execution_failed(format!("Sandbox write error: {e}")))?;

        // Update file read time after successful write
        if let Some(ref file_time) = ctx.file_time {
            file_time.record_read(&ctx.session_id, file_path).await;
        }

        // Generate preview (first 10 lines)
        let preview: String = content.lines().take(10).collect::<Vec<_>>().join("\n");

        Ok(ToolOutput::new(
            format!("Wrote {}", file_path.display()),
            format!("Successfully wrote {} bytes", content.len()),
        )
        .with_metadata(json!({
            "bytes": content.len(),
            "path": file_path.display().to_string(),
            "sandbox_path": sandbox_path.display().to_string(),
            "sandboxed": true,
            "preview": preview
        })))
    }
}

/// Normalize a path by resolving `.` and `..` components without requiring the path to exist.
fn normalize_path(path: &std::path::Path) -> PathBuf {
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Pop the last component if possible
                components.pop();
            }
            std::path::Component::CurDir => {
                // Skip current dir references
            }
            other => {
                components.push(other);
            }
        }
    }

    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn test_context_with_root(root: PathBuf) -> ToolContext {
        // Canonicalize to handle symlinks (e.g., /var -> /private/var on macOS)
        let canonical_root = root.canonicalize().unwrap_or(root);
        ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: canonical_root.clone(),
            cwd: canonical_root,
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
        }
    }

    #[tokio::test]
    async fn test_write_file() {
        let dir = tempdir().unwrap();
        // Canonicalize the path to handle symlinks
        let canonical_dir = dir.path().canonicalize().unwrap();
        let file_path = canonical_dir.join("test.txt");
        let ctx = test_context_with_root(canonical_dir);

        let tool = WriteTool;
        let result = tool
            .execute(
                json!({
                    "filePath": file_path.display().to_string(),
                    "content": "Hello, world!"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("13 bytes"));

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Hello, world!");
    }

    #[tokio::test]
    async fn test_write_creates_directories() {
        let dir = tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let file_path = canonical_dir.join("nested/dir/test.txt");
        let ctx = test_context_with_root(canonical_dir);

        let tool = WriteTool;
        tool.execute(
            json!({
                "filePath": file_path.display().to_string(),
                "content": "Hello!"
            }),
            &ctx,
        )
        .await
        .unwrap();

        assert!(file_path.exists());
    }

    #[tokio::test]
    async fn test_write_outside_root_denied() {
        let dir = tempdir().unwrap();
        let ctx = test_context_with_root(dir.path().to_path_buf());

        // Try to write outside the project root
        let tool = WriteTool;
        let result = tool
            .execute(
                json!({
                    "filePath": "/tmp/outside/file.txt",
                    "content": "Should fail"
                }),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_write_path_traversal_denied() {
        let dir = tempdir().unwrap();
        let ctx = test_context_with_root(dir.path().to_path_buf());

        // Try path traversal attack
        let evil_path = dir.path().join("subdir/../../etc/passwd");
        let tool = WriteTool;
        let result = tool
            .execute(
                json!({
                    "filePath": evil_path.display().to_string(),
                    "content": "Should fail"
                }),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_path(&PathBuf::from("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_path(&PathBuf::from("/a/./b/./c")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            normalize_path(&PathBuf::from("/a/b/c/../../d")),
            PathBuf::from("/a/d")
        );
    }
}
