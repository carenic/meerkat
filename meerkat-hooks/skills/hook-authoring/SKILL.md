---
name: Hook Authoring
description: Writing hooks for the 8 hook points, execution modes, and typed decision semantics
requires_capabilities: [hooks]
---

# Hook Authoring

Use hooks for runtime observation and policy decisions at typed lifecycle
points. Hooks can observe, allow, or deny; they should not become hidden owners
of runtime truth.

## Hook Points

Meerkat provides 8 hook points in the agent lifecycle:

1. **RunStarted** - When the agent run begins
2. **PreLlmRequest** - Before sending to the LLM
3. **PostLlmResponse** - After receiving the LLM response
4. **PreToolExecution** - Before executing a tool call
5. **PostToolExecution** - After tool execution completes
6. **TurnBoundary** - At the boundary between turns
7. **RunCompleted** - When the agent run completes successfully
8. **RunFailed** - When the agent run fails

## Execution Modes

- Foreground hooks run in ascending priority, then registration order. A deny
  short-circuits later foreground hooks at that point.
- Background hooks run concurrently and must declare `capability: observe`.
  Their decisions are discarded. Use them for logging and analytics, not
  policy enforcement.
- Runtime adapters are in-process handlers, commands, or HTTP endpoints. Each
  entry has a typed adapter, timeout, point, mode, capability, and priority.

## Decision Semantics

Foreground hooks return one of:

- Allow: proceed normally.
- Deny: block the operation with a reason.
- Observe only: return no decision and no patches.

## Boundaries

Semantic hook patches are retired. Hooks can observe typed projections and
deny through the typed decision shape; provider parameters, assistant text,
tool arguments/results, and final run text remain owned by the runtime/tool/LLM
authority that produced them.

There is no per-hook `failure_policy`. Invalid configuration, execution
failure, and timeout are typed engine errors. Foreground failures fail the run;
background failures are recorded as dropped background dispatches and do not
become hook-local denials.

Tool hook projections carry optional `ToolProvenance`, and LLM-response
projections carry typed provider-native `server_tool_content`. Classify those
typed fields synchronously instead of parsing display text or tool names.

Use WorkGraph for durable work state, Schedule for time, and memory for
knowledge retrieval. Hooks may observe those surfaces, but they do not own
their semantics.
