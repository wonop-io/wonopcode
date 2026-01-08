//! Sandboxed execution environment for wonopcode tools.
//!
//! This crate provides isolated execution environments for running AI-generated
//! code safely. It supports multiple backends:
//!
//! - **Docker**: Container-based isolation (Linux, macOS, Windows)
//! - **Podman**: Rootless container alternative to Docker
//! - **Lima**: Lightweight VM-based isolation (macOS)
//! - **Passthrough**: No isolation (for trusted environments)
//!
//! # Example
//!
//! ```rust,no_run
//! use wonopcode_sandbox::{SandboxConfig, SandboxManager, SandboxCapabilities};
//! use std::path::PathBuf;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create sandbox manager
//!     let config = SandboxConfig {
//!         enabled: true,
//!         ..Default::default()
//!     };
//!     let project_root = PathBuf::from("/path/to/project");
//!     let manager = SandboxManager::new(config, project_root).await?;
//!
//!     // Start the sandbox
//!     manager.start().await?;
//!
//!     // Execute a command
//!     let output = manager.execute(
//!         "echo hello",
//!         std::path::Path::new("/workspace"),
//!         Duration::from_secs(30),
//!         &SandboxCapabilities::default(),
//!     ).await?;
//!
//!     println!("Output: {}", output.stdout);
//!
//!     // Stop the sandbox
//!     manager.stop().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod error;
pub mod monitor;
pub mod path;
pub mod runtime;

pub use config::{
    MountConfig, NetworkPolicy, ResourceLimits, SandboxCapabilities, SandboxConfig,
    SandboxRuntimeType, DEFAULT_IMAGE,
};
pub use error::{SandboxError, SandboxResult};
pub use monitor::{ContinuousMonitor, ResourceEvent, ResourceMonitor, ResourceStats};
pub use path::PathMapper;
pub use runtime::SandboxManager;

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Result of command execution in sandbox.
#[derive(Debug, Clone)]
pub struct SandboxOutput {
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: i32,
    /// Whether the command succeeded (exit code 0)
    pub success: bool,
}

impl SandboxOutput {
    /// Create a new successful output.
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            success: true,
        }
    }

    /// Create a new failed output.
    pub fn failure(exit_code: i32, stderr: impl Into<String>) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
            success: false,
        }
    }

    /// Create output from stdout/stderr strings and exit code.
    pub fn from_output(stdout: String, stderr: String, exit_code: i32) -> Self {
        Self {
            stdout,
            stderr,
            exit_code,
            success: exit_code == 0,
        }
    }

    /// Get combined output (stdout + stderr).
    pub fn combined(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// File metadata from sandbox.
#[derive(Debug, Clone)]
pub struct SandboxMetadata {
    /// File size in bytes
    pub size: u64,
    /// Whether this is a directory
    pub is_dir: bool,
    /// Whether this is a file
    pub is_file: bool,
    /// Whether this is a symlink
    pub is_symlink: bool,
    /// File permissions (Unix mode)
    pub mode: Option<u32>,
}

impl SandboxMetadata {
    /// Create metadata for a file.
    pub fn file(size: u64) -> Self {
        Self {
            size,
            is_dir: false,
            is_file: true,
            is_symlink: false,
            mode: None,
        }
    }

    /// Create metadata for a directory.
    pub fn directory() -> Self {
        Self {
            size: 0,
            is_dir: true,
            is_file: false,
            is_symlink: false,
            mode: None,
        }
    }
}

/// Directory entry from sandbox.
#[derive(Debug, Clone)]
pub struct SandboxDirEntry {
    /// Entry name (not full path)
    pub name: String,
    /// Full path in sandbox
    pub path: PathBuf,
    /// Whether this is a directory
    pub is_dir: bool,
}

/// Status of the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxStatus {
    /// Sandbox is not initialized
    NotInitialized,
    /// Sandbox is stopped
    Stopped,
    /// Sandbox is starting
    Starting,
    /// Sandbox is running and ready
    Running,
    /// Sandbox is stopping
    Stopping,
    /// Sandbox encountered an error
    Error,
}

impl SandboxStatus {
    /// Check if the sandbox is ready to execute commands.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the sandbox can be started.
    pub fn can_start(&self) -> bool {
        matches!(self, Self::NotInitialized | Self::Stopped | Self::Error)
    }

    /// Check if the sandbox can be stopped.
    pub fn can_stop(&self) -> bool {
        matches!(self, Self::Running | Self::Error)
    }
}

impl std::fmt::Display for SandboxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "not initialized"),
            Self::Stopped => write!(f, "stopped"),
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Stopping => write!(f, "stopping"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Information about the sandbox.
#[derive(Debug, Clone)]
pub struct SandboxInfo {
    /// Unique identifier
    pub id: String,
    /// Runtime type
    pub runtime_type: SandboxRuntimeType,
    /// Current status
    pub status: SandboxStatus,
    /// Container/VM image
    pub image: String,
    /// Host project root
    pub host_root: PathBuf,
    /// Sandbox workspace path
    pub workspace_path: PathBuf,
}

/// Information about a sandbox snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// Snapshot ID
    pub id: String,
    /// Snapshot name (user-provided)
    pub name: String,
    /// Creation timestamp (Unix seconds)
    pub created_at: i64,
    /// Size in bytes (if known)
    pub size_bytes: Option<u64>,
    /// Description or comments
    pub description: Option<String>,
}

/// Trait for sandbox runtime implementations.
///
/// This trait defines the interface that all sandbox backends must implement.
/// It provides methods for lifecycle management, command execution, and
/// filesystem operations within the isolated environment.
#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    /// Get the unique identifier for this sandbox instance.
    fn id(&self) -> &str;

    /// Get the runtime type.
    fn runtime_type(&self) -> SandboxRuntimeType;

    /// Get the current status of the sandbox.
    async fn status(&self) -> SandboxStatus;

    /// Get information about the sandbox.
    async fn info(&self) -> SandboxInfo;

    /// Check if the sandbox is running and ready.
    async fn is_ready(&self) -> bool {
        self.status().await.is_ready()
    }

    /// Start the sandbox.
    ///
    /// This initializes the container/VM and makes it ready to execute commands.
    /// If the sandbox is already running, this is a no-op.
    async fn start(&self) -> SandboxResult<()>;

    /// Stop the sandbox.
    ///
    /// This terminates the container/VM. Any running processes will be killed.
    async fn stop(&self) -> SandboxResult<()>;

    /// Restart the sandbox.
    ///
    /// Equivalent to calling `stop()` followed by `start()`.
    async fn restart(&self) -> SandboxResult<()> {
        self.stop().await?;
        self.start().await
    }

    /// Execute a shell command in the sandbox.
    ///
    /// # Arguments
    ///
    /// * `command` - The shell command to execute
    /// * `workdir` - Working directory (sandbox path)
    /// * `timeout` - Maximum execution time
    /// * `capabilities` - Requested capabilities for this execution
    ///
    /// # Returns
    ///
    /// The command output including stdout, stderr, and exit code.
    async fn execute(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
        capabilities: &SandboxCapabilities,
    ) -> SandboxResult<SandboxOutput>;

    /// Read a file from the sandbox.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file (sandbox path)
    ///
    /// # Returns
    ///
    /// The file contents as bytes.
    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>>;

    /// Write a file to the sandbox.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file (sandbox path)
    /// * `content` - Content to write
    ///
    /// Creates parent directories if they don't exist.
    async fn write_file(&self, path: &Path, content: &[u8]) -> SandboxResult<()>;

    /// Check if a path exists in the sandbox.
    async fn path_exists(&self, path: &Path) -> SandboxResult<bool>;

    /// Get file metadata in the sandbox.
    async fn metadata(&self, path: &Path) -> SandboxResult<SandboxMetadata>;

    /// List directory contents in the sandbox.
    async fn read_dir(&self, path: &Path) -> SandboxResult<Vec<SandboxDirEntry>>;

    /// Create directories in the sandbox.
    ///
    /// Creates all parent directories as needed.
    async fn create_dir_all(&self, path: &Path) -> SandboxResult<()>;

    /// Remove a file in the sandbox.
    async fn remove_file(&self, path: &Path) -> SandboxResult<()>;

    /// Remove a directory in the sandbox.
    ///
    /// If `recursive` is true, removes all contents.
    async fn remove_dir(&self, path: &Path, recursive: bool) -> SandboxResult<()>;

    /// Get the path mapper for this sandbox.
    fn path_mapper(&self) -> &PathMapper;

    /// Create a snapshot of the current sandbox state.
    ///
    /// Returns a snapshot ID that can be used to restore the state later.
    /// This is useful for creating restore points before risky operations.
    async fn create_snapshot(&self, _name: &str) -> SandboxResult<String> {
        // Default implementation - not all runtimes support snapshotting
        Err(SandboxError::OperationNotSupported(
            "create_snapshot".to_string(),
        ))
    }

    /// Restore sandbox state from a snapshot.
    ///
    /// # Arguments
    ///
    /// * `snapshot_id` - The ID returned from `create_snapshot`
    async fn restore_snapshot(&self, _snapshot_id: &str) -> SandboxResult<()> {
        // Default implementation - not all runtimes support snapshotting
        Err(SandboxError::OperationNotSupported(
            "restore_snapshot".to_string(),
        ))
    }

    /// List available snapshots.
    async fn list_snapshots(&self) -> SandboxResult<Vec<SnapshotInfo>> {
        // Default implementation - not all runtimes support snapshotting
        Ok(Vec::new())
    }

    /// Delete a snapshot.
    async fn delete_snapshot(&self, _snapshot_id: &str) -> SandboxResult<()> {
        // Default implementation - not all runtimes support snapshotting
        Err(SandboxError::OperationNotSupported(
            "delete_snapshot".to_string(),
        ))
    }

    /// Check if snapshotting is supported.
    fn supports_snapshots(&self) -> bool {
        false
    }

    /// Get the workspace path inside the sandbox.
    fn workspace_path(&self) -> &Path {
        self.path_mapper().sandbox_root()
    }

    /// Map a host path to sandbox path.
    fn to_sandbox_path(&self, host_path: &Path) -> Option<PathBuf> {
        self.path_mapper().to_sandbox(host_path)
    }

    /// Map a sandbox path to host path.
    fn to_host_path(&self, sandbox_path: &Path) -> Option<PathBuf> {
        self.path_mapper().to_host(sandbox_path)
    }
}

/// Type alias for a boxed sandbox runtime.
pub type BoxedSandboxRuntime = Box<dyn SandboxRuntime>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_output() {
        let output = SandboxOutput::success("hello");
        assert!(output.success);
        assert_eq!(output.stdout, "hello");
        assert_eq!(output.exit_code, 0);

        let output = SandboxOutput::failure(1, "error");
        assert!(!output.success);
        assert_eq!(output.stderr, "error");
        assert_eq!(output.exit_code, 1);
    }

    #[test]
    fn test_sandbox_status() {
        assert!(SandboxStatus::Running.is_ready());
        assert!(!SandboxStatus::Stopped.is_ready());
        assert!(SandboxStatus::Stopped.can_start());
        assert!(!SandboxStatus::Running.can_start());
    }
}
