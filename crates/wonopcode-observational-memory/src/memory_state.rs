//! Memory state management for Observational Memory.
//!
//! This module provides the `MemoryState` struct which manages observations,
//! tracks token budgets, and handles the three-tier context window:
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

use crate::observation::{Observation, ObservationCategory, Priority};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Statistics about memory state operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Total observations created this session.
    pub total_observations: u32,
    /// Number of reflection passes run.
    pub reflections_run: u32,
    /// Total tokens processed by Observer.
    pub tokens_observed: u64,
    /// Total tokens after compression.
    pub tokens_after_compression: u64,
    /// Number of observations merged by Reflector.
    pub observations_merged: u32,
    /// Number of observations dropped by Reflector.
    pub observations_dropped: u32,
}

impl MemoryStats {
    /// Calculate the average compression ratio.
    pub fn compression_ratio(&self) -> f32 {
        if self.tokens_after_compression == 0 {
            return 0.0;
        }
        self.tokens_observed as f32 / self.tokens_after_compression as f32
    }
}

/// The complete memory state for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryState {
    /// Active observations (not dismissed).
    pub observations: Vec<Observation>,
    
    /// Indices of unobserved messages in the conversation history.
    /// These are message indices that haven't been processed by the Observer yet.
    pub unobserved_message_range: Option<(usize, usize)>,
    
    /// Token budget for the observation block.
    pub observation_token_budget: u32,
    
    /// Current token usage in the observation block.
    pub observation_tokens: u32,
    
    /// Token count of unobserved messages.
    pub unobserved_message_tokens: u32,
    
    /// Session ID for cross-session persistence.
    pub session_id: String,
    
    /// Workstream/project ID for persistence grouping.
    pub workstream_id: Option<String>,
    
    /// Last Observer run timestamp.
    pub last_observation: Option<DateTime<Utc>>,
    
    /// Last Reflector run timestamp.
    pub last_reflection: Option<DateTime<Utc>>,
    
    /// Pending observations from Observer (not yet reflected).
    pub pending_observations: Vec<Observation>,
    
    /// Whether this state was loaded from a previous session.
    pub loaded_from_previous_session: bool,
    
    /// Memory operation statistics.
    pub stats: MemoryStats,
    
    /// Observations grouped by category for efficient access.
    #[serde(skip)]
    category_index: HashMap<ObservationCategory, Vec<usize>>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self::new(uuid::Uuid::new_v4().to_string(), 40_000)
    }
}

impl MemoryState {
    /// Create a new memory state.
    ///
    /// # Arguments
    /// * `session_id` - Unique identifier for this session
    /// * `observation_token_budget` - Maximum tokens for the observation block (default: 40k)
    pub fn new(session_id: String, observation_token_budget: u32) -> Self {
        Self {
            observations: Vec::new(),
            unobserved_message_range: None,
            observation_token_budget,
            observation_tokens: 0,
            unobserved_message_tokens: 0,
            session_id,
            workstream_id: None,
            last_observation: None,
            last_reflection: None,
            pending_observations: Vec::new(),
            loaded_from_previous_session: false,
            stats: MemoryStats::default(),
            category_index: HashMap::new(),
        }
    }
    
    /// Create memory state with a workstream ID for persistence.
    pub fn with_workstream(mut self, workstream_id: impl Into<String>) -> Self {
        self.workstream_id = Some(workstream_id.into());
        self
    }
    
    /// Mark this state as loaded from a previous session.
    pub fn mark_loaded_from_previous(mut self) -> Self {
        self.loaded_from_previous_session = true;
        self
    }
    
    /// Add an observation from the Observer (goes to pending).
    pub fn add_pending(&mut self, observation: Observation) {
        self.pending_observations.push(observation);
    }
    
    /// Add multiple observations from the Observer.
    pub fn add_pending_batch(&mut self, observations: Vec<Observation>) {
        self.pending_observations.extend(observations);
    }
    
    /// Commit pending observations (called after Observer processes messages).
    ///
    /// This moves pending observations to the main observations list
    /// and updates token counts.
    pub fn commit_observations(&mut self, observed_message_count: usize) {
        let pending_tokens: u32 = self.pending_observations
            .iter()
            .map(|o| o.estimate_tokens())
            .sum();
        
        self.stats.total_observations += self.pending_observations.len() as u32;
        self.stats.tokens_after_compression += pending_tokens as u64;
        
        self.observations.append(&mut self.pending_observations);
        self.observation_tokens = self.calculate_observation_tokens();
        
        // Update unobserved range
        if let Some((start, end)) = self.unobserved_message_range {
            let new_start = start + observed_message_count;
            if new_start >= end {
                self.unobserved_message_range = None;
            } else {
                self.unobserved_message_range = Some((new_start, end));
            }
        }
        
        self.last_observation = Some(Utc::now());
        self.rebuild_index();
    }
    
    /// Commit reflected observations (called after Reflector processes).
    ///
    /// This replaces all observations with the restructured result.
    pub fn commit_reflection(&mut self, reflected_observations: Vec<Observation>, stats: ReflectionStats) {
        let old_count = self.observations.len() as u32;
        
        self.observations = reflected_observations;
        self.observation_tokens = self.calculate_observation_tokens();
        
        self.stats.reflections_run += 1;
        self.stats.observations_merged += stats.merged;
        self.stats.observations_dropped += old_count.saturating_sub(self.observations.len() as u32 + stats.merged);
        
        self.last_reflection = Some(Utc::now());
        self.rebuild_index();
    }
    
    /// Update the unobserved message range.
    pub fn set_unobserved_range(&mut self, start: usize, end: usize, tokens: u32) {
        self.unobserved_message_range = Some((start, end));
        self.unobserved_message_tokens = tokens;
        self.stats.tokens_observed += tokens as u64;
    }
    
    /// Get observations formatted for prompt injection.
    pub fn format_for_prompt(&self) -> String {
        if self.observations.is_empty() {
            return String::new();
        }
        
        let mut output = String::from("<memory_observations>\n");
        
        // Group by category for readability
        for category in [
            ObservationCategory::Context,
            ObservationCategory::Decision,
            ObservationCategory::Preference,
            ObservationCategory::Technical,
            ObservationCategory::Action,
            ObservationCategory::Fact,
        ] {
            let cat_observations: Vec<_> = self.observations
                .iter()
                .filter(|o| o.category == category && o.is_active())
                .collect();
            
            if !cat_observations.is_empty() {
                output.push_str(&format!("\n## {:?}\n", category));
                for obs in cat_observations {
                    let pin_marker = if obs.pinned { "📌 " } else { "" };
                    output.push_str(&format!(
                        "- {} {}{}\n",
                        obs.priority.emoji(),
                        pin_marker,
                        obs.content
                    ));
                    
                    // Add children with indentation
                    for child in &obs.children {
                        if child.is_active() {
                            output.push_str(&format!(
                                "  - {} {}\n",
                                child.priority.emoji(),
                                child.content
                            ));
                        }
                    }
                }
            }
        }
        
        output.push_str("\n</memory_observations>\n");
        output
    }
    
    /// Get all active (non-dismissed) observations.
    pub fn active_observations(&self) -> impl Iterator<Item = &Observation> {
        self.observations.iter().filter(|o| o.is_active())
    }
    
    /// Get observations by category.
    pub fn observations_by_category(&self, category: ObservationCategory) -> Vec<&Observation> {
        if let Some(indices) = self.category_index.get(&category) {
            indices
                .iter()
                .filter_map(|&i| self.observations.get(i))
                .filter(|o| o.is_active())
                .collect()
        } else {
            Vec::new()
        }
    }
    
    /// Get high-priority observations.
    pub fn high_priority_observations(&self) -> Vec<&Observation> {
        self.observations
            .iter()
            .filter(|o| o.priority == Priority::High && o.is_active())
            .collect()
    }
    
    /// Pin an observation by ID.
    pub fn pin_observation(&mut self, id: &str) -> bool {
        if let Some(obs) = self.observations.iter_mut().find(|o| o.id == id) {
            obs.pin();
            true
        } else {
            false
        }
    }
    
    /// Dismiss an observation by ID.
    pub fn dismiss_observation(&mut self, id: &str) -> bool {
        if let Some(obs) = self.observations.iter_mut().find(|o| o.id == id) {
            obs.dismiss();
            self.observation_tokens = self.calculate_observation_tokens();
            true
        } else {
            false
        }
    }
    
    /// Calculate total observation tokens.
    fn calculate_observation_tokens(&self) -> u32 {
        self.observations
            .iter()
            .filter(|o| o.is_active())
            .map(|o| o.estimate_tokens())
            .sum()
    }
    
    /// Rebuild the category index.
    fn rebuild_index(&mut self) {
        self.category_index.clear();
        for (idx, obs) in self.observations.iter().enumerate() {
            self.category_index
                .entry(obs.category)
                .or_default()
                .push(idx);
        }
    }
    
    /// Check if Observer should run (unobserved tokens exceed threshold).
    pub fn should_observe(&self, threshold: u32) -> bool {
        self.unobserved_message_tokens >= threshold
    }
    
    /// Check if Reflector should run (observation tokens exceed threshold).
    pub fn should_reflect(&self, threshold: u32) -> bool {
        self.observation_tokens >= threshold
    }
    
    /// Get total tokens (observations + unobserved messages).
    pub fn total_tokens(&self) -> u32 {
        self.observation_tokens + self.unobserved_message_tokens
    }
    
    /// Get the number of active observations.
    pub fn active_observation_count(&self) -> usize {
        self.observations.iter().filter(|o| o.is_active()).count()
    }
    
    /// Get context utilization as a fraction.
    pub fn utilization(&self, context_limit: u32) -> f32 {
        if context_limit == 0 {
            return 0.0;
        }
        self.total_tokens() as f32 / context_limit as f32
    }
}

/// Statistics from a reflection pass.
#[derive(Debug, Clone, Default)]
pub struct ReflectionStats {
    /// Number of observations merged.
    pub merged: u32,
    /// Number of observations dropped.
    pub dropped: u32,
    /// Number of meta-observations added.
    pub meta_added: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_memory_state_new() {
        let state = MemoryState::new("test-session".to_string(), 40_000);
        
        assert_eq!(state.session_id, "test-session");
        assert_eq!(state.observation_token_budget, 40_000);
        assert_eq!(state.observation_tokens, 0);
        assert!(state.observations.is_empty());
    }
    
    #[test]
    fn test_add_pending_observations() {
        let mut state = MemoryState::default();
        
        let obs = Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "User chose React",
            0.9,
        );
        
        state.add_pending(obs);
        assert_eq!(state.pending_observations.len(), 1);
        assert_eq!(state.observations.len(), 0);
        
        state.commit_observations(0);
        assert_eq!(state.pending_observations.len(), 0);
        assert_eq!(state.observations.len(), 1);
        assert!(state.observation_tokens > 0);
    }
    
    #[test]
    fn test_format_for_prompt() {
        let mut state = MemoryState::default();
        
        state.observations.push(Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "User chose PostgreSQL for database",
            0.9,
        ));
        state.observations.push(Observation::new(
            Priority::Medium,
            ObservationCategory::Preference,
            "User prefers explicit error handling",
            0.8,
        ));
        
        let formatted = state.format_for_prompt();
        
        assert!(formatted.contains("<memory_observations>"));
        assert!(formatted.contains("🔴"));
        assert!(formatted.contains("PostgreSQL"));
        assert!(formatted.contains("Decision"));
    }
    
    #[test]
    fn test_pin_dismiss() {
        let mut state = MemoryState::default();
        
        let obs = Observation::new(
            Priority::Medium,
            ObservationCategory::Fact,
            "Test observation",
            0.8,
        );
        let id = obs.id.clone();
        
        state.observations.push(obs);
        
        assert!(state.pin_observation(&id));
        assert!(state.observations[0].pinned);
        
        assert!(state.dismiss_observation(&id));
        assert!(state.observations[0].dismissed);
        assert_eq!(state.active_observation_count(), 0);
    }
    
    #[test]
    fn test_should_observe() {
        let mut state = MemoryState::default();
        
        state.unobserved_message_tokens = 25_000;
        
        assert!(!state.should_observe(30_000));
        assert!(state.should_observe(20_000));
    }
    
    #[test]
    fn test_should_reflect() {
        let mut state = MemoryState::default();
        
        // Add observations that exceed threshold
        for i in 0..100 {
            state.observations.push(Observation::new(
                Priority::Medium,
                ObservationCategory::Fact,
                format!("Observation {} with some content to increase token count", i),
                0.8,
            ));
        }
        state.observation_tokens = state.calculate_observation_tokens();
        
        // With default 40k threshold, 100 observations shouldn't exceed it
        assert!(!state.should_reflect(40_000));
        
        // But with a lower threshold...
        assert!(state.should_reflect(1_000));
    }
    
    #[test]
    fn test_stats_compression_ratio() {
        let mut stats = MemoryStats::default();
        
        stats.tokens_observed = 10_000;
        stats.tokens_after_compression = 2_000;
        
        assert!((stats.compression_ratio() - 5.0).abs() < 0.01);
    }
}
