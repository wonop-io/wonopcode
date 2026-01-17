//! Toast notification widget.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};

use wonopcode_tui_core::Theme;

/// Toast notification type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastType {
    Success,
    Error,
    Warning,
    Info,
}

/// A toast notification.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Toast type.
    pub toast_type: ToastType,
    /// Title.
    pub title: String,
    /// Message.
    pub message: Option<String>,
    /// When the toast was created.
    pub created_at: Instant,
    /// Duration to show.
    pub duration: Duration,
}

impl Toast {
    /// Create a new toast.
    pub fn new(toast_type: ToastType, title: impl Into<String>) -> Self {
        Self {
            toast_type,
            title: title.into(),
            message: None,
            created_at: Instant::now(),
            duration: Duration::from_secs(3),
        }
    }

    /// Add a message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Create a success toast.
    pub fn success(title: impl Into<String>) -> Self {
        Self::new(ToastType::Success, title)
    }

    /// Create an error toast.
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(ToastType::Error, title).with_duration(Duration::from_secs(5))
    }

    /// Create a warning toast.
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(ToastType::Warning, title)
    }

    /// Create an info toast.
    pub fn info(title: impl Into<String>) -> Self {
        Self::new(ToastType::Info, title)
    }

    /// Check if the toast has expired.
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    /// Get the progress (0.0 to 1.0) of the toast's lifetime.
    /// Used for fade-in/fade-out effects.
    pub fn progress(&self) -> f32 {
        let elapsed = self.created_at.elapsed().as_secs_f32();
        let duration = self.duration.as_secs_f32();
        (elapsed / duration).min(1.0)
    }

    /// Check if toast is in the fade-out phase (last 20% of duration).
    pub fn is_fading(&self) -> bool {
        self.progress() > 0.8
    }
}

/// Toast notification manager.
#[derive(Debug, Clone, Default)]
pub struct ToastManager {
    /// Active toasts.
    toasts: Vec<Toast>,
}

impl ToastManager {
    /// Create a new toast manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a toast.
    pub fn push(&mut self, toast: Toast) {
        self.toasts.push(toast);
    }

    /// Remove expired toasts.
    pub fn cleanup(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// Get active toasts.
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// Clear all toasts.
    pub fn clear(&mut self) {
        self.toasts.clear();
    }

    /// Render toasts in the top-right corner.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        self.cleanup();

        if self.toasts.is_empty() {
            return;
        }

        let toast_width = 40u16;
        let mut y = area.y + 1;

        for toast in &self.toasts {
            let height = if toast.message.is_some() { 4 } else { 3 };

            if y + height > area.height {
                break;
            }

            let toast_area = Rect::new(
                area.x + area.width.saturating_sub(toast_width + 2),
                y,
                toast_width,
                height,
            );

            self.render_toast(frame, toast_area, toast, theme);
            y += height + 1;
        }
    }

    fn render_toast(&self, frame: &mut Frame, area: Rect, toast: &Toast, theme: &Theme) {
        frame.render_widget(Clear, area);

        let (icon, border_color) = match toast.toast_type {
            ToastType::Success => ("✓", theme.success),
            ToastType::Error => ("✗", theme.error),
            ToastType::Warning => ("!", theme.warning),
            ToastType::Info => ("i", theme.info),
        };

        // Use dimmer style when fading out
        let text_style = if toast.is_fading() {
            theme.dim_style()
        } else {
            theme.text_style()
        };

        let border_style = if toast.is_fading() {
            ratatui::style::Style::default()
                .fg(border_color)
                .add_modifier(ratatui::style::Modifier::DIM)
        } else {
            ratatui::style::Style::default().fg(border_color)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let icon_style = if toast.is_fading() {
            ratatui::style::Style::default()
                .fg(border_color)
                .add_modifier(ratatui::style::Modifier::DIM)
        } else {
            ratatui::style::Style::default().fg(border_color)
        };

        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{icon} "), icon_style),
            Span::styled(&toast.title, text_style),
        ])];

        if let Some(msg) = &toast.message {
            let msg_style = if toast.is_fading() {
                theme.dim_style()
            } else {
                theme.muted_style()
            };
            lines.push(Line::from(Span::styled(msg, msg_style)));
        }

        let para = Paragraph::new(lines);
        frame.render_widget(para, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_type_clone() {
        let t = ToastType::Success;
        let cloned = t.clone();
        assert_eq!(cloned, ToastType::Success);
    }

    #[test]
    fn test_toast_type_debug() {
        assert!(format!("{:?}", ToastType::Success).contains("Success"));
        assert!(format!("{:?}", ToastType::Error).contains("Error"));
        assert!(format!("{:?}", ToastType::Warning).contains("Warning"));
        assert!(format!("{:?}", ToastType::Info).contains("Info"));
    }

    #[test]
    fn test_toast_type_equality() {
        assert_eq!(ToastType::Success, ToastType::Success);
        assert_ne!(ToastType::Success, ToastType::Error);
        assert_ne!(ToastType::Warning, ToastType::Info);
    }

    #[test]
    fn test_toast_new() {
        let toast = Toast::new(ToastType::Info, "Test");
        assert_eq!(toast.toast_type, ToastType::Info);
        assert_eq!(toast.title, "Test");
        assert!(toast.message.is_none());
        assert_eq!(toast.duration, Duration::from_secs(3));
    }

    #[test]
    fn test_toast_with_message() {
        let toast = Toast::new(ToastType::Info, "Title").with_message("Message");
        assert_eq!(toast.message, Some("Message".to_string()));
    }

    #[test]
    fn test_toast_with_duration() {
        let toast = Toast::new(ToastType::Info, "Test").with_duration(Duration::from_secs(10));
        assert_eq!(toast.duration, Duration::from_secs(10));
    }

    #[test]
    fn test_toast_success() {
        let toast = Toast::success("Success!");
        assert_eq!(toast.toast_type, ToastType::Success);
        assert_eq!(toast.title, "Success!");
    }

    #[test]
    fn test_toast_error() {
        let toast = Toast::error("Error!");
        assert_eq!(toast.toast_type, ToastType::Error);
        assert_eq!(toast.title, "Error!");
        assert_eq!(toast.duration, Duration::from_secs(5)); // Errors last longer
    }

    #[test]
    fn test_toast_warning() {
        let toast = Toast::warning("Warning!");
        assert_eq!(toast.toast_type, ToastType::Warning);
        assert_eq!(toast.title, "Warning!");
    }

    #[test]
    fn test_toast_info() {
        let toast = Toast::info("Info!");
        assert_eq!(toast.toast_type, ToastType::Info);
        assert_eq!(toast.title, "Info!");
    }

    #[test]
    fn test_toast_is_expired() {
        let toast = Toast::new(ToastType::Info, "Test").with_duration(Duration::from_millis(1));
        // Give time for it to expire
        std::thread::sleep(Duration::from_millis(5));
        assert!(toast.is_expired());
    }

    #[test]
    fn test_toast_not_expired() {
        let toast = Toast::new(ToastType::Info, "Test").with_duration(Duration::from_secs(100));
        assert!(!toast.is_expired());
    }

    #[test]
    fn test_toast_progress() {
        let toast = Toast::new(ToastType::Info, "Test");
        let progress = toast.progress();
        assert!(progress >= 0.0);
        assert!(progress <= 1.0);
    }

    #[test]
    fn test_toast_is_fading() {
        let toast = Toast::new(ToastType::Info, "Test").with_duration(Duration::from_secs(100));
        assert!(!toast.is_fading()); // Just created, shouldn't be fading
    }

    #[test]
    fn test_toast_clone() {
        let toast = Toast::new(ToastType::Success, "Test").with_message("Msg");
        let cloned = toast.clone();
        assert_eq!(cloned.toast_type, ToastType::Success);
        assert_eq!(cloned.title, "Test");
        assert_eq!(cloned.message, Some("Msg".to_string()));
    }

    #[test]
    fn test_toast_debug() {
        let toast = Toast::new(ToastType::Info, "Test");
        let debug = format!("{:?}", toast);
        assert!(debug.contains("Toast"));
    }

    // ToastManager tests

    #[test]
    fn test_toast_manager_new() {
        let manager = ToastManager::new();
        assert!(manager.toasts().is_empty());
    }

    #[test]
    fn test_toast_manager_default() {
        let manager = ToastManager::default();
        assert!(manager.toasts().is_empty());
    }

    #[test]
    fn test_toast_manager_push() {
        let mut manager = ToastManager::new();
        manager.push(Toast::info("Test"));
        assert_eq!(manager.toasts().len(), 1);
    }

    #[test]
    fn test_toast_manager_clear() {
        let mut manager = ToastManager::new();
        manager.push(Toast::info("Test1"));
        manager.push(Toast::info("Test2"));
        assert_eq!(manager.toasts().len(), 2);

        manager.clear();
        assert!(manager.toasts().is_empty());
    }

    #[test]
    fn test_toast_manager_cleanup() {
        let mut manager = ToastManager::new();
        manager.push(Toast::info("Long").with_duration(Duration::from_secs(100)));
        manager.push(Toast::info("Short").with_duration(Duration::from_millis(1)));

        std::thread::sleep(Duration::from_millis(5));
        manager.cleanup();

        assert_eq!(manager.toasts().len(), 1);
        assert_eq!(manager.toasts()[0].title, "Long");
    }

    #[test]
    fn test_toast_manager_clone() {
        let mut manager = ToastManager::new();
        manager.push(Toast::info("Test"));
        let cloned = manager.clone();
        assert_eq!(cloned.toasts().len(), 1);
    }

    #[test]
    fn test_toast_manager_debug() {
        let manager = ToastManager::new();
        let debug = format!("{:?}", manager);
        assert!(debug.contains("ToastManager"));
    }
}
