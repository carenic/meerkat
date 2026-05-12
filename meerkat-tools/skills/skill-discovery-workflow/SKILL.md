---
name: skill-discovery-workflow
description: How to browse, load, and use Meerkat skills during a session
requires_capabilities: [skills]
---

# Skill Discovery Workflow

Use skills when the available skill inventory contains domain guidance that
would improve the current turn. Skills are explicit instruction manuals; they
are discoverable by default but not automatically injected unless a session or
turn asked for them.

## Operating Rules

- Use `browse_skills` when the inventory is in collection mode, when the user
  mentions a domain, or when you suspect a built-in companion skill exists for
  a tool family.
- Use `load_skill` with a typed builtin `SkillKey` when you need the full
  operating guidance before calling a tool family.
- Treat tool descriptions as schema and capability summaries. Treat companion
  skills as the source for when, how, and why to use a tool family.
- Respect capability gating. If a skill is absent, continue with the tools that
  are actually available instead of assuming the capability exists.
- Do not preload broad skills just in case. Load only the skill needed for the
  current workflow.

## Companion Skills

Companion skills are embedded skills owned by a Meerkat crate or tool family.
They are gated by `requires_capabilities` and teach agent behavior for a
capability such as WorkGraph, Schedule, shell, memory, comms, hooks, or builtin
utilities.
