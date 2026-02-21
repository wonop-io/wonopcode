//! TicketReadTool implementation.

use crate::ticket::TicketDetails;
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

/// Tool for reading detailed ticket information.
pub struct TicketReadTool;

#[derive(Debug, Deserialize)]
struct TicketReadArgs {
    /// The ticket ID to read.
    ticket_id: String,
    /// Whether to include comments (default: true).
    #[serde(default = "default_true")]
    include_comments: bool,
    /// Whether to include attachments (default: false).
    #[serde(default)]
    include_attachments: bool,
}

fn default_true() -> bool {
    true
}

#[async_trait]
impl Tool for TicketReadTool {
    fn id(&self) -> &str {
        "ticket_read"
    }

    fn description(&self) -> &str {
        r#"Read detailed information about a specific ticket.

Use this tool to get comprehensive details about a ticket including:
- Full description (markdown preserved)
- Current status and assignee
- Labels and metadata
- Comments (optional, enabled by default)
- Attachments (optional, disabled by default)

Provide the ticket ID (e.g., WON-167, GITHUB-123) to fetch its details."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["ticket_id"],
            "properties": {
                "ticket_id": {
                    "type": "string",
                    "description": "The unique identifier of the ticket (e.g., WON-167, GITHUB-123)"
                },
                "include_comments": {
                    "type": "boolean",
                    "default": true,
                    "description": "Include ticket comments in the response"
                },
                "include_attachments": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include attachment metadata in the response"
                }
            }
        })
    }

    fn requires_permission(&self) -> bool {
        false // Read-only operation
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TicketReadArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Validate ticket ID
        let ticket_id = args.ticket_id.trim();
        if ticket_id.is_empty() {
            return Err(ToolError::validation("Ticket ID cannot be empty"));
        }

        debug!(
            ticket_id = %ticket_id,
            include_comments = args.include_comments,
            include_attachments = args.include_attachments,
            "Reading ticket"
        );

        // Get ticket service from context
        let ticket_service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        // Fetch ticket details
        let ticket = ticket_service
            .get_ticket(ticket_id, args.include_comments, args.include_attachments)
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Format output
        let output =
            format_ticket_details(&ticket, args.include_comments, args.include_attachments);

        Ok(
            ToolOutput::new(format!("{}: {}", ticket.id, ticket.title), output).with_metadata(
                json!({
                    "ticket_id": ticket.id,
                    "status": ticket.status.as_str(),
                    "tracker": ticket.tracker_name,
                    "comments_count": ticket.comments.len(),
                    "attachments_count": ticket.attachments.len()
                }),
            ),
        )
    }
}

/// Format ticket details as markdown.
fn format_ticket_details(
    ticket: &TicketDetails,
    include_comments: bool,
    include_attachments: bool,
) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!("# {}: {}\n\n", ticket.id, ticket.title));

    // Metadata section
    output.push_str("## Details\n\n");
    output.push_str(&format!("**Status:** {}\n", ticket.status));

    if let Some(assignee) = &ticket.assignee {
        output.push_str(&format!("**Assignee:** {}\n", assignee));
    } else {
        output.push_str("**Assignee:** Unassigned\n");
    }

    if !ticket.labels.is_empty() {
        output.push_str(&format!("**Labels:** {}\n", ticket.labels.join(", ")));
    }

    output.push_str(&format!("**Tracker:** {}\n", ticket.tracker_name));

    if let Some(url) = &ticket.url {
        output.push_str(&format!("**URL:** {}\n", url));
    }

    output.push_str(&format!(
        "**Created:** {}\n",
        ticket.created_at.format("%Y-%m-%d %H:%M UTC")
    ));
    output.push_str(&format!(
        "**Updated:** {}\n",
        ticket.updated_at.format("%Y-%m-%d %H:%M UTC")
    ));

    // Description
    output.push_str("\n## Description\n\n");
    if let Some(description) = &ticket.description {
        if description.trim().is_empty() {
            output.push_str("_No description provided._\n");
        } else {
            output.push_str(description);
            output.push('\n');
        }
    } else {
        output.push_str("_No description provided._\n");
    }

    // Comments section
    if include_comments {
        output.push_str(&format!("\n## Comments ({})\n\n", ticket.comments.len()));

        if ticket.comments.is_empty() {
            output.push_str("_No comments yet._\n");
        } else {
            for comment in &ticket.comments {
                output.push_str(&format!(
                    "### {} ({})\n\n",
                    comment.author,
                    comment.created_at.format("%Y-%m-%d %H:%M")
                ));
                output.push_str(&comment.body);
                output.push_str("\n\n---\n\n");
            }
        }
    }

    // Attachments section
    if include_attachments {
        output.push_str(&format!(
            "\n## Attachments ({})\n\n",
            ticket.attachments.len()
        ));

        if ticket.attachments.is_empty() {
            output.push_str("_No attachments._\n");
        } else {
            output.push_str("| Filename | Size | Type |\n");
            output.push_str("|:---------|:-----|:-----|\n");
            for attachment in &ticket.attachments {
                let size = format_file_size(attachment.size);
                let mime = attachment.mime_type.as_deref().unwrap_or("unknown");
                output.push_str(&format!(
                    "| {} | {} | {} |\n",
                    attachment.filename, size, mime
                ));
            }
        }
    }

    output
}

/// Format file size in human-readable format.
fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::mock::{sample_ticket_details, MockTicketService};
    use crate::ticket::{TicketAttachment, TicketComment, TicketError, TicketStatus, TicketUser};
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    fn sample_ticket() -> TicketDetails {
        TicketDetails {
            id: "WON-167".to_string(),
            title: "Add ticket tools".to_string(),
            description: Some("Implement ticket management tools for the agent.".to_string()),
            status: TicketStatus::InProgress,
            assignee: Some(TicketUser {
                id: "1".to_string(),
                username: "developer".to_string(),
                name: Some("Dev User".to_string()),
            }),
            labels: vec!["feature".to_string(), "tools".to_string()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            comments: vec![],
            attachments: vec![],
            tracker_name: "Linear".to_string(),
            url: Some("https://linear.app/team/WON-167".to_string()),
        }
    }

    #[test]
    fn test_ticket_read_tool_id() {
        let tool = TicketReadTool;
        assert_eq!(tool.id(), "ticket_read");
    }

    #[test]
    fn test_ticket_read_tool_description() {
        let tool = TicketReadTool;
        let desc = tool.description();
        assert!(desc.contains("detailed information"));
        assert!(desc.contains("ticket"));
    }

    #[test]
    fn test_ticket_read_tool_schema() {
        let tool = TicketReadTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("ticket_id")));

        assert!(schema["properties"]["ticket_id"].is_object());
        assert!(schema["properties"]["include_comments"].is_object());
        assert!(schema["properties"]["include_attachments"].is_object());
    }

    #[test]
    fn test_format_ticket_details_basic() {
        let ticket = sample_ticket();
        let output = format_ticket_details(&ticket, false, false);

        assert!(output.contains("# WON-167: Add ticket tools"));
        assert!(output.contains("**Status:** in-progress"));
        assert!(output.contains("Dev User (@developer)"));
        assert!(output.contains("feature, tools"));
        assert!(output.contains("Linear"));
        assert!(output.contains("Implement ticket management"));
    }

    #[test]
    fn test_format_ticket_details_with_comments() {
        let mut ticket = sample_ticket();
        ticket.comments = vec![TicketComment {
            id: "c1".to_string(),
            author: TicketUser {
                id: "2".to_string(),
                username: "reviewer".to_string(),
                name: None,
            },
            body: "Looks good!".to_string(),
            created_at: Utc::now(),
        }];

        let output = format_ticket_details(&ticket, true, false);
        assert!(output.contains("## Comments (1)"));
        assert!(output.contains("@reviewer"));
        assert!(output.contains("Looks good!"));
    }

    #[test]
    fn test_format_ticket_details_with_attachments() {
        let mut ticket = sample_ticket();
        ticket.attachments = vec![TicketAttachment {
            id: "a1".to_string(),
            filename: "screenshot.png".to_string(),
            size: 1024 * 500, // 500 KB
            mime_type: Some("image/png".to_string()),
            url: Some("https://example.com/screenshot.png".to_string()),
        }];

        let output = format_ticket_details(&ticket, false, true);
        assert!(output.contains("## Attachments (1)"));
        assert!(output.contains("screenshot.png"));
        assert!(output.contains("500.0 KB"));
        assert!(output.contains("image/png"));
    }

    #[test]
    fn test_format_ticket_details_no_description() {
        let mut ticket = sample_ticket();
        ticket.description = None;

        let output = format_ticket_details(&ticket, false, false);
        assert!(output.contains("_No description provided._"));
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_file_size(1536), "1.5 KB");
    }

    #[test]
    fn test_ticket_read_args_defaults() {
        let args: TicketReadArgs = serde_json::from_value(json!({
            "ticket_id": "WON-123"
        }))
        .unwrap();

        assert_eq!(args.ticket_id, "WON-123");
        assert!(args.include_comments); // default true
        assert!(!args.include_attachments); // default false
    }

    #[test]
    fn test_ticket_read_args_custom() {
        let args: TicketReadArgs = serde_json::from_value(json!({
            "ticket_id": "WON-456",
            "include_comments": false,
            "include_attachments": true
        }))
        .unwrap();

        assert_eq!(args.ticket_id, "WON-456");
        assert!(!args.include_comments);
        assert!(args.include_attachments);
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
        }
    }

    #[tokio::test]
    async fn test_execute_read_success() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_get_result(Ok(sample_ticket_details()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool.execute(json!({"ticket_id": "WON-123"}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("WON-123"));
        assert!(output.output.contains("Fix authentication bug"));
        assert!(output.output.contains("open"));

        // Verify ticket ID was captured
        let captured_id = mock.get_captured_ticket_id().unwrap();
        assert_eq!(captured_id, "WON-123");

        // Verify metadata
        assert_eq!(output.metadata["ticket_id"], "WON-123");
        assert_eq!(output.metadata["status"], "open");
        assert_eq!(output.metadata["tracker"], "Linear");
    }

    #[tokio::test]
    async fn test_execute_read_with_comments() {
        let mock = Arc::new(MockTicketService::new());
        let ticket = sample_ticket_details();
        mock.set_get_result(Ok(ticket));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool
            .execute(
                json!({"ticket_id": "WON-123", "include_comments": true}),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("## Comments"));
        assert!(output.output.contains("I can reproduce this issue"));
    }

    #[tokio::test]
    async fn test_execute_read_with_attachments() {
        let mock = Arc::new(MockTicketService::new());
        let ticket = sample_ticket_details();
        mock.set_get_result(Ok(ticket));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool
            .execute(
                json!({"ticket_id": "WON-123", "include_attachments": true}),
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("## Attachments"));
        assert!(output.output.contains("screenshot.png"));
    }

    #[tokio::test]
    async fn test_execute_read_no_ticket_service() {
        let ctx = create_test_context_no_service();
        let tool = TicketReadTool;

        let result = tool.execute(json!({"ticket_id": "WON-123"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_read_empty_ticket_id() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketReadTool;

        let result = tool.execute(json!({"ticket_id": "  "}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_execute_read_missing_ticket_id() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketReadTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Invalid arguments"));
    }

    #[tokio::test]
    async fn test_execute_read_ticket_not_found() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_get_result(Err(TicketError::TicketNotFound("WON-999".to_string())));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool.execute(json!({"ticket_id": "WON-999"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("WON-999"));
    }

    #[tokio::test]
    async fn test_execute_read_tracker_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_get_result(Err(TicketError::TrackerError("API timeout".to_string())));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool.execute(json!({"ticket_id": "WON-123"}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("API timeout"));
    }

    #[tokio::test]
    async fn test_execute_read_trims_ticket_id() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_get_result(Ok(sample_ticket_details()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool
            .execute(json!({"ticket_id": "  WON-123  "}), &ctx)
            .await;
        assert!(result.is_ok());

        let captured_id = mock.get_captured_ticket_id().unwrap();
        assert_eq!(captured_id, "WON-123");
    }

    #[tokio::test]
    async fn test_execute_read_metadata_counts() {
        let mock = Arc::new(MockTicketService::new());
        let ticket = sample_ticket_details();
        mock.set_get_result(Ok(ticket));

        let ctx = create_test_context(mock.clone());
        let tool = TicketReadTool;

        let result = tool.execute(json!({"ticket_id": "WON-123"}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.metadata["comments_count"], 1);
        assert_eq!(output.metadata["attachments_count"], 1);
    }
}
