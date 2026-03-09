//! IO abstraction for workflow execution
//!
//! This module defines traits that allow the workflow engine to communicate
//! with different frontends (terminal, Tauri UI, HTTP API, etc.) without
//! being coupled to any specific IO implementation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use wonop_templates::Artifact;

/// Events emitted during workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowEvent {
    /// Workflow execution started
    WorkflowStarted {
        template_name: String,
        total_steps: usize,
    },

    /// Workflow execution completed
    WorkflowCompleted {
        template_name: String,
        total_duration_ms: u64,
        artifacts_count: usize,
    },

    /// A step has started
    StepStarted {
        step_index: usize,
        step_name: String,
        step_type: String,
        description: Option<String>,
    },

    /// A step has completed successfully
    StepCompleted {
        step_index: usize,
        step_name: String,
        duration_ms: u64,
        artifacts: Vec<Artifact>,
    },

    /// A step was skipped (condition not met)
    StepSkipped {
        step_index: usize,
        step_name: String,
        reason: String,
    },

    /// Progress update (e.g., "Sending request to provider...")
    Progress {
        message: String,
    },

    /// LLM response received with token usage
    TokenUsage {
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
    },

    /// An artifact was created
    ArtifactCreated {
        artifact: Artifact,
    },

    /// Text output from LLM (for streaming display)
    TextOutput {
        text: String,
        is_complete: bool,
    },

    /// Tool call started
    ToolCallStarted {
        tool_name: String,
        tool_id: String,
    },

    /// Tool call completed
    ToolCallCompleted {
        tool_name: String,
        tool_id: String,
        success: bool,
        output: Option<String>,
    },

    /// Error occurred (non-fatal, workflow may continue)
    Warning {
        message: String,
    },

    /// Fatal error
    Error {
        message: String,
    },

    /// Conversation turn started
    ConversationTurnStarted {
        turn: usize,
        max_turns: usize,
    },

    /// Conversation completed
    ConversationCompleted {
        turns: usize,
        total_tokens: usize,
        duration: Duration,
    },
}

/// Input request from the workflow engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputRequest {
    /// Unique ID for this request
    pub id: String,
    /// The prompt to display
    pub prompt: String,
    /// Field name this input is for
    pub field: String,
    /// Whether the input is required
    pub required: bool,
    /// Default value if any
    pub default: Option<String>,
    /// Input type hint (text, password, multiline, select)
    pub input_type: InputType,
    /// Options for select input type
    pub options: Option<Vec<String>>,
}

/// Type of input expected
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    #[default]
    Text,
    Password,
    Multiline,
    Select,
    Confirm,
}

/// Trait for handling workflow IO
///
/// Implementations of this trait provide the bridge between the workflow
/// engine and the user interface. This allows the same workflow engine
/// to be used with:
/// - Terminal/CLI (proto binary)
/// - Tauri desktop app
/// - HTTP/WebSocket API
/// - Programmatic use in other Rust code
#[async_trait]
pub trait WorkflowIO: Send + Sync {
    /// Emit an event to the frontend
    ///
    /// This is the primary way the workflow engine communicates status,
    /// progress, and results to the frontend.
    async fn emit(&self, event: WorkflowEvent);

    /// Request input from the user
    ///
    /// Returns the user's input or an error if input could not be obtained.
    async fn request_input(&self, request: InputRequest) -> crate::Result<String>;

    /// Check if the operation has been cancelled
    ///
    /// The workflow engine should check this periodically and abort
    /// if cancelled.
    fn is_cancelled(&self) -> bool;

    /// Request confirmation from the user
    ///
    /// Returns true if confirmed, false if denied.
    async fn confirm(&self, message: &str) -> crate::Result<bool> {
        let response = self
            .request_input(InputRequest {
                id: uuid::Uuid::new_v4().to_string(),
                prompt: message.to_string(),
                field: "confirm".to_string(),
                required: true,
                default: Some("y".to_string()),
                input_type: InputType::Confirm,
                options: None,
            })
            .await?;
        Ok(response.to_lowercase() == "y" || response.to_lowercase() == "yes")
    }
}

/// A no-op IO implementation for testing or headless operation
pub struct NullIO;

#[async_trait]
impl WorkflowIO for NullIO {
    async fn emit(&self, _event: WorkflowEvent) {
        // Discard all events
    }

    async fn request_input(&self, request: InputRequest) -> crate::Result<String> {
        // Return default or empty string
        Ok(request.default.unwrap_or_default())
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

/// IO implementation that logs events
pub struct LoggingIO {
    cancelled: std::sync::atomic::AtomicBool,
}

impl LoggingIO {
    pub fn new() -> Self {
        Self {
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Default for LoggingIO {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkflowIO for LoggingIO {
    async fn emit(&self, event: WorkflowEvent) {
        match &event {
            WorkflowEvent::WorkflowStarted { template_name, .. } => {
                log::info!("Workflow started: {}", template_name);
            }
            WorkflowEvent::WorkflowCompleted { template_name, .. } => {
                log::info!("Workflow completed: {}", template_name);
            }
            WorkflowEvent::StepStarted { step_name, .. } => {
                log::info!("Step started: {}", step_name);
            }
            WorkflowEvent::StepCompleted { step_name, .. } => {
                log::info!("Step completed: {}", step_name);
            }
            WorkflowEvent::Progress { message } => {
                log::debug!("Progress: {}", message);
            }
            WorkflowEvent::Error { message } => {
                log::error!("Error: {}", message);
            }
            WorkflowEvent::Warning { message } => {
                log::warn!("Warning: {}", message);
            }
            _ => {
                log::trace!("Event: {:?}", event);
            }
        }
    }

    async fn request_input(&self, request: InputRequest) -> crate::Result<String> {
        log::warn!(
            "Input requested but LoggingIO cannot provide input: {}",
            request.prompt
        );
        Ok(request.default.unwrap_or_default())
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

