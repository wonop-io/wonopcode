//! ACE (Agentic Code Engine) framework integration.
//!
//! This module provides ticket-level artifact management based on ACE principles:
//!
//! - **Simplified hierarchy**: UC → REQ → DES/TC/TASK (no PROD/EPIC/FEAT)
//! - **Ticket-scoped artifacts**: IDs include ticket ID for traceability
//! - **Workflow state machine**: Requirements → Analysis → Design → Implementation → Verification → Deployment
//! - **Configurable storage**: Specs directory location via `.wonopcode/config.yaml`
//!
//! ## Directory Structure
//!
//! ```text
//! .wonopcode/
//!   config.yaml     # Repository configuration (committed)
//!   state.yaml      # Workstream state (not committed)
//! specs/            # Configurable via ace.specs_dir
//!   use-cases/
//!   requirements/
//!   designs/
//!   tests/
//!   tasks/
//!   workspace/staging/  # Draft artifacts before approval
//! ```

pub mod config;
pub mod state;
pub mod store;
pub mod tools;
pub mod types;

// Re-export commonly used types
pub use config::{AceConfig, WonopCodeConfig};
pub use state::WorkstreamState;
pub use store::ArtifactStore;
pub use types::{Artifact, ArtifactMetadata, ArtifactType, PhaseStatus, Priority, Progress, WorkflowPhase};

// Re-export tools for registration
pub use tools::{
    AceCreateArtifactTool, AceReadArtifactTool, AceSubmitCheckpointTool, AceTodoReadTool,
    AceTodoUpdateTool, AceTodoWriteTool, AceWhatNowTool,
};
