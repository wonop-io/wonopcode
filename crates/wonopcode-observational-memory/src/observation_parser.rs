//! Observation format parser.
//!
//! Parses the human-readable observation log format used by the Observer agent:
//!
//! ```text
//! Date: 2026-03-04
//! - 🔴 14:22 User is building a Next.js app with Supabase auth
//!   - 🔴 14:22 App uses server components with client-side hydration
//!   - 🟡 14:25 User asked about middleware configuration
//! - 🟡 14:35 Agent ran test suite, 3 failures
//!   - 🟢 14:35 Failures relate to missing session cookie
//! ```

use crate::observation::{DateGroup, Observation, ObservationCategory, Priority};
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};

/// Error type for observation parsing.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parse error at line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parser for the observation log format.
pub struct ObservationParser {
    /// Default confidence for parsed observations.
    default_confidence: f32,
    /// Whether to be lenient with malformed input.
    lenient: bool,
}

impl Default for ObservationParser {
    fn default() -> Self {
        Self {
            default_confidence: 0.8,
            lenient: true,
        }
    }
}

impl ObservationParser {
    /// Create a new parser with default settings.
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Set the default confidence for parsed observations.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.default_confidence = confidence;
        self
    }
    
    /// Set whether to be lenient with malformed input.
    pub fn lenient(mut self, lenient: bool) -> Self {
        self.lenient = lenient;
        self
    }
    
    /// Parse a complete observation log into date groups.
    pub fn parse(&self, input: &str) -> Result<Vec<DateGroup>, ParseError> {
        let mut groups = Vec::new();
        let mut current_group: Option<DateGroup> = None;
        let mut current_parent: Option<Observation> = None;
        
        for (line_num, line) in input.lines().enumerate() {
            let trimmed = line.trim();
            
            // Skip empty lines
            if trimmed.is_empty() {
                continue;
            }
            
            // Parse date header
            if let Some(date) = self.parse_date_header(trimmed) {
                // Save current parent to current group if exists
                if let Some(parent) = current_parent.take() {
                    if let Some(ref mut group) = current_group {
                        group.add(parent);
                    }
                }
                
                // Save current group if exists
                if let Some(group) = current_group.take() {
                    if !group.observations.is_empty() {
                        groups.push(group);
                    }
                }
                
                current_group = Some(DateGroup::new(date));
                continue;
            }
            
            // Parse observation line (pass original line to preserve indentation)
            if let Some((indent, obs)) = self.parse_observation_line(line, line_num)? {
                if indent == 0 {
                    // Top-level observation
                    if let Some(parent) = current_parent.take() {
                        if let Some(ref mut group) = current_group {
                            group.add(parent);
                        }
                    }
                    current_parent = Some(obs);
                } else {
                    // Child observation
                    if let Some(ref mut parent) = current_parent {
                        parent.children.push(obs);
                    } else if self.lenient {
                        // No parent, treat as top-level
                        current_parent = Some(obs);
                    } else {
                        return Err(ParseError {
                            line: line_num + 1,
                            message: "Child observation without parent".to_string(),
                        });
                    }
                }
            }
        }
        
        // Save final parent and group
        if let Some(parent) = current_parent {
            if let Some(ref mut group) = current_group {
                group.add(parent);
            }
        }
        if let Some(group) = current_group {
            if !group.observations.is_empty() {
                groups.push(group);
            }
        }
        
        Ok(groups)
    }
    
    /// Parse a single observation log and return flattened observations.
    pub fn parse_flat(&self, input: &str) -> Result<Vec<Observation>, ParseError> {
        let groups = self.parse(input)?;
        Ok(groups
            .into_iter()
            .flat_map(|g| g.observations)
            .collect())
    }
    
    /// Parse a date header line (e.g., "Date: 2026-03-04").
    fn parse_date_header(&self, line: &str) -> Option<DateTime<Utc>> {
        let line = line.trim();
        
        // Try "Date: YYYY-MM-DD" format
        if let Some(date_str) = line.strip_prefix("Date:").map(|s| s.trim()) {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
            }
        }
        
        // Try "## YYYY-MM-DD" format
        if let Some(date_str) = line.strip_prefix("##").map(|s| s.trim()) {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
            }
        }
        
        None
    }
    
    /// Parse an observation line.
    /// Returns (indent_level, Observation) or None if not an observation line.
    fn parse_observation_line(
        &self,
        line: &str,
        line_num: usize,
    ) -> Result<Option<(usize, Observation)>, ParseError> {
        // Determine indent level (count leading spaces, then "- ")
        let original_line = line;
        let trimmed = line.trim();
        
        // Must start with "- " after trimming
        if !trimmed.starts_with("- ") {
            return Ok(None);
        }
        
        // Calculate indent: count leading spaces before "-"
        let indent = original_line
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();
        let indent_level = if indent >= 2 { 1 } else { 0 };
        
        // Remove the "- " prefix
        let content = &trimmed[2..];
        
        // Parse priority emoji
        let (priority, content) = if content.starts_with("🔴") {
            (Priority::High, content[4..].trim())
        } else if content.starts_with("🟡") {
            (Priority::Medium, content[4..].trim())
        } else if content.starts_with("🟢") {
            (Priority::Low, content[4..].trim())
        } else if self.lenient {
            (Priority::Medium, content)
        } else {
            return Err(ParseError {
                line: line_num + 1,
                message: format!("Missing priority emoji in: {}", content),
            });
        };
        
        // Try to parse time (HH:MM format at start)
        let (time, content) = self.parse_time_prefix(content);
        
        // Determine category from content (heuristic)
        let category = self.infer_category(content);
        
        let mut obs = Observation::new(priority, category, content, self.default_confidence);
        
        // If we parsed a time, update the observation_date
        if let Some(time) = time {
            let date = obs.observation_date.date_naive();
            let datetime = date.and_time(time);
            obs.observation_date = Utc.from_utc_datetime(&datetime);
        }
        
        Ok(Some((indent_level, obs)))
    }
    
    /// Try to parse a time prefix (HH:MM) from the content.
    fn parse_time_prefix<'a>(&self, content: &'a str) -> (Option<NaiveTime>, &'a str) {
        // Look for HH:MM pattern at start
        if content.len() >= 5 {
            let potential_time = &content[..5];
            if let Ok(time) = NaiveTime::parse_from_str(potential_time, "%H:%M") {
                // Skip the time and any following whitespace
                let rest = content[5..].trim_start();
                return (Some(time), rest);
            }
        }
        (None, content)
    }
    
    /// Infer the observation category from content.
    fn infer_category(&self, content: &str) -> ObservationCategory {
        let lower = content.to_lowercase();
        
        // Technical indicators
        if lower.contains("file")
            || lower.contains("path")
            || lower.contains("config")
            || lower.contains("error")
            || lower.contains("warning")
            || lower.contains("test")
            || lower.contains("build")
            || lower.contains("compile")
        {
            return ObservationCategory::Technical;
        }
        
        // Decision indicators
        if lower.contains("chose")
            || lower.contains("decided")
            || lower.contains("selected")
            || lower.contains("using")
            || lower.contains("will use")
        {
            return ObservationCategory::Decision;
        }
        
        // Preference indicators
        if lower.contains("prefer")
            || lower.contains("like")
            || lower.contains("want")
            || lower.contains("should")
        {
            return ObservationCategory::Preference;
        }
        
        // Action indicators
        if lower.contains("agent")
            || lower.contains("ran")
            || lower.contains("executed")
            || lower.contains("created")
            || lower.contains("modified")
            || lower.contains("applied")
        {
            return ObservationCategory::Action;
        }
        
        // Default to Context
        ObservationCategory::Context
    }
}

/// Format a date group to the observation log format.
pub fn format_date_group(group: &DateGroup) -> String {
    let mut output = String::new();
    
    output.push_str(&format!("Date: {}\n", group.date.format("%Y-%m-%d")));
    
    for obs in &group.observations {
        format_observation_recursive(&mut output, obs, 0);
    }
    
    output
}

/// Format multiple date groups to the observation log format.
pub fn format_observations(groups: &[DateGroup]) -> String {
    groups
        .iter()
        .map(format_date_group)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_observation_recursive(output: &mut String, obs: &Observation, depth: usize) {
    let indent = "  ".repeat(depth);
    let time = obs.observation_date.format("%H:%M");
    
    output.push_str(&format!(
        "{}- {} {} {}\n",
        indent,
        obs.priority.emoji(),
        time,
        obs.content
    ));
    
    for child in &obs.children {
        format_observation_recursive(output, child, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_simple_observation() {
        let input = r#"
Date: 2026-03-04
- 🔴 14:22 User is building a Next.js app
"#;
        
        let parser = ObservationParser::new();
        let groups = parser.parse(input).unwrap();
        
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].observations.len(), 1);
        
        let obs = &groups[0].observations[0];
        assert_eq!(obs.priority, Priority::High);
        assert_eq!(obs.content, "User is building a Next.js app");
    }
    
    #[test]
    fn test_parse_hierarchical_observations() {
        let input = r#"
Date: 2026-03-04
- 🔴 14:22 User is building a Next.js app
  - 🔴 14:22 App uses server components
  - 🟡 14:25 User asked about middleware
- 🟡 14:35 Agent ran test suite
  - 🟢 14:35 Failures relate to missing cookie
"#;
        
        let parser = ObservationParser::new();
        let groups = parser.parse(input).unwrap();
        
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].observations.len(), 2);
        
        let obs1 = &groups[0].observations[0];
        assert_eq!(obs1.children.len(), 2);
        assert_eq!(obs1.children[0].priority, Priority::High);
        assert_eq!(obs1.children[1].priority, Priority::Medium);
        
        let obs2 = &groups[0].observations[1];
        assert_eq!(obs2.children.len(), 1);
        assert_eq!(obs2.children[0].priority, Priority::Low);
    }
    
    #[test]
    fn test_parse_multiple_dates() {
        let input = r#"
Date: 2026-03-03
- 🔴 10:00 Started the project

Date: 2026-03-04
- 🟡 14:22 Continued work
"#;
        
        let parser = ObservationParser::new();
        let groups = parser.parse(input).unwrap();
        
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].observations.len(), 1);
        assert_eq!(groups[1].observations.len(), 1);
    }
    
    #[test]
    fn test_format_roundtrip() {
        let mut group = DateGroup::new(Utc::now());
        
        let mut parent = Observation::new(
            Priority::High,
            ObservationCategory::Decision,
            "User chose JWT for auth",
            0.9,
        );
        parent.children.push(Observation::new(
            Priority::Medium,
            ObservationCategory::Technical,
            "Using jose library",
            0.8,
        ));
        group.add(parent);
        
        let formatted = format_date_group(&group);
        
        let parser = ObservationParser::new();
        let parsed = parser.parse(&formatted).unwrap();
        
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].observations.len(), 1);
        assert_eq!(parsed[0].observations[0].children.len(), 1);
    }
    
    #[test]
    fn test_category_inference() {
        let parser = ObservationParser::new();
        
        assert_eq!(
            parser.infer_category("User prefers tabs over spaces"),
            ObservationCategory::Preference
        );
        // "Agent ran" triggers Action, but "test" triggers Technical first
        // so use a phrase without "test"
        assert_eq!(
            parser.infer_category("Agent executed the command successfully"),
            ObservationCategory::Action
        );
        assert_eq!(
            parser.infer_category("Build error in main.rs line 42"),
            ObservationCategory::Technical
        );
        assert_eq!(
            parser.infer_category("User decided to use PostgreSQL"),
            ObservationCategory::Decision
        );
    }
    
    #[test]
    fn test_lenient_parsing() {
        // Missing priority emoji - lenient mode should handle it
        let input = "Date: 2026-03-04\n- 14:22 User message without emoji";
        
        let parser = ObservationParser::new().lenient(true);
        let result = parser.parse(input);
        assert!(result.is_ok());
        
        let groups = result.unwrap();
        assert_eq!(groups[0].observations[0].priority, Priority::Medium);
    }
    
    #[test]
    fn test_parse_empty_input() {
        let parser = ObservationParser::new();
        let groups = parser.parse("").unwrap();
        assert!(groups.is_empty());
    }
    
    #[test]
    fn test_parse_whitespace_only() {
        let parser = ObservationParser::new();
        let groups = parser.parse("   \n\n   \n").unwrap();
        assert!(groups.is_empty());
    }
    
    #[test]
    fn test_parse_flat() {
        let input = r#"
Date: 2026-03-04
- 🔴 14:22 First observation
  - 🟡 14:23 Child observation
- 🟢 14:30 Second observation
"#;
        
        let parser = ObservationParser::new();
        let flat = parser.parse_flat(input).unwrap();
        
        // Should get 2 top-level observations (children are nested, not flat)
        assert_eq!(flat.len(), 2);
    }
    
    #[test]
    fn test_parse_alternate_date_format() {
        let input = "## 2026-03-04\n- 🔴 Test observation";
        
        let parser = ObservationParser::new();
        let groups = parser.parse(input).unwrap();
        
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].observations.len(), 1);
    }
    
    #[test]
    fn test_parse_without_time() {
        let input = "Date: 2026-03-04\n- 🔴 Observation without timestamp";
        
        let parser = ObservationParser::new();
        let groups = parser.parse(input).unwrap();
        
        assert_eq!(groups[0].observations[0].content, "Observation without timestamp");
    }
    
    #[test]
    fn test_confidence_setting() {
        let input = "Date: 2026-03-04\n- 🔴 Test";
        
        let parser = ObservationParser::new().with_confidence(0.5);
        let groups = parser.parse(input).unwrap();
        
        assert!((groups[0].observations[0].confidence - 0.5).abs() < 0.01);
    }
    
    #[test]
    fn test_strict_mode_error() {
        let input = "Date: 2026-03-04\n- Missing emoji";
        
        let parser = ObservationParser::new().lenient(false);
        let result = parser.parse(input);
        
        assert!(result.is_err());
    }
    
    #[test]
    fn test_format_preserves_priorities() {
        let mut group = DateGroup::new(Utc::now());
        group.add(Observation::new(Priority::High, ObservationCategory::Decision, "High priority", 0.9));
        group.add(Observation::new(Priority::Medium, ObservationCategory::Fact, "Medium priority", 0.8));
        group.add(Observation::new(Priority::Low, ObservationCategory::Action, "Low priority", 0.7));
        
        let formatted = format_date_group(&group);
        
        assert!(formatted.contains("🔴"));
        assert!(formatted.contains("🟡"));
        assert!(formatted.contains("🟢"));
    }
    
    #[test]
    fn test_deeply_nested_observations() {
        // Parser only supports 2 levels (parent + child), but should handle gracefully
        let input = r#"
Date: 2026-03-04
- 🔴 Level 1
  - 🟡 Level 2
"#;
        
        let parser = ObservationParser::new();
        let groups = parser.parse(input).unwrap();
        
        assert_eq!(groups[0].observations.len(), 1);
        assert_eq!(groups[0].observations[0].children.len(), 1);
    }
}
