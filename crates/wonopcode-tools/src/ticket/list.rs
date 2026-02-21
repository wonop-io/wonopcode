//! TicketListTool implementation.

use crate::ticket::{TicketFilter, TicketStatus, TicketSummary};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

/// Tool for listing tickets from configured issue trackers.
pub struct TicketListTool;

#[derive(Debug, Deserialize)]
struct TicketListArgs {
    /// Filter by status.
    #[serde(default)]
    status: Option<Vec<String>>,
    /// Filter by assignee username.
    #[serde(default)]
    assignee: Option<String>,
    /// Filter by labels.
    #[serde(default)]
    labels: Option<Vec<String>>,
    /// Maximum number of results (default: 50).
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[async_trait]
impl Tool for TicketListTool {
    fn id(&self) -> &str {
        "ticket_list"
    }

    fn description(&self) -> &str {
        r#"List tickets from configured issue trackers with optional filtering.

Use this tool to get an overview of tickets in the project. You can filter by:
- status: Filter by ticket status (open, in-progress, review, closed)
- assignee: Filter by assigned user's username
- labels: Filter by labels (all must match)
- limit: Maximum number of results (default 50, max 100)

Returns a formatted table of tickets with ID, title, status, and assignee."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["open", "in-progress", "review", "closed"]
                    },
                    "description": "Filter by ticket status"
                },
                "assignee": {
                    "type": "string",
                    "description": "Filter by assignee username"
                },
                "labels": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Filter by labels (all must match)"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "default": 50,
                    "description": "Maximum number of tickets to return"
                }
            }
        })
    }

    fn requires_permission(&self) -> bool {
        false // Read-only operation
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TicketListArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        debug!(
            status = ?args.status,
            assignee = ?args.assignee,
            limit = args.limit,
            "Listing tickets"
        );

        // Get ticket service from context
        let ticket_service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        // Build filter
        let filter = TicketFilter {
            status: args.status.map(|s| {
                s.into_iter()
                    .map(|st| TicketStatus::parse(&st))
                    .collect()
            }),
            assignee: args.assignee,
            labels: args.labels.unwrap_or_default(),
            limit: args.limit.min(100),
        };

        // Execute the list operation
        let tickets = ticket_service
            .list_tickets(filter)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Format output
        let output = format_ticket_list(&tickets);

        Ok(ToolOutput::new("Ticket list", output).with_metadata(json!({
            "count": tickets.len(),
            "limit": args.limit
        })))
    }
}

/// Format ticket list as a markdown table.
fn format_ticket_list(tickets: &[TicketSummary]) -> String {
    if tickets.is_empty() {
        return "No tickets found matching the filter criteria.".to_string();
    }

    let mut output = format!("## Tickets ({} found)\n\n", tickets.len());
    output.push_str("| ID | Title | Status | Assignee | Labels |\n");
    output.push_str("|:---|:------|:-------|:---------|:-------|\n");

    for ticket in tickets {
        let title = truncate_string(&ticket.title, 60);
        let assignee = ticket
            .assignee
            .as_ref()
            .map(|u| format!("@{}", u.username))
            .unwrap_or_else(|| "-".to_string());
        let labels = if ticket.labels.is_empty() {
            "-".to_string()
        } else {
            ticket.labels.join(", ")
        };

        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            ticket.id, title, ticket.status, assignee, labels
        ));
    }

    output
}

/// Truncate a string to max length, adding ellipsis if truncated.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::mock::{sample_ticket_summaries, MockTicketService};
    use crate::ticket::TicketError;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn test_ticket_list_tool_id() {
        let tool = TicketListTool;
        assert_eq!(tool.id(), "ticket_list");
    }

    #[test]
    fn test_ticket_list_tool_description() {
        let tool = TicketListTool;
        let desc = tool.description();
        assert!(desc.contains("List tickets"));
        assert!(desc.contains("filter"));
    }

    #[test]
    fn test_ticket_list_tool_schema() {
        let tool = TicketListTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["status"].is_object());
        assert!(schema["properties"]["assignee"].is_object());
        assert!(schema["properties"]["labels"].is_object());
        assert!(schema["properties"]["limit"].is_object());
    }

    #[test]
    fn test_format_ticket_list_empty() {
        let tickets: Vec<TicketSummary> = vec![];
        let output = format_ticket_list(&tickets);
        assert!(output.contains("No tickets found"));
    }

    #[test]
    fn test_format_ticket_list_with_tickets() {
        let tickets = vec![
            TicketSummary {
                id: "WON-123".to_string(),
                title: "Fix bug".to_string(),
                status: TicketStatus::Open,
                assignee: Some(crate::ticket::TicketUser {
                    id: "1".to_string(),
                    username: "john".to_string(),
                    name: None,
                }),
                labels: vec!["bug".to_string()],
                updated_at: Utc::now(),
                tracker_name: "Linear".to_string(),
            },
            TicketSummary {
                id: "WON-124".to_string(),
                title: "Add feature".to_string(),
                status: TicketStatus::InProgress,
                assignee: None,
                labels: vec![],
                updated_at: Utc::now(),
                tracker_name: "Linear".to_string(),
            },
        ];

        let output = format_ticket_list(&tickets);
        assert!(output.contains("## Tickets (2 found)"));
        assert!(output.contains("WON-123"));
        assert!(output.contains("Fix bug"));
        assert!(output.contains("@john"));
        assert!(output.contains("WON-124"));
        assert!(output.contains("Add feature"));
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("this is a long string", 10), "this is...");
        assert_eq!(truncate_string("exactly10c", 10), "exactly10c");
    }

    #[test]
    fn test_ticket_list_args_defaults() {
        let args: TicketListArgs = serde_json::from_value(json!({})).unwrap();
        assert!(args.status.is_none());
        assert!(args.assignee.is_none());
        assert!(args.labels.is_none());
        assert_eq!(args.limit, 50);
    }

    #[test]
    fn test_ticket_list_args_with_values() {
        let args: TicketListArgs = serde_json::from_value(json!({
            "status": ["open", "in-progress"],
            "assignee": "john",
            "labels": ["bug", "urgent"],
            "limit": 25
        }))
        .unwrap();

        assert_eq!(
            args.status,
            Some(vec!["open".to_string(), "in-progress".to_string()])
        );
        assert_eq!(args.assignee, Some("john".to_string()));
        assert_eq!(
            args.labels,
            Some(vec!["bug".to_string(), "urgent".to_string()])
        );
        assert_eq!(args.limit, 25);
    }

    // Integration tests with MockTicketService

    fn create_test_context(mock: Arc<MockTicketService>) -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: Some(mock),
            memory_service: None,
        }
    }

    #[tokio::test]
    async fn test_execute_success() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("WON-123"));
        assert!(output.output.contains("WON-124"));
        assert!(output.output.contains("GH-456"));

        // Verify metadata
        assert_eq!(output.metadata["count"], 3);
    }

    #[tokio::test]
    async fn test_execute_with_status_filter() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool
            .execute(json!({"status": ["open", "in-progress"]}), &ctx)
            .await;
        assert!(result.is_ok());

        // Check that filter was passed correctly
        let captured = mock.get_captured_filter().unwrap();
        assert!(captured.status.is_some());
        let statuses = captured.status.unwrap();
        assert_eq!(statuses.len(), 2);
        assert!(statuses.contains(&TicketStatus::Open));
        assert!(statuses.contains(&TicketStatus::InProgress));
    }

    #[tokio::test]
    async fn test_execute_with_assignee_filter() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({"assignee": "john"}), &ctx).await;
        assert!(result.is_ok());

        let captured = mock.get_captured_filter().unwrap();
        assert_eq!(captured.assignee, Some("john".to_string()));
    }

    #[tokio::test]
    async fn test_execute_with_labels_filter() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool
            .execute(json!({"labels": ["bug", "urgent"]}), &ctx)
            .await;
        assert!(result.is_ok());

        let captured = mock.get_captured_filter().unwrap();
        assert_eq!(
            captured.labels,
            vec!["bug".to_string(), "urgent".to_string()]
        );
    }

    #[tokio::test]
    async fn test_execute_with_limit() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({"limit": 25}), &ctx).await;
        assert!(result.is_ok());

        let captured = mock.get_captured_filter().unwrap();
        assert_eq!(captured.limit, 25);
    }

    #[tokio::test]
    async fn test_execute_limit_capped_at_100() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({"limit": 500}), &ctx).await;
        assert!(result.is_ok());

        let captured = mock.get_captured_filter().unwrap();
        assert_eq!(captured.limit, 100); // Capped at max
    }

    #[tokio::test]
    async fn test_execute_empty_results() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Ok(vec![]));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("No tickets found"));
    }

    #[tokio::test]
    async fn test_execute_no_ticket_service() {
        let ctx = ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
            ticket_service: None,
            memory_service: None,
        };

        let tool = TicketListTool;
        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_tracker_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Err(TicketError::TrackerError(
            "API rate limit exceeded".to_string(),
        )));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("rate limit"));
    }

    #[tokio::test]
    async fn test_execute_no_trackers_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_result(Err(TicketError::NoTrackers));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("No issue trackers"));
    }

    #[tokio::test]
    async fn test_execute_invalid_args() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);

        let tool = TicketListTool;

        // Invalid status type
        let result = tool.execute(json!({"status": "not-an-array"}), &ctx).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid arguments"));
    }
}
