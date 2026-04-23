# Wave (c) — Shell-Code Rebuild Plan

**Branch:** `dogma/wave-a-demolition` (broken tree). Plan assumes wave-b foundations have landed fully — at the time of authoring, wave-b is partially complete (`meerkat-machine-schema` and `meerkat-core` themselves still fail to build due to B-1/B-2 handoff gaps). Wave-c starts when wave-b leaves the foundation crates (`meerkat-machine-schema`, `meerkat-machine-kernels`, `meerkat-machine-codegen`, `meerkat-core`, `meerkat-contracts`) green; every downstream crate is still broken.

**Wave-c is not a rewrite.** It is a retyping pass on consumer code plus a targeted set of design decisions where the typed foundation has collapsed a formerly-implicit seam. No shadow state is reintroduced. Callers that became obsolete because wave-a deleted their semantic owner are *deleted*, not retyped.

---

## Section 1 — Tombstone inventory

Counts are `rg` over the branch head. "Sites" = line hits; "files" = unique files.

| Cluster | Sites | Files | Example pattern | Owning wave-b foundation |
|---|---|---|---|---|
| C-SKL: `SkillId` legacy string-id | 58 | 20 | `skills::SkillId`, `SkillId(format!(...))` | V4 (`SkillKey` is canonical) |
| C-CNX-FIELDS: `.realm_id` / `.binding_id` field access | 278 | 47 | `scope.locator.realm_id`, `key.binding_id` | V6 (`ConnectionRef { realm, binding, profile? }`) |
| C-CNX-PARSE: `ConnectionRef::parse(..)` / `split_once(':')` callers | ~12 | 4 (`meerkat-cli/src/main.rs:3798`, `mcp.rs:193`, `meerkat-mcp-server/src/lib.rs:169,2583`, `meerkat-rest/src/lib.rs:848,924`) | V6 (CLI boundary parser is sole survivor) |
| C-TRP: `TrustedPeerSpec` unresolved import / use | 204 | 30 | `crate::comms::TrustedPeerSpec` | V5 (replaced by `PeerId` + `TrustedPeerDescriptor`) |
| C-CSS: `CredentialStorageSpec` unresolved | 2 | 1 (`meerkat-core/src/lib.rs:255`) | V6 (wave-a deleted the type, re-export is stale) |
| C-TM-V3: `provider_params: Option<serde_json::Value>`, stray `additional_instructions`, `render_metadata` reads | ~60 | 10 (`meerkat-core/src/session_recovery.rs:21,108,195,252`; `meerkat-session/src/persistent.rs:1041,1877,2148,2376`; `meerkat/src/surface/runtime_schedule_host.rs:145,160`) | V3 (`RuntimeTurnMetadata` typed) |
| C-TOOL-ERR: dispatcher returns `ToolError::NotFound` for policy denial | 4 | 2 (`meerkat-tools/src/dispatcher.rs:36,239,311,351`; `meerkat-tools/src/builtin/composite.rs:404`) | V7 (`AccessDenied` propagates) |
| C-WIRE-STATE: `json!({...})` hand-built runtime responses | ~12 | 2 (`meerkat-rest/src/lib.rs:1327,1658,1702,1772`) | V8 (`WireInputState`, `SessionAcceptInputParams`) |
| C-WIRE-RETIRED: consumers of deleted `RuntimeRetireParams/ResetParams/InputStateParams/InputListParams` | unresolved symbol (B-9 landed) — consumer-side imports dead | 1 (`meerkat-rpc/src/handlers/runtime.rs`) | V8 |
| C-MEMBER-BIND: `MemberSessionBindingSet/Rotated/Released` effects | ~15 | 3 (`meerkat-mob/src/machines/mob_machine.rs:1618,1630,1653,1666,1684`; `meerkat-mob/tests/member_session_bindings.rs:88..285`; `meerkat-rpc/src/realtime_ws.rs:2158`) | B-5 composition (wave-a collapsed to `MemberSessionBindingChanged` on one side only) |
| C-TRK-B-CMDS: `MobMachineCommand::{WireMembers,UnwireMembers,BindMemberSession,RotateMemberSession,ReleaseMemberSession}` callers | ~8 | 2 (`meerkat-mob/src/runtime/tools.rs:589,818,832`; tests) | delete — commands are gone |
| C-DEAD-DRV: composition driver module path pointing at deleted file | 3 | 2 (`meerkat-machine-schema/src/catalog/compositions.rs:305,307`; `meerkat-machine-codegen/tests/render_contracts.rs:305,367`) | B-5 (dispatcher trait in `meerkat-runtime/src/composition/`) |
| C-AGENT-RT-ID: `MobMemberSnapshot.agent_runtime_id: AgentRuntimeId` public leakage | ~50 | 20 (`meerkat-mob/src/runtime/handle.rs:226,228,230,322,324,363,365`; `public_mcp.rs`; tests) | design decision — retain internal, hide from wire |
| C-REST-READMUT: `ensure_runtime_session_registered` on GET-adjacent paths | 5 | 1 (`meerkat-rest/src/lib.rs:1280,1915,1986,2663,3495`) | delete for read handlers |
| C-CLI-DISCRIM: CLI still emits `mob_member_target` realtime discriminator | 2 | 1 (`meerkat-cli/src/main.rs:750,760`) | V8 (canonical `mob_member`) |
| C-EXT-WIRE-META: `wire/mob.rs:152 provider_params: Option<Value>` | 1 | 1 | V3 wire projection |

Total: ~760 tombstone sites across roughly 80 files. The V1 kernel-identity newtype cascade (landing under wave-b B-1/B-2/B-3) will drive another retyping pass inside `meerkat-machine-kernels/src/generated/**` which is codegen-owned and will fall out automatically once B-4 emits against typed identities; that retyping is **not** wave-c work.

---

## Section 2 — Classification

### Mechanical retype (no design decision)

- **C-SKL**: replace every `SkillId` with `SkillKey`. Ingress sites build `SkillKey` from `SourceUuid` + `SkillName`. `SkillId(format!("{}/{}", …))` at `meerkat-core/src/agent/runner.rs:990` is deleted; the typed `SkillKey` flows through the runner.
- **C-CNX-FIELDS**: replace `.realm_id` → `.realm.as_str()` (when a `&str` is needed) or `.realm.clone()` (owning). Similarly `.binding_id` → `.binding.as_str()`. `SessionLocator.realm_id: Option<String>` is a separate type *in `meerkat-contracts/src/session_locator.rs:8`* — keep as-is (realm handles live at a different layer) unless wave-b explicitly retypes it. Only `ConnectionRef`-typed structs get the rename.
- **C-CSS**: delete the stale `CredentialStorageSpec` re-export at `meerkat-core/src/lib.rs:255`. The symbol is gone.
- **C-TOOL-ERR**: replace `ToolError::NotFound` with `ToolError::AccessDenied` at the four dispatcher-gate sites. Wire mapping lands automatically via B-9's `WireToolErrorClass`.
- **C-DEAD-DRV**: replace string `module_path` driver declarations with typed `CompositionDispatcherHandle` ref (wave-b B-5 surface).
- **C-CLI-DISCRIM**: CLI emits the canonical `mob_member` variant of the realtime target enum; one-line fix.
- **C-EXT-WIRE-META**: projecting typed `ProviderParamsOverride` (from V3) through `wire/mob.rs:152` — mechanical map.

### Design decision required

- **C-TRP**: `TrustedPeerSpec` was the old pre-wave-b trust shape mixing routing (`peer_name`) and verification (public-key/cert). V5 splits `PeerId` (routing) from `TrustedPeerDescriptor` (verification). **Decision point**: every `TrustedPeerSpec` consumer must split into "which `PeerId` does this address" + "what is the trust policy for that peer" — the two travel together but are typed separately. For each of the 30 consumer files the deciding question is: *is this code doing routing (takes `PeerId`), or verification (takes `TrustedPeerDescriptor`), or both (takes a `PeerRoute` composite that pairs them)?* See Section 3 per-crate for per-file assignments.
- **C-TM-V3**: `session_recovery.rs:21` currently has `provider_params: Option<serde_json::Value>`. V3 types it. But the recovery module is replaying pre-wave-b persisted rows where `provider_params` may be a `Value`. **Decision**: v0 migration path — on read, deserialize `Value` into `ProviderParamsOverride` via `ProviderParamsOverride::from_legacy_value`, fail typed on malformed. Do *not* keep a "legacy mode" runtime-side variant. Persistent rows written post-wave-c land already-typed.
- **C-WIRE-STATE**: the four `json!({...})` REST handlers at `meerkat-rest/src/lib.rs:1327,1658,1702,1772` construct `RuntimeAcceptResult` / `WireInputState`-shaped blobs by hand. B-9 gives us typed structs. **Decision**: pass the typed struct straight through Axum's `Json(..)` responder; strip the hand-rolled JSON. If one of the four handlers was doing reshape beyond simple field-rename, that reshape becomes a typed `From<DomainT> for WireT` impl in contracts, not an inline JSON literal. (Verification: none of the four sites carries conditional fields — they are field-rename plus a wrapper.)
- **C-AGENT-RT-ID**: `AgentRuntimeId` and `FenceToken` still leak through `meerkat-mob/src/runtime/handle.rs:226,228,230,322,324,363,365` on `MobMemberSnapshot`. Wave-a deleted MCP-side serialization. **Decision**: keep the fields `pub(crate)`-scope for internal mob runtime use; remove `pub` from result structs that cross a wire/MCP boundary. Any consumer outside `meerkat-mob/src/runtime/` that still reads these fields is retyped to accept a public-facing `MobMemberView` that omits them.
- **C-MEMBER-BIND**: the effect triple `MemberSessionBindingSet/Rotated/Released` still exists in the runtime-used DSL (`meerkat-mob/src/machines/mob_machine.rs:1618-1700`). Wave-a-cleanup commit `7f88cb477` deleted the duplicate in the schema catalog but left the runtime DSL. **Decision**: wave-c collapses the runtime DSL to a single `MemberSessionBindingChanged { edge, from: Option<SessionId>, to: Option<SessionId> }` effect. The three transitions merge their emit lists; realtime WS consumers (`meerkat-rpc/src/realtime_ws.rs:2158`) filter on the `from`/`to` pair to recover "set vs rotated vs released" for channel-status derivation. This is the exact vocabulary that `compositions.rs:320` already watches.

### Delete (caller is obsolete)

- **C-WIRE-RETIRED**: `RuntimeRetireParams`, `RuntimeResetParams`, `InputStateParams`, `InputListParams` and their handler shells at `meerkat-rpc/src/handlers/runtime.rs` — the verbs are retired. Delete their RPC router entries, delete the handler functions, delete any SDK-facing test that exercised them. Python SDK still advertises `status`/`submit` — wave-d (SDK regen) owns that; wave-c deletes the Rust shell only.
- **C-TRK-B-CMDS**: `meerkat-mob/src/runtime/tools.rs:589,818,832` (`WireMembersArgs` + parser path). The `MobMachineCommand::{WireMembers, UnwireMembers, BindMemberSession, RotateMemberSession, ReleaseMemberSession}` variants are deleted — these agent-facing tool arguments for them should be deleted, not retyped. The canonical path is mob-machine-internal.
- **C-REST-READMUT**: `ensure_runtime_session_registered` on `meerkat-rest/src/lib.rs:3495` (and any other read-only handler sharing the helper) — delete the call. Read handlers do not mutate registration state.
- **Orphan tests** — the wave-a report flagged `meerkat-mob/src/runtime/tests.rs:9483,10298,11577` referencing deleted `wire()`/`unwire()` semantics, plus `meerkat-mob/tests/member_session_bindings.rs:88..285` asserting the three deleted binding effects. Wave-c audits these tests, rewrites them if they still exercise live semantics, or deletes them if they were testing the deleted shape.

---

## Section 3 — Per-crate task briefs

Dependencies reference wave-b foundations (V-prefix) and intra-wave-c tasks (C-prefix).

### C-1: `meerkat-core` consumer retype
- **Files:** `meerkat-core/src/agent.rs`, `agent/state.rs`, `agent/runner.rs`, `agent/builder.rs`, `interaction.rs`, `ops_lifecycle.rs`, `session_recovery.rs`, `session.rs`, `service/mod.rs`, `event.rs`, `lib.rs` (re-exports), `hooks.rs`, `runtime_bootstrap.rs`, `config.rs`, `config_runtime.rs`, `config_store.rs`, `auth/token_store.rs`, `skills/*` (consumer sites only — V4 owns the definition).
- **Deliverable:** `meerkat-core` compiles. `SkillId` purged from core (C-SKL). `TrustedPeerSpec` imports replaced with V5 typed pair (C-TRP, routing subset). `CredentialStorageSpec` stale re-export deleted (C-CSS). `session_recovery.rs` takes typed `ProviderParamsOverride` with one-time legacy-`Value` in-migration (C-TM-V3 decision branch).
- **Deps:** V3, V4, V5, V6, V7 must all be green in their foundation crates (`meerkat-machine-schema`, `meerkat-core/src/connection.rs`, `meerkat-core/src/skills/*`, `meerkat-core/src/comms.rs`).
- **Size:** medium (≈15 files, dense retype).

### C-2: `meerkat-contracts` consumer retype
- **Files:** `meerkat-contracts/src/lib.rs`, `emit.rs`, `rpc_catalog.rs`, `session_locator.rs`, `wire/{connection,mob,params,realtime,runtime,supervisor_bridge,mod}.rs`.
- **Deliverable:** stale `SkillId` / `TrustedPeerSpec` / `CredentialStorageSpec` exports removed (C-SKL, C-TRP, C-CSS). `wire/mob.rs:152 provider_params: Option<Value>` replaced with `Option<WireProviderParamsOverride>` (C-EXT-WIRE-META). `emit.rs` + `rpc_catalog.rs` drop retired verbs (C-WIRE-RETIRED).
- **Deps:** V3 wire projection, V4, V5, V6, V8 in contracts itself (B-9 already landed per commit log).
- **Size:** small (≈8 files).

### C-3: `meerkat-session` consumer retype + legacy row migration
- **Files:** `meerkat-session/src/persistent.rs`, `ephemeral.rs`.
- **Deliverable:** persistent round-trip compiles with typed `ProviderParamsOverride` / typed `RuntimeTurnMetadata`. Adds `from_legacy_value` migration helper used only on read; write path is typed. Test fixture `test-fixtures/session/pre_wave_c/*.json` captures v0 row shape; `persistent_round_trip_legacy.rs` asserts v0→v1 deserialize succeeds.
- **Deps:** C-1, C-2. (V3 + V8 + V6 landed in core/contracts.)
- **Size:** small (2 files + fixtures).

### C-4: `meerkat-tools` + skills consumers
- **Files:** `meerkat-tools/src/dispatcher.rs`, `builtin/composite.rs`, `builtin/skills/{browse,load,resources,functions}.rs`, `meerkat-skills/src/{resolve.rs, source/filesystem.rs, source/composite.rs, source/embedded.rs, source/protocol.rs}`.
- **Deliverable:** `ToolError::AccessDenied` returned at the four policy gates (C-TOOL-ERR). Every builtin skill tool queries `SourceIdentityRegistry::canonical_skill_key` instead of the deleted `canonical_key` helper. `SkillRef::Legacy` / legacy-string parse removed from skills sources.
- **Deps:** V4, V7 in core.
- **Size:** small-medium (≈10 files).

### C-5: `meerkat-comms` + trust/router retype
- **Files:** `meerkat-comms/src/{runtime/comms_runtime.rs, trust.rs, router.rs, inbox.rs}`.
- **Deliverable:** `TrustStore` keyed by `PeerId`. Router `send(dest: PeerId)` signature. Name→id lookups return typed ambiguity error. All remaining `TrustedPeerSpec` sites inside comms retype to `(PeerId, TrustedPeerDescriptor)` pair.
- **Deps:** V5.
- **Size:** medium (≈5 files, dense).

### C-6: `meerkat-runtime` consumer retype
- **Files:** `meerkat-runtime/src/{comms_bridge, comms_drain, mob_adapter, meerkat_machine/dispatch_*, ops_lifecycle, runtime_loop, driver}.rs`.
- **Deliverable:** V3 single-site `RuntimeTurnMetadata::for_input` replaces both construction sites. V5 `PeerId` threaded through `comms_drain`/`mob_adapter`. V2 `CompositionDispatcher` consumed in dispatch_*.rs as *the* routed-effect path. TrustedPeerSpec imports retyped.
- **Deps:** C-1, C-5, V2 (B-5 composition dispatcher), V3 foundation.
- **Size:** large (8+ files, core semantic seams).

### C-7: `meerkat-mob` consumer retype + effect collapse
- **Files:** `meerkat-mob/src/{machines/mob_machine.rs, runtime/actor.rs, runtime/handle.rs, runtime/tools.rs, runtime/provisioner.rs, runtime/provision_guard.rs, runtime/supervisor_bridge.rs, runtime/ops_adapter.rs, runtime/actor_turn_executor.rs, runtime/disposal.rs, runtime/builder.rs, runtime/edge_locks.rs, runtime/event_router.rs, roster.rs, event.rs, build.rs, profile.rs, tests.rs, tests/contracts.rs, tests/phase1_red_ok.rs}`, `meerkat-mob/tests/{member_binding_orthogonality,member_session_bindings}.rs`.
- **Deliverable:**
  - `meerkat-mob/src/machines/mob_machine.rs` DSL collapses `MemberSessionBindingSet/Rotated/Released` into `MemberSessionBindingChanged { edge, from, to }` (C-MEMBER-BIND).
  - `meerkat-mob/src/runtime/tools.rs:589,818,832` deletes `WireMembersArgs` + paired parsers (C-TRK-B-CMDS).
  - `MobMemberSnapshot` visibility tightened; `agent_runtime_id`/`fence_token` become `pub(crate)` (C-AGENT-RT-ID).
  - TrustedPeerSpec retyped to `(PeerId, TrustedPeerDescriptor)` at every call site inside mob.
  - Orphan tests asserting deleted wire/unwire/Bind/Rotate/Release transitions deleted or rewritten to cover `MemberSessionBindingChanged`.
- **Deps:** C-1, C-5, V1+V2+V5. Must follow C-6 because mob adapter sits between runtime and mob.
- **Size:** large (20+ files).

### C-8: `meerkat-mob-mcp` consumer retype
- **Files:** `meerkat-mob-mcp/src/{lib.rs, public_mcp.rs, agent_tools.rs}`.
- **Deliverable:** `TrustedPeerSpec` retyped. `agent_runtime_id`/`fence_token` removed from public MCP result shapes (consumes the `pub(crate)` change from C-7). `MobMemberSnapshot`→`MobMemberView` projection for MCP surface.
- **Deps:** C-7, V5.
- **Size:** small (3 files).

### C-9: `meerkat-rpc` consumer retype + retired-verb cleanup
- **Files:** `meerkat-rpc/src/{router.rs, session_runtime.rs, realtime_ws.rs, main.rs, handlers/{session, mob, runtime, auth}.rs}`.
- **Deliverable:** Router drops retired verb entries (C-WIRE-RETIRED). `runtime.rs` handler consumes typed `SessionAcceptInputParams`/`WireInputState` (C-WIRE-STATE). Realtime WS `MemberSessionBindingRotated` reader collapses onto `MemberSessionBindingChanged` delta semantics (C-MEMBER-BIND). `.realm_id`/`.binding_id` field access rewritten (C-CNX-FIELDS). SkillId purged (C-SKL).
- **Deps:** C-1, C-2, C-6, C-7.
- **Size:** large (~8 files, realtime_ws is dense).

### C-10: `meerkat-rest` consumer retype
- **Files:** `meerkat-rest/src/{lib.rs, schedule_host.rs, auth_endpoints.rs}`.
- **Deliverable:** `ensure_runtime_session_registered` deleted from GET-adjacent handlers at `:3495` and any other read-only site (C-REST-READMUT). Four `json!({...})` runtime handlers at `:1327,1658,1702,1772` use typed wire responders (C-WIRE-STATE). `.realm_id`/`.binding_id` retyped (C-CNX-FIELDS). No string-form `ConnectionRef` accepted — 400 on legacy.
- **Deps:** C-2, C-6.
- **Size:** medium (3 files, `lib.rs` is dense).

### C-11: `meerkat-mcp-server` consumer retype
- **Files:** `meerkat-mcp-server/src/{lib.rs, main.rs, runtime_ingress.rs, schedule_host.rs}`.
- **Deliverable:** `.realm_id` retype (C-CNX-FIELDS); any string-form `ConnectionRef` ingress rejected at wire boundary (C-CNX-PARSE). SkillId purged (C-SKL).
- **Deps:** C-2, C-1.
- **Size:** small (4 files).

### C-12: `meerkat-cli` consumer retype + single CLI-boundary parser
- **Files:** `meerkat-cli/src/{main.rs, mcp.rs, cli_parse.rs (new)}`.
- **Deliverable:** **Sole** `parse_connection_ref_user_input(s: &str) -> Result<ConnectionRef, CliError>` lives in `cli_parse.rs`. Every ad-hoc `split_once(':')`/`splitn(2, ':')` inside `main.rs:3798` and `mcp.rs:193` deleted (C-CNX-PARSE). `.realm_id`/`.binding_id` field access retyped (C-CNX-FIELDS). CLI stops emitting `mob_member_target` discriminator (C-CLI-DISCRIM). SkillId purged.
- **Deps:** C-2.
- **Size:** medium (2 existing files + 1 new, `main.rs` is dense — 278 realm/binding access sites concentrate here).

### C-13: `meerkat` facade retype
- **Files:** `meerkat/src/{factory.rs, service_factory.rs, surface.rs, surface/{runtime_backed.rs, runtime_schedule_host.rs, schedule_host.rs}, prompt_assembly.rs, lib.rs}`.
- **Deliverable:** V3 turn-metadata single-site construction (C-TM-V3). Factory `None => ...` branches at `:1241,1684,1819,2306,2884,2900` become typed "ambient credential selection refused — `connection_ref` required" errors, not silent-fallback (consumes wave-a dogma-#8 stance). SkillId purged from factory.
- **Deps:** C-1, C-2, C-6.
- **Size:** medium-large (~8 files).

### C-14: provider crates
- **Files:** `meerkat-anthropic/src/runtime/mod.rs`, `meerkat-openai/src/runtime/mod.rs`, `meerkat-gemini/src/runtime/mod.rs`, `meerkat-llm-core/src/{adapter.rs, provider_runtime/registry.rs, types.rs}`, `meerkat-auth-core/src/{auth_store/file.rs, auth_store/refresh.rs, resolver.rs}`.
- **Deliverable:** `.realm_id`/`.binding_id` retype, typed `ProviderParamsOverride` consumed where provider-param overrides land at the provider boundary.
- **Deps:** C-2.
- **Size:** small-medium (~7 files).

### C-15: test harness sweep
- **Files:** `meerkat-mob/tests/*`, `meerkat-core/tests/*`, `meerkat-contracts/tests/*`, `tests/integration/tests/*`, `meerkat-cli/tests/*`, `meerkat-rpc/tests/*`, `meerkat-mcp-server/tests/*`, `meerkat-rest/tests/*` (if any), `examples/**/*.rs`.
- **Deliverable:** obsolete tests deleted (wire()/unwire(), Bind/Rotate/Release). Live tests retyped. Examples compile. No new `#[allow(dead_code)]`.
- **Deps:** all prior C-* tasks.
- **Size:** medium.

### C-16: web/WASM + docs surface
- **Files:** `meerkat-web-runtime/src/lib.rs`, `sdks/web/src/*`, `sdks/typescript/src/*` (Rust-side only — TS regen is wave-d), `docs/api/*.mdx` minimal edits to remove retired-verb mentions (wave-d owns full regen; wave-c just deletes broken references that still compile-link to Rust code).
- **Deliverable:** `meerkat-web-runtime` re-typed for V5 PeerId and V6 ConnectionRef. No `allow_web_build` string-form bypass (dogma #67 residual).
- **Deps:** C-13.
- **Size:** small (~4 files).

---

## Section 4 — Dependency graph

```
                      [V1..V8 wave-b foundations green]
                                    |
                   +----------------+----------------+
                   |                                 |
                 C-1 meerkat-core                   C-2 meerkat-contracts
                   |           \_____                /
                   |                 \_______     __/
                   v                         \   v
                 C-5 meerkat-comms            C-4 meerkat-tools/skills
                   |
                   v
                 C-3 meerkat-session   (depends C-1, C-2)
                   |
                   v
                 C-6 meerkat-runtime   (depends C-1, C-5, V2)
                   |
                   +-----> C-14 providers (needs C-2 only; can run earlier)
                   v
                 C-7 meerkat-mob       (depends C-6)
                   |
                   +-----> C-8 mob-mcp
                   v
                 C-9 meerkat-rpc       (depends C-6, C-7)
                 C-10 meerkat-rest     (depends C-6)
                 C-11 meerkat-mcp-server
                 C-12 meerkat-cli      (depends C-2 only; touches main.rs widely)
                 C-13 meerkat facade   (depends C-1, C-2, C-6)
                 C-16 web/wasm         (depends C-13)
                   v
                 C-15 test harness sweep (final)
```

Parallel opportunities: C-1 ↔ C-2 ↔ C-4 ↔ C-5 ↔ C-14 can all run concurrently once foundations are green. C-6 is the choke point — everything mob/rpc/rest/facade blocks on it because V2 composition dispatcher + V3 turn-metadata single-site wiring live there. After C-6, C-7/C-9/C-10/C-11/C-12/C-13 fan out in parallel with minimal mutual contention (each owns distinct crates).

Serial spine: `foundations → C-1 → C-6 → C-7 → C-9`. Everything else is a parallel tributary.

---

## Section 5 — Parallelism strategy

**Recommendation: per-task worktrees for the fan-out tier, single branch for the serial spine.**

Reasoning:

- The serial spine (`C-1`, `C-6`, `C-7`) touches *cross-crate* semantic seams. Merges are trivial because only one agent is writing. Stay on `dogma/wave-a-demolition` for these.
- The fan-out tier (`C-9 meerkat-rpc`, `C-10 meerkat-rest`, `C-11 meerkat-mcp-server`, `C-12 meerkat-cli`, `C-13 meerkat facade`, `C-14 providers`, `C-8 mob-mcp`, `C-16 web`) partitions cleanly by crate. Worktree isolation prevents concurrent edits to `Cargo.lock` / `repo-cargo` caches / `.rkat/` derived files. Each agent gets its own worktree rebased against the tip of the spine as each spine task lands.

**Named conflict risks:**

1. `Cargo.lock`: any consumer crate bumping a dep transitively poisons the lock. Pin: only the spine touches `Cargo.toml`; fan-out tier is code-only.
2. `meerkat-contracts/src/lib.rs`: eight consumer crates all want their re-export. Solution: C-2 owns the final re-export list; downstream crates *consume*, never *add*.
3. `meerkat-cli/src/main.rs`: one file, 278 field-access sites. Keep C-12 single-agent — splitting it by line ranges invites merge-weaving bugs.
4. `meerkat-mob/src/runtime/tests.rs` + `meerkat-mob/tests/member_session_bindings.rs`: orphan tests from C-7 and C-15 both touch. Resolution: C-7 deletes orphan tests inline with the effect collapse; C-15 only retypes survivors.
5. Wave-b B-5 (composition dispatcher) may still be landing when fan-out starts. Mitigation: fan-out crates that do not touch composition (C-10 REST, C-11 MCP-server, C-12 CLI, C-14 providers, C-16 web) unblock first; C-7/C-9 wait for B-5.

**Commit hygiene:**

- `git commit -o <paths>` remains mandatory — prevents sibling-agent worktrees from sucking in each other's partials via implicit `-a`.
- `--no-verify` remains until the tree compiles. Once `cargo check --workspace` is clean, flip pre-commit hooks back on at the merge-to-main step. Do **not** attempt to flip them mid-wave; a half-compiling tree will fail `cargo fmt --check` and block progress.
- One crate per commit for fan-out tier. Spine tier commits per logical retype cluster (e.g., "C-1: SkillId purge", "C-1: TrustedPeerSpec retype").

---

## Section 6 — Risk register

1. **Risk — `SessionLocator.realm_id: String` vs `ConnectionRef.realm: RealmId` diverge silently.** `meerkat-contracts/src/session_locator.rs:8` keeps `realm_id: Option<String>` per wave-b. If C-12 CLI callers start passing `RealmId` down and downstream code expects `String`, compile-time checks may mask a behavioral mismatch.
   - **Catching assertion:** wave-c adds a `meerkat-contracts/tests/locator_realm_typed.rs` that round-trips `SessionLocator` through `RealmId::parse` and asserts the string-form is accepted only at the CLI boundary.

2. **Risk — V3 turn-metadata legacy-row deserialize silently drops provider-params knobs.** Persistent rows written pre-wave-c carry `Value` `provider_params` that the typed `ProviderParamsOverride` doesn't know about.
   - **Catching assertion:** `meerkat-session/tests/persistent_round_trip_legacy.rs` loads fixture with an unknown provider-knob and asserts deserialize *fails* typed (not silently drops). Fixture: `test-fixtures/session/pre_wave_c/unknown_knob.json`.

3. **Risk — C-7 collapse of `MemberSessionBindingSet/Rotated/Released` to `Changed { from, to }` loses the "no-from + no-to" distinction.** A plain `Changed { from: None, to: None }` is ambiguous.
   - **Catching assertion:** `meerkat-mob/tests/member_session_bindings.rs` asserts that every emit site supplies at least one of `from`/`to` and rejects `(None, None)` at the DSL level via a compile-time invariant or runtime validator.

4. **Risk — C-9 retired-verb deletion leaves dangling RPC method names in `meerkat-contracts/src/rpc_catalog.rs`.** Router registration and catalog must agree.
   - **Catching assertion:** existing `verify-schema-freshness` lane. Wave-c extends the schema emitter test to assert `rpc_catalog` contains no `session/retire`, `session/reset`, `session/submission`, `session/submissions` method entries.

5. **Risk — C-12 ad-hoc CLI parser regrowth.** CLI is the one place where `realm:binding` input form is allowed; nothing structural prevents a future developer from re-adding `split_once(':')` elsewhere.
   - **Catching assertion:** `meerkat-cli/tests/connection_ref_single_parser.rs` asserts exactly one `split_once(':')` call site inside `meerkat-cli/src/**` (grep count), and that it lives in `cli_parse.rs`.

6. **Risk — C-7 tightening `pub` → `pub(crate)` on `MobMemberSnapshot.agent_runtime_id` breaks an out-of-tree consumer we did not anticipate.** The facade or an example may read it through a public type boundary.
   - **Catching assertion:** the existing xtask `rmat-audit` run at the end of C-7; it fails if any surface crate transitively imports a now-private field. Add an explicit `meerkat-mob/tests/public_api.rs` that instantiates `MobMemberSnapshot` from outside the crate and confirms `agent_runtime_id` is not addressable.

7. **Risk — `ensure_runtime_session_registered` deletion from GET handlers (C-REST-READMUT) loses a side-effect that a downstream surface silently depended on.** The helper name suggests it was doing lazy registration, which might have been the only path that ever populated the registry on read.
   - **Catching assertion:** trace callers of `SessionRuntime::ensure_registered` across the crate; asserts at least one non-READ path exercises it. If zero non-READ callers remain, the helper itself should be deleted, not just the read-side invocations.

8. **Risk — V5 PeerId cutover leaves `PeerName`-keyed hashmaps lying around in non-`meerkat-comms` crates.** Routing regression is silent when `PeerName` happens to be unique in tests but duplicates at runtime.
   - **Catching assertion:** wave-b B-10 xtask lane already detects this. Wave-c verification step: `rg 'HashMap<PeerName|BTreeMap<PeerName' --type rust` on the final compiling tree returns zero hits outside `meerkat-comms/src/display_cache.rs`-style display-only sites.

---

## Section 7 — Completion criteria

Wave (c) is done when all are true:

- `./scripts/repo-cargo check --workspace --all-targets` exits zero.
- `./scripts/repo-cargo nextest run --workspace` passes with zero failures.
- `./scripts/repo-cargo clippy --workspace -- -D warnings` passes.
- `rg '#\[allow\(dead_code\)\]' --type rust | git diff origin/main..HEAD -- '*.rs' | grep '^+.*#\[allow(dead_code)\]'` returns zero rows (no wave-c `#[allow(dead_code)]` additions).
- `rg 'serde_json::Value' meerkat-contracts/src/wire/ meerkat-core/src/lifecycle/run_primitive.rs` matches only the allow-listed non-semantic pass-through fields (structured_output, params_schema, mcp server_config, tool-call args `Box<RawValue>`). No new `Value` fields in semantic seams.
- Every tombstone cluster from Section 1 returns zero `rg` hits against the respective deleted symbol (except the `pub(crate)` survivors in C-AGENT-RT-ID).
- `verify-schema-freshness` gate passes (schema artifacts regenerated only where the wire type shape changed under wave-b; retired verbs removed).
- All tests that existed pre-wave-a still exist or have been consciously deleted with a commit message citing obsolescence. Orphan tests from wave-a's table (Section 4 of the straggler report) are either rewritten or deleted; none remain compile-broken.
- `make ci` (local full lane) exits zero. GitHub `gate` job is green on the wave-c merge PR.
- Neither `cargo fmt --check` nor the pre-commit hook chain is skipped on the final merge commit (wave-c ends the `--no-verify` tolerance).

Wave-c does **not** include SDK regen, doc regen, new surfaces, or reintroduction of any wave-a-deleted shadow state. Those are wave-d.
