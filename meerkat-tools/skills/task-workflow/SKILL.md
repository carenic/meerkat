---
name: Task Workflow
description: How to use builtin task tools for private lightweight work tracking
requires_capabilities: [builtins]
---

# Task Workflow

Use builtin task tools for lightweight local/session/project work tracking.
They are useful scratch structure, not the shared durable WorkGraph.

## Operating Rules

- Use `task_create` for private planning, checklists, or local project tasks
  that do not need realm-wide claims, evidence, or cross-agent readiness.
- Use clear imperative subjects and descriptions with acceptance criteria.
- Move a task to `in_progress` when you start and `completed` only when it is
  actually done.
- Use `task_list` after completing a task to find the next local item.
- Keep dependencies simple. If blocking relationships must coordinate multiple
  agents or survive compaction/restarts as shared truth, use WorkGraph instead.

## Boundary With WorkGraph

- Builtin tasks: private scratch, local task lists, simple progress tracking.
- WorkGraph: realm-scoped durable commitments, readiness, dependency topology,
  claims, leases, evidence, and terminal truth.
- Schedule: time-based wakeups and recurrence.
- Memory: recalled knowledge, not live task state.
