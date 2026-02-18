//! Tracker credentials management for ticket tool integration.
//!
//! This module provides secure storage and retrieval of issue tracker
//! configurations (GitHub, Linear) including API tokens.
//!
//! Credentials can be loaded from:
//! - Tauri app data: `~/Library/Application Support/com.wonop.code/trackers.json` (Desktop app)
//! - Global config: `~/.config/wonopcode/trackers.json` (CLI/TUI)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::CoreResult;

/// Legacy tracker format used by the Desktop app (Tauri).
/// This is a simple array of trackers, not the versioned format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyTracker {
    id: String,
    name: String,
    tracker_type: String, // "github" or "linear"
    enabled: bool,
    config: serde_json::Value,
}

impl LegacyTracker {
    /// Convert to the new TrackerCredential format
    fn to_credential(self) -> Option<TrackerCredential> {
        let tracker_type = match self.tracker_type.to_lowercase().as_str() {
            "github" => TrackerType::Github,
            "linear" => TrackerType::Linear,
            _ => {
                warn!(tracker_type = %self.tracker_type, "Unknown tracker type");
                return None;
            }
        };

        let config = match tracker_type {
            TrackerType::Github => {
                // Legacy format: { "owner": "...", "repo": "...", "api_token": "..." }
                let owner = match self.config.get("owner").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        warn!(tracker_id = %self.id, "GitHub tracker missing 'owner' field");
                        return None;
                    }
                };
                let repo = match self.config.get("repo").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        warn!(tracker_id = %self.id, "GitHub tracker missing 'repo' field");
                        return None;
                    }
                };
                let token = match self.config.get("api_token").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        warn!(tracker_id = %self.id, "GitHub tracker missing 'api_token' field");
                        return None;
                    }
                };
                TrackerConfig::Github(GithubTrackerConfig { owner, repo, token })
            }
            TrackerType::Linear => {
                // Legacy format: { "api_key": "...", "team_id": "..." }
                let api_key = match self.config.get("api_key").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        warn!(tracker_id = %self.id, "Linear tracker missing 'api_key' field");
                        return None;
                    }
                };
                let team_id = self.config.get("team_id").and_then(|v| v.as_str()).map(String::from);
                debug!(
                    tracker_id = %self.id,
                    name = %self.name,
                    has_team_id = team_id.is_some(),
                    "Parsed Linear tracker config"
                );
                TrackerConfig::Linear(LinearTrackerConfig { api_key, team_id })
            }
        };

        info!(
            tracker_id = %self.id,
            name = %self.name,
            tracker_type = %tracker_type,
            enabled = self.enabled,
            "Converted legacy tracker to credential"
        );

        Some(TrackerCredential {
            id: self.id,
            name: self.name,
            enabled: self.enabled,
            config,
        })
    }
}

/// Tracker type enum matching the protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerType {
    Github,
    Linear,
}

impl std::fmt::Display for TrackerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackerType::Github => write!(f, "github"),
            TrackerType::Linear => write!(f, "linear"),
        }
    }
}

/// GitHub-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTrackerConfig {
    /// Repository owner (user or organization)
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// Personal access token
    pub token: String,
}

/// Linear-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearTrackerConfig {
    /// Linear API key
    pub api_key: String,
    /// Optional team ID to scope issues
    pub team_id: Option<String>,
}

/// Tracker-specific configuration (with secrets)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TrackerConfig {
    Github(GithubTrackerConfig),
    Linear(LinearTrackerConfig),
}

impl TrackerConfig {
    /// Get the tracker type for this config
    pub fn tracker_type(&self) -> TrackerType {
        match self {
            TrackerConfig::Github(_) => TrackerType::Github,
            TrackerConfig::Linear(_) => TrackerType::Linear,
        }
    }

    /// Get display info (e.g., "owner/repo" for GitHub)
    pub fn display_info(&self) -> String {
        match self {
            TrackerConfig::Github(cfg) => format!("{}/{}", cfg.owner, cfg.repo),
            TrackerConfig::Linear(cfg) => {
                cfg.team_id.clone().unwrap_or_else(|| "All teams".to_string())
            }
        }
    }

    /// Parse from JSON value based on tracker type
    pub fn from_json(tracker_type: TrackerType, value: serde_json::Value) -> CoreResult<Self> {
        match tracker_type {
            TrackerType::Github => {
                let cfg: GithubTrackerConfig = serde_json::from_value(value)?;
                Ok(TrackerConfig::Github(cfg))
            }
            TrackerType::Linear => {
                let cfg: LinearTrackerConfig = serde_json::from_value(value)?;
                Ok(TrackerConfig::Linear(cfg))
            }
        }
    }
}

/// A complete tracker credential entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerCredential {
    /// Unique tracker ID
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Whether the tracker is enabled
    pub enabled: bool,
    /// Tracker-specific configuration (contains secrets)
    pub config: TrackerConfig,
}

impl TrackerCredential {
    /// Get the tracker type
    pub fn tracker_type(&self) -> TrackerType {
        self.config.tracker_type()
    }

    /// Get display info for UI
    pub fn display_info(&self) -> String {
        self.config.display_info()
    }
}

/// File format for trackers.json
#[derive(Debug, Serialize, Deserialize)]
struct TrackersFile {
    /// Version for future migrations
    version: u32,
    /// Map of tracker ID to credential
    trackers: HashMap<String, TrackerCredential>,
}

impl Default for TrackersFile {
    fn default() -> Self {
        Self {
            version: 1,
            trackers: HashMap::new(),
        }
    }
}

/// Manages tracker configurations and credentials.
///
/// This provides a unified interface for loading/saving tracker credentials
/// that works across all platforms (CLI, TUI, Desktop).
pub struct TrackerCredentialsManager {
    /// Path to the trackers file
    config_path: PathBuf,
    /// Cached trackers
    trackers: HashMap<String, TrackerCredential>,
}

impl TrackerCredentialsManager {
    /// Create a new tracker credentials manager.
    ///
    /// Tries to load trackers from multiple locations in order:
    /// 1. Tauri app data: `~/Library/Application Support/com.wonop.code/trackers.json`
    /// 2. Global config: `~/.config/wonopcode/trackers.json`
    pub fn new() -> Option<Self> {
        info!("TrackerCredentialsManager::new() called");
        
        // Try Tauri app data directory first (Desktop app)
        if let Some(app_data) = Self::tauri_app_data_path() {
            info!(path = %app_data.display(), exists = app_data.exists(), "Checking Tauri app data path");
            if app_data.exists() {
                let mut manager = Self {
                    config_path: app_data.clone(),
                    trackers: HashMap::new(),
                };
                if let Err(e) = manager.load_legacy() {
                    warn!(error = %e, path = %app_data.display(), "Failed to load legacy tracker credentials");
                } else if !manager.trackers.is_empty() {
                    info!(
                        path = %app_data.display(),
                        count = manager.trackers.len(),
                        "Loaded trackers from Tauri app data"
                    );
                    return Some(manager);
                } else {
                    debug!(path = %app_data.display(), "Tauri app data file exists but no trackers were loaded");
                }
            }
        } else {
            debug!("Tauri app data path not available");
        }

        // Fall back to global config directory
        let config_path = Config::global_config_dir()?.join("trackers.json");
        info!(path = %config_path.display(), "Falling back to global config path");
        let mut manager = Self {
            config_path,
            trackers: HashMap::new(),
        };

        // Load existing trackers
        if let Err(e) = manager.load() {
            warn!(error = %e, "Failed to load tracker credentials");
        }

        Some(manager)
    }

    /// Get the Tauri app data path for trackers.json
    /// 
    /// Tries multiple possible app identifiers since the app bundle ID has changed over time.
    fn tauri_app_data_path() -> Option<PathBuf> {
        let data_dir = dirs::data_dir()?;
        
        // List of app identifiers to check (in order of preference)
        // - io.wonop.wonopcode: Current Tauri app identifier
        // - com.wonop.code: Legacy identifier
        // - com.wonop.code.staging: Staging builds
        let app_ids = [
            "io.wonop.wonopcode",
            "com.wonop.code",
            "com.wonop.code.staging",
        ];
        
        for app_id in &app_ids {
            let path = data_dir.join(app_id).join("trackers.json");
            if path.exists() {
                debug!(path = %path.display(), app_id = %app_id, "Found trackers.json");
                return Some(path);
            }
        }
        
        // If none exist, return the primary path (io.wonop.wonopcode)
        // so new trackers will be created there
        Some(data_dir.join("io.wonop.wonopcode").join("trackers.json"))
    }

    /// Create a tracker credentials manager with a custom path.
    pub fn with_path(config_path: PathBuf) -> Self {
        let mut manager = Self {
            config_path,
            trackers: HashMap::new(),
        };

        if let Err(e) = manager.load() {
            warn!(error = %e, "Failed to load tracker credentials");
        }

        manager
    }

    /// Load trackers from file (new versioned format).
    fn load(&mut self) -> CoreResult<()> {
        if !self.config_path.exists() {
            debug!(path = %self.config_path.display(), "Trackers file not found");
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.config_path)?;
        let file: TrackersFile = serde_json::from_str(&content)?;

        self.trackers = file.trackers;
        debug!(
            path = %self.config_path.display(),
            count = self.trackers.len(),
            "Loaded tracker credentials"
        );

        Ok(())
    }

    /// Load trackers from legacy format (Tauri Desktop app).
    /// Legacy format is a JSON array of tracker objects.
    fn load_legacy(&mut self) -> CoreResult<()> {
        if !self.config_path.exists() {
            debug!(path = %self.config_path.display(), "Legacy trackers file not found");
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.config_path)?;
        debug!(
            path = %self.config_path.display(),
            content_len = content.len(),
            "Read legacy trackers file"
        );
        
        // Try to parse as legacy array format
        let legacy_trackers: Vec<LegacyTracker> = serde_json::from_str(&content)?;
        info!(
            path = %self.config_path.display(),
            count = legacy_trackers.len(),
            "Parsed legacy trackers JSON"
        );

        // Convert to new format
        let mut converted = 0;
        let mut failed = 0;
        for legacy in legacy_trackers {
            let id = legacy.id.clone();
            if let Some(credential) = legacy.to_credential() {
                self.trackers.insert(credential.id.clone(), credential);
                converted += 1;
            } else {
                warn!(tracker_id = %id, "Failed to convert legacy tracker");
                failed += 1;
            }
        }

        info!(
            path = %self.config_path.display(),
            converted = converted,
            failed = failed,
            total = self.trackers.len(),
            "Loaded legacy tracker credentials"
        );

        Ok(())
    }

    /// Save trackers to file.
    fn save(&self) -> CoreResult<()> {
        let file = TrackersFile {
            version: 1,
            trackers: self.trackers.clone(),
        };

        let content = serde_json::to_string_pretty(&file)?;

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&self.config_path, content)?;
        debug!(
            path = %self.config_path.display(),
            count = self.trackers.len(),
            "Saved tracker credentials"
        );

        Ok(())
    }

    /// List all configured trackers.
    pub fn list_trackers(&self) -> Vec<&TrackerCredential> {
        self.trackers.values().collect()
    }

    /// List enabled trackers only.
    pub fn list_enabled_trackers(&self) -> Vec<&TrackerCredential> {
        self.trackers.values().filter(|t| t.enabled).collect()
    }

    /// Get a specific tracker by ID.
    pub fn get_tracker(&self, id: &str) -> Option<&TrackerCredential> {
        self.trackers.get(id)
    }

    /// Add or update a tracker credential.
    pub fn set_tracker(&mut self, credential: TrackerCredential) -> CoreResult<()> {
        let id = credential.id.clone();
        self.trackers.insert(id.clone(), credential);
        self.save()?;
        debug!(tracker_id = %id, "Tracker credential saved");
        Ok(())
    }

    /// Remove a tracker credential.
    pub fn remove_tracker(&mut self, id: &str) -> CoreResult<bool> {
        if self.trackers.remove(id).is_some() {
            self.save()?;
            debug!(tracker_id = %id, "Tracker credential removed");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Enable or disable a tracker.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> CoreResult<bool> {
        if let Some(tracker) = self.trackers.get_mut(id) {
            tracker.enabled = enabled;
            self.save()?;
            debug!(tracker_id = %id, enabled = enabled, "Tracker enabled state changed");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if any trackers are configured.
    pub fn has_trackers(&self) -> bool {
        !self.trackers.is_empty()
    }

    /// Check if any trackers are enabled.
    pub fn has_enabled_trackers(&self) -> bool {
        self.trackers.values().any(|t| t.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_tracker_config_github() {
        let json = serde_json::json!({
            "owner": "wonop-io",
            "repo": "wonopcode",
            "token": "ghp_xxx"
        });

        let config = TrackerConfig::from_json(TrackerType::Github, json).unwrap();
        assert_eq!(config.tracker_type(), TrackerType::Github);
        assert_eq!(config.display_info(), "wonop-io/wonopcode");
    }

    #[test]
    fn test_tracker_config_linear() {
        let json = serde_json::json!({
            "api_key": "lin_xxx",
            "team_id": "TEAM-123"
        });

        let config = TrackerConfig::from_json(TrackerType::Linear, json).unwrap();
        assert_eq!(config.tracker_type(), TrackerType::Linear);
        assert_eq!(config.display_info(), "TEAM-123");
    }

    #[test]
    fn test_tracker_credentials_manager() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("trackers.json");

        let mut manager = TrackerCredentialsManager::with_path(config_path.clone());

        // Initially empty
        assert!(!manager.has_trackers());

        // Add a tracker
        let credential = TrackerCredential {
            id: "test-github".to_string(),
            name: "Test GitHub".to_string(),
            enabled: true,
            config: TrackerConfig::Github(GithubTrackerConfig {
                owner: "test".to_string(),
                repo: "repo".to_string(),
                token: "token".to_string(),
            }),
        };

        manager.set_tracker(credential).unwrap();
        assert!(manager.has_trackers());
        assert!(manager.has_enabled_trackers());

        // Reload and verify persistence
        let manager2 = TrackerCredentialsManager::with_path(config_path);
        assert!(manager2.has_trackers());
        assert_eq!(manager2.get_tracker("test-github").unwrap().name, "Test GitHub");
    }

    #[test]
    fn test_remove_tracker() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("trackers.json");

        let mut manager = TrackerCredentialsManager::with_path(config_path);

        let credential = TrackerCredential {
            id: "to-remove".to_string(),
            name: "To Remove".to_string(),
            enabled: true,
            config: TrackerConfig::Linear(LinearTrackerConfig {
                api_key: "key".to_string(),
                team_id: None,
            }),
        };

        manager.set_tracker(credential).unwrap();
        assert!(manager.has_trackers());

        manager.remove_tracker("to-remove").unwrap();
        assert!(!manager.has_trackers());
    }

    #[test]
    fn test_load_legacy_linear_format() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("trackers.json");

        // Write legacy format - the exact format used by Tauri Desktop app
        let legacy_json = r#"[
  {
    "id": "tracker-1763472039984",
    "name": "Linear",
    "tracker_type": "linear",
    "enabled": true,
    "config": {
      "api_key": "lin_api_test123",
      "team_id": "WON"
    }
  }
]"#;
        std::fs::write(&config_path, legacy_json).unwrap();

        let mut manager = TrackerCredentialsManager::with_path(config_path.clone());
        
        // This should fail because with_path calls load() not load_legacy()
        // Let's manually test load_legacy
        manager.trackers.clear();
        manager.load_legacy().unwrap();
        
        println!("Trackers loaded: {}", manager.trackers.len());
        for (id, cred) in &manager.trackers {
            println!("  - {}: {} ({})", id, cred.name, cred.enabled);
        }
        
        assert!(manager.has_trackers(), "Should have trackers after load_legacy");
        assert!(manager.has_enabled_trackers(), "Should have enabled trackers");
        
        let tracker = manager.get_tracker("tracker-1763472039984").unwrap();
        assert_eq!(tracker.name, "Linear");
        assert!(tracker.enabled);
        
        match &tracker.config {
            TrackerConfig::Linear(cfg) => {
                assert_eq!(cfg.api_key, "lin_api_test123");
                assert_eq!(cfg.team_id, Some("WON".to_string()));
            }
            _ => panic!("Expected Linear tracker config"),
        }
    }

    #[test]
    fn test_new_loads_from_real_tauri_path() {
        // This test verifies that TrackerCredentialsManager::new() 
        // can find and load the actual trackers.json from Tauri app data.
        // The test passes whether or not trackers are configured - it just
        // verifies the loading mechanism works.
        
        // TrackerCredentialsManager::new() should not panic
        let _ = TrackerCredentialsManager::new();
    }
}
