# DAG Phase 9 Cutover

Status: DAG is now the canonical durable source for message history.

## What Changed

- `WONOPCODE_DAG_ENABLE_GRAPH_WRITES` now defaults to enabled.
- `WONOPCODE_DAG_ENABLE_GRAPH_READS` now defaults to enabled.
- `WONOPCODE_DAG_ENABLE_SESSION_FALLBACK` remains available as a migration
  safety valve for manual validation, but runtime transcript snapshots now rely
  on DAG projection as the canonical source.
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

The following APIs still exist for compatibility, parity diagnostics, and
manual migration auditing, but they are no longer canonical conversation reads:

- `SessionService::get_history`
- `SessionService::get_recent_history`
- `SessionService::legacy_history_for_fallback`
- `SessionService::recent_legacy_history_for_fallback`
- `ActiveWorkstream::get_conversation_history`
- `ActiveWorkstream::get_recent_conversation_history`

They are retained for compatibility and tooling, but DAG projection is the
sole durable read path that runtime snapshots and reconnect flows should use.

## Operational Guidance

- Leave fallback available while validating existing real `.wonopcode` state.
- Use `SessionService::audit_all_sessions()` to enumerate stored sessions and
  capture cutover diagnostics before and after any migration changes.
- Investigate any parity warning before disabling fallback globally.
- Once a deployment has proven stable on existing workstreams, disabling
  `WONOPCODE_DAG_ENABLE_SESSION_FALLBACK` should be a clean final confidence
  check, not the first cutover step.

## Final Cutover Posture

- DAG nodes and projection metadata are the only canonical durable source for
  root conversation and workflow/subagent transcript history.
- Legacy session reads remain only to support parity comparison, audit tooling,
  and explicit migration debugging while older on-disk state is still being
  validated.
- New runtime snapshot code should not introduce additional session-history
  fallback layers above `SessionService`; that migration policy belongs in core.

## Validation Matrix

Phase 9 validation should track which transcript shapes are covered by automated
tests versus which still require live `.wonopcode` validation.

| Area | Coverage | Notes |
| --- | --- | --- |
| Root messages | Automated | Core DAG-first history tests cover canonical user/assistant transcript reads. |
| Tools | Automated | Transcript parity tests cover assistant tool-call rows plus completed tool results. |
| Streaming | Manual | Existing reconnect snapshot coverage exists, but raw DAG/session parity still needs real workstream validation. |
| Manual compaction | Automated | Parity tests cover compaction rewrite plus persisted compaction marker visibility. |
| Branching | Automated | Core and workstream tests cover preferred-leaf continuation and sibling isolation. |
| Workflow children | Automated | Workflow DAG restore tests cover child transcript restoration after reload. |
| Nested children | Automated | Workflow DAG restore tests cover nested child restoration and mailbox return paths. |
| Mailbox returns | Automated | Workflow persistence tests cover returned child results after reload. |
| Reload and reconnect | Partially automated | Snapshot/reconnect tests are green, but existing real `.wonopcode` transcript parity remains to be exercised manually. |
| Workstream switch | Manual | Still needs explicit parity validation on existing on-disk workstreams. |
| Legacy-only, dual-write, DAG-primary-with-fallback | Automated | Mixed-mode session tests cover empty-DAG fallback, dual-written parity, and longer legacy history without silent override. |
| Existing `.wonopcode` data | Manual | Required before fallback can be disabled in broader environments. |
