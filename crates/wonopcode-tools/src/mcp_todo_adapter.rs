//! MCP TODO tool adapter for wonopcode.
//!
//! This module provides an adapter that detects MCP servers providing TODO tools
//! (todo_read, todo_write, todo_update) and bridges them to the native TODO system.
//! When MCP TODO tools are available, native TODO tools are disabled, but the
//! sidebar continues to show TODO data by intercepting MCP tool outputs and
//! converting them to standard TODO events.

use crate::{Tool, ToolEvent, ToolOutput};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::todo::{Phase, PhasedTodos, TodoItem, TodoPriority, TodoStatus};

/// MCP TODO tool adapter that bridges MCP TODO tools to native TODO events.
pub struct McpTodoAdapter {
    /// MCP tool that provides todo reading functionality
    pub read_tool: Option<Arc<dyn Tool>>,
    /// MCP tool that provides todo writing functionality  
    pub write_tool: Option<Arc<dyn Tool>>,
    /// MCP tool that provides todo updating functionality
    pub update_tool: Option<Arc<dyn Tool>>,
    /// Tool name mappings for event emission
    tool_mappings: HashMap<String, McpTodoToolType>,
}

/// Type of MCP TODO tool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTodoToolType {
    Read,
    Write,
    Update,
}

impl McpTodoAdapter {
    /// Create a new MCP TODO adapter by scanning available tools.
    pub fn new(tools: &[Arc<dyn Tool>]) -> Self {
        let mut read_tool = None;
        let mut write_tool = None;
        let mut update_tool = None;
        let mut tool_mappings = HashMap::new();

        for tool in tools {
            let tool_id = tool.id();

            // Check if this is an MCP TODO tool based on naming patterns
            if Self::is_mcp_todo_read_tool(tool_id) {
                info!("Detected MCP TODO read tool: {}", tool_id);
                read_tool = Some(tool.clone());
                tool_mappings.insert(tool_id.to_string(), McpTodoToolType::Read);
            } else if Self::is_mcp_todo_write_tool(tool_id) {
                info!("Detected MCP TODO write tool: {}", tool_id);
                write_tool = Some(tool.clone());
                tool_mappings.insert(tool_id.to_string(), McpTodoToolType::Write);
            } else if Self::is_mcp_todo_update_tool(tool_id) {
                info!("Detected MCP TODO update tool: {}", tool_id);
                update_tool = Some(tool.clone());
                tool_mappings.insert(tool_id.to_string(), McpTodoToolType::Update);
            }
        }

        Self {
            read_tool,
            write_tool,
            update_tool,
            tool_mappings,
        }
    }

    /// Check if any MCP TODO tools are available.
    pub fn has_mcp_todo_tools(&self) -> bool {
        self.read_tool.is_some() || self.write_tool.is_some() || self.update_tool.is_some()
    }

    /// Check if MCP TODO tools provide read functionality.
    pub fn has_mcp_todo_read(&self) -> bool {
        self.read_tool.is_some()
    }

    /// Check if MCP TODO tools provide write functionality.
    pub fn has_mcp_todo_write(&self) -> bool {
        self.write_tool.is_some()
    }

    /// Check if MCP TODO tools provide update functionality.
    pub fn has_mcp_todo_update(&self) -> bool {
        self.update_tool.is_some()
    }

    /// Check if the given tool ID corresponds to an MCP TODO tool managed by this adapter.
    pub fn is_mcp_todo_tool(&self, tool_id: &str) -> bool {
        self.tool_mappings.contains_key(tool_id)
    }

    /// Get a clone of the tool mappings for use in async contexts.
    /// This is useful when the adapter itself can't be moved due to Arc<dyn Tool> fields.
    pub fn tool_mappings(&self) -> HashMap<String, McpTodoToolType> {
        self.tool_mappings.clone()
    }

    /// Parse MCP TODO tool output and convert to PhasedTodos (public for external use).
    /// Returns the tool type and parsed todos if the tool is a recognized MCP TODO tool.
    pub fn parse_todo_output(&self, tool_id: &str, output: &str) -> Option<PhasedTodos> {
        if let Some(tool_type) = self.tool_mappings.get(tool_id) {
            self.parse_mcp_todo_output(output, *tool_type).ok()
        } else {
            None
        }
    }

    /// Intercept MCP TODO tool output and emit appropriate events.
    ///
    /// This method should be called by the runner when an MCP TODO tool completes execution.
    /// It parses the tool output and emits TodosUpdated events so the sidebar receives updates.
    pub fn intercept_and_emit_events(
        &self,
        tool_id: &str,
        output: &ToolOutput,
        event_tx: &Option<mpsc::UnboundedSender<ToolEvent>>,
    ) {
        if let Some(tool_type) = self.tool_mappings.get(tool_id) {
            debug!(
                "Intercepting MCP TODO tool output: {} ({:?})",
                tool_id, tool_type
            );

            if let Some(tx) = event_tx {
                // Parse the output and extract TODO data
                if let Ok(phased_todos) = self.parse_mcp_todo_output(&output.output, *tool_type) {
                    if let Err(e) = tx.send(ToolEvent::TodosUpdated(phased_todos)) {
                        warn!(
                            "Failed to emit TodosUpdated event for MCP tool {}: {}",
                            tool_id, e
                        );
                    } else {
                        debug!("Emitted TodosUpdated event for MCP tool: {}", tool_id);
                    }
                } else {
                    debug!(
                        "Could not parse TODO data from MCP tool output: {}",
                        tool_id
                    );
                }
            }
        }
    }

    /// Parse MCP TODO tool output and convert it to PhasedTodos.
    ///
    /// This attempts to parse the output in multiple formats:
    /// 1. Direct JSON format matching our PhasedTodos structure
    /// 2. Simplified JSON with flat todo arrays
    /// 3. Markdown format with phase headers
    /// 4. Plain text format
    fn parse_mcp_todo_output(
        &self,
        output: &str,
        _tool_type: McpTodoToolType,
    ) -> Result<PhasedTodos, serde_json::Error> {
        // Try to parse as JSON first
        if let Ok(value) = serde_json::from_str::<Value>(output) {
            // Try direct PhasedTodos format
            if let Ok(phased_todos) = serde_json::from_value::<PhasedTodos>(value.clone()) {
                return Ok(phased_todos);
            }

            // Try parsing as flat todo list (legacy format)
            if let Ok(todos) = serde_json::from_value::<Vec<TodoItem>>(value.clone()) {
                let mut phased_todos = PhasedTodos::new();
                if !todos.is_empty() {
                    let mut phase = Phase::new("default", "Tasks");
                    phase.todos = todos;
                    phased_todos.add_phase(phase);
                }
                return Ok(phased_todos);
            }

            // Try parsing phases array from JSON
            if let Some(phases_value) = value.get("phases") {
                if let Ok(phases) = serde_json::from_value::<Vec<Phase>>(phases_value.clone()) {
                    let mut phased_todos = PhasedTodos::new();
                    for phase in phases {
                        phased_todos.add_phase(phase);
                    }
                    return Ok(phased_todos);
                }
            }
        }

        // Try parsing markdown format
        if let Ok(phased_todos) = self.parse_markdown_todos(output) {
            return Ok(phased_todos);
        }

        // Fallback: create a simple phase with the output as a single todo
        let mut phased_todos = PhasedTodos::new();
        let mut phase = Phase::new("mcp_output", "MCP TODO Output");

        // Create a single todo item from the output
        let todo = TodoItem {
            id: format!("mcp_todo_{}", chrono::Utc::now().timestamp()),
            content: output.trim().to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::Medium,
            parents: vec![],
        };
        phase.add_todo(todo);
        phased_todos.add_phase(phase);

        Ok(phased_todos)
    }

    /// Parse TODO data from markdown format.
    ///
    /// Expected format:
    /// ```text
    /// ## ○ Phase Name (0/3 done)
    ///   [ ] [high] Task description (task_id)
    ///   [>] [medium] In progress task (task_id_2)
    ///   [x] [low] Completed task (task_id_3)
    /// ```
    fn parse_markdown_todos(&self, output: &str) -> Result<PhasedTodos, serde_json::Error> {
        let mut phased_todos = PhasedTodos::new();
        let mut current_phase: Option<Phase> = None;

        for line in output.lines() {
            let trimmed = line.trim();

            // Phase header: ## ○ Phase Name (0/3 done)
            if trimmed.starts_with("##") {
                // Save previous phase if exists
                if let Some(phase) = current_phase.take() {
                    phased_todos.add_phase(phase);
                }

                // Extract phase name
                let phase_line = trimmed.trim_start_matches("##").trim();
                let phase_name = if let Some(pos) = phase_line.find('(') {
                    phase_line[..pos].trim()
                } else {
                    phase_line
                };

                // Remove status icon if present
                let phase_name = phase_name.trim_start_matches(['○', '◐', '●']).trim();

                current_phase = Some(Phase::new(
                    format!("phase_{}", phased_todos.phases.len() + 1),
                    phase_name,
                ));
            }
            // Todo item: [ ] [priority] description (id)
            else if trimmed.starts_with('[') {
                if let Some(ref mut phase) = current_phase {
                    if let Some(todo) = self.parse_markdown_todo_line(trimmed) {
                        phase.add_todo(todo);
                    }
                }
            }
        }

        // Save final phase
        if let Some(phase) = current_phase {
            phased_todos.add_phase(phase);
        }

        // If no phases were parsed, create a default one
        if phased_todos.phases.is_empty() {
            return Err(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "No valid TODO phases found in markdown",
            )));
        }

        Ok(phased_todos)
    }

    /// Parse a single markdown TODO line.
    fn parse_markdown_todo_line(&self, line: &str) -> Option<TodoItem> {
        // Match patterns like: [x] [high] Task description (task_id)
        let line = line.trim();

        // Extract status icon
        let status = if line.starts_with("[ ]") {
            TodoStatus::Pending
        } else if line.starts_with("[>]") {
            TodoStatus::InProgress
        } else if line.starts_with("[x]") {
            TodoStatus::Completed
        } else if line.starts_with("[-]") {
            TodoStatus::Cancelled
        } else {
            return None; // Invalid format
        };

        // Remove status part
        let rest = line[3..].trim();

        // Extract priority if present: [high], [medium], [low]
        let (priority, rest) = if rest.starts_with('[') {
            if let Some(end) = rest.find(']') {
                let priority_str = &rest[1..end];
                let priority = match priority_str {
                    "high" => TodoPriority::High,
                    "medium" => TodoPriority::Medium,
                    "low" => TodoPriority::Low,
                    _ => TodoPriority::Medium,
                };
                (priority, rest[end + 1..].trim())
            } else {
                (TodoPriority::Medium, rest)
            }
        } else {
            (TodoPriority::Medium, rest)
        };

        // Extract ID from parentheses at the end
        let (content, id) = if let Some(start) = rest.rfind('(') {
            if let Some(end) = rest.rfind(')') {
                if end > start {
                    let id = rest[start + 1..end].trim().to_string();
                    let content = rest[..start].trim().to_string();
                    (content, id)
                } else {
                    (
                        rest.to_string(),
                        format!("todo_{}", chrono::Utc::now().timestamp()),
                    )
                }
            } else {
                (
                    rest.to_string(),
                    format!("todo_{}", chrono::Utc::now().timestamp()),
                )
            }
        } else {
            (
                rest.to_string(),
                format!("todo_{}", chrono::Utc::now().timestamp()),
            )
        };

        Some(TodoItem {
            id,
            content,
            status,
            priority,
            parents: vec![],
        })
    }

    /// Check if a tool ID represents an MCP TODO read tool.
    ///
    /// NOTE: This only detects tools from external MCP servers, not the native
    /// `todoread` tool. The native tool is ACE-backed and handles its own output.
    fn is_mcp_todo_read_tool(tool_id: &str) -> bool {
        // Standard MCP naming patterns (external MCP servers)
        tool_id.starts_with("mcp__")
            && (tool_id.ends_with("__todo_read")
                || tool_id.ends_with("__todoread")
                || tool_id.contains("__todo_read__")
                || tool_id.contains("__todoread__"))
    }

    /// Check if a tool ID represents an MCP TODO write tool.
    fn is_mcp_todo_write_tool(tool_id: &str) -> bool {
        Self::is_mcp_todo_write_tool_static(tool_id)
    }

    /// Check if a tool ID represents an MCP TODO write tool (public static version).
    ///
    /// This can be called without an McpTodoAdapter instance, which is useful
    /// when intercepting tool completions from external execution (e.g., Claude CLI)
    /// where no local MCP setup exists.
    ///
    /// NOTE: This only detects tools from external MCP servers, not the native
    /// `todowrite` tool. The native tool is ACE-backed and handles its own output.
    pub fn is_mcp_todo_write_tool_static(tool_id: &str) -> bool {
        // Standard MCP naming patterns (external MCP servers)
        tool_id.starts_with("mcp__")
            && (tool_id.ends_with("__todo_write")
                || tool_id.ends_with("__todowrite")
                || tool_id.contains("__todo_write__")
                || tool_id.contains("__todowrite__"))
    }

    /// Check if a tool ID represents an MCP TODO update tool.
    ///
    /// NOTE: This only detects tools from external MCP servers, not the native
    /// `ace_todo_update` tool. The native tool is ACE-backed and handles its own output.
    fn is_mcp_todo_update_tool(tool_id: &str) -> bool {
        // Standard MCP naming patterns (external MCP servers)
        tool_id.starts_with("mcp__")
            && (tool_id.ends_with("__todo_update")
                || tool_id.ends_with("__todoupdate")
                || tool_id.contains("__todo_update__")
                || tool_id.contains("__todoupdate__"))
    }

    /// Get summary of detected MCP TODO tools for logging.
    pub fn get_summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref tool) = self.read_tool {
            parts.push(format!("read({})", tool.id()));
        }
        if let Some(ref tool) = self.write_tool {
            parts.push(format!("write({})", tool.id()));
        }
        if let Some(ref tool) = self.update_tool {
            parts.push(format!("update({})", tool.id()));
        }

        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Helper function for scanning tools and detecting MCP TODO tools.
///
/// This is a convenience function for use in the runner.
pub fn detect_mcp_todo_tools(tools: &[Arc<dyn Tool>]) -> McpTodoAdapter {
    McpTodoAdapter::new(tools)
}

/// Helper function to check if any MCP TODO tools are present in a tool list.
pub fn has_any_mcp_todo_tools(tools: &[Arc<dyn Tool>]) -> bool {
    tools.iter().any(|tool| {
        let id = tool.id();
        McpTodoAdapter::is_mcp_todo_read_tool(id)
            || McpTodoAdapter::is_mcp_todo_write_tool(id)
            || McpTodoAdapter::is_mcp_todo_update_tool(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolContext, ToolResult};
    use async_trait::async_trait;

    struct MockMcpTodoTool {
        id: String,
    }

    #[async_trait]
    impl Tool for MockMcpTodoTool {
        fn id(&self) -> &str {
            &self.id
        }

        fn description(&self) -> &str {
            "Mock MCP TODO tool"
        }

        fn parameters_schema(&self) -> Value {
            serde_json::json!({})
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
            Ok(ToolOutput::new("Mock", "Mock output"))
        }
    }

    #[test]
    fn test_mcp_tool_detection() {
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(MockMcpTodoTool {
                id: "mcp__server1__todo_read".to_string(),
            }),
            Arc::new(MockMcpTodoTool {
                id: "mcp__server2__todowrite".to_string(),
            }),
            Arc::new(MockMcpTodoTool {
                id: "mcp__server3__todo_update".to_string(),
            }),
            Arc::new(MockMcpTodoTool {
                id: "regular_tool".to_string(), // Should be ignored
            }),
        ];

        let adapter = McpTodoAdapter::new(&tools);

        assert!(adapter.has_mcp_todo_read());
        assert!(adapter.has_mcp_todo_write());
        assert!(adapter.has_mcp_todo_update());
        assert!(adapter.has_mcp_todo_tools());

        assert!(adapter.is_mcp_todo_tool("mcp__server1__todo_read"));
        assert!(adapter.is_mcp_todo_tool("mcp__server2__todowrite"));
        assert!(adapter.is_mcp_todo_tool("mcp__server3__todo_update"));
        assert!(!adapter.is_mcp_todo_tool("regular_tool"));

        let summary = adapter.get_summary();
        assert!(summary.contains("read(mcp__server1__todo_read)"));
        assert!(summary.contains("write(mcp__server2__todowrite)"));
        assert!(summary.contains("update(mcp__server3__todo_update)"));
    }

    #[test]
    fn test_native_tools_not_detected() {
        // Native ACE-backed tools should NOT be detected by the MCP adapter
        // They handle their own output and events
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(MockMcpTodoTool {
                id: "todoread".to_string(),
            }),
            Arc::new(MockMcpTodoTool {
                id: "todowrite".to_string(),
            }),
            Arc::new(MockMcpTodoTool {
                id: "ace_todo_update".to_string(),
            }),
        ];

        let adapter = McpTodoAdapter::new(&tools);

        // Native tools should not be detected
        assert!(!adapter.has_mcp_todo_read());
        assert!(!adapter.has_mcp_todo_write());
        assert!(!adapter.has_mcp_todo_update());
        assert!(!adapter.has_mcp_todo_tools());

        assert!(!adapter.is_mcp_todo_tool("todoread"));
        assert!(!adapter.is_mcp_todo_tool("todowrite"));
        assert!(!adapter.is_mcp_todo_tool("ace_todo_update"));
    }

    #[test]
    fn test_has_any_mcp_todo_tools() {
        let tools_with_mcp: Vec<Arc<dyn Tool>> = vec![Arc::new(MockMcpTodoTool {
            id: "mcp__server__todoread".to_string(),
        })];

        let tools_without_mcp: Vec<Arc<dyn Tool>> = vec![Arc::new(MockMcpTodoTool {
            id: "regular_tool".to_string(),
        })];

        assert!(has_any_mcp_todo_tools(&tools_with_mcp));
        assert!(!has_any_mcp_todo_tools(&tools_without_mcp));
    }

    #[test]
    fn test_parse_markdown_todos() {
        let adapter = McpTodoAdapter::new(&[]);

        let markdown = r#"
## ○ Analysis Phase (1/3 done)
  [x] [high] Explore codebase structure (explore_task)
  [>] [medium] Find implementation details (find_task)
  [ ] [low] Document findings (document_task)

## ◐ Implementation Phase (0/2 done)
  [ ] [high] Implement feature (impl_task)
  [ ] [medium] Add tests (test_task)
"#;

        let result = adapter.parse_markdown_todos(markdown).unwrap();

        assert_eq!(result.phases.len(), 2);

        // Check first phase
        let phase1 = &result.phases[0];
        assert_eq!(phase1.name, "Analysis Phase");
        assert_eq!(phase1.todos.len(), 3);

        assert_eq!(phase1.todos[0].status, TodoStatus::Completed);
        assert_eq!(phase1.todos[0].priority, TodoPriority::High);
        assert_eq!(phase1.todos[0].content, "Explore codebase structure");
        assert_eq!(phase1.todos[0].id, "explore_task");

        assert_eq!(phase1.todos[1].status, TodoStatus::InProgress);
        assert_eq!(phase1.todos[2].status, TodoStatus::Pending);

        // Check second phase
        let phase2 = &result.phases[1];
        assert_eq!(phase2.name, "Implementation Phase");
        assert_eq!(phase2.todos.len(), 2);
    }

    #[test]
    fn test_parse_json_todos() {
        let adapter = McpTodoAdapter::new(&[]);

        let json_output = serde_json::json!({
            "phases": [
                {
                    "id": "phase1",
                    "name": "Test Phase",
                    "todos": [
                        {
                            "id": "task1",
                            "content": "Test task",
                            "status": "pending",
                            "priority": "high"
                        }
                    ]
                }
            ]
        })
        .to_string();

        let result = adapter
            .parse_mcp_todo_output(&json_output, McpTodoToolType::Read)
            .unwrap();

        assert_eq!(result.phases.len(), 1);
        assert_eq!(result.phases[0].name, "Test Phase");
        assert_eq!(result.phases[0].todos.len(), 1);
        assert_eq!(result.phases[0].todos[0].content, "Test task");
    }
}
