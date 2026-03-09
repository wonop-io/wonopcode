//! Workflow execution engine for wonopcode
//!
//! This crate provides a platform-agnostic workflow execution engine that can be
//! used with different frontends:
//!
//! - Terminal/CLI (proto binary)
//! - Tauri desktop app
//! - HTTP/WebSocket API
//! - Programmatic Rust integration
//!
//! # Architecture
//!
//! The core abstraction is the [`WorkflowIO`] trait, which defines how the workflow
//! engine communicates with the outside world. This allows the same engine to work
//! with different IO implementations.
//!
//! # Example
//!
//! ```rust,ignore
//! use wonopcode_workflow::{WorkflowEngine, EngineConfig, LoggingIO};
//! use wonop_templates::TemplateManager;
//!
//! // Create the engine
//! let config = EngineConfig::new(PathBuf::from("templates"))
//!     .with_provider("anthropic")
//!     .with_model("claude-sonnet-4-5");
//!
//! let template_manager = TemplateManager::new(config.templates_dir.clone())?;
//! let provider = ProviderClient::new(&config).await?;
//! let engine = WorkflowEngine::new(template_manager, provider, config);
//!
//! // Execute with logging IO
//! let io = LoggingIO::new();
//! let result = engine.execute(&template, inputs, &output_dir, &io).await?;
//! ```

pub mod config;
pub mod conversation;
pub mod engine;
pub mod error;
pub mod io;
pub mod provider;

// Re-export main types
pub use config::{EngineConfig, WorkflowConfig};
pub use conversation::{ConversationEngine, ConversationResult};
pub use engine::{CommandOutput, StepTiming, WorkflowEngine, WorkflowMetrics, WorkflowResult};
pub use error::{Result, WorkflowError};
pub use io::{InputRequest, InputType, LoggingIO, NullIO, WorkflowEvent, WorkflowIO};
pub use provider::{CollectedResponse, CollectedToolCall, ProviderClient};

