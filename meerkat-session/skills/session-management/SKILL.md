---
name: Session Management
description: Session persistence, resume patterns, event store replay, compaction tuning
requires_capabilities: [session_store]
---

# Session Management

Use session management guidance when resuming, inspecting, or reasoning about a
conversation's persisted runtime state.

## Operating Rules

- Resume with the `session_id` from a previous run. The runtime rebuilds the
  session from durable state when persistence is enabled.
- Treat projection files as derived. Canonical session state belongs to the
  runtime/store machinery, not to ad hoc file edits.
- Compaction may replace old transcript detail with summaries. Use skills,
  memory, files, and WorkGraph to recover relevant context when needed.
- Session persistence preserves conversation continuity. It does not replace
  WorkGraph for shared durable work or Schedule for future wakeups.

## Compaction

- Lower thresholds compact more often and reduce live context.
- Higher thresholds preserve more live context but cost more tokens.
- If compaction causes ambiguity, inspect durable artifacts or ask for the
  missing detail instead of inventing it.
