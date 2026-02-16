//! Timeout constants for permission operations.
//!
//! These constants define the default timeouts used across the wonopcode system
//! for permission dialogs. Provider-specific timeouts are defined by the
//! `LanguageModel::tool_timeout()` method.

/// Default timeout for permission requests in seconds.
///
/// This is the maximum time the system will wait for a user to respond to a
/// permission dialog when no provider-specific timeout is set. After this
/// timeout, the permission request is denied.
///
/// This is a very large value (~3 days) to allow users plenty of time to
/// respond. Providers that need shorter timeouts (like Claude CLI) override
/// this via `LanguageModel::tool_timeout()`.
///
/// Current value: 259200 seconds (~3 days)
pub const DEFAULT_PERMISSION_TIMEOUT_SECS: u64 = 259200;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_permission_timeout_is_reasonable() {
        // Should be at least 1 day
        assert!(DEFAULT_PERMISSION_TIMEOUT_SECS >= 86400);
        // Should be at most 1 week
        assert!(DEFAULT_PERMISSION_TIMEOUT_SECS <= 604800);
    }
}
