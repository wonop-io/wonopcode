//! Token threshold management for Observational Memory.
//!
//! This module provides threshold configuration and state machine logic for
//! triggering Observer and Reflector agents based on token counts.
//!
//! # Token Flow
//!
//! ```text
//! Messages accumulate (append-only, cached)
//!         │
//!         ▼ threshold: ~30k tokens
//! Observer compresses messages → observations
//!         │
//!         ▼ threshold: ~40k tokens
//! Reflector restructures observations → condensed observations
//! ```

use serde::{Deserialize, Serialize};

/// Configuration for token thresholds.
///
/// Thresholds are specified as ratios of the model's context limit,
/// making them portable across different models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Ratio of context limit that triggers Observer.
    /// Default: 0.15 (15% of context, e.g., 30k for a 200k model)
    pub observer_ratio: f32,
    
    /// Ratio of context limit for observation token budget.
    /// Default: 0.20 (20% of context, e.g., 40k for a 200k model)
    pub observation_budget_ratio: f32,
    
    /// Ratio at which Reflector is triggered.
    /// Default: 0.18 (90% of observation budget)
    pub reflector_ratio: f32,
    
    /// Minimum tokens before Observer can trigger.
    /// This prevents running Observer on very short conversations.
    /// Default: 1000
    pub minimum_observer_tokens: u32,
    
    /// Minimum observations before Reflector can run.
    /// Default: 10
    pub minimum_observations: usize,
    
    /// Maximum time between observations (seconds).
    /// Forces observation even below threshold if exceeded.
    /// Default: 300 (5 minutes)
    pub max_time_between_observations_secs: u64,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            observer_ratio: 0.15,
            observation_budget_ratio: 0.20,
            reflector_ratio: 0.18,
            minimum_observer_tokens: 1000,
            minimum_observations: 10,
            max_time_between_observations_secs: 300,
        }
    }
}

impl ThresholdConfig {
    /// Create a config with custom ratios.
    pub fn with_ratios(observer: f32, observation_budget: f32, reflector: f32) -> Self {
        Self {
            observer_ratio: observer,
            observation_budget_ratio: observation_budget,
            reflector_ratio: reflector,
            ..Default::default()
        }
    }
    
    /// Create a "conservative" config that runs Observer/Reflector less frequently.
    /// Good for cost optimization or high-context models.
    pub fn conservative() -> Self {
        Self {
            observer_ratio: 0.25,
            observation_budget_ratio: 0.30,
            reflector_ratio: 0.28,
            minimum_observer_tokens: 5000,
            minimum_observations: 20,
            max_time_between_observations_secs: 600,
        }
    }
    
    /// Create an "aggressive" config that runs Observer/Reflector more frequently.
    /// Good for maintaining fresh context or small-context models.
    pub fn aggressive() -> Self {
        Self {
            observer_ratio: 0.10,
            observation_budget_ratio: 0.15,
            reflector_ratio: 0.13,
            minimum_observer_tokens: 500,
            minimum_observations: 5,
            max_time_between_observations_secs: 120,
        }
    }
    
    /// Calculate absolute thresholds from a context limit.
    pub fn compute_thresholds(&self, context_limit: u32) -> ComputedThresholds {
        let observer_threshold = (context_limit as f32 * self.observer_ratio) as u32;
        let observation_budget = (context_limit as f32 * self.observation_budget_ratio) as u32;
        let reflector_threshold = (context_limit as f32 * self.reflector_ratio) as u32;
        
        ComputedThresholds {
            observer_threshold: observer_threshold.max(self.minimum_observer_tokens),
            observation_budget,
            reflector_threshold: reflector_threshold.min(observation_budget),
            context_limit,
        }
    }
    
    /// Alias for `compute_thresholds`.
    pub fn compute(&self, context_limit: u32) -> ComputedThresholds {
        self.compute_thresholds(context_limit)
    }
}

/// Computed absolute thresholds for a specific context limit.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComputedThresholds {
    /// Token count that triggers Observer.
    pub observer_threshold: u32,
    /// Maximum tokens for observation block.
    pub observation_budget: u32,
    /// Token count that triggers Reflector.
    pub reflector_threshold: u32,
    /// The model's context limit these were computed from.
    pub context_limit: u32,
}

impl ComputedThresholds {
    /// Check if Observer should run based on message tokens.
    pub fn should_observe(&self, message_tokens: u32) -> bool {
        message_tokens >= self.observer_threshold
    }
    
    /// Check if Reflector should run based on observation tokens.
    pub fn should_reflect(&self, observation_tokens: u32) -> bool {
        observation_tokens >= self.reflector_threshold
    }
    
    /// Get available space in the observation budget.
    pub fn observation_headroom(&self, observation_tokens: u32) -> u32 {
        self.observation_budget.saturating_sub(observation_tokens)
    }
    
    /// Calculate what percentage of the observer threshold we're at.
    pub fn observer_utilization(&self, message_tokens: u32) -> f32 {
        if self.observer_threshold == 0 {
            return 0.0;
        }
        message_tokens as f32 / self.observer_threshold as f32
    }
    
    /// Calculate what percentage of the observation budget we're at.
    pub fn observation_utilization(&self, observation_tokens: u32) -> f32 {
        if self.observation_budget == 0 {
            return 0.0;
        }
        observation_tokens as f32 / self.observation_budget as f32
    }
}

/// State of the token management state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenState {
    /// Normal operation - accepting messages, no thresholds hit.
    Normal,
    /// Observer should run - message tokens exceeded threshold.
    ObserverPending,
    /// Observer is currently running.
    Observing,
    /// Reflector should run - observation tokens exceeded threshold.
    ReflectorPending,
    /// Reflector is currently running.
    Reflecting,
}

impl Default for TokenState {
    fn default() -> Self {
        Self::Normal
    }
}

/// State machine for token-based Observer/Reflector triggering.
#[derive(Debug, Clone)]
pub struct TokenStateMachine {
    /// Current state.
    state: TokenState,
    /// Computed thresholds for the current model.
    thresholds: ComputedThresholds,
    /// Current message tokens (unobserved).
    message_tokens: u32,
    /// Current observation tokens.
    observation_tokens: u32,
}

impl TokenStateMachine {
    /// Create a new state machine with computed thresholds.
    pub fn new(thresholds: ComputedThresholds) -> Self {
        Self {
            state: TokenState::Normal,
            thresholds,
            message_tokens: 0,
            observation_tokens: 0,
        }
    }
    
    /// Create from a threshold config and context limit.
    pub fn from_config(config: &ThresholdConfig, context_limit: u32) -> Self {
        Self::new(config.compute_thresholds(context_limit))
    }
    
    /// Get the current state.
    pub fn state(&self) -> TokenState {
        self.state
    }
    
    /// Get the computed thresholds.
    pub fn thresholds(&self) -> ComputedThresholds {
        self.thresholds
    }
    
    /// Get current message tokens.
    pub fn message_tokens(&self) -> u32 {
        self.message_tokens
    }
    
    /// Get current observation tokens.
    pub fn observation_tokens(&self) -> u32 {
        self.observation_tokens
    }
    
    /// Update the thresholds (e.g., when model changes).
    pub fn update_thresholds(&mut self, thresholds: ComputedThresholds) {
        self.thresholds = thresholds;
    }
    
    /// Add message tokens (incremental update).
    pub fn add_message_tokens(&mut self, additional: u32) -> Option<StateTransition> {
        self.message_tokens = self.message_tokens.saturating_add(additional);
        let transition = self.check_transitions();
        if transition != StateTransition::None {
            Some(transition)
        } else {
            None
        }
    }
    
    /// Update message token count and transition state if needed.
    pub fn update_message_tokens(&mut self, tokens: u32) -> StateTransition {
        self.message_tokens = tokens;
        self.check_transitions()
    }
    
    /// Update observation token count and transition state if needed.
    pub fn update_observation_tokens(&mut self, tokens: u32) -> StateTransition {
        self.observation_tokens = tokens;
        self.check_transitions()
    }
    
    /// Signal that Observer has started.
    pub fn observer_started(&mut self) {
        if self.state == TokenState::ObserverPending {
            self.state = TokenState::Observing;
        }
    }
    
    /// Signal that Observer has completed.
    pub fn observer_completed(&mut self, new_observation_tokens: u32) {
        self.message_tokens = 0;
        self.observation_tokens = new_observation_tokens;
        self.state = TokenState::Normal;
        
        // Check if we now need to reflect
        self.check_transitions();
    }
    
    /// Signal that Reflector has started.
    pub fn reflector_started(&mut self) {
        if self.state == TokenState::ReflectorPending {
            self.state = TokenState::Reflecting;
        }
    }
    
    /// Signal that Reflector has completed.
    pub fn reflector_completed(&mut self, new_observation_tokens: u32) {
        self.observation_tokens = new_observation_tokens;
        self.state = TokenState::Normal;
    }
    
    // Convenience aliases for method naming consistency
    
    /// Alias for `observer_started`.
    pub fn start_observer(&mut self) {
        self.observer_started();
    }
    
    /// Alias for `observer_completed`.
    /// 
    /// `observed_tokens` is the token count that was observed (now set to 0).
    /// `observation_tokens` is the new observation block token count.
    pub fn complete_observer(&mut self, _observed_tokens: u32, observation_tokens: u32) {
        self.observer_completed(observation_tokens);
    }
    
    /// Alias for `reflector_started`.
    pub fn start_reflector(&mut self) {
        self.reflector_started();
    }
    
    /// Alias for `reflector_completed`.
    pub fn complete_reflector(&mut self, new_observation_tokens: u32) {
        self.reflector_completed(new_observation_tokens);
    }
    
    /// Check if any state transitions should occur.
    fn check_transitions(&mut self) -> StateTransition {
        let old_state = self.state;
        
        match self.state {
            TokenState::Normal => {
                if self.thresholds.should_observe(self.message_tokens) {
                    self.state = TokenState::ObserverPending;
                    return StateTransition::ToObserverPending;
                }
                if self.thresholds.should_reflect(self.observation_tokens) {
                    self.state = TokenState::ReflectorPending;
                    return StateTransition::ToReflectorPending;
                }
            }
            TokenState::ObserverPending | TokenState::Observing => {
                // Stay in these states until explicitly moved out
            }
            TokenState::ReflectorPending | TokenState::Reflecting => {
                // Stay in these states until explicitly moved out
            }
        }
        
        if old_state != self.state {
            StateTransition::Changed(old_state, self.state)
        } else {
            StateTransition::None
        }
    }
    
    /// Get current utilization metrics.
    pub fn utilization(&self) -> UtilizationMetrics {
        UtilizationMetrics {
            message_tokens: self.message_tokens,
            observation_tokens: self.observation_tokens,
            message_utilization: self.thresholds.observer_utilization(self.message_tokens),
            observation_utilization: self.thresholds.observation_utilization(self.observation_tokens),
            observer_threshold: self.thresholds.observer_threshold,
            observation_budget: self.thresholds.observation_budget,
        }
    }
}

/// Result of a state transition check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateTransition {
    /// No transition occurred.
    None,
    /// Transitioned to ObserverPending.
    ToObserverPending,
    /// Transitioned to ReflectorPending.
    ToReflectorPending,
    /// Generic state change.
    Changed(TokenState, TokenState),
}

/// Current utilization metrics.
#[derive(Debug, Clone, Copy)]
pub struct UtilizationMetrics {
    /// Current message tokens.
    pub message_tokens: u32,
    /// Current observation tokens.
    pub observation_tokens: u32,
    /// Message tokens as fraction of threshold.
    pub message_utilization: f32,
    /// Observation tokens as fraction of budget.
    pub observation_utilization: f32,
    /// Observer threshold.
    pub observer_threshold: u32,
    /// Observation budget.
    pub observation_budget: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_threshold_config_default() {
        let config = ThresholdConfig::default();
        
        assert_eq!(config.observer_ratio, 0.15);
        assert_eq!(config.observation_budget_ratio, 0.20);
    }
    
    #[test]
    fn test_compute_thresholds() {
        let config = ThresholdConfig::default();
        let thresholds = config.compute_thresholds(200_000);
        
        // 15% of 200k = 30k
        assert_eq!(thresholds.observer_threshold, 30_000);
        // 20% of 200k = 40k
        assert_eq!(thresholds.observation_budget, 40_000);
    }
    
    #[test]
    fn test_thresholds_should_observe() {
        let config = ThresholdConfig::default();
        let thresholds = config.compute_thresholds(200_000);
        
        assert!(!thresholds.should_observe(25_000));
        assert!(thresholds.should_observe(30_000));
        assert!(thresholds.should_observe(35_000));
    }
    
    #[test]
    fn test_state_machine_normal_to_observer() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        assert_eq!(sm.state(), TokenState::Normal);
        
        // Below threshold
        let transition = sm.update_message_tokens(20_000);
        assert_eq!(transition, StateTransition::None);
        assert_eq!(sm.state(), TokenState::Normal);
        
        // At threshold
        let transition = sm.update_message_tokens(30_000);
        assert_eq!(transition, StateTransition::ToObserverPending);
        assert_eq!(sm.state(), TokenState::ObserverPending);
    }
    
    #[test]
    fn test_state_machine_observer_flow() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        // Trigger observer
        sm.update_message_tokens(30_000);
        assert_eq!(sm.state(), TokenState::ObserverPending);
        
        // Start observing
        sm.observer_started();
        assert_eq!(sm.state(), TokenState::Observing);
        
        // Complete observation (produced 5k tokens of observations)
        sm.observer_completed(5_000);
        assert_eq!(sm.state(), TokenState::Normal);
    }
    
    #[test]
    fn test_state_machine_reflector_trigger() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        // Set observation tokens above reflector threshold (18% of 200k = 36k)
        sm.update_observation_tokens(38_000);
        assert_eq!(sm.state(), TokenState::ReflectorPending);
        
        sm.reflector_started();
        assert_eq!(sm.state(), TokenState::Reflecting);
        
        sm.reflector_completed(15_000);
        assert_eq!(sm.state(), TokenState::Normal);
    }
    
    #[test]
    fn test_utilization_metrics() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        sm.update_message_tokens(15_000);
        sm.update_observation_tokens(20_000);
        
        let metrics = sm.utilization();
        
        // 15k / 30k = 50%
        assert!((metrics.message_utilization - 0.5).abs() < 0.01);
        // 20k / 40k = 50%
        assert!((metrics.observation_utilization - 0.5).abs() < 0.01);
    }
    
    #[test]
    fn test_conservative_vs_aggressive() {
        let conservative = ThresholdConfig::conservative();
        let aggressive = ThresholdConfig::aggressive();
        
        // Conservative has higher ratios (triggers less often)
        assert!(conservative.observer_ratio > aggressive.observer_ratio);
        assert!(conservative.observation_budget_ratio > aggressive.observation_budget_ratio);
        
        // Verify on a 200k model
        let cons_thresholds = conservative.compute_thresholds(200_000);
        let agg_thresholds = aggressive.compute_thresholds(200_000);
        
        // Conservative needs more tokens to trigger
        assert!(cons_thresholds.observer_threshold > agg_thresholds.observer_threshold);
    }
    
    #[test]
    fn test_minimum_observer_tokens() {
        // With a very small context limit, minimum_observer_tokens should take precedence
        let config = ThresholdConfig {
            observer_ratio: 0.15,
            minimum_observer_tokens: 5000,
            ..Default::default()
        };
        
        // 15% of 10k = 1500, but minimum is 5000
        let thresholds = config.compute_thresholds(10_000);
        assert_eq!(thresholds.observer_threshold, 5000);
    }
    
    #[test]
    fn test_reflector_capped_by_budget() {
        // Reflector threshold should not exceed observation budget
        let config = ThresholdConfig {
            observation_budget_ratio: 0.10,  // 10%
            reflector_ratio: 0.20,            // 20% > budget!
            ..Default::default()
        };
        
        let thresholds = config.compute_thresholds(100_000);
        // Budget is 10k, reflector ratio would give 20k but gets capped
        assert!(thresholds.reflector_threshold <= thresholds.observation_budget);
    }
    
    #[test]
    fn test_add_message_tokens_incremental() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        // Add tokens incrementally
        let result = sm.add_message_tokens(10_000);
        assert!(result.is_none());
        assert_eq!(sm.message_tokens(), 10_000);
        
        let result = sm.add_message_tokens(10_000);
        assert!(result.is_none());
        assert_eq!(sm.message_tokens(), 20_000);
        
        // This should trigger observer (20k + 10k = 30k = threshold)
        let result = sm.add_message_tokens(10_000);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), StateTransition::ToObserverPending);
    }
    
    #[test]
    fn test_state_remains_pending_on_additional_tokens() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        // Trigger observer
        sm.update_message_tokens(35_000);
        assert_eq!(sm.state(), TokenState::ObserverPending);
        
        // Adding more tokens should not change state from pending
        let transition = sm.update_message_tokens(40_000);
        assert_eq!(transition, StateTransition::None);
        assert_eq!(sm.state(), TokenState::ObserverPending);
    }
    
    #[test]
    fn test_observer_to_reflector_chain() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        // Trigger observer
        sm.update_message_tokens(30_000);
        sm.observer_started();
        
        // Complete with enough observations to trigger reflector
        // Reflector threshold is 18% of 200k = 36k
        sm.observer_completed(40_000);
        
        // Should have transitioned to ReflectorPending
        assert_eq!(sm.state(), TokenState::ReflectorPending);
    }
    
    #[test]
    fn test_zero_context_limit() {
        let config = ThresholdConfig::default();
        let thresholds = config.compute_thresholds(0);
        
        // Should use minimum values
        assert_eq!(thresholds.observer_threshold, config.minimum_observer_tokens);
        assert_eq!(thresholds.observation_budget, 0);
    }
    
    #[test]
    fn test_observation_headroom() {
        let config = ThresholdConfig::default();
        let thresholds = config.compute_thresholds(200_000);
        
        // Budget is 40k
        assert_eq!(thresholds.observation_headroom(10_000), 30_000);
        assert_eq!(thresholds.observation_headroom(40_000), 0);
        assert_eq!(thresholds.observation_headroom(50_000), 0);  // Clamped to 0
    }
    
    #[test]
    fn test_with_ratios() {
        let config = ThresholdConfig::with_ratios(0.1, 0.3, 0.25);
        
        assert_eq!(config.observer_ratio, 0.1);
        assert_eq!(config.observation_budget_ratio, 0.3);
        assert_eq!(config.reflector_ratio, 0.25);
        
        // Other fields should be defaults
        assert_eq!(config.minimum_observer_tokens, ThresholdConfig::default().minimum_observer_tokens);
    }
    
    #[test]
    fn test_complete_observer_aliases() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        sm.update_message_tokens(30_000);
        
        // Use aliases
        sm.start_observer();
        assert_eq!(sm.state(), TokenState::Observing);
        
        sm.complete_observer(30_000, 5_000);
        assert_eq!(sm.state(), TokenState::Normal);
        assert_eq!(sm.message_tokens(), 0);
        assert_eq!(sm.observation_tokens(), 5_000);
    }
    
    #[test]
    fn test_complete_reflector_aliases() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 200_000);
        
        sm.update_observation_tokens(40_000);
        
        sm.start_reflector();
        assert_eq!(sm.state(), TokenState::Reflecting);
        
        sm.complete_reflector(15_000);
        assert_eq!(sm.state(), TokenState::Normal);
        assert_eq!(sm.observation_tokens(), 15_000);
    }
    
    #[test]
    fn test_update_thresholds_runtime() {
        let config = ThresholdConfig::default();
        let mut sm = TokenStateMachine::from_config(&config, 100_000);
        
        // Initially uses 100k context
        assert_eq!(sm.thresholds().observer_threshold, 15_000);
        
        // Update to 200k context
        let new_thresholds = config.compute_thresholds(200_000);
        sm.update_thresholds(new_thresholds);
        
        assert_eq!(sm.thresholds().observer_threshold, 30_000);
    }
    
    #[test]
    fn test_utilization_zero_budget() {
        let config = ThresholdConfig::with_ratios(0.0, 0.0, 0.0);
        let thresholds = config.compute_thresholds(100_000);
        
        // Observer threshold uses minimum_observer_tokens (1000) even with 0.0 ratio
        // So utilization is 1000/1000 = 1.0
        assert_eq!(thresholds.observer_threshold, 1000);
        assert_eq!(thresholds.observer_utilization(1000), 1.0);
        
        // Observation budget is truly 0 (no minimum)
        assert_eq!(thresholds.observation_budget, 0);
        // Should return 0 for utilization with zero budget
        assert_eq!(thresholds.observation_utilization(1000), 0.0);
    }
    
    #[test]
    fn test_state_transition_enum() {
        // Ensure StateTransition variants are comparable
        assert_ne!(StateTransition::None, StateTransition::ToObserverPending);
        assert_ne!(StateTransition::ToObserverPending, StateTransition::ToReflectorPending);
        
        let changed = StateTransition::Changed(TokenState::Normal, TokenState::ObserverPending);
        assert_ne!(changed, StateTransition::None);
    }
}
