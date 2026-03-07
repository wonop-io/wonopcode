//! Tool execution utilities for the standard loop.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use wonopcode_sandbox::SandboxRuntime;
use wonopcode_snapshot::SnapshotStore;
use wonopcode_tools::{
    ToolContext, ToolError, ToolEvent, ToolOutput, ToolRegistry, TsPermissionChecker,
};
use wonopcode_util::FileTimeState;

use crate::context::{PermissionCheckRequest, PermissionChecker};

/// Executes tools with permission checking and parallel execution support.
pub struct ToolExecutor<'a> {
    tools: &'a ToolRegistry,
    snapshot_store: Option<Arc<SnapshotStore>>,
    file_time: Arc<FileTimeState>,
    sandbox: Option<Arc<dyn SandboxRuntime>>,
    event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,
    permission_checker: Option<Arc<dyn PermissionChecker>>,
    /// Optional timeout for tool execution (including permission checks).
    /// When set, permission requests will be cancelled after this duration.
    tool_timeout: Option<std::time::Duration>,
    /// Optional ticket service for ticket management tools.
    ticket_service: Option<Arc<dyn wonopcode_tools::TicketService>>,
    /// Optional memory service for memory tools.
    memory_service: Option<wonopcode_tools::SharedMemoryService>,
    /// Optional permission checker for TypeScript tools.
    /// This allows TypeScript code to request fine-grained permissions.
    ts_permission_checker: Option<Arc<dyn TsPermissionChecker>>,
    /// Optional workstream ticket ID (for default tracker resolution).
    workstream_ticket_id: Option<String>,
    /// Optional default tracker ID for the workstream.
    workstream_default_tracker_id: Option<String>,
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
            permission_checker: None,
            tool_timeout: None,
            ticket_service: None,
            memory_service: None,
            ts_permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
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
            permission_checker: None,
            tool_timeout: None,
            ticket_service: None,
            memory_service: None,
            ts_permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        }
    }

    /// Create a new tool executor with permission checking.
    ///
    /// When a permission checker is provided, tool execution will check
    /// permissions before running. If permission is denied, an error is returned.
    ///
    /// The `tool_timeout` parameter specifies the maximum time to wait for
    /// permission decisions. Providers like Claude CLI have a hard timeout
    /// on MCP tool calls, so we need to cancel permission requests before
    /// the provider times out.
    pub fn with_permissions(
        tools: &'a ToolRegistry,
        snapshot_store: Option<Arc<SnapshotStore>>,
        file_time: Arc<FileTimeState>,
        sandbox: Option<Arc<dyn SandboxRuntime>>,
        event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,
        permission_checker: Option<Arc<dyn PermissionChecker>>,
        tool_timeout: Option<std::time::Duration>,
    ) -> Self {
        Self {
            tools,
            snapshot_store,
            file_time,
            sandbox,
            event_tx,
            permission_checker,
            tool_timeout,
            ticket_service: None,
            memory_service: None,
            ts_permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        }
    }

    /// Create a tool executor with all services.
    ///
    /// This is the full constructor that includes ticket and memory services.
    #[allow(clippy::too_many_arguments)]
    pub fn with_services(
        tools: &'a ToolRegistry,
        snapshot_store: Option<Arc<SnapshotStore>>,
        file_time: Arc<FileTimeState>,
        sandbox: Option<Arc<dyn SandboxRuntime>>,
        event_tx: Option<mpsc::UnboundedSender<ToolEvent>>,
        permission_checker: Option<Arc<dyn PermissionChecker>>,
        tool_timeout: Option<std::time::Duration>,
        ticket_service: Option<Arc<dyn wonopcode_tools::TicketService>>,
        memory_service: Option<wonopcode_tools::SharedMemoryService>,
        ts_permission_checker: Option<Arc<dyn TsPermissionChecker>>,
        workstream_ticket_id: Option<String>,
        workstream_default_tracker_id: Option<String>,
    ) -> Self {
        Self {
            tools,
            snapshot_store,
            file_time,
            sandbox,
            event_tx,
            permission_checker,
            tool_timeout,
            ticket_service,
            memory_service,
            ts_permission_checker,
            workstream_ticket_id,
            workstream_default_tracker_id,
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

        // Check permissions if a permission checker is configured
        if let Some(ref checker) = self.permission_checker {
            // Extract path from args for file-related tools
            let path = input
                .get("filePath")
                .or_else(|| input.get("path"))
                .or_else(|| input.get("file"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let has_sandbox = checker.is_sandbox_running();
            let request = PermissionCheckRequest {
                id: uuid::Uuid::new_v4().to_string(),
                tool: normalized_name.to_string(),
                action: "execute".to_string(),
                path: path.clone(),
                description: format!("Execute tool: {normalized_name}"),
                details: input.clone(),
            };

            info!(
                tool = %normalized_name,
                path = ?path,
                sandbox_running = has_sandbox,
                timeout = ?self.tool_timeout,
                "Checking permission for tool execution"
            );

            let allowed = checker
                .check_permission(session_id, request, self.tool_timeout)
                .await;

            if !allowed {
                warn!(
                    tool = %normalized_name,
                    path = ?path,
                    "Permission denied for tool execution"
                );
                return Err(ToolError::permission_denied(format!(
                    "Permission denied for tool '{normalized_name}'"
                )));
            }

            debug!(
                tool = %normalized_name,
                "Permission granted for tool execution"
            );
        }

        // Build context
        // Note: ace_service is set to None here - the execute_typescript tool
        // creates a FileAceService lazily from root_dir if needed.
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
            ticket_service: self.ticket_service.clone(),
            memory_service: self.memory_service.clone(),
            ace_service: None,
            permission_checker: self.ts_permission_checker.clone(),
            workstream_ticket_id: self.workstream_ticket_id.clone(),
            workstream_default_tracker_id: self.workstream_default_tracker_id.clone(),
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
