//! WebSocket support for real-time bidirectional communication.
//!
//! Provides a WebSocket endpoint that:
//! - Sends initial state on connection
//! - Streams events with sequence numbers
//! - Handles client messages (state requests, pings)

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use wonopcode_core::bus::SequencedEvent;

use crate::state::AppState;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_request_state_deserialize() {
        let json = r#"{"type": "request_state"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::RequestState));
    }

    #[test]
    fn test_client_message_ping_deserialize() {
        let json = r#"{"type": "ping"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::Ping));
    }

    #[test]
    fn test_client_message_subscribe_deserialize() {
        let json = r#"{"type": "subscribe", "events": ["session:started", "tool:invoked"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Subscribe { events } => {
                let events = events.unwrap();
                assert_eq!(events.len(), 2);
                assert_eq!(events[0], "session:started");
                assert_eq!(events[1], "tool:invoked");
            }
            _ => panic!("Expected Subscribe"),
        }
    }

    #[test]
    fn test_client_message_subscribe_no_events() {
        let json = r#"{"type": "subscribe"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Subscribe { events } => {
                assert!(events.is_none());
            }
            _ => panic!("Expected Subscribe"),
        }
    }

    #[test]
    fn test_client_message_unsubscribe_deserialize() {
        let json = r#"{"type": "unsubscribe", "events": ["session:ended"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Unsubscribe { events } => {
                let events = events.unwrap();
                assert_eq!(events.len(), 1);
                assert_eq!(events[0], "session:ended");
            }
            _ => panic!("Expected Unsubscribe"),
        }
    }

    #[test]
    fn test_server_message_pong_serialize() {
        let msg = ServerMessage::Pong;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"pong""#));
    }

    #[test]
    fn test_server_message_error_serialize() {
        let msg = ServerMessage::Error {
            message: "Test error".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""message":"Test error""#));
    }

    #[test]
    fn test_server_message_state_serialize() {
        let msg = ServerMessage::State {
            instance: InstanceState {
                directory: "/home/user".to_string(),
                project_id: "proj-123".to_string(),
                worktree: "/home/user/project".to_string(),
            },
            todos: vec![TodoState {
                id: "todo-1".to_string(),
                content: "Test task".to_string(),
                status: "pending".to_string(),
                priority: "high".to_string(),
            }],
            active_sessions: vec!["session-1".to_string()],
            events: EventSequenceState {
                current_seq: 42,
                oldest_seq: Some(1),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"state""#));
        assert!(json.contains(r#""directory":"/home/user""#));
        assert!(json.contains(r#""project_id":"proj-123""#));
        assert!(json.contains(r#""current_seq":42"#));
        assert!(json.contains(r#""oldest_seq":1"#));
    }

    #[test]
    fn test_instance_state_debug() {
        let state = InstanceState {
            directory: "/tmp".to_string(),
            project_id: "test".to_string(),
            worktree: "/tmp/work".to_string(),
        };
        let debug = format!("{:?}", state);
        assert!(debug.contains("InstanceState"));
        assert!(debug.contains("/tmp"));
    }

    #[test]
    fn test_todo_state_debug() {
        let state = TodoState {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
        };
        let debug = format!("{:?}", state);
        assert!(debug.contains("TodoState"));
        assert!(debug.contains("Task"));
    }

    #[test]
    fn test_event_sequence_state_debug() {
        let state = EventSequenceState {
            current_seq: 10,
            oldest_seq: Some(1),
        };
        let debug = format!("{:?}", state);
        assert!(debug.contains("EventSequenceState"));
        assert!(debug.contains("10"));
    }

    #[test]
    fn test_event_sequence_state_no_oldest() {
        let state = EventSequenceState {
            current_seq: 0,
            oldest_seq: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains(r#""current_seq":0"#));
        assert!(json.contains(r#""oldest_seq":null"#));
    }

    #[test]
    fn test_client_message_debug() {
        let msg = ClientMessage::RequestState;
        let debug = format!("{:?}", msg);
        assert!(debug.contains("RequestState"));
    }

    #[test]
    fn test_server_message_debug() {
        let msg = ServerMessage::Pong;
        let debug = format!("{:?}", msg);
        assert!(debug.contains("Pong"));
    }

    #[test]
    fn test_invalid_client_message_fails() {
        let json = r#"{"type": "invalid_type"}"#;
        let result = serde_json::from_str::<ClientMessage>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_server_message_event_serialize() {
        let event = SequencedEvent {
            seq: 1,
            event_type: "test".to_string(),
            payload: serde_json::json!({"key": "value"}),
            timestamp: 1234567890,
        };
        let msg = ServerMessage::Event(event);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"event""#));
        assert!(json.contains(r#""seq":1"#));
    }

    // === Additional serialization tests ===

    #[test]
    fn test_instance_state_serialize() {
        let state = InstanceState {
            directory: "/home/user".to_string(),
            project_id: "proj-1".to_string(),
            worktree: "/home/user/work".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"directory\":\"/home/user\""));
        assert!(json.contains("\"project_id\":\"proj-1\""));
        assert!(json.contains("\"worktree\":\"/home/user/work\""));
    }

    #[test]
    fn test_todo_state_serialize() {
        let state = TodoState {
            id: "todo-1".to_string(),
            content: "Fix bug".to_string(),
            status: "in_progress".to_string(),
            priority: "high".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"id\":\"todo-1\""));
        assert!(json.contains("\"content\":\"Fix bug\""));
        assert!(json.contains("\"status\":\"in_progress\""));
        assert!(json.contains("\"priority\":\"high\""));
    }

    #[test]
    fn test_event_sequence_state_serialize() {
        let state = EventSequenceState {
            current_seq: 100,
            oldest_seq: Some(5),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"current_seq\":100"));
        assert!(json.contains("\"oldest_seq\":5"));
    }

    #[test]
    fn test_server_message_state_full() {
        let msg = ServerMessage::State {
            instance: InstanceState {
                directory: "/dir".to_string(),
                project_id: "p1".to_string(),
                worktree: "/dir".to_string(),
            },
            todos: vec![
                TodoState {
                    id: "1".to_string(),
                    content: "Task 1".to_string(),
                    status: "pending".to_string(),
                    priority: "low".to_string(),
                },
                TodoState {
                    id: "2".to_string(),
                    content: "Task 2".to_string(),
                    status: "completed".to_string(),
                    priority: "medium".to_string(),
                },
            ],
            active_sessions: vec!["s1".to_string(), "s2".to_string()],
            events: EventSequenceState {
                current_seq: 50,
                oldest_seq: None,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"state\""));
        assert!(json.contains("Task 1"));
        assert!(json.contains("Task 2"));
    }

    // === ClientMessage edge cases ===

    #[test]
    fn test_client_message_subscribe_empty_events() {
        let json = r#"{"type": "subscribe", "events": []}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Subscribe { events } => {
                assert!(events.unwrap().is_empty());
            }
            _ => panic!("Expected Subscribe"),
        }
    }

    #[test]
    fn test_client_message_unsubscribe_no_events() {
        let json = r#"{"type": "unsubscribe"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Unsubscribe { events } => {
                assert!(events.is_none());
            }
            _ => panic!("Expected Unsubscribe"),
        }
    }

    // === ServerMessage Clone ===

    #[test]
    fn test_sequenced_event_clone() {
        let event = SequencedEvent {
            seq: 42,
            event_type: "test".to_string(),
            payload: serde_json::json!({"data": 123}),
            timestamp: 1000,
        };
        let cloned = event.clone();
        assert_eq!(cloned.seq, 42);
        assert_eq!(cloned.event_type, "test");
    }

    // === Debug format tests ===

    #[test]
    fn test_instance_state_clone() {
        let state = InstanceState {
            directory: "/test".to_string(),
            project_id: "p".to_string(),
            worktree: "/test".to_string(),
        };
        // InstanceState only has Debug, not Clone, so test debug format
        let debug = format!("{:?}", state);
        assert!(debug.contains("/test"));
    }

    #[test]
    fn test_todo_state_clone() {
        let state = TodoState {
            id: "1".to_string(),
            content: "Task".to_string(),
            status: "pending".to_string(),
            priority: "low".to_string(),
        };
        let debug = format!("{:?}", state);
        assert!(debug.contains("pending"));
    }
}

/// Message from client to server.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Request current state.
    RequestState,
    /// Ping for keepalive.
    Ping,
    /// Subscribe to specific event types (optional filtering).
    Subscribe { events: Option<Vec<String>> },
    /// Unsubscribe from specific event types.
    Unsubscribe { events: Option<Vec<String>> },
}

/// Message from server to client.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Initial state on connection.
    State {
        instance: InstanceState,
        todos: Vec<TodoState>,
        active_sessions: Vec<String>,
        events: EventSequenceState,
    },
    /// An event from the bus.
    Event(SequencedEvent),
    /// Pong response to ping.
    Pong,
    /// Error message.
    Error { message: String },
}

#[derive(Debug, Serialize)]
pub struct InstanceState {
    pub directory: String,
    pub project_id: String,
    pub worktree: String,
}

#[derive(Debug, Serialize)]
pub struct TodoState {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Serialize)]
pub struct EventSequenceState {
    pub current_seq: u64,
    pub oldest_seq: Option<u64>,
}

/// WebSocket upgrade handler.
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection.
#[allow(clippy::cognitive_complexity)]
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial state
    let initial_state = build_state(&state).await;
    if let Ok(msg) = serde_json::to_string(&initial_state) {
        if sender.send(Message::Text(msg.into())).await.is_err() {
            return;
        }
    }

    // Subscribe to events
    let mut event_rx = state.bus.subscribe_all();

    // Handle messages and events concurrently
    loop {
        tokio::select! {
            // Handle incoming client messages
            Some(msg) = receiver.next() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(client_msg) => {
                                match client_msg {
                                    ClientMessage::RequestState => {
                                        let state_msg = build_state(&state).await;
                                        if let Ok(json) = serde_json::to_string(&state_msg) {
                                            if sender.send(Message::Text(json.into())).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                    ClientMessage::Ping => {
                                        let pong = ServerMessage::Pong;
                                        if let Ok(json) = serde_json::to_string(&pong) {
                                            if sender.send(Message::Text(json.into())).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                    ClientMessage::Subscribe { events: _ } => {
                                        // TODO: Implement event filtering
                                        debug!("Client subscribed to events");
                                    }
                                    ClientMessage::Unsubscribe { events: _ } => {
                                        // TODO: Implement event filtering
                                        debug!("Client unsubscribed from events");
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Invalid client message: {}", e);
                                let error = ServerMessage::Error {
                                    message: format!("Invalid message: {e}"),
                                };
                                if let Ok(json) = serde_json::to_string(&error) {
                                    let _ = sender.send(Message::Text(json.into())).await;
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        break;
                    }
                    Err(e) => {
                        warn!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Forward events from the bus
            result = event_rx.recv() => {
                match result {
                    Ok(event) => {
                        let event_msg = ServerMessage::Event(event);
                        if let Ok(json) = serde_json::to_string(&event_msg) {
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {} events", n);
                        // Continue receiving
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        }
    }

    debug!("WebSocket connection closed");
}

/// Build the full state message.
async fn build_state(state: &AppState) -> ServerMessage {
    let instance = state.instance.read().await;
    let project_id = instance.project_id().await;
    let worktree = instance.worktree().await;

    let todos = state.get_todos().await;
    let todos_state: Vec<TodoState> = todos
        .iter()
        .map(|t| TodoState {
            id: t.id.clone(),
            content: t.content.clone(),
            status: format!("{:?}", t.status).to_lowercase(),
            priority: format!("{:?}", t.priority).to_lowercase(),
        })
        .collect();

    let runners = state.session_runners.read().await;
    let active_sessions: Vec<String> = runners.keys().cloned().collect();

    let current_seq = state.bus.current_sequence();
    let oldest_seq = state.bus.oldest_sequence().await;

    ServerMessage::State {
        instance: InstanceState {
            directory: instance.directory().display().to_string(),
            project_id,
            worktree: worktree.display().to_string(),
        },
        todos: todos_state,
        active_sessions,
        events: EventSequenceState {
            current_seq,
            oldest_seq,
        },
    }
}
