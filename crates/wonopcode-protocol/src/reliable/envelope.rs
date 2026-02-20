//! Message envelope for reliable delivery.
//!
//! Every message in the reliable protocol is wrapped in an envelope that provides:
//! - Unique message ID for correlation and deduplication
//! - Sequence numbers for ordering and gap detection
//! - Delivery mode (reliable with ack, or best-effort)
//! - Timestamps for debugging and timeout calculation

use serde::{Deserialize, Serialize};

/// Standard message envelope for all protocol communication.
///
/// # Example
///
/// ```
/// use wonopcode_protocol::reliable::{Envelope, DeliveryMode, ClientMessage};
///
/// let envelope = Envelope::reliable(
///     ClientMessage::ListWorkstreams,
///     42,  // sequence number
/// );
///
/// assert!(envelope.delivery.is_reliable());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// Unique message ID (UUID v7 for time-ordering).
    ///
    /// UUID v7 is preferred because it's time-ordered, making it easier
    /// to debug message flows and detect ordering issues.
    pub id: String,

    /// Correlation ID - links requests to responses.
    ///
    /// - For requests: `None` (or parent request ID if nested)
    /// - For responses/acks: The request ID being responded to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    /// Monotonic sequence number per session.
    ///
    /// Starts at 0, increments by 1 for each message sent.
    /// Used for gap detection: if client receives seq=5 but expected seq=3,
    /// it knows seq=3 and seq=4 were lost and can request replay.
    pub seq: u64,

    /// Server timestamp (milliseconds since Unix epoch).
    ///
    /// Server assigns this to ensure consistency across clients.
    /// Clients should not trust their own clocks for protocol timing.
    pub timestamp: u64,

    /// Message payload (see [`ClientMessage`] / [`ServerMessage`]).
    pub payload: T,

    /// Delivery requirements.
    pub delivery: DeliveryMode,
}

impl<T> Envelope<T> {
    /// Create a new envelope with reliable delivery.
    ///
    /// Uses default timeout (5s) and retries (3).
    pub fn reliable(payload: T, seq: u64) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            correlation_id: None,
            seq,
            timestamp: now_millis(),
            payload,
            delivery: DeliveryMode::Reliable {
                timeout_ms: super::DEFAULT_ACK_TIMEOUT_MS,
                max_retries: super::DEFAULT_MAX_RETRIES,
            },
        }
    }

    /// Create a new envelope with best-effort delivery.
    ///
    /// No ack required, message may be lost.
    pub fn best_effort(payload: T, seq: u64) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            correlation_id: None,
            seq,
            timestamp: now_millis(),
            payload,
            delivery: DeliveryMode::BestEffort,
        }
    }

    /// Create a response envelope correlated to a request.
    pub fn response_to(request_id: &str, payload: T, seq: u64) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            correlation_id: Some(request_id.to_string()),
            seq,
            timestamp: now_millis(),
            payload,
            delivery: DeliveryMode::Reliable {
                timeout_ms: super::DEFAULT_ACK_TIMEOUT_MS,
                max_retries: super::DEFAULT_MAX_RETRIES,
            },
        }
    }

    /// Check if this envelope requires acknowledgment.
    pub fn requires_ack(&self) -> bool {
        self.delivery.is_reliable()
    }

    /// Get the age of this message in milliseconds.
    pub fn age_ms(&self) -> u64 {
        let now = now_millis();
        if now > self.timestamp {
            now - self.timestamp
        } else {
            0
        }
    }
}

/// Delivery mode for protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Must be acknowledged, will retry on failure.
    ///
    /// If no ack is received within `timeout_ms`, the message is resent.
    /// After `max_retries` failures, the send is considered failed and
    /// the caller is notified.
    Reliable {
        /// Timeout in milliseconds before retry.
        timeout_ms: u32,
        /// Maximum retry attempts before giving up.
        max_retries: u8,
    },

    /// Best effort, no ack required.
    ///
    /// Use for high-frequency updates where occasional loss is acceptable,
    /// such as progress indicators or typing indicators.
    BestEffort,
}

impl DeliveryMode {
    /// Check if this mode requires acknowledgment.
    pub fn is_reliable(&self) -> bool {
        matches!(self, DeliveryMode::Reliable { .. })
    }

    /// Get the timeout for reliable mode, or None for best-effort.
    pub fn timeout_ms(&self) -> Option<u32> {
        match self {
            DeliveryMode::Reliable { timeout_ms, .. } => Some(*timeout_ms),
            DeliveryMode::BestEffort => None,
        }
    }

    /// Get the max retries for reliable mode, or None for best-effort.
    pub fn max_retries(&self) -> Option<u8> {
        match self {
            DeliveryMode::Reliable { max_retries, .. } => Some(*max_retries),
            DeliveryMode::BestEffort => None,
        }
    }
}

impl Default for DeliveryMode {
    fn default() -> Self {
        DeliveryMode::Reliable {
            timeout_ms: super::DEFAULT_ACK_TIMEOUT_MS,
            max_retries: super::DEFAULT_MAX_RETRIES,
        }
    }
}

/// Get current time in milliseconds since Unix epoch.
/// 
/// On WASM with the `js` feature, uses `js_sys::Date::now()`.
/// On other platforms, uses `std::time::SystemTime::now()`.
#[cfg(feature = "js")]
pub fn now_millis() -> u64 {
    js_sys::Date::now() as u64
}

/// Get current time in milliseconds since Unix epoch.
/// 
/// On WASM with the `js` feature, uses `js_sys::Date::now()`.
/// On other platforms, uses `std::time::SystemTime::now()`.
#[cfg(not(feature = "js"))]
pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_reliable() {
        let envelope = Envelope::reliable("test payload", 42);

        assert!(!envelope.id.is_empty());
        assert!(envelope.correlation_id.is_none());
        assert_eq!(envelope.seq, 42);
        assert!(envelope.timestamp > 0);
        assert!(envelope.requires_ack());
    }

    #[test]
    fn test_envelope_best_effort() {
        let envelope = Envelope::best_effort("test payload", 42);

        assert!(!envelope.requires_ack());
    }

    #[test]
    fn test_envelope_response() {
        let envelope = Envelope::response_to("req-123", "response payload", 43);

        assert_eq!(envelope.correlation_id, Some("req-123".to_string()));
        assert!(envelope.requires_ack());
    }

    #[test]
    fn test_delivery_mode_serialization() {
        let reliable = DeliveryMode::Reliable {
            timeout_ms: 5000,
            max_retries: 3,
        };
        let json = serde_json::to_string(&reliable).unwrap();
        assert!(json.contains("reliable"));
        assert!(json.contains("5000"));

        let best_effort = DeliveryMode::BestEffort;
        let json = serde_json::to_string(&best_effort).unwrap();
        assert!(json.contains("best_effort"));

        // Round-trip
        let parsed: DeliveryMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, DeliveryMode::BestEffort);
    }

    #[test]
    fn test_envelope_serialization() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct TestPayload {
            message: String,
        }

        let envelope = Envelope::reliable(
            TestPayload {
                message: "hello".to_string(),
            },
            1,
        );

        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: Envelope<TestPayload> = serde_json::from_str(&json).unwrap();

        assert_eq!(envelope.id, parsed.id);
        assert_eq!(envelope.seq, parsed.seq);
        assert_eq!(envelope.payload, parsed.payload);
    }
}
