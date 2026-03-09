//! Conversion between provider messages and session messages.
//!
//! This module provides bidirectional conversion between:
//! - `wonopcode_provider::Message` (used by Runner for AI context)
//! - `crate::message::Message` + `MessagePart` (used for persistence)
//!
//! This enables the Runner to persist conversation history to SessionRepository
//! while maintaining its internal ProviderMessage format for AI calls.

use crate::message::{
    AssistantMessage, AssistantTime, Message, MessagePart, ModelRef, PathContext, ReasoningPart,
    TextPart, TokenUsage, ToolPart, ToolState, ToolTime,
};
use wonopcode_provider::{ContentPart, Message as ProviderMessage, Role};
use wonopcode_util::Identifier;

/// Configuration for message conversion.
#[derive(Debug, Clone)]
pub struct ConversionContext {
    /// Session ID for the messages.
    pub session_id: String,
    /// Model ID (e.g., "claude-sonnet-4-5-20250929").
    pub model_id: String,
    /// Provider ID (e.g., "anthropic").
    pub provider_id: String,
    /// Current working directory.
    pub cwd: String,
    /// Project root directory.
    pub root: String,
    /// Agent name (e.g., "build", "plan").
    pub agent: String,
}

impl ConversionContext {
    /// Create a new conversion context.
    pub fn new(
        session_id: impl Into<String>,
        model_id: impl Into<String>,
        provider_id: impl Into<String>,
        cwd: impl Into<String>,
        root: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            model_id: model_id.into(),
            provider_id: provider_id.into(),
            cwd: cwd.into(),
            root: root.into(),
            agent: "build".to_string(),
        }
    }

    /// Set the agent name.
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = agent.into();
        self
    }
}

/// Result of converting a ProviderMessage to session format.
#[derive(Debug)]
pub struct ConvertedMessage {
    /// The converted Message (User or Assistant).
    pub message: Message,
    /// The message parts (text, tools, reasoning, etc.).
    pub parts: Vec<MessagePart>,
}

/// Convert a user ProviderMessage to session format.
///
/// User messages typically contain just text content.
pub fn convert_user_message(
    provider_msg: &ProviderMessage,
    ctx: &ConversionContext,
) -> ConvertedMessage {
    let user_msg = crate::message::UserMessage {
        id: Identifier::message(),
        session_id: ctx.session_id.clone(),
        time: crate::message::MessageTime::now(),
        summary: None,
        agent: ctx.agent.clone(),
        model: ModelRef {
            provider_id: ctx.provider_id.clone(),
            model_id: ctx.model_id.clone(),
        },
        system: None,
        tools: None,
    };

    let message_id = user_msg.id.clone();
    let mut parts = Vec::new();

    for content in &provider_msg.content {
        if let ContentPart::Text { text } = content {
            parts.push(MessagePart::Text(TextPart::new(
                &ctx.session_id,
                &message_id,
                text,
            )));
        }
        // User messages typically only have text, but handle other cases gracefully
    }

    ConvertedMessage {
        message: Message::User(user_msg),
        parts,
    }
}

/// Convert an assistant ProviderMessage to session format.
///
/// Assistant messages can contain text, tool calls, and reasoning/thinking.
pub fn convert_assistant_message(
    provider_msg: &ProviderMessage,
    ctx: &ConversionContext,
    parent_message_id: &str,
) -> ConvertedMessage {
    // Debug: Log incoming content parts
    let text_content_count = provider_msg.content.iter()
        .filter(|c| matches!(c, ContentPart::Text { .. }))
        .count();
    tracing::info!(
        content_parts = provider_msg.content.len(),
        text_content_parts = text_content_count,
        "📥 CONVERT_ASSISTANT_MESSAGE: Converting {} content parts ({} text)",
        provider_msg.content.len(),
        text_content_count
    );
    for (i, content) in provider_msg.content.iter().enumerate() {
        match content {
            ContentPart::Text { text } => {
                tracing::info!(
                    idx = i,
                    text_len = text.len(),
                    text_preview = %text.chars().take(50).collect::<String>(),
                    "📥 CONVERT_ASSISTANT_MESSAGE: ContentPart[{}] = Text({}...)",
                    i, text.chars().take(50).collect::<String>()
                );
            }
            ContentPart::ToolUse { id, name, .. } => {
                tracing::info!(idx = i, tool_id = %id, tool_name = %name, "📥 CONVERT_ASSISTANT_MESSAGE: ContentPart[{}] = ToolUse", i);
            }
            _ => {
                tracing::debug!(idx = i, "📥 CONVERT_ASSISTANT_MESSAGE: ContentPart[{}] = Other", i);
            }
        }
    }
    let assistant_msg = AssistantMessage {
        id: Identifier::message(),
        session_id: ctx.session_id.clone(),
        time: AssistantTime::started(),
        error: None,
        parent_id: parent_message_id.to_string(),
        model_id: ctx.model_id.clone(),
        provider_id: ctx.provider_id.clone(),
        agent: ctx.agent.clone(),
        path: PathContext {
            cwd: ctx.cwd.clone(),
            root: ctx.root.clone(),
        },
        summary: None,
        cost: 0.0,
        tokens: TokenUsage::default(),
        finish: None,
    };

    let message_id = assistant_msg.id.clone();
    let mut parts = Vec::new();
    let mut order: u32 = 0;

    for content in &provider_msg.content {
        match content {
            ContentPart::Text { text } => {
                let mut part = MessagePart::Text(TextPart::new(&ctx.session_id, &message_id, text));
                part.set_order(order);
                order += 1;
                parts.push(part);
            }
            ContentPart::Thinking { text } => {
                let mut part =
                    MessagePart::Reasoning(ReasoningPart::new(&ctx.session_id, &message_id, text));
                part.set_order(order);
                order += 1;
                parts.push(part);
            }
            ContentPart::ToolUse { id, name, input } => {
                let raw = serde_json::to_string(input).unwrap_or_default();
                let mut part = MessagePart::Tool(ToolPart::new(
                    &ctx.session_id,
                    &message_id,
                    id,
                    name,
                    input.clone(),
                    &raw,
                ));
                part.set_order(order);
                order += 1;
                parts.push(part);
            }
            ContentPart::ToolResult { .. } => {
                // Tool results are typically in separate Tool role messages
                // They update existing ToolPart entries rather than creating new parts
            }
            ContentPart::Image { .. } => {
                // Images not currently persisted to session storage
            }
        }
    }

    ConvertedMessage {
        message: Message::Assistant(assistant_msg),
        parts,
    }
}

/// Convert a ProviderMessage to session format based on its role.
///
/// For User and Assistant roles, creates the appropriate message and parts.
/// Tool role messages are handled separately via `update_tool_result`.
pub fn provider_to_session(
    provider_msg: &ProviderMessage,
    ctx: &ConversionContext,
    parent_message_id: Option<&str>,
) -> Option<ConvertedMessage> {
    match provider_msg.role {
        Role::User => Some(convert_user_message(provider_msg, ctx)),
        Role::Assistant => {
            let parent_id = parent_message_id.unwrap_or("");
            Some(convert_assistant_message(provider_msg, ctx, parent_id))
        }
        Role::Tool => {
            // Tool results update existing ToolPart entries
            // They don't create new Message entries
            None
        }
        Role::System => {
            // System messages are not persisted to conversation history
            None
        }
    }
}

/// Extract tool results from a Tool role ProviderMessage.
///
/// Returns a list of (tool_use_id, content, is_error) tuples.
pub fn extract_tool_results(provider_msg: &ProviderMessage) -> Vec<(String, String, bool)> {
    if provider_msg.role != Role::Tool {
        return Vec::new();
    }

    provider_msg
        .content
        .iter()
        .filter_map(|content| {
            if let ContentPart::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = content
            {
                Some((
                    tool_use_id.clone(),
                    content.clone(),
                    is_error.unwrap_or(false),
                ))
            } else {
                None
            }
        })
        .collect()
}

/// Reconstruct a user ProviderMessage from session Message and Parts.
fn reconstruct_user_message(parts: &[MessagePart]) -> ProviderMessage {
    let text: String = parts
        .iter()
        .filter_map(|p| match p {
            MessagePart::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    ProviderMessage::user(text)
}

/// Reconstruct an assistant ProviderMessage from session Message and Parts.
fn reconstruct_assistant_message(parts: &[MessagePart]) -> ProviderMessage {
    let mut content = Vec::new();

    for part in parts {
        match part {
            MessagePart::Text(t) => {
                content.push(ContentPart::Text {
                    text: t.text.clone(),
                });
            }
            MessagePart::Reasoning(r) => {
                content.push(ContentPart::Thinking {
                    text: r.text.clone(),
                });
            }
            MessagePart::Tool(t) => {
                let input = match &t.state {
                    ToolState::Pending { input, .. }
                    | ToolState::Running { input, .. }
                    | ToolState::Completed { input, .. }
                    | ToolState::Error { input, .. } => input.clone(),
                };
                content.push(ContentPart::ToolUse {
                    id: t.call_id.clone(),
                    name: t.tool.clone(),
                    input,
                });
            }
            // Other part types (File, Snapshot, etc.) don't map to ProviderMessage content
            _ => {}
        }
    }

    ProviderMessage {
        role: Role::Assistant,
        content,
    }
}

/// Reconstruct tool results as a Tool role ProviderMessage.
fn reconstruct_tool_results(parts: &[MessagePart]) -> Option<ProviderMessage> {
    let tool_results: Vec<ContentPart> = parts
        .iter()
        .filter_map(|part| {
            if let MessagePart::Tool(t) = part {
                match &t.state {
                    ToolState::Completed { output, .. } => Some(ContentPart::ToolResult {
                        tool_use_id: t.call_id.clone(),
                        content: output.clone(),
                        is_error: Some(false),
                    }),
                    ToolState::Error { error, .. } => Some(ContentPart::ToolResult {
                        tool_use_id: t.call_id.clone(),
                        content: error.clone(),
                        is_error: Some(true),
                    }),
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect();

    if tool_results.is_empty() {
        None
    } else {
        Some(ProviderMessage {
            role: Role::Tool,
            content: tool_results,
        })
    }
}

/// Convert session Message + Parts back to ProviderMessage(s).
///
/// Returns one or two ProviderMessages:
/// - For User messages: one user message
/// - For Assistant messages with completed tools: assistant message + tool results message
pub fn session_to_provider(message: &Message, parts: &[MessagePart]) -> Vec<ProviderMessage> {
    match message {
        Message::User(_) => {
            vec![reconstruct_user_message(parts)]
        }
        Message::Assistant(_) => {
            let mut messages = vec![reconstruct_assistant_message(parts)];

            // If there are completed tool calls, add tool results as a separate message
            if let Some(tool_results) = reconstruct_tool_results(parts) {
                messages.push(tool_results);
            }

            messages
        }
    }
}

/// Update a ToolPart with the result of tool execution.
///
/// This modifies the ToolState from Pending/Running to Completed/Error.
pub fn update_tool_state(
    tool_part: &mut ToolPart,
    output: String,
    success: bool,
    metadata: Option<serde_json::Value>,
) {
    let input = match &tool_part.state {
        ToolState::Pending { input, .. } => input.clone(),
        ToolState::Running { input, .. } => input.clone(),
        ToolState::Completed { input, .. } => input.clone(),
        ToolState::Error { input, .. } => input.clone(),
    };

    let mut time = ToolTime::started();
    time.end = Some(chrono::Utc::now().timestamp_millis());

    tool_part.state = if success {
        ToolState::Completed {
            input,
            output,
            title: tool_part.tool.clone(),
            metadata: metadata.unwrap_or(serde_json::Value::Null),
            time,
            attachments: None,
        }
    } else {
        ToolState::Error {
            input,
            error: output,
            metadata,
            time,
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> ConversionContext {
        ConversionContext::new(
            "ses_test123",
            "claude-sonnet-4-5-20250929",
            "anthropic",
            "/home/user/project",
            "/home/user/project",
        )
    }

    #[test]
    fn test_convert_user_message() {
        let ctx = test_context();
        let provider_msg = ProviderMessage::user("Hello, how are you?");

        let converted = convert_user_message(&provider_msg, &ctx);

        assert!(matches!(converted.message, Message::User(_)));
        assert_eq!(converted.parts.len(), 1);

        if let MessagePart::Text(text_part) = &converted.parts[0] {
            assert_eq!(text_part.text, "Hello, how are you?");
            assert_eq!(text_part.session_id, "ses_test123");
        } else {
            panic!("Expected TextPart");
        }
    }

    #[test]
    fn test_convert_assistant_message_text_only() {
        let ctx = test_context();
        let provider_msg = ProviderMessage::assistant("Here's my response.");

        let converted = convert_assistant_message(&provider_msg, &ctx, "msg_parent");

        assert!(matches!(converted.message, Message::Assistant(_)));
        assert_eq!(converted.parts.len(), 1);

        if let Message::Assistant(assistant) = &converted.message {
            assert_eq!(assistant.parent_id, "msg_parent");
            assert_eq!(assistant.model_id, "claude-sonnet-4-5-20250929");
        }
    }

    #[test]
    fn test_convert_assistant_message_with_tool() {
        let ctx = test_context();
        let mut provider_msg = ProviderMessage::assistant("Let me check that file.");
        provider_msg.content.push(ContentPart::ToolUse {
            id: "call_123".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({"path": "/etc/passwd"}),
        });

        let converted = convert_assistant_message(&provider_msg, &ctx, "msg_parent");

        assert_eq!(converted.parts.len(), 2);
        assert!(matches!(&converted.parts[0], MessagePart::Text(_)));
        assert!(matches!(&converted.parts[1], MessagePart::Tool(_)));

        if let MessagePart::Tool(tool_part) = &converted.parts[1] {
            assert_eq!(tool_part.call_id, "call_123");
            assert_eq!(tool_part.tool, "read");
        }
    }

    #[test]
    fn test_round_trip_user_message() {
        let ctx = test_context();
        let original = ProviderMessage::user("Test message");

        let converted = convert_user_message(&original, &ctx);
        let reconstructed = session_to_provider(&converted.message, &converted.parts);

        assert_eq!(reconstructed.len(), 1);
        assert_eq!(reconstructed[0].role, Role::User);
        assert_eq!(reconstructed[0].text(), "Test message");
    }

    #[test]
    fn test_round_trip_assistant_message() {
        let ctx = test_context();
        let original = ProviderMessage::assistant("Response text");

        let converted = convert_assistant_message(&original, &ctx, "parent");
        let reconstructed = session_to_provider(&converted.message, &converted.parts);

        assert_eq!(reconstructed.len(), 1);
        assert_eq!(reconstructed[0].role, Role::Assistant);
        assert_eq!(reconstructed[0].text(), "Response text");
    }

    #[test]
    fn test_extract_tool_results() {
        let mut msg = ProviderMessage {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult {
                tool_use_id: "call_123".to_string(),
                content: "File contents here".to_string(),
                is_error: Some(false),
            }],
        };

        let results = extract_tool_results(&msg);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "call_123");
        assert_eq!(results[0].1, "File contents here");
        assert!(!results[0].2);

        // Non-tool message returns empty
        msg.role = Role::User;
        let results = extract_tool_results(&msg);
        assert!(results.is_empty());
    }

    #[test]
    fn test_update_tool_state() {
        let mut tool_part = ToolPart::new(
            "ses_123",
            "msg_456",
            "call_789",
            "read",
            serde_json::json!({"path": "/test"}),
            r#"{"path": "/test"}"#,
        );

        // Initially pending
        assert!(matches!(tool_part.state, ToolState::Pending { .. }));

        // Update to completed
        update_tool_state(&mut tool_part, "File contents".to_string(), true, None);
        assert!(matches!(tool_part.state, ToolState::Completed { .. }));

        if let ToolState::Completed { output, .. } = &tool_part.state {
            assert_eq!(output, "File contents");
        }
    }
}
