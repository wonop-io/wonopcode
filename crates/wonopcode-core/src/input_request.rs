//! User input request system for interactive prompts.
//!
//! This module provides a queue-based system for requesting user input from agents.
//! It supports various input types:
//! - Permission requests (tool approvals)
//! - Selection requests (choose from options)
//! - Free text input
//! - Confirmation dialogs
//!
//! The queue lives on the workstream server (source of truth) and clients poll for pending requests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, RwLock};
use uuid::Uuid;

/// Default TTL for input requests (never expire by default)
pub const DEFAULT_INPUT_REQUEST_TTL_SECS: Option<u64> = None;

/// Unique identifier for an input request
pub type InputRequestId = String;

/// The type of input being requested
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum InputRequestType {
    /// Request permission for a tool execution
    Permission {
        tool_name: String,
        tool_input: serde_json::Value,
        description: Option<String>,
    },
    /// Request selection from a list of options
    Selection {
        prompt: String,
        options: Vec<SelectionOption>,
        allow_multiple: bool,
    },
    /// Request free text input
    FreeText {
        prompt: String,
        placeholder: Option<String>,
        multiline: bool,
    },
    /// Request a yes/no confirmation
    Confirmation {
        prompt: String,
        confirm_label: Option<String>,
        cancel_label: Option<String>,
    },
}

/// An option for selection requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

/// A user input request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputRequest {
    /// Unique identifier for this request
    pub id: InputRequestId,
    /// The workstream this request belongs to
    pub workstream_id: String,
    /// The session that created this request
    pub session_id: String,
    /// When the request was created (Unix timestamp ms)
    pub created_at: u64,
    /// When the request expires (Unix timestamp ms), None = never
    pub expires_at: Option<u64>,
    /// The type of input being requested
    pub request_type: InputRequestType,
}

impl UserInputRequest {
    /// Create a new input request
    pub fn new(
        workstream_id: String,
        session_id: String,
        request_type: InputRequestType,
        ttl: Option<Duration>,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let expires_at = ttl.map(|d| now + d.as_millis() as u64);

        Self {
            id: Uuid::new_v4().to_string(),
            workstream_id,
            session_id,
            created_at: now,
            expires_at,
            request_type,
        }
    }

    /// Check if this request has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            now > expires_at
        } else {
            false
        }
    }
}

/// The response type for different input requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum InputResponseType {
    /// Response to a permission request
    Permission {
        approved: bool,
        /// If true, remember this decision for future requests of the same type
        #[serde(default)]
        remember: bool,
    },
    /// Response to a selection request
    Selection { selected_ids: Vec<String> },
    /// Response to a free text request
    FreeText { text: String },
    /// Response to a confirmation request
    Confirmation { confirmed: bool },
    /// Request was cancelled (timeout, user dismissed, etc.)
    Cancelled { reason: String },
}

/// A response to a user input request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputResponse {
    /// The request ID this responds to
    pub request_id: InputRequestId,
    /// The response
    pub response: InputResponseType,
}

/// Internal state for a pending request
struct PendingRequest {
    request: UserInputRequest,
    response_tx: Option<oneshot::Sender<UserInputResponse>>,
    created_instant: Instant,
}

/// Queue for managing user input requests on a workstream server
///
/// This is the source of truth for pending input requests.
/// Clients poll this queue to discover requests and submit responses.
pub struct InputRequestQueue {
    /// Pending requests keyed by request ID
    pending: RwLock<HashMap<InputRequestId, PendingRequest>>,
}

impl InputRequestQueue {
    /// Create a new empty queue
    pub fn new() -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
        }
    }

    /// Add a new request to the queue
    ///
    /// Returns a receiver that will get the response when the user responds.
    pub async fn add_request(
        &self,
        request: UserInputRequest,
    ) -> oneshot::Receiver<UserInputResponse> {
        let (tx, rx) = oneshot::channel();
        let id = request.id.clone();

        let pending_request = PendingRequest {
            request,
            response_tx: Some(tx),
            created_instant: Instant::now(),
        };

        let mut pending = self.pending.write().await;
        pending.insert(id, pending_request);

        rx
    }

    /// Cancel a request (e.g., on timeout from agent side)
    ///
    /// Returns true if the request was found and cancelled.
    pub async fn cancel_request(&self, request_id: &str, reason: &str) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(mut pending_request) = pending.remove(request_id) {
            if let Some(tx) = pending_request.response_tx.take() {
                let _ = tx.send(UserInputResponse {
                    request_id: request_id.to_string(),
                    response: InputResponseType::Cancelled {
                        reason: reason.to_string(),
                    },
                });
            }
            true
        } else {
            false
        }
    }

    /// Submit a response to a pending request
    ///
    /// Returns true if the request was found and response delivered.
    pub async fn respond(&self, response: UserInputResponse) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(mut pending_request) = pending.remove(&response.request_id) {
            if let Some(tx) = pending_request.response_tx.take() {
                let _ = tx.send(response);
                return true;
            }
        }
        false
    }

    /// List all pending requests (for client polling)
    ///
    /// This also cleans up expired requests.
    pub async fn list_pending(&self) -> Vec<UserInputRequest> {
        let mut pending = self.pending.write().await;

        // Collect expired request IDs
        let expired_ids: Vec<String> = pending
            .iter()
            .filter(|(_, pr)| pr.request.is_expired())
            .map(|(id, _)| id.clone())
            .collect();

        // Cancel expired requests
        for id in expired_ids {
            if let Some(mut pr) = pending.remove(&id) {
                if let Some(tx) = pr.response_tx.take() {
                    let _ = tx.send(UserInputResponse {
                        request_id: id,
                        response: InputResponseType::Cancelled {
                            reason: "Request expired".to_string(),
                        },
                    });
                }
            }
        }

        // Return remaining pending requests in FIFO order
        let mut requests: Vec<_> = pending
            .values()
            .map(|pr| (pr.created_instant, pr.request.clone()))
            .collect();
        requests.sort_by_key(|(instant, _)| *instant);
        requests.into_iter().map(|(_, req)| req).collect()
    }

    /// Get a specific pending request by ID
    pub async fn get_request(&self, request_id: &str) -> Option<UserInputRequest> {
        let pending = self.pending.read().await;
        pending.get(request_id).map(|pr| pr.request.clone())
    }

    /// Get the count of pending requests
    pub async fn pending_count(&self) -> usize {
        let pending = self.pending.read().await;
        pending.len()
    }
}

impl Default for InputRequestQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe handle to an input request queue
pub type SharedInputRequestQueue = Arc<InputRequestQueue>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add_and_respond() {
        let queue = InputRequestQueue::new();

        let request = UserInputRequest::new(
            "workstream-1".to_string(),
            "session-1".to_string(),
            InputRequestType::Confirmation {
                prompt: "Continue?".to_string(),
                confirm_label: None,
                cancel_label: None,
            },
            None,
        );
        let request_id = request.id.clone();

        let rx = queue.add_request(request).await;

        // Check it's in the pending list
        let pending = queue.list_pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, request_id);

        // Respond to the request
        let responded = queue
            .respond(UserInputResponse {
                request_id: request_id.clone(),
                response: InputResponseType::Confirmation { confirmed: true },
            })
            .await;
        assert!(responded);

        // Check response received
        let response = rx.await.unwrap();
        assert_eq!(response.request_id, request_id);
        assert_eq!(
            response.response,
            InputResponseType::Confirmation { confirmed: true }
        );

        // Check it's removed from pending
        let pending = queue.list_pending().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_cancel_request() {
        let queue = InputRequestQueue::new();

        let request = UserInputRequest::new(
            "workstream-1".to_string(),
            "session-1".to_string(),
            InputRequestType::Permission {
                tool_name: "bash".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                description: None,
            },
            None,
        );
        let request_id = request.id.clone();

        let rx = queue.add_request(request).await;

        // Cancel the request
        let cancelled = queue.cancel_request(&request_id, "User timeout").await;
        assert!(cancelled);

        // Check response is cancelled
        let response = rx.await.unwrap();
        assert!(matches!(
            response.response,
            InputResponseType::Cancelled { .. }
        ));

        // Check it's removed from pending
        let pending = queue.list_pending().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_expired_request_cleanup() {
        let queue = InputRequestQueue::new();

        // Create a request that expires immediately
        let request = UserInputRequest::new(
            "workstream-1".to_string(),
            "session-1".to_string(),
            InputRequestType::FreeText {
                prompt: "Enter name".to_string(),
                placeholder: None,
                multiline: false,
            },
            Some(Duration::from_millis(1)), // Expires in 1ms
        );

        let _rx = queue.add_request(request).await;

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(10)).await;

        // List pending should clean up expired requests
        let pending = queue.list_pending().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_fifo_ordering() {
        let queue = InputRequestQueue::new();

        // Add multiple requests
        let req1 = UserInputRequest::new(
            "ws".to_string(),
            "s".to_string(),
            InputRequestType::Confirmation {
                prompt: "First".to_string(),
                confirm_label: None,
                cancel_label: None,
            },
            None,
        );
        let id1 = req1.id.clone();
        let _rx1 = queue.add_request(req1).await;

        // Small delay to ensure ordering
        tokio::time::sleep(Duration::from_millis(1)).await;

        let req2 = UserInputRequest::new(
            "ws".to_string(),
            "s".to_string(),
            InputRequestType::Confirmation {
                prompt: "Second".to_string(),
                confirm_label: None,
                cancel_label: None,
            },
            None,
        );
        let id2 = req2.id.clone();
        let _rx2 = queue.add_request(req2).await;

        let pending = queue.list_pending().await;
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, id1);
        assert_eq!(pending[1].id, id2);
    }
}
