//! Todo tools - manage task lists for coding sessions.
//!
//! These tools help track progress on complex tasks:
//! - todowrite: Create/update the todo list with phases
//! - todoread: Read the current todo list
//!
//! Todos are organized into phases, where each phase contains multiple todo items.
//! This allows breaking down complex work into logical stages.
//!
//! Todos are stored in memory by default (InMemoryTodoStore) but can optionally
//! be persisted to `.wonopcode/todos.json` using FileTodoStore.

use crate::{Tool, ToolContext, ToolError, ToolEvent, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;
use tracing::debug;

/// A phase containing related todo items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Unique identifier for the phase.
    pub id: String,
    /// Human-readable name for the phase.
    pub name: String,
    /// Todo items within this phase.
    pub todos: Vec<TodoItem>,
}

impl Phase {
    /// Create a new phase.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            todos: Vec::new(),
        }
    }

    /// Add a todo item to this phase.
    pub fn add_todo(&mut self, todo: TodoItem) {
        self.todos.push(todo);
    }

    /// Get the overall status of the phase based on its todos.
    pub fn status(&self) -> PhaseStatus {
        if self.todos.is_empty() {
            return PhaseStatus::NotStarted;
        }

        let all_completed = self
            .todos
            .iter()
            .all(|t| t.status == TodoStatus::Completed || t.status == TodoStatus::Cancelled);
        let any_in_progress = self.todos.iter().any(|t| t.status == TodoStatus::InProgress);
        let any_started = self.todos.iter().any(|t| {
            t.status == TodoStatus::InProgress || t.status == TodoStatus::Completed
        });

        if all_completed {
            PhaseStatus::Finished
        } else if any_in_progress || any_started {
            PhaseStatus::InProgress
        } else {
            PhaseStatus::NotStarted
        }
    }

    /// Count todos by status.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let pending = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::Pending)
            .count();
        let in_progress = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let completed = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let cancelled = self
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::Cancelled)
            .count();
        (pending, in_progress, completed, cancelled)
    }
}

/// Phase status derived from its todo items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    /// No todos started yet.
    NotStarted,
    /// At least one todo is in progress or completed.
    InProgress,
    /// All todos are completed or cancelled.
    Finished,
}

impl PhaseStatus {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            PhaseStatus::NotStarted => "not_started",
            PhaseStatus::InProgress => "in_progress",
            PhaseStatus::Finished => "finished",
        }
    }

    /// Get display icon for the phase status.
    pub fn icon(&self) -> &'static str {
        match self {
            PhaseStatus::NotStarted => "○",
            PhaseStatus::InProgress => "◐",
            PhaseStatus::Finished => "●",
        }
    }
}

/// Container for phased todos.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhasedTodos {
    /// Ordered list of phases.
    pub phases: Vec<Phase>,
}

impl PhasedTodos {
    /// Create a new empty phased todos container.
    pub fn new() -> Self {
        Self { phases: Vec::new() }
    }

    /// Add a phase.
    pub fn add_phase(&mut self, phase: Phase) {
        self.phases.push(phase);
    }

    /// Get a phase by ID.
    pub fn get_phase(&self, id: &str) -> Option<&Phase> {
        self.phases.iter().find(|p| p.id == id)
    }

    /// Get a mutable phase by ID.
    pub fn get_phase_mut(&mut self, id: &str) -> Option<&mut Phase> {
        self.phases.iter_mut().find(|p| p.id == id)
    }

    /// Remove a phase by ID.
    pub fn remove_phase(&mut self, id: &str) -> Option<Phase> {
        if let Some(pos) = self.phases.iter().position(|p| p.id == id) {
            Some(self.phases.remove(pos))
        } else {
            None
        }
    }

    /// Get all todos as a flat list (for backward compatibility).
    pub fn all_todos(&self) -> Vec<&TodoItem> {
        self.phases.iter().flat_map(|p| p.todos.iter()).collect()
    }

    /// Get total counts across all phases.
    pub fn total_counts(&self) -> (usize, usize, usize, usize) {
        let mut pending = 0;
        let mut in_progress = 0;
        let mut completed = 0;
        let mut cancelled = 0;

        for phase in &self.phases {
            let (p, i, c, x) = phase.counts();
            pending += p;
            in_progress += i;
            completed += c;
            cancelled += x;
        }

        (pending, in_progress, completed, cancelled)
    }

    /// Check if empty (no phases or all phases have no todos).
    pub fn is_empty(&self) -> bool {
        self.phases.is_empty() || self.phases.iter().all(|p| p.todos.is_empty())
    }

    /// Get total number of todos.
    pub fn total_todos(&self) -> usize {
        self.phases.iter().map(|p| p.todos.len()).sum()
    }
}

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
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
            TodoStatus::Cancelled => "cancelled",
        }
    }

    /// Get display icon for the status.
    pub fn icon(&self) -> &'static str {
        match self {
            TodoStatus::Pending => "[ ]",
            TodoStatus::InProgress => "[>]",
            TodoStatus::Completed => "[x]",
            TodoStatus::Cancelled => "[-]",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => TodoStatus::Pending,
            "in_progress" => TodoStatus::InProgress,
            "completed" => TodoStatus::Completed,
            "cancelled" => TodoStatus::Cancelled,
            _ => TodoStatus::Pending,
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
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            TodoPriority::High => "high",
            TodoPriority::Medium => "medium",
            TodoPriority::Low => "low",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "high" => TodoPriority::High,
            "medium" => TodoPriority::Medium,
            "low" => TodoPriority::Low,
            _ => TodoPriority::Medium,
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
    /// Get phased todos for a project.
    fn get_phased(&self, project_root: &Path) -> PhasedTodos;

    /// Set phased todos for a project (replaces existing).
    fn set_phased(
        &self,
        project_root: &Path,
        todos: PhasedTodos,
    ) -> Result<(), TodoStoreError>;

    /// Clear all todos for a project.
    fn clear(&self, project_root: &Path);

    /// Get all todos as a flat list (backward compatibility).
    fn get(&self, project_root: &Path) -> Vec<TodoItem> {
        self.get_phased(project_root)
            .phases
            .into_iter()
            .flat_map(|p| p.todos)
            .collect()
    }

    /// Set todos from a flat list (backward compatibility).
    /// Creates a single "default" phase containing all todos.
    fn set(&self, project_root: &Path, todos: Vec<TodoItem>) -> Result<(), TodoStoreError> {
        let mut phased = PhasedTodos::new();
        if !todos.is_empty() {
            let mut phase = Phase::new("default", "Tasks");
            phase.todos = todos;
            phased.add_phase(phase);
        }
        self.set_phased(project_root, phased)
    }
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
    todos: RwLock<HashMap<PathBuf, PhasedTodos>>,
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
    fn get_phased(&self, project_root: &Path) -> PhasedTodos {
        self.todos
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(project_root)
            .cloned()
            .unwrap_or_default()
    }

    fn set_phased(
        &self,
        project_root: &Path,
        todos: PhasedTodos,
    ) -> Result<(), TodoStoreError> {
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

    /// Initialize the environment variable for shared todo storage.
    /// Call this early in the application startup before any component
    /// that needs todo storage is created.
    /// Returns the path that was set.
    pub fn init_env() -> std::path::PathBuf {
        if let Ok(path) = std::env::var(TODO_FILE_ENV_VAR) {
            std::path::PathBuf::from(path)
        } else {
            let path =
                std::env::temp_dir().join(format!("wonopcode-todos-{}.json", std::process::id()));
            std::env::set_var(TODO_FILE_ENV_VAR, &path);
            path
        }
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
    fn get_phased(&self, _project_root: &Path) -> PhasedTodos {
        if !self.path.exists() {
            return PhasedTodos::new();
        }

        match std::fs::read_to_string(&self.path) {
            Ok(content) => {
                // Try to parse as PhasedTodos first
                if let Ok(phased) = serde_json::from_str::<PhasedTodos>(&content) {
                    return phased;
                }
                // Fall back to legacy Vec<TodoItem> format
                if let Ok(todos) = serde_json::from_str::<Vec<TodoItem>>(&content) {
                    let mut phased = PhasedTodos::new();
                    if !todos.is_empty() {
                        let mut phase = Phase::new("default", "Tasks");
                        phase.todos = todos;
                        phased.add_phase(phase);
                    }
                    return phased;
                }
                PhasedTodos::new()
            }
            Err(_) => PhasedTodos::new(),
        }
    }

    fn set_phased(
        &self,
        _project_root: &Path,
        todos: PhasedTodos,
    ) -> Result<(), TodoStoreError> {
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
    fn get_phased(&self, project_root: &Path) -> PhasedTodos {
        let path = Self::todos_file_path(project_root);
        if !path.exists() {
            return PhasedTodos::new();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                // Try to parse as PhasedTodos first
                if let Ok(phased) = serde_json::from_str::<PhasedTodos>(&content) {
                    return phased;
                }
                // Fall back to legacy Vec<TodoItem> format
                if let Ok(todos) = serde_json::from_str::<Vec<TodoItem>>(&content) {
                    let mut phased = PhasedTodos::new();
                    if !todos.is_empty() {
                        let mut phase = Phase::new("default", "Tasks");
                        phase.todos = todos;
                        phased.add_phase(phase);
                    }
                    return phased;
                }
                PhasedTodos::new()
            }
            Err(_) => PhasedTodos::new(),
        }
    }

    fn set_phased(
        &self,
        project_root: &Path,
        todos: PhasedTodos,
    ) -> Result<(), TodoStoreError> {
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

/// Input for a single todo item.
#[derive(Debug, Deserialize)]
struct TodoItemInput {
    id: String,
    content: String,
    status: String,
    priority: String,
}

/// Input for a phase with its todos.
#[derive(Debug, Deserialize)]
struct PhaseInput {
    id: String,
    name: String,
    todos: Vec<TodoItemInput>,
}

/// Arguments for the todowrite tool.
#[derive(Debug, Deserialize)]
struct TodoWriteArgs {
    /// Phases containing todos (new format).
    #[serde(default)]
    phases: Vec<PhaseInput>,
    /// Legacy flat todo list (for backward compatibility).
    #[serde(default)]
    todos: Vec<TodoItemInput>,
}

impl TodoItemInput {
    fn to_todo_item(self) -> TodoItem {
        TodoItem {
            id: self.id,
            content: self.content,
            status: TodoStatus::from_str(&self.status),
            priority: TodoPriority::from_str(&self.priority),
        }
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        r#"Create and manage a structured task list organized by phases.

Use this tool to:
- Organize work into logical phases (e.g., "Requirements", "Implementation", "Testing")
- Track progress on complex multi-step tasks within each phase
- Show progress to the user with phase-level status

Phase Status (derived from todos):
- not_started: No todos in the phase have been started
- in_progress: At least one todo is in progress or completed (but not all done)
- finished: All todos are completed or cancelled

Todo States:
- pending: Task not yet started
- in_progress: Currently working on (limit to ONE at a time across all phases)
- completed: Task finished successfully
- cancelled: Task no longer needed"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "phases": {
                    "type": "array",
                    "description": "Phases containing grouped todos. Each phase represents a logical stage of work.",
                    "items": {
                        "type": "object",
                        "required": ["id", "name", "todos"],
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique identifier for the phase (e.g., 'phase_1', 'requirements')"
                            },
                            "name": {
                                "type": "string",
                                "description": "Human-readable name for the phase (e.g., 'Requirements Gathering')"
                            },
                            "todos": {
                                "type": "array",
                                "description": "Todo items within this phase",
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
                    }
                },
                "todos": {
                    "type": "array",
                    "description": "Legacy: flat todo list (use 'phases' instead for better organization)",
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

        // Build PhasedTodos from input
        let mut phased_todos = PhasedTodos::new();

        if !args.phases.is_empty() {
            // New phased format
            for phase_input in args.phases {
                let mut phase = Phase::new(phase_input.id, phase_input.name);
                for todo_input in phase_input.todos {
                    phase.add_todo(todo_input.to_todo_item());
                }
                phased_todos.add_phase(phase);
            }
        } else if !args.todos.is_empty() {
            // Legacy flat format - put all in a default phase
            let mut phase = Phase::new("default", "Tasks");
            for todo_input in args.todos {
                phase.add_todo(todo_input.to_todo_item());
            }
            phased_todos.add_phase(phase);
        }

        // Calculate counts
        let (pending, in_progress, completed, cancelled) = phased_todos.total_counts();
        let total = phased_todos.total_todos();

        // Save to store
        if let Err(e) = self.store.set_phased(&ctx.root_dir, phased_todos.clone()) {
            return Err(ToolError::execution_failed(format!(
                "Failed to save todos: {e}"
            )));
        }

        // Convert to flat list for event (for backward compatibility with TUI)
        let items: Vec<TodoItem> = phased_todos
            .phases
            .iter()
            .flat_map(|p| p.todos.clone())
            .collect();

        // Publish event immediately via event channel if available
        if let Some(ref event_tx) = ctx.event_tx {
            debug!(
                session_id = %ctx.session_id,
                phases = phased_todos.phases.len(),
                items = items.len(),
                "Publishing TodosUpdated event"
            );

            if let Err(e) = event_tx.send(ToolEvent::TodosUpdated(phased_todos.clone())) {
                debug!("Failed to send TodosUpdated event: {}", e);
            }
        }

        // Format output
        let output = format_phased_todos(&phased_todos);

        Ok(ToolOutput::new(
            format!(
                "Todo list updated: {} phases, {pending} pending, {in_progress} in progress, {completed} completed",
                phased_todos.phases.len()
            ),
            output,
        )
        .with_metadata(json!({
            "phases": phased_todos.phases.len(),
            "total": total,
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
        "Read your current todo list organized by phases."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let phased_todos = self.store.get_phased(&ctx.root_dir);

        if phased_todos.is_empty() {
            return Ok(ToolOutput::new(
                "No todos",
                "No todo items found for this project.",
            ));
        }

        let output = format_phased_todos(&phased_todos);
        let (pending, in_progress, completed, _cancelled) = phased_todos.total_counts();
        let total = phased_todos.total_todos();

        Ok(ToolOutput::new(
            format!(
                "{} phases, {} todos: {} pending, {} in progress, {} completed",
                phased_todos.phases.len(),
                total,
                pending,
                in_progress,
                completed
            ),
            output,
        )
        .with_metadata(json!({
            "phases": phased_todos.phases.len(),
            "total": total,
            "pending": pending,
            "in_progress": in_progress,
            "completed": completed
        })))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format phased todos for display.
fn format_phased_todos(phased: &PhasedTodos) -> String {
    let mut output = String::new();

    for phase in &phased.phases {
        let status = phase.status();
        let (pending, in_progress, completed, cancelled) = phase.counts();
        let total = phase.todos.len();

        output.push_str(&format!(
            "\n## {} {} ({}/{} done)\n",
            status.icon(),
            phase.name,
            completed + cancelled,
            total
        ));

        if phase.todos.is_empty() {
            output.push_str("  (no tasks)\n");
        } else {
            for item in &phase.todos {
                output.push_str(&format!(
                    "  {} [{}] {} ({})\n",
                    item.status.icon(),
                    item.priority.as_str(),
                    item.content,
                    item.id
                ));
            }
        }

        // Add phase summary if there are multiple items
        if total > 1 {
            output.push_str(&format!(
                "  Status: {} pending, {} in progress, {} completed\n",
                pending, in_progress, completed
            ));
        }
    }

    output.trim().to_string()
}

/// Format todo list for display (legacy flat format).
#[cfg(test)]
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

/// Get phased todos from a store for a project.
///
/// This is used by the runner to sync todos to the TUI.
pub fn get_phased_todos(store: &dyn TodoStore, root_dir: &Path) -> PhasedTodos {
    store.get_phased(root_dir)
}

/// Get todos from a store for a project (flat list).
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
            event_tx: None,
        }
    }

    #[test]
    fn test_todo_status_as_str() {
        assert_eq!(TodoStatus::Pending.as_str(), "pending");
        assert_eq!(TodoStatus::InProgress.as_str(), "in_progress");
        assert_eq!(TodoStatus::Completed.as_str(), "completed");
        assert_eq!(TodoStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn test_todo_status_icon() {
        assert_eq!(TodoStatus::Pending.icon(), "[ ]");
        assert_eq!(TodoStatus::InProgress.icon(), "[>]");
        assert_eq!(TodoStatus::Completed.icon(), "[x]");
        assert_eq!(TodoStatus::Cancelled.icon(), "[-]");
    }

    #[test]
    fn test_todo_priority_as_str() {
        assert_eq!(TodoPriority::High.as_str(), "high");
        assert_eq!(TodoPriority::Medium.as_str(), "medium");
        assert_eq!(TodoPriority::Low.as_str(), "low");
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
    fn test_in_memory_store_default() {
        let store = InMemoryTodoStore::default();
        assert!(store.get(&PathBuf::from("/test")).is_empty());
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

    #[test]
    fn test_file_store_default() {
        let store = FileTodoStore::default();
        assert!(store.get(&PathBuf::from("/nonexistent")).is_empty());
    }

    #[test]
    fn test_shared_file_todo_store() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("todos.json");
        let store = SharedFileTodoStore::new(file_path.clone());

        assert_eq!(store.path(), file_path);

        // Initially empty
        assert!(store.get(dir.path()).is_empty());

        // Set todos
        let todos = vec![TodoItem {
            id: "1".to_string(),
            content: "Shared task".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        }];
        store.set(dir.path(), todos).unwrap();

        // Get them back
        let retrieved = store.get(dir.path());
        assert_eq!(retrieved.len(), 1);

        // Clear
        store.clear(dir.path());
        assert!(!file_path.exists());
    }

    #[test]
    fn test_shared_file_todo_store_cleanup() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("todos.json");
        std::fs::write(&file_path, "[]").unwrap();

        let store = SharedFileTodoStore::new(file_path.clone());
        assert!(file_path.exists());

        store.cleanup();
        assert!(!file_path.exists());
    }

    #[test]
    fn test_todowrite_tool_id() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoWriteTool::new(store);
        assert_eq!(tool.id(), "todowrite");
    }

    #[test]
    fn test_todowrite_tool_description() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoWriteTool::new(store);
        let desc = tool.description();
        assert!(desc.contains("task list"));
        assert!(desc.contains("pending"));
        assert!(desc.contains("in_progress"));
        assert!(desc.contains("completed"));
        assert!(desc.contains("cancelled"));
    }

    #[test]
    fn test_todowrite_tool_parameters_schema() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoWriteTool::new(store);
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        // Both phases and todos are optional now
        assert!(schema["properties"]["todos"].is_object());
        assert!(schema["properties"]["phases"].is_object());
    }

    #[test]
    fn test_todowrite_tool_in_memory() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoWriteTool::in_memory(store);
        assert_eq!(tool.id(), "todowrite");
    }

    #[test]
    fn test_todoread_tool_id() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoReadTool::new(store);
        assert_eq!(tool.id(), "todoread");
    }

    #[test]
    fn test_todoread_tool_description() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoReadTool::new(store);
        assert!(tool.description().contains("todo list"));
    }

    #[test]
    fn test_todoread_tool_parameters_schema() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoReadTool::new(store);
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn test_todoread_tool_in_memory() {
        let store = Arc::new(InMemoryTodoStore::new());
        let tool = TodoReadTool::in_memory(store);
        assert_eq!(tool.id(), "todoread");
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
    async fn test_todowrite_all_statuses_and_priorities() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        let tool = TodoWriteTool::new(store.clone());
        let result = tool
            .execute(
                json!({
                    "todos": [
                        {"id": "1", "content": "Pending", "status": "pending", "priority": "high"},
                        {"id": "2", "content": "In Progress", "status": "in_progress", "priority": "medium"},
                        {"id": "3", "content": "Completed", "status": "completed", "priority": "low"},
                        {"id": "4", "content": "Cancelled", "status": "cancelled", "priority": "high"}
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result.metadata["total"], 4);
        assert_eq!(result.metadata["pending"], 1);
        assert_eq!(result.metadata["in_progress"], 1);
        assert_eq!(result.metadata["completed"], 1);
        assert_eq!(result.metadata["cancelled"], 1);
    }

    #[tokio::test]
    async fn test_todowrite_unknown_status_defaults() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        let tool = TodoWriteTool::new(store.clone());
        let result = tool
            .execute(
                json!({
                    "todos": [
                        {"id": "1", "content": "Unknown status", "status": "unknown", "priority": "unknown"}
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Unknown status defaults to pending, unknown priority defaults to medium
        assert_eq!(result.metadata["pending"], 1);
    }

    #[tokio::test]
    async fn test_todowrite_empty_args_creates_empty_phased_todos() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        // With phased todos, empty args or unrecognized keys create empty phased todos
        let tool = TodoWriteTool::new(store.clone());
        let result = tool.execute(json!({"not_todos": []}), &ctx).await;

        // Empty input is valid - it clears all todos
        assert!(result.is_ok());
        let output = result.unwrap();
        // Should have 0 phases (clears the todo list)
        assert_eq!(output.metadata["phases"], 0);
        assert_eq!(output.metadata["total"], 0);

        // Verify stored in memory is empty
        let phased = store.get_phased(&ctx.root_dir);
        assert!(phased.phases.is_empty());
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

        // Tasks should appear in output
        assert!(result.output.contains("Task A"));
        assert!(result.output.contains("Task B"));
        // In phased format, tasks show status icons and are under phase headers
        assert!(result.output.contains("[x]")); // Completed icon for Task A
        assert!(result.output.contains("[ ]")); // Pending icon for Task B
        // Metadata should reflect counts
        assert_eq!(result.metadata["total"], 2);
        assert_eq!(result.metadata["completed"], 1);
        assert_eq!(result.metadata["pending"], 1);
    }

    #[tokio::test]
    async fn test_todoread_empty() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/empty"));

        let tool = TodoReadTool::new(store);
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("No todo items"));
    }

    #[tokio::test]
    async fn test_todoread_metadata() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        // Write todos with various statuses
        let write_tool = TodoWriteTool::new(store.clone());
        write_tool
            .execute(
                json!({
                    "todos": [
                        {"id": "1", "content": "Pending 1", "status": "pending", "priority": "high"},
                        {"id": "2", "content": "Pending 2", "status": "pending", "priority": "medium"},
                        {"id": "3", "content": "In progress", "status": "in_progress", "priority": "high"},
                        {"id": "4", "content": "Done", "status": "completed", "priority": "low"}
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        let read_tool = TodoReadTool::new(store);
        let result = read_tool.execute(json!({}), &ctx).await.unwrap();

        assert_eq!(result.metadata["total"], 4);
        assert_eq!(result.metadata["pending"], 2);
        assert_eq!(result.metadata["in_progress"], 1);
        assert_eq!(result.metadata["completed"], 1);
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

    #[test]
    fn test_format_todo_list_empty() {
        let items: Vec<TodoItem> = vec![];
        let output = format_todo_list(&items);
        assert!(output.is_empty());
    }

    #[test]
    fn test_format_todo_list_all_statuses() {
        let items = vec![
            TodoItem {
                id: "1".to_string(),
                content: "Completed".to_string(),
                status: TodoStatus::Completed,
                priority: TodoPriority::High,
            },
            TodoItem {
                id: "2".to_string(),
                content: "Cancelled".to_string(),
                status: TodoStatus::Cancelled,
                priority: TodoPriority::Low,
            },
        ];

        let output = format_todo_list(&items);
        assert!(output.contains("COMPLETED"));
        assert!(output.contains("CANCELLED"));
        assert!(output.contains("[x]")); // Completed icon
        assert!(output.contains("[-]")); // Cancelled icon
    }

    #[test]
    fn test_get_todos_helper() {
        let store = InMemoryTodoStore::new();
        let root = PathBuf::from("/test");

        store
            .set(
                &root,
                vec![TodoItem {
                    id: "1".to_string(),
                    content: "Test".to_string(),
                    status: TodoStatus::Pending,
                    priority: TodoPriority::High,
                }],
            )
            .unwrap();

        let todos = get_todos(&store, &root);
        assert_eq!(todos.len(), 1);
    }

    #[test]
    fn test_clear_todos_helper() {
        let store = InMemoryTodoStore::new();
        let root = PathBuf::from("/test");

        store
            .set(
                &root,
                vec![TodoItem {
                    id: "1".to_string(),
                    content: "Test".to_string(),
                    status: TodoStatus::Pending,
                    priority: TodoPriority::High,
                }],
            )
            .unwrap();

        clear_todos(&store, &root);
        assert!(store.get(&root).is_empty());
    }

    #[test]
    fn test_todo_item_serialization() {
        let item = TodoItem {
            id: "test-id".to_string(),
            content: "Test content".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::High,
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("test-id"));
        assert!(json.contains("Test content"));
        assert!(json.contains("in_progress"));
        assert!(json.contains("high"));

        let parsed: TodoItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-id");
        assert_eq!(parsed.status, TodoStatus::InProgress);
        assert_eq!(parsed.priority, TodoPriority::High);
    }

    #[test]
    fn test_todo_store_error_display() {
        let io_error = TodoStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_error.to_string().contains("I/O error"));

        // Can't easily test serde error, but it's covered by the Display impl
    }

    // ========================================================================
    // Phased Todos Tests
    // ========================================================================

    #[test]
    fn test_phase_new() {
        let phase = Phase::new("test-phase", "Test Phase");
        assert_eq!(phase.id, "test-phase");
        assert_eq!(phase.name, "Test Phase");
        assert!(phase.todos.is_empty());
    }

    #[test]
    fn test_phase_add_todo() {
        let mut phase = Phase::new("test", "Test");
        let todo = TodoItem {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        };
        phase.add_todo(todo);
        assert_eq!(phase.todos.len(), 1);
    }

    #[test]
    fn test_phase_status() {
        let mut phase = Phase::new("test", "Test");

        // Empty phase is not started
        assert_eq!(phase.status(), PhaseStatus::NotStarted);

        // Add pending todo - still not started
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Pending".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::Medium,
        });
        assert_eq!(phase.status(), PhaseStatus::NotStarted);

        // Add in-progress todo - now in progress
        phase.add_todo(TodoItem {
            id: "2".to_string(),
            content: "In progress".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::High,
        });
        assert_eq!(phase.status(), PhaseStatus::InProgress);
    }

    #[test]
    fn test_phase_status_finished() {
        let mut phase = Phase::new("test", "Test");
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Done".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Medium,
        });
        phase.add_todo(TodoItem {
            id: "2".to_string(),
            content: "Cancelled".to_string(),
            status: TodoStatus::Cancelled,
            priority: TodoPriority::Low,
        });
        assert_eq!(phase.status(), PhaseStatus::Finished);
    }

    #[test]
    fn test_phase_counts() {
        let mut phase = Phase::new("test", "Test");
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Pending".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });
        phase.add_todo(TodoItem {
            id: "2".to_string(),
            content: "In progress".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::Medium,
        });
        phase.add_todo(TodoItem {
            id: "3".to_string(),
            content: "Completed".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Low,
        });
        phase.add_todo(TodoItem {
            id: "4".to_string(),
            content: "Cancelled".to_string(),
            status: TodoStatus::Cancelled,
            priority: TodoPriority::Low,
        });

        let (pending, in_progress, completed, cancelled) = phase.counts();
        assert_eq!(pending, 1);
        assert_eq!(in_progress, 1);
        assert_eq!(completed, 1);
        assert_eq!(cancelled, 1);
    }

    #[test]
    fn test_phase_status_icon() {
        assert_eq!(PhaseStatus::NotStarted.icon(), "○");
        assert_eq!(PhaseStatus::InProgress.icon(), "◐");
        assert_eq!(PhaseStatus::Finished.icon(), "●");
    }

    #[test]
    fn test_phased_todos_new() {
        let phased = PhasedTodos::new();
        assert!(phased.phases.is_empty());
        assert!(phased.is_empty());
    }

    #[test]
    fn test_phased_todos_add_phase() {
        let mut phased = PhasedTodos::new();
        let mut phase = Phase::new("planning", "Planning");
        // Add a todo to the phase so it's not empty
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });
        phased.add_phase(phase);
        assert_eq!(phased.phases.len(), 1);
        // is_empty returns false if there are phases with todos
        assert!(!phased.is_empty());
    }

    #[test]
    fn test_phased_todos_total_counts() {
        let mut phased = PhasedTodos::new();

        let mut phase1 = Phase::new("phase1", "Phase 1");
        phase1.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Pending".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });

        let mut phase2 = Phase::new("phase2", "Phase 2");
        phase2.add_todo(TodoItem {
            id: "2".to_string(),
            content: "Completed".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Medium,
        });

        phased.add_phase(phase1);
        phased.add_phase(phase2);

        let (pending, in_progress, completed, cancelled) = phased.total_counts();
        assert_eq!(pending, 1);
        assert_eq!(in_progress, 0);
        assert_eq!(completed, 1);
        assert_eq!(cancelled, 0);
    }

    #[test]
    fn test_phased_todos_total_todos() {
        let mut phased = PhasedTodos::new();

        let mut phase1 = Phase::new("phase1", "Phase 1");
        phase1.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Task 1".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });
        phase1.add_todo(TodoItem {
            id: "2".to_string(),
            content: "Task 2".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Medium,
        });

        let mut phase2 = Phase::new("phase2", "Phase 2");
        phase2.add_todo(TodoItem {
            id: "3".to_string(),
            content: "Task 3".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::Low,
        });

        phased.add_phase(phase1);
        phased.add_phase(phase2);

        assert_eq!(phased.total_todos(), 3);
    }

    #[tokio::test]
    async fn test_todowrite_with_phases() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        let tool = TodoWriteTool::new(store.clone());
        let result = tool
            .execute(
                json!({
                    "phases": [
                        {
                            "id": "planning",
                            "name": "Planning Phase",
                            "todos": [
                                {"id": "1", "content": "Define requirements", "status": "completed", "priority": "high"},
                                {"id": "2", "content": "Create design", "status": "in_progress", "priority": "medium"}
                            ]
                        },
                        {
                            "id": "implementation",
                            "name": "Implementation Phase",
                            "todos": [
                                {"id": "3", "content": "Write code", "status": "pending", "priority": "high"}
                            ]
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Check output contains phase and task info
        assert!(result.output.contains("Planning Phase"));
        assert!(result.output.contains("Implementation Phase"));
        assert!(result.output.contains("Define requirements"));
        assert!(result.output.contains("Write code"));

        // Check metadata
        assert_eq!(result.metadata["phases"], 2);
        assert_eq!(result.metadata["total"], 3);
        assert_eq!(result.metadata["completed"], 1);
        assert_eq!(result.metadata["in_progress"], 1);
        assert_eq!(result.metadata["pending"], 1);

        // Verify stored correctly
        let phased = store.get_phased(&ctx.root_dir);
        assert_eq!(phased.phases.len(), 2);
        assert_eq!(phased.phases[0].id, "planning");
        assert_eq!(phased.phases[0].todos.len(), 2);
        assert_eq!(phased.phases[1].id, "implementation");
        assert_eq!(phased.phases[1].todos.len(), 1);
    }

    #[tokio::test]
    async fn test_todoread_with_phases() {
        let store = Arc::new(InMemoryTodoStore::new());
        let ctx = test_context(PathBuf::from("/test/project"));

        // Write phased todos
        let write_tool = TodoWriteTool::new(store.clone());
        write_tool
            .execute(
                json!({
                    "phases": [
                        {
                            "id": "analysis",
                            "name": "Analysis",
                            "todos": [
                                {"id": "1", "content": "Analyze requirements", "status": "completed", "priority": "high"}
                            ]
                        },
                        {
                            "id": "coding",
                            "name": "Coding",
                            "todos": [
                                {"id": "2", "content": "Implement feature", "status": "in_progress", "priority": "high"}
                            ]
                        }
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Read them back
        let read_tool = TodoReadTool::new(store);
        let result = read_tool.execute(json!({}), &ctx).await.unwrap();

        // Output should show phase structure
        assert!(result.output.contains("Analysis"));
        assert!(result.output.contains("Coding"));
        assert!(result.output.contains("Analyze requirements"));
        assert!(result.output.contains("Implement feature"));

        // Metadata
        assert_eq!(result.metadata["phases"], 2);
        assert_eq!(result.metadata["total"], 2);
    }

    #[test]
    fn test_in_memory_store_phased() {
        let store = InMemoryTodoStore::new();
        let root = PathBuf::from("/test/project");

        // Initially empty
        let phased = store.get_phased(&root);
        assert!(phased.is_empty());

        // Set phased todos
        let mut phased_todos = PhasedTodos::new();
        let mut phase = Phase::new("test", "Test Phase");
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });
        phased_todos.add_phase(phase);

        store.set_phased(&root, phased_todos).unwrap();

        // Get them back
        let retrieved = store.get_phased(&root);
        assert_eq!(retrieved.phases.len(), 1);
        assert_eq!(retrieved.phases[0].id, "test");
        assert_eq!(retrieved.phases[0].todos.len(), 1);
    }

    #[test]
    fn test_file_store_phased() {
        let dir = tempdir().unwrap();
        let store = FileTodoStore::new();

        // Set phased todos
        let mut phased_todos = PhasedTodos::new();
        let mut phase = Phase::new("test", "Test Phase");
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });
        phased_todos.add_phase(phase);

        store.set_phased(dir.path(), phased_todos).unwrap();

        // File should exist
        let file_path = dir.path().join(".wonopcode").join("todos.json");
        assert!(file_path.exists());

        // Get them back
        let retrieved = store.get_phased(dir.path());
        assert_eq!(retrieved.phases.len(), 1);
    }

    #[test]
    fn test_get_phased_todos_helper() {
        let store = InMemoryTodoStore::new();
        let root = PathBuf::from("/test");

        let mut phased = PhasedTodos::new();
        let mut phase = Phase::new("test", "Test");
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        });
        phased.add_phase(phase);

        store.set_phased(&root, phased).unwrap();

        let retrieved = get_phased_todos(&store, &root);
        assert_eq!(retrieved.phases.len(), 1);
    }

    #[test]
    fn test_format_phased_todos() {
        let mut phased = PhasedTodos::new();

        let mut phase = Phase::new("dev", "Development");
        phase.add_todo(TodoItem {
            id: "1".to_string(),
            content: "Write tests".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::High,
        });
        phased.add_phase(phase);

        let output = format_phased_todos(&phased);

        // Check format includes phase status icon and name
        assert!(output.contains("◐")); // In-progress phase icon
        assert!(output.contains("Development"));
        assert!(output.contains("[>]")); // In-progress task icon
        assert!(output.contains("Write tests"));
    }
}
