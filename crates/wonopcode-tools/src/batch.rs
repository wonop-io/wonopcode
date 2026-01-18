//! Batch tool for executing multiple tools in parallel.
//!
//! This tool allows running multiple tool calls concurrently for efficiency.

use crate::{BoxedTool, Tool, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolResult};
use async_trait::async_trait;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::debug;

/// Maximum number of tool calls per batch.
const MAX_BATCH_SIZE: usize = 10;

/// Tools that cannot be batched.
const DISALLOWED_TOOLS: &[&str] = &["batch", "patch", "task"];

/// Batch tool for parallel execution of multiple tools.
pub struct BatchTool {
    registry: Arc<ToolRegistry>,
}

impl BatchTool {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

#[derive(Debug, Deserialize)]
struct BatchArgs {
    /// Array of tool calls to execute in parallel.
    tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    /// The name of the tool to execute.
    tool: String,
    /// Parameters for the tool.
    parameters: Value,
}

/// Result of a single tool call within the batch.
#[derive(Debug)]
struct BatchResult {
    tool: String,
    success: bool,
    output: Option<ToolOutput>,
    error: Option<String>,
}

#[async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        "batch"
    }

    fn description(&self) -> &str {
        r#"Execute multiple tools in parallel for efficiency.

Use this tool when you need to:
- Run multiple independent file reads
- Execute several grep/glob searches simultaneously
- Perform batch operations that don't depend on each other

Limitations:
- Maximum 10 tool calls per batch
- Cannot nest batch calls
- Some tools (patch, task) cannot be batched
- MCP tools must be called directly

Example:
{
  "tool_calls": [
    {"tool": "read", "parameters": {"file_path": "src/main.rs"}},
    {"tool": "read", "parameters": {"file_path": "src/lib.rs"}},
    {"tool": "glob", "parameters": {"pattern": "**/*.md"}}
  ]
}"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["tool_calls"],
            "properties": {
                "tool_calls": {
                    "type": "array",
                    "description": "Array of tool calls to execute in parallel",
                    "minItems": 1,
                    "maxItems": MAX_BATCH_SIZE,
                    "items": {
                        "type": "object",
                        "required": ["tool", "parameters"],
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "The name of the tool to execute"
                            },
                            "parameters": {
                                "type": "object",
                                "description": "Parameters for the tool"
                            }
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: BatchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        if args.tool_calls.is_empty() {
            return Err(ToolError::validation("tool_calls array cannot be empty"));
        }

        // Validate and collect tool calls
        let mut validated_calls: Vec<(String, BoxedTool, Value)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for (idx, call) in args.tool_calls.iter().enumerate() {
            // Check if tool is disallowed
            if DISALLOWED_TOOLS.contains(&call.tool.as_str()) {
                errors.push(format!(
                    "Call {}: Tool '{}' cannot be batched",
                    idx + 1,
                    call.tool
                ));
                continue;
            }

            // Check batch size limit
            if validated_calls.len() >= MAX_BATCH_SIZE {
                errors.push(format!(
                    "Call {}: Exceeds maximum batch size of {}",
                    idx + 1,
                    MAX_BATCH_SIZE
                ));
                continue;
            }

            // Look up tool
            match self.registry.get(&call.tool) {
                Some(tool) => {
                    validated_calls.push((
                        call.tool.clone(),
                        tool.clone(),
                        call.parameters.clone(),
                    ));
                }
                None => {
                    errors.push(format!("Call {}: Unknown tool '{}'", idx + 1, call.tool));
                }
            }
        }

        if validated_calls.is_empty() {
            return Err(ToolError::validation(format!(
                "No valid tool calls to execute. Errors:\n{}",
                errors.join("\n")
            )));
        }

        debug!(
            count = validated_calls.len(),
            "Executing batch of tool calls"
        );

        // Execute all tools in parallel
        let futures: Vec<_> = validated_calls
            .into_iter()
            .map(|(name, tool, params)| {
                let ctx = ctx.clone();
                let tool_name = name.clone();
                async move {
                    let _timing = wonopcode_util::TimingGuard::tool(&tool_name);
                    let result = tool.execute(params, &ctx).await;
                    match result {
                        Ok(output) => BatchResult {
                            tool: name,
                            success: true,
                            output: Some(output),
                            error: None,
                        },
                        Err(e) => BatchResult {
                            tool: name,
                            success: false,
                            output: None,
                            error: Some(e.to_string()),
                        },
                    }
                }
            })
            .collect();

        let results = join_all(futures).await;

        // Aggregate results
        let total = results.len();
        let successful: Vec<_> = results.iter().filter(|r| r.success).collect();
        let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();

        let mut output_parts: Vec<String> = Vec::new();
        let mut details: Vec<Value> = Vec::new();

        for result in &results {
            if result.success {
                if let Some(ref output) = result.output {
                    output_parts.push(format!(
                        "=== {} ===\n{}\n{}",
                        result.tool, output.title, output.output
                    ));
                    details.push(json!({
                        "tool": result.tool,
                        "success": true,
                        "title": output.title,
                        "metadata": output.metadata
                    }));
                }
            } else {
                output_parts.push(format!(
                    "=== {} (FAILED) ===\nError: {}",
                    result.tool,
                    result.error.as_deref().unwrap_or("Unknown error")
                ));
                details.push(json!({
                    "tool": result.tool,
                    "success": false,
                    "error": result.error
                }));
            }
        }

        // Add any validation errors
        for error in &errors {
            output_parts.push(format!("=== Validation Error ===\n{error}"));
        }

        let summary = if failed.is_empty() && errors.is_empty() {
            format!(
                "All {total} tools executed successfully.\n\nKeep using the batch tool when you need to run multiple independent operations."
            )
        } else {
            format!(
                "Executed {}/{} tools successfully. {} failed.",
                successful.len(),
                total,
                failed.len()
            )
        };

        let output = format!("{}\n\n{}", summary, output_parts.join("\n\n"));

        Ok(
            ToolOutput::new("Batch execution", output).with_metadata(json!({
                "total_calls": total,
                "successful": successful.len(),
                "failed": failed.len(),
                "tools": results.iter().map(|r| r.tool.as_str()).collect::<Vec<_>>(),
                "details": details
            })),
        )
    }
}

// We need ToolContext to be Clone for parallel execution
impl Clone for ToolContext {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            message_id: self.message_id.clone(),
            agent: self.agent.clone(),
            abort: self.abort.clone(),
            root_dir: self.root_dir.clone(),
            cwd: self.cwd.clone(),
            snapshot: self.snapshot.clone(),
            file_time: self.file_time.clone(),
            sandbox: self.sandbox.clone(),
            event_tx: self.event_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    // Mock tool that always succeeds
    struct MockSuccessTool {
        name: String,
    }

    impl MockSuccessTool {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    #[async_trait]
    impl Tool for MockSuccessTool {
        fn id(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "Mock tool for testing"
        }

        fn parameters_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "data": {"type": "string"}
                }
            })
        }

        async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
            let data = args["data"].as_str().unwrap_or("default");
            Ok(ToolOutput::new(
                format!("{} executed", self.name),
                format!("Processed: {}", data),
            ))
        }
    }

    // Mock tool that always fails
    struct MockFailureTool {
        name: String,
    }

    impl MockFailureTool {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    #[async_trait]
    impl Tool for MockFailureTool {
        fn id(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "Mock tool that fails"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
            Err(ToolError::execution_failed("Intentional failure"))
        }
    }

    fn test_context() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
        }
    }

    fn create_test_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockSuccessTool::new("tool1")));
        registry.register(Arc::new(MockSuccessTool::new("tool2")));
        registry.register(Arc::new(MockSuccessTool::new("tool3")));
        registry.register(Arc::new(MockFailureTool::new("failure_tool")));
        Arc::new(registry)
    }

    #[test]
    fn test_disallowed_tools() {
        assert!(DISALLOWED_TOOLS.contains(&"batch"));
        assert!(DISALLOWED_TOOLS.contains(&"patch"));
        assert!(DISALLOWED_TOOLS.contains(&"task"));
    }

    #[tokio::test]
    async fn test_batch_single_tool() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        let args = json!({
            "tool_calls": [
                {
                    "tool": "tool1",
                    "parameters": {"data": "test_value"}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await.unwrap();

        assert!(result.output.contains("All 1 tools executed successfully"));
        assert!(result.output.contains("tool1 executed"));
        assert!(result.output.contains("Processed: test_value"));
        assert_eq!(result.metadata["total_calls"], 1);
        assert_eq!(result.metadata["successful"], 1);
        assert_eq!(result.metadata["failed"], 0);
    }

    #[tokio::test]
    async fn test_batch_multiple_tools() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        let args = json!({
            "tool_calls": [
                {
                    "tool": "tool1",
                    "parameters": {"data": "first"}
                },
                {
                    "tool": "tool2",
                    "parameters": {"data": "second"}
                },
                {
                    "tool": "tool3",
                    "parameters": {"data": "third"}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await.unwrap();

        assert!(result.output.contains("All 3 tools executed successfully"));
        assert!(result.output.contains("tool1"));
        assert!(result.output.contains("tool2"));
        assert!(result.output.contains("tool3"));
        assert!(result.output.contains("Processed: first"));
        assert!(result.output.contains("Processed: second"));
        assert!(result.output.contains("Processed: third"));
        assert_eq!(result.metadata["total_calls"], 3);
        assert_eq!(result.metadata["successful"], 3);
        assert_eq!(result.metadata["failed"], 0);
    }

    #[tokio::test]
    async fn test_batch_tool_error_handling() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        let args = json!({
            "tool_calls": [
                {
                    "tool": "tool1",
                    "parameters": {"data": "success"}
                },
                {
                    "tool": "failure_tool",
                    "parameters": {}
                },
                {
                    "tool": "tool2",
                    "parameters": {"data": "also_success"}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await.unwrap();

        // Should report partial success
        assert!(result
            .output
            .contains("Executed 2/3 tools successfully. 1 failed"));
        assert!(result.output.contains("tool1"));
        assert!(result.output.contains("tool2"));
        assert!(result.output.contains("failure_tool (FAILED)"));
        assert!(result.output.contains("Intentional failure"));
        assert_eq!(result.metadata["total_calls"], 3);
        assert_eq!(result.metadata["successful"], 2);
        assert_eq!(result.metadata["failed"], 1);

        // Verify details metadata
        let details = result.metadata["details"].as_array().unwrap();
        assert_eq!(details.len(), 3);

        let success_count = details
            .iter()
            .filter(|d| d["success"].as_bool().unwrap_or(false))
            .count();
        let failure_count = details
            .iter()
            .filter(|d| !d["success"].as_bool().unwrap_or(true))
            .count();
        assert_eq!(success_count, 2);
        assert_eq!(failure_count, 1);
    }

    #[tokio::test]
    async fn test_batch_empty_tools() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        let args = json!({
            "tool_calls": []
        });

        let result = batch_tool.execute(args, &test_context()).await;

        assert!(result.is_err());
        match result {
            Err(ToolError::Validation(msg)) => {
                assert!(msg.contains("tool_calls array cannot be empty"));
            }
            _ => panic!("Expected validation error"),
        }
    }

    #[tokio::test]
    async fn test_batch_validation() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        // Test 1: Unknown tool
        let args = json!({
            "tool_calls": [
                {
                    "tool": "nonexistent_tool",
                    "parameters": {}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::Validation(msg)) => {
                assert!(msg.contains("Unknown tool 'nonexistent_tool'"));
            }
            _ => panic!("Expected validation error for unknown tool"),
        }

        // Test 2: Disallowed tool (batch)
        let args = json!({
            "tool_calls": [
                {
                    "tool": "batch",
                    "parameters": {}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::Validation(msg)) => {
                assert!(msg.contains("Tool 'batch' cannot be batched"));
            }
            _ => panic!("Expected validation error for disallowed tool"),
        }

        // Test 3: Disallowed tool (patch)
        let args = json!({
            "tool_calls": [
                {
                    "tool": "patch",
                    "parameters": {}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::Validation(msg)) => {
                assert!(msg.contains("Tool 'patch' cannot be batched"));
            }
            _ => panic!("Expected validation error for disallowed tool"),
        }

        // Test 4: Exceeds max batch size
        let mut tool_calls = Vec::new();
        for i in 0..15 {
            tool_calls.push(json!({
                "tool": "tool1",
                "parameters": {"data": format!("item_{}", i)}
            }));
        }

        let args = json!({
            "tool_calls": tool_calls
        });

        let result = batch_tool.execute(args, &test_context()).await.unwrap();

        // Only MAX_BATCH_SIZE tools should be executed
        assert_eq!(result.metadata["total_calls"], MAX_BATCH_SIZE);
        assert!(result.output.contains("Validation Error"));
        assert!(result.output.contains("Exceeds maximum batch size"));
    }

    #[tokio::test]
    async fn test_batch_invalid_arguments() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        // Test with missing tool_calls field
        let args = json!({
            "wrong_field": []
        });

        let result = batch_tool.execute(args, &test_context()).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::Validation(msg)) => {
                assert!(msg.contains("Invalid arguments"));
            }
            _ => panic!("Expected validation error for invalid arguments"),
        }
    }

    #[tokio::test]
    async fn test_batch_metadata_structure() {
        let registry = create_test_registry();
        let batch_tool = BatchTool::new(registry);

        let args = json!({
            "tool_calls": [
                {
                    "tool": "tool1",
                    "parameters": {"data": "test1"}
                },
                {
                    "tool": "tool2",
                    "parameters": {"data": "test2"}
                }
            ]
        });

        let result = batch_tool.execute(args, &test_context()).await.unwrap();

        // Verify metadata structure
        assert!(result.metadata.is_object());
        assert!(result.metadata["total_calls"].is_number());
        assert!(result.metadata["successful"].is_number());
        assert!(result.metadata["failed"].is_number());
        assert!(result.metadata["tools"].is_array());
        assert!(result.metadata["details"].is_array());

        let tools = result.metadata["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].as_str().unwrap(), "tool1");
        assert_eq!(tools[1].as_str().unwrap(), "tool2");

        let details = result.metadata["details"].as_array().unwrap();
        assert_eq!(details.len(), 2);
        for detail in details {
            assert!(detail["tool"].is_string());
            assert!(detail["success"].is_boolean());
            if detail["success"].as_bool().unwrap() {
                assert!(detail["title"].is_string());
            }
        }
    }
}
