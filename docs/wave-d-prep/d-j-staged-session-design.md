# D-j design note — PendingSession lifecycle → canonical owner

Context: `meerkat-rpc/src/session_runtime.rs` holds an RPC-local
`PendingSession` + `PendingSessionPhase::{Staged, Promoting}` two-phase
lifecycle for sessions that have been created (ID returned) but not yet
materialized in the service. Ten branching sites read `.phase` inside
the RPC surface. The surface is currently the authority for "staged but
not materialized" — a Dogma #17 + #3 violation (transport skin owns
semantic state; duplicate authority path alongside `SessionService`).

The task contract offers two closures.

## Option A — `MeerkatMachine` grows a `Staged` phase

Shape: DSL adds a `Staged` variant to `MeerkatPhase` (or a parallel
per-session discriminant). RPC calls `machine.stage(session_id, build_config)`
then `machine.promote(session_id)` to advance to `Attached` / `Running`.

Problem: the thing a staged session carries is an `AgentBuildConfig`
bundle — `Arc<dyn AgentToolDispatcher>`, `Arc<dyn LlmClient>`, the
attached `McpRouterAdapter`, `SessionRuntimeBindings` from
`MeerkatMachine::prepare_bindings`, tokio channel senders, and a live
`Session` with the claimed ID already embedded. None of that is
representable as DSL state: the DSL is a pure value machine (TLC
verifiable), and the build bundle is full of non-`Serialize` Arcs and
live handles.

The honest shape of Option A is therefore:

- DSL owns a `Staged | Promoting | Attached` discriminant, plus the
  `SessionId` set.
- Shell holds a side map `SessionId → AgentBuildConfig` indexed by
  staged session — same `IndexMap<SessionId, _>` the RPC surface
  already has, renamed.

That is the same split-ownership anti-pattern the task targets,
relocated: DSL gates transitions, shell holds the real payload, the
two must stay in lock-step. The concurrency guard the
`Staged → Promoting` flip currently provides (`SESSION_BUSY` on
reentrant promotion) would cross the DSL-shell boundary on every
materialization — not free.

Option A would also cascade schema regen and a workspace codegen
rebuild for a change whose on-disk truth is still an `Arc`-heavy bundle
the DSL can't model.

## Option B — `SessionService` owns `stage / promote / abandon_staged`

Shape: the canonical `SessionService` trait grows three typed methods:

```rust
async fn stage_session(&self, req: StageSessionRequest) -> Result<SessionId, SessionError>;
async fn promote_session(&self, id: &SessionId, req: PromoteSessionRequest) -> Result<RunResult, SessionError>;
async fn abandon_staged(&self, id: &SessionId) -> Result<(), SessionError>;
```

`StageSessionRequest` carries the build data (model, prompt, system
prompt, labels, deferred prompt policy, `SessionBuildOptions`, optional
pre-allocated `SessionId`), exactly what the RPC surface currently
stores in `PendingSession`. `PromoteSessionRequest` carries the first
turn's prompt and overrides. `abandon_staged` is the no-turn exit path.

`PersistentSessionService` owns the staged map directly — a private
`DashMap<SessionId, StagedSlot>` next to its existing live-session
storage. Concurrency gate (`SESSION_BUSY` on concurrent promote) is
enforced inside the service, not by the transport. List/read/archive
see staged sessions via the same mechanism that exposes live sessions.

Why this is right for Meerkat:

- `SessionService` is **not** a projection layer. It is the canonical
  lifecycle authority for session existence: `create_session`,
  `archive`, `interrupt`. Adding a pre-materialization state alongside
  "live session" is extending the existing authority, not creating a
  new one.

- `MeerkatMachine` is a **per-session runtime-behavior machine**. Its
  own `Initializing → Idle → Attached → Running` lifecycle begins
  with `RegisterSession`. "Before `RegisterSession` has run" is
  pre-DSL-existence — conceptually outside the DSL's domain. The
  service layer is where "session is named but not yet materialized"
  lives naturally.

- The build bundle stays where it belongs (live Arc references in the
  service layer), with no DSL-shell shadow pair.

- The catching assertion from the task (`rg 'PendingSession' meerkat-rpc/src/`
  zero production hits; canonical owner holds the staged phase) is
  fully satisfied: after closure the RPC surface has no local staged
  storage and no `.phase`-branching code paths — every entry point
  flows through `SessionService::{stage_session,promote_session,abandon_staged}`
  or the existing `start_turn` / `archive`.

## Chosen path: B

Implementation plan (one landing, architectural-prerequisite widen-scope rule):

1. `meerkat-core/src/service/mod.rs` — add `StageSessionRequest`,
   `PromoteSessionRequest`, trait methods on `SessionService` (default
   `Unsupported`), and a `SessionStagingState` read helper for `list /
   read` to surface staged sessions.
2. `meerkat-session/src/persistent.rs` — `PersistentSessionService`
   implements the three methods. Internal map is the new authoritative
   home for staged sessions; `list` / `read` / `archive` extend to see
   them; the internal materialization path that `start_turn` already
   uses for the first-turn-on-created-session case routes through the
   same helpers.
3. `meerkat-rpc/src/session_runtime.rs` — delete `PendingSession`,
   `PendingSessionPhase`, and the `pending: RwLock<IndexMap<_, _>>`
   field. Every callsite (session/create, start_turn's first-turn
   branch, append_system_context, list_sessions, read_session_rich,
   archive_session, restore_pending_from_promoting, recovery) routes
   through the new `SessionService` methods. `restore_pending_from_promoting`
   becomes unnecessary — rollback on override-validation failure is a
   `stage_session` after the service-side promote failed.
4. Tests in `meerkat-session` and `meerkat-rpc`:
   - staged → promote round-trip exercises the service authority
   - staged → abandon without promote
   - staged → promote concurrency gate returns `Busy`
   - `append_system_context` on a staged session mutates staged state
   - `list` / `read` see staged sessions
5. Catching assertion: `rg 'PendingSession|PendingSessionPhase' meerkat-rpc/src/`
   returns zero hits in production code. Service owns the staged phase
   canonically.

No DSL schema change, no codegen regen — the staged phase is a service
concept, not a machine-level fact, and it belongs in the canonical
session-lifecycle authority that already owns `create_session /
archive / interrupt`.
