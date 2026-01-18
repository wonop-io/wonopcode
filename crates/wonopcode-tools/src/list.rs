//! List tool - directory listing with tree structure.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Default patterns to ignore (common build/cache directories).
const IGNORE_PATTERNS: &[&str] = &[
    "node_modules",
    "__pycache__",
    ".git",
    "dist",
    "build",
    "target",
    "vendor",
    "bin",
    "obj",
    ".idea",
    ".vscode",
    ".zig-cache",
    "zig-out",
    ".coverage",
    "coverage",
    "tmp",
    "temp",
    ".cache",
    "cache",
    "logs",
    ".venv",
    "venv",
    "env",
];

/// Maximum number of files to list.
const LIMIT: usize = 100;

/// List files and directories in a tree structure.
pub struct ListTool;

#[async_trait]
impl Tool for ListTool {
    fn id(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        r#"Lists files and directories in a given path with tree structure.

- The path parameter must be absolute; omit it to use the current workspace directory
- You can optionally provide an array of glob patterns to ignore
- Respects .gitignore files
- You should generally prefer the Glob and Grep tools if you know which directories to search"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute path to the directory to list (must be absolute, not relative)"
                },
                "ignore": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of glob patterns to ignore"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let search_path = args["path"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Collect additional ignore patterns from args
        let mut ignore_patterns: Vec<String> =
            IGNORE_PATTERNS.iter().map(|p| format!("!{p}/**")).collect();

        if let Some(extra) = args["ignore"].as_array() {
            for pattern in extra {
                if let Some(p) = pattern.as_str() {
                    ignore_patterns.push(format!("!{p}"));
                }
            }
        }

        // Use globwalk to get all files, respecting ignores
        let patterns: Vec<&str> = vec!["**/*"];
        let walker = globwalk::GlobWalkerBuilder::from_patterns(&search_path, &patterns)
            .follow_links(false)
            .build()
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Also build an ignore walker to check against patterns
        let ignore_walker = ignore::WalkBuilder::new(&search_path)
            .hidden(false) // Include hidden files
            .git_ignore(true) // Respect .gitignore
            .git_global(true)
            .git_exclude(true)
            .build();

        // Collect paths from ignore walker (respects .gitignore)
        let mut gitignore_paths: BTreeSet<PathBuf> = BTreeSet::new();
        for entry in ignore_walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                if let Ok(rel) = path.strip_prefix(&search_path) {
                    gitignore_paths.insert(rel.to_path_buf());
                }
            }
        }

        // Filter files using both globwalk and gitignore
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let rel_path = match path.strip_prefix(&search_path) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };

            // Check if path is in gitignore-allowed paths
            if !gitignore_paths.contains(&rel_path) {
                continue;
            }

            // Check against our hardcoded ignore patterns
            let path_str = rel_path.to_string_lossy();
            let should_ignore = IGNORE_PATTERNS
                .iter()
                .any(|p| path_str.starts_with(p) || path_str.contains(&format!("/{p}/")));

            if should_ignore {
                continue;
            }

            files.push(rel_path);

            if files.len() >= LIMIT {
                break;
            }
        }

        // Sort files alphabetically
        files.sort();

        // Build tree structure
        let tree_output = build_tree(&search_path, &files);

        let count = files.len();
        let truncated = count >= LIMIT;
        let title = if truncated {
            format!(
                "{} (showing {} files, truncated)",
                search_path.display(),
                count
            )
        } else {
            format!("{} ({} files)", search_path.display(), count)
        };

        Ok(ToolOutput::new(title, tree_output).with_metadata(json!({
            "count": count,
            "truncated": truncated,
            "path": search_path.display().to_string()
        })))
    }
}

/// Build a tree-structured output from a list of file paths.
fn build_tree(root: &Path, files: &[PathBuf]) -> String {
    // Track directories and their files
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut files_by_dir: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();

    for file in files {
        // Add all parent directories
        let mut current = file.parent();
        while let Some(dir) = current {
            if dir.as_os_str().is_empty() {
                dirs.insert(PathBuf::from("."));
            } else {
                dirs.insert(dir.to_path_buf());
            }
            current = dir.parent();
        }
        dirs.insert(PathBuf::from(".")); // Root

        // Add file to its directory
        let dir = file
            .parent()
            .map(|p| {
                if p.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    p.to_path_buf()
                }
            })
            .unwrap_or_else(|| PathBuf::from("."));

        let filename = file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        files_by_dir.entry(dir).or_default().push(filename);
    }

    // Sort files in each directory
    for files in files_by_dir.values_mut() {
        files.sort();
    }

    // Render tree
    let mut output = format!("{}/\n", root.display());
    output.push_str(&render_dir(&PathBuf::from("."), 0, &dirs, &files_by_dir));

    output
}

/// Render a directory and its contents recursively.
fn render_dir(
    dir_path: &PathBuf,
    depth: usize,
    all_dirs: &BTreeSet<PathBuf>,
    files_by_dir: &BTreeMap<PathBuf, Vec<String>>,
) -> String {
    let mut output = String::new();
    let _indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);

    // Get child directories
    let children: Vec<_> = all_dirs
        .iter()
        .filter(|d| {
            if let Some(parent) = d.parent() {
                let parent_path = if parent.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    parent.to_path_buf()
                };
                parent_path == *dir_path && *d != dir_path
            } else {
                false
            }
        })
        .collect();

    // Render subdirectories first
    for child in &children {
        if let Some(name) = child.file_name() {
            output.push_str(&format!("{}{}/\n", child_indent, name.to_string_lossy()));
            output.push_str(&render_dir(child, depth + 1, all_dirs, files_by_dir));
        }
    }

    // Render files in this directory
    if let Some(files) = files_by_dir.get(dir_path) {
        for file in files {
            output.push_str(&format!("{child_indent}{file}\n"));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    async fn test_list_simple_directory() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create some files
        fs::write(base.join("file1.txt"), "content").unwrap();
        fs::write(base.join("file2.rs"), "content").unwrap();
        fs::create_dir(base.join("src")).unwrap();
        fs::write(base.join("src/main.rs"), "content").unwrap();

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("file1.txt"));
        assert!(result.output.contains("file2.rs"));
        assert!(result.output.contains("src/"));
        assert!(result.output.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_list_ignores_node_modules() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create files including node_modules
        fs::write(base.join("index.js"), "content").unwrap();
        fs::create_dir(base.join("node_modules")).unwrap();
        fs::write(base.join("node_modules/package.json"), "content").unwrap();

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("index.js"));
        assert!(!result.output.contains("node_modules"));
    }

    #[tokio::test]
    async fn test_list_nested_directories() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create nested directory structure
        fs::create_dir_all(base.join("level1/level2/level3")).unwrap();
        fs::write(base.join("root.txt"), "content").unwrap();
        fs::write(base.join("level1/file1.txt"), "content").unwrap();
        fs::write(base.join("level1/level2/file2.txt"), "content").unwrap();
        fs::write(base.join("level1/level2/level3/file3.txt"), "content").unwrap();

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // Verify all nested files are listed
        assert!(result.output.contains("root.txt"));
        assert!(result.output.contains("level1/"));
        assert!(result.output.contains("file1.txt"));
        assert!(result.output.contains("level2/"));
        assert!(result.output.contains("file2.txt"));
        assert!(result.output.contains("level3/"));
        assert!(result.output.contains("file3.txt"));

        // Verify metadata
        let metadata = result.metadata;
        assert_eq!(metadata["count"], 4);
        assert_eq!(metadata["truncated"], false);
    }

    #[tokio::test]
    async fn test_list_depth_limit() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create many files to exceed LIMIT
        for i in 0..150 {
            fs::write(base.join(format!("file{}.txt", i)), "content").unwrap();
        }

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // Verify truncation
        let metadata = result.metadata;
        assert_eq!(metadata["count"], LIMIT);
        assert_eq!(metadata["truncated"], true);
        assert!(result.title.contains("truncated"));
    }

    #[tokio::test]
    async fn test_list_hidden_files() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create hidden and normal files
        fs::write(base.join("visible.txt"), "content").unwrap();
        fs::write(base.join(".hidden"), "content").unwrap();
        fs::write(base.join(".env"), "content").unwrap();

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // The tool includes hidden files (hidden(false) in ignore walker)
        assert!(result.output.contains("visible.txt"));
        assert!(result.output.contains(".hidden"));
        assert!(result.output.contains(".env"));

        // Verify count includes hidden files
        let metadata = result.metadata;
        assert_eq!(metadata["count"], 3);
    }

    #[tokio::test]
    async fn test_list_file_info() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create files with different content
        fs::write(base.join("small.txt"), "x").unwrap();
        fs::write(base.join("large.txt"), "x".repeat(1000)).unwrap();

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // Verify files are listed
        assert!(result.output.contains("small.txt"));
        assert!(result.output.contains("large.txt"));

        // Verify metadata contains count
        let metadata = result.metadata;
        assert_eq!(metadata["count"], 2);
        assert!(metadata["path"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_list_empty_directory() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create an empty directory
        fs::create_dir(base.join("empty")).unwrap();

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // Verify no files listed
        let metadata = result.metadata;
        assert_eq!(metadata["count"], 0);
        assert_eq!(metadata["truncated"], false);

        // Output should show the root path
        assert!(result.output.contains(&base.display().to_string()));
    }

    #[tokio::test]
    async fn test_list_nonexistent_path() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Try to list a path that doesn't exist
        let nonexistent = base.join("does_not_exist");

        let tool = ListTool;
        let ctx = test_context(base.to_path_buf());
        let result = tool
            .execute(
                json!({
                    "path": nonexistent.to_string_lossy().to_string()
                }),
                &ctx,
            )
            .await;

        // The tool should return an error when the path doesn't exist
        // If it doesn't error, it should at least indicate the issue in metadata
        match result {
            Err(_) => {
                // Expected behavior - error on nonexistent path
            }
            Ok(output) => {
                // Alternative behavior - succeeds but returns empty list
                // Verify it's empty
                assert_eq!(output.metadata["count"], 0);
            }
        }
    }
}
