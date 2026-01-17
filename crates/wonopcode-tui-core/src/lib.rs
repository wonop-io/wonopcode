//! Core types and utilities for wonopcode TUI.
//!
//! This crate provides foundational types shared across all TUI crates:
//! - Theme system with color definitions
//! - Keybind configuration and management
//! - Event handling
//! - Performance metrics
//! - Model state persistence

pub mod event;
pub mod keybind;
pub mod metrics;
pub mod model_state;
pub mod theme;

pub use event::{is_backspace, is_enter, is_escape, is_quit, Event, EventHandler, EventLoopHandle};
pub use keybind::{KeyAction, Keybind, KeybindConfig, KeybindManager};
pub use metrics::{
    complete_input_latency, event_timer, frame_timer, get as get_metrics, init as init_metrics,
    is_enabled as metrics_enabled, mark_input_start, record_event, record_frame, record_input_latency,
    record_scroll, record_widget, reset as reset_metrics, set_enabled as set_metrics_enabled,
    summary as metrics_summary, widget_timer, EventType, MetricsSummary, TimerGuard, TuiMetrics,
    WidgetSummary, SLOW_FRAME_THRESHOLD_MS, VERY_SLOW_FRAME_THRESHOLD_MS,
};
pub use model_state::ModelState;
pub use theme::{AgentMode, RenderSettings, Theme};
