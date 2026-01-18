//! Message types for AI conversations.

use serde::{Deserialize, Serialize};

/// The role of a message in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System message (instructions).
    System,
    /// User message.
    User,
    /// Assistant (AI) message.
    Assistant,
    /// Tool result message.
    Tool,
}

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The role of the message sender.
    pub role: Role,
    /// The content of the message.
    pub content: Vec<ContentPart>,
}

impl Message {
    /// Create a new system message.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: vec![ContentPart::Text { text: text.into() }],
        }
    }

    /// Create a new user message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentPart::Text { text: text.into() }],
        }
    }

    /// Create a new assistant message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentPart::Text { text: text.into() }],
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error: None,
            }],
        }
    }

    /// Create a tool error message.
    pub fn tool_error(tool_use_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: error.into(),
                is_error: Some(true),
            }],
        }
    }

    /// Add a content part to the message.
    pub fn with_part(mut self, part: ContentPart) -> Self {
        self.content.push(part);
        self
    }

    /// Get the text content of the message (concatenated).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// A part of a message's content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Text content.
    #[serde(rename = "text")]
    Text { text: String },

    /// Image content.
    #[serde(rename = "image")]
    Image { source: ImageSource },

    /// Tool use request (from assistant).
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool result (from user/tool).
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },

    /// Thinking/reasoning content (from assistant).
    #[serde(rename = "thinking")]
    Thinking { text: String },
}

impl ContentPart {
    /// Create a text content part.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create a tool use content part.
    pub fn tool_use(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        Self::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    /// Create a tool result content part.
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: None,
        }
    }
}

/// Image source for image content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ImageSource {
    /// Base64 encoded image.
    #[serde(rename = "base64")]
    Base64 { media_type: String, data: String },
    /// URL to an image.
    #[serde(rename = "url")]
    Url { url: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "Hello");
    }

    #[test]
    fn test_message_with_parts() {
        let msg =
            Message::assistant("Here's the answer").with_part(ContentPart::text("\n\nMore text"));
        assert_eq!(msg.text(), "Here's the answer\n\nMore text");
    }

    #[test]
    fn test_tool_result_message() {
        let msg = Message::tool_result("call_123", "Success!");
        assert_eq!(msg.role, Role::Tool);
        match &msg.content[0] {
            ContentPart::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_123");
                assert_eq!(content, "Success!");
                assert_eq!(*is_error, None);
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_serialization() {
        let msg = Message::user("Hello");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, Role::User);
        assert_eq!(parsed.text(), "Hello");
    }
}
