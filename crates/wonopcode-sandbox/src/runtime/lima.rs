//! Lima VM-based sandbox runtime (macOS only).
//!
//! This module provides a sandbox implementation using Lima VMs on macOS.
//! Lima provides lightweight virtual machines with automatic file sharing
//! and better isolation than Docker on macOS.
//!
//! ## Features
//!
//! - Native ARM64 support (Apple Silicon)
//! - Rosetta 2 emulation for x86_64 binaries
//! - Automatic file sharing via virtiofs/9p
//! - Better isolation than Docker Desktop
//!
//! ## Requirements
//!
//! - macOS 10.15+ (Catalina or later)
//! - Lima installed: `brew install lima`
//! - QEMU (installed automatically with Lima)

use crate::{
    config::SandboxConfig,
    error::{SandboxError, SandboxResult},
    path::PathMapper,
    SandboxCapabilities, SandboxDirEntry, SandboxInfo, SandboxMetadata, SandboxOutput,
    SandboxRuntime, SandboxRuntimeType, SandboxStatus,
};
use async_trait::async_trait;
use std::{path::Path, time::Duration};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Lima VM name prefix for wonopcode sandboxes.
const VM_NAME_PREFIX: &str = "wonopcode-sandbox";

/// Default Lima template path (relative to crate).
const DEFAULT_LIMA_TEMPLATE: &str = include_str!("../image/lima.yaml");

/// Lima VM-based sandbox runtime.
///
/// Lima provides lightweight VMs on macOS with automatic file sharing.
/// This is often preferred over Docker Desktop for better isolation
/// and native ARM64 support on Apple Silicon.
pub struct LimaRuntime {
    /// Unique identifier for this sandbox
    id: String,
    /// Lima VM name
    vm_name: String,
    /// Sandbox configuration
    config: SandboxConfig,
    /// Path mapper
    path_mapper: PathMapper,
    /// Current status
    status: RwLock<SandboxStatus>,
    /// Cached VM running state
    vm_running: RwLock<bool>,
}

impl LimaRuntime {
    /// Create a new Lima runtime.
    ///
    /// This will check if Lima is installed and accessible.
    pub async fn new(config: SandboxConfig, path_mapper: PathMapper) -> SandboxResult<Self> {
        // Verify Lima is installed
        if !is_limactl_available().await {
            return Err(SandboxError::RuntimeNotAvailable(
                "Lima (limactl) is not installed. Install with: brew install lima".to_string(),
            ));
        }

        let id = format!("wonopcode-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let vm_name = format!("{}-{}", VM_NAME_PREFIX, &id[10..]);

        info!(id = %id, vm_name = %vm_name, "Lima runtime created");

        Ok(Self {
            id,
            vm_name,
            config,
            path_mapper,
            status: RwLock::new(SandboxStatus::NotInitialized),
            vm_running: RwLock::new(false),
        })
    }

    /// Run a limactl command.
    async fn limactl(&self, args: &[&str]) -> SandboxResult<std::process::Output> {
        debug!(args = ?args, "Running limactl");

        Command::new("limactl")
            .args(args)
            .output()
            .await
            .map_err(|e| SandboxError::ExecFailed(format!("limactl failed: {}", e)))
    }

    /// Check if the VM exists.
    async fn vm_exists(&self) -> bool {
        let output = match self.limactl(&["list", "--format", "{{.Name}}"]).await {
            Ok(o) => o,
            Err(_) => return false,
        };

        if !output.status.success() {
            return false;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.lines().any(|line| line.trim() == self.vm_name)
    }

    /// Check if the VM is running.
    async fn check_vm_running(&self) -> bool {
        let output = match self
            .limactl(&["list", "--format", "{{.Name}}:{{.Status}}"])
            .await
        {
            Ok(o) => o,
            Err(_) => return false,
        };

        if !output.status.success() {
            return false;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .any(|line| line.starts_with(&self.vm_name) && line.contains("Running"))
    }

    /// Create the Lima VM.
    async fn create_vm(&self) -> SandboxResult<()> {
        info!(vm_name = %self.vm_name, "Creating Lima VM");

        // Write Lima template to a temp file
        let template_path = self.write_lima_template().await?;

        // Create the VM
        let output = self
            .limactl(&["create", "--name", &self.vm_name, &template_path])
            .await?;

        // Clean up template file
        let _ = tokio::fs::remove_file(&template_path).await;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SandboxError::CreateFailed(format!(
                "Failed to create Lima VM: {}",
                stderr
            )));
        }

        info!(vm_name = %self.vm_name, "Lima VM created");
        Ok(())
    }

    /// Write the Lima template YAML to a temporary file.
    async fn write_lima_template(&self) -> SandboxResult<String> {
        let temp_dir = std::env::temp_dir();
        let template_path = temp_dir.join(format!("{}.yaml", self.vm_name));

        // Generate template with configuration
        let template = self.generate_lima_template();

        tokio::fs::write(&template_path, template)
            .await
            .map_err(|e| SandboxError::CreateFailed(format!("Failed to write template: {}", e)))?;

        Ok(template_path.to_string_lossy().to_string())
    }

    /// Generate Lima YAML template with current configuration.
    fn generate_lima_template(&self) -> String {
        let host_root = self.path_mapper.host_root().to_string_lossy();
        let sandbox_root = self.path_mapper.sandbox_root().to_string_lossy();
        let memory = &self.config.resources.memory;
        let cpus = self.config.resources.cpus as u32;

        // Parse the default template and customize it
        let mut template = DEFAULT_LIMA_TEMPLATE.to_string();

        // Replace placeholders
        template = template.replace("{{HOST_ROOT}}", &host_root);
        template = template.replace("{{SANDBOX_ROOT}}", &sandbox_root);
        template = template.replace("{{MEMORY}}", memory);
        template = template.replace("{{CPUS}}", &cpus.to_string());
        template = template.replace(
            "{{WRITABLE}}",
            if self.config.mounts.workspace_writable {
                "true"
            } else {
                "false"
            },
        );

        template
    }

    /// Execute a command in the Lima VM.
    async fn exec_in_vm(
        &self,
        command: &str,
        workdir: &Path,
        timeout: Duration,
    ) -> SandboxResult<SandboxOutput> {
        let workdir_str = workdir.to_string_lossy();

        // Build the full command with cd
        let full_command = format!("cd '{}' && {}", workdir_str, command);

        debug!(command = %command, workdir = %workdir_str, "Executing in Lima VM");

        let result = tokio::time::timeout(timeout, async {
            let output = Command::new("limactl")
                .args(["shell", &self.vm_name, "sh", "-c", &full_command])
                .output()
                .await
                .map_err(|e| SandboxError::ExecFailed(e.to_string()))?;

            Ok::<_, SandboxError>(SandboxOutput {
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
}

#[async_trait]
impl SandboxRuntime for LimaRuntime {
    fn id(&self) -> &str {
        &self.id
    }

    fn runtime_type(&self) -> SandboxRuntimeType {
        SandboxRuntimeType::Lima
    }

    async fn status(&self) -> SandboxStatus {
        *self.status.read().await
    }

    async fn info(&self) -> SandboxInfo {
        SandboxInfo {
            id: self.id.clone(),
            runtime_type: SandboxRuntimeType::Lima,
            status: self.status().await,
            image: format!("lima:{}", self.vm_name),
            host_root: self.path_mapper.host_root().to_path_buf(),
            workspace_path: self.path_mapper.sandbox_root().to_path_buf(),
        }
    }

    async fn is_ready(&self) -> bool {
        *self.vm_running.read().await && self.check_vm_running().await
    }

    async fn start(&self) -> SandboxResult<()> {
        // Check if already running
        if self.check_vm_running().await {
            debug!(vm_name = %self.vm_name, "Lima VM already running");
            *self.status.write().await = SandboxStatus::Running;
            *self.vm_running.write().await = true;
            return Ok(());
        }

        *self.status.write().await = SandboxStatus::Starting;

        // Create VM if it doesn't exist
        if !self.vm_exists().await {
            self.create_vm().await?;
        }

        // Start the VM
        info!(vm_name = %self.vm_name, "Starting Lima VM");

        let output = self.limactl(&["start", &self.vm_name]).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            *self.status.write().await = SandboxStatus::Error;
            return Err(SandboxError::StartFailed(format!(
                "Failed to start Lima VM: {}",
                stderr
            )));
        }

        // Wait for VM to be ready
        let mut attempts = 0;
        while attempts < 30 {
            if self.check_vm_running().await {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            attempts += 1;
        }

        if attempts >= 30 {
            *self.status.write().await = SandboxStatus::Error;
            return Err(SandboxError::StartFailed(
                "Lima VM did not become ready in time".to_string(),
            ));
        }

        *self.status.write().await = SandboxStatus::Running;
        *self.vm_running.write().await = true;

        info!(vm_name = %self.vm_name, "Lima VM started");
        Ok(())
    }

    async fn stop(&self) -> SandboxResult<()> {
        if !self.check_vm_running().await {
            debug!(vm_name = %self.vm_name, "Lima VM not running");
            return Ok(());
        }

        *self.status.write().await = SandboxStatus::Stopping;

        info!(vm_name = %self.vm_name, "Stopping Lima VM");

        let output = self.limactl(&["stop", &self.vm_name]).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(vm_name = %self.vm_name, error = %stderr, "Failed to stop Lima VM cleanly");
        }

        // Optionally delete the VM if not keeping alive
        if !self.config.keep_alive {
            let output = self.limactl(&["delete", "--force", &self.vm_name]).await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(vm_name = %self.vm_name, error = %stderr, "Failed to delete Lima VM");
            }
        }

        *self.status.write().await = SandboxStatus::Stopped;
        *self.vm_running.write().await = false;

        info!(vm_name = %self.vm_name, "Lima VM stopped");
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

        self.exec_in_vm(command, workdir, timeout).await
    }

    async fn read_file(&self, path: &Path) -> SandboxResult<Vec<u8>> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        // Use cat to read the file
        let command = format!("cat '{}'", path.to_string_lossy().replace('\'', "'\\''"));
        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(30))
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
        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        let command = format!("echo '{}' | base64 -d > '{}'", content_b64, path_escaped);

        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(30))
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

        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        let command = format!("test -e '{}'", path_escaped);
        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(10))
            .await?;

        Ok(output.success)
    }

    async fn metadata(&self, path: &Path) -> SandboxResult<SandboxMetadata> {
        if !self.is_ready().await {
            return Err(SandboxError::NotRunning);
        }

        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        // Use stat with format that works on both Linux and macOS
        let command = format!(
            "stat -c '%s %F %a' '{}' 2>/dev/null || stat -f '%z %HT %Lp' '{}'",
            path_escaped, path_escaped
        );

        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(10))
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

        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        let command = format!(
            "find '{}' -maxdepth 1 -mindepth 1 -printf '%f\\t%y\\n' 2>/dev/null || ls -1F '{}'",
            path_escaped, path_escaped
        );

        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(30))
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

        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        let command = format!("mkdir -p '{}'", path_escaped);
        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(10))
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

        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        let command = format!("rm -f '{}'", path_escaped);
        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(10))
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

        let path_escaped = path.to_string_lossy().replace('\'', "'\\''");
        let command = if recursive {
            format!("rm -rf '{}'", path_escaped)
        } else {
            format!("rmdir '{}'", path_escaped)
        };

        let output = self
            .exec_in_vm(&command, Path::new("/"), Duration::from_secs(30))
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

impl Drop for LimaRuntime {
    fn drop(&mut self) {
        debug!(id = %self.id, vm_name = %self.vm_name, "Lima runtime dropped");
    }
}

/// Check if limactl is available.
async fn is_limactl_available() -> bool {
    match Command::new("limactl").arg("--version").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Base64 encode bytes (same as docker.rs).
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
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(base64_encode(b""), "");
    }

    #[tokio::test]
    async fn test_lima_availability_check() {
        // This test just verifies the function runs without panicking
        let _available = is_limactl_available().await;
        // We don't assert the result because Lima may or may not be installed
    }
}
