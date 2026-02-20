//! Input request types for user interaction.
//!
//! These types represent requests for user input that are pushed to clients
//! and included in state snapshots. They integrate with the existing
//! `InputRequestQueue` on the server side.

use serde::{Deserialize, Serialize};

/// A request for user input.
///
/// This is sent from the server to the client when the agent needs
/// user input (permission, confirmation, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputRequest {
    /// Unique identifier for this request.
    pub id: String,

    /// The workstream this request belongs to.
    pub workstream_id: String,

    /// The session that created this request.
    pub session_id: String,

    /// When the request was created (Unix timestamp ms).
    pub created_at: u64,

    /// When the request expires (Unix timestamp ms), None = never.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,

    /// The type of input being requested.
    pub request_type: InputRequestType,
}

impl UserInputRequest {
    /// Check if this request has expired.
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

    /// Get the age of this request in milliseconds.
    pub fn age_ms(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now.saturating_sub(self.created_at)
    }
}

/// The type of input being requested.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum InputRequestType {
    /// Request permission for a tool execution.
    Permission {
        /// Tool name.
        tool_name: String,
        /// Tool input as JSON.
        tool_input: serde_json::Value,
        /// Human-readable description.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Path being accessed (if relevant).
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// Action being performed (e.g., "execute", "write", "delete").
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<String>,
    },

    /// Request selection from a list of options.
    Selection {
        /// Prompt text.
        prompt: String,
        /// Available options.
        options: Vec<SelectionOption>,
        /// Whether multiple selection is allowed.
        #[serde(default)]
        allow_multiple: bool,
    },

    /// Request free text input.
    FreeText {
        /// Prompt text.
        prompt: String,
        /// Placeholder text.
        #[serde(skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        /// Whether multiline input is allowed.
        #[serde(default)]
        multiline: bool,
    },

    /// Request a yes/no confirmation.
    Confirmation {
        /// Prompt text.
        prompt: String,
        /// Label for confirm button.
        #[serde(skip_serializing_if = "Option::is_none")]
        confirm_label: Option<String>,
        /// Label for cancel button.
        #[serde(skip_serializing_if = "Option::is_none")]
        cancel_label: Option<String>,
    },
}

/// An option for selection requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionOption {
    /// Option ID.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response to a user input request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputResponse {
    /// The request ID this responds to.
    pub request_id: String,
    /// The response.
    pub response: InputResponseType,
}

/// Response types for different input requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum InputResponseType {
    /// Response to a permission request.
    Permission {
        /// Whether approved.
        approved: bool,
        /// Remember this decision.
        #[serde(default)]
        remember: bool,
    },

    /// Response to a selection request.
    Selection {
        /// Selected option IDs.
        selected_ids: Vec<String>,
    },

    /// Response to a free text request.
    FreeText {
        /// The entered text.
        text: String,
    },

    /// Response to a confirmation request.
    Confirmation {
        /// Whether confirmed.
        confirmed: bool,
    },

    /// Request was cancelled.
    Cancelled {
        /// Reason for cancellation.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_permission_request_serialization() {
        let request = UserInputRequest {
            id: "req-123".to_string(),
            workstream_id: "feature/login".to_string(),
            session_id: "session-1".to_string(),
            created_at: now_millis(),
            expires_at: None,
            request_type: InputRequestType::Permission {
                tool_name: "bash".to_string(),
                tool_input: serde_json::json!({"command": "rm -rf /tmp/test"}),
                description: Some("Delete temporary files".to_string()),
                path: Some("/tmp/test".to_string()),
                action: Some("delete".to_string()),
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("permission"));
        assert!(json.contains("bash"));
        assert!(json.contains("/tmp/test"));

        let parsed: UserInputRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "req-123");
        if let InputRequestType::Permission { tool_name, .. } = parsed.request_type {
            assert_eq!(tool_name, "bash");
        } else {
            panic!("Wrong request type");
        }
    }

    #[test]
    fn test_selection_request_serialization() {
        let request = UserInputRequest {
            id: "req-456".to_string(),
            workstream_id: "main".to_string(),
            session_id: "session-1".to_string(),
            created_at: now_millis(),
            expires_at: None,
            request_type: InputRequestType::Selection {
                prompt: "Choose a branch".to_string(),
                options: vec![
                    SelectionOption {
                        id: "main".to_string(),
                        label: "main".to_string(),
                        description: Some("Main branch".to_string()),
                    },
                    SelectionOption {
                        id: "develop".to_string(),
                        label: "develop".to_string(),
                        description: None,
                    },
                ],
                allow_multiple: false,
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("selection"));
        assert!(json.contains("Choose a branch"));
        assert!(json.contains("Main branch"));
    }

    #[test]
    fn test_confirmation_request_serialization() {
        let request = UserInputRequest {
            id: "req-789".to_string(),
            workstream_id: "main".to_string(),
            session_id: "session-1".to_string(),
            created_at: now_millis(),
            expires_at: None,
            request_type: InputRequestType::Confirmation {
                prompt: "Are you sure you want to delete this file?".to_string(),
                confirm_label: Some("Delete".to_string()),
                cancel_label: Some("Keep".to_string()),
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("confirmation"));
        assert!(json.contains("Delete"));
        assert!(json.contains("Keep"));
    }

    #[test]
    fn test_input_response_serialization() {
        let permission = UserInputResponse {
            request_id: "req-123".to_string(),
            response: InputResponseType::Permission {
                approved: true,
                remember: true,
            },
        };
        let json = serde_json::to_string(&permission).unwrap();
        assert!(json.contains("permission"));
        assert!(json.contains("true"));

        let selection = UserInputResponse {
            request_id: "req-456".to_string(),
            response: InputResponseType::Selection {
                selected_ids: vec!["option-1".to_string(), "option-2".to_string()],
            },
        };
        let json = serde_json::to_string(&selection).unwrap();
        assert!(json.contains("selection"));
        assert!(json.contains("option-1"));

        let cancelled = UserInputResponse {
            request_id: "req-789".to_string(),
            response: InputResponseType::Cancelled {
                reason: "User timeout".to_string(),
            },
        };
        let json = serde_json::to_string(&cancelled).unwrap();
        assert!(json.contains("cancelled"));
        assert!(json.contains("User timeout"));
    }

    #[test]
    fn test_request_expiry() {
        // Non-expiring request
        let request = UserInputRequest {
            id: "req-1".to_string(),
            workstream_id: "main".to_string(),
            session_id: "session-1".to_string(),
            created_at: now_millis(),
            expires_at: None,
            request_type: InputRequestType::Confirmation {
                prompt: "Continue?".to_string(),
                confirm_label: None,
                cancel_label: None,
            },
        };
        assert!(!request.is_expired());

        // Already expired request
        let expired_request = UserInputRequest {
            id: "req-2".to_string(),
            workstream_id: "main".to_string(),
            session_id: "session-1".to_string(),
            created_at: now_millis() - 10000,
            expires_at: Some(now_millis() - 5000), // Expired 5 seconds ago
            request_type: InputRequestType::Confirmation {
                prompt: "Continue?".to_string(),
                confirm_label: None,
                cancel_label: None,
            },
        };
        assert!(expired_request.is_expired());
    }
}
