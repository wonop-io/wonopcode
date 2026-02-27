//! TicketService trait and types for ticket operations.
//!
//! This module defines the abstraction layer for ticket operations,
//! allowing tools to access ticket data without direct coupling to
//! the Tauri application or specific tracker implementations.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Error type for ticket operations.
#[derive(Debug)]
pub enum TicketError {
    /// No trackers are configured.
    NoTrackers,
    /// The specified tracker was not found.
    TrackerNotFound(String),
    /// The specified ticket was not found.
    TicketNotFound(String),
    /// An error occurred while communicating with the tracker.
    TrackerError(String),
    /// Validation error for input parameters.
    ValidationError(String),
}

impl fmt::Display for TicketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TicketError::NoTrackers => write!(
                f,
                "No issue trackers configured. Please configure a tracker in Settings."
            ),
            TicketError::TrackerNotFound(id) => {
                write!(f, "Tracker '{}' not found or not enabled", id)
            }
            TicketError::TicketNotFound(id) => {
                write!(f, "Ticket '{}' not found in any tracker", id)
            }
            TicketError::TrackerError(msg) => write!(f, "Tracker error: {}", msg),
            TicketError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for TicketError {}

/// Ticket status matching beat-core Status enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TicketStatus {
    Open,
    InProgress,
    Review,
    Closed,
    Custom(String),
}

impl TicketStatus {
    /// Parse status from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "open" => TicketStatus::Open,
            "in-progress" | "inprogress" | "in_progress" => TicketStatus::InProgress,
            "review" => TicketStatus::Review,
            "closed" | "done" | "completed" => TicketStatus::Closed,
            other => TicketStatus::Custom(other.to_string()),
        }
    }

    /// Convert to string representation.
    pub fn as_str(&self) -> &str {
        match self {
            TicketStatus::Open => "open",
            TicketStatus::InProgress => "in-progress",
            TicketStatus::Review => "review",
            TicketStatus::Closed => "closed",
            TicketStatus::Custom(s) => s,
        }
    }
}

impl fmt::Display for TicketStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// User information for assignees and comment authors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketUser {
    /// Unique identifier for the user.
    pub id: String,
    /// Username/handle.
    pub username: String,
    /// Display name (optional).
    pub name: Option<String>,
}

impl fmt::Display for TicketUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = &self.name {
            write!(f, "{} (@{})", name, self.username)
        } else {
            write!(f, "@{}", self.username)
        }
    }
}

/// Summary ticket information for list/search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketSummary {
    /// Unique ticket identifier.
    pub id: String,
    /// Ticket title/summary.
    pub title: String,
    /// Current status.
    pub status: TicketStatus,
    /// Assigned user (if any).
    pub assignee: Option<TicketUser>,
    /// Labels/tags.
    pub labels: Vec<String>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Name of the tracker this ticket belongs to.
    pub tracker_name: String,
}

/// Full ticket details including description and comments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketDetails {
    /// Unique ticket identifier.
    pub id: String,
    /// Ticket title/summary.
    pub title: String,
    /// Full description (markdown).
    pub description: Option<String>,
    /// Current status.
    pub status: TicketStatus,
    /// Assigned user (if any).
    pub assignee: Option<TicketUser>,
    /// Labels/tags.
    pub labels: Vec<String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
    /// Comments on the ticket.
    pub comments: Vec<TicketComment>,
    /// Attachments on the ticket.
    pub attachments: Vec<TicketAttachment>,
    /// Name of the tracker this ticket belongs to.
    pub tracker_name: String,
    /// URL to view the ticket in the tracker's web interface.
    pub url: Option<String>,
}

/// A comment on a ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketComment {
    /// Unique comment identifier.
    pub id: String,
    /// Comment author.
    pub author: TicketUser,
    /// Comment body (markdown).
    pub body: String,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Attachment metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketAttachment {
    /// Unique attachment identifier.
    pub id: String,
    /// Filename.
    pub filename: String,
    /// File size in bytes.
    pub size: u64,
    /// MIME type (optional).
    pub mime_type: Option<String>,
    /// URL to download the attachment.
    pub url: Option<String>,
}

/// Filter options for listing tickets.
#[derive(Debug, Clone, Default)]
pub struct TicketFilter {
    /// Filter by status (any of these statuses).
    pub status: Option<Vec<TicketStatus>>,
    /// Filter by assignee username.
    pub assignee: Option<String>,
    /// Filter by labels (all must match).
    pub labels: Vec<String>,
    /// Maximum number of results to return.
    pub limit: usize,
}

/// Data for creating a new ticket.
#[derive(Debug, Clone)]
pub struct NewTicket {
    /// Ticket title (required).
    pub title: String,
    /// Ticket description (optional, markdown).
    pub description: Option<String>,
    /// Target tracker ID (optional, uses first enabled tracker if not specified).
    pub tracker_id: Option<String>,
    /// Assignee username (optional).
    pub assignee: Option<String>,
    /// Labels to apply.
    pub labels: Vec<String>,
    /// Initial status (optional, defaults to Open).
    pub status: Option<TicketStatus>,
}

/// Result of ticket creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedTicket {
    /// Generated ticket ID.
    pub id: String,
    /// Ticket title.
    pub title: String,
    /// URL to view the ticket in the tracker's web interface.
    pub url: Option<String>,
    /// Name of the tracker where the ticket was created.
    pub tracker_name: String,
}

/// Basic tracker information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerInfo {
    /// Unique tracker identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Type of tracker (github, linear, etc.).
    pub tracker_type: String,
    /// Whether the tracker is enabled.
    pub enabled: bool,
}

/// Label information including tracker source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelInfo {
    /// The label name.
    pub name: String,
    /// Hex color code (e.g., "ff0000").
    pub color: Option<String>,
    /// Label description.
    pub description: Option<String>,
    /// Which tracker this label belongs to.
    pub tracker_name: String,
}

/// Trait for ticket operations.
///
/// This trait abstracts ticket operations, allowing tools to access
/// ticket data without direct coupling to Tauri or specific trackers.
/// Implementations should aggregate data from all enabled trackers.
#[async_trait]
pub trait TicketService: Send + Sync {
    /// List tickets with optional filtering.
    ///
    /// Returns tickets from all enabled trackers matching the filter.
    async fn list_tickets(&self, filter: TicketFilter) -> Result<Vec<TicketSummary>, TicketError>;

    /// Search tickets by query string.
    ///
    /// Searches ticket titles and descriptions across all enabled trackers.
    async fn search_tickets(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<TicketSummary>, TicketError>;

    /// Get full ticket details.
    ///
    /// Fetches comprehensive information about a ticket including
    /// description, comments, and attachments.
    async fn get_ticket(
        &self,
        id: &str,
        include_comments: bool,
        include_attachments: bool,
    ) -> Result<TicketDetails, TicketError>;

    /// Create a new ticket.
    ///
    /// Creates a ticket in the specified tracker (or first enabled tracker).
    async fn create_ticket(&self, ticket: NewTicket) -> Result<CreatedTicket, TicketError>;

    /// List available trackers.
    ///
    /// Returns information about all configured trackers.
    async fn list_trackers(&self) -> Result<Vec<TrackerInfo>, TicketError>;

    /// Add labels to a ticket.
    ///
    /// Adds the specified labels to a ticket without removing existing labels.
    async fn add_labels(
        &self,
        ticket_id: &str,
        labels: Vec<String>,
    ) -> Result<TicketDetails, TicketError>;

    /// Remove labels from a ticket.
    ///
    /// Removes the specified labels from a ticket while preserving other labels.
    async fn remove_labels(
        &self,
        ticket_id: &str,
        labels: Vec<String>,
    ) -> Result<TicketDetails, TicketError>;

    /// List available labels across all enabled trackers.
    ///
    /// Returns labels from all trackers with their associated tracker names.
    async fn list_labels(&self) -> Result<Vec<LabelInfo>, TicketError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ticket_status_from_str() {
        assert_eq!(TicketStatus::parse("open"), TicketStatus::Open);
        assert_eq!(TicketStatus::parse("OPEN"), TicketStatus::Open);
        assert_eq!(
            TicketStatus::parse("in-progress"),
            TicketStatus::InProgress
        );
        assert_eq!(
            TicketStatus::parse("in_progress"),
            TicketStatus::InProgress
        );
        assert_eq!(TicketStatus::parse("review"), TicketStatus::Review);
        assert_eq!(TicketStatus::parse("closed"), TicketStatus::Closed);
        assert_eq!(TicketStatus::parse("done"), TicketStatus::Closed);
        assert_eq!(
            TicketStatus::parse("custom-status"),
            TicketStatus::Custom("custom-status".to_string())
        );
    }

    #[test]
    fn test_ticket_status_as_str() {
        assert_eq!(TicketStatus::Open.as_str(), "open");
        assert_eq!(TicketStatus::InProgress.as_str(), "in-progress");
        assert_eq!(TicketStatus::Review.as_str(), "review");
        assert_eq!(TicketStatus::Closed.as_str(), "closed");
        assert_eq!(
            TicketStatus::Custom("custom".to_string()).as_str(),
            "custom"
        );
    }

    #[test]
    fn test_ticket_status_display() {
        assert_eq!(format!("{}", TicketStatus::Open), "open");
        assert_eq!(format!("{}", TicketStatus::InProgress), "in-progress");
    }

    #[test]
    fn test_ticket_user_display() {
        let user_with_name = TicketUser {
            id: "1".to_string(),
            username: "john".to_string(),
            name: Some("John Doe".to_string()),
        };
        assert_eq!(format!("{}", user_with_name), "John Doe (@john)");

        let user_without_name = TicketUser {
            id: "2".to_string(),
            username: "jane".to_string(),
            name: None,
        };
        assert_eq!(format!("{}", user_without_name), "@jane");
    }

    #[test]
    fn test_ticket_error_display() {
        assert!(TicketError::NoTrackers
            .to_string()
            .contains("No issue trackers"));
        assert!(TicketError::TrackerNotFound("test".to_string())
            .to_string()
            .contains("test"));
        assert!(TicketError::TicketNotFound("WON-123".to_string())
            .to_string()
            .contains("WON-123"));
        assert!(TicketError::TrackerError("API failed".to_string())
            .to_string()
            .contains("API failed"));
        assert!(TicketError::ValidationError("empty title".to_string())
            .to_string()
            .contains("empty title"));
    }

    #[test]
    fn test_ticket_filter_default() {
        let filter = TicketFilter::default();
        assert!(filter.status.is_none());
        assert!(filter.assignee.is_none());
        assert!(filter.labels.is_empty());
        assert_eq!(filter.limit, 0);
    }
}

/// Mock implementation of TicketService for testing.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Mutex;

    /// A configurable mock for TicketService.
    ///
    /// Allows tests to set up expected return values for each method.
    pub struct MockTicketService {
        pub list_result: Mutex<Option<Result<Vec<TicketSummary>, TicketError>>>,
        pub search_result: Mutex<Option<Result<Vec<TicketSummary>, TicketError>>>,
        pub get_result: Mutex<Option<Result<TicketDetails, TicketError>>>,
        pub create_result: Mutex<Option<Result<CreatedTicket, TicketError>>>,
        pub trackers_result: Mutex<Option<Result<Vec<TrackerInfo>, TicketError>>>,
        pub add_labels_result: Mutex<Option<Result<TicketDetails, TicketError>>>,
        pub remove_labels_result: Mutex<Option<Result<TicketDetails, TicketError>>>,
        pub list_labels_result: Mutex<Option<Result<Vec<LabelInfo>, TicketError>>>,
        /// Captured filter from last list_tickets call.
        pub captured_filter: Mutex<Option<TicketFilter>>,
        /// Captured query from last search_tickets call.
        pub captured_search_query: Mutex<Option<String>>,
        /// Captured ticket ID from last get_ticket call.
        pub captured_ticket_id: Mutex<Option<String>>,
        /// Captured new ticket from last create_ticket call.
        pub captured_new_ticket: Mutex<Option<NewTicket>>,
        /// Captured ticket ID and labels from last add_labels call.
        pub captured_add_labels: Mutex<Option<(String, Vec<String>)>>,
        /// Captured ticket ID and labels from last remove_labels call.
        pub captured_remove_labels: Mutex<Option<(String, Vec<String>)>>,
    }

    impl MockTicketService {
        /// Create a new mock with no results configured.
        pub fn new() -> Self {
            Self {
                list_result: Mutex::new(None),
                search_result: Mutex::new(None),
                get_result: Mutex::new(None),
                create_result: Mutex::new(None),
                trackers_result: Mutex::new(None),
                add_labels_result: Mutex::new(None),
                remove_labels_result: Mutex::new(None),
                list_labels_result: Mutex::new(None),
                captured_filter: Mutex::new(None),
                captured_search_query: Mutex::new(None),
                captured_ticket_id: Mutex::new(None),
                captured_new_ticket: Mutex::new(None),
                captured_add_labels: Mutex::new(None),
                captured_remove_labels: Mutex::new(None),
            }
        }

        /// Set the result for list_tickets.
        pub fn set_list_result(&self, result: Result<Vec<TicketSummary>, TicketError>) {
            *self.list_result.lock().unwrap() = Some(result);
        }

        /// Set the result for search_tickets.
        pub fn set_search_result(&self, result: Result<Vec<TicketSummary>, TicketError>) {
            *self.search_result.lock().unwrap() = Some(result);
        }

        /// Set the result for get_ticket.
        pub fn set_get_result(&self, result: Result<TicketDetails, TicketError>) {
            *self.get_result.lock().unwrap() = Some(result);
        }

        /// Set the result for create_ticket.
        pub fn set_create_result(&self, result: Result<CreatedTicket, TicketError>) {
            *self.create_result.lock().unwrap() = Some(result);
        }

        /// Set the result for list_trackers.
        #[allow(dead_code)]
        pub fn set_trackers_result(&self, result: Result<Vec<TrackerInfo>, TicketError>) {
            *self.trackers_result.lock().unwrap() = Some(result);
        }

        /// Get the captured filter from last list_tickets call.
        pub fn get_captured_filter(&self) -> Option<TicketFilter> {
            self.captured_filter.lock().unwrap().take()
        }

        /// Get the captured query from last search_tickets call.
        pub fn get_captured_search_query(&self) -> Option<String> {
            self.captured_search_query.lock().unwrap().take()
        }

        /// Get the captured ticket ID from last get_ticket call.
        pub fn get_captured_ticket_id(&self) -> Option<String> {
            self.captured_ticket_id.lock().unwrap().take()
        }

        /// Get the captured new ticket from last create_ticket call.
        pub fn get_captured_new_ticket(&self) -> Option<NewTicket> {
            self.captured_new_ticket.lock().unwrap().take()
        }

        /// Set the result for add_labels.
        pub fn set_add_labels_result(&self, result: Result<TicketDetails, TicketError>) {
            *self.add_labels_result.lock().unwrap() = Some(result);
        }

        /// Set the result for remove_labels.
        pub fn set_remove_labels_result(&self, result: Result<TicketDetails, TicketError>) {
            *self.remove_labels_result.lock().unwrap() = Some(result);
        }

        /// Set the result for list_labels.
        pub fn set_list_labels_result(&self, result: Result<Vec<LabelInfo>, TicketError>) {
            *self.list_labels_result.lock().unwrap() = Some(result);
        }

        /// Get the captured ticket ID and labels from last add_labels call.
        pub fn get_captured_add_labels(&self) -> Option<(String, Vec<String>)> {
            self.captured_add_labels.lock().unwrap().take()
        }

        /// Get the captured ticket ID and labels from last remove_labels call.
        pub fn get_captured_remove_labels(&self) -> Option<(String, Vec<String>)> {
            self.captured_remove_labels.lock().unwrap().take()
        }
    }

    impl Default for MockTicketService {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl TicketService for MockTicketService {
        async fn list_tickets(
            &self,
            filter: TicketFilter,
        ) -> Result<Vec<TicketSummary>, TicketError> {
            *self.captured_filter.lock().unwrap() = Some(filter);
            self.list_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::NoTrackers))
        }

        async fn search_tickets(
            &self,
            query: &str,
            _limit: usize,
        ) -> Result<Vec<TicketSummary>, TicketError> {
            *self.captured_search_query.lock().unwrap() = Some(query.to_string());
            self.search_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::NoTrackers))
        }

        async fn get_ticket(
            &self,
            id: &str,
            _include_comments: bool,
            _include_attachments: bool,
        ) -> Result<TicketDetails, TicketError> {
            *self.captured_ticket_id.lock().unwrap() = Some(id.to_string());
            self.get_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::TicketNotFound(id.to_string())))
        }

        async fn create_ticket(&self, ticket: NewTicket) -> Result<CreatedTicket, TicketError> {
            *self.captured_new_ticket.lock().unwrap() = Some(ticket);
            self.create_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::NoTrackers))
        }

        async fn list_trackers(&self) -> Result<Vec<TrackerInfo>, TicketError> {
            self.trackers_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::NoTrackers))
        }

        async fn add_labels(
            &self,
            ticket_id: &str,
            labels: Vec<String>,
        ) -> Result<TicketDetails, TicketError> {
            *self.captured_add_labels.lock().unwrap() = Some((ticket_id.to_string(), labels));
            self.add_labels_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::TicketNotFound(ticket_id.to_string())))
        }

        async fn remove_labels(
            &self,
            ticket_id: &str,
            labels: Vec<String>,
        ) -> Result<TicketDetails, TicketError> {
            *self.captured_remove_labels.lock().unwrap() = Some((ticket_id.to_string(), labels));
            self.remove_labels_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(TicketError::TicketNotFound(ticket_id.to_string())))
        }

        async fn list_labels(&self) -> Result<Vec<LabelInfo>, TicketError> {
            self.list_labels_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Ok(Vec::new()))
        }
    }

    /// Helper function to create sample ticket summaries for testing.
    pub fn sample_ticket_summaries() -> Vec<TicketSummary> {
        vec![
            TicketSummary {
                id: "WON-123".to_string(),
                title: "Fix authentication bug".to_string(),
                status: TicketStatus::Open,
                assignee: Some(TicketUser {
                    id: "user1".to_string(),
                    username: "john".to_string(),
                    name: Some("John Doe".to_string()),
                }),
                labels: vec!["bug".to_string(), "auth".to_string()],
                updated_at: chrono::Utc::now(),
                tracker_name: "Linear".to_string(),
            },
            TicketSummary {
                id: "WON-124".to_string(),
                title: "Add new dashboard feature".to_string(),
                status: TicketStatus::InProgress,
                assignee: None,
                labels: vec!["feature".to_string()],
                updated_at: chrono::Utc::now(),
                tracker_name: "Linear".to_string(),
            },
            TicketSummary {
                id: "GH-456".to_string(),
                title: "Update documentation".to_string(),
                status: TicketStatus::Review,
                assignee: Some(TicketUser {
                    id: "user2".to_string(),
                    username: "jane".to_string(),
                    name: None,
                }),
                labels: vec!["docs".to_string()],
                updated_at: chrono::Utc::now(),
                tracker_name: "GitHub".to_string(),
            },
        ]
    }

    /// Helper function to create a sample ticket details for testing.
    pub fn sample_ticket_details() -> TicketDetails {
        TicketDetails {
            id: "WON-123".to_string(),
            title: "Fix authentication bug".to_string(),
            description: Some("The login flow fails when...".to_string()),
            status: TicketStatus::Open,
            assignee: Some(TicketUser {
                id: "user1".to_string(),
                username: "john".to_string(),
                name: Some("John Doe".to_string()),
            }),
            labels: vec!["bug".to_string(), "auth".to_string()],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            comments: vec![TicketComment {
                id: "comment1".to_string(),
                author: TicketUser {
                    id: "user2".to_string(),
                    username: "jane".to_string(),
                    name: None,
                },
                body: "I can reproduce this issue.".to_string(),
                created_at: chrono::Utc::now(),
            }],
            attachments: vec![TicketAttachment {
                id: "attach1".to_string(),
                filename: "screenshot.png".to_string(),
                size: 12345,
                mime_type: Some("image/png".to_string()),
                url: Some("https://example.com/screenshot.png".to_string()),
            }],
            tracker_name: "Linear".to_string(),
            url: Some("https://linear.app/team/WON-123".to_string()),
        }
    }

    /// Helper function to create sample tracker info for testing.
    #[allow(dead_code)]
    pub fn sample_tracker_info() -> Vec<TrackerInfo> {
        vec![
            TrackerInfo {
                id: "linear-1".to_string(),
                name: "Linear".to_string(),
                tracker_type: "linear".to_string(),
                enabled: true,
            },
            TrackerInfo {
                id: "github-1".to_string(),
                name: "GitHub".to_string(),
                tracker_type: "github".to_string(),
                enabled: true,
            },
        ]
    }

    /// Helper function to create sample label info for testing.
    pub fn sample_label_info() -> Vec<LabelInfo> {
        vec![
            LabelInfo {
                name: "bug".to_string(),
                color: Some("d73a4a".to_string()),
                description: Some("Something isn't working".to_string()),
                tracker_name: "GitHub".to_string(),
            },
            LabelInfo {
                name: "feature".to_string(),
                color: Some("a2eeef".to_string()),
                description: Some("New feature request".to_string()),
                tracker_name: "GitHub".to_string(),
            },
            LabelInfo {
                name: "enhancement".to_string(),
                color: Some("84b6eb".to_string()),
                description: None,
                tracker_name: "Linear".to_string(),
            },
        ]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_ticket_service_list() {
            let mock = MockTicketService::new();
            mock.set_list_result(Ok(sample_ticket_summaries()));

            let filter = TicketFilter {
                status: Some(vec![TicketStatus::Open]),
                assignee: Some("john".to_string()),
                labels: vec!["bug".to_string()],
                limit: 10,
            };

            let result = mock.list_tickets(filter).await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 3);

            let captured = mock.get_captured_filter().unwrap();
            assert_eq!(captured.assignee, Some("john".to_string()));
            assert_eq!(captured.limit, 10);
        }

        #[tokio::test]
        async fn test_mock_ticket_service_error() {
            let mock = MockTicketService::new();
            mock.set_list_result(Err(TicketError::TrackerError("API error".to_string())));

            let filter = TicketFilter::default();
            let result = mock.list_tickets(filter).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("API error"));
        }
    }
}
