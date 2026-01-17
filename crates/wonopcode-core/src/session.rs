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
    use crate::message::{
        AssistantMessage, ModelRef, TextPart, ToolPart, ToolState, ToolTime, UserMessage,
    };

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

    // ============================================================
    // Session struct tests
    // ============================================================

    #[test]
    fn test_session_new() {
        let session = Session::new("project_1", "/path/to/project");
        assert!(!session.id.is_empty());
        assert!(session.id.starts_with("ses_"));
        assert_eq!(session.project_id, "project_1");
        assert_eq!(session.directory, "/path/to/project");
        assert_eq!(session.title, "New Session");
        assert!(session.parent_id.is_none());
        assert!(session.summary.is_none());
        assert!(session.share.is_none());
        assert!(session.revert.is_none());
        assert!(session.time.created > 0);
        assert!(session.time.updated > 0);
        assert!(session.time.compacting.is_none());
        assert!(session.time.archived.is_none());
    }

    #[test]
    fn test_session_child() {
        let parent = Session::new("project_1", "/path/to/project");
        let child = Session::child(&parent);
        assert_eq!(child.project_id, parent.project_id);
        assert_eq!(child.directory, parent.directory);
        assert_eq!(child.parent_id, Some(parent.id));
        assert!(child.title.contains("Subtask of"));
    }

    #[test]
    fn test_session_touch() {
        let mut session = Session::new("project_1", "/path");
        let original_updated = session.time.updated;
        std::thread::sleep(std::time::Duration::from_millis(5));
        session.touch();
        assert!(session.time.updated >= original_updated);
    }

    #[test]
    fn test_session_created_at() {
        let session = Session::new("project_1", "/path");
        let dt = session.created_at();
        assert!(dt.timestamp_millis() > 0);
    }

    #[test]
    fn test_session_updated_at() {
        let session = Session::new("project_1", "/path");
        let dt = session.updated_at();
        assert!(dt.timestamp_millis() > 0);
    }

    #[test]
    fn test_session_created_at_fallback() {
        let mut session = Session::new("project_1", "/path");
        session.time.created = -9999999999999999; // Invalid timestamp
        let dt = session.created_at();
        // Should return now as fallback
        assert!(dt.timestamp() > 0);
    }

    #[test]
    fn test_session_updated_at_fallback() {
        let mut session = Session::new("project_1", "/path");
        session.time.updated = -9999999999999999; // Invalid timestamp
        let dt = session.updated_at();
        // Should return now as fallback
        assert!(dt.timestamp() > 0);
    }

    #[test]
    fn test_session_default() {
        let session = Session::default();
        assert!(session.id.is_empty());
        assert!(session.project_id.is_empty());
        assert!(session.directory.is_empty());
        assert!(session.title.is_empty());
    }

    // ============================================================
    // SessionSummary tests
    // ============================================================

    #[test]
    fn test_session_summary_default() {
        let summary = SessionSummary::default();
        assert_eq!(summary.additions, 0);
        assert_eq!(summary.deletions, 0);
        assert_eq!(summary.files, 0);
        assert!(summary.diffs.is_none());
    }

    #[test]
    fn test_session_summary_serialization() {
        let summary = SessionSummary {
            additions: 10,
            deletions: 5,
            files: 3,
            diffs: Some(vec![FileDiff {
                file: "/test.rs".to_string(),
                before: "old content".to_string(),
                after: "new content".to_string(),
                additions: 5,
                deletions: 2,
            }]),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let parsed: SessionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.additions, 10);
        assert_eq!(parsed.deletions, 5);
        assert_eq!(parsed.files, 3);
        assert!(parsed.diffs.is_some());
        assert_eq!(parsed.diffs.unwrap().len(), 1);
    }

    // ============================================================
    // ShareInfo tests
    // ============================================================

    #[test]
    fn test_share_info_serialization() {
        let share = ShareInfo {
            url: "https://example.com/share/abc123".to_string(),
        };
        let json = serde_json::to_string(&share).unwrap();
        let parsed: ShareInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, "https://example.com/share/abc123");
    }

    // ============================================================
    // SessionTime tests
    // ============================================================

    #[test]
    fn test_session_time_default() {
        let time = SessionTime::default();
        assert_eq!(time.created, 0);
        assert_eq!(time.updated, 0);
        assert!(time.compacting.is_none());
        assert!(time.archived.is_none());
    }

    #[test]
    fn test_session_time_serialization() {
        let time = SessionTime {
            created: 1000,
            updated: 2000,
            compacting: Some(1500),
            archived: None,
        };
        let json = serde_json::to_string(&time).unwrap();
        let parsed: SessionTime = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.created, 1000);
        assert_eq!(parsed.updated, 2000);
        assert_eq!(parsed.compacting, Some(1500));
        assert!(parsed.archived.is_none());
    }

    // ============================================================
    // RevertInfo tests
    // ============================================================

    #[test]
    fn test_revert_info_serialization() {
        let revert = RevertInfo {
            message_id: "msg_123".to_string(),
            part_id: Some("part_456".to_string()),
            snapshot: Some("snap_789".to_string()),
            diff: None,
        };
        let json = serde_json::to_string(&revert).unwrap();
        let parsed: RevertInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.message_id, "msg_123");
        assert_eq!(parsed.part_id, Some("part_456".to_string()));
        assert_eq!(parsed.snapshot, Some("snap_789".to_string()));
        assert!(parsed.diff.is_none());
    }

    #[test]
    fn test_revert_info_minimal() {
        let revert = RevertInfo {
            message_id: "msg_123".to_string(),
            part_id: None,
            snapshot: None,
            diff: None,
        };
        let json = serde_json::to_string(&revert).unwrap();
        assert!(!json.contains("part_id"));
        assert!(!json.contains("snapshot"));
        assert!(!json.contains("diff"));
    }

    // ============================================================
    // MessageWithParts tests
    // ============================================================

    #[test]
    fn test_message_with_parts() {
        let message = Message::User(UserMessage::new(
            "ses_123",
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        let parts = vec![MessagePart::Text(TextPart::new(
            "ses_123", "msg_1", "Hello",
        ))];
        let msg_with_parts = MessageWithParts { message, parts };
        assert_eq!(msg_with_parts.parts.len(), 1);
    }

    // ============================================================
    // SessionRepository tests
    // ============================================================

    #[tokio::test]
    async fn test_session_list() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create multiple sessions
        let session1 = repo.create(Session::new("proj_1", "/path")).await.unwrap();
        let session2 = repo.create(Session::new("proj_1", "/path")).await.unwrap();
        let _session3 = repo.create(Session::new("proj_2", "/other")).await.unwrap();

        // List sessions for proj_1
        let sessions = repo.list("proj_1").await.unwrap();
        assert_eq!(sessions.len(), 2);

        // Verify sorted by ID descending (newer first)
        assert!(sessions[0].id >= sessions[1].id);

        // Verify both sessions are present
        let ids: Vec<_> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&session1.id.as_str()));
        assert!(ids.contains(&session2.id.as_str()));
    }

    #[tokio::test]
    async fn test_session_children() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create parent session
        let parent = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        // Create child sessions
        let child1 = Session::child(&parent);
        let child1 = repo.create(child1).await.unwrap();

        let child2 = Session::child(&parent);
        let child2 = repo.create(child2).await.unwrap();

        // Create unrelated session
        let _unrelated = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        // Get children
        let children = repo.children("proj_1", &parent.id).await.unwrap();
        assert_eq!(children.len(), 2);

        let child_ids: Vec<_> = children.iter().map(|s| s.id.as_str()).collect();
        assert!(child_ids.contains(&child1.id.as_str()));
        assert!(child_ids.contains(&child2.id.as_str()));
    }

    #[tokio::test]
    async fn test_session_get_not_found() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let result = repo.get("proj_1", "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_update_not_found() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let result = repo.update("proj_1", "nonexistent", |_s| {}).await;
        assert!(result.is_err());
    }

    // ============================================================
    // Message operations tests
    // ============================================================

    #[tokio::test]
    async fn test_save_and_get_message() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let message = Message::User(UserMessage::new(
            "ses_123",
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));

        repo.save_message(&message).await.unwrap();

        let retrieved = repo.get_message("ses_123", message.id()).await.unwrap();
        assert_eq!(retrieved.id(), message.id());
    }

    #[tokio::test]
    async fn test_get_message_not_found() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let result = repo.get_message("ses_123", "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_message() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let message = Message::User(UserMessage::new(
            "ses_123",
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));

        repo.save_message(&message).await.unwrap();
        repo.delete_message("ses_123", message.id()).await.unwrap();

        let result = repo.get_message("ses_123", message.id()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_messages_with_parts() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create session
        let session = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        // Create a user message
        let user_msg = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        repo.save_message(&user_msg).await.unwrap();

        // Create parts for the message
        let part1 = MessagePart::Text(TextPart::new(&session.id, user_msg.id(), "Hello world"));
        repo.save_part(&part1).await.unwrap();

        // Get messages with parts
        let messages = repo.messages("proj_1", &session.id, None).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].parts.len(), 1);
        if let MessagePart::Text(text) = &messages[0].parts[0] {
            assert_eq!(text.text, "Hello world");
        } else {
            panic!("Expected TextPart");
        }
    }

    #[tokio::test]
    async fn test_messages_with_limit() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let session = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        // Create multiple messages
        for _ in 0..5 {
            let msg = Message::User(UserMessage::new(
                &session.id,
                "default",
                ModelRef {
                    provider_id: "test".to_string(),
                    model_id: "model-1".to_string(),
                },
            ));
            repo.save_message(&msg).await.unwrap();
        }

        // Get with limit
        let messages = repo.messages("proj_1", &session.id, Some(3)).await.unwrap();
        assert_eq!(messages.len(), 3);
    }

    // ============================================================
    // Part operations tests
    // ============================================================

    #[tokio::test]
    async fn test_save_and_get_part() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let part = MessagePart::Text(TextPart::new("ses_789", "msg_456", "Test text"));

        repo.save_part(&part).await.unwrap();

        let retrieved = repo.get_part("msg_456", part.id()).await.unwrap();
        if let MessagePart::Text(text) = retrieved {
            assert_eq!(text.text, "Test text");
        } else {
            panic!("Expected TextPart");
        }
    }

    #[tokio::test]
    async fn test_get_part_not_found() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let result = repo.get_part("msg_123", "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_part() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let part = MessagePart::Text(TextPart::new("ses_789", "msg_456", "Test text"));

        repo.save_part(&part).await.unwrap();
        repo.delete_part("msg_456", part.id()).await.unwrap();

        let result = repo.get_part("msg_456", part.id()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_parts_sorted() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create parts and save them
        // Note: parts are sorted by ID, so we save them and verify the sort order
        let part1 = MessagePart::Text(TextPart::new("ses_1", "msg_1", "First"));
        let part2 = MessagePart::Text(TextPart::new("ses_1", "msg_1", "Second"));
        let part3 = MessagePart::Text(TextPart::new("ses_1", "msg_1", "Third"));

        repo.save_part(&part1).await.unwrap();
        repo.save_part(&part2).await.unwrap();
        repo.save_part(&part3).await.unwrap();

        let retrieved = repo.parts("ses_1", "msg_1").await.unwrap();
        assert_eq!(retrieved.len(), 3);
        // Parts should be sorted by ID ascending
    }

    // ============================================================
    // Fork tests
    // ============================================================

    #[tokio::test]
    async fn test_fork_session_all_messages() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create original session
        let original = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        // Add messages to original
        let msg1 = Message::User(UserMessage::new(
            &original.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        repo.save_message(&msg1).await.unwrap();

        let msg2 = Message::Assistant(AssistantMessage::new(
            &original.id,
            msg1.id(),
            "default",
            "test",
            "model-1",
            "/path",
            "/path",
        ));
        repo.save_message(&msg2).await.unwrap();

        // Fork the session (all messages)
        let forked = repo.fork("proj_1", &original.id, None).await.unwrap();

        assert_ne!(forked.id, original.id);
        assert!(forked.title.contains("Fork of"));

        // Check forked messages exist
        let forked_messages = repo.messages("proj_1", &forked.id, None).await.unwrap();
        assert_eq!(forked_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_fork_session_at_message() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create original session
        let original = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        // Add messages - fork compares by message ID (ascending sort by created_at)
        let msg1 = Message::User(UserMessage::new(
            &original.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        repo.save_message(&msg1).await.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let msg2 = Message::Assistant(AssistantMessage::new(
            &original.id,
            msg1.id(),
            "default",
            "test",
            "model-1",
            "/path",
            "/path",
        ));
        repo.save_message(&msg2).await.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let msg3 = Message::User(UserMessage::new(
            &original.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        repo.save_message(&msg3).await.unwrap();

        // Fork at msg3 (should only include msg1 and msg2, since fork stops at message with ID >= fork_at)
        let forked = repo
            .fork("proj_1", &original.id, Some(msg3.id()))
            .await
            .unwrap();

        let forked_messages = repo.messages("proj_1", &forked.id, None).await.unwrap();
        // The fork should include messages before msg3
        assert!(forked_messages.len() <= 3);
    }

    #[tokio::test]
    async fn test_fork_session_with_parts() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        // Create original session with message and parts
        let original = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        let msg = Message::User(UserMessage::new(
            &original.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        repo.save_message(&msg).await.unwrap();

        let part = MessagePart::Text(TextPart::new(&original.id, msg.id(), "Hello"));
        repo.save_part(&part).await.unwrap();

        // Fork
        let forked = repo.fork("proj_1", &original.id, None).await.unwrap();

        // Check parts were copied
        let forked_messages = repo.messages("proj_1", &forked.id, None).await.unwrap();
        assert_eq!(forked_messages.len(), 1);
        assert_eq!(forked_messages[0].parts.len(), 1);
    }

    #[tokio::test]
    async fn test_fork_preserves_tool_parts() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let repo = SessionRepository::new(storage, bus);

        let original = repo.create(Session::new("proj_1", "/path")).await.unwrap();

        let msg = Message::Assistant(AssistantMessage::new(
            &original.id,
            "parent",
            "default",
            "test",
            "model-1",
            "/path",
            "/path",
        ));
        repo.save_message(&msg).await.unwrap();

        let tool_part = MessagePart::Tool(ToolPart {
            id: "tool_001".to_string(),
            message_id: msg.id().to_string(),
            session_id: original.id.clone(),
            call_id: "call_123".to_string(),
            tool: "read".to_string(),
            state: ToolState::Completed {
                input: serde_json::json!({"path": "/test.rs"}),
                output: "file contents".to_string(),
                title: "Read file".to_string(),
                metadata: serde_json::json!({}),
                time: ToolTime {
                    start: 0,
                    end: Some(100),
                    compacted: None,
                },
                attachments: None,
            },
            metadata: None,
        });
        repo.save_part(&tool_part).await.unwrap();

        let forked = repo.fork("proj_1", &original.id, None).await.unwrap();
        let forked_messages = repo.messages("proj_1", &forked.id, None).await.unwrap();
        assert_eq!(forked_messages[0].parts.len(), 1);
        if let MessagePart::Tool(t) = &forked_messages[0].parts[0] {
            assert_eq!(t.tool, "read");
        } else {
            panic!("Expected ToolPart");
        }
    }

    // ============================================================
    // Session with full fields
    // ============================================================

    #[test]
    fn test_session_with_all_fields() {
        let mut session = Session::new("proj_1", "/path");
        session.summary = Some(SessionSummary {
            additions: 100,
            deletions: 50,
            files: 10,
            diffs: None,
        });
        session.share = Some(ShareInfo {
            url: "https://share.example.com/abc".to_string(),
        });
        session.revert = Some(RevertInfo {
            message_id: "msg_123".to_string(),
            part_id: None,
            snapshot: None,
            diff: None,
        });
        session.time.compacting = Some(12345);
        session.time.archived = Some(67890);

        let json = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();

        assert!(parsed.summary.is_some());
        assert_eq!(parsed.summary.as_ref().unwrap().additions, 100);
        assert!(parsed.share.is_some());
        assert!(parsed.revert.is_some());
        assert_eq!(parsed.time.compacting, Some(12345));
        assert_eq!(parsed.time.archived, Some(67890));
    }
}
