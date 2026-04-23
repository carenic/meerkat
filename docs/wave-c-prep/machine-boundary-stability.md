# Machine Boundary Stability — Wave (c) Prep

Status: Analysis input for wave (c) coupling decisions. READ-ONLY survey.
Scope: The 5 canonical machines in the 0.6 catalog as of `meerkat-machine-schema/src/catalog/dsl/`.

## 0. Baseline

Historical base rate from the 0.5→0.6 chronicle: **1 of 16 v0.5 machines survived to 0.6 by name** (`flow_run`); 94% non-survival. That is the prior we should couple against. Wave (c) will bind shell code much more tightly to today's machines; anything we hardcode by Rust type or struct name becomes the next wave-a demolition target when one of them moves.

Current canonical set (5), sizes measured against `catalog/dsl/*.rs`:

| Machine | DSL LOC | Phases | Core role |
|---|---|---|---|
| `MeerkatMachine` | 3059 | 8 (Initializing / Idle / Attached / Running / Retired / Stopped / Destroyed / + Recovering dead) | session-scoped execution kernel |
| `MobMachine` | 1387 | 4 (Running / Stopped / Completed / Destroyed) | mob-scoped orchestration + topology |
| `OccurrenceLifecycleMachine` | 345 | 9 (Pending / Claimed / Dispatching / AwaitingCompletion / Completed / Skipped / Misfired / Superseded / DeliveryFailed) | per-occurrence dispatch |
| `ScheduleLifecycleMachine` | 224 | 3 (Active / Paused / Deleted) | schedule trigger lifecycle |
| `AuthMachine` | 164 | 5 (Valid / Expiring / Refreshing / ReauthRequired / Released) | per-binding auth lease |

The sizes matter: `MeerkatMachine` is an order of magnitude bigger than any sibling. Big machines have more heterogeneous concerns, and therefore more split risk.

## 1. Per-machine concern enumeration

### 1.1 MeerkatMachine (`meerkat-machine-schema/src/catalog/dsl/meerkat_machine.rs:9-220`)

Concerns owned by the extended state block (verbatim groups from the DSL file, cited):

1. **Session lifecycle & identity** — `session_id`, `active_runtime_id`, `active_fence_token`, `current_run_id`, `pre_run_phase`, `lifecycle_phase` (lines 10-15).
2. **Ops / turn execution** — `silent_intent_overrides` (16), committed-visible-set transitions (`PublishCommittedVisibleSet*` lines 981-1087), boundary apply (`BoundaryAppliedPublish` 972).
3. **Realtime attachment authority** — `realtime_intent_present`, `realtime_binding_state`, `realtime_binding_authority_epoch`, `realtime_reattach_required`, `realtime_next_authority_epoch` (20-24); plus `realtime_product_turn_phase` (76), `realtime_projection_freshness` (87), `realtime_reconnect_policy` (96).
4. **Live-topology reconfigure** — `live_topology_phase` (27), `ReconfigureSessionLlmIdentity*` transitions (803-832).
5. **MCP server authority** — `mcp_server_states` (32).
6. **Peer interaction lifecycle** — `pending_peer_requests`, `inbound_peer_requests` (41-43).
7. **Session context advancement** — `last_session_context_updated_at_ms` (53).
8. **Interaction stream lifecycle** — `reserved_interaction_streams`, `attached_interaction_streams` (66-67).
9. **Peer-ingress transport capability** — `peer_ingress_owner_kind`, `peer_ingress_comms_runtime_id`, `peer_ingress_mob_id` (110-112).
10. **Supervisor-bridge authorization** — `supervisor_binding_kind` + bound name/peer_id/address/epoch (130-134).
11. **Track-B peer projection** — `local_endpoint`, `direct_peer_endpoints`, `mob_overlay_peer_endpoints`, `peer_projection_epoch`, `mob_overlay_epoch` (175-179).
12. **Drain lifecycle** — `NotifyDrainExited` (928), `EnsureDrainRunning*` (1283-1299).

That is 12 distinct concerns in one machine. Nothing enforces they stay together.

### 1.2 MobMachine (`.../dsl/mob_machine.rs:9-50`)

1. **Mob lifecycle** — `lifecycle_phase` with 4 phases (72-76).
2. **Member lifecycle (runtime roster)** — `live_runtime_ids`, `externally_addressable_runtime_ids`, `runtime_fence_tokens`, `member_state_markers` (11-21).
3. **Coordinator binding** — `coordinator_bound` (16), `BindCoordinator`/`UnbindCoordinator` signals (151-152).
4. **Run accounting** — `active_run_count`, `pending_spawn_count` (14-15).
5. **Identity↔runtime map** — `identity_to_runtime` (29).
6. **Wiring graph** — `wiring_edges` (25).
7. **Member-session bindings (W3-H / dogma #4)** — `member_session_bindings: Map<AgentIdentity, SessionId>` (48) + `topology_epoch` (49).
8. **Task board** — `tasks`, `in_progress_task_ids`, `completed_task_ids` (32-36).
9. **Flow/frame/loop execution** — consumed via compat bridges (`FlowRunMachine`, `FlowFrameMachine`, `LoopIterationMachine` in `meerkat-machine-schema/src/compat/`); the mob authority machine calls out to those kernels rather than owning them inline.

### 1.3 OccurrenceLifecycleMachine (`.../dsl/occurrence_lifecycle.rs:8-29`)

1. **Occurrence lifecycle phase** — 9 phases, terminal set `[Completed, Skipped, Misfired, Superseded, DeliveryFailed]` (53-63).
2. **Claim / lease management** — `claimed_by`, `lease_expires_at_utc_ms`, `claimed_at_utc_ms`, `claim_token` (15-19).
3. **Dispatch correlation** — `delivery_correlation_id`, `last_receipt`, `dispatched_at_utc_ms`, `completed_at_utc_ms` (20-25).
4. **Failure classification** — `failure_class`, `failure_detail`, `attempt_count` (22-26).
5. **Revision / supersede** — `schedule_revision`, `occurrence_ordinal`, `superseded_by_revision` (12-27).
6. **Targeting** — `target_binding_key`, `due_at_utc_ms` (14-15).

### 1.4 ScheduleLifecycleMachine (`.../dsl/schedule_lifecycle.rs:8-18`)

1. **Schedule phase** — Active / Paused / Deleted (33-37).
2. **Trigger key** — `trigger_key` (11).
3. **Target binding** — `target_binding_key` (12).
4. **Policies** — `misfire_policy`, `overlap_policy`, `missing_target_policy` (13-15).
5. **Planning window** — `planning_cursor_utc_ms`, `next_occurrence_ordinal` (16-17).
6. **Revision** — `revision` (10).

### 1.5 AuthMachine (`.../dsl/auth_machine.rs:24-50`)

1. **Lease phase** — Valid / Expiring / Refreshing / ReauthRequired / Released (44-50).
2. **Expiry watermark** — `expires_at` (31).
3. **Refresh bookkeeping** — `last_refresh`, `refresh_attempt` (32-33).

That's it. Per-binding, 3 fields, <200 LOC including transitions.

## 2. Split-risk analysis

### MeerkatMachine — **HIGH** split risk

This is the obvious candidate. Twelve concerns, 3059 LOC, three of them (realtime attachment, supervisor-bridge authorization, Track-B peer projection) are recent additions with their own epoch namespaces (`realtime_next_authority_epoch`, `supervisor_bound_epoch`, `peer_projection_epoch` distinct from `mob_overlay_epoch`). The DSL comments even disclaim the coupling — Track-B calls out "dogma #11: derived projections never authoritative" (lines 149-150), and the peer-ingress block explicitly names "the s71 regression class closed structurally" (lines 107-108) as the rationale for absorbing it.

Candidate future splits:

- **Realtime attachment sub-machine.** 8 realtime fields (20-24, 76, 87-88, 96) with their own authority epoch. A realtime-focused overhaul (the `realtime-259-port-plan.md` already cited in comments) is the most likely trigger.
- **Peer interaction / interaction-stream sub-machine.** Fields 41-67 are one coupled subsystem (peer request lifecycle, stream reservation/attach). Already has two independent cleanup effects (`PeerInteractionCleanup`, `InteractionStreamCleanup`).
- **Supervisor-bridge authorization sub-machine.** Fields 130-134 are a self-contained authorization fact with its own invariant (`supervisor_binding_consistency`).
- **Track-B peer projection sub-machine.** Already described as "peer-projection state" with its own epoch (175-179).

The machine also straddles two timescales: *per-turn* (pre_run_phase, silent_intent_overrides, current_run_id) versus *across-turns* (mcp_server_states, supervisor_binding_kind, realtime_binding_state). That heterogeneity is the textbook tell for future separability.

### MobMachine — **MEDIUM** split risk

Slimmer and more coherent after the 0.6 simplification pass (`machine-simplification-proposal.md:43-97` documents the deliberate pruning of kickoff / step-dispatch / frame-terminal / loop-mirror layers). However:

- **Flow/frame/loop execution** lives in separate compat kernels (`FlowRunMachine`, `FlowFrameMachine`, `LoopIterationMachine`). They are the one name-survivor from 0.5 for a reason — they have already proven separable. Risk: they re-absorb into Mob, or further split as distributed mob topologies land.
- **Task board** (fields 32-36) is orthogonal to roster/wiring. If cross-mob task migration or persistent task queues land, this could lift into its own machine.
- **W3-H member-session bindings** are DSL-canonical but new (2026-04 per memory/feedback_preserve_bridge_transport). Any identity-first refactor that changes the identity↔runtime↔session cardinality will touch it.

### OccurrenceLifecycleMachine — **LOW–MEDIUM** split risk

A 9-phase lifecycle with a clean terminal set. Claim/lease management is arguably separable from dispatch-correlation state, and future leader-elected delivery would stress that seam, but today it's tight.

### ScheduleLifecycleMachine — **LOW** split risk

Small, simple, 3 phases. No plausible split axis.

### AuthMachine — **LOW** split risk

164 LOC, 3 stateful fields, per-binding instance. Already the result of a *merge rejection*: the auth_machine.rs header (lines 3-23) documents that the original design absorbed auth into MeerkatMachine and was **refactored out after review**. So the direction of drift has already been settled against absorption.

## 3. Merge-risk analysis

- **Occurrence + Schedule merge.** They share a domain (scheduler), but Schedule is a 1:N parent to Occurrence. `SupersedePendingOccurrences` in `meerkat-schedule/src/machines/schedule_lifecycle.rs:82` is `disposition => routed [OccurrenceLifecycleMachine]`, i.e. already modeled as cross-machine routing. Merging would force Schedule state to carry per-occurrence state, which is a strictly worse data model. **Low merge risk.** Opportunity: if a scheduler rewrite re-does the cardinality, they could collapse, but nothing in-repo hints at this.
- **AuthMachine → MeerkatMachine absorption.** Already rejected and documented (`auth_machine.rs:3-23`). **Very low merge risk.**
- **Schedule+Occurrence → MeerkatMachine.** No — they are runtime-level, not session-scoped. Cardinality is wrong.
- **MobMachine + MeerkatMachine.** No — different scope (mob vs session). But the member-session binding map (`member_session_bindings`) is the exact seam where the two meet today, and cross-machine invariants (Track-B overlay) already flow between them. Risk is *further seam-protocolization*, not merge.

Net: the 5 machines look stable against merges; splits are the only realistic direction of change.

## 4. Current tight coupling (top offenders)

### 4.1 Struct-name imports of `MeerkatMachine`

237 constructor sites (`MeerkatMachine::ephemeral()` / `MeerkatMachine::persistent(...)`) across non-target, non-test code. Highest-density offenders (live):

- `meerkat-cli/src/main.rs` — 14 occurrences, 5 at construction (`main.rs:5250, 9398, 9463, 9904, 10066`).
- `meerkat-rpc/src/router.rs` — 4 occurrences, including `router.rs:2539` constructing `MeerkatMachine::persistent(...)` directly in the RPC surface.
- `meerkat-rest/src/lib.rs` — 3 occurrences.
- `meerkat-mob/src/runtime/local_bridge.rs` — 7 constructor sites (`local_bridge.rs:252, 265, 278, 297, 316, 335, 350`).
- `meerkat-mob-mcp/src/` — 5 constructor sites across `surface.rs`, `lib.rs`, `agent_tools.rs`.
- `meerkat/src/service_factory.rs` — 2 constructor sites (`service_factory.rs:719, 920`).
- `meerkat-openai/src/realtime_attachment.rs` — 3 sites.

All of these bind the surface crate to the concrete struct `MeerkatMachine`. If the machine splits into (e.g.) `SessionCoreMachine + RealtimeAttachmentMachine`, every one of these call sites becomes a wave-a-style edit.

### 4.2 String-literal machine names

`MachineId::parse("...")` appears in the compat bridges (`meerkat-machine-schema/src/compat/*.rs:15,598,1942,2089,2095,2101`) and `xtask/src/ownership_ledger.rs` has **28 literal `"MeerkatMachine"` occurrences** plus `xtask/src/rmat_policy.rs:191,193` hardcoding the `("MobMachine", ..., "MeerkatMachine")` tuple for routed-disposition rewrites. These are the RMAT-audit side; they are policy code, but they still encode machine identity as a string key and will need touching on any rename.

### 4.3 Schedule / Occurrence / Auth

- `meerkat-schedule/src/lifecycle.rs` uses `sched_dsl::ScheduleLifecycleMachineAuthority` / `occ_dsl::OccurrenceLifecycleMachineAuthority` directly (lines 188-720). Single crate, tight coupling, but scoped.
- `meerkat-machine-schema/src/lib.rs:61-79` hardcodes the four names `"ScheduleLifecycleMachine"`, `"OccurrenceLifecycleMachine"` in composition contract checks.
- `AuthMachine` is only referenced in schema / catalog / tests / `e2e_auth_lane.rs` — no surface crate hardcodes its name today. Good.

### 4.4 No typed `MachineId`-keyed shell dispatch exists

`MachineId` is a kernel-level identity (`meerkat-machine-kernels/src/runtime.rs:176-217`) used by the schema, compositions, and coverage manifests. It is **not** used by any shell crate for lookup; shells reach for `meerkat_runtime::MeerkatMachine` by Rust path. Wave (b)'s B-1 typed newtype exists but has no consumer in the shell dispatch path.

## 5. Coupling recommendations for wave (c)

Bind shell code to machines in ways that tolerate a future split:

1. **Shell-level lookup by `MachineId`, not by Rust path.** Wave (c) should introduce an `Arc<dyn MachineHandle>` (or equivalent trait object) keyed by `MachineId`, so CLI/REST/RPC/MCP/mob-bridge obtain the session/mob/schedule/occurrence/auth handle via typed lookup. The 237 `MeerkatMachine::ephemeral()` call sites should reduce to a single factory that a split cannot multiply.
2. **Never re-export concrete `MeerkatMachine`/`MobMachine` from the facade.** Today `meerkat-runtime::MeerkatMachine` is imported from 7+ surface crates. If wave (c) wraps this in a `MachineRegistry` or `SessionBindings` opaque type, the split cost collapses to one crate.
3. **Namespace DSL per machine so splitting is a module-move.** `catalog/dsl/meerkat_machine.rs` is one 3059-line file. If the realtime sub-state (fields 20-96) were in `catalog/dsl/meerkat_machine/realtime.rs` today, lifting it to its own machine would be an rg+mv plus a regen. Wave (c) should enforce one-concern-per-file within any machine >500 LOC.
4. **Keep machine identity off the wire.** Audit `meerkat-contracts` for any public wire/SDK type that mentions `MeerkatMachine`/`MobMachine`/etc. by name. SDK users should not be able to build code against a machine name; a split then has no external contract cost. (Not exhaustively checked in this pass — flag for wave (c) implementers.)
5. **Protocolize cross-machine seams before wave (c) hardens shell bindings.** Today, `member_session_bindings` (Mob) and Track-B peer projection (Meerkat) are implicitly coupled via `RecomputeMobPeerOverlay`. Formalize that seam as an in-repo obligation ledger entry (the `formal-seam-closure.md` pattern) so a future Meerkat split cannot silently drop the obligation.
6. **RMAT policy table by typed machine id.** `xtask/src/rmat_policy.rs:191-193` uses `(&str, &str, &str)` tuple keys; migrate to `(MachineId, InputVariantId, MachineId)`. Compiler-enforced after a rename.

## 6. Leading indicators to track

The chronicle's "`meerkat-core` churn velocity" is one. Others specific to machine-boundary moves:

1. **DSL LOC growth rate per machine.** `MeerkatMachine` going from N to 2N in a release is the loudest split signal; track the 3 biggest machines per minor.
2. **Number of distinct epoch namespaces inside one machine.** `MeerkatMachine` today has at least 4 independent epoch counters (`realtime_next_authority_epoch`, `peer_projection_epoch`, `mob_overlay_epoch`, `supervisor_bound_epoch`, plus implicit turn epoch). Every new epoch field is a latent split axis — each epoch usually means "this concern has its own linearization order."
3. **Count of distinct terminal/cleanup effects.** `PeerInteractionCleanup` + `InteractionStreamCleanup` + `CommsTrustReconcileRequested` living in one machine means that machine is running three independent reclamation loops. >2 is a smell.
4. **Cross-machine routed effects.** Each new `routed [Other]` disposition (e.g. `SupersedePendingOccurrences` → `OccurrenceLifecycleMachine`) is evidence the seam is load-bearing. If a single machine accumulates many inbound routes, it's pulling gravity; if a machine accumulates many outbound routes, it's probably fissioning.
5. **Surface-only input growth.** `machine-simplification-proposal.md:43-58` shows 40+ Meerkat queries moved to `surface_only_inputs`. Continued growth of this list in one machine is a tell that the machine has become an API facade rather than an authority — a refactoring pressure that often precedes a split.
6. **TLC state-space inflation per minor.** Post-simplification baseline (`machine-simplification-proposal.md:29,37`): Meerkat 3,959 distinct states, Mob 6,401. Monitor: a >2x jump with no new phases means new orthogonal state — i.e. an in-progress hidden machine.
7. **Hopcroft quotient ratio (raw / phase).** Same source: phase-quotient close to raw quotient = clean phase-projection; divergence = internal state the phase projection does not see, which is also a split tell.

Track items (1), (2), (4) routinely in CI; (6) and (7) are already emitted by `cargo xtask machine-hopcroft`. Budget a wave-c-review gate that fails if any machine's DSL LOC grows >25% in one release without a corresponding split.

## Summary

`MeerkatMachine` carries high split risk (12 concerns, multiple epoch namespaces, three recent additions that already read as future sub-machines); `MobMachine` is medium (stable core but flow/frame kernels and task board are separable); `OccurrenceLifecycleMachine` is low-medium; `ScheduleLifecycleMachine` and `AuthMachine` are low. The biggest wave-(c)-observable coupling problem today is the 237+ direct constructor sites of `MeerkatMachine` spread across every surface crate plus `meerkat-mob-mcp`, `meerkat-openai`, and the CLI — any split turns each into an edit. Wave (c) should route all shell-to-machine lookups through a `MachineId`-keyed registry, keep machine identity off the public wire, file-split any machine >500 LOC by concern, and treat cross-machine routed dispositions + per-machine epoch-namespace count as the leading indicators that a boundary move is coming.
