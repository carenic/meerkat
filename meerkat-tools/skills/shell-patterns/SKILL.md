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
- For ordinary long-running commands, use `shell` with `background: true`,
  keep the returned `job_id`, then inspect with `shell_job_status` or list jobs
  with `shell_jobs`.
- A background shell submission is durable, but its worker is explicitly
  non-resumable. After a restart the job record remains and a lost worker
  becomes `worker_lost`; Meerkat does not silently replay the command.
- Use `monitor_start` only when that high-trust tool is explicitly visible and
  the script follows its declared protocol. Framed JSONL can emit typed
  notification, checkpoint, progress, and complete frames; notifications do
  not complete the job.
- Cancel stuck or obsolete jobs with `shell_job_cancel`.
- Shell output is truncated. Shape large output with tools such as `rg`,
  `head`, `tail`, or structured command flags.
- Set the working directory intentionally. Prefer absolute paths when a command
  crosses project boundaries.
- Treat shell output as evidence to reason from, not as durable work state. Add
  WorkGraph evidence separately when a shared item needs proof.
- Shell is classified as mutating because Meerkat cannot prove an arbitrary
  command is read-only. It is unavailable under read-only tool policy.
