//! MCP transport implementations.

use crate::error::{McpError, McpResult};
use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

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

/// stdio transport for local MCP servers.
pub struct StdioTransport {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    stdout: Mutex<Option<BufReader<tokio::process::ChildStdout>>>,
}

impl StdioTransport {
    /// Create a new stdio transport.
    pub async fn new(
        command: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
        cwd: Option<&std::path::Path>,
    ) -> McpResult<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .envs(env);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        debug!(command = command, args = ?args, "Starting MCP server process");

        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::ProcessError(format!("Failed to start server: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::ProcessError("Failed to get stdin".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::ProcessError("Failed to get stdout".to_string()))?;

        Ok(Self {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Some(stdin)),
            stdout: Mutex::new(Some(BufReader::new(stdout))),
        })
    }

    /// Send a line to the server.
    async fn send_line(&self, line: &str) -> McpResult<()> {
        let mut stdin_guard = self.stdin.lock().await;
        let stdin = stdin_guard
            .as_mut()
            .ok_or_else(|| McpError::connection_failed("Transport closed"))?;

        trace!(line = line, "Sending to MCP server");
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        Ok(())
    }

    /// Read a line from the server.
    async fn read_line(&self) -> McpResult<String> {
        let mut stdout_guard = self.stdout.lock().await;
        let stdout = stdout_guard
            .as_mut()
            .ok_or_else(|| McpError::connection_failed("Transport closed"))?;

        let mut line = String::new();
        let bytes_read = stdout.read_line(&mut line).await?;

        if bytes_read == 0 {
            return Err(McpError::connection_failed("Server closed connection"));
        }

        trace!(line = line.trim(), "Received from MCP server");
        Ok(line)
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse> {
        let request_json = serde_json::to_string(&request)?;
        self.send_line(&request_json).await?;

        // Read response
        let response_line = self.read_line().await?;
        let response: JsonRpcResponse = serde_json::from_str(&response_line).map_err(|e| {
            McpError::protocol_error(format!(
                "Invalid response: {} - {}",
                e,
                response_line.trim()
            ))
        })?;

        if let Some(ref error) = response.error {
            warn!(
                code = error.code,
                message = %error.message,
                "MCP server returned error"
            );
        }

        Ok(response)
    }

    async fn notify(&self, notification: JsonRpcNotification) -> McpResult<()> {
        let notification_json = serde_json::to_string(&notification)?;
        self.send_line(&notification_json).await
    }

    async fn close(&self) -> McpResult<()> {
        // Close stdin to signal server to exit
        let mut stdin_guard = self.stdin.lock().await;
        *stdin_guard = None;

        // Wait for child process to exit
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            // Give it a moment to exit gracefully
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Kill if still running
            let _ = child.kill().await;
        }

        debug!("Closed MCP server transport");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        // Check if stdin is still available
        // This is a simple check; actual connection state may differ
        true // We'd need to try sending to know for sure
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Attempt to kill the child process if still running
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(ref mut child) = *guard {
                let _ = child.start_kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_stdio_transport_creation_failure() {
        // Try to create a transport with a non-existent command
        let result =
            StdioTransport::new("nonexistent_command_12345", &[], &HashMap::new(), None).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stdio_transport_echo() {
        // Use echo as a simple "server"
        // Note: This won't work as a real MCP server, just testing transport
        let transport = StdioTransport::new("cat", &[], &HashMap::new(), None).await;

        // cat should start successfully
        assert!(transport.is_ok());

        // Close it
        let transport = transport.unwrap();
        transport.close().await.unwrap();
    }
}
