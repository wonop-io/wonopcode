//! Adapter to bridge WebFetchTool to codemode WebService trait.
//!
//! Implements:
//! - fetch: Uses WebFetchTool logic
//! - search: Returns "not available" (requires external API)
//! - code_search: Returns "not available" (requires external API)

use async_trait::async_trait;
use std::time::Duration;
use wonopcode_codemode::{
    WebService as CodemodeService,
    WebSearchResult,
    ServiceError, ServiceResult,
};
use tracing::debug;
use url::Url;

/// Maximum response size in bytes (5MB).
const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024;

/// Adapter that implements codemode WebService trait.
pub struct WebServiceAdapter;

impl WebServiceAdapter {
    /// Create a new WebServiceAdapter.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebServiceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CodemodeService for WebServiceAdapter {
    async fn search(
        &self,
        _query: &str,
        _num_results: Option<usize>,
        _search_type: Option<String>,
    ) -> ServiceResult<Vec<WebSearchResult>> {
        // Web search requires external API (Google/Bing) - not available in CLI
        Err(ServiceError::not_available("web.search"))
    }

    async fn fetch(
        &self,
        url: &str,
        format: Option<String>,
        timeout: Option<u64>,
    ) -> ServiceResult<String> {
        let format = format.unwrap_or_else(|| "text".to_string());
        let timeout_secs = timeout.unwrap_or(30).min(120);
        
        // Parse and validate URL
        let mut parsed_url = Url::parse(url)
            .map_err(|e| ServiceError::new("INVALID_URL", format!("Invalid URL: {e}")))?;
        
        // Upgrade HTTP to HTTPS
        if parsed_url.scheme() == "http" {
            parsed_url.set_scheme("https").ok();
        }
        
        // Validate scheme
        if parsed_url.scheme() != "https" {
            return Err(ServiceError::new(
                "INVALID_SCHEME",
                format!("Only HTTPS URLs are supported, got: {}", parsed_url.scheme()),
            ));
        }
        
        debug!(url = %parsed_url, format = %format, "Fetching URL");
        
        // Build HTTP client
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("wonopcode/0.1")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| ServiceError::new("CLIENT_ERROR", format!("Failed to create HTTP client: {e}")))?;
        
        // Fetch the URL
        let response = client.get(parsed_url.as_str()).send().await.map_err(|e| {
            if e.is_timeout() {
                ServiceError::new("TIMEOUT", format!("Request timed out after {timeout_secs}s"))
            } else if e.is_redirect() {
                ServiceError::new("REDIRECT_ERROR", "Too many redirects")
            } else {
                ServiceError::new("REQUEST_FAILED", format!("Request failed: {e}"))
            }
        })?;
        
        // Check status
        let status = response.status();
        if !status.is_success() {
            return Err(ServiceError::new(
                "HTTP_ERROR",
                format!("HTTP {} {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown")),
            ));
        }
        
        // Get content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();
        
        // Read body with size limit
        let bytes = response.bytes().await
            .map_err(|e| ServiceError::new("READ_ERROR", format!("Failed to read response: {e}")))?;
        
        if bytes.len() > MAX_RESPONSE_SIZE {
            return Err(ServiceError::new(
                "RESPONSE_TOO_LARGE",
                format!("Response too large: {} bytes (max {} bytes)", bytes.len(), MAX_RESPONSE_SIZE),
            ));
        }
        
        // Convert to string
        let text = String::from_utf8_lossy(&bytes).to_string();
        
        // Format the content
        let content = match format.as_str() {
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
        let (content, _truncated) = truncate_content(&content, 50000);
        
        Ok(content)
    }

    async fn code_search(
        &self,
        _query: &str,
        _tokens_num: Option<usize>,
    ) -> ServiceResult<String> {
        // Code search requires external API - not available in CLI
        Err(ServiceError::not_available("web.codeSearch"))
    }
}

/// Convert HTML to plain text.
fn html_to_text(html: &str) -> String {
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
                i += 6;
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

/// Convert HTML to Markdown (simplified).
fn html_to_markdown(html: &str) -> String {
    // Simplified - just use text conversion for now
    // A full implementation would handle headers, lists, etc.
    html_to_text(html)
}

/// Truncate content if too long.
fn truncate_content(content: &str, max_len: usize) -> (String, bool) {
    if content.len() <= max_len {
        return (content.to_string(), false);
    }

    let mut boundary = max_len;
    while boundary > 0 && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }

    let truncated = format!(
        "{}\n\n... [content truncated, showing first {} chars of {}] ...",
        &content[..boundary],
        boundary,
        content.len()
    );

    (truncated, true)
}
