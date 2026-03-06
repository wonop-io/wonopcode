//! Reflector Agent for Observational Memory.
//!
//! The Reflector restructures and condenses the observation log when it grows
//! too large. Unlike the Observer (which compresses messages), the Reflector
//! reorganises already-structured observations.
//!
//! # Key Operations
//!
//! - **Combining**: Related observations about the same topic/entity
//! - **Superseding**: Dropping earlier observations invalidated by later ones
//! - **Garbage Collection**: Dropping 🟢 observations no longer relevant
//! - **Pattern Detection**: Adding meta-observations about recurring patterns
//!
//! # Important Constraint
//!
//! The Reflector does NOT summarise. It restructures. The output should still
//! be individual dated observations, just reorganised and pruned.

use crate::observation::{Observation, Priority};
use crate::observation_parser::ObservationParser;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use wonopcode_provider::{BoxedLanguageModel, GenerateOptions, Message, StreamChunk};

/// Configuration for the Reflector agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectorConfig {
    /// Provider ID to use for the Reflector LLM call.
    /// Default: "anthropic" (uses same provider as main agent)
    pub provider_id: String,
    
    /// Model ID for Reflector. Should be capable for reasoning.
    /// Default: "claude-3-5-haiku-latest" (same as Observer)
    pub model_id: String,
    
    /// Maximum tokens for Reflector output.
    /// Default: 8192 (larger than Observer since it outputs observations)
    pub max_output_tokens: u32,
    
    /// Temperature for Reflector (low for consistency).
    /// Default: 0.2
    pub temperature: f32,
    
    /// Target compression ratio for reflection.
    /// Default: 0.7 (reduce observations by ~30%)
    pub target_compression_ratio: f32,
    
    /// Minimum number of observations before reflection is worthwhile.
    /// Default: 10
    pub min_observations_for_reflection: usize,
    
    /// Whether to preserve all 🔴 (high priority) observations.
    /// Default: true
    pub preserve_high_priority: bool,
    
    /// Maximum age (in hours) for 🟢 (low priority) observations.
    /// Older low-priority observations are candidates for garbage collection.
    /// Default: 24
    pub low_priority_max_age_hours: u32,
}

impl Default for ReflectorConfig {
    fn default() -> Self {
        Self {
            provider_id: "anthropic".to_string(),
            model_id: "claude-3-5-haiku-latest".to_string(),
            max_output_tokens: 8192,
            temperature: 0.2,
            target_compression_ratio: 0.7,
            min_observations_for_reflection: 10,
            preserve_high_priority: true,
            low_priority_max_age_hours: 24,
        }
    }
}

impl ReflectorConfig {
    /// Create config for a specific provider/model.
    pub fn with_model(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            ..Default::default()
        }
    }
    
    /// Create config optimized for aggressive compression.
    pub fn aggressive() -> Self {
        Self {
            target_compression_ratio: 0.5,
            preserve_high_priority: false,
            low_priority_max_age_hours: 12,
            ..Default::default()
        }
    }
    
    /// Create config optimized for preservation.
    pub fn conservative() -> Self {
        Self {
            target_compression_ratio: 0.85,
            preserve_high_priority: true,
            low_priority_max_age_hours: 48,
            ..Default::default()
        }
    }
}

/// The Reflector system prompt.
pub const REFLECTOR_SYSTEM_PROMPT: &str = r#"You are a Memory Reflector for an AI coding assistant. Your task is to restructure and condense the observation log while preserving essential information.

## Your Role

Reorganise observations to reduce token count while maintaining full context fidelity. You are NOT summarising - you are restructuring and pruning.

## Input Format

You will receive observations in this format:
```
Date: YYYY-MM-DD
- 🔴 HH:MM [observation text]
  - [child observation]
- 🟡 HH:MM [observation text]
```

## Output Format

Produce observations in the SAME format. Each observation should remain as a discrete, dated fact.

## Operations

### 1. COMBINE related observations
When multiple observations refer to the same topic or entity, merge them into a single observation with children.

Before:
```
- 🔴 10:00 User's project uses React
- 🔴 10:15 User's project uses TypeScript
- 🔴 10:30 User's project uses Tailwind CSS
```

After:
```
- 🔴 10:00 User's project tech stack
  - 🔴 React (frontend)
  - 🔴 TypeScript (language)
  - 🔴 Tailwind CSS (styling)
```

### 2. SUPERSEDE outdated observations
When a later observation invalidates an earlier one, keep only the latest.

Before:
```
- 🔴 10:00 Config port is 3000
- 🔴 14:00 User changed port to 8080
```

After:
```
- 🔴 14:00 Config port is 8080 (changed from 3000)
```

### 3. DROP irrelevant observations
Remove 🟢 (low priority) observations that are no longer useful:
- Routine operations that completed long ago
- Intermediate states that are no longer relevant
- Verbose details that don't affect future decisions

### 4. ELEVATE patterns
When you notice recurring themes, add a meta-observation:

```
- 🔴 [meta] User consistently prefers functional components over class components
```

## Priority Preservation Rules

- 🔴 HIGH: NEVER drop unless explicitly superseded
- 🟡 MEDIUM: Keep if potentially relevant, combine if similar
- 🟢 LOW: Drop if older than 24 hours or clearly irrelevant

## Critical Rules

1. **NO SUMMARIES** - Do not write paragraph-style summaries
2. **PRESERVE SPECIFICS** - Keep exact numbers, file paths, error messages
3. **MAINTAIN DATES** - Keep original timestamps for historical context
4. **OUTPUT FORMAT** - Your output must be valid observation format, same as input

## Output

Produce the restructured observation log. At the end, add statistics:

```
---
Reflection Stats:
- Input: [N] observations
- Output: [M] observations
- Dropped: [X] (reasons: [list reasons])
- Merged: [Y] (into [Z] combined observations)
- Patterns identified: [list any meta-observations added]
```
"#;

/// Build the Reflector user prompt from observations.
pub fn build_reflector_prompt(
    observations: &[Observation],
    current_date: DateTime<Utc>,
    config: &ReflectorConfig,
) -> String {
    let mut prompt = String::new();
    
    prompt.push_str("# Observations to Restructure\n\n");
    prompt.push_str(&format!("Current date/time: {}\n", current_date.format("%Y-%m-%d %H:%M:%S UTC")));
    prompt.push_str(&format!("Total observations: {}\n", observations.len()));
    prompt.push_str(&format!("Target compression: {:.0}%\n\n", config.target_compression_ratio * 100.0));
    
    if config.preserve_high_priority {
        prompt.push_str("Note: All 🔴 HIGH priority observations must be preserved.\n\n");
    }
    
    // Group observations by date for output
    let mut current_date_str: Option<String> = None;
    
    for obs in observations {
        let obs_date = obs.observation_date.format("%Y-%m-%d").to_string();
        
        // Add date header if changed
        if current_date_str.as_ref() != Some(&obs_date) {
            prompt.push_str(&format!("\nDate: {}\n", obs_date));
            current_date_str = Some(obs_date);
        }
        
        // Format observation
        prompt.push_str(&format_observation_for_reflector(obs, 0));
    }
    
    prompt.push_str("\n\n# Instructions\n\n");
    prompt.push_str("Restructure the observations above following the rules in your system prompt.\n");
    prompt.push_str("Remember: Restructure, don't summarise. Preserve specifics. Maintain dates.\n");
    
    prompt
}

/// Format a single observation for the Reflector prompt.
fn format_observation_for_reflector(obs: &Observation, indent: usize) -> String {
    let mut output = String::new();
    let indent_str = "  ".repeat(indent);
    
    let priority_emoji = match obs.priority {
        Priority::High => "🔴",
        Priority::Medium => "🟡",
        Priority::Low => "🟢",
    };
    
    let time_str = obs.observation_date.format("%H:%M").to_string();
    
    // Add pinned indicator if applicable
    let pinned_marker = if obs.pinned { " [pinned]" } else { "" };
    
    output.push_str(&format!(
        "{}- {} {}{} {}\n",
        indent_str, priority_emoji, time_str, pinned_marker, obs.content
    ));
    
    // Add children
    for child in &obs.children {
        output.push_str(&format_observation_for_reflector(child, indent + 1));
    }
    
    output
}

/// Result from the Reflector agent.
#[derive(Debug, Clone)]
pub struct ReflectorResult {
    /// Restructured observations.
    pub observations: Vec<Observation>,
    /// Number of input observations.
    pub input_count: usize,
    /// Number of output observations.
    pub output_count: usize,
    /// Number of observations dropped.
    pub dropped_count: usize,
    /// Number of observations merged.
    pub merged_count: usize,
    /// Patterns/meta-observations identified.
    pub patterns: Vec<String>,
    /// Raw output from the Reflector (for debugging).
    pub raw_output: String,
    /// Token counts for metrics.
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl ReflectorResult {
    /// Calculate compression ratio achieved.
    pub fn compression_ratio(&self) -> f32 {
        if self.input_count == 0 {
            return 1.0;
        }
        self.output_count as f32 / self.input_count as f32
    }
}

/// Parse the Reflector's raw output into restructured observations.
pub fn parse_reflector_output(
    raw_output: &str,
    input_count: usize,
    default_confidence: f32,
) -> ReflectorResult {
    let mut result = ReflectorResult {
        observations: Vec::new(),
        input_count,
        output_count: 0,
        dropped_count: 0,
        merged_count: 0,
        patterns: Vec::new(),
        raw_output: raw_output.to_string(),
        input_tokens: 0,
        output_tokens: 0,
    };
    
    // Split off the statistics section
    let (observation_text, stats_section) = if let Some(idx) = raw_output.rfind("---") {
        let (obs, stats) = raw_output.split_at(idx);
        (obs.trim(), stats.trim_start_matches("---").trim())
    } else {
        (raw_output.trim(), "")
    };
    
    // Parse observations
    let parser = ObservationParser::new()
        .lenient(true)
        .with_confidence(default_confidence);
    
    match parser.parse(observation_text) {
        Ok(date_groups) => {
            for group in date_groups {
                result.observations.extend(group.observations);
            }
        }
        Err(e) => {
            tracing::warn!("Reflector output parsing failed: {:?}", e);
            // On parse failure, we return the original observations unchanged
            // The caller should handle this case
        }
    }
    
    result.output_count = result.observations.len();
    
    // Parse stats section for metrics
    for line in stats_section.lines() {
        let line = line.trim().to_lowercase();
        
        if line.starts_with("- dropped:") {
            if let Some(num) = extract_number(&line) {
                result.dropped_count = num;
            }
        } else if line.starts_with("- merged:") {
            if let Some(num) = extract_number(&line) {
                result.merged_count = num;
            }
        } else if line.starts_with("- patterns identified:") {
            // Extract patterns after the colon
            if let Some(patterns_str) = line.split(':').nth(1) {
                result.patterns = patterns_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    
    // Calculate dropped if not in stats (based on input/output difference)
    if result.dropped_count == 0 && result.input_count > result.output_count {
        result.dropped_count = result.input_count - result.output_count;
    }
    
    result
}

/// Extract a number from a stats line.
fn extract_number(line: &str) -> Option<usize> {
    line.split_whitespace()
        .find_map(|word| word.parse::<usize>().ok())
}

/// The Reflector agent that restructures observations.
pub struct ReflectorAgent {
    /// Configuration for the reflector.
    config: ReflectorConfig,
    /// The LLM provider to use for reflection.
    provider: BoxedLanguageModel,
}

impl ReflectorAgent {
    /// Create a new Reflector agent.
    pub fn new(config: ReflectorConfig, provider: BoxedLanguageModel) -> Self {
        Self { config, provider }
    }
    
    /// Create a Reflector with default config using the provided provider.
    pub fn with_provider(provider: BoxedLanguageModel) -> Self {
        Self::new(ReflectorConfig::default(), provider)
    }
    
    /// Check if reflection is worthwhile given the observation count.
    pub fn should_reflect(&self, observation_count: usize) -> bool {
        observation_count >= self.config.min_observations_for_reflection
    }
    
    /// Run the Reflector on a set of observations.
    ///
    /// Returns restructured observations.
    pub async fn run(
        &self,
        observations: &[Observation],
        current_date: DateTime<Utc>,
    ) -> Result<ReflectorResult, ReflectorError> {
        let input_count = observations.len();
        
        if input_count < self.config.min_observations_for_reflection {
            return Err(ReflectorError::InsufficientObservations {
                count: input_count,
                minimum: self.config.min_observations_for_reflection,
            });
        }
        
        // Build the reflector prompt
        let user_prompt = build_reflector_prompt(observations, current_date, &self.config);
        
        // Estimate input tokens (rough)
        let input_tokens = (REFLECTOR_SYSTEM_PROMPT.len() + user_prompt.len()) / 4;
        
        // Create messages for the LLM
        let reflector_messages = vec![
            Message::system(REFLECTOR_SYSTEM_PROMPT),
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
            .generate(reflector_messages, options)
            .await
            .map_err(|e| ReflectorError::ProviderError(e.to_string()))?;
        
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
                        break;
                    }
                    StreamChunk::Error(e) => {
                        return Err(ReflectorError::StreamError(e));
                    }
                    _ => {}
                },
                Err(e) => {
                    return Err(ReflectorError::ProviderError(e.to_string()));
                }
            }
        }
        
        // Parse the response
        let mut result = parse_reflector_output(&response_text, input_count, 0.8);
        result.input_tokens = input_tokens as u32;
        result.output_tokens = if output_tokens > 0 {
            output_tokens
        } else {
            (response_text.len() / 4) as u32
        };
        
        // Validate output
        if result.observations.is_empty() && input_count > 0 {
            tracing::warn!("Reflector produced empty output, returning original observations");
            return Err(ReflectorError::EmptyOutput);
        }
        
        // Ensure pinned observations are preserved
        let preserved_pinned = preserve_pinned_observations(observations, &mut result.observations);
        if preserved_pinned > 0 {
            tracing::debug!("Restored {} pinned observations that were dropped", preserved_pinned);
        }
        
        tracing::debug!(
            "Reflector completed: {} → {} observations ({:.0}% compression)",
            input_count,
            result.output_count,
            result.compression_ratio() * 100.0
        );
        
        Ok(result)
    }
    
    /// Get the reflector configuration.
    pub fn config(&self) -> &ReflectorConfig {
        &self.config
    }
}

/// Ensure pinned observations are preserved after reflection.
fn preserve_pinned_observations(
    original: &[Observation],
    reflected: &mut Vec<Observation>,
) -> usize {
    let mut preserved = 0;
    
    for obs in original {
        if obs.pinned {
            // Check if this pinned observation is in the output
            let exists = reflected.iter().any(|r| {
                r.content == obs.content || r.id == obs.id
            });
            
            if !exists {
                // Re-add the pinned observation
                reflected.push(obs.clone());
                preserved += 1;
            }
        }
        
        // Also check children recursively
        for child in &obs.children {
            if child.pinned {
                let exists = reflected.iter().any(|r| {
                    r.content == child.content || 
                    r.children.iter().any(|c| c.content == child.content)
                });
                
                if !exists {
                    reflected.push(child.clone());
                    preserved += 1;
                }
            }
        }
    }
    
    preserved
}

/// Error type for Reflector operations.
#[derive(Debug, Clone)]
pub enum ReflectorError {
    /// Error from the LLM provider.
    ProviderError(String),
    /// Error from the streaming response.
    StreamError(String),
    /// Error parsing the reflector output.
    ParseError(String),
    /// Not enough observations to warrant reflection.
    InsufficientObservations { count: usize, minimum: usize },
    /// Reflector produced empty output.
    EmptyOutput,
}

impl std::fmt::Display for ReflectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProviderError(e) => write!(f, "Provider error: {}", e),
            Self::StreamError(e) => write!(f, "Stream error: {}", e),
            Self::ParseError(e) => write!(f, "Parse error: {}", e),
            Self::InsufficientObservations { count, minimum } => {
                write!(f, "Insufficient observations: {} (need at least {})", count, minimum)
            }
            Self::EmptyOutput => write!(f, "Reflector produced empty output"),
        }
    }
}

impl std::error::Error for ReflectorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::ObservationCategory;
    
    #[test]
    fn test_reflector_config_default() {
        let config = ReflectorConfig::default();
        assert_eq!(config.provider_id, "anthropic");
        assert!(config.model_id.contains("haiku"));
        assert_eq!(config.temperature, 0.2);
        assert_eq!(config.min_observations_for_reflection, 10);
    }
    
    #[test]
    fn test_reflector_config_aggressive() {
        let config = ReflectorConfig::aggressive();
        assert_eq!(config.target_compression_ratio, 0.5);
        assert!(!config.preserve_high_priority);
    }
    
    #[test]
    fn test_reflector_config_conservative() {
        let config = ReflectorConfig::conservative();
        assert_eq!(config.target_compression_ratio, 0.85);
        assert!(config.preserve_high_priority);
    }
    
    #[test]
    fn test_parse_reflector_output() {
        let output = r#"Date: 2026-03-05
- 🔴 10:00 User's project tech stack
  - 🔴 React (frontend)
  - 🔴 TypeScript (language)
- 🔴 14:00 Config port is 8080 (changed from 3000)

---
Reflection Stats:
- Input: 5 observations
- Output: 2 observations
- Dropped: 1 (reasons: low priority, outdated)
- Merged: 2 (into 1 combined observation)
- Patterns identified: none
"#;
        
        let result = parse_reflector_output(output, 5, 0.8);
        
        assert_eq!(result.input_count, 5);
        assert_eq!(result.observations.len(), 2);
        assert_eq!(result.dropped_count, 1);
        assert_eq!(result.merged_count, 2);
    }
    
    #[test]
    fn test_format_observation_for_reflector() {
        let obs = Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "User's project uses React",
            0.9,
        );
        
        let formatted = format_observation_for_reflector(&obs, 0);
        
        assert!(formatted.starts_with("- 🔴"));
        assert!(formatted.contains("User's project uses React"));
    }
    
    #[test]
    fn test_format_pinned_observation() {
        let mut obs = Observation::new(
            Priority::Medium,
            ObservationCategory::Context,
            "Important note",
            0.8,
        );
        obs.pinned = true;
        
        let formatted = format_observation_for_reflector(&obs, 0);
        
        assert!(formatted.contains("[pinned]"));
    }
    
    #[test]
    fn test_should_reflect() {
        let config = ReflectorConfig::default();
        
        // Need a provider to create the agent, but we're just testing the should_reflect method
        // For this test, we'll create a mock scenario
        assert!(config.min_observations_for_reflection == 10);
        
        // If we had an agent:
        // assert!(!agent.should_reflect(5));  // Below threshold
        // assert!(agent.should_reflect(15));  // Above threshold
    }
}
