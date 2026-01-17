//! ULID-based identifier generation with prefixes.
//!
//! Identifiers in wonopcode follow the pattern: `prefix_ulid`
//! For example: `ses_01HQXYZ...` for sessions.

use ulid::Ulid;

/// Known identifier prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdPrefix {
    Session,
    Message,
    Part,
    Project,
}

impl IdPrefix {
    /// Get the string prefix for this identifier type.
    pub fn as_str(&self) -> &'static str {
        match self {
            IdPrefix::Session => "ses",
            IdPrefix::Message => "msg",
            IdPrefix::Part => "prt",
            IdPrefix::Project => "prj",
        }
    }

    /// Parse a prefix from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ses" => Some(IdPrefix::Session),
            "msg" => Some(IdPrefix::Message),
            "prt" => Some(IdPrefix::Part),
            "prj" => Some(IdPrefix::Project),
            _ => None,
        }
    }
}

/// Identifier generation and parsing utilities.
pub struct Identifier;

impl Identifier {
    /// Generate a new ascending identifier (newer = larger).
    ///
    /// This is the default for most identifiers where we want
    /// chronological ordering.
    pub fn ascending(prefix: IdPrefix) -> String {
        let ulid = Ulid::new();
        format!("{}_{}", prefix.as_str(), ulid.to_string().to_lowercase())
    }

    /// Generate a new descending identifier (newer = smaller).
    ///
    /// This is useful for session IDs where we want the most
    /// recent sessions to sort first.
    pub fn descending(prefix: IdPrefix) -> String {
        let ulid = Ulid::new();
        // Invert the ULID bits for descending order
        let inverted = !ulid.0;
        let inverted_ulid = Ulid(inverted);
        format!(
            "{}_{}",
            prefix.as_str(),
            inverted_ulid.to_string().to_lowercase()
        )
    }

    /// Generate an identifier with a specific ULID (for testing or imports).
    pub fn with_ulid(prefix: IdPrefix, ulid: Ulid) -> String {
        format!("{}_{}", prefix.as_str(), ulid.to_string().to_lowercase())
    }

    /// Parse an identifier into its prefix and ULID parts.
    pub fn parse(id: &str) -> Option<(IdPrefix, Ulid)> {
        let parts: Vec<&str> = id.splitn(2, '_').collect();
        if parts.len() != 2 {
            return None;
        }

        let prefix = IdPrefix::parse(parts[0])?;
        let ulid = Ulid::from_string(parts[1]).ok()?;
        Some((prefix, ulid))
    }

    /// Check if an identifier has the expected prefix.
    pub fn has_prefix(id: &str, prefix: IdPrefix) -> bool {
        id.starts_with(prefix.as_str()) && id.chars().nth(prefix.as_str().len()) == Some('_')
    }

    /// Generate a session ID (descending for recency sort).
    pub fn session() -> String {
        Self::descending(IdPrefix::Session)
    }

    /// Generate a message ID (ascending for chronological order).
    pub fn message() -> String {
        Self::ascending(IdPrefix::Message)
    }

    /// Generate a part ID (ascending for chronological order).
    pub fn part() -> String {
        Self::ascending(IdPrefix::Part)
    }

    /// Generate a project ID (ascending).
    pub fn project() -> String {
        Self::ascending(IdPrefix::Project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascending_id() {
        let id = Identifier::ascending(IdPrefix::Session);
        assert!(id.starts_with("ses_"));
        assert_eq!(id.len(), 30); // "ses_" (4) + ULID (26)
    }

    #[test]
    fn test_descending_id() {
        let id = Identifier::descending(IdPrefix::Session);
        assert!(id.starts_with("ses_"));
        assert_eq!(id.len(), 30);
    }

    #[test]
    fn test_ascending_order() {
        let id1 = Identifier::ascending(IdPrefix::Message);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = Identifier::ascending(IdPrefix::Message);
        assert!(id1 < id2, "Ascending IDs should increase over time");
    }

    #[test]
    fn test_descending_order() {
        let id1 = Identifier::descending(IdPrefix::Session);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = Identifier::descending(IdPrefix::Session);
        assert!(id1 > id2, "Descending IDs should decrease over time");
    }

    #[test]
    fn test_parse_id() {
        let id = Identifier::ascending(IdPrefix::Session);
        let (prefix, _ulid) = Identifier::parse(&id).unwrap();
        assert_eq!(prefix, IdPrefix::Session);
    }

    #[test]
    fn test_has_prefix() {
        let id = Identifier::session();
        assert!(Identifier::has_prefix(&id, IdPrefix::Session));
        assert!(!Identifier::has_prefix(&id, IdPrefix::Message));
    }

    #[test]
    fn test_convenience_functions() {
        assert!(Identifier::session().starts_with("ses_"));
        assert!(Identifier::message().starts_with("msg_"));
        assert!(Identifier::part().starts_with("prt_"));
        assert!(Identifier::project().starts_with("prj_"));
    }

    #[test]
    fn test_id_prefix_as_str_all_variants() {
        assert_eq!(IdPrefix::Session.as_str(), "ses");
        assert_eq!(IdPrefix::Message.as_str(), "msg");
        assert_eq!(IdPrefix::Part.as_str(), "prt");
        assert_eq!(IdPrefix::Project.as_str(), "prj");
    }

    #[test]
    fn test_id_prefix_parse_all_variants() {
        assert_eq!(IdPrefix::parse("ses"), Some(IdPrefix::Session));
        assert_eq!(IdPrefix::parse("msg"), Some(IdPrefix::Message));
        assert_eq!(IdPrefix::parse("prt"), Some(IdPrefix::Part));
        assert_eq!(IdPrefix::parse("prj"), Some(IdPrefix::Project));
        assert_eq!(IdPrefix::parse("unknown"), None);
    }

    #[test]
    fn test_parse_invalid_format_no_underscore() {
        assert!(Identifier::parse("nounderscore").is_none());
    }

    #[test]
    fn test_parse_invalid_format_unknown_prefix() {
        assert!(Identifier::parse("xyz_01HQXYZ").is_none());
    }

    #[test]
    fn test_parse_invalid_ulid() {
        assert!(Identifier::parse("ses_notaulid").is_none());
    }

    #[test]
    fn test_with_ulid() {
        let ulid = Ulid::new();
        let id = Identifier::with_ulid(IdPrefix::Message, ulid);
        assert!(id.starts_with("msg_"));
        let (_, parsed_ulid) = Identifier::parse(&id).unwrap();
        assert_eq!(parsed_ulid, ulid);
    }

    #[test]
    fn test_has_prefix_without_underscore() {
        // "ses123" starts with "ses" but doesn't have underscore after
        assert!(!Identifier::has_prefix("ses123", IdPrefix::Session));
    }

    #[test]
    fn test_all_prefixes_different_lengths() {
        // All our prefixes are 3 chars, verify IDs have correct length
        let session_id = Identifier::session();
        let message_id = Identifier::message();
        let part_id = Identifier::part();
        let project_id = Identifier::project();

        assert_eq!(session_id.len(), 30);
        assert_eq!(message_id.len(), 30);
        assert_eq!(part_id.len(), 30);
        assert_eq!(project_id.len(), 30);
    }
}
