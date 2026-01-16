//! OAuth callback server for MCP authentication.
//!
//! Implements an HTTP server that listens for OAuth callbacks from the browser.

use crate::error::{McpError, McpResult};
use crate::oauth::{OAUTH_CALLBACK_PATH, OAUTH_CALLBACK_PORT};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// HTML response for successful authorization.
const HTML_SUCCESS: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Wonopcode - Authorization Successful</title>
  <style>
    body { font-family: system-ui, -apple-system, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #1a1a2e; color: #eee; }
    .container { text-align: center; padding: 2rem; }
    h1 { color: #4ade80; margin-bottom: 1rem; }
    p { color: #aaa; }
  </style>
</head>
<body>
  <div class="container">
    <h1>Authorization Successful</h1>
    <p>You can close this window and return to Wonopcode.</p>
  </div>
  <script>setTimeout(() => window.close(), 2000);</script>
</body>
</html>"#;

/// HTML response for failed authorization.
fn html_error(error: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <title>Wonopcode - Authorization Failed</title>
  <style>
    body {{ font-family: system-ui, -apple-system, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #1a1a2e; color: #eee; }}
    .container {{ text-align: center; padding: 2rem; }}
    h1 {{ color: #f87171; margin-bottom: 1rem; }}
    p {{ color: #aaa; }}
    .error {{ color: #fca5a5; font-family: monospace; margin-top: 1rem; padding: 1rem; background: rgba(248,113,113,0.1); border-radius: 0.5rem; }}
  </style>
</head>
<body>
  <div class="container">
    <h1>Authorization Failed</h1>
    <p>An error occurred during authorization.</p>
    <div class="error">{}</div>
  </div>
</body>
</html>"#,
        html_escape(error)
    )
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Pending authorization request.
struct PendingAuth {
    /// Channel to send the authorization code.
    sender: oneshot::Sender<Result<String, String>>,
}

/// OAuth callback server state.
struct CallbackServerState {
    /// Pending authorization requests keyed by state parameter.
    pending: HashMap<String, PendingAuth>,
}

/// OAuth callback server for handling browser OAuth redirects.
pub struct OAuthCallbackServer {
    /// Server state.
    state: Arc<RwLock<CallbackServerState>>,
    /// Shutdown signal sender.
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// Whether the server is running.
    running: Arc<RwLock<bool>>,
}

impl OAuthCallbackServer {
    /// Create a new OAuth callback server.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(CallbackServerState {
                pending: HashMap::new(),
            })),
            shutdown_tx: Mutex::new(None),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Check if the callback port is already in use.
    pub async fn is_port_in_use() -> bool {
        let addr = SocketAddr::from(([127, 0, 0, 1], OAUTH_CALLBACK_PORT));
        TcpListener::bind(addr).await.is_err()
    }

    /// Start the OAuth callback server.
    pub async fn start(&self) -> McpResult<()> {
        // Check if already running
        {
            let running = self.running.read().await;
            if *running {
                return Ok(());
            }
        }

        // Check if another instance is using the port
        if Self::is_port_in_use().await {
            info!(
                port = OAUTH_CALLBACK_PORT,
                "OAuth callback server already running on another instance"
            );
            return Ok(());
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], OAUTH_CALLBACK_PORT));
        let listener = TcpListener::bind(addr).await.map_err(|e| {
            McpError::connection_failed(format!("Failed to bind OAuth callback server: {e}"))
        })?;

        info!(port = OAUTH_CALLBACK_PORT, "OAuth callback server started");

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        {
            let mut tx = self.shutdown_tx.lock().await;
            *tx = Some(shutdown_tx);
        }
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        let state = self.state.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let state = state.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(stream, state).await {
                                        warn!(error = %e, "Error handling OAuth callback");
                                    }
                                });
                            }
                            Err(e) => {
                                warn!(error = %e, "Error accepting connection");
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        info!("OAuth callback server shutting down");
                        break;
                    }
                }
            }

            let mut running = running.write().await;
            *running = false;
        });

        Ok(())
    }

    /// Wait for an OAuth callback with the given state parameter.
    ///
    /// Returns the authorization code when the callback is received.
    pub async fn wait_for_callback(&self, oauth_state: String) -> McpResult<String> {
        let (tx, rx) = oneshot::channel();

        {
            let mut state = self.state.write().await;
            state
                .pending
                .insert(oauth_state.clone(), PendingAuth { sender: tx });
        }

        // Wait for callback with timeout
        let timeout = tokio::time::Duration::from_secs(5 * 60); // 5 minutes
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(code))) => Ok(code),
            Ok(Ok(Err(error))) => Err(McpError::AuthFailed(error)),
            Ok(Err(_)) => {
                // Channel closed, clean up
                let mut state = self.state.write().await;
                state.pending.remove(&oauth_state);
                Err(McpError::AuthFailed("Authorization cancelled".to_string()))
            }
            Err(_) => {
                // Timeout, clean up
                let mut state = self.state.write().await;
                state.pending.remove(&oauth_state);
                Err(McpError::AuthFailed(
                    "OAuth callback timeout - authorization took too long".to_string(),
                ))
            }
        }
    }

    /// Cancel a pending authorization.
    pub async fn cancel_pending(&self, oauth_state: &str) {
        let mut state = self.state.write().await;
        if let Some(pending) = state.pending.remove(oauth_state) {
            let _ = pending
                .sender
                .send(Err("Authorization cancelled".to_string()));
        }
    }

    /// Check if the server is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Stop the OAuth callback server.
    pub async fn stop(&self) {
        let mut tx = self.shutdown_tx.lock().await;
        if let Some(sender) = tx.take() {
            let _ = sender.send(());
        }

        // Cancel all pending authorizations
        let mut state = self.state.write().await;
        for (_, pending) in state.pending.drain() {
            let _ = pending
                .sender
                .send(Err("OAuth callback server stopped".to_string()));
        }
    }
}

impl Default for OAuthCallbackServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle an incoming HTTP connection.
#[allow(clippy::cognitive_complexity)]
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    state: Arc<RwLock<CallbackServerState>>,
) -> McpResult<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buffer = [0u8; 4096];
    let n = stream
        .read(&mut buffer)
        .await
        .map_err(|e| McpError::protocol_error(format!("Failed to read request: {e}")))?;

    let request = String::from_utf8_lossy(&buffer[..n]);

    // Parse HTTP request line
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 {
        let response = http_response(400, "text/plain", "Bad Request");
        stream.write_all(response.as_bytes()).await.ok();
        return Ok(());
    }

    let path = parts[1];

    // Parse URL
    let url = format!("http://127.0.0.1{path}");
    let parsed = match url::Url::parse(&url) {
        Ok(u) => u,
        Err(_) => {
            let response = http_response(400, "text/plain", "Invalid URL");
            stream.write_all(response.as_bytes()).await.ok();
            return Ok(());
        }
    };

    // Check path
    if parsed.path() != OAUTH_CALLBACK_PATH {
        let response = http_response(404, "text/plain", "Not Found");
        stream.write_all(response.as_bytes()).await.ok();
        return Ok(());
    }

    // Parse query parameters
    let params: HashMap<String, String> = parsed
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let code = params.get("code");
    let oauth_state = params.get("state");
    let error = params.get("error");
    let error_description = params.get("error_description");

    debug!(
        has_code = code.is_some(),
        state = ?oauth_state,
        error = ?error,
        "Received OAuth callback"
    );

    // Validate state parameter
    let oauth_state = match oauth_state {
        Some(s) => s.clone(),
        None => {
            error!("OAuth callback missing state parameter");
            let html = html_error("Missing required state parameter - potential CSRF attack");
            let response = http_response(400, "text/html", &html);
            stream.write_all(response.as_bytes()).await.ok();
            return Ok(());
        }
    };

    // Handle error response
    if let Some(err) = error {
        let error_msg = error_description.cloned().unwrap_or_else(|| err.clone());

        let mut state = state.write().await;
        if let Some(pending) = state.pending.remove(&oauth_state) {
            let _ = pending.sender.send(Err(error_msg.clone()));
        }

        let html = html_error(&error_msg);
        let response = http_response(200, "text/html", &html);
        stream.write_all(response.as_bytes()).await.ok();
        return Ok(());
    }

    // Validate code
    let code = match code {
        Some(c) => c.clone(),
        None => {
            let html = html_error("No authorization code provided");
            let response = http_response(400, "text/html", &html);
            stream.write_all(response.as_bytes()).await.ok();
            return Ok(());
        }
    };

    // Check for pending auth
    let mut server_state = state.write().await;
    let pending = match server_state.pending.remove(&oauth_state) {
        Some(p) => p,
        None => {
            error!(state = %oauth_state, "OAuth callback with invalid state");
            let html = html_error("Invalid or expired state parameter - potential CSRF attack");
            let response = http_response(400, "text/html", &html);
            stream.write_all(response.as_bytes()).await.ok();
            return Ok(());
        }
    };

    // Send the code to the waiting task
    let _ = pending.sender.send(Ok(code));

    // Send success response
    let response = http_response(200, "text/html", HTML_SUCCESS);
    stream.write_all(response.as_bytes()).await.ok();

    Ok(())
}

/// Build an HTTP response.
fn http_response(status: u16, content_type: &str, body: &str) -> String {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Unknown",
    };

    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        status_text,
        content_type,
        body.len(),
        body
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_html_escape_single_quote() {
        assert_eq!(html_escape("it's"), "it&#39;s");
    }

    #[test]
    fn test_html_escape_combined() {
        assert_eq!(
            html_escape("<a href=\"test?a=1&b=2\">"),
            "&lt;a href=&quot;test?a=1&amp;b=2&quot;&gt;"
        );
    }

    #[test]
    fn test_html_escape_empty() {
        assert_eq!(html_escape(""), "");
    }

    #[test]
    fn test_html_escape_no_special_chars() {
        assert_eq!(html_escape("hello world"), "hello world");
    }

    #[test]
    fn test_html_error() {
        let html = html_error("Test error");
        assert!(html.contains("Test error"));
        assert!(html.contains("Authorization Failed"));
    }

    #[test]
    fn test_html_error_with_special_chars() {
        let html = html_error("<script>alert('xss')</script>");
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn test_html_success_content() {
        assert!(HTML_SUCCESS.contains("Authorization Successful"));
        assert!(HTML_SUCCESS.contains("window.close()"));
        assert!(HTML_SUCCESS.contains("<!DOCTYPE html>"));
    }

    #[tokio::test]
    async fn test_callback_server_creation() {
        let server = OAuthCallbackServer::new();
        assert!(!server.is_running().await);
    }

    #[tokio::test]
    async fn test_callback_server_default() {
        let server = OAuthCallbackServer::default();
        assert!(!server.is_running().await);
    }

    #[tokio::test]
    async fn test_callback_server_stop_when_not_running() {
        let server = OAuthCallbackServer::new();
        
        // Stop the server when not started should be safe
        server.stop().await;
        
        // Should not be running
        assert!(!server.is_running().await);
    }

    #[tokio::test]
    async fn test_callback_server_multiple_stops() {
        let server = OAuthCallbackServer::new();
        
        // Multiple stops should be safe
        server.stop().await;
        server.stop().await;
        
        assert!(!server.is_running().await);
    }

    #[tokio::test]
    async fn test_is_port_in_use() {
        // This test just checks the function runs without panicking
        let _ = OAuthCallbackServer::is_port_in_use().await;
    }
}
