//! ACP session management.
//!
//! Manages the mapping between ACP sessions and internal wonopcode sessions.

use crate::types::{AcpSessionState, JsonRpcError, McpServer, ModelRef};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

/// ACP session manager.
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, AcpSessionState>>>,
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session.
    pub async fn create(
        &self,
        session_id: String,
        cwd: String,
        mcp_servers: Vec<McpServer>,
        model: Option<ModelRef>,
    ) -> AcpSessionState {
        let state = AcpSessionState {
            id: session_id.clone(),
            cwd,
            mcp_servers,
            created_at: chrono::Utc::now(),
            model,
            mode_id: None,
        };

        info!("Creating ACP session: {}", session_id);

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, state.clone());

        state
    }

    /// Load an existing session.
    pub async fn load(
        &self,
        session_id: String,
        cwd: String,
        mcp_servers: Vec<McpServer>,
        model: Option<ModelRef>,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> AcpSessionState {
        let state = AcpSessionState {
            id: session_id.clone(),
            cwd,
            mcp_servers,
            created_at,
            model,
            mode_id: None,
        };

        info!("Loading ACP session: {}", session_id);

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, state.clone());

        state
    }

    /// Get a session by ID.
    pub async fn get(&self, session_id: &str) -> Result<AcpSessionState, JsonRpcError> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned().ok_or_else(|| {
            error!("Session not found: {}", session_id);
            JsonRpcError::invalid_params(format!("Session not found: {session_id}"))
        })
    }

    /// Get the model for a session.
    pub async fn get_model(&self, session_id: &str) -> Result<Option<ModelRef>, JsonRpcError> {
        let session = self.get(session_id).await?;
        Ok(session.model)
    }

    /// Set the model for a session.
    pub async fn set_model(
        &self,
        session_id: &str,
        model: ModelRef,
    ) -> Result<AcpSessionState, JsonRpcError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            JsonRpcError::invalid_params(format!("Session not found: {session_id}"))
        })?;

        session.model = Some(model);
        Ok(session.clone())
    }

    /// Set the mode (agent) for a session.
    pub async fn set_mode(
        &self,
        session_id: &str,
        mode_id: String,
    ) -> Result<AcpSessionState, JsonRpcError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            JsonRpcError::invalid_params(format!("Session not found: {session_id}"))
        })?;

        session.mode_id = Some(mode_id);
        Ok(session.clone())
    }

    /// Remove a session.
    pub async fn remove(&self, session_id: &str) -> Option<AcpSessionState> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id)
    }

    /// List all session IDs.
    pub async fn list(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_lifecycle() {
        let manager = SessionManager::new();

        // Create session
        let state = manager
            .create(
                "test-session".to_string(),
                "/tmp".to_string(),
                vec![],
                Some(ModelRef {
                    provider_id: "anthropic".to_string(),
                    model_id: "claude-3-5-sonnet".to_string(),
                }),
            )
            .await;

        assert_eq!(state.id, "test-session");
        assert_eq!(state.cwd, "/tmp");

        // Get session
        let retrieved = manager.get("test-session").await.unwrap();
        assert_eq!(retrieved.id, "test-session");

        // Set model
        let updated = manager
            .set_model(
                "test-session",
                ModelRef {
                    provider_id: "openai".to_string(),
                    model_id: "gpt-4".to_string(),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.model.unwrap().provider_id, "openai");

        // Set mode
        let updated = manager
            .set_mode("test-session", "default".to_string())
            .await
            .unwrap();

        assert_eq!(updated.mode_id, Some("default".to_string()));

        // Remove session
        let removed = manager.remove("test-session").await;
        assert!(removed.is_some());

        // Verify removed
        let result = manager.get("test-session").await;
        assert!(result.is_err());
    }
}
