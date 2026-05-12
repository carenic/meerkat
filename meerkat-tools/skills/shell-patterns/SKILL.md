---
name: Shell Patterns
description: Background job patterns with shell and job management tools
requires_capabilities: [builtins, shell]
---

# Shell Patterns

Use shell tools for concrete local commands, verification, and long-running
processes. Keep commands scoped to the user's workspace and report important
results back into the conversation.

## Operating Rules

- Use `shell` for short foreground commands.
- For long-running servers, watchers, or tests, use `background=true`, keep the
  returned `job_id`, then inspect with `shell_job_status`.
- Cancel stuck or obsolete jobs with `shell_job_cancel`.
- Shell output is truncated. Shape large output with tools such as `rg`,
  `head`, `tail`, or structured command flags.
- Set the working directory intentionally. Prefer absolute paths when a command
  crosses project boundaries.
- Treat shell output as evidence to reason from, not as durable work state. Add
  WorkGraph evidence separately when a shared item needs proof.
