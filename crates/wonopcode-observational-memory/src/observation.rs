//! Observational Memory types.
//!
//! This module defines the core data structures for the Observational Memory system,
//! which maintains a dense, append-only event log that replaces raw messages as they accumulate.
//!
//! # Priority Levels
//!
//! Observations are tagged with priority levels using emoji markers:
//! - 🔴 High: Critical context the agent needs (deadlines, architecture decisions)
//! - 🟡 Medium: Potentially relevant (questions asked, intermediate results)
//! - 🟢 Low: Informational only (routine tool outputs, minor details)
//!
//! # Three-Date Model
//!
//! Each observation carries up to three temporal anchors:
//! - `observation_date`: When the observation was created
//! - `referenced_date`: A date mentioned in the content (e.g., "deadline is January 22nd")
//! - `relative_date`: Computed offset from observation date (e.g., "2 days from now")

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Priority level for an observation.
///
/// These priorities serve as signals from the Observer to the Reflector.
/// The Reflector uses priorities to decide what to keep, merge, or drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// 🔴 Critical context the agent needs.
    /// Examples: User's app name, architecture decisions, deadlines, user corrections.
    High,
    
    /// 🟡 Potentially relevant information.
    /// Examples: Questions asked, intermediate results, agent reasoning.
    #[default]
    Medium,
    
    /// 🟢 Informational only.
    /// Examples: Routine tool outputs, verbose compiler warnings, boilerplate metadata.
    Low,
}

impl Priority {
    /// Returns the emoji marker for this priority level.
    pub fn emoji(&self) -> &'static str {
        match self {
            Priority::High => "🔴",
            Priority::Medium => "🟡",
            Priority::Low => "🟢",
        }
    }
    
    /// Parse priority from an emoji marker.
    pub fn from_emoji(s: &str) -> Option<Self> {
        match s.trim() {
            "🔴" => Some(Priority::High),
            "🟡" => Some(Priority::Medium),
            "🟢" => Some(Priority::Low),
            _ => None,
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.emoji())
    }
}

/// Category for grouping observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationCategory {
    /// User preferences and patterns.
    Preference,
    /// Decisions made during the session.
    Decision,
    /// Important context about the task/project.
    Context,
    /// Factual information discovered.
    Fact,
    /// Technical details (file paths, configs, etc.).
    Technical,
    /// Agent actions and tool call results.
    Action,
}

/// A single observation extracted by the Observer Agent.
///
/// Observations are dated, prioritized events that capture specific facts,
/// decisions, state changes, and user preferences. Unlike summaries, observations
/// preserve event-level granularity with temporal anchoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Unique identifier (UUID).
    pub id: String,
    
    /// Priority level (High/Medium/Low).
    pub priority: Priority,
    
    /// Category for grouping.
    pub category: ObservationCategory,
    
    /// The observation content.
    pub content: String,
    
    /// Confidence score from Observer (0.0 - 1.0).
    pub confidence: f32,
    
    /// When this observation was created.
    pub observation_date: DateTime<Utc>,
    
    /// A date referenced in the content (e.g., "deadline is January 22nd").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referenced_date: Option<DateTime<Utc>>,
    
    /// Computed offset description (e.g., "7 days from today").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_date: Option<String>,
    
    /// Source message indices that produced this observation.
    pub source_message_range: (usize, usize),
    
    /// Number of times this observation has been reinforced.
    #[serde(default)]
    pub reinforcement_count: u32,
    
    /// Whether user has pinned this observation (prevents eviction).
    #[serde(default)]
    pub pinned: bool,
    
    /// Whether user has dismissed this observation.
    #[serde(default)]
    pub dismissed: bool,
    
    /// Child observations (for hierarchical grouping).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Observation>,
}

impl Observation {
    /// Create a new observation with the given priority, category, and content.
    pub fn new(
        priority: Priority,
        category: ObservationCategory,
        content: impl Into<String>,
        confidence: f32,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            priority,
            category,
            content: content.into(),
            confidence,
            observation_date: Utc::now(),
            referenced_date: None,
            relative_date: None,
            source_message_range: (0, 0),
            reinforcement_count: 0,
            pinned: false,
            dismissed: false,
            children: Vec::new(),
        }
    }
    
    /// Estimate token count for this observation.
    ///
    /// Uses a rough approximation of 4 characters per token plus overhead for metadata.
    pub fn estimate_tokens(&self) -> u32 {
        let content_tokens = (self.content.len() / 4) as u32;
        let children_tokens: u32 = self.children.iter().map(|c| c.estimate_tokens()).sum();
        content_tokens + children_tokens + 10 // +10 for metadata overhead
    }
    
    /// Estimate tokens using a specific chars-per-token ratio.
    pub fn estimate_tokens_with_ratio(&self, chars_per_token: f32) -> u32 {
        let content_tokens = (self.content.len() as f32 / chars_per_token).ceil() as u32;
        let children_tokens: u32 = self.children
            .iter()
            .map(|c| c.estimate_tokens_with_ratio(chars_per_token))
            .sum();
        content_tokens + children_tokens + 10 // +10 for metadata overhead
    }
    
    /// Add a child observation.
    pub fn with_child(mut self, child: Observation) -> Self {
        self.children.push(child);
        self
    }
    
    /// Set the referenced date.
    pub fn with_referenced_date(mut self, date: DateTime<Utc>) -> Self {
        self.referenced_date = Some(date);
        self
    }
    
    /// Set the relative date description.
    pub fn with_relative_date(mut self, relative: impl Into<String>) -> Self {
        self.relative_date = Some(relative.into());
        self
    }
    
    /// Set the source message range.
    pub fn with_source_range(mut self, start: usize, end: usize) -> Self {
        self.source_message_range = (start, end);
        self
    }
    
    /// Pin this observation (prevents eviction by Reflector).
    pub fn pin(&mut self) {
        self.pinned = true;
    }
    
    /// Dismiss this observation (will be removed from context).
    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }
    
    /// Reinforce this observation (increases importance).
    pub fn reinforce(&mut self) {
        self.reinforcement_count += 1;
    }
    
    /// Check if this observation should be included in the prompt.
    pub fn is_active(&self) -> bool {
        !self.dismissed
    }
}

/// A date-grouped collection of observations.
///
/// Used for organizing observations by date in the observation log format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateGroup {
    /// The date for this group.
    pub date: DateTime<Utc>,
    
    /// Observations for this date.
    pub observations: Vec<Observation>,
}

impl DateGroup {
    /// Create a new date group.
    pub fn new(date: DateTime<Utc>) -> Self {
        Self {
            date,
            observations: Vec::new(),
        }
    }
    
    /// Add an observation to this group.
    pub fn add(&mut self, observation: Observation) {
        self.observations.push(observation);
    }
    
    /// Total estimated tokens for this group.
    pub fn estimate_tokens(&self) -> u32 {
        self.observations.iter().map(|o| o.estimate_tokens()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_priority_emoji() {
        assert_eq!(Priority::High.emoji(), "🔴");
        assert_eq!(Priority::Medium.emoji(), "🟡");
        assert_eq!(Priority::Low.emoji(), "🟢");
    }
    
    #[test]
    fn test_priority_from_emoji() {
        assert_eq!(Priority::from_emoji("🔴"), Some(Priority::High));
        assert_eq!(Priority::from_emoji("🟡"), Some(Priority::Medium));
        assert_eq!(Priority::from_emoji("🟢"), Some(Priority::Low));
        assert_eq!(Priority::from_emoji("invalid"), None);
    }
    
    #[test]
    fn test_observation_new() {
        let obs = Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "User chose JWT for authentication",
            0.95,
        );
        
        assert_eq!(obs.priority, Priority::High);
        assert_eq!(obs.category, ObservationCategory::Decision);
        assert_eq!(obs.content, "User chose JWT for authentication");
        assert!((obs.confidence - 0.95).abs() < 0.001);
        assert!(!obs.pinned);
        assert!(!obs.dismissed);
        assert!(obs.is_active());
    }
    
    #[test]
    fn test_observation_estimate_tokens() {
        let obs = Observation::new(
            Priority::Medium,
            ObservationCategory::Fact,
            "This is a test observation with about 40 characters", // ~10 tokens at 4 chars/token
            0.8,
        );
        
        // 51 chars / 4 = ~12 tokens + 10 overhead = 22
        let tokens = obs.estimate_tokens();
        assert!(tokens > 10 && tokens < 30);
    }
    
    #[test]
    fn test_observation_with_children() {
        let parent = Observation::new(
            Priority::High,
            ObservationCategory::Context,
            "User is building a Next.js app",
            0.9,
        )
        .with_child(Observation::new(
            Priority::Medium,
            ObservationCategory::Technical,
            "Using App Router",
            0.85,
        ))
        .with_child(Observation::new(
            Priority::Low,
            ObservationCategory::Fact,
            "Project started today",
            0.7,
        ));
        
        assert_eq!(parent.children.len(), 2);
    }
    
    #[test]
    fn test_observation_pin_dismiss() {
        let mut obs = Observation::new(
            Priority::Medium,
            ObservationCategory::Preference,
            "User prefers tabs over spaces",
            0.9,
        );
        
        assert!(obs.is_active());
        
        obs.dismiss();
        assert!(!obs.is_active());
        
        let mut obs2 = Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "Important decision",
            0.95,
        );
        obs2.pin();
        assert!(obs2.pinned);
    }
    
    #[test]
    fn test_date_group() {
        let mut group = DateGroup::new(Utc::now());
        
        group.add(Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "Decision 1",
            0.9,
        ));
        group.add(Observation::new(
            Priority::Medium,
            ObservationCategory::Fact,
            "Fact 1",
            0.8,
        ));
        
        assert_eq!(group.observations.len(), 2);
        assert!(group.estimate_tokens() > 0);
    }
}
