---
name: Hook Authoring
description: Writing hooks for the 8 hook points, execution modes, and decision semantics
requires_capabilities: [hooks]
---

# Hook Authoring

Use hooks for runtime observation and policy decisions at typed lifecycle
points. Hooks can observe, allow, or deny; they should not become hidden owners
of runtime truth.

## Hook Points

Meerkat provides 8 hook points in the agent lifecycle:
1. **RunStarted** — When the agent run begins
2. **PreLlmRequest** — Before sending to the LLM
3. **PostLlmResponse** — After receiving LLM response
4. **PreToolExecution** — Before executing a tool call
5. **PostToolExecution** — After tool execution completes
6. **TurnBoundary** — At the boundary between turns
7. **RunCompleted** — When the agent run completes successfully
8. **RunFailed** — When the agent run fails

## Execution Modes

- Foreground blocks execution and can deny. Use for policy enforcement.
- Background runs concurrently. Use for logging and analytics.

## Decision Semantics

Hooks return one of:
- Allow: proceed normally.
- Deny: block the operation with a reason.
- Observe only: return no decision and no patches.

## Boundaries

Semantic hook patches are retired. Hooks can observe typed projections and
deny through the typed decision shape; provider parameters, assistant text,
tool arguments/results, and final run text remain owned by the runtime/tool/LLM
authority that produced them.

`failure_policy` is retained for compatibility. Current hook runtime failures
fail closed through typed engine errors; they are not converted into
warning-only success or hook-local denials by `fail_open` / `fail_closed`.

Use WorkGraph for durable work state, Schedule for time, and memory for
knowledge retrieval. Hooks may observe those surfaces, but they do not own
their semantics.
