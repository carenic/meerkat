---
name: schedule-workflow
description: How to author and inspect durable schedules from agent tools
requires_capabilities: [schedule]
---

# Schedule Workflow

Use Schedule when something should happen later, repeatedly, or on a wall-clock
calendar. Schedules own time and delivery. They do not own live work state.

## Operating Rules

- Create schedules with `meerkat_schedule_create` when the user asks for a
  reminder, recurrence, follow-up, monitor, wakeup, or routine automation.
- Choose the smallest trigger that matches the request: `once` for one future
  instant, `interval` for fixed cadence, `calendar` for wall-clock recurrence
  in a named timezone.
- Use `resumable_session` when the schedule should wake an existing session and
  `materialize_on_demand_session` when no session exists yet.
- Prefer `misfire_policy: {"type":"skip"}`,
  `overlap_policy: "skip_if_running"`, and
  `missing_target_policy: "mark_misfired"` unless the user asks for catch-up or
  concurrency.
- Inspect with `meerkat_schedule_get`, `meerkat_schedule_list`, and
  `meerkat_schedule_occurrences` before creating duplicates.
- Pause for temporary suspension, resume to reactivate, update to change future
  behavior, and delete only when the schedule should stop permanently.

## Boundaries

- Use WorkGraph for pending work, dependencies, claims, and evidence.
- Use Schedule for time. A scheduled prompt may ask an agent to inspect
  WorkGraph, but Schedule should not duplicate WorkGraph readiness logic.
- Use memory for recalled knowledge, not for future wakeups.
- Use builtin tasks for private scratch items that do not need scheduled
  delivery or shared durable coordination.
