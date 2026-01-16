//! Session sharing functionality.
//!
//! This module provides the ability to share sessions with others.
//! Sessions can be shared either:
//! - Locally via export/URL
//! - To a remote sharing service (if configured)

use crate::error::CoreResult;
use crate::session::SessionRepository;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};

/// Default sharing service URL (can be overridden via config)
pub const DEFAULT_SHARE_URL: &str = "https://api.wonopcode.com";

/// Share information stored with a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareInfo {
    /// Public URL where the session can be viewed
    pub url: String,

    /// Secret for updating/deleting the share
    pub secret: String,

    /// When the share was created
    pub created_at: i64,
}

/// Response from share creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareCreateResponse {
    /// Public URL where the session can be viewed
    pub url: String,

    /// Secret for managing the share
    pub secret: String,
}

/// Session sharing client
pub struct ShareClient {
    client: reqwest::Client,
    base_url: String,
}

impl ShareClient {
    /// Create a new share client
    pub fn new(base_url: Option<&str>) -> Self {
        let base_url = base_url.unwrap_or(DEFAULT_SHARE_URL).to_string();

        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("wonopcode/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url,
        }
    }

    /// Create a new share for a session
    pub async fn create(&self, session_id: &str) -> Result<ShareCreateResponse, ShareError> {
        let url = format!("{}/share_create", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "sessionID": session_id }))
            .send()
            .await
            .map_err(|e| ShareError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ShareError::Api {
                status,
                message: body,
            });
        }

        response
            .json()
            .await
            .map_err(|e| ShareError::Parse(e.to_string()))
    }

    /// Delete a share
    pub async fn delete(&self, session_id: &str, secret: &str) -> Result<(), ShareError> {
        let url = format!("{}/share_delete", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "sessionID": session_id,
                "secret": secret
            }))
            .send()
            .await
            .map_err(|e| ShareError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ShareError::Api {
                status,
                message: body,
            });
        }

        Ok(())
    }

    /// Sync session data to the sharing service
    pub async fn sync(
        &self,
        session_id: &str,
        secret: &str,
        key: &str,
        content: serde_json::Value,
    ) -> Result<(), ShareError> {
        let url = format!("{}/share_sync", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "sessionID": session_id,
                "secret": secret,
                "key": key,
                "content": content
            }))
            .send()
            .await
            .map_err(|e| ShareError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ShareError::Api {
                status,
                message: body,
            });
        }

        debug!("Synced {} to share", key);
        Ok(())
    }
}

impl Default for ShareClient {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Error type for share operations
#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Session not found")]
    SessionNotFound,

    #[error("Share not found")]
    ShareNotFound,
}

/// Share a session
pub async fn share_session(
    repo: &SessionRepository,
    project_id: &str,
    session_id: &str,
    share_url: Option<&str>,
) -> Result<ShareInfo, ShareError> {
    // Get the session (verify it exists)
    let _session = repo
        .get(project_id, session_id)
        .await
        .map_err(|_| ShareError::SessionNotFound)?;

    // Create share via API
    let client = ShareClient::new(share_url);
    let response = client.create(session_id).await?;

    let share_info = ShareInfo {
        url: response.url,
        secret: response.secret,
        created_at: chrono::Utc::now().timestamp_millis(),
    };

    // Update session with share info
    repo.update(project_id, session_id, |s| {
        s.share = Some(crate::session::ShareInfo {
            url: share_info.url.clone(),
        });
    })
    .await
    .map_err(|_| ShareError::SessionNotFound)?;

    info!("Shared session {} at {}", session_id, share_info.url);
    Ok(share_info)
}

/// Unshare a session
pub async fn unshare_session(
    repo: &SessionRepository,
    project_id: &str,
    session_id: &str,
    secret: &str,
    share_url: Option<&str>,
) -> Result<(), ShareError> {
    // Delete share via API
    let client = ShareClient::new(share_url);
    client.delete(session_id, secret).await?;

    // Update session to remove share info
    repo.update(project_id, session_id, |s| {
        s.share = None;
    })
    .await
    .map_err(|_| ShareError::SessionNotFound)?;

    info!("Unshared session {}", session_id);
    Ok(())
}

/// Export a session to a shareable JSON file
pub async fn export_session_to_file(
    repo: &SessionRepository,
    project_id: &str,
    session_id: &str,
    output_path: &Path,
) -> CoreResult<()> {
    let session = repo.get(project_id, session_id).await?;
    let messages = repo.messages(project_id, session_id, None).await?;

    let export_data = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "session": session,
        "messages": messages
    });

    let json = serde_json::to_string_pretty(&export_data)?;
    tokio::fs::write(output_path, json).await?;

    info!(
        "Exported session {} to {}",
        session_id,
        output_path.display()
    );
    Ok(())
}

/// Generate a shareable URL for local file sharing
pub fn generate_file_share_url(file_path: &Path) -> String {
    format!("file://{}", file_path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_share_info_serialization() {
        let info = ShareInfo {
            url: "https://share.wonopcode.com/abc123".to_string(),
            secret: "secret123".to_string(),
            created_at: 1234567890,
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: ShareInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.url, info.url);
        assert_eq!(parsed.secret, info.secret);
    }

    #[test]
    fn test_share_create_response_serialization() {
        let response = ShareCreateResponse {
            url: "https://share.wonopcode.com/xyz789".to_string(),
            secret: "secret456".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ShareCreateResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.url, response.url);
        assert_eq!(parsed.secret, response.secret);
    }

    #[test]
    fn test_share_client_new_default_url() {
        let client = ShareClient::new(None);
        assert_eq!(client.base_url, DEFAULT_SHARE_URL);
    }

    #[test]
    fn test_share_client_new_custom_url() {
        let client = ShareClient::new(Some("https://custom.example.com"));
        assert_eq!(client.base_url, "https://custom.example.com");
    }

    #[test]
    fn test_share_client_default() {
        let client = ShareClient::default();
        assert_eq!(client.base_url, DEFAULT_SHARE_URL);
    }

    #[test]
    fn test_share_error_display() {
        let network_err = ShareError::Network("connection failed".to_string());
        assert!(network_err.to_string().contains("Network error"));

        let api_err = ShareError::Api {
            status: 400,
            message: "Bad request".to_string(),
        };
        assert!(api_err.to_string().contains("400"));
        assert!(api_err.to_string().contains("Bad request"));

        let parse_err = ShareError::Parse("invalid json".to_string());
        assert!(parse_err.to_string().contains("Parse error"));

        let not_found = ShareError::SessionNotFound;
        assert!(not_found.to_string().contains("Session not found"));

        let share_not_found = ShareError::ShareNotFound;
        assert!(share_not_found.to_string().contains("Share not found"));
    }

    #[test]
    fn test_generate_file_share_url() {
        let url = generate_file_share_url(std::path::Path::new("/tmp/session.json"));
        assert_eq!(url, "file:///tmp/session.json");
    }

    #[test]
    fn test_generate_file_share_url_with_spaces() {
        let url = generate_file_share_url(std::path::Path::new("/tmp/my session.json"));
        assert!(url.starts_with("file://"));
        assert!(url.contains("my session.json"));
    }
}
