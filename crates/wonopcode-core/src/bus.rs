//! Event bus for inter-component communication.
//!
//! The event bus provides a publish/subscribe mechanism for components
//! to communicate without direct coupling. Events are typed and can
//! carry arbitrary payload data.
//!
//! # Example
//!
//! ```ignore
//! let bus = Bus::new();
//!
//! // Subscribe to session created events
//! let mut rx = bus.subscribe::<SessionCreated>();
//! tokio::spawn(async move {
//!     while let Ok(event) = rx.recv().await {
//!         println!("Session created: {}", event.session_id);
//!     }
//! });
//!
//! // Publish an event
//! bus.publish(SessionCreated { session_id: "ses_123".to_string() });
//! ```

use serde::{Deserialize, Serialize};
use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

/// Default channel capacity.
const DEFAULT_CAPACITY: usize = 256;

/// Maximum number of events to keep in the replay buffer.
const REPLAY_BUFFER_SIZE: usize = 1000;

/// Trait for events that can be published on the bus.
pub trait Event: Clone + Send + Sync + 'static {
    /// Event type name for serialization/logging.
    fn event_type() -> &'static str;
}

/// The event bus for pub/sub communication.
#[derive(Clone)]
pub struct Bus {
    inner: Arc<BusInner>,
}

struct BusInner {
    /// Typed channels by TypeId.
    channels: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    /// Wildcard subscribers (receive all events as JSON).
    wildcard: broadcast::Sender<SequencedEvent>,
    /// Monotonically increasing sequence number.
    sequence: AtomicU64,
    /// Ring buffer of recent events for replay.
    replay_buffer: RwLock<VecDeque<SequencedEvent>>,
}

/// A serialized event with sequence number for reliable delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedEvent {
    /// Unique sequence number (monotonically increasing).
    pub seq: u64,
    /// Timestamp in milliseconds since Unix epoch.
    pub timestamp: i64,
    /// Event type name.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Event payload as JSON.
    pub payload: serde_json::Value,
}

/// A serialized event for wildcard subscribers (legacy, kept for backwards compatibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusEvent {
    /// Event type name.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Event payload as JSON.
    pub payload: serde_json::Value,
}

impl From<SequencedEvent> for BusEvent {
    fn from(event: SequencedEvent) -> Self {
        Self {
            event_type: event.event_type,
            payload: event.payload,
        }
    }
}

impl Bus {
    /// Create a new event bus.
    pub fn new() -> Self {
        let (wildcard, _) = broadcast::channel(DEFAULT_CAPACITY);
        Self {
            inner: Arc::new(BusInner {
                channels: RwLock::new(HashMap::new()),
                wildcard,
                sequence: AtomicU64::new(0),
                replay_buffer: RwLock::new(VecDeque::with_capacity(REPLAY_BUFFER_SIZE)),
            }),
        }
    }

    /// Get the current sequence number (the next event will have seq + 1).
    pub fn current_sequence(&self) -> u64 {
        self.inner.sequence.load(Ordering::SeqCst)
    }

    /// Publish an event to all subscribers.
    pub async fn publish<E: Event + Serialize>(&self, event: E) {
        let type_id = TypeId::of::<E>();

        // Send to typed subscribers
        let channels = self.inner.channels.read().await;
        if let Some(sender) = channels.get(&type_id) {
            if let Some(tx) = sender.downcast_ref::<broadcast::Sender<E>>() {
                // Ignore send errors (no receivers)
                let _ = tx.send(event.clone());
            }
        }
        drop(channels);

        // Send to wildcard subscribers with sequence number
        if let Ok(payload) = serde_json::to_value(&event) {
            let seq = self.inner.sequence.fetch_add(1, Ordering::SeqCst) + 1;
            let timestamp = chrono::Utc::now().timestamp_millis();

            let sequenced_event = SequencedEvent {
                seq,
                timestamp,
                event_type: E::event_type().to_string(),
                payload,
            };

            // Add to replay buffer
            {
                let mut buffer = self.inner.replay_buffer.write().await;
                if buffer.len() >= REPLAY_BUFFER_SIZE {
                    buffer.pop_front();
                }
                buffer.push_back(sequenced_event.clone());
            }

            let _ = self.inner.wildcard.send(sequenced_event);
        }
    }

    /// Subscribe to events of type E.
    pub async fn subscribe<E: Event>(&self) -> broadcast::Receiver<E> {
        let type_id = TypeId::of::<E>();

        // Check if channel exists
        {
            let channels = self.inner.channels.read().await;
            if let Some(sender) = channels.get(&type_id) {
                if let Some(tx) = sender.downcast_ref::<broadcast::Sender<E>>() {
                    return tx.subscribe();
                }
            }
        }

        // Create new channel
        let mut channels = self.inner.channels.write().await;
        let (tx, rx) = broadcast::channel::<E>(DEFAULT_CAPACITY);
        channels.insert(type_id, Box::new(tx));
        rx
    }

    /// Subscribe to all events (wildcard) with sequencing support.
    pub fn subscribe_all(&self) -> broadcast::Receiver<SequencedEvent> {
        self.inner.wildcard.subscribe()
    }

    /// Get events from the replay buffer starting from a given sequence number.
    /// Returns events with seq > from_seq, up to limit events.
    pub async fn replay_from(&self, from_seq: u64, limit: usize) -> Vec<SequencedEvent> {
        let buffer = self.inner.replay_buffer.read().await;
        buffer
            .iter()
            .filter(|e| e.seq > from_seq)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get the oldest sequence number in the replay buffer.
    pub async fn oldest_sequence(&self) -> Option<u64> {
        let buffer = self.inner.replay_buffer.read().await;
        buffer.front().map(|e| e.seq)
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in Event Types
// ============================================================================

/// Session created event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreated {
    pub session_id: String,
    pub project_id: String,
    pub title: String,
}

impl Event for SessionCreated {
    fn event_type() -> &'static str {
        "session.created"
    }
}

/// Session updated event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdated {
    pub session_id: String,
}

impl Event for SessionUpdated {
    fn event_type() -> &'static str {
        "session.updated"
    }
}

/// Session deleted event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDeleted {
    pub session_id: String,
}

impl Event for SessionDeleted {
    fn event_type() -> &'static str {
        "session.deleted"
    }
}

/// Message updated event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUpdated {
    pub session_id: String,
    pub message_id: String,
}

impl Event for MessageUpdated {
    fn event_type() -> &'static str {
        "message.updated"
    }
}

/// Message removed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRemoved {
    pub session_id: String,
    pub message_id: String,
}

impl Event for MessageRemoved {
    fn event_type() -> &'static str {
        "message.removed"
    }
}

/// Message part updated event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartUpdated {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    /// For text parts, the delta (new text appended).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
}

impl Event for PartUpdated {
    fn event_type() -> &'static str {
        "message.part.updated"
    }
}

/// Message part removed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartRemoved {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
}

impl Event for PartRemoved {
    fn event_type() -> &'static str {
        "message.part.removed"
    }
}

/// Session status changed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub status: Status,
}

impl Event for SessionStatus {
    fn event_type() -> &'static str {
        "session.status"
    }
}

/// Session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Running,
    Pending,
    Compacting,
}

/// Session idle event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIdle {
    pub session_id: String,
}

impl Event for SessionIdle {
    fn event_type() -> &'static str {
        "session.idle"
    }
}

/// Session compacted event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCompacted {
    pub session_id: String,
    pub message_id: String,
}

impl Event for SessionCompacted {
    fn event_type() -> &'static str {
        "session.compacted"
    }
}

/// Todo list updated event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoUpdated {
    pub session_id: String,
    pub items: Vec<TodoItem>,
}

impl Event for TodoUpdated {
    fn event_type() -> &'static str {
        "todo.updated"
    }
}

/// A todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

/// Todo status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

/// Todo priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

/// Permission request event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub session_id: String,
    pub tool: String,
    pub action: String,
    /// Human-readable description of what the tool wants to do.
    pub description: String,
    /// Path involved (for file operations).
    pub path: Option<String>,
    /// Additional details (JSON).
    pub details: serde_json::Value,
}

impl Event for PermissionRequest {
    fn event_type() -> &'static str {
        "permission.request"
    }
}

/// Permission response event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub id: String,
    pub allowed: bool,
    pub remember: bool,
}

impl Event for PermissionResponse {
    fn event_type() -> &'static str {
        "permission.response"
    }
}

/// File edited event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEdited {
    pub file: String,
    pub session_id: String,
}

impl Event for FileEdited {
    fn event_type() -> &'static str {
        "file.edited"
    }
}

/// Project updated event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUpdated {
    pub project_id: String,
}

impl Event for ProjectUpdated {
    fn event_type() -> &'static str {
        "project.updated"
    }
}

/// Instance disposed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDisposed {
    pub directory: String,
}

impl Event for InstanceDisposed {
    fn event_type() -> &'static str {
        "instance.disposed"
    }
}

// ============================================================================
// Sandbox Event Types
// ============================================================================

/// Sandbox status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxState {
    /// Sandbox is not enabled/available.
    Disabled,
    /// Sandbox is stopped.
    Stopped,
    /// Sandbox is starting up.
    Starting,
    /// Sandbox is running and ready.
    Running,
    /// Sandbox encountered an error.
    Error,
}

impl Default for SandboxState {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Sandbox status changed event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStatusChanged {
    /// Current sandbox state.
    pub state: SandboxState,
    /// Sandbox runtime type (e.g., "docker", "lima", "passthrough").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_type: Option<String>,
    /// Optional error message if state is Error or Disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Event for SandboxStatusChanged {
    fn event_type() -> &'static str {
        "sandbox.status"
    }
}

/// Sandbox tool execution event.
///
/// Fired when a tool is executed inside the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxToolExecution {
    /// Session ID.
    pub session_id: String,
    /// Tool name (e.g., "bash", "read", "write").
    pub tool: String,
    /// Whether the execution was sandboxed.
    pub sandboxed: bool,
    /// Brief description of what was executed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Event for SandboxToolExecution {
    fn event_type() -> &'static str {
        "sandbox.tool_execution"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = Bus::new();

        let mut rx = bus.subscribe::<SessionCreated>().await;

        bus.publish(SessionCreated {
            session_id: "ses_123".to_string(),
            project_id: "proj_456".to_string(),
            title: "Test Session".to_string(),
        })
        .await;

        let event = rx.recv().await.unwrap();
        assert_eq!(event.session_id, "ses_123");
        assert_eq!(event.project_id, "proj_456");
    }

    #[tokio::test]
    async fn test_wildcard_subscribe() {
        let bus = Bus::new();

        let mut rx = bus.subscribe_all();

        bus.publish(SessionCreated {
            session_id: "ses_123".to_string(),
            project_id: "proj_456".to_string(),
            title: "Test".to_string(),
        })
        .await;

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type, "session.created");
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = Bus::new();

        let mut rx1 = bus.subscribe::<SessionCreated>().await;
        let mut rx2 = bus.subscribe::<SessionCreated>().await;

        bus.publish(SessionCreated {
            session_id: "ses_123".to_string(),
            project_id: "proj_456".to_string(),
            title: "Test".to_string(),
        })
        .await;

        assert_eq!(rx1.recv().await.unwrap().session_id, "ses_123");
        assert_eq!(rx2.recv().await.unwrap().session_id, "ses_123");
    }

    #[tokio::test]
    async fn test_sandbox_status_changed() {
        let bus = Bus::new();

        let mut rx = bus.subscribe::<SandboxStatusChanged>().await;

        bus.publish(SandboxStatusChanged {
            state: SandboxState::Running,
            runtime_type: Some("docker".to_string()),
            error: None,
        })
        .await;

        let event = rx.recv().await.unwrap();
        assert_eq!(event.state, SandboxState::Running);
        assert_eq!(event.runtime_type, Some("docker".to_string()));
        assert!(event.error.is_none());
    }

    #[tokio::test]
    async fn test_sandbox_tool_execution() {
        let bus = Bus::new();

        let mut rx = bus.subscribe::<SandboxToolExecution>().await;

        bus.publish(SandboxToolExecution {
            session_id: "ses_123".to_string(),
            tool: "bash".to_string(),
            sandboxed: true,
            description: Some("Running npm install".to_string()),
        })
        .await;

        let event = rx.recv().await.unwrap();
        assert_eq!(event.session_id, "ses_123");
        assert_eq!(event.tool, "bash");
        assert!(event.sandboxed);
        assert_eq!(event.description, Some("Running npm install".to_string()));
    }

    #[test]
    fn test_sandbox_state_default() {
        let state = SandboxState::default();
        assert_eq!(state, SandboxState::Disabled);
    }
}
