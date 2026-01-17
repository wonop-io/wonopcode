//! Animated spinner widget with simple dot animation.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::time::{Duration, Instant};

use wonopcode_tui_core::Theme;

/// Simple animated spinner with braille dots.
/// Displays as: `⠋ Thinking...` with animated spinner character.
#[derive(Debug, Clone)]
pub struct Spinner {
    /// Current animation frame.
    frame: usize,
    /// Last update time.
    last_update: Instant,
    /// Animation speed.
    speed: Duration,
    /// Whether active.
    active: bool,
    /// Label text.
    label: String,
    /// Animation frames (braille spinner).
    frames: Vec<&'static str>,
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Spinner {
    /// Create a new spinner.
    pub fn new() -> Self {
        Self {
            frame: 0,
            last_update: Instant::now(),
            speed: Duration::from_millis(80),
            active: false,
            label: String::new(),
            // Braille spinner animation frames
            frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
        }
    }

    /// Start the spinner.
    pub fn start(&mut self) {
        self.active = true;
        self.frame = 0;
        self.last_update = Instant::now();
    }

    /// Stop the spinner.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Set the label.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Whether active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Tick the animation.
    pub fn tick(&mut self) {
        if !self.active {
            return;
        }

        if self.last_update.elapsed() >= self.speed {
            self.frame = (self.frame + 1) % self.frames.len();
            self.last_update = Instant::now();
        }
    }

    /// Get the current spinner character.
    pub fn char(&self) -> &'static str {
        self.frames[self.frame]
    }

    /// Render the spinner.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.active {
            return;
        }

        let spinner_char = self.char();

        let spans = vec![
            Span::styled(spinner_char, theme.highlight_style()),
            Span::styled(" ", theme.text_style()),
            Span::styled(&self.label, theme.text_style()),
        ];

        let line = Line::from(spans);
        let para = Paragraph::new(line);
        frame.render_widget(para, area);
    }
}

/// Alias for backward compatibility.
pub type DotsSpinner = Spinner;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_new() {
        let spinner = Spinner::new();
        assert_eq!(spinner.frame, 0);
        assert!(!spinner.active);
        assert!(spinner.label.is_empty());
        assert_eq!(spinner.frames.len(), 10);
    }

    #[test]
    fn test_spinner_default() {
        let spinner = Spinner::default();
        assert_eq!(spinner.frame, 0);
        assert!(!spinner.active);
    }

    #[test]
    fn test_spinner_start_stop() {
        let mut spinner = Spinner::new();
        assert!(!spinner.is_active());

        spinner.start();
        assert!(spinner.is_active());
        assert_eq!(spinner.frame, 0);

        spinner.stop();
        assert!(!spinner.is_active());
    }

    #[test]
    fn test_spinner_set_label() {
        let mut spinner = Spinner::new();
        spinner.set_label("Loading...");
        assert_eq!(spinner.label, "Loading...");

        spinner.set_label(String::from("Processing"));
        assert_eq!(spinner.label, "Processing");
    }

    #[test]
    fn test_spinner_char() {
        let spinner = Spinner::new();
        assert_eq!(spinner.char(), "⠋");
    }

    #[test]
    fn test_spinner_tick_inactive() {
        let mut spinner = Spinner::new();
        let initial_frame = spinner.frame;
        spinner.tick();
        // Should not advance when inactive
        assert_eq!(spinner.frame, initial_frame);
    }

    #[test]
    fn test_spinner_tick_active() {
        let mut spinner = Spinner::new();
        spinner.start();
        // Fast-forward the last_update to force tick
        spinner.last_update = Instant::now() - Duration::from_millis(100);
        spinner.tick();
        assert_eq!(spinner.frame, 1);

        // Another tick
        spinner.last_update = Instant::now() - Duration::from_millis(100);
        spinner.tick();
        assert_eq!(spinner.frame, 2);
    }

    #[test]
    fn test_spinner_tick_wrap_around() {
        let mut spinner = Spinner::new();
        spinner.start();
        spinner.frame = 9; // Last frame
        spinner.last_update = Instant::now() - Duration::from_millis(100);
        spinner.tick();
        assert_eq!(spinner.frame, 0); // Should wrap to first frame
    }

    #[test]
    fn test_spinner_clone() {
        let mut spinner = Spinner::new();
        spinner.set_label("Test");
        spinner.start();
        let cloned = spinner.clone();
        assert_eq!(cloned.label, "Test");
        assert!(cloned.is_active());
    }

    #[test]
    fn test_spinner_debug() {
        let spinner = Spinner::new();
        let debug = format!("{:?}", spinner);
        assert!(debug.contains("Spinner"));
    }

    #[test]
    fn test_dots_spinner_alias() {
        let spinner: DotsSpinner = Spinner::new();
        assert!(!spinner.is_active());
    }
}
