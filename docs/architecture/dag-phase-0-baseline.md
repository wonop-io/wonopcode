# DAG Phase 0 Baseline

Status: approved implementation baseline for `wonopcode-dag-prd.md`

## Purpose

This document closes Phase 0 of the DAG migration work. It records:

- the v1 scope and guardrails
- the current session-centric seams in the codebase
- the compatibility strategy for existing on-disk data
- the minimum parity matrix we must keep green while migrating

The goal is to stop the codebase from drifting further into a session-first
model while the DAG implementation is introduced.

## V1 Scope

The v1 storage model is:

- one append-only DAG per `Workstream`
- every node and edge carries `workstream_id`
- linear conversation views are derived from `Prev` edges only
- `ChatStart` and `Compaction` are the only boundary node types
- branching is allowed by having multiple children point at the same parent
- subagents live in the same workstream DAG and return via dedicated
  `AgentResult` nodes
- enrichment is lazy and outside the primary write path
- v1 does not support cross-workstream traversal
- v1 does not support replication, sync, or distributed consistency

## Guardrails And Invariants

The migration must preserve these invariants:

- Canonical history becomes graph-native, not `Vec<Message>`-native.
- `ChatCompletionMessage` lineage is single-parent for conversation
  reconstruction.
- Semantic edges never affect linear conversation extraction.
- A workstream can have multiple leaves, but a reconstructed conversation path
  is always one boundary-to-leaf lineage.
- Dual-write is mandatory before DAG becomes the only source of truth.
- Existing UI behavior must stay stable during dual-write.
- Enrichment, indexing, compaction, and pruning must not block message append or
  streaming.
- Retention tier changes lifespan, not deletability.

## Current-State Mapping

The current implementation is still primarily session-centric. These are the
main seams that the DAG migration must replace or wrap.

### Primary Conversation Persistence

- `crates/shared/core/wonopcode-core/src/session_service.rs`
  - high-level conversation lifecycle
  - creates or restores a current session per workstream directory
  - loads history for the runner
  - saves user and assistant messages
  - exposes persisted history to the workstream server
- `crates/shared/core/wonopcode-core/src/session.rs`
  - `Session` and `SessionRepository`
  - stores session metadata, parent session links, messages, and parts
  - supports session fork and child-session discovery
- `crates/shared/core/wonopcode-core/src/message.rs`
  - canonical persisted message and part types for the legacy model
- `crates/shared/core/wonopcode-core/src/message_convert.rs`
  - converts between provider messages and legacy session/message storage

### Runner And Execution Flow

- `crates/agent/runner/wonopcode-runner/src/runner.rs`
  - loads history from `SessionService`
  - persists user and assistant activity back into session storage
  - drives compaction, todo updates, and observational-memory side effects
  - still contains legacy assumptions like mailbox wake prompts and
    session-derived history loading

### Workflow Tree And Subagents

- `crates/agent/loop/wonopcode-workflow-tree/src/tree.rs`
  - owns workflow nodes, parent-child structure, and mailbox-based returns
- `crates/agent/loop/wonopcode-workflow-tree/src/persistence.rs`
  - persists workflow tree snapshots and per-node conversations separately from
    core session storage
- `crates/agent/loop/wonopcode-workflow-tree/SPEC.md`
  - documents a conversation-per-node model that is still separate from the
    proposed DAG

### Workstream State And Reliable Snapshots

- `crates/protocol/types/wonopcode-protocol/src/reliable/snapshot.rs`
  - current reconnect/reload source of truth for a workstream
  - includes `messages: Vec<ConversationHistoryMessage>` and other operational
    state in a session-like structure
- `crates/workstream/server/wonopcode-pro-server/src/workstream_protocol.rs`
  - server-facing workstream snapshot and protocol surface
  - mirrors the same session-oriented conversation assumptions

### Desktop Read Path

- `crates/ui/desktop/wonopcode-desktop-frontend/src/contexts/transcript_projection.rs`
  - merges snapshot history, live events, mailbox rows, workflow rows, and
    subagent rows into a visible transcript timeline
- `crates/ui/desktop/wonopcode-desktop-frontend/src/contexts/workstream_context.rs`
  - stores the current workstream transcript and operational state
- `crates/ui/desktop/wonopcode-desktop-frontend/src/components/workstream_chat_view_enhanced.rs`
  - renders the workstream chat and still assumes a single visible root
    conversation stream

### Adjacent Persistent State That Must Eventually Attach To The DAG

- `crates/execution/orchestrator/wonopcode-wasm-orchestrator/src/observation_persistence.rs`
  - persists observational memory outside the core conversation history
- `crates/tools/wonopcode-tools-todo/src/lib.rs`
  - todo/task logic that currently is not modeled as durable DAG nodes
- `crates/storage/wonopcode-snapshot/src/store.rs`
  - file snapshot/revert system that must remain compatible with DAG-backed
    transcript history and file-diff nodes

## Session-Centric Assumptions We Must Remove

These assumptions exist today and must not be carried forward into new DAG work:

- A conversation is primarily an ordered vector of messages.
- A single current session owns the main conversation history.
- Subagent history can be persisted outside the main conversation model and
  stitched back in later.
- Reconnect snapshots can treat `messages` as the canonical history object.
- Forking is modeled as session copying rather than lineage branching.
- User-input, todo status, file diffs, and agent returns do not need first-class
  durable graph records.

## Compatibility Strategy For Existing On-Disk Data

The migration will preserve existing workstreams and sessions by using the
following compatibility policy.

### Canonical Policy During Migration

- Legacy session storage remains readable throughout the migration.
- New activity is dual-written to legacy session storage and DAG storage before
  any read-path cutover.
- DAG read-path rollout must be feature-flagged independently from DAG writes.
- Session fallback remains available until DAG parity is proven on real
  workstreams.

### Existing Data Mapping

- A legacy workstream keeps its existing session history on disk.
- On first DAG-aware access, that history can be imported or lazily backfilled
  into DAG records for the same workstream.
- The first boundary for imported history is a synthetic `ChatStart`.
- Legacy session forking maps to multiple descendants from a shared lineage
  node, not to copied vectors in the final model.

### On-Disk Compatibility Constraints

- We must not require deleting existing `.wonopcode` state to adopt the DAG.
- We must tolerate mixed-mode workstreams during rollout:
  - workstreams with legacy-only history
  - workstreams with dual-written legacy plus DAG history
  - workstreams reading primarily from DAG with legacy fallback
- Any secondary index can be rebuilt from canonical DAG node and edge files.
- DAG corruption must never silently discard recoverable legacy history.

## Phase 0 Parity Matrix

The following scenarios are the minimum matrix the migration must preserve from
the first dual-write build onward.

| Area | Required parity |
| --- | --- |
| Root conversation | Same visible user/assistant transcript before and after reload |
| Streaming root message | Same partial message after reconnect and snapshot restore |
| Tool execution rows | Same ordering, lifecycle state, and terminal output after replay |
| Manual compaction | Same post-compaction visible transcript and continued execution |
| Automatic or emergency compaction | Same UX-visible state and same recoverable continuation |
| Root branching | Same ability to continue from a chosen leaf without corrupting siblings |
| Subagent start | Same visible subagent creation and stable parent linkage |
| Subagent completion | Same returned result content and same durable final row |
| Nested subagents | Same parent-child structure and completion propagation |
| Mailbox or user-input flow | Same pending prompt visibility and same response linkage after reconnect |
| Todo lifecycle | Same current task state and same ordered status history |
| File diffs | Same visible change summaries and same lookup by affected path |
| URI, tag, observation enrichment | No user-facing regression; enrichment remains asynchronous |
| Workstream switch | Same isolation between workstreams and no history bleed |
| Reload and reconnect | Same visible timeline from snapshot plus replay as before migration |
| Legacy import | Imported legacy history must reconstruct to the same visible transcript as session storage |

## Phase 0 Exit Criteria

Phase 0 is complete when all of the following are true:

- the v1 scope is written down and shared
- the main session-centric seams are documented
- the mixed-mode compatibility strategy is written down
- the minimum parity matrix is defined
- the repository still passes `bazel build //...` and `bazel test //...`

## Immediate Next Phases

The next implementation phases should proceed in this order:

1. Add graph domain types, storage traits, and disk layout.
2. Implement indexes and conversation extraction helpers.
3. Dual-write the existing session-centric persistence path.
4. Migrate workflow tree, subagents, todos, user-input, and file diffs.
5. Move snapshots and frontend read-paths to DAG-derived views.
