//! Podman container-based sandbox runtime.
//!
//! This module provides a sandbox implementation using Podman containers.
//! Podman is a rootless, daemonless alternative to Docker that provides
//! enhanced security through user namespace isolation.
//!
//! Key differences from Docker:
//! - Runs without a daemon (each container is a child process)
//! - Native rootless support (doesn't require root privileges)
//! - Uses fork/exec model instead of client-server
//! - Compatible with Docker images and Dockerfiles
// @ace:implements COMP-T90R7O-674

use crate::{
    config::SandboxConfig,
    error::{SandboxError, SandboxResult},
    path::PathMapper,
    SandboxCapabilities, SandboxDirEntry, SandboxInfo, SandboxMetadata, SandboxOutput,
    SandboxRuntime, SandboxRuntimeType, SandboxStatus,
};
use async_trait::async_trait;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Podman-based sandbox runtime.
pub struct PodmanRuntime {
    /// Unique identifier for this sandbox
    id: String,
    /// Container ID (once created)
    container_id: RwLock<Option<String>>,
    /// Sandbox configuration
    config: SandboxConfig,
    /// Path mapper
    path_mapper: PathMapper,
    /// Current status
    status: RwLock<SandboxStatus>,
}

impl PodmanRuntime {
    /// Create a new Podman runtime.
    pub async fn new(config: SandboxConfig, path_mapper: PathMapper) -> SandboxResult<Self> {
        // Verify Podman is accessible
        let output = Command::new("podman")
            .args(["version", "--format", "{{.Version}}"])
            .output()
            .await
            .map_err(|e| SandboxError::connection_failed(format!("Podman not available: {e}")))?;

        if !output.status.success() {
            return Err(SandboxError::connection_failed(
                "Podman version check failed".to_string(),
            ));
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Generate a deterministic container ID based on the project path.
        // This ensures the same project always uses the same container.
        let project_path = path_mapper.host_root().to_string_lossy();
        let hash = Self::hash_path(&project_path);
        let id = format!("wonopcode-{hash}");

        info!(id = %id, version = %version, project = %project_path, "Podman runtime created");

        Ok(Self {
            id,
            container_id: RwLock::new(None),
            config,
            path_mapper,
            status: RwLock::new(SandboxStatus::NotInitialized),
        })
    }

    /// Generate a short hash from a path for deterministic container naming.
    fn hash_path(path: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let hash = hasher.finish();
        format!("{hash:016x}")[..12].to_string()
    }

    /// Run a podman command and return output.
    async fn podman(&self, args: &[&str]) -> SandboxResult<std::process::Output> {
        debug!(args = ?args, "Running podman command");
        Command::new("podman")
            .args(args)
            .output()
            .await
            .map_err(|e| SandboxError::ExecFailed(format!("Podman command failed: {e}")))
    }

    /// Ensure the container image is available.
    async fn ensure_image(&self) -> SandboxResult<()> {
        let image = self.config.image();

        // Check if image exists locally
        let output = self.podman(&["image", "exists", image]).await?;
        if output.status.success() {
            debug!(image = %image, "Image already exists locally");
            return Ok(());
        }

        info!(image = %image, "Pulling image...");
        let output = self.podman(&["pull", image]).await?;

        if output.status.success() {
            info!(image = %image, "Image pulled successfully");
            Ok(())
        } else {
            Err(SandboxError::image_pull_failed(
                image,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    /// Build podman run arguments.
    fn build_run_args(&self) -> Vec<String> {
        let image = self.config.image().to_string();
        let host_root = self.path_mapper.host_root().to_string_lossy().to_string();
        let sandbox_root = self
            .path_mapper
            .sandbox_root()
            .to_string_lossy()
            .to_string();

        let mut args = vec![
            "run".to_string(),
            "-d".to_string(), // Detached mode
            "--name".to_string(),
            self.id.clone(),
            "--label".to_string(),
            "wonopcode=true".to_string(),
            "--label".to_string(),
            format!("wonopcode.sandbox.id={}", self.id),
        ];

        // Working directory
        args.push("-w".to_string());
        args.push(sandbox_root.clone());

        // Volume mounts
        let mount_opt = if self.config.mounts.workspace_writable {
            "rw"
        } else {
            "ro"
        };
        args.push("-v".to_string());
        args.push(format!("{host_root}:{sandbox_root}:{mount_opt}"));

        // Cache mounts (named volumes)
        if self.config.mounts.persist_caches {
            args.push("-v".to_string());
            args.push(format!("wonopcode-pip-cache-{}:/root/.cache/pip", self.id));
            args.push("-v".to_string());
            args.push(format!("wonopcode-npm-cache-{}:/root/.npm", self.id));
            args.push("-v".to_string());
            args.push(format!(
                "wonopcode-cargo-cache-{}:/root/.cargo/registry",
                self.id
            ));
        }

        // Resource limits
        if let Some(mem_bytes) = self.config.resources.memory_bytes() {
            args.push("--memory".to_string());
            args.push(format!("{mem_bytes}b"));
        }
        args.push("--cpus".to_string());
        args.push(format!("{}", self.config.resources.cpus));
        args.push("--pids-limit".to_string());
        args.push(format!("{}", self.config.resources.pids));

        // Network mode
        if let Some(network_mode) = self.config.network.docker_network_mode() {
            args.push("--network".to_string());
            args.push(network_mode);
        }

        // Read-only rootfs
        if self.config.resources.readonly_rootfs {
            args.push("--read-only".to_string());
        }

        // Environment variables
        args.push("-e".to_string());
        args.push("TERM=dumb".to_string());
        args.push("-e".to_string());
        args.push("NO_COLOR=1".to_string());
        args.push("-e".to_string());
        args.push("GIT_TERMINAL_PROMPT=0".to_string());
        args.push("-e".to_string());
        args.push("DEBIAN_FRONTEND=noninteractive".to_string());

        // Image and command
        args.push(image);
        args.push("sleep".to_string());
        args.push("infinity".to_string());

        args
    }

    /// Check if the container is running.
    async fn is_container_running(&self) -> bool {
        let container_id = self.container_id.read().await;
        if let Some(id) = container_id.as_ref() {
            let output = self
                .podman(&["inspect", "--format", "{{.State.Running}}", id])
                .await;

            match output {
                Ok(out) => String::from_utf8_lossy(&out.stdout).trim() == "true",
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Find existing container for this sandbox.
    async fn find_existing_container(&self) -> Option<String> {
        let filter = format!("label=wonopcode.sandbox.id={}", self.id);
        let output = self
            .podman(&["ps", "-a", "--filter", &filter, "--format", "{{.ID}}"])
            .await
            .ok()?;

        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    }

    /// Execute a command in the container.
    async fn exec_command(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
    ) -> SandboxResult<SandboxOutput> {
        let container_id = self.container_id.read().await;
        let container_id = container_id.as_ref().ok_or(SandboxError::NotRunning)?;

        let workdir_str = workdir.to_string_lossy().to_string();

        let mut cmd = Command::new("podman");
        cmd.args([
            "exec",
            "-w",
            &workdir_str,
            container_id,
            "sh",
            "-c",
            command,
        ]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let result = tokio::time::timeout(timeout, async {
            let mut child = cmd
                .spawn()
                .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            if let Some(mut stdout_handle) = child.stdout.take() {
                stdout_handle
                    .read_to_end(&mut stdout)
                    .await
                    .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;
            }

            if let Some(mut stderr_handle) = child.stderr.take() {
                stderr_handle
                    .read_to_end(&mut stderr)
                    .await
                    .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;
            }

            let status = child
                .wait()
                .await
                .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

            let exit_code = status.code().unwrap_or(-1);

            Ok(SandboxOutput {
                stdout: String::from_utf8_lossy(&stdout).to_string(),
                stderr: String::from_utf8_lossy(&stderr).to_string(),
                exit_code,
                success: exit_code == 0,
            })
        })
        .await;

        match result {
            Ok(output) => output,
            Err(_) => Err(SandboxError::Timeout(timeout)),
        }
    }
}

#[async_trait]
impl SandboxRuntime for PodmanRuntime {
    fn id(&self) -> &str {
        &self.id
    }

    fn runtime_type(&self) -> SandboxRuntimeType {
        SandboxRuntimeType::Podman
    }

    async fn status(&self) -> SandboxStatus {
        *self.status.read().await
    }

    async fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            runtime_type: SandboxRuntimeType::Podman,
            status: self.status().await,
            image: self.config.image().to_string(),
            host_root: self.path_mapper.host_root().to_path_buf(),
            workspace_path: self.path_mapper.sandbox_root().to_path_buf(),
        }
    }

    async fn is_ready(&self) -> bool {
        self.is_container_running().await
    }

    async fn start(&self) -> SandboxResult<()> {
        // Check if already running
        if self.is_container_running().await {
            debug!("Container already running");
            return Ok(());
        }

        *self.status.write().await = SandboxStatus::Starting;

        // Ensure image is available
        self.ensure_image().await?;

        // Check for existing container
        if let Some(existing_id) = self.find_existing_container().await {
            debug!(container_id = %existing_id, "Found existing container");
            *self.container_id.write().await = Some(existing_id.clone());

            // Try to start it
            let output = self.podman(&["start", &existing_id]).await?;
            if !output.status.success() {
                return Err(SandboxError::StartFailed(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }

            *self.status.write().await = SandboxStatus::Running;
            return Ok(());
        }

        // Create new container
        let run_args = self.build_run_args();
        let args: Vec<&str> = run_args.iter().map(|s| s.as_str()).collect();
        let output = self.podman(&args).await?;

        if !output.status.success() {
            return Err(SandboxError::CreateFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        info!(container_id = %container_id, "Container created and started");
        *self.container_id.write().await = Some(container_id);
        *self.status.write().await = SandboxStatus::Running;

        Ok(())
    }

    async fn stop(&self) -> SandboxResult<()> {
        let container_id = self.container_id.read().await.clone();

        if let Some(id) = container_id {
            *self.status.write().await = SandboxStatus::Stopping;

            // Stop container
            let _ = self.podman(&["stop", "-t", "10", &id]).await;

            // Remove container if not keeping alive
            if !self.config.keep_alive {
                let _ = self.podman(&["rm", "-f", &id]).await;
                *self.container_id.write().await = None;
            }

            *self.status.write().await = SandboxStatus::Stopped;
            info!(container_id = %id, "Container stopped");
        }

        Ok(())
    }

    async fn execute(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
        _capabilities: &SandboxCapabilities,
    ) -> SandboxResult<SandboxOutput> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        debug!(command = %command, workdir = %workdir.display(), "Executing command");
        self.exec_command(command, workdir, timeout).await
    }

    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = format!("cat {}", shell_escape(&path.to_string_lossy()));
        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(30))
            .await?;

        if output.success {
            Ok(output.stdout.into_bytes())
        } else {
            Err(SandboxError::read_failed(path, output.stderr))
        }
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> SandboxResult<()> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        // Create parent directory
        if let Some(parent) = path.parent() {
            self.create_dir_all(parent).await?;
        }

        // Use base64 for binary-safe writing
        let content_b64 = base64_encode(content);
        let command = format!(
            "echo '{}' | base64 -d > {}",
            content_b64,
            shell_escape(&path.to_string_lossy())
        );

        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(30))
            .await?;

        if output.success {
            Ok(())
        } else {
            Err(SandboxError::write_failed(path, output.stderr))
        }
    }

    async fn path_exists(&self, path: &Path) -> SandboxResult<bool> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = format!("test -e {}", shell_escape(&path.to_string_lossy()));
        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(10))
            .await?;

        Ok(output.success)
    }

    async fn metadata(&self, path: &Path) -> SandboxResult<SandboxMetadata> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = format!(
            "stat -c '%s %F %a' {} 2>/dev/null || stat -f '%z %HT %Lp' {}",
            shell_escape(&path.to_string_lossy()),
            shell_escape(&path.to_string_lossy())
        );
        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(10))
            .await?;

        if !output.success {
            return Err(SandboxError::FileNotFound(path.to_path_buf()));
        }

        // Parse output: "size type mode"
        let parts: Vec<&str> = output.stdout.split_whitespace().collect();
        if parts.len() >= 2 {
            let size = parts[0].parse().unwrap_or(0);
            let file_type = parts[1].to_lowercase();
            let mode = parts.get(2).and_then(|m| u32::from_str_radix(m, 8).ok());

            Ok(SandboxMetadata {
                size,
                is_dir: file_type.contains("directory"),
                is_file: file_type.contains("regular") || file_type.contains("file"),
                is_symlink: file_type.contains("link"),
                mode,
            })
        } else {
            Err(SandboxError::read_failed(path, "Failed to parse metadata"))
        }
    }

    async fn read_dir(&self, path: &Path) -> SandboxResult<Vec<SandboxDirEntry>> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = format!(
            "find {} -maxdepth 1 -mindepth 1 -printf '%f\\t%y\\n' 2>/dev/null || ls -1F {}",
            shell_escape(&path.to_string_lossy()),
            shell_escape(&path.to_string_lossy())
        );
        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(30))
            .await?;

        if !output.success {
            return Err(SandboxError::read_failed(path, output.stderr));
        }

        let entries = output
            .stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                let (name, is_dir) = if parts.len() == 2 {
                    (parts[0].to_string(), parts[1] == "d")
                } else {
                    let name = line.trim_end_matches('/').to_string();
                    let is_dir = line.ends_with('/');
                    (name, is_dir)
                };

                SandboxDirEntry {
                    name: name.clone(),
                    path: path.join(&name),
                    is_dir,
                }
            })
            .collect();

        Ok(entries)
    }

    async fn create_dir_all(&self, path: &Path) -> SandboxResult<()> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = format!("mkdir -p {}", shell_escape(&path.to_string_lossy()));
        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(10))
            .await?;

        if output.success {
            Ok(())
        } else {
            Err(SandboxError::write_failed(path, output.stderr))
        }
    }

    async fn remove_file(&self, path: &Path) -> SandboxResult<()> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = format!("rm -f {}", shell_escape(&path.to_string_lossy()));
        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(10))
            .await?;

        if output.success {
            Ok(())
        } else {
            Err(SandboxError::write_failed(path, output.stderr))
        }
    }

    async fn remove_dir(&self, path: &Path, recursive: bool) -> SandboxResult<()> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let command = if recursive {
            format!("rm -rf {}", shell_escape(&path.to_string_lossy()))
        } else {
            format!("rmdir {}", shell_escape(&path.to_string_lossy()))
        };

        let output = self
            .exec_command(&command, Path::new("/"), Duration::from_secs(30))
            .await?;

        if output.success {
            Ok(())
        } else {
            Err(SandboxError::write_failed(path, output.stderr))
        }
    }

    fn path_mapper(&self) -> &PathMapper {
        &self.path_mapper
    }
}

impl Drop for PodmanRuntime {
    fn drop(&mut self) {
        debug!(id = %self.id, "Podman runtime dropped");
    }
}

/// Escape a string for use in shell commands.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Base64 encode bytes.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;

        let n = (b0 << 16) | (b1 << 8) | b2;

        result.push(ALPHABET[(n >> 18) as usize & 0x3F] as char);
        result.push(ALPHABET[(n >> 12) as usize & 0x3F] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[(n >> 6) as usize & 0x3F] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[n as usize & 0x3F] as char);
        } else {
            result.push('=');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("hello"), "'hello'");
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(base64_encode(b""), "");
    }
}