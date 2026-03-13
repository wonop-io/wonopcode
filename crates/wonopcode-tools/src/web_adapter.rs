//! Adapter to bridge WebFetchTool to codemode WebService trait.
//!
//! Implements:
//! - fetch: Uses WebFetchTool logic
//! - search: Uses DuckDuckGo HTML scraping
//! - code_search: Searches code documentation sites

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
        query: &str,
        num_results: Option<usize>,
        _search_type: Option<String>,
    ) -> ServiceResult<Vec<WebSearchResult>> {
        let num_results = num_results.unwrap_or(10).min(20);
        
        debug!(query = %query, num_results = %num_results, "Performing web search");
        
        // Build DuckDuckGo HTML search URL
        let encoded_query = urlencoding::encode(query);
        let search_url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);
        
        // Build HTTP client
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (compatible; wonopcode/0.1)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| ServiceError::new("CLIENT_ERROR", format!("Failed to create HTTP client: {e}")))?;
        
        // Fetch search results page
        let response = client.get(&search_url).send().await.map_err(|e| {
            if e.is_timeout() {
                ServiceError::new("TIMEOUT", "Search request timed out")
            } else {
                ServiceError::new("REQUEST_FAILED", format!("Search request failed: {e}"))
            }
        })?;
        
        if !response.status().is_success() {
            return Err(ServiceError::new(
                "HTTP_ERROR",
                format!("Search returned HTTP {}", response.status().as_u16()),
            ));
        }
        
        let html = response.text().await
            .map_err(|e| ServiceError::new("READ_ERROR", format!("Failed to read response: {e}")))?;
        
        // Parse DuckDuckGo HTML results
        let results = parse_duckduckgo_results(&html, num_results);
        
        if results.is_empty() {
            debug!("No search results found for query: {}", query);
        }
        
        Ok(results)
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
        query: &str,
        tokens_num: Option<usize>,
    ) -> ServiceResult<String> {
        let max_tokens = tokens_num.unwrap_or(8000).min(16000);
        
        debug!(query = %query, max_tokens = %max_tokens, "Performing code search");
        
        // Search code-related sites
        let code_sites = "site:stackoverflow.com OR site:github.com OR site:docs.rs OR site:doc.rust-lang.org OR site:developer.mozilla.org OR site:devdocs.io";
        let full_query = format!("{} {}", query, code_sites);
        
        // Use the search method to get results
        let results = self.search(&full_query, Some(5), None).await?;
        
        if results.is_empty() {
            return Ok(format!("No code documentation found for: {}", query));
        }
        
        // Format results as text
        let mut output = format!("# Code Search Results for: {}\n\n", query);
        
        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!("## {}. {}\n", i + 1, result.title));
            output.push_str(&format!("URL: {}\n", result.url));
            output.push_str(&format!("{}\n\n", result.snippet));
        }
        
        // Optionally fetch content from top results to fill token budget
        let mut fetched_content = String::new();
        let mut current_tokens = output.len() / 4; // rough estimate: 4 chars per token
        
        for result in results.iter().take(3) {
            if current_tokens >= max_tokens {
                break;
            }
            
            // Try to fetch content from the URL
            if let Ok(content) = self.fetch(&result.url, Some("text".to_string()), Some(10)).await {
                let remaining_tokens = max_tokens.saturating_sub(current_tokens);
                let max_chars = remaining_tokens * 4;
                let truncated = if content.len() > max_chars {
                    &content[..max_chars]
                } else {
                    &content
                };
                
                fetched_content.push_str(&format!("---\n## Content from: {}\n{}\n\n", result.url, truncated));
                current_tokens += truncated.len() / 4;
            }
        }
        
        if !fetched_content.is_empty() {
            output.push_str("\n# Fetched Content\n\n");
            output.push_str(&fetched_content);
        }
        
        Ok(output)
    }
}

/// Parse DuckDuckGo HTML search results.
fn parse_duckduckgo_results(html: &str, max_results: usize) -> Vec<WebSearchResult> {
    let mut results = Vec::new();
    
    // DuckDuckGo HTML results are in <div class="result"> elements
    // Each contains:
    // - <a class="result__a"> with href and title
    // - <a class="result__snippet"> with snippet text
    
    // Simple regex-free parsing for robustness
    let mut pos = 0;
    while results.len() < max_results {
        // Find next result div
        let result_start = match html[pos..].find("class=\"result ") {
            Some(idx) => pos + idx,
            None => break,
        };
        
        // Find the end of this result div (next result or end)
        let result_end = html[result_start + 20..]
            .find("class=\"result ")
            .map(|idx| result_start + 20 + idx)
            .unwrap_or(html.len());
        
        let result_html = &html[result_start..result_end];
        
        // Extract URL from result__a href
        let url = extract_href_from_result(result_html);
        let title = extract_title_from_result(result_html);
        let snippet = extract_snippet_from_result(result_html);
        
        if let (Some(url), Some(title)) = (url, title) {
            // DuckDuckGo uses redirect URLs, extract the actual URL
            let actual_url = extract_actual_url(&url).unwrap_or(url);
            
            results.push(WebSearchResult {
                title,
                url: actual_url,
                snippet: snippet.unwrap_or_default(),
                content: None,
            });
        }
        
        pos = result_end;
    }
    
    results
}

/// Extract href from result__a link.
fn extract_href_from_result(html: &str) -> Option<String> {
    // Look for href in result__a link
    let marker = "class=\"result__a\"";
    let link_start = html.find(marker)?;
    
    // Look for the <a tag containing result__a and extract its href
    let a_start = html[..link_start].rfind("<a ")?;
    let a_end = html[link_start..].find(">").map(|i| link_start + i)?;
    let a_tag = &html[a_start..a_end];
    
    // Extract href from the a tag
    let href_start = a_tag.find("href=\"")?;
    let href_value_start = href_start + 6;
    let href_end = a_tag[href_value_start..].find("\"")? + href_value_start;
    
    Some(html_decode(&a_tag[href_value_start..href_end]))
}

/// Extract title text from result__a link.
fn extract_title_from_result(html: &str) -> Option<String> {
    let marker = "class=\"result__a\"";
    let link_start = html.find(marker)?;
    let tag_end = html[link_start..].find(">")? + link_start + 1;
    let close_tag = html[tag_end..].find("</a>")? + tag_end;
    
    let title_html = &html[tag_end..close_tag];
    Some(html_to_text(title_html).trim().to_string())
}

/// Extract snippet text from result__snippet.
fn extract_snippet_from_result(html: &str) -> Option<String> {
    let marker = "class=\"result__snippet\"";
    let snippet_start = html.find(marker)?;
    let tag_end = html[snippet_start..].find(">")? + snippet_start + 1;
    let close_tag = html[tag_end..].find("</a>")? + tag_end;
    
    let snippet_html = &html[tag_end..close_tag];
    Some(html_to_text(snippet_html).trim().to_string())
}

/// Extract actual URL from DuckDuckGo redirect URL.
fn extract_actual_url(ddg_url: &str) -> Option<String> {
    // DuckDuckGo URLs look like: //duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=...
    if ddg_url.contains("uddg=") {
        let start = ddg_url.find("uddg=")? + 5;
        let end = ddg_url[start..].find('&').map(|i| start + i).unwrap_or(ddg_url.len());
        let encoded = &ddg_url[start..end];
        return Some(urlencoding::decode(encoded).ok()?.into_owned());
    }
    
    // Some direct URLs start with //
    if ddg_url.starts_with("//") {
        return Some(format!("https:{}", ddg_url));
    }
    
    None
}

/// Decode HTML entities.
fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
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