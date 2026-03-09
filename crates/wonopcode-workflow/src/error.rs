//! Error types for workflow execution

use thiserror::Error;

/// Workflow-specific errors
#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Template error: {0}")]
    Template(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Workflow depth exceeded: max depth is {0}")]
    DepthExceeded(usize),

    #[error("Too many steps: {count} steps (max: {max})")]
    TooManySteps { count: usize, max: usize },

    #[error("Step timeout: {step_name} exceeded {timeout_secs}s")]
    StepTimeout { step_name: String, timeout_secs: u64 },

    #[error("Workflow timeout: exceeded {timeout_secs}s")]
    WorkflowTimeout { timeout_secs: u64 },

    #[error("Condition evaluation error: {0}")]
    ConditionEval(String),

    #[error("Input mapping error: {0}")]
    InputMapping(String),

    #[error("Output mapping error: {0}")]
    OutputMapping(String),

    #[error("Command execution error: {0}")]
    CommandExecution(String),

    #[error("User cancelled operation")]
    Cancelled,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("{0}")]
    Other(String),
}

/// Result type for workflow operations
pub type Result<T> = std::result::Result<T, WorkflowError>;

