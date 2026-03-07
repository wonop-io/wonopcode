//! Workflow management tools (ace_what_now, ace_submit_checkpoint).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ace::config::WonopCodeConfig;
use crate::ace::{
    Artifact, ArtifactStore, ArtifactType, Priority, Progress, WorkflowPhase, WorkstreamState,
};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolResult};

/// ace_what_now tool - provides guidance on what to do next.
pub struct AceWhatNowTool;

#[async_trait]
impl Tool for AceWhatNowTool {
    fn id(&self) -> &str {
        "ace_what_now"
    }

    fn description(&self) -> &str {
        "Get guidance on what to do next based on current workflow state, artifacts, and active tasks."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let config = WonopCodeConfig::load(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to load config: {e}")))?;

        // Auto-initialize workstream if needed
        let state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to initialize workstream: {e}")))?;

        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to create store: {e}")))?;

        let mut output = String::new();

        // Header
        output.push_str(&format!("## Workstream: {}\n\n", state.ticket_id));
        if let Some(ref title) = state.ticket_title {
            output.push_str(&format!("**Title:** {}\n", title));
        }
        output.push_str(&format!(
            "**Current Phase:** {}\n",
            state.workflow.current_phase
        ));

        if let Some(ref active) = state.active_task {
            output.push_str(&format!("**Active Task:** `{}`\n", active));
        } else {
            output.push_str("**Active Task:** None\n");
        }

        output.push_str("\n---\n\n");

        // Recommendations based on phase
        output.push_str("## Recommended Next Steps\n\n");

        let phase = state.workflow.current_phase;
        let ticket_id = &state.ticket_id;
        match phase {
            WorkflowPhase::Requirements => {
                recommend_requirements_phase(&store, &config, ticket_id, &mut output)?;
            }
            WorkflowPhase::Analysis => {
                output.push_str("### Analysis Phase\n\n");
                output.push_str(
                    "Review and analyze the requirements for completeness and feasibility.\n\n",
                );
                output.push_str("When ready, the workflow will advance to the Design phase.\n");
            }
            WorkflowPhase::Design => {
                recommend_design_phase(&store, &config, ticket_id, &mut output)?;
            }
            WorkflowPhase::Implementation => {
                recommend_implementation_phase(&store, &state, ticket_id, &mut output)?;
            }
            WorkflowPhase::Verification => {
                recommend_verification_phase(&store, ticket_id, &mut output)?;
            }
            WorkflowPhase::Deployment => {
                output.push_str("### Deployment Phase\n\n");
                output.push_str("Prepare and execute deployment.\n");
            }
        }

        // Artifact summary - filtered by ticket ID
        output.push_str("\n---\n\n## Artifact Summary\n\n");
        output.push_str("| Type | Total | Done | In Progress |\n");
        output.push_str("|------|-------|------|-------------|\n");

        for artifact_type in [
            ArtifactType::UseCase,
            ArtifactType::Requirement,
            ArtifactType::Design,
            ArtifactType::TestCase,
            ArtifactType::Task,
        ] {
            let artifacts = store.list_artifacts_for_ticket(artifact_type, &state.ticket_id).unwrap_or_default();
            let done = artifacts
                .iter()
                .filter(|a| a.metadata.progress.is_terminal())
                .count();
            let in_progress = artifacts
                .iter()
                .filter(|a| a.metadata.progress == Progress::InProgress)
                .count();
            output.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                artifact_type.directory(),
                artifacts.len(),
                done,
                in_progress
            ));
        }

        Ok(ToolOutput::new(
            format!(
                "Phase: {} | {}",
                phase,
                state.active_task.as_deref().unwrap_or("no active task")
            ),
            output,
        ))
    }
}

fn recommend_requirements_phase(
    store: &ArtifactStore,
    config: &WonopCodeConfig,
    ticket_id: &str,
    output: &mut String,
) -> Result<(), ToolError> {
    let use_cases = store
        .list_artifacts_for_ticket(ArtifactType::UseCase, ticket_id)
        .unwrap_or_default();
    let requirements = store
        .list_artifacts_for_ticket(ArtifactType::Requirement, ticket_id)
        .unwrap_or_default();

    output.push_str("### Requirements Phase\n\n");
    output.push_str(
        "⚠️ **MANDATORY WORKFLOW**: You MUST complete requirements before implementation.\n",
    );
    output.push_str("DO NOT write code until the user has approved requirements and designs.\n\n");

    if use_cases.is_empty() {
        output.push_str("**Step 1: Create Use Cases** (REQUIRED)\n\n");
        output.push_str("Document how users will interact with the system:\n\n");
        output.push_str("```\nace_create_artifact(\n");
        output.push_str("  type=\"use-case\",\n");
        output.push_str("  title=\"User performs action\",\n");
        output
            .push_str("  content=\"## Primary Actor\\n...\\n## Main Success Scenario\\n1. ...\"\n");
        output.push_str(")\n```\n\n");
    } else if requirements.is_empty() {
        output.push_str("**Step 2: Create Requirements** (REQUIRED)\n\n");
        output.push_str("Derive specific, testable requirements from your use cases:\n\n");
        output.push_str("Available use cases:\n");
        for uc in &use_cases {
            output.push_str(&format!("- `{}`: {}\n", uc.metadata.id, uc.title));
        }
        output.push_str("\n```\nace_create_artifact(\n");
        output.push_str("  type=\"requirement\",\n");
        output.push_str(&format!("  parents=[\"{}\"],\n", use_cases[0].metadata.id));
        output.push_str("  title=\"System must...\",\n");
        output.push_str("  content=\"Description of the requirement...\"\n");
        output.push_str(")\n```\n\n");
    } else if config.ace.workflow.checkpoints.requirements.required {
        output.push_str("**Step 3: Request User Review** (REQUIRED)\n\n");
        output.push_str("⛔ **STOP AND WAIT FOR APPROVAL** before proceeding.\n\n");
        output.push_str("Requirements are ready for user review:\n\n");
        output.push_str(&format!("- {} use case(s)\n", use_cases.len()));
        output.push_str(&format!("- {} requirement(s)\n\n", requirements.len()));
        output.push_str("Present these to the user and ask for approval:\n");
        output.push_str("```\nace_submit_checkpoint(\n");
        output.push_str("  checkpoint=\"requirements\",\n");
        output.push_str("  action=\"request_review\"\n");
        output.push_str(")\n```\n\n");
        output.push_str("Wait for user to say \"approved\" before continuing.\n");
    } else {
        output.push_str("Requirements phase complete. Workflow can advance to Analysis.\n");
    }

    Ok(())
}

fn recommend_design_phase(
    store: &ArtifactStore,
    config: &WonopCodeConfig,
    ticket_id: &str,
    output: &mut String,
) -> Result<(), ToolError> {
    let requirements = store
        .list_artifacts_for_ticket(ArtifactType::Requirement, ticket_id)
        .unwrap_or_default();
    let designs = store
        .list_artifacts_for_ticket(ArtifactType::Design, ticket_id)
        .unwrap_or_default();
    let test_cases = store
        .list_artifacts_for_ticket(ArtifactType::TestCase, ticket_id)
        .unwrap_or_default();

    output.push_str("### Design Phase\n\n");
    output.push_str("⚠️ **MANDATORY WORKFLOW**: You MUST create BOTH designs AND test cases.\n");
    output.push_str("🧪 **TEST CASES ARE REQUIRED** - DO NOT skip test case creation!\n");
    output.push_str("❌ DO NOT write any implementation code until designs are approved.\n\n");

    // Find requirements without designs
    let designed_parents: std::collections::HashSet<_> = designs
        .iter()
        .flat_map(|d| d.metadata.parents.iter())
        .collect();

    let undesigned: Vec<_> = requirements
        .iter()
        .filter(|r| !designed_parents.contains(&r.metadata.id))
        .collect();

    // Find requirements without test cases
    let tested_parents: std::collections::HashSet<_> = test_cases
        .iter()
        .flat_map(|tc| tc.metadata.parents.iter())
        .collect();

    let untested: Vec<_> = requirements
        .iter()
        .filter(|r| !tested_parents.contains(&r.metadata.id))
        .collect();

    // Status summary
    output.push_str(&format!(
        "**Status:** {} design(s), {} test case(s) for {} requirement(s)\n\n",
        designs.len(),
        test_cases.len(),
        requirements.len()
    ));

    if !undesigned.is_empty() {
        output.push_str("**Step 1: Create Design Documents** (REQUIRED - {} remaining)\n\n");
        output.push_str("The following requirements need design documents:\n\n");
        for req in undesigned.iter().take(5) {
            output.push_str(&format!("- `{}`: {}\n", req.metadata.id, req.title));
        }
        if undesigned.len() > 5 {
            output.push_str(&format!("- ... and {} more\n", undesigned.len() - 5));
        }
        output.push_str("\n```\nace_create_artifact(\n");
        output.push_str("  type=\"design\",\n");
        output.push_str(&format!("  parents=[\"{}\"],\n", undesigned[0].metadata.id));
        output.push_str("  title=\"Technical design for...\",\n");
        output.push_str("  content=\"## Overview\\n...\\n## Implementation Approach\\n...\"\n");
        output.push_str(")\n```\n\n");

        // Remind about test cases
        if untested.len() == requirements.len() {
            output.push_str(
                "📝 **Note:** After creating designs, you MUST also create test cases.\n\n",
            );
        }
    } else if !untested.is_empty() {
        output.push_str(&format!(
            "**Step 2: Create Test Cases** (REQUIRED - {} remaining)\n\n",
            untested.len()
        ));
        output.push_str("🧪 **CRITICAL**: Every requirement needs at least one test case.\n");
        output.push_str("Test cases define how we verify the implementation works correctly.\n\n");
        output.push_str("Requirements missing test cases:\n\n");
        for req in untested.iter().take(5) {
            output.push_str(&format!("- `{}`: {}\n", req.metadata.id, req.title));
        }
        if untested.len() > 5 {
            output.push_str(&format!("- ... and {} more\n", untested.len() - 5));
        }
        output.push_str("\n**Create test cases with specific, verifiable scenarios:**\n\n");
        output.push_str("```\nace_create_artifact(\n");
        output.push_str("  type=\"test-case\",\n");
        output.push_str(&format!("  parents=[\"{}\"],\n", untested[0].metadata.id));
        output.push_str("  title=\"Test: verify [specific behavior]\",\n");
        output.push_str("  content=\"## Preconditions\\n- ...\\n\\n## Steps\\n1. ...\\n2. ...\\n\\n## Expected Result\\n- ...\"\n");
        output.push_str(")\n```\n\n");
    } else if config.ace.workflow.checkpoints.design.required {
        output.push_str("**Step 3: Request User Review** (REQUIRED)\n\n");
        output
            .push_str("⛔ **STOP AND WAIT FOR APPROVAL** before creating implementation plan.\n\n");
        output.push_str(&format!(
            "✅ All {} requirement(s) have:\n- {} design document(s)\n- {} test case(s)\n\n",
            requirements.len(),
            designs.len(),
            test_cases.len()
        ));
        output.push_str("Present designs and test cases to the user:\n");
        output.push_str("```\nace_submit_checkpoint(\n");
        output.push_str("  checkpoint=\"design\",\n");
        output.push_str("  action=\"request_review\"\n");
        output.push_str(")\n```\n\n");
        output.push_str("Wait for user to say \"approved\".\n\n");
        output.push_str("**After design approval, you will:**\n");
        output.push_str("1. Create tasks that break down the implementation work\n");
        output.push_str("2. Submit the implementation plan for approval\n");
        output.push_str("3. Only then begin coding\n");
    } else {
        output
            .push_str("Design phase complete. Workflow can advance to Implementation Planning.\n");
    }

    Ok(())
}

fn recommend_implementation_phase(
    store: &ArtifactStore,
    state: &WorkstreamState,
    ticket_id: &str,
    output: &mut String,
) -> Result<(), ToolError> {
    let tasks = store.list_artifacts_for_ticket(ArtifactType::Task, ticket_id).unwrap_or_default();
    let designs = store
        .list_artifacts_for_ticket(ArtifactType::Design, ticket_id)
        .unwrap_or_default();
    let test_cases = store
        .list_artifacts_for_ticket(ArtifactType::TestCase, ticket_id)
        .unwrap_or_default();
    let requirements = store
        .list_artifacts_for_ticket(ArtifactType::Requirement, ticket_id)
        .unwrap_or_default();

    let pending_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| {
            !t.metadata.progress.is_terminal() && t.metadata.progress != Progress::InProgress
        })
        .collect();

    let done_tasks = tasks
        .iter()
        .filter(|t| t.metadata.progress.is_terminal())
        .count();

    output.push_str("### Implementation Phase\n\n");

    // Gate implementation: require test cases before proceeding
    if !requirements.is_empty() && test_cases.is_empty() {
        output.push_str("⛔ **STOP: Test cases required before implementation!**\n\n");
        output.push_str("You have {} requirement(s) but no test cases.\n\n");
        output.push_str("**You MUST create test cases first.** DO NOT implement code without test coverage.\n\n");
        output.push_str("Go back and create test cases:\n");
        output.push_str("```\nace_create_artifact(\n");
        output.push_str("  type=\"test-case\",\n");
        output.push_str(&format!(
            "  parents=[\"{}\"],\n",
            requirements[0].metadata.id
        ));
        output.push_str("  title=\"Test: verify [behavior]\",\n");
        output.push_str("  content=\"## Steps\\n1. ...\\n## Expected\\n- ...\"\n");
        output.push_str(")\n```\n\n");
        return Ok(());
    }

    // Check if implementation plan has been approved (tasks exist and we have an active task or all done)
    let plan_approved = !tasks.is_empty()
        && (state.active_task.is_some()
            || done_tasks > 0
            || tasks
                .iter()
                .any(|t| t.metadata.progress == Progress::InProgress));

    // Tasks must be created BEFORE implementation begins
    if tasks.is_empty() {
        output.push_str("**Step 1: Create Implementation Plan** (REQUIRED before coding)\n\n");
        output.push_str("⚠️ **You MUST create tasks BEFORE writing any code!**\n");
        output.push_str("Tasks break down the implementation work and provide trackability.\n\n");

        // Show available parents
        if !designs.is_empty() {
            output.push_str("Available designs:\n");
            for des in designs.iter().take(3) {
                output.push_str(&format!("- `{}`\n", des.metadata.id));
            }
        } else if !requirements.is_empty() {
            output.push_str("Available requirements:\n");
            for req in requirements.iter().take(3) {
                output.push_str(&format!("- `{}`\n", req.metadata.id));
            }
        }

        output.push_str("\n```\ntodowrite(tasks=[\n");
        if let Some(parent) = designs.first().or(requirements.first()) {
            output.push_str(&format!(
                "  {{content: \"Implement feature\", parent: \"{}\", priority: \"high\"}},\n",
                parent.metadata.id
            ));
            output.push_str(&format!(
                "  {{content: \"Add tests\", parent: \"{}\", priority: \"medium\"}}\n",
                parent.metadata.id
            ));
        }
        output.push_str("])\n```\n\n");
        output.push_str("**DO NOT write implementation code until the plan is approved!**\n");
    } else if !plan_approved && state.active_task.is_none() {
        // Tasks exist but plan not yet approved
        output.push_str("**Step 2: Submit Implementation Plan for Approval** (REQUIRED)\n\n");
        output.push_str("⛔ **STOP AND WAIT FOR APPROVAL** before writing any code.\n\n");
        output.push_str(&format!(
            "You have created {} task(s). Submit them for review:\n\n",
            tasks.len()
        ));
        output.push_str("```\nace_submit_checkpoint(\n");
        output.push_str("  checkpoint=\"implementation_plan\",\n");
        output.push_str("  action=\"request_review\"\n");
        output.push_str(")\n```\n\n");
        output.push_str("Wait for user to say \"approved\" before starting implementation.\n");
    } else if let Some(ref active) = state.active_task {
        output.push_str(&format!("**Complete Active Task:** `{}`\n\n", active));
        output.push_str("When done:\n");
        output.push_str("```\nace_todo_update(\n");
        output.push_str(&format!("  node_id=\"{}\",\n", active));
        output.push_str("  status=\"done\"\n");
        output.push_str(")\n```\n\n");
        output.push_str(&format!(
            "Progress: {}/{} tasks complete\n",
            done_tasks,
            tasks.len()
        ));
    } else if !pending_tasks.is_empty() {
        output.push_str("**Start Next Task**\n\n");
        output.push_str("Pending tasks:\n");
        for task in pending_tasks.iter().take(5) {
            output.push_str(&format!(
                "- `{}`: {} [{}]\n",
                task.metadata.id, task.title, task.metadata.priority
            ));
        }
        output.push_str("\n```\nace_todo_update(\n");
        output.push_str(&format!(
            "  node_id=\"{}\",\n",
            pending_tasks[0].metadata.id
        ));
        output.push_str("  status=\"in_progress\"\n");
        output.push_str(")\n```\n");
    } else {
        output.push_str("**All Tasks Complete!**\n\n");
        output.push_str(&format!(
            "{}/{} tasks done. Ready to advance to Verification.\n",
            done_tasks,
            tasks.len()
        ));
    }

    Ok(())
}

fn recommend_verification_phase(
    store: &ArtifactStore,
    ticket_id: &str,
    output: &mut String,
) -> Result<(), ToolError> {
    let test_cases = store
        .list_artifacts_for_ticket(ArtifactType::TestCase, ticket_id)
        .unwrap_or_default();
    let tasks = store.list_artifacts_for_ticket(ArtifactType::Task, ticket_id).unwrap_or_default();

    let done_tasks = tasks
        .iter()
        .filter(|t| t.metadata.progress.is_terminal())
        .count();
    let passed_tests = test_cases
        .iter()
        .filter(|tc| tc.metadata.progress == Progress::Done)
        .count();

    output.push_str("### Verification Phase\n\n");
    output.push_str(&format!(
        "{} test case(s) defined from design phase.\n\n",
        test_cases.len()
    ));
    output.push_str(&format!(
        "**Implementation Progress:** {}/{} tasks complete\n",
        done_tasks,
        tasks.len()
    ));
    output.push_str(&format!(
        "**Test Progress:** {}/{} tests passed\n\n",
        passed_tests,
        test_cases.len()
    ));

    if passed_tests < test_cases.len() {
        output.push_str("**Run and verify tests:**\n");
        output.push_str("- Execute the test cases defined in the design phase\n");
        output.push_str("- Mark test cases as done when they pass:\n");
        output.push_str("```\nace_todo_update(node_id=\"TC-...\", status=\"done\")\n```\n\n");
    } else if !test_cases.is_empty() {
        output.push_str("✅ All test cases passed! Ready for deployment.\n");
    } else {
        output.push_str("No test cases defined. Consider adding test coverage.\n");
    }

    Ok(())
}

/// ace_submit_checkpoint tool - requests or records human approval.
pub struct AceSubmitCheckpointTool;

#[derive(Debug, Deserialize)]
struct SubmitCheckpointArgs {
    checkpoint: String,
    action: String,
    #[serde(default)]
    #[allow(dead_code)]
    comments: Option<String>,
}

#[async_trait]
impl Tool for AceSubmitCheckpointTool {
    fn id(&self) -> &str {
        "ace_submit_checkpoint"
    }

    fn description(&self) -> &str {
        r#"Request or record human approval for a workflow checkpoint.

Checkpoints:
- requirements: Review use cases and requirements before design
- design: Review technical designs and test cases before planning implementation
- implementation_plan: Review the task breakdown before coding begins
- verification: Verify tests pass before deployment

Actions:
- request_review: Present artifacts to the user and ask for approval
- approve: Record that the user has approved (use after they confirm)

Example flow:
1. Call with action="request_review" to present the review
2. Wait for user to say "approved"
3. Call with action="approve" to advance the workflow

IMPORTANT: After design approval, you must create tasks and get the implementation
plan approved BEFORE writing any code."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["checkpoint", "action"],
            "properties": {
                "checkpoint": {
                    "type": "string",
                    "enum": ["requirements", "design", "implementation_plan", "verification"],
                    "description": "The checkpoint to submit"
                },
                "action": {
                    "type": "string",
                    "enum": ["request_review", "approve"],
                    "description": "Action to take"
                },
                "comments": {
                    "type": "string",
                    "description": "Optional review comments"
                }
            }
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolResult<ToolOutput> {
        let args: SubmitCheckpointArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation(format!("Invalid arguments: {e}")))?;

        // Load state (auto-initializing if needed)
        let mut state = WorkstreamState::ensure_initialized(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to initialize workstream: {e}")))?;

        let store = ArtifactStore::new(&ctx.root_dir)
            .map_err(|e| ToolError::execution_failed(format!("Failed to create store: {e}")))?;

        match args.action.as_str() {
            "request_review" => {
                // Generate review summary
                let mut summary =
                    format!("## {} Review Request\n\n", args.checkpoint.to_uppercase());
                summary.push_str(&format!("**Ticket:** {}\n\n", state.ticket_id));

                match args.checkpoint.as_str() {
                    "requirements" => {
                        let use_cases = store
                            .list_artifacts_for_ticket(ArtifactType::UseCase, &state.ticket_id)
                            .unwrap_or_default();
                        let requirements = store
                            .list_artifacts_for_ticket(ArtifactType::Requirement, &state.ticket_id)
                            .unwrap_or_default();

                        summary.push_str("### Use Cases\n\n");
                        if use_cases.is_empty() {
                            summary.push_str("*(none)*\n\n");
                        } else {
                            for uc in &use_cases {
                                summary
                                    .push_str(&format!("- **{}**: {}\n", uc.metadata.id, uc.title));
                            }
                            summary.push('\n');
                        }

                        summary.push_str("### Requirements\n\n");
                        if requirements.is_empty() {
                            summary.push_str("*(none)*\n\n");
                        } else {
                            for req in &requirements {
                                summary.push_str(&format!(
                                    "- **{}**: {} (parent: {})\n",
                                    req.metadata.id,
                                    req.title,
                                    req.metadata.parents.join(", ")
                                ));
                            }
                            summary.push('\n');
                        }
                    }
                    "design" => {
                        let requirements = store
                            .list_artifacts_for_ticket(ArtifactType::Requirement, &state.ticket_id)
                            .unwrap_or_default();
                        let designs = store
                            .list_artifacts_for_ticket(ArtifactType::Design, &state.ticket_id)
                            .unwrap_or_default();
                        let test_cases = store
                            .list_artifacts_for_ticket(ArtifactType::TestCase, &state.ticket_id)
                            .unwrap_or_default();

                        // Validate: require at least one test case if there are requirements
                        if !requirements.is_empty() && test_cases.is_empty() {
                            return Err(ToolError::validation(
                                "Cannot submit design checkpoint: No test cases defined.\n\n\
                                 Every requirement must have at least one test case.\n\
                                 Use `ace_create_artifact(type=\"test-case\", ...)` to create test cases first.\n\n\
                                 Call `ace_what_now()` to see which requirements need test cases."
                            ));
                        }

                        // Validate: check test case coverage
                        let tested_req_ids: std::collections::HashSet<_> = test_cases
                            .iter()
                            .flat_map(|tc| tc.metadata.parents.iter())
                            .collect();
                        let untested: Vec<_> = requirements
                            .iter()
                            .filter(|r| !tested_req_ids.contains(&r.metadata.id))
                            .collect();

                        if !untested.is_empty() {
                            let untested_list: String = untested
                                .iter()
                                .take(5)
                                .map(|r| format!("  - {}: {}", r.metadata.id, r.title))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let more_msg = if untested.len() > 5 {
                                format!("\n  ... and {} more", untested.len() - 5)
                            } else {
                                String::new()
                            };
                            return Err(ToolError::validation(format!(
                                "Cannot submit design checkpoint: {} requirement(s) have no test cases.\n\n\
                                 Requirements missing test coverage:\n{}{}\n\n\
                                 Create test cases for each requirement before submitting for review.",
                                untested.len(), untested_list, more_msg
                            )));
                        }

                        summary.push_str("### Designs\n\n");
                        if designs.is_empty() {
                            summary.push_str("*(none)*\n\n");
                        } else {
                            for des in &designs {
                                summary.push_str(&format!(
                                    "- **{}**: {} (for: {})\n",
                                    des.metadata.id,
                                    des.title,
                                    des.metadata.parents.join(", ")
                                ));
                            }
                            summary.push('\n');
                        }

                        summary.push_str("### Test Cases\n\n");
                        for tc in &test_cases {
                            summary.push_str(&format!(
                                "- **{}**: {} (for: {})\n",
                                tc.metadata.id,
                                tc.title,
                                tc.metadata.parents.join(", ")
                            ));
                        }
                        summary.push('\n');
                    }
                    "implementation_plan" => {
                        let tasks = store.list_artifacts_for_ticket(ArtifactType::Task, &state.ticket_id).unwrap_or_default();
                        let designs = store
                            .list_artifacts_for_ticket(ArtifactType::Design, &state.ticket_id)
                            .unwrap_or_default();
                        let requirements = store
                            .list_artifacts_for_ticket(ArtifactType::Requirement, &state.ticket_id)
                            .unwrap_or_default();
                        let use_cases = store
                            .list_artifacts_for_ticket(ArtifactType::UseCase, &state.ticket_id)
                            .unwrap_or_default();
                        let test_cases = store
                            .list_artifacts_for_ticket(ArtifactType::TestCase, &state.ticket_id)
                            .unwrap_or_default();

                        // Validate: require at least one task
                        if tasks.is_empty() {
                            return Err(ToolError::validation(
                                "Cannot submit implementation plan checkpoint: No tasks defined.\n\n\
                                 You must create tasks that break down the implementation work.\n\
                                 Use `todowrite(tasks=[...])` to create tasks first.\n\n\
                                 Call `ace_what_now()` for guidance on creating tasks."
                            ));
                        }

                        // Build set of valid parent IDs (designs, requirements, use cases, test cases)
                        let valid_parents: std::collections::HashSet<String> = designs
                            .iter()
                            .map(|a| a.metadata.id.clone())
                            .chain(requirements.iter().map(|a| a.metadata.id.clone()))
                            .chain(use_cases.iter().map(|a| a.metadata.id.clone()))
                            .chain(test_cases.iter().map(|a| a.metadata.id.clone()))
                            .collect();

                        // Validate: each task must have at least one valid parent
                        let orphan_tasks: Vec<_> = tasks
                            .iter()
                            .filter(|t| {
                                t.metadata.parents.is_empty()
                                    || !t.metadata.parents.iter().any(|p| valid_parents.contains(p))
                            })
                            .collect();

                        if !orphan_tasks.is_empty() {
                            let orphan_list: String = orphan_tasks
                                .iter()
                                .take(5)
                                .map(|t| {
                                    format!(
                                        "  - `{}`: {} (parents: {})",
                                        t.metadata.id,
                                        t.title,
                                        if t.metadata.parents.is_empty() {
                                            "none".to_string()
                                        } else {
                                            t.metadata.parents.join(", ")
                                        }
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            let more_msg = if orphan_tasks.len() > 5 {
                                format!("\n  ... and {} more", orphan_tasks.len() - 5)
                            } else {
                                String::new()
                            };

                            let available_parents: String = designs
                                .iter()
                                .take(3)
                                .map(|d| format!("  - `{}` (design)", d.metadata.id))
                                .chain(
                                    requirements
                                        .iter()
                                        .take(3)
                                        .map(|r| format!("  - `{}` (requirement)", r.metadata.id)),
                                )
                                .collect::<Vec<_>>()
                                .join("\n");

                            return Err(ToolError::validation(format!(
                                "Cannot submit implementation plan: {} task(s) have no valid parent artifact.\n\n\
                                 Each task must reference at least one design, requirement, use case, or test case.\n\n\
                                 Tasks without valid parents:\n{}{}\n\n\
                                 Available parent artifacts:\n{}\n\n\
                                 Update tasks with: `todowrite(tasks=[{{content: \"...\", parent: \"DES-xxx\"}}])`",
                                orphan_tasks.len(), orphan_list, more_msg, available_parents
                            )));
                        }

                        summary.push_str("### Implementation Plan\n\n");
                        summary.push_str(
                            "The following tasks break down the implementation work:\n\n",
                        );

                        // Group tasks by parent design/requirement
                        let mut by_parent: std::collections::HashMap<String, Vec<&Artifact>> =
                            std::collections::HashMap::new();
                        for task in &tasks {
                            let parent = task
                                .metadata
                                .parents
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "unassigned".to_string());
                            by_parent.entry(parent).or_default().push(task);
                        }

                        for (parent_id, parent_tasks) in &by_parent {
                            // Find parent title
                            let parent_title = designs
                                .iter()
                                .find(|d| &d.metadata.id == parent_id)
                                .map(|d| d.title.as_str())
                                .unwrap_or("Unknown");

                            summary.push_str(&format!("**{}** ({})\n", parent_id, parent_title));
                            for task in parent_tasks {
                                let priority_icon = match task.metadata.priority {
                                    Priority::High => "🔴",
                                    Priority::Medium => "🟡",
                                    Priority::Low => "🟢",
                                };
                                summary.push_str(&format!(
                                    "  - {} `{}`: {}\n",
                                    priority_icon, task.metadata.id, task.title
                                ));
                            }
                            summary.push('\n');
                        }

                        summary.push_str(&format!("**Total:** {} task(s)\n\n", tasks.len()));
                        summary.push_str("⚠️ **After approval, implementation will begin.**\n");
                        summary
                            .push_str("The agent will work through these tasks one at a time.\n");
                    }
                    "verification" => {
                        let test_cases = store
                            .list_artifacts_for_ticket(ArtifactType::TestCase, &state.ticket_id)
                            .unwrap_or_default();

                        summary.push_str("### Test Cases\n\n");
                        if test_cases.is_empty() {
                            summary.push_str("*(none)*\n\n");
                        } else {
                            for tc in &test_cases {
                                summary.push_str(&format!(
                                    "- **{}**: {} [{}]\n",
                                    tc.metadata.id, tc.title, tc.metadata.progress
                                ));
                            }
                            summary.push('\n');
                        }
                    }
                    _ => {}
                }

                summary.push_str("---\n\n");
                summary.push_str("**Please review the above artifacts.**\n\n");
                summary.push_str(
                    "Reply with **\"approved\"** to proceed, or provide feedback for revisions.",
                );

                // Update phase status
                state.set_awaiting_approval(&args.checkpoint);
                state.save(&ctx.root_dir).map_err(|e| {
                    ToolError::execution_failed(format!("Failed to save state: {e}"))
                })?;

                Ok(ToolOutput::new(
                    format!("{} review requested", args.checkpoint),
                    summary,
                ))
            }
            "approve" => {
                // Determine which artifact types to promote based on checkpoint
                let types_to_promote = match args.checkpoint.as_str() {
                    "requirements" => vec![ArtifactType::UseCase, ArtifactType::Requirement],
                    "design" => vec![ArtifactType::Design, ArtifactType::TestCase],
                    "implementation_plan" => vec![ArtifactType::Task],
                    "verification" => vec![], // Tasks already promoted at implementation_plan
                    _ => vec![],
                };

                // Promote staged artifacts to specs
                let promoted_count = if !types_to_promote.is_empty() {
                    store
                        .promote_artifacts_by_types(&types_to_promote)
                        .unwrap_or(0)
                } else {
                    0
                };

                // Advance to next phase
                let old_phase = state.workflow.current_phase;
                let new_phase = state.advance_phase();

                state.save(&ctx.root_dir).map_err(|e| {
                    ToolError::execution_failed(format!("Failed to save state: {e}"))
                })?;

                let promoted_msg = if promoted_count > 0 {
                    format!(
                        "\n\n**Promoted {} artifact(s)** from staging to approved specs.",
                        promoted_count
                    )
                } else {
                    String::new()
                };

                // Load config for phase recommendations
                let config = WonopCodeConfig::load(&ctx.root_dir).map_err(|e| {
                    ToolError::execution_failed(format!("Failed to load config: {e}"))
                })?;

                // Generate next steps based on the new phase
                let mut next_steps = String::new();
                let ticket_id = &state.ticket_id;
                if let Some(new) = new_phase {
                    next_steps.push_str("\n\n---\n\n## Next Steps\n\n");
                    match new {
                        WorkflowPhase::Requirements => {
                            recommend_requirements_phase(&store, &config, ticket_id, &mut next_steps)?;
                        }
                        WorkflowPhase::Analysis => {
                            next_steps.push_str("### Analysis Phase\n\n");
                            next_steps.push_str(
                                "Review and analyze the requirements for completeness and feasibility.\n\n",
                            );
                            next_steps.push_str(
                                "When ready, the workflow will advance to the Design phase.\n",
                            );
                        }
                        WorkflowPhase::Design => {
                            recommend_design_phase(&store, &config, ticket_id, &mut next_steps)?;
                        }
                        WorkflowPhase::Implementation => {
                            recommend_implementation_phase(&store, &state, ticket_id, &mut next_steps)?;
                        }
                        WorkflowPhase::Verification => {
                            recommend_verification_phase(&store, ticket_id, &mut next_steps)?;
                        }
                        WorkflowPhase::Deployment => {
                            next_steps.push_str("### Deployment Phase\n\n");
                            next_steps.push_str("Prepare and execute deployment.\n");
                        }
                    }
                }

                let message = if let Some(new) = new_phase {
                    format!(
                        "## {} Checkpoint Approved\n\n\
                         **Current phase:** {}\n\
                         **Next phase:** {}\n\n\
                         Workflow advanced from **{}** to **{}**.{}{}",
                        args.checkpoint, old_phase, new, old_phase, new, promoted_msg, next_steps
                    )
                } else {
                    format!(
                        "## {} Checkpoint Approved\n\n\
                         **Current phase:** {}\n\
                         **Next phase:** (complete)\n\n\
                         Workflow complete! All phases finished.{}",
                        args.checkpoint, old_phase, promoted_msg
                    )
                };

                Ok(ToolOutput::new(
                    format!(
                        "{} approved | Current phase: {} | Next phase: {}",
                        args.checkpoint,
                        old_phase,
                        new_phase
                            .map(|p| p.to_string())
                            .unwrap_or("complete".to_string())
                    ),
                    message,
                ))
            }
            _ => Err(ToolError::validation(format!(
                "Invalid action: {}. Use 'request_review' or 'approve'.",
                args.action
            ))),
        }
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
    async fn test_what_now_no_state() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        // No state created - should auto-initialize
        let tool = AceWhatNowTool;
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        // Should auto-initialize with a WS- prefixed ID and show requirements phase
        assert!(result.output.contains("WS-")); // Auto-generated ticket ID
        assert!(result.output.contains("Requirements Phase"));

        // State file should now exist
        let state_path = dir.path().join(".wonopcode").join("state.yaml");
        assert!(state_path.exists());
    }

    #[tokio::test]
    async fn test_what_now_with_state() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.ticket_title = Some("Test ticket".to_string());
        state.save(dir.path()).unwrap();

        let tool = AceWhatNowTool;
        let result = tool.execute(json!({}), &ctx).await.unwrap();

        assert!(result.output.contains("WON-123"));
        assert!(result.output.contains("Requirements Phase"));
        assert!(result.output.contains("Create Use Cases"));
    }

    #[tokio::test]
    async fn test_submit_checkpoint_request_review() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let tool = AceSubmitCheckpointTool;
        let result = tool
            .execute(
                json!({
                    "checkpoint": "requirements",
                    "action": "request_review"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("REQUIREMENTS Review Request"));
        assert!(result.output.contains("approved"));
    }

    #[tokio::test]
    async fn test_submit_checkpoint_approve() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let tool = AceSubmitCheckpointTool;
        let result = tool
            .execute(
                json!({
                    "checkpoint": "requirements",
                    "action": "approve"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("Approved"));
        assert!(result.output.contains("Current phase:"));
        assert!(result.output.contains("Next phase:"));
        assert!(result.output.contains("analysis"));

        // Verify state was updated
        let state = WorkstreamState::load(dir.path()).unwrap().unwrap();
        assert_eq!(state.workflow.current_phase, WorkflowPhase::Analysis);
    }

    #[tokio::test]
    async fn test_implementation_plan_requires_tasks() {
        let dir = tempdir().unwrap();
        let ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let tool = AceSubmitCheckpointTool;
        let result = tool
            .execute(
                json!({
                    "checkpoint": "implementation_plan",
                    "action": "request_review"
                }),
                &ctx,
            )
            .await;

        // Should fail because no tasks exist
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No tasks defined"));
    }

    #[tokio::test]
    async fn test_implementation_plan_requires_valid_parents() {
        let dir = tempdir().unwrap();
        let _ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let store = ArtifactStore::new(dir.path()).unwrap();
        store.ensure_directories().unwrap();

        // Try to create a task WITHOUT a valid parent (empty parents)
        // The store should reject this at creation time
        let result = store.create_artifact(
            &mut state,
            ArtifactType::Task,
            "Orphan task",
            "",
            vec![], // No parent!
            Priority::Medium,
            false,
        );

        // The store validates parent types at creation time
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("requires at least one parent"));
    }

    #[tokio::test]
    async fn test_implementation_plan_with_valid_parents() {
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

        // Create proper artifact hierarchy: SESSION -> UC -> REQ -> DES -> TASK
        let uc = store
            .create_artifact(
                &mut state,
                ArtifactType::UseCase,
                "Test Use Case",
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
                "Test Requirement",
                "Content",
                vec![uc.metadata.id.clone()],
                Priority::Medium,
                false,
            )
            .unwrap();

        let des = store
            .create_artifact(
                &mut state,
                ArtifactType::Design,
                "Test Design",
                "Content",
                vec![req.metadata.id.clone()],
                Priority::Medium,
                false,
            )
            .unwrap();

        // Create task with valid parent (the design)
        store
            .create_artifact(
                &mut state,
                ArtifactType::Task,
                "Implement feature",
                "",
                vec![des.metadata.id.clone()],
                Priority::High,
                false,
            )
            .unwrap();

        state.save(dir.path()).unwrap();

        let tool = AceSubmitCheckpointTool;
        let result = tool
            .execute(
                json!({
                    "checkpoint": "implementation_plan",
                    "action": "request_review"
                }),
                &ctx,
            )
            .await;

        // Should succeed because task has valid parent
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.output.contains("Implementation Plan"));
        assert!(output.output.contains("Implement feature"));
        assert!(output.output.contains(&des.metadata.id));
    }

    #[tokio::test]
    async fn test_implementation_plan_rejects_invalid_parent_id() {
        let dir = tempdir().unwrap();
        let _ctx = test_context(dir.path().to_path_buf());

        let mut state = WorkstreamState::new("WON-123");
        state.save(dir.path()).unwrap();

        let store = ArtifactStore::new(dir.path()).unwrap();
        store.ensure_directories().unwrap();

        // Try to create a task with a parent ID that doesn't exist
        // The store should reject this at creation time
        let result = store.create_artifact(
            &mut state,
            ArtifactType::Task,
            "Task with fake parent",
            "",
            vec!["DES-FAKE-001".to_string()], // Non-existent parent
            Priority::Medium,
            false,
        );

        // The store validates that parent artifacts exist
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Parent artifact not found") || err.contains("DES-FAKE-001"));
    }
}
