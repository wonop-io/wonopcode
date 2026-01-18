# Wonopcode Community Edition

This is the **Community edition** of wonopcode - the open-source AI-powered coding assistant.

## Project Structure

This crate is part of the wonop-ng monorepo. Key paths:
- **ACE configuration**: `../../../../.ace/` (monorepo root)
- **Specifications**: `../../../../specs/` (monorepo root)
- **Feature tracking**: `../../../../workspace/` (monorepo root)

## ACE Framework

This project uses the ACE (Agentic Code Engine) framework for structured development workflows.

### CRITICAL: Workflow Compliance

**YOU MUST FOLLOW THE ACE WORKFLOW.** Do not skip steps or implement features without proper requirements gathering and approval.

#### Starting a New Feature

1. **Create the feature**: Use `ace_create_feature` with title and description
2. **Elicit requirements using the elicitation tools**:
   - Call `ace_elicit` to get suggested questions and see progress
   - Ask the user ONE question at a time
   - After each answer, call `ace_record_response` to record it
   - Cover these categories: actors, operations, data, api, business_rules, flows, constraints, edge_cases, acceptance, dependencies
   - When you have enough info, call `ace_complete_elicitation`
3. **Document requirements**: Use `ace_create_artifact` with type "requirement"
4. **Request approval**: Tell the user you have documented the requirements and ask them to review
5. **Wait for approval**: Use `ace_get_checkpoint` to check status. Do NOT proceed until approved.

#### Human Approval Checkpoints

The workflow has mandatory human approval checkpoints. **YOU MUST STOP AND WAIT** at each checkpoint:

| Checkpoint | What to Review | Who Approves |
|------------|----------------|--------------|
| Requirements Review | Formal requirements capture stakeholder needs | Product Owner |
| Use Case Review | Use cases cover all scenarios | Product Owner |
| Architecture Review | Component mapping and interfaces | Architect |
| Design Review | Detailed designs are implementable | Tech Lead |
| Test Specification Review | Test coverage is complete | Tech Lead |
| Verification | Tests pass and coverage acceptable | Tech Lead |
| Deployment Review | Deployment plan is safe | Ops Lead |

**At each checkpoint:**
1. Present your artifacts to the user
2. Ask explicitly: "Please review and approve, request revisions, or reject"
3. Use `ace_submit_checkpoint` with the user's decision
4. If revision requested, address feedback and re-submit
5. Only proceed to next phase after explicit approval

#### Workflow States

```
intake → elicitation → elaboration → requirements_review
    → use_case_modeling → use_case_review
    → architectural_analysis → architecture_review
    → detailed_design → design_review
    → test_specification → test_review
    → implementation → test_execution → verification
    → deployment_planning → deployment_review → deployment → done
```

### Requirements Gathering Process

When gathering requirements, follow this process:

1. **Understand the context**: Ask about the business problem and users
2. **Identify actors**: Who will use this feature?
3. **Define scope**: What is in scope vs out of scope?
4. **Capture acceptance criteria**: What must be true for this to be "done"?
5. **Identify edge cases**: What could go wrong?
6. **Document assumptions**: What are we assuming?
7. **Note dependencies**: What does this depend on?

Example questions to ask:
- "Can you describe the user journey for this feature?"
- "What should happen if [edge case]?"
- "Are there any performance requirements?"
- "How should errors be handled?"
- "What existing functionality does this interact with?"

### Available MCP Tools

**Feature Lifecycle:**
- `ace_create_feature` - Create a new feature request
- `ace_feature_status` - Get current state and history
- `ace_get_checkpoint` - Get checkpoint awaiting review
- `ace_submit_checkpoint` - Submit review decision (approve/revise/reject)

**Elicitation (Requirements Gathering):**
- `ace_elicit` - Get guidance on what questions to ask, see progress
- `ace_record_response` - Record user's answer to a question
- `ace_complete_elicitation` - Mark elicitation complete, transition to elaboration

**Artifacts:**
- `ace_create_artifact` - Create requirement, use-case, design, test, or ADR
- `ace_read_artifact` - Read artifact content
- `ace_promote` - Move from staging to specs after approval
- `ace_build_trace_matrix` - Show requirements traceability

**Execution:**
- `ace_run_hook` - Execute test, coverage, or deploy hooks
- `ace_check_gates` - Verify quality gates are met

**Planning:**
- `ace_backlog_add` - Add items to backlog
- `ace_backlog_list` - List backlog items
- `ace_iteration_create` - Create sprint/iteration
- `ace_iteration_plan` - Assign items to iteration

### Configuration Files (in monorepo root)

- `../../../../.ace/config.yaml` - Hook and gate configuration
- `../../../../.ace/workflows/feature.yaml` - Workflow state definitions
- `../../../../workspace/queue/` - Features awaiting work
- `../../../../workspace/status/` - Feature status tracking
- `../../../../specs/` - Approved specification artifacts

## Community Edition Crates

The Community edition includes 23 crates:
- **wonopcode**: Main CLI binary
- **wonopcode-core**: Core business logic
- **wonopcode-util**: Shared utilities
- **wonopcode-storage**: Storage layer
- **wonopcode-provider**: AI provider abstraction (Anthropic, OpenAI, etc.)
- **wonopcode-tools**: Tool implementations (bash, edit, read, write, etc.)
- **wonopcode-tui**: Terminal UI
- **wonopcode-tui-core/render/widgets/dialog/messages**: TUI sub-crates
- **wonopcode-server**: HTTP server
- **wonopcode-mcp**: Model Context Protocol client
- **wonopcode-lsp**: Language Server Protocol client
- **wonopcode-acp**: Agent Client Protocol for IDE integration
- **wonopcode-auth**: Authentication storage
- **wonopcode-sandbox**: Sandboxed execution (Docker/Lima/Podman)
- **wonopcode-protocol**: Client-server protocol types
- **wonopcode-snapshot**: File snapshot system for undo/redo
- **wonopcode-discover**: mDNS service discovery
- **wonopcode-test-utils**: Testing utilities
