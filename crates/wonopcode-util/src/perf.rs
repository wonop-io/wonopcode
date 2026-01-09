//! Performance monitoring and metrics collection.
//!
//! This module provides structured performance logging to a separate file
//! (`wonopcode-performance.log`) using JSON format for easy analysis.
//!
//! Metrics are logged in JSON Lines format (one JSON object per line) for
//! compatibility with tools like `jq`, pandas, and log analysis platforms.

use chrono::Utc;
use once_cell::sync::OnceCell;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Global performance logger instance.
static PERF_LOGGER: OnceCell<PerfLogger> = OnceCell::new();

/// Global metrics counters.
static METRICS: OnceCell<Metrics> = OnceCell::new();

/// Performance event types.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PerfEventType {
    /// Memory snapshot
    Memory,
    /// Render frame timing
    Render,
    /// Message history operation
    MessageHistory,
    /// Cache operation
    Cache,
    /// Tool execution
    ToolExecution,
    /// Compaction operation
    Compaction,
    /// Scroll calculation
    Scroll,
    /// General timing
    Timing,
}

/// A performance event logged to the performance log file.
#[derive(Debug, Serialize)]
pub struct PerfEvent {
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Event type
    pub event_type: PerfEventType,
    /// Component that generated the event
    pub component: String,
    /// Operation name
    pub operation: String,
    /// Duration in microseconds (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,
    /// Count (e.g., number of messages, cache entries)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// Size in bytes (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<usize>,
    /// Additional context as key-value pairs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

impl PerfEvent {
    /// Create a new performance event.
    pub fn new(
        event_type: PerfEventType,
        component: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            event_type,
            component: component.into(),
            operation: operation.into(),
            duration_us: None,
            count: None,
            size_bytes: None,
            context: None,
        }
    }

    /// Set duration from a Duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_us = Some(duration.as_micros() as u64);
        self
    }

    /// Set duration in microseconds.
    pub fn with_duration_us(mut self, us: u64) -> Self {
        self.duration_us = Some(us);
        self
    }

    /// Set count.
    pub fn with_count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Set size in bytes.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size_bytes = Some(size);
        self
    }

    /// Set additional context.
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = Some(context);
        self
    }
}

/// Performance logger that writes to a separate log file.
pub struct PerfLogger {
    writer: Mutex<Option<BufWriter<File>>>,
    enabled: std::sync::atomic::AtomicBool,
}

impl PerfLogger {
    /// Create a new performance logger.
    fn new() -> Self {
        Self {
            writer: Mutex::new(None),
            enabled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Initialize the logger with a file path.
    fn init(&self, path: PathBuf) -> std::io::Result<()> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        *writer = Some(BufWriter::new(file));
        self.enabled.store(true, Ordering::SeqCst);

        // Write a session start marker
        if let Some(ref mut w) = *writer {
            let start_event = serde_json::json!({
                "timestamp": Utc::now().to_rfc3339(),
                "event_type": "session_start",
                "component": "perf_logger",
                "operation": "init",
            });
            let _ = writeln!(
                w,
                "{}",
                serde_json::to_string(&start_event).unwrap_or_default()
            );
            let _ = w.flush();
        }

        Ok(())
    }

    /// Log a performance event.
    fn log(&self, event: PerfEvent) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(mut writer) = self.writer.lock() {
            if let Some(ref mut w) = *writer {
                if let Ok(json) = serde_json::to_string(&event) {
                    let _ = writeln!(w, "{json}");
                    // Flush periodically for real-time monitoring
                    let _ = w.flush();
                }
            }
        }
    }

    /// Check if logging is enabled.
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// Runtime metrics counters for performance tracking.
pub struct Metrics {
    /// Total messages in history
    pub message_count: AtomicUsize,
    /// Total size of message content in bytes
    pub message_size_bytes: AtomicUsize,
    /// Number of cached render entries
    pub render_cache_entries: AtomicUsize,
    /// Total render cache size estimate
    pub render_cache_size: AtomicUsize,
    /// Number of tool calls executed
    pub tool_calls_total: AtomicU64,
    /// Total render time in microseconds
    pub render_time_total_us: AtomicU64,
    /// Number of render frames
    pub render_frames: AtomicU64,
    /// Number of compactions performed
    pub compactions: AtomicU64,
    /// Messages compacted
    pub messages_compacted: AtomicU64,
    /// Session start time
    session_start: Instant,
}

impl Metrics {
    /// Create new metrics.
    fn new() -> Self {
        Self {
            message_count: AtomicUsize::new(0),
            message_size_bytes: AtomicUsize::new(0),
            render_cache_entries: AtomicUsize::new(0),
            render_cache_size: AtomicUsize::new(0),
            tool_calls_total: AtomicU64::new(0),
            render_time_total_us: AtomicU64::new(0),
            render_frames: AtomicU64::new(0),
            compactions: AtomicU64::new(0),
            messages_compacted: AtomicU64::new(0),
            session_start: Instant::now(),
        }
    }

    /// Get session uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.session_start.elapsed().as_secs()
    }

    /// Get average render time in microseconds.
    pub fn avg_render_time_us(&self) -> u64 {
        let frames = self.render_frames.load(Ordering::Relaxed);
        if frames == 0 {
            0
        } else {
            self.render_time_total_us.load(Ordering::Relaxed) / frames
        }
    }

    /// Create a snapshot of current metrics as a JSON value.
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "uptime_secs": self.uptime_secs(),
            "message_count": self.message_count.load(Ordering::Relaxed),
            "message_size_bytes": self.message_size_bytes.load(Ordering::Relaxed),
            "render_cache_entries": self.render_cache_entries.load(Ordering::Relaxed),
            "render_cache_size": self.render_cache_size.load(Ordering::Relaxed),
            "tool_calls_total": self.tool_calls_total.load(Ordering::Relaxed),
            "render_frames": self.render_frames.load(Ordering::Relaxed),
            "avg_render_time_us": self.avg_render_time_us(),
            "compactions": self.compactions.load(Ordering::Relaxed),
            "messages_compacted": self.messages_compacted.load(Ordering::Relaxed),
        })
    }
}

/// Initialize the performance logging system.
///
/// This creates the performance log file at the standard location:
/// - macOS: ~/Library/Logs/wonopcode/wonopcode-performance.log
/// - Linux: ~/.local/state/wonopcode/logs/wonopcode-performance.log
/// - Windows: %LOCALAPPDATA%/wonopcode/logs/wonopcode-performance.log
pub fn init() -> std::io::Result<PathBuf> {
    let path = get_perf_log_path();

    let logger = PERF_LOGGER.get_or_init(PerfLogger::new);
    logger.init(path.clone())?;

    // Initialize metrics
    METRICS.get_or_init(Metrics::new);

    Ok(path)
}

/// Initialize performance logging to a specific path.
pub fn init_with_path(path: PathBuf) -> std::io::Result<()> {
    let logger = PERF_LOGGER.get_or_init(PerfLogger::new);
    logger.init(path)?;
    METRICS.get_or_init(Metrics::new);
    Ok(())
}

/// Get the performance log file path.
pub fn get_perf_log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Logs/wonopcode/wonopcode-performance.log");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(state_dir) = dirs::state_dir() {
            return state_dir.join("wonopcode/logs/wonopcode-performance.log");
        }
        if let Some(home) = dirs::home_dir() {
            return home.join(".local/state/wonopcode/logs/wonopcode-performance.log");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app) = dirs::data_local_dir() {
            return local_app.join("wonopcode/logs/wonopcode-performance.log");
        }
    }

    // Fallback
    PathBuf::from(".wonopcode/logs/wonopcode-performance.log")
}

/// Check if performance logging is enabled.
pub fn is_enabled() -> bool {
    PERF_LOGGER.get().map(|l| l.is_enabled()).unwrap_or(false)
}

/// Log a performance event.
pub fn log(event: PerfEvent) {
    if let Some(logger) = PERF_LOGGER.get() {
        logger.log(event);
    }
}

/// Get the global metrics instance.
pub fn metrics() -> Option<&'static Metrics> {
    METRICS.get()
}

/// Log a memory snapshot event.
pub fn log_memory(component: &str, operation: &str, count: usize, size_bytes: usize) {
    log(PerfEvent::new(PerfEventType::Memory, component, operation)
        .with_count(count)
        .with_size(size_bytes));
}

/// Log a render frame timing.
pub fn log_render(component: &str, duration: Duration, context: Option<serde_json::Value>) {
    if let Some(m) = metrics() {
        m.render_time_total_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        m.render_frames.fetch_add(1, Ordering::Relaxed);
    }

    let mut event =
        PerfEvent::new(PerfEventType::Render, component, "frame").with_duration(duration);
    if let Some(ctx) = context {
        event = event.with_context(ctx);
    }
    log(event);
}

/// Log a cache operation.
pub fn log_cache(component: &str, operation: &str, entries: usize, size_bytes: Option<usize>) {
    let mut event = PerfEvent::new(PerfEventType::Cache, component, operation).with_count(entries);
    if let Some(size) = size_bytes {
        event = event.with_size(size);
    }
    log(event);
}

/// Log a tool execution.
pub fn log_tool(name: &str, duration: Duration, success: bool) {
    if let Some(m) = metrics() {
        m.tool_calls_total.fetch_add(1, Ordering::Relaxed);
    }

    log(PerfEvent::new(PerfEventType::ToolExecution, "runner", name)
        .with_duration(duration)
        .with_context(serde_json::json!({ "success": success })));
}

/// Log a compaction event.
pub fn log_compaction(messages_before: usize, messages_after: usize, duration: Duration) {
    if let Some(m) = metrics() {
        m.compactions.fetch_add(1, Ordering::Relaxed);
        m.messages_compacted
            .fetch_add((messages_before - messages_after) as u64, Ordering::Relaxed);
    }

    log(
        PerfEvent::new(PerfEventType::Compaction, "runner", "compact")
            .with_duration(duration)
            .with_count(messages_before)
            .with_context(serde_json::json!({
                "messages_before": messages_before,
                "messages_after": messages_after,
                "messages_removed": messages_before - messages_after,
            })),
    );
}

/// Log a message history operation.
pub fn log_message_history(operation: &str, count: usize, size_bytes: usize) {
    if let Some(m) = metrics() {
        m.message_count.store(count, Ordering::Relaxed);
        m.message_size_bytes.store(size_bytes, Ordering::Relaxed);
    }

    log(
        PerfEvent::new(PerfEventType::MessageHistory, "runner", operation)
            .with_count(count)
            .with_size(size_bytes),
    );
}

/// Log a periodic metrics snapshot.
pub fn log_metrics_snapshot() {
    if let Some(m) = metrics() {
        log(
            PerfEvent::new(PerfEventType::Memory, "system", "metrics_snapshot")
                .with_context(m.snapshot()),
        );
    }
}

/// RAII guard for timing an operation.
pub struct TimingGuard {
    component: String,
    operation: String,
    start: Instant,
    event_type: PerfEventType,
}

impl TimingGuard {
    /// Create a new timing guard.
    pub fn new(
        event_type: PerfEventType,
        component: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            component: component.into(),
            operation: operation.into(),
            start: Instant::now(),
            event_type,
        }
    }
}

impl Drop for TimingGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        log(
            PerfEvent::new(self.event_type.clone(), &self.component, &self.operation)
                .with_duration(duration),
        );
    }
}

/// Create a timing guard for an operation.
#[macro_export]
macro_rules! perf_time {
    ($event_type:expr, $component:expr, $operation:expr) => {
        $crate::perf::TimingGuard::new($event_type, $component, $operation)
    };
}

/// Log a performance event with timing.
#[macro_export]
macro_rules! perf_log {
    ($event_type:expr, $component:expr, $operation:expr) => {
        $crate::perf::log($crate::perf::PerfEvent::new(
            $event_type,
            $component,
            $operation,
        ))
    };
    ($event_type:expr, $component:expr, $operation:expr, count = $count:expr) => {
        $crate::perf::log(
            $crate::perf::PerfEvent::new($event_type, $component, $operation).with_count($count),
        )
    };
    ($event_type:expr, $component:expr, $operation:expr, size = $size:expr) => {
        $crate::perf::log(
            $crate::perf::PerfEvent::new($event_type, $component, $operation).with_size($size),
        )
    };
    ($event_type:expr, $component:expr, $operation:expr, duration = $duration:expr) => {
        $crate::perf::log(
            $crate::perf::PerfEvent::new($event_type, $component, $operation)
                .with_duration($duration),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_perf_event_serialization() {
        let event = PerfEvent::new(PerfEventType::Render, "test", "frame")
            .with_duration(Duration::from_micros(1234))
            .with_count(10)
            .with_size(1024);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event_type\":\"render\""));
        assert!(json.contains("\"duration_us\":1234"));
        assert!(json.contains("\"count\":10"));
        assert!(json.contains("\"size_bytes\":1024"));
    }

    #[test]
    fn test_metrics_snapshot() {
        let metrics = Metrics::new();
        metrics.message_count.store(50, Ordering::Relaxed);
        metrics.render_frames.store(100, Ordering::Relaxed);
        metrics.render_time_total_us.store(10000, Ordering::Relaxed);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot["message_count"], 50);
        assert_eq!(snapshot["render_frames"], 100);
        assert_eq!(snapshot["avg_render_time_us"], 100);
    }

    #[test]
    fn test_init_with_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test-perf.log");

        // Note: Can only init once per process, so this test may fail
        // if run after other tests that call init()
        let _ = init_with_path(path);

        // File should be created
        // Note: May not work if already initialized
    }
}
