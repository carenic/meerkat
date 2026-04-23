# Wave (c) — Shell-Code Rebuild Plan

**Branch:** `dogma/wave-a-demolition`. Wave-c starts when wave-b leaves the foundation crates (`meerkat-machine-schema`, `meerkat-machine-kernels`, `meerkat-machine-codegen`, `meerkat-core`, `meerkat-contracts`) green. At plan authoring, wave-b is partially complete — `meerkat-machine-schema` and `meerkat-core` still fail to build (B-1/B-2 handoff gaps). Downstream crates are entirely broken.

Wave-c is a retyping pass on consumer code plus a short list of design decisions where the typed foundation has collapsed a formerly-implicit seam. No shadow state is reintroduced. Callers that became obsolete because wave-a deleted their semantic owner are *deleted*, not retyped.

---

## Section 1 — Tombstone inventory

`rg` counts over branch head. "Sites" = line hits; "files" = unique files.

| Cluster | Sites | Files | Owning wave-b foundation |
|---|---:|---:|---|
| C-SKL — `SkillId` legacy string-id | 58 | 20 | V4 (`SkillKey` canonical) |
| C-CNX-FIELDS — `.realm_id` / `.binding_id` on `ConnectionRef` | 278 | 47 | V6 |
| C-CNX-PARSE — `ConnectionRef::parse`, `split_once(':')` on conn strings | ~12 | 4 | V6 (CLI boundary parser sole survivor) |
| C-TRP — `TrustedPeerSpec` unresolved | 204 | 30 | V5 (`PeerId` + `TrustedPeerDescriptor`) |
| C-CSS — `CredentialStorageSpec` unresolved re-export | 2 | 1 (`meerkat-core/src/lib.rs:255`) | V6 (deleted) |
| C-TM-V3 — `provider_params: Option<Value>`, `additional_instructions`, `render_metadata` reads | ~60 | 10 | V3 |
| C-TOOL-ERR — dispatcher returns `NotFound` for policy denial | 4 | 2 | V7 |
| C-WIRE-STATE — `json!({...})` hand-built runtime responses | ~12 | 1 (`meerkat-rest/src/lib.rs`) | V8 |
| C-WIRE-RETIRED — dead imports of retired `Runtime{Retire,Reset,InputState,InputList}Params` | — | 1 (`meerkat-rpc/src/handlers/runtime.rs`) | V8 |
| C-MEMBER-BIND — `MemberSessionBinding{Set,Rotated,Released}` effects | ~15 | 3 | B-5 (collapsed only on one side) |
| C-TRK-B-CMDS — callers of deleted `MobMachineCommand::{Wire,Unwire,Bind,Rotate,Release}Member*` | ~8 | 2 | delete |
| C-DEAD-DRV — string `module_path` pointing at deleted composition driver | 3 | 2 | B-5 |
| C-AGENT-RT-ID — public `MobMemberSnapshot.agent_runtime_id`/`fence_token` | ~50 | 20 | design decision |
| C-REST-READMUT — `ensure_runtime_session_registered` on GET paths | 5 | 1 (`meerkat-rest/src/lib.rs:1280,1915,1986,2663,3495`) | delete |
| C-CLI-DISCRIM — CLI emits `mob_member_target` | 2 | 1 (`meerkat-cli/src/main.rs:750,760`) | V8 |
| C-EXT-WIRE-META — `wire/mob.rs:152 provider_params: Option<Value>` | 1 | 1 | V3 |

Total: ≈760 tombstone sites across ≈80 files. The V1 kernel-identity cascade inside `meerkat-machine-kernels/src/generated/**` falls out automatically under B-4 codegen re-emission and is **not** wave-c work.

---

## Section 2 — Classification

### Mechanical retype

- **C-SKL** — `SkillId` → `SkillKey` everywhere; delete the `SkillId(format!(…))` lowering at `meerkat-core/src/agent/runner.rs:990`.
- **C-CNX-FIELDS** — `.realm_id`/`.binding_id` → `.realm.as_str()` / `.binding.as_str()` (or owning clone). Only `ConnectionRef`-typed structs are touched; `SessionLocator.realm_id: Option<String>` at `meerkat-contracts/src/session_locator.rs:8` stays as-is unless wave-b retypes it separately.
- **C-CSS** — delete the dead re-export line.
- **C-TOOL-ERR** — swap four `NotFound` → `AccessDenied` calls; V7 wire mapping handles downstream propagation.
- **C-DEAD-DRV** — replace string `module_path` driver declarations with typed `CompositionDispatcherHandle` refs.
- **C-CLI-DISCRIM** — one-line rename.
- **C-EXT-WIRE-META** — project typed `ProviderParamsOverride` (V3) into `wire/mob.rs:152`.

### Design decision

- **C-TRP** — `TrustedPeerSpec` mixed routing (`peer_name`) and verification (keys/certs). V5 splits into `PeerId` + `TrustedPeerDescriptor`. For each of the 30 files the question is: *routing, verification, or both?* Decision assigned per-file in §3.
- **C-TM-V3** — `meerkat-core/src/session_recovery.rs:21` (`provider_params: Option<Value>`) must accept pre-wave-c persisted rows. Introduce a one-shot `ProviderParamsOverride::from_legacy_value` on read; the write path is typed. No "legacy mode" enum variant. Unknown knobs fail typed rather than silently drop.
- **C-WIRE-STATE** — the four `meerkat-rest/src/lib.rs` `json!({...})` sites become typed `From<DomainT> for WireT` + `Json(wire)`. If a site carried conditional reshape, it becomes a typed impl in `meerkat-contracts`. (Inspection confirms all four are field-rename + wrapper, no conditional logic.)
- **C-AGENT-RT-ID** — `MobMemberSnapshot.agent_runtime_id` / `.fence_token` drop from `pub` to `pub(crate)` inside `meerkat-mob/src/runtime/handle.rs:226-365`. External consumers read a new `MobMemberView` projection that omits them.
- **C-MEMBER-BIND** — runtime DSL at `meerkat-mob/src/machines/mob_machine.rs:1618-1700` collapses the three effects into one `MemberSessionBindingChanged { edge, from: Option<SessionId>, to: Option<SessionId> }`. Realtime WS (`meerkat-rpc/src/realtime_ws.rs:2158`) derives "set/rotated/released" from the `(from, to)` pair. This matches `meerkat-machine-schema/src/catalog/compositions.rs:320` which already watches `Changed`.

### Delete

- **C-WIRE-RETIRED** — delete router entries + handler bodies in `meerkat-rpc/src/handlers/runtime.rs` for retired verbs. Python SDK cleanup is wave-d.
- **C-TRK-B-CMDS** — delete `WireMembersArgs` + parsers at `meerkat-mob/src/runtime/tools.rs:589,818,832`. These tool-facing args no longer have a command.
- **C-REST-READMUT** — delete `ensure_runtime_session_registered` call from read-only handlers (`:3495`). Audit whether any non-READ caller survives; if zero, delete the helper entirely.
- Orphan tests asserting deleted `wire()`/`unwire()` (`meerkat-mob/src/runtime/tests.rs:9483,10298,11577`) and the deleted binding-effect triple (`meerkat-mob/tests/member_session_bindings.rs:88-285`) — rewrite to cover `MemberSessionBindingChanged` or delete as obsolete.

---

## Section 3 — Per-crate task briefs

Deps reference wave-b foundations (V-prefix) and intra-wave-c tasks (C-prefix).

### C-1 · meerkat-core consumer retype
- **Files:** `agent.rs`, `agent/{state,runner,builder}.rs`, `interaction.rs`, `ops_lifecycle.rs`, `session_recovery.rs`, `session.rs`, `service/mod.rs`, `event.rs`, `lib.rs`, `hooks.rs`, `runtime_bootstrap.rs`, `config{,_runtime,_store}.rs`, `auth/token_store.rs`.
- **Deliverable:** `meerkat-core` compiles. C-SKL purged. C-TRP retyped (routing subset). C-CSS dead re-export removed. C-TM-V3 typed + legacy-`Value` migration helper.
- **Deps:** V3, V4, V5, V6, V7 green.
- **Size:** medium (~15 files).

### C-2 · meerkat-contracts cleanup
- **Files:** `lib.rs`, `emit.rs`, `rpc_catalog.rs`, `session_locator.rs`, `wire/{connection,mob,params,realtime,runtime,supervisor_bridge,mod}.rs`.
- **Deliverable:** retired re-exports purged. `wire/mob.rs:152` → typed. `emit.rs`/`rpc_catalog.rs` drop retired verbs.
- **Deps:** V-foundations (B-9 already landed).
- **Size:** small (~8 files).

### C-3 · meerkat-session migration
- **Files:** `persistent.rs`, `ephemeral.rs`.
- **Deliverable:** typed round-trip. `from_legacy_value` helper invoked on read-only path. Fixture `test-fixtures/session/pre_wave_c/*.json`. `persistent_round_trip_legacy.rs` test.
- **Deps:** C-1, C-2.
- **Size:** small.

### C-4 · meerkat-tools + skills
- **Files:** `meerkat-tools/src/dispatcher.rs`, `builtin/composite.rs`, `builtin/skills/{browse,load,resources,functions}.rs`, `meerkat-skills/src/{resolve,source/{filesystem,composite,embedded,protocol}}.rs`.
- **Deliverable:** C-TOOL-ERR fixed; builtin tools consume `SourceIdentityRegistry::canonical_skill_key`; `SkillRef::Legacy` deleted from sources.
- **Deps:** V4, V7.
- **Size:** small-medium (~10 files).

### C-5 · meerkat-comms
- **Files:** `runtime/comms_runtime.rs`, `trust.rs`, `router.rs`, `inbox.rs`.
- **Deliverable:** TrustStore keyed by `PeerId`. `Router::send(dest: PeerId)`. Name→id ambiguity is a typed error.
- **Deps:** V5.
- **Size:** medium (~5 dense files).

### C-6 · meerkat-runtime
- **Files:** `comms_bridge.rs`, `comms_drain.rs`, `mob_adapter.rs`, `meerkat_machine/dispatch_*.rs`, `ops_lifecycle.rs`, `runtime_loop.rs`, `driver.rs`.
- **Deliverable:** V3 single-site `RuntimeTurnMetadata::for_input`; V5 `PeerId` threaded; V2 `CompositionDispatcher` consumed as *the* routed-effect path in all `dispatch_*.rs`; TrustedPeerSpec retyped.
- **Deps:** C-1, C-5, V2 (B-5), V3.
- **Size:** large (≥8 files; core semantic seams). Choke point.

### C-7 · meerkat-mob
- **Files:** `machines/mob_machine.rs`, `runtime/{actor,handle,tools,provisioner,provision_guard,supervisor_bridge,ops_adapter,actor_turn_executor,disposal,builder,edge_locks,event_router}.rs`, `roster.rs`, `event.rs`, `build.rs`, `profile.rs`, `tests.rs`, `tests/{contracts,phase1_red_ok}.rs`, `tests/member_{binding_orthogonality,session_bindings}.rs`.
- **Deliverable:** C-MEMBER-BIND effect collapse; C-TRK-B-CMDS deletion; C-AGENT-RT-ID `pub(crate)` tightening + `MobMemberView` projection; TrustedPeerSpec retyped throughout; orphan tests rewritten/deleted.
- **Deps:** C-1, C-5, C-6.
- **Size:** large (~20 files).

### C-8 · meerkat-mob-mcp
- **Files:** `lib.rs`, `public_mcp.rs`, `agent_tools.rs`.
- **Deliverable:** consumes `MobMemberView`; TrustedPeerSpec retyped.
- **Deps:** C-7, V5.
- **Size:** small.

### C-9 · meerkat-rpc
- **Files:** `router.rs`, `session_runtime.rs`, `realtime_ws.rs`, `main.rs`, `handlers/{session,mob,runtime,auth}.rs`.
- **Deliverable:** retired verbs deleted; `runtime.rs` consumes typed `SessionAcceptInputParams`/`WireInputState`; realtime WS reads `MemberSessionBindingChanged`; `.realm_id`/`.binding_id` retyped; SkillId purged.
- **Deps:** C-1, C-2, C-6, C-7.
- **Size:** large.

### C-10 · meerkat-rest
- **Files:** `lib.rs`, `schedule_host.rs`, `auth_endpoints.rs`.
- **Deliverable:** C-REST-READMUT fix; four `json!({…})` sites become typed `Json(wire)`; string-form `ConnectionRef` rejected at 400; `.realm_id`/`.binding_id` retyped.
- **Deps:** C-2, C-6.
- **Size:** medium (`lib.rs` is dense).

### C-11 · meerkat-mcp-server
- **Files:** `lib.rs`, `main.rs`, `runtime_ingress.rs`, `schedule_host.rs`.
- **Deliverable:** `.realm_id` retyped; string-form `ConnectionRef` ingress rejected; SkillId purged.
- **Deps:** C-2, C-1.
- **Size:** small.

### C-12 · meerkat-cli
- **Files:** `main.rs`, `mcp.rs`, new `cli_parse.rs`.
- **Deliverable:** sole `parse_connection_ref_user_input(&str) -> Result<ConnectionRef, CliError>` lives in `cli_parse.rs`. Ad-hoc `split_once(':')` at `main.rs:3798` and `mcp.rs:193` deleted. `.realm_id`/`.binding_id` retyped (~122 sites in `main.rs`). C-CLI-DISCRIM fixed. SkillId purged.
- **Deps:** C-2.
- **Size:** medium (`main.rs` is dense — the 278-site hot spot).

### C-13 · meerkat facade
- **Files:** `factory.rs`, `service_factory.rs`, `surface.rs`, `surface/{runtime_backed,runtime_schedule_host,schedule_host}.rs`, `prompt_assembly.rs`, `lib.rs`.
- **Deliverable:** V3 single-site `RuntimeTurnMetadata`; `None =>` fallback arms at `factory.rs:1241,1684,1819,2306,2884,2900` become typed `ambient credential selection refused` errors (dogma #8 stance); SkillId purged.
- **Deps:** C-1, C-2, C-6.
- **Size:** medium-large.

### C-14 · provider crates
- **Files:** `meerkat-anthropic/src/runtime/mod.rs`, `meerkat-openai/src/runtime/mod.rs`, `meerkat-gemini/src/runtime/mod.rs`, `meerkat-llm-core/src/{adapter,provider_runtime/registry,types}.rs`, `meerkat-auth-core/src/{auth_store/{file,refresh},resolver}.rs`.
- **Deliverable:** `.realm_id`/`.binding_id` retyped; typed `ProviderParamsOverride` at the provider boundary.
- **Deps:** C-2.
- **Size:** small-medium.

### C-15 · test-harness sweep
- **Files:** `meerkat-*/tests/**`, `tests/integration/**`, `examples/**`.
- **Deliverable:** obsolete tests deleted (wire/unwire, Bind/Rotate/Release); live tests retyped; examples compile; no new `#[allow(dead_code)]`.
- **Deps:** all prior C-* tasks.
- **Size:** medium.

### C-16 · web / WASM
- **Files:** `meerkat-web-runtime/src/lib.rs`, `sdks/web/src/*.ts` (Rust-side bindings only).
- **Deliverable:** retyped V5/V6. `allow_web_build` string-form bypass deleted (dogma #67 residual).
- **Deps:** C-13.
- **Size:** small.

---

## Section 4 — Dependency graph

```
    [V1..V8 wave-b foundations green]
                 |
        +--------+--------+
        |                 |
      C-1 core         C-2 contracts
        |    \__        /
        |       \      /
        v        \    v
      C-5 comms   C-4 tools/skills
        |
        v
      C-3 session   (← C-1, C-2)
        |
        v
      C-6 runtime   (choke point; ← C-1, C-5, V2)
        |
        +---> C-14 providers (can start earlier; ← C-2)
        v
      C-7 mob
        |
        +---> C-8 mob-mcp
        v
    [fan-out: C-9 rpc | C-10 rest | C-11 mcp-server | C-12 cli | C-13 facade]
                                                                    |
                                                                    v
                                                                C-16 web
                                                                    |
                                                                    v
                                                               C-15 test sweep
```

Serial spine: `foundations → C-1 → C-6 → C-7 → C-9`. Everything else is a parallel tributary. C-1, C-2, C-4, C-5, C-14 can all start concurrently once foundations are green. After C-7, the fan-out tier (C-8..C-13) partitions cleanly by crate.

---

## Section 5 — Parallelism strategy

**Recommendation: per-task worktrees for the fan-out tier, single branch for the serial spine.**

- Spine (C-1, C-6, C-7) touches cross-crate seams; only one author at a time; stay on `dogma/wave-a-demolition`.
- Fan-out (C-8, C-9, C-10, C-11, C-12, C-13, C-14, C-16) partitions by crate — worktree isolation prevents concurrent edits to `Cargo.lock`, build caches, and derived `.rkat/` files. Each fan-out worktree rebases against the spine's tip when each serial task lands.

**Named conflict risks.**
1. `Cargo.lock` — only the spine touches `Cargo.toml`; fan-out is code-only.
2. `meerkat-contracts/src/lib.rs` re-exports — C-2 owns the final list; downstream crates only *consume*.
3. `meerkat-cli/src/main.rs` — 278 field-access sites in one file. Keep C-12 single-agent; no line-range splitting.
4. `meerkat-mob/src/runtime/tests.rs` + `meerkat-mob/tests/member_session_bindings.rs` — C-7 deletes orphans inline with the effect collapse; C-15 only retypes survivors.
5. Wave-b B-5 (composition dispatcher) may still be landing — C-10/C-11/C-12/C-14/C-16 unblock first; C-7 and C-9 wait for B-5.

**Commit hygiene.** `git commit -o <paths>` remains mandatory — prevents sibling worktrees from absorbing each other's partials via implicit `-a`. `--no-verify` remains until `cargo check --workspace` is clean; flip hooks back on at the merge-to-main step. Do not attempt mid-wave — a half-compiling tree fails `cargo fmt --check` and blocks progress. One crate per commit in the fan-out tier; spine commits split per cluster (e.g., "C-1: SkillId purge", "C-1: TrustedPeerSpec retype").

---

## Section 6 — Risk register

1. **`SessionLocator.realm_id: String` vs `ConnectionRef.realm: RealmId` diverge silently.** `meerkat-contracts/src/session_locator.rs:8` stays `Option<String>`; callers mixing the two lose compile-time help.
   *Catching assertion:* `meerkat-contracts/tests/locator_realm_typed.rs` round-trips via `RealmId::parse`; asserts string-form is accepted only at the CLI boundary.

2. **V3 legacy-row deserialize silently drops unknown provider knobs.**
   *Catching assertion:* `meerkat-session/tests/persistent_round_trip_legacy.rs` loads a fixture with an unknown knob and asserts deserialize *fails* typed, not silently drops. Fixture: `test-fixtures/session/pre_wave_c/unknown_knob.json`.

3. **C-MEMBER-BIND collapse loses the "no-from + no-to" invariant.** `MemberSessionBindingChanged { from: None, to: None }` is ambiguous.
   *Catching assertion:* `meerkat-mob/tests/member_session_bindings.rs` rejects `(None, None)` at the DSL/runtime validator level; every emit site supplies at least one side.

4. **C-WIRE-RETIRED leaves dangling method names in the RPC catalog.**
   *Catching assertion:* `verify-schema-freshness` extended to assert `rpc_catalog` contains no `session/{retire,reset,submission,submissions}` entries.

5. **C-12 ad-hoc CLI parser regrowth.**
   *Catching assertion:* `meerkat-cli/tests/connection_ref_single_parser.rs` asserts exactly one `split_once(':')` site inside `meerkat-cli/src/**`, living in `cli_parse.rs`.

6. **C-AGENT-RT-ID `pub(crate)` tightening breaks an out-of-tree consumer.**
   *Catching assertion:* `rmat-audit` run at end of C-7 + `meerkat-mob/tests/public_api.rs` instantiates `MobMemberSnapshot` externally and confirms the field is unaddressable.

7. **C-REST-READMUT deletion silently loses a lazy registration side-effect.**
   *Catching assertion:* trace `SessionRuntime::ensure_registered` call sites; at least one non-READ caller must remain, else delete the helper entirely.

8. **V5 `PeerName`-keyed maps leak outside `meerkat-comms`.**
   *Catching assertion:* wave-c verification `rg 'HashMap<PeerName|BTreeMap<PeerName' --type rust` returns zero hits outside display-only sites in `meerkat-comms`.

---

## Section 7 — Completion criteria

Wave (c) is done when all hold:

- `./scripts/repo-cargo check --workspace --all-targets` exits zero.
- `./scripts/repo-cargo nextest run --workspace` passes with zero failures.
- `./scripts/repo-cargo clippy --workspace -- -D warnings` passes.
- `git diff origin/main..HEAD -- '*.rs' | grep '^+.*#\[allow(dead_code)\]'` returns zero.
- No new `serde_json::Value` fields in semantic seams. The only surviving `Value`s in `meerkat-contracts/src/wire/` and `meerkat-core/src/lifecycle/run_primitive.rs` are the allow-listed pass-through fields (structured_output, params_schema, mcp server_config, tool-call args as `Box<RawValue>`).
- Every tombstone cluster in §1 returns zero `rg` hits against its deleted-symbol fingerprint, except the `pub(crate)` survivors in C-AGENT-RT-ID.
- `verify-schema-freshness` passes; retired verbs removed from catalog.
- Tests that existed pre-wave-a either exist or are consciously deleted with a commit citing obsolescence. Orphan tests from the wave-a straggler report §4 are rewritten or deleted; none remain compile-broken.
- `make ci` exits zero locally. GitHub `gate` green on the wave-c merge PR.
- Neither `cargo fmt --check` nor the pre-commit hook chain is skipped on the final merge commit — wave-c ends the `--no-verify` tolerance.

Wave-c excludes SDK regen, doc regen, new surfaces, and any wave-a-deleted shadow state. Those are wave-d.
