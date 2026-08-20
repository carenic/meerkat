# 019 — Mob: Pipeline (Rust)

Construct and validate a staged mob definition, spawn a coordinator plus stage
workers, wire their topology, and submit illustrative lint and test turns.
The example is a topology and manual-dispatch walkthrough, not an executing
pass/fail pipeline engine.

## Concepts
- `MobDefinition` profiles, skills, topology, limits, and a sample flow DAG
- Definition validation before mob creation
- Explicit coordinator and stage-worker wiring
- Manual turns sent to the lint and test members
- In-memory mob storage and ephemeral sessions

## Pipeline Stages
```
MobDefinition -> validate -> create mob -> spawn members -> wire topology
                                                        |
                                                        +-> lint turn
                                                        +-> test turn
```

The deploy member is spawned for topology completeness, but this example does
not run a deploy turn, inspect the lint result to gate the test turn, or invoke
the declared `FlowSpec`. Use the flow APIs when application-owned execution and
gating are required.

## Run
```bash
# From the repository root
ANTHROPIC_API_KEY=sk-... ./scripts/repo-cargo run -p meerkat-mob \
  --example 019-mob-pipeline
```
