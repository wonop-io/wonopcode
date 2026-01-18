//! Plan mode tools for entering and exiting planning mode.
//!
//! These tools allow the AI to switch between the default "build" agent
//! and the "plan" agent which has restricted permissions (read-only).

use crate::{Tool, ToolContext, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// Tool to enter plan mode (switches to "plan" agent).
pub struct EnterPlanModeTool;

impl EnterPlanModeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnterPlanModeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct EnterPlanModeArgs {
    /// Optional reason for entering plan mode.
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn id(&self) -> &str {
        "enterplanmode"
    }

    fn description(&self) -> &str {
        r#"Enter plan mode for thinking and planning without making changes.

In plan mode, you have restricted permissions:
- Read-only access to files (read, glob, grep, list)
- Read-only bash commands (ls, cat, git log, etc.)
- No write, edit, or destructive operations

Use plan mode when you need to:
- Think through a complex problem
- Analyze code before making changes
- Create a detailed plan before implementation
- Explore the codebase safely

Call ExitPlanMode when you're ready to implement your plan."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional reason for entering plan mode"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: EnterPlanModeArgs =
            serde_json::from_value(args).unwrap_or(EnterPlanModeArgs { reason: None });

        let current_agent = &ctx.agent;

        // Already in plan mode
        if current_agent == "plan" {
            return Ok(ToolOutput::new(
                "Already in plan mode",
                "You are already in plan mode. Continue planning or call ExitPlanMode when ready to implement.",
            ));
        }

        let message = if let Some(reason) = &args.reason {
            format!(
                "Entered plan mode. Reason: {reason}\n\nYou now have read-only access. Use this time to analyze and plan. Call ExitPlanMode when ready to implement."
            )
        } else {
            "Entered plan mode.\n\nYou now have read-only access. Use this time to analyze and plan. Call ExitPlanMode when ready to implement.".to_string()
        };

        Ok(
            ToolOutput::new("Entered plan mode", message).with_metadata(json!({
                "agent_change": "plan",
                "previous_agent": current_agent
            })),
        )
    }
}

/// Tool to exit plan mode (switches back to "build" agent).
pub struct ExitPlanModeTool;

impl ExitPlanModeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExitPlanModeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ExitPlanModeArgs {
    /// Optional summary of the plan.
    #[serde(default)]
    summary: Option<String>,
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn id(&self) -> &str {
        "exitplanmode"
    }

    fn description(&self) -> &str {
        r#"Exit plan mode and return to build mode for implementation.

After exiting plan mode, you will have full permissions again:
- Read and write access to files
- Full bash command access
- Ability to edit and create files

Call this when you're ready to implement your plan."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Optional summary of the plan you created"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: ExitPlanModeArgs =
            serde_json::from_value(args).unwrap_or(ExitPlanModeArgs { summary: None });

        let current_agent = &ctx.agent;

        // Not in plan mode
        if current_agent != "plan" {
            return Ok(ToolOutput::new(
                "Not in plan mode",
                format!(
                    "You are currently in {current_agent} mode, not plan mode. No change needed."
                ),
            ));
        }

        let message = if let Some(summary) = &args.summary {
            format!(
                "Exited plan mode. Plan summary: {summary}\n\nYou now have full permissions. Proceed with implementation."
            )
        } else {
            "Exited plan mode.\n\nYou now have full permissions. Proceed with implementation."
                .to_string()
        };

        Ok(
            ToolOutput::new("Exited plan mode", message).with_metadata(json!({
                "agent_change": "build",
                "previous_agent": "plan"
            })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    fn create_test_context(agent: &str) -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            agent: agent.to_string(),
            abort: CancellationToken::new(),
            root_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            snapshot: None,
            file_time: None,
            sandbox: None,
            event_tx: None,
        }
    }

    #[tokio::test]
    async fn test_enter_plan_mode() {
        let tool = EnterPlanModeTool::new();
        let ctx = create_test_context("build");
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("Entered plan mode"));
        let metadata = result.metadata.as_object().unwrap();
        assert_eq!(metadata.get("agent_change").unwrap(), "plan");
    }

    #[tokio::test]
    async fn test_enter_plan_mode_already_in_plan() {
        let tool = EnterPlanModeTool::new();
        let ctx = create_test_context("plan");
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("already in plan mode"));
        // No agent_change metadata when already in plan mode
        assert!(result.metadata.get("agent_change").is_none());
    }

    #[tokio::test]
    async fn test_exit_plan_mode() {
        let tool = ExitPlanModeTool::new();
        let ctx = create_test_context("plan");
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("Exited plan mode"));
        let metadata = result.metadata.as_object().unwrap();
        assert_eq!(metadata.get("agent_change").unwrap(), "build");
    }

    #[tokio::test]
    async fn test_exit_plan_mode_not_in_plan() {
        let tool = ExitPlanModeTool::new();
        let ctx = create_test_context("build");
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("not plan mode"));
        // No agent_change metadata when not in plan mode
        assert!(result.metadata.get("agent_change").is_none());
    }
}
