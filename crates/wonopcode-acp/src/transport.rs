//! ACP transport layer for stdio communication.
//!
//! Implements newline-delimited JSON (ndjson) transport over stdin/stdout
//! for communication with IDE clients.

use crate::types::{JsonRpcError, JsonRpcId, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};

/// Transport error.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Request timed out")]
    Timeout,

    #[error("Invalid response")]
    InvalidResponse,
}

/// Message from client.
#[derive(Debug)]
pub enum IncomingMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
}

/// Pending request awaiting response.
struct PendingRequest {
    sender: oneshot::Sender<Result<serde_json::Value, JsonRpcError>>,
}

/// ACP transport over stdio.
pub struct StdioTransport {
    /// Sender for outgoing messages.
    outgoing_tx: mpsc::Sender<String>,
    /// Pending requests awaiting responses.
    pending: Arc<Mutex<HashMap<JsonRpcId, PendingRequest>>>,
    /// Next request ID.
    next_id: Arc<Mutex<i64>>,
}

impl StdioTransport {
    /// Create a new stdio transport and start the I/O loops.
    pub fn new() -> (Self, mpsc::Receiver<IncomingMessage>) {
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<String>(100);
        let (incoming_tx, incoming_rx) = mpsc::channel::<IncomingMessage>(100);
        let pending = Arc::new(Mutex::new(HashMap::new()));

        // Start stdin reader task
        let pending_clone = pending.clone();
        let incoming_tx_clone = incoming_tx;
        tokio::spawn(async move {
            Self::stdin_loop(incoming_tx_clone, pending_clone).await;
        });

        // Start stdout writer task
        tokio::spawn(async move {
            Self::stdout_loop(outgoing_rx).await;
        });

        let transport = Self {
            outgoing_tx,
            pending,
            next_id: Arc::new(Mutex::new(1)),
        };

        (transport, incoming_rx)
    }

    /// Read from stdin and dispatch messages.
    #[allow(clippy::cognitive_complexity)]
    async fn stdin_loop(
        incoming_tx: mpsc::Sender<IncomingMessage>,
        pending: Arc<Mutex<HashMap<JsonRpcId, PendingRequest>>>,
    ) {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    debug!("Received: {}", line);

                    // Try to parse as a response first
                    if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) {
                        let mut pending = pending.lock().await;
                        if let Some(req) = pending.remove(&response.id) {
                            let result = if let Some(error) = response.error {
                                Err(error)
                            } else {
                                Ok(response.result.unwrap_or(serde_json::Value::Null))
                            };
                            let _ = req.sender.send(result);
                        }
                        continue;
                    }

                    // Try to parse as a request
                    if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&line) {
                        if let Err(e) = incoming_tx.send(IncomingMessage::Request(request)).await {
                            error!("Failed to send request: {}", e);
                            break;
                        }
                        continue;
                    }

                    // Try to parse as a notification
                    if let Ok(notification) = serde_json::from_str::<JsonRpcNotification>(&line) {
                        if let Err(e) = incoming_tx
                            .send(IncomingMessage::Notification(notification))
                            .await
                        {
                            error!("Failed to send notification: {}", e);
                            break;
                        }
                        continue;
                    }

                    warn!("Failed to parse message: {}", line);
                }
                Ok(None) => {
                    info!("stdin closed");
                    break;
                }
                Err(e) => {
                    error!("Error reading stdin: {}", e);
                    break;
                }
            }
        }
    }

    /// Write messages to stdout.
    #[allow(clippy::cognitive_complexity)]
    async fn stdout_loop(mut rx: mpsc::Receiver<String>) {
        let mut stdout = tokio::io::stdout();

        while let Some(msg) = rx.recv().await {
            debug!("Sending: {}", msg);
            if let Err(e) = stdout.write_all(msg.as_bytes()).await {
                error!("Error writing to stdout: {}", e);
                break;
            }
            if let Err(e) = stdout.write_all(b"\n").await {
                error!("Error writing newline: {}", e);
                break;
            }
            if let Err(e) = stdout.flush().await {
                error!("Error flushing stdout: {}", e);
                break;
            }
        }
    }

    /// Send a response to a request.
    pub async fn send_response(
        &self,
        id: JsonRpcId,
        result: Result<serde_json::Value, JsonRpcError>,
    ) -> Result<(), TransportError> {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: result.as_ref().ok().cloned(),
            error: result.err(),
        };

        let json = serde_json::to_string(&response)?;
        self.outgoing_tx
            .send(json)
            .await
            .map_err(|_| TransportError::ChannelClosed)?;

        Ok(())
    }

    /// Send a notification (no response expected).
    pub async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), TransportError> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(params),
        };

        let json = serde_json::to_string(&notification)?;
        self.outgoing_tx
            .send(json)
            .await
            .map_err(|_| TransportError::ChannelClosed)?;

        Ok(())
    }

    /// Send a request and wait for response.
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = *next_id;
            *next_id += 1;
            JsonRpcId::Number(id)
        };

        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id.clone(), PendingRequest { sender: tx });
        }

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id.clone()),
            method: method.to_string(),
            params: Some(params),
        };

        let json = serde_json::to_string(&request)?;
        self.outgoing_tx
            .send(json)
            .await
            .map_err(|_| TransportError::ChannelClosed)?;

        // Wait for response with timeout
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => {
                result.map_err(|e| TransportError::Io(std::io::Error::other(e.message)))
            }
            Ok(Err(_)) => Err(TransportError::ChannelClosed),
            Err(_) => {
                // Remove pending request
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                Err(TransportError::Timeout)
            }
        }
    }
}

/// Connection wrapping the transport for the agent.
pub struct Connection {
    transport: Arc<StdioTransport>,
}

impl Connection {
    pub fn new(transport: Arc<StdioTransport>) -> Self {
        Self { transport }
    }

    /// Send a session update notification.
    pub async fn session_update(
        &self,
        params: crate::types::SessionUpdateNotification,
    ) -> Result<(), TransportError> {
        self.transport
            .send_notification("session/update", serde_json::to_value(params)?)
            .await
    }

    /// Request permission from the client.
    pub async fn request_permission(
        &self,
        params: crate::types::PermissionRequest,
    ) -> Result<crate::types::PermissionResponse, TransportError> {
        let result = self
            .transport
            .send_request("permission/request", serde_json::to_value(params)?)
            .await?;

        serde_json::from_value(result).map_err(|_| TransportError::InvalidResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId::Number(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({"protocolVersion": 1})),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_json_rpc_response_serialization() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: JsonRpcId::Number(1),
            result: Some(serde_json::json!({"success": true})),
            error: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"success\":true"));
    }
}
