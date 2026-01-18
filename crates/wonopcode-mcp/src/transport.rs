//! MCP transport implementations.

use crate::error::McpResult;
use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use async_trait::async_trait;

/// Transport trait for MCP communication.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a request and wait for a response.
    async fn request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse>;

    /// Send a notification (no response expected).
    async fn notify(&self, notification: JsonRpcNotification) -> McpResult<()>;

    /// Close the transport.
    async fn close(&self) -> McpResult<()>;

    /// Check if the transport is connected.
    fn is_connected(&self) -> bool;
}
