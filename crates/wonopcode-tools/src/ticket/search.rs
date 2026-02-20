//! TicketSearchTool implementation.

use crate::ticket::TicketSummary;
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

/// Tool for searching tickets across all enabled trackers.
pub struct TicketSearchTool;

#[derive(Debug, Deserialize)]
struct TicketSearchArgs {
    /// Search query string.
    query: String,
    /// Maximum number of results (default: 20).
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

#[async_trait]
impl Tool for TicketSearchTool {
    fn id(&self) -> &str {
        "ticket_search"
    }

    fn description(&self) -> &str {
        r#"Search for tickets across all enabled issue trackers.

Use this tool to find specific tickets by searching their titles and descriptions.
The search query can include:
- Keywords that appear in ticket titles or descriptions
- Ticket IDs (partial or full)

Returns a list of matching tickets sorted by relevance."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to match against ticket titles and descriptions",
                    "minLength": 1
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 20,
                    "description": "Maximum number of results to return"
                }
            }
        })
    }

    fn requires_permission(&self) -> bool {
        false // Read-only operation
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TicketSearchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Validate query
        let query = args.query.trim();
        if query.is_empty() {
            return Err(ToolError::validation("Search query cannot be empty"));
        }

        debug!(query = %query, limit = args.limit, "Searching tickets");

        // Get ticket service from context
        let ticket_service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        // Execute the search
        let tickets = ticket_service
            .search_tickets(query, args.limit.min(50))
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Format output
        let output = format_search_results(query, &tickets);

        Ok(
            ToolOutput::new(format!("Search: {}", query), output).with_metadata(json!({
                "query": query,
                "count": tickets.len(),
                "limit": args.limit
            })),
        )
    }
}

/// Format search results.
fn format_search_results(query: &str, tickets: &[TicketSummary]) -> String {
    if tickets.is_empty() {
        return format!(
            "No tickets found matching \"{}\".\n\nTry:\n- Using different keywords\n- Searching for a specific ticket ID\n- Checking if trackers are configured and enabled",
            query
        );
    }

    let mut output = format!(
        "## Search Results for \"{}\" ({} found)\n\n",
        query,
        tickets.len()
    );
    output.push_str("| ID | Title | Status | Tracker |\n");
    output.push_str("|:---|:------|:-------|:--------|\n");

    for ticket in tickets {
        let title = truncate_string(&ticket.title, 50);
        output.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            ticket.id, title, ticket.status, ticket.tracker_name
        ));
    }

    output.push_str("\nUse `ticket_read` with a ticket ID to see full details.");

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
    use crate::ticket::{TicketError, TicketStatus, TicketUser};
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn test_ticket_search_tool_id() {
        let tool = TicketSearchTool;
        assert_eq!(tool.id(), "ticket_search");
    }

    #[test]
    fn test_ticket_search_tool_description() {
        let tool = TicketSearchTool;
        let desc = tool.description();
        assert!(desc.contains("Search"));
        assert!(desc.contains("tickets"));
    }

    #[test]
    fn test_ticket_search_tool_schema() {
        let tool = TicketSearchTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("query")));

        assert!(schema["properties"]["query"].is_object());
        assert!(schema["properties"]["limit"].is_object());
    }

    #[test]
    fn test_format_search_results_empty() {
        let results: Vec<TicketSummary> = vec![];
        let output = format_search_results("test query", &results);
        assert!(output.contains("No tickets found"));
        assert!(output.contains("test query"));
    }

    #[test]
    fn test_format_search_results_with_results() {
        let results = vec![
            TicketSummary {
                id: "WON-100".to_string(),
                title: "Bug in login".to_string(),
                status: TicketStatus::Open,
                assignee: None,
                labels: vec![],
                updated_at: Utc::now(),
                tracker_name: "GitHub".to_string(),
            },
            TicketSummary {
                id: "WON-101".to_string(),
                title: "Login feature enhancement".to_string(),
                status: TicketStatus::InProgress,
                assignee: Some(TicketUser {
                    id: "1".to_string(),
                    username: "dev".to_string(),
                    name: None,
                }),
                labels: vec!["feature".to_string()],
                updated_at: Utc::now(),
                tracker_name: "Linear".to_string(),
            },
        ];

        let output = format_search_results("login", &results);
        assert!(output.contains("Search Results"));
        assert!(output.contains("login"));
        assert!(output.contains("2 found"));
        assert!(output.contains("WON-100"));
        assert!(output.contains("WON-101"));
        assert!(output.contains("ticket_read"));
    }

    #[test]
    fn test_ticket_search_args_defaults() {
        let args: TicketSearchArgs = serde_json::from_value(json!({
            "query": "test"
        }))
        .unwrap();
        assert_eq!(args.query, "test");
        assert_eq!(args.limit, 20);
    }

    #[test]
    fn test_ticket_search_args_with_limit() {
        let args: TicketSearchArgs = serde_json::from_value(json!({
            "query": "bug fix",
            "limit": 10
        }))
        .unwrap();
        assert_eq!(args.query, "bug fix");
        assert_eq!(args.limit, 10);
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
        }
    }

    fn create_test_context_no_service() -> ToolContext {
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
            ticket_service: None,
        }
    }

    #[tokio::test]
    async fn test_execute_search_success() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_search_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketSearchTool;

        let result = tool.execute(json!({"query": "authentication"}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("WON-123"));
        assert!(output.output.contains("Search Results"));

        // Verify query was captured
        let captured_query = mock.get_captured_search_query().unwrap();
        assert_eq!(captured_query, "authentication");

        // Verify metadata
        assert_eq!(output.metadata["query"], "authentication");
        assert_eq!(output.metadata["count"], 3);
    }

    #[tokio::test]
    async fn test_execute_search_with_custom_limit() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_search_result(Ok(vec![]));

        let ctx = create_test_context(mock.clone());
        let tool = TicketSearchTool;

        let result = tool
            .execute(json!({"query": "test", "limit": 10}), &ctx)
            .await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.metadata["limit"], 10);
    }

    #[tokio::test]
    async fn test_execute_search_empty_results() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_search_result(Ok(vec![]));

        let ctx = create_test_context(mock.clone());
        let tool = TicketSearchTool;

        let result = tool.execute(json!({"query": "nonexistent"}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("No tickets found"));
        assert!(output.output.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_execute_search_no_ticket_service() {
        let ctx = create_test_context_no_service();
        let tool = TicketSearchTool;

        let result = tool.execute(json!({"query": "test"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_search_empty_query() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketSearchTool;

        let result = tool.execute(json!({"query": "  "}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_execute_search_missing_query() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketSearchTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Invalid arguments"));
    }

    #[tokio::test]
    async fn test_execute_search_tracker_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_search_result(Err(TicketError::TrackerError(
            "Connection failed".to_string(),
        )));

        let ctx = create_test_context(mock.clone());
        let tool = TicketSearchTool;

        let result = tool.execute(json!({"query": "test"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Connection failed"));
    }

    #[tokio::test]
    async fn test_execute_search_limit_capped() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_search_result(Ok(vec![]));

        let ctx = create_test_context(mock.clone());
        let tool = TicketSearchTool;

        // Request more than 50, should be capped
        let result = tool
            .execute(json!({"query": "test", "limit": 100}), &ctx)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_search_trims_query() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_search_result(Ok(sample_ticket_summaries()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketSearchTool;

        let result = tool
            .execute(json!({"query": "  authentication  "}), &ctx)
            .await;
        assert!(result.is_ok());

        let captured_query = mock.get_captured_search_query().unwrap();
        assert_eq!(captured_query, "authentication");
    }
}
