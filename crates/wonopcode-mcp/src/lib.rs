//! Model Context Protocol (MCP) client for wonopcode.
//!
//! MCP allows wonopcode to connect to external tool servers, dramatically
//! extending its capabilities without modifying the core codebase.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │  wonopcode  │────▶│  MCP Client  │────▶│ MCP Servers │
//! │    (AI)     │◀────│              │◀────│   (tools)   │
//! └─────────────┘     └──────────────┘     └─────────────┘
//! ```
//!
//! # Supported Transports
//!
//! - **SSE**: Servers via Server-Sent Events (HTTP)
//! - **OAuth**: Authentication for remote servers
//!
//! # Example
//!
//! ```no_run
//! use wonopcode_mcp::{McpClient, ServerConfig};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Configure an MCP server via HTTP/SSE
//! let config = ServerConfig::sse(
//!     "my-server",
//!     "http://localhost:3000/mcp/sse",
//! );
//!
//! // Create client and connect
//! let mut client = McpClient::new();
//! client.add_server(config).await?;
//!
//! // Discover tools
//! let tools = client.list_tools().await;
//!
//! // Call a tool
//! let result = client.call_tool("read_file", serde_json::json!({
//!     "path": "/path/to/file"
//! })).await?;
//! # Ok(())
//! # }
//! ```

pub mod callback;
mod client;
mod error;
pub mod http_serve;
pub mod oauth;
pub mod protocol;
pub mod serve;
mod server;
pub mod sse;
mod transport;

pub use callback::OAuthCallbackServer;
pub use client::McpClient;
pub use error::{McpError, McpResult};
pub use http_serve::{create_mcp_router, McpHttpState};
pub use oauth::{
    OAuthConfig, OAuthProvider, OAuthTokens, OAUTH_CALLBACK_PATH, OAUTH_CALLBACK_PORT,
};
pub use protocol::{
    McpTool, PermissionRequestParams, PermissionResponseParams, ToolCallResult, ToolContent,
    METHOD_PERMISSION_REQUEST, METHOD_PERMISSION_RESPONSE,
};
pub use serve::{
    McpServerTool, McpToolContext, McpToolExecutor, PendingPermissions, PERMISSION_TIMEOUT_SECS,
};
pub use server::ServerConfig;
pub use sse::{SseConfig, SseTransport};
pub use transport::Transport;
