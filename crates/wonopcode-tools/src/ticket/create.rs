//! TicketCreateTool implementation.

use crate::ticket::{NewTicket, TicketStatus};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

/// Tool for creating new tickets in issue trackers.
pub struct TicketCreateTool;

#[derive(Debug, Deserialize)]
struct TicketCreateArgs {
    /// Ticket title (required).
    title: String,
    /// Ticket description (optional, supports markdown).
    #[serde(default)]
    description: Option<String>,
    /// Target tracker ID (optional, uses first enabled tracker if not specified).
    #[serde(default)]
    tracker_id: Option<String>,
    /// Assignee username (optional).
    #[serde(default)]
    assignee: Option<String>,
    /// Labels to apply (optional).
    #[serde(default)]
    labels: Option<Vec<String>>,
    /// Initial status (optional, defaults to "open").
    #[serde(default)]
    status: Option<String>,
}

#[async_trait]
impl Tool for TicketCreateTool {
    fn id(&self) -> &str {
        "ticket_create"
    }

    fn description(&self) -> &str {
        r#"Create a new ticket in the specified issue tracker.

Use this tool to create tickets for tracking bugs, features, or tasks.
Required fields:
- title: A clear, descriptive title for the ticket

Optional fields:
- description: Detailed description (markdown supported)
- tracker_id: Which tracker to create in (uses the workstream's tracker by default, 
              or first enabled tracker if not in a workstream)
- assignee: Username to assign the ticket to
- labels: Array of labels/tags to apply
- status: Initial status ("open" or "in-progress", defaults to "open")

Returns the created ticket's ID and URL."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 500,
                    "description": "The title/summary of the ticket"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed description of the ticket (supports markdown)"
                },
                "tracker_id": {
                    "type": "string",
                    "description": "The ID of the tracker to create the ticket in. If not provided, uses the first enabled tracker."
                },
                "assignee": {
                    "type": "string",
                    "description": "Username to assign the ticket to"
                },
                "labels": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Labels to apply to the ticket"
                },
                "status": {
                    "type": "string",
                    "enum": ["open", "in-progress"],
                    "default": "open",
                    "description": "Initial status of the ticket"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TicketCreateArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Validate title
        let title = args.title.trim();
        if title.is_empty() {
            return Err(ToolError::validation("Title cannot be empty"));
        }
        if title.len() > 500 {
            return Err(ToolError::validation(
                "Title is too long (maximum 500 characters)",
            ));
        }

        debug!(
            title = %title,
            tracker_id = ?args.tracker_id,
            assignee = ?args.assignee,
            "Creating ticket"
        );

        // Get ticket service from context
        let ticket_service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        // Parse status if provided
        let status = args.status.map(|s| TicketStatus::parse(&s));

        // Determine which tracker to use:
        // 1. Explicit tracker_id from the request
        // 2. Workstream's default tracker (from context)
        // 3. BeatTicketService will fall back to first enabled tracker
        let explicit_tracker = args.tracker_id.clone();
        let workstream_tracker = ctx.workstream_default_tracker_id.clone();
        let effective_tracker_id = explicit_tracker
            .clone()
            .or_else(|| workstream_tracker.clone());

        debug!(
            explicit_tracker = ?explicit_tracker,
            workstream_tracker = ?workstream_tracker,
            effective_tracker = ?effective_tracker_id,
            "Determining target tracker"
        );

        // Build the new ticket
        let new_ticket = NewTicket {
            title: title.to_string(),
            description: args.description,
            tracker_id: effective_tracker_id,
            assignee: args.assignee,
            labels: args.labels.unwrap_or_default(),
            status,
        };

        // Create the ticket
        let created = ticket_service
            .create_ticket(new_ticket)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Format output
        let output = format_created_ticket(&created);

        Ok(
            ToolOutput::new(format!("Created: {}", created.id), output).with_metadata(json!({
                "ticket_id": created.id,
                "title": created.title,
                "tracker": created.tracker_name,
                "url": created.url
            })),
        )
    }
}

/// Format the created ticket confirmation.
fn format_created_ticket(ticket: &crate::ticket::CreatedTicket) -> String {
    let mut output = String::new();

    output.push_str("## Ticket Created Successfully\n\n");
    output.push_str(&format!("**ID:** {}\n", ticket.id));
    output.push_str(&format!("**Title:** {}\n", ticket.title));
    output.push_str(&format!("**Tracker:** {}\n", ticket.tracker_name));

    if let Some(url) = &ticket.url {
        output.push_str(&format!("**URL:** {}\n", url));
    }

    output.push_str(&format!(
        "\nYou can view the ticket details using `ticket_read` with ID `{}`.",
        ticket.id
    ));

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::mock::MockTicketService;
    use crate::ticket::{CreatedTicket, TicketError};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn test_ticket_create_tool_id() {
        let tool = TicketCreateTool;
        assert_eq!(tool.id(), "ticket_create");
    }

    #[test]
    fn test_ticket_create_tool_description() {
        let tool = TicketCreateTool;
        let desc = tool.description();
        assert!(desc.contains("Create a new ticket"));
        assert!(desc.contains("title"));
    }

    #[test]
    fn test_ticket_create_tool_schema() {
        let tool = TicketCreateTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("title")));

        assert!(schema["properties"]["title"].is_object());
        assert!(schema["properties"]["description"].is_object());
        assert!(schema["properties"]["tracker_id"].is_object());
        assert!(schema["properties"]["assignee"].is_object());
        assert!(schema["properties"]["labels"].is_object());
        assert!(schema["properties"]["status"].is_object());
    }

    #[test]
    fn test_format_created_ticket() {
        let ticket = CreatedTicket {
            id: "WON-200".to_string(),
            title: "New feature request".to_string(),
            url: Some("https://linear.app/team/WON-200".to_string()),
            tracker_name: "Linear".to_string(),
        };

        let output = format_created_ticket(&ticket);
        assert!(output.contains("Ticket Created Successfully"));
        assert!(output.contains("WON-200"));
        assert!(output.contains("New feature request"));
        assert!(output.contains("Linear"));
        assert!(output.contains("https://linear.app/team/WON-200"));
        assert!(output.contains("ticket_read"));
    }

    #[test]
    fn test_format_created_ticket_no_url() {
        let ticket = CreatedTicket {
            id: "ISSUE-1".to_string(),
            title: "Bug fix".to_string(),
            url: None,
            tracker_name: "GitHub".to_string(),
        };

        let output = format_created_ticket(&ticket);
        assert!(output.contains("ISSUE-1"));
        assert!(output.contains("Bug fix"));
        assert!(output.contains("GitHub"));
        assert!(!output.contains("**URL:**"));
    }

    #[test]
    fn test_ticket_create_args_minimal() {
        let args: TicketCreateArgs = serde_json::from_value(json!({
            "title": "Simple ticket"
        }))
        .unwrap();

        assert_eq!(args.title, "Simple ticket");
        assert!(args.description.is_none());
        assert!(args.tracker_id.is_none());
        assert!(args.assignee.is_none());
        assert!(args.labels.is_none());
        assert!(args.status.is_none());
    }

    #[test]
    fn test_ticket_create_args_full() {
        let args: TicketCreateArgs = serde_json::from_value(json!({
            "title": "Full ticket",
            "description": "Detailed description here",
            "tracker_id": "tracker-1",
            "assignee": "john",
            "labels": ["bug", "urgent"],
            "status": "in-progress"
        }))
        .unwrap();

        assert_eq!(args.title, "Full ticket");
        assert_eq!(
            args.description,
            Some("Detailed description here".to_string())
        );
        assert_eq!(args.tracker_id, Some("tracker-1".to_string()));
        assert_eq!(args.assignee, Some("john".to_string()));
        assert_eq!(
            args.labels,
            Some(vec!["bug".to_string(), "urgent".to_string()])
        );
        assert_eq!(args.status, Some("in-progress".to_string()));
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
            ace_service: None,
            permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        default_shell: None,
        hms_service: None,
        typescript_executor: None,
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
            memory_service: None,
            ace_service: None,
            permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        default_shell: None,
        hms_service: None,
        typescript_executor: None,
        }
    }

    fn sample_created_ticket() -> CreatedTicket {
        CreatedTicket {
            id: "WON-200".to_string(),
            title: "New feature request".to_string(),
            url: Some("https://linear.app/team/WON-200".to_string()),
            tracker_name: "Linear".to_string(),
        }
    }

    #[tokio::test]
    async fn test_execute_create_success() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool
            .execute(json!({"title": "New feature request"}), &ctx)
            .await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("Ticket Created Successfully"));
        assert!(output.output.contains("WON-200"));
        assert!(output.output.contains("New feature request"));

        // Verify new ticket was captured
        let captured = mock.get_captured_new_ticket().unwrap();
        assert_eq!(captured.title, "New feature request");

        // Verify metadata
        assert_eq!(output.metadata["ticket_id"], "WON-200");
        assert_eq!(output.metadata["tracker"], "Linear");
    }

    #[tokio::test]
    async fn test_execute_create_with_description() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool
            .execute(
                json!({
                    "title": "Bug report",
                    "description": "Detailed description of the bug"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        let captured = mock.get_captured_new_ticket().unwrap();
        assert_eq!(
            captured.description,
            Some("Detailed description of the bug".to_string())
        );
    }

    #[tokio::test]
    async fn test_execute_create_with_all_fields() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool
            .execute(
                json!({
                    "title": "Full ticket",
                    "description": "Details here",
                    "tracker_id": "tracker-1",
                    "assignee": "john",
                    "labels": ["bug", "urgent"],
                    "status": "in-progress"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        let captured = mock.get_captured_new_ticket().unwrap();
        assert_eq!(captured.title, "Full ticket");
        assert_eq!(captured.tracker_id, Some("tracker-1".to_string()));
        assert_eq!(captured.assignee, Some("john".to_string()));
        assert_eq!(
            captured.labels,
            vec!["bug".to_string(), "urgent".to_string()]
        );
        assert!(captured.status.is_some());
    }

    #[tokio::test]
    async fn test_execute_create_no_ticket_service() {
        let ctx = create_test_context_no_service();
        let tool = TicketCreateTool;

        let result = tool.execute(json!({"title": "Test ticket"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_create_empty_title() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketCreateTool;

        let result = tool.execute(json!({"title": "  "}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_execute_create_missing_title() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketCreateTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Invalid arguments"));
    }

    #[tokio::test]
    async fn test_execute_create_title_too_long() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketCreateTool;

        let long_title = "a".repeat(501);
        let result = tool.execute(json!({"title": long_title}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("too long"));
    }

    #[tokio::test]
    async fn test_execute_create_trims_title() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool
            .execute(json!({"title": "  Trimmed title  "}), &ctx)
            .await;
        assert!(result.is_ok());

        let captured = mock.get_captured_new_ticket().unwrap();
        assert_eq!(captured.title, "Trimmed title");
    }

    #[tokio::test]
    async fn test_execute_create_tracker_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Err(TicketError::TrackerError(
            "Rate limit exceeded".to_string(),
        )));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool.execute(json!({"title": "Test ticket"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Rate limit"));
    }

    #[tokio::test]
    async fn test_execute_create_no_trackers() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Err(TicketError::NoTrackers));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool.execute(json!({"title": "Test ticket"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("No issue trackers"));
    }

    #[tokio::test]
    async fn test_execute_create_tracker_not_found() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Err(TicketError::TrackerNotFound("unknown".to_string())));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool
            .execute(
                json!({"title": "Test ticket", "tracker_id": "unknown"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("unknown"));
    }

    #[tokio::test]
    async fn test_execute_create_with_url_in_metadata() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketCreateTool;

        let result = tool.execute(json!({"title": "Test ticket"}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.metadata["url"], "https://linear.app/team/WON-200");
    }

    #[tokio::test]
    async fn test_execute_create_uses_workstream_default_tracker() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        // Create context with workstream default tracker set
        let mut ctx = create_test_context(mock.clone());
        ctx.workstream_ticket_id = Some("WON-123".to_string());
        ctx.workstream_default_tracker_id = Some("linear-workstream-tracker".to_string());

        let tool = TicketCreateTool;

        // Create ticket without explicit tracker_id
        let result = tool
            .execute(json!({"title": "Ticket using workstream tracker"}), &ctx)
            .await;
        assert!(result.is_ok());

        // Verify the workstream tracker was used
        let captured = mock.get_captured_new_ticket().unwrap();
        assert_eq!(
            captured.tracker_id,
            Some("linear-workstream-tracker".to_string())
        );
    }

    #[tokio::test]
    async fn test_execute_create_explicit_tracker_overrides_workstream() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_create_result(Ok(sample_created_ticket()));

        // Create context with workstream default tracker set
        let mut ctx = create_test_context(mock.clone());
        ctx.workstream_ticket_id = Some("WON-123".to_string());
        ctx.workstream_default_tracker_id = Some("linear-workstream-tracker".to_string());

        let tool = TicketCreateTool;

        // Create ticket WITH explicit tracker_id - should override workstream default
        let result = tool
            .execute(
                json!({
                    "title": "Ticket with explicit tracker",
                    "tracker_id": "github-explicit"
                }),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        // Verify the explicit tracker was used, not workstream default
        let captured = mock.get_captured_new_ticket().unwrap();
        assert_eq!(captured.tracker_id, Some("github-explicit".to_string()));
    }
}