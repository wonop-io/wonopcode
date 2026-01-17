//! Session revert functionality.
//!
//! Allows reverting a session to a previous message, undoing all changes
//! made after that point.

use crate::bus::{Bus, MessageRemoved, PartRemoved};
use crate::error::CoreResult;
use crate::message::MessagePart;
use crate::session::{RevertInfo, Session, SessionRepository};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Input for a revert operation.
#[derive(Debug, Clone)]
pub struct RevertInput {
    /// Session ID.
    pub session_id: String,
    /// Message ID to revert to.
    pub message_id: String,
    /// Optional part ID (to revert to a specific part within a message).
    pub part_id: Option<String>,
}

/// Session revert operations.
pub struct SessionRevert {
    session_repo: Arc<SessionRepository>,
    bus: Bus,
}

impl SessionRevert {
    /// Create a new session revert handler.
    pub fn new(session_repo: Arc<SessionRepository>, bus: Bus) -> Self {
        Self { session_repo, bus }
    }

    /// Revert a session to a specific message.
    ///
    /// This marks the revert point in the session. The actual message cleanup
    /// happens when the user continues (via `cleanup`).
    #[allow(clippy::cognitive_complexity)]
    pub async fn revert(&self, project_id: &str, input: RevertInput) -> CoreResult<Session> {
        info!(
            session_id = %input.session_id,
            message_id = %input.message_id,
            part_id = ?input.part_id,
            "Reverting session"
        );

        // Get all messages
        let messages = self
            .session_repo
            .messages(project_id, &input.session_id, None)
            .await?;

        // Find the revert point
        let mut revert_message_id = input.message_id.clone();
        let mut revert_part_id = input.part_id.clone();
        let mut found_revert_point = false;
        let mut last_user_message_id: Option<String> = None;

        for msg_with_parts in &messages {
            let msg = &msg_with_parts.message;

            // Track last user message (for reverting to user message before assistant)
            if msg.is_user() {
                last_user_message_id = Some(msg.id().to_string());
            }

            let mut remaining_useful_parts = Vec::new();

            for part in &msg_with_parts.parts {
                if found_revert_point {
                    continue;
                }

                // Check if this is the revert point
                let is_target_message = msg.id() == input.message_id;
                let is_target_part = input.part_id.as_ref().is_some_and(|pid| part.id() == pid);

                if (is_target_message && input.part_id.is_none()) || is_target_part {
                    // If no useful parts left, revert to the whole message
                    let has_useful_parts = remaining_useful_parts.iter().any(|p: &MessagePart| {
                        matches!(p, MessagePart::Text(_) | MessagePart::Tool(_))
                    });

                    if !has_useful_parts && input.part_id.is_some() {
                        // Revert to the message, not the part
                        revert_part_id = None;
                    }

                    // If reverting to an assistant message, actually revert to the user message before it
                    if !msg.is_user() && revert_part_id.is_none() {
                        if let Some(ref user_msg_id) = last_user_message_id {
                            revert_message_id = user_msg_id.clone();
                        }
                    }

                    found_revert_point = true;
                }

                remaining_useful_parts.push(part.clone());
            }
        }

        if !found_revert_point {
            warn!(
                session_id = %input.session_id,
                message_id = %input.message_id,
                "Revert point not found"
            );
            return self.session_repo.get(project_id, &input.session_id).await;
        }

        // Update session with revert info
        let session = self
            .session_repo
            .update(project_id, &input.session_id, |s| {
                s.revert = Some(RevertInfo {
                    message_id: revert_message_id,
                    part_id: revert_part_id,
                    snapshot: None, // Snapshot integration can be added later
                    diff: None,
                });
            })
            .await?;

        info!(
            session_id = %session.id,
            revert_to = ?session.revert,
            "Session reverted"
        );

        Ok(session)
    }

    /// Unrevert a session, clearing the revert state.
    ///
    /// This restores the session to its pre-revert state (messages are not deleted).
    pub async fn unrevert(&self, project_id: &str, session_id: &str) -> CoreResult<Session> {
        info!(session_id = %session_id, "Unreverting session");

        let session = self.session_repo.get(project_id, session_id).await?;

        if session.revert.is_none() {
            debug!(session_id = %session_id, "No revert to undo");
            return Ok(session);
        }

        // Clear revert info
        let session = self
            .session_repo
            .update(project_id, session_id, |s| {
                s.revert = None;
            })
            .await?;

        info!(session_id = %session_id, "Session unrevert complete");

        Ok(session)
    }

    /// Clean up after a revert when the user continues.
    ///
    /// This removes messages and parts after the revert point.
    #[allow(clippy::cognitive_complexity)]
    pub async fn cleanup(&self, project_id: &str, session_id: &str) -> CoreResult<()> {
        let session = self.session_repo.get(project_id, session_id).await?;

        let revert = match &session.revert {
            Some(r) => r.clone(),
            None => return Ok(()),
        };

        info!(
            session_id = %session_id,
            message_id = %revert.message_id,
            "Cleaning up after revert"
        );

        // Get all messages
        let messages = self
            .session_repo
            .messages(project_id, session_id, None)
            .await?;

        // Find messages to remove (after revert point)
        let mut found_revert_point = false;
        let mut messages_to_remove = Vec::new();
        let mut last_message_parts_to_remove: Vec<String> = Vec::new();
        let mut last_message_id: Option<String> = None;

        for msg_with_parts in &messages {
            let msg_id = msg_with_parts.message.id().to_string();

            if found_revert_point {
                messages_to_remove.push(msg_id.clone());
                continue;
            }

            if msg_id == revert.message_id {
                if let Some(ref part_id) = revert.part_id {
                    // Remove parts after the target part
                    let mut found_part = false;
                    for part in &msg_with_parts.parts {
                        if found_part {
                            last_message_parts_to_remove.push(part.id().to_string());
                        }
                        if part.id() == part_id {
                            found_part = true;
                        }
                    }
                    last_message_id = Some(msg_id.clone());
                }
                found_revert_point = true;
            }
        }

        // Remove parts from the last message (if reverting to a part)
        if let Some(ref msg_id) = last_message_id {
            for part_id in &last_message_parts_to_remove {
                if let Err(e) = self.session_repo.delete_part(msg_id, part_id).await {
                    warn!(message_id = %msg_id, part_id = %part_id, error = %e, "Failed to delete part");
                }
                self.bus
                    .publish(PartRemoved {
                        session_id: session_id.to_string(),
                        message_id: msg_id.clone(),
                        part_id: part_id.clone(),
                    })
                    .await;
            }
        }

        // Remove messages after revert point
        for msg_id in &messages_to_remove {
            if let Err(e) = self.session_repo.delete_message(session_id, msg_id).await {
                warn!(message_id = %msg_id, error = %e, "Failed to delete message");
            }
            self.bus
                .publish(MessageRemoved {
                    session_id: session_id.to_string(),
                    message_id: msg_id.clone(),
                })
                .await;
        }

        // Clear revert info
        self.session_repo
            .update(project_id, session_id, |s| {
                s.revert = None;
            })
            .await?;

        info!(
            session_id = %session_id,
            removed_messages = messages_to_remove.len(),
            removed_parts = last_message_parts_to_remove.len(),
            "Cleanup complete"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{AssistantMessage, Message, ModelRef, TextPart, UserMessage};
    use crate::session::{RevertInfo, Session};
    use wonopcode_storage::json::JsonStorage;

    fn create_test_storage() -> JsonStorage {
        let dir = tempfile::tempdir().unwrap();
        JsonStorage::new(dir.keep())
    }

    // ============================================================
    // RevertInput tests
    // ============================================================

    #[test]
    fn test_revert_input() {
        let input = RevertInput {
            session_id: "ses_123".to_string(),
            message_id: "msg_456".to_string(),
            part_id: None,
        };
        assert_eq!(input.session_id, "ses_123");
        assert_eq!(input.message_id, "msg_456");
        assert!(input.part_id.is_none());
    }

    #[test]
    fn test_revert_input_with_part() {
        let input = RevertInput {
            session_id: "ses_123".to_string(),
            message_id: "msg_456".to_string(),
            part_id: Some("part_789".to_string()),
        };
        assert_eq!(input.part_id, Some("part_789".to_string()));
    }

    #[test]
    fn test_revert_input_debug() {
        let input = RevertInput {
            session_id: "ses_123".to_string(),
            message_id: "msg_456".to_string(),
            part_id: None,
        };
        let debug_str = format!("{:?}", input);
        assert!(debug_str.contains("RevertInput"));
        assert!(debug_str.contains("ses_123"));
    }

    #[test]
    fn test_revert_input_clone() {
        let input = RevertInput {
            session_id: "ses_123".to_string(),
            message_id: "msg_456".to_string(),
            part_id: Some("part_789".to_string()),
        };
        let cloned = input.clone();
        assert_eq!(cloned.session_id, input.session_id);
        assert_eq!(cloned.message_id, input.message_id);
        assert_eq!(cloned.part_id, input.part_id);
    }

    // ============================================================
    // SessionRevert tests
    // ============================================================

    #[tokio::test]
    async fn test_session_revert_new() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));
        let revert = SessionRevert::new(session_repo, bus);
        // Just verify construction doesn't panic
        let _ = revert;
    }

    #[tokio::test]
    async fn test_revert_not_found() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session first
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        // Create a message
        let msg = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        session_repo.save_message(&msg).await.unwrap();

        let revert = SessionRevert::new(session_repo, bus);

        // Try to revert to a non-existent message
        let input = RevertInput {
            session_id: session.id.clone(),
            message_id: "nonexistent_msg".to_string(),
            part_id: None,
        };

        let result = revert.revert("proj_1", input).await.unwrap();
        // When revert point is not found, it returns the current session unchanged
        assert!(result.revert.is_none());
    }

    #[tokio::test]
    async fn test_revert_to_message() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        // Create messages
        let msg1 = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        session_repo.save_message(&msg1).await.unwrap();

        // Add a text part
        let part = MessagePart::Text(TextPart::new(&session.id, msg1.id(), "Hello"));
        session_repo.save_part(&part).await.unwrap();

        let msg2 = Message::Assistant(AssistantMessage::new(
            &session.id,
            msg1.id(),
            "default",
            "test",
            "model-1",
            "/path",
            "/path",
        ));
        session_repo.save_message(&msg2).await.unwrap();

        let revert_handler = SessionRevert::new(session_repo.clone(), bus);

        // Revert to msg1
        let input = RevertInput {
            session_id: session.id.clone(),
            message_id: msg1.id().to_string(),
            part_id: None,
        };

        let result = revert_handler.revert("proj_1", input).await.unwrap();
        assert!(result.revert.is_some());
        let revert_info = result.revert.unwrap();
        assert_eq!(revert_info.message_id, msg1.id());
    }

    #[tokio::test]
    async fn test_unrevert_no_revert() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session without revert
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        let revert_handler = SessionRevert::new(session_repo, bus);

        // Try to unrevert when there's no revert
        let result = revert_handler
            .unrevert("proj_1", &session.id)
            .await
            .unwrap();
        assert!(result.revert.is_none());
    }

    #[tokio::test]
    async fn test_unrevert_with_revert() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session with revert info
        let mut session = Session::new("proj_1", "/path");
        session.revert = Some(RevertInfo {
            message_id: "msg_123".to_string(),
            part_id: None,
            snapshot: None,
            diff: None,
        });
        let session = session_repo.create(session).await.unwrap();

        let revert_handler = SessionRevert::new(session_repo, bus);

        // Unrevert
        let result = revert_handler
            .unrevert("proj_1", &session.id)
            .await
            .unwrap();
        assert!(result.revert.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_no_revert() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session without revert
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        let revert_handler = SessionRevert::new(session_repo, bus);

        // Cleanup should do nothing when there's no revert
        let result = revert_handler.cleanup("proj_1", &session.id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_with_revert() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        // Create messages
        let msg1 = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        session_repo.save_message(&msg1).await.unwrap();

        let msg2 = Message::Assistant(AssistantMessage::new(
            &session.id,
            msg1.id(),
            "default",
            "test",
            "model-1",
            "/path",
            "/path",
        ));
        session_repo.save_message(&msg2).await.unwrap();

        let msg3 = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        session_repo.save_message(&msg3).await.unwrap();

        // Set revert to msg1
        session_repo
            .update("proj_1", &session.id, |s| {
                s.revert = Some(RevertInfo {
                    message_id: msg1.id().to_string(),
                    part_id: None,
                    snapshot: None,
                    diff: None,
                });
            })
            .await
            .unwrap();

        let revert_handler = SessionRevert::new(session_repo.clone(), bus);

        // Cleanup should remove messages after msg1
        let result = revert_handler.cleanup("proj_1", &session.id).await;
        assert!(result.is_ok());

        // Verify revert info is cleared
        let updated_session = session_repo.get("proj_1", &session.id).await.unwrap();
        assert!(updated_session.revert.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_with_part_revert() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        // Create a message with multiple parts
        let msg = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        session_repo.save_message(&msg).await.unwrap();

        let part1 = MessagePart::Text(TextPart::new(&session.id, msg.id(), "First"));
        session_repo.save_part(&part1).await.unwrap();

        let part2 = MessagePart::Text(TextPart::new(&session.id, msg.id(), "Second"));
        session_repo.save_part(&part2).await.unwrap();

        let part3 = MessagePart::Text(TextPart::new(&session.id, msg.id(), "Third"));
        session_repo.save_part(&part3).await.unwrap();

        // Set revert to part1 (should remove part2 and part3)
        session_repo
            .update("proj_1", &session.id, |s| {
                s.revert = Some(RevertInfo {
                    message_id: msg.id().to_string(),
                    part_id: Some(part1.id().to_string()),
                    snapshot: None,
                    diff: None,
                });
            })
            .await
            .unwrap();

        let revert_handler = SessionRevert::new(session_repo.clone(), bus);

        // Cleanup runs successfully
        let result = revert_handler.cleanup("proj_1", &session.id).await;
        assert!(result.is_ok());

        // Verify revert info is cleared
        let updated = session_repo.get("proj_1", &session.id).await.unwrap();
        assert!(updated.revert.is_none());
    }

    #[tokio::test]
    async fn test_revert_to_assistant_message() {
        let storage = create_test_storage();
        let bus = Bus::new();
        let session_repo = Arc::new(SessionRepository::new(storage, bus.clone()));

        // Create a session
        let session = Session::new("proj_1", "/path");
        let session = session_repo.create(session).await.unwrap();

        // Create user message first
        let user_msg = Message::User(UserMessage::new(
            &session.id,
            "default",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model-1".to_string(),
            },
        ));
        session_repo.save_message(&user_msg).await.unwrap();

        // Add a part so it's "useful"
        let part = MessagePart::Text(TextPart::new(&session.id, user_msg.id(), "User input"));
        session_repo.save_part(&part).await.unwrap();

        // Small delay to ensure different timestamps for message ordering
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Create assistant message with a part
        let assistant_msg = Message::Assistant(AssistantMessage::new(
            &session.id,
            user_msg.id(),
            "default",
            "test",
            "model-1",
            "/path",
            "/path",
        ));
        session_repo.save_message(&assistant_msg).await.unwrap();

        // Assistant needs a part too
        let assistant_part =
            MessagePart::Text(TextPart::new(&session.id, assistant_msg.id(), "Response"));
        session_repo.save_part(&assistant_part).await.unwrap();

        let revert_handler = SessionRevert::new(session_repo.clone(), bus);

        // Revert to assistant message
        let input = RevertInput {
            session_id: session.id.clone(),
            message_id: assistant_msg.id().to_string(),
            part_id: None,
        };

        let result = revert_handler.revert("proj_1", input).await.unwrap();
        // The revert function returns a session with revert info set if found
        // When reverting to an assistant message without a part_id, it reverts to the user message before it
        assert!(result.revert.is_some(), "revert info should be set");
        let revert_info = result.revert.unwrap();
        // Should be the user message (not the assistant message we targeted)
        assert_eq!(
            revert_info.message_id,
            user_msg.id(),
            "should revert to user message before assistant"
        );
    }
}
