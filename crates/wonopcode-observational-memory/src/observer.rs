//! Observer Agent for Observational Memory.
//!
//! The Observer watches the conversation and produces structured observations.
//! It compresses raw messages into a dense, dated event log.
//!
//! # Observation Format
//!
//! ```text
//! Date: 2026-03-04
//! - 🔴 14:22 User is building a Next.js app with Supabase auth
//!   - 🔴 14:22 App uses server components with client-side hydration
//!   - 🟡 14:25 User asked about middleware configuration
//! - 🟡 14:35 Agent ran test suite, 3 failures in auth middleware
//!   - 🟢 14:35 Failures relate to missing session cookie
//! ```
//!
//! # Priority Levels
//!
//! - 🔴 High: Critical context (requirements, architecture, deadlines, corrections)
//! - 🟡 Medium: Potentially relevant (questions, intermediate results)
//! - 🟢 Low: Informational only (routine operations, minor details)

use crate::observation::{Observation, ObservationCategory, Priority};
use crate::observation_parser::ObservationParser;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use wonopcode_provider::{BoxedLanguageModel, GenerateOptions, Message, StreamChunk};

/// Find a safe byte index for slicing a string that respects UTF-8 character boundaries.
/// Returns the largest valid index <= `target` that is a character boundary.
fn safe_byte_index(s: &str, target: usize) -> usize {
    if target >= s.len() {
        return s.len();
    }
    // Walk backwards from target until we find a char boundary
    let mut idx = target;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Safely truncate a string from the end, finding a valid UTF-8 boundary.
/// Returns the smallest valid index >= `target` that is a character boundary.
fn safe_byte_index_from_end(s: &str, bytes_from_end: usize) -> usize {
    if bytes_from_end >= s.len() {
        return 0;
    }
    let target = s.len() - bytes_from_end;
    let mut idx = target;
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Configuration for the Observer agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverConfig {
    /// Provider ID to use for the Observer LLM call.
    /// Default: "anthropic" (uses same provider as main agent)
    pub provider_id: String,
    
    /// Model ID for Observer. Should be fast and cheap.
    /// Default: "claude-3-5-haiku-latest"
    pub model_id: String,
    
    /// Maximum tokens for Observer output.
    /// Default: 4096
    pub max_output_tokens: u32,
    
    /// Temperature for Observer (low for consistency).
    /// Default: 0.3
    pub temperature: f32,
    
    /// Whether to include system prompt context in observations.
    /// Default: false (system prompt is always available)
    pub include_system_context: bool,
    
    /// Default confidence for observations when not specified.
    /// Default: 0.8
    pub default_confidence: f32,
    
    /// Target compression ratio (input tokens / output tokens).
    /// Used for logging/metrics, not enforced.
    /// Default: 5.0 (5:1 compression)
    pub target_compression_ratio: f32,
}

impl Default for ObserverConfig {
    fn default() -> Self {
        Self {
            provider_id: "anthropic".to_string(),
            model_id: "claude-3-5-haiku-latest".to_string(),
            max_output_tokens: 4096,
            temperature: 0.3,
            include_system_context: false,
            default_confidence: 0.8,
            target_compression_ratio: 5.0,
        }
    }
}

impl ObserverConfig {
    /// Create config for a specific provider/model.
    pub fn with_model(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            ..Default::default()
        }
    }
    
    /// Create config optimized for speed (uses fastest available model).
    pub fn fast() -> Self {
        Self {
            provider_id: "anthropic".to_string(),
            model_id: "claude-3-5-haiku-latest".to_string(),
            max_output_tokens: 2048,
            temperature: 0.2,
            ..Default::default()
        }
    }
    
    /// Create config optimized for quality (uses more capable model).
    pub fn quality() -> Self {
        Self {
            provider_id: "anthropic".to_string(),
            model_id: "claude-sonnet-4-20250514".to_string(),
            max_output_tokens: 8192,
            temperature: 0.4,
            ..Default::default()
        }
    }
}

/// The Observer prompt template.
pub const OBSERVER_SYSTEM_PROMPT: &str = r#"You are a Memory Observer. Extract observations from the conversation.

CRITICAL: Your output MUST use EXACTLY this format. NO YAML. NO JSON. NO other formats.

REQUIRED OUTPUT FORMAT:
Date: YYYY-MM-DD
- 🔴 HH:MM observation text here
  - 🔴 HH:MM child observation text
- 🟡 HH:MM another observation
- 🟢 HH:MM low priority observation

---
Current Task: what the agent is doing right now
Suggested Response: how the agent should continue

EXAMPLE OUTPUT:
Date: 2026-03-05
- 🔴 14:22 User is building a Next.js dashboard with Supabase auth
  - 🔴 14:22 Project name is "Acme Dashboard"
  - 🟡 14:25 Asked about middleware configuration
- 🔴 14:35 Test suite: 47 passed, 3 failed (auth_test.rs: lines 42, 67)
  - 🔴 14:38 Agent fixed by adding mock session provider
- 🟢 14:40 Routine file save to src/config.ts

---
Current Task: Implementing the --verbose flag
Suggested Response: Add verbose flag to clap configuration

PRIORITY LEVELS:
🔴 HIGH = critical context (requirements, architecture, failures, corrections)
🟡 MEDIUM = potentially relevant (questions, intermediate results)
🟢 LOW = routine operations (file saves, successful tests)

RULES:
1. BE SPECIFIC: "🔴 Renamed auth → identity in src/modules/" NOT "🟡 Renamed a module"
2. PRESERVE NUMBERS: "🔴 Tests: 47 passed, 3 failed" NOT "🟡 Some tests failed"  
3. COMPRESS TOOL OUTPUT: Extract the key result, not raw output
4. USE EMOJI PREFIX: Every observation line starts with 🔴, 🟡, or 🟢
5. USE BULLET FORMAT: Every observation starts with "- " (dash space)

DO NOT:
- Output YAML or JSON format
- Write summaries or paragraphs
- Skip the Date: header
- Forget the emoji prefix on each line
"#;

/// Build the Observer user prompt from messages to observe.
pub fn build_observer_prompt(
    messages: &[Message],
    current_date: DateTime<Utc>,
) -> String {
    let mut prompt = String::new();
    
    prompt.push_str("# Messages to Observe\n\n");
    prompt.push_str(&format!("Current date/time: {}\n\n", current_date.format("%Y-%m-%d %H:%M:%S UTC")));
    
    for (idx, msg) in messages.iter().enumerate() {
        let role_str = format!("{:?}", msg.role).to_lowercase();
        prompt.push_str(&format!("## Message {} ({})\n\n", idx + 1, role_str));
        
        for part in &msg.content {
            match part {
                wonopcode_provider::ContentPart::Text { text } => {
                    // Truncate very long text (use safe byte indices for UTF-8)
                    if text.len() > 10000 {
                        let start_end = safe_byte_index(text, 5000);
                        let tail_start = safe_byte_index_from_end(text, 2000);
                        prompt.push_str(&text[..start_end]);
                        prompt.push_str("\n\n[... truncated ...]\n\n");
                        prompt.push_str(&text[tail_start..]);
                    } else {
                        prompt.push_str(text);
                    }
                    prompt.push_str("\n\n");
                }
                wonopcode_provider::ContentPart::ToolUse { id, name, input } => {
                    prompt.push_str(&format!("**Tool Call: {}** (id: {})\n", name, id));
                    // Truncate large tool inputs (use safe byte indices for UTF-8)
                    let input_str = input.to_string();
                    if input_str.len() > 2000 {
                        let end_idx = safe_byte_index(&input_str, 1000);
                        prompt.push_str(&input_str[..end_idx]);
                        prompt.push_str("\n[... truncated ...]\n");
                    } else {
                        prompt.push_str(&input_str);
                    }
                    prompt.push_str("\n\n");
                }
                wonopcode_provider::ContentPart::ToolResult { tool_use_id, content, .. } => {
                    prompt.push_str(&format!("**Tool Result** (id: {})\n", tool_use_id));
                    // Truncate large tool results (use safe byte indices for UTF-8)
                    if content.len() > 5000 {
                        let start_end = safe_byte_index(content, 2500);
                        let tail_start = safe_byte_index_from_end(content, 1000);
                        prompt.push_str(&content[..start_end]);
                        prompt.push_str("\n\n[... truncated ...]\n\n");
                        prompt.push_str(&content[tail_start..]);
                    } else {
                        prompt.push_str(content);
                    }
                    prompt.push_str("\n\n");
                }
                wonopcode_provider::ContentPart::Thinking { text } => {
                    prompt.push_str("**[Agent Thinking]**\n");
                    // Use safe byte indices for UTF-8
                    if text.len() > 2000 {
                        let end_idx = safe_byte_index(text, 1000);
                        prompt.push_str(&text[..end_idx]);
                        prompt.push_str("\n[... truncated ...]\n");
                    } else {
                        prompt.push_str(text);
                    }
                    prompt.push_str("\n\n");
                }
                _ => {
                    // Skip other content types
                }
            }
        }
        
        prompt.push_str("---\n\n");
    }
    
    prompt.push_str("\n# Instructions\n\n");
    prompt.push_str("Produce your observation log using EXACTLY this format:\n\n");
    prompt.push_str("Date: YYYY-MM-DD\n");
    prompt.push_str("- 🔴 HH:MM observation\n");
    prompt.push_str("- 🟡 HH:MM observation\n\n");
    prompt.push_str("---\nCurrent Task: ...\nSuggested Response: ...\n\n");
    prompt.push_str("IMPORTANT: Start with 'Date:' header. Use bullet points with emoji prefixes. NO YAML or JSON.\n");
    
    prompt
}

/// Result from the Observer agent.
#[derive(Debug, Clone)]
pub struct ObserverResult {
    /// Extracted observations.
    pub observations: Vec<Observation>,
    /// Current task extracted from response.
    pub current_task: Option<String>,
    /// Suggested response extracted from response.
    pub suggested_response: Option<String>,
    /// Raw output from the Observer (for debugging).
    pub raw_output: String,
    /// Token counts for metrics.
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl ObserverResult {
    /// Calculate compression ratio.
    pub fn compression_ratio(&self) -> f32 {
        if self.output_tokens == 0 {
            return 0.0;
        }
        self.input_tokens as f32 / self.output_tokens as f32
    }
}

/// Parse the Observer's raw output into structured observations.
pub fn parse_observer_output(
    raw_output: &str,
    default_confidence: f32,
) -> ObserverResult {
    let mut result = ObserverResult {
        observations: Vec::new(),
        current_task: None,
        suggested_response: None,
        raw_output: raw_output.to_string(),
        input_tokens: 0,
        output_tokens: 0,
    };
    
    // Split off the special fields section
    let (observation_text, special_fields) = if let Some(idx) = raw_output.rfind("---") {
        let (obs, special) = raw_output.split_at(idx);
        (obs.trim(), special.trim_start_matches("---").trim())
    } else {
        (raw_output.trim(), "")
    };
    
    // Parse observations
    let parser = ObservationParser::new()
        .lenient(true)
        .with_confidence(default_confidence);
    
    // Log the first 500 chars of observation_text for debugging (use safe indexing)
    let preview = if observation_text.len() > 500 {
        let safe_end = safe_byte_index(observation_text, 500);
        format!("{}...", &observation_text[..safe_end])
    } else {
        observation_text.to_string()
    };
    tracing::debug!("Observer raw output preview: {}", preview);
    
    match parser.parse(observation_text) {
        Ok(date_groups) => {
            let total_obs: usize = date_groups.iter().map(|g| g.observations.len()).sum();
            tracing::debug!("Parsed {} date groups with {} total observations", date_groups.len(), total_obs);
            for group in date_groups {
                result.observations.extend(group.observations);
            }
        }
        Err(e) => {
            tracing::warn!("Observer output parsing failed: {:?}. Raw output preview: {}", e, preview);
            // Fall back to creating a single observation from the raw text
            if !observation_text.is_empty() {
                result.observations.push(Observation::new(
                    Priority::Medium,
                    ObservationCategory::Context,
                    observation_text,
                    default_confidence,
                ));
            }
        }
    }
    
    // Parse special fields
    for line in special_fields.lines() {
        let line = line.trim();
        if let Some(task) = line.strip_prefix("Current Task:") {
            result.current_task = Some(task.trim().to_string());
        } else if let Some(response) = line.strip_prefix("Suggested Response:") {
            result.suggested_response = Some(response.trim().to_string());
        }
    }
    
    result
}

/// The Observer agent that compresses messages into observations.
pub struct ObserverAgent {
    /// Configuration for the observer.
    config: ObserverConfig,
    /// The LLM provider to use for observation.
    provider: BoxedLanguageModel,
}

impl ObserverAgent {
    /// Create a new Observer agent.
    pub fn new(config: ObserverConfig, provider: BoxedLanguageModel) -> Self {
        Self { config, provider }
    }
    
    /// Create an Observer with default config using the provided provider.
    pub fn with_provider(provider: BoxedLanguageModel) -> Self {
        Self::new(ObserverConfig::default(), provider)
    }
    
    /// Run the Observer on a set of messages.
    ///
    /// Returns observations extracted from the messages.
    pub async fn run(
        &self,
        messages: &[Message],
        current_date: DateTime<Utc>,
    ) -> Result<ObserverResult, ObserverError> {
        if messages.is_empty() {
            return Ok(ObserverResult {
                observations: Vec::new(),
                current_task: None,
                suggested_response: None,
                raw_output: String::new(),
                input_tokens: 0,
                output_tokens: 0,
            });
        }
        
        // Build the observer prompt
        let user_prompt = build_observer_prompt(messages, current_date);
        
        // Estimate input tokens (rough)
        let input_tokens = (OBSERVER_SYSTEM_PROMPT.len() + user_prompt.len()) / 4;
        
        // Create messages for the LLM
        let observer_messages = vec![
            Message::system(OBSERVER_SYSTEM_PROMPT),
            Message::user(&user_prompt),
        ];
        
        // Configure generation options
        let options = GenerateOptions {
            max_tokens: Some(self.config.max_output_tokens),
            temperature: Some(self.config.temperature),
            tools: Vec::new(),
            ..Default::default()
        };
        
        // Call the LLM
        let mut stream = self.provider
            .generate(observer_messages, options)
            .await
            .map_err(|e| ObserverError::ProviderError(e.to_string()))?;
        
        // Collect the response
        let mut response_text = String::new();
        let mut output_tokens = 0u32;
        
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => match chunk {
                    StreamChunk::TextDelta(text) => {
                        response_text.push_str(&text);
                    }
                    StreamChunk::FinishStep { usage, .. } => {
                        output_tokens = usage.output_tokens;
                        // We're done when we get a finish step
                        break;
                    }
                    StreamChunk::Error(e) => {
                        return Err(ObserverError::StreamError(e));
                    }
                    _ => {} // Ignore other chunk types (tool calls, start/end markers, etc.)
                },
                Err(e) => {
                    return Err(ObserverError::ProviderError(e.to_string()));
                }
            }
        }
        
        // Parse the response
        let mut result = parse_observer_output(&response_text, self.config.default_confidence);
        result.input_tokens = input_tokens as u32;
        result.output_tokens = if output_tokens > 0 {
            output_tokens
        } else {
            // Estimate if not provided
            (response_text.len() / 4) as u32
        };
        
        tracing::debug!(
            "Observer completed: {} observations, {:.1}x compression",
            result.observations.len(),
            result.compression_ratio()
        );
        
        Ok(result)
    }
    
    /// Get the observer configuration.
    pub fn config(&self) -> &ObserverConfig {
        &self.config
    }
}

/// Error type for Observer operations.
#[derive(Debug, Clone)]
pub enum ObserverError {
    /// Error from the LLM provider.
    ProviderError(String),
    /// Error from the streaming response.
    StreamError(String),
    /// Error parsing the observer output.
    ParseError(String),
    /// No messages to observe.
    NoMessages,
}

impl std::fmt::Display for ObserverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderError(e) => write!(f, "Provider error: {}", e),
            Self::StreamError(e) => write!(f, "Stream error: {}", e),
            Self::ParseError(e) => write!(f, "Parse error: {}", e),
            Self::NoMessages => write!(f, "No messages to observe"),
        }
    }
}

impl std::error::Error for ObserverError {}

/// Coding-specific priority classification hints.
/// These help the Observer make better priority decisions.
pub struct CodingPriorityHints {
    /// Patterns that indicate high priority.
    pub high_priority_patterns: Vec<&'static str>,
    /// Patterns that indicate low priority.
    pub low_priority_patterns: Vec<&'static str>,
}

impl Default for CodingPriorityHints {
    fn default() -> Self {
        Self {
            high_priority_patterns: vec![
                "renamed", "refactored", "architecture", "framework",
                "database", "deadline", "requirement", "constraint",
                "test failed", "error:", "panic", "bug fix",
                "no, ", "actually", "correction", "I meant",
                "configuration", "environment", "secret", "key",
            ],
            low_priority_patterns: vec![
                "file written successfully",
                "test passed",
                "compilation successful",
                "no warnings",
                "formatting applied",
                "dependencies installed",
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_observer_config_default() {
        let config = ObserverConfig::default();
        assert_eq!(config.provider_id, "anthropic");
        assert!(config.model_id.contains("haiku"));
        assert_eq!(config.temperature, 0.3);
    }
    
    #[test]
    fn test_parse_observer_output_simple() {
        let output = r#"Date: 2026-03-05
- 🔴 14:22 User is building a Rust CLI tool
  - 🔴 14:22 Using clap for argument parsing
- 🟡 14:25 Agent created main.rs skeleton

---
Current Task: Implementing the --verbose flag
Suggested Response: Add the verbose flag to the clap configuration
"#;
        
        let result = parse_observer_output(output, 0.8);
        
        assert_eq!(result.observations.len(), 2);
        assert_eq!(result.observations[0].priority, Priority::High);
        assert_eq!(result.observations[0].children.len(), 1);
        assert_eq!(result.current_task, Some("Implementing the --verbose flag".to_string()));
        assert!(result.suggested_response.unwrap().contains("verbose flag"));
    }
    
    #[test]
    fn test_parse_observer_output_no_special_fields() {
        let output = r#"Date: 2026-03-05
- 🔴 10:00 User wants to add logging
- 🟢 10:05 Routine file save
"#;
        
        let result = parse_observer_output(output, 0.8);
        
        assert_eq!(result.observations.len(), 2);
        assert!(result.current_task.is_none());
        assert!(result.suggested_response.is_none());
    }
    
    #[test]
    fn test_build_observer_prompt() {
        use wonopcode_provider::Role;
        
        let messages = vec![
            Message {
                role: Role::User,
                content: vec![wonopcode_provider::ContentPart::Text {
                    text: "Hello, please help me write a test".to_string(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![wonopcode_provider::ContentPart::Text {
                    text: "I'll help you write a test. Let me analyze the code.".to_string(),
                }],
            },
        ];
        
        let prompt = build_observer_prompt(&messages, Utc::now());
        
        assert!(prompt.contains("Messages to Observe"));
        assert!(prompt.contains("Message 1 (user)"));
        assert!(prompt.contains("Message 2 (assistant)"));
        assert!(prompt.contains("help me write a test"));
    }
}
