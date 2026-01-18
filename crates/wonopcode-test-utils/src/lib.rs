//! Testing utilities, fixtures, and mocks for wonopcode.
//!
//! This crate provides common testing infrastructure used across the wonopcode workspace:
//!
//! - **Fixtures**: Pre-built test data and project structures
//! - **Mocks**: Mock implementations for isolated testing
//! - **Assertions**: Custom assertion helpers for common test patterns
//! - **Builders**: Builder patterns for constructing test objects
//! - **Providers**: Test provider implementations for AI model testing
//! - **Sandbox**: Mock sandbox for testing without containers
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use wonopcode_test_utils::{
//!     fixtures::TestProject,
//!     mocks::MockCommandExecutor,
//!     providers::RecordingProvider,
//!     sandbox::MockSandbox,
//! };
//!
//! #[tokio::test]
//! async fn test_file_operations() {
//!     let project = TestProject::new()
//!         .with_file("src/main.rs", "fn main() {}")
//!         .with_file("Cargo.toml", "[package]\nname = \"test\"")
//!         .build();
//!
//!     // Use project.path() for test operations
//!     assert!(project.path().join("src/main.rs").exists());
//! }
//! ```

pub mod assertions;
pub mod builders;
pub mod fixtures;
pub mod mocks;
pub mod providers;
pub mod sandbox;

// Re-export commonly used items
pub use fixtures::TestProject;
pub use mocks::MockCommandExecutor;
pub use providers::RecordingProvider;
pub use sandbox::{MockSandbox, SandboxTestScenario};
