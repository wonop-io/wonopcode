//! Smart session compaction.
//!
//! Compaction happens in two phases:
//! 1. **Prune phase**: Mark old tool outputs as compacted (>40K tokens ago)
//! 2. **Summarize phase**: AI summarization of older messages
//!

use futures::StreamExt;
use std::sync::Arc;
use tracing::{debug, info, warn};
use wonopcode_util::truncate_to_char_boundary;
use wonopcode_provider::{
    BoxedLanguageModel, ContentPart, GenerateOptions, Message as ProviderMessage, Role, StreamChunk,
};

/// Progress update for chunked summarization.
#[derive(Debug, Clone)]
pub struct CompactionProgress {
    /// Current chunk being processed (1-indexed).
    pub current_chunk: usize,
    /// Total number of chunks.
    pub total_chunks: usize,
    /// Phase: "summarizing" or "combining".
    pub phase: String,
}

/// Callback type for receiving compaction progress updates.
pub type ProgressCallback = Arc<dyn Fn(CompactionProgress) + Send + Sync>;

/// Minimum tokens of tool outputs to prune (20K tokens).
pub const PRUNE_MINIMUM: u32 = 20_000;

/// Token threshold to protect recent tool outputs (40K tokens).
/// Tool outputs within this threshold won't be pruned.
pub const PRUNE_PROTECT: u32 = 40_000;

/// Tools whose outputs are never pruned.
pub const PROTECTED_TOOLS: &[&str] = &["skill"];

/// Default output token reserve.
pub const OUTPUT_TOKEN_MAX: u32 = 16_000;

/// Number of recent messages to preserve during summarization.
/// When compacting, we keep the last N messages and summarize everything older.
pub const PRESERVE_RECENT_MESSAGES: usize = 50;

/// Threshold for triggering compaction based on message count.
/// When message count exceeds this, compaction is triggered.
pub const COMPACTION_MESSAGE_THRESHOLD: usize = 100;

/// Target token count after compaction.
/// After compaction, we aim to have at most this many tokens.
/// This ensures context stays manageable even with very long individual messages.
pub const TARGET_TOKENS_AFTER_COMPACTION: u32 = 50_000;

/// Maximum percentage of context that can be used for summarization input.
/// If messages to summarize exceed this, we use chunked summarization.
pub const MAX_SUMMARIZATION_CONTEXT_PERCENT: f32 = 0.40;

/// Default context limit to use when not specified by the model.
/// Most modern models support at least 128K tokens.
pub const DEFAULT_CONTEXT_LIMIT: u32 = 128_000;

/// Maximum recursion depth for hierarchical summarization.
/// Prevents infinite loops in edge cases.
pub const MAX_SUMMARIZATION_DEPTH: usize = 3;

/// Target tokens per chunk when doing chunked summarization.
/// Each chunk should be summarizable within context limits.
pub const CHUNK_TARGET_TOKENS: u32 = 30_000;

/// Configuration for compaction behavior.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Whether automatic compaction is enabled.
    /// Controlled by config.compaction.auto
    pub auto: bool,

    /// Whether pruning is enabled.
    /// Controlled by config.compaction.prune
    pub prune: bool,

    /// Number of recent messages (user turns) to preserve during pruning.
    /// Default: 2 turns before considering for prune
    pub preserve_turns: usize,

    /// Message count threshold for triggering compaction.
    /// Default: 100 messages
    pub compaction_threshold: usize,

    /// Number of recent messages to preserve during summarization.
    /// Default: 50 messages
    pub preserve_recent_messages: usize,

    /// Maximum output tokens to reserve.
    pub output_reserve: u32,

    /// Target token count after compaction.
    /// After compaction, we aim to have at most this many tokens.
    pub target_tokens: u32,

    /// Context limit for the model (used for chunked summarization).
    /// If not set, defaults to DEFAULT_CONTEXT_LIMIT.
    pub context_limit: u32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            auto: true,
            prune: true,
            preserve_turns: 2,
            compaction_threshold: COMPACTION_MESSAGE_THRESHOLD,
            preserve_recent_messages: PRESERVE_RECENT_MESSAGES,
            output_reserve: OUTPUT_TOKEN_MAX,
            target_tokens: TARGET_TOKENS_AFTER_COMPACTION,
            context_limit: DEFAULT_CONTEXT_LIMIT,
        }
    }
}

impl CompactionConfig {
    /// Create a config with a specific context limit.
    pub fn with_context_limit(mut self, limit: u32) -> Self {
        self.context_limit = limit;
        self
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
/// Strategy:
/// 1. Prune old tool outputs (mark as [compacted])
/// 2. If message count > compaction_threshold (default: 100), summarize older messages
/// 3. OR if token count > target_tokens (default: 50K), force summarization
/// 4. Keep the last preserve_recent_messages (default: 50) messages
/// 5. Optionally add "Continue if you have next steps" message
///
/// This ensures we always produce a summary when compacting, making the behavior
/// predictable. Summarization is triggered when either:
/// - Message count exceeds threshold (100+)
/// - Token count exceeds target (50K+) after pruning
///
/// The `progress_callback` parameter is optional and allows receiving progress
/// updates during chunked summarization (for large conversations).
pub async fn compact(
    messages: &mut [ProviderMessage],
    provider: &BoxedLanguageModel,
    config: &CompactionConfig,
    _tokens: &TokenUsage,
    _context_limit: u32,
    auto_continue: bool,
    progress_callback: Option<ProgressCallback>,
) -> CompactionResult {
    // Phase 1: Prune tool outputs (always do this to reduce token count)
    let pruned_tokens = prune_tool_outputs(messages, config);

    if pruned_tokens > 0 {
        debug!(pruned = pruned_tokens, "Pruned tool outputs");
    }

    // Re-estimate tokens after pruning
    let current_tokens = estimate_messages_tokens(messages);

    // Phase 2: Summarize if we have more messages than threshold OR tokens still too high
    // Trigger at 100 messages OR 50K tokens (after pruning)
    let needs_message_based_summarization = messages.len() > config.compaction_threshold;
    let needs_token_based_summarization = current_tokens > config.target_tokens;
    let needs_summarization = needs_message_based_summarization || needs_token_based_summarization;

    if needs_token_based_summarization && !needs_message_based_summarization {
        info!(
            current_tokens = current_tokens,
            target_tokens = config.target_tokens,
            message_count = messages.len(),
            "Token count exceeds target after pruning, forcing summarization"
        );
    }

    if !needs_summarization {
        // Not enough messages or tokens to summarize, but we may have pruned
        if pruned_tokens > 0 {
            return CompactionResult::Compacted {
                messages: messages.to_vec(),
                summary: String::new(),
                messages_summarized: 0,
            };
        }
        return CompactionResult::NotNeeded;
    }

    info!(
        message_count = messages.len(),
        threshold = config.compaction_threshold,
        preserve_recent = config.preserve_recent_messages,
        "Summarizing older messages"
    );

    // Phase 2: AI summarization - always run when we have enough messages
    let mut result = compact_with_summary(messages, provider, config, progress_callback).await;

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
///
/// Strategy: Keep the last N messages (default: 50) and summarize everything older.
/// For example, with 120 messages and preserve_recent_messages=50:
/// - Messages 1 (first user message) - kept as-is
/// - Messages 2-70 (69 messages) - summarized into one AI-generated summary
/// - Messages 71-120 (last 50) - kept as-is
/// - Result: 1 (first) + 1 (summary) + 50 (recent) = 52 messages
///
/// If there are fewer messages than preserve_recent_messages, we dynamically reduce
/// the number of recent messages to keep, ensuring we always summarize at least
/// half of the messages (leaving room for high-token individual messages).
///
/// For very large conversations that exceed the context limit, this function
/// automatically uses chunked summarization (Map-Reduce pattern) to handle
/// the messages in smaller pieces.
pub async fn compact_with_summary(
    messages: &[ProviderMessage],
    provider: &BoxedLanguageModel,
    config: &CompactionConfig,
    progress_callback: Option<ProgressCallback>,
) -> CompactionResult {
    // Need at least 4 messages to summarize meaningfully:
    // 1 first + at least 2 to summarize + 1 recent
    if messages.len() < 4 {
        return CompactionResult::InsufficientMessages;
    }

    // Dynamically calculate preserve_recent based on BOTH message count AND token budget.
    // We want to keep recent messages but must respect the target_tokens limit.
    
    // Step 1: Calculate message-based limit
    // - Default: config.preserve_recent_messages (e.g., 50)
    // - If fewer messages, keep at most half the messages
    let message_based_limit = config
        .preserve_recent_messages
        .min(messages.len() / 2)
        .max(1);
    
    // Step 2: Calculate token-based limit
    // Reserve tokens for: first message + summary (~5K) + some headroom
    let first_msg_tokens = estimate_message_tokens(&messages[0]);
    let summary_estimate = 5000_u32; // Estimate for the summary message
    let headroom = config.target_tokens / 10; // 10% headroom
    let available_for_recent = config.target_tokens
        .saturating_sub(first_msg_tokens)
        .saturating_sub(summary_estimate)
        .saturating_sub(headroom);
    
    // Calculate how many recent messages can fit in the token budget
    // Start from the end and count backwards until we exceed budget
    let mut token_based_limit = 0;
    let mut tokens_used = 0_u32;
    
    for msg in messages.iter().rev() {
        let msg_tokens = estimate_message_tokens(msg);
        if tokens_used + msg_tokens > available_for_recent {
            break;
        }
        tokens_used += msg_tokens;
        token_based_limit += 1;
    }
    
    // Use the smaller of message-based and token-based limits
    let preserve_recent = message_based_limit.min(token_based_limit).max(1);
    
    info!(
        "COMPACTION: preserve_recent calculation: message_limit={}, token_limit={}, chosen={}, target_tokens={}, available_for_recent={}",
        message_based_limit, token_based_limit, preserve_recent, config.target_tokens, available_for_recent
    );

    // Find the split point: keep first message, summarize middle, keep recent N messages
    // Example with 219 messages, preserve_recent=50:
    //   - first_message = messages[0]
    //   - messages_to_summarize = messages[1..169] (168 messages)
    //   - recent_messages = messages[169..219] (50 messages)
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

    // Check if messages exceed the safe summarization limit
    // If so, use chunked summarization (Map-Reduce pattern)
    let summary = if needs_chunked_summarization(messages_to_summarize, config) {
        info!(
            "COMPACTION: Using chunked summarization for {} messages (exceeds safe context limit)",
            messages_to_summarize.len()
        );
        
        match generate_chunked_summary(provider, messages_to_summarize, config, progress_callback).await {
            Ok(s) => s,
            Err(e) => {
                warn!("COMPACTION: Chunked summarization failed: {}", e);
                return CompactionResult::Failed(e);
            }
        }
    } else {
        // Standard single-pass summarization
        let conversation_text = format_messages_for_summary(messages_to_summarize);

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

        match generate_summary(provider, summary_messages, options).await {
            Ok(s) => s,
            Err(e) => return CompactionResult::Failed(e),
        }
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

/// Generate a summary using the provider with a timeout.
/// Compaction summaries should complete within 60 seconds.
async fn generate_summary(
    provider: &BoxedLanguageModel,
    messages: Vec<ProviderMessage>,
    options: GenerateOptions,
) -> Result<String, String> {
    // Wrap the actual summary generation in a timeout
    match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        generate_summary_inner(provider, messages, options)
    ).await {
        Ok(result) => result,
        Err(_) => {
            warn!("COMPACTION_DEBUG: Summary generation timed out after 120 seconds");
            Err("Summary generation timed out after 120 seconds".to_string())
        }
    }
}

/// Inner function for summary generation (called with timeout wrapper).
async fn generate_summary_inner(
    provider: &BoxedLanguageModel,
    messages: Vec<ProviderMessage>,
    options: GenerateOptions,
) -> Result<String, String> {
    info!("COMPACTION_DEBUG: generate_summary() starting, calling provider.generate()");
    let start = std::time::Instant::now();
    
    let stream = provider
        .generate(messages, options)
        .await
        .map_err(|e| {
            warn!("COMPACTION_DEBUG: provider.generate() failed: {}", e);
            format!("Failed to start summary generation: {e}")
        })?;
    
    info!("COMPACTION_DEBUG: provider.generate() returned successfully after {:?}, starting to consume stream", start.elapsed());

    let mut stream = Box::pin(stream);
    let mut summary = String::new();
    let mut chunk_count = 0;
    let mut last_chunk_time = std::time::Instant::now();

    while let Some(chunk_result) = stream.next().await {
        chunk_count += 1;
        let now = std::time::Instant::now();
        let chunk_interval = now.duration_since(last_chunk_time);
        last_chunk_time = now;
        
        // Log if chunks are arriving slowly (more than 5 seconds apart)
        if chunk_interval.as_secs() > 5 {
            debug!(
                "COMPACTION_DEBUG: Slow chunk - {} seconds since last chunk (chunk #{})", 
                chunk_interval.as_secs(), chunk_count
            );
        }
        
        match chunk_result {
            Ok(StreamChunk::TextDelta(text)) => {
                summary.push_str(&text);
                if chunk_count % 10 == 0 {
                    debug!("COMPACTION_DEBUG: Received {} chunks, summary length so far: {}", chunk_count, summary.len());
                }
            }
            Ok(StreamChunk::Error(e)) => {
                warn!("COMPACTION_DEBUG: Error generating summary at chunk {}: {}", chunk_count, e);
                return Err(format!("Summary generation error: {e}"));
            }
            Err(e) => {
                warn!("COMPACTION_DEBUG: Stream error at chunk {}: {}", chunk_count, e);
                return Err(format!("Stream error: {e}"));
            }
            _ => {}
        }
    }

    info!(
        "COMPACTION_DEBUG: generate_summary() completed after {:?}, received {} chunks, summary length: {}", 
        start.elapsed(), chunk_count, summary.len()
    );
    Ok(summary.trim().to_string())
}

/// Represents a chunk of messages to be summarized.
#[derive(Debug)]
struct MessageChunk {
    /// The messages in this chunk.
    messages: Vec<ProviderMessage>,
    /// Estimated token count for these messages.
    estimated_tokens: u32,
    /// Index of the first message in the original list (for logging).
    start_index: usize,
    /// Index of the last message in the original list (for logging).
    end_index: usize,
}

/// Check if messages need chunked summarization.
/// Returns true if the messages to summarize would exceed the safe context limit.
fn needs_chunked_summarization(messages: &[ProviderMessage], config: &CompactionConfig) -> bool {
    let estimated_tokens = estimate_messages_tokens(messages);
    let max_safe_tokens = (config.context_limit as f32 * MAX_SUMMARIZATION_CONTEXT_PERCENT) as u32;
    
    let needs_chunking = estimated_tokens > max_safe_tokens;
    
    if needs_chunking {
        info!(
            "CHUNKED_SUMMARIZATION: Messages exceed safe limit - estimated={}, max_safe={}, will use chunked approach",
            estimated_tokens, max_safe_tokens
        );
    }
    
    needs_chunking
}

/// Split messages into chunks that can be safely summarized.
/// 
/// Strategy:
/// 1. Target ~30K tokens per chunk (to stay well within context limits)
/// 2. Try to split at conversation boundaries (user messages)
/// 3. Never split in the middle of a tool call/result pair
fn chunk_messages_for_summarization(
    messages: &[ProviderMessage],
    target_tokens_per_chunk: u32,
) -> Vec<MessageChunk> {
    let mut chunks = Vec::new();
    let mut current_chunk_messages = Vec::new();
    let mut current_chunk_tokens: u32 = 0;
    let mut chunk_start_index = 0;
    
    for (i, msg) in messages.iter().enumerate() {
        let msg_tokens = estimate_message_tokens(msg);
        
        // Check if adding this message would exceed chunk target
        let would_exceed = current_chunk_tokens + msg_tokens > target_tokens_per_chunk;
        
        // Determine if this is a good split point (prefer user messages)
        let is_good_split_point = msg.role == Role::User && !current_chunk_messages.is_empty();
        
        // Split if we'd exceed AND we have a reasonable chunk AND this is a good split point
        // OR if we'd exceed by a lot (>50% over target)
        let should_split = would_exceed && (
            is_good_split_point || 
            current_chunk_tokens > target_tokens_per_chunk / 2
        );
        
        if should_split && !current_chunk_messages.is_empty() {
            // Save current chunk
            chunks.push(MessageChunk {
                messages: std::mem::take(&mut current_chunk_messages),
                estimated_tokens: current_chunk_tokens,
                start_index: chunk_start_index,
                end_index: i - 1,
            });
            current_chunk_tokens = 0;
            chunk_start_index = i;
        }
        
        current_chunk_messages.push(msg.clone());
        current_chunk_tokens += msg_tokens;
    }
    
    // Don't forget the last chunk
    if !current_chunk_messages.is_empty() {
        chunks.push(MessageChunk {
            messages: current_chunk_messages,
            estimated_tokens: current_chunk_tokens,
            start_index: chunk_start_index,
            end_index: messages.len() - 1,
        });
    }
    
    info!(
        "CHUNKED_SUMMARIZATION: Split {} messages into {} chunks",
        messages.len(),
        chunks.len()
    );
    
    for (i, chunk) in chunks.iter().enumerate() {
        debug!(
            "CHUNKED_SUMMARIZATION: Chunk {}: messages {}..{}, ~{} tokens",
            i + 1, chunk.start_index, chunk.end_index, chunk.estimated_tokens
        );
    }
    
    chunks
}

/// System prompt for chunk summarization (more focused than full summarization).
const CHUNK_SUMMARIZATION_PROMPT: &str = r#"You are summarizing a PORTION of a longer conversation. This is chunk {chunk_num} of {total_chunks}.

Provide a concise but complete summary of what happened in this portion. Include:
- Key actions taken
- Important decisions made
- Files modified or created
- Errors encountered and how they were resolved
- Any unfinished work

Be factual and concise. This summary will be combined with other chunk summaries to create a full conversation summary."#;

/// System prompt for combining chunk summaries into a final summary.
const COMBINE_SUMMARIES_PROMPT: &str = r#"You are combining multiple summaries of conversation chunks into a single coherent summary.

The following are summaries of consecutive parts of a conversation, in chronological order.
Combine them into a single, well-structured summary that:
- Preserves the chronological flow of events
- Removes redundancy between chunks
- Highlights the most important information for continuing the conversation
- Includes: what was done, what's being worked on, files involved, and next steps

Format your response as a clear, structured summary. Do not include any preamble."#;

/// Summarize a single chunk of messages.
async fn summarize_chunk(
    provider: &BoxedLanguageModel,
    chunk: &MessageChunk,
    chunk_num: usize,
    total_chunks: usize,
) -> Result<String, String> {
    let conversation_text = format_messages_for_summary(&chunk.messages);
    
    let system_prompt = CHUNK_SUMMARIZATION_PROMPT
        .replace("{chunk_num}", &chunk_num.to_string())
        .replace("{total_chunks}", &total_chunks.to_string());
    
    let summary_messages = vec![ProviderMessage {
        role: Role::User,
        content: vec![ContentPart::text(format!(
            "Summarize this portion of the conversation:\n\n{conversation_text}"
        ))],
    }];
    
    let options = GenerateOptions {
        system: Some(system_prompt),
        temperature: Some(0.3),
        max_tokens: Some(1500), // Smaller limit for chunk summaries
        ..Default::default()
    };
    
    info!(
        "CHUNKED_SUMMARIZATION: Summarizing chunk {}/{} ({} messages, ~{} tokens)",
        chunk_num, total_chunks, chunk.messages.len(), chunk.estimated_tokens
    );
    
    generate_summary(provider, summary_messages, options).await
}

/// Combine multiple chunk summaries into a final summary.
fn combine_chunk_summaries<'a>(
    provider: &'a BoxedLanguageModel,
    chunk_summaries: Vec<String>,
    config: &'a CompactionConfig,
    depth: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>> {
    Box::pin(async move {
        // Check if we need to recursively chunk the summaries
        let combined_text: String = chunk_summaries
            .iter()
            .enumerate()
            .map(|(i, s)| format!("--- Chunk {} Summary ---\n{}\n", i + 1, s))
            .collect();
        
        let combined_tokens = estimate_tokens(&combined_text);
        let max_safe_tokens = (config.context_limit as f32 * MAX_SUMMARIZATION_CONTEXT_PERCENT) as u32;
        
        // If combined summaries are still too large and we haven't hit max depth, recurse
        if combined_tokens > max_safe_tokens && depth < MAX_SUMMARIZATION_DEPTH {
            warn!(
                "CHUNKED_SUMMARIZATION: Combined summaries still too large ({} tokens > {} max), recursing (depth {})",
                combined_tokens, max_safe_tokens, depth + 1
            );
            
            // Create pseudo-messages from the summaries and re-chunk
            let summary_messages: Vec<ProviderMessage> = chunk_summaries
                .into_iter()
                .map(|s| ProviderMessage::assistant(&s))
                .collect();
            
            let chunks = chunk_messages_for_summarization(&summary_messages, CHUNK_TARGET_TOKENS);
            let mut new_summaries = Vec::new();
            
            for (i, chunk) in chunks.iter().enumerate() {
                match summarize_chunk(provider, chunk, i + 1, chunks.len()).await {
                    Ok(summary) => new_summaries.push(summary),
                    Err(e) => return Err(format!("Failed to summarize meta-chunk {}: {}", i + 1, e)),
                }
            }
            
            // Recurse to combine the new summaries
            return combine_chunk_summaries(provider, new_summaries, config, depth + 1).await;
        }
        
        // Combine summaries into final summary
        info!(
            "CHUNKED_SUMMARIZATION: Combining {} chunk summaries (~{} tokens) into final summary",
            chunk_summaries.len(), combined_tokens
        );
        
        let summary_messages = vec![ProviderMessage {
            role: Role::User,
            content: vec![ContentPart::text(format!(
                "Here are summaries of consecutive parts of a conversation:\n\n{combined_text}\n\nCombine these into a single coherent summary."
            ))],
        }];
        
        let options = GenerateOptions {
            system: Some(COMBINE_SUMMARIES_PROMPT.to_string()),
            temperature: Some(0.3),
            max_tokens: Some(2500), // Slightly larger for final combined summary
            ..Default::default()
        };
        
        generate_summary(provider, summary_messages, options).await
    })
}

/// Generate a summary using chunked approach for large conversations.
/// 
/// This handles the "prompt too long" error by:
/// 1. Splitting messages into manageable chunks (~30K tokens each)
/// 2. Summarizing each chunk independently
/// 3. Combining chunk summaries into a final summary
/// 4. Recursively chunking if the combined summaries are still too large
async fn generate_chunked_summary(
    provider: &BoxedLanguageModel,
    messages: &[ProviderMessage],
    config: &CompactionConfig,
    progress_callback: Option<ProgressCallback>,
) -> Result<String, String> {
    info!(
        "CHUNKED_SUMMARIZATION: Starting chunked summarization for {} messages",
        messages.len()
    );
    
    // Step 1: Split messages into chunks
    let chunks = chunk_messages_for_summarization(messages, CHUNK_TARGET_TOKENS);
    
    if chunks.is_empty() {
        return Err("No chunks created from messages".to_string());
    }
    
    if chunks.len() == 1 {
        // Only one chunk, try direct summarization
        info!("CHUNKED_SUMMARIZATION: Only one chunk, using direct summarization");
        // Emit progress for single chunk
        if let Some(ref cb) = progress_callback {
            cb(CompactionProgress {
                current_chunk: 1,
                total_chunks: 1,
                phase: "summarizing".to_string(),
            });
        }
        return summarize_chunk(provider, &chunks[0], 1, 1).await;
    }
    
    // Step 2: Summarize each chunk
    let mut chunk_summaries = Vec::new();
    let total_chunks = chunks.len();
    
    for (i, chunk) in chunks.iter().enumerate() {
        // Emit progress update
        if let Some(ref cb) = progress_callback {
            cb(CompactionProgress {
                current_chunk: i + 1,
                total_chunks,
                phase: "summarizing".to_string(),
            });
        }
        
        match summarize_chunk(provider, chunk, i + 1, total_chunks).await {
            Ok(summary) => {
                if summary.is_empty() {
                    warn!("CHUNKED_SUMMARIZATION: Chunk {} returned empty summary", i + 1);
                    // Use a placeholder instead of failing entirely
                    chunk_summaries.push(format!(
                        "[Chunk {} could not be summarized - contained {} messages]",
                        i + 1, chunk.messages.len()
                    ));
                } else {
                    chunk_summaries.push(summary);
                }
            }
            Err(e) => {
                warn!("CHUNKED_SUMMARIZATION: Failed to summarize chunk {}: {}", i + 1, e);
                // Continue with other chunks rather than failing entirely
                chunk_summaries.push(format!(
                    "[Chunk {} summarization failed: {}]",
                    i + 1, e
                ));
            }
        }
    }
    
    // Step 3: Combine chunk summaries
    // Emit progress for combining phase
    if let Some(ref cb) = progress_callback {
        cb(CompactionProgress {
            current_chunk: total_chunks,
            total_chunks,
            phase: "combining".to_string(),
        });
    }
    
    combine_chunk_summaries(provider, chunk_summaries, config, 0).await
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
                        output.push_str(truncate_to_char_boundary(text, 2000));
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
                            truncate_to_char_boundary(content, 500)
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
                        output.push_str(&format!(
                            "[Thinking: {}... [truncated]]\n",
                            truncate_to_char_boundary(text, 500)
                        ));
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

    #[test]
    fn test_needs_chunked_summarization() {
        let config = CompactionConfig::default();
        
        // Small messages - should not need chunking
        let small_messages: Vec<ProviderMessage> = (0..10)
            .map(|i| ProviderMessage::user(&format!("Message {}", i)))
            .collect();
        assert!(!needs_chunked_summarization(&small_messages, &config));
        
        // Very large messages - should need chunking
        // Max safe is 40% of 128K = ~51K tokens
        // Create messages with ~60K tokens total (240K chars / 4)
        let large_messages: Vec<ProviderMessage> = (0..20)
            .map(|_| ProviderMessage::user(&"x".repeat(12000))) // ~3K tokens each = 60K total
            .collect();
        assert!(needs_chunked_summarization(&large_messages, &config));
    }

    #[test]
    fn test_chunk_messages_for_summarization() {
        // Create 10 messages with ~5K tokens each = 50K total
        let messages: Vec<ProviderMessage> = (0..10)
            .map(|i| {
                if i % 3 == 0 {
                    ProviderMessage::user(&"x".repeat(20000)) // ~5K tokens
                } else {
                    ProviderMessage::assistant(&"y".repeat(20000)) // ~5K tokens
                }
            })
            .collect();
        
        // Target 15K tokens per chunk - should create ~3-4 chunks
        let chunks = chunk_messages_for_summarization(&messages, 15_000);
        
        // Should have multiple chunks
        assert!(chunks.len() >= 2, "Expected multiple chunks, got {}", chunks.len());
        
        // All messages should be accounted for
        let total_messages: usize = chunks.iter().map(|c| c.messages.len()).sum();
        assert_eq!(total_messages, messages.len());
        
        // Chunks should try to split at user messages (conversation boundaries)
        for chunk in &chunks {
            // Each chunk should have reasonable token count (not drastically over target)
            assert!(
                chunk.estimated_tokens < 25_000,
                "Chunk has {} tokens, expected < 25K",
                chunk.estimated_tokens
            );
        }
    }
}
