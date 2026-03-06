//! TicketAddLabelsTool implementation.

use crate::ticket::TicketDetails;
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

/// Tool for adding labels to a ticket.
pub struct TicketAddLabelsTool;

#[derive(Debug, Deserialize)]
struct TicketAddLabelsArgs {
    /// The ticket ID.
    ticket_id: String,
    /// Labels to add.
    labels: Vec<String>,
}

#[async_trait]
impl Tool for TicketAddLabelsTool {
    fn id(&self) -> &str {
        "ticket_add_labels"
    }

    fn description(&self) -> &str {
        r#"Add labels to a ticket.

Adds the specified labels to an existing ticket without removing existing labels.
Use this when you need to tag a ticket with additional labels.

Parameters:
- ticket_id: The ticket ID (e.g., WON-123, GITHUB-456)
- labels: Array of label names to add"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ticket_id": {
                    "type": "string",
                    "description": "The ticket ID (e.g., WON-123, GITHUB-456)",
                    "minLength": 1
                },
                "labels": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Labels to add to the ticket",
                    "minItems": 1
                }
            },
            "required": ["ticket_id", "labels"]
        })
    }

    fn requires_permission(&self) -> bool {
        true // Modifies ticket
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        debug!("ticket_add_labels called with args: {:?}", args);

        let args: TicketAddLabelsArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {}", e)))?;

        let ticket_id = args.ticket_id.trim();
        if ticket_id.is_empty() {
            return Err(ToolError::validation("ticket_id cannot be empty"));
        }

        if args.labels.is_empty() {
            return Err(ToolError::validation("labels cannot be empty"));
        }

        let service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        let ticket = service
            .add_labels(ticket_id, args.labels.clone())
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        let output = format_output(ticket_id, &ticket, &args.labels);
        Ok(ToolOutput::new(
            format!("Labels added to {}", ticket_id),
            output,
        ))
    }
}

fn format_output(ticket_id: &str, ticket: &TicketDetails, labels: &[String]) -> String {
    format!(
        "Added labels to {}:\n\nLabels added: {}\n\nCurrent labels: {}\n\nTicket: {} - {}",
        ticket_id,
        labels.join(", "),
        if ticket.labels.is_empty() {
            "(none)".to_string()
        } else {
            ticket.labels.join(", ")
        },
        ticket.id,
        ticket.title
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::mock::{sample_ticket_details, MockTicketService};
    use crate::ticket::TicketError;
    use crate::ToolContext;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

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
            permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
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
            permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        }
    }

    #[test]
    fn test_tool_id() {
        let tool = TicketAddLabelsTool;
        assert_eq!(tool.id(), "ticket_add_labels");
    }

    #[test]
    fn test_tool_description() {
        let tool = TicketAddLabelsTool;
        let desc = tool.description();
        assert!(desc.contains("Add labels"));
        assert!(desc.contains("ticket"));
    }

    #[test]
    fn test_tool_requires_permission() {
        let tool = TicketAddLabelsTool;
        assert!(tool.requires_permission());
    }

    #[test]
    fn test_schema() {
        let tool = TicketAddLabelsTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("ticket_id")));
        assert!(required.contains(&json!("labels")));

        assert!(schema["properties"]["ticket_id"].is_object());
        assert!(schema["properties"]["labels"].is_object());
    }

    #[tokio::test]
    async fn test_execute_success() {
        let mock = Arc::new(MockTicketService::new());
        let mut ticket = sample_ticket_details();
        ticket.labels = vec!["bug".to_string(), "auth".to_string(), "urgent".to_string()];
        mock.set_add_labels_result(Ok(ticket));

        let ctx = create_test_context(mock.clone());
        let tool = TicketAddLabelsTool;

        let result = tool
            .execute(
                json!({
                    "ticket_id": "WON-123",
                    "labels": ["urgent"]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("WON-123"));
        assert!(output.output.contains("urgent"));

        // Verify captured args
        let (captured_id, captured_labels) = mock.get_captured_add_labels().unwrap();
        assert_eq!(captured_id, "WON-123");
        assert_eq!(captured_labels, vec!["urgent".to_string()]);
    }

    #[tokio::test]
    async fn test_execute_no_service() {
        let ctx = create_test_context_no_service();
        let tool = TicketAddLabelsTool;

        let result = tool
            .execute(
                json!({
                    "ticket_id": "WON-123",
                    "labels": ["bug"]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_empty_ticket_id() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketAddLabelsTool;

        let result = tool
            .execute(
                json!({
                    "ticket_id": "  ",
                    "labels": ["bug"]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_execute_empty_labels() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketAddLabelsTool;

        let result = tool
            .execute(
                json!({
                    "ticket_id": "WON-123",
                    "labels": []
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn test_execute_missing_args() {
        let mock = Arc::new(MockTicketService::new());
        let ctx = create_test_context(mock);
        let tool = TicketAddLabelsTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_ticket_not_found() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_add_labels_result(Err(TicketError::TicketNotFound("WON-999".to_string())));

        let ctx = create_test_context(mock);
        let tool = TicketAddLabelsTool;

        let result = tool
            .execute(
                json!({
                    "ticket_id": "WON-999",
                    "labels": ["bug"]
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("WON-999"));
    }

    #[test]
    fn test_format_output() {
        let ticket = sample_ticket_details();
        let labels = vec!["urgent".to_string()];
        let output = format_output("WON-123", &ticket, &labels);

        assert!(output.contains("Added labels to WON-123"));
        assert!(output.contains("Labels added: urgent"));
        assert!(output.contains("Current labels: bug, auth"));
    }

    #[test]
    fn test_format_output_no_labels() {
        let mut ticket = sample_ticket_details();
        ticket.labels = vec![];
        let labels = vec!["first-label".to_string()];
        let output = format_output("WON-123", &ticket, &labels);

        assert!(output.contains("Current labels: (none)"));
    }
}
