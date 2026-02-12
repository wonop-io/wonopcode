//! Configuration loading for ACE framework integration.
//!
//! Loads configuration from `.wonopcode/config.yaml` in the repository root.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// ACE configuration section from .wonopcode/config.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AceConfig {
    /// Whether ACE is enabled for this repository.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Directory for specifications (relative to repository root).
    #[serde(default = "default_specs_dir")]
    pub specs_dir: String,
    /// Workflow configuration.
    #[serde(default)]
    pub workflow: WorkflowConfig,
    /// Quality gates configuration.
    #[serde(default)]
    pub gates: GatesConfig,
    /// Hooks configuration.
    #[serde(default)]
    pub hooks: HooksConfig,
}

fn default_enabled() -> bool {
    true
}

fn default_specs_dir() -> String {
    "specs".to_string()
}

impl Default for AceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            specs_dir: "specs".to_string(),
            workflow: WorkflowConfig::default(),
            gates: GatesConfig::default(),
            hooks: HooksConfig::default(),
        }
    }
}

/// Workflow configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// Workflow phases (default: requirements, analysis, design, implementation, verification, deployment).
    #[serde(default = "default_phases")]
    pub phases: Vec<String>,
    /// Checkpoint configuration.
    #[serde(default)]
    pub checkpoints: CheckpointsConfig,
}

fn default_phases() -> Vec<String> {
    vec![
        "requirements".to_string(),
        "analysis".to_string(),
        "design".to_string(),
        "implementation".to_string(),
        "verification".to_string(),
        "deployment".to_string(),
    ]
}

/// Checkpoint configurations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckpointsConfig {
    /// Requirements checkpoint configuration.
    #[serde(default)]
    pub requirements: CheckpointConfig,
    /// Design checkpoint configuration.
    #[serde(default)]
    pub design: CheckpointConfig,
    /// Verification checkpoint configuration.
    #[serde(default)]
    pub verification: CheckpointConfig,
}

/// Configuration for a single checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Whether this checkpoint requires human approval.
    #[serde(default = "default_checkpoint_required")]
    pub required: bool,
}

fn default_checkpoint_required() -> bool {
    true
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self { required: true }
    }
}

/// Quality gates configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatesConfig {
    /// Test coverage gate.
    #[serde(default)]
    pub test_coverage: TestCoverageGate,
    /// Lint gate.
    #[serde(default)]
    pub lint: LintGate,
}

/// Test coverage gate configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestCoverageGate {
    /// Whether the gate is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Minimum coverage threshold (percentage).
    #[serde(default = "default_coverage_threshold")]
    pub threshold: u32,
}

fn default_coverage_threshold() -> u32 {
    80
}

/// Lint gate configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintGate {
    /// Whether the gate is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Command to run for linting.
    #[serde(default)]
    pub command: Option<String>,
}

/// Hooks configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    /// Commands to run before a checkpoint.
    #[serde(default)]
    pub pre_checkpoint: Vec<String>,
    /// Commands to run after deployment.
    #[serde(default)]
    pub post_deployment: Vec<String>,
}

/// Root configuration file structure (.wonopcode/config.yaml).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WonopCodeConfig {
    /// ACE framework configuration.
    #[serde(default)]
    pub ace: AceConfig,
}

impl WonopCodeConfig {
    /// Load configuration from .wonopcode/config.yaml.
    ///
    /// Returns default configuration if file doesn't exist.
    pub fn load(root_dir: &Path) -> Result<Self> {
        let config_path = root_dir.join(".wonopcode").join("config.yaml");

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;

        let config: Self = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;

        Ok(config)
    }

    /// Save configuration to .wonopcode/config.yaml.
    pub fn save(&self, root_dir: &Path) -> Result<()> {
        let wonopcode_dir = root_dir.join(".wonopcode");
        std::fs::create_dir_all(&wonopcode_dir)?;

        let config_path = wonopcode_dir.join("config.yaml");
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&config_path, content)?;

        Ok(())
    }

    /// Get the absolute path to the specs directory.
    pub fn specs_dir(&self, root_dir: &Path) -> PathBuf {
        root_dir.join(&self.ace.specs_dir)
    }

    /// Check if ACE is enabled.
    pub fn is_enabled(&self) -> bool {
        self.ace.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = WonopCodeConfig::default();
        assert!(config.ace.enabled);
        assert_eq!(config.ace.specs_dir, "specs");
        assert!(config.ace.workflow.checkpoints.requirements.required);
    }

    #[test]
    fn test_load_missing_config() {
        let dir = tempdir().unwrap();
        let config = WonopCodeConfig::load(dir.path()).unwrap();
        assert!(config.ace.enabled);
        assert_eq!(config.ace.specs_dir, "specs");
    }

    #[test]
    fn test_load_custom_config() {
        let dir = tempdir().unwrap();
        let wonopcode_dir = dir.path().join(".wonopcode");
        std::fs::create_dir_all(&wonopcode_dir).unwrap();

        let config_content = r#"
ace:
  enabled: true
  specs_dir: "docs/specs"
  workflow:
    checkpoints:
      requirements:
        required: false
"#;
        std::fs::write(wonopcode_dir.join("config.yaml"), config_content).unwrap();

        let config = WonopCodeConfig::load(dir.path()).unwrap();
        assert!(config.ace.enabled);
        assert_eq!(config.ace.specs_dir, "docs/specs");
        assert!(!config.ace.workflow.checkpoints.requirements.required);
    }

    #[test]
    fn test_specs_dir_path() {
        let config = WonopCodeConfig::default();
        let root = Path::new("/project");
        assert_eq!(config.specs_dir(root), PathBuf::from("/project/specs"));
    }

    #[test]
    fn test_save_config() {
        let dir = tempdir().unwrap();
        let config = WonopCodeConfig::default();
        config.save(dir.path()).unwrap();

        let config_path = dir.path().join(".wonopcode").join("config.yaml");
        assert!(config_path.exists());

        let loaded = WonopCodeConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.ace.specs_dir, config.ace.specs_dir);
    }
}
