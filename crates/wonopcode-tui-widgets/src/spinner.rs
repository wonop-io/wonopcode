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
