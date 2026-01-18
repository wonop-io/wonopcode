//! Message types for sessions.
//!
//! Messages are the fundamental unit of conversation. Each message belongs
//! to a session and contains one or more parts (text, tool calls, etc.).
// @ace:design DES-T90R4U-13QO
// @ace:implements COMP-T90R4U-147K

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

    #[test]
    fn test_message_methods() {
        let user_msg = UserMessage::new(
            "ses_123",
            "agent",
            ModelRef {
                provider_id: "test".to_string(),
                model_id: "model".to_string(),
            },
        );
        let user = Message::User(user_msg.clone());

        assert!(user.is_user());
        assert!(!user.is_assistant());
        assert_eq!(user.id(), user_msg.id);
        assert_eq!(user.session_id(), "ses_123");
        assert!(user.created_at() > 0);
    }

    #[test]
    fn test_assistant_message_methods() {
        let assistant_msg = AssistantMessage::new(
            "ses_123",
            "msg_parent",
            "agent",
            "anthropic",
            "claude",
            "/cwd",
            "/root",
        );
        let assistant = Message::Assistant(assistant_msg.clone());

        assert!(!assistant.is_user());
        assert!(assistant.is_assistant());
        assert_eq!(assistant.id(), assistant_msg.id);
        assert_eq!(assistant.session_id(), "ses_123");
        assert!(assistant.created_at() > 0);
    }

    #[test]
    fn test_assistant_message_complete() {
        let mut assistant = AssistantMessage::new(
            "ses_123",
            "msg_parent",
            "agent",
            "anthropic",
            "claude",
            "/cwd",
            "/root",
        );
        assert!(assistant.time.completed.is_none());
        assert!(assistant.finish.is_none());

        assistant.complete(Some("end_turn".to_string()));

        assert!(assistant.time.completed.is_some());
        assert_eq!(assistant.finish, Some("end_turn".to_string()));
    }

    #[test]
    fn test_message_time() {
        let time = MessageTime::now();
        assert!(time.created > 0);
    }

    #[test]
    fn test_assistant_time() {
        let time = AssistantTime::started();
        assert!(time.created > 0);
        assert!(time.completed.is_none());
    }

    #[test]
    fn test_part_time() {
        let mut time = PartTime::started();
        assert!(time.start > 0);
        assert!(time.end.is_none());

        time.finish();
        assert!(time.end.is_some());
        assert!(time.end.unwrap() >= time.start);
    }

    #[test]
    fn test_tool_time() {
        let time = ToolTime::started();
        assert!(time.start > 0);
        assert!(time.end.is_none());
        assert!(time.compacted.is_none());
    }

    #[test]
    fn test_part_base() {
        let base = PartBase::new("ses_123", "msg_456");
        assert!(!base.id.is_empty());
        assert_eq!(base.session_id, "ses_123");
        assert_eq!(base.message_id, "msg_456");
    }

    #[test]
    fn test_reasoning_part() {
        let part = ReasoningPart::new("ses_123", "msg_456", "thinking...");
        assert!(!part.id.is_empty());
        assert_eq!(part.session_id, "ses_123");
        assert_eq!(part.message_id, "msg_456");
        assert_eq!(part.text, "thinking...");
        assert!(part.time.is_some());
    }

    #[test]
    fn test_tool_part() {
        let part = ToolPart::new(
            "ses_123",
            "msg_456",
            "call_789",
            "Read",
            serde_json::json!({"filePath": "/test.txt"}),
            r#"{"filePath": "/test.txt"}"#,
        );
        assert!(!part.id.is_empty());
        assert_eq!(part.session_id, "ses_123");
        assert_eq!(part.message_id, "msg_456");
        assert_eq!(part.call_id, "call_789");
        assert_eq!(part.tool, "Read");
        assert!(matches!(part.state, ToolState::Pending { .. }));
    }

    #[test]
    fn test_message_part_accessors() {
        // Test Text
        let text_part = MessagePart::Text(TextPart::new("ses_1", "msg_1", "hello"));
        assert!(!text_part.id().is_empty());
        assert_eq!(text_part.message_id(), "msg_1");
        assert_eq!(text_part.session_id(), "ses_1");

        // Test Reasoning
        let reasoning = MessagePart::Reasoning(ReasoningPart::new("ses_2", "msg_2", "think"));
        assert!(!reasoning.id().is_empty());
        assert_eq!(reasoning.message_id(), "msg_2");
        assert_eq!(reasoning.session_id(), "ses_2");

        // Test Tool
        let tool = MessagePart::Tool(ToolPart::new(
            "ses_3",
            "msg_3",
            "call_1",
            "Bash",
            serde_json::json!({}),
            "{}",
        ));
        assert!(!tool.id().is_empty());
        assert_eq!(tool.message_id(), "msg_3");
        assert_eq!(tool.session_id(), "ses_3");
    }

    #[test]
    fn test_file_part() {
        let part = MessagePart::File(FilePart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            path: "/path/to/file.txt".to_string(),
            mime: "text/plain".to_string(),
            url: Some("file:///path/to/file.txt".to_string()),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.message_id(), "msg_1");
        assert_eq!(part.session_id(), "ses_1");
    }

    #[test]
    fn test_step_parts() {
        let start = MessagePart::StepStart(StepStartPart {
            id: "part_start".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
        });
        assert_eq!(start.id(), "part_start");
        assert_eq!(start.session_id(), "ses_1");

        let finish = MessagePart::StepFinish(StepFinishPart {
            id: "part_finish".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            reason: "end_turn".to_string(),
            snapshot: None,
            cost: 0.01,
            tokens: TokenUsage::default(),
        });
        assert_eq!(finish.id(), "part_finish");
        assert_eq!(finish.message_id(), "msg_1");
    }

    #[test]
    fn test_snapshot_part() {
        let part = MessagePart::Snapshot(SnapshotPart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            snapshot: "snap_123".to_string(),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.session_id(), "ses_1");
    }

    #[test]
    fn test_patch_part() {
        let part = MessagePart::Patch(PatchPart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            patch: "+line\n-line".to_string(),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.message_id(), "msg_1");
    }

    #[test]
    fn test_subtask_part() {
        let part = MessagePart::Subtask(SubtaskPart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            subtask_session_id: "sub_ses_1".to_string(),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.session_id(), "ses_1");
    }

    #[test]
    fn test_agent_part() {
        let part = MessagePart::Agent(AgentPart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            agent: "explorer".to_string(),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.session_id(), "ses_1");
    }

    #[test]
    fn test_retry_part() {
        let part = MessagePart::Retry(RetryPart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            reason: "rate_limit".to_string(),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.message_id(), "msg_1");
    }

    #[test]
    fn test_compaction_part() {
        let part = MessagePart::Compaction(CompactionPart {
            id: "part_1".to_string(),
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            original_message_id: "msg_original".to_string(),
        });
        assert_eq!(part.id(), "part_1");
        assert_eq!(part.session_id(), "ses_1");
    }

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input, 0);
        assert_eq!(usage.output, 0);
        assert_eq!(usage.reasoning, 0);
        assert_eq!(usage.cache.read, 0);
        assert_eq!(usage.cache.write, 0);
    }

    #[test]
    fn test_user_summary() {
        let summary = UserSummary {
            title: Some("Test title".to_string()),
            body: Some("Test body".to_string()),
            diffs: vec![FileDiff {
                file: "test.txt".to_string(),
                before: "old".to_string(),
                after: "new".to_string(),
                additions: 1,
                deletions: 1,
            }],
        };
        assert_eq!(summary.title, Some("Test title".to_string()));
        assert_eq!(summary.diffs.len(), 1);
    }

    #[test]
    fn test_model_ref() {
        let model = ModelRef {
            provider_id: "anthropic".to_string(),
            model_id: "claude-3-5-sonnet".to_string(),
        };
        let json = serde_json::to_string(&model).unwrap();
        let parsed: ModelRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider_id, "anthropic");
        assert_eq!(parsed.model_id, "claude-3-5-sonnet");
    }

    #[test]
    fn test_path_context() {
        let ctx = PathContext {
            cwd: "/home/user/project".to_string(),
            root: "/home/user/project".to_string(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: PathContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cwd, "/home/user/project");
    }

    #[test]
    fn test_message_error_variants() {
        let errors = vec![
            MessageError::Auth {
                message: "Invalid API key".to_string(),
            },
            MessageError::Unknown {
                message: "Unknown error".to_string(),
            },
            MessageError::OutputLength {
                message: "Output too long".to_string(),
            },
            MessageError::Aborted,
            MessageError::Api {
                status: 500,
                message: "Server error".to_string(),
            },
        ];

        for err in errors {
            let json = serde_json::to_string(&err).unwrap();
            let _parsed: MessageError = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_tool_state_variants() {
        let running = ToolState::Running {
            input: serde_json::json!({}),
            title: Some("Running tool".to_string()),
            metadata: None,
            time: ToolTime::started(),
        };
        let json = serde_json::to_string(&running).unwrap();
        assert!(json.contains(r#""status":"running""#));

        let completed = ToolState::Completed {
            input: serde_json::json!({}),
            output: "result".to_string(),
            title: "Done".to_string(),
            metadata: serde_json::json!({}),
            time: ToolTime::started(),
            attachments: None,
        };
        let json = serde_json::to_string(&completed).unwrap();
        assert!(json.contains(r#""status":"completed""#));

        let error = ToolState::Error {
            input: serde_json::json!({}),
            error: "failed".to_string(),
            metadata: None,
            time: ToolTime::started(),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""status":"error""#));
    }

    #[test]
    fn test_assistant_message_serialization() {
        let assistant = AssistantMessage::new(
            "ses_123",
            "msg_parent",
            "default",
            "anthropic",
            "claude-3-5-sonnet",
            "/cwd",
            "/root",
        );

        let msg = Message::Assistant(assistant);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"assistant""#));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_assistant());
    }
}