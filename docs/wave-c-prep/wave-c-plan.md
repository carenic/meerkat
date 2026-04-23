# Wave (c) — Shell-Code Rebuild Plan

**Branch:** `dogma/wave-a-demolition`. Wave-c opens when wave-b leaves the foundation crates (`meerkat-machine-schema`, `meerkat-machine-kernels`, `meerkat-machine-codegen`, `meerkat-core`, `meerkat-contracts`) green. At plan authoring, wave-b is partially landed (B-1, B-2, B-5, B-7, B-8, B-9, B-10 in flight); downstream crates are broken.

Wave-c is a retyping pass on consumer code plus a short list of design decisions where the typed foundation collapsed a formerly-implicit seam. No shadow state is reintroduced. Callers that became obsolete because wave-a deleted their semantic owner are *deleted*, not retyped. Six late findings (roster/peer separation, DSL peer retype, composition-dispatcher producer+consumer wiring, OwnerProvided binding extension, SessionStore append-only hardening, persistence v0→v1 migration) fold into the task list rather than living as follow-ups.

This plan integrates seven prior sources, all in `docs/wave-c-prep/`: `wave-c-plan.md` (base draft, `907dd711a`), `persistence-migration.md` (`c970cadcd`), `test-coverage-audit.md` (`f51777501`), `dogma-blind-spots.md` (`5a39c15e1`), `realtime-substrate-audit.md` (`a92e3abf1`), `machine-boundary-stability.md` (`33a0e4d72`), `state-scope-audit.md` (`38ea190e3`).

---

## Section 1 — Integrated task list

Tasks are grouped by purpose. Dependencies reference wave-b foundations (V-prefix, B-prefix) and intra-wave-c tasks (C-prefix).

### Core retype spine

**C-1 · meerkat-core consumer retype.** Files: `agent.rs`, `agent/{state,runner,builder}.rs`, `interaction.rs`, `ops_lifecycle.rs`, `session_recovery.rs`, `session.rs`, `service/mod.rs`, `event.rs`, `lib.rs`, `hooks.rs`, `runtime_bootstrap.rs`, `config{,_runtime,_store}.rs`, `auth/token_store.rs`, `lifecycle/run_primitive.rs`. **Deliverable:** `meerkat-core` compiles; C-SKL (`SkillId` → `SkillKey`) purged; C-TRP retyped to `PeerId` + `TrustedPeerDescriptor` (routing subset); C-CSS dead re-export deleted; C-TM-V3 typed with `from_legacy_value` migration helper. Also adds `ProviderTag::Unknown { bag: StructuredProviderExtension }` on core — today it lives only on wire (see persistence-migration.md §3.1), and its absence would force silent drops on unknown provider knobs. **Deps:** V3, V4, V5, V6, V7 green. **Size:** medium (~16 files). **Risk:** core rebuild cascade — start of spine, blocks everything downstream.

**C-2 · meerkat-contracts cleanup.** Files: `lib.rs`, `emit.rs`, `rpc_catalog.rs`, `session_locator.rs`, `wire/{connection,mob,params,realtime,runtime,supervisor_bridge,mod}.rs`. **Deliverable:** retired re-exports purged; `wire/mob.rs:152` (`provider_params: Option<Value>`) retyped to `ProviderParamsOverride`; `emit.rs`/`rpc_catalog.rs` drop retired `session/{retire,reset,submission,submissions}` verbs; `SessionLocator.realm_id: Option<String>` (`session_locator.rs:8`) retyped to `Option<RealmId>`. **Deps:** V-foundations (B-9 landed). **Size:** small (~8 files). **Risk:** SDK wire shape changes — caught by wave-d regen, not wave-c scope.

**C-3 · meerkat-session persistence migration (v0 → v1).** Files: `persistent.rs`, `ephemeral.rs`, new `persistent/migrations.rs` submodule, new `tests/persistence_compat.rs`, fixture tree `tests/fixtures/pre_wave_b/`. **Deliverable:** per-entity schema version bytes land as designed in `persistence-migration.md` §2: bump `SESSION_VERSION = 2`; add `schema_version: u32` to `SessionMetadata` and `stored_input_state_version: u32` to `InputStateSerde`. Opportunistic upgrade-on-read, rewrite as v1 on next save. Fixture matrix from §5 (12 fixtures covering empty metadata, OpenAI/Anthropic/thinking provider params, slug valid/invalid, hot-swap identity mixed, input-state full/minimal/unknown-provider, runtime-snapshot drift). `rkat debug migrate-sessions` CLI as stretch. **Deps:** C-1, C-2. **Size:** medium. **Risk:** silent lossy migration on unknown provider knobs — catching assertion is the Anthropic `thinking` fixture (§5 #4), explicitly designed to be the one most likely to be silently dropped.

**C-4 · meerkat-tools + meerkat-skills retype.** Files: `meerkat-tools/src/dispatcher.rs`, `builtin/composite.rs`, `builtin/skills/{browse,load,resources,functions}.rs`, `meerkat-skills/src/{resolve,source/{filesystem,composite,embedded,protocol,git,http}}.rs`. **Deliverable:** C-TOOL-ERR fixed (4 `NotFound` → `AccessDenied` at dispatcher); builtin tools consume `SourceIdentityRegistry::canonical_skill_key`; `SkillRef::Legacy` deleted from sources. **Deps:** V4, V7. **Size:** small-medium (~12 files). **Risk:** skill resolver precedence regression (see C-T below) — tests rebuilt under C-T.

**C-5 · meerkat-comms retype.** Files: `runtime/comms_runtime.rs`, `trust.rs`, `router.rs`, `inbox.rs`. **Deliverable:** `TrustStore` keyed by `PeerId`; `Router::send(dest: PeerId)`; name→id ambiguity is a typed error. **Deps:** V5. **Size:** medium (~5 dense files). **Risk:** `PeerName`-keyed maps leaking outside the crate — catching assertion: `rg 'HashMap<PeerName|BTreeMap<PeerName' --type rust` returns zero hits outside display-only sites.

### Producer/consumer wiring (B-5 handoff — LATE FINDING C)

B-5 (`856500ceb`) delivered the typed `CompositionDispatcher` trait, `CatalogCompositionDispatcher` impl, and `CompositionBinding<E>::{Standalone, Wired(Arc<dyn ...>)}` witness. The grep canary at `meerkat-runtime/tests/composition_dispatch_is_the_path.rs::no_legacy_composition_helpers_in_routed_effect_call_sites` only checks for legacy helper absence; it does *not* verify the typed path is used. B-10 adds the RMAT semantic rule "every InputVariantId applied traverses dispatcher" — that rule cannot *fail on a real violation* until wave-c wires both sides.

**C-6p · Composition dispatcher producer wiring (mob side).** Files: `meerkat-mob/src/runtime/actor.rs`, `meerkat-mob/src/runtime/ops_adapter.rs`, `meerkat-mob/src/runtime/event_router.rs`. **Deliverable:** every routed effect emitted from the mob actor flows through `CompositionDispatcher::dispatch`, not through direct peer calls. String `module_path` driver declarations (C-DEAD-DRV, 3 sites) replaced with typed `CompositionDispatcherHandle`. **Deps:** C-1, V2 (B-5). **Size:** small. **Risk:** dispatcher becoming an "optional fast path" where the old direct call still exists next to the typed path. Catching assertion: add a property test that `ConsumerSurface::apply_input` is reachable from *only* `CompositionDispatcher::dispatch` (AST lint in xtask, extending the B-10 rule).

**C-6c · Composition dispatcher consumer wiring (meerkat-machine side).** Files: `meerkat-runtime/src/meerkat_machine/mod.rs`, `meerkat-runtime/src/meerkat_machine/dispatch_*.rs`, `meerkat-runtime/src/mob_adapter.rs`. **Deliverable:** `MeerkatMachine` implements `ConsumerSurface`; `dispatch_*.rs` accept inputs via the dispatcher seam, not via direct method calls. **Deps:** C-6p. **Size:** medium. **Risk:** duplicate DSL drift between mob composition declarations and the schema catalog — catching assertion: `meerkat-machine-schema/tests/composition_routes_match_producer_emissions.rs` cross-references both.

**C-6o · OwnerProvided binding extension (issue #342).** Files: `meerkat-machine-codegen/src/composition.rs`, `meerkat-runtime/src/composition_dispatch.rs` (re-added minimal), `meerkat-mob/src/runtime/actor.rs`. **Deliverable:** `CompositionBinding` gains a third variant `OwnerProvided(Arc<dyn ContextProvider<E>>)` for routes where `session_id` isn't in the producer effect body. Typed context struct, not `Value`. **Deps:** C-6p, C-6c. **Size:** small. **Risk:** scope creep — if the typed context struct grows open-ended, stop and defer. Catching assertion: `ContextProvider<E>` has exactly one method and no `Value` anywhere in its signature.

**C-6r · meerkat-runtime retype.** Files: `comms_bridge.rs`, `comms_drain.rs`, `meerkat_machine/dispatch_*.rs` (if not already under C-6c), `ops_lifecycle.rs`, `runtime_loop.rs`, `driver.rs`, `meerkat_machine/dsl.rs`. **Deliverable:** V3 single-site `RuntimeTurnMetadata::for_input`; V5 `PeerId` threaded; TrustedPeerSpec retyped; the DSL `PeerEndpoint { name: String, peer_id: String, address: String }` at `meerkat-runtime/src/meerkat_machine/dsl.rs:1721-1757` retyped to `PeerEndpoint { name: PeerName, peer_id: PeerId, address: PeerAddress }` so `From<TrustedPeerSpec> for PeerEndpoint` stops erasing typing at the catalog seam (LATE FINDING B). **Deps:** C-1, C-5, V2, V3, C-6c. **Size:** large (≥8 files; core semantic seams). **Risk:** choke point for the fan-out tier below; stay on consolidation branch.

### Mob + downstream consumers

**C-7 · meerkat-mob retype.** Files: `machines/mob_machine.rs`, `runtime/{actor,handle,tools,provisioner,provision_guard,supervisor_bridge,ops_adapter,actor_turn_executor,disposal,builder,edge_locks,event_router}.rs`, `roster.rs`, `event.rs`, `build.rs`, `profile.rs`, `tests.rs`, `tests/{contracts,phase1_red_ok}.rs`, `tests/member_{binding_orthogonality,session_bindings}.rs`. **Deliverable:** C-MEMBER-BIND effect collapse (`MemberSessionBinding{Set,Rotated,Released}` → single `MemberSessionBindingChanged { edge, from: Option<SessionId>, to: Option<SessionId> }` at `mob_machine.rs:1618-1700`); C-TRK-B-CMDS deletion; C-AGENT-RT-ID tightening (`MobMemberSnapshot.agent_runtime_id` / `.fence_token` drop from `pub` to `pub(crate)`; external consumers read `MobMemberView`); TrustedPeerSpec retyped; orphan tests rewritten/deleted. **Deps:** C-1, C-5, C-6r. **Size:** large (~20 files). **Risk:** `(None, None)` emit on `MemberSessionBindingChanged` is ambiguous; catching assertion: DSL-level validator rejects at every emit site.

**C-7r · Roster external-peer separation (LATE FINDING A).** Files: `meerkat-mob/src/roster.rs` (lines 208, 213 and the `RosterEntry.wired_to` field), `meerkat-mob/src/event.rs`, `meerkat-mob/src/runtime/actor.rs` (handler for `ExternalPeerWired`/`Unwired`). **Deliverable:** `RosterEntry.wired_to: BTreeSet<AgentIdentity>` no longer receives `AgentIdentity::from(PeerName)` coercions; a new field `external_peers: BTreeSet<PeerEndpoint>` (or similar typed container) owns external-comms peer membership. The `From<PeerName> for AgentIdentity` usage is deleted — external peers are *not* mob members and do not belong in the AgentIdentity-keyed roster. PR #340's typed translation (`RecomputeMobPeerOverlayDriver`) is preserved on the boundary; this task cleans up the one remaining polluted seam. **Deps:** C-7. **Size:** small (2-3 files). **Risk:** downstream callers iterating `wired_to` assume "all peers, typed or coerced" — catching assertion: retype returns `BTreeSet<AgentIdentity>` (mob-internal only) and a parallel iteration method for external peers.

**C-8 · meerkat-mob-mcp.** Files: `lib.rs`, `public_mcp.rs`, `agent_tools.rs`. **Deliverable:** consumes `MobMemberView`; TrustedPeerSpec retyped; `realtime_status_from_mob_status` dead-code projection deleted (C-R2). **Deps:** C-7, V5. **Size:** small.

### Realtime substrate (from `realtime-substrate-audit.md`)

Ten subtasks from the audit; wiring them into the main numbering rather than a parallel series.

**C-9 · meerkat-rpc + realtime WS substrate (absorbs C-R1..C-R10 except C-R10).** Files: `router.rs`, `session_runtime.rs`, `realtime_ws.rs`, `main.rs`, `handlers/{session,mob,runtime,realtime,auth}.rs`, plus `meerkat/src/realtime.rs` (for C-R1). **Deliverable:**

- Retired RPC verbs deleted; `runtime.rs` consumes typed `SessionAcceptInputParams`/`WireInputState`; realtime WS reads `MemberSessionBindingChanged`; `.realm_id`/`.binding_id` retyped; SkillId purged.
- **C-R1**: delete panicking `RealtimeChannel::mob_member` deprecation stub.
- **C-R3**: `projection_to_channel_status` / `realtime_status_from_runtime` collapsed into a single canonical `impl From<RealtimeAttachmentStatus> for RealtimeChannelStatus` in `meerkat-runtime/src/meerkat_machine_types.rs` (or a `meerkat-contracts` helper); both RPC and MCP delegate.
- **C-R4**: typed `attempt_count` / `next_retry_at` in `RealtimeAttachmentStatus` — reconnect overlay flows into DSL input; status queries see real retry state, not hard-coded `0/1`.
- **C-R5**: typed `RealtimeActionResult` for interrupt/close on `RealtimeProductSessionCommand`; kill the `preemptive_interrupt_can_be_ignored` message-string match.
- **C-R6**: expand `RealtimeErrorCode`; rewrite `realtime_client_error_frame`. Auth/ContentFiltered/ModelNotFound each get a typed code.
- **C-R7**: `session.close()` timeout in product-session actor at `realtime_ws.rs:2885`.
- **C-R8**: on broadcast `Lagged` at `realtime_ws.rs:1965`, re-resolve the binding map via `mob_handle.current_realtime_binding(identity)` (closes the "socket stays pinned to retired session_id" gap in §3.3 of the audit).
- **C-R9**: invariant — every `handle.abort()` on a realtime task is paired with `detach_live`. Grep audit + optional AST lint.

**Deps:** C-1, C-2, C-6r, C-7. **Size:** large.

**C-R10 — Realtime schema regen** is wave-d; listed in §7 Out of scope.

### Surface crates

**C-10 · meerkat-rest.** Files: `lib.rs`, `schedule_host.rs`, `auth_endpoints.rs`. **Deliverable:** C-REST-READMUT fix (`ensure_runtime_session_registered` deleted from 5 read-only sites at `lib.rs:1280,1915,1986,2663,3495`); four `json!({…})` sites become typed `Json(wire)`; string-form `ConnectionRef` rejected at 400; `.realm_id`/`.binding_id` retyped. **Deps:** C-2, C-6r. **Size:** medium.

**C-11 · meerkat-mcp-server.** Files: `lib.rs`, `main.rs`, `runtime_ingress.rs`, `schedule_host.rs`. **Deliverable:** `.realm_id` retyped; string-form `ConnectionRef` ingress rejected; SkillId purged. **Deps:** C-2, C-1. **Size:** small.

**C-12 · meerkat-cli.** Files: `main.rs`, `mcp.rs`, new `cli_parse.rs`. **Deliverable:** sole `parse_connection_ref_user_input(&str) -> Result<ConnectionRef, CliError>` lives in `cli_parse.rs`. Ad-hoc `split_once(':')` at `main.rs:3798` and `mcp.rs:193` deleted. `.realm_id`/`.binding_id` retyped (~122 sites in `main.rs`). C-CLI-DISCRIM fixed. SkillId purged. **Deps:** C-2. **Size:** medium (`main.rs` is dense — the 278-site hot spot). **Risk:** ad-hoc parser regrowth; catching assertion: `meerkat-cli/tests/connection_ref_single_parser.rs` asserts exactly one `split_once(':')` site in `meerkat-cli/src/**`.

**C-13 · meerkat facade.** Files: `factory.rs`, `service_factory.rs`, `surface.rs`, `surface/{runtime_backed,runtime_schedule_host,schedule_host}.rs`, `prompt_assembly.rs`, `realtime.rs`, `lib.rs`. **Deliverable:** V3 single-site `RuntimeTurnMetadata`; `None =>` fallback arms at `factory.rs:1241,1684,1819,2306,2884,2900` become typed "ambient credential selection refused" errors (dogma #8 stance); SkillId purged. **Deps:** C-1, C-2, C-6r. **Size:** medium-large.

**C-14 · provider crates.** Files: `meerkat-anthropic/src/runtime/mod.rs`, `meerkat-openai/src/runtime/mod.rs`, `meerkat-openai/src/realtime_attachment.rs` (C-R9), `meerkat-gemini/src/runtime/mod.rs`, `meerkat-llm-core/src/{adapter,provider_runtime/registry,types}.rs`, `meerkat-auth-core/src/{auth_store/{file,refresh},resolver}.rs`. **Deliverable:** `.realm_id`/`.binding_id` retyped; typed `ProviderParamsOverride` at the provider boundary. **Deps:** C-2. **Size:** small-medium.

**C-16 · web / WASM.** Files: `meerkat-web-runtime/src/lib.rs`, `sdks/web/src/*.ts` (Rust-side bindings only). **Deliverable:** retyped V5/V6; `allow_web_build` string-form bypass deleted. **Deps:** C-13. **Size:** small.

### Hardening

**C-H1 · SessionStore append-only hardening (F1/F7 from state-scope-audit.md).** Files: `meerkat-core/src/session_store.rs`, `meerkat-core/src/session.rs`, `meerkat-session/src/**`. **Deliverable:** Option B from §3 of the state-scope audit — formalize the doc ("snapshot = projection"), route all persistence through `EventStore`, make `SessionStore` a test-only convenience via an `AppendOnlySessionStore` alias trait that guards `save()` as `append_or_extend_only`. The `Session.messages: Arc<Vec<Message>>` gets an `AppendOnlyMessages` newtype exposing only `.push()`/`.extend()`; `fork_at` becomes an explicit fork on a new `SessionId`. **Deps:** C-1, C-3. **Size:** medium. **Risk:** surface-level callers relying on `save()` replace semantics (unlikely; audit found none).

**C-H2 · Collapse `comms_drain_slots` into `RuntimeSessionEntry` (F5).** Files: `meerkat-runtime/src/meerkat_machine/mod.rs` (around line 417). **Deliverable:** `CommsDrainSlot` moves into `RuntimeSessionEntry`; single HashMap insertion for "session exists"; eliminates the two-maps-desync bug class. **Deps:** C-6r. **Size:** small.

**C-H3 · Inbox TOCTOU fix (flagged in `dogma-blind-spots.md` §7 commentary).** Files: `meerkat-comms/src/inbox.rs` (the `483 vs 505` window). **Deliverable:** tightened single-pass check+apply. **Deps:** C-5. **Size:** small. **Risk:** file a follow-up GitHub issue citing the specific line range so the fix is externally reviewable.

### Tests (the "blocker" list)

**C-T · Test rebuild sweep (covers the 16-item minimum list in `test-coverage-audit.md` §6).** Files: `meerkat-mob/tests/`, `meerkat-runtime/tests/`, `meerkat-comms/tests/`, `meerkat-skills/tests/` (new), `meerkat-tools/tests/` (new), `meerkat-core/tests/`. **Deliverable:** rebuild the six blocker-list tests (trust-reconcile concurrency + add-failure typed error; respawn overlay rotation; shadow-mode parity; release-binding sweep; mob restore-failure classification; turn-boundary-denial) plus the 10 non-blocker "lost and concerning" tests (skill stdio protocol, filesystem source, resolver precedence, browse/load tools, schedule-host typed mapping, agent run-completed hook-failure, comms peer-request promotion). **Deps:** all prior C-* tasks. **Size:** medium (~40 targeted tests). **Risk:** writing DSL-level tests that reference the old authority enums — catching assertion: each blocker-list test is either a `meerkat-machine-schema` DSL reducer test or an actor-path end-to-end test; no hand-written authority reducers.

**C-15 · test-harness sweep.** Files: `meerkat-*/tests/**`, `tests/integration/**`, `examples/**`. **Deliverable:** obsolete tests deleted (wire/unwire, Bind/Rotate/Release); live tests retyped; examples compile; no new `#[allow(dead_code)]`. **Deps:** all prior. **Size:** medium.

### Deferred to post-0.6 (NOT wave-c)

- Issue **#341** (fault-explicit state machines) — the 11 dogma blind-spot classes; `ephemeral.rs:2548-2572` cancellation races are the highest-leverage seam but wave-c does not close them.
- Issue **#343** (Signal-route typed dispatcher — parallel `CompositionSignalDispatcher`) — wave-c keeps the single-dispatcher shape.

---

## Section 2 — Dependency graph

```
    [V1..V8 wave-b foundations green]
                 |
        +--------+--------+
        |                 |
      C-1 core         C-2 contracts
        |   \ \__________   \
        |    \           \   \
        v     v           v   v
      C-5 comms       C-4 tools/skills      C-14 providers (early)
        |
        v
      C-3 session (legacy v0 fixtures)
        |
        v
      C-6p composition-dispatcher producer (mob)
        |
        v
      C-6c composition-dispatcher consumer (meerkat)
        |   \
        |    +--> C-6o OwnerProvided binding
        v
      C-6r meerkat-runtime retype (CHOKE POINT)
        |
        +--> C-H2 drain-slot collapse
        v
      C-7 meerkat-mob
        |   \
        |    +--> C-7r roster external-peer separation
        +---------> C-8 mob-mcp
        v
    [fan-out: C-9 rpc+realtime | C-10 rest | C-11 mcp-server | C-12 cli | C-13 facade]
                                                                              |
                                                                              v
                                                                        C-16 web
                                                                              |
                +------- C-H1 SessionStore append-only ----------+            |
                |        C-H3 inbox TOCTOU                       |            |
                v                                                v            v
                                                              C-T blocker-list tests
                                                              C-15 test sweep
```

**Serial spine:** `foundations → C-1 → C-6p → C-6c → C-6r → C-7 → C-9`. Everything else parallelizes off the spine.

**Critical path for enforcement.** C-6p and C-6c must land before B-10's semantic RMAT rule ("every InputVariantId applied traverses dispatcher") can fail a real violation. If B-10 lands before the wiring, the rule is a no-op. Gate: after C-6c, run the B-10 AST rule and confirm it flags a synthetic violation (remove a `dispatch` call from a test fixture, confirm the lint reports, restore).

**Fan-out after C-7:** C-8, C-9, C-10, C-11, C-12, C-13, C-14, C-16 partition by crate.

---

## Section 3 — Parallelism / worktree strategy

Base branch stays `dogma/wave-a-demolition`.

**Spine tasks** (C-1, C-6p/c/r, C-7, C-9) run on the consolidation branch directly. Cross-crate seams; only one author at a time.

**Fan-out tasks** (C-8, C-10, C-11, C-12, C-13, C-14, C-16, C-T) run in per-task worktrees that rebase against the spine's tip when each serial task lands. Worktree isolation prevents concurrent edits to `Cargo.lock`, build caches, and derived `.rkat/` files.

**Commit hygiene.** `git commit -o <paths>` continues. `--no-verify` gets phased out as the tree regains compilation — not earlier; a half-compiling tree fails `cargo fmt --check` and blocks progress. Flip hooks back on at the merge-to-main step. One crate per commit in the fan-out tier; spine commits split per cluster (e.g., "C-1: SkillId purge", "C-1: TrustedPeerSpec retype").

**Named conflict risks.**

1. `Cargo.lock` — only spine touches `Cargo.toml`; fan-out is code-only.
2. `meerkat-contracts/src/lib.rs` re-exports — C-2 owns the final list; downstream crates only consume.
3. `meerkat-cli/src/main.rs` — 278 field-access sites in one file. Keep C-12 single-agent; no line-range splitting.
4. `meerkat-mob/src/runtime/tests.rs` + `meerkat-mob/tests/member_session_bindings.rs` — C-7 deletes orphans inline with the effect collapse; C-15 only retypes survivors.

---

## Section 4 — Phases within wave-c

**c.1 — Enabling wiring.** C-1 core + C-2 contracts + C-6p + C-6c. Unlocks every downstream consumer and makes B-10 enforceable. Exit gate: B-10 RMAT rule flags a synthetic violation.

**c.2 — Consumer rebuilds (parallel).** C-3 session (parallel with C-4 tools/skills, C-5 comms, C-14 providers) once C-1+C-2 done. Exit gate: each of the 5 crates `cargo check` clean in isolation.

**c.3 — Runtime + mob.** C-6r choke point on spine; then C-6o (OwnerProvided), C-H2 (drain-slot collapse), C-7 mob retype, C-7r (roster external-peer separation), C-8 (mob-mcp). Exit gate: `cargo check -p meerkat-mob -p meerkat-mob-mcp` clean; composition dispatcher is *the* path.

**c.4 — Surface layer (parallel fan-out).** C-9 realtime+rpc, C-10 rest, C-11 mcp-server, C-12 cli, C-13 facade, C-16 web. Exit gate: `cargo check --workspace` clean.

**c.5 — Hardening + test rebuild.** C-H1 SessionStore append-only, C-H3 inbox TOCTOU, C-T blocker-list tests, C-15 test sweep. Exit gate: §6 completion criteria satisfied.

---

## Section 5 — Risk register

Integrated from all seven sources.

1. **`SessionLocator.realm_id: String` vs `ConnectionRef.realm: RealmId` diverge silently.** C-2 retypes `SessionLocator.realm_id` to `Option<RealmId>`. Catching assertion: `meerkat-contracts/tests/locator_realm_typed.rs` round-trips via `RealmId::parse`.

2. **V3 legacy-row deserialize silently drops unknown provider knobs.** The Anthropic `thinking: {type:"enabled", budget_tokens:32000}` case is the one most at risk (persistence-migration.md §5 fixture #4). Catching assertion: `meerkat-session/tests/persistence_compat.rs` loads the thinking fixture and asserts the typed bag round-trips losslessly, not that it silently drops. Depends on C-1 adding `ProviderTag::Unknown { bag }` on core.

3. **C-MEMBER-BIND collapse loses the "no-from + no-to" invariant.** `MemberSessionBindingChanged { from: None, to: None }` is ambiguous. Catching assertion: `meerkat-mob/tests/member_session_bindings.rs` rejects `(None, None)` at the DSL-level validator.

4. **C-WIRE-RETIRED leaves dangling method names in the RPC catalog.** Catching assertion: `verify-schema-freshness` extended to assert `rpc_catalog` contains no `session/{retire,reset,submission,submissions}` entries.

5. **C-12 ad-hoc CLI parser regrowth.** Catching assertion: `meerkat-cli/tests/connection_ref_single_parser.rs` asserts exactly one `split_once(':')` site inside `meerkat-cli/src/**`.

6. **C-AGENT-RT-ID `pub(crate)` tightening breaks an out-of-tree consumer.** Catching assertion: `rmat-audit` run at end of C-7 + `meerkat-mob/tests/public_api.rs` instantiates `MobMemberSnapshot` externally and confirms the field is unaddressable.

7. **C-REST-READMUT deletion silently loses a lazy registration side-effect.** Trace `SessionRuntime::ensure_registered` call sites; if zero non-READ callers survive, delete the helper entirely.

8. **V5 `PeerName`-keyed maps leak outside `meerkat-comms`.** Catching assertion: `rg 'HashMap<PeerName|BTreeMap<PeerName' --type rust` returns zero hits outside display-only comms sites.

9. **Composition dispatcher becomes an "optional fast path"** (LATE FINDING C risk). If C-6p wires producers but direct calls survive next to dispatcher calls, B-10 passes without enforcing. Catching assertion: AST lint in xtask that forbids direct `ConsumerSurface::apply_input` callers outside `CompositionDispatcher::dispatch`.

10. **Duplicate DSL drift (mob composition emit vs schema catalog).** Catching assertion: `meerkat-machine-schema/tests/composition_routes_match_producer_emissions.rs` cross-references both sides; fails if a mob-actor `dispatch(input)` has no matching declared route in the catalog.

11. **PeerName/AgentIdentity roster conflation surviving wave-c** (LATE FINDING A). If C-7r is deferred, `RosterEntry.wired_to` keeps the coerced `AgentIdentity::from(PeerName)` entries; downstream overlay recompute treats external peers as mob members. Catching assertion: compile-time — delete `From<PeerName> for AgentIdentity` entirely; use-sites force the typed separation.

12. **Test coverage attrition.** 97 tests lost across wave-a; 6 are blockers (trust-reconcile concurrency, respawn overlay rotation, shadow-mode parity, release-binding sweep, mob restore-failure classification, turn-boundary-denial). Catching assertion: C-T rebuild list is reviewed tick-by-tick against `test-coverage-audit.md` §6; orphaned entries force a conscious replacement decision (either the new DSL test is sufficient or a new actor-path test is written).

13. **Persistence migration failure modes.** Worst case — a v0 `connection_ref` slug fails regex even after slugification. Mitigation: `migrate(Value) -> Result<Session, SessionMigrationError>` with `SessionMigrationError::Partial` preserving the legacy payload under `legacy_connection_ref`. No outright session loss.

14. **Realtime rotation pinning on broadcast `Lagged` (audit §3.3).** C-R8 closes this by re-resolving the binding map on `Lagged`, not just polling attachment status. Catching assertion: realtime WS test that injects synthetic `Lagged` followed by out-of-band binding change and asserts the socket converges to the canonical binding.

15. **DSL stringly-typed PeerEndpoint residue** (LATE FINDING B). If C-6r doesn't retype `meerkat-runtime/src/meerkat_machine/dsl.rs:1721-1757`, typing evaporates at the catalog seam and B-8's wave-b work regresses. Catching assertion: grep `name: String` inside `PeerEndpoint`-adjacent files returns zero hits.

---

## Section 6 — Completion criteria

Wave (c) is done when all hold:

- `./scripts/repo-cargo check --workspace --all-targets` exits zero.
- `./scripts/repo-cargo nextest run --workspace` passes with zero failures.
- `./scripts/repo-cargo clippy --workspace -- -D warnings` passes.
- `git diff origin/main..HEAD -- '*.rs' | grep '^+.*#\[allow(dead_code)\]'` returns zero (no new allow).
- No new `serde_json::Value` fields in semantic seams. The only surviving `Value`s in `meerkat-contracts/src/wire/` and `meerkat-core/src/lifecycle/run_primitive.rs` are the allow-listed pass-through fields (structured_output, params_schema, MCP server_config, tool-call args as `Box<RawValue>`).
- Every tombstone cluster in the base draft §1 returns zero `rg` hits against its deleted-symbol fingerprint, except the `pub(crate)` survivors in C-AGENT-RT-ID.
- B-10 semantic RMAT rule ("every InputVariantId applied traverses dispatcher") is active and green — dispatcher is THE path. Verified via synthetic-violation injection.
- Blocker-list tests from test-coverage-audit.md §6 (items 1-9) are restored or consciously replaced with DSL+actor-path equivalents.
- Persistence migration validated via the 12-fixture matrix in persistence-migration.md §5.
- `SessionStore` is type-level append-only (F1 from state-scope-audit.md): no `save()` call can shorten a session's message history under a stable `SessionId`.
- `RosterEntry.wired_to` holds only real mob members; external peers live in a typed parallel container.
- `PeerEndpoint` in DSL carries `PeerId`/`PeerAddress`/`PeerName` — no `String` fields.
- `verify-schema-freshness` passes; retired verbs removed from catalog.
- `make ci` exits zero locally. GitHub `gate` green on the wave-c merge PR.
- Neither `cargo fmt --check` nor the pre-commit hook chain is skipped on the final merge commit — wave-c ends the `--no-verify` tolerance.

---

## Section 7 — Out of scope (wave-d's problems)

- **SDK codegen regeneration** (`make regen-schemas`, Python SDK `sdks/python/meerkat/generated/`, TypeScript SDK `sdks/typescript/src/generated/`, web SDK `sdks/web/`).
- **C-R10 schema regen** + version parity — triggered by C-R4, C-R5, C-R6 wire changes, but the regen itself is wave-d.
- **`artifacts/schemas/` JSON regeneration** from `meerkat-contracts`.
- **Doc regeneration** from typed catalog (`docs/reference/capability-matrix.mdx` and similar).
- **Release infrastructure updates** (`Cargo.toml` workspace version bump, `make verify-version-parity` post-retype).
- **Issue #341** (fault-explicit state machines — the 11 dogma blind-spot classes). Top-5 seeds from `dogma-blind-spots.md` §7 (`ephemeral.rs:2548-2572`, `actor.rs:3320`+`actor_turn_executor.rs:269`, Anthropic timing profile, unbounded channel overflow policy, `require_peer_auth` type-state) are real but post-0.6.
- **Issue #343** (Signal-route typed dispatcher). Wave-c keeps one dispatcher shape.
- **Auth→Meerkat machine merge.** Per `machine-boundary-stability.md` §6: protocolize the seam today (under C-6r/C-7), leave the fold itself for a later release. Leading indicator to watch: auth-state drift across the Auth/Meerkat boundary.
