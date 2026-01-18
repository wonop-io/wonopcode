//! LSP transport implementation (JSON-RPC over stdio).

use crate::error::{LspError, LspResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, trace};

/// JSON-RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

/// LSP transport over stdio.
pub struct LspTransport {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    stdout: Mutex<Option<BufReader<tokio::process::ChildStdout>>>,
}

impl LspTransport {
    /// Create a new LSP transport by spawning the server process.
    pub async fn new(
        command: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
        cwd: Option<&std::path::Path>,
    ) -> LspResult<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .envs(env);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        debug!(command = command, args = ?args, "Starting LSP server");

        let mut child = cmd
            .spawn()
            .map_err(|e| LspError::ProcessError(format!("Failed to start server: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::ProcessError("Failed to get stdin".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::ProcessError("Failed to get stdout".to_string()))?;

        Ok(Self {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Some(stdin)),
            stdout: Mutex::new(Some(BufReader::new(stdout))),
        })
    }

    /// Send a request and wait for response.
    pub async fn request(&self, request: JsonRpcRequest) -> LspResult<JsonRpcResponse> {
        // Send request
        self.send_message(&serde_json::to_string(&request)?).await?;

        // Read response
        let response = self.read_message().await?;
        let response: JsonRpcResponse = serde_json::from_str(&response)
            .map_err(|e| LspError::protocol_error(format!("Invalid response: {e}")))?;

        Ok(response)
    }

    /// Send a notification.
    pub async fn notify(&self, notification: JsonRpcNotification) -> LspResult<()> {
        self.send_message(&serde_json::to_string(&notification)?)
            .await
    }

    /// Send an LSP message with Content-Length header.
    async fn send_message(&self, content: &str) -> LspResult<()> {
        let mut stdin_guard = self.stdin.lock().await;
        let stdin = stdin_guard
            .as_mut()
            .ok_or_else(|| LspError::connection_failed("Transport closed"))?;

        let message = format!("Content-Length: {}\r\n\r\n{}", content.len(), content);
        trace!(message = %content, "Sending LSP message");

        stdin.write_all(message.as_bytes()).await?;
        stdin.flush().await?;

        Ok(())
    }

    /// Read an LSP message.
    async fn read_message(&self) -> LspResult<String> {
        let mut stdout_guard = self.stdout.lock().await;
        let stdout = stdout_guard
            .as_mut()
            .ok_or_else(|| LspError::connection_failed("Transport closed"))?;

        // Read headers
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let bytes = stdout.read_line(&mut line).await?;
            if bytes == 0 {
                return Err(LspError::connection_failed("Server closed connection"));
            }

            let line = line.trim();
            if line.is_empty() {
                break;
            }

            if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                content_length = Some(
                    len_str
                        .parse()
                        .map_err(|_| LspError::protocol_error("Invalid Content-Length"))?,
                );
            }
        }

        let content_length = content_length
            .ok_or_else(|| LspError::protocol_error("Missing Content-Length header"))?;

        // Read content
        let mut content = vec![0u8; content_length];
        stdout.read_exact(&mut content).await?;

        let content = String::from_utf8(content)
            .map_err(|e| LspError::protocol_error(format!("Invalid UTF-8: {e}")))?;

        trace!(content = %content, "Received LSP message");

        Ok(content)
    }

    /// Close the transport.
    pub async fn close(&self) -> LspResult<()> {
        // Close stdin
        let mut stdin_guard = self.stdin.lock().await;
        *stdin_guard = None;

        // Wait for process to exit
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let _ = child.kill().await;
        }

        debug!("Closed LSP server transport");
        Ok(())
    }
}

impl Drop for LspTransport {
    fn drop(&mut self) {
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
    async fn test_transport_creation_failure() {
        let result = LspTransport::new("nonexistent_lsp_12345", &[], &HashMap::new(), None).await;

        assert!(result.is_err());
    }
}
