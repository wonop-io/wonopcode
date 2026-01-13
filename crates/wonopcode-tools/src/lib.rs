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
    /// Todo list was updated with new items.
    TodosUpdated(Vec<todo::TodoItem>),
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
