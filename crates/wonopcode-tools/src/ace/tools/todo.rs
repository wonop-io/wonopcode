//! Task management tools (todoread, todowrite, ace_todo_update).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::ace::{Artifact, ArtifactStore, ArtifactType, Priority, Progress, WorkstreamState};
use crate::todo::{Phase, PhasedTodos, TodoItem, TodoPriority, TodoStatus};
use crate::{Tool, ToolContext, ToolError, ToolEvent, ToolOutput, ToolResult};

// ============================================================================
// Helper Functions for Plan View Integration
// ============================================================================

/// Build PhasedTodos from ACE artifacts for UI synchronization.
///
/// This converts ACE artifacts into the PhasedTodos format expected by the
/// Plan View in the desktop UI.
///
/// Tasks are grouped by their `phase` field. Tasks without a phase are placed
/// in an "Unphased" group for backwards compatibility.
///
/// Parent artifacts (requirements, designs, test-cases) are added to the plan
/// once all their child tasks are done, with the following rules:
/// - Requirements must be validated before their test-cases can be implemented
/// - Designs are shown after their parent requirement's tasks are done
/// - Test-cases are shown after their parent requirement is validated
fn build_phased_todos_from_tasks(tasks: &[Artifact], all_artifacts: &[Artifact]) -> PhasedTodos {
    let mut phased_todos = PhasedTodos::new();
    let mut phase_map: HashMap<String, Vec<&Artifact>> = HashMap::new();
    
    // Group tasks by phase
    for task in tasks {
        let phase_name = task
            .metadata
            .phase
            .clone()
            .unwrap_or_else(|| "Unphased".to_string());
        phase_map.entry(phase_name).or_default().push(task);
    }

    // Collect all phase names and sort them
    // We want a consistent ordering: defined phases first (in the order they appear),
    // then "Unphased" at the end
    let mut phase_names: Vec<String> = phase_map.keys().cloned().collect();
    phase_names.sort_by(|a, b| {
        if a == "Unphased" {
            std::cmp::Ordering::Greater
        } else if b == "Unphased" {
            std::cmp::Ordering::Less
        } else {
            a.cmp(b)
        }
    });

    // Build task phases
    for phase_name in &phase_names {
        if let Some(phase_tasks) = phase_map.get(phase_name) {
            let phase_id = phase_name.to_lowercase().replace(' ', "_");
            let mut phase = Phase::new(phase_id, phase_name.clone());
            
            for task in phase_tasks {
                let todo_status = match task.metadata.progress {
                    Progress::InProgress => TodoStatus::InProgress,
                    Progress::Done => TodoStatus::Completed,
                    Progress::Discarded => TodoStatus::Cancelled,
                    _ => TodoStatus::Pending,
                };
                let todo_priority = match task.metadata.priority {
                    Priority::High => TodoPriority::High,
                    Priority::Medium => TodoPriority::Medium,
                    Priority::Low => TodoPriority::Low,
                };
                phase.add_todo(TodoItem {
                    id: task.metadata.id.clone(),
                    content: task.title.clone(),
                    status: todo_status,
                    priority: todo_priority,
                    parents: task.metadata.parents.clone(),
                });
            }
            phased_todos.add_phase(phase);
        }
    }

    // Add parent artifacts that have all children done
    // This creates a "Validation" phase for artifacts ready to be validated
    let validation_items = collect_validation_items(tasks, all_artifacts);
    if !validation_items.is_empty() {
        let mut validation_phase = Phase::new("validation".to_string(), "Validation".to_string());
        for item in validation_items {
            validation_phase.add_todo(item);
        }
        phased_todos.add_phase(validation_phase);
    }

    phased_todos
}

/// Collect parent artifacts that are ready for validation.
///
/// An artifact is ready for validation when:
/// - All its child tasks are done
/// - For requirements: no additional constraints
/// - For test-cases: their parent requirement must be validated (done) first
fn collect_validation_items(tasks: &[Artifact], all_artifacts: &[Artifact]) -> Vec<TodoItem> {
    let mut items = Vec::new();
    
    // Build a map of parent_id -> child tasks
    let mut children_by_parent: HashMap<String, Vec<&Artifact>> = HashMap::new();
    for task in tasks {
        for parent_id in &task.metadata.parents {
            children_by_parent
                .entry(parent_id.clone())
                .or_default()
                .push(task);
        }
    }

    // Find artifacts that have all children done
    // We need to check requirements, designs, and test-cases
    for artifact in all_artifacts {
        // Skip tasks - they don't have children in this context
        if artifact.metadata.artifact_type == ArtifactType::Task {
            continue;
        }

        // Skip artifacts that are already done or discarded
        if artifact.metadata.progress.is_terminal() {
            continue;
        }

        // Get child tasks for this artifact
        let children = children_by_parent.get(&artifact.metadata.id);
        
        // If no children, skip (nothing to validate)
        let children = match children {
            Some(c) if !c.is_empty() => c,
            _ => continue,
        };

        // Check if all children are done
        let all_children_done = children.iter().all(|c| c.metadata.progress == Progress::Done);
        if !all_children_done {
            continue;
        }

        // For test-cases, check if parent requirement is validated
        if artifact.metadata.artifact_type == ArtifactType::TestCase {
            // Find the parent requirement
            let parent_req = artifact
                .metadata
                .parents
                .iter()
                .find_map(|parent_id| {
                    all_artifacts.iter().find(|a| {
                        a.metadata.id == *parent_id
                            && a.metadata.artifact_type == ArtifactType::Requirement
                    })
                });

            // If parent requirement exists and is not done, skip this test-case
            if let Some(req) = parent_req {
                if req.metadata.progress != Progress::Done {
                    continue;
                }
            }
        }

        // Add the artifact as a validation item
        let type_prefix = match artifact.metadata.artifact_type {
            ArtifactType::Session => "SESSION",
            ArtifactType::UseCase => "UC",
            ArtifactType::Requirement => "REQ",
            ArtifactType::Design => "DES",
            ArtifactType::TestCase => "TC",
            ArtifactType::Task => "TASK",
        };

        let content = format!("Validate {} {}", type_prefix, artifact.title);
        
        // Mark as in_progress if currently ready_to_validate
        let status = if artifact.metadata.progress == Progress::ReadyToValidate {
            TodoStatus::InProgress
        } else {
            TodoStatus::Pending
        };

        items.push(TodoItem {
            id: format!("VALIDATE-{}", artifact.metadata.id),
            content,
            status,
            priority: TodoPriority::High, // Validation is always high priority
            parents: vec![artifact.metadata.id.clone()],
        });
    }

    items
}

/// Emit TodosUpdated event with tasks for the current workstream's ticket.
///
/// This reads tasks that belong to the current ticket from the artifact store
/// and emits a `ToolEvent::TodosUpdated` so the Plan View in the desktop UI updates immediately.
fn emit_todos_updated(ctx: &ToolContext, store: &ArtifactStore, ticket_id: &str) {
    if let Some(ref event_tx) = ctx.event_tx {
        tracing::info!("emit_todos_updated: event_tx is available, reading artifacts for ticket {}...", ticket_id);
        
        // Read tasks
        let tasks = match store.list_artifacts_for_ticket(ArtifactType::Task, ticket_id) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("emit_todos_updated: Failed to list tasks from store: {}", e);
                return;
            }
        };

        // Read all artifacts for validation items
        let all_artifacts = match store.list_all_artifacts_for_ticket(ticket_id) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("emit_todos_updated: Failed to list all artifacts from store: {}", e);
                return;
            }
        };

        tracing::info!(
            "emit_todos_updated: found {} tasks and {} total artifacts for ticket {}, building phased todos",
            tasks.len(),
            all_artifacts.len(),
            ticket_id
        );
        
        let phased_todos = build_phased_todos_from_tasks(&tasks, &all_artifacts);
        tracing::info!(
            "emit_todos_updated: built {} phases",
            phased_todos.phases.len()
        );
        
        if let Err(e) = event_tx.send(ToolEvent::TodosUpdated(phased_todos)) {
            tracing::warn!(
                "emit_todos_updated: Failed to send TodosUpdated event: {}",
                e
            );
        } else {
            tracing::info!("emit_todos_updated: Successfully sent TodosUpdated event");
        }
    } else {
        tracing::warn!("emit_todos_updated: event_tx is None, cannot emit event!");
    }
}

/// todoread tool - reads the current task tree (ACE-backed).
pub struct AceTodoReadTool;

#[async_trait]
impl Tool for AceTodoReadTool {
    fn id(&self) -> &str {
        "todoread"
    }

    fn description(&self) -> &str {
        "Read the current task tree organized by progress status and parent artifacts."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        tracing::info!(
            "📖 todoread: Starting execution with root_dir={}",
            ctx.root_dir.display()
        );

        // Auto-initialize workstream if needed
        let state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| {
                tracing::error!("📖 todoread: Failed to initialize workstream: {}", e);
                ToolError::execution_failed(format!("Failed to initialize workstream: {e}"))
            })?;

        tracing::info!(
            "📖 todoread: Workstream state loaded - ticket_id={}, active_task={:?}",
            state.ticket_id,
            state.active_task
        );

        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| {
                tracing::error!("📖 todoread: Failed to create store: {}", e);
                ToolError::execution_failed(format!("Failed to create store: {e}"))
            })?;

        tracing::debug!(
            "📖 todoread: ArtifactStore created, specs_dir={}",
            store.specs_dir().display()
        );

        // Filter tasks to only those belonging to the current workstream's ticket
        tracing::debug!(
            "📖 todoread: Listing tasks for ticket_id={}",
            state.ticket_id
        );

        let tasks = store
            .list_artifacts_for_ticket(ArtifactType::Task, &state.ticket_id)
            .map_err(|e| {
                tracing::error!("📖 todoread: Failed to list tasks: {}", e);
                ToolError::execution_failed(format!("Failed to list tasks: {e}"))
            })?;

        tracing::info!(
            "📖 todoread: Found {} tasks for ticket {}",
            tasks.len(),
            state.ticket_id
        );

        // Log each task found
        for task in &tasks {
            tracing::debug!(
                "📖 todoread: Task {} - '{}' (progress={}, phase={:?})",
                task.metadata.id,
                task.title,
                task.metadata.progress,
                task.metadata.phase
            );
        }

        if tasks.is_empty() {
            tracing::info!("📖 todoread: No tasks found for ticket {}", state.ticket_id);
            // Also log what's in the tasks directory
            let tasks_dir = store.specs_dir().join("tasks");
            if tasks_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&tasks_dir) {
                    let files: Vec<_> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect();
                    tracing::debug!(
                        "📖 todoread: Files in {}: {:?}",
                        tasks_dir.display(),
                        files
                    );
                }
            } else {
                tracing::debug!(
                    "📖 todoread: Tasks directory {} does not exist",
                    tasks_dir.display()
                );
            }
            let mut output = String::new();
            output.push_str(&format!("## Ticket: {}\n", state.ticket_id));
            output.push_str(&format!("## Phase: {}\n\n", state.workflow.current_phase));
            output.push_str("No tasks found for this workstream.\n\n");
            output.push_str("Create tasks with:\n");
            output.push_str(
                "```\ntodowrite(tasks=[{content: \"...\", phase: \"Setup\"}])\n```",
            );

            return Ok(ToolOutput::new("No tasks", output));
        }

        // Group tasks by progress
        let mut by_progress: HashMap<&str, Vec<_>> = HashMap::new();
        for task in &tasks {
            by_progress
                .entry(task.metadata.progress.as_str())
                .or_default()
                .push(task);
        }

        let mut output = String::new();

        output.push_str(&format!("## Ticket: {}\n", state.ticket_id));
        output.push_str(&format!("## Phase: {}\n\n", state.workflow.current_phase));

        if let Some(ref active) = state.active_task {
            output.push_str(&format!("**Active Task:** `{}`\n\n", active));
        }

        output.push_str("---\n\n");

        // Display tasks grouped by status
        for (status, icon) in [
            ("in_progress", "▶"),
            ("blocked", "⊘"),
            ("backlog", "○"),
            ("parked", "⏸"),
            ("ready_to_validate", "◎"),
            ("done", "●"),
            ("discarded", "✗"),
        ] {
            if let Some(tasks) = by_progress.get(status) {
                output.push_str(&format!(
                    "\n### {} {} ({})\n\n",
                    icon,
                    status.replace('_', " ").to_uppercase(),
                    tasks.len()
                ));
                for task in tasks {
                    let priority_char = match task.metadata.priority {
                        Priority::High => "H",
                        Priority::Medium => "M",
                        Priority::Low => "L",
                    };
                    output.push_str(&format!(
                        "- [{}] **{}** `{}`\n",
                        priority_char, task.title, task.metadata.id,
                    ));
                    if !task.metadata.parents.is_empty() {
                        output
                            .push_str(&format!("  Parent: {}\n", task.metadata.parents.join(", ")));
                    }
                }
            }
        }

        let counts = (
            by_progress.get("backlog").map(|v| v.len()).unwrap_or(0),
            by_progress.get("in_progress").map(|v| v.len()).unwrap_or(0),
            by_progress.get("done").map(|v| v.len()).unwrap_or(0),
        );

        Ok(ToolOutput::new(
            format!(
                "{} tasks: {} backlog, {} in progress, {} done",
                tasks.len(),
                counts.0,
                counts.1,
                counts.2
            ),
            output,
        )
        .with_metadata(json!({
            "total": tasks.len(),
            "backlog": counts.0,
            "in_progress": counts.1,
            "done": counts.2,
        })))
    }
}

/// ace_todo_update tool - updates an artifact's progress status.
pub struct AceTodoUpdateTool;

#[derive(Debug, Deserialize)]
struct TodoUpdateArgs {
    node_id: String,
    status: String,
}

#[async_trait]
impl Tool for AceTodoUpdateTool {
    fn id(&self) -> &str {
        "ace_todo_update"
    }

    fn description(&self) -> &str {
        r#"Update an artifact's progress status.

Valid statuses:
- backlog: Not yet started
- in_progress: Currently being worked on (only ONE task at a time)
- blocked: Waiting on external dependency
- parked: Temporarily set aside
- ready_to_validate: Work complete, awaiting verification
- done: Completed successfully
- discarded: No longer needed

Note: Setting a task to in_progress will fail if another task is already active.
Use 'parked' or 'done' on the current active task first."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["node_id", "status"],
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "The artifact ID to update (e.g., TASK-WON-122-001)"
                },
                "status": {
                    "type": "string",
                    "enum": ["backlog", "in_progress", "blocked", "parked", "ready_to_validate", "done", "discarded"],
                    "description": "New progress status"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: TodoUpdateArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        let progress = Progress::parse(&args.status)
            .ok_or_else(|| ToolError::validation(format!("Invalid status: {}", args.status)))?;

        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to create store: {e}")))?;

        // Load state (auto-initializing if needed)
        let mut state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to initialize workstream: {e}")))?;

        // Get old status for event
        let old_artifact = store
            .read_artifact(&args.node_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to read artifact: {e}")))?
            .ok_or_else(|| {
                ToolError::execution_failed(format!("Artifact not found: {}", args.node_id))
            })?;

        // Verify the artifact belongs to the current workstream's ticket
        if !old_artifact.belongs_to_ticket(&state.ticket_id) {
            return Err(ToolError::execution_failed(format!(
                "Artifact {} does not belong to the current workstream (ticket: {}).\n\n\
                 This artifact belongs to a different ticket. Use artifacts from the current workstream only.",
                args.node_id, state.ticket_id
            )));
        }

        let old_status = old_artifact.metadata.progress.as_str().to_string();

        // Enforce single active task constraint
        if progress == Progress::InProgress {
            if let Some(ref active) = state.active_task {
                if active != &args.node_id {
                    return Err(ToolError::execution_failed(format!(
                        "Cannot set {} to in_progress: {} is already active.\n\n\
                         First update the active task:\n\
                         - `ace_todo_update(node_id=\"{}\", status=\"parked\")` to pause it\n\
                         - `ace_todo_update(node_id=\"{}\", status=\"done\")` to complete it",
                        args.node_id, active, active, active
                    )));
                }
            }
            state.active_task = Some(args.node_id.clone());
        } else if state.active_task.as_ref() == Some(&args.node_id) {
            state.active_task = None;
        }

        // Update artifact
        let artifact = store
            .update_artifact_progress(&args.node_id, progress)
            .map_err(|e| ToolError::execution_failed(format!("Failed to update artifact: {e}")))?;

        // Save state
        state
            .save(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to save state: {e}")))?;

        // Emit TaskStatusChanged event
        if let Some(ref event_tx) = ctx.event_tx {
            let _ = event_tx.send(ToolEvent::TaskStatusChanged {
                id: args.node_id.clone(),
                old_status: old_status.clone(),
                new_status: args.status.clone(),
            });
        }

        // Emit TodosUpdated so Plan View updates immediately
        emit_todos_updated(ctx, &store, &state.ticket_id);

        let active_info = if progress == Progress::InProgress {
            format!("\n\n**Active task:** `{}`", args.node_id)
        } else if state.active_task.is_none() {
            "\n\n**No active task.** Use `ace_todo_update(node_id=\"...\", status=\"in_progress\")` to start one.".to_string()
        } else {
            format!(
                "\n\n**Active task:** `{}`",
                state.active_task.unwrap_or_default()
            )
        };

        Ok(ToolOutput::new(
            format!("{}: {} → {}", args.node_id, old_status, args.status),
            format!(
                "Updated `{}` from **{}** to **{}**{}",
                artifact.metadata.id, old_status, args.status, active_info
            ),
        )
        .with_metadata(json!({
            "id": artifact.metadata.id,
            "old_status": old_status,
            "new_status": args.status,
        })))
    }
}

/// todowrite tool - creates tasks (ACE-backed).
pub struct AceTodoWriteTool;

#[derive(Debug, Deserialize)]
struct TodoWriteArgs {
    tasks: Vec<TaskInput>,
}

#[derive(Debug, Deserialize)]
struct TaskInput {
    content: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// Phase for grouping tasks in the implementation plan.
    /// Tasks with the same phase are grouped together in the plan view.
    #[serde(default)]
    phase: Option<String>,
}

#[async_trait]
impl Tool for AceTodoWriteTool {
    fn id(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        r#"Create tasks for the current workstream.

Each task requires:
- A phase for grouping in the implementation plan
- Optionally a parent artifact (defaults to the session if not specified)

Tasks are created in the specs/tasks/ directory.

Example (simple todo list - tasks parent to session):
```json
{
  "tasks": [
    {"content": "Set up development environment", "phase": "Setup", "priority": "high"},
    {"content": "Implement core functionality", "phase": "Implementation", "priority": "high"},
    {"content": "Write tests", "phase": "Testing", "priority": "medium"}
  ]
}
```

Example (with explicit parent - for formal ACE workflow):
```json
{
  "tasks": [
    {"content": "Set up database schema", "parent": "REQ-WON-122-001", "phase": "Setup", "priority": "high"},
    {"content": "Implement login endpoint", "parent": "REQ-WON-122-001", "phase": "Core Implementation", "priority": "high"}
  ]
}
```"#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["tasks"],
            "properties": {
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["content", "phase"],
                        "properties": {
                            "content": {
                                "type": "string",
                                "description": "Task description (becomes the title)"
                            },
                            "parent": {
                                "type": "string",
                                "description": "Parent artifact ID (requirement, design, test-case, or session). Defaults to the session if not specified."
                            },
                            "phase": {
                                "type": "string",
                                "description": "Phase name for grouping tasks in the implementation plan (e.g., 'Setup', 'Core Implementation', 'Testing')"
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["high", "medium", "low"],
                                "description": "Task priority (default: medium)"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["backlog", "in_progress"],
                                "description": "Initial status (default: backlog)"
                            }
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        tracing::info!(
            "📝 todowrite: Starting execution with root_dir={}",
            ctx.root_dir.display()
        );
        tracing::debug!("📝 todowrite: Raw args: {}", args);

        let args: TodoWriteArgs = serde_json::from_value(args)
            .map_err(|e| {
                tracing::error!("📝 todowrite: Failed to parse arguments: {}", e);
                ToolError::validation(format!("Invalid arguments: {e}"))
            })?;

        tracing::info!("📝 todowrite: Parsed {} task(s) to create", args.tasks.len());

        if args.tasks.is_empty() {
            tracing::warn!("📝 todowrite: No tasks provided");
            return Err(ToolError::validation("No tasks provided".to_string()));
        }

        tracing::debug!("📝 todowrite: Creating ArtifactStore...");
        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| {
                tracing::error!("📝 todowrite: Failed to create store: {}", e);
                ToolError::execution_failed(format!("Failed to create store: {e}"))
            })?;
        tracing::info!("📝 todowrite: ArtifactStore created, specs_dir={}", store.specs_dir().display());

        tracing::debug!("📝 todowrite: Ensuring directories exist...");
        store.ensure_directories().map_err(|e| {
            tracing::error!("📝 todowrite: Failed to create directories: {}", e);
            ToolError::execution_failed(format!("Failed to create directories: {e}"))
        })?;
        tracing::info!("📝 todowrite: Directories ensured");

        // Load state (auto-initializing if needed)
        tracing::debug!("📝 todowrite: Loading/initializing workstream state...");
        let mut state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| {
                tracing::error!("📝 todowrite: Failed to initialize workstream: {}", e);
                ToolError::execution_failed(format!("Failed to initialize workstream: {e}"))
            })?;
        tracing::info!(
            "📝 todowrite: Workstream state loaded - ticket_id={}, active_task={:?}",
            state.ticket_id,
            state.active_task
        );

        // Ensure session exists (auto-creates if needed)
        tracing::debug!("📝 todowrite: Ensuring session exists...");
        let session_id = store
            .ensure_session(&mut state, &ctx.root_dir)
            .map_err(|e| {
                tracing::error!("📝 todowrite: Failed to ensure session: {}", e);
                ToolError::execution_failed(format!("Failed to ensure session: {e}"))
            })?;
        tracing::info!("📝 todowrite: Session ensured - session_id={}", session_id);

        let mut created_ids = Vec::new();

        for (idx, task_input) in args.tasks.into_iter().enumerate() {
            tracing::info!(
                "📝 todowrite: Processing task {} - content='{}', parent={:?}, phase={:?}",
                idx + 1,
                task_input.content,
                task_input.parent,
                task_input.phase
            );

            // Use session ID as default parent if not specified
            let parent = task_input.parent.unwrap_or_else(|| {
                tracing::debug!("📝 todowrite: Task {} using session as parent", idx + 1);
                session_id.clone()
            });

            // Phase is required for new tasks (backwards compatibility: existing tasks may not have it)
            let phase = task_input.phase.ok_or_else(|| {
                tracing::error!("📝 todowrite: Task {} missing phase", idx + 1);
                ToolError::validation(
                    "Task requires a phase for grouping in the implementation plan (e.g., 'Setup', 'Core Implementation', 'Testing')"
                        .to_string(),
                )
            })?;

            let priority = task_input
                .priority
                .as_deref()
                .and_then(Priority::parse)
                .unwrap_or(Priority::Medium);

            tracing::debug!(
                "📝 todowrite: Creating artifact - parent={}, phase={}, priority={:?}",
                parent,
                phase,
                priority
            );

            let artifact = store
                .create_artifact_with_phase(
                    &mut state,
                    ArtifactType::Task,
                    &task_input.content,
                    "", // Tasks don't need content body
                    vec![parent.clone()],
                    priority,
                    false, // Tasks go directly to specs, not staging
                    Some(phase.clone()),
                )
                .map_err(|e| {
                    tracing::error!(
                        "📝 todowrite: Failed to create task {} (parent={}, phase={}): {}",
                        idx + 1,
                        parent,
                        phase,
                        e
                    );
                    ToolError::execution_failed(format!("Failed to create task: {e}"))
                })?;

            tracing::info!(
                "📝 todowrite: Created artifact id={}, path={}",
                artifact.metadata.id,
                artifact.path.display()
            );

            // Verify file was actually written
            if artifact.path.exists() {
                tracing::info!("📝 todowrite: ✓ File exists at {}", artifact.path.display());
            } else {
                tracing::error!("📝 todowrite: ✗ File NOT found at {} after creation!", artifact.path.display());
            }

            // If status is in_progress, update it
            if task_input.status.as_deref() == Some("in_progress") {
                if state.active_task.is_some() {
                    tracing::error!("📝 todowrite: Cannot set in_progress - active task already exists");
                    return Err(ToolError::execution_failed(
                        "Cannot create task as in_progress: another task is already active"
                            .to_string(),
                    ));
                }
                tracing::debug!("📝 todowrite: Setting task {} to in_progress", artifact.metadata.id);
                store
                    .update_artifact_progress(&artifact.metadata.id, Progress::InProgress)
                    .map_err(|e| {
                        tracing::error!("📝 todowrite: Failed to update task status: {}", e);
                        ToolError::execution_failed(format!("Failed to update task status: {e}"))
                    })?;
                state.active_task = Some(artifact.metadata.id.clone());
            }

            created_ids.push((artifact.metadata.id, task_input.content));
        }

        tracing::debug!("📝 todowrite: Saving workstream state...");
        state
            .save(&ctx.root_dir)
            .map_err(|e| {
                tracing::error!("📝 todowrite: Failed to save state: {}", e);
                ToolError::execution_failed(format!("Failed to save state: {e}"))
            })?;
        tracing::info!("📝 todowrite: State saved successfully");

        // Emit TodosUpdated so Plan View updates immediately
        tracing::debug!("📝 todowrite: Emitting TodosUpdated event...");
        emit_todos_updated(ctx, &store, &state.ticket_id);

        // Emit ArtifactCreated for each created task so Documents View updates
        if let Some(ref event_tx) = ctx.event_tx {
            for (id, _) in &created_ids {
                tracing::debug!("📝 todowrite: Emitting ArtifactCreated for {}", id);
                let _ = event_tx.send(crate::ToolEvent::ArtifactCreated {
                    id: id.clone(),
                    artifact_type: "task".to_string(),
                });
            }
        }

        let output = format!(
            "Created {} task(s):\n\n{}",
            created_ids.len(),
            created_ids
                .iter()
                .map(|(id, content)| format!("- `{}`: {}", id, content))
                .collect::<Vec<_>>()
                .join("\n")
        );

        tracing::info!(
            "📝 todowrite: Completed successfully - created {} tasks: {:?}",
            created_ids.len(),
            created_ids.iter().map(|(id, _)| id).collect::<Vec<_>>()
        );

        Ok(
            ToolOutput::new(format!("Created {} tasks", created_ids.len()), output).with_metadata(
                json!({
                    "created": created_ids.iter().map(|(id, _)| id).collect::<Vec<_>>(),
                }),
            ),
        )
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
        }
    }

    #[tokio::test]
    async fn test_todo_read_no_workstream() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        // No state created - should auto-initialize and return empty tasks
        let tool = AceTodoReadTool;
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // Should auto-initialize and show no tasks
        assert!(result.output.contains("No tasks"));

        // State file should now exist
        let state_path = dir.path().join(".wonopcode").join("state.yaml");
        assert!(state_path.exists());
    }

    #[tokio::test]
    async fn test_todo_read_empty() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let tool = AceTodoReadTool;
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("No tasks"));
    }

    #[tokio::test]
    async fn test_create_and_read_tasks() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let store = ArtifactStore::new(dir.path()).unwrap();
        store.ensure_directories().unwrap();

        // Create session first (UseCases require a Session parent)
        let session = store
            .create_artifact(
                &mut state,
                ArtifactType::Session,
                "Test Session",
                "Session for testing",
                vec![],
                Priority::Medium,
                false,
            )
            .unwrap();

        // Create a UseCase with session as parent
        let uc = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test UC",
                "Content",
                vec![session.metadata.id],
                Priority::Medium,
                false,
            )
            .unwrap();

        let req = store
            .create_artifact(
                &mut state,
                ArtifactType::Requirement,
                "Test REQ",
                "Content",
                vec![uc.metadata.id],
                Priority::Medium,
                false,
            )
            .unwrap();

        state.save(dir.path()).unwrap();

        // Create tasks
        let write_tool = AceTodoWriteTool;
        let result = write_tool
            .execute(
                json!({
                    "tasks": [
                        {"content": "Task 1", "parent": req.metadata.id, "phase": "Setup", "priority": "high"},
                        {"content": "Task 2", "parent": req.metadata.id, "phase": "Testing", "priority": "low"}
                    ]
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("Created 2 task"));

        // Read tasks
        let read_tool = AceTodoReadTool;
        let result = read_tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("Task 1"));
        assert!(result.output.contains("Task 2"));
        assert!(result.output.contains("BACKLOG"));
    }

    #[tokio::test]
    async fn test_single_active_task_constraint() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let store = ArtifactStore::new(dir.path()).unwrap();
        store.ensure_directories().unwrap();

        // Create session first (UseCases require a Session parent)
        let session = store
            .create_artifact(
                &mut state,
                ArtifactType::Session,
                "Test Session",
                "",
                vec![],
                Priority::Medium,
                false,
            )
            .unwrap();

        // Create parent artifacts
        let uc = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "UC",
                "",
                vec![session.metadata.id],
                Priority::Medium,
                false,
            )
            .unwrap();

        let req = store
            .create_artifact(
                &mut state,
                ArtifactType::Requirement,
                "REQ",
                "",
                vec![uc.metadata.id],
                Priority::Medium,
                false,
            )
            .unwrap();

        // Create two tasks
        let task1 = store
            .create_artifact(
                &mut state,
                ArtifactType::Task,
                "Task 1",
                "",
                vec![req.metadata.id.clone()],
                Priority::Medium,
                false,
            )
            .unwrap();

        let task2 = store
            .create_artifact(
                &mut state,
                ArtifactType::Task,
                "Task 2",
                "",
                vec![req.metadata.id],
                Priority::Medium,
                false,
            )
            .unwrap();

        state.save(dir.path()).unwrap();

        let update_tool = AceTodoUpdateTool;

        // Set task1 to in_progress
        update_tool
            .execute(
                json!({
                    "node_id": task1.metadata.id,
                    "status": "in_progress"
                }),
                &ctx,
            )
            .await
            .unwrap();

        // Try to set task2 to in_progress - should fail
        let result = update_tool
            .execute(
                json!({
                    "node_id": task2.metadata.id,
                    "status": "in_progress"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already active"));
    }
}
