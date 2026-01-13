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
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection.
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
                                    message: format!("Invalid message: {}", e),
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
