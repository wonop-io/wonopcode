//! Event handling for the TUI.

use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEvent,
};
use std::time::Duration;
use tokio::sync::mpsc;

/// Events that can occur in the TUI.
#[derive(Debug, Clone)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse event occurred.
    Mouse(MouseEvent),
    /// The terminal was resized.
    Resize(u16, u16),
    /// A tick event for periodic updates.
    Tick,
    /// Text was pasted (from bracketed paste mode).
    Paste(String),
    /// A message from the AI.
    Message(String),
    /// Status update (e.g., "thinking", "done").
    Status(String),
    /// Error occurred.
    Error(String),
}

/// Handles events from the terminal and other sources.
pub struct EventHandler {
    /// Sender for events.
    sender: mpsc::UnboundedSender<Event>,
    /// Receiver for events.
    receiver: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    /// Create a new event handler.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self { sender, receiver }
    }

    /// Get a sender for sending events.
    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.sender.clone()
    }

    /// Start the event loop.
    pub fn start(&self) -> EventLoopHandle {
        let sender = self.sender.clone();
        let handle = tokio::spawn(async move {
            // Use longer tick rate to reduce CPU usage on idle.
            // 250ms = 4 ticks/sec for animations, good enough for spinners.
            let tick_rate = Duration::from_millis(250);

            loop {
                // Check for crossterm events
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            if sender.send(Event::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Mouse(mouse)) => {
                            if sender.send(Event::Mouse(mouse)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Resize(w, h)) => {
                            if sender.send(Event::Resize(w, h)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Paste(text)) => {
                            tracing::info!("CrosstermEvent::Paste received: {} bytes", text.len());
                            if sender.send(Event::Paste(text)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::FocusGained) => {}
                        Ok(CrosstermEvent::FocusLost) => {}
                        Err(e) => {
                            tracing::warn!("Error reading event: {}", e);
                        }
                    }
                } else {
                    // Send tick event
                    if sender.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        EventLoopHandle { handle }
    }

    /// Receive the next event.
    pub async fn next(&mut self) -> Option<Event> {
        self.receiver.recv().await
    }
}

impl Default for EventHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to the event loop task.
pub struct EventLoopHandle {
    handle: tokio::task::JoinHandle<()>,
}

impl EventLoopHandle {
    /// Abort the event loop.
    pub fn abort(self) {
        self.handle.abort();
    }
}

/// Check if a key event is Ctrl+C.
pub fn is_quit(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Check if a key event is Escape.
pub fn is_escape(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc
}

/// Check if a key event is Enter.
pub fn is_enter(key: &KeyEvent) -> bool {
    key.code == KeyCode::Enter
}

/// Check if a key event is Backspace.
pub fn is_backspace(key: &KeyEvent) -> bool {
    key.code == KeyCode::Backspace
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_key_with_ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    // === Event enum tests ===

    #[test]
    fn test_event_key_debug() {
        let event = Event::Key(make_key(KeyCode::Enter));
        let debug = format!("{event:?}");
        assert!(debug.contains("Key"));
    }

    #[test]
    fn test_event_tick_debug() {
        let event = Event::Tick;
        let debug = format!("{event:?}");
        assert!(debug.contains("Tick"));
    }

    #[test]
    fn test_event_resize_debug() {
        let event = Event::Resize(80, 24);
        let debug = format!("{event:?}");
        assert!(debug.contains("Resize"));
    }

    #[test]
    fn test_event_paste_debug() {
        let event = Event::Paste("hello".to_string());
        let debug = format!("{event:?}");
        assert!(debug.contains("Paste"));
    }

    #[test]
    fn test_event_message_debug() {
        let event = Event::Message("msg".to_string());
        let debug = format!("{event:?}");
        assert!(debug.contains("Message"));
    }

    #[test]
    fn test_event_status_debug() {
        let event = Event::Status("thinking".to_string());
        let debug = format!("{event:?}");
        assert!(debug.contains("Status"));
    }

    #[test]
    fn test_event_error_debug() {
        let event = Event::Error("error".to_string());
        let debug = format!("{event:?}");
        assert!(debug.contains("Error"));
    }

    #[test]
    fn test_event_clone() {
        let event = Event::Tick;
        let cloned = event.clone();
        assert!(matches!(cloned, Event::Tick));
    }

    // === is_quit tests ===

    #[test]
    fn test_is_quit_ctrl_c() {
        let key = make_key_with_ctrl(KeyCode::Char('c'));
        assert!(is_quit(&key));
    }

    #[test]
    fn test_is_quit_just_c() {
        let key = make_key(KeyCode::Char('c'));
        assert!(!is_quit(&key));
    }

    #[test]
    fn test_is_quit_ctrl_other() {
        let key = make_key_with_ctrl(KeyCode::Char('a'));
        assert!(!is_quit(&key));
    }

    // === is_escape tests ===

    #[test]
    fn test_is_escape_esc_key() {
        let key = make_key(KeyCode::Esc);
        assert!(is_escape(&key));
    }

    #[test]
    fn test_is_escape_other_key() {
        let key = make_key(KeyCode::Enter);
        assert!(!is_escape(&key));
    }

    // === is_enter tests ===

    #[test]
    fn test_is_enter_enter_key() {
        let key = make_key(KeyCode::Enter);
        assert!(is_enter(&key));
    }

    #[test]
    fn test_is_enter_other_key() {
        let key = make_key(KeyCode::Esc);
        assert!(!is_enter(&key));
    }

    // === is_backspace tests ===

    #[test]
    fn test_is_backspace_backspace_key() {
        let key = make_key(KeyCode::Backspace);
        assert!(is_backspace(&key));
    }

    #[test]
    fn test_is_backspace_other_key() {
        let key = make_key(KeyCode::Delete);
        assert!(!is_backspace(&key));
    }

    // === EventHandler tests ===

    #[test]
    fn test_event_handler_new() {
        let handler = EventHandler::new();
        let _sender = handler.sender();
        // Test passes if no panic
    }

    #[test]
    fn test_event_handler_default() {
        let handler = EventHandler::default();
        let _sender = handler.sender();
        // Test passes if no panic
    }

    #[tokio::test]
    async fn test_event_handler_send_receive() {
        let mut handler = EventHandler::new();
        let sender = handler.sender();

        sender.send(Event::Tick).unwrap();

        let event = handler.next().await;
        assert!(matches!(event, Some(Event::Tick)));
    }

    #[tokio::test]
    async fn test_event_handler_multiple_events() {
        let mut handler = EventHandler::new();
        let sender = handler.sender();

        sender.send(Event::Tick).unwrap();
        sender.send(Event::Message("test".to_string())).unwrap();

        let event1 = handler.next().await;
        assert!(matches!(event1, Some(Event::Tick)));

        let event2 = handler.next().await;
        assert!(matches!(event2, Some(Event::Message(_))));
    }
}
