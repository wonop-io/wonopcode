//! Resource monitoring for sandbox containers.
//!
//! This module provides utilities for monitoring resource usage
//! within sandbox containers, including CPU, memory, and I/O.

use crate::{SandboxResult, SandboxRuntimeType};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

/// Resource usage statistics for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceStats {
    /// CPU usage percentage (0-100 per core, can exceed 100 with multiple cores)
    pub cpu_percent: f64,
    /// Memory usage in bytes
    pub memory_bytes: u64,
    /// Memory limit in bytes
    pub memory_limit: u64,
    /// Memory usage percentage
    pub memory_percent: f64,
    /// Number of running processes
    pub pids: u32,
    /// Network I/O received bytes
    pub network_rx_bytes: u64,
    /// Network I/O transmitted bytes
    pub network_tx_bytes: u64,
    /// Block I/O read bytes
    pub block_read_bytes: u64,
    /// Block I/O written bytes
    pub block_write_bytes: u64,
}

impl ResourceStats {
    /// Check if memory usage is above a threshold percentage.
    pub fn is_memory_critical(&self, threshold: f64) -> bool {
        self.memory_percent > threshold
    }

    /// Check if CPU usage is above a threshold percentage.
    pub fn is_cpu_high(&self, threshold: f64) -> bool {
        self.cpu_percent > threshold
    }

    /// Get a human-readable memory usage string.
    pub fn memory_display(&self) -> String {
        format_bytes(self.memory_bytes)
    }

    /// Get a human-readable memory limit string.
    pub fn memory_limit_display(&self) -> String {
        format_bytes(self.memory_limit)
    }
}

/// Resource monitor for tracking sandbox resource usage.
pub struct ResourceMonitor {
    /// Container ID to monitor
    container_id: String,
    /// Runtime type
    runtime_type: SandboxRuntimeType,
}

impl ResourceMonitor {
    /// Create a new resource monitor.
    pub fn new(container_id: String, runtime_type: SandboxRuntimeType) -> Self {
        Self {
            container_id,
            runtime_type,
        }
    }

    /// Get current resource statistics.
    pub async fn stats(&self) -> SandboxResult<ResourceStats> {
        match self.runtime_type {
            SandboxRuntimeType::Docker => self.docker_stats().await,
            SandboxRuntimeType::Podman => self.podman_stats().await,
            _ => Ok(ResourceStats::default()),
        }
    }

    /// Get Docker container stats.
    async fn docker_stats(&self) -> SandboxResult<ResourceStats> {
        // Use docker stats with --no-stream for a single snapshot
        let output = Command::new("docker")
            .args([
                "stats",
                "--no-stream",
                "--format",
                "{{.CPUPerc}}\t{{.MemUsage}}\t{{.MemPerc}}\t{{.NetIO}}\t{{.BlockIO}}\t{{.PIDs}}",
                &self.container_id,
            ])
            .output()
            .await
            .map_err(|e| crate::SandboxError::ExecFailed(format!("docker stats failed: {}", e)))?;

        if !output.status.success() {
            return Ok(ResourceStats::default());
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        self.parse_stats_output(&output_str)
    }

    /// Get Podman container stats.
    async fn podman_stats(&self) -> SandboxResult<ResourceStats> {
        let output = Command::new("podman")
            .args([
                "stats",
                "--no-stream",
                "--format",
                "{{.CPUPerc}}\t{{.MemUsage}}\t{{.MemPerc}}\t{{.NetIO}}\t{{.BlockIO}}\t{{.PIDs}}",
                &self.container_id,
            ])
            .output()
            .await
            .map_err(|e| crate::SandboxError::ExecFailed(format!("podman stats failed: {}", e)))?;

        if !output.status.success() {
            return Ok(ResourceStats::default());
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        self.parse_stats_output(&output_str)
    }

    /// Parse docker/podman stats output.
    fn parse_stats_output(&self, output: &str) -> SandboxResult<ResourceStats> {
        let line = output.lines().next().unwrap_or_default().trim();
        if line.is_empty() {
            return Ok(ResourceStats::default());
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            debug!(output = %output, "Unexpected stats format");
            return Ok(ResourceStats::default());
        }

        let cpu_percent = parse_percent(parts[0]);
        let (memory_bytes, memory_limit) = parse_mem_usage(parts[1]);
        let memory_percent = parse_percent(parts[2]);
        let (network_rx_bytes, network_tx_bytes) = parse_io(parts[3]);
        let (block_read_bytes, block_write_bytes) = parse_io(parts[4]);
        let pids = parts[5].parse().unwrap_or(0);

        Ok(ResourceStats {
            cpu_percent,
            memory_bytes,
            memory_limit,
            memory_percent,
            pids,
            network_rx_bytes,
            network_tx_bytes,
            block_read_bytes,
            block_write_bytes,
        })
    }
}

/// Parse a percentage string like "15.2%".
fn parse_percent(s: &str) -> f64 {
    s.trim_end_matches('%').trim().parse().unwrap_or(0.0)
}

/// Parse memory usage string like "100MiB / 2GiB".
fn parse_mem_usage(s: &str) -> (u64, u64) {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        (parse_size_string(parts[0]), parse_size_string(parts[1]))
    } else {
        (0, 0)
    }
}

/// Parse I/O string like "1.5MB / 2.3MB".
fn parse_io(s: &str) -> (u64, u64) {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        (parse_size_string(parts[0]), parse_size_string(parts[1]))
    } else {
        (0, 0)
    }
}

/// Parse a size string like "1.5GiB" or "100MB".
fn parse_size_string(s: &str) -> u64 {
    let s = s.trim();

    // Try to find the numeric part
    let num_end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num_str, unit) = s.split_at(num_end);

    let num: f64 = num_str.parse().unwrap_or(0.0);
    let unit = unit.trim().to_uppercase();

    let multiplier: u64 = match unit.as_str() {
        "B" | "" => 1,
        "KB" | "K" => 1000,
        "KIB" => 1024,
        "MB" | "M" => 1_000_000,
        "MIB" => 1024 * 1024,
        "GB" | "G" => 1_000_000_000,
        "GIB" => 1024 * 1024 * 1024,
        "TB" | "T" => 1_000_000_000_000,
        "TIB" => 1024 * 1024 * 1024 * 1024,
        _ => 1,
    };

    (num * multiplier as f64) as u64
}

/// Format bytes as human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.1}GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1}MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1}KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{}B", bytes)
    }
}

/// Resource usage event for monitoring callbacks.
#[derive(Debug, Clone)]
pub enum ResourceEvent {
    /// Memory usage crossed a threshold
    MemoryWarning { usage_percent: f64, threshold: f64 },
    /// CPU usage is sustained high
    CpuWarning { usage_percent: f64, threshold: f64 },
    /// Process limit approaching
    PidsWarning { current: u32, limit: u32 },
}

/// Continuous resource monitor that emits events.
pub struct ContinuousMonitor {
    monitor: ResourceMonitor,
    memory_threshold: f64,
    cpu_threshold: f64,
}

impl ContinuousMonitor {
    /// Create a new continuous monitor.
    ///
    /// Note: The interval parameter is currently unused as continuous polling
    /// is not yet implemented. Call `check_warnings()` manually for now.
    pub fn new(
        container_id: String,
        runtime_type: SandboxRuntimeType,
        _interval: Duration,
    ) -> Self {
        Self {
            monitor: ResourceMonitor::new(container_id, runtime_type),
            memory_threshold: 90.0, // 90% memory usage warning
            cpu_threshold: 95.0,    // 95% CPU usage warning
        }
    }

    /// Set memory warning threshold (0-100).
    pub fn with_memory_threshold(mut self, threshold: f64) -> Self {
        self.memory_threshold = threshold;
        self
    }

    /// Set CPU warning threshold (0-100+).
    pub fn with_cpu_threshold(mut self, threshold: f64) -> Self {
        self.cpu_threshold = threshold;
        self
    }

    /// Get current stats.
    pub async fn stats(&self) -> SandboxResult<ResourceStats> {
        self.monitor.stats().await
    }

    /// Check for resource warnings.
    pub async fn check_warnings(&self) -> SandboxResult<Vec<ResourceEvent>> {
        let stats = self.monitor.stats().await?;
        let mut warnings = Vec::new();

        if stats.is_memory_critical(self.memory_threshold) {
            warnings.push(ResourceEvent::MemoryWarning {
                usage_percent: stats.memory_percent,
                threshold: self.memory_threshold,
            });
        }

        if stats.is_cpu_high(self.cpu_threshold) {
            warnings.push(ResourceEvent::CpuWarning {
                usage_percent: stats.cpu_percent,
                threshold: self.cpu_threshold,
            });
        }

        Ok(warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_percent() {
        assert!((parse_percent("15.2%") - 15.2).abs() < f64::EPSILON);
        assert!((parse_percent("100%") - 100.0).abs() < f64::EPSILON);
        assert!((parse_percent("0%") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_size_string() {
        assert_eq!(parse_size_string("100MiB"), 100 * 1024 * 1024);
        assert_eq!(parse_size_string("2GiB"), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size_string("1.5GB"), 1_500_000_000);
        assert_eq!(parse_size_string("1024KB"), 1_024_000);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1.0KiB");
        assert_eq!(format_bytes(1048576), "1.0MiB");
        assert_eq!(format_bytes(1073741824), "1.0GiB");
    }

    #[test]
    fn test_parse_mem_usage() {
        let (used, limit) = parse_mem_usage("100MiB / 2GiB");
        assert_eq!(used, 100 * 1024 * 1024);
        assert_eq!(limit, 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_resource_stats_checks() {
        let stats = ResourceStats {
            memory_percent: 85.0,
            cpu_percent: 50.0,
            ..Default::default()
        };

        assert!(!stats.is_memory_critical(90.0));
        assert!(stats.is_memory_critical(80.0));
        assert!(!stats.is_cpu_high(60.0));
        assert!(stats.is_cpu_high(40.0));
    }
}
