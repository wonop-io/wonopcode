//! Web search and code search tools using Exa AI API.
//!
//! These tools provide web search and code context retrieval capabilities
//! via the Exa AI MCP API.

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::{debug, warn};

const EXA_MCP_URL: &str = "https://mcp.exa.ai/mcp";
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Web search tool using Exa AI.
pub struct WebSearchTool {
    client: Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct WebSearchArgs {
    /// Search query.
    query: String,
    /// Number of results to return (default: 8).
    #[serde(default = "default_num_results")]
    num_results: u32,
    /// Livecrawl mode: "fallback" or "preferred".
    #[serde(default = "default_livecrawl")]
    livecrawl: String,
    /// Search type: "auto", "fast", or "deep".
    #[serde(default = "default_search_type")]
    search_type: String,
    /// Maximum characters for context.
    context_max_characters: Option<u32>,
}

fn default_num_results() -> u32 {
    8
}

fn default_livecrawl() -> String {
    "fallback".to_string()
}

fn default_search_type() -> String {
    "auto".to_string()
}

#[async_trait]
impl Tool for WebSearchTool {
    fn id(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        r#"Search the web for information using Exa AI.

Use this tool when you need to:
- Find documentation for libraries or frameworks
- Look up current information that may not be in your training data
- Research best practices or tutorials
- Find solutions to specific technical problems

The search is optimized for technical and programming-related queries."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of search results to return (default: 8)",
                    "default": 8,
                    "minimum": 1,
                    "maximum": 20
                },
                "livecrawl": {
                    "type": "string",
                    "enum": ["fallback", "preferred"],
                    "description": "Livecrawl mode - 'fallback' uses cached results when available, 'preferred' always fetches fresh content",
                    "default": "fallback"
                },
                "search_type": {
                    "type": "string",
                    "enum": ["auto", "fast", "deep"],
                    "description": "Search type - 'auto' balances speed and quality, 'fast' prioritizes speed, 'deep' prioritizes comprehensiveness",
                    "default": "auto"
                },
                "context_max_characters": {
                    "type": "integer",
                    "description": "Maximum characters for context per result",
                    "minimum": 100,
                    "maximum": 10000
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: WebSearchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        debug!(query = %args.query, "Executing web search");

        // Build arguments, only including contextMaxCharacters if provided
        let mut arguments = json!({
            "query": args.query,
            "numResults": args.num_results,
            "livecrawl": args.livecrawl,
            "type": args.search_type
        });
        if let Some(max_chars) = args.context_max_characters {
            arguments["contextMaxCharacters"] = json!(max_chars);
        }

        // Build MCP JSON-RPC request
        let request = McpRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tools/call",
            params: McpToolCall {
                name: "web_search_exa",
                arguments,
            },
        };

        let response = call_exa_api(&self.client, &request, ctx, DEFAULT_TIMEOUT_SECS).await?;

        Ok(
            ToolOutput::new(format!("Web search: {}", args.query), response).with_metadata(json!({
                "query": args.query,
                "num_results": args.num_results,
                "search_type": args.search_type
            })),
        )
    }
}

/// Code search tool using Exa AI.
pub struct CodeSearchTool {
    client: Client,
}

impl CodeSearchTool {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl Default for CodeSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct CodeSearchArgs {
    /// Search query for code context.
    query: String,
    /// Number of tokens for response (default: 5000).
    #[serde(default = "default_tokens_num")]
    tokens_num: u32,
}

fn default_tokens_num() -> u32 {
    5000
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn id(&self) -> &str {
        "codesearch"
    }

    fn description(&self) -> &str {
        r#"Search for code examples, API documentation, and programming context using Exa AI.

Use this tool when you need to:
- Find code examples for specific libraries or APIs
- Look up SDK documentation and usage patterns
- Research how to implement specific functionality
- Find programming tutorials and guides

This tool is optimized for code-related queries and returns programming context."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for APIs, libraries, and SDKs"
                },
                "tokens_num": {
                    "type": "integer",
                    "description": "Number of tokens for the response (default: 5000)",
                    "default": 5000,
                    "minimum": 1000,
                    "maximum": 50000
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: CodeSearchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        debug!(query = %args.query, "Executing code search");

        // Build MCP JSON-RPC request
        let request = McpRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tools/call",
            params: McpToolCall {
                name: "get_code_context_exa",
                arguments: json!({
                    "query": args.query,
                    "tokensNum": args.tokens_num
                }),
            },
        };

        let response = call_exa_api(&self.client, &request, ctx, DEFAULT_TIMEOUT_SECS).await?;

        Ok(
            ToolOutput::new(format!("Code search: {}", args.query), response).with_metadata(
                json!({
                    "query": args.query,
                    "tokens_num": args.tokens_num
                }),
            ),
        )
    }
}

// MCP JSON-RPC request types
#[derive(Debug, Serialize)]
struct McpRequest<'a> {
    jsonrpc: &'a str,
    id: u32,
    method: &'a str,
    params: McpToolCall<'a>,
}

#[derive(Debug, Serialize)]
struct McpToolCall<'a> {
    name: &'a str,
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct McpResponse {
    result: Option<McpResult>,
    error: Option<McpError>,
}

#[derive(Debug, Deserialize)]
struct McpResult {
    content: Vec<McpContent>,
}

#[derive(Debug, Deserialize)]
struct McpContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpError {
    message: String,
    /// Error code from MCP API (present in response but not used).
    #[serde(default)]
    _code: Option<i32>,
}

/// Call the Exa MCP API.
async fn call_exa_api(
    client: &Client,
    request: &McpRequest<'_>,
    ctx: &ToolContext,
    timeout_secs: u64,
) -> ToolResult<String> {
    // Check for cancellation
    if ctx.abort.is_cancelled() {
        return Err(ToolError::Cancelled);
    }

    let response = client
        .post(EXA_MCP_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream, application/json")
        .timeout(Duration::from_secs(timeout_secs))
        .json(request)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ToolError::Timeout(Duration::from_secs(timeout_secs))
            } else {
                ToolError::execution_failed(format!("HTTP request failed: {e}"))
            }
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ToolError::execution_failed(format!(
            "Exa API returned status {status}: {body}"
        )));
    }

    // Parse SSE response
    let body = response
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response body: {e}")))?;

    parse_sse_response(&body)
}

/// Parse Server-Sent Events response from Exa API.
fn parse_sse_response(body: &str) -> ToolResult<String> {
    // SSE format: each message starts with "data: " followed by JSON
    for line in body.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                continue;
            }

            match serde_json::from_str::<McpResponse>(data) {
                Ok(response) => {
                    if let Some(error) = response.error {
                        return Err(ToolError::execution_failed(format!(
                            "Exa API error: {}",
                            error.message
                        )));
                    }

                    if let Some(result) = response.result {
                        // Extract text content
                        for content in result.content {
                            if content.content_type == "text" {
                                if let Some(text) = content.text {
                                    return Ok(text);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse SSE data: {}", e);
                    continue;
                }
            }
        }
    }

    // If no SSE format, try parsing as direct JSON
    if let Ok(response) = serde_json::from_str::<McpResponse>(body) {
        if let Some(error) = response.error {
            return Err(ToolError::execution_failed(format!(
                "Exa API error: {}",
                error.message
            )));
        }

        if let Some(result) = response.result {
            for content in result.content {
                if content.content_type == "text" {
                    if let Some(text) = content.text {
                        return Ok(text);
                    }
                }
            }
        }
    }

    Ok("No results found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_response() {
        let sse_data = r#"data: {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Search results here"}]}}
data: [DONE]"#;

        let result = parse_sse_response(sse_data).unwrap();
        assert_eq!(result, "Search results here");
    }

    #[test]
    fn test_parse_sse_error() {
        let sse_data =
            r#"data: {"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"API error"}}"#;

        let result = parse_sse_response(sse_data);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("API error"));
    }

    #[test]
    fn test_parse_direct_json() {
        let json_data = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Direct result"}]}}"#;

        let result = parse_sse_response(json_data).unwrap();
        assert_eq!(result, "Direct result");
    }

    #[test]
    fn test_parse_direct_json_error() {
        let json_data =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Direct error"}}"#;

        let result = parse_sse_response(json_data);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Direct error"));
    }

    #[test]
    fn test_parse_sse_no_results() {
        let sse_data = r#"data: {"jsonrpc":"2.0","id":1,"result":{"content":[]}}"#;

        let result = parse_sse_response(sse_data).unwrap();
        assert_eq!(result, "No results found");
    }

    #[test]
    fn test_parse_sse_skips_done() {
        let sse_data = r#"data: [DONE]
data: {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"After done"}]}}"#;

        let result = parse_sse_response(sse_data).unwrap();
        assert_eq!(result, "After done");
    }

    #[test]
    fn test_parse_sse_skips_invalid_json() {
        let sse_data = r#"data: invalid json
data: {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Valid result"}]}}"#;

        let result = parse_sse_response(sse_data).unwrap();
        assert_eq!(result, "Valid result");
    }

    #[test]
    fn test_parse_sse_non_text_content_type() {
        let sse_data = r#"data: {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"image","data":"base64"}]}}"#;

        let result = parse_sse_response(sse_data).unwrap();
        assert_eq!(result, "No results found");
    }

    #[test]
    fn test_parse_sse_text_without_text_field() {
        let sse_data = r#"data: {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text"}]}}"#;

        let result = parse_sse_response(sse_data).unwrap();
        assert_eq!(result, "No results found");
    }

    #[test]
    fn test_parse_empty_body() {
        let result = parse_sse_response("").unwrap();
        assert_eq!(result, "No results found");
    }

    #[test]
    fn test_web_search_tool_new() {
        let tool = WebSearchTool::new();
        assert_eq!(tool.id(), "websearch");
    }

    #[test]
    fn test_web_search_tool_default() {
        let tool = WebSearchTool::default();
        assert_eq!(tool.id(), "websearch");
    }

    #[test]
    fn test_web_search_tool_description() {
        let tool = WebSearchTool::new();
        let desc = tool.description();
        assert!(desc.contains("Search the web"));
        assert!(desc.contains("Exa AI"));
    }

    #[test]
    fn test_web_search_tool_parameters_schema() {
        let tool = WebSearchTool::new();
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("query")));
        assert!(schema["properties"]["num_results"].get("default").is_some());
    }

    #[test]
    fn test_code_search_tool_new() {
        let tool = CodeSearchTool::new();
        assert_eq!(tool.id(), "codesearch");
    }

    #[test]
    fn test_code_search_tool_default() {
        let tool = CodeSearchTool::default();
        assert_eq!(tool.id(), "codesearch");
    }

    #[test]
    fn test_code_search_tool_description() {
        let tool = CodeSearchTool::new();
        let desc = tool.description();
        assert!(desc.contains("code examples"));
        assert!(desc.contains("API documentation"));
    }

    #[test]
    fn test_code_search_tool_parameters_schema() {
        let tool = CodeSearchTool::new();
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("query")));
        assert!(schema["properties"]["tokens_num"].get("default").is_some());
    }

    #[test]
    fn test_default_functions() {
        assert_eq!(default_num_results(), 8);
        assert_eq!(default_livecrawl(), "fallback");
        assert_eq!(default_search_type(), "auto");
        assert_eq!(default_tokens_num(), 5000);
    }

    #[test]
    fn test_web_search_args_deserialization() {
        let args: WebSearchArgs = serde_json::from_value(json!({
            "query": "rust programming"
        }))
        .unwrap();

        assert_eq!(args.query, "rust programming");
        assert_eq!(args.num_results, 8); // default
        assert_eq!(args.livecrawl, "fallback"); // default
        assert_eq!(args.search_type, "auto"); // default
        assert!(args.context_max_characters.is_none());
    }

    #[test]
    fn test_web_search_args_with_options() {
        let args: WebSearchArgs = serde_json::from_value(json!({
            "query": "rust programming",
            "num_results": 15,
            "livecrawl": "preferred",
            "search_type": "deep",
            "context_max_characters": 5000
        }))
        .unwrap();

        assert_eq!(args.query, "rust programming");
        assert_eq!(args.num_results, 15);
        assert_eq!(args.livecrawl, "preferred");
        assert_eq!(args.search_type, "deep");
        assert_eq!(args.context_max_characters, Some(5000));
    }

    #[test]
    fn test_code_search_args_deserialization() {
        let args: CodeSearchArgs = serde_json::from_value(json!({
            "query": "async rust"
        }))
        .unwrap();

        assert_eq!(args.query, "async rust");
        assert_eq!(args.tokens_num, 5000); // default
    }

    #[test]
    fn test_code_search_args_with_tokens() {
        let args: CodeSearchArgs = serde_json::from_value(json!({
            "query": "async rust",
            "tokens_num": 10000
        }))
        .unwrap();

        assert_eq!(args.query, "async rust");
        assert_eq!(args.tokens_num, 10000);
    }

    #[test]
    fn test_mcp_response_deserialization() {
        let response: McpResponse = serde_json::from_value(json!({
            "result": {
                "content": [
                    {"type": "text", "text": "Result text"}
                ]
            }
        }))
        .unwrap();

        assert!(response.result.is_some());
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].content_type, "text");
        assert_eq!(result.content[0].text, Some("Result text".to_string()));
    }

    #[test]
    fn test_mcp_error_deserialization() {
        let response: McpResponse = serde_json::from_value(json!({
            "error": {
                "code": -32000,
                "message": "Error occurred"
            }
        }))
        .unwrap();

        assert!(response.result.is_none());
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.message, "Error occurred");
    }
}
