# DAG Phase 9 Cutover

Status: DAG-first read path is now the default migration posture.

## What Changed

- `WONOPCODE_DAG_ENABLE_GRAPH_WRITES` now defaults to enabled.
- `WONOPCODE_DAG_ENABLE_GRAPH_READS` now defaults to enabled.
- `WONOPCODE_DAG_ENABLE_SESSION_FALLBACK` remains enabled by default so mixed
  workstreams can still recover from legacy-only history or temporary parity
  gaps.
- Workstream snapshots prefer DAG-derived conversation extraction and carry
  active leaf plus branch metadata through the protocol and desktop frontend.
- Workflow tree persistence no longer treats `conversations.json` as a durable
  source of truth. Per-node conversation ownership stays in the DAG; workflow
  persistence only stores operational tree state plus graph projection metadata.

## Cutover Diagnostics

The cutover now exposes graph/session diagnostics in core:

- whether DAG reads and writes are enabled
- whether legacy fallback is still permitted
- the current active leaf, if any
- legacy history message count
- DAG projection message count
- parity result and first mismatch index, when available

The workstream snapshot path logs a warning when DAG/session parity diverges
while DAG reads are enabled.

## Remaining Legacy Boundaries

The following APIs still exist for compatibility but should not be treated as
the primary read path anymore:

- `SessionService::get_history`
- `SessionService::get_recent_history`
- `ActiveWorkstream::get_conversation_history`
- `ActiveWorkstream::get_recent_conversation_history`

They are retained for compatibility and tooling, but the DAG projection is the
intended primary conversation source.

## Operational Guidance

- Leave fallback enabled while validating existing real `.wonopcode` state.
- Investigate any parity warning before disabling fallback globally.
- Once a deployment has proven stable on existing workstreams, disabling
  `WONOPCODE_DAG_ENABLE_SESSION_FALLBACK` should be a clean final confidence
  check, not the first cutover step.
