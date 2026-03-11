//! HMS MCP tool implementations.
//!
//! These tools provide the MCP interface for the Hierarchical Memory System.

use std::path::PathBuf;
use std::str::FromStr;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};

use super::{HmsError, MemoryEntryData, MemoryValue, StorageClass, Visibility};

/// Convert JSON value to MemoryValue.
fn json_to_memory_value(value: Value) -> MemoryValue {
    match value {
        Value::String(s) => MemoryValue::String(s),
        Value::Array(arr) => {
            let strings: Vec<String> = arr
                .into_iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            MemoryValue::List(strings)
        }
        Value::Object(map) => {
            let yaml_map: serde_yaml::Mapping = map
                .into_iter()
                .map(|(k, v)| {
                    (
                        serde_yaml::Value::String(k),
                        serde_json::from_value::<serde_yaml::Value>(v).unwrap_or(serde_yaml::Value::Null),
                    )
                })
                .collect();
            MemoryValue::Map(yaml_map)
        }
        _ => MemoryValue::String(value.to_string()),
    }
}

/// Convert MemoryValue to JSON.
fn memory_value_to_json(value: &MemoryValue) -> Value {
    match value {
        MemoryValue::String(s) => Value::String(s.clone()),
        MemoryValue::List(items) => Value::Array(items.iter().map(|s| Value::String(s.clone())).collect()),
        MemoryValue::Map(map) => serde_json::to_value(map).unwrap_or(Value::Null),
    }
}

/// Truncate a string for display.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        { let mut end = max_len.saturating_sub(3); while end > 0 && !s.is_char_boundary(end) { end -= 1; } format!("{}...", &s[..end]) }
    }
}

/// Shared HMS service type (re-exported for convenience).
type SharedHmsService = super::SharedHmsService;

/// Get HMS service from context or return error.
fn get_hms_service(ctx: &ToolContext) -> ToolResult<SharedHmsService> {
    ctx.hms_service
        .as_ref()
        .cloned()
        .ok_or_else(|| ToolError::execution_failed("HMS service not configured"))
}

// ============================================================================
// hms_set Tool
// ============================================================================

/// Tool for setting memory entries.
pub struct HmsSetTool;

#[derive(Debug, Deserialize)]
struct HmsSetArgs {
    key: String,
    value: Value,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    storage: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[async_trait]
impl Tool for HmsSetTool {
    fn id(&self) -> &str {
        "hms_set"
    }

    fn description(&self) -> &str {
        r#"Store a memory entry in the hierarchical memory system.

Memories are stored in memory.yaml files and propagate based on visibility:
- public: Visible everywhere in the repository
- upstream: Visible to parent directories only
- downstream: Visible to child directories only
- private: Visible only in the defining directory

Storage locations:
- tracked: .wonopcode/memory.yaml (committed to git)
- local: .wonopcode-local/memory.yaml (gitignored)
- user: ~/.config/wonopcode/memory.yaml (global)"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["key", "value"],
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Unique key for the memory entry"
                },
                "value": {
                    "description": "Memory value (string, array, or object)"
                },
                "visibility": {
                    "type": "string",
                    "enum": ["public", "upstream", "downstream", "private"],
                    "default": "public",
                    "description": "Visibility scope for propagation"
                },
                "storage": {
                    "type": "string",
                    "enum": ["tracked", "local", "user"],
                    "default": "tracked",
                    "description": "Storage location"
                },
                "directory": {
                    "type": "string",
                    "description": "Target directory (defaults to current working directory)"
                },
                "description": {
                    "type": "string",
                    "description": "Optional description"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tags"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let hms_arc = get_hms_service(ctx)?;

        let args: HmsSetArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let target_dir = args
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        let storage = args
            .storage
            .as_deref()
            .map(StorageClass::from_str)
            .transpose()
            .map_err(|e: HmsError| ToolError::validation(e.to_string()))?
            .unwrap_or_default();

        let visibility = args
            .visibility
            .as_deref()
            .map(Visibility::from_str)
            .transpose()
            .map_err(|e: HmsError| ToolError::validation(e.to_string()))?
            .unwrap_or_default();

        let mut entry = MemoryEntryData::new(json_to_memory_value(args.value))
            .with_visibility(visibility);

        if let Some(desc) = args.description {
            entry = entry.with_description(desc);
        }
        if let Some(tags) = args.tags {
            entry = entry.with_tags(tags);
        }

        debug!(key = %args.key, ?storage, ?visibility, "Setting HMS memory");

        // Acquire read lock for set_memory (it only needs &self)
        let hms = hms_arc.read().await;

        hms.set_memory(&target_dir, &args.key, entry, storage)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        Ok(ToolOutput::new(
            format!("Set memory: {}", args.key),
            format!(
                "Memory '{}' stored in {} with {} visibility",
                args.key, storage, visibility
            ),
        )
        .with_metadata(json!({
            "key": args.key,
            "storage": storage.to_string(),
            "visibility": visibility.to_string(),
            "directory": target_dir.display().to_string()
        })))
    }

    fn requires_permission(&self) -> bool {
        true // Modifies files
    }
}

// ============================================================================
// hms_get Tool
// ============================================================================

/// Tool for retrieving memory entries.
pub struct HmsGetTool;

#[derive(Debug, Deserialize)]
struct HmsGetArgs {
    key: String,
    #[serde(default)]
    directory: Option<String>,
}

#[async_trait]
impl Tool for HmsGetTool {
    fn id(&self) -> &str {
        "hms_get"
    }

    fn description(&self) -> &str {
        r#"Retrieve a specific memory entry by key.

Returns the resolved memory value considering visibility rules and directory hierarchy."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key of the memory to retrieve"
                },
                "directory": {
                    "type": "string",
                    "description": "Directory context (defaults to cwd)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let hms_arc = get_hms_service(ctx)?;

        let args: HmsGetArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let target_dir = args
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Acquire read lock for resolver access
        let hms = hms_arc.read().await;

        let memories = hms
            .resolver()
            .resolve(&target_dir)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        let memory = memories
            .iter()
            .find(|m| m.key == args.key)
            .ok_or_else(|| ToolError::execution_failed(format!("Memory '{}' not found", args.key)))?;

        Ok(ToolOutput::new(
            format!("Memory: {}", args.key),
            format!(
                "**Key:** {}\n**Value:** {}\n**Visibility:** {}\n**Source:** {}",
                memory.key,
                memory.value.to_string_lossy(),
                memory.visibility,
                memory.source_path.display()
            ),
        )
        .with_metadata(json!({
            "key": memory.key,
            "value": memory_value_to_json(&memory.value),
            "visibility": memory.visibility.to_string(),
            "source": memory.source_path.display().to_string(),
            "storage_class": memory.storage_class.to_string()
        })))
    }

    fn requires_permission(&self) -> bool {
        false // Read-only
    }
}

// ============================================================================
// hms_delete Tool
// ============================================================================

/// Tool for deleting memory entries.
pub struct HmsDeleteTool;

#[derive(Debug, Deserialize)]
struct HmsDeleteArgs {
    key: String,
    #[serde(default)]
    storage: Option<String>,
    #[serde(default)]
    directory: Option<String>,
}

#[async_trait]
impl Tool for HmsDeleteTool {
    fn id(&self) -> &str {
        "hms_delete"
    }

    fn description(&self) -> &str {
        "Delete a memory entry from memory.yaml. Specify storage class to target specific file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key of the memory to delete"
                },
                "storage": {
                    "type": "string",
                    "enum": ["tracked", "local", "user"],
                    "description": "Storage class to delete from (searches all if not specified)"
                },
                "directory": {
                    "type": "string",
                    "description": "Directory context (defaults to cwd)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let hms_arc = get_hms_service(ctx)?;

        let args: HmsDeleteArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let target_dir = args
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Acquire read lock for delete_memory (it only needs &self)
        let hms = hms_arc.read().await;

        let deleted = hms
            .delete_memory(&target_dir, &args.key, args.storage.as_deref())
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        if deleted {
            Ok(ToolOutput::new(
                format!("Deleted memory: {}", args.key),
                format!("Memory '{}' has been deleted", args.key),
            )
            .with_metadata(json!({
                "key": args.key,
                "deleted": true
            })))
        } else {
            Err(ToolError::execution_failed(format!(
                "Memory '{}' not found",
                args.key
            )))
        }
    }

    fn requires_permission(&self) -> bool {
        true // Modifies files
    }
}

// ============================================================================
// hms_list Tool
// ============================================================================

/// Tool for listing memory entries.
pub struct HmsListTool;

#[derive(Debug, Deserialize)]
struct HmsListArgs {
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    filter_visibility: Option<String>,
    #[serde(default)]
    filter_tags: Option<Vec<String>>,
}

#[async_trait]
impl Tool for HmsListTool {
    fn id(&self) -> &str {
        "hms_list"
    }

    fn description(&self) -> &str {
        "List all memory entries visible at a directory, considering visibility propagation."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "directory": {
                    "type": "string",
                    "description": "Directory context (defaults to cwd)"
                },
                "filter_visibility": {
                    "type": "string",
                    "enum": ["public", "upstream", "downstream", "private"],
                    "description": "Filter by visibility"
                },
                "filter_tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter by tags (all must match)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let hms_arc = get_hms_service(ctx)?;

        let args: HmsListArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let target_dir = args
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Acquire read lock for resolver access
        let hms = hms_arc.read().await;

        let mut memories = hms
            .resolver()
            .resolve(&target_dir)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Apply filters
        if let Some(vis_str) = args.filter_visibility {
            let target_vis = Visibility::from_str(&vis_str)
                .map_err(|e: HmsError| ToolError::validation(e.to_string()))?;
            memories.retain(|m| m.visibility == target_vis);
        }

        if let Some(tags) = args.filter_tags {
            memories.retain(|m| tags.iter().all(|t| m.tags.contains(t)));
        }

        // Format output
        let mut output = format!(
            "Found {} memories at {}:\n\n",
            memories.len(),
            target_dir.display()
        );

        for mem in &memories {
            output.push_str(&format!(
                "- **{}** ({})\n  Source: {}\n  Value: {}\n\n",
                mem.key,
                mem.visibility,
                mem.source_path.display(),
                truncate(&mem.value.to_string_lossy(), 100)
            ));
        }

        Ok(ToolOutput::new(
            format!("Listed {} memories", memories.len()),
            output,
        )
        .with_metadata(json!({
            "count": memories.len(),
            "directory": target_dir.display().to_string(),
            "entries": memories.iter().map(|m| json!({
                "key": m.key,
                "visibility": m.visibility.to_string(),
                "source": m.source_path.display().to_string()
            })).collect::<Vec<_>>()
        })))
    }

    fn requires_permission(&self) -> bool {
        false // Read-only
    }
}

// ============================================================================
// hms_render Tool
// ============================================================================

/// Tool for rendering AGENTS.md from templates.
pub struct HmsRenderTool;

#[derive(Debug, Deserialize)]
struct HmsRenderArgs {
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    write: Option<bool>,
}

#[async_trait]
impl Tool for HmsRenderTool {
    fn id(&self) -> &str {
        "hms_render"
    }

    fn description(&self) -> &str {
        r#"Render AGENTS.md from AGENTS.TEMPLATE.md and resolved memories.

Set write=true to save to .wonopcode/AGENTS.md, otherwise returns preview."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "directory": {
                    "type": "string",
                    "description": "Target directory (defaults to cwd)"
                },
                "write": {
                    "type": "boolean",
                    "default": false,
                    "description": "Write output to .wonopcode/AGENTS.md"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let hms_arc = get_hms_service(ctx)?;

        let args: HmsRenderArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let target_dir = args
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        // Acquire write lock for generate/write operations (they need &mut self)
        let mut hms = hms_arc.write().await;

        if args.write.unwrap_or(false) {
            let path = hms
                .write_agents_md(&target_dir)
                .await
                .map_err(|e| ToolError::execution_failed(e.to_string()))?;

            Ok(ToolOutput::new(
                "AGENTS.md generated",
                format!("Written to: {}", path.display()),
            )
            .with_metadata(json!({
                "path": path.display().to_string(),
                "written": true
            })))
        } else {
            let content = hms
                .generate(&target_dir)
                .await
                .map_err(|e| ToolError::execution_failed(e.to_string()))?;

            Ok(ToolOutput::new(
                "AGENTS.md preview",
                format!("```markdown\n{}\n```", content),
            )
            .with_metadata(json!({
                "preview": true,
                "content_length": content.len()
            })))
        }
    }

    fn requires_permission(&self) -> bool {
        true // Can write files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hms::HmsService;
    use crate::ToolContext;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    fn create_test_context(root: &std::path::Path, hms: Option<SharedHmsService>) -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: root.to_path_buf(),
            cwd: root.to_path_buf(),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: None,
            memory_service: None,
            hms_service: hms,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        }
    }

    /// Create a shared HMS service for testing.
    fn create_shared_hms(root: &std::path::Path) -> SharedHmsService {
        Arc::new(tokio::sync::RwLock::new(HmsService::new(root.to_path_buf())))
    }

    #[test]
    fn test_hms_set_tool_metadata() {
        let tool = HmsSetTool;
        assert_eq!(tool.id(), "hms_set");
        assert!(tool.requires_permission());
    }

    #[test]
    fn test_hms_get_tool_metadata() {
        let tool = HmsGetTool;
        assert_eq!(tool.id(), "hms_get");
        assert!(!tool.requires_permission());
    }

    #[test]
    fn test_hms_delete_tool_metadata() {
        let tool = HmsDeleteTool;
        assert_eq!(tool.id(), "hms_delete");
        assert!(tool.requires_permission());
    }

    #[test]
    fn test_hms_list_tool_metadata() {
        let tool = HmsListTool;
        assert_eq!(tool.id(), "hms_list");
        assert!(!tool.requires_permission());
    }

    #[test]
    fn test_hms_render_tool_metadata() {
        let tool = HmsRenderTool;
        assert_eq!(tool.id(), "hms_render");
        assert!(tool.requires_permission());
    }

    #[tokio::test]
    async fn test_hms_set_no_service() {
        let temp = TempDir::new().unwrap();
        let ctx = create_test_context(temp.path(), None);

        let tool = HmsSetTool;
        let result = tool
            .execute(
                json!({
                    "key": "test",
                    "value": "test"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn test_hms_set_and_get() {
        let temp = TempDir::new().unwrap();
        let hms = create_shared_hms(temp.path());
        let ctx = create_test_context(temp.path(), Some(hms));

        // Set a memory
        let set_tool = HmsSetTool;
        let result = set_tool
            .execute(
                json!({
                    "key": "test_key",
                    "value": "test_value"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        // Get it back
        let get_tool = HmsGetTool;
        let result = get_tool
            .execute(
                json!({
                    "key": "test_key"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("test_value"));
    }

    #[tokio::test]
    async fn test_hms_list() {
        let temp = TempDir::new().unwrap();
        let hms = create_shared_hms(temp.path());
        let ctx = create_test_context(temp.path(), Some(hms.clone()));

        // Set some memories
        {
            let guard = hms.read().await;
            guard.set_memory(
                temp.path(),
                "key1",
                MemoryEntryData::new("value1"),
                StorageClass::Tracked,
            )
            .await
            .unwrap();

            guard.set_memory(
                temp.path(),
                "key2",
                MemoryEntryData::new("value2"),
                StorageClass::Tracked,
            )
            .await
            .unwrap();
        }

        // List them
        let list_tool = HmsListTool;
        let result = list_tool.execute(json!({}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("key1"));
        assert!(output.output.contains("key2"));
    }

    #[tokio::test]
    async fn test_hms_delete() {
        let temp = TempDir::new().unwrap();
        let hms = create_shared_hms(temp.path());
        let ctx = create_test_context(temp.path(), Some(hms.clone()));

        // Set a memory
        {
            let guard = hms.read().await;
            guard.set_memory(
                temp.path(),
                "delete_me",
                MemoryEntryData::new("value"),
                StorageClass::Tracked,
            )
            .await
            .unwrap();
        }

        // Delete it
        let delete_tool = HmsDeleteTool;
        let result = delete_tool
            .execute(
                json!({
                    "key": "delete_me"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        // Verify it's gone
        let get_tool = HmsGetTool;
        let result = get_tool
            .execute(
                json!({
                    "key": "delete_me"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hms_render_no_template() {
        let temp = TempDir::new().unwrap();
        let hms = create_shared_hms(temp.path());
        let ctx = create_test_context(temp.path(), Some(hms));

        let render_tool = HmsRenderTool;
        let result = render_tool.execute(json!({}), &ctx).await;

        // Should fail because no template exists
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hms_render_with_template() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create template
        fs::create_dir_all(root.join(".wonopcode")).await.unwrap();
        fs::write(
            root.join(".wonopcode/AGENTS.TEMPLATE.md"),
            "# Hello {{ name | default(value='World') }}",
        )
        .await
        .unwrap();

        let hms = create_shared_hms(root);

        // Set a memory
        {
            let guard = hms.read().await;
            guard.set_memory(
                root,
                "name",
                MemoryEntryData::new("Test"),
                StorageClass::Tracked,
            )
            .await
            .unwrap();
        }

        let ctx = create_test_context(root, Some(hms));

        let render_tool = HmsRenderTool;
        let result = render_tool
            .execute(
                json!({
                    "write": false
                }),
                &ctx,
            )
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("Hello Test"));
    }
}
