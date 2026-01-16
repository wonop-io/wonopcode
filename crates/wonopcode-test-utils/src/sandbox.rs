//! Sandbox test utilities.
//!
//! Provides mock sandbox implementations for testing without requiring
//! actual container runtimes like Docker or Podman.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wonopcode_sandbox::{
    PathMapper, SandboxCapabilities, SandboxDirEntry, SandboxError, SandboxInfo, SandboxMetadata,
    SandboxOutput, SandboxResult, SandboxRuntime, SandboxRuntimeType, SandboxStatus, SnapshotInfo,
};

/// A mock sandbox runtime for testing.
///
/// This implementation uses an in-memory filesystem and records all
/// command executions without actually running them. Useful for:
///
/// - Unit testing code that uses sandbox functionality
/// - Integration tests that don't need actual container isolation
/// - Fast CI/CD pipelines without Docker dependencies
///
/// # Example
///
/// ```rust,ignore
/// use wonopcode_test_utils::sandbox::MockSandbox;
/// use std::time::Duration;
///
/// #[tokio::test]
/// async fn test_sandbox_execution() {
///     let sandbox = MockSandbox::new("/project")
///         .with_command_response("echo hello", SandboxOutput::success("hello\n"))
///         .with_file("/project/test.txt", "file content");
///
///     // Execute a command
///     let output = sandbox.execute(
///         "echo hello",
///         std::path::Path::new("/project"),
///         Duration::from_secs(10),
///         &SandboxCapabilities::default(),
///     ).await.unwrap();
///
///     assert_eq!(output.stdout.trim(), "hello");
///     assert!(sandbox.command_was_executed("echo hello"));
/// }
/// ```
pub struct MockSandbox {
    id: String,
    path_mapper: PathMapper,
    status: Arc<Mutex<SandboxStatus>>,
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    directories: Arc<Mutex<Vec<PathBuf>>>,
    command_responses: Arc<Mutex<HashMap<String, SandboxOutput>>>,
    default_command_response: Arc<Mutex<SandboxOutput>>,
    executed_commands: Arc<Mutex<Vec<ExecutedCommand>>>,
    snapshots: Arc<Mutex<HashMap<String, SnapshotData>>>,
}

/// A recorded command execution.
#[derive(Debug, Clone)]
pub struct ExecutedCommand {
    /// The command that was executed.
    pub command: String,
    /// The working directory.
    pub workdir: PathBuf,
    /// The timeout that was specified.
    pub timeout: Duration,
    /// The capabilities that were requested.
    pub capabilities: SandboxCapabilities,
}

/// Snapshot data for mock snapshots.
#[derive(Debug, Clone)]
struct SnapshotData {
    info: SnapshotInfo,
    files: HashMap<PathBuf, Vec<u8>>,
    directories: Vec<PathBuf>,
}

impl MockSandbox {
    /// Create a new mock sandbox with the given project root.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        let id = format!("mock-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let path_mapper = PathMapper::new(project_root.clone(), project_root);

        Self {
            id,
            path_mapper,
            status: Arc::new(Mutex::new(SandboxStatus::Stopped)),
            files: Arc::new(Mutex::new(HashMap::new())),
            directories: Arc::new(Mutex::new(Vec::new())),
            command_responses: Arc::new(Mutex::new(HashMap::new())),
            default_command_response: Arc::new(Mutex::new(SandboxOutput::success(""))),
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a mock sandbox with a temporary directory.
    pub fn with_temp_dir() -> (Self, tempfile::TempDir) {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let sandbox = Self::new(temp_dir.path());
        (sandbox, temp_dir)
    }

    /// Configure a response for a specific command.
    pub fn with_command_response(self, command: &str, output: SandboxOutput) -> Self {
        self.command_responses
            .lock()
            .unwrap()
            .insert(command.to_string(), output);
        self
    }

    /// Configure the default response for unmatched commands.
    pub fn with_default_command_response(self, output: SandboxOutput) -> Self {
        *self.default_command_response.lock().unwrap() = output;
        self
    }

    /// Add a file to the mock filesystem.
    pub fn with_file(self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Self {
        self.files
            .lock()
            .unwrap()
            .insert(path.as_ref().to_path_buf(), content.as_ref().to_vec());
        self
    }

    /// Add a text file to the mock filesystem.
    pub fn with_text_file(self, path: impl AsRef<Path>, content: impl Into<String>) -> Self {
        self.with_file(path, content.into().into_bytes())
    }

    /// Add a directory to the mock filesystem.
    pub fn with_directory(self, path: impl AsRef<Path>) -> Self {
        self.directories
            .lock()
            .unwrap()
            .push(path.as_ref().to_path_buf());
        self
    }

    /// Set the initial status of the sandbox.
    pub fn with_status(self, status: SandboxStatus) -> Self {
        *self.status.lock().unwrap() = status;
        self
    }

    /// Get all executed commands.
    pub fn executed_commands(&self) -> Vec<ExecutedCommand> {
        self.executed_commands.lock().unwrap().clone()
    }

    /// Check if a specific command was executed.
    pub fn command_was_executed(&self, command: &str) -> bool {
        self.executed_commands
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.command.contains(command))
    }

    /// Get the number of commands executed.
    pub fn command_count(&self) -> usize {
        self.executed_commands.lock().unwrap().len()
    }

    /// Clear executed commands.
    pub fn clear_executed_commands(&self) {
        self.executed_commands.lock().unwrap().clear();
    }

    /// Get all files in the mock filesystem.
    pub fn all_files(&self) -> HashMap<PathBuf, Vec<u8>> {
        self.files.lock().unwrap().clone()
    }

    /// Check if a file was written during testing.
    pub fn file_was_written(&self, path: impl AsRef<Path>) -> bool {
        self.files.lock().unwrap().contains_key(path.as_ref())
    }

    /// Get the content of a file in the mock filesystem.
    pub fn get_file_content(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(path.as_ref()).cloned()
    }

    /// Get the content of a text file in the mock filesystem.
    pub fn get_text_file_content(&self, path: impl AsRef<Path>) -> Option<String> {
        self.get_file_content(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
    }
}

#[async_trait]
impl SandboxRuntime for MockSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    fn runtime_type(&self) -> SandboxRuntimeType {
        SandboxRuntimeType::None
    }

    async fn status(&self) -> SandboxStatus {
        *self.status.lock().unwrap()
    }

    async fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            runtime_type: SandboxRuntimeType::None,
            status: *self.status.lock().unwrap(),
            image: "mock".to_string(),
            host_root: self.path_mapper.host_root().to_path_buf(),
            workspace_path: self.path_mapper.sandbox_root().to_path_buf(),
        }
    }

    async fn is_ready(&self) -> bool {
        self.status().await.is_ready()
    }

    async fn start(&self) -> SandboxResult<()> {
        *self.status.lock().unwrap() = SandboxStatus::Running;
        Ok(())
    }

    async fn stop(&self) -> SandboxResult<()> {
        *self.status.lock().unwrap() = SandboxStatus::Stopped;
        Ok(())
    }

    async fn execute(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
        capabilities: &SandboxCapabilities,
    ) -> SandboxResult<SandboxOutput> {
        // Record the execution
        self.executed_commands
            .lock()
            .unwrap()
            .push(ExecutedCommand {
                command: command.to_string(),
                workdir: workdir.to_path_buf(),
                timeout,
                capabilities: capabilities.clone(),
            });

        // Find a matching response
        let responses = self.command_responses.lock().unwrap();

        // Try exact match first
        if let Some(output) = responses.get(command) {
            return Ok(output.clone());
        }

        // Try prefix match
        for (cmd, output) in responses.iter() {
            if command.starts_with(cmd) {
                return Ok(output.clone());
            }
        }

        drop(responses);

        // Return default response
        Ok(self.default_command_response.lock().unwrap().clone())
    }

    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| SandboxError::FileNotFound(path.to_path_buf()))
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> SandboxResult<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.to_vec());
        Ok(())
    }

    async fn path_exists(&self, path: &Path) -> SandboxResult<bool> {
        let files = self.files.lock().unwrap();
        let dirs = self.directories.lock().unwrap();

        Ok(files.contains_key(path) || dirs.contains(&path.to_path_buf()))
    }

    async fn metadata(&self, path: &Path) -> SandboxResult<SandboxMetadata> {
        let files = self.files.lock().unwrap();
        let dirs = self.directories.lock().unwrap();

        if let Some(content) = files.get(path) {
            Ok(SandboxMetadata {
                size: content.len() as u64,
                is_dir: false,
                is_file: true,
                is_symlink: false,
                mode: Some(0o644),
            })
        } else if dirs.contains(&path.to_path_buf()) {
            Ok(SandboxMetadata {
                size: 0,
                is_dir: true,
                is_file: false,
                is_symlink: false,
                mode: Some(0o755),
            })
        } else {
            Err(SandboxError::FileNotFound(path.to_path_buf()))
        }
    }

    async fn read_dir(&self, path: &Path) -> SandboxResult<Vec<SandboxDirEntry>> {
        let files = self.files.lock().unwrap();
        let dirs = self.directories.lock().unwrap();

        let mut entries = Vec::new();

        // Find files in this directory
        for file_path in files.keys() {
            if file_path.parent() == Some(path) {
                if let Some(name) = file_path.file_name() {
                    entries.push(SandboxDirEntry {
                        name: name.to_string_lossy().to_string(),
                        path: file_path.clone(),
                        is_dir: false,
                    });
                }
            }
        }

        // Find subdirectories in this directory
        for dir_path in dirs.iter() {
            if dir_path.parent() == Some(path) {
                if let Some(name) = dir_path.file_name() {
                    entries.push(SandboxDirEntry {
                        name: name.to_string_lossy().to_string(),
                        path: dir_path.clone(),
                        is_dir: true,
                    });
                }
            }
        }

        Ok(entries)
    }

    async fn create_dir_all(&self, path: &Path) -> SandboxResult<()> {
        let mut dirs = self.directories.lock().unwrap();

        // Add all parent directories
        let mut current = path.to_path_buf();
        while !current.as_os_str().is_empty() {
            if !dirs.contains(&current) {
                dirs.push(current.clone());
            }
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> SandboxResult<()> {
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .ok_or_else(|| SandboxError::FileNotFound(path.to_path_buf()))?;
        Ok(())
    }

    async fn remove_dir(&self, path: &Path, recursive: bool) -> SandboxResult<()> {
        let mut files = self.files.lock().unwrap();
        let mut dirs = self.directories.lock().unwrap();

        if recursive {
            // Remove all files and directories under this path
            files.retain(|p, _| !p.starts_with(path));
            dirs.retain(|p| !p.starts_with(path));
        } else {
            // Only remove if empty
            let has_children = files.keys().any(|p| p.parent() == Some(path))
                || dirs.iter().any(|p| p.parent() == Some(path));

            if has_children {
                return Err(SandboxError::ExecFailed("Directory not empty".to_string()));
            }

            dirs.retain(|p| p != path);
        }

        Ok(())
    }

    fn path_mapper(&self) -> &PathMapper {
        &self.path_mapper
    }

    fn supports_snapshots(&self) -> bool {
        true
    }

    async fn create_snapshot(&self, name: &str) -> SandboxResult<String> {
        let snapshot_id = format!("snap-{}", &uuid::Uuid::new_v4().to_string()[..8]);

        let files = self.files.lock().unwrap().clone();
        let directories = self.directories.lock().unwrap().clone();

        let snapshot = SnapshotData {
            info: SnapshotInfo {
                id: snapshot_id.clone(),
                name: name.to_string(),
                created_at: chrono::Utc::now().timestamp(),
                size_bytes: Some(files.values().map(|v| v.len() as u64).sum()),
                description: None,
            },
            files,
            directories,
        };

        self.snapshots
            .lock()
            .unwrap()
            .insert(snapshot_id.clone(), snapshot);

        Ok(snapshot_id)
    }

    async fn restore_snapshot(&self, snapshot_id: &str) -> SandboxResult<()> {
        let snapshots = self.snapshots.lock().unwrap();

        let snapshot = snapshots
            .get(snapshot_id)
            .ok_or_else(|| SandboxError::SnapshotNotFound(snapshot_id.to_string()))?;

        *self.files.lock().unwrap() = snapshot.files.clone();
        *self.directories.lock().unwrap() = snapshot.directories.clone();

        Ok(())
    }

    async fn list_snapshots(&self) -> SandboxResult<Vec<SnapshotInfo>> {
        Ok(self
            .snapshots
            .lock()
            .unwrap()
            .values()
            .map(|s| s.info.clone())
            .collect())
    }

    async fn delete_snapshot(&self, snapshot_id: &str) -> SandboxResult<()> {
        self.snapshots
            .lock()
            .unwrap()
            .remove(snapshot_id)
            .ok_or_else(|| SandboxError::SnapshotNotFound(snapshot_id.to_string()))?;
        Ok(())
    }
}

/// Builder for creating test scenarios with MockSandbox.
///
/// Provides a fluent API for setting up common test scenarios.
///
/// # Example
///
/// ```rust,ignore
/// use wonopcode_test_utils::sandbox::SandboxTestScenario;
///
/// let scenario = SandboxTestScenario::rust_project()
///     .with_test_file("tests/integration.rs", "fn test() {}")
///     .build();
///
/// // Use scenario.sandbox() for testing
/// ```
pub struct SandboxTestScenario {
    sandbox: MockSandbox,
}

impl SandboxTestScenario {
    /// Create a new test scenario with an empty sandbox.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            sandbox: MockSandbox::new(project_root).with_status(SandboxStatus::Running),
        }
    }

    /// Create a scenario for a Rust project.
    pub fn rust_project() -> Self {
        let project_root = PathBuf::from("/project");
        Self::new(&project_root)
            .with_file(
                project_root.join("Cargo.toml"),
                r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#,
            )
            .with_file(project_root.join("src/main.rs"), "fn main() {}\n")
            .with_directory(project_root.join("src"))
            .with_directory(project_root.join("target"))
    }

    /// Create a scenario for a Node.js project.
    pub fn nodejs_project() -> Self {
        let project_root = PathBuf::from("/project");
        Self::new(&project_root)
            .with_file(
                project_root.join("package.json"),
                r#"{"name": "test-project", "version": "1.0.0"}"#,
            )
            .with_file(project_root.join("index.js"), "console.log('hello');\n")
            .with_directory(project_root.join("node_modules"))
    }

    /// Create a scenario for a Python project.
    pub fn python_project() -> Self {
        let project_root = PathBuf::from("/project");
        Self::new(&project_root)
            .with_file(
                project_root.join("requirements.txt"),
                "pytest>=7.0\nrequests>=2.28\n",
            )
            .with_file(project_root.join("main.py"), "print('hello')\n")
            .with_directory(project_root.join("venv"))
    }

    /// Add a file to the scenario.
    pub fn with_file(mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Self {
        self.sandbox = self.sandbox.with_text_file(path, content);
        self
    }

    /// Add a directory to the scenario.
    pub fn with_directory(mut self, path: impl AsRef<Path>) -> Self {
        self.sandbox = self.sandbox.with_directory(path);
        self
    }

    /// Configure a command response.
    pub fn with_command_response(mut self, command: &str, output: SandboxOutput) -> Self {
        self.sandbox = self.sandbox.with_command_response(command, output);
        self
    }

    /// Configure a successful command response.
    pub fn with_successful_command(self, command: &str, stdout: &str) -> Self {
        self.with_command_response(command, SandboxOutput::success(stdout))
    }

    /// Configure a failing command response.
    pub fn with_failing_command(self, command: &str, exit_code: i32, stderr: &str) -> Self {
        self.with_command_response(command, SandboxOutput::failure(exit_code, stderr))
    }

    /// Build the scenario and return the sandbox.
    pub fn build(self) -> MockSandbox {
        self.sandbox
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_sandbox_execute() {
        let sandbox = MockSandbox::new("/project")
            .with_status(SandboxStatus::Running)
            .with_command_response("echo hello", SandboxOutput::success("hello\n"));

        let output = sandbox
            .execute(
                "echo hello",
                Path::new("/project"),
                Duration::from_secs(10),
                &SandboxCapabilities::default(),
            )
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.stdout.trim(), "hello");
        assert!(sandbox.command_was_executed("echo hello"));
        assert_eq!(sandbox.command_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_sandbox_files() {
        let sandbox =
            MockSandbox::new("/project").with_text_file("/project/test.txt", "file content");

        // Read file
        let content = sandbox
            .read_file(Path::new("/project/test.txt"))
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&content), "file content");

        // Write file
        sandbox
            .write_file(Path::new("/project/new.txt"), b"new content")
            .await
            .unwrap();
        assert!(sandbox.file_was_written("/project/new.txt"));

        // Check exists
        assert!(sandbox
            .path_exists(Path::new("/project/test.txt"))
            .await
            .unwrap());
        assert!(sandbox
            .path_exists(Path::new("/project/new.txt"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_mock_sandbox_directories() {
        let sandbox = MockSandbox::new("/project")
            .with_directory("/project/src")
            .with_text_file("/project/src/main.rs", "fn main() {}");

        // Check directory exists
        assert!(sandbox
            .path_exists(Path::new("/project/src"))
            .await
            .unwrap());

        // List directory
        let entries = sandbox.read_dir(Path::new("/project/src")).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "main.rs");

        // Get metadata
        let meta = sandbox.metadata(Path::new("/project/src")).await.unwrap();
        assert!(meta.is_dir);
    }

    #[tokio::test]
    async fn test_mock_sandbox_snapshots() {
        let sandbox = MockSandbox::new("/project").with_text_file("/project/file.txt", "original");

        // Create snapshot
        let snap_id = sandbox.create_snapshot("test").await.unwrap();

        // Modify file
        sandbox
            .write_file(Path::new("/project/file.txt"), b"modified")
            .await
            .unwrap();

        // Verify modification
        let content = sandbox
            .read_file(Path::new("/project/file.txt"))
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&content), "modified");

        // Restore snapshot
        sandbox.restore_snapshot(&snap_id).await.unwrap();

        // Verify restoration
        let content = sandbox
            .read_file(Path::new("/project/file.txt"))
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&content), "original");
    }

    #[tokio::test]
    async fn test_mock_sandbox_lifecycle() {
        let sandbox = MockSandbox::new("/project");

        assert_eq!(sandbox.status().await, SandboxStatus::Stopped);

        sandbox.start().await.unwrap();
        assert_eq!(sandbox.status().await, SandboxStatus::Running);
        assert!(sandbox.is_ready().await);

        sandbox.stop().await.unwrap();
        assert_eq!(sandbox.status().await, SandboxStatus::Stopped);
    }

    #[tokio::test]
    async fn test_sandbox_test_scenario_rust() {
        let sandbox = SandboxTestScenario::rust_project()
            .with_successful_command("cargo build", "Compiling test-project v0.1.0\n")
            .build();

        // Verify project files exist
        assert!(sandbox
            .path_exists(Path::new("/project/Cargo.toml"))
            .await
            .unwrap());
        assert!(sandbox
            .path_exists(Path::new("/project/src/main.rs"))
            .await
            .unwrap());

        // Execute build command
        let output = sandbox
            .execute(
                "cargo build",
                Path::new("/project"),
                Duration::from_secs(60),
                &SandboxCapabilities::default(),
            )
            .await
            .unwrap();

        assert!(output.success);
        assert!(output.stdout.contains("Compiling"));
    }

    #[tokio::test]
    async fn test_sandbox_test_scenario_nodejs() {
        let sandbox = SandboxTestScenario::nodejs_project()
            .with_successful_command("npm test", "All tests passed\n")
            .build();

        assert!(sandbox
            .path_exists(Path::new("/project/package.json"))
            .await
            .unwrap());
        assert!(sandbox
            .path_exists(Path::new("/project/index.js"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_sandbox_test_scenario_python() {
        let sandbox = SandboxTestScenario::python_project()
            .with_successful_command("pytest", "2 passed in 0.1s\n")
            .build();

        assert!(sandbox
            .path_exists(Path::new("/project/requirements.txt"))
            .await
            .unwrap());
        assert!(sandbox
            .path_exists(Path::new("/project/main.py"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_mock_sandbox_default_response() {
        let sandbox = MockSandbox::new("/project")
            .with_default_command_response(SandboxOutput::success("default output\n"));

        let output = sandbox
            .execute(
                "any command",
                Path::new("/project"),
                Duration::from_secs(10),
                &SandboxCapabilities::default(),
            )
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.stdout.trim(), "default output");
    }

    #[tokio::test]
    async fn test_mock_sandbox_failing_command() {
        let sandbox = MockSandbox::new("/project")
            .with_command_response("bad command", SandboxOutput::failure(1, "command failed\n"));

        let output = sandbox
            .execute(
                "bad command",
                Path::new("/project"),
                Duration::from_secs(10),
                &SandboxCapabilities::default(),
            )
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.exit_code, 1);
        assert!(output.stderr.contains("failed"));
    }
}
