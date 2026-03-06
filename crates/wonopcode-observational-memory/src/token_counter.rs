//! Token counting for Observational Memory.
//!
//! This module provides token estimation using the `chars_per_token` ratio
//! from `ModelInfo.tokenizer`. This enables fast, accurate token counting
//! without expensive API calls.
//!
//! # Token Estimation
//!
//! Different models have different tokenizers with varying average characters per token:
//! - Claude models: ~3.8 chars/token
//! - GPT-4/GPT-5: ~4.0 chars/token  
//! - Gemini: ~3.5 chars/token
//!
//! The estimation formula is: `tokens ≈ chars / chars_per_token`

use serde::{Deserialize, Serialize};

/// Configuration for token counting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCounterConfig {
    /// Characters per token ratio for the current model.
    /// Default: 4.0 (safe fallback)
    pub chars_per_token: f32,
    
    /// Overhead per message (for role markers, separators, etc.)
    /// Default: 4 tokens
    pub message_overhead: u32,
    
    /// Overhead for tool call formatting
    /// Default: 20 tokens
    pub tool_call_overhead: u32,
}

impl Default for TokenCounterConfig {
    fn default() -> Self {
        Self {
            chars_per_token: 4.0,
            message_overhead: 4,
            tool_call_overhead: 20,
        }
    }
}

impl TokenCounterConfig {
    /// Create config from a chars_per_token ratio.
    pub fn from_ratio(chars_per_token: f32) -> Self {
        Self {
            chars_per_token,
            ..Default::default()
        }
    }
    
    /// Create config for Claude models (3.8 chars/token).
    pub fn claude() -> Self {
        Self::from_ratio(3.8)
    }
    
    /// Create config for OpenAI GPT models (4.0 chars/token).
    pub fn openai() -> Self {
        Self::from_ratio(4.0)
    }
    
    /// Create config for Google Gemini models (3.5 chars/token).
    pub fn gemini() -> Self {
        Self::from_ratio(3.5)
    }
}

/// Token counter for estimating token usage.
#[derive(Debug, Clone)]
pub struct TokenCounter {
    config: TokenCounterConfig,
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self {
            config: TokenCounterConfig::default(),
        }
    }
}

impl TokenCounter {
    /// Create a new token counter with the given config.
    pub fn new(config: TokenCounterConfig) -> Self {
        Self { config }
    }
    
    /// Create a token counter from a chars_per_token ratio.
    pub fn from_ratio(chars_per_token: f32) -> Self {
        Self::new(TokenCounterConfig::from_ratio(chars_per_token))
    }
    
    /// Get the chars per token ratio.
    pub fn chars_per_token(&self) -> f32 {
        self.config.chars_per_token
    }
    
    /// Estimate tokens for a string.
    pub fn count_str(&self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        
        let char_count = s.chars().count();
        (char_count as f32 / self.config.chars_per_token).ceil() as u32
    }
    
    /// Estimate tokens for a message (includes overhead).
    pub fn count_message(&self, content: &str) -> u32 {
        self.count_str(content) + self.config.message_overhead
    }
    
    /// Estimate tokens for a tool call (includes overhead).
    pub fn count_tool_call(&self, name: &str, input: &str, output: &str) -> u32 {
        self.count_str(name)
            + self.count_str(input)
            + self.count_str(output)
            + self.config.tool_call_overhead
    }
    
    /// Estimate tokens for multiple strings.
    pub fn count_many<S: AsRef<str>>(&self, strings: &[S]) -> u32 {
        strings.iter().map(|s| self.count_str(s.as_ref())).sum()
    }
    
    /// Estimate tokens for a conversation (system + messages).
    pub fn count_conversation<S: AsRef<str>>(
        &self,
        system_prompt: &str,
        messages: &[S],
    ) -> u32 {
        let system_tokens = self.count_message(system_prompt);
        let message_tokens: u32 = messages
            .iter()
            .map(|m| self.count_message(m.as_ref()))
            .sum();
        
        system_tokens + message_tokens
    }
}

/// Statistics about token counting operations.
#[derive(Debug, Clone, Default)]
pub struct TokenCountStats {
    /// Total characters counted.
    pub total_chars: u64,
    /// Total tokens estimated.
    pub total_tokens: u64,
    /// Number of count operations.
    pub count_operations: u64,
}

impl TokenCountStats {
    /// Record a count operation.
    pub fn record(&mut self, chars: usize, tokens: u32) {
        self.total_chars += chars as u64;
        self.total_tokens += tokens as u64;
        self.count_operations += 1;
    }
    
    /// Get the actual chars per token ratio observed.
    pub fn observed_ratio(&self) -> f32 {
        if self.total_tokens == 0 {
            return 0.0;
        }
        self.total_chars as f32 / self.total_tokens as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_token_counter_default() {
        let counter = TokenCounter::default();
        assert_eq!(counter.chars_per_token(), 4.0);
    }
    
    #[test]
    fn test_count_str() {
        let counter = TokenCounter::from_ratio(4.0);
        
        // 40 chars / 4.0 = 10 tokens
        assert_eq!(counter.count_str("This is exactly forty characters text.!"), 10);
        
        // Empty string
        assert_eq!(counter.count_str(""), 0);
        
        // Single char
        assert_eq!(counter.count_str("a"), 1); // ceil(1/4) = 1
    }
    
    #[test]
    fn test_count_str_different_ratios() {
        let text = "This is a test string with about 40 chars.";
        
        // Claude (3.8 chars/token) - more tokens for same text
        let claude = TokenCounter::new(TokenCounterConfig::claude());
        let claude_tokens = claude.count_str(text);
        
        // GPT (4.0 chars/token)
        let gpt = TokenCounter::new(TokenCounterConfig::openai());
        let gpt_tokens = gpt.count_str(text);
        
        // Gemini (3.5 chars/token) - even more tokens
        let gemini = TokenCounter::new(TokenCounterConfig::gemini());
        let gemini_tokens = gemini.count_str(text);
        
        // Lower chars/token ratio = more tokens
        assert!(gemini_tokens >= claude_tokens);
        assert!(claude_tokens >= gpt_tokens);
    }
    
    #[test]
    fn test_count_message() {
        let counter = TokenCounter::default();
        
        let content = "Hello world"; // ~3 tokens
        let tokens = counter.count_message(content);
        
        // Should include overhead
        assert!(tokens > counter.count_str(content));
        assert_eq!(tokens, counter.count_str(content) + 4); // default overhead
    }
    
    #[test]
    fn test_count_tool_call() {
        let counter = TokenCounter::default();
        
        let tokens = counter.count_tool_call("read_file", "{\"path\": \"/test\"}", "file contents here");
        
        // Should include tool_call_overhead
        let content_tokens = counter.count_str("read_file")
            + counter.count_str("{\"path\": \"/test\"}")
            + counter.count_str("file contents here");
        
        assert_eq!(tokens, content_tokens + 20); // default tool overhead
    }
    
    #[test]
    fn test_count_conversation() {
        let counter = TokenCounter::default();
        
        let system = "You are a helpful assistant.";
        let messages = vec!["Hello", "How are you?", "I'm good, thanks!"];
        
        let total = counter.count_conversation(system, &messages);
        
        // Should be sum of all messages with overhead
        let expected = counter.count_message(system)
            + counter.count_message("Hello")
            + counter.count_message("How are you?")
            + counter.count_message("I'm good, thanks!");
        
        assert_eq!(total, expected);
    }
    
    #[test]
    fn test_token_count_stats() {
        let mut stats = TokenCountStats::default();
        
        stats.record(100, 25);
        stats.record(200, 50);
        
        assert_eq!(stats.total_chars, 300);
        assert_eq!(stats.total_tokens, 75);
        assert_eq!(stats.count_operations, 2);
        
        // 300 chars / 75 tokens = 4.0
        assert!((stats.observed_ratio() - 4.0).abs() < 0.01);
    }
}
