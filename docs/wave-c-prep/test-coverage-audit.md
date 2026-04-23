# Wave (a) Test Coverage Audit

**Baseline:** `origin/main` (`9f035fdb7`)
**Head:** `dogma/wave-a-demolition` (`c0cb12071`)
**Diff:** 146 files, -20 973 / +8 621 LOC across 85 commits
**Counting rule:** `#[test]` / `#[tokio::test]` attribute occurrences per file, summed per crate.

## 1. Per-crate test count delta

Net change: **5047 → 4950 (-97 tests, -1.9 %)**. Eleven crates moved; the rest are flat.

| Crate | Pre | Post | Delta |
|---|---:|---:|---:|
| meerkat-skills | 91 | 31 | **-60** |
| meerkat-mob | 799 | 775 | -24 |
| meerkat-runtime | 673 | 649 | -24 |
| meerkat-core | 666 | 665 | -1 (hides -24 deletions + 23 new tests across 3 new integration files) |
| meerkat-tools | 412 | 409 | -3 (7 deletions, 4 new) |
| meerkat-rest | 80 | 75 | -5 |
| meerkat | 255 | 251 | -4 |
| meerkat-mob-pack | 41 | 37 | -4 |
| meerkat-cli | 155 | 152 | -3 |
| meerkat-rpc | 319 | 317 | -2 |
| meerkat-llm-core | 84 | 82 | -2 |
| meerkat-mob-mcp | 80 | 79 | -1 |
| meerkat-machine-schema | 44 | 61 | **+17** (new typed-identity suites) |
| meerkat-machine-kernels | 8 | 16 | **+8** |
| meerkat-contracts | 149 | 154 | +5 |
| meerkat-comms | 317 | 323 | +6 |
| **Total** | **5047** | **4950** | **-97** |

## 2. Deleted test-bearing files

Every pre-wave-a `.rs` file that contained `#[test]` and no longer exists at HEAD:

| File | Pre-tests | Deleted in |
|---|---:|---|
| `meerkat-mob/src/runtime/mob_member_lifecycle_authority.rs` | 4 | `949162121` |
| `meerkat-mob/src/runtime/mob_wiring_authority.rs` | 3 | `0ad584cde` |
| `meerkat-mob/tests/track_b_cutover_source_scan.rs` | 6 | `7f88cb477` |
| `meerkat-runtime/src/composition_dispatch.rs` | 9 | `ce2dbe35e` |
| `meerkat-runtime/src/recompute_mob_peer_overlay.rs` | 9 | `ce2dbe35e` |
| `meerkat-runtime/src/comms_trust_reconcile.rs` | 7 | `ce2dbe35e` |
| `meerkat-runtime/tests/recompute_mob_peer_overlay_e2e.rs` | 1 | `ce2dbe35e` |
| `meerkat-rpc/tests/router_realtime_target.rs` | 1 | `488944b7d` |
| **Total** | **40** | |

### Behaviors covered pre-wave-a (from source of deleted files)

- **mob_member_lifecycle_authority**: member terminal classification in the face of `restore_failure`, missing active session, unknown sessionless members, retiring members. Pure state-machine reducer unit tests.
- **mob_wiring_authority**: symmetric local-edge reconciliation, idempotent external wire spec, unwire-when-no-edges is a no-op. Pure reducer unit tests.
- **track_b_cutover_source_scan**: compile-time/source invariants — mob machine declares topology epoch + Track-B effects, meerkat machine declares peer projection + overlay driver, overlay driver module remains public, dead flow-frame loop driver stays deleted.
- **composition_dispatch**: composition-driver dispatcher contract — name/descriptor agreement, duplicate rejection, effect→watcher routing, declared-route enforcement, decision-failure surfacing as typed error.
- **recompute_mob_peer_overlay**: overlay-driver unit tests — wire two members to distinct sessions and route cross-endpoints, respawn rotates session and clears prior overlay, unwire clears mutual overlays, idempotent recompute, release-binding removes member, shadow-mode parity with actor restore.
- **recompute_mob_peer_overlay_e2e**: full respawn flow installs+rotates trust via driver+reconciler (integration).
- **comms_trust_reconcile**: first-reconcile registers effective peers, subsequent reconcile adds/removes, stale-epoch accept-as-no-op, add-failure surfaces typed error, empty-set clears trusted peers, serialization/concurrency tests on trust-store mutations.
- **router_realtime_target**: RPC realtime-open-info rejects mob-member target.

### Big in-file deletions (not whole-file deletions, but whole test blocks)

| File | Pre | Post | Δ | Notes |
|---|---:|---:|---:|---|
| `meerkat-skills/src/resolve.rs` | 11 | 0 | -11 | Skill resolver precedence, defaults, stdio wiring |
| `meerkat-skills/src/source/filesystem.rs` | 11 | 0 | -11 | Filesystem scan/quarantine/collection tests |
| `meerkat-skills/src/source/protocol.rs` | 10 | 0 | -10 | Stdio protocol handshake, capability mismatch, timeout |
| `meerkat-skills/src/source/composite.rs` | 8 | 0 | -8 | Composite source ordering/precedence |
| `meerkat-skills/src/source/git.rs` | 8 | 0 | -8 | Git source fetch/quarantine |
| `meerkat-skills/src/source/http.rs` | 6 | 0 | -6 | HTTP source fetch/health |
| `meerkat-core/src/skills/mod.rs` | 21 | 8 | -13 | Skill introspection serde, collection derivation, ref boundary |
| `meerkat-mob/src/roster.rs` | 39 | 29 | -10 | Roster-level assertions on removed Track-B variants |
| `meerkat-core/src/agent/state.rs` | 38 | 33 | -5 | `run_completed` lifecycle, turn-boundary denial, hook rewrite |
| `meerkat-core/src/comms.rs` | 14 | 9 | -5 | Peer-request params/body promotion, input-stream mode |
| `meerkat-tools/src/builtin/skills/browse.rs` | 5 | 0 | -5 | `test_browse_*` end-to-end tool tests |
| `meerkat-tools/src/builtin/skills/load.rs` | 2 | 0 | -2 | Skill load tool tests |
| `meerkat-mob-pack/src/pack.rs` | 12 | 8 | -4 | Pack-format serde cases |
| `meerkat/src/surface/schedule_host.rs` | 3 | 0 | -3 | Schedule dispatch/admission/completion typing |
| `meerkat-rest/src/lib.rs` | 55 | 52 | -3 | Endpoints for deleted mob-realtime attach/detach |

## 3. Orphan / stale references

Scan for symbols from deleted modules in files that still compile:

| Location | Dead reference | Severity |
|---|---|---|
| `meerkat-machine-codegen/tests/render_contracts.rs:305,367` | String-literal `"use meerkat_runtime::composition_dispatch::*;"` — test asserts on generated output that imports a deleted module | medium (codegen contract test is vacuous — any downstream crate instantiating it will fail to compile) |
| `meerkat-machine-codegen/src/artifacts.rs` | Doc-comment + generator emits `meerkat_runtime::composition_dispatch::CompositionDriverTrait` | medium (codegen emits references to deleted module) |
| `meerkat-machine-schema/src/composition.rs` + `tests/schema_contracts.rs` | Comments cite `meerkat-runtime::composition_dispatch` | low (doc-only) |

No actual broken `use` statements in test files. The codegen references are the real orphan risk: they will silently emit code that cannot compile once the driver is wired. This is consistent with the known broken tree state.

## 4. Behaviors now untested (lost-vs-replaced analysis)

| Pre-wave-a behavior (test location) | Replacement path | Tested now? |
|---|---|---|
| Mob Wire / Unwire reducer (mob_wiring_authority) | `MobMachineInput::WireMembers` / `::UnwireMembers` via DSL | **Yes** — `meerkat-mob/tests/member_session_bindings.rs` exercises both variants |
| Mob member lifecycle terminal classification | Authority collapsed into `MobMachine` DSL guards | **Partial** — DSL guard tests exist; restore-failure-breaks-present-member scenario not found |
| Composition dispatcher contract (9 tests) | Composition dispatch deleted; drivers now catalog-declared | **No direct replacement** — schema round-trip tests (`catalog_typed_round_trip`) assert declaration, not dispatch |
| Recompute mob peer overlay (9 unit + 1 e2e) | DSL effect in `MeerkatMachine::peer_projection` | **Partial** — `meerkat-runtime/tests/peer_projection_dsl.rs` (13 tests) covers endpoint publish/clear, direct peer add/remove, overlay apply; does **not** cover respawn rotation, shadow-mode parity, release-binding sweep |
| Comms trust reconcile (7 tests, incl. concurrency) | Authority now lives in `meerkat_machine::dsl` peer projection | **Partial** — 6 references in `peer_projection_dsl.rs`; concurrency/serialization + add-failure typed error paths are **not covered** |
| Skills resolver precedence + defaults (11 tests) | Collapsed resolver in `resolver.rs` | **Partial** — 5 tests in `resolver.rs`; precedence, explicit-roots, no-roots-fallback **not covered** |
| Filesystem skill source (11 tests) | `meerkat-skills/src/source/filesystem.rs` retypedered | **No** — quarantine diagnostics, collection-md fallback, recursive scan, invalid-ratio transitions **not covered** |
| Skill stdio protocol (10 tests) | `meerkat-skills/src/source/protocol.rs` retypedered | **No** — handshake caching, capability mismatch, unknown-skill-not-found, transport timeout **not covered** |
| Skill browse/load builtin tools (7 tests) | Live builtins in `meerkat-tools/src/builtin/skills/` | **No** — tool integration tests for root/collection/search/empty-collection listings deleted outright |
| Router realtime-target rejection | Realtime endpoints deleted | Intentional |
| Schedule host typed dispatch/admission (3 tests) | `meerkat/src/surface/schedule_host.rs` rewritten | **Partial** — no surface-level tests pinning the rejected-runtime / runtime-terminated mappings |
| Agent state run-completed / turn-boundary denial (5 tests) | `meerkat-core/src/agent/state.rs` | **Partial** — 33 tests survive; the specific `run_completed_hook_failure_emits_run_failed_without_run_completed` and `turn_boundary_denial_blocks_boundary_side_effects_and_turn_completed` paths are not obviously covered by name |
| Comms peer request params/body promotion (5 tests) | `meerkat-core/src/comms.rs` | **No** — params/body promotion rules dropped with their test |

## 5. Classification

### Lost but intentional (behavior gone)

- Router realtime-target rejection (endpoints deleted)
- Realtime MCP leaky member tools (deleted wholesale)
- `MobCommand::{Wire,Unwire}` enum + dispatch arms (replaced by typed `MobMachineInput`)
- Track-B split binding vocabulary
- Legacy string-keyed skill composition

### Lost and concerning (behavior exists, no test)

- **Skill source protocol** (stdio handshake caching, capability mismatch, transport timeout) — behaviors still exist in `source/protocol.rs`; 10 tests gone
- **Skill filesystem source** (recursive scan, quarantine diagnostics, collection-md fallback) — 11 tests gone, code still present
- **Skill resolver precedence** (defaults, explicit-roots, no-roots fallback) — 6 of 11 tests gone
- **Skill browse/load tools** (root/collection/search/empty listing) — 7 tests gone; tools still dispatched
- **Composition dispatcher contract** (effect→watcher routing, duplicate rejection, decision-failure error type) — dispatcher deleted but catalog-declared drivers still need *some* contract; replacement is schema-shape round-trip, not runtime behavior
- **Schedule host typed dispatch** (rejected-runtime and runtime-terminated mapping) — surface tests gone; behavior still reachable from `meerkat-cli`, `meerkat-rest`, `meerkat-rpc` schedule hosts
- **Agent `run_completed` lifecycle edge cases** (hook-rewrite, hook-failure emits run-failed, turn-boundary denial blocks side effects)
- **Comms peer-request params/body promotion** (5-variant precedence rules)
- **Pack serde round-trip** (4 missing cases)

### Lost and blocker (critical path, no test)

- **Trust reconcile concurrency + error surfacing**: serialized-under-concurrency semantics and "add-failure surfaces typed error and does not update applied view" were explicit regressions fixed in PR #340 per memory. Wave (a) deleted the tests; `peer_projection_dsl.rs` references `trust_reconcile` 6 times but does not pin the concurrency invariant.
- **Respawn overlay rotation** (`respawn_of_m1_rotates_to_new_session_and_clears_prior_session_overlay`): load-bearing for mob member lifecycle; DSL tests cover simpler endpoint mutation but not the rotation scenario.
- **Shadow-mode parity with actor restore** (`shadow_mode_parity_matches_actor_restore_wiring_after_respawn`): the specific property that DSL-projected overlay matches shell-restored wiring. With shell authority demolished, parity is the cutover gate.
- **Release-binding removes member from recompute**: session-lifecycle-triggered overlay cleanup.
- **Mob member restore-failure terminal classification**: post-restore, a failed member must be classified Broken. Not covered by the new DSL Wire/Unwire tests.
- **Turn-boundary denial blocks side-effects**: pre-wave-a `turn_boundary_denial_blocks_boundary_side_effects_and_turn_completed` pinned authority around side-effects during deny. Core path; no obvious replacement.

## 6. Minimum test-rebuild list for wave (c)

Wave (c) MUST add tests for the following before it can claim coverage parity:

1. **Peer-projection concurrency**: `concurrent_reconciles_are_serialized_and_stale_short_circuits` — port against the DSL reducer path.
2. **Peer-projection stale reconcile under serialization**: `stale_reconcile_under_serialization_does_not_leak_trust_store_mutations`.
3. **Trust reconcile add-failure**: `add_failure_surfaces_typed_error_and_does_not_update_applied_view` — surface test through the peer-projection DSL, not deleted `comms_trust_reconcile`.
4. **Respawn overlay rotation**: wire M1+M2, respawn M1, assert the prior session overlay is cleared and the new session receives endpoints.
5. **Release-binding sweep**: release the binding of M1, assert M1 is removed from the peer-overlay projection.
6. **Shadow-mode parity** (or its successor): after actor restore, DSL-projected overlay equals the expected wired set.
7. **Mob member restore-failure breaks present member**: terminal-classification for a member whose restore failed.
8. **Agent run-completed hook-failure emits run-failed, not run-completed**: port from pre-wave-a `state.rs`.
9. **Turn-boundary denial blocks boundary side-effects**: port.
10. **Composition-driver runtime contract**: replacement for the 9 `composition_dispatch` tests — minimal surface covering name/descriptor agreement, duplicate rejection, effect→watcher routing, typed decision-failure error. Can live against the catalog-declared driver shape.
11. **Schedule host typed mapping**: `dispatch_from_admission_keeps_rejected_runtime_meaning_typed` + the two scheduled-completion-future mappings. Hoist to a shared surface test used by CLI/REST/RPC hosts.
12. **Skill stdio protocol**: handshake-cache-per-source-instance, capability-mismatch typed error, unknown-skill not-found, transport-timeout typed error.
13. **Skill filesystem**: recursive scan, quarantine diagnostics retention, collection-md fallback, invalid-ratio health transition.
14. **Skill resolver precedence**: defaults, mixed repos, explicit-roots-context-over-user, no-roots-skips-filesystem-defaults.
15. **Skill browse/load tool surface**: root listing, collection filter, search, empty collection, load-missing returns not-found — restore `meerkat-tools/tests/` coverage.
16. **Comms peer-request promotion**: params-only, body-only-promotes-to-params, both-prefers-params, neither-gives-empty-object.

Estimated rebuild: **~40 targeted tests** to restore parity of behaviors still present in the tree. Rebuilding 60+ skills tests lost in B-7 collapse is optional if the typed SkillKey/SkillRef foundation is deemed sufficient — but browse/load/filesystem/protocol are runtime-behavior tests that typed-identity cannot substitute for.
