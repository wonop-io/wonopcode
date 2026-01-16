//! Tool registry.

use crate::BoxedTool;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available tools.
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
    }

    #[test]
    fn tool_registry_with_builtins_arc_returns_arc() {
        let registry = ToolRegistry::with_builtins_arc();
        assert!(registry.get("read").is_some());
    }
}
