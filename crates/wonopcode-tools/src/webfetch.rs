//! WebFetch tool - fetch content from URLs.
//!
//! Fetches web content and returns it in various formats:
//! - text: Plain text
//! - markdown: HTML converted to markdown
//! - html: Raw HTML

// Allow identical blocks for HTML entity handling (e.g., &nbsp; and &#160; both map to space)
#![allow(clippy::if_same_then_else)]

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tracing::debug;
use url::Url;

/// Default timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum timeout in seconds.
const MAX_TIMEOUT_SECS: u64 = 120;

/// Maximum response size in bytes (5MB).
const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024;

/// WebFetch tool for fetching URL content.
pub struct WebFetchTool;

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
    #[serde(default = "default_format")]
    format: String,
    #[serde(default)]
    timeout: Option<u64>,
}

fn default_format() -> String {
    "text".to_string()
}

#[async_trait]
impl Tool for WebFetchTool {
    fn id(&self) -> &str {
        "webfetch"
    }

    fn description(&self) -> &str {
        r#"Fetches content from a specified URL.

Usage:
- The URL must be a fully-formed valid URL.
- HTTP URLs will be automatically upgraded to HTTPS.
- Returns content in the specified format (text, markdown, or html).
- Results may be summarized if content is very large."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["url", "format"],
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch content from"
                },
                "format": {
                    "type": "string",
                    "enum": ["text", "markdown", "html"],
                    "description": "The format to return the content in"
                },
                "timeout": {
                    "type": "number",
                    "description": "Optional timeout in seconds (max 120)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: WebFetchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Parse and validate URL
        let mut url = Url::parse(&args.url)
            .map_err(|e| ToolError::validation(format!("Invalid URL: {e}")))?;

        // Upgrade HTTP to HTTPS
        if url.scheme() == "http" {
            url.set_scheme("https").ok();
        }

        // Validate scheme
        if url.scheme() != "https" {
            return Err(ToolError::validation(format!(
                "Only HTTPS URLs are supported, got: {}",
                url.scheme()
            )));
        }

        // Calculate timeout
        let timeout_secs = args
            .timeout
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);

        debug!(url = %url, format = %args.format, "Fetching URL");

        // Build HTTP client
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("wonopcode/0.1")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| {
                ToolError::execution_failed(format!("Failed to create HTTP client: {e}"))
            })?;

        // Fetch the URL
        let response = client.get(url.as_str()).send().await.map_err(|e| {
            if e.is_timeout() {
                ToolError::execution_failed(format!("Request timed out after {timeout_secs}s"))
            } else if e.is_redirect() {
                ToolError::execution_failed("Too many redirects")
            } else {
                ToolError::execution_failed(format!("Request failed: {e}"))
            }
        })?;

        // Check status
        let status = response.status();
        if !status.is_success() {
            return Err(ToolError::execution_failed(format!(
                "HTTP {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Check for redirects to different host
        let final_url = response.url().clone();
        if final_url.host() != url.host() {
            return Ok(ToolOutput::new(
                format!("Redirect to {final_url}"),
                format!("The URL redirected to a different host: {final_url}\nPlease make a new request with this URL."),
            ).with_metadata(json!({
                "redirect": true,
                "original_url": url.to_string(),
                "final_url": final_url.to_string()
            })));
        }

        // Get content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();

        // Read body with size limit
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;

        if bytes.len() > MAX_RESPONSE_SIZE {
            return Err(ToolError::execution_failed(format!(
                "Response too large: {} bytes (max {} bytes)",
                bytes.len(),
                MAX_RESPONSE_SIZE
            )));
        }

        // Convert to string
        let text = String::from_utf8_lossy(&bytes).to_string();

        // Format the content
        let content = match args.format.as_str() {
            "html" => text,
            "text" => {
                if content_type.contains("html") {
                    html_to_text(&text)
                } else {
                    text
                }
            }
            "markdown" => {
                if content_type.contains("html") {
                    html_to_markdown(&text)
                } else {
                    text
                }
            }
            _ => text,
        };

        // Truncate if too long
        let (content, truncated) = truncate_content(&content, 50000);

        Ok(
            ToolOutput::new(format!("Fetched {url}"), content).with_metadata(json!({
                "url": url.to_string(),
                "content_type": content_type,
                "size": bytes.len(),
                "truncated": truncated
            })),
        )
    }
}

/// Convert HTML to plain text.
fn html_to_text(html: &str) -> String {
    // Simple HTML stripping - remove all tags
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_space = false;

    let html_lower = html.to_lowercase();
    let mut i = 0;
    let chars: Vec<char> = html.chars().collect();

    while i < chars.len() {
        let ch = chars[i];

        // Check for script/style start
        if !in_tag && ch == '<' {
            let remaining = &html_lower[i..];
            if remaining.starts_with("<script") {
                in_script = true;
            } else if remaining.starts_with("<style") {
                in_style = true;
            } else if remaining.starts_with("</script") {
                in_script = false;
            } else if remaining.starts_with("</style") {
                in_style = false;
            }
            in_tag = true;
            i += 1;
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            i += 1;
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        // Handle HTML entities
        if ch == '&' {
            let remaining: String = chars[i..].iter().take(10).collect();
            if remaining.starts_with("&nbsp;") || remaining.starts_with("&#160;") {
                result.push(' ');
                i += 6; // Both &nbsp; and &#160; are 6 chars
                last_was_space = true;
                continue;
            } else if remaining.starts_with("&lt;") {
                result.push('<');
                i += 4;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&gt;") {
                result.push('>');
                i += 4;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&amp;") {
                result.push('&');
                i += 5;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&quot;") {
                result.push('"');
                i += 6;
                last_was_space = false;
                continue;
            }
        }

        // Normalize whitespace
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(if ch == '\n' { '\n' } else { ' ' });
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }

        i += 1;
    }

    // Clean up multiple newlines
    let mut final_result = String::new();
    let mut newline_count = 0;

    for ch in result.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                final_result.push(ch);
            }
        } else {
            newline_count = 0;
            final_result.push(ch);
        }
    }

    final_result.trim().to_string()
}

/// Convert HTML to Markdown.
#[allow(clippy::cognitive_complexity)]
fn html_to_markdown(html: &str) -> String {
    // Start with text conversion
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut current_tag = String::new();
    let mut in_script = false;
    let mut in_style = false;
    let mut _in_code = false;
    let mut _in_pre = false;
    let mut list_depth: usize = 0;

    let _html_lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '<' {
            in_tag = true;
            current_tag.clear();
            i += 1;
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
                let tag = current_tag.to_lowercase();

                // Handle different tags
                if tag.starts_with("script") {
                    in_script = true;
                } else if tag == "/script" {
                    in_script = false;
                } else if tag.starts_with("style") {
                    in_style = true;
                } else if tag == "/style" {
                    in_style = false;
                } else if tag.starts_with("code") {
                    result.push('`');
                    _in_code = true;
                } else if tag == "/code" {
                    result.push('`');
                    _in_code = false;
                } else if tag.starts_with("pre") {
                    result.push_str("\n```\n");
                    _in_pre = true;
                } else if tag == "/pre" {
                    result.push_str("\n```\n");
                    _in_pre = false;
                } else if tag.starts_with("h1") {
                    result.push_str("\n# ");
                } else if tag.starts_with("h2") {
                    result.push_str("\n## ");
                } else if tag.starts_with("h3") {
                    result.push_str("\n### ");
                } else if tag.starts_with("h4") {
                    result.push_str("\n#### ");
                } else if tag.starts_with("h5") || tag.starts_with("h6") {
                    result.push_str("\n##### ");
                } else if tag == "/h1"
                    || tag == "/h2"
                    || tag == "/h3"
                    || tag == "/h4"
                    || tag == "/h5"
                    || tag == "/h6"
                {
                    result.push('\n');
                } else if tag.starts_with("p") || tag == "br" || tag == "br/" {
                    result.push_str("\n\n");
                } else if tag == "/p" {
                    result.push('\n');
                } else if tag.starts_with("strong") || tag.starts_with("b ") || tag == "b" {
                    result.push_str("**");
                } else if tag == "/strong" || tag == "/b" {
                    result.push_str("**");
                } else if tag.starts_with("em") || tag.starts_with("i ") || tag == "i" {
                    result.push('*');
                } else if tag == "/em" || tag == "/i" {
                    result.push('*');
                } else if tag.starts_with("ul") || tag.starts_with("ol") {
                    list_depth += 1;
                    result.push('\n');
                } else if tag == "/ul" || tag == "/ol" {
                    list_depth = list_depth.saturating_sub(1);
                    result.push('\n');
                } else if tag.starts_with("li") {
                    result.push('\n');
                    for _ in 0..list_depth.saturating_sub(1) {
                        result.push_str("  ");
                    }
                    result.push_str("- ");
                } else if tag == "/li" {
                    // Nothing needed
                } else if tag.starts_with("a ") {
                    result.push('[');
                } else if tag == "/a" {
                    result.push_str("](link)");
                }

                i += 1;
                continue;
            }
            current_tag.push(ch);
            i += 1;
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        // Handle HTML entities
        if ch == '&' {
            let remaining: String = chars[i..].iter().take(10).collect();
            if remaining.starts_with("&nbsp;") {
                result.push(' ');
                i += 6;
                continue;
            } else if remaining.starts_with("&lt;") {
                result.push('<');
                i += 4;
                continue;
            } else if remaining.starts_with("&gt;") {
                result.push('>');
                i += 4;
                continue;
            } else if remaining.starts_with("&amp;") {
                result.push('&');
                i += 5;
                continue;
            } else if remaining.starts_with("&quot;") {
                result.push('"');
                i += 6;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    // Clean up excessive whitespace
    let mut final_result = String::new();
    let mut newline_count = 0;

    for ch in result.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                final_result.push(ch);
            }
        } else if ch.is_whitespace() {
            if !final_result.ends_with(' ') && !final_result.ends_with('\n') {
                final_result.push(' ');
            }
            newline_count = 0;
        } else {
            newline_count = 0;
            final_result.push(ch);
        }
    }

    final_result.trim().to_string()
}

/// Truncate content if too long.
fn truncate_content(content: &str, max_len: usize) -> (String, bool) {
    if content.len() <= max_len {
        return (content.to_string(), false);
    }

    let truncated = format!(
        "{}\n\n... [content truncated, showing first {} chars of {}] ...",
        &content[..max_len],
        max_len,
        content.len()
    );

    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    fn create_test_context() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: std::path::PathBuf::from("/test"),
            cwd: std::path::PathBuf::from("/test"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
        }
    }

    #[test]
    fn test_html_to_text() {
        let html = "<html><body><h1>Title</h1><p>Hello <b>world</b>!</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains("<"));
    }

    #[test]
    fn test_html_to_text_script_removal() {
        let html = "<p>Hello</p><script>alert('evil');</script><p>World</p>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_html_to_markdown() {
        let html = "<h1>Title</h1><p>Hello <strong>world</strong>!</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("**world**"));
    }

    #[test]
    fn test_html_entities() {
        let html = "&lt;hello&gt; &amp; &quot;world&quot;";
        let text = html_to_text(html);
        assert!(text.contains("<hello>"));
        assert!(text.contains("& \"world\""));
    }

    #[test]
    fn test_truncate_content() {
        let short = "hello";
        let (result, truncated) = truncate_content(short, 100);
        assert_eq!(result, "hello");
        assert!(!truncated);

        let long = "x".repeat(1000);
        let (result, truncated) = truncate_content(&long, 100);
        assert!(result.len() < long.len());
        assert!(truncated);
    }

    #[test]
    fn test_webfetch_tool_id() {
        let tool = WebFetchTool;
        assert_eq!(tool.id(), "webfetch");
    }

    #[test]
    fn test_webfetch_tool_description() {
        let tool = WebFetchTool;
        let desc = tool.description();
        assert!(desc.contains("Fetches content"));
        assert!(desc.contains("URL"));
        assert!(desc.contains("HTTPS"));
    }

    #[test]
    fn test_webfetch_tool_parameters_schema() {
        let tool = WebFetchTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("url")));
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("format")));
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["format"].is_object());
        assert!(schema["properties"]["timeout"].is_object());
    }

    #[test]
    fn test_default_format() {
        assert_eq!(default_format(), "text");
    }

    #[test]
    fn test_webfetch_args_deserialization() {
        let args: WebFetchArgs = serde_json::from_value(json!({
            "url": "https://example.com",
            "format": "markdown"
        }))
        .unwrap();
        assert_eq!(args.url, "https://example.com");
        assert_eq!(args.format, "markdown");
        assert!(args.timeout.is_none());
    }

    #[test]
    fn test_webfetch_args_with_timeout() {
        let args: WebFetchArgs = serde_json::from_value(json!({
            "url": "https://example.com",
            "format": "html",
            "timeout": 60
        }))
        .unwrap();
        assert_eq!(args.url, "https://example.com");
        assert_eq!(args.format, "html");
        assert_eq!(args.timeout, Some(60));
    }

    #[test]
    fn test_webfetch_args_default_format() {
        let args: WebFetchArgs = serde_json::from_value(json!({
            "url": "https://example.com"
        }))
        .unwrap();
        assert_eq!(args.format, "text"); // default
    }

    #[tokio::test]
    async fn test_webfetch_invalid_url() {
        let tool = WebFetchTool;
        let ctx = create_test_context();
        let result = tool
            .execute(
                json!({
                    "url": "not-a-valid-url",
                    "format": "text"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn test_webfetch_invalid_scheme() {
        let tool = WebFetchTool;
        let ctx = create_test_context();
        let result = tool
            .execute(
                json!({
                    "url": "ftp://example.com/file.txt",
                    "format": "text"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("HTTPS URLs are supported"));
    }

    #[tokio::test]
    async fn test_webfetch_invalid_args() {
        let tool = WebFetchTool;
        let ctx = create_test_context();
        let result = tool
            .execute(
                json!({
                    "not_url": "something"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid arguments"));
    }

    #[test]
    fn test_html_to_text_style_removal() {
        let html = "<p>Hello</p><style>.cls { color: red; }</style><p>World</p>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("color"));
    }

    #[test]
    fn test_html_to_text_nbsp() {
        let html = "Hello&nbsp;World";
        let text = html_to_text(html);
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn test_html_to_text_numeric_entity() {
        let html = "Hello&#160;World";
        let text = html_to_text(html);
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn test_html_to_text_whitespace_normalization() {
        let html = "Hello    \t   World";
        let text = html_to_text(html);
        // Multiple whitespace should be collapsed
        assert!(!text.contains("    "));
    }

    #[test]
    fn test_html_to_text_multiple_newlines() {
        let html = "Line1\n\n\n\n\nLine2";
        let text = html_to_text(html);
        // Should not have more than 2 consecutive newlines
        assert!(!text.contains("\n\n\n"));
    }

    #[test]
    fn test_html_to_markdown_code() {
        let html = "<code>fn main()</code>";
        let md = html_to_markdown(html);
        assert!(md.contains("`fn main()`"));
    }

    #[test]
    fn test_html_to_markdown_pre() {
        let html = "<pre>code block</pre>";
        let md = html_to_markdown(html);
        assert!(md.contains("```"));
    }

    #[test]
    fn test_html_to_markdown_headings() {
        let html = "<h1>H1</h1><h2>H2</h2><h3>H3</h3><h4>H4</h4><h5>H5</h5><h6>H6</h6>";
        let md = html_to_markdown(html);
        assert!(md.contains("# H1"));
        assert!(md.contains("## H2"));
        assert!(md.contains("### H3"));
        assert!(md.contains("#### H4"));
        assert!(md.contains("##### H5"));
        assert!(md.contains("##### H6")); // H5 and H6 use same prefix
    }

    #[test]
    fn test_html_to_markdown_lists() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("- Item 1"));
        assert!(md.contains("- Item 2"));
    }

    #[test]
    fn test_html_to_markdown_nested_lists() {
        let html = "<ul><li>Outer<ul><li>Inner</li></ul></li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("- Outer"));
        assert!(md.contains("Inner"));
    }

    #[test]
    fn test_html_to_markdown_em() {
        let html = "<em>italic</em> and <i>also italic</i>";
        let md = html_to_markdown(html);
        assert!(md.contains("*italic*"));
        assert!(md.contains("*also italic*"));
    }

    #[test]
    fn test_html_to_markdown_bold() {
        let html = "<b>bold</b>";
        let md = html_to_markdown(html);
        assert!(md.contains("**bold**"));
    }

    #[test]
    fn test_html_to_markdown_link() {
        let html = "<a href='https://example.com'>link text</a>";
        let md = html_to_markdown(html);
        assert!(md.contains("[link text]"));
    }

    #[test]
    fn test_html_to_markdown_br() {
        let html = "Line1<br>Line2<br/>Line3";
        let md = html_to_markdown(html);
        assert!(md.contains("Line1"));
        assert!(md.contains("Line2"));
        assert!(md.contains("Line3"));
    }

    #[test]
    fn test_html_to_markdown_script_removal() {
        let html = "<p>Hello</p><script>evil();</script><p>World</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Hello"));
        assert!(md.contains("World"));
        assert!(!md.contains("evil"));
    }

    #[test]
    fn test_html_to_markdown_style_removal() {
        let html = "<p>Hello</p><style>.x{}</style><p>World</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Hello"));
        assert!(md.contains("World"));
        assert!(!md.contains(".x"));
    }

    #[test]
    fn test_html_to_markdown_entities() {
        let html = "&lt;code&gt; &amp; &quot;test&quot;";
        let md = html_to_markdown(html);
        assert!(md.contains("<code>"));
        assert!(md.contains("&"));
        assert!(md.contains("\"test\""));
    }

    #[test]
    fn test_html_to_markdown_nbsp() {
        let html = "Hello&nbsp;World";
        let md = html_to_markdown(html);
        assert!(md.contains("Hello World"));
    }

    #[test]
    fn test_truncate_content_exact_limit() {
        let content = "x".repeat(100);
        let (result, truncated) = truncate_content(&content, 100);
        assert_eq!(result, content);
        assert!(!truncated);
    }

    #[test]
    fn test_truncate_content_just_over() {
        let content = "x".repeat(101);
        let (result, truncated) = truncate_content(&content, 100);
        assert!(result.len() > 100); // includes the truncation message
        assert!(result.contains("truncated"));
        assert!(truncated);
    }
}
