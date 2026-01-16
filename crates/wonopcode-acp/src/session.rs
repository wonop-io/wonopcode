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

    #[test]
    fn test_session_manager_new() {
        let manager = SessionManager::new();
        // Just verify construction doesn't panic
        let _ = manager;
    }

    #[test]
    fn test_session_manager_default() {
        let manager = SessionManager::default();
        // Just verify default construction doesn't panic
        let _ = manager;
    }

    #[tokio::test]
    async fn test_create_session() {
        let manager = SessionManager::new();

        let state = manager
            .create(
                "session_1".to_string(),
                "/home/user".to_string(),
                vec![],
                None,
            )
            .await;

        assert_eq!(state.id, "session_1");
        assert_eq!(state.cwd, "/home/user");
        assert!(state.mcp_servers.is_empty());
        assert!(state.model.is_none());
        assert!(state.mode_id.is_none());
    }

    #[tokio::test]
    async fn test_create_session_with_mcp_servers() {
        use crate::types::McpServerRemote;

        let manager = SessionManager::new();

        let mcp_server = McpServer::Remote(McpServerRemote {
            name: "test-server".to_string(),
            url: "http://localhost:8080".to_string(),
            headers: vec![],
            server_type: "remote".to_string(),
        });

        let state = manager
            .create(
                "session_1".to_string(),
                "/home/user".to_string(),
                vec![mcp_server],
                None,
            )
            .await;

        assert_eq!(state.mcp_servers.len(), 1);
        // Verify we have one server
        match &state.mcp_servers[0] {
            McpServer::Remote(remote) => assert_eq!(remote.url, "http://localhost:8080"),
            _ => panic!("Expected remote server"),
        }
    }

    #[tokio::test]
    async fn test_load_session() {
        let manager = SessionManager::new();
        let created_at = chrono::Utc::now() - chrono::Duration::hours(1);

        let state = manager
            .load(
                "loaded_session".to_string(),
                "/project".to_string(),
                vec![],
                Some(ModelRef {
                    provider_id: "openai".to_string(),
                    model_id: "gpt-4".to_string(),
                }),
                created_at,
            )
            .await;

        assert_eq!(state.id, "loaded_session");
        assert_eq!(state.cwd, "/project");
        assert_eq!(state.created_at, created_at);
        assert!(state.model.is_some());
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let manager = SessionManager::new();

        let result = manager.get("nonexistent").await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.message.contains("Session not found"));
    }

    #[tokio::test]
    async fn test_get_model() {
        let manager = SessionManager::new();

        manager
            .create(
                "session_1".to_string(),
                "/tmp".to_string(),
                vec![],
                Some(ModelRef {
                    provider_id: "anthropic".to_string(),
                    model_id: "claude-3-5-sonnet".to_string(),
                }),
            )
            .await;

        let model = manager.get_model("session_1").await.unwrap();
        assert!(model.is_some());
        assert_eq!(model.unwrap().provider_id, "anthropic");
    }

    #[tokio::test]
    async fn test_get_model_none() {
        let manager = SessionManager::new();

        manager
            .create("session_1".to_string(), "/tmp".to_string(), vec![], None)
            .await;

        let model = manager.get_model("session_1").await.unwrap();
        assert!(model.is_none());
    }

    #[tokio::test]
    async fn test_get_model_session_not_found() {
        let manager = SessionManager::new();

        let result = manager.get_model("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_model_session_not_found() {
        let manager = SessionManager::new();

        let result = manager
            .set_model(
                "nonexistent",
                ModelRef {
                    provider_id: "test".to_string(),
                    model_id: "model".to_string(),
                },
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_mode_session_not_found() {
        let manager = SessionManager::new();

        let result = manager
            .set_mode("nonexistent", "default".to_string())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let manager = SessionManager::new();

        let result = manager.remove("nonexistent").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let manager = SessionManager::new();

        // Initially empty
        let list = manager.list().await;
        assert!(list.is_empty());

        // Add sessions
        manager
            .create("session_1".to_string(), "/tmp".to_string(), vec![], None)
            .await;
        manager
            .create("session_2".to_string(), "/tmp".to_string(), vec![], None)
            .await;

        let list = manager.list().await;
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"session_1".to_string()));
        assert!(list.contains(&"session_2".to_string()));
    }

    #[tokio::test]
    async fn test_multiple_operations() {
        let manager = SessionManager::new();

        // Create multiple sessions
        manager
            .create("s1".to_string(), "/a".to_string(), vec![], None)
            .await;
        manager
            .create("s2".to_string(), "/b".to_string(), vec![], None)
            .await;
        manager
            .create("s3".to_string(), "/c".to_string(), vec![], None)
            .await;

        // Modify one
        manager
            .set_mode("s2", "explorer".to_string())
            .await
            .unwrap();

        // Remove one
        manager.remove("s1").await;

        // Verify state
        let list = manager.list().await;
        assert_eq!(list.len(), 2);
        assert!(!list.contains(&"s1".to_string()));

        let s2 = manager.get("s2").await.unwrap();
        assert_eq!(s2.mode_id, Some("explorer".to_string()));
    }
}
