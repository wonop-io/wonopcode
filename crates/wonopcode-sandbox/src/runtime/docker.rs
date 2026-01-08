//! Docker container-based sandbox runtime.
//!
//! This module provides a sandbox implementation using Docker containers.
//! It supports both Docker and Podman (via Docker-compatible API).

use crate::{
    config::SandboxConfig,
    error::{SandboxError, SandboxResult},
    path::PathMapper,
    SandboxCapabilities, SandboxDirEntry, SandboxInfo, SandboxMetadata, SandboxOutput,
    SandboxRuntime, SandboxRuntimeType, SandboxStatus,
};
use async_trait::async_trait;
use bollard::{
    container::{
        Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
        StartContainerOptions, StopContainerOptions,
    },
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
    image::CreateImageOptions,
    models::HostConfig,
    Docker,
};
use futures::StreamExt;
use std::{collections::HashMap, path::Path, time::Duration};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Docker-based sandbox runtime.
pub struct DockerRuntime {
    /// Unique identifier for this sandbox
    id: String,
    /// Docker client
    docker: Docker,
    /// Container ID (once created)
    container_id: RwLock<Option<String>>,
    /// Sandbox configuration
    config: SandboxConfig,
    /// Path mapper
    path_mapper: PathMapper,
    /// Current status
    status: RwLock<SandboxStatus>,
}

impl DockerRuntime {
    /// Create a new Docker runtime.
    pub async fn new(config: SandboxConfig, path_mapper: PathMapper) -> SandboxResult<Self> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| SandboxError::connection_failed(e.to_string()))?;

        // Verify Docker is accessible
        docker
            .ping()
            .await
            .map_err(|e| SandboxError::connection_failed(format!("Docker ping failed: {}", e)))?;

        // Generate a deterministic container ID based on the project path.
        // This ensures the same project always uses the same container.
        let project_path = path_mapper.host_root().to_string_lossy();
        let hash = Self::hash_path(&project_path);
        let id = format!("wonopcode-{}", hash);

        info!(id = %id, project = %project_path, "Docker runtime created");

        Ok(Self {
            id,
            docker,
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
        format!("{:016x}", hash)[..12].to_string()
    }

    /// Cleanup orphaned wonopcode containers that are stopped.
    /// Only removes stopped containers to avoid disrupting other running agents.
    /// Running containers from other projects are left alone - they may be in use.
    pub async fn cleanup_orphaned_containers(&self) -> SandboxResult<()> {
        let filters: HashMap<String, Vec<String>> = HashMap::from([
            ("label".to_string(), vec!["wonopcode=true".to_string()]),
            // Only target stopped/exited containers
            (
                "status".to_string(),
                vec![
                    "exited".to_string(),
                    "dead".to_string(),
                    "created".to_string(),
                ],
            ),
        ]);

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        match self.docker.list_containers(Some(options)).await {
            Ok(containers) => {
                for container in containers {
                    if let Some(ref container_id) = container.id {
                        // Skip our own container (even if stopped, we might restart it)
                        if container.names.as_ref().is_some_and(|names| {
                            names.iter().any(|n| n.trim_start_matches('/') == self.id)
                        }) {
                            continue;
                        }

                        // Get container name for logging
                        let name = container
                            .names
                            .as_ref()
                            .and_then(|n| n.first())
                            .map(|n| n.trim_start_matches('/').to_string())
                            .unwrap_or_else(|| container_id.clone());

                        info!(container = %name, "Cleaning up stopped wonopcode container");

                        // Remove the container (it's already stopped)
                        let remove_options = RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        };
                        if let Err(e) = self
                            .docker
                            .remove_container(container_id, Some(remove_options))
                            .await
                        {
                            warn!(container = %name, error = %e, "Failed to remove orphaned container");
                        } else {
                            info!(container = %name, "Removed orphaned container");
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to list containers for cleanup");
            }
        }

        Ok(())
    }

    /// Ensure the container image is available.
    async fn ensure_image(&self) -> SandboxResult<()> {
        let image = self.config.image();

        // Check if image exists locally
        match self.docker.inspect_image(image).await {
            Ok(_) => {
                debug!(image = %image, "Image already exists locally");
                return Ok(());
            }
            Err(_) => {
                info!(image = %image, "Pulling image...");
            }
        }

        // Pull the image
        let options = CreateImageOptions {
            from_image: image,
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        debug!(status = %status, "Image pull progress");
                    }
                }
                Err(e) => {
                    return Err(SandboxError::image_pull_failed(image, e.to_string()));
                }
            }
        }

        info!(image = %image, "Image pulled successfully");
        Ok(())
    }

    /// Build the container configuration.
    fn container_config(&self) -> Config<String> {
        let image = self.config.image().to_string();
        let host_root = self.path_mapper.host_root().to_string_lossy().to_string();
        let sandbox_root = self
            .path_mapper
            .sandbox_root()
            .to_string_lossy()
            .to_string();

        // Build volume bindings
        let mut binds = vec![format!(
            "{}:{}:{}",
            host_root,
            sandbox_root,
            if self.config.mounts.workspace_writable {
                "rw"
            } else {
                "ro"
            }
        )];

        // Add cache mounts if enabled
        if self.config.mounts.persist_caches {
            binds.push(format!("wonopcode-pip-cache-{}:/root/.cache/pip", self.id));
            binds.push(format!("wonopcode-npm-cache-{}:/root/.npm", self.id));
            binds.push(format!(
                "wonopcode-cargo-cache-{}:/root/.cargo/registry",
                self.id
            ));
        }

        // Add readonly mounts
        for (host_path, container_path) in &self.config.mounts.readonly {
            binds.push(format!("{}:{}:ro", host_path, container_path));
        }

        let host_config = HostConfig {
            binds: Some(binds),
            memory: self.config.resources.memory_bytes().map(|m| m as i64),
            nano_cpus: Some(self.config.resources.cpu_nano()),
            pids_limit: Some(self.config.resources.pids as i64),
            network_mode: self.config.network.docker_network_mode(),
            auto_remove: Some(false),
            readonly_rootfs: Some(self.config.resources.readonly_rootfs),
            ..Default::default()
        };

        // Environment variables
        let env = vec![
            "TERM=dumb".to_string(),
            "NO_COLOR=1".to_string(),
            "GIT_TERMINAL_PROMPT=0".to_string(),
            "DEBIAN_FRONTEND=noninteractive".to_string(),
        ];

        Config {
            image: Some(image),
            cmd: Some(vec!["sleep".to_string(), "infinity".to_string()]),
            working_dir: Some(sandbox_root),
            host_config: Some(host_config),
            env: Some(env),
            tty: Some(false),
            attach_stdin: Some(false),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            labels: Some(HashMap::from([
                ("wonopcode".to_string(), "true".to_string()),
                ("wonopcode.sandbox.id".to_string(), self.id.clone()),
            ])),
            ..Default::default()
        }
    }

    /// Check if the container is running.
    async fn is_container_running(&self) -> bool {
        let container_id = self.container_id.read().await;
        if let Some(id) = container_id.as_ref() {
            match self.docker.inspect_container(id, None).await {
                Ok(info) => info.state.and_then(|s| s.running).unwrap_or(false),
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Find existing container for this sandbox.
    async fn find_existing_container(&self) -> Option<String> {
        let filters: HashMap<String, Vec<String>> = HashMap::from([(
            "label".to_string(),
            vec![format!("wonopcode.sandbox.id={}", self.id)],
        )]);

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        match self.docker.list_containers(Some(options)).await {
            Ok(containers) => containers.first().and_then(|c| c.id.clone()),
            Err(_) => None,
        }
    }

    /// Execute a command and return output.
    async fn exec_command(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
    ) -> SandboxResult<SandboxOutput> {
        let container_id = self.container_id.read().await;
        let container_id = container_id.as_ref().ok_or(SandboxError::NotRunning)?;

        let workdir_str = workdir.to_string_lossy().to_string();
        let exec_config = CreateExecOptions::<String> {
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                command.to_string(),
            ]),
            working_dir: Some(workdir_str),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(false),
            ..Default::default()
        };

        let exec = self
            .docker
            .create_exec(container_id, exec_config)
            .await
            .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

        let start_config = StartExecOptions {
            detach: false,
            ..Default::default()
        };

        let result = tokio::time::timeout(timeout, async {
            let start_result = self
                .docker
                .start_exec(&exec.id, Some(start_config))
                .await
                .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

            match start_result {
                StartExecResults::Attached { mut output, .. } => {
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();

                    while let Some(chunk) = output.next().await {
                        match chunk {
                            Ok(bollard::container::LogOutput::StdOut { message }) => {
                                stdout.extend_from_slice(&message);
                            }
                            Ok(bollard::container::LogOutput::StdErr { message }) => {
                                stderr.extend_from_slice(&message);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                warn!(error = %e, "Error reading exec output");
                            }
                        }
                    }

                    // Get exit code
                    let inspect = self
                        .docker
                        .inspect_exec(&exec.id)
                        .await
                        .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

                    let exit_code = inspect.exit_code.unwrap_or(-1) as i32;

                    Ok(SandboxOutput {
                        stdout: String::from_utf8_lossy(&stdout).to_string(),
                        stderr: String::from_utf8_lossy(&stderr).to_string(),
                        exit_code,
                        success: exit_code == 0,
                    })
                }
                StartExecResults::Detached => Err(SandboxError::ExecFailed(
                    "Unexpected detached exec".to_string(),
                )),
            }
        })
        .await;

        match result {
            Ok(output) => output,
            Err(_) => Err(SandboxError::Timeout(timeout)),
        }
    }
}

#[async_trait]
impl SandboxRuntime for DockerRuntime {
    fn id(&self) -> &str {
        &self.id
    }

    fn runtime_type(&self) -> SandboxRuntimeType {
        SandboxRuntimeType::Docker
    }

    async fn status(&self) -> SandboxStatus {
        *self.status.read().await
    }

    async fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            runtime_type: SandboxRuntimeType::Docker,
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

        // Clean up any orphaned containers from previous runs
        // This prevents container accumulation over time
        if let Err(e) = self.cleanup_orphaned_containers().await {
            warn!(error = %e, "Failed to cleanup orphaned containers");
        }

        // Ensure image is available
        self.ensure_image().await?;

        // Check for existing container
        if let Some(existing_id) = self.find_existing_container().await {
            debug!(container_id = %existing_id, "Found existing container");
            *self.container_id.write().await = Some(existing_id.clone());

            // Try to start it
            self.docker
                .start_container(&existing_id, None::<StartContainerOptions<String>>)
                .await
                .map_err(|e| SandboxError::StartFailed(e.to_string()))?;

            *self.status.write().await = SandboxStatus::Running;
            return Ok(());
        }

        // Create new container
        let options = CreateContainerOptions {
            name: &self.id,
            platform: None,
        };

        let config = self.container_config();

        let container = self
            .docker
            .create_container(Some(options), config)
            .await
            .map_err(|e| SandboxError::CreateFailed(e.to_string()))?;

        info!(container_id = %container.id, "Container created");
        *self.container_id.write().await = Some(container.id.clone());

        // Start container
        self.docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| SandboxError::StartFailed(e.to_string()))?;

        *self.status.write().await = SandboxStatus::Running;
        info!(container_id = %container.id, "Container started");

        Ok(())
    }

    async fn stop(&self) -> SandboxResult<()> {
        let container_id = self.container_id.read().await.clone();

        if let Some(id) = container_id {
            *self.status.write().await = SandboxStatus::Stopping;

            // Stop container with a short grace period (2 seconds)
            // This provides quick feedback while still allowing graceful shutdown
            let options = StopContainerOptions { t: 2 };
            if let Err(e) = self.docker.stop_container(&id, Some(options)).await {
                warn!(error = %e, "Error stopping container");
            }

            // Remove container if not keeping alive
            if !self.config.keep_alive {
                let options = RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                };
                if let Err(e) = self.docker.remove_container(&id, Some(options)).await {
                    warn!(error = %e, "Error removing container");
                }
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

        // Use cat to read the file
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

        // For binary-safe writing, use base64
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
                    // Fallback: check for trailing /
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

impl Drop for DockerRuntime {
    fn drop(&mut self) {
        // Note: Async cleanup would need to be handled by the manager
        debug!(id = %self.id, "Docker runtime dropped");
    }
}

/// Escape a string for use in shell commands.
fn shell_escape(s: &str) -> String {
    // Use single quotes and escape any single quotes in the string
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
