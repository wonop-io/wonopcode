# DAG Conversation Notes

Status: discovery draft

## Vision

- Replace session-centric storage with a global DAG of immutable nodes.
- Treat a "conversation" as a reconstructable view over a path in that DAG.
- Support forking, subagents, compaction, observations, tags, URI extraction, and file diffs as first-class graph data.
- Support agent-to-agent communication, including subagent result delivery back to a parent agent.
- Carry explicit user handles on user-originated messages, starting with `@user` and expanding later for SaaS identities.

## Initial Node Candidates

- `ChatCompletionConfiguration`
- `ChatCompletionMessage`
- `ChatStart`
- `TodoTask`
- `TodoStatusUpdate`
- `Workstream`
- `RequestToUser`
- `ResponseFromUser`
- `Compaction`
- `UriReference`
- `FileDiff`
- `Observation`
- `Tag`

## Core Capabilities

- Reconstruct a linear conversation from any leaf back to the nearest boundary node.
- Compact a conversation path into a new node and continue from that compacted state.
- Derive observations, tags, URI references, and file references from traversed messages.
- Start subagents from any leaf.
- Fork conversations from any existing node.
- Query graph data by node id, tag, URI, and file.

## Early Design Direction

- Prefer append-only immutable nodes.
- Separate graph edges from node payloads.
- Persist nodes individually on disk so the full graph never needs to be loaded.
- Maintain secondary indexes for tag, URI, file, and leaf lookup.
- Keep conversational lineage single-parent for `ChatCompletionMessage` nodes.
- Generate observations, tags, and URI references lazily in the background or immediately after message streaming completes.
- Start with a DAG scoped to a workstream, with a migration path to app-wide and eventually distributed storage.
- Design for future pruning of old nodes while retaining distilled learnings or memory artifacts.
- Keep storage abstraction compatible with a future object-store backend such as S3.

## Confirmed Constraints

- `ChatCompletionMessage` lineage is always single-parent.
- Enrichment is lazy, not required on the write path.
- The first implementation should operate as a single DAG for a workstream.
- Long term the model should support a wider app-level DAG and potentially cross-machine distribution.

## Additional Concepts

- Agent-to-agent messages need an explicit representation, not just inferred `ChatCompletionMessage` records.
- Subagents must be able to return structured results to a parent agent.
- User-authored messages must include a handle field even before multi-user support is active.

## Evolution Concerns

- Trimming must preserve important derived knowledge even if raw historical nodes are deleted.
- Remote/object-store persistence should be possible without redesigning node identity or index semantics.

## Open Questions

- Whether `ChatStart` and `AgentStart` are the same concept or separate node types.
- Whether `Compaction` is a subtype of `ChatStart` or a distinct boundary node.
- Whether tags are standalone nodes, edge metadata, or both.
- Whether `FileDiff` stores patches inline, by chunk, or by reference to artifact blobs.
- How strict conversation reconstruction should be when a node has multiple parents or mixed reference types.
- Whether agent-to-agent communication is best represented as a dedicated node type or as a specialized message payload with routing metadata.
- Whether pruning should be tombstone-based, snapshot-based, or implemented through compaction plus garbage collection.
