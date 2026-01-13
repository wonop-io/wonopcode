//! Server-Sent Events support.

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use wonopcode_core::Bus;

/// Create an SSE stream from the event bus.
/// Events now include sequence numbers for reliable delivery.
pub fn create_event_stream(bus: Bus) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut rx = bus.subscribe_all();

        loop {
            match rx.recv().await {
                Ok(sequenced_event) => {
                    // Include sequence number in the event ID for client tracking
                    let event = Event::default()
                        .event(&sequenced_event.event_type)
                        .id(sequenced_event.seq.to_string())
                        .data(serde_json::json!({
                            "seq": sequenced_event.seq,
                            "timestamp": sequenced_event.timestamp,
                            "payload": sequenced_event.payload
                        }).to_string());

                    yield Ok(event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE stream lagged by {} events", n);
                    // Continue receiving
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
