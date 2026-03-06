//! Observational Memory for Wonop Code.
//!
//! This crate implements the Observational Memory (OM) system, which maintains
//! a dense, append-only event log that replaces raw messages as they accumulate.
//! This provides a bounded, predictable, and prompt-cacheable context window.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │  SYSTEM PROMPT                                  │
//! ├─────────────────────────────────────────────────┤
//! │  OBSERVATION BLOCK (Tier 1 + Tier 2)            │
//! │  - Reflections (condensed, restructured)        │
//! │  - Observations (dated event log)               │
//! ├─────────────────────────────────────────────────┤
//! │  MESSAGE HISTORY (Tier 3)                       │
//! │  - Raw conversation not yet observed            │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - [`Observation`] - Individual dated, prioritized event extracted from conversation
//! - [`MemoryState`] - Complete memory state including observations and statistics
//! - [`ObserverAgent`] - LLM agent that compresses messages into observations
//! - [`ReflectorAgent`] - LLM agent that restructures and condenses observations
//! - [`TokenStateMachine`] - State machine for triggering observation/reflection
//!
//! # Usage
//!
//! ```rust,ignore
//! use wonopcode_observational_memory::{
//!     MemoryState, ThresholdConfig, TokenStateMachine, ObserverAgent, ReflectorAgent
//! };
//!
//! // Initialize memory state
//! let mut memory = MemoryState::new("session-123".to_string(), 40_000);
//!
//! // Set up threshold-based triggering
//! let config = ThresholdConfig::default();
//! let mut tsm = TokenStateMachine::from_config(&config, 200_000);
//!
//! // When message tokens exceed threshold, run Observer
//! if tsm.thresholds().should_observe(message_tokens) {
//!     let observer = ObserverAgent::with_provider(provider.clone());
//!     let result = observer.run(&messages, Utc::now()).await?;
//!     memory.add_pending_batch(result.observations);
//!     memory.commit_observations(message_count);
//! }
//! ```

mod memory_state;
mod observation;
mod observation_parser;
mod observer;
mod reflector;
mod threshold;
mod token_counter;

// Re-export core types
pub use memory_state::{MemoryState, MemoryStats, ReflectionStats};
pub use observation::{DateGroup, Observation, ObservationCategory, Priority};
pub use observation_parser::{format_date_group, format_observations, ObservationParser, ParseError};
pub use observer::{
    build_observer_prompt, parse_observer_output, CodingPriorityHints, ObserverAgent,
    ObserverConfig, ObserverError, ObserverResult, OBSERVER_SYSTEM_PROMPT,
};
pub use reflector::{
    build_reflector_prompt, parse_reflector_output, ReflectorAgent, ReflectorConfig,
    ReflectorError, ReflectorResult, REFLECTOR_SYSTEM_PROMPT,
};
pub use threshold::{
    ComputedThresholds, StateTransition, ThresholdConfig, TokenState, TokenStateMachine,
    UtilizationMetrics,
};
pub use token_counter::{TokenCountStats, TokenCounter, TokenCounterConfig};

/// Feature flag for enabling Observational Memory.
///
/// When this is false, the system falls back to legacy compaction.
#[derive(Debug, Clone, Default)]
pub struct ObservationalMemoryConfig {
    /// Whether OM is enabled.
    pub enabled: bool,
    /// Threshold configuration.
    pub thresholds: ThresholdConfig,
    /// Observer configuration.
    pub observer: ObserverConfig,
    /// Reflector configuration.
    pub reflector: ReflectorConfig,
}

impl ObservationalMemoryConfig {
    /// Create a new config with OM enabled.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Create a new config with OM disabled (uses legacy compaction).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_enabled() {
        let config = ObservationalMemoryConfig::enabled();
        assert!(config.enabled);
    }

    #[test]
    fn test_config_disabled() {
        let config = ObservationalMemoryConfig::disabled();
        assert!(!config.enabled);
    }
}
