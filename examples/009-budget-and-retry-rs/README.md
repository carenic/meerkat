# 009 — Budget & Retry Policies (Rust)

Production guardrails: token budgets, tool-call limits, and retry policies for
resilient agent execution.

## Concepts
- `BudgetLimits` — hard caps on tokens, tool calls, and duration
- `RetryPolicy` - exponential backoff for provider failures classified as
  retryable
- Applying a retry policy to an `AgentBuilder`
- Handling budget exhaustion returned by the agent run

## Budget Types
| Limit | Description |
|-------|-------------|
| `max_tokens` | Hard cap on cumulative token usage |
| `max_tool_calls` | Max tool invocations |
| `max_duration` | Wall clock timeout |

## Retry Strategy

When the provider returns a typed retryable failure, the configured policy
uses this schedule. The example prints the policy but does not force a live
provider failure.

```
Attempt 1 → fail → wait 500ms →
Attempt 2 → fail → wait 1s →
Attempt 3 → fail → wait 2s →
Attempt 4 → give up
```

## Run
```bash
# From the repository root
ANTHROPIC_API_KEY=sk-... ./scripts/repo-cargo run -p meerkat \
  --example 009-budget-and-retry --features jsonl-store
```
