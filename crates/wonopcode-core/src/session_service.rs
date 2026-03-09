//! Session service for managing conversation state.
//!
//! This service provides a high-level API for managing conversation history,
//! bridging between the Runner's execution model and persistent storage.
//!
//! Key responsibilities:
//! - Session lifecycle management (create, get, switch sessions)
//! - Message persistence (save user/assistant messages)
//! - History loading for Runner initialization
//! - History access for client queries
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐         ┌─────────────────┐
//! │    Runner    │◄───────►│ SessionService  │
//! │  (Execution) │         │ (State Mgmt)    │
//! └──────────────┘         └────────┬────────┘
//!                                   │
//!                                   ▼
//!                          ┌─────────────────┐
//!                          │SessionRepository│
//!                          │  (Persistence)  │
//!                          └─────────────────┘
//! ```

use crate::bus::Bus;
use crate::error::CoreResult;
use crate::message::MessagePart;
use crate::message_convert::{
    convert_assistant_message, convert_user_message, session_to_provider, update_tool_state,
    ConversionContext,
};
use crate::session::{MessageWithParts, Session, SessionRepository};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use wonopcode_provider::Message as ProviderMessage;
use wonopcode_storage::json::JsonStorage;

/// Service for managing session state and conversation history.
///
/// This service provides:
/// - Session lifecycle management
/// - Message persistence
/// - History loading for Runner initialization
/// - History access for client queries
///
/// The service is designed to be shared (via Arc) between the Runner
/// and other components that need access to conversation history.
#[derive(Clone)]
pub struct SessionService {
    /// The underlying repository for persistence.
    repo: SessionRepository,
    /// Current active session ID.
    current_session: Arc<RwLock<Option<String>>>,
    /// Project ID for this service.
    project_id: String,
    /// Conversion context for message format conversion.
    conversion_ctx: Arc<RwLock<ConversionContext>>,
}

impl SessionService {
    /// Create a new session service.
    pub fn new(
        storage: JsonStorage,
        bus: Bus,
        project_id: impl Into<String>,
        model_id: impl Into<String>,
        provider_id: impl Into<String>,
        cwd: impl Into<String>,
        root: impl Into<String>,
    ) -> Self {
        let project_id = project_id.into();
        let cwd_str = cwd.into();
        let root_str = root.into();

        let conversion_ctx = ConversionContext::new(
            "", // Will be set when session is created/loaded
            model_id,
            provider_id,
            &cwd_str,
            &root_str,
        );

        Self {
            repo: SessionRepository::new(storage, bus),
            current_session: Arc::new(RwLock::new(None)),
            project_id,
            conversion_ctx: Arc::new(RwLock::new(conversion_ctx)),
        }
    }

    /// Create a session service from an Instance.
    ///
    /// This is the preferred way to create a SessionService when you have
    /// an Instance available (e.g., in workstream creation).
    ///
    /// This method will automatically load the most recent session for this workstream's
    /// directory if one exists, ensuring that conversation history is available when
    /// switching between workstreams.
    ///
    /// IMPORTANT: Sessions are filtered by directory (workstream path), not just project_id.
    /// Different worktrees of the same git repository share the same project_id but have
    /// different directories. Each workstream should only see its own sessions.
    pub async fn from_instance(
        instance: &crate::Instance,
        model_id: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Self {
        let project_id = instance.project_id().await;
        let cwd = instance.directory().display().to_string();
        let worktree = instance.worktree().await;
        let root = worktree.display().to_string();

        let service = Self::new(
            instance.storage().clone(),
            instance.bus().clone(),
            project_id.clone(),
            model_id,
            provider_id,
            cwd.clone(),
            root,
        );

        // Try to load the most recent session for THIS WORKSTREAM (filtered by directory)
        // This ensures:
        // 1. Conversation history is available when switching workstreams
        // 2. Sessions from other worktrees (same project_id, different directory) are NOT loaded
        match service.repo.list(&project_id).await {
            Ok(sessions) => {
                debug!(
                    project_id = %project_id,
                    directory = %cwd,
                    session_count = sessions.len(),
                    "Found {} sessions for project, filtering by directory",
                    sessions.len()
                );
                
                // Log all sessions for debugging
                for s in &sessions {
                    debug!(
                        session_id = %s.id,
                        session_directory = %s.directory,
                        matches_cwd = (s.directory == cwd),
                        "Session {} has directory '{}' (matches: {})",
                        s.id, s.directory, s.directory == cwd
                    );
                }
                
                // Filter sessions to only those matching this workstream's directory
                // Sessions are sorted by ID ascending (with inverted ULIDs, smaller = newer)
                let matching_session = sessions.into_iter().find(|s| s.directory == cwd);

                if let Some(session) = matching_session {
                    info!(
                        project_id = %project_id,
                        directory = %cwd,
                        session_id = %session.id,
                        "Restored session {} for workstream",
                        session.id
                    );
                    {
                        let mut current = service.current_session.write().await;
                        *current = Some(session.id.clone());
                    }
                    {
                        let mut ctx = service.conversion_ctx.write().await;
                        ctx.session_id = session.id.clone();
                    }
                } else {
                    info!(
                        project_id = %project_id,
                        directory = %cwd,
                        "No matching session found for directory, will create new session on first message"
                    );
                }
            }
            Err(e) => {
                warn!(
                    project_id = %project_id,
                    directory = %cwd,
                    error = %e,
                    "Failed to list sessions, starting without history"
                );
            }
        }

        service
    }

    /// Get the underlying SessionRepository.
    ///
    /// Useful for advanced operations not covered by this service.
    pub fn repository(&self) -> &SessionRepository {
        &self.repo
    }

    /// Get the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    // ========================================================================
    // Session Management
    // ========================================================================

    /// Ensure a session exists, creating one if needed.
    ///
    /// Returns the current session, creating a new one if none is active.
    pub async fn ensure_session(&self) -> CoreResult<Session> {
        // Check if we already have an active session
        {
            let current = self.current_session.read().await;
            if let Some(ref session_id) = *current {
                if let Ok(session) = self.repo.get(&self.project_id, session_id).await {
                    return Ok(session);
                }
                // Session no longer exists, will create new one
            }
        }

        // Create new session
        let cwd = {
            let ctx = self.conversion_ctx.read().await;
            ctx.cwd.clone()
        };

        let session = Session::new(&self.project_id, &cwd);
        let session = self.repo.create(session).await?;

        // Update current session and conversion context
        {
            let mut current = self.current_session.write().await;
            *current = Some(session.id.clone());
        }
        {
            let mut ctx = self.conversion_ctx.write().await;
            ctx.session_id = session.id.clone();
        }

        info!(session_id = %session.id, "Created new session");
        Ok(session)
    }

    /// Get the current session ID, if any.
    pub async fn current_session_id(&self) -> Option<String> {
        self.current_session.read().await.clone()
    }

    /// Get the current session, if any.
    pub async fn current_session(&self) -> Option<Session> {
        let session_id = self.current_session_id().await?;
        self.repo.get(&self.project_id, &session_id).await.ok()
    }

    /// Set the current session by ID.
    ///
    /// Returns the session if it exists, or an error if not found.
    pub async fn set_session(&self, session_id: impl Into<String>) -> CoreResult<Session> {
        let session_id = session_id.into();
        let session = self.repo.get(&self.project_id, &session_id).await?;

        {
            let mut current = self.current_session.write().await;
            *current = Some(session_id.clone());
        }
        {
            let mut ctx = self.conversion_ctx.write().await;
            ctx.session_id = session_id;
        }

        info!(session_id = %session.id, "Switched to session");
        Ok(session)
    }

    /// Clear the current session and create a new empty one.
    ///
    /// This creates a new session immediately to ensure that when the app
    /// restarts, the new (empty) session is loaded instead of the old one.
    /// If session creation fails, we still clear the current session.
    pub async fn clear_session(&self) {
        let old_session_id = {
            let current = self.current_session.read().await;
            current.clone()
        };
        
        if let Some(ref session_id) = old_session_id {
            info!(session_id = %session_id, "Clearing current session");
        }

        // Create a new empty session immediately
        // This ensures the new session is the "most recent" one when the app restarts
        let cwd = {
            let ctx = self.conversion_ctx.read().await;
            ctx.cwd.clone()
        };

        match self.repo.create(Session::new(&self.project_id, &cwd)).await {
            Ok(new_session) => {
                info!(session_id = %new_session.id, "Created new empty session after clear");
                {
                    let mut current = self.current_session.write().await;
                    *current = Some(new_session.id.clone());
                }
                {
                    let mut ctx = self.conversion_ctx.write().await;
                    ctx.session_id = new_session.id;
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to create new session after clear, clearing anyway");
                let mut current = self.current_session.write().await;
                *current = None;
            }
        }
    }

    /// List all sessions for this project.
    pub async fn list_sessions(&self) -> CoreResult<Vec<Session>> {
        self.repo.list(&self.project_id).await
    }

    /// Update the model/provider in the conversion context.
    ///
    /// Called when the model changes during a session.
    pub async fn update_model(&self, model_id: impl Into<String>, provider_id: impl Into<String>) {
        let mut ctx = self.conversion_ctx.write().await;
        ctx.model_id = model_id.into();
        ctx.provider_id = provider_id.into();
    }

    /// Update the agent in the conversion context.
    pub async fn update_agent(&self, agent: impl Into<String>) {
        let mut ctx = self.conversion_ctx.write().await;
        ctx.agent = agent.into();
    }

    // ========================================================================
    // History Loading (for Runner initialization)
    // ========================================================================

    /// Load conversation history as ProviderMessages.
    ///
    /// Used by Runner to initialize its conversation context.
    /// Returns messages in chronological order (oldest first).
    pub async fn load_history(&self) -> CoreResult<Vec<ProviderMessage>> {
        let session_id = match self.current_session_id().await {
            Some(id) => id,
            None => {
                debug!("No active session, returning empty history");
                return Ok(Vec::new());
            }
        };

        let messages_with_parts = self
            .repo
            .messages(&self.project_id, &session_id, None)
            .await?;

        let mut provider_messages = Vec::new();
        for mwp in &messages_with_parts {
            let converted = session_to_provider(&mwp.message, &mwp.parts);
            provider_messages.extend(converted);
        }

        info!(
            session_id = %session_id,
            message_count = provider_messages.len(),
            "Loaded conversation history"
        );

        Ok(provider_messages)
    }

    /// Load recent history with a limit.
    ///
    /// Useful for loading just the most recent messages when full history
    /// would be too large.
    pub async fn load_recent_history(&self, limit: usize) -> CoreResult<Vec<ProviderMessage>> {
        let session_id = match self.current_session_id().await {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        let messages_with_parts = self
            .repo
            .messages(&self.project_id, &session_id, Some(limit))
            .await?;

        let mut provider_messages = Vec::new();
        for mwp in &messages_with_parts {
            let converted = session_to_provider(&mwp.message, &mwp.parts);
            provider_messages.extend(converted);
        }

        Ok(provider_messages)
    }

    // ========================================================================
    // Message Persistence (called by Runner)
    // ========================================================================

    /// Save a user message.
    ///
    /// Called when the user sends a prompt. Returns the message ID for
    /// linking the assistant's response.
    pub async fn save_user_message(&self, provider_msg: &ProviderMessage) -> CoreResult<String> {
        let session = self.ensure_session().await?;

        let ctx = self.conversion_ctx.read().await;
        let converted = convert_user_message(provider_msg, &ctx);

        let message_id = converted.message.id().to_string();

        self.repo.save_message(&converted.message).await?;
        for part in &converted.parts {
            self.repo.save_part(part).await?;
        }

        debug!(
            session_id = %session.id,
            message_id = %message_id,
            part_count = converted.parts.len(),
            "Saved user message"
        );

        Ok(message_id)
    }

    /// Save an assistant message.
    ///
    /// Called when the assistant's response is complete (or partially complete
    /// on cancellation). The parent_message_id links to the user's prompt.
    pub async fn save_assistant_message(
        &self,
        provider_msg: &ProviderMessage,
        parent_message_id: &str,
    ) -> CoreResult<String> {
        let session = self.ensure_session().await?;

        let ctx = self.conversion_ctx.read().await;
        let converted = convert_assistant_message(provider_msg, &ctx, parent_message_id);

        let message_id = converted.message.id().to_string();
        
        // Debug: Log content parts before conversion
        let text_part_count = converted.parts.iter()
            .filter(|p| matches!(p, crate::message::MessagePart::Text(_)))
            .count();
        info!(
            session_id = %session.id,
            message_id = %message_id,
            total_parts = converted.parts.len(),
            text_parts = text_part_count,
            "📝 SAVE_ASSISTANT_MESSAGE: About to save message"
        );
        
        // Debug: Log each text part
        for (i, part) in converted.parts.iter().enumerate() {
            if let crate::message::MessagePart::Text(t) = part {
                info!(
                    message_id = %message_id,
                    part_idx = i,
                    part_id = %t.id,
                    text_len = t.text.len(),
                    text_preview = %t.text.chars().take(50).collect::<String>(),
                    "📝 SAVE_ASSISTANT_MESSAGE: TextPart[{}] preview",
                    i
                );
            }
        }

        self.repo.save_message(&converted.message).await?;
        for part in &converted.parts {
            self.repo.save_part(part).await?;
        }

        debug!(
            session_id = %session.id,
            message_id = %message_id,
            parent_id = %parent_message_id,
            part_count = converted.parts.len(),
            "Saved assistant message"
        );

        Ok(message_id)
    }

    /// Update a tool's execution state.
    ///
    /// Called when a tool completes (successfully or with error).
    /// Finds the ToolPart by call_id and updates its state.
    pub async fn update_tool_result(
        &self,
        message_id: &str,
        call_id: &str,
        output: String,
        success: bool,
        metadata: Option<serde_json::Value>,
    ) -> CoreResult<()> {
        let session_id = self.current_session_id().await.ok_or_else(|| {
            crate::error::SessionError::NotFound {
                id: "no active session".to_string(),
            }
        })?;

        // Get all parts for the message
        let parts = self.repo.parts(&session_id, message_id).await?;

        // Find and update the tool part
        for part in parts {
            if let MessagePart::Tool(mut tool_part) = part {
                if tool_part.call_id == call_id {
                    update_tool_state(&mut tool_part, output.clone(), success, metadata.clone());
                    self.repo.save_part(&MessagePart::Tool(tool_part)).await?;

                    debug!(
                        message_id = %message_id,
                        call_id = %call_id,
                        success = success,
                        "Updated tool state"
                    );
                    return Ok(());
                }
            }
        }

        warn!(
            message_id = %message_id,
            call_id = %call_id,
            "Tool part not found for update"
        );

        Ok(())
    }

    /// Save a compaction event as a system message.
    ///
    /// Called when context compaction completes. Creates a special "system" message
    /// that represents the compaction event, allowing it to appear in the correct
    /// chronological order when history is reloaded.
    ///
    /// The parent_message_id should be the ID of the user message that triggered
    /// the compaction (i.e., the message that was being processed when compaction
    /// was needed).
    pub async fn save_compaction_event(
        &self,
        parent_message_id: &str,
        compaction_type: &str,
        messages_before: usize,
        messages_after: usize,
        tokens_before: u32,
        tokens_after: u32,
        summary: Option<String>,
    ) -> CoreResult<String> {
        let session = self.ensure_session().await?;
        let session_id = session.id.clone();

        // Create a synthetic assistant message to hold the compaction part
        let message_id = wonopcode_util::Identifier::message();
        let ctx = self.conversion_ctx.read().await;
        let message = crate::message::Message::Assistant(crate::message::AssistantMessage {
            id: message_id.clone(),
            session_id: session_id.clone(),
            time: crate::message::AssistantTime {
                created: chrono::Utc::now().timestamp_millis(),
                completed: Some(chrono::Utc::now().timestamp_millis()),
            },
            error: None,
            parent_id: parent_message_id.to_string(),
            model_id: ctx.model_id.clone(),
            provider_id: ctx.provider_id.clone(),
            agent: ctx.agent.clone(),
            path: crate::message::PathContext {
                cwd: ctx.cwd.clone(),
                root: ctx.root.clone(),
            },
            summary: None,
            cost: 0.0,
            tokens: crate::message::TokenUsage::default(),
            finish: Some("compaction".to_string()),
        });

        // Create the compaction part
        let compaction_part = MessagePart::Compaction(crate::message::CompactionPart {
            id: wonopcode_util::Identifier::part(),
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            original_message_id: String::new(),
            compaction_type: compaction_type.to_string(),
            messages_before,
            messages_after,
            tokens_before,
            tokens_after,
            summary,
        });

        // Save the message and part
        self.repo.save_message(&message).await?;
        self.repo.save_part(&compaction_part).await?;

        debug!(
            session_id = %session_id,
            message_id = %message_id,
            compaction_type = %compaction_type,
            messages_before = messages_before,
            messages_after = messages_after,
            "Saved compaction event"
        );

        Ok(message_id)
    }

    /// Replace all session messages with compacted messages.
    ///
    /// This is called after compaction to persist the new, reduced message history.
    /// It deletes all existing messages and saves the new compacted messages,
    /// along with a compaction marker message that will be rendered in the UI.
    ///
    /// The compacted messages typically consist of:
    /// 1. First user message (preserved)
    /// 2. Summary assistant message (AI-generated summary of compacted messages)
    /// 3. Recent messages (last N messages, preserved as-is)
    /// 4. Compaction marker message (added by this method)
    ///
    /// Returns the number of messages saved.
    pub async fn replace_with_compacted_messages(
        &self,
        compacted_messages: &[ProviderMessage],
        compaction_type: &str,
        messages_before: usize,
        tokens_before: u32,
        tokens_after: u32,
        summary: Option<String>,
    ) -> CoreResult<usize> {
        let session = self.ensure_session().await?;
        let session_id = session.id.clone();

        // First, get all existing message IDs
        let existing_messages = self.repo
            .messages(&self.project_id, &session_id, None)
            .await?;

        info!(
            session_id = %session_id,
            existing_count = existing_messages.len(),
            new_count = compacted_messages.len(),
            "Replacing session messages with compacted history"
        );

        // Delete all existing messages
        for msg_with_parts in &existing_messages {
            let msg_id = msg_with_parts.message.id();
            if let Err(e) = self.repo.delete_message(&session_id, msg_id).await {
                warn!(
                    message_id = %msg_id,
                    error = %e,
                    "Failed to delete message during compaction"
                );
            }
        }

        // Save the new compacted messages
        let mut saved_count = 0;
        let mut last_saved_id = String::new();
        let mut summary_message_id: Option<String> = None;

        for (idx, provider_msg) in compacted_messages.iter().enumerate() {
            match provider_msg.role {
                wonopcode_provider::Role::User => {
                    match self.save_user_message(provider_msg).await {
                        Ok(id) => {
                            last_saved_id = id;
                            saved_count += 1;
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to save user message during compaction");
                        }
                    }
                }
                wonopcode_provider::Role::Assistant => {
                    match self.save_assistant_message(provider_msg, &last_saved_id).await {
                        Ok(id) => {
                            // The second message (index 1) is the summary message
                            if idx == 1 {
                                summary_message_id = Some(id.clone());
                            }
                            last_saved_id = id;
                            saved_count += 1;
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to save assistant message during compaction");
                        }
                    }
                }
                // System and Tool messages are not persisted to conversation history
                wonopcode_provider::Role::System | wonopcode_provider::Role::Tool => {
                    debug!("Skipping {:?} message during compaction persistence", provider_msg.role);
                }
            }
        }

        // Save a compaction marker message right after the summary
        // This will be rendered as the CompactionNotice in the UI
        let parent_id = summary_message_id.unwrap_or_else(|| last_saved_id.clone());
        match self.save_compaction_event(
            &parent_id,
            compaction_type,
            messages_before,
            compacted_messages.len(),
            tokens_before,
            tokens_after,
            summary,
        ).await {
            Ok(_marker_id) => {
                saved_count += 1;
            }
            Err(e) => {
                warn!(error = %e, "Failed to save compaction marker message");
            }
        }

        info!(
            session_id = %session_id,
            deleted = existing_messages.len(),
            saved = saved_count,
            "Compaction persistence complete"
        );

        Ok(saved_count)
    }

    // ========================================================================
    // History Access (for client queries)
    // ========================================================================

    /// Get conversation history as MessageWithParts.
    ///
    /// Used by the workstream server to send history to connecting clients.
    /// Returns messages in chronological order.
    pub async fn get_history(&self) -> CoreResult<Vec<MessageWithParts>> {
        let session_id = match self.current_session_id().await {
            Some(id) => {
                info!(
                    project_id = %self.project_id,
                    session_id = %id,
                    "📜 Loading history for session"
                );
                id
            },
            None => {
                warn!(
                    project_id = %self.project_id,
                    "⚠️ No current session, returning empty history"
                );
                return Ok(Vec::new());
            }
        };
        
        let messages = self.repo
            .messages(&self.project_id, &session_id, None)
            .await?;
        
        info!(
            project_id = %self.project_id,
            session_id = %session_id,
            message_count = messages.len(),
            "📜 Loaded {} messages from session repository",
            messages.len()
        );
        
        // Log message details for debugging - DETAILED for duplication tracking
        for (i, msg) in messages.iter().enumerate() {
            let role = match &msg.message {
                crate::message::Message::User(_) => "user",
                crate::message::Message::Assistant(_) => "assistant",
            };
            let text_parts: Vec<_> = msg.parts.iter()
                .filter_map(|p| match p {
                    crate::message::MessagePart::Text(t) => Some((t.id.clone(), t.text.len())),
                    _ => None,
                })
                .collect();
            info!(
                idx = i,
                role = role,
                id = %msg.message.id(),
                total_parts = msg.parts.len(),
                text_parts = text_parts.len(),
                "📖 GET_HISTORY: Message[{}] {} has {} parts ({} text parts)",
                i, role, msg.parts.len(), text_parts.len()
            );
            // Log each text part for debugging
            for (j, (part_id, text_len)) in text_parts.iter().enumerate() {
                info!(
                    message_idx = i,
                    text_part_idx = j,
                    part_id = %part_id,
                    text_len = text_len,
                    "📖 GET_HISTORY: TextPart[{}] in message {}",
                    j, i
                );
            }
        }
        
        Ok(messages)
    }

    /// Get recent history with a limit.
    pub async fn get_recent_history(&self, limit: usize) -> CoreResult<Vec<MessageWithParts>> {
        let session_id = match self.current_session_id().await {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        self.repo
            .messages(&self.project_id, &session_id, Some(limit))
            .await
    }

    /// Get the message count for the current session.
    pub async fn message_count(&self) -> CoreResult<usize> {
        let messages = self.get_history().await?;
        Ok(messages.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use wonopcode_storage::json::JsonStorage;

    fn create_test_service() -> SessionService {
        let dir = tempfile::tempdir().unwrap();
        let storage = JsonStorage::new(dir.keep());
        let bus = Bus::new();

        SessionService::new(
            storage,
            bus,
            "test_project",
            "claude-sonnet-4-5-20250929",
            "anthropic",
            "/test/cwd",
            "/test/root",
        )
    }

    #[tokio::test]
    async fn test_ensure_session() {
        let service = create_test_service();

        // Initially no session
        assert!(service.current_session_id().await.is_none());

        // Ensure creates a session
        let session = service.ensure_session().await.unwrap();
        assert!(!session.id.is_empty());
        assert_eq!(session.project_id, "test_project");

        // Subsequent calls return the same session
        let session2 = service.ensure_session().await.unwrap();
        assert_eq!(session.id, session2.id);
    }

    #[tokio::test]
    async fn test_save_and_load_messages() {
        let service = create_test_service();

        // Save a user message
        let user_msg = ProviderMessage::user("Hello, how are you?");
        let user_id = service.save_user_message(&user_msg).await.unwrap();
        assert!(!user_id.is_empty());

        // Save an assistant message
        let assistant_msg = ProviderMessage::assistant("I'm doing well, thanks!");
        let assistant_id = service
            .save_assistant_message(&assistant_msg, &user_id)
            .await
            .unwrap();
        assert!(!assistant_id.is_empty());

        // Load history
        let history = service.load_history().await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].text(), "Hello, how are you?");
        assert_eq!(history[1].text(), "I'm doing well, thanks!");
    }

    #[tokio::test]
    async fn test_clear_session() {
        let service = create_test_service();

        // Create a session
        let session1 = service.ensure_session().await.unwrap();

        // Clear it - this now creates a new empty session immediately
        service.clear_session().await;
        
        // After clear, a new session is created
        let new_session_id = service.current_session_id().await;
        assert!(new_session_id.is_some());
        assert_ne!(new_session_id.as_ref().unwrap(), &session1.id);

        // Ensure returns the new session (doesn't create another)
        let session2 = service.ensure_session().await.unwrap();
        assert_eq!(new_session_id.unwrap(), session2.id);
    }

    #[tokio::test]
    async fn test_empty_history() {
        let service = create_test_service();

        // No session - empty history
        let history = service.load_history().await.unwrap();
        assert!(history.is_empty());

        // With session but no messages
        service.ensure_session().await.unwrap();
        let history = service.load_history().await.unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_get_history_for_clients() {
        let service = create_test_service();

        // Save some messages
        let user_msg = ProviderMessage::user("Test prompt");
        let user_id = service.save_user_message(&user_msg).await.unwrap();

        let assistant_msg = ProviderMessage::assistant("Test response");
        service
            .save_assistant_message(&assistant_msg, &user_id)
            .await
            .unwrap();

        // Get history as MessageWithParts
        let history = service.get_history().await.unwrap();
        assert_eq!(history.len(), 2);

        // First message should be user
        assert!(history[0].message.is_user());
        // Second should be assistant
        assert!(history[1].message.is_assistant());
    }
}
