---
name: workgraph-workflow
description: How to use WorkGraph for durable commitments, dependencies, claims, and evidence
requires_capabilities: [work_graph]
---

# WorkGraph Workflow

Use WorkGraph when work must survive sessions, compaction, restarts, schedules,
or coordination between agents. It is the shared commitment graph, not private
scratch space and not semantic memory.

## Operating Rules

- Use `workgraph_ready` to find eligible work. Do not infer readiness from item
  fields, blocker counts, due times, or edges yourself.
- Claim an item before doing durable or shared work with `workgraph_claim`.
  Include your typed owner and the current `revision`.
- If a write fails with a stale revision, reload the item with `workgraph_get`
  or `workgraph_snapshot`, reconsider the current state, then retry only if the
  work still makes sense.
- Use `workgraph_link` for real dependencies and relationships. Use `blocks`
  only when the target should not be ready until the source is terminally
  resolved.
- Attach evidence with `workgraph_add_evidence` for artifacts, PRs, logs,
  summaries, external tickets, or other proof that the work changed state.
- Close with `workgraph_close` only when terminal truth exists. Use
  `completed`, `failed`, or `cancelled` honestly.
- Release a claim with `workgraph_release` when you are stopping before terminal
  completion and the work should be claimable by someone else.

## Boundaries

- Use builtin `task_*` tools for private, local, lightweight scratch tracking.
- Use WorkGraph for shared durable commitments, dependencies, readiness, claims,
  and evidence.
- Use Schedule for time-based wakeups and recurrence. A schedule can wake an
  agent, but WorkGraph remains the live work state.
- Use memory for knowledge retrieval and historical context. Memory does not
  own live work state.

## Typical Loop

1. Call `workgraph_ready` for the active realm and namespace.
2. Pick an item that matches the current objective.
3. Claim it with the current `revision`.
4. Do the work.
5. Add evidence for durable outputs.
6. Update the item if the scope, timing, priority, or labels changed.
7. Close it only when the outcome is terminal, or release it if another agent
   should continue.
