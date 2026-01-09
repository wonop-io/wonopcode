//! MCP server for exposing wonopcode tools.
//!
//! This module provides shared types and utilities for the MCP server.
//! The actual HTTP/SSE server implementation is in `http_serve.rs`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     HTTP/SSE           ┌──────────────────┐
//! │   Claude CLI    │ ◄────────────────────► │   McpHttpState   │
//! │   (MCP client)  │                        │ (wonopcode tools)│
//! └─────────────────┘                        └──────────────────┘
//!                                                      │
//!                                                      ▼
//!                                            ┌──────────────────┐
//!                                            │   ToolRegistry   │
//!                                            │ (bash, read, etc)│
//!                                            └──────────────────┘
//! ```

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

/// Tool definition for the MCP server.
/// This is a simplified interface that doesn't require the full wonopcode-tools crate.
#[derive(Clone)]
pub struct McpServerTool {
    /// Tool name/ID.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON Schema for parameters.
    pub parameters: Value,
    /// Tool executor function.
    pub executor: Arc<dyn McpToolExecutor>,
}

impl std::fmt::Debug for McpServerTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

/// Trait for tool execution.
#[async_trait::async_trait]
pub trait McpToolExecutor: Send + Sync {
    /// Execute the tool with given arguments.
    async fn execute(&self, args: Value, ctx: &McpToolContext) -> Result<String, String>;
}

/// Context provided to tools during execution.
#[derive(Debug, Clone)]
pub struct McpToolContext {
    /// Session ID.
    pub session_id: String,
    /// Working directory.
    pub cwd: PathBuf,
    /// Root directory (project root).
    pub root_dir: PathBuf,
}

impl Default for McpToolContext {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        Self {
            session_id: "mcp-default".to_string(),
            cwd: cwd.clone(),
            root_dir: cwd,
        }
    }
}

/// Default timeout for permission requests (5 minutes).
pub const PERMISSION_TIMEOUT_SECS: u64 = 300;

/// Shared state for pending permission requests.
///
/// This is shared between the server's main loop (which receives permission responses)
/// and tool executors (which wait for permission).
#[derive(Debug, Default)]
pub struct PendingPermissions {
    /// Map of request_id -> oneshot sender to deliver the permission decision.
    requests: RwLock<HashMap<String, oneshot::Sender<bool>>>,
}

impl PendingPermissions {
    /// Create a new pending permissions tracker.
    pub fn new() -> Self {
        Self {
            requests: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new pending permission request.
    /// Returns a receiver that will get the permission decision.
    pub async fn register(&self, request_id: String) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.requests.write().await.insert(request_id, tx);
        rx
    }

    /// Resolve a pending permission request.
    /// Returns true if the request was found and resolved.
    pub async fn resolve(&self, request_id: &str, allowed: bool) -> bool {
        if let Some(tx) = self.requests.write().await.remove(request_id) {
            let _ = tx.send(allowed);
            true
        } else {
            false
        }
    }

    /// Cancel a pending permission request (e.g., on timeout).
    pub async fn cancel(&self, request_id: &str) {
        self.requests.write().await.remove(request_id);
    }
}

/// Builder for McpServerTool.
pub struct McpServerToolBuilder {
    name: String,
    description: String,
    parameters: Value,
}

impl McpServerToolBuilder {
    /// Create a new tool builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    /// Set the tool description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the parameters schema.
    pub fn parameters(mut self, parameters: Value) -> Self {
        self.parameters = parameters;
        self
    }

    /// Build the tool with an executor.
    pub fn build(self, executor: impl McpToolExecutor + 'static) -> McpServerTool {
        McpServerTool {
            name: self.name,
            description: self.description,
            parameters: self.parameters,
            executor: Arc::new(executor),
        }
    }
}

/// Simple executor that wraps a closure.
pub struct ClosureExecutor<F>
where
    F: Fn(Value, &McpToolContext) -> Result<String, String> + Send + Sync,
{
    f: F,
}

impl<F> ClosureExecutor<F>
where
    F: Fn(Value, &McpToolContext) -> Result<String, String> + Send + Sync,
{
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

#[async_trait::async_trait]
impl<F> McpToolExecutor for ClosureExecutor<F>
where
    F: Fn(Value, &McpToolContext) -> Result<String, String> + Send + Sync,
{
    async fn execute(&self, args: Value, ctx: &McpToolContext) -> Result<String, String> {
        (self.f)(args, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_context_default() {
        let ctx = McpToolContext::default();
        assert_eq!(ctx.session_id, "mcp-default");
    }

    #[test]
    fn test_pending_permissions() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let pending = PendingPermissions::new();

            // Register a permission request
            let rx = pending.register("test-id".to_string()).await;

            // Resolve it
            let found = pending.resolve("test-id", true).await;
            assert!(found);

            // Should receive the response
            let result = rx.await.unwrap();
            assert!(result);
        });
    }

    #[test]
    fn test_closure_executor() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let executor = ClosureExecutor::new(|args, _ctx| {
                let msg = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                Ok(msg.to_string())
            });

            let ctx = McpToolContext::default();
            let result = executor
                .execute(serde_json::json!({"message": "hello"}), &ctx)
                .await;
            assert_eq!(result.unwrap(), "hello");
        });
    }
}
