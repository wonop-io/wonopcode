//! Patch tool for applying unified diff patches to files.
//!
//! Supports a custom patch format with Add, Delete, Update, and Move operations.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, warn};

/// Patch tool for applying multi-file patches.
pub struct PatchTool;

#[derive(Debug, Deserialize)]
struct PatchArgs {
    /// The full patch text describing all changes to be made.
    patch_text: String,
}

/// A parsed patch hunk.
#[derive(Debug, Clone)]
enum Hunk {
    /// Add a new file.
    Add { path: PathBuf, contents: String },
    /// Delete a file.
    Delete { path: PathBuf },
    /// Update an existing file.
    Update {
        path: PathBuf,
        move_to: Option<PathBuf>,
        chunks: Vec<UpdateChunk>,
    },
}

/// A chunk of changes within an update hunk.
#[derive(Debug, Clone)]
struct UpdateChunk {
    /// Context line to find the location.
    context: Option<String>,
    /// Lines to remove (without - prefix).
    old_lines: Vec<String>,
    /// Lines to add (without + prefix).
    new_lines: Vec<String>,
    /// Is this the end of file marker?
    is_end_of_file: bool,
}

#[async_trait]
impl Tool for PatchTool {
    fn id(&self) -> &str {
        "patch"
    }

    fn description(&self) -> &str {
        r#"Apply a patch to one or more files.

The patch format supports:
- Adding new files
- Deleting files
- Updating existing files with contextual changes
- Moving/renaming files

Patch Format:
```
*** Begin Patch
*** Add File: path/to/new/file.ts
+line 1
+line 2

*** Delete File: path/to/delete.ts

*** Update File: path/to/existing.ts
*** Move to: path/to/new/location.ts
@@ context line to find location
 unchanged line (for context)
-line to remove
+line to add

*** End Patch
```

Guidelines:
- Use @@ to specify context for finding the change location
- Lines starting with + are added
- Lines starting with - are removed
- Lines starting with space are unchanged context
- Multiple chunks can be in one Update File section"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["patch_text"],
            "properties": {
                "patch_text": {
                    "type": "string",
                    "description": "The full patch text that describes all changes to be made"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: PatchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {}", e)))?;

        // Parse the patch
        let hunks = parse_patch(&args.patch_text)?;

        if hunks.is_empty() {
            return Err(ToolError::validation("No valid hunks found in patch"));
        }

        let mut results = Vec::new();
        let mut files_modified = 0;
        let mut files_added = 0;
        let mut files_deleted = 0;
        let mut total_additions = 0;
        let mut total_deletions = 0;

        for hunk in hunks {
            match hunk {
                Hunk::Add { path, contents } => {
                    let full_path = resolve_path(&ctx.cwd, &path);

                    // Create parent directories if needed
                    if let Some(parent) = full_path.parent() {
                        fs::create_dir_all(parent).await.map_err(|e| {
                            ToolError::execution_failed(format!(
                                "Failed to create directory {}: {}",
                                parent.display(),
                                e
                            ))
                        })?;
                    }

                    // Take snapshot if available
                    if let Some(snapshot) = &ctx.snapshot {
                        let _ = snapshot
                            .take(
                                &[full_path.clone()],
                                &ctx.session_id,
                                &ctx.message_id,
                                "patch: add file",
                            )
                            .await;
                    }

                    fs::write(&full_path, &contents).await.map_err(|e| {
                        ToolError::execution_failed(format!(
                            "Failed to write file {}: {}",
                            full_path.display(),
                            e
                        ))
                    })?;

                    let lines = contents.lines().count();
                    total_additions += lines;
                    files_added += 1;
                    results.push(format!("Added: {} (+{} lines)", path.display(), lines));
                }

                Hunk::Delete { path } => {
                    let full_path = resolve_path(&ctx.cwd, &path);

                    if !full_path.exists() {
                        warn!("File to delete does not exist: {}", full_path.display());
                        results.push(format!("Skipped delete (not found): {}", path.display()));
                        continue;
                    }

                    // Count lines before deletion
                    let old_content = fs::read_to_string(&full_path).await.unwrap_or_default();
                    let lines = old_content.lines().count();
                    total_deletions += lines;

                    // Take snapshot if available
                    if let Some(snapshot) = &ctx.snapshot {
                        let _ = snapshot
                            .take(
                                &[full_path.clone()],
                                &ctx.session_id,
                                &ctx.message_id,
                                "patch: delete file",
                            )
                            .await;
                    }

                    fs::remove_file(&full_path).await.map_err(|e| {
                        ToolError::execution_failed(format!(
                            "Failed to delete file {}: {}",
                            full_path.display(),
                            e
                        ))
                    })?;

                    files_deleted += 1;
                    results.push(format!("Deleted: {} (-{} lines)", path.display(), lines));
                }

                Hunk::Update {
                    path,
                    move_to,
                    chunks,
                } => {
                    let full_path = resolve_path(&ctx.cwd, &path);

                    if !full_path.exists() {
                        return Err(ToolError::file_not_found(full_path.display().to_string()));
                    }

                    // Take snapshot if available
                    if let Some(snapshot) = &ctx.snapshot {
                        let _ = snapshot
                            .take(
                                &[full_path.clone()],
                                &ctx.session_id,
                                &ctx.message_id,
                                "patch: update file",
                            )
                            .await;
                    }

                    let old_content = fs::read_to_string(&full_path).await.map_err(|e| {
                        ToolError::execution_failed(format!(
                            "Failed to read file {}: {}",
                            full_path.display(),
                            e
                        ))
                    })?;

                    let new_content = apply_chunks(&old_content, &chunks)?;

                    // Count changes
                    let (additions, deletions) = count_changes(&old_content, &new_content);
                    total_additions += additions;
                    total_deletions += deletions;

                    // Handle move operation
                    let target_path = if let Some(new_path) = move_to {
                        let target = resolve_path(&ctx.cwd, &new_path);

                        // Create parent directories
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent).await.map_err(|e| {
                                ToolError::execution_failed(format!(
                                    "Failed to create directory {}: {}",
                                    parent.display(),
                                    e
                                ))
                            })?;
                        }

                        // Write to new location
                        fs::write(&target, &new_content).await.map_err(|e| {
                            ToolError::execution_failed(format!(
                                "Failed to write file {}: {}",
                                target.display(),
                                e
                            ))
                        })?;

                        // Delete old file
                        fs::remove_file(&full_path).await.map_err(|e| {
                            ToolError::execution_failed(format!(
                                "Failed to delete old file {}: {}",
                                full_path.display(),
                                e
                            ))
                        })?;

                        results.push(format!(
                            "Moved: {} -> {} (+{} -{} lines)",
                            path.display(),
                            new_path.display(),
                            additions,
                            deletions
                        ));
                        target
                    } else {
                        fs::write(&full_path, &new_content).await.map_err(|e| {
                            ToolError::execution_failed(format!(
                                "Failed to write file {}: {}",
                                full_path.display(),
                                e
                            ))
                        })?;

                        results.push(format!(
                            "Updated: {} (+{} -{} lines)",
                            path.display(),
                            additions,
                            deletions
                        ));
                        full_path
                    };

                    files_modified += 1;
                    debug!(path = %target_path.display(), "File updated");
                }
            }
        }

        let summary = format!(
            "{} file(s) modified, {} added, {} deleted (+{} -{})",
            files_modified, files_added, files_deleted, total_additions, total_deletions
        );

        let output = format!("{}\n\n{}", summary, results.join("\n"));

        Ok(
            ToolOutput::new("Patch applied", output).with_metadata(json!({
                "files_modified": files_modified,
                "files_added": files_added,
                "files_deleted": files_deleted,
                "additions": total_additions,
                "deletions": total_deletions
            })),
        )
    }
}

/// Parse a patch text into hunks.
fn parse_patch(text: &str) -> ToolResult<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut lines = text.lines().peekable();
    let mut current_hunk: Option<HunkBuilder> = None;

    while let Some(line) = lines.next() {
        let line = line.trim_end();

        // Skip empty lines and patch markers
        if line.is_empty() || line == "*** Begin Patch" || line == "*** End Patch" || line == "```"
        {
            continue;
        }

        // Add file
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            // Finalize previous hunk
            if let Some(builder) = current_hunk.take() {
                hunks.push(builder.build()?);
            }

            let mut contents = String::new();
            while let Some(&next_line) = lines.peek() {
                if next_line.starts_with("*** ") {
                    break;
                }
                let next_line = lines.next().unwrap();
                if let Some(content) = next_line.strip_prefix('+') {
                    contents.push_str(content);
                    contents.push('\n');
                }
            }

            hunks.push(Hunk::Add {
                path: PathBuf::from(path.trim()),
                contents,
            });
            continue;
        }

        // Delete file
        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            if let Some(builder) = current_hunk.take() {
                hunks.push(builder.build()?);
            }

            hunks.push(Hunk::Delete {
                path: PathBuf::from(path.trim()),
            });
            continue;
        }

        // Update file
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            if let Some(builder) = current_hunk.take() {
                hunks.push(builder.build()?);
            }

            current_hunk = Some(HunkBuilder::new_update(PathBuf::from(path.trim())));
            continue;
        }

        // Move to (must come after Update File)
        if let Some(path) = line.strip_prefix("*** Move to: ") {
            if let Some(ref mut builder) = current_hunk {
                builder.set_move_to(PathBuf::from(path.trim()));
            }
            continue;
        }

        // End of file marker
        if line == "*** End of File" {
            if let Some(ref mut builder) = current_hunk {
                builder.mark_end_of_file();
            }
            continue;
        }

        // Context line (@@)
        if let Some(context) = line.strip_prefix("@@ ") {
            if let Some(ref mut builder) = current_hunk {
                builder.start_chunk(Some(context.to_string()));
            }
            continue;
        }

        // Also accept just "@@" to start a chunk without context
        if line == "@@" {
            if let Some(ref mut builder) = current_hunk {
                builder.start_chunk(None);
            }
            continue;
        }

        // Change lines
        if let Some(ref mut builder) = current_hunk {
            if let Some(removed) = line.strip_prefix('-') {
                builder.add_old_line(removed.to_string());
            } else if let Some(added) = line.strip_prefix('+') {
                builder.add_new_line(added.to_string());
            } else if line.starts_with(' ') || line.is_empty() {
                // Context line (unchanged)
                let content = line.strip_prefix(' ').unwrap_or(line);
                builder.add_context_line(content.to_string());
            }
        }
    }

    // Finalize last hunk
    if let Some(builder) = current_hunk {
        hunks.push(builder.build()?);
    }

    Ok(hunks)
}

/// Builder for constructing update hunks.
struct HunkBuilder {
    path: PathBuf,
    move_to: Option<PathBuf>,
    chunks: Vec<UpdateChunk>,
    current_chunk: Option<ChunkBuilder>,
}

impl HunkBuilder {
    fn new_update(path: PathBuf) -> Self {
        Self {
            path,
            move_to: None,
            chunks: Vec::new(),
            current_chunk: None,
        }
    }

    fn set_move_to(&mut self, path: PathBuf) {
        self.move_to = Some(path);
    }

    fn start_chunk(&mut self, context: Option<String>) {
        // Finalize previous chunk
        if let Some(chunk) = self.current_chunk.take() {
            self.chunks.push(chunk.build());
        }
        self.current_chunk = Some(ChunkBuilder::new(context));
    }

    fn add_old_line(&mut self, line: String) {
        if self.current_chunk.is_none() {
            self.current_chunk = Some(ChunkBuilder::new(None));
        }
        if let Some(ref mut chunk) = self.current_chunk {
            chunk.add_old_line(line);
        }
    }

    fn add_new_line(&mut self, line: String) {
        if self.current_chunk.is_none() {
            self.current_chunk = Some(ChunkBuilder::new(None));
        }
        if let Some(ref mut chunk) = self.current_chunk {
            chunk.add_new_line(line);
        }
    }

    fn add_context_line(&mut self, line: String) {
        if let Some(ref mut chunk) = self.current_chunk {
            chunk.add_context_line(line);
        }
    }

    fn mark_end_of_file(&mut self) {
        if let Some(ref mut chunk) = self.current_chunk {
            chunk.is_end_of_file = true;
        }
    }

    fn build(mut self) -> ToolResult<Hunk> {
        if let Some(chunk) = self.current_chunk.take() {
            self.chunks.push(chunk.build());
        }

        Ok(Hunk::Update {
            path: self.path,
            move_to: self.move_to,
            chunks: self.chunks,
        })
    }
}

/// Builder for constructing update chunks.
struct ChunkBuilder {
    context: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    is_end_of_file: bool,
}

impl ChunkBuilder {
    fn new(context: Option<String>) -> Self {
        Self {
            context,
            old_lines: Vec::new(),
            new_lines: Vec::new(),
            is_end_of_file: false,
        }
    }

    fn add_old_line(&mut self, line: String) {
        self.old_lines.push(line);
    }

    fn add_new_line(&mut self, line: String) {
        self.new_lines.push(line);
    }

    fn add_context_line(&mut self, line: String) {
        // Context lines are both old and new (unchanged)
        self.old_lines.push(line.clone());
        self.new_lines.push(line);
    }

    fn build(self) -> UpdateChunk {
        UpdateChunk {
            context: self.context,
            old_lines: self.old_lines,
            new_lines: self.new_lines,
            is_end_of_file: self.is_end_of_file,
        }
    }
}

/// Apply chunks to file content.
fn apply_chunks(content: &str, chunks: &[UpdateChunk]) -> ToolResult<String> {
    let mut result = content.to_string();

    for chunk in chunks {
        result = apply_chunk(&result, chunk)?;
    }

    Ok(result)
}

/// Apply a single chunk to content.
fn apply_chunk(content: &str, chunk: &UpdateChunk) -> ToolResult<String> {
    let lines: Vec<&str> = content.lines().collect();

    // Find the location to apply the chunk
    let start_idx = if let Some(context) = &chunk.context {
        // Find the context line
        lines.iter().position(|line| line.contains(context))
    } else if chunk.is_end_of_file {
        // Apply at end of file
        Some(lines.len())
    } else {
        // Try to find matching old_lines
        find_matching_lines(&lines, &chunk.old_lines)
    };

    let start_idx = start_idx.ok_or_else(|| {
        ToolError::execution_failed(format!(
            "Could not find location to apply chunk. Context: {:?}, Old lines: {:?}",
            chunk.context,
            chunk.old_lines.first()
        ))
    })?;

    // Build new content
    let mut new_lines: Vec<String> = Vec::new();

    // Add lines before the chunk
    for line in &lines[..start_idx] {
        new_lines.push(line.to_string());
    }

    // Add new lines from the chunk
    for line in &chunk.new_lines {
        new_lines.push(line.clone());
    }

    // Calculate how many old lines to skip
    let skip_count = chunk.old_lines.len().saturating_sub(
        chunk
            .old_lines
            .iter()
            .zip(&chunk.new_lines)
            .take_while(|(old, new)| old == new)
            .count(),
    );

    // Add remaining lines after the chunk
    let after_idx = start_idx + skip_count.min(lines.len() - start_idx);
    for line in &lines[after_idx..] {
        new_lines.push(line.to_string());
    }

    Ok(new_lines.join("\n") + if content.ends_with('\n') { "\n" } else { "" })
}

/// Find matching lines in content.
fn find_matching_lines(lines: &[&str], old_lines: &[String]) -> Option<usize> {
    if old_lines.is_empty() {
        return None;
    }

    let first_old = &old_lines[0];
    for (idx, line) in lines.iter().enumerate() {
        if line.contains(first_old.as_str()) || *line == first_old.as_str() {
            // Check if subsequent lines match
            let mut matches = true;
            for (offset, old_line) in old_lines.iter().enumerate().skip(1) {
                if idx + offset >= lines.len() || lines[idx + offset] != old_line.as_str() {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Some(idx);
            }
        }
    }

    None
}

/// Resolve a path relative to the working directory.
fn resolve_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Count additions and deletions between two strings.
fn count_changes(old_content: &str, new_content: &str) -> (usize, usize) {
    let diff = TextDiff::from_lines(old_content, new_content);
    let mut additions = 0;
    let mut deletions = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => additions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => {}
        }
    }

    (additions, deletions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_add_file() {
        let patch = r#"*** Begin Patch
*** Add File: test/new.txt
+line 1
+line 2
*** End Patch"#;

        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 1);

        match &hunks[0] {
            Hunk::Add { path, contents } => {
                assert_eq!(path.to_str().unwrap(), "test/new.txt");
                assert_eq!(contents, "line 1\nline 2\n");
            }
            _ => panic!("Expected Add hunk"),
        }
    }

    #[test]
    fn test_parse_delete_file() {
        let patch = r#"*** Begin Patch
*** Delete File: old/file.txt
*** End Patch"#;

        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 1);

        match &hunks[0] {
            Hunk::Delete { path } => {
                assert_eq!(path.to_str().unwrap(), "old/file.txt");
            }
            _ => panic!("Expected Delete hunk"),
        }
    }

    #[test]
    fn test_parse_update_file() {
        let patch = r#"*** Begin Patch
*** Update File: src/main.rs
@@ fn main
-    println!("Hello");
+    println!("Hello, World!");
*** End Patch"#;

        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 1);

        match &hunks[0] {
            Hunk::Update { path, chunks, .. } => {
                assert_eq!(path.to_str().unwrap(), "src/main.rs");
                assert_eq!(chunks.len(), 1);
                assert_eq!(chunks[0].context, Some("fn main".to_string()));
            }
            _ => panic!("Expected Update hunk"),
        }
    }

    #[test]
    fn test_apply_chunk() {
        let content = "line 1\nline 2\nline 3\n";
        let chunk = UpdateChunk {
            context: Some("line 2".to_string()),
            old_lines: vec!["line 2".to_string()],
            new_lines: vec!["modified line 2".to_string()],
            is_end_of_file: false,
        };

        let result = apply_chunk(content, &chunk).unwrap();
        assert!(result.contains("modified line 2"));
    }
}
