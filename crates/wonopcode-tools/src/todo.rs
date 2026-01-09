//! Todo tools - manage task lists for coding sessions.
//!
//! These tools help track progress on complex tasks:
//! - todowrite: Create/update the todo list
//! - todoread: Read the current todo list
//!
//! Todos are stored in memory by default (InMemoryTodoStore) but can optionally
//! be persisted to `.wonopcode/todos.json` using FileTodoStore.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

/// A todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

/// Todo status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl TodoStatus {
    fn as_str(&self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
            TodoStatus::Cancelled => "cancelled",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            TodoStatus::Pending => "[ ]",
            TodoStatus::InProgress => "[>]",
            TodoStatus::Completed => "[x]",
            TodoStatus::Cancelled => "[-]",
        }
    }
}

/// Todo priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

impl TodoPriority {
    fn as_str(&self) -> &'static str {
        match self {
            TodoPriority::High => "high",
            TodoPriority::Medium => "medium",
            TodoPriority::Low => "low",
        }
    }
}

// ============================================================================
// TodoStore Trait and Implementations
// ============================================================================

/// Trait for todo storage backends.
///
/// This allows switching between different storage implementations:
/// - `InMemoryTodoStore`: Default, todos are lost when session ends
/// - `FileTodoStore`: Persists todos to `.wonopcode/todos.json`
pub trait TodoStore: Send + Sync {
    /// Get all todos for a project.
    fn get(&self, project_root: &Path) -> Vec<TodoItem>;

    /// Set all todos for a project (replaces existing).
    fn set(&self, project_root: &Path, todos: Vec<TodoItem>) -> Result<(), TodoStoreError>;

    /// Clear all todos for a project.
    fn clear(&self, project_root: &Path);
}

/// Error type for todo store operations.
#[derive(Debug, thiserror::Error)]
pub enum TodoStoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// In-memory todo storage (default).
///
/// Todos are stored in memory and lost when the process exits.
/// This is the preferred storage for normal sessions since todos
/// are typically only relevant during active work.
pub struct InMemoryTodoStore {
    todos: RwLock<HashMap<PathBuf, Vec<TodoItem>>>,
}

impl InMemoryTodoStore {
    /// Create a new in-memory todo store.
    pub fn new() -> Self {
        Self {
            todos: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryTodoStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoStore for InMemoryTodoStore {
    fn get(&self, project_root: &Path) -> Vec<TodoItem> {
        self.todos
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(project_root)
            .cloned()
            .unwrap_or_default()
    }

    fn set(&self, project_root: &Path, todos: Vec<TodoItem>) -> Result<(), TodoStoreError> {
        self.todos
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(project_root.to_path_buf(), todos);
        Ok(())
    }

    fn clear(&self, project_root: &Path) {
        self.todos
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project_root);
    }
}

/// Environment variable for shared todo file path.
/// When set, both the TUI and MCP server use this file for todo storage,
/// enabling cross-process communication.
pub const TODO_FILE_ENV_VAR: &str = "WONOPCODE_TODO_FILE";

/// Shared file-based todo storage.
///
/// Uses a file path from either:
/// 1. The `WONOPCODE_TODO_FILE` environment variable (for cross-process sharing)
/// 2. A path provided at construction time
///
/// This enables the TUI and MCP server (which run as separate processes) to share todo state.
pub struct SharedFileTodoStore {
    path: PathBuf,
}

impl SharedFileTodoStore {
    /// Create a new shared file todo store at the given path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create from environment variable, or generate a new temp path.
    /// If creating a new path, also sets the environment variable so child processes inherit it.
    pub fn from_env_or_create() -> Self {
        if let Ok(path) = std::env::var(TODO_FILE_ENV_VAR) {
            Self {
                path: PathBuf::from(path),
            }
        } else {
            let path =
                std::env::temp_dir().join(format!("wonopcode-todos-{}.json", std::process::id()));
            // Set env var so child processes (MCP server) inherit it
            std::env::set_var(TODO_FILE_ENV_VAR, &path);
            Self { path }
        }
    }

    /// Create from environment variable only. Returns None if not set.
    pub fn from_env() -> Option<Self> {
        std::env::var(TODO_FILE_ENV_VAR).ok().map(|path| Self {
            path: PathBuf::from(path),
        })
    }

    /// Get the path to the shared file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Delete the shared file (call on cleanup).
    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl TodoStore for SharedFileTodoStore {
    fn get(&self, _project_root: &Path) -> Vec<TodoItem> {
        if !self.path.exists() {
            return Vec::new();
        }

        match std::fs::read_to_string(&self.path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn set(&self, _project_root: &Path, todos: Vec<TodoItem>) -> Result<(), TodoStoreError> {
        let content = serde_json::to_string_pretty(&todos)?;
        std::fs::write(&self.path, content)?;
        Ok(())
    }

    fn clear(&self, _project_root: &Path) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// File-based todo storage.
///
/// Persists todos to `.wonopcode/todos.json` in the project root.
/// Use this when you want todos to persist across process restarts.
pub struct FileTodoStore;

impl FileTodoStore {
    /// Create a new file-based todo store.
    pub fn new() -> Self {
        Self
    }

    /// Get the path to the todos file for a given root directory.
    fn todos_file_path(root_dir: &Path) -> PathBuf {
        root_dir.join(".wonopcode").join("todos.json")
    }
}

impl Default for FileTodoStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoStore for FileTodoStore {
    fn get(&self, project_root: &Path) -> Vec<TodoItem> {
        let path = Self::todos_file_path(project_root);
        if !path.exists() {
            return Vec::new();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn set(&self, project_root: &Path, todos: Vec<TodoItem>) -> Result<(), TodoStoreError> {
        let path = Self::todos_file_path(project_root);

        // Ensure the directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&todos)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    fn clear(&self, project_root: &Path) {
        let path = Self::todos_file_path(project_root);
        let _ = std::fs::remove_file(path);
    }
}

// ============================================================================
// Tool Implementations
// ============================================================================

/// TodoWrite tool - create/update todo list.
pub struct TodoWriteTool {
    store: Arc<dyn TodoStore>,
}

impl TodoWriteTool {
    /// Create a new TodoWriteTool with the given store.
    pub fn new(store: Arc<dyn TodoStore>) -> Self {
        Self { store }
    }

    /// Create a new TodoWriteTool with in-memory storage.
    pub fn in_memory(store: Arc<InMemoryTodoStore>) -> Self {
        Self { store }
    }
}

#[derive(Debug, Deserialize)]
struct TodoWriteArgs {
    todos: Vec<TodoItemInput>,
}

#[derive(Debug, Deserialize)]
struct TodoItemInput {
    id: String,
    content: String,
    status: String,
    priority: String,
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        r#"Create and manage a structured task list for your current coding session.

Use this tool to:
- Track progress on complex multi-step tasks
- Break down large tasks into smaller steps
- Show progress to the user

Task States:
- pending: Task not yet started
- in_progress: Currently working on (limit to ONE at a time)
- completed: Task finished successfully
- cancelled: Task no longer needed"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["todos"],
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The updated todo list",
                    "items": {
                        "type": "object",
                        "required": ["id", "content", "status", "priority"],
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique identifier for the todo item"
                            },
                            "content": {
                                "type": "string",
                                "description": "Brief description of the task"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"],
                                "description": "Current status of the task"
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["high", "medium", "low"],
                                "description": "Priority level of the task"
                            }
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TodoWriteArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Convert input to TodoItems
        let items: Vec<TodoItem> = args
            .todos
            .into_iter()
            .map(|input| {
                let status = match input.status.as_str() {
                    "pending" => TodoStatus::Pending,
                    "in_progress" => TodoStatus::InProgress,
                    "completed" => TodoStatus::Completed,
                    "cancelled" => TodoStatus::Cancelled,
                    _ => TodoStatus::Pending,
                };
                let priority = match input.priority.as_str() {
                    "high" => TodoPriority::High,
                    "medium" => TodoPriority::Medium,
                    "low" => TodoPriority::Low,
                    _ => TodoPriority::Medium,
                };
                TodoItem {
                    id: input.id,
                    content: input.content,
                    status,
                    priority,
                }
            })
            .collect();

        // Count by status
        let pending = items
            .iter()
            .filter(|t| t.status == TodoStatus::Pending)
            .count();
        let in_progress = items
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let completed = items
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let cancelled = items
            .iter()
            .filter(|t| t.status == TodoStatus::Cancelled)
            .count();

        // Save to store
        if let Err(e) = self.store.set(&ctx.root_dir, items.clone()) {
            return Err(ToolError::execution_failed(format!(
                "Failed to save todos: {e}"
            )));
        }

        // Format output
        let output = format_todo_list(&items);

        Ok(ToolOutput::new(
            format!(
                "Todo list updated: {pending} pending, {in_progress} in progress, {completed} completed"
            ),
            output,
        )
        .with_metadata(json!({
            "total": items.len(),
            "pending": pending,
            "in_progress": in_progress,
            "completed": completed,
            "cancelled": cancelled
        })))
    }
}

/// TodoRead tool - read current todo list.
pub struct TodoReadTool {
    store: Arc<dyn TodoStore>,
}

impl TodoReadTool {
    /// Create a new TodoReadTool with the given store.
    pub fn new(store: Arc<dyn TodoStore>) -> Self {
        Self { store }
    }

    /// Create a new TodoReadTool with in-memory storage.
    pub fn in_memory(store: Arc<InMemoryTodoStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn id(&self) -> &str {
        "todoread"
    }

    fn description(&self) -> &str {
        "Read your current todo list."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let items = self.store.get(&ctx.root_dir);

        if items.is_empty() {
            return Ok(ToolOutput::new(
                "No todos",
                "No todo items found for this project.",
            ));
        }

        let output = format_todo_list(&items);

        let pending = items
            .iter()
            .filter(|t| t.status == TodoStatus::Pending)
            .count();
        let in_progress = items
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let completed = items
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();

        Ok(ToolOutput::new(
            format!(
                "{} todos: {} pending, {} in progress, {} completed",
                items.len(),
                pending,
                in_progress,
                completed
            ),
            output,
        )
        .with_metadata(json!({
            "total": items.len(),
            "pending": pending,
            "in_progress": in_progress,
            "completed": completed
        })))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format todo list for display.
fn format_todo_list(items: &[TodoItem]) -> String {
    let mut output = String::new();

    // Group by status
    let mut by_status: HashMap<&str, Vec<&TodoItem>> = HashMap::new();
    for item in items {
        by_status
            .entry(item.status.as_str())
            .or_default()
            .push(item);
    }

    // Display in order: in_progress, pending, completed, cancelled
    for status in &["in_progress", "pending", "completed", "cancelled"] {
        if let Some(items) = by_status.get(status) {
            if !items.is_empty() {
                output.push_str(&format!(
                    "\n## {}\n",
                    status.replace('_', " ").to_uppercase()
                ));
                for item in items {
                    output.push_str(&format!(
                        "{} [{}] {} ({})\n",
                        item.status.icon(),
                        item.priority.as_str(),
                        item.content,
                        item.id
                    ));
                }
            }
        }
    }

    output.trim().to_string()
}

// ============================================================================
// Public API for Runner Integration
// ============================================================================

/// Get todos from a store for a project.
///
/// This is used by the runner to sync todos to the TUI.
pub fn get_todos(store: &dyn TodoStore, root_dir: &Path) -> Vec<TodoItem> {
    store.get(root_dir)
}

/// Clear todos from a store for a project.
///
/// This is primarily useful for testing.
pub fn clear_todos(store: &dyn TodoStore, root_dir: &Path) {
    store.clear(root_dir)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn test_context(root_dir: PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test_session".to_string(),
            message_id: "test_message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: root_dir.clone(),
            cwd: root_dir,
            snapshot: None,
            file_time: None,
            sandbox: None,
        }
    }

    #[test]
    fn test_in_memory_store() {
        let store = InMemoryTodoStore::new();
        let root = PathBuf::from("/test/project");

        // Initially empty
        assert!(store.get(&root).is_empty());

        // Set some todos
        let todos = vec![
            TodoItem {
                id: "1".to_string(),
                content: "Task 1".to_string(),
                status: TodoStatus::Pending,
                priority: TodoPriority::High,
            },
            TodoItem {
                id: "2".to_string(),
                content: "Task 2".to_string(),
                status: TodoStatus::InProgress,
                priority: TodoPriority::Medium,
            },
        ];
        store.set(&root, todos).unwrap();

        // Get them back
        let retrieved = store.get(&root);
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].id, "1");
        assert_eq!(retrieved[1].id, "2");

        // Clear
        store.clear(&root);
        assert!(store.get(&root).is_empty());
    }

    #[test]
    fn test_file_store() {
        let dir = tempdir().unwrap();
        let store = FileTodoStore::new();

        // Initially empty
        assert!(store.get(dir.path()).is_empty());

        // Set some todos
        let todos = vec![TodoItem {
            id: "1".to_string(),
            content: "Task 1".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Low,
        }];
        store.set(dir.path(), todos).unwrap();

        // File should exist
        let file_path = dir.path().join(".wonopcode").join("todos.json");
        assert!(file_path.exists());

        // Get them back
        let retrieved = store.get(dir.path());
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].status, TodoStatus::Completed);

        // Clear
        store.clear(dir.path());
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_todowrite_with_in_memory_store() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        let tool = TodoWriteTool::new(store.clone());
        let result = tool
            .execute(
                json!({
                    "todos": [
                        {"id": "1", "content": "First task", "status": "pending", "priority": "high"},
                        {"id": "2", "content": "Second task", "status": "in_progress", "priority": "medium"}
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("First task"));
        assert!(result.output.contains("Second task"));
        assert_eq!(result.metadata["total"], 2);
        assert_eq!(result.metadata["pending"], 1);
        assert_eq!(result.metadata["in_progress"], 1);

        // Verify stored in memory
        let stored = store.get(&ctx.root_dir);
        assert_eq!(stored.len(), 2);
    }

    #[tokio::test]
    async fn test_todoread_with_in_memory_store() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        // Write some todos
        let write_tool = TodoWriteTool::new(store.clone());
        write_tool
            .execute(
                json!({
                    "todos": [
                        {"id": "1", "content": "Task A", "status": "completed", "priority": "high"},
                        {"id": "2", "content": "Task B", "status": "pending", "priority": "low"}
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Read them back
        let read_tool = TodoReadTool::new(store);
        let result = read_tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("Task A"));
        assert!(result.output.contains("Task B"));
        assert!(result.output.contains("COMPLETED"));
        assert!(result.output.contains("PENDING"));
    }

    #[tokio::test]
    async fn test_todoread_empty() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/empty"));

        let tool = TodoReadTool::new(store);
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("No todo items"));
    }

    #[test]
    fn test_format_todo_list() {
        let items = vec![
            TodoItem {
                id: "1".to_string(),
                content: "In progress task".to_string(),
                status: TodoStatus::InProgress,
                priority: TodoPriority::High,
            },
            TodoItem {
                id: "2".to_string(),
                content: "Pending task".to_string(),
                status: TodoStatus::Pending,
                priority: TodoPriority::Medium,
            },
        ];

        let output = format_todo_list(&items);
        assert!(output.contains("IN PROGRESS"));
        assert!(output.contains("PENDING"));
        assert!(output.contains("[>]")); // In progress icon
        assert!(output.contains("[ ]")); // Pending icon
    }
}
