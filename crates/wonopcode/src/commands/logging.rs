//! Logging initialization and configuration.
//!
//! Handles logging setup for both headless and interactive modes,
//! with support for file-based logging and platform-specific log directories.

use std::path::PathBuf;

/// Initialize logging based on verbosity and mode.
/// In headless mode, logs are written to stdout.
/// Otherwise, logs are written to a file in the standard log directory.
/// Returns the log file path if logging to file.
pub fn init_logging(verbose: bool, headless: bool) -> Option<PathBuf> {
    let filter = if verbose {
        "wonopcode=debug,wonopcode_core=debug,wonopcode_provider=debug,wonopcode_tools=debug,tower_http=debug"
    } else if headless {
        // In headless mode, include info-level HTTP request logging
        "wonopcode=info,tower_http=info"
    } else {
        "wonopcode=info"
    };

    if headless {
        // In headless mode, log to stdout with colors
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_ansi(true)
            .init();
        return None;
    }

    // Get log directory
    let log_dir = get_log_dir();

    // Create log directory if needed
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("Warning: Could not create log directory: {e}");
        return None;
    }

    // Create log file path
    let log_file = log_dir.join("wonopcode.log");

    // Open log file for appending
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: Could not open log file: {e}");
            return None;
        }
    };

    // Initialize tracing to file
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false)
        .with_writer(file)
        .init();

    Some(log_file)
}

/// Get the log directory path.
pub fn get_log_dir() -> PathBuf {
    // macOS: ~/Library/Logs/wonopcode
    // Linux: ~/.local/state/wonopcode/logs
    // Windows: %LOCALAPPDATA%/wonopcode/logs

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Logs/wonopcode");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(state_dir) = dirs::state_dir() {
            return state_dir.join("wonopcode/logs");
        }
        if let Some(home) = dirs::home_dir() {
            return home.join(".local/state/wonopcode/logs");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app) = dirs::data_local_dir() {
            return local_app.join("wonopcode/logs");
        }
    }

    // Fallback
    PathBuf::from(".wonopcode/logs")
}
