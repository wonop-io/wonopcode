//! TUI performance metrics for measuring responsiveness.
//!
//! This module provides detailed performance tracking for the TUI including:
//! - Frame render times (with percentile tracking)
//! - Event handling latency
//! - Input responsiveness (time from keypress to screen update)
//! - Scroll performance
//! - Per-widget render times
//!
//! # Usage
//!
//! Enable metrics collection:
//! ```ignore
//! use wonopcode_tui::metrics;
//!
//! // Initialize metrics (call once at startup)
//! metrics::init();
//!
//! // Time a frame render
//! let _guard = metrics::frame_timer();
//!
//! // Or manually:
//! let start = std::time::Instant::now();
//! // ... render ...
//! metrics::record_frame(start.elapsed());
//! ```
//!
//! # Metrics Summary
//!
//! Get a summary of current metrics:
//! ```ignore
//! if let Some(summary) = metrics::summary() {
//!     println!("Avg frame time: {}ms", summary.avg_frame_ms);
//!     println!("P99 frame time: {}ms", summary.p99_frame_ms);
//! }
//! ```

use once_cell::sync::OnceCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Global metrics instance.
static METRICS: OnceCell<TuiMetrics> = OnceCell::new();

/// Whether metrics collection is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Maximum samples to keep for percentile calculations.
const MAX_SAMPLES: usize = 1000;

/// Threshold for "slow" frame warning (16.67ms = 60fps).
pub const SLOW_FRAME_THRESHOLD_MS: f64 = 16.67;

/// Threshold for "very slow" frame (50ms = 20fps).
pub const VERY_SLOW_FRAME_THRESHOLD_MS: f64 = 50.0;

/// Initialize the metrics system.
pub fn init() {
    METRICS.get_or_init(TuiMetrics::new);
    ENABLED.store(true, Ordering::SeqCst);
}

/// Check if metrics are enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Enable or disable metrics collection.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

/// Get the global metrics instance.
pub fn get() -> Option<&'static TuiMetrics> {
    if is_enabled() {
        METRICS.get()
    } else {
        None
    }
}

/// Record a frame render time.
pub fn record_frame(duration: Duration) {
    if let Some(m) = get() {
        m.record_frame(duration);
    }
}

/// Record an event handling time.
pub fn record_event(event_type: EventType, duration: Duration) {
    if let Some(m) = get() {
        m.record_event(event_type, duration);
    }
}

/// Record input-to-render latency (full round trip).
pub fn record_input_latency(duration: Duration) {
    if let Some(m) = get() {
        m.record_input_latency(duration);
    }
}

/// Record a scroll operation time.
pub fn record_scroll(duration: Duration, lines: usize) {
    if let Some(m) = get() {
        m.record_scroll(duration, lines);
    }
}

/// Record a widget render time.
pub fn record_widget(name: &str, duration: Duration) {
    if let Some(m) = get() {
        m.record_widget(name, duration);
    }
}

/// Mark the start of input processing (for latency measurement).
pub fn mark_input_start() -> Option<Instant> {
    if is_enabled() {
        Some(Instant::now())
    } else {
        None
    }
}

/// Complete input latency measurement.
pub fn complete_input_latency(start: Option<Instant>) {
    if let Some(start) = start {
        record_input_latency(start.elapsed());
    }
}

/// Create a timer guard for frame rendering.
pub fn frame_timer() -> Option<TimerGuard> {
    if is_enabled() {
        Some(TimerGuard::new(TimerType::Frame))
    } else {
        None
    }
}

/// Create a timer guard for event handling.
pub fn event_timer(event_type: EventType) -> Option<TimerGuard> {
    if is_enabled() {
        Some(TimerGuard::new(TimerType::Event(event_type)))
    } else {
        None
    }
}

/// Create a timer guard for widget rendering.
pub fn widget_timer(name: &'static str) -> Option<TimerGuard> {
    if is_enabled() {
        Some(TimerGuard::new(TimerType::Widget(name)))
    } else {
        None
    }
}

/// Get a summary of current metrics.
pub fn summary() -> Option<MetricsSummary> {
    get().map(|m| m.summary())
}

/// Reset all metrics.
pub fn reset() {
    if let Some(m) = get() {
        m.reset();
    }
}

/// Types of events we track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Key,
    Mouse,
    Resize,
    Tick,
    Update,
}

/// Timer type for the guard.
enum TimerType {
    Frame,
    Event(EventType),
    Widget(&'static str),
}

/// RAII guard for timing operations.
pub struct TimerGuard {
    timer_type: TimerType,
    start: Instant,
}

impl TimerGuard {
    fn new(timer_type: TimerType) -> Self {
        Self {
            timer_type,
            start: Instant::now(),
        }
    }
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        match &self.timer_type {
            TimerType::Frame => record_frame(duration),
            TimerType::Event(et) => record_event(*et, duration),
            TimerType::Widget(name) => record_widget(name, duration),
        }
    }
}

/// Rolling statistics tracker.
struct RollingStats {
    samples: VecDeque<f64>,
    sum: f64,
    min: f64,
    max: f64,
    count: u64,
}

impl RollingStats {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            sum: 0.0,
            min: f64::MAX,
            max: 0.0,
            count: 0,
        }
    }

    fn record(&mut self, value_ms: f64) {
        // Update running stats
        self.sum += value_ms;
        self.min = self.min.min(value_ms);
        self.max = self.max.max(value_ms);
        self.count += 1;

        // Add to rolling window
        if self.samples.len() >= MAX_SAMPLES {
            if let Some(old) = self.samples.pop_front() {
                self.sum -= old;
            }
        }
        self.samples.push_back(value_ms);
    }

    fn avg(&self) -> f64 {
        if self.samples.is_empty() {
            0.0
        } else {
            self.sum / self.samples.len() as f64
        }
    }

    fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn reset(&mut self) {
        self.samples.clear();
        self.sum = 0.0;
        self.min = f64::MAX;
        self.max = 0.0;
        self.count = 0;
    }
}

/// Per-widget metrics.
struct WidgetMetrics {
    name: String,
    stats: RollingStats,
}

impl WidgetMetrics {
    fn new(name: String) -> Self {
        Self {
            name,
            stats: RollingStats::new(),
        }
    }
}

/// Main TUI metrics container.
pub struct TuiMetrics {
    /// Frame render times.
    frame_stats: Mutex<RollingStats>,
    /// Event handling times by type.
    event_stats: Mutex<[RollingStats; 5]>,
    /// Input-to-render latency.
    input_latency: Mutex<RollingStats>,
    /// Scroll operation times.
    scroll_stats: Mutex<RollingStats>,
    /// Per-widget render times.
    widget_stats: Mutex<Vec<WidgetMetrics>>,
    /// Total frames rendered.
    total_frames: AtomicU64,
    /// Slow frames (>16.67ms).
    slow_frames: AtomicU64,
    /// Very slow frames (>50ms).
    very_slow_frames: AtomicU64,
    /// Session start time.
    start_time: Instant,
}

impl TuiMetrics {
    fn new() -> Self {
        Self {
            frame_stats: Mutex::new(RollingStats::new()),
            event_stats: Mutex::new([
                RollingStats::new(),
                RollingStats::new(),
                RollingStats::new(),
                RollingStats::new(),
                RollingStats::new(),
            ]),
            input_latency: Mutex::new(RollingStats::new()),
            scroll_stats: Mutex::new(RollingStats::new()),
            widget_stats: Mutex::new(Vec::new()),
            total_frames: AtomicU64::new(0),
            slow_frames: AtomicU64::new(0),
            very_slow_frames: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    fn record_frame(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;

        self.total_frames.fetch_add(1, Ordering::Relaxed);

        if ms > SLOW_FRAME_THRESHOLD_MS {
            self.slow_frames.fetch_add(1, Ordering::Relaxed);
        }
        if ms > VERY_SLOW_FRAME_THRESHOLD_MS {
            self.very_slow_frames.fetch_add(1, Ordering::Relaxed);
        }

        if let Ok(mut stats) = self.frame_stats.lock() {
            stats.record(ms);
        }
    }

    fn record_event(&self, event_type: EventType, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        let idx = match event_type {
            EventType::Key => 0,
            EventType::Mouse => 1,
            EventType::Resize => 2,
            EventType::Tick => 3,
            EventType::Update => 4,
        };

        if let Ok(mut stats) = self.event_stats.lock() {
            stats[idx].record(ms);
        }
    }

    fn record_input_latency(&self, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        if let Ok(mut stats) = self.input_latency.lock() {
            stats.record(ms);
        }
    }

    fn record_scroll(&self, duration: Duration, _lines: usize) {
        let ms = duration.as_secs_f64() * 1000.0;
        if let Ok(mut stats) = self.scroll_stats.lock() {
            stats.record(ms);
        }
    }

    fn record_widget(&self, name: &str, duration: Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        if let Ok(mut widgets) = self.widget_stats.lock() {
            // Find or create widget stats
            if let Some(w) = widgets.iter_mut().find(|w| w.name == name) {
                w.stats.record(ms);
            } else {
                let mut w = WidgetMetrics::new(name.to_string());
                w.stats.record(ms);
                widgets.push(w);
            }
        }
    }

    fn reset(&self) {
        if let Ok(mut s) = self.frame_stats.lock() {
            s.reset();
        }
        if let Ok(mut s) = self.event_stats.lock() {
            for stat in s.iter_mut() {
                stat.reset();
            }
        }
        if let Ok(mut s) = self.input_latency.lock() {
            s.reset();
        }
        if let Ok(mut s) = self.scroll_stats.lock() {
            s.reset();
        }
        if let Ok(mut s) = self.widget_stats.lock() {
            s.clear();
        }
        self.total_frames.store(0, Ordering::Relaxed);
        self.slow_frames.store(0, Ordering::Relaxed);
        self.very_slow_frames.store(0, Ordering::Relaxed);
    }

    /// Get a summary of current metrics.
    pub fn summary(&self) -> MetricsSummary {
        let frame = self.frame_stats.lock().ok();
        let events = self.event_stats.lock().ok();
        let input = self.input_latency.lock().ok();
        let scroll = self.scroll_stats.lock().ok();
        let widgets = self.widget_stats.lock().ok();

        let total_frames = self.total_frames.load(Ordering::Relaxed);
        let slow_frames = self.slow_frames.load(Ordering::Relaxed);
        let very_slow_frames = self.very_slow_frames.load(Ordering::Relaxed);

        let uptime_secs = self.start_time.elapsed().as_secs_f64();
        let fps = if uptime_secs > 0.0 {
            total_frames as f64 / uptime_secs
        } else {
            0.0
        };

        MetricsSummary {
            uptime_secs,
            total_frames,
            slow_frames,
            very_slow_frames,
            slow_frame_pct: if total_frames > 0 {
                (slow_frames as f64 / total_frames as f64) * 100.0
            } else {
                0.0
            },
            fps,
            avg_frame_ms: frame.as_ref().map(|f| f.avg()).unwrap_or(0.0),
            p50_frame_ms: frame.as_ref().map(|f| f.percentile(50.0)).unwrap_or(0.0),
            p95_frame_ms: frame.as_ref().map(|f| f.percentile(95.0)).unwrap_or(0.0),
            p99_frame_ms: frame.as_ref().map(|f| f.percentile(99.0)).unwrap_or(0.0),
            max_frame_ms: frame.as_ref().map(|f| f.max).unwrap_or(0.0),
            avg_key_event_ms: events.as_ref().map(|e| e[0].avg()).unwrap_or(0.0),
            avg_mouse_event_ms: events.as_ref().map(|e| e[1].avg()).unwrap_or(0.0),
            avg_input_latency_ms: input.as_ref().map(|i| i.avg()).unwrap_or(0.0),
            p99_input_latency_ms: input.as_ref().map(|i| i.percentile(99.0)).unwrap_or(0.0),
            avg_scroll_ms: scroll.as_ref().map(|s| s.avg()).unwrap_or(0.0),
            widget_stats: widgets
                .map(|w| {
                    w.iter()
                        .map(|ws| WidgetSummary {
                            name: ws.name.clone(),
                            avg_ms: ws.stats.avg(),
                            max_ms: ws.stats.max,
                            count: ws.stats.count,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// Summary of TUI performance metrics.
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    /// Uptime in seconds.
    pub uptime_secs: f64,
    /// Total frames rendered.
    pub total_frames: u64,
    /// Frames taking >16.67ms (below 60fps).
    pub slow_frames: u64,
    /// Frames taking >50ms (below 20fps).
    pub very_slow_frames: u64,
    /// Percentage of slow frames.
    pub slow_frame_pct: f64,
    /// Average FPS.
    pub fps: f64,
    /// Average frame time in ms.
    pub avg_frame_ms: f64,
    /// Median (p50) frame time in ms.
    pub p50_frame_ms: f64,
    /// 95th percentile frame time in ms.
    pub p95_frame_ms: f64,
    /// 99th percentile frame time in ms.
    pub p99_frame_ms: f64,
    /// Maximum frame time in ms.
    pub max_frame_ms: f64,
    /// Average key event handling time in ms.
    pub avg_key_event_ms: f64,
    /// Average mouse event handling time in ms.
    pub avg_mouse_event_ms: f64,
    /// Average input-to-render latency in ms.
    pub avg_input_latency_ms: f64,
    /// 99th percentile input latency in ms.
    pub p99_input_latency_ms: f64,
    /// Average scroll operation time in ms.
    pub avg_scroll_ms: f64,
    /// Per-widget statistics.
    pub widget_stats: Vec<WidgetSummary>,
}

impl MetricsSummary {
    /// Check if performance is acceptable (< 5% slow frames).
    pub fn is_healthy(&self) -> bool {
        self.slow_frame_pct < 5.0 && self.p99_frame_ms < VERY_SLOW_FRAME_THRESHOLD_MS
    }

    /// Get a human-readable status.
    pub fn status(&self) -> &'static str {
        if self.slow_frame_pct < 1.0 && self.p99_frame_ms < SLOW_FRAME_THRESHOLD_MS {
            "excellent"
        } else if self.slow_frame_pct < 5.0 && self.p99_frame_ms < VERY_SLOW_FRAME_THRESHOLD_MS {
            "good"
        } else if self.slow_frame_pct < 20.0 {
            "degraded"
        } else {
            "poor"
        }
    }

    /// Format as a human-readable report.
    pub fn to_report(&self) -> String {
        let mut report = String::new();
        report.push_str("=== TUI Performance Metrics ===\n\n");

        report.push_str(&format!("Status: {}\n", self.status().to_uppercase()));
        report.push_str(&format!("Uptime: {:.1}s\n\n", self.uptime_secs));

        report.push_str("Frame Statistics:\n");
        report.push_str(&format!("  Total frames:  {}\n", self.total_frames));
        report.push_str(&format!("  Average FPS:   {:.1}\n", self.fps));
        report.push_str(&format!("  Avg frame:     {:.2}ms\n", self.avg_frame_ms));
        report.push_str(&format!("  P50 frame:     {:.2}ms\n", self.p50_frame_ms));
        report.push_str(&format!("  P95 frame:     {:.2}ms\n", self.p95_frame_ms));
        report.push_str(&format!("  P99 frame:     {:.2}ms\n", self.p99_frame_ms));
        report.push_str(&format!("  Max frame:     {:.2}ms\n", self.max_frame_ms));
        report.push_str(&format!(
            "  Slow frames:   {} ({:.1}%)\n",
            self.slow_frames, self.slow_frame_pct
        ));
        report.push_str(&format!("  Very slow:     {}\n\n", self.very_slow_frames));

        report.push_str("Input Responsiveness:\n");
        report.push_str(&format!(
            "  Avg key event:     {:.2}ms\n",
            self.avg_key_event_ms
        ));
        report.push_str(&format!(
            "  Avg mouse event:   {:.2}ms\n",
            self.avg_mouse_event_ms
        ));
        report.push_str(&format!(
            "  Avg input latency: {:.2}ms\n",
            self.avg_input_latency_ms
        ));
        report.push_str(&format!(
            "  P99 input latency: {:.2}ms\n",
            self.p99_input_latency_ms
        ));
        report.push_str(&format!(
            "  Avg scroll:        {:.2}ms\n\n",
            self.avg_scroll_ms
        ));

        if !self.widget_stats.is_empty() {
            report.push_str("Widget Render Times (top 10 by avg):\n");
            let mut sorted = self.widget_stats.clone();
            sorted.sort_by(|a, b| {
                b.avg_ms
                    .partial_cmp(&a.avg_ms)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for w in sorted.iter().take(10) {
                report.push_str(&format!(
                    "  {:20} avg: {:6.2}ms  max: {:6.2}ms  ({} calls)\n",
                    w.name, w.avg_ms, w.max_ms, w.count
                ));
            }
        }

        report
    }

    /// Convert to JSON for logging/export.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status(),
            "uptime_secs": self.uptime_secs,
            "frames": {
                "total": self.total_frames,
                "slow": self.slow_frames,
                "very_slow": self.very_slow_frames,
                "slow_pct": self.slow_frame_pct,
                "fps": self.fps,
            },
            "frame_times_ms": {
                "avg": self.avg_frame_ms,
                "p50": self.p50_frame_ms,
                "p95": self.p95_frame_ms,
                "p99": self.p99_frame_ms,
                "max": self.max_frame_ms,
            },
            "input_ms": {
                "key_event_avg": self.avg_key_event_ms,
                "mouse_event_avg": self.avg_mouse_event_ms,
                "latency_avg": self.avg_input_latency_ms,
                "latency_p99": self.p99_input_latency_ms,
                "scroll_avg": self.avg_scroll_ms,
            },
            "widgets": self.widget_stats.iter().map(|w| {
                serde_json::json!({
                    "name": w.name,
                    "avg_ms": w.avg_ms,
                    "max_ms": w.max_ms,
                    "count": w.count,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

/// Per-widget performance summary.
#[derive(Debug, Clone)]
pub struct WidgetSummary {
    /// Widget name.
    pub name: String,
    /// Average render time in ms.
    pub avg_ms: f64,
    /// Maximum render time in ms.
    pub max_ms: f64,
    /// Number of render calls.
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rolling_stats() {
        let mut stats = RollingStats::new();

        // Add some samples
        for i in 1..=100 {
            stats.record(i as f64);
        }

        assert_eq!(stats.count, 100);
        assert!((stats.avg() - 50.5).abs() < 0.1);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 100.0);

        // Check percentiles
        assert!((stats.percentile(50.0) - 50.0).abs() < 2.0);
        assert!((stats.percentile(99.0) - 99.0).abs() < 2.0);
    }

    #[test]
    fn test_metrics_summary() {
        init();

        // Record some frame times
        for _ in 0..10 {
            record_frame(Duration::from_millis(10));
        }
        record_frame(Duration::from_millis(20)); // slow
        record_frame(Duration::from_millis(60)); // very slow

        let summary = summary().unwrap();
        assert!(summary.total_frames >= 12);
        assert!(summary.slow_frames >= 1);
        assert!(summary.very_slow_frames >= 1);
    }

    #[test]
    fn test_timer_guard() {
        init();

        {
            let _guard = frame_timer();
            std::thread::sleep(Duration::from_millis(1));
        }

        let summary = summary().unwrap();
        assert!(summary.total_frames >= 1);
        assert!(summary.avg_frame_ms >= 1.0);
    }

    #[test]
    fn test_widget_metrics() {
        init();

        record_widget("messages", Duration::from_millis(5));
        record_widget("messages", Duration::from_millis(10));
        record_widget("sidebar", Duration::from_millis(2));

        let summary = summary().unwrap();
        assert!(summary.widget_stats.len() >= 2);

        let messages = summary.widget_stats.iter().find(|w| w.name == "messages");
        assert!(messages.is_some());
        assert!((messages.unwrap().avg_ms - 7.5).abs() < 0.5);
    }
}
