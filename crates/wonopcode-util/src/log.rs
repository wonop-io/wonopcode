//! Logging setup using tracing.
//!
//! This module provides consistent logging configuration across wonopcode.

use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Log level configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    /// Parse a log level from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(LogLevel::Trace),
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" => Some(LogLevel::Warn),
            "error" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

/// Logging configuration.
pub struct LogConfig {
    /// Whether to print logs to stderr.
    pub print: bool,
    /// Log level.
    pub level: LogLevel,
    /// Whether to include file/line info in logs.
    pub include_location: bool,
    /// Log file path (if any).
    pub file: Option<PathBuf>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            print: false,
            level: LogLevel::Info,
            include_location: false,
            file: None,
        }
    }
}

/// Initialize logging with the given configuration.
///
/// This should be called once at application startup.
pub fn init(config: LogConfig) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(config.level.as_str()));

    let subscriber = tracing_subscriber::registry().with(filter);

    if config.print {
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_level(true)
            .with_file(config.include_location)
            .with_line_number(config.include_location);

        subscriber.with(fmt_layer).init();
    } else {
        // If not printing, just set up the registry with filter
        // (logs will go nowhere, but spans still work)
        subscriber.init();
    }
}

/// Get the default log file path.
pub fn default_log_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("wonopcode").join("logs").join("wonopcode.log"))
}

/// Create a tracing span for a service.
#[macro_export]
macro_rules! service_span {
    ($name:expr) => {
        tracing::info_span!("service", name = $name)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::parse("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("DEBUG"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("invalid"), None);
    }

    #[test]
    fn test_log_level_as_str() {
        assert_eq!(LogLevel::Debug.as_str(), "debug");
        assert_eq!(LogLevel::Error.as_str(), "error");
    }

    #[test]
    fn test_default_log_config() {
        let config = LogConfig::default();
        assert!(!config.print);
        assert_eq!(config.level, LogLevel::Info);
    }
}
