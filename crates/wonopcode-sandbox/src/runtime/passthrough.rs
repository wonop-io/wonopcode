//! Passthrough runtime - no sandboxing.
//!
//! This runtime executes commands directly on the host system without any
//! isolation. It's used when sandboxing is disabled or no sandbox runtime
//! is available.

use crate::{
    error::{SandboxError, SandboxResult},
    path::PathMapper,
    SandboxCapabilities, SandboxDirEntry, SandboxInfo, SandboxMetadata, SandboxOutput,
    SandboxRuntime, SandboxRuntimeType, SandboxStatus,
};
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;
use tracing::debug;

/// Passthrough runtime that executes commands directly on the host.
///
/// This provides the same interface as other sandbox runtimes but without
/// any isolation. Useful for:
/// - Development/debugging
/// - Trusted environments
/// - When no sandbox runtime is available
pub struct PassthroughRuntime {
    /// Unique identifier
    id: String,
    /// Path mapper (identity mapping in passthrough mode)
    path_mapper: PathMapper,
}

impl PassthroughRuntime {
    /// Create a new passthrough runtime.
    pub fn new(path_mapper: PathMapper) -> Self {
        let id = format!("passthrough-{}", &uuid::Uuid::new_v4().to_string()[..8]);

        debug!(id = %id, "Passthrough runtime created");

        Self { id, path_mapper }
    }

    /// Create a passthrough runtime with default path mapping.
    pub fn with_root(project_root: PathBuf) -> Self {
        // In passthrough mode, paths map to themselves
        let path_mapper = PathMapper::new(project_root.clone(), project_root);
        Self::new(path_mapper)
    }
}

#[async_trait]
impl SandboxRuntime for PassthroughRuntime {
    fn id(&self) -> &str {
        &self.id
    }

    fn runtime_type(&self) -> SandboxRuntimeType {
        SandboxRuntimeType::None
    }

    async fn status(&self) -> SandboxStatus {
        // Passthrough is always "running"
        SandboxStatus::Running
    }

    async fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            runtime_type: SandboxRuntimeType::None,
            status: SandboxStatus::Running,
            image: "passthrough".to_string(),
            host_root: self.path_mapper.host_root().to_path_buf(),
            workspace_path: self.path_mapper.sandbox_root().to_path_buf(),
        }
    }

    async fn is_ready(&self) -> bool {
        true
    }

    async fn start(&self) -> SandboxResult<()> {
        // No-op for passthrough
        Ok(())
    }

    async fn stop(&self) -> SandboxResult<()> {
        // No-op for passthrough
        Ok(())
    }

    async fn execute(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
        _capabilities: &SandboxCapabilities,
    ) -> SandboxResult<SandboxOutput> {
        debug!(command = %command, workdir = %workdir.display(), "Executing command (passthrough)");

        let result = tokio::time::timeout(timeout, async {
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(workdir)
                .env("TERM", "dumb")
                .env("NO_COLOR", "1")
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .await
                .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

            Ok(SandboxOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code().unwrap_or(-1),
                success: output.status.success(),
            })
        })
        .await;

        match result {
            Ok(output) => output,
            Err(_) => Err(SandboxError::Timeout(timeout)),
        }
    }

    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>> {
        tokio::fs::read(path)
            .await
            .map_err(|e| SandboxError::read_failed(path, e.to_string()))
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> SandboxResult<()> {
        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SandboxError::write_failed(path, e.to_string()))?;
        }

        tokio::fs::write(path, content)
            .await
            .map_err(|e| SandboxError::write_failed(path, e.to_string()))
    }

    async fn path_exists(&self, path: &Path) -> SandboxResult<bool> {
        Ok(tokio::fs::try_exists(path).await.unwrap_or(false))
    }

    async fn metadata(&self, path: &Path) -> SandboxResult<SandboxMetadata> {
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|_| SandboxError::FileNotFound(path.to_path_buf()))?;

        Ok(SandboxMetadata {
            size: meta.len(),
            is_dir: meta.is_dir(),
            is_file: meta.is_file(),
            is_symlink: meta.is_symlink(),
            #[cfg(unix)]
            mode: {
                use std::os::unix::fs::PermissionsExt;
                Some(meta.permissions().mode())
            },
            #[cfg(not(unix))]
            mode: None,
        })
    }

    async fn read_dir(&self, path: &Path) -> SandboxResult<Vec<SandboxDirEntry>> {
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| SandboxError::read_failed(path, e.to_string()))?;

        while let Some(entry) = dir.next_entry().await.transpose() {
            let entry = entry.map_err(|e| SandboxError::read_failed(path, e.to_string()))?;
            let file_type = entry.file_type().await.ok();

            entries.push(SandboxDirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path(),
                is_dir: file_type.map(|t| t.is_dir()).unwrap_or(false),
            });
        }

        Ok(entries)
    }

    async fn create_dir_all(&self, path: &Path) -> SandboxResult<()> {
        tokio::fs::create_dir_all(path)
            .await
            .map_err(|e| SandboxError::write_failed(path, e.to_string()))
    }

    async fn remove_file(&self, path: &Path) -> SandboxResult<()> {
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| SandboxError::write_failed(path, e.to_string()))
    }

    async fn remove_dir(&self, path: &Path, recursive: bool) -> SandboxResult<()> {
        if recursive {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|e| SandboxError::write_failed(path, e.to_string()))
        } else {
            tokio::fs::remove_dir(path)
                .await
                .map_err(|e| SandboxError::write_failed(path, e.to_string()))
        }
    }

    fn path_mapper(&self) -> &PathMapper {
        &self.path_mapper
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_passthrough_execute() {
        let temp_dir = TempDir::new().unwrap();
        let runtime = PassthroughRuntime::with_root(temp_dir.path().to_path_buf());

        let output = runtime
            .execute(
                "echo hello",
                temp_dir.path(),
                Duration::from_secs(10),
                &SandboxCapabilities::default(),
            )
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_passthrough_read_write() {
        let temp_dir = TempDir::new().unwrap();
        let runtime = PassthroughRuntime::with_root(temp_dir.path().to_path_buf());

        let file_path = temp_dir.path().join("test.txt");
        let content = b"hello world";

        // Write file
        runtime.write_file(&file_path, content).await.unwrap();

        // Read file
        let read_content = runtime.read_file(&file_path).await.unwrap();
        assert_eq!(read_content, content);

        // Check exists
        assert!(runtime.path_exists(&file_path).await.unwrap());

        // Get metadata
        let meta = runtime.metadata(&file_path).await.unwrap();
        assert!(meta.is_file);
        assert_eq!(meta.size, content.len() as u64);
    }

    #[tokio::test]
    async fn test_passthrough_directory_operations() {
        let temp_dir = TempDir::new().unwrap();
        let runtime = PassthroughRuntime::with_root(temp_dir.path().to_path_buf());

        let dir_path = temp_dir.path().join("test_dir/nested");

        // Create directories
        runtime.create_dir_all(&dir_path).await.unwrap();
        assert!(runtime.path_exists(&dir_path).await.unwrap());

        // Create some files
        runtime
            .write_file(&dir_path.join("file1.txt"), b"content1")
            .await
            .unwrap();
        runtime
            .write_file(&dir_path.join("file2.txt"), b"content2")
            .await
            .unwrap();

        // List directory
        let entries = runtime.read_dir(&dir_path).await.unwrap();
        assert_eq!(entries.len(), 2);

        // Remove directory recursively
        runtime
            .remove_dir(&temp_dir.path().join("test_dir"), true)
            .await
            .unwrap();
        assert!(!runtime
            .path_exists(&temp_dir.path().join("test_dir"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_passthrough_timeout() {
        let temp_dir = TempDir::new().unwrap();
        let runtime = PassthroughRuntime::with_root(temp_dir.path().to_path_buf());

        let result = runtime
            .execute(
                "sleep 10",
                temp_dir.path(),
                Duration::from_millis(100),
                &SandboxCapabilities::default(),
            )
            .await;

        assert!(matches!(result, Err(SandboxError::Timeout(_))));
    }
}
