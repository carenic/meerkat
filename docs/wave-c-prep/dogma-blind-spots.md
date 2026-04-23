# Dogma Catalog Blind Spots — Wave-C Prep

**Scope.** The 70-item catalog at `~/.codex/dogma-violations.md` is focused on
semantic-state-ownership: stringly typing, shadow state, duplicate authority,
name-keyed routing, surface-level mutation, folklore comments. These don't exhaust
the production-bug surface. This doc enumerates **classes of failure modes the
catalog cannot see**, with concrete sites and enforcement paths. Seeds issue #341.

Line citations: worktree `/Users/luka/src/meerkat/.claude/worktrees/wave-a-demo`,
branch `dogma/wave-a-demolition`.

---

## 1. Async cancellation races

**Def.** Futures dropped mid-transition leave state partially applied. A `select!`
arm wins over a half-written row; a spawned task runs on after its owner; an `.await`
between two state updates that the supervising future never reaches.

**Why catalog misses it.** Catalog reasons about field values, not about which future
owns the continuation. Cancel-safety is a property of the *code path*, not the machine.

**Sample sites.**
- `meerkat-session/src/ephemeral.rs:2548-2572` — top-level `select!` races `run_fut`
  vs `interrupt_wait` vs `agent_event_rx.recv()`. `run_fut` mutates agent state; on
  interrupt, dropped mid-turn. Blast radius = whole agent loop; no proof every inner
  `await` is cancel-safe.
- `meerkat-mob/src/runtime/actor.rs:3320` — `spawn_fut.catch_unwind().await` converts
  panics to `MobError::Internal`, but outer actor cancellation mid-spawn orphans
  `spawn_ticket` with no `SpawnProvisioned` reply.
- `meerkat-mob/src/runtime/actor.rs:6102` — flow-stream `select!`; on actor shutdown
  the completion signal is lost.
- `meerkat-rpc/src/realtime_ws.rs:2891` — realtime WS `select!` over DSL + audio +
  keepalive; comment at line 1789 explicitly flags "arms cannot carry state" —
  cancellation semantics load-bearing and undermodeled.
- `meerkat-anthropic/src/client.rs:966` — streaming `while let Some(chunk) = stream.next().await`;
  drop mid-SSE leaves partial buffer, retry replay hazard.

**Severity.** **High.** Interrupt/cancel is exercised on every user `^C`; persistent
agents add partial-write failure modes that no unit test covers.

**Enforcement path.** Annotate every `async fn` that mutates durable state with a
`#[cancel_safe]` or `#[cancel_not_safe]` attribute; a compile-time lint (custom clippy
or `xtask` AST walker) rejects `select!`/`spawn` arms that drop a `#[cancel_not_safe]`
future without an explicit `rollback` or `abort_handle` pairing. Cheaper first step:
land a `CancellationContext` newtype threaded through every multi-step transition, with
Drop-guarded compensation actions.

---

## 2. Resource lifetime / leaks

**Def.** Who holds the last `Arc`, who calls `abort()`, when does the TCP socket actually
close. Not the logical state of the session, but the OS / allocator liveness of its
backing resources.

**Why catalog misses it.** Ownership graphs are orthogonal to state-machine semantics;
dogma checks run on authority writes, not on `Drop` ordering.

**Sample sites.**
- `meerkat-cli/src/main.rs:140-142` — `TuiPipeline { verbose_task, stream_task,
  primary_to_scoped_bridge_task }` holds three `Option<JoinHandle<_>>`. `abort()` at 5452
  is called on one branch; nothing proves the other two handles are awaited or aborted
  on every exit path. Orphaned handles keep `Arc`s alive past session end.
- `meerkat-rest/src/lib.rs:2169-2195` — `drain_event_forwarder` expects a handle handed
  off by `spawn_event_forwarder`; 4-line window between spawn and drain during which
  panic leaks the task.
- `meerkat-store/src/realm.rs:114-214` — `join: Option<JoinHandle<()>>` on the realm
  lease heartbeat; abort is only called in `stop()`, which is optional, so forgetting to
  call it is a silent leak of a task doing SQLite writes.
- 130 `Arc<Mutex<...>>` / `Arc<RwLock<...>>` fields workspace-wide — none flagged for
  whether the contained value needs explicit `Drop` (e.g., closing DB connections,
  flushing WAL).
- `meerkat-comms/src/inproc.rs:108+` — the `InprocRegistry` `static` global never
  unregisters dropped senders without explicit `unregister()` calls; reinforced by the
  17 `pub fn` in the file with no ownership contract.

**Severity.** **Medium–High.** Single agent: cosmetic. Long-lived RPC/REST server with
thousands of sessions: memory growth within hours.

**Enforcement path.** `meerkat-core` trait `ResourceOwned` with `async fn shutdown(&mut
self) -> Result<(), ShutdownError>`; xtask lint rejects any struct holding `JoinHandle`,
`broadcast::Sender`, or `tokio::sync::Notify` without either implementing `ResourceOwned`
or carrying an explicit `#[leak_ok = "reason"]` doc attribute. Pair with
`#[must_use = "task handle must be awaited or aborted"]` on the wrapper.

---

## 3. Timing / duration violations

**Def.** State machines carry no model of *how long* a transition takes. A `WaitingForLlm`
state with a 300s timeout is indistinguishable from one with 30s until a user waits
too long and complains.

**Why catalog misses it.** Durations don't appear in machine DSL; the DSL models order,
not wall-clock.

**Sample sites.**
- `meerkat-anthropic/src/client.rs:19` — `DEFAULT_REQUEST_TIMEOUT: Duration = 300s`;
  hardcoded, not surfaced in `Config`. Users who legitimately need 600s silently fail.
- `meerkat-anthropic/src/client.rs:1483-1484` — stub-server tests hardcode 5s connect /
  120s request; no relationship to production values.
- `meerkat-store/src/realm.rs:89-91` — lease-stale threshold 30s, retry delay 20ms,
  overall timeout 5s. Three magic durations, no test that relates them.
- `meerkat-store/src/realm.rs:612,952,1043` — three separate 2s-deadline + 10ms-sleep
  busy-wait loops, each copy-pasted. No shared budget type.
- `meerkat-session/src/ephemeral.rs:45` — `EVENT_CHANNEL_CAPACITY: usize = 256`; used for
  both mpsc and broadcast. Slow consumer on broadcast trips `Lagged` — a timing failure
  mode the agent loop doesn't distinguish from a network stall.

**Severity.** **Medium.** Causes user-visible flakiness rather than data loss, but
flakiness is the #1 reported issue across LLM platforms in 2026.

**Enforcement path.** Central `TimingProfile` struct consumed at every `.timeout()`
call; compile-time lint rejects literal `Duration::from_*` outside `timing.rs`.
Pair with machine-DSL `deadline_within: Duration` on long-running state annotations so
TLC can express "WaitingForLlm ≤ 300s ⇒ eventual transition."

---

## 4. Serialization skew / schema drift

**Def.** Persisted shape changes (new field, renamed variant) but a consumer on disk
or on wire reads the old format. Machine state looks right in memory; projector or
downgrade path silently corrupts.

**Why catalog misses it.** Catalog enforces *current* schema; it doesn't model the
set of prior schemas in flight.

**Sample sites.**
- `meerkat-session/src/event_store.rs:25` — `EVENT_SCHEMA_VERSION: u32 = 1`. Never bumped,
  with no test case for v0→v1. If bumped silently to 2, all existing `.rkat/` sessions
  become unreadable.
- `meerkat-mob/src/event.rs:218,529` — mob event schema at v6 with `bail!` on mismatch.
  No migration path on disk; v5 session files are unrecoverable after upgrade.
- `meerkat-mob/src/event.rs:921` — a test writes `schema_version + 1` to assert rejection,
  confirming the policy is "refuse unknown," not "migrate." That's a legitimate choice,
  but nothing surfaces it to users.
- `meerkat-contracts/src/wire/session.rs:362` — `args: serde_json::from_str(args.get())
  .unwrap_or(Value::Null)` silently downgrades malformed tool args to null. A schema
  change to tool-args shape that passes provider validation but fails wire parsing
  becomes `null` at the boundary — indistinguishable from "tool called with no args."
- `meerkat-rpc/src/handlers/session.rs:61-102` — 15 consecutive `#[serde(default)]` on
  `CreateSessionRequest` fields: adding a field is safe, but removing one is silently
  accepted from stale clients and treated as default.

**Severity.** **High.** Data loss risk on upgrade; silent-default risk on every wire.

**Enforcement path.** Ship a `schema-freshness` CI gate (partially exists — extend to
each versioned struct). Require every `#[serde(default)]` to reference a default-value
const with a doc explaining why "missing" ≠ "zero." Add a proptest per versioned
envelope that fuzzes v0..=vN payloads and asserts either accept+upgrade or reject.

---

## 5. Backpressure and memory pressure

**Def.** Channels without bounds, Vecs without caps, broadcast slow consumers. The
machine's transitions all succeed; the process OOMs.

**Sample sites.**
- `meerkat-rest/src/lib.rs:2362` + `5771`, `meerkat-rpc/src/session_runtime.rs:3741` —
  three `mpsc::unbounded_channel()` calls in production paths. Any peer that never
  drains grows the queue until the process dies.
- `meerkat-session/src/ephemeral.rs:1487` — broadcast of 256 `EventEnvelope` capacity.
  A single slow web client causes `Lagged` — currently logged and continued (2568),
  but events for that subscriber are silently dropped with no error to the agent loop.
- `meerkat-session/src/ephemeral.rs:2316,2343` — `blocks.extend(turn.into_blocks())`
  appends unbounded to session history. Compaction is the only backpressure; if
  compactor is disabled (supported configuration), history grows forever.
- `meerkat/src/service_factory.rs:778,868,874,890,975,980,1178,1184,1190,1209,1215,1233`
  — 13 sites of `mpsc::channel(8)` with receiver immediately dropped (`_rx`). These
  are test harnesses, but the pattern (bound 8, drop rx) would deadlock in production.

**Severity.** **High** for unbounded channels in persistent surfaces.

**Enforcement path.** Ban `unbounded_channel` via clippy lint
(`disallowed_methods`); require every `mpsc::channel(N)` to cite a `Capacity` doc-const
with rationale. Add `cargo xtask audit-queues` that enumerates every channel and its
documented backpressure strategy.

---

## 6. External state drift

**Def.** Machine says session is alive; provider evicted it (Anthropic 30-day expiry,
OpenAI response store eviction, OAuth refresh invalidation). Machine's invariants hold;
the world's do not.

**Why catalog misses it.** Catalog enforces internal coherence; external drift is by
definition unobservable until next call.

**Sample sites.**
- `meerkat-anthropic/src/client.rs:17` — pool idle timeout 90s, but `reqwest` connection
  pool doesn't inform the state machine when it discards an idle connection. Next
  request silently reconnects; machine thinks it's the same session.
- `meerkat-core/src/error.rs:327-332,433` — `retry_after` is parsed from provider
  response but the machine has no `WaitingForRetryAfter` state; the delay is applied
  in the retry layer, not modeled.
- `meerkat-store/src/realm.rs:89` — 30s lease stale threshold: a remote instance that
  pauses (GC, fork) for 31s loses its lease without local state reflecting it until
  next refresh.
- `meerkat-openai/src/live.rs:2069` — `conversation_id: None` means we never thread the
  server-side conversation ref; any future OpenAI change to require it would silently
  start a fresh context.

**Severity.** **Medium.** Rare under happy-path traffic; 100% reproducible on long-idle
sessions, quota changes, or provider rotations.

**Enforcement path.** Every external-state-dependent transition must declare
`liveness_probe: fn() -> Result<Fresh, Stale>` and run it before trusting machine state
on cold-start. Model "Stale" as an explicit variant, not an error.

---

## 7. Security boundary holes

**Def.** Authz check elided, `pub` exposes an internal, TOCTOU between signature verify
and trust lookup, privilege escalation via omitted `require_peer_auth`.

**Sample sites.**
- `meerkat-comms/src/io_task.rs:39,63` — `require_peer_auth: bool` is a *parameter*;
  caller must remember to pass `true`. The default is inherited from caller context;
  no compile-time guarantee the production path sets it. See
  `meerkat-comms/src/inbox.rs:505` for one of two enforcement points.
- `meerkat-comms/src/inbox.rs:483 vs 505` — TOCTOU: line 483 reads `trusted_peers` under
  `queue.lock()`; line 505 re-reads `trusted_peers.read()` separately. If trust is
  revoked between, enqueue succeeds but dequeue rejects — state for the trust set
  diverges from envelope acceptance.
- `meerkat-comms/src/inproc.rs:108+` — 17 `pub fn` on a global `InprocRegistry`; any
  caller in the workspace can `register_with_meta_in_namespace` impersonating another
  agent's pubkey. No auth on the registration side.
- `meerkat-contracts/src/wire/session.rs:362` — `unwrap_or(Value::Null)` on tool args
  means a malformed, *possibly adversarial* payload becomes a no-args call — could
  bypass tool-input validation that depends on required fields.
- 6769 `.unwrap()`/`.expect()` occurrences workspace-wide outside tests — every one is
  a potential remote DOS vector if a hostile input reaches it.

**Severity.** **High** for the TOCTOU + the 6769-unwrap count; **Medium** for the
ambient `pub` surface.

**Enforcement path.** Promote `require_peer_auth` from parameter to type-state (compile
error if a bridge is constructed without it in non-test code). Ban `unwrap`/`expect`
in non-test code via clippy-workspace rule; convert current sites with `xtask bulk-fix`.
Wrap `InprocRegistry` mutation in a token that only surface construction holds.

---

## 8. Byzantine / adversarial inputs passing spec validation

**Def.** Inputs that the spec accepts as well-formed but that violate an invariant the
machine relies on: overlong strings, pathological nesting, Unicode confusables, huge
numbers, JSON duplicate keys.

**Sample sites.**
- `meerkat-session/src/ephemeral.rs` — `blocks.extend(...)` accepts arbitrary count of
  blocks per turn. A 100k-block turn from a runaway provider bloats memory; machine
  has no per-turn bound.
- `meerkat-contracts/src/wire/session.rs:815+` — `WireContentBlock` parsers round-trip
  arbitrary JSON `Value`; no size limit on raw-value payloads in tool args.
- `meerkat-comms/src/io_task.rs` — envelope size only bounded by the transport; signed
  envelope with 1GB body passes `verify()` before any size check.
- Skill names, tool names, mob-member names: no workspace-wide Unicode-normalization
  pass; name-keyed lookups (catalog violation #14 adjacent) also vulnerable to
  NFKC/NFC confusables.

**Severity.** **Medium.** Requires adversarial provider or peer; low in single-tenant,
high in mob-federated deployments.

**Enforcement path.** `meerkat-contracts` gains a `BoundedString<MAX>` and
`BoundedBytes<MAX>` newtype; deserialization enforces. Every envelope / wire type
annotated with its size bound. Add fuzz targets to `xtask fuzz`.

---

## 9. Panic paths (transition body panics)

**Def.** A transition panics; the panic is caught (`catch_unwind`) but the machine's
in-memory state is now past the pre-state and before the post-state. Future transitions
read corrupt state.

**Sample sites.**
- `meerkat-mob/src/runtime/actor.rs:3320` — `spawn.catch_unwind()` converts panic to
  `MobError::Internal`, but any partial mutation before the panic inside the spawn
  closure is preserved. No rollback.
- `meerkat-mob/src/runtime/actor_turn_executor.rs:269` — `reconcile.catch_unwind()`;
  same pattern. Reconcile may have half-applied to session state.
- `meerkat-core/src/model_profile/capabilities.rs:222` +
  `model_profile/schema_builder.rs:323,344` — 3 `unwrap_or_else(|| panic!(...))` in
  what looks like startup-only code but compiles into library surface; a misconfigured
  runtime panics at first model lookup.

**Severity.** **Medium.** Rust panics are rare; when they occur mid-transition, the
failure is hard to reproduce.

**Enforcement path.** Wrap every transition body in a `TransitionGuard` that snapshots
pre-state, runs body, and on `catch_unwind` *restores* pre-state before propagating
the error — not just converting to `Internal`.

---

## 10. Cascading failure without composition spec coverage

**Def.** Composition spec (closed-world, per CLAUDE.md) verifies declared routes, but
failure *propagation* paths aren't declared. When route X fails, does Y retry, back
off, die, or silently accept the void?

**Why catalog misses it.** Catalog verifies routes exist, not that failure responses
are exhaustive.

**Sample sites.**
- `meerkat-mob/src/runtime/flow_frame_engine.rs:314-337` — `for _ in 0..=max_retries`
  with `CAS exhausted` `bail!`. No spec states what happens after bail — does the
  flow retry, mark failed, or cascade to parent?
- `meerkat-rpc/src/session_runtime.rs:3741` — unbounded lifecycle-channel drop
  triggers nothing; downstream assumes liveness.
- `meerkat-session/src/ephemeral.rs:2568` — `event_stream_open = false` on sink drop;
  event is logged and **ignored**. No spec line states "lost events on sink drop are
  acceptable."

**Severity.** **Medium.** Failure modes work *individually*; nothing proves they
compose.

**Enforcement path.** Extend composition spec to require every `Result::Err` arm at a
seam to declare a `FailureRoute` (retry / give-up / escalate); TLC verifies that every
route is declared and that the transitive fault graph has no silent absorbers.

---

## 11. (New) Silent test-mode divergence

Found while scanning: several production code paths have test-only behavior gated on
features or on `#[cfg(test)]` that changes semantics.

**Sample sites.**
- `meerkat-anthropic/src/client.rs:1483-1484` — test-only timeouts 5s/120s, vs 30s/300s
  in production. Tests pass at 5s; prod fails at 5s; nobody notices until deploy.
- `meerkat/src/service_factory.rs:778-1233` — 13 `_rx` drops; harness code exercises
  a path production will never hit.

**Severity.** **Low-Medium.** Works as intended; the risk is false confidence from
green tests.

**Enforcement path.** Per-module annotation `#![cfg_attr(test, production_divergence)]`
with doc requirement — flags reviewer attention.

---

## Overlap with wave-b

| Class | Wave-B coverage |
|---|---|
| 1. Async cancellation races | **None** |
| 2. Resource lifetime | **None** |
| 3. Timing violations | **None** |
| 4. Serialization skew | **Partial** — wave-b tightens wire typing (`ConnectionRef`, `SkillKey`) which reduces parse-silent-fail surface, but doesn't add versioning machinery |
| 5. Backpressure | **None** |
| 6. External state drift | **Partial** — `AuthMachine` per wave-b plan forces auth-lease liveness; doesn't cover provider-session eviction |
| 7. Security boundaries | **Partial** — typed newtypes close accidental-string-mixing; `require_peer_auth` type-state and the 6769 unwraps are untouched |
| 8. Byzantine input | **None** |
| 9. Panic paths | **None** |
| 10. Cascading failure | **None** |
| 11. Test-mode divergence | **None** |

Wave-b is an SOS (State Ownership Sanitization) wave. These classes need Wave-C+.

---

## Top-5 seeds for issue #341 (fault-explicit state machines)

Ranked by production-bug potential × fix tractability:

1. **`meerkat-session/src/ephemeral.rs:2548-2572`** — model the three-way `select!`
   (run/interrupt/event) as explicit states with a `CancellationObligation` on
   `run_fut`. Highest-value single seam: every session cancel goes through here.
2. **`meerkat-mob/src/runtime/actor.rs:3320` + `actor_turn_executor.rs:269`** — pair
   `catch_unwind` with `TransitionGuard::snapshot_and_restore`; teach the mob authority
   the "partial-transition" state.
3. **`meerkat-anthropic/src/client.rs:19` + the 300s/5s/120s cluster** — surface
   `TimingProfile` through config; TLC annotates `WaitingForLlm` with deadline.
4. **`meerkat-rest/src/lib.rs:2362,5771` + `meerkat-rpc/src/session_runtime.rs:3741`** —
   kill the 3 `unbounded_channel` sites; pick `Capacity` constants and document
   overflow policy (drop-oldest / reject / block).
5. **`meerkat-comms/src/io_task.rs:39,63`** — promote `require_peer_auth` from
   parameter to type-state; fixes category 7 TOCTOU + closes the ambient impersonation
   surface on `InprocRegistry`.

These five, executed rigorously, unblock the formal-fault-model pass. Remaining
classes (serialization, byzantine, test-mode) are tractable but lower-leverage for an
initial fault-explicit pass.
