//! Artifact creation and reading tools.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ace::{ArtifactStore, ArtifactType, Priority, WorkstreamState};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};

/// ace_create_artifact tool - creates a new artifact within the ticket scope.
pub struct AceCreateArtifactTool;

#[derive(Debug, Deserialize)]
struct CreateArtifactArgs {
    #[serde(rename = "type")]
    artifact_type: String,
    title: String,
    content: String,
    #[serde(default)]
    parents: Vec<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    staging: Option<bool>,
}

#[async_trait]
impl Tool for AceCreateArtifactTool {
    fn id(&self) -> &str {
        "ace_create_artifact"
    }

    fn description(&self) -> &str {
        r#"Create a new artifact within the ticket scope.

Artifact types and their parent requirements:
- use-case: Describes a user interaction scenario. Parents to session (auto-assigned if not specified).
- requirement: A specific, testable requirement. Parent: use-case
- design: Technical design documentation. Parent: requirement
- test-case: Verification criteria. Parent: requirement
- task: Atomic unit of work. Parent: session, requirement, design, or test-case

The artifact ID is automatically generated: {TYPE}-{TICKET_ID}-{SEQUENCE}
Example: UC-WON-122-001, REQ-WON-122-003

Artifacts are created in staging by default. Use ace_submit_checkpoint to request approval."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["type", "title", "content"],
            "properties": {
                "type": {
                    "type": "string",
                    "enum": ["use-case", "requirement", "design", "test-case", "task"],
                    "description": "The type of artifact to create"
                },
                "title": {
                    "type": "string",
                    "description": "Human-readable title for the artifact"
                },
                "content": {
                    "type": "string",
                    "description": "Markdown content for the artifact body"
                },
                "parents": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Parent artifact IDs (required for requirement, design, test-case, task)"
                },
                "priority": {
                    "type": "string",
                    "enum": ["high", "medium", "low"],
                    "description": "Priority level (default: medium)"
                },
                "staging": {
                    "type": "boolean",
                    "description": "Create in staging area for review (default: true)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: CreateArtifactArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let artifact_type = ArtifactType::parse(&args.artifact_type).ok_or_else(|| {
            ToolError::validation(format!("Invalid artifact type: {}", args.artifact_type))
        })?;

        let priority = args
            .priority
            .as_deref()
            .and_then(Priority::parse)
            .unwrap_or(Priority::Medium);

        // Load state (auto-initializing if needed)
        let mut state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to initialize workstream: {e}")))?;

        // Create artifact
        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to create store: {e}")))?;

        store.ensure_directories().map_err(|e| {
            ToolError::execution_failed(format!("Failed to create directories: {e}"))
        })?;

        // Ensure session exists (auto-creates if needed)
        let session_id = store
            .ensure_session(&mut state, &ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to ensure session: {e}")))?;

        // Auto-assign session as parent for use-cases and tasks if no parent specified
        let parents = if args.parents.is_empty() {
            match artifact_type {
                ArtifactType::UseCase | ArtifactType::Task => vec![session_id],
                _ => args.parents.clone(),
            }
        } else {
            args.parents.clone()
        };

        let staging = args.staging.unwrap_or(true);
        let artifact = store
            .create_artifact(
                &mut state,
                artifact_type,
                &args.title,
                &args.content,
                parents.clone(),
                priority,
                staging,
            )
            .map_err(|e| ToolError::execution_failed(format!("Failed to create artifact: {e}")))?;

        // Save state
        state
            .save(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to save state: {e}")))?;

        let location = if staging { "staging" } else { "specs" };

        // Emit ArtifactCreated event so Documents View updates
        if let Some(ref event_tx) = ctx.event_tx {
            tracing::debug!(
                "📝 ace_create_artifact: Emitting ArtifactCreated for {}",
                artifact.metadata.id
            );
            let _ = event_tx.send(crate::ToolEvent::ArtifactCreated {
                id: artifact.metadata.id.clone(),
                artifact_type: args.artifact_type.clone(),
            });
        }

        Ok(ToolOutput::new(
            format!(
                "Created {} artifact: {}",
                args.artifact_type, artifact.metadata.id
            ),
            format!(
                "Created {} artifact:\n\n\
                 - **ID:** {}\n\
                 - **Title:** {}\n\
                 - **Location:** {}\n\
                 - **Parents:** {}\n\
                 - **Path:** {}",
                args.artifact_type,
                artifact.metadata.id,
                artifact.title,
                location,
                if parents.is_empty() {
                    "(none)".to_string()
                } else {
                    parents.join(", ")
                },
                artifact.path.display()
            ),
        )
        .with_metadata(json!({
            "id": artifact.metadata.id,
            "type": args.artifact_type,
            "path": artifact.path.to_string_lossy(),
            "staging": staging,
            "parents": parents,
        })))
    }
}

/// ace_read_artifact tool - reads artifact content and metadata.
pub struct AceReadArtifactTool;

#[derive(Debug, Deserialize)]
struct ReadArtifactArgs {
    id: String,
}

#[async_trait]
impl Tool for AceReadArtifactTool {
    fn id(&self) -> &str {
        "ace_read_artifact"
    }

    fn description(&self) -> &str {
        "Read an artifact's content and metadata by ID (e.g., UC-WON-122-001)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["id"],
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The artifact ID (e.g., UC-WON-122-001, REQ-WON-122-003)"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: ReadArtifactArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Load state to get current ticket ID
        let state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to initialize workstream: {e}")))?;

        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to create store: {e}")))?;

        let artifact = store
            .read_artifact(&args.id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to read artifact: {e}")))?
            .ok_or_else(|| {
                ToolError::execution_failed(format!("Artifact not found: {}", args.id))
            })?;

        // Verify the artifact belongs to the current workstream's ticket
        if !artifact.belongs_to_ticket(&state.ticket_id) {
            return Err(ToolError::execution_failed(format!(
                "Artifact {} does not belong to the current workstream (ticket: {}).\n\n\
                 This artifact belongs to a different ticket. Use artifacts from the current workstream only.",
                args.id, state.ticket_id
            )));
        }

        let output = format!(
            "---\n\
             id: {}\n\
             type: {}\n\
             progress: {}\n\
             parents: [{}]\n\
             priority: {}\n\
             created: {}\n\
             updated: {}\n\
             ---\n\n\
             # {}\n\n\
             {}",
            artifact.metadata.id,
            artifact.metadata.artifact_type,
            artifact.metadata.progress,
            artifact.metadata.parents.join(", "),
            artifact.metadata.priority,
            artifact.metadata.created.format("%Y-%m-%d %H:%M:%S UTC"),
            artifact.metadata.updated.format("%Y-%m-%d %H:%M:%S UTC"),
            artifact.title,
            artifact.content,
        );

        Ok(ToolOutput::new(
            format!("{}: {}", artifact.metadata.id, artifact.title),
            output,
        )
        .with_metadata(json!({
            "id": artifact.metadata.id,
            "type": artifact.metadata.artifact_type.to_string(),
            "progress": artifact.metadata.progress.to_string(),
            "parents": artifact.metadata.parents,
            "priority": artifact.metadata.priority.to_string(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn test_context(root_dir: PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test_session".to_string(),
            message_id: "test_message".to_string(),
            agent: "test".to_string(),
            abort: CancellationToken::new(),
            root_dir: root_dir.clone(),
            cwd: root_dir,
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

    #[tokio::test]
    async fn test_create_and_read_artifact() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        // Initialize state
        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        // Create UseCase (session will be auto-created via ensure_session)
        let tool = AceCreateArtifactTool;
        let result = tool
            .execute(
                json!({
                    "type": "use-case",
                    "title": "User logs in",
                    "content": "As a user, I want to log in to access my account.",
                    "staging": false
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("UC-WON-123-001"));
        // Verify session was auto-assigned as parent
        assert!(result.output.contains("SESSION-WON-123-001"));

        // Read artifact
        let read_tool = AceReadArtifactTool;
        let read_result = read_tool
            .execute(
                json!({
                    "id": "UC-WON-123-001"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(read_result.output.contains("User logs in"));
        assert!(read_result.output.contains("As a user"));
    }

    #[tokio::test]
    async fn test_create_requirement_needs_parent() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let tool = AceCreateArtifactTool;
        let result = tool
            .execute(
                json!({
                    "type": "requirement",
                    "title": "Must validate email",
                    "content": "The system must validate email format."
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("requires at least one parent"));
    }
}