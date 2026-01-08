//! Configuration types for sandbox settings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    /// Enable sandboxing (default: false for backward compatibility)
    pub enabled: bool,

    /// Sandbox runtime type
    pub runtime: SandboxRuntimeType,

    /// Container image to use (default: wonopcode/sandbox:latest)
    pub image: Option<String>,

    /// Resource limits
    pub resources: ResourceLimits,

    /// Network policy
    pub network: NetworkPolicy,

    /// Mount configuration
    pub mounts: MountConfig,

    /// Tools that bypass sandbox (run on host)
    #[serde(default)]
    pub bypass_tools: Vec<String>,

    /// Keep sandbox running between commands (improves performance)
    #[serde(default = "default_true")]
    pub keep_alive: bool,

    /// Timeout for sandbox startup in seconds
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_secs: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            runtime: SandboxRuntimeType::default(),
            image: None,
            resources: ResourceLimits::default(),
            network: NetworkPolicy::default(),
            mounts: MountConfig::default(),
            bypass_tools: vec![
                "todoread".to_string(),
                "todowrite".to_string(),
                "skill".to_string(),
            ],
            keep_alive: true,
            startup_timeout_secs: 60,
        }
    }
}

impl SandboxConfig {
    /// Get the container image to use
    pub fn image(&self) -> &str {
        self.image.as_deref().unwrap_or(DEFAULT_IMAGE)
    }

    /// Check if a tool should bypass the sandbox
    pub fn should_bypass(&self, tool_name: &str) -> bool {
        self.bypass_tools.iter().any(|t| t == tool_name)
    }

    /// Merge with another config (other takes precedence)
    pub fn merge(self, other: Self) -> Self {
        Self {
            enabled: other.enabled,
            runtime: if matches!(other.runtime, SandboxRuntimeType::Auto) {
                self.runtime
            } else {
                other.runtime
            },
            image: other.image.or(self.image),
            resources: self.resources.merge(other.resources),
            network: other.network,
            mounts: self.mounts.merge(other.mounts),
            bypass_tools: if other.bypass_tools.is_empty() {
                self.bypass_tools
            } else {
                other.bypass_tools
            },
            keep_alive: other.keep_alive,
            startup_timeout_secs: other.startup_timeout_secs,
        }
    }
}

/// Default container image - Ubuntu 24.04 with common dev tools
///
/// This is a well-maintained base image. For more complete dev environments:
/// - `mcr.microsoft.com/devcontainers/base:ubuntu` - VS Code devcontainer base
/// - `ghcr.io/devcontainers/images/universal:latest` - Full devcontainer with many languages
pub const DEFAULT_IMAGE: &str = "ubuntu:24.04";

/// Sandbox runtime type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SandboxRuntimeType {
    /// Auto-detect: Docker > Podman > Lima (on macOS)
    #[default]
    Auto,
    /// Docker runtime
    Docker,
    /// Podman runtime
    Podman,
    /// Lima runtime (macOS only)
    Lima,
    /// No sandboxing (passthrough)
    None,
}

impl SandboxRuntimeType {
    /// Check if this is a container-based runtime
    pub fn is_container(&self) -> bool {
        matches!(self, Self::Docker | Self::Podman)
    }

    /// Check if this is the Lima VM runtime
    pub fn is_lima(&self) -> bool {
        matches!(self, Self::Lima)
    }

    /// Check if sandboxing is disabled
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Resource limits for the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceLimits {
    /// Memory limit (e.g., "2G", "512M")
    pub memory: String,

    /// CPU limit (e.g., 2.0 = 2 CPUs)
    pub cpus: f32,

    /// Disk space limit (optional)
    pub disk: Option<String>,

    /// Process (PID) limit
    pub pids: u32,

    /// Read-only root filesystem
    #[serde(default)]
    pub readonly_rootfs: bool,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory: "2G".to_string(),
            cpus: 2.0,
            disk: None,
            pids: 256,
            readonly_rootfs: false,
        }
    }
}

impl ResourceLimits {
    /// Parse memory string to bytes
    pub fn memory_bytes(&self) -> Option<u64> {
        parse_size(&self.memory)
    }

    /// Get CPU limit in nanoseconds (for Docker API)
    pub fn cpu_nano(&self) -> i64 {
        (self.cpus * 1_000_000_000.0) as i64
    }

    /// Merge with another config
    pub fn merge(self, other: Self) -> Self {
        Self {
            memory: if other.memory != Self::default().memory {
                other.memory
            } else {
                self.memory
            },
            cpus: if (other.cpus - Self::default().cpus).abs() > f32::EPSILON {
                other.cpus
            } else {
                self.cpus
            },
            disk: other.disk.or(self.disk),
            pids: if other.pids != Self::default().pids {
                other.pids
            } else {
                self.pids
            },
            readonly_rootfs: other.readonly_rootfs,
        }
    }
}

/// Network policy for the sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicy {
    /// Allow outbound to common ports (80, 443, 22)
    #[default]
    Limited,
    /// Full network access
    Full,
    /// No network access
    None,
}

impl NetworkPolicy {
    /// Check if network is completely disabled
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Get Docker network mode
    pub fn docker_network_mode(&self) -> Option<String> {
        match self {
            Self::None => Some("none".to_string()),
            _ => None, // Use default bridge network
        }
    }
}

/// Mount configuration for the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MountConfig {
    /// Mount project directory read-write (default: true)
    pub workspace_writable: bool,

    /// Additional read-only mounts (host_path -> container_path)
    pub readonly: HashMap<String, String>,

    /// Persist package caches across sessions
    pub persist_caches: bool,

    /// Custom workspace path in container (default: /workspace)
    pub workspace_path: String,
}

impl Default for MountConfig {
    fn default() -> Self {
        Self {
            workspace_writable: true,
            readonly: HashMap::new(),
            persist_caches: true,
            workspace_path: "/workspace".to_string(),
        }
    }
}

impl MountConfig {
    /// Merge with another config
    pub fn merge(self, other: Self) -> Self {
        let mut readonly = self.readonly;
        readonly.extend(other.readonly);

        Self {
            workspace_writable: other.workspace_writable,
            readonly,
            persist_caches: other.persist_caches,
            workspace_path: if other.workspace_path != Self::default().workspace_path {
                other.workspace_path
            } else {
                self.workspace_path
            },
        }
    }
}

/// Capabilities requested for sandbox execution.
#[derive(Debug, Clone, Default)]
pub struct SandboxCapabilities {
    /// Allow network access
    pub network: bool,
    /// Mount workspace read-only
    pub read_only_workspace: bool,
    /// Allow privileged operations (not recommended)
    pub privileged: bool,
}

// Helper functions

fn default_true() -> bool {
    true
}

fn default_startup_timeout() -> u64 {
    60
}

/// Parse a size string like "2G" or "512M" to bytes
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();
    let (num_str, multiplier) = if let Some(n) = s.strip_suffix('G') {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1)
    } else {
        (s.as_str(), 1)
    };

    num_str.trim().parse::<u64>().ok().map(|n| n * multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("2G"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512M"), Some(512 * 1024 * 1024));
        assert_eq!(parse_size("1024K"), Some(1024 * 1024));
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("invalid"), None);
    }

    #[test]
    fn test_default_config() {
        let config = SandboxConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.runtime, SandboxRuntimeType::Auto);
        assert_eq!(config.image(), DEFAULT_IMAGE);
    }

    #[test]
    fn test_should_bypass() {
        let config = SandboxConfig::default();
        assert!(config.should_bypass("todoread"));
        assert!(config.should_bypass("todowrite"));
        assert!(!config.should_bypass("bash"));
    }

    #[test]
    fn test_resource_limits() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.memory_bytes(), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(limits.cpu_nano(), 2_000_000_000);
    }
}
