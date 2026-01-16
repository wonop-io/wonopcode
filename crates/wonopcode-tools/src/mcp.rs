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
    use wonopcode_mcp::protocol::ResourceContent;

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

    #[test]
    fn test_description_with_description() {
        let tool_def = McpToolDef {
            name: "read_file".to_string(),
            description: Some("Read a file from disk".to_string()),
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);
        assert_eq!(wrapper.description(), "Read a file from disk");
    }

    #[test]
    fn test_description_without_description() {
        let tool_def = McpToolDef {
            name: "read_file".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);
        assert_eq!(wrapper.description(), "MCP tool");
    }

    #[test]
    fn test_parameters_schema_with_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        });

        let tool_def = McpToolDef {
            name: "read_file".to_string(),
            description: None,
            input_schema: Some(schema.clone()),
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);
        assert_eq!(wrapper.parameters_schema(), schema);
    }

    #[test]
    fn test_parameters_schema_without_schema() {
        let tool_def = McpToolDef {
            name: "read_file".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);
        let schema = wrapper.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["additionalProperties"].as_bool().unwrap());
    }

    #[test]
    fn test_tool_def_accessor() {
        let tool_def = McpToolDef {
            name: "test_tool".to_string(),
            description: Some("Test".to_string()),
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def.clone(), None);
        assert_eq!(wrapper.tool_def().name, "test_tool");
    }

    #[test]
    fn test_convert_result_text_content() {
        let tool_def = McpToolDef {
            name: "test".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        let result = ToolCallResult {
            content: vec![ToolContent::Text {
                text: "Hello, world!".to_string(),
            }],
            is_error: false,
        };

        let output = wrapper.convert_result(result).unwrap();
        assert!(output.output.contains("Hello, world!"));
    }

    #[test]
    fn test_convert_result_image_content() {
        let tool_def = McpToolDef {
            name: "test".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        let result = ToolCallResult {
            content: vec![ToolContent::Image {
                data: "base64data".to_string(),
                mime_type: "image/png".to_string(),
            }],
            is_error: false,
        };

        let output = wrapper.convert_result(result).unwrap();
        assert!(output.output.contains("[Image:"));
        assert!(output.output.contains("image/png"));
    }

    #[test]
    fn test_convert_result_resource_with_text() {
        let tool_def = McpToolDef {
            name: "test".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        let result = ToolCallResult {
            content: vec![ToolContent::Resource {
                resource: ResourceContent {
                    uri: "file:///path/to/file".to_string(),
                    text: Some("File content here".to_string()),
                    blob: None,
                    mime_type: None,
                },
            }],
            is_error: false,
        };

        let output = wrapper.convert_result(result).unwrap();
        assert!(output.output.contains("File content here"));
    }

    #[test]
    fn test_convert_result_resource_without_text() {
        let tool_def = McpToolDef {
            name: "test".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        let result = ToolCallResult {
            content: vec![ToolContent::Resource {
                resource: ResourceContent {
                    uri: "file:///path/to/file".to_string(),
                    text: None,
                    blob: Some("base64blob".to_string()),
                    mime_type: None,
                },
            }],
            is_error: false,
        };

        let output = wrapper.convert_result(result).unwrap();
        assert!(output.output.contains("[Resource:"));
        assert!(output.output.contains("file:///path/to/file"));
    }

    #[test]
    fn test_convert_result_error() {
        let tool_def = McpToolDef {
            name: "test".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        let result = ToolCallResult {
            content: vec![ToolContent::Text {
                text: "Error occurred".to_string(),
            }],
            is_error: true,
        };

        let output = wrapper.convert_result(result);
        assert!(output.is_err());
        let err = output.unwrap_err().to_string();
        assert!(err.contains("Error occurred"));
    }

    #[test]
    fn test_convert_result_multiple_content() {
        let tool_def = McpToolDef {
            name: "test".to_string(),
            description: None,
            input_schema: None,
        };

        let wrapper = McpToolWrapper::new(Arc::new(McpClient::new()), tool_def, None);

        let result = ToolCallResult {
            content: vec![
                ToolContent::Text {
                    text: "Line 1".to_string(),
                },
                ToolContent::Text {
                    text: "Line 2".to_string(),
                },
            ],
            is_error: false,
        };

        let output = wrapper.convert_result(result).unwrap();
        assert!(output.output.contains("Line 1"));
        assert!(output.output.contains("Line 2"));
    }

    #[test]
    fn test_mcp_tools_builder_new() {
        let client = Arc::new(McpClient::new());
        let builder = McpToolsBuilder::new(client);
        assert!(builder.prefix.is_none());
    }

    #[test]
    fn test_mcp_tools_builder_with_prefix() {
        let client = Arc::new(McpClient::new());
        let builder = McpToolsBuilder::new(client).with_prefix("server");
        assert_eq!(builder.prefix, Some("server".to_string()));
    }

    #[tokio::test]
    async fn test_mcp_tools_builder_build_all_empty() {
        let client = Arc::new(McpClient::new());
        let builder = McpToolsBuilder::new(client);
        let tools = builder.build_all().await;
        assert!(tools.is_empty()); // No servers connected
    }
}
