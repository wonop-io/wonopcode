//! Model state persistence.
//!
//! Saves and loads the user's model selection between sessions.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Model state that persists between sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelState {
    /// Recently used models (provider/model format).
    #[serde(default)]
    pub recent: Vec<String>,
    /// Favorite/pinned models.
    #[serde(default)]
    pub favorite: Vec<String>,
}

impl ModelState {
    /// Maximum number of recent models to track.
    const MAX_RECENT: usize = 10;

    /// Load model state from the default file location.
    pub fn load() -> Self {
        if let Some(path) = Self::file_path() {
            Self::load_from(&path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Load model state from a specific file.
    pub fn load_from(path: &PathBuf) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save model state to the default file location.
    pub fn save(&self) {
        if let Some(path) = Self::file_path() {
            self.save_to(&path);
        }
    }

    /// Save model state to a specific file.
    pub fn save_to(&self, path: &PathBuf) {
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, content);
        }
    }

    /// Get the default file path for model state.
    pub fn file_path() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir().map(|h| h.join("Library/Application Support/wonopcode/model.json"))
        }

        #[cfg(target_os = "linux")]
        {
            dirs::state_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
                .map(|d| d.join("wonopcode/model.json"))
        }

        #[cfg(target_os = "windows")]
        {
            dirs::data_local_dir().map(|d| d.join("wonopcode/model.json"))
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }

    /// Get the most recently used model.
    pub fn most_recent(&self) -> Option<&str> {
        self.recent.first().map(|s| s.as_str())
    }

    /// Add a model to the recent list.
    pub fn add_recent(&mut self, model: String) {
        // Remove if already exists (to move to front)
        self.recent.retain(|m| m != &model);
        // Add to front
        self.recent.insert(0, model);
        // Trim to max size
        if self.recent.len() > Self::MAX_RECENT {
            self.recent.truncate(Self::MAX_RECENT);
        }
    }

    /// Toggle a model as favorite.
    pub fn toggle_favorite(&mut self, model: &str) {
        if self.favorite.contains(&model.to_string()) {
            self.favorite.retain(|m| m != model);
        } else {
            self.favorite.push(model.to_string());
        }
    }

    /// Check if a model is a favorite.
    pub fn is_favorite(&self, model: &str) -> bool {
        self.favorite.contains(&model.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_recent() {
        let mut state = ModelState::default();
        state.add_recent("anthropic/claude-3".to_string());
        state.add_recent("openai/gpt-4o".to_string());
        state.add_recent("anthropic/claude-3".to_string()); // Should move to front

        assert_eq!(state.recent.len(), 2);
        assert_eq!(state.most_recent(), Some("anthropic/claude-3"));
    }

    #[test]
    fn test_toggle_favorite() {
        let mut state = ModelState::default();

        state.toggle_favorite("openai/gpt-4o");
        assert!(state.is_favorite("openai/gpt-4o"));

        state.toggle_favorite("openai/gpt-4o");
        assert!(!state.is_favorite("openai/gpt-4o"));
    }
}
