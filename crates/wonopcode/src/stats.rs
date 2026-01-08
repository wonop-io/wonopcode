//! Usage statistics command.
//!
//! Aggregates token usage, costs, and tool statistics across sessions.

use std::collections::HashMap;
use std::path::Path;
use wonopcode_core::{message::MessagePart, Instance};

/// Aggregated session statistics.
#[derive(Debug, Default)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub total_messages: usize,
    pub total_cost: f64,
    pub total_tokens: TokenStats,
    pub tool_usage: HashMap<String, usize>,
    pub date_range: DateRange,
    pub days: usize,
    pub cost_per_day: f64,
    pub tokens_per_session: f64,
    pub median_tokens_per_session: f64,
}

#[derive(Debug, Default)]
pub struct TokenStats {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenStats {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.reasoning
    }
}

#[derive(Debug, Default)]
pub struct DateRange {
    pub earliest: i64,
    pub latest: i64,
}

/// Aggregate statistics from sessions.
pub async fn aggregate_session_stats(
    cwd: &Path,
    days: Option<u32>,
    project_filter: Option<String>,
) -> anyhow::Result<SessionStats> {
    let instance = Instance::new(cwd).await?;
    let current_project_id = instance.project_id().await;

    let mut stats = SessionStats::default();
    let now = chrono::Utc::now().timestamp_millis();
    stats.date_range.earliest = now;
    stats.date_range.latest = 0;

    const MS_IN_DAY: i64 = 24 * 60 * 60 * 1000;

    // Calculate cutoff time
    let cutoff_time = if let Some(d) = days {
        if d == 0 {
            // Today only - and_hms_opt(0, 0, 0) is guaranteed to succeed for valid dates

            chrono::Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|dt| dt.and_utc().timestamp_millis())
                .unwrap_or(now)
        } else {
            now - (d as i64) * MS_IN_DAY
        }
    } else {
        0 // All time
    };

    // Get all sessions
    let sessions = instance.list_sessions().await;
    let project_id = instance.project_id().await;

    // Filter sessions
    let filtered_sessions: Vec<_> = sessions
        .into_iter()
        .filter(|s| {
            // Time filter
            if s.time.updated < cutoff_time {
                return false;
            }

            // Project filter
            if let Some(ref filter) = project_filter {
                if filter.is_empty() {
                    // Current project only
                    if s.project_id != current_project_id {
                        return false;
                    }
                } else if s.project_id != *filter {
                    return false;
                }
            }

            true
        })
        .collect();

    if filtered_sessions.is_empty() {
        instance.dispose().await;
        return Ok(stats);
    }

    stats.total_sessions = filtered_sessions.len();

    let mut session_total_tokens: Vec<u64> = Vec::new();
    let mut earliest_time = now;
    let mut latest_time: i64 = 0;

    // Process each session
    for session in &filtered_sessions {
        let messages = instance
            .session_repo()
            .messages(&project_id, &session.id, None)
            .await
            .unwrap_or_default();

        let mut session_tokens: u64 = 0;

        for msg_with_parts in &messages {
            stats.total_messages += 1;

            // Get cost and tokens from assistant messages
            if let wonopcode_core::message::Message::Assistant(assistant_msg) =
                &msg_with_parts.message
            {
                stats.total_cost += assistant_msg.cost;

                stats.total_tokens.input += assistant_msg.tokens.input as u64;
                stats.total_tokens.output += assistant_msg.tokens.output as u64;
                stats.total_tokens.reasoning += assistant_msg.tokens.reasoning as u64;
                stats.total_tokens.cache_read += assistant_msg.tokens.cache.read as u64;
                stats.total_tokens.cache_write += assistant_msg.tokens.cache.write as u64;

                session_tokens += (assistant_msg.tokens.input
                    + assistant_msg.tokens.output
                    + assistant_msg.tokens.reasoning) as u64;
            }

            // Count tool usage
            for part in &msg_with_parts.parts {
                if let MessagePart::Tool(tool_part) = part {
                    *stats.tool_usage.entry(tool_part.tool.clone()).or_insert(0) += 1;
                }
            }
        }

        session_total_tokens.push(session_tokens);
        earliest_time = earliest_time.min(session.time.created);
        latest_time = latest_time.max(session.time.updated);
    }

    // Calculate derived statistics
    stats.date_range.earliest = earliest_time;
    stats.date_range.latest = latest_time;
    stats.days = ((latest_time - earliest_time) / MS_IN_DAY).max(1) as usize;
    stats.cost_per_day = stats.total_cost / stats.days as f64;
    stats.tokens_per_session = if stats.total_sessions > 0 {
        stats.total_tokens.total() as f64 / stats.total_sessions as f64
    } else {
        0.0
    };

    // Calculate median
    session_total_tokens.sort();
    let len = session_total_tokens.len();
    stats.median_tokens_per_session = if len == 0 {
        0.0
    } else if len % 2 == 0 {
        (session_total_tokens[len / 2 - 1] + session_total_tokens[len / 2]) as f64 / 2.0
    } else {
        session_total_tokens[len / 2] as f64
    };

    instance.dispose().await;
    Ok(stats)
}

/// Display statistics in a nice format.
pub fn display_stats(stats: &SessionStats, tool_limit: Option<usize>) {
    const WIDTH: usize = 56;

    fn render_row(label: &str, value: &str) -> String {
        let available_width = WIDTH - 2;
        let padding_needed = available_width.saturating_sub(label.len() + value.len());
        format!("│ {}{}{} │", label, " ".repeat(padding_needed), value)
    }

    // Overview section
    println!("┌{}┐", "─".repeat(WIDTH));
    println!("│{:^WIDTH$}│", "OVERVIEW");
    println!("├{}┤", "─".repeat(WIDTH));
    println!(
        "{}",
        render_row("Sessions", &format_number(stats.total_sessions as u64))
    );
    println!(
        "{}",
        render_row("Messages", &format_number(stats.total_messages as u64))
    );
    println!("{}", render_row("Days", &stats.days.to_string()));
    println!("└{}┘", "─".repeat(WIDTH));
    println!();

    // Cost & Tokens section
    println!("┌{}┐", "─".repeat(WIDTH));
    println!("│{:^WIDTH$}│", "COST & TOKENS");
    println!("├{}┤", "─".repeat(WIDTH));

    let cost = if stats.total_cost.is_nan() {
        0.0
    } else {
        stats.total_cost
    };
    let cost_per_day = if stats.cost_per_day.is_nan() {
        0.0
    } else {
        stats.cost_per_day
    };
    let tokens_per_session = if stats.tokens_per_session.is_nan() {
        0.0
    } else {
        stats.tokens_per_session
    };
    let median_tokens = if stats.median_tokens_per_session.is_nan() {
        0.0
    } else {
        stats.median_tokens_per_session
    };

    println!("{}", render_row("Total Cost", &format!("${:.2}", cost)));
    println!(
        "{}",
        render_row("Avg Cost/Day", &format!("${:.2}", cost_per_day))
    );
    println!(
        "{}",
        render_row(
            "Avg Tokens/Session",
            &format_number(tokens_per_session.round() as u64)
        )
    );
    println!(
        "{}",
        render_row(
            "Median Tokens/Session",
            &format_number(median_tokens.round() as u64)
        )
    );
    println!(
        "{}",
        render_row("Input", &format_number(stats.total_tokens.input))
    );
    println!(
        "{}",
        render_row("Output", &format_number(stats.total_tokens.output))
    );
    if stats.total_tokens.reasoning > 0 {
        println!(
            "{}",
            render_row("Reasoning", &format_number(stats.total_tokens.reasoning))
        );
    }
    println!(
        "{}",
        render_row("Cache Read", &format_number(stats.total_tokens.cache_read))
    );
    println!(
        "{}",
        render_row(
            "Cache Write",
            &format_number(stats.total_tokens.cache_write)
        )
    );
    println!("└{}┘", "─".repeat(WIDTH));
    println!();

    // Tool Usage section
    if !stats.tool_usage.is_empty() {
        let mut sorted_tools: Vec<_> = stats.tool_usage.iter().collect();
        sorted_tools.sort_by(|a, b| b.1.cmp(a.1));

        let tools_to_display = if let Some(limit) = tool_limit {
            sorted_tools.into_iter().take(limit).collect::<Vec<_>>()
        } else {
            sorted_tools
        };

        let max_count = tools_to_display.iter().map(|(_, c)| **c).max().unwrap_or(1);
        let total_tool_usage: usize = stats.tool_usage.values().sum();

        println!("┌{}┐", "─".repeat(WIDTH));
        println!("│{:^WIDTH$}│", "TOOL USAGE");
        println!("├{}┤", "─".repeat(WIDTH));

        for (tool, count) in tools_to_display {
            let bar_length = (((*count as f64) / (max_count as f64)) * 20.0).round() as usize;
            let bar_length = bar_length.max(1);
            let bar = "█".repeat(bar_length);
            let percentage = (*count as f64 / total_tool_usage as f64) * 100.0;

            let tool_display = if tool.len() > 18 {
                format!("{}...", &tool[..15])
            } else {
                tool.clone()
            };

            let content = format!(
                " {:18} {:20} {:>3} ({:>4.1}%)",
                tool_display, bar, count, percentage
            );
            let padding = WIDTH.saturating_sub(content.len() + 1);
            println!("│{}{} │", content, " ".repeat(padding));
        }

        println!("└{}┘", "─".repeat(WIDTH));
    }
    println!();
}

/// Format a number with K/M suffixes.
fn format_number(num: u64) -> String {
    if num >= 1_000_000 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}K", num as f64 / 1_000.0)
    } else {
        num.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(500), "500");
        assert_eq!(format_number(1500), "1.5K");
        assert_eq!(format_number(1_500_000), "1.5M");
    }
}
