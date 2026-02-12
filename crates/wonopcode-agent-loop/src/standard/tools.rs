//! Tool execution utilities for the standard loop.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use wonopcode_sandbox::SandboxRuntime;
use wonopcode_snapshot::SnapshotStore;
use wonopcode_tools::{ToolContext, ToolError, ToolEvent, ToolOutput, ToolRegistry};
use wonopcode_util::FileTimeState;

/// Executes tools with permission checking and parallel execution support.
pub struct ToolExecutor<'a> {
    tools: &'a ToolRegistry,
    snapshot_store: Option<Arc<SnapshotStore>>,
    file_time: Arc<FileTimeState>,
    sandbox: Option<Arc<dyn SandboxRuntime>>,
    event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,
}

impl<'a> ToolExecutor<'a> {
    /// Create a new tool executor.
    pub fn new(
        tools: &'a ToolRegistry,
        snapshot_store: Option<Arc<SnapshotStore>>,
        file_time: Arc<FileTimeState>,
        sandbox: Option<Arc<dyn SandboxRuntime>>,
    ) -> Self {
        Self {
            tools,
            snapshot_store,
            file_time,
            sandbox,
            event_tx: None,
        }
    }

    /// Create a new tool executor with an event channel.
    ///
    /// The event channel allows tools to emit events (like `TodosUpdated`)
    /// that can be forwarded to the UI for real-time updates.
    pub fn with_event_tx(
        tools: &'a ToolRegistry,
        snapshot_store: Option<Arc<SnapshotStore>>,
        file_time: Arc<FileTimeState>,
        sandbox: Option<Arc<dyn SandboxRuntime>>,
        event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,
    ) -> Self {
        Self {
            tools,
            snapshot_store,
            file_time,
            sandbox,
            event_tx,
        }
    }

    /// Execute a single tool.
    pub async fn execute(
        &self,
        tool_name: &str,
        input: serde_json::Value,
        cwd: &Path,
        session_id: &str,
        cancel: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        // Normalize tool name - MCP tools have prefix like "mcp__wonopcode-tools__read"
        let normalized_name = tool_name.rsplit("__").next().unwrap_or(tool_name);

        debug!(
            tool = %normalized_name,
            original_name = %tool_name,
            "Executing tool"
        );

        // Get tool from registry
        let tool = self.tools.get(normalized_name).or_else(|| {
            // Try original name if normalized didn't work
            self.tools.get(tool_name)
        });

        let tool = match tool {
            Some(t) => t,
            None => {
                return Err(ToolError::validation(format!("Unknown tool: {tool_name}")));
            }
        };

        // Build context
        let ctx = ToolContext {
            session_id: session_id.to_string(),
            message_id: format!("msg-{}", uuid::Uuid::new_v4()),
            agent: "standard".to_string(),
            abort: cancel.clone(),
            root_dir: cwd.to_path_buf(),
            cwd: cwd.to_path_buf(),
            snapshot: self.snapshot_store.clone(),
            file_time: Some(self.file_time.clone()),
            sandbox: self.sandbox.clone(),
            event_tx: self.event_tx.clone(),
        };

        // Execute
        info!(
            tool = %tool.id(),
            "Tool execution starting"
        );

        let result = tool.execute(input, &ctx).await;

        match &result {
            Ok(output) => {
                info!(
                    tool = %tool.id(),
                    output_len = output.output.len(),
                    "Tool execution succeeded"
                );
            }
            Err(e) => {
                info!(
                    tool = %tool.id(),
                    error = %e,
                    "Tool execution failed"
                );
            }
        }

        result
    }

    /// Execute multiple tools sequentially.
    ///
    /// Returns results in the same order as the input.
    /// Note: For true parallel execution, the caller should spawn tasks.
    pub async fn execute_all(
        &self,
        calls: &[(String, String, serde_json::Value)], // (id, name, args)
        cwd: &Path,
        session_id: &str,
        cancel: &CancellationToken,
    ) -> Vec<(String, Result<ToolOutput, ToolError>)> {
        let mut results = Vec::with_capacity(calls.len());

        for (id, name, args) in calls {
            let result = self
                .execute(name, args.clone(), cwd, session_id, cancel)
                .await;
            results.push((id.clone(), result));
        }

        results
    }
}

#[cfg(test)]
mod tests {
    // Note: Full tests would require mocking the tool registry and tools.
    // This is a placeholder for the test structure.

    #[test]
    fn test_tool_name_normalization() {
        // Test that MCP tool names are normalized correctly
        let full_name = "mcp__wonopcode-tools__read";
        let normalized = full_name.rsplit("__").next().unwrap_or(full_name);
        assert_eq!(normalized, "read");

        // Regular tool names stay the same
        let regular_name = "read";
        let normalized = regular_name.rsplit("__").next().unwrap_or(regular_name);
        assert_eq!(normalized, "read");
    }
}
