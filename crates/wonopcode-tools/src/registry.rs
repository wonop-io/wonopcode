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
