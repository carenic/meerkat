# Wave-c persistence migration (v0 → v1)

Scope: design only. Covers the v0→v1 migration induced by wave-b retypes
(`RuntimeTurnMetadata` V3, `WireInputState` typed fields, `ConnectionRef`
structural). Implementation lands in wave-c.

## 1. Persisted-row inventory

Three physically distinct stores carry serialized shapes that wave-b
touches, plus the wire-only envelope that rides on top of RPC.

### 1.1 `sessions` table (SQLite) — `SessionStore` blob row
File: `meerkat-store/src/sqlite_store.rs:15-24` (DDL) and
`meerkat-store/src/sqlite_store.rs:71-108` (writer).

```
sessions(session_id, created_at_ms, updated_at_ms, message_count,
         total_tokens, metadata_json TEXT, session_json BLOB)
```

`session_json` is `serde_json::to_vec(&Session)` of
`meerkat-core/src/session.rs:32-47`. `Session` already carries
`version: u32` (`SESSION_VERSION = 1` — `meerkat-core/src/session.rs:26`),
with a `#[serde(default = "default_version")]` so pre-versioned rows
deserialize as v1. `Session.metadata: serde_json::Map<String,Value>` holds
`SESSION_METADATA_KEY → SessionMetadata` (`session.rs:696-711`,
`session.rs:852-892`).

Fields changed by wave-b inside `SessionMetadata`:
- `provider_params: Option<serde_json::Value>` → `Option<ProviderParamsOverride>` (`session.rs:861`; commit `197a70b4a`)
- `connection_ref: Option<ConnectionRef>` — *structural change*, same field name; v0 inner shape is `{realm_id: String, binding_id: String, profile: Option<String>}`, v1 is `{realm: RealmId, binding: BindingId, profile: Option<ProfileId>}` (`session.rs:891`; commit `cf90208b7`, `meerkat-core/src/connection.rs:86-106`)
- `realm_id: Option<String>` (session.rs:871) and the string `realm_id`/`binding_id` pair inside v0 `ConnectionRef` — both collapse under v1 `RealmId`/`BindingId` newtypes
- `SessionLlmIdentity.provider_params: Option<Value>` (`session.rs:907`) and `SessionLlmIdentity.connection_ref` (`session.rs:921`) — same pair of changes, projected through hot-swap

The secondary `metadata_json` column is a denormalized copy of
`session.metadata` used only for listing (`sqlite_store.rs:76,188-214`);
fields here are the same `Map<String,Value>`, not the typed struct.

### 1.2 `runtime_*` tables — `RuntimeStore`
File: `meerkat-runtime/src/store/sqlite.rs:18-43` (DDL).

```
runtime_input_states(runtime_id, input_id, state_json BLOB)
runtime_boundary_receipts(runtime_id, run_id, sequence, receipt_json)
runtime_session_snapshots(runtime_id, session_snapshot BLOB)
runtime_states(runtime_id, runtime_state_json)
runtime_ops_lifecycle(runtime_id, state_json)
```

- `runtime_input_states.state_json` = `StoredInputState` via the custom
  `InputStateSerde` helper (`meerkat-runtime/src/input_state.rs:257-336`).
  `persisted_input: Option<Input>` (`input_state.rs:278`) recursively
  carries `PromptInput`/`PeerInput`/`FlowStepInput`/... each of which
  owns `turn_metadata: Option<RuntimeTurnMetadata>`
  (`meerkat-runtime/src/input.rs:265,291,409`). Every field retyped in
  B-6 lives under here when the input was persisted:
  - `RuntimeTurnMetadata.provider_params` — `Option<Value>` → `Option<ProviderParamsOverride>` (`run_primitive.rs:292`)
  - `RuntimeTurnMetadata.model: Option<String>` → `Option<ModelId>` (`run_primitive.rs:286`)
  - `RuntimeTurnMetadata.provider: Option<String>` → `Option<Provider>` (`run_primitive.rs:289`)
  - `RuntimeTurnMetadata.additional_instructions: Option<Vec<String>>` → `Option<Vec<TurnInstruction>>` (`run_primitive.rs:283`)
  - NEW fields: `connection_ref: Option<ConnectionRef>` (`run_primitive.rs:295`), `keep_alive: Option<KeepAlivePolicy>` (`run_primitive.rs:298`)
- `runtime_session_snapshots.session_snapshot` is a second serialization
  of `Session` (written in `SessionDelta`,
  `meerkat-runtime/src/store/mod.rs:38-41`,
  `sqlite.rs:87-101,224-263`). Same migration surface as §1.1.
- `runtime_states.runtime_state_json` = `RuntimeState`
  (`meerkat-runtime/src/runtime_state.rs:27`). Enum of lifecycle phases,
  no wave-b-typed fields.
- `runtime_boundary_receipts.receipt_json` = `RunBoundaryReceipt`
  (conversation_digest + counts). No wave-b-typed fields.
- `runtime_ops_lifecycle.state_json` = `PersistedOpsSnapshot`
  (optional; `store/mod.rs:177`). No wave-b-typed fields.

### 1.3 `.rkat/sessions/{id}/events.jsonl` — projector output
File: `meerkat-session/src/projector.rs:67-121`. Each line is
`StoredEvent` (`meerkat-session/src/event_store.rs:12-25`) which already
has `schema_version: u32` (`EVENT_SCHEMA_VERSION = 1`). Payload is
`AgentEvent`. AgentEvent variants do not embed `RuntimeTurnMetadata` or
`WireInputState`, but some (`RunStarted`, hot-swap notifications)
carry `ConnectionRef` indirectly when it is present in the session
snapshot echoed back in events. `.rkat/` is derived and disposable
(`CLAUDE.md` rule); replaying the event store regenerates it.

### 1.4 Wire-only shapes (not persisted but cross-process)
`WireInputState` (`meerkat-contracts/src/wire/runtime.rs:235-260`) and
`WireRuntimeTurnMetadata` (same file, added in commit `18e12d3a4`) are
**wire projections** only. The B-9 V8 commit (`5fb027af1`) explicitly
notes: "Runtime-side projection of the rich internal shapes into the
typed enums is wave-c plumbing; for now these fields ride through as
None (no untyped payload leaks either way)." They are not on the
persistence path. However, a pre-wave-b deployment running a
`rkat-rpc` client talking to a newer server could send v0 wire frames
— captured below under "mixed deployment".

## 2. Versioning strategy

**Pick: per-entity schema-version byte, piggy-backed on the existing
`Session.version` field plus a new `SESSION_METADATA_SCHEMA_VERSION`
discriminator embedded in the `SessionMetadata` JSON, and a new
`stored_input_state_version: u32` field added to `InputStateSerde`.**

Rationale:
- A single monolithic session version is insufficient. `Session`
  already has one (`session.rs:26`), but `StoredInputState` rows and
  `runtime_states` rows are written by a different store and evolve
  independently.
- A separate version column per table would require a DDL migration
  plus CI plumbing; the existing JSON bodies already have natural
  extension points.
- An opportunistic "try v1, fall back to v0" scheme sounds appealing
  but is footgun-laced: serde is happy to silently accept a v0
  `provider_params: Value::Object({...})` into a v1
  `Option<ProviderParamsOverride>` if any v0 key happens to match a v1
  field (e.g. `temperature`), producing a lossy success. A typed
  discriminator forces an explicit branch.
- The wave-d SDK-regen pass does not care about the Rust envelope; it
  consumes `meerkat-contracts` wire types + JSON schemas, which are
  already versioned via `ContractVersion::CURRENT`. A per-entity
  version inside the Rust persistence layer is orthogonal and does not
  require SDK re-emit.

Concretely:
- Bump `SESSION_VERSION: u32 = 2`.
- Add `#[serde(default)] pub schema_version: u32` to `SessionMetadata`
  (default 1 on read, write 2 after wave-c).
- Add `#[serde(default)] stored_input_state_version: u32` to
  `InputStateSerde` (default 1 on read, write 2).
- Leave `StoredEvent.schema_version` as-is; AgentEvent payloads were
  not retyped.

## 3. v0 → v1 migration matrix

Read-path migrations live behind `serde_json::Value`-typed "envelope"
readers in `meerkat-session::persistent::migrations::{session,input_state}`
(new module, wave-c). Each envelope reads a Value, inspects the
version discriminator, and dispatches.

### 3.1 `SessionMetadata.provider_params` — `Value` → `ProviderParamsOverride`
- v0 shape: `serde_json::Value::Object({...})` with any of
  `temperature`, `top_p`, `max_output_tokens`, `reasoning`,
  `thinking_budget_tokens`, plus arbitrary provider-specific keys.
- v1 target: `ProviderParamsOverride`
  (`run_primitive.rs:218-231`) — six typed fields, plus
  `provider_tag: Option<ProviderTag>`.
- Mapping: pick known keys by literal name into the typed slot; type
  coerce numbers (`reasoning` string → `ReasoningMode` enum). Unknown
  keys move into `ProviderTag` variant matching the session's
  `Provider` (Anthropic/OpenAI/Gemini). If the session has no
  resolvable Provider, unknown keys go to a `ProviderTag::Unknown
  { bag: StructuredProviderExtension }` — which does NOT exist on the
  core type today (only on wire). **Wave-c must add it on core** or
  drop unknown v0 keys with a `tracing::warn!`; I recommend the
  former so nothing is lost silently.
- Fallback for fully unrecognizable value (non-object, e.g. a bare
  string): log + `None`, mark session with metadata flag
  `provider_params_v0_drop = true`, retain original under
  `legacy_provider_params_v0: Value`.
- Fails to map: no. Worst case is a provider-tag dump.

### 3.2 `SessionMetadata.connection_ref`
- v0 inner: `{realm_id: String, binding_id: String, profile:
  Option<String>}`.
- v1 inner: `{realm: RealmId, binding: BindingId, profile:
  Option<ProfileId>}` where each newtype validates a slug regex
  (`connection.rs:86-90`; tests at `connection.rs:610-616` reject
  empty / `bad space` / `bad:colon`).
- Mapping: read v0 field names, call `RealmId::parse` /
  `BindingId::parse` / `ProfileId::parse`.
- Ambiguity: v0 `realm_id` could legally contain characters that v1
  slug validation rejects. Mitigation: on parse failure, slugify
  (lowercase, `[^a-z0-9_.-]` → `_`) and retain original under
  `legacy_connection_ref: {realm_id, binding_id, profile}` — mark
  session as needing operator review. Do NOT silently drop the
  connection: a resume that lands on the wrong realm would cause
  credential bleed (`session.rs:908-916` explicitly calls this out).
- Fail-to-map: if after slugification the newtype still refuses, the
  session is loaded with `connection_ref = None` and will re-resolve
  against env defaults on resume, which is the pre-existing
  behavior for sessions that never had a realm.

### 3.3 `RuntimeTurnMetadata` (inside `StoredInputState.persisted_input`)
- `provider_params`: same rule as §3.1.
- `model: Option<String>` → `Option<ModelId>`: `ModelId::new(s)`;
  on slug failure, keep as `Unknown("{raw}")` via a new ModelId
  escape hatch, OR drop to `None` with trace. Preferred: drop to
  `None` — the turn was already admitted on v0 and its model is
  effectively frozen in the conversation history; the override
  only mattered for a *future* retry.
- `provider: Option<String>` → `Option<Provider>`: use
  `Provider::parse_strict` (existed pre-wave-b). Unknown → `None` +
  trace. Hardest case: gemini-preview strings that worked pre-strict.
  Tracked in fixture 5.4.
- `additional_instructions: Option<Vec<String>>` →
  `Option<Vec<TurnInstruction>>`: lift each `String` to
  `TurnInstruction { kind: TurnInstructionKind::AppendUser, body:
  s }` (the default kind — confirm which kind in wave-c; v0
  semantics were "append as additional user/system guidance"; the
  conservative choice is whichever kind the active code path
  treated a bare string as).
- NEW fields `connection_ref`, `keep_alive`: absent in v0 JSON;
  default `None` via `#[serde(default)]`. No migration needed.

### 3.4 `ConnectionRef` inside `RuntimeTurnMetadata` on v0
- Not applicable — v0 `RuntimeTurnMetadata` did not carry
  `connection_ref` (it was introduced in B-6). v0 rows deserialize
  with the new `Option<ConnectionRef>` field as `None` via
  `serde(default)`.

### 3.5 Extra / unknown fields
All serde helpers use implicit `#[serde(deny_unknown_fields)]`?
No — `InputStateSerde` and `SessionMetadata` do **not** set it. Extra
fields are silently dropped on read. Wave-c will NOT flip this; the
explicit migration envelope handles known-delta fields and leaves the
rest to serde's lenient default.

### 3.6 Sessions that fail to map
A v0 session whose `provider` is an unreachable string (e.g. the old
`perplexity` provider, deleted in a prior wave) cannot re-resolve on
resume. Migration leaves it as `None`; the session-factory resume
path already surfaces "provider missing" as a typed error, which is
the correct operator signal. No session is outright discarded by the
migrator.

## 4. Write-side strategy

**Pick: opportunistic upgrade on read — migrate-in-memory on load,
rewrite as v1 on next successful `save()` / `atomic_apply()`.**

Rationale:
- Single write path (`sqlite_store.rs:71-108`,
  `runtime/store/sqlite.rs:127-150`) already upserts on every session
  save, so the v0 row naturally flips to v1 the first time the session
  does anything. No background sweep required.
- A "migrate lazily forever" stance pollutes the read path with a v0
  fallback that must stay correct across future waves; eventually
  wave-e or wave-f wants to assume v1-only to simplify review.
- A "migrate-all-on-boot" stance forces a maintenance window and blocks
  `rkat` startup on a potentially-large table scan.

Consequence: after wave-c ships, every session that has run at least
one turn is v1. A session that was archived pre-wave-b and never
re-opened stays v0 in the table — that is fine; it migrates the next
time it is loaded, or never, if the operator deletes it.

A single diagnostic CLI (`rkat debug migrate-sessions`, wave-c stretch)
loads + saves every row in one pass for operators who want a clean
snapshot.

## 5. Fixture matrix

Location: `meerkat-session/tests/fixtures/pre_wave_b/`. Each fixture
is a literal JSON byte file (not a helper) so helper drift does not
mask regressions. Loaded by a new
`meerkat-session/tests/persistence_compat.rs`.

Session-blob fixtures:
1. `session_empty_metadata.json` — no `session_metadata` key. Typed
   migration must be a no-op; guards against over-eager rewrite.
2. `session_provider_params_openai.json` — `provider_params: {temperature: 0.2,
   reasoning: "silent", encrypted_content: "..."}`. Expect
   `temperature=Some(0.2)`, `reasoning=Some(Silent)`,
   `provider_tag=Some(ProviderTag::OpenAi{encrypted_content,...})`.
3. `session_provider_params_anthropic_signature.json` — Anthropic
   `signature` untyped key → `ProviderTag::Anthropic{signature}`.
4. `session_provider_params_anthropic_thinking.json` — production
   shape `{thinking: {type:"enabled", budget_tokens:32000}}`. The
   variant most likely to be silently dropped; must land under
   `provider_tag.extension` without loss (see risk 4).
5. `session_provider_params_unknown.json` — non-object value (numeric).
   Expect `None` + legacy value retained under `legacy_provider_params_v0`.
6. `session_connection_ref_slug_valid.json` — `{realm_id:"dev",
   binding_id:"default_openai", profile:null}`; clean typed parse.
7. `session_connection_ref_slug_invalid.json` — `realm_id:"dev mode"`;
   slugified to `dev_mode` + legacy retention.
8. `session_hot_swap_identity_mixed.json` — `SessionLlmIdentity`
   v0 shape but `SessionMetadata` v1 shape (inconsistent mid-flash).
   Both independently migrated; no cross-contamination.

Runtime-store fixtures:
9. `input_state_prompt_full_turn_metadata.json` — `StoredInputState`
   whose `persisted_input.Prompt.turn_metadata` has v0 `provider_params`,
   v0 `additional_instructions: ["foo","bar"]`, v0 `model: "gpt-4o-mini"`.
   Expect typed `TurnInstruction` list + best-effort `ModelId`.
10. `input_state_continuation_minimal.json` — no turn metadata at all;
    verifies `serde(default)` for the NEW `connection_ref`/`keep_alive`
    fields on pre-B6 rows.
11. `input_state_provider_unknown_string.json` — retired provider
    string; `provider=None`, session still loadable.
12. `runtime_session_snapshot_drift.json` — a
    `runtime_session_snapshots` row that drifts from the `sessions`
    row (crash-recovery scenario, `persistent.rs:406-417`). Migration
    picks the newer `updated_at` and migrates the winner only.

## 6. Crates touched

| Crate | Migration-adjacent paths | Wave-b ref |
|---|---|---|
| `meerkat-core` | `session.rs` SessionMetadata/SessionLlmIdentity serde; `connection.rs` RealmId/BindingId/ProfileId newtypes; `lifecycle/run_primitive.rs` RuntimeTurnMetadata V3 + ProviderParamsOverride | `197a70b4a`, `0cd3e4430`, `18e12d3a4`, `cf90208b7` |
| `meerkat-contracts` | `wire/runtime.rs` WireInputState typed fields + StructuredProviderExtension; `wire/connection.rs` WireConnectionRef | `5fb027af1`, `974e81799`, `7eeaf66ac` |
| `meerkat-runtime` | `input_state.rs` InputStateSerde bump + default; `input.rs` turn_metadata field; `store/sqlite.rs` read path | (wave-c only) |
| `meerkat-store` | `sqlite_store.rs` session_json read path — delegates to core Session serde | (wave-c only) |
| `meerkat-session` | `persistent.rs` (new `migrations` submodule); `tests/persistence_compat.rs` (new) | (wave-c only) |

Wave-d SDK regen (`make regen-schemas`) is triggered by the
`meerkat-contracts` movement of `ConnectionRef` to typed newtypes,
not by the persistence-migration work itself. Python/TypeScript SDKs
re-emit from the wire types and do not see the persistence envelope.

## 7. Risk register

1. **v0 row in prod that can't be migrated.** Symptom: load returns
   `SessionError::Agent(InternalError)` from the serde bail at
   `persistent.rs:394-400` / `395` and `persistent.rs:739`.
   Catching assertion: compat test 11 (unknown provider string) + an
   integration test that loads every fixture file and asserts
   `load()` is `Ok(Some(_))`. Belt-and-braces: wrap the v0→v1
   translation in `fn migrate(value: Value) -> Result<Session,
   SessionMigrationError>` and emit `SessionMigrationError::Partial`
   with the retained legacy payload instead of ever returning `Err`
   from `load()`. A partially-migrated session is always better than
   an unloadable one.

2. **Roll-back after v1 writes.** A v1.5 deployment is rolled back to
   v0 binary; the SQLite rows are v1. v0 serde will reject v1
   `provider_params: {kind: "openai", ...}` on the
   `SessionMetadata.provider_params: Value` field because it is a
   Value — actually it's fine, serde_json::Value accepts any JSON,
   and `default_version()` hides the v2 marker. The subtle failure is
   `RealmId`/`BindingId` newtypes being structural, so v0 binaries
   that expect `{realm_id, binding_id}` choke on v1 `{realm,
   binding}`. Catching assertion: a `cargo test -p meerkat-store
   --test rollback_v1_to_v0` that serialises a v1 Session, strips
   the migrator, and attempts v0 deserialisation — expect loud
   failure, not silent corruption. Operator guidance: no rollback
   across wave-c without `rkat debug export-sessions` first.

3. **Shared deployment running mixed v0/v1 readers.** Two
   `SqliteSessionStore` handles against the same DB (e.g. CLI
   process + RPC daemon), one pre-wave-c, one post. The v1 writer
   upgrades on save; the v0 reader then hits case 2. Same catching
   test. Mitigation: wave-c release notes require all `rkat*`
   binaries to upgrade in lockstep; enforce by stamping the DB with a
   schema-header row (`PRAGMA user_version = 2`) — but this requires
   a DDL bump and is out of scope if the row-level versioning is
   adequate. Recommended: add `user_version = 2` PRAGMA in wave-c as
   belt-and-braces; old binaries open the file and log an explicit
   "DB is newer than this binary" rather than hitting a parse error
   deep inside serde.

4. **Fixture miss most likely to bite.** The v0 `provider_params`
   variant used by a real-world `meerkat-mob` member whose owner
   typed raw Anthropic extended-thinking overrides. The production
   shape is `{thinking: {type: "enabled", budget_tokens: 32000}}`,
   not the simpler `thinking_budget_tokens: u32` v1 promises.
   Catching assertion: fixture 3 plus a dedicated mob-spawn
   integration test `test_v0_mob_member_with_anthropic_thinking` that
   replays a real production payload captured from `meerkat-mob`
   tests. Without it, we ship a silently-dropped field.

5. **`realm_id` vs `connection_ref.realm` double-write drift.**
   `SessionMetadata.realm_id: Option<String>` (session.rs:871) and
   `SessionMetadata.connection_ref.realm: RealmId` are redundant
   post-wave-b. A v0 session with `realm_id = "dev"` and no
   `connection_ref` must derive `connection_ref.realm` from it — or
   every resume lands on env defaults. Catching assertion: compat
   test that asserts post-load `connection_ref.realm.as_str() ==
   session_metadata.realm_id.unwrap()` when both exist, and
   post-load `connection_ref = Some(_)` when only `realm_id` was
   present on v0. Wave-c cleanup candidate: delete `realm_id` as a
   top-level field in a follow-up wave.

---

Summary: persistence-layer migration is localized to two JSON blob
surfaces (`sessions.session_json` and `runtime_input_states.state_json`),
both already carry version bytes (or can be given one via a
`#[serde(default)]` field), and the retype damage is mechanical in
every dimension except `provider_params`, which requires a provider-aware
unknown-keys promotion path. Opportunistic rewrite on next save keeps
the code path single-file and avoids a boot-time migration sweep.
