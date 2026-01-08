//! Session management.
//!
//! A session represents a conversation with an AI assistant. Sessions
//! belong to projects and contain messages exchanged between the user
//! and assistant.

use crate::bus::{Bus, SessionCreated, SessionDeleted, SessionUpdated};
use crate::error::{CoreResult, SessionError};
use crate::message::{FileDiff, Message, MessagePart};
use serde::{Deserialize, Serialize};
use wonopcode_storage::json::JsonStorage;
use wonopcode_storage::Storage;
use wonopcode_util::Identifier;

/// Session information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    /// Session ID (descending - newer sorts first).
    #[serde(default)]
    pub id: String,

    /// Project ID.
    #[serde(default)]
    pub project_id: String,

    /// Working directory.
    #[serde(default)]
    pub directory: String,

    /// Parent session ID (for child sessions/subtasks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Session summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionSummary>,

    /// Share information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareInfo>,

    /// Session title.
    #[serde(default)]
    pub title: String,

    /// Application version.
    #[serde(default)]
    pub version: String,

    /// Timestamps.
    #[serde(default)]
    pub time: SessionTime,

    /// Revert information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert: Option<RevertInfo>,
}

/// Session summary (computed from diffs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSummary {
    pub additions: u32,
    pub deletions: u32,
    pub files: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<FileDiff>>,
}

/// Share information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareInfo {
    pub url: String,
}

/// Session timestamps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacting: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<i64>,
}

/// Revert information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertInfo {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// A message with its parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageWithParts {
    pub message: Message,
    pub parts: Vec<MessagePart>,
}

impl Session {
    /// Create a new session.
    pub fn new(project_id: impl Into<String>, directory: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id: Identifier::session(),
            project_id: project_id.into(),
            directory: directory.into(),
            parent_id: None,
            summary: None,
            share: None,
            title: "New Session".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            time: SessionTime {
                created: now,
                updated: now,
                compacting: None,
                archived: None,
            },
            revert: None,
        }
    }

    /// Create a child session.
    pub fn child(parent: &Session) -> Self {
        let mut session = Self::new(&parent.project_id, &parent.directory);
        session.parent_id = Some(parent.id.clone());
        session.title = format!("Subtask of {}", parent.title);
        session
    }

    /// Update the last modified time.
    pub fn touch(&mut self) {
        self.time.updated = chrono::Utc::now().timestamp_millis();
    }

    /// Get created_at as a DateTime.
    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.time.created).unwrap_or_else(chrono::Utc::now)
    }

    /// Get updated_at as a DateTime.
    pub fn updated_at(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.time.updated).unwrap_or_else(chrono::Utc::now)
    }
}

/// Session repository for CRUD operations.
pub struct SessionRepository {
    storage: JsonStorage,
    bus: Bus,
}

impl SessionRepository {
    /// Create a new session repository.
    pub fn new(storage: JsonStorage, bus: Bus) -> Self {
        Self { storage, bus }
    }

    /// Create a new session.
    pub async fn create(&self, mut session: Session) -> CoreResult<Session> {
        session.touch();

        // Save session
        let key = ["session", &session.project_id, &session.id];
        self.storage.write(&key, &session).await?;

        // Publish event
        self.bus
            .publish(SessionCreated {
                session_id: session.id.clone(),
                project_id: session.project_id.clone(),
                title: session.title.clone(),
            })
            .await;

        Ok(session)
    }

    /// Get a session by ID.
    pub async fn get(&self, project_id: &str, session_id: &str) -> CoreResult<Session> {
        let key = ["session", project_id, session_id];
        self.storage.read(&key).await?.ok_or_else(|| {
            SessionError::NotFound {
                id: session_id.to_string(),
            }
            .into()
        })
    }

    /// Update a session.
    pub async fn update<F>(&self, project_id: &str, session_id: &str, f: F) -> CoreResult<Session>
    where
        F: FnOnce(&mut Session) + Send,
    {
        let key = ["session", project_id, session_id];

        // Read current session
        let mut session: Session =
            self.storage
                .read(&key)
                .await?
                .ok_or_else(|| SessionError::NotFound {
                    id: session_id.to_string(),
                })?;

        // Apply update
        f(&mut session);
        session.touch();

        // Write back
        self.storage.write(&key, &session).await?;

        // Publish event
        self.bus
            .publish(SessionUpdated {
                session_id: session.id.clone(),
            })
            .await;

        Ok(session)
    }

    /// Delete a session.
    pub async fn delete(&self, project_id: &str, session_id: &str) -> CoreResult<()> {
        // Delete messages and parts first
        self.delete_all_messages(project_id, session_id).await?;

        // Delete session
        let key = ["session", project_id, session_id];
        self.storage.remove(&key).await?;

        // Publish event
        self.bus
            .publish(SessionDeleted {
                session_id: session_id.to_string(),
            })
            .await;

        Ok(())
    }

    /// List all sessions for a project.
    pub async fn list(&self, project_id: &str) -> CoreResult<Vec<Session>> {
        let prefix = ["session", project_id];
        let keys = self.storage.list(&prefix).await?;

        let mut sessions = Vec::new();
        for key in keys {
            let key_refs: Vec<&str> = key.iter().map(|s| s.as_str()).collect();
            if let Some(session) = self.storage.read::<Session>(&key_refs).await? {
                sessions.push(session);
            }
        }

        // Sort by ID (descending - newer first)
        sessions.sort_by(|a, b| b.id.cmp(&a.id));

        Ok(sessions)
    }

    /// List child sessions.
    pub async fn children(&self, project_id: &str, parent_id: &str) -> CoreResult<Vec<Session>> {
        let all = self.list(project_id).await?;
        Ok(all
            .into_iter()
            .filter(|s| s.parent_id.as_deref() == Some(parent_id))
            .collect())
    }

    /// Fork a session at a specific message.
    ///
    /// Creates a new session with all messages up to (but not including) the specified message.
    /// If message_id is None, copies all messages.
    pub async fn fork(
        &self,
        project_id: &str,
        session_id: &str,
        message_id: Option<&str>,
    ) -> CoreResult<Session> {
        let original = self.get(project_id, session_id).await?;

        // Create the forked session
        let mut forked = Session::new(&original.project_id, &original.directory);
        forked.title = format!("Fork of {}", original.title);
        let forked = self.create(forked).await?;

        // Get all messages from original session
        let messages = self.messages(project_id, session_id, None).await?;

        // Copy messages up to the specified message_id
        for msg_with_parts in messages {
            // Stop if we hit the fork point
            if let Some(fork_at) = message_id {
                if msg_with_parts.message.id() >= fork_at {
                    break;
                }
            }

            // Clone the message with a new ID and the forked session's ID
            let mut cloned_message = msg_with_parts.message.clone();
            let new_message_id = Identifier::message();

            match &mut cloned_message {
                Message::User(u) => {
                    u.id = new_message_id.clone();
                    u.session_id = forked.id.clone();
                }
                Message::Assistant(a) => {
                    a.id = new_message_id.clone();
                    a.session_id = forked.id.clone();
                }
            }

            self.save_message(&cloned_message).await?;

            // Clone and save all parts with new IDs
            for part in msg_with_parts.parts {
                let mut cloned_part = part.clone();
                let new_part_id = Identifier::part();

                match &mut cloned_part {
                    MessagePart::Text(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Reasoning(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Tool(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::File(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::StepStart(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::StepFinish(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Snapshot(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Patch(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Subtask(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Agent(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Retry(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                    MessagePart::Compaction(p) => {
                        p.id = new_part_id;
                        p.message_id = new_message_id.clone();
                        p.session_id = forked.id.clone();
                    }
                }

                self.save_part(&cloned_part).await?;
            }
        }

        Ok(forked)
    }

    // ========================================================================
    // Message Operations
    // ========================================================================

    /// Save a message.
    pub async fn save_message(&self, message: &Message) -> CoreResult<()> {
        let key = ["message", message.session_id(), message.id()];
        self.storage.write(&key, message).await?;
        Ok(())
    }

    /// Get a message.
    pub async fn get_message(&self, session_id: &str, message_id: &str) -> CoreResult<Message> {
        let key = ["message", session_id, message_id];
        self.storage.read(&key).await?.ok_or_else(|| {
            SessionError::MessageNotFound {
                id: message_id.to_string(),
            }
            .into()
        })
    }

    /// Delete a message and its parts.
    pub async fn delete_message(&self, session_id: &str, message_id: &str) -> CoreResult<()> {
        // Delete parts first
        self.delete_all_parts(session_id, message_id).await?;

        // Delete message
        let key = ["message", session_id, message_id];
        self.storage.remove(&key).await?;

        Ok(())
    }

    /// Get all messages for a session with their parts.
    pub async fn messages(
        &self,
        _project_id: &str,
        session_id: &str,
        limit: Option<usize>,
    ) -> CoreResult<Vec<MessageWithParts>> {
        let prefix = ["message", session_id];
        let keys = self.storage.list(&prefix).await?;

        let mut messages = Vec::new();
        for key in keys.into_iter().take(limit.unwrap_or(usize::MAX)) {
            let message_id = key.last().cloned().unwrap_or_default();
            let key_refs: Vec<&str> = key.iter().map(|s| s.as_str()).collect();
            if let Some(message) = self.storage.read::<Message>(&key_refs).await? {
                let parts = self.parts(session_id, &message_id).await?;
                messages.push(MessageWithParts { message, parts });
            }
        }

        // Sort by creation time (ascending - oldest first)
        messages.sort_by_key(|m| m.message.created_at());

        Ok(messages)
    }

    /// Delete all messages for a session.
    async fn delete_all_messages(&self, _project_id: &str, session_id: &str) -> CoreResult<()> {
        let prefix = ["message", session_id];
        let keys = self.storage.list(&prefix).await?;

        for key in keys {
            let message_id = key.last().cloned().unwrap_or_default();
            self.delete_message(session_id, &message_id).await?;
        }

        Ok(())
    }

    // ========================================================================
    // Part Operations
    // ========================================================================

    /// Save a message part.
    pub async fn save_part(&self, part: &MessagePart) -> CoreResult<()> {
        let key = ["part", part.message_id(), part.id()];
        self.storage.write(&key, part).await?;
        Ok(())
    }

    /// Get a message part.
    pub async fn get_part(&self, message_id: &str, part_id: &str) -> CoreResult<MessagePart> {
        let key = ["part", message_id, part_id];
        self.storage.read(&key).await?.ok_or_else(|| {
            SessionError::PartNotFound {
                id: part_id.to_string(),
            }
            .into()
        })
    }

    /// Delete a message part.
    pub async fn delete_part(&self, message_id: &str, part_id: &str) -> CoreResult<()> {
        let key = ["part", message_id, part_id];
        self.storage.remove(&key).await?;
        Ok(())
    }

    /// Get all parts for a message.
    pub async fn parts(&self, _session_id: &str, message_id: &str) -> CoreResult<Vec<MessagePart>> {
        let prefix = ["part", message_id];
        let keys = self.storage.list(&prefix).await?;

        let mut parts = Vec::new();
        for key in keys {
            let key_refs: Vec<&str> = key.iter().map(|s| s.as_str()).collect();
            if let Some(part) = self.storage.read::<MessagePart>(&key_refs).await? {
                parts.push(part);
            }
        }

        // Sort by ID (ascending - parts are created in order)
        parts.sort_by(|a, b| a.id().cmp(b.id()));

        Ok(parts)
    }

    /// Delete all parts for a message.
    async fn delete_all_parts(&self, _session_id: &str, message_id: &str) -> CoreResult<()> {
        let prefix = ["part", message_id];
        let keys = self.storage.list(&prefix).await?;

        for key in keys {
            let part_id = key.last().cloned().unwrap_or_default();
            self.delete_part(message_id, &part_id).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_storage() -> JsonStorage {
        let dir = tempfile::tempdir().unwrap();
        JsonStorage::new(dir.keep())
    }

    #[tokio::test]
    async fn test_session_crud() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create
        let session = Session::new("proj_123", "/home/user/project");
        let created = repo.create(session).await.unwrap();
        assert!(!created.id.is_empty());
        assert_eq!(created.project_id, "proj_123");

        // Read
        let read = repo.get(&created.project_id, &created.id).await.unwrap();
        assert_eq!(read.id, created.id);

        // Update
        let updated = repo
            .update(&created.project_id, &created.id, |s| {
                s.title = "Updated Title".to_string();
            })
            .await
            .unwrap();
        assert_eq!(updated.title, "Updated Title");

        // Delete
        repo.delete(&created.project_id, &created.id).await.unwrap();
        let result = repo.get(&created.project_id, &created.id).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_session_serialization() {
        let session = Session::new("proj_123", "/home/user/project");
        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.project_id, "proj_123");
    }
}
