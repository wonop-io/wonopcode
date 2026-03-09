//! Configuration types for workflow execution

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// Maximum nesting depth for sub-workflows
    pub max_depth: usize,

    /// Maximum steps per template
    pub max_steps: usize,

    /// Timeout for entire workflow
    #[serde(
        serialize_with = "serialize_duration_as_secs",
        deserialize_with = "deserialize_duration_from_secs"
    )]
    pub workflow_timeout: Duration,

    /// Timeout per step
    #[serde(
        serialize_with = "serialize_duration_as_secs",
        deserialize_with = "deserialize_duration_from_secs"
    )]
    pub step_timeout: Duration,
}

fn serialize_duration_as_secs<S>(
    duration: &Duration,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_u64(duration.as_secs())
}

fn deserialize_duration_from_secs<'de, D>(
    deserializer: D,
) -> std::result::Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let secs = u64::deserialize(deserializer)?;
    Ok(Duration::from_secs(secs))
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            max_depth: 5,
            max_steps: 20,
            workflow_timeout: Duration::from_secs(30 * 60), // 30 minutes
            step_timeout: Duration::from_secs(10 * 60),     // 10 minutes
        }
    }
}

/// Configuration for the workflow engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Directory containing templates
    pub templates_dir: PathBuf,

    /// Output directory for artifacts
    pub output_dir: PathBuf,

    /// Provider name (anthropic, openai, compoundcoders, etc.)
    pub provider: String,

    /// Model name (optional, uses provider default if not specified)
    pub model: Option<String>,

    /// Maximum output tokens for LLM responses
    pub max_tokens: Option<u64>,

    /// Maximum conversation turns before stopping
    pub max_conversation_turns: usize,

    /// Enable tool support
    pub enable_tools: bool,

    /// List of enabled tools (empty = all tools enabled)
    pub enabled_tools: Vec<String>,

    /// Workflow execution configuration
    pub workflow: WorkflowConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            templates_dir: PathBuf::from("templates"),
            output_dir: PathBuf::from("."),
            provider: "anthropic".to_string(),
            model: None,
            max_tokens: None,
            max_conversation_turns: 75,
            enable_tools: false,
            enabled_tools: Vec::new(),
            workflow: WorkflowConfig::default(),
        }
    }
}

impl EngineConfig {
    /// Create a new engine config with the given templates directory
    pub fn new(templates_dir: PathBuf) -> Self {
        Self {
            templates_dir,
            ..Default::default()
        }
    }

    /// Set the output directory
    pub fn with_output_dir(mut self, output_dir: PathBuf) -> Self {
        self.output_dir = output_dir;
        self
    }

    /// Set the provider
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Set the model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Enable tools
    pub fn with_tools(mut self, enabled: bool) -> Self {
        self.enable_tools = enabled;
        self
    }

    /// Set enabled tools
    pub fn with_enabled_tools(mut self, tools: Vec<String>) -> Self {
        self.enabled_tools = tools;
        self
    }

    /// Set max conversation turns
    pub fn with_max_turns(mut self, max_turns: usize) -> Self {
        self.max_conversation_turns = max_turns;
        self
    }

    /// Set workflow config
    pub fn with_workflow_config(mut self, config: WorkflowConfig) -> Self {
        self.workflow = config;
        self
    }
}

