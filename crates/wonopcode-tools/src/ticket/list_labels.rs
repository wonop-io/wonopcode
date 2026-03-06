//! TicketListLabelsTool implementation.

use crate::ticket::LabelInfo;
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::debug;

/// Tool for listing available labels across trackers.
pub struct TicketListLabelsTool;

#[derive(Debug, Deserialize, Default)]
struct TicketListLabelsArgs {
    /// Optional: filter to a specific tracker.
    #[serde(default)]
    tracker_id: Option<String>,
}

#[async_trait]
impl Tool for TicketListLabelsTool {
    fn id(&self) -> &str {
        "ticket_list_labels"
    }

    fn description(&self) -> &str {
        r#"List available labels across all configured trackers.

Returns label names, colors, and descriptions organized by tracker.
Use this to see what labels are available for tagging tickets.

Parameters:
- tracker_id: (optional) Filter to a specific tracker"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tracker_id": {
                    "type": "string",
                    "description": "Optional: filter to a specific tracker"
                }
            }
        })
    }

    fn requires_permission(&self) -> bool {
        false // Read-only operation
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        debug!("ticket_list_labels called with args: {:?}", args);

        let args: TicketListLabelsArgs = serde_json::from_value(args).unwrap_or_default();

        let service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        let labels = service
            .list_labels()
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        let output = format_output(&labels, args.tracker_id.as_deref());
        let label_count = labels.len();
        Ok(ToolOutput::new(
            format!("Found {} labels", label_count),
            output,
        ))
    }
}

fn format_output(labels: &[LabelInfo], tracker_filter: Option<&str>) -> String {
    // Group by tracker
    let mut by_tracker: HashMap<String, Vec<&LabelInfo>> = HashMap::new();

    for label in labels {
        if let Some(filter) = tracker_filter {
            if !label.tracker_name.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }
        by_tracker
            .entry(label.tracker_name.clone())
            .or_default()
            .push(label);
    }

    if by_tracker.is_empty() {
        return "No labels found in any tracker.".to_string();
    }

    let mut output = String::new();
    for (tracker, tracker_labels) in by_tracker {
        output.push_str(&format!("## {}\n\n", tracker));
        if tracker_labels.is_empty() {
            output.push_str("(no labels)\n\n");
        } else {
            for label in tracker_labels {
                let color = label
                    .color
                    .as_ref()
                    .map(|c| format!(" `#{}`", c))
                    .unwrap_or_default();
                let desc = label
                    .description
                    .as_ref()
                    .map(|d| format!(" - {}", d))
                    .unwrap_or_default();
                output.push_str(&format!("- **{}**{}{}\n", label.name, color, desc));
            }
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::mock::{sample_label_info, MockTicketService};
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
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        }
    }

    #[test]
    fn test_tool_id() {
        let tool = TicketListLabelsTool;
        assert_eq!(tool.id(), "ticket_list_labels");
    }

    #[test]
    fn test_tool_description() {
        let tool = TicketListLabelsTool;
        let desc = tool.description();
        assert!(desc.contains("List"));
        assert!(desc.contains("labels"));
    }

    #[test]
    fn test_tool_does_not_require_permission() {
        let tool = TicketListLabelsTool;
        assert!(!tool.requires_permission()); // Read-only operation
    }

    #[test]
    fn test_schema() {
        let tool = TicketListLabelsTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");

        // tracker_id is optional, so no required field needed
        assert!(schema["properties"]["tracker_id"].is_object());
    }

    #[tokio::test]
    async fn test_execute_success() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_labels_result(Ok(sample_label_info()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListLabelsTool;

        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("bug"));
        assert!(output.output.contains("feature"));
        assert!(output.output.contains("GitHub"));
    }

    #[tokio::test]
    async fn test_execute_with_filter() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_labels_result(Ok(sample_label_info()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListLabelsTool;

        let result = tool
            .execute(json!({"tracker_id": "Linear"}), &ctx)
            .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        // Should only include Linear labels
        assert!(output.output.contains("enhancement"));
        assert!(output.output.contains("Linear"));
    }

    #[tokio::test]
    async fn test_execute_empty_labels() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_labels_result(Ok(vec![]));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListLabelsTool;

        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("No labels found"));
    }

    #[tokio::test]
    async fn test_execute_no_service() {
        let ctx = create_test_context_no_service();
        let tool = TicketListLabelsTool;

        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_tracker_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_list_labels_result(Err(TicketError::TrackerError("API error".to_string())));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListLabelsTool;

        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("API error"));
    }

    #[test]
    fn test_format_output_with_labels() {
        let labels = sample_label_info();
        let output = format_output(&labels, None);

        assert!(output.contains("GitHub"));
        assert!(output.contains("bug"));
        assert!(output.contains("feature"));
        assert!(output.contains("#d73a4a"));
        assert!(output.contains("Something isn't working"));
    }

    #[test]
    fn test_format_output_no_labels() {
        let labels: Vec<LabelInfo> = vec![];
        let output = format_output(&labels, None);

        assert!(output.contains("No labels found"));
    }

    #[test]
    fn test_format_output_with_filter() {
        let labels = sample_label_info();
        let output = format_output(&labels, Some("Linear"));

        assert!(output.contains("Linear"));
        assert!(output.contains("enhancement"));
        assert!(!output.contains("bug")); // GitHub labels filtered out
    }

    #[test]
    fn test_format_output_filter_no_match() {
        let labels = sample_label_info();
        let output = format_output(&labels, Some("nonexistent"));

        assert!(output.contains("No labels found"));
    }
}
