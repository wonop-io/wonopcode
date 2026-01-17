//! Tool implementations for wonopcode.
//!
//! This crate provides the tools that AI agents can use to interact
//! with the codebase and environment.

pub mod error;
pub mod registry;

// Tool implementations
pub mod bash;
pub mod batch;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod list;
pub mod lsp;
pub mod mcp;
pub mod multiedit;
pub mod patch;
pub mod plan_mode;
pub mod read;
pub mod search;
pub mod skill;
pub mod task;
pub mod todo;
pub mod webfetch;
pub mod write;

pub use error::{ToolError, ToolResult};
pub use registry::ToolRegistry;

use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wonopcode_sandbox::SandboxRuntime;
use wonopcode_snapshot::SnapshotStore;
use wonopcode_util::FileTimeState;

/// Event that tools can emit to notify listeners of state changes.
#[derive(Debug, Clone)]
pub enum ToolEvent {
    /// Todo list was updated with new phased structure.
    TodosUpdated(todo::PhasedTodos),
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
        let output = ToolOutput::new("Title", "Content")
            .with_metadata(json!({"key": "value"}));
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
        });
        phased.add_phase(phase);

        let event = ToolEvent::TodosUpdated(phased);

        // Test that we can clone the event
        let cloned = event.clone();
        let ToolEvent::TodosUpdated(phased_todos) = cloned;
        assert_eq!(phased_todos.phases.len(), 1);
        assert_eq!(phased_todos.phases[0].todos.len(), 1);
        assert_eq!(phased_todos.phases[0].todos[0].id, "1");
    }
}
