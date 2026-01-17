//! Builder patterns for constructing test objects.
//!
//! Provides fluent builders for complex objects commonly used in tests.

use serde_json::Value;
use wonopcode_provider::message::{ContentPart, Message, Role};

/// Builder for constructing test messages.
///
/// # Example
///
/// ```rust
/// use wonopcode_test_utils::builders::MessageBuilder;
///
/// let message = MessageBuilder::user()
///     .text("Hello, how are you?")
///     .build();
///
/// let assistant_msg = MessageBuilder::assistant()
///     .text("I'm doing well!")
///     .tool_call("read", "call_1", r#"{"path": "test.txt"}"#)
///     .build();
/// ```
pub struct MessageBuilder {
    role: Role,
    content: Vec<ContentPart>,
}

impl MessageBuilder {
    /// Create a builder for a user message.
    pub fn user() -> Self {
        Self {
            role: Role::User,
            content: Vec::new(),
        }
    }

    /// Create a builder for an assistant message.
    pub fn assistant() -> Self {
        Self {
            role: Role::Assistant,
            content: Vec::new(),
        }
    }

    /// Create a builder for a system message.
    pub fn system() -> Self {
        Self {
            role: Role::System,
            content: Vec::new(),
        }
    }

    /// Create a builder for a tool result message.
    pub fn tool() -> Self {
        Self {
            role: Role::Tool,
            content: Vec::new(),
        }
    }

    /// Add text content.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.content.push(ContentPart::Text { text: text.into() });
        self
    }

    /// Add a tool call (for assistant messages).
    pub fn tool_call(mut self, name: &str, id: &str, arguments: &str) -> Self {
        self.content.push(ContentPart::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input: serde_json::from_str(arguments).unwrap_or(Value::Null),
        });
        self
    }

    /// Add a tool result (for tool messages).
    pub fn tool_result(mut self, tool_use_id: &str, content: &str, is_error: bool) -> Self {
        self.content.push(ContentPart::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: content.to_string(),
            is_error: Some(is_error),
        });
        self
    }

    /// Add thinking/reasoning content.
    pub fn thinking(mut self, thinking_text: impl Into<String>) -> Self {
        self.content.push(ContentPart::Thinking {
            text: thinking_text.into(),
        });
        self
    }

    /// Build the message.
    pub fn build(self) -> Message {
        Message {
            role: self.role,
            content: self.content,
        }
    }
}

/// Builder for constructing conversation histories.
///
/// # Example
///
/// ```rust
/// use wonopcode_test_utils::builders::ConversationBuilder;
///
/// let history = ConversationBuilder::new()
///     .user("Hello!")
///     .assistant("Hi there!")
///     .user("How are you?")
///     .build();
///
/// assert_eq!(history.len(), 3);
/// ```
pub struct ConversationBuilder {
    messages: Vec<Message>,
}

impl ConversationBuilder {
    /// Create a new conversation builder.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// Add a system message.
    pub fn system(mut self, text: impl Into<String>) -> Self {
        self.messages.push(Message::system(text.into()));
        self
    }

    /// Add a user message.
    pub fn user(mut self, text: impl Into<String>) -> Self {
        self.messages.push(Message::user(text.into()));
        self
    }

    /// Add an assistant message.
    pub fn assistant(mut self, text: impl Into<String>) -> Self {
        self.messages.push(Message::assistant(text.into()));
        self
    }

    /// Add a custom message.
    pub fn message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    /// Add a tool call from assistant and its result.
    pub fn tool_interaction(
        mut self,
        tool_name: &str,
        tool_id: &str,
        arguments: &str,
        result: &str,
    ) -> Self {
        // Assistant message with tool call
        self.messages.push(
            MessageBuilder::assistant()
                .tool_call(tool_name, tool_id, arguments)
                .build(),
        );

        // Tool result
        self.messages.push(
            MessageBuilder::tool()
                .tool_result(tool_id, result, false)
                .build(),
        );

        self
    }

    /// Build the conversation history.
    pub fn build(self) -> Vec<Message> {
        self.messages
    }
}

impl Default for ConversationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing tool definitions.
#[derive(Default)]
pub struct ToolDefinitionBuilder {
    name: String,
    description: String,
    parameters: Value,
}

impl ToolDefinitionBuilder {
    /// Create a new tool definition builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameters: Value::Object(Default::default()),
        }
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set parameters from JSON string.
    pub fn parameters_json(mut self, json: &str) -> Self {
        self.parameters = serde_json::from_str(json).unwrap_or(Value::Null);
        self
    }

    /// Add a string parameter.
    pub fn string_param(mut self, name: &str, description: &str, required: bool) -> Self {
        let params = self.parameters.as_object_mut().unwrap();

        // Ensure properties exists
        if !params.contains_key("properties") {
            params.insert("properties".to_string(), Value::Object(Default::default()));
        }
        if !params.contains_key("required") {
            params.insert("required".to_string(), Value::Array(Vec::new()));
        }

        // Add property
        let properties = params
            .get_mut("properties")
            .unwrap()
            .as_object_mut()
            .unwrap();
        properties.insert(
            name.to_string(),
            serde_json::json!({
                "type": "string",
                "description": description
            }),
        );

        // Add to required if needed
        if required {
            let req = params.get_mut("required").unwrap().as_array_mut().unwrap();
            req.push(Value::String(name.to_string()));
        }

        self
    }

    /// Build the tool definition as JSON.
    pub fn build(self) -> Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": {
                "type": "object",
                "properties": self.parameters.get("properties").unwrap_or(&Value::Object(Default::default())),
                "required": self.parameters.get("required").unwrap_or(&Value::Array(Vec::new()))
            }
        })
    }
}

/// Builder for configuration objects.
#[derive(Default)]
pub struct ConfigBuilder {
    values: serde_json::Map<String, Value>,
}

impl ConfigBuilder {
    /// Create a new config builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a string value.
    pub fn set(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.values.insert(key.to_string(), value.into());
        self
    }

    /// Set the theme.
    pub fn theme(self, theme: &str) -> Self {
        self.set("theme", theme)
    }

    /// Set the model.
    pub fn model(self, model: &str) -> Self {
        self.set("model", model)
    }

    /// Build as JSON string.
    pub fn build_json(&self) -> String {
        serde_json::to_string_pretty(&self.values).unwrap()
    }

    /// Build as Value.
    pub fn build(self) -> Value {
        Value::Object(self.values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_builder() {
        let msg = MessageBuilder::user().text("Hello").build();
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn test_message_builder_assistant() {
        let msg = MessageBuilder::assistant().text("Hello").build();
        assert_eq!(msg.role, Role::Assistant);
    }

    #[test]
    fn test_message_builder_system() {
        let msg = MessageBuilder::system().text("You are helpful").build();
        assert_eq!(msg.role, Role::System);
    }

    #[test]
    fn test_message_builder_tool() {
        let msg = MessageBuilder::tool()
            .tool_result("call_1", "Success", false)
            .build();
        assert_eq!(msg.role, Role::Tool);
    }

    #[test]
    fn test_message_builder_with_tool_call() {
        let msg = MessageBuilder::assistant()
            .tool_call("read", "call_1", r#"{"path": "test.txt"}"#)
            .build();
        assert_eq!(msg.content.len(), 1);
        if let ContentPart::ToolUse { id, name, .. } = &msg.content[0] {
            assert_eq!(id, "call_1");
            assert_eq!(name, "read");
        } else {
            panic!("Expected ToolUse");
        }
    }

    #[test]
    fn test_message_builder_with_thinking() {
        let msg = MessageBuilder::assistant()
            .thinking("Let me think about this...")
            .text("Here's my answer")
            .build();
        assert_eq!(msg.content.len(), 2);
    }

    #[test]
    fn test_conversation_builder() {
        let history = ConversationBuilder::new()
            .user("Hi")
            .assistant("Hello!")
            .build();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].role, Role::Assistant);
    }

    #[test]
    fn test_conversation_builder_with_system() {
        let history = ConversationBuilder::new()
            .system("You are helpful")
            .user("Hi")
            .build();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::System);
    }

    #[test]
    fn test_conversation_builder_with_message() {
        let custom_msg = MessageBuilder::assistant().text("Custom message").build();

        let history = ConversationBuilder::new()
            .user("Hi")
            .message(custom_msg)
            .build();

        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_conversation_builder_tool_interaction() {
        let history = ConversationBuilder::new()
            .user("Read a file")
            .tool_interaction("read", "call_1", r#"{"path": "test.txt"}"#, "file content")
            .build();

        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, Role::Assistant);
        assert_eq!(history[2].role, Role::Tool);
    }

    #[test]
    fn test_conversation_builder_default() {
        let builder = ConversationBuilder::default();
        let history = builder.user("Hi").build();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_tool_definition_builder() {
        let tool = ToolDefinitionBuilder::new("read")
            .description("Read a file")
            .string_param("path", "File path to read", true)
            .build();

        assert_eq!(tool["name"], "read");
        assert!(tool["input_schema"]["properties"]["path"].is_object());
    }

    #[test]
    fn test_tool_definition_builder_optional_param() {
        let tool = ToolDefinitionBuilder::new("search")
            .description("Search for text")
            .string_param("query", "Search query", true)
            .string_param("limit", "Max results", false)
            .build();

        let required = tool["input_schema"]["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "query");
    }

    #[test]
    fn test_tool_definition_builder_with_parameters_json() {
        let tool = ToolDefinitionBuilder::new("custom")
            .description("Custom tool")
            .parameters_json(r#"{"custom": "params"}"#)
            .build();

        assert_eq!(tool["name"], "custom");
    }

    #[test]
    fn test_tool_definition_builder_default() {
        let builder = ToolDefinitionBuilder::default();
        let tool = builder.build();
        assert_eq!(tool["name"], "");
    }

    #[test]
    fn test_config_builder() {
        let config = ConfigBuilder::new()
            .theme("dark")
            .model("anthropic/claude-sonnet-4-5-20250929")
            .build();

        assert_eq!(config["theme"], "dark");
        assert_eq!(config["model"], "anthropic/claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_config_builder_set_various_types() {
        let config = ConfigBuilder::new()
            .set("string_val", "hello")
            .set("number_val", 42)
            .set("bool_val", true)
            .build();

        assert_eq!(config["string_val"], "hello");
        assert_eq!(config["number_val"], 42);
        assert_eq!(config["bool_val"], true);
    }

    #[test]
    fn test_config_builder_build_json() {
        let builder = ConfigBuilder::new().theme("light");
        let json = builder.build_json();
        assert!(json.contains("light"));
    }
}
