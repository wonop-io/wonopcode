//! MCP tool wrapper for wonopcode.
//!
//! This module wraps MCP (Model Context Protocol) tools as native wonopcode tools,
//! allowing the AI to call tools from connected MCP servers.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use wonopcode_mcp::{McpClient, McpTool as McpToolDef, ToolCallResult, ToolContent};

/// A wrapper that makes an MCP tool available as a wonopcode tool.
pub struct McpToolWrapper {
    /// The MCP client.
    client: Arc<McpClient>,
    /// The tool definition from the MCP server.
    tool_def: McpToolDef,
    /// Tool ID (may be prefixed with server name).
    tool_id: String,
}

impl McpToolWrapper {
    /// Create a new MCP tool wrapper.
    pub fn new(client: Arc<McpClient>, tool_def: McpToolDef, prefix: Option<&str>) -> Self {
        let tool_id = if let Some(p) = prefix {
            format!("{}_{}", p, tool_def.name)
        } else {
            tool_def.name.clone()
        };

        Self {
            client,
            tool_def,
            tool_id,
        }
    }

    /// Get the underlying MCP tool definition.
    pub fn tool_def(&self) -> &McpToolDef {
        &self.tool_def
    }

    /// Convert MCP tool call result to wonopcode ToolOutput.
    fn convert_result(&self, result: ToolCallResult) -> ToolResult<ToolOutput> {
        // Collect text content from the result
        let mut output_parts = Vec::new();
        for content in &result.content {
            match content {
                ToolContent::Text { text } => {
                    output_parts.push(text.clone());
                }
                ToolContent::Image { data, mime_type } => {
                    output_parts.push(format!(
                        "[Image: {} bytes, type: {}]",
                        data.len(),
                        mime_type
                    ));
                }
                ToolContent::Resource { resource } => {
                    if let Some(text) = &resource.text {
                        output_parts.push(text.clone());
                    } else {
                        output_parts.push(format!("[Resource: {}]", resource.uri));
                    }
                }
            }
        }

        let output_text = output_parts.join("\n");

        if result.is_error {
            Err(ToolError::execution_failed(output_text))
        } else {
            Ok(ToolOutput::new(
                format!("MCP: {}", self.tool_def.name),
                output_text,
            ))
        }
    }
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn id(&self) -> &str {
        &self.tool_id
    }

    fn description(&self) -> &str {
        self.tool_def.description.as_deref().unwrap_or("MCP tool")
    }

    fn parameters_schema(&self) -> Value {
        self.tool_def.input_schema.clone().unwrap_or_else(|| {
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            })
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
        // Call the MCP tool
        let result = self
            .client
            .call_tool(&self.tool_def.name, args)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        self.convert_result(result)
    }
}

/// Builder for registering MCP tools.
pub struct McpToolsBuilder {
    client: Arc<McpClient>,
    prefix: Option<String>,
}

impl McpToolsBuilder {
    /// Create a new builder.
    pub fn new(client: Arc<McpClient>) -> Self {
        Self {
            client,
            prefix: None,
        }
    }

    /// Set a prefix for all tool IDs (e.g., server name).
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Build tool wrappers for all tools from the MCP client.
    pub async fn build_all(&self) -> Vec<Arc<dyn Tool>> {
        let tools = self.client.list_tools().await;
        tools
            .into_iter()
            .map(|tool_def| {
                Arc::new(McpToolWrapper::new(
                    self.client.clone(),
                    tool_def,
                    self.prefix.as_deref(),
                )) as Arc<dyn Tool>
            })
            .collect()
    }

    /// Build tool wrappers for tools from a specific server.
    pub async fn build_from_server(&self, server_name: &str) -> Result<Vec<Arc<dyn Tool>>, String> {
        let tools = self
            .client
            .list_tools_from_server(server_name)
            .await
            .map_err(|e| e.to_string())?;

        Ok(tools
            .into_iter()
            .map(|tool_def| {
                Arc::new(McpToolWrapper::new(
                    self.client.clone(),
                    tool_def,
                    self.prefix.as_deref(),
                )) as Arc<dyn Tool>
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_id_with_prefix() {
        let tool_def = McpToolDef {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, Some("filesystem"));

        assert_eq!(wrapper.id(), "filesystem_read_file");
    }

    #[test]
    fn test_tool_id_without_prefix() {
        let tool_def = McpToolDef {
            name: "read_file".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        assert_eq!(wrapper.id(), "read_file");
    }
}
