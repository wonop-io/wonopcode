//! Tool registry.

use crate::BoxedTool;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available tools.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, BoxedTool>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Create a registry with all built-in tools.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // Register built-in tools
        registry.register(Arc::new(crate::read::ReadTool));
        registry.register(Arc::new(crate::write::WriteTool));
        registry.register(Arc::new(crate::edit::EditTool));
        registry.register(Arc::new(crate::multiedit::MultiEditTool));
        registry.register(Arc::new(crate::glob::GlobTool));
        registry.register(Arc::new(crate::grep::GrepTool));
        registry.register(Arc::new(crate::list::ListTool));
        registry.register(Arc::new(crate::patch::PatchTool));
        registry.register(Arc::new(crate::search::WebSearchTool::new()));
        registry.register(Arc::new(crate::search::CodeSearchTool::new()));

        // Register ACE tools
        registry.register(Arc::new(crate::ace::AceCreateArtifactTool));
        registry.register(Arc::new(crate::ace::AceReadArtifactTool));
        registry.register(Arc::new(crate::ace::AceTodoReadTool));
        registry.register(Arc::new(crate::ace::AceTodoWriteTool));
        registry.register(Arc::new(crate::ace::AceTodoUpdateTool));
        registry.register(Arc::new(crate::ace::AceWhatNowTool));
        registry.register(Arc::new(crate::ace::AceSubmitCheckpointTool));

        // Register ticket tools
        registry.register(Arc::new(crate::ticket::TicketListTool));
        registry.register(Arc::new(crate::ticket::TicketSearchTool));
        registry.register(Arc::new(crate::ticket::TicketReadTool));
        registry.register(Arc::new(crate::ticket::TicketCreateTool));
        registry.register(Arc::new(crate::ticket::TicketListTrackersTool));

        // Register memory tools
        registry.register(Arc::new(crate::memory::MemoryStoreTool));
        registry.register(Arc::new(crate::memory::MemoryRecallTool));
        registry.register(Arc::new(crate::memory::MemorySearchTool));
        registry.register(Arc::new(crate::memory::MemoryClearTool));

        registry
    }

    /// Create a registry with all built-in tools, returning an Arc for batch support.
    pub fn with_builtins_arc() -> Arc<Self> {
        // Register the batch tool which needs a reference to the registry
        // Note: This creates a reference cycle, but it's intentional for batch
        // The batch tool will be registered separately by the caller

        Arc::new(Self::with_builtins())
    }

    /// Register a tool.
    pub fn register(&mut self, tool: BoxedTool) {
        self.tools.insert(tool.id().to_string(), tool);
    }

    /// Get a tool by ID.
    pub fn get(&self, id: &str) -> Option<&BoxedTool> {
        self.tools.get(id)
    }

    /// List all tool IDs.
    pub fn list(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Get all tools.
    pub fn all(&self) -> impl Iterator<Item = &BoxedTool> {
        self.tools.values()
    }

    /// Get tools filtered by a predicate.
    pub fn filter<F>(&self, predicate: F) -> Vec<&BoxedTool>
    where
        F: Fn(&str) -> bool,
    {
        self.tools
            .iter()
            .filter(|(id, _)| predicate(id))
            .map(|(_, tool)| tool)
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tool, ToolContext, ToolOutput, ToolResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    struct MockTool {
        id: String,
    }

    impl MockTool {
        fn new(id: &str) -> Self {
            Self { id: id.to_string() }
        }
    }

    #[async_trait]
    impl Tool for MockTool {
        fn id(&self) -> &str {
            &self.id
        }

        fn description(&self) -> &str {
            "Mock tool for testing"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::new("Success", "Mock output"))
        }
    }

    #[test]
    fn tool_registry_new_creates_empty() {
        let registry = ToolRegistry::new();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn tool_registry_default_creates_empty() {
        let registry = ToolRegistry::default();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn tool_registry_register_adds_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("test_tool")));

        assert_eq!(registry.list().len(), 1);
        assert!(registry.get("test_tool").is_some());
    }

    #[test]
    fn tool_registry_get_returns_none_for_unknown() {
        let registry = ToolRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn tool_registry_list_returns_all_ids() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("tool_a")));
        registry.register(Arc::new(MockTool::new("tool_b")));

        let ids = registry.list();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"tool_a"));
        assert!(ids.contains(&"tool_b"));
    }

    #[test]
    fn tool_registry_all_iterates_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("tool_1")));
        registry.register(Arc::new(MockTool::new("tool_2")));

        let tools: Vec<_> = registry.all().collect();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn tool_registry_filter_by_predicate() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::new("read")));
        registry.register(Arc::new(MockTool::new("write")));
        registry.register(Arc::new(MockTool::new("readdir")));

        let read_tools = registry.filter(|id| id.starts_with("read"));
        assert_eq!(read_tools.len(), 2);
    }

    #[test]
    fn tool_registry_with_builtins_has_tools() {
        let registry = ToolRegistry::with_builtins();
        let tools = registry.list();

        // Should have core tools
        assert!(tools.contains(&"read"));
        assert!(tools.contains(&"write"));
        assert!(tools.contains(&"edit"));
        assert!(tools.contains(&"glob"));
        assert!(tools.contains(&"grep"));

        // Should have ACE tools
        assert!(tools.contains(&"ace_create_artifact"));
        assert!(tools.contains(&"ace_read_artifact"));
        assert!(tools.contains(&"ace_todo_read"));
        assert!(tools.contains(&"ace_todo_write"));
        assert!(tools.contains(&"ace_todo_update"));
        assert!(tools.contains(&"ace_what_now"));
        assert!(tools.contains(&"ace_submit_checkpoint"));

        // Should have ticket tools
        assert!(tools.contains(&"ticket_list"));
        assert!(tools.contains(&"ticket_search"));
        assert!(tools.contains(&"ticket_read"));
        assert!(tools.contains(&"ticket_create"));
        assert!(tools.contains(&"ticket_list_trackers"));
    }

    #[test]
    fn tool_registry_with_builtins_arc_returns_arc() {
        let registry = ToolRegistry::with_builtins_arc();
        assert!(registry.get("read").is_some());
    }

    // Ticket tools integration tests

    #[test]
    fn tool_registry_ticket_list_has_valid_schema() {
        let registry = ToolRegistry::with_builtins();
        let tool = registry
            .get("ticket_list")
            .expect("ticket_list should be registered");

        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["status"].is_object());
        assert!(schema["properties"]["assignee"].is_object());
        assert!(schema["properties"]["labels"].is_object());
        assert!(schema["properties"]["limit"].is_object());
    }

    #[test]
    fn tool_registry_ticket_search_has_valid_schema() {
        let registry = ToolRegistry::with_builtins();
        let tool = registry
            .get("ticket_search")
            .expect("ticket_search should be registered");

        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"]
            .as_array()
            .expect("should have required array");
        assert!(required.contains(&json!("query")));

        assert!(schema["properties"]["query"].is_object());
        assert!(schema["properties"]["limit"].is_object());
    }

    #[test]
    fn tool_registry_ticket_read_has_valid_schema() {
        let registry = ToolRegistry::with_builtins();
        let tool = registry
            .get("ticket_read")
            .expect("ticket_read should be registered");

        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"]
            .as_array()
            .expect("should have required array");
        assert!(required.contains(&json!("ticket_id")));

        assert!(schema["properties"]["ticket_id"].is_object());
        assert!(schema["properties"]["include_comments"].is_object());
        assert!(schema["properties"]["include_attachments"].is_object());
    }

    #[test]
    fn tool_registry_ticket_create_has_valid_schema() {
        let registry = ToolRegistry::with_builtins();
        let tool = registry
            .get("ticket_create")
            .expect("ticket_create should be registered");

        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"]
            .as_array()
            .expect("should have required array");
        assert!(required.contains(&json!("title")));

        assert!(schema["properties"]["title"].is_object());
        assert!(schema["properties"]["description"].is_object());
        assert!(schema["properties"]["tracker_id"].is_object());
        assert!(schema["properties"]["assignee"].is_object());
        assert!(schema["properties"]["labels"].is_object());
        assert!(schema["properties"]["status"].is_object());
    }

    #[test]
    fn tool_registry_ticket_tools_have_descriptions() {
        let registry = ToolRegistry::with_builtins();

        let ticket_tools = [
            "ticket_list",
            "ticket_search",
            "ticket_read",
            "ticket_create",
            "ticket_list_trackers",
        ];
        for tool_id in &ticket_tools {
            let tool = registry
                .get(tool_id)
                .unwrap_or_else(|| panic!("{tool_id} should be registered"));
            let desc = tool.description();
            assert!(
                !desc.is_empty(),
                "{} should have a non-empty description",
                tool_id
            );
            assert!(
                desc.len() > 20,
                "{} description should be meaningful",
                tool_id
            );
        }
    }

    #[test]
    fn tool_registry_filter_ticket_tools() {
        let registry = ToolRegistry::with_builtins();

        let ticket_tools = registry.filter(|id| id.starts_with("ticket_"));
        assert_eq!(ticket_tools.len(), 5);

        let tool_ids: Vec<&str> = ticket_tools.iter().map(|t| t.id()).collect();
        assert!(tool_ids.contains(&"ticket_list"));
        assert!(tool_ids.contains(&"ticket_search"));
        assert!(tool_ids.contains(&"ticket_read"));
        assert!(tool_ids.contains(&"ticket_create"));
        assert!(tool_ids.contains(&"ticket_list_trackers"));
    }

    #[test]
    fn tool_registry_ticket_tools_unique_ids() {
        let registry = ToolRegistry::with_builtins();

        let tools = registry.list();
        let ticket_ids: Vec<&str> = tools
            .into_iter()
            .filter(|id| id.starts_with("ticket_"))
            .collect();

        // Ensure no duplicates
        let mut unique = ticket_ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            ticket_ids.len(),
            unique.len(),
            "ticket tool IDs should be unique"
        );
    }
}
