//! Session retry functionality.
//!
//! Provides retry logic for failed API requests with exponential backoff
//! and rate limit handling.

use std::time::Duration;
use tracing::debug;

/// Initial retry delay in milliseconds.
pub const RETRY_INITIAL_DELAY_MS: u64 = 2000;

/// Backoff factor for exponential delay.
pub const RETRY_BACKOFF_FACTOR: u64 = 2;

/// Maximum delay when no rate limit headers are present.
pub const RETRY_MAX_DELAY_NO_HEADERS_MS: u64 = 30_000;

/// Maximum number of retry attempts.
pub const RETRY_MAX_ATTEMPTS: u32 = 5;

/// Calculate the delay before retrying.
///
/// If rate limit headers are provided, uses those. Otherwise,
/// uses exponential backoff with a maximum delay.
pub fn calculate_delay(attempt: u32, rate_limit_info: Option<&RateLimitInfo>) -> Duration {
    if let Some(info) = rate_limit_info {
        // Use retry-after-ms if available
        if let Some(ms) = info.retry_after_ms {
            return Duration::from_millis(ms);
        }

        // Use retry-after (seconds) if available
        if let Some(secs) = info.retry_after_secs {
            return Duration::from_secs(secs);
        }

        // Use reset time if available
        if let Some(reset_at) = info.reset_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if reset_at > now {
                return Duration::from_secs(reset_at - now);
            }
        }
    }

    // Exponential backoff with cap
    let delay = RETRY_INITIAL_DELAY_MS * RETRY_BACKOFF_FACTOR.pow(attempt.saturating_sub(1));
    Duration::from_millis(delay.min(RETRY_MAX_DELAY_NO_HEADERS_MS))
}

/// Rate limit information from response headers.
#[derive(Debug, Clone, Default)]
pub struct RateLimitInfo {
    /// Retry after (milliseconds).
    pub retry_after_ms: Option<u64>,
    /// Retry after (seconds).
    pub retry_after_secs: Option<u64>,
    /// Unix timestamp when rate limit resets.
    pub reset_at: Option<u64>,
}

impl RateLimitInfo {
    /// Parse rate limit info from response headers.
    pub fn from_headers(headers: &[(String, String)]) -> Option<Self> {
        let mut info = RateLimitInfo::default();
        let mut has_info = false;

        for (key, value) in headers {
            let key_lower = key.to_lowercase();

            if key_lower == "retry-after-ms" {
                if let Ok(ms) = value.parse::<u64>() {
                    info.retry_after_ms = Some(ms);
                    has_info = true;
                }
            } else if key_lower == "retry-after" {
                // Try parsing as seconds first
                if let Ok(secs) = value.parse::<u64>() {
                    info.retry_after_secs = Some(secs);
                    has_info = true;
                }
                // Could also parse HTTP date format here if needed
            } else if key_lower == "x-ratelimit-reset" || key_lower == "x-rate-limit-reset" {
                if let Ok(reset) = value.parse::<u64>() {
                    info.reset_at = Some(reset);
                    has_info = true;
                }
            }
        }

        if has_info {
            Some(info)
        } else {
            None
        }
    }
}

/// Error classification for retry decisions.
#[derive(Debug, Clone)]
pub enum RetryableError {
    /// Rate limited - should retry with delay.
    RateLimited { message: String },
    /// Server overloaded - should retry.
    Overloaded { message: String },
    /// Server error - may retry.
    ServerError { message: String },
    /// Not retryable.
    NotRetryable,
}

/// Check if an error is retryable.
pub fn classify_error(status: Option<u16>, message: &str) -> RetryableError {
    // Check HTTP status
    if let Some(status) = status {
        match status {
            429 => {
                return RetryableError::RateLimited {
                    message: "Rate limited".to_string(),
                };
            }
            500..=599 => {
                return RetryableError::ServerError {
                    message: format!("Server error: {}", status),
                };
            }
            _ => {}
        }
    }

    // Check message content
    let message_lower = message.to_lowercase();

    if message_lower.contains("overloaded") {
        return RetryableError::Overloaded {
            message: "Provider is overloaded".to_string(),
        };
    }

    if message_lower.contains("rate_limit") || message_lower.contains("too_many_requests") {
        return RetryableError::RateLimited {
            message: "Rate limited".to_string(),
        };
    }

    if message_lower.contains("server_error") || message_lower.contains("internal_error") {
        return RetryableError::ServerError {
            message: "Server error".to_string(),
        };
    }

    if message_lower.contains("exhausted") || message_lower.contains("unavailable") {
        return RetryableError::Overloaded {
            message: "Provider unavailable".to_string(),
        };
    }

    RetryableError::NotRetryable
}

/// Check if we should retry based on the error.
pub fn should_retry(error: &RetryableError) -> bool {
    !matches!(error, RetryableError::NotRetryable)
}

/// Sleep for the specified duration, respecting cancellation.
pub async fn sleep_with_cancel(
    duration: Duration,
    cancel: &tokio_util::sync::CancellationToken,
) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => true,
        _ = cancel.cancelled() => false,
    }
}

/// Retry helper that handles the retry loop.
pub struct RetryHelper {
    max_attempts: u32,
    current_attempt: u32,
}

impl RetryHelper {
    /// Create a new retry helper.
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            current_attempt: 0,
        }
    }

    /// Create with default max attempts.
    pub fn default_attempts() -> Self {
        Self::new(RETRY_MAX_ATTEMPTS)
    }

    /// Check if we should retry and get the delay.
    ///
    /// Returns None if we've exhausted retries.
    pub fn next_attempt(&mut self, rate_limit_info: Option<&RateLimitInfo>) -> Option<Duration> {
        self.current_attempt += 1;

        if self.current_attempt > self.max_attempts {
            debug!(
                attempt = self.current_attempt,
                max = self.max_attempts,
                "Max retry attempts reached"
            );
            return None;
        }

        let delay = calculate_delay(self.current_attempt, rate_limit_info);
        debug!(
            attempt = self.current_attempt,
            max = self.max_attempts,
            delay_ms = delay.as_millis(),
            "Scheduling retry"
        );

        Some(delay)
    }

    /// Get the current attempt number.
    pub fn current_attempt(&self) -> u32 {
        self.current_attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_delay_no_info() {
        // First attempt
        let delay = calculate_delay(1, None);
        assert_eq!(delay, Duration::from_millis(2000));

        // Second attempt (exponential backoff)
        let delay = calculate_delay(2, None);
        assert_eq!(delay, Duration::from_millis(4000));

        // Third attempt
        let delay = calculate_delay(3, None);
        assert_eq!(delay, Duration::from_millis(8000));
    }

    #[test]
    fn test_calculate_delay_with_retry_after() {
        let info = RateLimitInfo {
            retry_after_ms: Some(5000),
            ..Default::default()
        };
        let delay = calculate_delay(1, Some(&info));
        assert_eq!(delay, Duration::from_millis(5000));
    }

    #[test]
    fn test_classify_error() {
        assert!(matches!(
            classify_error(Some(429), ""),
            RetryableError::RateLimited { .. }
        ));

        assert!(matches!(
            classify_error(Some(500), ""),
            RetryableError::ServerError { .. }
        ));

        assert!(matches!(
            classify_error(None, "Provider is overloaded"),
            RetryableError::Overloaded { .. }
        ));

        assert!(matches!(
            classify_error(Some(400), "bad request"),
            RetryableError::NotRetryable
        ));
    }

    #[test]
    fn test_retry_helper() {
        let mut helper = RetryHelper::new(3);

        assert!(helper.next_attempt(None).is_some());
        assert_eq!(helper.current_attempt(), 1);

        assert!(helper.next_attempt(None).is_some());
        assert_eq!(helper.current_attempt(), 2);

        assert!(helper.next_attempt(None).is_some());
        assert_eq!(helper.current_attempt(), 3);

        assert!(helper.next_attempt(None).is_none());
    }
}
