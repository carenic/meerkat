# 024 - Multi-Turn Event Processing (Rust)

Submit multiple turns to one in-process session and observe context and event
streaming across those turns.

## What This Example Does

Creates an incident-response coordinator using `EphemeralSessionService`. The
program directly submits three prompts that simulate an incident changing over
time:

1. **Turn 1**: Initial alert (CPU spike on prod-web-03)
2. **Turn 2**: Monitoring update (memory high, recent deploy found)
3. **Turn 3**: Resolution (rollback, metrics normalizing)

Each turn streams `AgentEvent`s and the agent maintains conversation context
across all three turns within the current process.

## Concepts
- `EphemeralSessionService` - in-memory session lifecycle with dedicated Tokio tasks
- `create_session()` - creates a session and runs the initial turn
- `start_turn()` - directly submits each later prompt
- `AgentEvent` streaming - real-time events across multiple turns
- Session state reads - observe accumulating context between turns
- `archive()` - clean shutdown of the session task

## Runtime Boundary

This example does not register a comms identity, receive webhooks, run a
scheduler, or recover after process exit. Those behaviors belong to the
runtime-backed CLI, REST, JSON-RPC, MCP, and SDK surfaces. For example:

```bash
rkat run --keep-alive --comms-name processor "Process incoming events"
```

Use this example when embedding the standalone session service or testing
multi-turn behavior. Use a runtime-backed surface for durable ingress and
long-lived operational agents.

## Run
```bash
# From the repository root
ANTHROPIC_API_KEY=sk-... ./scripts/repo-cargo run -p meerkat \
  --example 024-host-mode-event-mesh --features jsonl-store
```
