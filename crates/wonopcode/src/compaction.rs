//! Smart session compaction.
//!
//! Compaction happens in two phases:
//! 1. **Prune phase**: Mark old tool outputs as compacted (>40K tokens ago)
//! 2. **Summarize phase**: AI summarization of older messages
//!

use futures::StreamExt;
use tracing::{debug, info, warn};
use wonopcode_provider::{
    BoxedLanguageModel, ContentPart, GenerateOptions, Message as ProviderMessage, Role, StreamChunk,
};

/// Minimum tokens of tool outputs to prune (20K tokens).
pub const PRUNE_MINIMUM: u32 = 20_000;

/// Token threshold to protect recent tool outputs (40K tokens).
/// Tool outputs within this threshold won't be pruned.
pub const PRUNE_PROTECT: u32 = 40_000;

/// Tools whose outputs are never pruned.
pub const PROTECTED_TOOLS: &[&str] = &["skill"];

/// Default output token reserve.
pub const OUTPUT_TOKEN_MAX: u32 = 16_000;

/// Configuration for compaction behavior.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Whether automatic compaction is enabled.
    /// Controlled by config.compaction.auto
    pub auto: bool,

    /// Whether pruning is enabled.
    /// Controlled by config.compaction.prune
    pub prune: bool,

    /// Number of recent messages (user turns) to preserve.
    /// Default: 2 turns before considering for prune
    pub preserve_turns: usize,

    /// Maximum output tokens to reserve.
    pub output_reserve: u32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto: true,
            prune: true,
            preserve_turns: 2,
            output_reserve: OUTPUT_TOKEN_MAX,
        }
    }
}

/// Token usage from a response.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input: u32,
    pub output: u32,
    pub cache_read: u32,
    pub cache_write: u32,
}

#[allow(dead_code)] // Public API methods for library consumers
impl TokenUsage {
    /// Get total tokens used (input + cache_read + output).
    pub fn total(&self) -> u32 {
        self.input + self.cache_read + self.output
    }

    /// Create from provider usage values.
    /// Useful for creating TokenUsage from actual API response values.
    /// Part of public API for library consumers.
    pub fn from_provider(input: u32, output: u32, cache_read: u32, cache_write: u32) -> Self {
        Self {
            input,
            output,
            cache_read,
            cache_write,
        }
    }

    /// Add usage from another TokenUsage.
    /// Useful for accumulating usage across multiple API calls.
    /// Part of public API for library consumers.
    pub fn add(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }
}

/// Check if context is overflowing (needs compaction).
pub fn is_overflow(tokens: &TokenUsage, context_limit: u32, output_reserve: u32) -> bool {
    if context_limit == 0 {
        return false;
    }

    let count = tokens.total();
    let output = output_reserve.min(OUTPUT_TOKEN_MAX);
    let usable = context_limit.saturating_sub(output);

    count > usable
}

/// Result of a compaction operation.
#[derive(Debug)]
#[allow(dead_code)] // Fields are part of public API for library consumers
pub enum CompactionResult {
    /// No compaction needed.
    NotNeeded,
    /// Compaction needed but not enough messages.
    InsufficientMessages,
    /// Compaction performed successfully.
    Compacted {
        /// The compacted messages to use.
        messages: Vec<ProviderMessage>,
        /// The AI-generated summary text (for logging/display).
        /// Available for callers who want to log or display the compaction summary.
        summary: String,
        /// Number of messages that were summarized.
        messages_summarized: usize,
    },
    /// Compaction failed.
    Failed(String),
}

/// Represents a message part that can be pruned.
/// This struct documents the pruning data model. The actual implementation
/// uses inline tuples for efficiency, but this serves as reference documentation.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Documentation-only struct showing the pruning data model
struct PrunablePart {
    /// Message index in the array.
    pub message_index: usize,
    /// Part index within the message.
    pub part_index: usize,
    /// Tool name (for tool results).
    pub tool_name: Option<String>,
    /// Estimated token count.
    pub tokens: u32,
    /// Whether this part has been marked as compacted.
    pub compacted: bool,
}

/// Prune old tool outputs to reduce context usage.
///
/// Goes backwards through messages, protecting the last 40K tokens of tool
/// outputs, then marks older outputs as compacted if they would prune >20K tokens.
///
#[allow(clippy::cognitive_complexity)]
fn prune_tool_outputs(messages: &mut [ProviderMessage], config: &CompactionConfig) -> u32 {
    if !config.prune {
        return 0;
    }

    let mut total_tokens: u32 = 0;
    let mut prunable_tokens: u32 = 0;
    let mut parts_to_prune: Vec<(usize, usize)> = Vec::new();
    let mut turns_seen = 0;

    // Go backwards through messages
    for msg_idx in (0..messages.len()).rev() {
        let msg = &messages[msg_idx];

        // Count user turns
        if msg.role == Role::User {
            turns_seen += 1;
        }

        // Skip first 2 turns
        if turns_seen < config.preserve_turns {
            continue;
        }

        // Check for compaction marker (stop if we hit one)
        if is_compaction_message(msg) {
            break;
        }

        // Process parts backwards
        for part_idx in (0..msg.content.len()).rev() {
            let part = &msg.content[part_idx];

            if let ContentPart::ToolResult {
                tool_use_id,
                content,
                ..
            } = part
            {
                // Find the corresponding tool use to get the name
                let tool_name = find_tool_name(messages, tool_use_id);

                // Skip protected tools
                if let Some(ref name) = tool_name {
                    if PROTECTED_TOOLS.contains(&name.as_str()) {
                        continue;
                    }
                }

                // Check if already compacted (content is empty or marker)
                if content.is_empty() || content.starts_with("[compacted]") {
                    continue;
                }

                let estimate = estimate_tokens(content);
                total_tokens += estimate;

                // If we're past the protection threshold, mark for pruning
                if total_tokens > PRUNE_PROTECT {
                    prunable_tokens += estimate;
                    parts_to_prune.push((msg_idx, part_idx));
                }
            }
        }
    }

    debug!(
        total = total_tokens,
        prunable = prunable_tokens,
        parts = parts_to_prune.len(),
        "Prune analysis"
    );

    // Only prune if we'd save enough tokens
    if prunable_tokens < PRUNE_MINIMUM {
        return 0;
    }

    // Mark parts as compacted
    let mut pruned_count = 0;
    for (msg_idx, part_idx) in parts_to_prune {
        if let ContentPart::ToolResult { tool_use_id, .. } = &messages[msg_idx].content[part_idx] {
            messages[msg_idx].content[part_idx] = ContentPart::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: "[compacted]".to_string(),
                is_error: None,
            };
            pruned_count += 1;
        }
    }

    info!(
        pruned = pruned_count,
        tokens_saved = prunable_tokens,
        "Pruned tool outputs"
    );

    prunable_tokens
}

/// Check if a message is a compaction summary message.
fn is_compaction_message(msg: &ProviderMessage) -> bool {
    if msg.role != Role::Assistant {
        return false;
    }

    for part in &msg.content {
        if let ContentPart::Text { text } = part {
            if text.contains("[Previous conversation summary") || text.contains("[compacted]") {
                return true;
            }
        }
    }

    false
}

/// Find the tool name for a given tool_use_id by searching messages.
fn find_tool_name(messages: &[ProviderMessage], tool_use_id: &str) -> Option<String> {
    for msg in messages {
        for part in &msg.content {
            if let ContentPart::ToolUse { id, name, .. } = part {
                if id == tool_use_id {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

/// Estimate token count for text (roughly 4 chars per token).
fn estimate_tokens(text: &str) -> u32 {
    (text.len() / 4).max(1) as u32
}

/// System prompt for the compaction agent.
const COMPACTION_SYSTEM_PROMPT: &str = r#"You are a helpful AI assistant tasked with summarizing conversations.

When asked to summarize, provide a detailed but concise summary of the conversation.
Focus on information that would be helpful for continuing the conversation, including:
- What was done
- What is currently being worked on
- Which files are being modified
- What needs to be done next
- Key user requests, constraints, or preferences that should persist
- Important technical decisions and why they were made

Your summary should be comprehensive enough to provide context but concise enough to be quickly understood.

Format your response as a clear, structured summary. Do not include any preamble like "Here's a summary" - just provide the summary directly."#;

const COMPACTION_USER_PROMPT: &str = r#"Provide a detailed prompt for continuing our conversation above. Focus on information that would be helpful for continuing the conversation, including what we did, what we're doing, which files we're working on, and what we're going to do next considering new session will not have access to our conversation."#;

/// Perform full compaction: prune first, then summarize if needed.
///
/// 1. Prune old tool outputs
/// 2. If still over limit, create AI summary
/// 3. Optionally add "Continue if you have next steps" message
pub async fn compact(
    messages: &mut [ProviderMessage],
    provider: &BoxedLanguageModel,
    config: &CompactionConfig,
    tokens: &TokenUsage,
    context_limit: u32,
    auto_continue: bool,
) -> CompactionResult {
    // Phase 1: Prune tool outputs
    let pruned_tokens = prune_tool_outputs(messages, config);

    if pruned_tokens > 0 {
        debug!(pruned = pruned_tokens, "Pruned tool outputs");
    }

    // Check if we still need compaction after pruning
    let adjusted_tokens = TokenUsage {
        input: tokens.input.saturating_sub(pruned_tokens),
        output: tokens.output,
        cache_read: tokens.cache_read,
        cache_write: tokens.cache_write,
    };

    if !is_overflow(&adjusted_tokens, context_limit, config.output_reserve) {
        if pruned_tokens > 0 {
            return CompactionResult::Compacted {
                messages: messages.to_vec(),
                summary: String::new(),
                messages_summarized: 0,
            };
        }
        return CompactionResult::NotNeeded;
    }

    // Phase 2: AI summarization
    let mut result = compact_with_summary(messages, provider, config).await;

    // Phase 3: Add auto-continue message if requested
    if auto_continue {
        if let CompactionResult::Compacted {
            messages: ref mut msgs,
            ..
        } = result
        {
            // Add synthetic user message to continue
            msgs.push(ProviderMessage {
                role: Role::User,
                content: vec![ContentPart::text("Continue if you have next steps")],
            });
        }
    }

    result
}

/// Perform smart compaction by summarizing older messages.
///
/// This creates a summary of older messages using the AI, then returns
/// a new message list with the summary replacing the old messages.
pub async fn compact_with_summary(
    messages: &[ProviderMessage],
    provider: &BoxedLanguageModel,
    _config: &CompactionConfig,
) -> CompactionResult {
    // Need at least a few messages to summarize
    if messages.len() < 4 {
        return CompactionResult::InsufficientMessages;
    }

    // Find the split point: keep first message, summarize middle, keep recent
    // "Recent" = last 2 user-assistant exchanges (4 messages)
    let preserve_recent = 4.min(messages.len() - 1);
    let middle_end = messages.len().saturating_sub(preserve_recent);

    if middle_end <= 1 {
        return CompactionResult::InsufficientMessages;
    }

    let first_message = &messages[0];
    let messages_to_summarize = &messages[1..middle_end];
    let recent_messages = &messages[middle_end..];

    if messages_to_summarize.is_empty() {
        return CompactionResult::InsufficientMessages;
    }

    info!(
        first = 1,
        to_summarize = messages_to_summarize.len(),
        recent = recent_messages.len(),
        "Compacting messages with AI summary"
    );

    // Build conversation text for summarization
    let conversation_text = format_messages_for_summary(messages_to_summarize);

    // Create summarization request
    let summary_messages = vec![ProviderMessage {
        role: Role::User,
        content: vec![ContentPart::text(format!(
            "Here is a conversation to summarize:\n\n{conversation_text}\n\n{COMPACTION_USER_PROMPT}"
        ))],
    }];

    let options = GenerateOptions {
        system: Some(COMPACTION_SYSTEM_PROMPT.to_string()),
        temperature: Some(0.3), // Lower temperature for consistency
        max_tokens: Some(2000), // Reasonable limit for summaries
        ..Default::default()
    };

    // Generate summary
    let summary = match generate_summary(provider, summary_messages, options).await {
        Ok(s) => s,
        Err(e) => return CompactionResult::Failed(e),
    };

    if summary.is_empty() {
        return CompactionResult::Failed("Empty summary generated".to_string());
    }

    // Build new message list
    let mut new_messages = Vec::with_capacity(preserve_recent + 2);

    // Keep first message
    new_messages.push(first_message.clone());

    new_messages.push(ProviderMessage {
        role: Role::Assistant,
        content: vec![ContentPart::text(format!(
            "[Previous conversation summary ({} messages)]\n\n{}",
            messages_to_summarize.len(),
            summary
        ))],
    });

    // Add recent messages
    new_messages.extend(recent_messages.iter().cloned());

    CompactionResult::Compacted {
        messages: new_messages,
        summary,
        messages_summarized: messages_to_summarize.len(),
    }
}

/// Generate a summary using the provider.
async fn generate_summary(
    provider: &BoxedLanguageModel,
    messages: Vec<ProviderMessage>,
    options: GenerateOptions,
) -> Result<String, String> {
    let stream = provider
        .generate(messages, options)
        .await
        .map_err(|e| format!("Failed to start summary generation: {e}"))?;

    let mut stream = Box::pin(stream);
    let mut summary = String::new();

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(StreamChunk::TextDelta(text)) => {
                summary.push_str(&text);
            }
            Ok(StreamChunk::Error(e)) => {
                warn!("Error generating summary: {}", e);
                return Err(format!("Summary generation error: {e}"));
            }
            Err(e) => {
                warn!("Stream error: {}", e);
                return Err(format!("Stream error: {e}"));
            }
            _ => {}
        }
    }

    Ok(summary.trim().to_string())
}

/// Format messages for inclusion in summary prompt.
fn format_messages_for_summary(messages: &[ProviderMessage]) -> String {
    let mut output = String::new();

    for msg in messages {
        let role_label = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
            Role::Tool => "Tool",
        };

        output.push_str(&format!("--- {role_label} ---\n"));

        for part in &msg.content {
            match part {
                ContentPart::Text { text } => {
                    // Truncate very long text parts
                    if text.len() > 2000 {
                        output.push_str(&text[..2000]);
                        output.push_str("... [truncated]\n");
                    } else {
                        output.push_str(text);
                        output.push('\n');
                    }
                }
                ContentPart::ToolUse { name, input, .. } => {
                    output.push_str(&format!("[Tool: {name} with input: {input:?}]\n"));
                }
                ContentPart::ToolResult { content, .. } => {
                    // Skip compacted results
                    if content == "[compacted]" {
                        output.push_str("[Tool result: compacted]\n");
                    } else if content.len() > 500 {
                        output.push_str(&format!(
                            "[Tool result: {}... [truncated]]\n",
                            &content[..500]
                        ));
                    } else {
                        output.push_str(&format!("[Tool result: {content}]\n"));
                    }
                }
                ContentPart::Image { .. } => {
                    output.push_str("[Image]\n");
                }
                ContentPart::Thinking { text } => {
                    if text.len() > 500 {
                        output.push_str(&format!("[Thinking: {}... [truncated]]\n", &text[..500]));
                    } else {
                        output.push_str(&format!("[Thinking: {text}]\n"));
                    }
                }
            }
        }

        output.push('\n');
    }

    output
}

/// Legacy function for backward compatibility.
/// Check if compaction is needed based on estimated token usage.
pub fn needs_compaction(
    messages: &[ProviderMessage],
    context_limit: u32,
    config: &CompactionConfig,
) -> bool {
    if !config.auto {
        return false;
    }

    if messages.len() < 6 {
        return false;
    }

    let estimated_tokens = estimate_messages_tokens(messages);
    let threshold = context_limit.saturating_sub(config.output_reserve);

    debug!(
        estimated_tokens = estimated_tokens,
        threshold = threshold,
        context_limit = context_limit,
        messages = messages.len(),
        "Checking if compaction needed"
    );

    estimated_tokens > threshold
}

/// Estimate token count for a list of messages.
pub fn estimate_messages_tokens(messages: &[ProviderMessage]) -> u32 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Create estimated TokenUsage from messages (for pre-prompt compaction checks).
pub fn estimate_token_usage(messages: &[ProviderMessage]) -> TokenUsage {
    let estimated = estimate_messages_tokens(messages);
    TokenUsage {
        input: estimated,
        output: 0,
        cache_read: 0,
        cache_write: 0,
    }
}

/// Estimate token count for a single message.
pub fn estimate_message_tokens(msg: &ProviderMessage) -> u32 {
    let mut chars: usize = 0;

    for part in &msg.content {
        chars += match part {
            ContentPart::Text { text } => text.len(),
            ContentPart::ToolUse { name, input, .. } => name.len() + input.to_string().len(),
            ContentPart::ToolResult { content, .. } => content.len(),
            ContentPart::Image { .. } => 1000,
            ContentPart::Thinking { text } => text.len(),
        };
    }

    // Add overhead for role and structure
    chars += 20;

    // Rough estimate: ~4 chars per token
    (chars / 4) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_overflow() {
        let tokens = TokenUsage {
            input: 90_000,
            output: 5_000,
            cache_read: 0,
            cache_write: 0,
        };

        // 95K used, 100K limit, 16K reserve = should overflow
        assert!(is_overflow(&tokens, 100_000, 16_000));

        // 95K used, 200K limit = should not overflow
        assert!(!is_overflow(&tokens, 200_000, 16_000));
    }

    #[test]
    fn test_protected_tools() {
        assert!(PROTECTED_TOOLS.contains(&"skill"));
        assert!(!PROTECTED_TOOLS.contains(&"bash"));
    }

    #[test]
    fn test_needs_compaction() {
        let config = CompactionConfig::default();

        // Empty messages - no compaction
        assert!(!needs_compaction(&[], 100_000, &config));

        // Few messages - no compaction
        let few_messages: Vec<ProviderMessage> =
            (0..3).map(|_| ProviderMessage::user("test")).collect();
        assert!(!needs_compaction(&few_messages, 100_000, &config));
    }

    #[test]
    fn test_estimate_tokens() {
        let msg = ProviderMessage::user("Hello, this is a test message.");
        let tokens = estimate_message_tokens(&msg);
        // ~30 chars + 20 overhead = 50, /4 = 12-13 tokens
        assert!(tokens > 5 && tokens < 20);
    }

    #[test]
    fn test_is_compaction_message() {
        let normal_msg = ProviderMessage::assistant("Hello");
        assert!(!is_compaction_message(&normal_msg));

        let compaction_msg = ProviderMessage::assistant(
            "[Previous conversation summary (5 messages)]\n\nSummary here",
        );
        assert!(is_compaction_message(&compaction_msg));
    }

    #[test]
    fn test_prune_skips_protected() {
        // Create messages with skill tool result
        let mut messages = vec![
            ProviderMessage::user("test"),
            ProviderMessage::user("test2"),
            ProviderMessage::user("test3"),
            ProviderMessage {
                role: Role::Assistant,
                content: vec![ContentPart::ToolUse {
                    id: "tool1".to_string(),
                    name: "skill".to_string(),
                    input: serde_json::json!({}),
                }],
            },
            ProviderMessage {
                role: Role::Tool,
                content: vec![ContentPart::ToolResult {
                    tool_use_id: "tool1".to_string(),
                    content: "x".repeat(100_000), // Large output
                    is_error: None,
                }],
            },
        ];

        let config = CompactionConfig::default();
        let pruned = prune_tool_outputs(&mut messages, &config);

        // Should not prune skill tool output
        assert_eq!(pruned, 0);
    }
}
