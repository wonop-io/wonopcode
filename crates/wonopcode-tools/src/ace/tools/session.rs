//! Session management tools (ace_session_log).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ace::{ArtifactStore, SessionLogImportance, WorkstreamState};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};

/// ace_session_log tool - append entries to the session changelog.
pub struct AceSessionLogTool;

#[derive(Debug, Deserialize)]
struct SessionLogArgs {
    importance: String,
    message: String,
    #[serde(default)]
    indent: usize,
}

#[async_trait]
impl Tool for AceSessionLogTool {
    fn id(&self) -> &str {
        "ace_session_log"
    }

    fn description(&self) -> &str {
        r#"Append an entry to the session changelog.

Use this tool to record important decisions, discoveries, and context.

Importance levels:
- important (🔴): Key decisions, critical info, user requirements
- maybe_important (🟡): Worth noting but not critical
- info_only (🟢): General observations, progress updates

Example:
```json
{
  "importance": "important",
  "message": "User clarified: admin routes need role-based access",
  "indent": 0
}
```"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["importance", "message"],
            "properties": {
                "importance": {
                    "type": "string",
                    "enum": ["important", "maybe_important", "info_only"],
                    "description": "Importance level"
                },
                "message": {
                    "type": "string",
                    "description": "The message to log"
                },
                "indent": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "default": 0,
                    "description": "Indentation level"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: SessionLogArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let importance = SessionLogImportance::parse(&args.importance).ok_or_else(|| {
            ToolError::validation(format!(
                "Invalid importance level: {}. Use: important, maybe_important, info_only",
                args.importance
            ))
        })?;

        let mut state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to load workstream: {e}")))?;

        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to create store: {e}")))?;

        // Ensure session exists (auto-creates if needed)
        let session_id = store
            .ensure_session(&mut state, &ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to ensure session: {e}")))?;

        store
            .append_session_log(&session_id, importance, &args.message, args.indent)
            .map_err(|e| ToolError::execution_failed(format!("Failed to append log: {e}")))?;

        let emoji = importance.emoji();
        Ok(ToolOutput::new(
            format!("{} Logged to session", emoji),
            format!("Added to session changelog:\n\n{} {}", emoji, args.message),
        ))
    }
}
