//! GitHub API rate limiter.
//!
//! Implements rate limiting following GitHub's best practices:
//! - Serial request execution (no concurrent requests)
//! - Minimum 1 second delay between mutative requests (POST, PATCH, PUT, DELETE)
//! - Exponential backoff on rate limit errors
//! - Respect Retry-After and x-ratelimit-reset headers

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// GitHub rate limit headers.
const HEADER_RATE_LIMIT_REMAINING: &str = "x-ratelimit-remaining";
const HEADER_RATE_LIMIT_RESET: &str = "x-ratelimit-reset";
const HEADER_RETRY_AFTER: &str = "retry-after";

/// Minimum delay between mutative requests (POST, PATCH, PUT, DELETE).
const MUTATIVE_REQUEST_DELAY: Duration = Duration::from_millis(1000);

/// Minimum delay between all requests to avoid secondary rate limits.
const MIN_REQUEST_DELAY: Duration = Duration::from_millis(100);

/// Maximum backoff delay.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Initial backoff delay.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);

/// Rate limiter state.
#[derive(Debug)]
struct RateLimiterState {
    /// Last request timestamp.
    last_request: Option<Instant>,
    /// Last mutative request timestamp.
    last_mutative_request: Option<Instant>,
    /// Current backoff delay (for exponential backoff).
    current_backoff: Duration,
    /// Number of remaining requests according to GitHub.
    remaining_requests: Option<u32>,
    /// When the rate limit resets (Unix timestamp).
    rate_limit_reset: Option<u64>,
}

impl Default for RateLimiterState {
    fn default() -> Self {
        Self {
            last_request: None,
            last_mutative_request: None,
            current_backoff: INITIAL_BACKOFF,
            remaining_requests: None,
            rate_limit_reset: None,
        }
    }
}

/// GitHub API rate limiter.
///
/// This rate limiter ensures compliance with GitHub's rate limiting policies
/// by serializing requests and enforcing delays between them.
#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<RateLimiterState>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimiterState::default())),
        }
    }

    /// Wait before making a request.
    ///
    /// This ensures proper spacing between requests to avoid rate limiting.
    /// For mutative requests (POST, PATCH, PUT, DELETE), enforces a longer delay.
    pub async fn wait_for_request(&self, is_mutative: bool) {
        let mut state = self.state.lock().await;

        // Check if we need to wait based on rate limit info
        if let (Some(remaining), Some(reset)) = (state.remaining_requests, state.rate_limit_reset) {
            if remaining == 0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                if reset > now {
                    let wait_duration = Duration::from_secs(reset - now + 1);
                    warn!(
                        "Rate limit exhausted, waiting {} seconds until reset",
                        wait_duration.as_secs()
                    );
                    drop(state);
                    tokio::time::sleep(wait_duration).await;
                    state = self.state.lock().await;
                }
            }
        }

        // Calculate required delay
        let now = Instant::now();
        let required_delay = if is_mutative {
            // For mutative requests, ensure at least 1 second since last mutative request
            state
                .last_mutative_request
                .map(|last| {
                    let elapsed = now.duration_since(last);
                    MUTATIVE_REQUEST_DELAY.saturating_sub(elapsed)
                })
                .unwrap_or(Duration::ZERO)
        } else {
            // For read requests, just ensure minimum spacing
            state
                .last_request
                .map(|last| {
                    let elapsed = now.duration_since(last);
                    MIN_REQUEST_DELAY.saturating_sub(elapsed)
                })
                .unwrap_or(Duration::ZERO)
        };

        if !required_delay.is_zero() {
            debug!(
                "Rate limiter: waiting {:?} before {} request",
                required_delay,
                if is_mutative { "mutative" } else { "read" }
            );
            drop(state);
            tokio::time::sleep(required_delay).await;
            state = self.state.lock().await;
        }

        // Update timestamps
        let now = Instant::now();
        state.last_request = Some(now);
        if is_mutative {
            state.last_mutative_request = Some(now);
        }
    }

    /// Record a successful response and update rate limit info from headers.
    pub async fn record_success(&self, headers: &reqwest::header::HeaderMap) {
        let mut state = self.state.lock().await;

        // Reset backoff on success
        state.current_backoff = INITIAL_BACKOFF;

        // Parse rate limit headers
        if let Some(remaining) = headers.get(HEADER_RATE_LIMIT_REMAINING) {
            if let Ok(remaining_str) = remaining.to_str() {
                if let Ok(remaining_val) = remaining_str.parse::<u32>() {
                    state.remaining_requests = Some(remaining_val);
                    debug!("Rate limit remaining: {}", remaining_val);
                }
            }
        }

        if let Some(reset) = headers.get(HEADER_RATE_LIMIT_RESET) {
            if let Ok(reset_str) = reset.to_str() {
                if let Ok(reset_val) = reset_str.parse::<u64>() {
                    state.rate_limit_reset = Some(reset_val);
                }
            }
        }
    }

    /// Handle a rate limit error (HTTP 429 or 403 with rate limit message).
    ///
    /// Returns the duration to wait before retrying.
    pub async fn handle_rate_limit(&self, headers: &reqwest::header::HeaderMap) -> Duration {
        let mut state = self.state.lock().await;

        // Check Retry-After header first
        if let Some(retry_after) = headers.get(HEADER_RETRY_AFTER) {
            if let Ok(retry_str) = retry_after.to_str() {
                if let Ok(seconds) = retry_str.parse::<u64>() {
                    let wait_duration = Duration::from_secs(seconds);
                    warn!("Rate limited, Retry-After header says wait {} seconds", seconds);
                    return wait_duration;
                }
            }
        }

        // Check x-ratelimit-reset header
        if let Some(reset) = headers.get(HEADER_RATE_LIMIT_RESET) {
            if let Ok(reset_str) = reset.to_str() {
                if let Ok(reset_timestamp) = reset_str.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    if reset_timestamp > now {
                        let wait_duration = Duration::from_secs(reset_timestamp - now + 1);
                        warn!(
                            "Rate limited, reset at {}, waiting {} seconds",
                            reset_timestamp,
                            wait_duration.as_secs()
                        );
                        state.remaining_requests = Some(0);
                        state.rate_limit_reset = Some(reset_timestamp);
                        return wait_duration;
                    }
                }
            }
        }

        // Fall back to exponential backoff
        let backoff = state.current_backoff;
        state.current_backoff = (state.current_backoff * 2).min(MAX_BACKOFF);
        warn!(
            "Rate limited, using exponential backoff: {} seconds",
            backoff.as_secs()
        );
        backoff
    }

    /// Check if a response indicates a rate limit error.
    pub fn is_rate_limited(status: reqwest::StatusCode, body: &str) -> bool {
        // HTTP 429 is the standard rate limit status
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return true;
        }

        // GitHub sometimes returns 403 for rate limits
        if status == reqwest::StatusCode::FORBIDDEN {
            let lower = body.to_lowercase();
            if lower.contains("rate limit") || lower.contains("secondary rate limit") {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new();
        // First request should not block
        limiter.wait_for_request(false).await;
    }

    #[tokio::test]
    async fn test_mutative_request_delay() {
        let limiter = RateLimiter::new();

        // First mutative request
        let start = Instant::now();
        limiter.wait_for_request(true).await;

        // Second mutative request should wait ~1 second
        limiter.wait_for_request(true).await;
        let elapsed = start.elapsed();

        // Should have waited at least the mutative delay
        assert!(elapsed >= MUTATIVE_REQUEST_DELAY - Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_read_requests_faster() {
        let limiter = RateLimiter::new();

        // Read requests should be faster than mutative
        let start = Instant::now();
        limiter.wait_for_request(false).await;
        limiter.wait_for_request(false).await;
        limiter.wait_for_request(false).await;
        let elapsed = start.elapsed();

        // Should be much less than 3 seconds (what mutative would take)
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn test_is_rate_limited() {
        // 429 is always rate limited
        assert!(RateLimiter::is_rate_limited(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            ""
        ));

        // 403 with rate limit message
        assert!(RateLimiter::is_rate_limited(
            reqwest::StatusCode::FORBIDDEN,
            "API rate limit exceeded"
        ));

        assert!(RateLimiter::is_rate_limited(
            reqwest::StatusCode::FORBIDDEN,
            "You have exceeded a secondary rate limit"
        ));

        // 403 without rate limit message
        assert!(!RateLimiter::is_rate_limited(
            reqwest::StatusCode::FORBIDDEN,
            "Resource not accessible"
        ));

        // 200 is not rate limited
        assert!(!RateLimiter::is_rate_limited(
            reqwest::StatusCode::OK,
            ""
        ));
    }
}
