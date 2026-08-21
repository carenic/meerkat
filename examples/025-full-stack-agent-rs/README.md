# 025 - Composed Agent (Rust)

A focused standalone example that composes built-in tools, two domain tools,
budget limits, a file-backed store, inline behavior instructions, and event
streaming. It is not an exhaustive production reference.

## Features Used
- `AgentBuilder` configuration
- `CompositeDispatcher` - merge built-in and domain tools
- `BudgetLimits` - cap total tokens and tool calls
- Inline system-prompt behavior instructions
- Event streaming with `spawn_event_logger`
- `JsonlStore` in a temporary directory for this run

## Architecture
```
┌─────────────────────────────────┐
│         Composed Agent          │
│                                 │
│  ┌─────────┐   ┌────────────┐  │
│  │ Builtins│   │ Domain     │  │
│  │ tasks   │   │ search_docs│  │
│  │ datetime│   │ create_tkt │  │
│  └────┬────┘   └─────┬──────┘  │
│       └───────┬───────┘         │
│        Composite Dispatcher     │
│               │                 │
│  ┌────────────┴──────────────┐  │
│  │      Agent Loop           │  │
│  │  LLM → Tools → Events    │  │
│  │  Budget + event stream    │  │
│  └───────────────────────────┘  │
│               │                 │
│  ┌────────────┴──────────────┐  │
│  │ JsonlStore (temporary dir)│  │
│  └───────────────────────────┘  │
└─────────────────────────────────┘
```

This program does not configure the canonical skill engine, hooks, structured
output, shell, MCP, comms, delegation, or runtime-backed restart recovery.
Follow the focused examples for those surfaces.

## Run
```bash
# From the repository root
ANTHROPIC_API_KEY=sk-... ./scripts/repo-cargo run -p meerkat \
  --example 025-full-stack-agent --features jsonl-store
```
