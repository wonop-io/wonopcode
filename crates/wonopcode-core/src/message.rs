//! Message types for sessions.
//!
//! Messages are the fundamental unit of conversation. Each message belongs
//! to a session and contains one or more parts (text, tool calls, etc.).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wonopcode_util::Identifier;

/// A message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
}

impl Message {
    /// Get the message ID.
    pub fn id(&self) -> &str {
        match self {
            Message::User(m) => &m.id,
            Message::Assistant(m) => &m.id,
        }
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        match self {
            Message::User(m) => &m.session_id,
            Message::Assistant(m) => &m.session_id,
        }
    }

    /// Get the creation time.
    pub fn created_at(&self) -> i64 {
        match self {
            Message::User(m) => m.time.created,
            Message::Assistant(m) => m.time.created,
        }
    }

    /// Check if this is a user message.
    pub fn is_user(&self) -> bool {
        matches!(self, Message::User(_))
    }

    /// Check if this is an assistant message.
    pub fn is_assistant(&self) -> bool {
        matches!(self, Message::Assistant(_))
    }
}

/// A user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    /// Message ID.
    pub id: String,

    /// Session ID.
    pub session_id: String,

    /// Creation time.
    pub time: MessageTime,

    /// Summary of user's request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<UserSummary>,

    /// Agent handling this message.
    pub agent: String,

    /// Model used for response.
    pub model: ModelRef,

    /// System prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Tool availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, bool>>,
}

impl UserMessage {
    /// Create a new user message.
    pub fn new(session_id: impl Into<String>, agent: impl Into<String>, model: ModelRef) -> Self {
        Self {
            id: Identifier::message(),
            session_id: session_id.into(),
            time: MessageTime::now(),
            summary: None,
            agent: agent.into(),
            model,
            system: None,
            tools: None,
        }
    }
}

/// An assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// Message ID.
    pub id: String,

    /// Session ID.
    pub session_id: String,

    /// Creation time.
    pub time: AssistantTime,

    /// Error if response failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<MessageError>,

    /// Parent user message ID.
    pub parent_id: String,

    /// Model ID.
    pub model_id: String,

    /// Provider ID.
    pub provider_id: String,

    /// Agent name.
    pub agent: String,

    /// Path context.
    pub path: PathContext,

    /// Whether this contains a summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<bool>,

    /// Cost in cents.
    pub cost: f64,

    /// Token usage.
    pub tokens: TokenUsage,

    /// Finish reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
}

impl AssistantMessage {
    /// Create a new assistant message.
    pub fn new(
        session_id: impl Into<String>,
        parent_id: impl Into<String>,
        agent: impl Into<String>,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        cwd: impl Into<String>,
        root: impl Into<String>,
    ) -> Self {
        Self {
            id: Identifier::message(),
            session_id: session_id.into(),
            time: AssistantTime::started(),
            error: None,
            parent_id: parent_id.into(),
            model_id: model_id.into(),
            provider_id: provider_id.into(),
            agent: agent.into(),
            path: PathContext {
                cwd: cwd.into(),
                root: root.into(),
            },
            summary: None,
            cost: 0.0,
            tokens: TokenUsage::default(),
            finish: None,
        }
    }

    /// Mark the message as completed.
    pub fn complete(&mut self, finish: Option<String>) {
        self.time.completed = Some(chrono::Utc::now().timestamp_millis());
        self.finish = finish;
    }
}

/// Message timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTime {
    /// Creation timestamp (ms).
    pub created: i64,
}

impl MessageTime {
    pub fn now() -> Self {
        Self {
            created: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Assistant message timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantTime {
    /// Creation timestamp (ms).
    pub created: i64,

    /// Completion timestamp (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<i64>,
}

impl AssistantTime {
    pub fn started() -> Self {
        Self {
            created: chrono::Utc::now().timestamp_millis(),
            completed: None,
        }
    }
}

/// User message summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    #[serde(default)]
    pub diffs: Vec<FileDiff>,
}

/// Model reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

/// Path context for message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathContext {
    /// Current working directory.
    pub cwd: String,
    /// Project root.
    pub root: String,
}

/// Message error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageError {
    Auth { message: String },
    Unknown { message: String },
    OutputLength { message: String },
    Aborted,
    Api { status: u16, message: String },
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u32,
    pub output: u32,
    #[serde(default)]
    pub reasoning: u32,
    pub cache: CacheUsage,
}

/// Cache hit/miss statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheUsage {
    pub read: u32,
    pub write: u32,
}

/// File diff summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub file: String,
    pub before: String,
    pub after: String,
    pub additions: u32,
    pub deletions: u32,
}

// ============================================================================
// Message Parts
// ============================================================================

/// A part of a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MessagePart {
    Text(TextPart),
    Reasoning(ReasoningPart),
    Tool(ToolPart),
    File(FilePart),
    StepStart(StepStartPart),
    StepFinish(StepFinishPart),
    Snapshot(SnapshotPart),
    Patch(PatchPart),
    Subtask(SubtaskPart),
    Agent(AgentPart),
    Retry(RetryPart),
    Compaction(CompactionPart),
}

impl MessagePart {
    /// Get the part ID.
    pub fn id(&self) -> &str {
        match self {
            MessagePart::Text(p) => &p.id,
            MessagePart::Reasoning(p) => &p.id,
            MessagePart::Tool(p) => &p.id,
            MessagePart::File(p) => &p.id,
            MessagePart::StepStart(p) => &p.id,
            MessagePart::StepFinish(p) => &p.id,
            MessagePart::Snapshot(p) => &p.id,
            MessagePart::Patch(p) => &p.id,
            MessagePart::Subtask(p) => &p.id,
            MessagePart::Agent(p) => &p.id,
            MessagePart::Retry(p) => &p.id,
            MessagePart::Compaction(p) => &p.id,
        }
    }

    /// Get the message ID.
    pub fn message_id(&self) -> &str {
        match self {
            MessagePart::Text(p) => &p.message_id,
            MessagePart::Reasoning(p) => &p.message_id,
            MessagePart::Tool(p) => &p.message_id,
            MessagePart::File(p) => &p.message_id,
            MessagePart::StepStart(p) => &p.message_id,
            MessagePart::StepFinish(p) => &p.message_id,
            MessagePart::Snapshot(p) => &p.message_id,
            MessagePart::Patch(p) => &p.message_id,
            MessagePart::Subtask(p) => &p.message_id,
            MessagePart::Agent(p) => &p.message_id,
            MessagePart::Retry(p) => &p.message_id,
            MessagePart::Compaction(p) => &p.message_id,
        }
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        match self {
            MessagePart::Text(p) => &p.session_id,
            MessagePart::Reasoning(p) => &p.session_id,
            MessagePart::Tool(p) => &p.session_id,
            MessagePart::File(p) => &p.session_id,
            MessagePart::StepStart(p) => &p.session_id,
            MessagePart::StepFinish(p) => &p.session_id,
            MessagePart::Snapshot(p) => &p.session_id,
            MessagePart::Patch(p) => &p.session_id,
            MessagePart::Subtask(p) => &p.session_id,
            MessagePart::Agent(p) => &p.session_id,
            MessagePart::Retry(p) => &p.session_id,
            MessagePart::Compaction(p) => &p.session_id,
        }
    }
}

/// Common fields for all parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartBase {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
}

impl PartBase {
    pub fn new(session_id: impl Into<String>, message_id: impl Into<String>) -> Self {
        Self {
            id: Identifier::part(),
            session_id: session_id.into(),
            message_id: message_id.into(),
        }
    }
}

/// Text part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub text: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<PartTime>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl TextPart {
    pub fn new(
        session_id: impl Into<String>,
        message_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id: Identifier::part(),
            session_id: session_id.into(),
            message_id: message_id.into(),
            text: text.into(),
            synthetic: None,
            ignored: None,
            time: Some(PartTime::started()),
            metadata: None,
        }
    }
}

/// Reasoning/thinking part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub text: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<PartTime>,
}

impl ReasoningPart {
    pub fn new(
        session_id: impl Into<String>,
        message_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id: Identifier::part(),
            session_id: session_id.into(),
            message_id: message_id.into(),
            text: text.into(),
            time: Some(PartTime::started()),
        }
    }
}

/// Part timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartTime {
    pub start: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
}

impl PartTime {
    pub fn started() -> Self {
        Self {
            start: chrono::Utc::now().timestamp_millis(),
            end: None,
        }
    }

    pub fn finish(&mut self) {
        self.end = Some(chrono::Utc::now().timestamp_millis());
    }
}

/// Tool call part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub call_id: String,
    pub tool: String,
    pub state: ToolState,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl ToolPart {
    pub fn new(
        session_id: impl Into<String>,
        message_id: impl Into<String>,
        call_id: impl Into<String>,
        tool: impl Into<String>,
        input: serde_json::Value,
        raw: impl Into<String>,
    ) -> Self {
        Self {
            id: Identifier::part(),
            session_id: session_id.into(),
            message_id: message_id.into(),
            call_id: call_id.into(),
            tool: tool.into(),
            state: ToolState::Pending {
                input,
                raw: raw.into(),
            },
            metadata: None,
        }
    }
}

/// Tool execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ToolState {
    Pending {
        input: serde_json::Value,
        raw: String,
    },
    Running {
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
        time: ToolTime,
    },
    Completed {
        input: serde_json::Value,
        output: String,
        title: String,
        metadata: serde_json::Value,
        time: ToolTime,
        #[serde(skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<FilePart>>,
    },
    Error {
        input: serde_json::Value,
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
        time: ToolTime,
    },
}

/// Tool timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTime {
    pub start: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacted: Option<i64>,
}

impl ToolTime {
    pub fn started() -> Self {
        Self {
            start: chrono::Utc::now().timestamp_millis(),
            end: None,
            compacted: None,
        }
    }
}

/// File attachment part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub path: String,
    pub mime: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Step start marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepStartPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
}

/// Step finish with usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepFinishPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    pub cost: f64,
    pub tokens: TokenUsage,
}

/// Snapshot reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub snapshot: String,
}

/// Patch/diff part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub patch: String,
}

/// Subtask reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub subtask_session_id: String,
}

/// Agent switch marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub agent: String,
}

/// Retry marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub reason: String,
}

/// Compaction marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionPart {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub original_message_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let user = Message::User(UserMessage::new(
            "ses_123",
            "default",
            ModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3-5-sonnet".to_string(),
            },
        ));

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains(r#""role":"user""#));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, Message::User(_)));
    }

    #[test]
    fn test_part_serialization() {
        let text = MessagePart::Text(TextPart::new("ses_123", "msg_456", "Hello, world!"));

        let json = serde_json::to_string(&text).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains("Hello, world!"));

        let parsed: MessagePart = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, MessagePart::Text(_)));
    }

    #[test]
    fn test_tool_state_serialization() {
        let pending = ToolState::Pending {
            input: serde_json::json!({"path": "/tmp/test.txt"}),
            raw: r#"{"path": "/tmp/test.txt"}"#.to_string(),
        };

        let json = serde_json::to_string(&pending).unwrap();
        assert!(json.contains(r#""status":"pending""#));
    }
}
