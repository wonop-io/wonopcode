//! Messages widget for displaying conversation history.

use crate::metrics;
use crate::theme::{AgentMode, RenderSettings, Theme};
use crate::widgets::markdown::{render_markdown_with_settings, wrap_line};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use std::cell::RefCell;

/// Maximum length for stored tool outputs (10KB).
const MAX_TOOL_OUTPUT_LEN: usize = 10_000;

/// Maximum number of messages to keep in memory before pruning old ones.
const MAX_MESSAGES_IN_MEMORY: usize = 200;

/// Target number of messages after pruning.
const TARGET_MESSAGES_AFTER_PRUNE: usize = 100;

/// Number of messages around the viewport to keep cached.
const CACHE_BUFFER_SIZE: usize = 20;

/// Interval for periodic cache cleanup (in render frames).
const CACHE_CLEANUP_INTERVAL: usize = 60;

/// Truncate tool output if it exceeds the maximum length.
fn truncate_tool_output(output: Option<String>) -> Option<String> {
    output.map(|s| {
        if s.len() > MAX_TOOL_OUTPUT_LEN {
            // Find a valid UTF-8 char boundary at or before MAX_TOOL_OUTPUT_LEN
            let mut truncate_at = MAX_TOOL_OUTPUT_LEN;
            while truncate_at > 0 && !s.is_char_boundary(truncate_at) {
                truncate_at -= 1;
            }
            format!(
                "{}... [truncated {} bytes]",
                &s[..truncate_at],
                s.len() - truncate_at
            )
        } else {
            s
        }
    })
}

/// Cache for rendered markdown content.
#[derive(Debug, Clone, Default)]
struct RenderCache {
    /// The width used for rendering (cache key).
    width: usize,
    /// Cached rendered lines (for legacy single-content messages).
    lines: Vec<Line<'static>>,
    /// Cached rendered lines per text segment index (for segmented messages).
    /// Key is the segment index, value is the rendered lines for that text segment.
    segment_lines: Vec<Vec<Line<'static>>>,
}

impl RenderCache {
    fn is_valid(&self, width: usize) -> bool {
        self.width == width && !self.lines.is_empty()
    }

    fn is_segments_valid(&self, width: usize, segment_count: usize) -> bool {
        self.width == width && self.segment_lines.len() == segment_count
    }

    fn set(&mut self, width: usize, lines: Vec<Line<'static>>) {
        self.width = width;
        self.lines = lines;
    }

    fn set_segments(&mut self, width: usize, segment_lines: Vec<Vec<Line<'static>>>) {
        self.width = width;
        self.segment_lines = segment_lines;
    }

    fn get(&self) -> &[Line<'static>] {
        &self.lines
    }

    fn get_segment(&self, index: usize) -> &[Line<'static>] {
        self.segment_lines
            .get(index)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Clear the cache to free memory.
    fn clear(&mut self) {
        self.width = 0;
        self.lines.clear();
        self.lines.shrink_to_fit();
        self.segment_lines.clear();
        self.segment_lines.shrink_to_fit();
    }

    /// Estimate memory size of this cache in bytes.
    fn estimated_size(&self) -> usize {
        // Rough estimate: each Line contains Spans with styled text
        // Estimate ~50 bytes per line on average (styles + text refs)
        let lines_size = self.lines.len() * 50;
        let segment_size: usize = self.segment_lines.iter().map(|s| s.len() * 50).sum();
        lines_size + segment_size
    }
}

/// A content segment in a message - either text or a tool call.
#[derive(Debug, Clone)]
pub enum MessageSegment {
    /// Text content
    Text(String),
    /// Tool call
    Tool(DisplayToolCall),
}

/// A message in the conversation.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: MessageRole,
    /// Legacy single content field (used for user/system messages)
    pub content: String,
    /// Ordered segments of content (used for assistant messages to preserve text/tool order)
    pub segments: Vec<MessageSegment>,
    /// Legacy tool_calls field (kept for backward compatibility)
    pub tool_calls: Vec<DisplayToolCall>,
    pub agent: AgentMode,
    pub model: Option<String>,
    pub duration: Option<String>,
    /// Cache for rendered markdown (interior mutability for rendering).
    render_cache: RefCell<RenderCache>,
}

impl DisplayMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            segments: vec![],
            tool_calls: vec![],
            agent: AgentMode::Build,
            model: None,
            duration: None,
            render_cache: RefCell::new(RenderCache::default()),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        let content_str = content.into();
        Self {
            role: MessageRole::Assistant,
            content: content_str,
            segments: vec![], // Will be populated when created with segments
            tool_calls: vec![],
            agent: AgentMode::Build,
            model: None,
            duration: None,
            render_cache: RefCell::new(RenderCache::default()),
        }
    }

    /// Create an assistant message with ordered segments.
    pub fn assistant_with_segments(segments: Vec<MessageSegment>) -> Self {
        // Also build the legacy content string for compatibility
        let content = segments
            .iter()
            .filter_map(|s| match s {
                MessageSegment::Text(t) => Some(t.as_str()),
                MessageSegment::Tool(_) => None,
            })
            .collect::<Vec<_>>()
            .join("");

        // Extract tools for legacy field
        let tool_calls: Vec<DisplayToolCall> = segments
            .iter()
            .filter_map(|s| match s {
                MessageSegment::Tool(t) => Some(t.clone()),
                MessageSegment::Text(_) => None,
            })
            .collect();

        Self {
            role: MessageRole::Assistant,
            content,
            segments,
            tool_calls,
            agent: AgentMode::Build,
            model: None,
            duration: None,
            render_cache: RefCell::new(RenderCache::default()),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            segments: vec![],
            tool_calls: vec![],
            agent: AgentMode::Build,
            model: None,
            duration: None,
            render_cache: RefCell::new(RenderCache::default()),
        }
    }

    /// Get or render cached markdown content for this message.
    fn get_or_render_content(
        &self,
        width: usize,
        theme: &Theme,
        settings: &RenderSettings,
    ) -> Vec<Line<'static>> {
        let mut cache = self.render_cache.borrow_mut();
        if cache.is_valid(width) {
            return cache.get().to_vec();
        }

        // Render and cache
        let rendered = render_markdown_with_settings(&self.content, theme, width, settings);
        let lines: Vec<Line<'static>> = rendered.lines.into_iter().collect();
        cache.set(width, lines.clone());
        lines
    }

    /// Ensure segment cache is populated for the given width.
    /// Returns true if cache was already valid, false if it was rebuilt.
    fn ensure_segment_cache(&self, width: usize, theme: &Theme, settings: &RenderSettings) -> bool {
        let mut cache = self.render_cache.borrow_mut();

        // Count text segments for cache validation
        let text_segment_count = self
            .segments
            .iter()
            .filter(|s| matches!(s, MessageSegment::Text(_)))
            .count();

        if cache.is_segments_valid(width, text_segment_count) {
            return true;
        }

        // Render each text segment and cache separately
        let mut segment_lines: Vec<Vec<Line<'static>>> = Vec::with_capacity(text_segment_count);
        for segment in &self.segments {
            if let MessageSegment::Text(text) = segment {
                let rendered = render_markdown_with_settings(text, theme, width, settings);
                segment_lines.push(rendered.lines.into_iter().collect());
            }
        }
        cache.set_segments(width, segment_lines);
        false
    }

    /// Get cached lines for a specific text segment index.
    fn get_segment_lines(&self, text_segment_index: usize) -> Vec<Line<'static>> {
        self.render_cache
            .borrow()
            .get_segment(text_segment_index)
            .to_vec()
    }

    /// Clear the render cache to free memory.
    pub fn clear_cache(&self) {
        self.render_cache.borrow_mut().clear();
    }

    /// Check if this message has a cached render.
    pub fn has_cache(&self) -> bool {
        !self.render_cache.borrow().lines.is_empty()
    }

    /// Set model and agent info (builder pattern).
    pub fn with_model_agent(mut self, model: Option<String>, agent: Option<AgentMode>) -> Self {
        if let Some(m) = model {
            self.model = Some(m);
        }
        if let Some(a) = agent {
            self.agent = a;
        }
        self
    }

    /// Estimate the memory size of this message's cache in bytes.
    pub fn cache_size(&self) -> usize {
        self.render_cache.borrow().estimated_size()
    }

    /// Estimate total memory size of this message in bytes.
    pub fn estimated_size(&self) -> usize {
        let content_size = self.content.len();
        let segments_size: usize = self
            .segments
            .iter()
            .map(|s| match s {
                MessageSegment::Text(t) => t.len(),
                MessageSegment::Tool(t) => {
                    t.input.as_ref().map(|i| i.len()).unwrap_or(0)
                        + t.output.as_ref().map(|o| o.len()).unwrap_or(0)
                }
            })
            .sum();
        let tool_calls_size: usize = self
            .tool_calls
            .iter()
            .map(|t| {
                t.input.as_ref().map(|i| i.len()).unwrap_or(0)
                    + t.output.as_ref().map(|o| o.len()).unwrap_or(0)
            })
            .sum();
        let cache_size = self.cache_size();
        content_size + segments_size + tool_calls_size + cache_size
    }

    /// Estimate the number of rendered lines for this message.
    /// This provides a more accurate estimate than a fixed value,
    /// helping to reduce scroll position jumping.
    pub fn estimate_line_count(&self, width: usize) -> usize {
        // Base: header line + role line + spacing
        let mut estimate = 3usize;

        // Estimate content lines based on character count and width
        // Account for markdown overhead (~1.3x) and word wrapping
        let content_len = self.content.len();
        let effective_width = width.saturating_sub(4).max(40); // Account for margins
        let content_lines = if content_len > 0 {
            // Rough estimate: chars / (width * 0.7) to account for word boundaries
            (content_len as f64 / (effective_width as f64 * 0.7)).ceil() as usize
        } else {
            0
        };
        estimate += content_lines;

        // Each tool call adds roughly 5-15 lines depending on state
        let tool_count = self.tool_calls.len()
            + self
                .segments
                .iter()
                .filter(|s| matches!(s, MessageSegment::Tool(_)))
                .count();
        estimate += tool_count * 8; // Conservative estimate per tool

        // Completion footer for assistant messages
        if self.role == MessageRole::Assistant && self.model.is_some() {
            estimate += 2;
        }

        // Minimum of 3 lines for any message
        estimate.max(3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone)]
pub struct DisplayToolCall {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
    pub input: Option<String>,
    pub output: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub expanded: bool,
}

impl DisplayToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status: ToolStatus::Pending,
            input: None,
            output: None,
            metadata: None,
            expanded: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    Running,
    Success,
    Error,
}

/// Get icon for a tool by name.
fn tool_icon(name: &str) -> &'static str {
    let base_name = normalize_tool_name(name);
    match base_name {
        "bash" => "#",
        "read" => "→",
        "write" => "←",
        "edit" => "←",
        "glob" => "✱",
        "grep" => "✱",
        "list" => "→",
        "task" => "◉",
        "webfetch" => "%",
        "todowrite" | "todoread" => "⚙",
        "lsp" => "⊕",
        _ => "◇",
    }
}

/// Check if a tool should be rendered as a block (with border) or inline.
fn is_block_tool(name: &str) -> bool {
    let base_name = normalize_tool_name(name);
    matches!(
        base_name,
        "bash" | "edit" | "write" | "task" | "webfetch" | "read" | "glob" | "grep"
    )
}

/// Normalize MCP tool names to their base form.
/// e.g., "mcp__wonopcode-tools__bash" -> "bash"
fn normalize_tool_name(name: &str) -> &str {
    // Handle MCP tool names: mcp__<server>__<tool>
    if name.starts_with("mcp__") {
        if let Some(last_sep) = name.rfind("__") {
            if last_sep > 4 {
                // Skip past the "__"
                return &name[last_sep + 2..];
            }
        }
    }
    name
}

/// Get a human-readable title for a tool based on its name, input, and metadata.
/// Returns (main_title, optional_params_string)
fn tool_title(
    name: &str,
    input: Option<&str>,
    metadata: Option<&serde_json::Value>,
) -> (String, Option<String>) {
    let parsed: serde_json::Value = input
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null);

    // Normalize MCP tool names to their base form
    let base_name = normalize_tool_name(name);

    match base_name {
        "bash" => {
            let desc = parsed
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("Shell");
            (desc.to_string(), None)
        }
        "read" => {
            let path = parsed
                .get("filePath")
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let mut params = Vec::new();
            if let Some(offset) = parsed.get("offset").and_then(|v| v.as_u64()) {
                params.push(format!("offset={offset}"));
            }
            if let Some(limit) = parsed.get("limit").and_then(|v| v.as_u64()) {
                params.push(format!("limit={limit}"));
            }
            let params_str = if params.is_empty() {
                None
            } else {
                Some(params.join(", "))
            };
            (format!("Read {}", shorten_path(path)), params_str)
        }
        "write" => {
            let path = parsed
                .get("filePath")
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            // Show bytes written if available
            let bytes = metadata
                .and_then(|m| m.get("bytes"))
                .and_then(|v| v.as_u64());
            let suffix = bytes.map(|b| format!(" ({b} bytes)")).unwrap_or_default();
            (format!("Wrote {}{}", shorten_path(path), suffix), None)
        }
        "edit" => {
            let path = parsed
                .get("filePath")
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let mut params = Vec::new();
            if let Some(replace_all) = parsed.get("replaceAll").and_then(|v| v.as_bool()) {
                if replace_all {
                    params.push("replaceAll".to_string());
                }
            }
            let params_str = if params.is_empty() {
                None
            } else {
                Some(params.join(", "))
            };
            (format!("Edit {}", shorten_path(path)), params_str)
        }
        "glob" => {
            let pattern = parsed
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("*");
            let path = parsed.get("path").and_then(|v| v.as_str());
            // Get match count from metadata
            let count = metadata
                .and_then(|m| m.get("count"))
                .and_then(|v| v.as_u64());
            let count_str = count.map(|c| format!(" ({c} matches)")).unwrap_or_default();
            let title = if let Some(p) = path {
                format!("Glob \"{}\" in {}{}", pattern, shorten_path(p), count_str)
            } else {
                format!("Glob \"{pattern}\"{count_str}")
            };
            (title, None)
        }
        "grep" => {
            let pattern = parsed.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = parsed.get("path").and_then(|v| v.as_str());
            let include = parsed.get("include").and_then(|v| v.as_str());
            // Get match count from metadata
            let count = metadata
                .and_then(|m| m.get("matches"))
                .and_then(|v| v.as_u64());
            let count_str = count.map(|c| format!(" ({c} matches)")).unwrap_or_default();
            let title = if let Some(p) = path {
                format!("Grep \"{}\" in {}{}", pattern, shorten_path(p), count_str)
            } else {
                format!("Grep \"{pattern}\"{count_str}")
            };
            let params_str = include.map(|i| format!("include={i}"));
            (title, params_str)
        }
        "list" => {
            let path = parsed.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            // Get file count from metadata
            let count = metadata
                .and_then(|m| m.get("count"))
                .and_then(|v| v.as_u64());
            let count_str = count.map(|c| format!(" ({c} items)")).unwrap_or_default();
            (format!("List {}{}", shorten_path(path), count_str), None)
        }
        "task" => {
            let desc = parsed
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("Task");
            let subagent = parsed.get("subagent_type").and_then(|v| v.as_str());
            let title = if let Some(agent) = subagent {
                format!("{agent} Task \"{desc}\"")
            } else {
                format!("Task \"{desc}\"")
            };
            (title, None)
        }
        "webfetch" => {
            let url = parsed.get("url").and_then(|v| v.as_str()).unwrap_or("URL");
            (format!("WebFetch {}", shorten_url(url)), None)
        }
        "todowrite" => {
            // Show todo counts from metadata
            let pending = metadata
                .and_then(|m| m.get("pending"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let in_progress = metadata
                .and_then(|m| m.get("in_progress"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let completed = metadata
                .and_then(|m| m.get("completed"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let total = metadata
                .and_then(|m| m.get("total"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if total > 0 {
                (
                    format!(
                        "Update todos ({pending} pending, {in_progress} in progress, {completed} done)"
                    ),
                    None,
                )
            } else {
                ("Update todos".to_string(), None)
            }
        }
        "todoread" => ("Read todos".to_string(), None),
        "lsp" => {
            let action = parsed
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("query");
            (format!("LSP {action}"), None)
        }
        _ => (name.to_string(), None),
    }
}

/// Shorten a file path for display.
fn shorten_path(path: &str) -> &str {
    // Get just the filename or last component
    path.rsplit('/').next().unwrap_or(path)
}

/// Shorten a URL for display.
fn shorten_url(url: &str) -> String {
    // Remove protocol and get host
    let url = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if url.chars().count() > 40 {
        let truncated: String = url.chars().take(37).collect();
        format!("{truncated}...")
    } else {
        url.to_string()
    }
}

/// Memory statistics for the messages widget.
#[derive(Debug, Clone, Default)]
pub struct MessageWidgetStats {
    /// Total number of messages.
    pub message_count: usize,
    /// Total content size in bytes.
    pub total_content_bytes: usize,
    /// Total cache size in bytes.
    pub total_cache_bytes: usize,
    /// Number of messages with cached renders.
    pub cached_messages: usize,
}

/// Selection state for text copying.
#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    /// Whether selection mode is active.
    pub active: bool,
    /// Currently selected message index.
    pub message_index: usize,
    /// Start position within message (line).
    pub start_line: usize,
    /// End position within message (line).
    pub end_line: usize,
}

/// A segment of streaming content - either text or a tool call.
#[derive(Debug, Clone)]
enum StreamSegment {
    /// Text content
    Text(String),
    /// Index into active_tools
    Tool(usize),
}

/// Cache for streaming content to avoid re-rendering on every frame.
///
/// Key optimization: We cache rendered lines for text that has already been processed.
/// When new text arrives, we only need to render the NEW portion and append it.
#[derive(Debug, Clone, Default)]
struct StreamingCache {
    /// The width used for rendering (cache key).
    width: usize,
    /// Cached rendered lines for each text segment.
    /// Key is segment index, value is (text_prefix_length, rendered_lines).
    /// We cache lines for text[0..text_prefix_length] - when text grows, we only
    /// need to re-render if text changed (not just appended).
    segment_cache: Vec<(usize, Vec<Line<'static>>)>,
    /// Total line count from cached segments (for scroll calculation).
    total_cached_lines: usize,
    /// Whether the cache is valid.
    valid: bool,
}

impl StreamingCache {
    fn new() -> Self {
        Self::default()
    }

    fn clear(&mut self) {
        self.width = 0;
        self.segment_cache.clear();
        self.total_cached_lines = 0;
        self.valid = false;
    }
}

/// Cache for rendered lines to enable proper viewport-based rendering.
///
/// Key optimization: we store pre-rendered lines per message, not all lines concatenated.
/// This allows us to:
/// 1. Only render messages in the visible viewport
/// 2. Reuse rendered lines without cloning the entire buffer
/// 3. Efficiently calculate scroll positions using cumulative line counts
#[derive(Debug, Clone, Default)]
struct RenderedLinesCache {
    /// The render width this cache was built for.
    width: usize,
    /// Number of messages this cache was built for.
    message_count: usize,
    /// Pre-rendered lines for each message (index = message index).
    /// Each entry contains all the lines for that single message.
    message_lines: Vec<Vec<Line<'static>>>,
    /// Cumulative line count at the END of each message (for binary search).
    /// cumulative_lines[i] = total lines from message 0 through message i (inclusive).
    cumulative_lines: Vec<usize>,
    /// Whether the cache is valid.
    valid: bool,
}

/// A clickable code region tracked after rendering.
#[derive(Debug, Clone)]
pub struct ClickableCodeRegion {
    /// Starting line index (absolute, in rendered output).
    pub start_line: usize,
    /// Ending line index (exclusive).
    pub end_line: usize,
    /// The code content for copying.
    pub content: String,
    /// Language identifier.
    pub language: String,
}

#[derive(Debug, Clone)]
pub struct MessagesWidget {
    messages: Vec<DisplayMessage>,
    scroll: usize,
    focused: bool,
    streaming: bool,
    streaming_text: String,
    streaming_agent: AgentMode,
    active_tools: Vec<DisplayToolCall>,
    /// Ordered segments of streaming content (text and tool references)
    /// This preserves the order in which text and tools appeared
    stream_segments: Vec<StreamSegment>,
    /// Index of the revert point (messages at and after this are "undone").
    /// None means no undo has been performed.
    revert_index: Option<usize>,
    /// Whether to show thinking/reasoning blocks.
    show_thinking: bool,
    /// Selection state for copying.
    selection: SelectionState,
    /// Last known render width (for code block backgrounds).
    render_width: usize,
    /// Whether to auto-scroll to bottom when new content arrives during streaming.
    auto_scroll: bool,
    /// Frame counter for periodic cache cleanup.
    frame_counter: usize,
    /// Cached line count for scroll calculations (width, message_count, line_count).
    line_count_cache: Option<(usize, usize, usize)>,
    /// Cache for streaming content rendering.
    streaming_cache: StreamingCache,
    /// Whether the widget content has changed since last render (dirty flag).
    dirty: bool,
    /// Cache for fully rendered lines (non-streaming).
    rendered_cache: RenderedLinesCache,
    /// Render settings for performance optimization.
    render_settings: RenderSettings,
    /// Clickable code regions from the last render.
    code_regions: Vec<ClickableCodeRegion>,
    /// Last rendered scroll position (for click detection offset).
    last_render_scroll: usize,
    /// Last rendered area (for click coordinate conversion).
    last_render_area: ratatui::layout::Rect,
}

impl Default for MessagesWidget {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            scroll: 0,
            focused: false,
            streaming: false,
            streaming_text: String::new(),
            streaming_agent: AgentMode::Build,
            active_tools: Vec::new(),
            stream_segments: Vec::new(),
            revert_index: None,
            show_thinking: true,
            selection: SelectionState::default(),
            render_width: 0,
            auto_scroll: true,
            frame_counter: 0,
            line_count_cache: None,
            streaming_cache: StreamingCache::new(),
            dirty: true,
            rendered_cache: RenderedLinesCache::default(),
            render_settings: RenderSettings::default(),
            code_regions: Vec::new(),
            last_render_scroll: 0,
            last_render_area: ratatui::layout::Rect::default(),
        }
    }
}

impl MessagesWidget {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new MessagesWidget with the given render settings.
    pub fn with_render_settings(settings: RenderSettings) -> Self {
        Self {
            render_settings: settings,
            ..Default::default()
        }
    }

    /// Get the number of messages.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Set whether to show thinking/reasoning blocks.
    pub fn set_show_thinking(&mut self, show: bool) {
        self.show_thinking = show;
    }

    /// Get whether thinking is shown.
    pub fn show_thinking(&self) -> bool {
        self.show_thinking
    }

    /// Get a transcript of all messages for export.
    pub fn get_transcript(&self) -> Option<String> {
        if self.messages.is_empty() {
            return None;
        }

        let mut transcript = String::new();
        let visible_count = self.revert_index.unwrap_or(self.messages.len());

        for msg in self.messages.iter().take(visible_count) {
            let role = match msg.role {
                MessageRole::User => "## User",
                MessageRole::Assistant => "## Assistant",
                MessageRole::System => "## System",
                MessageRole::Tool => "## Tool",
            };
            transcript.push_str(role);
            transcript.push_str("\n\n");
            transcript.push_str(&msg.content);
            transcript.push_str("\n\n");

            // Include tool calls
            for tool in &msg.tool_calls {
                transcript.push_str(&format!("### Tool: {}\n", tool.name));
                if let Some(input) = &tool.input {
                    transcript.push_str("```json\n");
                    transcript.push_str(input);
                    transcript.push_str("\n```\n");
                }
                if let Some(output) = &tool.output {
                    transcript.push_str("\n**Output:**\n```\n");
                    // Truncate long outputs
                    if output.chars().count() > 1000 {
                        let truncated: String = output.chars().take(1000).collect();
                        transcript.push_str(&truncated);
                        transcript.push_str("\n... (truncated)");
                    } else {
                        transcript.push_str(output);
                    }
                    transcript.push_str("\n```\n");
                }
                transcript.push('\n');
            }
        }

        Some(transcript)
    }

    pub fn add_message(&mut self, message: DisplayMessage) {
        self.messages.push(message);
        self.invalidate_render_cache();

        // Prune old messages if we exceed the limit
        if self.messages.len() > MAX_MESSAGES_IN_MEMORY {
            self.prune_old_messages();
        }
    }

    /// Replace all messages with a new set (used when loading a session).
    /// Scrolls to the bottom to show the most recent messages.
    pub fn set_messages(&mut self, messages: Vec<DisplayMessage>) {
        // Clear existing caches
        for msg in &self.messages {
            msg.clear_cache();
        }
        self.messages = messages;
        self.revert_index = None;
        self.invalidate_render_cache();

        // Prune if needed
        if self.messages.len() > MAX_MESSAGES_IN_MEMORY {
            self.prune_old_messages();
        }

        // Scroll to bottom to show most recent messages
        self.scroll_to_bottom();
    }

    /// Invalidate all render caches, forcing a full rebuild on next render.
    fn invalidate_render_cache(&mut self) {
        self.line_count_cache = None;
        self.dirty = true;
        self.rendered_cache.valid = false;
        // Reset width to force a full rebuild on next render
        self.rendered_cache.width = 0;
        self.streaming_cache.clear();
    }

    /// Public method to invalidate all caches (e.g., when render settings change).
    pub fn invalidate_cache(&mut self) {
        // Clear all message-level caches
        for msg in &self.messages {
            msg.clear_cache();
        }
        self.invalidate_render_cache();
    }

    /// Set render settings and invalidate caches if changed.
    pub fn set_render_settings(&mut self, settings: RenderSettings) {
        self.render_settings = settings;
    }

    /// Get the current render settings.
    pub fn render_settings(&self) -> &RenderSettings {
        &self.render_settings
    }

    /// Prune old messages to prevent unbounded memory growth.
    fn prune_old_messages(&mut self) {
        if self.messages.len() <= TARGET_MESSAGES_AFTER_PRUNE {
            return;
        }

        let to_remove = self.messages.len() - TARGET_MESSAGES_AFTER_PRUNE;

        // Keep the first message (usually important context) and remove from the middle
        if to_remove > 0 && self.messages.len() > 2 {
            // Clear caches of messages being removed
            for msg in self.messages.iter().skip(1).take(to_remove) {
                msg.clear_cache();
            }

            // Remove messages from index 1 to (1 + to_remove)
            self.messages.drain(1..(1 + to_remove));

            // Update revert index if needed
            if let Some(ref mut idx) = self.revert_index {
                *idx = idx.saturating_sub(to_remove);
            }

            self.invalidate_render_cache();
            tracing::debug!(
                removed = to_remove,
                remaining = self.messages.len(),
                "Pruned old messages to prevent memory growth"
            );
        }
    }

    /// Clear render caches for messages far from the current viewport.
    /// This should be called periodically during rendering.
    pub fn cleanup_distant_caches(&mut self, visible_start: usize, visible_end: usize) {
        let buffer_start = visible_start.saturating_sub(CACHE_BUFFER_SIZE);
        let buffer_end = (visible_end + CACHE_BUFFER_SIZE).min(self.messages.len());

        let mut cleared = 0;
        for (i, msg) in self.messages.iter().enumerate() {
            if (i < buffer_start || i >= buffer_end) && msg.has_cache() {
                msg.clear_cache();
                cleared += 1;
            }
        }

        // Also clear our rendered lines cache for distant messages
        for i in 0..buffer_start.min(self.rendered_cache.message_lines.len()) {
            if !self.rendered_cache.message_lines[i].is_empty() {
                self.rendered_cache.message_lines[i].clear();
                self.rendered_cache.message_lines[i].shrink_to_fit();
                cleared += 1;
            }
        }
        for i in buffer_end..self.rendered_cache.message_lines.len() {
            if !self.rendered_cache.message_lines[i].is_empty() {
                self.rendered_cache.message_lines[i].clear();
                self.rendered_cache.message_lines[i].shrink_to_fit();
                cleared += 1;
            }
        }

        if cleared > 0 {
            tracing::trace!(cleared = cleared, "Cleared distant message caches");
        }
    }

    /// Get memory statistics for this widget.
    pub fn memory_stats(&self) -> MessageWidgetStats {
        let mut total_content_bytes = 0;
        let mut total_cache_bytes = 0;
        let mut cached_messages = 0;

        for msg in &self.messages {
            total_content_bytes += msg.estimated_size();
            let cache_size = msg.cache_size();
            if cache_size > 0 {
                total_cache_bytes += cache_size;
                cached_messages += 1;
            }
        }

        MessageWidgetStats {
            message_count: self.messages.len(),
            total_content_bytes,
            total_cache_bytes,
            cached_messages,
        }
    }

    pub fn start_streaming(&mut self) {
        self.streaming = true;
        self.streaming_text.clear();
        self.active_tools.clear();
        self.stream_segments.clear();
        self.streaming_cache.clear();
        self.dirty = true;
        // Enable auto-scroll when streaming starts
        self.auto_scroll = true;
        self.scroll = usize::MAX; // Start at bottom
    }

    pub fn append_streaming(&mut self, text: &str) {
        self.streaming_text.push_str(text);
        self.dirty = true;

        // Add to or extend the last text segment
        match self.stream_segments.last_mut() {
            Some(StreamSegment::Text(existing)) => {
                existing.push_str(text);
            }
            _ => {
                // Either no segments or last was a tool - add new text segment
                self.stream_segments
                    .push(StreamSegment::Text(text.to_string()));
            }
        }
    }

    pub fn set_streaming_agent(&mut self, agent: AgentMode) {
        self.streaming_agent = agent;
    }

    pub fn add_tool_call(&mut self, id: String, name: String) {
        let tool_index = self.active_tools.len();
        self.active_tools.push(DisplayToolCall::new(id, name));
        if let Some(tool) = self.active_tools.last_mut() {
            tool.status = ToolStatus::Running;
        }
        // Add tool reference to segments
        self.stream_segments.push(StreamSegment::Tool(tool_index));
        self.dirty = true;
    }

    pub fn add_tool_call_with_input(&mut self, id: String, name: String, input: String) {
        let tool_index = self.active_tools.len();
        let mut tool = DisplayToolCall::new(id, name);
        tool.status = ToolStatus::Running;
        tool.input = Some(input);
        self.active_tools.push(tool);
        // Add tool reference to segments
        self.stream_segments.push(StreamSegment::Tool(tool_index));
        self.dirty = true;
    }

    pub fn update_tool_status(&mut self, id: &str, status: ToolStatus, output: Option<String>) {
        if let Some(tool) = self.active_tools.iter_mut().find(|t| t.id == id) {
            tool.status = status;
            tool.output = truncate_tool_output(output);
            self.dirty = true;
        }
    }

    pub fn update_tool_status_with_metadata(
        &mut self,
        id: &str,
        status: ToolStatus,
        output: Option<String>,
        metadata: Option<serde_json::Value>,
    ) {
        if let Some(tool) = self.active_tools.iter_mut().find(|t| t.id == id) {
            tool.status = status;
            tool.output = truncate_tool_output(output);
            tool.metadata = metadata;
            self.dirty = true;
        }
    }

    /// End streaming and return message segments preserving text/tool order.
    pub fn end_streaming(&mut self) -> Vec<MessageSegment> {
        self.streaming = false;

        // Convert stream segments to message segments
        let segments: Vec<MessageSegment> = self
            .stream_segments
            .drain(..)
            .filter_map(|seg| match seg {
                StreamSegment::Text(text) if !text.is_empty() => Some(MessageSegment::Text(text)),
                StreamSegment::Text(_) => None, // Skip empty text
                StreamSegment::Tool(idx) => self
                    .active_tools
                    .get(idx)
                    .cloned()
                    .map(MessageSegment::Tool),
            })
            .collect();

        // Clear state
        self.streaming_text.clear();
        self.active_tools.clear();
        self.streaming_cache.clear();
        self.dirty = true;

        segments
    }

    /// End streaming and return legacy format (for backward compatibility).
    pub fn end_streaming_legacy(&mut self) -> (String, Vec<DisplayToolCall>) {
        self.streaming = false;
        let text = std::mem::take(&mut self.streaming_text);
        let tools = std::mem::take(&mut self.active_tools);
        self.stream_segments.clear();
        self.streaming_cache.clear();
        self.dirty = true;
        (text, tools)
    }

    /// End streaming and immediately add the message in one atomic operation.
    /// This avoids the flicker that can occur when end_streaming() and add_message()
    /// are called separately with a render in between.
    pub fn end_streaming_and_add_message(&mut self, mut message: DisplayMessage) {
        // Convert stream segments to message segments
        let segments: Vec<MessageSegment> = self
            .stream_segments
            .drain(..)
            .filter_map(|seg| match seg {
                StreamSegment::Text(text) if !text.is_empty() => Some(MessageSegment::Text(text)),
                StreamSegment::Text(_) => None,
                StreamSegment::Tool(idx) => self
                    .active_tools
                    .get(idx)
                    .cloned()
                    .map(MessageSegment::Tool),
            })
            .collect();

        // Update the message with the segments
        message.segments = segments.clone();
        message.content = segments
            .iter()
            .filter_map(|s| match s {
                MessageSegment::Text(t) => Some(t.as_str()),
                MessageSegment::Tool(_) => None,
            })
            .collect::<Vec<_>>()
            .join("");
        message.tool_calls = segments
            .iter()
            .filter_map(|s| match s {
                MessageSegment::Tool(t) => Some(t.clone()),
                MessageSegment::Text(_) => None,
            })
            .collect();

        // Clear streaming state
        self.streaming = false;
        self.streaming_text.clear();
        self.active_tools.clear();
        self.streaming_cache.clear();

        // Add the message - but use a lighter cache invalidation
        // We only need to mark the cache as needing an update for the new message,
        // not invalidate all existing cached renders
        self.messages.push(message);

        // Extend rendered cache arrays to accommodate the new message
        // without clearing existing cached renders
        let new_count = self.messages.len();
        if self.rendered_cache.message_lines.len() < new_count {
            self.rendered_cache.message_lines.push(Vec::new());
            self.rendered_cache.cumulative_lines.push(0);
            self.rendered_cache.message_count = new_count;
        }

        // Mark that cumulative counts need recalculating
        self.rendered_cache.valid = false;
        self.line_count_cache = None;
        self.dirty = true;

        // Ensure we stay at bottom
        self.scroll = usize::MAX;
        self.auto_scroll = true;

        // Prune old messages if we exceed the limit
        if self.messages.len() > MAX_MESSAGES_IN_MEMORY {
            self.prune_old_messages();
        }
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    pub fn scroll_up(&mut self, amount: usize) {
        let start = std::time::Instant::now();
        self.scroll = self.scroll.saturating_sub(amount);
        // Disable auto-scroll when user scrolls up during streaming
        if self.streaming {
            self.auto_scroll = false;
        }
        metrics::record_scroll(start.elapsed(), amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let start = std::time::Instant::now();
        self.scroll = self.scroll.saturating_add(amount);
        metrics::record_scroll(start.elapsed(), amount);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = usize::MAX;
        self.auto_scroll = true;
    }

    /// Check if we're currently at or near the bottom of the scroll area.
    #[allow(dead_code)]
    fn is_near_bottom(&self, max_scroll: usize) -> bool {
        self.scroll + 5 >= max_scroll
    }

    /// Scroll to bring a specific message into view.
    pub fn scroll_to_message(&mut self, message_index: usize) {
        // This is a rough approximation - scroll position is line-based
        // We estimate ~5 lines per message on average
        let estimated_line = message_index.saturating_mul(5);
        self.scroll = estimated_line;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Enter selection mode - selects the current message.
    pub fn enter_selection_mode(&mut self) {
        let visible = self.visible_count();
        if visible > 0 {
            // Start with last assistant message selected
            let idx = self.messages[..visible]
                .iter()
                .rposition(|m| m.role == MessageRole::Assistant)
                .unwrap_or(visible.saturating_sub(1));

            self.selection = SelectionState {
                active: true,
                message_index: idx,
                start_line: 0,
                end_line: 0,
            };

            // Scroll to make the selected message visible
            self.scroll_to_message(idx);
        }
    }

    /// Exit selection mode.
    pub fn exit_selection_mode(&mut self) {
        self.selection.active = false;
    }

    /// Check if in selection mode.
    pub fn is_selecting(&self) -> bool {
        self.selection.active
    }

    /// Move selection to previous message.
    pub fn select_prev_message(&mut self) {
        if self.selection.active && self.selection.message_index > 0 {
            self.selection.message_index -= 1;
            // Scroll to make the selected message visible
            self.scroll_to_message(self.selection.message_index);
        }
    }

    /// Move selection to next message.
    pub fn select_next_message(&mut self) {
        let visible = self.visible_count();
        if self.selection.active && self.selection.message_index < visible.saturating_sub(1) {
            self.selection.message_index += 1;
            // Scroll to make the selected message visible
            self.scroll_to_message(self.selection.message_index);
        }
    }

    /// Get the content of the selected message.
    pub fn get_selected_content(&self) -> Option<String> {
        if !self.selection.active {
            return None;
        }

        let visible = self.visible_count();
        if self.selection.message_index < visible {
            let msg = &self.messages[self.selection.message_index];
            Some(msg.content.clone())
        } else {
            None
        }
    }

    /// Handle a click at the given terminal coordinates.
    /// Returns the code content if a code block or inline code was clicked, None otherwise.
    pub fn handle_click(&self, x: u16, y: u16) -> Option<String> {
        // Check if click is within our rendered area
        if x < self.last_render_area.x
            || x >= self.last_render_area.x + self.last_render_area.width
            || y < self.last_render_area.y
            || y >= self.last_render_area.y + self.last_render_area.height
        {
            return None;
        }

        // Convert terminal y coordinate to line index in rendered content
        // y is the terminal row, we need to find which line of content that corresponds to
        let row_in_widget = (y - self.last_render_area.y) as usize;
        let absolute_line = self.last_render_scroll + row_in_widget;

        // Calculate column position within the widget
        let col_in_widget = (x - self.last_render_area.x) as usize;

        // Check if this line falls within any fenced code block region
        for region in &self.code_regions {
            if absolute_line >= region.start_line && absolute_line < region.end_line {
                return Some(region.content.clone());
            }
        }

        // If not in a fenced code block, check for inline code in the clicked line
        // Try to find which message and line was clicked and extract inline code from it
        self.find_inline_code_at_position(absolute_line, col_in_widget)
    }

    /// Try to find inline code at the given rendered line and column position.
    /// This looks at the actual rendered spans to find inline code with background styling.
    fn find_inline_code_at_position(&self, rendered_line: usize, col: usize) -> Option<String> {
        let visible = self.visible_count();
        let mut current_line = 0usize;

        for idx in 0..visible {
            let msg_rendered_lines = self
                .rendered_cache
                .message_lines
                .get(idx)
                .map(|l| l.len())
                .unwrap_or(0);

            if current_line + msg_rendered_lines > rendered_line {
                // This click is within this message's rendered lines
                let line_in_msg = rendered_line - current_line;

                // Get the actual rendered line and look for inline code spans
                if let Some(msg_lines) = self.rendered_cache.message_lines.get(idx) {
                    if let Some(line) = msg_lines.get(line_in_msg) {
                        // Track horizontal position as we iterate through spans
                        let mut current_col = 0usize;

                        for span in &line.spans {
                            let span_width = span.content.chars().count();
                            let span_end = current_col + span_width;

                            // Check if click is within this span AND span has background color
                            if col >= current_col && col < span_end && span.style.bg.is_some() {
                                let content = span.content.trim();
                                if !content.is_empty() {
                                    return Some(content.to_string());
                                }
                            }

                            current_col = span_end;
                        }
                    }
                }
                return None;
            }

            current_line += msg_rendered_lines;
        }

        None
    }

    /// Extract all inline code snippets from a line.
    #[cfg(test)]
    fn extract_inline_code(line: &str) -> Vec<String> {
        let mut codes = Vec::new();
        let mut in_code = false;
        let mut current_code = String::new();

        for c in line.chars() {
            if c == '`' {
                if in_code {
                    // End of inline code
                    if !current_code.is_empty() {
                        codes.push(current_code.clone());
                    }
                    current_code.clear();
                    in_code = false;
                } else {
                    // Start of inline code
                    in_code = true;
                }
            } else if in_code {
                current_code.push(c);
            }
        }

        codes
    }

    /// Extract all code blocks from a piece of markdown content.
    /// Returns a list of (start_line_in_rendered, end_line_in_rendered, code_content).
    fn extract_code_blocks_from_content(content: &str) -> Vec<(String, String)> {
        let mut blocks = Vec::new();
        let mut in_code_block = false;
        let mut code_block_lang = String::new();
        let mut code_lines: Vec<&str> = Vec::new();

        for line in content.lines() {
            if line.starts_with("```") {
                if in_code_block {
                    // End of code block
                    let code_content = code_lines.join("\n");
                    blocks.push((code_block_lang.clone(), code_content));
                    code_lines.clear();
                    code_block_lang.clear();
                    in_code_block = false;
                } else {
                    // Start of code block
                    code_block_lang = line.strip_prefix("```").unwrap_or("").trim().to_string();
                    in_code_block = true;
                }
            } else if in_code_block {
                code_lines.push(line);
            }
        }

        // Handle unclosed code block
        if in_code_block && !code_lines.is_empty() {
            blocks.push((code_block_lang, code_lines.join("\n")));
        }

        blocks
    }

    /// Get all code blocks from visible messages.
    /// Returns a list of (language, content) pairs.
    pub fn get_all_code_blocks(&self) -> Vec<(String, String)> {
        let visible = self.visible_count();
        let mut all_blocks = Vec::new();

        for msg in self.messages.iter().take(visible) {
            if msg.role == MessageRole::Assistant {
                all_blocks.extend(Self::extract_code_blocks_from_content(&msg.content));
            }
        }

        // Also check streaming content
        if self.streaming && !self.streaming_text.is_empty() {
            all_blocks.extend(Self::extract_code_blocks_from_content(&self.streaming_text));
        }

        all_blocks
    }

    /// Extract code regions from content and add them to the provided regions vector.
    /// The line_offset is the cumulative line count before this message.
    fn extract_code_regions_into(
        content: &str,
        line_offset: usize,
        regions: &mut Vec<ClickableCodeRegion>,
    ) {
        let mut in_code_block = false;
        let mut code_block_lang = String::new();
        let mut code_lines: Vec<&str> = Vec::new();

        // Track source line to rendered line mapping
        // This is approximate - each code block header takes 1 line, each code line takes 1 line
        // Plus indentation/wrapping which we estimate
        let mut rendered_line = line_offset;

        // Skip message header (role indicator) - approximately 1 line for assistant
        rendered_line += 1;

        for line in content.lines() {
            if line.starts_with("```") {
                if in_code_block {
                    // End of code block
                    let code_content = code_lines.join("\n");

                    // Calculate approximate rendered line range
                    // Header line + code lines
                    let code_block_rendered_lines = 1 + code_lines.len();
                    let start_rendered = rendered_line;
                    let end_rendered = rendered_line + code_block_rendered_lines;

                    regions.push(ClickableCodeRegion {
                        start_line: start_rendered,
                        end_line: end_rendered,
                        content: code_content,
                        language: code_block_lang.clone(),
                    });

                    rendered_line = end_rendered;
                    code_lines.clear();
                    code_block_lang.clear();
                    in_code_block = false;
                } else {
                    // Start of code block
                    code_block_lang = line.strip_prefix("```").unwrap_or("").trim().to_string();
                    in_code_block = true;
                }
            } else if in_code_block {
                code_lines.push(line);
            } else {
                // Regular text line - estimate 1 rendered line (may wrap, but approximate)
                rendered_line += 1;
            }
        }

        // Handle unclosed code block
        if in_code_block && !code_lines.is_empty() {
            let code_content = code_lines.join("\n");
            let code_block_rendered_lines = 1 + code_lines.len();
            let start_rendered = rendered_line;
            let end_rendered = rendered_line + code_block_rendered_lines;

            regions.push(ClickableCodeRegion {
                start_line: start_rendered,
                end_line: end_rendered,
                content: code_content,
                language: code_block_lang,
            });
        }
    }

    /// Get the content of the last assistant message, if any.
    pub fn get_last_assistant_content(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
            .map(|m| m.content.as_str())
    }

    /// Get all messages (for export/copy).
    pub fn get_messages(&self) -> &[DisplayMessage] {
        &self.messages
    }

    /// Get the number of visible messages (not undone).
    pub fn visible_count(&self) -> usize {
        self.revert_index.unwrap_or(self.messages.len())
    }

    /// Check if there are messages that can be undone.
    pub fn can_undo(&self) -> bool {
        let visible = self.visible_count();
        // Need at least 2 messages (1 user + 1 assistant) to undo
        visible >= 2
    }

    /// Check if there are undone messages that can be redone.
    pub fn can_redo(&self) -> bool {
        self.revert_index
            .map(|idx| idx < self.messages.len())
            .unwrap_or(false)
    }

    /// Undo the last user message and its response.
    /// Returns the user message content if undo was successful.
    pub fn undo(&mut self) -> Option<String> {
        if !self.can_undo() {
            return None;
        }

        let visible = self.visible_count();

        // Find the last user message in visible messages
        let mut user_idx = None;
        for i in (0..visible).rev() {
            if self.messages[i].role == MessageRole::User {
                user_idx = Some(i);
                break;
            }
        }

        let user_idx = user_idx?;

        // Set revert point to the user message (hiding it and everything after)
        self.revert_index = Some(user_idx);
        self.invalidate_render_cache();

        // Return the user message content so it can be restored to input
        Some(self.messages[user_idx].content.clone())
    }

    /// Redo the last undone messages.
    /// Returns true if redo was successful.
    pub fn redo(&mut self) -> bool {
        let Some(current_revert) = self.revert_index else {
            return false;
        };

        if current_revert >= self.messages.len() {
            return false;
        }

        // Find the next user message after current revert point
        let mut next_user_idx = None;
        for i in (current_revert + 1)..self.messages.len() {
            if self.messages[i].role == MessageRole::User {
                next_user_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = next_user_idx {
            // Move revert point to next user message
            self.revert_index = Some(idx);
        } else {
            // No more user messages, clear revert (show all)
            self.revert_index = None;
        }

        self.invalidate_render_cache();
        true
    }

    /// Clear the revert state (called when new message is sent after undo).
    /// This permanently removes undone messages.
    pub fn commit_revert(&mut self) {
        if let Some(idx) = self.revert_index.take() {
            // Remove messages from revert point onwards
            self.messages.truncate(idx);
            self.invalidate_render_cache();
        }
    }

    /// Get the number of undone messages.
    pub fn undone_count(&self) -> usize {
        if let Some(idx) = self.revert_index {
            self.messages.len() - idx
        } else {
            0
        }
    }

    /// Check if we're in a reverted state.
    pub fn is_reverted(&self) -> bool {
        self.revert_index.is_some()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let _timer = metrics::widget_timer("messages");

        // Store area for click detection
        self.last_render_area = area;

        let width = area.width as usize;
        self.render_width = width;
        let visible_count = self.visible_count();
        let visible_height = area.height as usize;

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 1: Cache Management
        // ═══════════════════════════════════════════════════════════════════

        let width_changed = self.rendered_cache.width != width;
        let count_changed = self.rendered_cache.message_count != visible_count;

        if width_changed || count_changed {
            if width_changed {
                // Width changed - all cached renders are invalid
                self.rendered_cache.message_lines.clear();
                self.streaming_cache.clear();
            }
            self.rendered_cache
                .message_lines
                .resize_with(visible_count, Vec::new);
            self.rendered_cache
                .cumulative_lines
                .resize(visible_count, 0);
            self.rendered_cache.width = width;
            self.rendered_cache.message_count = visible_count;
        }

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 2: Calculate total line count (use cached values, fast)
        // ═══════════════════════════════════════════════════════════════════

        // Only recalculate cumulative counts if cache structure changed
        if width_changed || count_changed || !self.rendered_cache.valid {
            let mut running_total = 0usize;
            for idx in 0..visible_count {
                let line_count = if !self.rendered_cache.message_lines[idx].is_empty() {
                    self.rendered_cache.message_lines[idx].len()
                } else {
                    // Use content-aware estimate instead of fixed 15
                    self.messages[idx].estimate_line_count(width)
                };
                running_total += line_count;
                self.rendered_cache.cumulative_lines[idx] = running_total;
            }
            self.rendered_cache.valid = true;
        }

        let base_total_lines = self
            .rendered_cache
            .cumulative_lines
            .last()
            .copied()
            .unwrap_or(0);

        // Estimate streaming content lines
        let streaming_lines_estimate = if self.streaming {
            self.streaming_cache.total_cached_lines + 20 // cached + buffer for new content
        } else {
            0
        };

        // Note: +4 accounts for 3 padding lines + 1 cursor line during streaming
        let total_lines_estimate = base_total_lines + streaming_lines_estimate + 4;
        let max_scroll = total_lines_estimate.saturating_sub(visible_height);

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 3: Handle Scrolling
        // ═══════════════════════════════════════════════════════════════════

        // During streaming: auto_scroll keeps us at bottom unless user scrolls up
        // We DON'T clamp scroll here during streaming because the estimate might be
        // inaccurate. The actual clamping happens in Phase 7 after we know real line count.
        if self.streaming {
            if self.auto_scroll {
                // Auto-scroll to estimated bottom - will be adjusted in Phase 7
                self.scroll = max_scroll;
            }
            // When not auto_scroll, let the user's scroll position stand.
            // Phase 7 will clamp if necessary after computing actual content.
        } else {
            self.scroll = self.scroll.min(max_scroll);
        }

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 4: Determine visible messages
        // ═══════════════════════════════════════════════════════════════════

        let (first_msg, last_msg, _) = self.find_visible_messages(self.scroll, visible_height);
        let buffer = 2;
        // During streaming, include ALL messages (start_msg = 0) to ensure
        // lines_above = 0 and scroll calculations are simple and correct.
        // This is less efficient but guarantees correct behavior.
        let (start_msg, end_msg) = if self.streaming {
            (0, visible_count)
        } else {
            (
                first_msg.saturating_sub(buffer),
                (last_msg + buffer).min(visible_count),
            )
        };

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 5: Lazy render visible messages + build code regions
        // ═══════════════════════════════════════════════════════════════════

        let mut any_rendered = false;
        // Clear and rebuild code regions (we track them per render)
        self.code_regions.clear();
        let mut cumulative_line_offset = 0usize;

        for idx in start_msg..end_msg {
            let is_selected = self.selection.active && idx == self.selection.message_index;

            // Extract code blocks from this message's content
            // We need to extract content info before borrowing self mutably
            let (role, content) = {
                let msg = &self.messages[idx];
                (msg.role, msg.content.clone())
            };

            if role == MessageRole::Assistant {
                Self::extract_code_regions_into(
                    &content,
                    cumulative_line_offset,
                    &mut self.code_regions,
                );
            }

            if self.rendered_cache.message_lines[idx].is_empty() {
                let msg = &self.messages[idx];
                let mut msg_lines: Vec<Line<'static>> = Vec::new();
                self.render_message(&mut msg_lines, msg, theme, is_selected);
                msg_lines.push(Line::from("")); // Spacing

                self.rendered_cache.message_lines[idx] = msg_lines;
                any_rendered = true;
            }

            // Update cumulative offset for next message
            cumulative_line_offset += self.rendered_cache.message_lines[idx].len();
        }

        // Update cumulative counts if we rendered anything
        if any_rendered {
            let mut running_total = 0usize;
            for idx in 0..visible_count {
                let line_count = if !self.rendered_cache.message_lines[idx].is_empty() {
                    self.rendered_cache.message_lines[idx].len()
                } else {
                    // Use content-aware estimate
                    self.messages[idx].estimate_line_count(width)
                };
                running_total += line_count;
                self.rendered_cache.cumulative_lines[idx] = running_total;
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 6: Build output lines
        // During streaming: use simple approach for correct scroll behavior
        // After streaming: use line-level virtualization for performance
        // ═══════════════════════════════════════════════════════════════════

        // Calculate lines_above (lines in messages before our visible window)
        let lines_above = if start_msg > 0 {
            self.rendered_cache.cumulative_lines[start_msg - 1]
        } else {
            0
        };

        let (lines, lines_skipped) = if self.streaming {
            // During streaming: simpler approach without line-level virtualization
            // This ensures scrolling works correctly while content is being added
            let expected_lines: usize = (start_msg..end_msg)
                .map(|idx| {
                    self.rendered_cache
                        .message_lines
                        .get(idx)
                        .map(|l| l.len())
                        .unwrap_or(0)
                })
                .sum();
            let mut lines: Vec<Line<'static>> = Vec::with_capacity(expected_lines + 50);

            for idx in start_msg..end_msg {
                if let Some(msg_lines) = self.rendered_cache.message_lines.get(idx) {
                    if !msg_lines.is_empty() {
                        lines.extend(msg_lines.iter().cloned());
                    }
                }
            }

            // Add streaming content
            self.render_streaming_lines_cached(&mut lines, theme);

            // Bottom padding
            for _ in 0..3 {
                lines.push(Line::from(""));
            }

            (lines, 0usize)
        } else {
            // Not streaming: use line-level virtualization for better performance
            let scroll_offset_in_window = self.scroll.saturating_sub(lines_above);
            let line_buffer = 10;
            let skip_lines = scroll_offset_in_window.saturating_sub(line_buffer);
            let take_lines = visible_height + line_buffer * 2;

            let mut lines: Vec<Line<'static>> = Vec::with_capacity(take_lines + 20);
            let mut current_line = 0usize;
            let mut lines_skipped = 0usize;

            for idx in start_msg..end_msg {
                if let Some(msg_lines) = self.rendered_cache.message_lines.get(idx) {
                    if !msg_lines.is_empty() {
                        let msg_line_count = msg_lines.len();

                        if current_line + msg_line_count <= skip_lines {
                            lines_skipped += msg_line_count;
                        } else if current_line >= skip_lines + take_lines {
                            break;
                        } else {
                            let start_in_msg = skip_lines.saturating_sub(current_line);
                            let end_in_msg =
                                (skip_lines + take_lines - current_line).min(msg_line_count);

                            if start_in_msg < end_in_msg {
                                lines.extend(msg_lines[start_in_msg..end_in_msg].iter().cloned());
                                if start_in_msg > 0 {
                                    lines_skipped += start_in_msg;
                                }
                            }
                        }
                        current_line += msg_line_count;
                    }
                }
            }

            // Revert indicator (only if we're at/near the end)
            if self.revert_index.is_some() && end_msg >= visible_count {
                let undone = self.undone_count();
                lines.push(Line::from(vec![
                    Span::styled("  ", theme.muted_style()),
                    Span::styled(
                        format!(
                            "── {} message{} undone ──",
                            undone,
                            if undone == 1 { "" } else { "s" }
                        ),
                        theme.warning_style(),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  ", theme.muted_style()),
                    Span::styled("Press ", theme.muted_style()),
                    Span::styled("Ctrl+X R", theme.accent_style()),
                    Span::styled(" to redo, or type to discard", theme.muted_style()),
                ]));
                lines.push(Line::from(""));
            }

            // Bottom padding
            for _ in 0..3 {
                lines.push(Line::from(""));
            }

            (lines, lines_skipped)
        };

        // Adjust lines_above to account for the lines we skipped within visible messages
        let adjusted_lines_above = lines_above + lines_skipped;

        // ═══════════════════════════════════════════════════════════════════
        // PHASE 7: Render to terminal
        // ═══════════════════════════════════════════════════════════════════

        // Calculate actual total lines
        // For non-streaming: use cumulative cache which has accurate totals
        // For streaming: use lines_above + lines.len() since streaming content
        // isn't in cumulative cache
        let actual_total = if self.streaming {
            lines_above + lines.len()
        } else {
            // Use cumulative count for total message lines, plus padding
            self.rendered_cache
                .cumulative_lines
                .last()
                .copied()
                .unwrap_or(0)
                + 3 // padding lines
        };
        let final_max_scroll = actual_total.saturating_sub(visible_height);

        // Handle scroll position based on mode
        if self.streaming {
            if self.auto_scroll {
                // Auto-scroll: jump to ACTUAL bottom (not the estimate from Phase 3)
                // This ensures we track the real content as it streams in
                self.scroll = final_max_scroll;
            }
            // When not auto_scroll during streaming: don't clamp.
            // Let user scroll freely to any position they want.

            // Re-enable auto_scroll if user has scrolled to (or near) the actual bottom
            if !self.auto_scroll && self.scroll >= final_max_scroll.saturating_sub(2) {
                self.auto_scroll = true;
            }
        } else {
            // Not streaming: clamp scroll to actual content bounds
            self.scroll = self.scroll.min(final_max_scroll);
        }

        // Store the final scroll position for click detection
        self.last_render_scroll = self.scroll;

        // Calculate scroll offset within our sliced line buffer
        // We've already skipped `lines_skipped` lines, so we only need to scroll
        // by the remaining offset within our buffer
        let scroll_offset = self.scroll.saturating_sub(adjusted_lines_above);

        // Safety: ensure scroll_offset doesn't exceed our buffer
        // (this shouldn't happen if math is correct, but prevents weird rendering)
        let safe_scroll_offset = scroll_offset.min(lines.len().saturating_sub(1));

        let paragraph = Paragraph::new(Text::from(lines))
            .scroll((safe_scroll_offset.min(u16::MAX as usize) as u16, 0));
        frame.render_widget(paragraph, area);

        // Scrollbar
        if actual_total > visible_height && self.focused {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█");

            let mut scrollbar_state = ScrollbarState::new(final_max_scroll).position(self.scroll);
            frame.render_stateful_widget(
                scrollbar,
                Rect::new(area.x + area.width - 1, area.y, 1, area.height),
                &mut scrollbar_state,
            );
        }

        // Periodic cleanup
        self.frame_counter += 1;
        if self.frame_counter % (CACHE_CLEANUP_INTERVAL * 2) == 0 {
            self.cleanup_distant_caches(start_msg, end_msg);
        }
    }

    /// Find which messages are visible at the given scroll position.
    /// Returns (first_visible_msg_idx, last_visible_msg_idx, lines_to_skip_in_first_msg).
    fn find_visible_messages(&self, scroll: usize, visible_height: usize) -> (usize, usize, usize) {
        let cumulative = &self.rendered_cache.cumulative_lines;

        if cumulative.is_empty() {
            return (0, 0, 0);
        }

        // Binary search to find first message that ends after scroll position
        let first_msg = cumulative
            .binary_search(&scroll)
            .unwrap_or_else(|i| i)
            .min(cumulative.len().saturating_sub(1));

        // Lines to skip in the first message
        let skip_lines = if first_msg > 0 {
            scroll.saturating_sub(cumulative[first_msg - 1])
        } else {
            scroll
        };

        // Find last visible message
        let end_line = scroll + visible_height;
        let last_msg = cumulative
            .binary_search(&end_line)
            .unwrap_or_else(|i| i)
            .min(cumulative.len().saturating_sub(1));

        (first_msg, last_msg, skip_lines)
    }

    /// Render streaming lines with incremental caching.
    ///
    /// Key optimization: We cache rendered lines and only re-render when text CHANGES
    /// (not when it grows). For streaming, text typically only appends, so we can
    /// often skip re-rendering entirely for segments that haven't changed.
    fn render_streaming_lines_cached(&mut self, lines: &mut Vec<Line<'static>>, theme: &Theme) {
        // Invalidate cache if width changed
        if self.streaming_cache.width != self.render_width {
            self.streaming_cache.clear();
            self.streaming_cache.width = self.render_width;
        }

        let mut text_segment_idx = 0;
        let mut total_cached_lines = 0usize;

        for segment in &self.stream_segments {
            match segment {
                StreamSegment::Text(text) => {
                    if !text.is_empty() {
                        // Check cache for this segment
                        let cached = self.streaming_cache.segment_cache.get(text_segment_idx);

                        // Use cache if:
                        // 1. We have a cache entry for this segment
                        // 2. The cached length matches OR the text is a prefix extension
                        //    (common case: streaming appends to existing text)
                        let (use_cache, needs_rerender) = if let Some((cached_len, _)) = cached {
                            if *cached_len == text.len() {
                                (true, false) // Exact match - use cache
                            } else {
                                // Text changed - need to re-render
                                // (Could optimize to only render new portion, but markdown
                                // context makes this complex)
                                (false, true)
                            }
                        } else {
                            (false, true) // No cache - need to render
                        };

                        if use_cache {
                            let (_, cached_lines) =
                                &self.streaming_cache.segment_cache[text_segment_idx];
                            for line in cached_lines {
                                let mut new_line = vec![Span::styled("  ", theme.text_style())];
                                new_line.extend(line.spans.iter().cloned());
                                lines.push(Line::from(new_line));
                            }
                            total_cached_lines += cached_lines.len();
                        } else if needs_rerender {
                            // Render the text
                            let content_text = render_markdown_with_settings(
                                text,
                                theme,
                                self.render_width,
                                &self.render_settings,
                            );
                            let rendered_lines: Vec<Line<'static>> =
                                content_text.lines.into_iter().collect();

                            // Add to output
                            for line in &rendered_lines {
                                let mut new_line = vec![Span::styled("  ", theme.text_style())];
                                new_line.extend(line.spans.iter().cloned());
                                lines.push(Line::from(new_line));
                            }

                            total_cached_lines += rendered_lines.len();

                            // Update cache
                            if text_segment_idx < self.streaming_cache.segment_cache.len() {
                                self.streaming_cache.segment_cache[text_segment_idx] =
                                    (text.len(), rendered_lines);
                            } else {
                                self.streaming_cache
                                    .segment_cache
                                    .push((text.len(), rendered_lines));
                            }
                        }
                    }
                    text_segment_idx += 1;
                }
                StreamSegment::Tool(index) => {
                    if let Some(tool) = self.active_tools.get(*index) {
                        self.render_tool_call(lines, tool, theme);
                        total_cached_lines += 5; // Estimate for tool display
                    }
                }
            }
        }

        self.streaming_cache.total_cached_lines = total_cached_lines;
        self.streaming_cache.valid = true;

        // Streaming cursor
        lines.push(Line::from(vec![
            Span::styled("  ", theme.text_style()),
            Span::styled("▌", theme.primary_style()),
        ]));
    }

    fn render_message(
        &self,
        lines: &mut Vec<Line<'static>>,
        msg: &DisplayMessage,
        theme: &Theme,
        is_selected: bool,
    ) {
        let agent_color = theme.agent_color(msg.agent);

        // When selected, add a visual indicator
        let selection_indicator = if is_selected { "▶ " } else { "" };
        let text_style = if is_selected {
            theme.text_style().add_modifier(Modifier::REVERSED)
        } else {
            theme.text_style()
        };

        match msg.role {
            MessageRole::User => {
                // User message with left border
                lines.push(Line::from(vec![
                    Span::styled(selection_indicator, theme.accent_style()),
                    Span::styled("┃ ", Style::default().fg(agent_color)),
                    Span::styled("You", text_style.add_modifier(Modifier::BOLD)),
                ]));

                // Calculate available width for content (accounting for prefix)
                let prefix_len = if is_selected { 4 } else { 2 }; // "  ┃ " or "┃ "
                let content_width = self.render_width.saturating_sub(prefix_len);

                // Content with left border continuation and wrapping
                for line in msg.content.lines() {
                    let content_line = Line::from(Span::styled(line.to_string(), text_style));
                    let wrapped = wrap_line(content_line, content_width);
                    for wrapped_line in wrapped {
                        let mut new_line = vec![
                            Span::styled(if is_selected { "  " } else { "" }, theme.text_style()),
                            Span::styled("┃ ", Style::default().fg(agent_color)),
                        ];
                        new_line.extend(wrapped_line.spans);
                        lines.push(Line::from(new_line));
                    }
                }
            }
            MessageRole::Assistant => {
                // Selection indicator for assistant messages
                if is_selected {
                    lines.push(Line::from(vec![
                        Span::styled("▶ ", theme.accent_style()),
                        Span::styled("[selected - press y to copy]", theme.muted_style()),
                    ]));
                }

                // If we have segments, use them to preserve text/tool order
                if !msg.segments.is_empty() {
                    // Ensure segment cache is populated (renders markdown once per width change)
                    msg.ensure_segment_cache(self.render_width, theme, &self.render_settings);

                    // Track which text segment we're on for cache lookup
                    let mut text_segment_idx = 0;

                    for segment in &msg.segments {
                        match segment {
                            MessageSegment::Text(_) => {
                                // Use cached rendered lines instead of re-parsing markdown
                                let cached_lines = msg.get_segment_lines(text_segment_idx);
                                text_segment_idx += 1;

                                for line in cached_lines {
                                    let mut new_line = vec![Span::styled("  ", theme.text_style())];
                                    if is_selected {
                                        for span in line.spans {
                                            new_line.push(Span::styled(
                                                span.content.to_string(),
                                                span.style.add_modifier(Modifier::REVERSED),
                                            ));
                                        }
                                    } else {
                                        new_line.extend(line.spans.into_iter());
                                    }
                                    lines.push(Line::from(new_line));
                                }
                            }
                            MessageSegment::Tool(tool) => {
                                self.render_tool_call(lines, tool, theme);
                            }
                        }
                    }
                } else {
                    // Legacy fallback: render content then tools (with caching)
                    let cached_lines =
                        msg.get_or_render_content(self.render_width, theme, &self.render_settings);
                    for line in cached_lines {
                        let mut new_line = vec![Span::styled("  ", theme.text_style())];
                        if is_selected {
                            for span in line.spans {
                                new_line.push(Span::styled(
                                    span.content.to_string(),
                                    span.style.add_modifier(Modifier::REVERSED),
                                ));
                            }
                        } else {
                            new_line.extend(line.spans.into_iter());
                        }
                        lines.push(Line::from(new_line));
                    }

                    // Tool calls (legacy)
                    for tool in &msg.tool_calls {
                        self.render_tool_call(lines, tool, theme);
                    }
                }

                // Completion indicator
                if msg.model.is_some() || msg.duration.is_some() {
                    let mut completion_spans = vec![
                        Span::styled("  ", theme.text_style()),
                        Span::styled("▣ ", Style::default().fg(agent_color)),
                        Span::styled(msg.agent.name().to_string(), theme.text_style()),
                    ];

                    if let Some(model) = &msg.model {
                        completion_spans.push(Span::styled(" · ", theme.muted_style()));
                        completion_spans.push(Span::styled(model.clone(), theme.muted_style()));
                    }

                    if let Some(duration) = &msg.duration {
                        completion_spans.push(Span::styled(" · ", theme.muted_style()));
                        completion_spans.push(Span::styled(duration.clone(), theme.muted_style()));
                    }

                    lines.push(Line::from(completion_spans));
                }
            }
            MessageRole::System => {
                // System messages display with a subtle style
                // The message content may include icons like ⬡ or ◇
                lines.push(Line::from(vec![
                    Span::styled("  ", theme.text_style()),
                    Span::styled(msg.content.clone(), theme.muted_style()),
                ]));
            }
            MessageRole::Tool => {
                // Tool result rendered inline
                lines.push(Line::from(vec![
                    Span::styled("  ", theme.text_style()),
                    Span::styled(msg.content.clone(), theme.muted_style()),
                ]));
            }
        }
    }

    fn render_tool_call(
        &self,
        lines: &mut Vec<Line<'static>>,
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        let icon = tool_icon(&tool.name);
        let is_block = is_block_tool(&tool.name);
        let (title, params) = tool_title(&tool.name, tool.input.as_deref(), tool.metadata.as_ref());

        let (status_icon, status_style) = match tool.status {
            ToolStatus::Pending => ("○", theme.muted_style()),
            ToolStatus::Running => ("●", theme.warning_style()),
            ToolStatus::Success => ("●", theme.success_style()),
            ToolStatus::Error => ("●", theme.error_style()),
        };

        // Build the params string if present
        let params_span = params.map(|p| format!(" [{p}]"));

        if is_block {
            // Block tools: bordered container with background
            // Top border
            let mut header_spans = vec![
                Span::styled("  ╭─ ", theme.tool_border_style()),
                Span::styled(
                    format!("{icon} "),
                    theme.accent_style().add_modifier(Modifier::BOLD),
                ),
                Span::styled(title, theme.muted_style()),
            ];
            if let Some(ref p) = params_span {
                header_spans.push(Span::styled(p.clone(), theme.dim_style()));
            }
            header_spans.push(Span::styled(" ", theme.text_style()));
            header_spans.push(Span::styled(status_icon, status_style));
            lines.push(Line::from(header_spans));

            // Tool-specific content
            self.render_block_tool_content(lines, tool, theme);

            // Bottom border
            lines.push(Line::from(vec![Span::styled(
                "  ╰─",
                theme.tool_border_style(),
            )]));
        } else {
            // Inline tools: just the tool line with minimal formatting
            let mut spans = vec![
                Span::styled("  ", theme.text_style()),
                Span::styled(
                    format!("{icon} "),
                    theme.accent_style().add_modifier(Modifier::BOLD),
                ),
                Span::styled(title, theme.muted_style()),
            ];
            if let Some(ref p) = params_span {
                spans.push(Span::styled(p.clone(), theme.dim_style()));
            }
            spans.push(Span::styled(" ", theme.text_style()));
            spans.push(Span::styled(status_icon, status_style));
            lines.push(Line::from(spans));
        }
    }

    fn render_block_tool_content(
        &self,
        lines: &mut Vec<Line<'static>>,
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        // Parse input for tool-specific content
        let input: serde_json::Value = tool
            .input
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);

        match tool.name.as_str() {
            "bash" => {
                // Show the command
                if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled("$ ", theme.accent_style()),
                        Span::styled(cmd.to_string(), theme.text_style()),
                    ]));
                }
            }
            "edit" | "write" => {
                // Show the file path
                if let Some(path) = input.get("filePath").and_then(|v| v.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled(path.to_string(), theme.muted_style()),
                    ]));
                }
            }
            "read" => {
                // Show the file path
                if let Some(path) = input.get("filePath").and_then(|v| v.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled(path.to_string(), theme.muted_style()),
                    ]));
                }
            }
            "glob" => {
                // Show the pattern
                if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled("pattern: ", theme.muted_style()),
                        Span::styled(pattern.to_string(), theme.accent_style()),
                    ]));
                }
            }
            "grep" => {
                // Show the pattern
                if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled("pattern: ", theme.muted_style()),
                        Span::styled(pattern.to_string(), theme.accent_style()),
                    ]));
                }
            }
            _ => {}
        }

        // Show output preview for completed tools
        // Note: We always show some output indicator for block tools to give user feedback
        // Debug: Show status for troubleshooting
        if tool.status == ToolStatus::Success || tool.status == ToolStatus::Error {
            match &tool.output {
                Some(output) if !output.is_empty() => {
                    let output_lines: Vec<&str> = output.lines().collect();
                    let total_lines = output_lines.len();

                    if total_lines > 0 {
                        // Tool-specific rendering
                        match tool.name.as_str() {
                            "edit" => {
                                // Render colored diff
                                self.render_diff_output(lines, &output_lines, tool, theme);
                            }
                            "read" => {
                                // Render file content preview
                                self.render_read_output(lines, &output_lines, tool, theme);
                            }
                            "glob" | "grep" => {
                                // Render match preview
                                self.render_search_output(lines, &output_lines, tool, theme);
                            }
                            "write" => {
                                // Render write preview from metadata if available
                                self.render_write_output(lines, output, tool, theme);
                            }
                            _ => {
                                // Default rendering for bash, task, webfetch, etc.
                                self.render_default_output(lines, &output_lines, tool, theme);
                            }
                        }
                    } else {
                        // Output has content but no lines (shouldn't happen)
                        lines.push(Line::from(vec![
                            Span::styled("  │ ", theme.tool_border_style()),
                            Span::styled("(empty output)", theme.dim_style()),
                        ]));
                    }
                }
                Some(_) => {
                    // Output is empty string
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled("(no output)", theme.dim_style()),
                    ]));
                }
                None => {
                    // Output is None - shouldn't happen for completed tools
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled("(output not captured)", theme.dim_style()),
                    ]));
                }
            }
        }
    }

    /// Toggle expansion of all tool outputs in a specific message.
    pub fn toggle_tool_expansion(&mut self, message_index: usize) {
        let visible_count = self.revert_index.unwrap_or(self.messages.len());
        if message_index >= visible_count {
            return;
        }

        let msg = &mut self.messages[message_index];

        // Toggle all tools in this message
        let any_collapsed = msg.tool_calls.iter().any(|t| !t.expanded);

        for tool in &mut msg.tool_calls {
            tool.expanded = any_collapsed; // Expand all if any collapsed, otherwise collapse all
        }

        // Also handle segments
        for segment in &mut msg.segments {
            if let MessageSegment::Tool(ref mut tool) = segment {
                tool.expanded = any_collapsed;
            }
        }
    }

    /// Toggle expansion of tools in the currently selected message (in selection mode).
    pub fn toggle_selected_tool_expansion(&mut self) {
        if self.selection.active {
            self.toggle_tool_expansion(self.selection.message_index);
        }
    }

    /// Render colored diff output for edit tool.
    fn render_diff_output(
        &self,
        lines: &mut Vec<Line<'static>>,
        output_lines: &[&str],
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        // Reduced limits for better scroll performance
        let max_lines = if tool.expanded { 50 } else { 10 };
        let total = output_lines.len();

        for line in output_lines.iter().take(max_lines) {
            let (style, prefix) = if line.starts_with('+') && !line.starts_with("+++") {
                (theme.diff_added_style(), "+ ")
            } else if line.starts_with('-') && !line.starts_with("---") {
                (theme.diff_removed_style(), "- ")
            } else if line.starts_with("@@") {
                (theme.diff_hunk_style(), "@ ")
            } else {
                (theme.muted_style(), "  ")
            };

            let content = line
                .trim_start_matches(&['+', '-', '@', ' '][..])
                .to_string();
            let truncated = if content.chars().count() > 70 && !tool.expanded {
                let truncated_content: String = content.chars().take(67).collect();
                format!("{truncated_content}...")
            } else {
                content
            };

            lines.push(Line::from(vec![
                Span::styled("  │ ", theme.tool_border_style()),
                Span::styled(prefix, style),
                Span::styled(truncated, style),
            ]));
        }

        if total > max_lines {
            lines.push(Line::from(vec![
                Span::styled("  │ ", theme.tool_border_style()),
                Span::styled(
                    format!("... {} more lines", total - max_lines),
                    theme.dim_style(),
                ),
            ]));
        }

        self.render_expand_hint(lines, tool, total, theme);
    }

    /// Render file content preview for read tool with syntax highlighting.
    fn render_read_output(
        &self,
        lines: &mut Vec<Line<'static>>,
        output_lines: &[&str],
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        // Reduced limits for better scroll performance
        let max_lines = if tool.expanded { 50 } else { 8 };
        let total = output_lines.len();

        // Extract file path from input to determine language for syntax highlighting
        let input: serde_json::Value = tool
            .input
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);

        let file_path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
        let language = crate::widgets::syntax::language_from_path(file_path);

        // Extract content lines (stripping line number prefix but preserving whitespace)
        let content_lines: Vec<String> = output_lines
            .iter()
            .take(max_lines)
            .map(|line| {
                if let Some(idx) = line.find('|') {
                    // Skip the '|' and the single tab that follows, but preserve the rest
                    let after_pipe = &line[idx + 1..];
                    after_pipe
                        .strip_prefix('\t')
                        .unwrap_or(after_pipe)
                        .to_string()
                } else {
                    (*line).to_string()
                }
            })
            .collect();

        // Apply syntax highlighting if language is detected
        if !language.is_empty() {
            let code = content_lines.join("\n");
            let highlighted = crate::widgets::syntax::highlight_code(&code, language, theme);

            for highlighted_line in highlighted {
                let mut new_line = vec![Span::styled("  │ ", theme.tool_border_style())];
                new_line.extend(highlighted_line.spans);
                lines.push(Line::from(new_line));
            }
        } else {
            // Fallback to plain code style (no syntax highlighting)
            for content in content_lines {
                let truncated = if content.chars().count() > 70 && !tool.expanded {
                    let t: String = content.chars().take(67).collect();
                    format!("{t}...")
                } else {
                    content
                };

                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled(truncated, theme.code_style()),
                ]));
            }
        }

        if total > max_lines {
            lines.push(Line::from(vec![
                Span::styled("  │ ", theme.tool_border_style()),
                Span::styled(
                    format!("... {} more lines", total - max_lines),
                    theme.dim_style(),
                ),
            ]));
        }

        self.render_expand_hint(lines, tool, total, theme);
    }

    /// Render search results for glob/grep tools.
    fn render_search_output(
        &self,
        lines: &mut Vec<Line<'static>>,
        output_lines: &[&str],
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        let max_lines = if tool.expanded { 50 } else { 5 };
        let total = output_lines.len();

        for line in output_lines.iter().take(max_lines) {
            let truncated = if line.chars().count() > 70 && !tool.expanded {
                let t: String = line.chars().take(67).collect();
                format!("{t}...")
            } else {
                (*line).to_string()
            };

            lines.push(Line::from(vec![
                Span::styled("  │ ", theme.tool_border_style()),
                Span::styled(truncated, theme.muted_style()),
            ]));
        }

        if total > max_lines {
            lines.push(Line::from(vec![
                Span::styled("  │ ", theme.tool_border_style()),
                Span::styled(
                    format!("... {} more matches", total - max_lines),
                    theme.dim_style(),
                ),
            ]));
        }

        self.render_expand_hint(lines, tool, total, theme);
    }

    /// Render write tool output with preview from metadata and syntax highlighting.
    fn render_write_output(
        &self,
        lines: &mut Vec<Line<'static>>,
        output: &str,
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        // Extract file path from input to determine language for syntax highlighting
        let input: serde_json::Value = tool
            .input
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);

        let file_path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
        let language = crate::widgets::syntax::language_from_path(file_path);

        // Check if metadata has preview
        if let Some(metadata) = &tool.metadata {
            if let Some(preview) = metadata.get("preview").and_then(|v| v.as_str()) {
                let preview_lines: Vec<&str> = preview.lines().collect();
                let max_lines = if tool.expanded { 50 } else { 10 };
                let total = preview_lines.len();

                // Apply syntax highlighting if language is detected
                if !language.is_empty() {
                    let code: String = preview_lines
                        .iter()
                        .take(max_lines)
                        .copied()
                        .collect::<Vec<_>>()
                        .join("\n");
                    let highlighted =
                        crate::widgets::syntax::highlight_code(&code, language, theme);

                    for highlighted_line in highlighted {
                        let mut new_line = vec![Span::styled("  │ ", theme.tool_border_style())];
                        new_line.extend(highlighted_line.spans);
                        lines.push(Line::from(new_line));
                    }
                } else {
                    // Fallback to plain code style
                    for line in preview_lines.iter().take(max_lines) {
                        let truncated = if line.chars().count() > 70 && !tool.expanded {
                            let t: String = line.chars().take(67).collect();
                            format!("{t}...")
                        } else {
                            (*line).to_string()
                        };

                        lines.push(Line::from(vec![
                            Span::styled("  │ ", theme.tool_border_style()),
                            Span::styled(truncated, theme.code_style()),
                        ]));
                    }
                }

                if total > max_lines {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", theme.tool_border_style()),
                        Span::styled(
                            format!("... {} more lines", total - max_lines),
                            theme.dim_style(),
                        ),
                    ]));
                }

                self.render_expand_hint(lines, tool, total, theme);
                return;
            }
        }

        // Fallback to default output rendering
        let output_lines: Vec<&str> = output.lines().collect();
        self.render_default_output(lines, &output_lines, tool, theme);
    }

    /// Render default output for bash, task, webfetch, etc.
    fn render_default_output(
        &self,
        lines: &mut Vec<Line<'static>>,
        output_lines: &[&str],
        tool: &DisplayToolCall,
        theme: &Theme,
    ) {
        let total_lines = output_lines.len();
        let style = if tool.status == ToolStatus::Error {
            theme.error_style()
        } else {
            theme.muted_style()
        };

        let truncate_line = |line: &str, expanded: bool| -> String {
            let max_len = if expanded { 200 } else { 70 };
            let char_count = line.chars().count();
            if char_count > max_len {
                let truncated: String = line.chars().take(max_len.saturating_sub(3)).collect();
                format!("{truncated}...")
            } else {
                line.to_string()
            }
        };

        // Reduced limits for better scroll performance
        let show_full = tool.expanded || total_lines <= 10;

        if show_full {
            let max_display_lines = if tool.expanded { 50 } else { 10 };
            let display_lines = output_lines.len().min(max_display_lines);

            for line in output_lines.iter().take(display_lines) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled(truncate_line(line, tool.expanded), style),
                ]));
            }

            if output_lines.len() > max_display_lines {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled(
                        format!(
                            "... {} more lines (truncated at {}) ...",
                            output_lines.len() - max_display_lines,
                            max_display_lines
                        ),
                        theme.dim_style(),
                    ),
                ]));
            }

            if tool.expanded && total_lines > 15 {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled("[press ", theme.dim_style()),
                    Span::styled("o", theme.accent_style()),
                    Span::styled(" to collapse]", theme.dim_style()),
                ]));
            }
        } else {
            // Show preview (first 2, hidden count, last 2)
            for line in output_lines.iter().take(2) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled(truncate_line(line, false), style),
                ]));
            }

            let hidden_lines = total_lines - 4;
            lines.push(Line::from(vec![
                Span::styled("  │ ", theme.tool_border_style()),
                Span::styled(format!("... {hidden_lines} more lines "), theme.dim_style()),
                Span::styled("[press ", theme.dim_style()),
                Span::styled("o", theme.accent_style()),
                Span::styled(" to expand]", theme.dim_style()),
            ]));

            for line in output_lines.iter().skip(total_lines - 2) {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled(truncate_line(line, false), style),
                ]));
            }
        }
    }

    /// Render expand/collapse hint.
    fn render_expand_hint(
        &self,
        lines: &mut Vec<Line<'static>>,
        tool: &DisplayToolCall,
        total_lines: usize,
        theme: &Theme,
    ) {
        let threshold = match tool.name.as_str() {
            "read" => 10,
            "glob" | "grep" => 5,
            "edit" => 20,
            _ => 15,
        };

        if total_lines > threshold {
            if tool.expanded {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled("[press ", theme.dim_style()),
                    Span::styled("o", theme.accent_style()),
                    Span::styled(" to collapse]", theme.dim_style()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme.tool_border_style()),
                    Span::styled("[press ", theme.dim_style()),
                    Span::styled("o", theme.accent_style()),
                    Span::styled(" to expand]", theme.dim_style()),
                ]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_mode() {
        let mut widget = MessagesWidget::new();

        // Add some messages
        widget.add_message(DisplayMessage::user("Hello"));
        widget.add_message(DisplayMessage::assistant("Hi there"));
        widget.add_message(DisplayMessage::user("How are you?"));
        widget.add_message(DisplayMessage::assistant("I'm doing well"));

        // Initially not in selection mode
        assert!(!widget.is_selecting());
        assert!(widget.get_selected_content().is_none());

        // Enter selection mode
        widget.enter_selection_mode();
        assert!(widget.is_selecting());

        // Should select the last assistant message (index 3)
        assert_eq!(widget.selection.message_index, 3);

        // Get selected content
        let content = widget.get_selected_content();
        assert!(content.is_some());
        assert_eq!(content.unwrap(), "I'm doing well");

        // Navigate to previous message (user message at index 2)
        widget.select_prev_message();
        assert_eq!(widget.selection.message_index, 2);
        assert_eq!(widget.get_selected_content().unwrap(), "How are you?");

        // Navigate to previous message (assistant message at index 1)
        widget.select_prev_message();
        assert_eq!(widget.selection.message_index, 1);
        assert_eq!(widget.get_selected_content().unwrap(), "Hi there");

        // Navigate to next message
        widget.select_next_message();
        assert_eq!(widget.selection.message_index, 2);

        // Exit selection mode
        widget.exit_selection_mode();
        assert!(!widget.is_selecting());
        assert!(widget.get_selected_content().is_none());
    }

    #[test]
    fn test_selection_with_no_messages() {
        let mut widget = MessagesWidget::new();

        // Enter selection mode with no messages
        widget.enter_selection_mode();

        // Should not be in selection mode
        assert!(!widget.is_selecting());
    }

    #[test]
    fn test_selection_with_only_user_messages() {
        let mut widget = MessagesWidget::new();

        widget.add_message(DisplayMessage::user("Hello"));
        widget.add_message(DisplayMessage::user("World"));

        // Enter selection mode
        widget.enter_selection_mode();
        assert!(widget.is_selecting());

        // Should select the last message (index 1) since no assistant messages
        assert_eq!(widget.selection.message_index, 1);
        assert_eq!(widget.get_selected_content().unwrap(), "World");
    }

    #[test]
    fn test_extract_inline_code() {
        // Single inline code
        let codes = MessagesWidget::extract_inline_code("Use `cargo build` to compile");
        assert_eq!(codes, vec!["cargo build"]);

        // Multiple inline codes
        let codes = MessagesWidget::extract_inline_code("Run `npm install` then `npm start`");
        assert_eq!(codes, vec!["npm install", "npm start"]);

        // No inline code
        let codes = MessagesWidget::extract_inline_code("Just plain text here");
        assert!(codes.is_empty());

        // Empty inline code (should be ignored)
        let codes = MessagesWidget::extract_inline_code("Empty `` code");
        assert!(codes.is_empty());

        // Complex inline code
        let codes =
            MessagesWidget::extract_inline_code("The function `fn main() {}` is the entry point");
        assert_eq!(codes, vec!["fn main() {}"]);
    }

    #[test]
    fn test_extract_code_blocks() {
        let content =
            "Some text\n```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\nMore text";
        let blocks = MessagesWidget::extract_code_blocks_from_content(content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "rust");
        assert_eq!(blocks[0].1, "fn main() {\n    println!(\"Hello\");\n}");

        // Multiple code blocks
        let content = "```python\nprint('hello')\n```\ntext\n```js\nconsole.log('hi')\n```";
        let blocks = MessagesWidget::extract_code_blocks_from_content(content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, "python");
        assert_eq!(blocks[1].0, "js");
    }
}
