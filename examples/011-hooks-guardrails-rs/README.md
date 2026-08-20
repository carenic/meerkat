# 011 — Hooks & Guardrails (Rust)

Intercept and control agent behavior at 8 defined hook points. Use hooks for
audit logging, content filtering, approval gates, cost tracking, and more.

## Concepts

- `HookPoint` - 8 interception points in the agent lifecycle
- `HookCapability` - observe (read-only) or guardrail (Allow/Deny)
- `HookExecutionMode` - foreground (blocking) or background (async)
- `HookAdapterConfig` - command, HTTP, or in-process execution
- `DefaultHookEngine` - the standard hook processor

## Hook Points
1. `run_started`
2. `pre_llm_request`
3. `post_llm_response`
4. `pre_tool_execution`
5. `post_tool_execution`
6. `turn_boundary`
7. `run_completed`
8. `run_failed`

## Run
```bash
# From the repository root
ANTHROPIC_API_KEY=sk-... ./scripts/repo-cargo run -p meerkat \
  --example 011-hooks-guardrails --features jsonl-store
```
