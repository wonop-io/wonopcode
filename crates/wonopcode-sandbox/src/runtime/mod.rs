//! Sandbox runtime implementations.
//!
//! This module provides different backend implementations for the sandbox:
//!
//! - `docker`: Docker container-based sandbox
//! - `podman`: Podman container-based sandbox (rootless alternative to Docker)
//! - `lima`: Lima VM-based sandbox (macOS only)
//! - `passthrough`: No-op implementation for non-sandboxed execution

pub mod docker;
#[cfg(target_os = "macos")]
pub mod lima;
pub mod passthrough;
pub mod podman;

use crate::{PathMapper, SandboxConfig, SandboxResult, SandboxRuntime, SandboxRuntimeType};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

pub use docker::DockerRuntime;
#[cfg(target_os = "macos")]
pub use lima::LimaRuntime;
pub use passthrough::PassthroughRuntime;
pub use podman::PodmanRuntime;

/// Manager for sandbox lifecycle and runtime selection.
///
/// The `SandboxManager` handles:
/// - Auto-detection of available runtimes
/// - Creating and managing sandbox instances
/// - Providing a unified interface regardless of backend
///
/// # Example
///
/// ```rust,no_run
/// use wonopcode_sandbox::{SandboxConfig, SandboxManager};
/// use std::path::PathBuf;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = SandboxConfig::default();
///     let project_root = PathBuf::from("/path/to/project");
///     
///     let manager = SandboxManager::new(config, project_root).await?;
///     
///     if manager.is_available() {
///         manager.start().await?;
///         // ... use sandbox
///         manager.stop().await?;
///     }
///     
///     Ok(())
/// }
/// ```
pub struct SandboxManager {
    /// The sandbox configuration
    config: SandboxConfig,
    /// Project root directory on the host
    project_root: PathBuf,
    /// The active runtime (lazily initialized)
    runtime: RwLock<Option<Arc<dyn SandboxRuntime>>>,
    /// Detected runtime type (lazily detected if Auto)
    detected_runtime: RwLock<Option<SandboxRuntimeType>>,
    /// Whether the sandbox was explicitly stopped by the user.
    /// When true, auto-start should be disabled.
    explicitly_stopped: RwLock<bool>,
}

impl SandboxManager {
    /// Create a new sandbox manager.
    ///
    /// This will auto-detect available runtimes if `config.runtime` is `Auto`.
    pub async fn new(config: SandboxConfig, project_root: PathBuf) -> SandboxResult<Self> {
        let detected_runtime = if config.runtime == SandboxRuntimeType::Auto {
            detect_runtime().await
        } else {
            config.runtime.clone()
        };

        info!(
            runtime = ?detected_runtime,
            enabled = config.enabled,
            "Sandbox manager initialized"
        );

        Ok(Self {
            config,
            project_root,
            runtime: RwLock::new(None),
            detected_runtime: RwLock::new(Some(detected_runtime)),
            // Start with explicitly_stopped = true so sandbox doesn't auto-start.
            // User must explicitly call /sandbox start to enable isolation.
            explicitly_stopped: RwLock::new(true),
        })
    }

    /// Create a new sandbox manager with lazy runtime detection.
    ///
    /// This defers runtime detection until the sandbox is first used,
    /// improving startup time when the sandbox isn't immediately needed.
    pub fn new_lazy(config: SandboxConfig, project_root: PathBuf) -> Self {
        let detected_runtime = if config.runtime != SandboxRuntimeType::Auto {
            // If runtime is explicitly specified, use it immediately
            Some(config.runtime.clone())
        } else {
            // Defer detection for Auto mode
            None
        };

        debug!(
            runtime = ?detected_runtime,
            enabled = config.enabled,
            "Sandbox manager created (lazy)"
        );

        Self {
            config,
            project_root,
            runtime: RwLock::new(None),
            detected_runtime: RwLock::new(detected_runtime),
            explicitly_stopped: RwLock::new(true),
        }
    }

    /// Ensure runtime is detected (lazy initialization).
    async fn ensure_runtime_detected(&self) -> SandboxRuntimeType {
        // Fast path: already detected
        {
            let guard = self.detected_runtime.read().await;
            if let Some(runtime) = guard.as_ref() {
                return runtime.clone();
            }
        }

        // Slow path: need to detect
        let mut guard = self.detected_runtime.write().await;

        // Double-check after acquiring write lock
        if let Some(runtime) = guard.as_ref() {
            return runtime.clone();
        }

        let runtime = detect_runtime().await;
        info!(runtime = ?runtime, "Sandbox runtime detected (lazy)");
        *guard = Some(runtime.clone());
        runtime
    }

    /// Check if sandboxing is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check if a sandbox runtime is available.
    ///
    /// Note: For lazy-initialized managers, this will trigger runtime detection.
    pub async fn is_available_async(&self) -> bool {
        let runtime = self.ensure_runtime_detected().await;
        !matches!(runtime, SandboxRuntimeType::None)
    }

    /// Check if a sandbox runtime is available (sync version).
    ///
    /// Returns false if runtime detection hasn't completed yet.
    /// Use `is_available_async()` for accurate results with lazy managers.
    pub fn is_available(&self) -> bool {
        // Try to get the detected runtime without blocking
        if let Ok(guard) = self.detected_runtime.try_read() {
            if let Some(runtime) = guard.as_ref() {
                return !matches!(runtime, SandboxRuntimeType::None);
            }
        }
        // If not yet detected, assume unavailable (conservative)
        false
    }

    /// Get the detected runtime type (async version for lazy detection).
    pub async fn runtime_type_async(&self) -> SandboxRuntimeType {
        self.ensure_runtime_detected().await
    }

    /// Get the detected runtime type (sync, returns None if not yet detected).
    pub fn runtime_type(&self) -> Option<SandboxRuntimeType> {
        self.detected_runtime
            .try_read()
            .ok()
            .and_then(|g| g.clone())
    }

    /// Get the runtime type as a display string.
    /// Returns "Auto" if detection is pending, or the runtime name.
    pub fn runtime_type_display(&self) -> String {
        match self.runtime_type() {
            Some(rt) => format!("{:?}", rt),
            None => "Auto".to_string(),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Get the project root.
    pub fn project_root(&self) -> &PathBuf {
        &self.project_root
    }

    /// Get or create the runtime instance.
    pub async fn runtime(&self) -> SandboxResult<Arc<dyn SandboxRuntime>> {
        // Check if we already have a runtime
        {
            let guard = self.runtime.read().await;
            if let Some(runtime) = guard.as_ref() {
                return Ok(Arc::clone(runtime));
            }
        }

        // Create a new runtime
        let mut guard = self.runtime.write().await;

        // Double-check after acquiring write lock
        if let Some(runtime) = guard.as_ref() {
            return Ok(Arc::clone(runtime));
        }

        let runtime = self.create_runtime().await?;
        *guard = Some(Arc::clone(&runtime));

        Ok(runtime)
    }

    /// Create a new runtime instance based on configuration.
    async fn create_runtime(&self) -> SandboxResult<Arc<dyn SandboxRuntime>> {
        let path_mapper = PathMapper::new(
            self.project_root.clone(),
            PathBuf::from(&self.config.mounts.workspace_path),
        );

        // Ensure runtime is detected (lazy initialization)
        let detected = self.ensure_runtime_detected().await;

        match detected {
            SandboxRuntimeType::Docker => {
                debug!("Creating Docker runtime");
                let runtime = DockerRuntime::new(self.config.clone(), path_mapper).await?;
                Ok(Arc::new(runtime))
            }
            SandboxRuntimeType::Podman => {
                debug!("Creating Podman runtime");
                let runtime = PodmanRuntime::new(self.config.clone(), path_mapper).await?;
                Ok(Arc::new(runtime))
            }
            #[cfg(target_os = "macos")]
            SandboxRuntimeType::Lima => {
                debug!("Creating Lima runtime");
                let runtime = LimaRuntime::new(self.config.clone(), path_mapper).await?;
                Ok(Arc::new(runtime))
            }
            #[cfg(not(target_os = "macos"))]
            SandboxRuntimeType::Lima => {
                warn!("Lima runtime is only available on macOS, falling back to passthrough");
                Ok(Arc::new(PassthroughRuntime::new(path_mapper)))
            }
            SandboxRuntimeType::None | SandboxRuntimeType::Auto => {
                debug!("Creating passthrough runtime (no sandbox)");
                Ok(Arc::new(PassthroughRuntime::new(path_mapper)))
            }
        }
    }

    /// Start the sandbox.
    ///
    /// This will create and start the container/VM if not already running.
    pub async fn start(&self) -> SandboxResult<()> {
        if !self.config.enabled {
            debug!("Sandbox is disabled, skipping start");
            return Ok(());
        }

        // Clear the explicitly_stopped flag when starting
        {
            let mut stopped = self.explicitly_stopped.write().await;
            *stopped = false;
        }

        let runtime = self.runtime().await?;
        runtime.start().await
    }

    /// Stop the sandbox.
    /// This will stop the container even if we didn't start it ourselves,
    /// which handles the case where the MCP server started it.
    pub async fn stop(&self) -> SandboxResult<()> {
        // Set the explicitly_stopped flag
        {
            let mut stopped = self.explicitly_stopped.write().await;
            *stopped = true;
        }

        // Get or create the runtime - this ensures we have a runtime that can
        // find and stop the container by its deterministic name, even if we
        // didn't start it ourselves (e.g., the MCP server started it)
        let runtime = self.runtime().await?;
        runtime.stop().await
    }

    /// Check if the sandbox was explicitly stopped by the user.
    /// When true, tools should not auto-start the sandbox.
    pub async fn is_explicitly_stopped(&self) -> bool {
        *self.explicitly_stopped.read().await
    }

    /// Check if the sandbox is ready.
    pub async fn is_ready(&self) -> bool {
        if !self.config.enabled {
            return true; // Passthrough is always "ready"
        }

        let guard = self.runtime.read().await;
        if let Some(runtime) = guard.as_ref() {
            runtime.is_ready().await
        } else {
            false
        }
    }

    /// Execute a command in the sandbox.
    ///
    /// If sandboxing is disabled, this will execute on the host.
    pub async fn execute(
        &self,
        command: &str,
        workdir: &std::path::Path,
        timeout: std::time::Duration,
        capabilities: &crate::SandboxCapabilities,
    ) -> SandboxResult<crate::SandboxOutput> {
        let runtime = self.runtime().await?;

        // Start if not running
        if !runtime.is_ready().await {
            runtime.start().await?;
        }

        runtime
            .execute(command, workdir, timeout, capabilities)
            .await
    }

    /// Read a file from the sandbox.
    pub async fn read_file(&self, path: &std::path::Path) -> SandboxResult<Vec<u8>> {
        let runtime = self.runtime().await?;
        runtime.read_file(path).await
    }

    /// Write a file to the sandbox.
    pub async fn write_file(&self, path: &std::path::Path, content: &[u8]) -> SandboxResult<()> {
        let runtime = self.runtime().await?;

        // Start if not running
        if !runtime.is_ready().await {
            runtime.start().await?;
        }

        runtime.write_file(path, content).await
    }

    /// Get the path mapper.
    pub async fn path_mapper(&self) -> SandboxResult<PathMapper> {
        let runtime = self.runtime().await?;
        Ok(runtime.path_mapper().clone())
    }

    /// Check if a tool should bypass the sandbox.
    pub fn should_bypass_tool(&self, tool_name: &str) -> bool {
        self.config.should_bypass(tool_name)
    }
}

/// Detect available sandbox runtimes.
///
/// Checks for Docker, Podman, and Lima in order of preference.
pub async fn detect_runtime() -> SandboxRuntimeType {
    // Check Docker
    if is_docker_available().await {
        info!("Detected Docker runtime");
        return SandboxRuntimeType::Docker;
    }

    // Check Podman
    if is_podman_available().await {
        info!("Detected Podman runtime");
        return SandboxRuntimeType::Podman;
    }

    // Check Lima (macOS only)
    #[cfg(target_os = "macos")]
    if is_lima_available().await {
        info!("Detected Lima runtime");
        return SandboxRuntimeType::Lima;
    }

    warn!("No sandbox runtime detected");
    SandboxRuntimeType::None
}

/// Check if Docker is available.
async fn is_docker_available() -> bool {
    use tokio::process::Command;

    match Command::new("docker").arg("info").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Check if Podman is available.
async fn is_podman_available() -> bool {
    use tokio::process::Command;

    match Command::new("podman").arg("info").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Check if Lima is available (macOS only).
#[cfg(target_os = "macos")]
async fn is_lima_available() -> bool {
    use tokio::process::Command;

    match Command::new("limactl").arg("--version").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sandbox_manager_creation() {
        let config = SandboxConfig::default();
        let project_root = PathBuf::from("/tmp/test");

        let manager = SandboxManager::new(config, project_root).await.unwrap();
        assert!(!manager.is_enabled());
    }

    #[tokio::test]
    async fn test_sandbox_manager_disabled() {
        let config = SandboxConfig {
            enabled: false,
            ..Default::default()
        };
        let project_root = PathBuf::from("/tmp/test");

        let manager = SandboxManager::new(config, project_root).await.unwrap();

        // Starting when disabled should be a no-op
        manager.start().await.unwrap();
    }
}
