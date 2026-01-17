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
