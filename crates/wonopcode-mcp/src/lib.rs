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
//! - **stdio**: Local servers via stdin/stdout
//! - **SSE**: Remote servers via Server-Sent Events (HTTP)
//! - **OAuth**: Authentication for remote servers
//!
//! # Example
//!
//! ```no_run
//! use wonopcode_mcp::{McpClient, ServerConfig, Transport};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Configure a local MCP server
//! let config = ServerConfig::stdio(
//!     "filesystem",
//!     "npx",
//!     vec!["-y", "@modelcontextprotocol/server-filesystem"],
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
pub use protocol::{McpTool, ToolCallResult, ToolContent};
pub use serve::{McpServer, McpServerTool, McpToolContext, McpToolExecutor};
pub use server::ServerConfig;
pub use sse::{SseConfig, SseTransport};
pub use transport::Transport;
