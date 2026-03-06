//! Tool implementations for wonopcode.
//!
//! This crate provides the tools that AI agents can use to interact
//! with the codebase and environment.

pub mod error;
pub mod registry;

// Tool implementations
pub mod ace;
pub mod ace_todo_store;
pub mod bash;
pub mod batch;
pub mod edit;
pub mod execute_typescript;
pub mod glob;
pub mod grep;
pub mod list;
pub mod lsp;
pub mod mcp;
pub mod memory;
pub mod mcp_todo_adapter;
pub mod multiedit;
pub mod patch;
pub mod plan_mode;
pub mod read;
pub mod search;
pub mod skill;
pub mod task;
pub mod ticket;
pub mod todo;
pub mod webfetch;
pub mod write;

pub use error::{ToolError, ToolResult};
pub use registry::ToolRegistry;

// Re-export ACE tools for convenience
pub use ace::{
    AceCreateArtifactTool, AceReadArtifactTool, AceSessionLogTool, AceSubmitCheckpointTool,
    AceTodoReadTool, AceTodoUpdateTool, AceTodoWriteTool, AceWhatNowTool,
};
pub use ace_todo_store::AceTodoStore;

// Re-export ticket tools for convenience
pub use ticket::{
    TicketCreateTool, TicketListTool, TicketReadTool, TicketSearchTool, TicketService,
};

// Re-export memory tools for convenience
pub use memory::{
    MemoryClearTool, MemoryRecallTool, MemorySearchTool, MemoryStoreTool, SharedMemoryService,
};

// Re-export Code Mode tools for convenience
pub use execute_typescript::ExecuteTypescriptTool;

use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wonopcode_sandbox::SandboxRuntime;
use wonopcode_snapshot::SnapshotStore;
use wonopcode_util::FileTimeState;

/// Permission check request for TypeScript tools.
///
/// This is similar to PermissionCheck in wonopcode-core but defined here
/// to avoid circular dependencies.
#[derive(Debug, Clone)]
pub struct TsPermissionRequest {
    /// Unique identifier for this request.
    pub id: String,
    /// Tool name requesting permission (e.g., "edit", "bash").
    pub tool: String,
    /// Action being performed (e.g., "write", "execute").
    pub action: String,
    /// Path involved (for file operations).
    pub path: Option<String>,
    /// Human-readable description of what's being requested.
    pub description: String,
    /// Additional details (like command or file content preview).
    pub details: Option<serde_json::Value>,
}

/// Trait for checking permissions from TypeScript code.
///
/// This trait abstracts permission checking so tools don't need to depend
/// directly on wonopcode-core. Implementations wrap the PermissionManager.
#[async_trait]
pub trait TsPermissionChecker: Send + Sync {
    /// Check if an operation is allowed.
    ///
    /// Returns `true` if the operation is allowed, `false` if denied.
    /// Implementations may block waiting for user input.
    async fn check(&self, session_id: &str, request: TsPermissionRequest) -> bool;
}

/// Event that tools can emit to notify listeners of state changes.
#[derive(Debug, Clone)]
pub enum ToolEvent {
    /// Todo list was updated with new phased structure (legacy, for backward compatibility).
    TodosUpdated(todo::PhasedTodos),
    /// An artifact was created.
    ArtifactCreated { id: String, artifact_type: String },
    /// An artifact was updated.
    ArtifactUpdated { id: String },
    /// A task's status changed.
    TaskStatusChanged {
        id: String,
        old_status: String,
        new_status: String,
    },
    /// A workflow phase was completed.
    PhaseCompleted { phase: String },
    /// A checkpoint review was requested.
    CheckpointRequested { checkpoint: String },
    /// The workflow is complete.
    WorkflowComplete,
}

/// Context provided to tools during execution.
pub struct ToolContext {
    /// Session ID.
    pub session_id: String,
    /// Message ID.
    pub message_id: String,
    /// Agent name.
    pub agent: String,
    /// Cancellation token.
    pub abort: CancellationToken,
    /// Project root directory.
    pub root_dir: PathBuf,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Snapshot store for file versioning.
    pub snapshot: Option<Arc<SnapshotStore>>,
    /// File time tracker for concurrent edit detection.
    pub file_time: Option<Arc<FileTimeState>>,
    /// Optional sandbox runtime for isolated execution.
    pub sandbox: Option<Arc<dyn SandboxRuntime>>,
    /// Optional event sender for immediate notifications.
    pub event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,
    /// Optional ticket service for ticket management tools.
    pub ticket_service: Option<Arc<dyn TicketService>>,
    /// Optional memory service for memory tools.
    pub memory_service: Option<SharedMemoryService>,
    /// Optional permission checker for TypeScript tools.
    /// When set, TypeScript code can request user permission before performing actions.
    pub permission_checker: Option<Arc<dyn TsPermissionChecker>>,
    /// Workstream's ticket ID (from .wonopcode/state.yaml).
    /// This is the ticket ID associated with the current workstream.
    pub workstream_ticket_id: Option<String>,
    /// Default tracker ID for the workstream.
    /// This is the tracker that owns the workstream's ticket.
    /// Tools should use this as the default for ticket operations.
    pub workstream_default_tracker_id: Option<String>,
}

impl ToolContext {
    /// Check if sandbox execution is enabled.
    pub fn is_sandboxed(&self) -> bool {
        self.sandbox.is_some()
    }

    /// Get the sandbox runtime if available.
    pub fn sandbox(&self) -> Option<&Arc<dyn SandboxRuntime>> {
        self.sandbox.as_ref()
    }

    /// Convert a host path to sandbox path.
    ///
    /// Returns the original path if not sandboxed or if path is outside project.
    pub fn to_sandbox_path(&self, host_path: &Path) -> PathBuf {
        if let Some(sandbox) = &self.sandbox {
            sandbox
                .to_sandbox_path(host_path)
                .unwrap_or_else(|| host_path.to_path_buf())
        } else {
            host_path.to_path_buf()
        }
    }

    /// Convert a sandbox path to host path.
    ///
    /// Returns the original path if not sandboxed or if path is outside workspace.
    pub fn to_host_path(&self, sandbox_path: &Path) -> PathBuf {
        if let Some(sandbox) = &self.sandbox {
            sandbox
                .to_host_path(sandbox_path)
                .unwrap_or_else(|| sandbox_path.to_path_buf())
        } else {
            sandbox_path.to_path_buf()
        }
    }

    /// Get the effective working directory (sandbox or host).
    pub fn effective_cwd(&self) -> PathBuf {
        self.to_sandbox_path(&self.cwd)
    }

    /// Get the effective root directory (sandbox or host).
    pub fn effective_root(&self) -> PathBuf {
        self.to_sandbox_path(&self.root_dir)
    }
}

/// Result of tool execution.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Title/summary of the operation.
    pub title: String,
    /// Output text.
    pub output: String,
    /// Tool-specific metadata.
    pub metadata: Value,
}

impl ToolOutput {
    /// Create a new tool output.
    pub fn new(title: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            output: output.into(),
            metadata: Value::Null,
        }
    }

    /// Add metadata to the output.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// The main trait for tools.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool ID.
    fn id(&self) -> &str;

    /// Get the tool description (for the AI).
    fn description(&self) -> &str;

    /// Get the JSON Schema for the tool's parameters.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool.
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput>;

    /// Whether this tool requires user permission to execute.
    ///
    /// Read-only tools (like listing files, searching, reading tickets) can return false
    /// to allow execution without explicit permission. Write operations should return true.
    ///
    /// Default is true for safety.
    fn requires_permission(&self) -> bool {
        true
    }
}

/// A boxed tool for dynamic dispatch.
pub type BoxedTool = Arc<dyn Tool>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_context() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/test/root"),
            cwd: PathBuf::from("/test/root/subdir"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: None,
            memory_service: None,
            permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        }
    }

    #[test]
    fn test_tool_context_is_sandboxed() {
        let ctx = create_test_context();
        assert!(!ctx.is_sandboxed());
    }

    #[test]
    fn test_tool_context_sandbox_none() {
        let ctx = create_test_context();
        assert!(ctx.sandbox().is_none());
    }

    #[test]
    fn test_tool_context_to_sandbox_path_no_sandbox() {
        let ctx = create_test_context();
        let path = PathBuf::from("/test/file.txt");
        let result = ctx.to_sandbox_path(&path);
        assert_eq!(result, path); // Should return the same path when no sandbox
    }

    #[test]
    fn test_tool_context_to_host_path_no_sandbox() {
        let ctx = create_test_context();
        let path = PathBuf::from("/test/file.txt");
        let result = ctx.to_host_path(&path);
        assert_eq!(result, path); // Should return the same path when no sandbox
    }

    #[test]
    fn test_tool_context_effective_cwd() {
        let ctx = create_test_context();
        let result = ctx.effective_cwd();
        assert_eq!(result, PathBuf::from("/test/root/subdir"));
    }

    #[test]
    fn test_tool_context_effective_root() {
        let ctx = create_test_context();
        let result = ctx.effective_root();
        assert_eq!(result, PathBuf::from("/test/root"));
    }

    #[test]
    fn test_tool_output_new() {
        let output = ToolOutput::new("Title", "Content");
        assert_eq!(output.title, "Title");
        assert_eq!(output.output, "Content");
        assert!(output.metadata.is_null());
    }

    #[test]
    fn test_tool_output_with_metadata() {
        let output = ToolOutput::new("Title", "Content").with_metadata(json!({"key": "value"}));
        assert_eq!(output.title, "Title");
        assert_eq!(output.output, "Content");
        assert_eq!(output.metadata["key"], "value");
    }

    #[test]
    fn test_tool_event_clone() {
        let mut phased = todo::PhasedTodos::new();
        let mut phase = todo::Phase::new("phase_1", "Test Phase");
        phase.add_todo(todo::TodoItem {
            id: "1".to_string(),
            content: "Test".to_string(),
            status: todo::TodoStatus::Pending,
            priority: todo::TodoPriority::High,
            parents: vec![],
        });
        phased.add_phase(phase);

        let event = ToolEvent::TodosUpdated(phased);

        // Test that we can clone the event
        let cloned = event;
        if let ToolEvent::TodosUpdated(phased_todos) = cloned {
            assert_eq!(phased_todos.phases.len(), 1);
            assert_eq!(phased_todos.phases[0].todos.len(), 1);
            assert_eq!(phased_todos.phases[0].todos[0].id, "1");
        } else {
            panic!("Expected TodosUpdated event");
        }
    }

    #[test]
    fn test_tool_event_ace_variants() {
        // Test ACE event variants can be created and cloned
        let event1 = ToolEvent::ArtifactCreated {
            id: "UC-WON-123-001".to_string(),
            artifact_type: "use-case".to_string(),
        };
        // Clone to test Clone derive works, then verify original
        let _cloned1 = event1.clone();
        if let ToolEvent::ArtifactCreated { id, artifact_type } = event1 {
            assert_eq!(id, "UC-WON-123-001");
            assert_eq!(artifact_type, "use-case");
        }

        let event2 = ToolEvent::TaskStatusChanged {
            id: "TASK-WON-123-001".to_string(),
            old_status: "backlog".to_string(),
            new_status: "in_progress".to_string(),
        };
        // Clone to test Clone derive works, then verify original
        let _cloned2 = event2.clone();
        if let ToolEvent::TaskStatusChanged {
            id,
            old_status,
            new_status,
        } = event2
        {
            assert_eq!(id, "TASK-WON-123-001");
            assert_eq!(old_status, "backlog");
            assert_eq!(new_status, "in_progress");
        }
    }
}
