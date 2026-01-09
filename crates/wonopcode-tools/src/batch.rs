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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disallowed_tools() {
        assert!(DISALLOWED_TOOLS.contains(&"batch"));
        assert!(DISALLOWED_TOOLS.contains(&"patch"));
        assert!(DISALLOWED_TOOLS.contains(&"task"));
    }
}
