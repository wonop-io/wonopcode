//! Memory panel widget for displaying observational memory state.
//!
//! Shows:
//! - Token budget bar with segments for system/observations/messages
//! - Observation list with priority indicators (🔴🟡🟢)
//! - Session statistics

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use wonopcode_tui_core::Theme;

/// Priority level for observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObservationPriority {
    /// 🔴 High - Critical context
    High,
    /// 🟡 Medium - Potentially relevant
    #[default]
    Medium,
    /// 🟢 Low - Informational only
    Low,
}

impl ObservationPriority {
    /// Get the emoji for this priority.
    pub fn emoji(&self) -> &'static str {
        match self {
            ObservationPriority::High => "🔴",
            ObservationPriority::Medium => "🟡",
            ObservationPriority::Low => "🟢",
        }
    }

    /// Get the character indicator for terminals without emoji support.
    pub fn indicator(&self) -> &'static str {
        match self {
            ObservationPriority::High => "●",
            ObservationPriority::Medium => "●",
            ObservationPriority::Low => "●",
        }
    }
}

/// An observation to display in the memory panel.
#[derive(Debug, Clone)]
pub struct DisplayObservation {
    /// Priority level.
    pub priority: ObservationPriority,
    /// Timestamp (HH:MM format).
    pub timestamp: String,
    /// Observation content.
    pub content: String,
    /// Whether this is a child observation (indented).
    pub is_child: bool,
    /// Whether this observation is pinned by the user.
    pub is_pinned: bool,
    /// Whether this observation is from a previous session.
    pub from_previous_session: bool,
}

impl DisplayObservation {
    /// Create a new observation.
    pub fn new(
        priority: ObservationPriority,
        timestamp: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            priority,
            timestamp: timestamp.into(),
            content: content.into(),
            is_child: false,
            is_pinned: false,
            from_previous_session: false,
        }
    }

    /// Mark as a child observation.
    pub fn as_child(mut self) -> Self {
        self.is_child = true;
        self
    }

    /// Mark as pinned.
    pub fn pinned(mut self) -> Self {
        self.is_pinned = true;
        self
    }

    /// Mark as from previous session.
    pub fn from_previous(mut self) -> Self {
        self.from_previous_session = true;
        self
    }
}

/// Token budget state for the budget bar.
#[derive(Debug, Clone, Default)]
pub struct TokenBudget {
    /// System prompt tokens (fixed).
    pub system_tokens: u32,
    /// Observation tokens.
    pub observation_tokens: u32,
    /// Unobserved message tokens.
    pub message_tokens: u32,
    /// Maximum context tokens.
    pub max_tokens: u32,
    /// Observer threshold (when observation triggers).
    pub observer_threshold: u32,
    /// Reflector threshold (when reflection triggers).
    pub reflector_threshold: u32,
}

impl TokenBudget {
    /// Create a new token budget.
    pub fn new(max_tokens: u32) -> Self {
        Self {
            system_tokens: 0,
            observation_tokens: 0,
            message_tokens: 0,
            max_tokens,
            observer_threshold: 30_000,
            reflector_threshold: 40_000,
        }
    }

    /// Calculate total tokens used.
    pub fn total(&self) -> u32 {
        self.system_tokens + self.observation_tokens + self.message_tokens
    }

    /// Calculate usage percentage.
    pub fn usage_percent(&self) -> f64 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        (self.total() as f64 / self.max_tokens as f64) * 100.0
    }

    /// Check if observer should trigger.
    pub fn should_observe(&self) -> bool {
        self.message_tokens >= self.observer_threshold
    }

    /// Check if reflector should trigger.
    pub fn should_reflect(&self) -> bool {
        self.observation_tokens >= self.reflector_threshold
    }
}

/// Session statistics for the memory panel.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Total observations created.
    pub total_observations: u32,
    /// Total reflections performed.
    pub total_reflections: u32,
    /// Average compression ratio.
    pub avg_compression_ratio: f32,
    /// Estimated cost savings from caching.
    pub estimated_savings: f64,
    /// Cache hit rate (0.0 to 1.0).
    pub cache_hit_rate: f32,
}

/// Memory event for inline chat indicators.
#[derive(Debug, Clone)]
pub enum MemoryEvent {
    /// Observer ran and compressed messages.
    Observed {
        messages_before: u32,
        observations_after: u32,
        compression_ratio: f32,
    },
    /// Reflector ran and restructured observations.
    Reflected {
        observations_before: u32,
        observations_after: u32,
        dropped: u32,
        merged: u32,
    },
}

impl MemoryEvent {
    /// Format the event as an inline indicator.
    pub fn format(&self) -> String {
        match self {
            MemoryEvent::Observed {
                messages_before,
                observations_after,
                compression_ratio,
            } => {
                format!(
                    "Memory updated · {messages_before} messages → {observations_after} observations · {compression_ratio:.1}× compression"
                )
            }
            MemoryEvent::Reflected {
                observations_before,
                observations_after,
                dropped,
                merged,
            } => {
                format!(
                    "Memory reflected · {observations_before} observations → {observations_after} observations · {dropped} dropped, {merged} merged"
                )
            }
        }
    }
}

/// Memory panel widget.
#[derive(Debug, Clone, Default)]
pub struct MemoryPanelWidget {
    /// Whether the panel is visible.
    visible: bool,
    /// Panel width when visible.
    width: u16,
    /// Token budget state.
    budget: TokenBudget,
    /// Observations to display.
    observations: Vec<DisplayObservation>,
    /// Date groups for observations.
    date_groups: Vec<String>,
    /// Session statistics.
    stats: MemoryStats,
    /// Scroll offset for observations list.
    scroll_offset: usize,
    /// Selected observation index (for pin/dismiss).
    selected_index: Option<usize>,
    /// Whether developer mode is enabled.
    developer_mode: bool,
    /// Cross-session banner message.
    cross_session_banner: Option<String>,
}

impl MemoryPanelWidget {
    /// Create a new memory panel widget.
    pub fn new() -> Self {
        Self {
            visible: false,
            width: 40,
            budget: TokenBudget::new(128_000),
            observations: Vec::new(),
            date_groups: Vec::new(),
            stats: MemoryStats::default(),
            scroll_offset: 0,
            selected_index: None,
            developer_mode: false,
            cross_session_banner: None,
        }
    }

    /// Get the width (0 if not visible).
    pub fn width(&self) -> u16 {
        if self.visible {
            self.width
        } else {
            0
        }
    }

    /// Check if visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Update token budget.
    pub fn update_budget(&mut self, budget: TokenBudget) {
        self.budget = budget;
    }

    /// Set observations to display.
    pub fn set_observations(&mut self, observations: Vec<DisplayObservation>) {
        self.observations = observations;
    }

    /// Add a date group header.
    pub fn add_date_group(&mut self, date: impl Into<String>) {
        self.date_groups.push(date.into());
    }

    /// Update session statistics.
    pub fn update_stats(&mut self, stats: MemoryStats) {
        self.stats = stats;
    }

    /// Set cross-session banner.
    pub fn set_cross_session_banner(&mut self, message: Option<String>) {
        self.cross_session_banner = message;
    }

    /// Enable/disable developer mode.
    pub fn set_developer_mode(&mut self, enabled: bool) {
        self.developer_mode = enabled;
    }

    /// Scroll up.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Scroll down.
    pub fn scroll_down(&mut self) {
        if self.scroll_offset < self.observations.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    /// Select previous observation.
    pub fn select_prev(&mut self) {
        match self.selected_index {
            Some(0) => self.selected_index = None,
            Some(i) => self.selected_index = Some(i - 1),
            None if !self.observations.is_empty() => {
                self.selected_index = Some(self.observations.len() - 1);
            }
            None => {}
        }
    }

    /// Select next observation.
    pub fn select_next(&mut self) {
        match self.selected_index {
            Some(i) if i < self.observations.len() - 1 => {
                self.selected_index = Some(i + 1);
            }
            Some(_) => self.selected_index = None,
            None if !self.observations.is_empty() => {
                self.selected_index = Some(0);
            }
            None => {}
        }
    }

    /// Get the selected observation index.
    pub fn selected(&self) -> Option<usize> {
        self.selected_index
    }

    /// Pin the selected observation.
    pub fn pin_selected(&mut self) {
        if let Some(idx) = self.selected_index {
            if let Some(obs) = self.observations.get_mut(idx) {
                obs.is_pinned = true;
                // Force high priority for pinned
                obs.priority = ObservationPriority::High;
            }
        }
    }

    /// Dismiss the selected observation.
    pub fn dismiss_selected(&mut self) -> Option<DisplayObservation> {
        if let Some(idx) = self.selected_index {
            if idx < self.observations.len() {
                let removed = self.observations.remove(idx);
                // Adjust selection
                if self.observations.is_empty() {
                    self.selected_index = None;
                } else if idx >= self.observations.len() {
                    self.selected_index = Some(self.observations.len() - 1);
                }
                return Some(removed);
            }
        }
        None
    }

    /// Render the memory panel.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        let block = Block::default()
            .title(" Memory ")
            .borders(Borders::ALL)
            .border_style(theme.border_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Layout: budget bar (3 lines) + observations (rest) + stats (2 lines)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Budget bar + banner
                Constraint::Min(5),    // Observations
                Constraint::Length(2), // Stats
            ])
            .split(inner);

        self.render_budget_bar(frame, chunks[0], theme);
        self.render_observations(frame, chunks[1], theme);
        self.render_stats(frame, chunks[2], theme);
    }

    /// Render the token budget bar.
    fn render_budget_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Cross-session banner if present
        let (banner_area, budget_area) = if self.cross_session_banner.is_some() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(area);
            (Some(chunks[0]), chunks[1])
        } else {
            (None, area)
        };

        // Render banner
        if let (Some(banner), Some(banner_rect)) = (&self.cross_session_banner, banner_area) {
            let banner_line = Line::from(Span::styled(
                format!("📂 {banner}"),
                theme.info_style(),
            ));
            frame.render_widget(Paragraph::new(banner_line), banner_rect);
        }

        // Calculate percentages
        let total = self.budget.max_tokens as f64;
        let system_pct = if total > 0.0 {
            self.budget.system_tokens as f64 / total
        } else {
            0.0
        };
        let obs_pct = if total > 0.0 {
            self.budget.observation_tokens as f64 / total
        } else {
            0.0
        };
        let msg_pct = if total > 0.0 {
            self.budget.message_tokens as f64 / total
        } else {
            0.0
        };

        // Budget label
        let budget_label = format!(
            "{}k / {}k ({:.0}%)",
            self.budget.total() / 1000,
            self.budget.max_tokens / 1000,
            self.budget.usage_percent()
        );

        // Determine color based on usage
        let gauge_color = if self.budget.should_reflect() {
            theme.error
        } else if self.budget.should_observe() {
            theme.warning
        } else {
            theme.success
        };

        let gauge = Gauge::default()
            .block(Block::default().title(" Tokens "))
            .gauge_style(Style::default().fg(gauge_color))
            .ratio((system_pct + obs_pct + msg_pct).min(1.0))
            .label(budget_label);

        frame.render_widget(gauge, budget_area);
    }

    /// Render the observations list.
    fn render_observations(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let mut lines: Vec<Line> = Vec::new();

        for (idx, obs) in self.observations.iter().enumerate() {
            let is_selected = self.selected_index == Some(idx);

            // Priority indicator with color
            let priority_style = match obs.priority {
                ObservationPriority::High => Style::default().fg(theme.error),
                ObservationPriority::Medium => Style::default().fg(theme.warning),
                ObservationPriority::Low => Style::default().fg(theme.success),
            };

            // Build the line
            let mut spans = Vec::new();

            // Indent for child observations
            if obs.is_child {
                spans.push(Span::raw("  "));
            }

            // Priority indicator
            spans.push(Span::styled(obs.priority.indicator(), priority_style));
            spans.push(Span::raw(" "));

            // Timestamp
            spans.push(Span::styled(&obs.timestamp, theme.dim_style()));
            spans.push(Span::raw(" "));

            // Pin indicator
            if obs.is_pinned {
                spans.push(Span::styled("📌 ", theme.info_style()));
            }

            // Content (truncated to fit)
            let max_content_len = area.width.saturating_sub(15) as usize;
            let content = if obs.content.len() > max_content_len {
                format!("{}...", &obs.content[..max_content_len.saturating_sub(3)])
            } else {
                obs.content.clone()
            };

            let content_style = if obs.from_previous_session {
                theme.dim_style()
            } else if is_selected {
                theme.text_style().add_modifier(Modifier::REVERSED)
            } else {
                theme.text_style()
            };

            spans.push(Span::styled(content, content_style));

            lines.push(Line::from(spans));
        }

        // Empty state
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No observations yet",
                theme.dim_style(),
            )));
        }

        let para = Paragraph::new(lines).scroll((self.scroll_offset as u16, 0));
        frame.render_widget(para, area);

        // Scrollbar
        if self.observations.len() > area.height as usize {
            let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
            let mut scrollbar_state = ScrollbarState::new(self.observations.len())
                .position(self.scroll_offset);
            frame.render_stateful_widget(
                scrollbar,
                area,
                &mut scrollbar_state,
            );
        }
    }

    /// Render session statistics.
    fn render_stats(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let stats_text = format!(
            "{} obs · {} ref · {:.1}× comp · ${:.2} saved",
            self.stats.total_observations,
            self.stats.total_reflections,
            self.stats.avg_compression_ratio,
            self.stats.estimated_savings,
        );

        let line = Line::from(Span::styled(stats_text, theme.dim_style()));
        frame.render_widget(Paragraph::new(line), area);
    }
}

/// Change type for reflection diff view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionChangeType {
    /// Observation kept unchanged.
    Kept,
    /// Observations merged together.
    Merged { from_count: usize },
    /// Observation dropped.
    Dropped { reason: String },
    /// New meta-observation added.
    MetaAdded,
}

/// A single change in the reflection diff.
#[derive(Debug, Clone)]
pub struct ReflectionDiffEntry {
    /// The observation content (after change for merged/meta, before for dropped).
    pub content: String,
    /// Type of change.
    pub change_type: ReflectionChangeType,
    /// Original observations (for merged).
    pub originals: Vec<String>,
}

impl ReflectionDiffEntry {
    /// Create a kept entry.
    pub fn kept(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            change_type: ReflectionChangeType::Kept,
            originals: Vec::new(),
        }
    }

    /// Create a merged entry.
    pub fn merged(content: impl Into<String>, originals: Vec<String>) -> Self {
        Self {
            content: content.into(),
            change_type: ReflectionChangeType::Merged {
                from_count: originals.len(),
            },
            originals,
        }
    }

    /// Create a dropped entry.
    pub fn dropped(content: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            change_type: ReflectionChangeType::Dropped {
                reason: reason.into(),
            },
            originals: Vec::new(),
        }
    }

    /// Create a meta-observation entry.
    pub fn meta_added(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            change_type: ReflectionChangeType::MetaAdded,
            originals: Vec::new(),
        }
    }
}

/// Widget for displaying reflection diff in developer mode.
#[derive(Debug, Clone, Default)]
pub struct ReflectionDiffWidget {
    /// Diff entries to display.
    entries: Vec<ReflectionDiffEntry>,
    /// Scroll offset.
    scroll_offset: usize,
    /// Selected entry for expansion.
    selected_index: Option<usize>,
}

impl ReflectionDiffWidget {
    /// Create a new reflection diff widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the diff entries.
    pub fn set_entries(&mut self, entries: Vec<ReflectionDiffEntry>) {
        self.entries = entries;
        self.scroll_offset = 0;
        self.selected_index = None;
    }

    /// Scroll up.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Scroll down.
    pub fn scroll_down(&mut self) {
        if self.scroll_offset < self.entries.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    /// Select an entry for expansion.
    pub fn select(&mut self, index: Option<usize>) {
        self.selected_index = index;
    }

    /// Toggle selection on current entry.
    pub fn toggle_selection(&mut self) {
        if let Some(idx) = self.selected_index {
            self.selected_index = None;
            // Re-select next if available
            if idx < self.entries.len() - 1 {
                self.selected_index = Some(idx);
            }
        } else if !self.entries.is_empty() {
            self.selected_index = Some(0);
        }
    }

    /// Get summary statistics.
    pub fn summary(&self) -> (usize, usize, usize, usize) {
        let mut kept = 0;
        let mut merged = 0;
        let mut dropped = 0;
        let mut meta = 0;

        for entry in &self.entries {
            match &entry.change_type {
                ReflectionChangeType::Kept => kept += 1,
                ReflectionChangeType::Merged { .. } => merged += 1,
                ReflectionChangeType::Dropped { .. } => dropped += 1,
                ReflectionChangeType::MetaAdded => meta += 1,
            }
        }

        (kept, merged, dropped, meta)
    }

    /// Render the reflection diff.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .title(" Reflection Diff ")
            .borders(Borders::ALL)
            .border_style(theme.border_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Summary header
        let (kept, merged, dropped, meta) = self.summary();
        let summary = Line::from(vec![
            Span::styled(format!("{kept} kept"), theme.dim_style()),
            Span::raw(" │ "),
            Span::styled(format!("{merged} merged"), theme.info_style()),
            Span::raw(" │ "),
            Span::styled(format!("{dropped} dropped"), theme.error_style()),
            Span::raw(" │ "),
            Span::styled(format!("{meta} new"), theme.success_style()),
        ]);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        frame.render_widget(Paragraph::new(summary), chunks[0]);

        // Diff entries
        let mut lines: Vec<Line> = Vec::new();

        for (idx, entry) in self.entries.iter().enumerate() {
            let is_selected = self.selected_index == Some(idx);

            let (prefix, style) = match &entry.change_type {
                ReflectionChangeType::Kept => ("  ", theme.dim_style()),
                ReflectionChangeType::Merged { .. } => {
                    ("+", theme.info_style().add_modifier(Modifier::BOLD))
                }
                ReflectionChangeType::Dropped { .. } => {
                    ("-", theme.error_style().add_modifier(Modifier::CROSSED_OUT))
                }
                ReflectionChangeType::MetaAdded => {
                    ("★", theme.success_style().add_modifier(Modifier::BOLD))
                }
            };

            let content_style = if is_selected {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            };

            // Truncate content
            let max_len = area.width.saturating_sub(5) as usize;
            let content = if entry.content.len() > max_len {
                format!("{}...", &entry.content[..max_len.saturating_sub(3)])
            } else {
                entry.content.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::raw(" "),
                Span::styled(content, content_style),
            ]));

            // Show merged-from annotations if selected
            if is_selected {
                if let ReflectionChangeType::Merged { .. } = &entry.change_type {
                    for orig in &entry.originals {
                        let truncated = if orig.len() > max_len - 4 {
                            format!("{}...", &orig[..max_len.saturating_sub(7)])
                        } else {
                            orig.clone()
                        };
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("← ", theme.dim_style()),
                            Span::styled(truncated, theme.muted_style()),
                        ]));
                    }
                }
                if let ReflectionChangeType::Dropped { reason } = &entry.change_type {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("reason: ", theme.dim_style()),
                        Span::styled(reason, theme.muted_style()),
                    ]));
                }
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled("No reflection diff", theme.dim_style())));
        }

        let para = Paragraph::new(lines).scroll((self.scroll_offset as u16, 0));
        frame.render_widget(para, chunks[1]);
    }
}

/// Widget for inline memory event indicators in the chat.
#[derive(Debug, Clone)]
pub struct MemoryEventIndicator {
    event: MemoryEvent,
}

impl MemoryEventIndicator {
    /// Create a new indicator.
    pub fn new(event: MemoryEvent) -> Self {
        Self { event }
    }

    /// Render as a divider line.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let text = self.event.format();

        // Create divider: ─── text ───
        let text_len = text.len();
        let remaining = area.width.saturating_sub(text_len as u16 + 2) as usize;
        let left_dashes = remaining / 2;
        let right_dashes = remaining - left_dashes;

        let line = Line::from(vec![
            Span::styled("─".repeat(left_dashes), theme.dim_style()),
            Span::styled(format!(" {text} "), theme.muted_style()),
            Span::styled("─".repeat(right_dashes), theme.dim_style()),
        ]);

        frame.render_widget(Paragraph::new(line), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ObservationPriority tests

    #[test]
    fn test_priority_emoji() {
        assert_eq!(ObservationPriority::High.emoji(), "🔴");
        assert_eq!(ObservationPriority::Medium.emoji(), "🟡");
        assert_eq!(ObservationPriority::Low.emoji(), "🟢");
    }

    #[test]
    fn test_priority_indicator() {
        assert_eq!(ObservationPriority::High.indicator(), "●");
        assert_eq!(ObservationPriority::Medium.indicator(), "●");
        assert_eq!(ObservationPriority::Low.indicator(), "●");
    }

    #[test]
    fn test_priority_default() {
        assert_eq!(ObservationPriority::default(), ObservationPriority::Medium);
    }

    // DisplayObservation tests

    #[test]
    fn test_observation_new() {
        let obs = DisplayObservation::new(ObservationPriority::High, "14:22", "Test content");
        assert_eq!(obs.priority, ObservationPriority::High);
        assert_eq!(obs.timestamp, "14:22");
        assert_eq!(obs.content, "Test content");
        assert!(!obs.is_child);
        assert!(!obs.is_pinned);
        assert!(!obs.from_previous_session);
    }

    #[test]
    fn test_observation_as_child() {
        let obs = DisplayObservation::new(ObservationPriority::Low, "14:22", "Child").as_child();
        assert!(obs.is_child);
    }

    #[test]
    fn test_observation_pinned() {
        let obs = DisplayObservation::new(ObservationPriority::Low, "14:22", "Pinned").pinned();
        assert!(obs.is_pinned);
    }

    #[test]
    fn test_observation_from_previous() {
        let obs =
            DisplayObservation::new(ObservationPriority::Low, "14:22", "Old").from_previous();
        assert!(obs.from_previous_session);
    }

    // TokenBudget tests

    #[test]
    fn test_token_budget_new() {
        let budget = TokenBudget::new(128_000);
        assert_eq!(budget.max_tokens, 128_000);
        assert_eq!(budget.total(), 0);
    }

    #[test]
    fn test_token_budget_total() {
        let budget = TokenBudget {
            system_tokens: 1000,
            observation_tokens: 5000,
            message_tokens: 3000,
            max_tokens: 100_000,
            observer_threshold: 30_000,
            reflector_threshold: 40_000,
        };
        assert_eq!(budget.total(), 9000);
    }

    #[test]
    fn test_token_budget_usage_percent() {
        let budget = TokenBudget {
            system_tokens: 10_000,
            observation_tokens: 0,
            message_tokens: 0,
            max_tokens: 100_000,
            observer_threshold: 30_000,
            reflector_threshold: 40_000,
        };
        assert!((budget.usage_percent() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_token_budget_should_observe() {
        let mut budget = TokenBudget::new(100_000);
        budget.message_tokens = 25_000;
        assert!(!budget.should_observe());

        budget.message_tokens = 30_000;
        assert!(budget.should_observe());
    }

    #[test]
    fn test_token_budget_should_reflect() {
        let mut budget = TokenBudget::new(100_000);
        budget.observation_tokens = 35_000;
        assert!(!budget.should_reflect());

        budget.observation_tokens = 40_000;
        assert!(budget.should_reflect());
    }

    // MemoryStats tests

    #[test]
    fn test_memory_stats_default() {
        let stats = MemoryStats::default();
        assert_eq!(stats.total_observations, 0);
        assert_eq!(stats.total_reflections, 0);
        assert!((stats.avg_compression_ratio - 0.0).abs() < 0.01);
    }

    // MemoryEvent tests

    #[test]
    fn test_memory_event_observed_format() {
        let event = MemoryEvent::Observed {
            messages_before: 14,
            observations_after: 5,
            compression_ratio: 7.3,
        };
        let formatted = event.format();
        assert!(formatted.contains("14 messages"));
        assert!(formatted.contains("5 observations"));
        assert!(formatted.contains("7.3×"));
    }

    #[test]
    fn test_memory_event_reflected_format() {
        let event = MemoryEvent::Reflected {
            observations_before: 42,
            observations_after: 28,
            dropped: 3,
            merged: 11,
        };
        let formatted = event.format();
        assert!(formatted.contains("42 observations → 28 observations"));
        assert!(formatted.contains("3 dropped"));
        assert!(formatted.contains("11 merged"));
    }

    // MemoryPanelWidget tests

    #[test]
    fn test_panel_new() {
        let panel = MemoryPanelWidget::new();
        assert!(!panel.is_visible());
        assert_eq!(panel.width(), 0);
    }

    #[test]
    fn test_panel_toggle() {
        let mut panel = MemoryPanelWidget::new();
        assert!(!panel.is_visible());

        panel.toggle();
        assert!(panel.is_visible());
        assert_eq!(panel.width(), 40);

        panel.toggle();
        assert!(!panel.is_visible());
    }

    #[test]
    fn test_panel_set_visible() {
        let mut panel = MemoryPanelWidget::new();
        panel.set_visible(true);
        assert!(panel.is_visible());

        panel.set_visible(false);
        assert!(!panel.is_visible());
    }

    #[test]
    fn test_panel_scroll() {
        let mut panel = MemoryPanelWidget::new();
        panel.set_observations(vec![
            DisplayObservation::new(ObservationPriority::High, "14:22", "Obs 1"),
            DisplayObservation::new(ObservationPriority::Medium, "14:23", "Obs 2"),
            DisplayObservation::new(ObservationPriority::Low, "14:24", "Obs 3"),
        ]);

        assert_eq!(panel.scroll_offset, 0);
        panel.scroll_down();
        assert_eq!(panel.scroll_offset, 1);
        panel.scroll_down();
        assert_eq!(panel.scroll_offset, 2);
        panel.scroll_down(); // Should not go past last
        assert_eq!(panel.scroll_offset, 2);

        panel.scroll_up();
        assert_eq!(panel.scroll_offset, 1);
        panel.scroll_up();
        assert_eq!(panel.scroll_offset, 0);
        panel.scroll_up(); // Should not go below 0
        assert_eq!(panel.scroll_offset, 0);
    }

    #[test]
    fn test_panel_selection() {
        let mut panel = MemoryPanelWidget::new();
        panel.set_observations(vec![
            DisplayObservation::new(ObservationPriority::High, "14:22", "Obs 1"),
            DisplayObservation::new(ObservationPriority::Medium, "14:23", "Obs 2"),
        ]);

        assert_eq!(panel.selected(), None);

        panel.select_next();
        assert_eq!(panel.selected(), Some(0));

        panel.select_next();
        assert_eq!(panel.selected(), Some(1));

        panel.select_next(); // Wraps to None
        assert_eq!(panel.selected(), None);

        panel.select_prev();
        assert_eq!(panel.selected(), Some(1));

        panel.select_prev();
        assert_eq!(panel.selected(), Some(0));

        panel.select_prev(); // Wraps to None
        assert_eq!(panel.selected(), None);
    }

    #[test]
    fn test_panel_pin_selected() {
        let mut panel = MemoryPanelWidget::new();
        panel.set_observations(vec![DisplayObservation::new(
            ObservationPriority::Low,
            "14:22",
            "Test",
        )]);

        panel.select_next();
        panel.pin_selected();

        assert!(panel.observations[0].is_pinned);
        assert_eq!(panel.observations[0].priority, ObservationPriority::High);
    }

    #[test]
    fn test_panel_dismiss_selected() {
        let mut panel = MemoryPanelWidget::new();
        panel.set_observations(vec![
            DisplayObservation::new(ObservationPriority::High, "14:22", "Obs 1"),
            DisplayObservation::new(ObservationPriority::Medium, "14:23", "Obs 2"),
        ]);

        panel.select_next(); // Select first
        let dismissed = panel.dismiss_selected();

        assert!(dismissed.is_some());
        assert_eq!(dismissed.unwrap().content, "Obs 1");
        assert_eq!(panel.observations.len(), 1);
        assert_eq!(panel.selected(), Some(0)); // Selection adjusted
    }

    #[test]
    fn test_panel_update_budget() {
        let mut panel = MemoryPanelWidget::new();
        let budget = TokenBudget {
            system_tokens: 5000,
            observation_tokens: 20000,
            message_tokens: 10000,
            max_tokens: 128_000,
            observer_threshold: 30_000,
            reflector_threshold: 40_000,
        };
        panel.update_budget(budget);

        assert_eq!(panel.budget.system_tokens, 5000);
        assert_eq!(panel.budget.total(), 35000);
    }

    #[test]
    fn test_panel_update_stats() {
        let mut panel = MemoryPanelWidget::new();
        let stats = MemoryStats {
            total_observations: 47,
            total_reflections: 3,
            avg_compression_ratio: 6.8,
            estimated_savings: 0.12,
            cache_hit_rate: 0.85,
        };
        panel.update_stats(stats);

        assert_eq!(panel.stats.total_observations, 47);
        assert_eq!(panel.stats.total_reflections, 3);
    }

    #[test]
    fn test_panel_cross_session_banner() {
        let mut panel = MemoryPanelWidget::new();
        assert!(panel.cross_session_banner.is_none());

        panel.set_cross_session_banner(Some("Loaded 23 observations from March 2nd".to_string()));
        assert!(panel.cross_session_banner.is_some());

        panel.set_cross_session_banner(None);
        assert!(panel.cross_session_banner.is_none());
    }

    #[test]
    fn test_panel_developer_mode() {
        let mut panel = MemoryPanelWidget::new();
        assert!(!panel.developer_mode);

        panel.set_developer_mode(true);
        assert!(panel.developer_mode);
    }

    // ReflectionChangeType tests

    #[test]
    fn test_reflection_change_type_equality() {
        assert_eq!(ReflectionChangeType::Kept, ReflectionChangeType::Kept);
        assert_eq!(
            ReflectionChangeType::Merged { from_count: 2 },
            ReflectionChangeType::Merged { from_count: 2 }
        );
        assert_ne!(
            ReflectionChangeType::Kept,
            ReflectionChangeType::MetaAdded
        );
    }

    // ReflectionDiffEntry tests

    #[test]
    fn test_diff_entry_kept() {
        let entry = ReflectionDiffEntry::kept("Test observation");
        assert_eq!(entry.content, "Test observation");
        assert_eq!(entry.change_type, ReflectionChangeType::Kept);
        assert!(entry.originals.is_empty());
    }

    #[test]
    fn test_diff_entry_merged() {
        let originals = vec!["Obs 1".to_string(), "Obs 2".to_string()];
        let entry = ReflectionDiffEntry::merged("Combined observation", originals);
        assert_eq!(entry.content, "Combined observation");
        assert_eq!(
            entry.change_type,
            ReflectionChangeType::Merged { from_count: 2 }
        );
        assert_eq!(entry.originals.len(), 2);
    }

    #[test]
    fn test_diff_entry_dropped() {
        let entry = ReflectionDiffEntry::dropped("Old observation", "superseded");
        assert_eq!(entry.content, "Old observation");
        assert_eq!(
            entry.change_type,
            ReflectionChangeType::Dropped {
                reason: "superseded".to_string()
            }
        );
    }

    #[test]
    fn test_diff_entry_meta_added() {
        let entry = ReflectionDiffEntry::meta_added("User prefers dark theme");
        assert_eq!(entry.content, "User prefers dark theme");
        assert_eq!(entry.change_type, ReflectionChangeType::MetaAdded);
    }

    // ReflectionDiffWidget tests

    #[test]
    fn test_diff_widget_new() {
        let widget = ReflectionDiffWidget::new();
        assert!(widget.entries.is_empty());
        assert_eq!(widget.scroll_offset, 0);
        assert_eq!(widget.selected_index, None);
    }

    #[test]
    fn test_diff_widget_set_entries() {
        let mut widget = ReflectionDiffWidget::new();
        widget.set_entries(vec![
            ReflectionDiffEntry::kept("Kept"),
            ReflectionDiffEntry::dropped("Dropped", "old"),
        ]);
        assert_eq!(widget.entries.len(), 2);
    }

    #[test]
    fn test_diff_widget_summary() {
        let mut widget = ReflectionDiffWidget::new();
        widget.set_entries(vec![
            ReflectionDiffEntry::kept("Kept 1"),
            ReflectionDiffEntry::kept("Kept 2"),
            ReflectionDiffEntry::merged("Merged", vec!["A".into(), "B".into()]),
            ReflectionDiffEntry::dropped("Dropped", "reason"),
            ReflectionDiffEntry::meta_added("Meta"),
        ]);

        let (kept, merged, dropped, meta) = widget.summary();
        assert_eq!(kept, 2);
        assert_eq!(merged, 1);
        assert_eq!(dropped, 1);
        assert_eq!(meta, 1);
    }

    #[test]
    fn test_diff_widget_scroll() {
        let mut widget = ReflectionDiffWidget::new();
        widget.set_entries(vec![
            ReflectionDiffEntry::kept("1"),
            ReflectionDiffEntry::kept("2"),
            ReflectionDiffEntry::kept("3"),
        ]);

        assert_eq!(widget.scroll_offset, 0);
        widget.scroll_down();
        assert_eq!(widget.scroll_offset, 1);
        widget.scroll_up();
        assert_eq!(widget.scroll_offset, 0);
    }

    #[test]
    fn test_diff_widget_select() {
        let mut widget = ReflectionDiffWidget::new();
        widget.set_entries(vec![ReflectionDiffEntry::kept("Test")]);

        assert_eq!(widget.selected_index, None);
        widget.select(Some(0));
        assert_eq!(widget.selected_index, Some(0));
        widget.select(None);
        assert_eq!(widget.selected_index, None);
    }
}
