//! TicketListTrackersTool implementation.

use crate::ticket::TrackerInfo;
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::debug;

/// Tool for listing available issue trackers.
pub struct TicketListTrackersTool;

#[async_trait]
impl Tool for TicketListTrackersTool {
    fn id(&self) -> &str {
        "ticket_list_trackers"
    }

    fn description(&self) -> &str {
        r#"List all configured issue trackers.

Use this tool to discover available trackers before using other ticket tools.
Returns information about each configured tracker including:
- id: Unique identifier to use with other ticket tools (e.g., ticket_list, ticket_create)
- name: Human-readable name
- type: Tracker type (github, linear, etc.)
- enabled: Whether the tracker is currently enabled

Use the tracker ID when you want to target a specific tracker for operations."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn requires_permission(&self) -> bool {
        false // Read-only operation
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        debug!("Listing available trackers");

        // Get ticket service from context
        let ticket_service = ctx.ticket_service.as_ref().ok_or_else(|| {
            ToolError::execution_failed(
                "Ticket service not available. Please ensure trackers are configured.",
            )
        })?;

        // Get all trackers
        let trackers = ticket_service
            .list_trackers()
            .await
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        // Get current default tracker for context
        let default_tracker_id = ctx.workstream_default_tracker_id.clone();

        // Format output
        let output = format_tracker_list(&trackers, default_tracker_id.as_deref());

        let enabled_count = trackers.iter().filter(|t| t.enabled).count();

        Ok(
            ToolOutput::new("Available trackers", output).with_metadata(json!({
                "total_count": trackers.len(),
                "enabled_count": enabled_count,
                "default_tracker_id": default_tracker_id
            })),
        )
    }
}

/// Format tracker list as a markdown table.
fn format_tracker_list(trackers: &[TrackerInfo], default_id: Option<&str>) -> String {
    if trackers.is_empty() {
        return "No issue trackers configured.\n\nTo configure trackers, go to Settings > Integrations.".to_string();
    }

    let mut output = format!("## Available Trackers ({} total)\n\n", trackers.len());

    if let Some(default) = default_id {
        output.push_str(&format!("**Current default tracker:** `{}`\n\n", default));
    }

    output.push_str("| ID | Name | Type | Enabled | Default |\n");
    output.push_str("|:---|:-----|:-----|:--------|:--------|\n");

    for tracker in trackers {
        let enabled_str = if tracker.enabled { "✓" } else { "✗" };
        let is_default = default_id.map(|d| d == tracker.id).unwrap_or(false);
        let default_str = if is_default { "★" } else { "" };

        output.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            tracker.id, tracker.name, tracker.tracker_type, enabled_str, default_str
        ));
    }

    output.push_str("\n**Usage tips:**\n");
    output.push_str("- Use the tracker ID with `tracker_id` parameter in `ticket_list`, `ticket_search`, and `ticket_create`\n");
    output.push_str("- Example: `ticket_list` with `tracker_id: \"linear-1\"` to list only Linear tickets\n");
    if default_id.is_some() {
        output.push_str("- The default tracker (★) is used when no `tracker_id` is specified\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::mock::MockTicketService;
    use crate::ticket::TicketError;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn test_ticket_list_trackers_tool_id() {
        let tool = TicketListTrackersTool;
        assert_eq!(tool.id(), "ticket_list_trackers");
    }

    #[test]
    fn test_ticket_list_trackers_tool_description() {
        let tool = TicketListTrackersTool;
        let desc = tool.description();
        assert!(desc.contains("List"));
        assert!(desc.contains("tracker"));
    }

    #[test]
    fn test_ticket_list_trackers_tool_schema() {
        let tool = TicketListTrackersTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn test_format_tracker_list_empty() {
        let trackers: Vec<TrackerInfo> = vec![];
        let output = format_tracker_list(&trackers, None);
        assert!(output.contains("No issue trackers configured"));
    }

    #[test]
    fn test_format_tracker_list_with_trackers() {
        let trackers = vec![
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
            TrackerInfo {
                id: "jira-1".to_string(),
                name: "Jira".to_string(),
                tracker_type: "jira".to_string(),
                enabled: false,
            },
        ];

        let output = format_tracker_list(&trackers, Some("linear-1"));
        assert!(output.contains("Available Trackers (3 total)"));
        assert!(output.contains("linear-1"));
        assert!(output.contains("github-1"));
        assert!(output.contains("jira-1"));
        assert!(output.contains("Linear"));
        assert!(output.contains("GitHub"));
        assert!(output.contains("✓")); // enabled
        assert!(output.contains("✗")); // disabled
        assert!(output.contains("★")); // default marker
        assert!(output.contains("Current default tracker"));
    }

    #[test]
    fn test_format_tracker_list_no_default() {
        let trackers = vec![TrackerInfo {
            id: "linear-1".to_string(),
            name: "Linear".to_string(),
            tracker_type: "linear".to_string(),
            enabled: true,
        }];

        let output = format_tracker_list(&trackers, None);
        assert!(!output.contains("Current default tracker"));
        assert!(!output.contains("★"));
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
        }
    }

    fn sample_tracker_info() -> Vec<TrackerInfo> {
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

    #[tokio::test]
    async fn test_execute_success() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_trackers_result(Ok(sample_tracker_info()));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTrackersTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("linear-1"));
        assert!(output.output.contains("github-1"));
        assert!(output.output.contains("Linear"));
        assert!(output.output.contains("GitHub"));

        // Verify metadata
        assert_eq!(output.metadata["total_count"], 2);
        assert_eq!(output.metadata["enabled_count"], 2);
    }

    #[tokio::test]
    async fn test_execute_with_default_tracker() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_trackers_result(Ok(sample_tracker_info()));

        let mut ctx = create_test_context(mock.clone());
        ctx.workstream_default_tracker_id = Some("linear-1".to_string());

        let tool = TicketListTrackersTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("Current default tracker"));
        assert!(output.output.contains("★")); // Default marker

        // Verify metadata
        assert_eq!(output.metadata["default_tracker_id"], "linear-1");
    }

    #[tokio::test]
    async fn test_execute_empty_trackers() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_trackers_result(Ok(vec![]));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTrackersTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.output.contains("No issue trackers configured"));

        // Verify metadata
        assert_eq!(output.metadata["total_count"], 0);
        assert_eq!(output.metadata["enabled_count"], 0);
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
            ace_service: None,
            permission_checker: None,
            workstream_ticket_id: None,
            workstream_default_tracker_id: None,
        };

        let tool = TicketListTrackersTool;
        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Ticket service not available"));
    }

    #[tokio::test]
    async fn test_execute_tracker_error() {
        let mock = Arc::new(MockTicketService::new());
        mock.set_trackers_result(Err(TicketError::TrackerError(
            "Connection failed".to_string(),
        )));

        let ctx = create_test_context(mock.clone());
        let tool = TicketListTrackersTool;

        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.to_string().contains("Connection failed"));
    }
}
