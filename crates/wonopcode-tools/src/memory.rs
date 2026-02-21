//! Memory tools for the Super Memory system.
//!
//! These tools provide a unified interface for storing, recalling, and searching
//! memory across three scopes:
//! - Global: User-wide persistent memory (preferences, patterns, lessons learned)
//! - Workstream: Project/branch-specific memory (architecture, decisions, context)
//! - Session: Current conversation memory (recent context, temporary notes)
//!
//! The tools automatically route to the appropriate scope based on content and context.
//!
//! # Pattern
//!
//! These tools follow the same pattern as ticket tools:
//! - Simple stateless structs registered in `ToolRegistry::with_builtins()`
//! - Access the service through `ctx.memory_service` at execution time
//! - Return an error if the memory service is not configured

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};
use wonop_memory::{MemoryClearParams, MemoryRecallParams, MemorySearchParams, MemoryService, MemoryStoreParams};

/// Shared memory service that can be used across tools.
pub type SharedMemoryService = Arc<MemoryService>;

/// Tool for storing memories.
///
/// This is a stateless struct that accesses the memory service through
/// `ctx.memory_service` at execution time, following the same pattern as ticket tools.
pub struct MemoryStoreTool;

#[derive(Debug, Deserialize)]
struct MemoryStoreArgs {
    /// The key/topic for this memory (e.g., "coding_preferences", "project_architecture").
    key: String,
    /// The content to remember.
    content: String,
    /// Optional explicit scope: "global", "workstream", or "session".
    /// If not provided, the scope is auto-detected based on content.
    scope: Option<String>,
    /// Optional tags for categorization.
    tags: Option<Vec<String>>,
    /// Optional metadata as key-value pairs.
    metadata: Option<HashMap<String, Value>>,
}

#[async_trait]
impl Tool for MemoryStoreTool {
    fn id(&self) -> &str {
        "memory_store"
    }

    fn description(&self) -> &str {
        r#"Store information in persistent memory for later retrieval.

Use this tool to remember:
- User preferences and coding patterns (stored globally)
- Project architecture decisions and context (stored per workstream)
- Session-specific notes and temporary context (stored in session)

The scope is auto-detected based on content, or you can specify it explicitly.
Examples of auto-routing:
- "User prefers tabs over spaces" → global
- "This project uses a microservices architecture" → workstream
- "Remember to fix the bug in auth.rs" → session"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["key", "content"],
            "properties": {
                "key": {
                    "type": "string",
                    "description": "The key/topic for this memory (e.g., 'coding_preferences', 'project_architecture')"
                },
                "content": {
                    "type": "string",
                    "description": "The content to remember"
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "workstream", "session"],
                    "description": "Optional explicit scope. If not provided, auto-detected based on content."
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tags for categorization"
                },
                "metadata": {
                    "type": "object",
                    "description": "Optional metadata as key-value pairs"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let memory_service = ctx.memory_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Memory service not configured. Memory tools require a memory service to be initialized.",
            )
        })?;

        let args: MemoryStoreArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        debug!(key = %args.key, scope = ?args.scope, "Storing memory");

        let params = MemoryStoreParams {
            key: args.key.clone(),
            content: args.content.clone(),
            scope: args.scope.clone(),
            tags: args.tags,
            index: Some(true), // Always index for semantic search
            metadata: args.metadata,
        };

        let entry = memory_service
            .store(params)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to store memory: {e}")))?;

        info!(id = %entry.id, key = %entry.key, scope = %entry.scope, "Memory stored");

        Ok(ToolOutput::new(
            format!("Stored memory: {}", args.key),
            format!(
                "Memory stored successfully.\n\nID: {}\nKey: {}\nScope: {}\nContent: {}",
                entry.id,
                entry.key,
                entry.scope,
                if args.content.len() > 100 {
                    format!("{}...", &args.content[..100])
                } else {
                    args.content
                }
            ),
        )
        .with_metadata(json!({
            "id": entry.id,
            "key": entry.key,
            "scope": entry.scope,
            "created_at": entry.created_at.to_rfc3339()
        })))
    }

    fn requires_permission(&self) -> bool {
        false // Memory operations don't need explicit permission
    }
}

/// Tool for recalling memories by key or semantic query.
///
/// This is a stateless struct that accesses the memory service through
/// `ctx.memory_service` at execution time, following the same pattern as ticket tools.
pub struct MemoryRecallTool;

#[derive(Debug, Deserialize)]
struct MemoryRecallArgs {
    /// Key to lookup (exact match).
    key: Option<String>,
    /// Semantic query for similarity search.
    query: Option<String>,
    /// Scope to search: "global", "workstream", "session", or "all" (default).
    scope: Option<String>,
    /// Maximum number of results (default: 5).
    limit: Option<usize>,
}

#[async_trait]
impl Tool for MemoryRecallTool {
    fn id(&self) -> &str {
        "memory_recall"
    }

    fn description(&self) -> &str {
        r#"Recall stored memories by key or semantic query.

Use this tool to retrieve previously stored information:
- Exact key lookup: Retrieve a specific memory by its key
- Semantic search: Find relevant memories using natural language

Examples:
- Recall by key: key="coding_preferences"
- Semantic recall: query="What are the user's preferences for code formatting?"

You can limit results to specific scopes or search across all scopes."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key to lookup (exact match)"
                },
                "query": {
                    "type": "string",
                    "description": "Semantic query for similarity search"
                },
                "scope": {
                    "type": "string",
                    "enum": ["global", "workstream", "session", "all"],
                    "description": "Scope to search. Default: 'all'"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 5,
                    "description": "Maximum number of results (default: 5)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let memory_service = ctx.memory_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Memory service not configured. Memory tools require a memory service to be initialized.",
            )
        })?;

        let args: MemoryRecallArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        if args.key.is_none() && args.query.is_none() {
            return Err(ToolError::validation(
                "Either 'key' or 'query' must be provided",
            ));
        }

        debug!(key = ?args.key, query = ?args.query, scope = ?args.scope, "Recalling memory");

        let params = MemoryRecallParams {
            key: args.key.clone(),
            query: args.query.clone(),
            scope: args.scope,
            limit: args.limit,
        };

        let entries = memory_service
            .recall(params)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to recall memory: {e}")))?;

        if entries.is_empty() {
            return Ok(ToolOutput::new(
                "No memories found",
                format!(
                    "No memories found matching {}",
                    args.key
                        .map(|k| format!("key '{}'", k))
                        .or(args.query.map(|q| format!("query '{}'", q)))
                        .unwrap_or_else(|| "the criteria".to_string())
                ),
            ));
        }

        let mut output = format!("Found {} memories:\n\n", entries.len());
        for (i, entry) in entries.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}** ({})\n   {}\n\n",
                i + 1,
                entry.key,
                entry.scope,
                if entry.content.len() > 200 {
                    format!("{}...", &entry.content[..200])
                } else {
                    entry.content.clone()
                }
            ));
        }

        Ok(ToolOutput::new(
            format!("Recalled {} memories", entries.len()),
            output,
        )
        .with_metadata(json!({
            "count": entries.len(),
            "entries": entries.iter().map(|e| json!({
                "id": e.id,
                "key": e.key,
                "scope": e.scope,
                "created_at": e.created_at.to_rfc3339()
            })).collect::<Vec<_>>()
        })))
    }

    fn requires_permission(&self) -> bool {
        false
    }
}

/// Tool for semantic search across all memories.
///
/// This is a stateless struct that accesses the memory service through
/// `ctx.memory_service` at execution time, following the same pattern as ticket tools.
pub struct MemorySearchTool;

#[derive(Debug, Deserialize)]
struct MemorySearchArgs {
    /// Natural language query for semantic search.
    query: String,
    /// Scopes to search. Default: all scopes.
    scopes: Option<Vec<String>>,
    /// Maximum number of results (default: 10).
    limit: Option<usize>,
    /// Minimum relevance threshold (0.0 to 1.0, default: 0.3).
    min_similarity: Option<f32>,
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn id(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        r#"Search across all memories using semantic similarity.

This is the most powerful memory retrieval tool. Use it to:
- Find relevant context from past conversations
- Discover related decisions and discussions
- Search for specific topics or concepts

The search uses embeddings to find semantically similar content,
not just keyword matches. This means "user likes dark mode" will
match "prefer dark theme for IDE"."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural language query for semantic search"
                },
                "scopes": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["global", "workstream", "session"]
                    },
                    "description": "Scopes to search. Default: all scopes"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 10,
                    "description": "Maximum number of results (default: 10)"
                },
                "min_similarity": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "default": 0.3,
                    "description": "Minimum relevance threshold (0.0 to 1.0, default: 0.3)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let memory_service = ctx.memory_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Memory service not configured. Memory tools require a memory service to be initialized.",
            )
        })?;

        let args: MemorySearchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        debug!(query = %args.query, scopes = ?args.scopes, limit = ?args.limit, "Searching memories");

        let params = MemorySearchParams {
            query: args.query.clone(),
            scopes: args.scopes,
            threshold: args.min_similarity.map(|s| s as f64),
            limit: args.limit,
        };

        let search_result = memory_service
            .search(params)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to search memories: {e}")))?;

        let entries = &search_result.entries;
        if entries.is_empty() {
            return Ok(ToolOutput::new(
                "No memories found",
                format!("No memories found matching query: '{}'", args.query),
            ));
        }

        let mut output = format!(
            "Found {} relevant memories for '{}':\n\n",
            entries.len(),
            args.query
        );
        for (i, entry) in entries.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}** ({}) - relevance: {:.2}\n   {}\n\n",
                i + 1,
                entry.key,
                entry.scope,
                entry.relevance_score.unwrap_or(0.0),
                if entry.content.len() > 200 {
                    format!("{}...", &entry.content[..200])
                } else {
                    entry.content.clone()
                }
            ));
        }

        Ok(ToolOutput::new(
            format!("Found {} relevant memories", entries.len()),
            output,
        )
        .with_metadata(json!({
            "count": entries.len(),
            "query": args.query,
            "results": entries.iter().map(|e| json!({
                "id": e.id,
                "key": e.key,
                "scope": e.scope,
                "relevance": e.relevance_score,
                "created_at": e.created_at.to_rfc3339()
            })).collect::<Vec<_>>()
        })))
    }

    fn requires_permission(&self) -> bool {
        false
    }
}

/// Tool for clearing memories.
///
/// This is a stateless struct that accesses the memory service through
/// `ctx.memory_service` at execution time, following the same pattern as ticket tools.
pub struct MemoryClearTool;

#[derive(Debug, Deserialize)]
struct MemoryClearArgs {
    /// Clear memories from specific scope: "global", "workstream", "session", or "all".
    scope: Option<String>,
    /// Clear memories matching key pattern (supports * wildcard).
    pattern: Option<String>,
    /// Clear memories older than this duration (e.g., "7d", "24h", "30m").
    older_than: Option<String>,
}

#[async_trait]
impl Tool for MemoryClearTool {
    fn id(&self) -> &str {
        "memory_clear"
    }

    fn description(&self) -> &str {
        r#"Clear memories by scope, pattern, or age.

Use this tool to:
- Clear all session memories at the end of a conversation
- Remove outdated information
- Clean up specific categories of memories

This is a destructive operation and requires permission."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["global", "workstream", "session", "all"],
                    "default": "session",
                    "description": "Clear memories from specific scope (default: session)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Clear memories matching key pattern (supports * wildcard)"
                },
                "older_than": {
                    "type": "string",
                    "description": "Clear memories older than this duration (e.g., '7d', '24h', '30m')"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let memory_service = ctx.memory_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Memory service not configured. Memory tools require a memory service to be initialized.",
            )
        })?;

        let args: MemoryClearArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        debug!(scope = ?args.scope, pattern = ?args.pattern, older_than = ?args.older_than, "Clearing memories");

        // Scope defaults to "session" if not specified (safest default)
        let params = MemoryClearParams {
            scope: args.scope.clone().unwrap_or_else(|| "session".to_string()),
            pattern: args.pattern.clone(),
            older_than: args.older_than.clone(),
        };

        let count = memory_service
            .clear(params)
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to clear memories: {e}")))?;

        info!(count = count, scope = ?args.scope, pattern = ?args.pattern, "Memories cleared");

        Ok(ToolOutput::new(
            format!("Cleared {} memories", count),
            format!(
                "Successfully cleared {} memories{}{}{}",
                count,
                args.scope
                    .as_ref()
                    .map(|s| format!(" from scope '{}'", s))
                    .unwrap_or_default(),
                args.pattern
                    .as_ref()
                    .map(|p| format!(" matching pattern '{}'", p))
                    .unwrap_or_default(),
                args.older_than
                    .as_ref()
                    .map(|o| format!(" older than {}", o))
                    .unwrap_or_else(String::new)
            ),
        )
        .with_metadata(json!({
            "cleared_count": count,
            "scope": args.scope,
            "pattern": args.pattern,
            "older_than": args.older_than
        })))
    }

    fn requires_permission(&self) -> bool {
        true // Destructive operation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolContext;
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    fn create_test_context() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp/test"),
            cwd: PathBuf::from("/tmp/test"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: None,
            memory_service: None,
        }
    }

    fn create_test_context_with_memory() -> ToolContext {
        let service = Arc::new(MemoryService::new().unwrap());
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp/test"),
            cwd: PathBuf::from("/tmp/test"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: None,
            memory_service: Some(service),
        }
    }

    #[test]
    fn test_memory_store_tool_metadata() {
        let tool = MemoryStoreTool;

        assert_eq!(tool.id(), "memory_store");
        assert!(tool.description().contains("Store information"));

        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], json!(["key", "content"]));
    }

    #[test]
    fn test_memory_recall_tool_metadata() {
        let tool = MemoryRecallTool;

        assert_eq!(tool.id(), "memory_recall");
        assert!(tool.description().contains("Recall stored memories"));
    }

    #[test]
    fn test_memory_search_tool_metadata() {
        let tool = MemorySearchTool;

        assert_eq!(tool.id(), "memory_search");
        assert!(tool.description().contains("Search across all memories"));

        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], json!(["query"]));
    }

    #[test]
    fn test_memory_clear_tool_metadata() {
        let tool = MemoryClearTool;

        assert_eq!(tool.id(), "memory_clear");
        assert!(tool.description().contains("Clear memories"));
        assert!(tool.requires_permission()); // Destructive operation
    }

    #[tokio::test]
    async fn test_memory_store_no_service() {
        let tool = MemoryStoreTool;
        let ctx = create_test_context();

        let result = tool
            .execute(
                json!({
                    "key": "test_key",
                    "content": "test content"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Memory service not configured"));
    }

    #[tokio::test]
    async fn test_memory_recall_no_service() {
        let tool = MemoryRecallTool;
        let ctx = create_test_context();

        let result = tool
            .execute(
                json!({
                    "key": "test_key"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Memory service not configured"));
    }

    #[tokio::test]
    async fn test_memory_recall_requires_key_or_query() {
        let tool = MemoryRecallTool;
        let ctx = create_test_context_with_memory();

        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Either 'key' or 'query'"));
    }

    #[tokio::test]
    async fn test_memory_store_execute() {
        let tool = MemoryStoreTool;
        let ctx = create_test_context_with_memory();

        let result = tool
            .execute(
                json!({
                    "key": "test_key",
                    "content": "test content for memory"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.title.contains("Stored memory"));
    }

    #[tokio::test]
    async fn test_memory_store_and_recall() {
        let service = Arc::new(MemoryService::new().unwrap());
        let ctx = ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp/test"),
            cwd: PathBuf::from("/tmp/test"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: None,
            memory_service: Some(service),
        };

        // Store a memory
        let store_tool = MemoryStoreTool;
        let store_result = store_tool
            .execute(
                json!({
                    "key": "user_preference",
                    "content": "User prefers dark mode for all applications"
                }),
                &ctx,
            )
            .await;
        assert!(store_result.is_ok());

        // Recall by key
        let recall_tool = MemoryRecallTool;
        let recall_result = recall_tool
            .execute(
                json!({
                    "key": "user_preference"
                }),
                &ctx,
            )
            .await;
        assert!(recall_result.is_ok());
        let output = recall_result.unwrap();
        assert!(output.output.contains("dark mode"));
    }
}
