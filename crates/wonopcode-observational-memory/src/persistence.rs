//! Persistence for Observational Memory across sessions.
//!
//! This module provides file-based persistence for the observation log,
//! enabling cross-session memory. Observations are stored in the project's
//! `.wonopcode/memory/` directory.
//!
//! # File Structure
//!
//! ```text
//! .wonopcode/
//! └── memory/
//!     ├── observations.json      # Current session observations
//!     ├── observations.bak.json  # Backup of previous session
//!     └── stats.json             # Session statistics
//! ```
//!
//! # Cross-Session Memory
//!
//! When a session ends, observations are persisted. When a new session starts,
//! the previous session's observations are loaded and can bootstrap the agent's
//! context, giving it "memory" of the project.

use crate::{MemoryState, MemoryStats, Observation, Priority};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors from persistence operations.
#[derive(Debug, Error)]
pub enum PersistenceError {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Directory not found.
    #[error("Directory not found: {0}")]
    DirectoryNotFound(PathBuf),
    /// Invalid file format.
    #[error("Invalid file format")]
    InvalidFormat,
}

/// Persisted observation data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedObservations {
    /// Version for format compatibility.
    pub version: u32,
    /// Session ID that created these observations.
    pub session_id: String,
    /// Workstream/project ID.
    pub workstream_id: Option<String>,
    /// When the observations were saved.
    pub saved_at: DateTime<Utc>,
    /// The observations.
    pub observations: Vec<Observation>,
    /// Session statistics.
    pub stats: MemoryStats,
}

impl PersistedObservations {
    /// Current persistence format version.
    pub const CURRENT_VERSION: u32 = 1;

    /// Create from a memory state.
    pub fn from_memory_state(state: &MemoryState) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            session_id: state.session_id.clone(),
            workstream_id: state.workstream_id.clone(),
            saved_at: Utc::now(),
            observations: state.observations.clone(),
            stats: state.stats.clone(),
        }
    }

    /// Check if these observations are stale.
    ///
    /// Observations older than the threshold are considered stale
    /// and may need reflection before use.
    pub fn is_stale(&self, max_age: chrono::Duration) -> bool {
        Utc::now() - self.saved_at > max_age
    }

    /// Get the age of these observations.
    pub fn age(&self) -> chrono::Duration {
        Utc::now() - self.saved_at
    }
    
    /// Get the saved_at date formatted as a string (e.g., "March 2nd").
    pub fn saved_at_formatted(&self) -> String {
        self.saved_at.format("%B %-d").to_string()
    }
}

/// Observation persistence manager.
#[derive(Debug, Clone)]
pub struct ObservationPersistence {
    /// Root directory for persistence (usually .wonopcode).
    root_dir: PathBuf,
    /// Memory subdirectory.
    memory_dir: PathBuf,
}

impl ObservationPersistence {
    /// Name of the observations file.
    const OBSERVATIONS_FILE: &'static str = "observations.json";
    /// Name of the backup file.
    const BACKUP_FILE: &'static str = "observations.bak.json";
    /// Name of the stats file.
    const STATS_FILE: &'static str = "stats.json";

    /// Create a new persistence manager.
    ///
    /// # Arguments
    /// * `project_dir` - The project root directory.
    pub fn new(project_dir: impl AsRef<Path>) -> Self {
        let root_dir = project_dir.as_ref().join(".wonopcode");
        let memory_dir = root_dir.join("memory");

        Self {
            root_dir,
            memory_dir,
        }
    }

    /// Create from an explicit wonopcode directory.
    pub fn from_wonopcode_dir(wonopcode_dir: impl AsRef<Path>) -> Self {
        let root_dir = wonopcode_dir.as_ref().to_path_buf();
        let memory_dir = root_dir.join("memory");

        Self {
            root_dir,
            memory_dir,
        }
    }

    /// Get the root directory.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Ensure the memory directory exists.
    pub fn ensure_directory(&self) -> Result<(), PersistenceError> {
        if !self.memory_dir.exists() {
            fs::create_dir_all(&self.memory_dir)?;
        }
        Ok(())
    }

    /// Get the path to the observations file.
    pub fn observations_path(&self) -> PathBuf {
        self.memory_dir.join(Self::OBSERVATIONS_FILE)
    }

    /// Get the path to the backup file.
    pub fn backup_path(&self) -> PathBuf {
        self.memory_dir.join(Self::BACKUP_FILE)
    }

    /// Get the path to the stats file.
    pub fn stats_path(&self) -> PathBuf {
        self.memory_dir.join(Self::STATS_FILE)
    }

    /// Save observations to disk.
    pub fn save(&self, state: &MemoryState) -> Result<(), PersistenceError> {
        self.ensure_directory()?;

        let data = PersistedObservations::from_memory_state(state);

        // Create backup of existing file
        let obs_path = self.observations_path();
        if obs_path.exists() {
            fs::copy(&obs_path, self.backup_path())?;
        }

        // Write new observations
        let json = serde_json::to_string_pretty(&data)?;
        fs::write(&obs_path, json)?;

        // Write stats separately for quick access
        let stats_json = serde_json::to_string_pretty(&state.stats)?;
        fs::write(self.stats_path(), stats_json)?;

        tracing::info!(
            "Saved {} observations to {}",
            data.observations.len(),
            obs_path.display()
        );

        Ok(())
    }

    /// Load observations from disk.
    ///
    /// Returns None if no saved observations exist.
    pub fn load(&self) -> Result<Option<PersistedObservations>, PersistenceError> {
        let obs_path = self.observations_path();

        if !obs_path.exists() {
            return Ok(None);
        }

        let json = fs::read_to_string(&obs_path)?;
        let data: PersistedObservations = serde_json::from_str(&json)?;

        // Version check
        if data.version > PersistedObservations::CURRENT_VERSION {
            tracing::warn!(
                "Observation file version {} is newer than supported version {}",
                data.version,
                PersistedObservations::CURRENT_VERSION
            );
        }

        tracing::info!(
            "Loaded {} observations from {} (saved {})",
            data.observations.len(),
            obs_path.display(),
            humanize_duration(data.age())
        );

        Ok(Some(data))
    }

    /// Load observations into a new MemoryState.
    ///
    /// This creates a new MemoryState with the loaded observations
    /// and marks it as loaded from a previous session.
    pub fn load_into_state(
        &self,
        session_id: impl Into<String>,
        observation_token_budget: u32,
    ) -> Result<(MemoryState, Option<String>), PersistenceError> {
        let mut state = MemoryState::new(session_id.into(), observation_token_budget);
        let mut loaded_session_date = None;

        if let Some(persisted) = self.load()? {
            loaded_session_date = Some(persisted.saved_at_formatted());
            state.observations = persisted.observations;
            state.workstream_id = persisted.workstream_id;
            state.loaded_from_previous_session = true;

            // Recalculate observation tokens
            state.observation_tokens = state
                .observations
                .iter()
                .filter(|o| o.is_active())
                .map(|o| o.estimate_tokens())
                .sum();
        }

        Ok((state, loaded_session_date))
    }

    /// Check if there are saved observations.
    pub fn has_saved_observations(&self) -> bool {
        self.observations_path().exists()
    }

    /// Get stats without loading full observations.
    pub fn load_stats(&self) -> Result<Option<MemoryStats>, PersistenceError> {
        let stats_path = self.stats_path();

        if !stats_path.exists() {
            return Ok(None);
        }

        let json = fs::read_to_string(&stats_path)?;
        let stats: MemoryStats = serde_json::from_str(&json)?;

        Ok(Some(stats))
    }

    /// Clear all persisted observations.
    pub fn clear(&self) -> Result<(), PersistenceError> {
        if self.observations_path().exists() {
            fs::remove_file(self.observations_path())?;
        }
        if self.backup_path().exists() {
            fs::remove_file(self.backup_path())?;
        }
        if self.stats_path().exists() {
            fs::remove_file(self.stats_path())?;
        }

        tracing::info!("Cleared persisted observations");
        Ok(())
    }

    /// Restore from backup if main file is corrupted.
    pub fn restore_from_backup(&self) -> Result<bool, PersistenceError> {
        let backup_path = self.backup_path();
        let obs_path = self.observations_path();

        if !backup_path.exists() {
            return Ok(false);
        }

        fs::copy(&backup_path, &obs_path)?;
        tracing::info!("Restored observations from backup");

        Ok(true)
    }
}

/// Convert a duration to a human-readable string.
fn humanize_duration(duration: chrono::Duration) -> String {
    let days = duration.num_days();
    let hours = duration.num_hours();
    let minutes = duration.num_minutes();

    if days > 0 {
        format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
    } else if hours > 0 {
        format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
    } else if minutes > 0 {
        format!("{} minute{} ago", minutes, if minutes == 1 { "" } else { "s" })
    } else {
        "just now".to_string()
    }
}

/// Cleanup stale observations based on age.
///
/// # Arguments
/// * `persistence` - The persistence manager.
/// * `max_age_days` - Maximum age in days before observations are considered stale.
/// * `action` - What to do with stale observations.
pub fn cleanup_stale_observations(
    persistence: &ObservationPersistence,
    max_age_days: i64,
    action: StaleObservationAction,
) -> Result<CleanupResult, PersistenceError> {
    let persisted = match persistence.load()? {
        Some(p) => p,
        None => {
            return Ok(CleanupResult {
                was_stale: false,
                observations_removed: 0,
                observations_kept: 0,
            })
        }
    };

    let max_age = chrono::Duration::days(max_age_days);
    let is_stale = persisted.is_stale(max_age);

    if !is_stale {
        return Ok(CleanupResult {
            was_stale: false,
            observations_removed: 0,
            observations_kept: persisted.observations.len(),
        });
    }

    match action {
        StaleObservationAction::Clear => {
            let count = persisted.observations.len();
            persistence.clear()?;
            Ok(CleanupResult {
                was_stale: true,
                observations_removed: count,
                observations_kept: 0,
            })
        }
        StaleObservationAction::Keep => Ok(CleanupResult {
            was_stale: true,
            observations_removed: 0,
            observations_kept: persisted.observations.len(),
        }),
        StaleObservationAction::DropLowPriority => {
            // Keep only high priority observations
            let high_priority: Vec<_> = persisted
                .observations
                .into_iter()
                .filter(|o| o.priority == Priority::High || o.pinned)
                .collect();

            let original_count = persisted.stats.total_observations as usize;
            let kept = high_priority.len();
            let removed = original_count.saturating_sub(kept);

            // Create a minimal state just for saving
            let mut state = MemoryState::new(persisted.session_id, 40_000);
            state.observations = high_priority;
            state.workstream_id = persisted.workstream_id;

            persistence.save(&state)?;

            Ok(CleanupResult {
                was_stale: true,
                observations_removed: removed,
                observations_kept: kept,
            })
        }
    }
}

/// Action to take with stale observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleObservationAction {
    /// Clear all stale observations.
    Clear,
    /// Keep stale observations (do nothing).
    Keep,
    /// Drop low-priority observations, keep high-priority and pinned.
    DropLowPriority,
}

/// Result of a cleanup operation.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// Whether the observations were stale.
    pub was_stale: bool,
    /// Number of observations removed.
    pub observations_removed: usize,
    /// Number of observations kept.
    pub observations_kept: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObservationCategory;
    use tempfile::TempDir;

    fn create_test_state() -> MemoryState {
        let mut state = MemoryState::new("test-session".to_string(), 40_000);
        state.workstream_id = Some("test-project".to_string());

        // Add some test observations
        state.observations.push(Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "User chose Rust for the backend",
            0.95,
        ));
        state.observations.push(Observation::new(
            Priority::Medium,
            ObservationCategory::Preference,
            "User prefers explicit error handling",
            0.8,
        ));

        state
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = ObservationPersistence::new(temp_dir.path());

        let state = create_test_state();
        persistence.save(&state).unwrap();

        // Verify files exist
        assert!(persistence.observations_path().exists());
        assert!(persistence.stats_path().exists());

        // Load and verify
        let loaded = persistence.load().unwrap().unwrap();
        assert_eq!(loaded.observations.len(), 2);
        assert_eq!(loaded.session_id, "test-session");
        assert_eq!(loaded.workstream_id, Some("test-project".to_string()));
    }

    #[test]
    fn test_load_into_state() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = ObservationPersistence::new(temp_dir.path());

        let state = create_test_state();
        persistence.save(&state).unwrap();

        // Load into new state
        let (new_state, loaded_date) = persistence
            .load_into_state("new-session", 50_000)
            .unwrap();

        assert_eq!(new_state.session_id, "new-session");
        assert_eq!(new_state.observations.len(), 2);
        assert!(new_state.loaded_from_previous_session);
        assert!(new_state.observation_tokens > 0);
        assert!(loaded_date.is_some());
    }

    #[test]
    fn test_backup() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = ObservationPersistence::new(temp_dir.path());

        // Save twice to create backup
        let mut state = create_test_state();
        persistence.save(&state).unwrap();

        state.observations.push(Observation::new(
            Priority::Low,
            ObservationCategory::Fact,
            "Additional observation",
            0.5,
        ));
        persistence.save(&state).unwrap();

        // Backup should exist
        assert!(persistence.backup_path().exists());
    }

    #[test]
    fn test_clear() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = ObservationPersistence::new(temp_dir.path());

        let state = create_test_state();
        persistence.save(&state).unwrap();
        assert!(persistence.has_saved_observations());

        persistence.clear().unwrap();
        assert!(!persistence.has_saved_observations());
    }

    #[test]
    fn test_humanize_duration() {
        assert_eq!(
            humanize_duration(chrono::Duration::days(5)),
            "5 days ago"
        );
        assert_eq!(
            humanize_duration(chrono::Duration::hours(3)),
            "3 hours ago"
        );
        assert_eq!(
            humanize_duration(chrono::Duration::minutes(30)),
            "30 minutes ago"
        );
        assert_eq!(
            humanize_duration(chrono::Duration::seconds(10)),
            "just now"
        );
    }
}
