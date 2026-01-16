//! RAII-based timing utilities for measuring and logging operation durations.
//!
//! # Example
//!
//! ```rust,ignore
//! use wonopcode_util::timing::TimingGuard;
//!
//! async fn execute_tool(name: &str) {
//!     let _timing = TimingGuard::new("tool", name);
//!     // ... tool execution ...
//!     // Duration is logged when _timing is dropped
//! }
//! ```

use std::time::Instant;
use tracing::{debug, info, warn};

/// RAII guard that measures and logs the duration of an operation.
///
/// When dropped, logs the elapsed time since creation.
/// Uses tracing for structured logging with the operation type and name.
pub struct TimingGuard {
    /// Type of operation (e.g., "tool", "request", "query")
    operation_type: &'static str,
    /// Name of the specific operation (e.g., "bash", "read", "edit")
    operation_name: String,
    /// When the operation started
    start: Instant,
    /// Minimum duration to log at info level (below this uses debug)
    info_threshold_ms: u64,
    /// Minimum duration to log at warn level (for slow operations)
    warn_threshold_ms: u64,
}

impl TimingGuard {
    /// Create a new timing guard.
    ///
    /// The duration will be logged when the guard is dropped.
    pub fn new(operation_type: &'static str, operation_name: impl Into<String>) -> Self {
        let operation_name = operation_name.into();
        debug!(
            operation_type = operation_type,
            operation_name = %operation_name,
            "Starting operation"
        );
        Self {
            operation_type,
            operation_name,
            start: Instant::now(),
            info_threshold_ms: 100,  // Log at info if >= 100ms
            warn_threshold_ms: 5000, // Log at warn if >= 5s
        }
    }

    /// Create a timing guard for tool execution.
    pub fn tool(name: impl Into<String>) -> Self {
        Self::new("tool", name)
    }

    /// Create a timing guard for MCP tool execution.
    pub fn mcp_tool(name: impl Into<String>) -> Self {
        Self::new("mcp_tool", name)
    }

    /// Set the threshold for info-level logging (in milliseconds).
    pub fn with_info_threshold(mut self, ms: u64) -> Self {
        self.info_threshold_ms = ms;
        self
    }

    /// Set the threshold for warn-level logging (in milliseconds).
    pub fn with_warn_threshold(mut self, ms: u64) -> Self {
        self.warn_threshold_ms = ms;
        self
    }

    /// Get the elapsed time so far.
    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }

    /// Get the elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> u128 {
        self.start.elapsed().as_millis()
    }
}

impl Drop for TimingGuard {
    #[allow(clippy::cognitive_complexity)]
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let duration_ms = duration.as_millis();

        // Format duration nicely
        let duration_str = if duration_ms < 1000 {
            format!("{duration_ms}ms")
        } else if duration_ms < 60_000 {
            format!("{:.2}s", duration_ms as f64 / 1000.0)
        } else {
            let mins = duration_ms / 60_000;
            let secs = (duration_ms % 60_000) as f64 / 1000.0;
            format!("{mins}m {secs:.1}s")
        };

        if duration_ms >= self.warn_threshold_ms as u128 {
            warn!(
                operation_type = self.operation_type,
                operation_name = %self.operation_name,
                duration_ms = duration_ms as u64,
                duration = %duration_str,
                "Slow operation completed"
            );
        } else if duration_ms >= self.info_threshold_ms as u128 {
            info!(
                operation_type = self.operation_type,
                operation_name = %self.operation_name,
                duration_ms = duration_ms as u64,
                duration = %duration_str,
                "Operation completed"
            );
        } else {
            debug!(
                operation_type = self.operation_type,
                operation_name = %self.operation_name,
                duration_ms = duration_ms as u64,
                duration = %duration_str,
                "Operation completed"
            );
        }
    }
}

/// Convenience macro for timing a block of code.
///
/// # Example
///
/// ```rust,ignore
/// use wonopcode_util::time_operation;
///
/// let result = time_operation!("tool", "bash", {
///     execute_bash_command().await
/// });
/// ```
#[macro_export]
macro_rules! time_operation {
    ($op_type:expr, $op_name:expr, $block:expr) => {{
        let _timing = $crate::timing::TimingGuard::new($op_type, $op_name);
        $block
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_timing_guard_basic() {
        let guard = TimingGuard::new("test", "basic");
        sleep(Duration::from_millis(10));
        assert!(guard.elapsed_ms() >= 10);
        drop(guard);
    }

    #[test]
    fn test_timing_guard_tool() {
        let guard = TimingGuard::tool("read");
        sleep(Duration::from_millis(5));
        assert!(guard.elapsed_ms() >= 5);
    }

    #[test]
    fn test_timing_guard_thresholds() {
        let guard = TimingGuard::new("test", "thresholds")
            .with_info_threshold(50)
            .with_warn_threshold(1000);
        sleep(Duration::from_millis(10));
        drop(guard);
    }
}
