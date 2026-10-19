# Mobs

This is the detailed reference for Meerkat mobs across Rust SDK, CLI, MCP, REST, RPC, Python SDK, and TypeScript SDK.

## Positioning

- Mobs are an optional extension for multi-agent orchestration.
- Base Meerkat workflows remain session/turn centered.
- On CLI, primary mob UX is tool-driven through `run`/`run --resume` with `--tools full` or config `tools.mob_enabled=true`.
- Direct `rkat mob ...` is the helper/artifact operational surface. Lifecycle creation, wiring, and member management use agent `mob_*` tools or RPC `mob/*`.

## Runtime model

Core entities:

- `Mob`: persisted aggregate (definition, members, status, events).
- `Mob member`: spawned runtime participant identified by `agent_identity`.
- `Profile`: role contract (model/tools/skills posture).
- `Wiring`: peer graph edges.
- `Mob event`: append-only lifecycle records.
- `Flow run` (optional): DAG execution record with step and failure ledgers.

Lifecycle:

1. create
2. spawn
3. wire
4. turn and/or flow runs
5. stop/resume/complete
6. destroy

### Member runtime mode (current default)

- Default is `autonomous_host` when `runtime_mode` is omitted.
- `autonomous_host` members are long-lived peers; mob dispatch routes via injector/subscription path.
- `turn_driven` is explicit opt-in; mob dispatch routes via `start_turn`.

Override points:

- profile-level: `[profiles.<name>].runtime_mode`
- spawn-level: `runtime_mode` argument on spawn tool/command

## Definition model

Common sections:

- `[mob]`
- `[profiles.<name>]`
- `[skills.<name>]` (optional)
- `[wiring]`
- `[topology]` (optional)
- `[supervisor]` (optional)
- `[limits]` (optional)
- `[flows.<flow_id>]` (optional)

Important semantics:

- `orchestrator` chooses the orchestration role.
- `external_addressable` gates external turnability.
- `wiring.auto_wire_orchestrator` and `wiring.role_wiring` shape default graph edges.
- topology rules can enforce strict role-level communication policy.
- `runtime_mode` omitted means `autonomous_host` (new default).

## Rust SDK (detailed)

Primary crates:

- `meerkat_mob` for runtime and state.
- `meerkat_mob_mcp` for mob tool dispatcher and in-memory state helper.

### Core Rust types

- `MobDefinition`
- `MobStorage` (SQLite persistent via `SqliteMobStores`, in-memory for tests, WASM and `rkat mob deploy`)
- `MobBuilder`
- `MobHandle`
- `MobSessionService`
- `MobState`
- `MobRun`, `MobRunStatus`
- `FlowRunConfig`

### `MobBuilder` API

- `MobBuilder::new(definition, storage)`
- `MobBuilder::from_mobpack(definition, packed_skills, storage)` — create from mobpack with inline skills
- `MobBuilder::for_resume(storage)`
- `.with_session_service(Arc<dyn MobSessionService>)`
- `.allow_ephemeral_sessions(bool)`
- `.notify_orchestrator_on_resume(bool)` (a notification that fails after the resume returned is recorded as an `OrchestratorResumeNotificationFailed` mob event once its input has failed terminally, after the runtime's bounded retries; one a retry delivers records nothing; the mob keeps running. The event is recorded in process: a crash before the input's terminal loses it)
- `.with_default_llm_client(client)` — override LLM client (primarily for testing)
- `.register_tool_bundle(name, dispatcher)` / `.register_tool_bundle_with_account(name, account, dispatcher)` (states the `OperationAccount` the bundle's operations act with)
- `.with_member_runtime_registration_hook(hook)` (`runtime-adapter`) - the host selects each member's destination and agent mandate. Since 0.9.4 `MemberRuntimeBuildIdentity::registration()` names the exact registration a call is for (a new member's prepared one, or a recovered member's current one): bind decisions to it, not to the session id or labels. The identity is no longer `UnwindSafe`; wrap it in `AssertUnwindSafe` to move it into `catch_unwind`
- `.create().await`
- `.resume().await`

### `MobHandle` API

- inspection: `status()`, `definition()`, `mob_id()`, `roster()`, `list_members()`, `list_all_members()`, `get_member()`, `events()`
- membership: `spawn_spec(spec)`, `spawn_many(specs)`, `retire(identity)`, `respawn(identity)`, `retire_all()`, `set_spawn_policy()` — all identity-keyed via `AgentIdentity`
- graph: `wire()`, `unwire()`
- turns: `member(id)` returns a `MemberHandle` with `send(content, handling_mode)` and `internal_turn(content)`
- lifecycle: `stop()` (returns a `MobStopReport`), `resume()`, `complete()`, `reset()`, `destroy()`, `shutdown()`, `shutdown_with_report(options)`
- flows: `list_flows()`, `run_flow()`, `run_flow_with_stream()`, `flow_status()`, `cancel_flow()`
- subscriptions: `subscribe_agent_events()`, `subscribe_all_agent_events()`, `subscribe_mob_events()`, `subscribe_mob_events_with_config()`
- admission: `close_if_idle()`, `close_admission(reason)`, `admission_closed()`, `observer()`, `acquire_owner()`, `is_public_owner()`
- delivery scope: `capture_member_delivery_scope(identity)`, `submit_work_with_mode_and_delivery_identity_bounded(&scope, ...)`, `recover_bounded_work_at_scope(&scope, ...)`
- held spawns (native, `runtime-adapter`): `spawn_held_within(spec, deadline)` / `spawn_held_until(spec, deadline)`; publish the `HeldSpawn` with `activate()` or abort it with `abort()`

Admission close: every exported `MobHandle` clone shares one public-owner
token. When the last owner (or `acquire_owner()` lease) drops, the mob closes
admission as soon as it is idle; the builder's internal clones, `observer()`,
`read_handle()` and `events()` do not count as owners. A closed mob refuses
spawn, respawn, work submission, flow runs, resume, reset, peer messages,
live opens, wiring and host binds with `MobError::AdmissionClosed { mob_id,
reason }`; stop, shutdown, destroy, retirement, cancellation and reads stay
available, and admitted work is never stopped. Keep a `MobHandle` clone for
as long as the mob must keep admitting work. `close_if_idle()` answers
`Closed`, `AlreadyClosed(reason)` or `Busy(MobBusy)`. Since 0.9.4 every actor
exit closes admission first, with a typed `MobAdmissionCloseReason` (an
earlier close keeps its reason): an admitted Shutdown (`Shutdown`) or destroy
(`Destroyed`; a mob restored with an unfinished destroy starts closed), a
durability fail-stop (`DurabilityFailStop`), a lost actor task (`ActorLost`)
or a composition dispatch failure (`DispatchFailed`), besides `IdleClose`,
`LastOwnerReleased` and `StandDown`. While a Shutdown drains, a request the
close does not refuse gets the retryable `MobError::ShutdownInProgress`
(wire `SESSION_BUSY`); `ActorCommandChannelClosed` means the actor has exited
or is exiting. `MobError` is `#[non_exhaustive]`.

Delivery scope: a host that persists effect-bearing deliveries captures the
member's `MemberDeliveryScope` (runtime incarnation, fence and session; it
serializes with a `version` field), persists it, then submits against it. A
member session that moved under a current fence is refused as
`MobError::StaleDeliveryScope` (wire `STALE_DELIVERY_SCOPE`) with zero effects
and is never retargeted: re-capture the scope. `recover_bounded_work_at_scope`
reads only the scope's session and returns a `ScopedWorkState` (`Absent` is
never retry permission). A member with no session binding has no scope
(`DeliveryScopeUnavailable`, wire `CAPABILITY_UNAVAILABLE`).

Scratch `task_create` / `task_update` / `task_list` / `task_get` are separately
enabled agent tools, not `MobHandle` methods. Durable shared commitments belong
to WorkGraph, not that scratch-task surface.

### Rust example: full lifecycle via `MobBuilder` + `MobHandle`

```rust
use std::sync::Arc;
use meerkat_mob::{
    AgentIdentity, FlowId, MobBuilder, MobDefinition, MobSessionService, MobStorage,
    SpawnMemberSpec,
};

async fn run_mob(
    definition_toml: &str,
    session_service: Arc<dyn MobSessionService>,
) -> Result<(), Box<dyn std::error::Error>> {
    let definition = MobDefinition::from_toml(definition_toml)?;
    let storage = MobStorage::persistent("./mob.db")?; // SQLite/WAL-backed

    let handle = MobBuilder::new(definition, storage)
        .with_session_service(session_service)
        .create()
        .await?;

    handle
        .spawn_spec(SpawnMemberSpec::new("lead", AgentIdentity::from("lead-1")))
        .await?;
    handle
        .spawn_spec(SpawnMemberSpec::new("worker", AgentIdentity::from("worker-1")))
        .await?;
    handle
        .wire(
            AgentIdentity::from("lead-1"),
            AgentIdentity::from("worker-1"),
        )
        .await?;
    handle
        .member(&AgentIdentity::from("lead-1"))
        .await?
        .send(
            "Coordinate a short execution plan.".to_string(),
            meerkat_core::types::HandlingMode::Queue,
        )
        .await?;

    let run_id = handle
        .run_flow(FlowId::from("release_flow"), serde_json::json!({"severity":"critical"}))
        .await?;
    let _run = handle.flow_status(run_id).await?;

    handle.complete().await?;
    Ok(())
}
```

### Rust example: high-level in-memory mob state helper

```rust
use meerkat_mob::{AgentIdentity, MobDefinition, ProfileName};
use meerkat_mob_mcp::MobMcpState;

async fn in_memory() -> Result<(), Box<dyn std::error::Error>> {
    let state = MobMcpState::new_in_memory();
    let definition = MobDefinition::from_toml(r#"
[mob]
id = "my-mob"
orchestrator = "lead"

[profiles.lead]
model = "claude-opus-4-8"
external_addressable = true

[profiles.lead.tools]
builtins = true
comms = true
mob = true

[profiles.worker]
model = "claude-sonnet-4-6"

[profiles.worker.tools]
builtins = true
comms = true
"#)?;
    let mob_id = state.mob_create_definition(definition).await?;

    state
        .mob_spawn(
            &mob_id,
            ProfileName::from("lead"),
            AgentIdentity::from("lead-1"),
            None,
            None,
            None, // no explicit host placement
        )
        .await?;
    let _status = state.mob_status(&mob_id).await?;
    Ok(())
}
```

## Shared integration model (`meerkat-mob-mcp`)

Outside direct `meerkat_mob` usage, agent-facing mob capability is provided by
late-binding `meerkat_mob_mcp::AgentMobToolSurfaceFactory` into
`SessionBuildOptions.mob_tools`. The factory receives the owning session ID,
runtime-injected `MobToolAuthorityContext`, effective authority handle, and
optional comms runtime during `AgentFactory::build_agent()`, then returns the
session-scoped `AgentMobToolSurface` dispatcher.

```rust
use std::sync::Arc;
use meerkat_core::ToolCategoryOverride;
use meerkat_core::service::{MobToolsFactory, SessionBuildOptions};
use meerkat_mob::{MobControlPrincipal, MobSessionService};
use meerkat_mob_mcp::{AgentMobToolSurfaceFactory, MobMcpState};

fn with_mob_tools(
    session_service: Arc<dyn MobSessionService>,
) -> Result<SessionBuildOptions, Box<dyn std::error::Error>> {
    // `new` is fallible (runtime adapter acquisition); `into_shared()` lets the
    // state run its own council recovery sweep.
    let state = MobMcpState::new(session_service, MobControlPrincipal::Owner)?.into_shared();
    let factory: Arc<dyn MobToolsFactory> =
        Arc::new(AgentMobToolSurfaceFactory::new(state));
    let mut build = SessionBuildOptions {
        mob_tools: Some(factory),
        ..Default::default()
    };
    build.apply_generated_create_only_mob_operator_access(ToolCategoryOverride::Enable);
    Ok(build)
}
```

Pass the returned options as `CreateSessionRequest.build` to a factory-backed service.
It supplies both dispatcher infrastructure and explicit per-build enablement;
the factory/runtime handoff mints generated create-only operator authority.
It does not grant access to arbitrary existing mobs. `.mob(true)` alone is
only an ambient factory default; never construct authority or runtime bindings
by hand to compensate for missing composition.

`external_tools` is still the right slot for callback tools and MCP-backed
dispatchers. Mob orchestration uses the separate `mob_tools` slot so operator
authority and session-scoped wiring are injected at build time.

## Surface matrix

| Surface | Mob access | Current behavior |
|---|---|---|
| CLI `run` / `run --resume` | `mob_*` tools in prompt-driven runs when mob tools are enabled | Primary CLI mob UX |
| CLI `rkat mob ...` | helper, artifact, and explicit operator commands | Secondary operational surface, including member-host, grant, observation, and live verbs |
| CLI `rkat mob pack/deploy/web build` | artifact and browser distribution | Portable deploy + web target |
| RPC | explicit `mob/*` methods | canonical typed substrate for SDKs; generic `mob/tools` / `mob/call` escape hatches do not exist |
| REST | session HTTP endpoints plus mob helpers | Existing helper/member routes plus the read-only multi-host observations |
| MCP | `meerkat_*` session tools plus `meerkat_mob_*` public tools, including `meerkat_mob_wait_ready` and profile CRUD | typed public mob control plane for host access |
| Python SDK | `Mob` class via `create_mob()` | first-class mob lifecycle, member mgmt, flow control, event subscriptions |
| TypeScript SDK | `Mob` class via `createMob()` | first-class mob lifecycle, member mgmt, flow control, event subscriptions |
| Web SDK | `Mob` class via `createMob()` | same WASM-backed mob lifecycle with typed `EventSubscription<T>` |

### Multi-host surface matrix

This is the authoritative exposure matrix for the multi-host console family.
The asymmetry is intentional: an absent mutation is a security boundary, not
an SDK backlog.

| Surface | Multi-host exposure |
|---|---|
| JSON-RPC | Full typed family: `mob/grant_scopes`, `mob/revoke_scopes`, `mob/grants`; `mob/member_history`, `mob/hosts`, `mob/route_installs`; `mob/bind_host`, `mob/revoke_host`; `mob/hard_cancel_member`; and `mob/member_live_open`, `mob/member_live_close`, `mob/member_live_status`, `mob/member_live_control`. `mob/spawn` and each `mob/spawn_many` spec also accept optional `placement`. |
| REST | Read-only additions only: `GET /mob/{id}/members/{agent_identity}/history`, `GET /mob/{id}/hosts`, and `GET /mob/{id}/route-installs`. Existing helper, event, status, cancel, and respawn routes remain remote-transparent and use the same typed multi-host error projector. No host/grant/live mutation endpoints. |
| Public MCP | Read-only additions only: `meerkat_mob_member_history`, `meerkat_mob_hosts`, and `meerkat_mob_route_installs`. No host/grant/live/hard-cancel tools, so an LLM-reachable MCP roster cannot acquire those operator mutations. |
| CLI | `rkat mob host`, `bind-host`, `revoke-host`, `hosts`; `grant`, `revoke-grant`, `grants`; `member-history`, `route-installs`; and the `live open/close/status/control` family. Existing helper/member verbs route by stable member identity. |
| Python / TypeScript | First-class wrappers for every JSON-RPC method above, including placed spawn. Return the shared generated wire types; do not invent SDK-only envelopes. |
| Agent tools | `delegate` and `mob_spawn_member` accept optional `placement`. Host/grant/observation/live/hard-cancel console verbs are deliberately absent from the LLM-visible roster. |
| Web / WASM | Single-host. Spawn carriers accept the source-compatible optional placement field, but a non-local placement is rejected by the typed capability boundary. No member-host, grant, observation-console, or member-live console exports. |

Member-live open/control is WebSocket-only for both local and placed members.
The generic session `live/*` family can negotiate WebRTC only when the session
is local to that RPC host; a controlling host cannot use `session/*` or
`live/*` to proxy a placed member's remote session.

All console surfaces consume the same four classified failures (wire codes
`SCOPE_DENIED`, `HOST_UNAVAILABLE`, `STALE_CURSOR`, and `STALE_FENCE`), plus
`STALE_DELIVERY_SCOPE` (JSON-RPC -32036, HTTP 409) for a scope-bound submit
whose member session moved under a current fence (re-capture the scope;
`STALE_FENCE` means re-resolve the member).
Unclassified failures keep each route or protocol's legacy rendering.

Runtime-mode behavior is shared across these surfaces because dispatch comes from the same mob runtime:

- autonomous members: event injection/subscription dispatch
- turn-driven members: direct `start_turn` dispatch

### Destroy completion

A destroy (`MobMcpState::mob_destroy`, RPC `mob/destroy`, the lifecycle
destroy on RPC, MCP and Web, the agent `mob_destroy` tool, and the cleanup of
a session's owned mobs on session archive) removes the mob's database files
only after its actor task ended and its database observer drained. While a
store, storage bundle, handle or reader in the same process still owns that
observer (including a `MobHandle` the caller kept across the destroy, or one
held by another `MobMcpState`), it keeps the files and answers
`MobMcpDestroyError::Incomplete`, reported on every surface with
`code: "mob_destroy_incomplete"`, the `destroy_report` and `retryable: true`
(JSON-RPC and MCP: internal-error data; REST session archive: HTTP 500). Drop
those handles and retry as the same caller; the retry continues the owed
cleanup and answers the canonical report. Creating, restoring or inserting a
mob under that id is refused until the cleanup is done. An admitted destroy
closes the mob's admission first (`MobAdmissionCloseReason::Destroyed`). A
host that drains observers itself reads a `SqliteMobObserverDrainReport`;
since 0.9.4 its `retained_registrations` counts change-watch notification
registrations still running when the drain stopped them (a struct literal
must set it).

## External members

This section covers one already-running peer process that Meerkat does not
materialize or own. It does not cover a managed session-backed member placed on
a bound member host. Managed cross-host members use `SpawnMemberSpec.placement`
with `RuntimeBinding::Session`; placement is machine-owned and is not an
`External` backend.

An unmanaged external member usually runs in another process, sandbox, or
host. It requires an explicit `RuntimeBinding::External` at spawn time; a bare
external backend tag is not enough. The current binding shape carries `kind:
"external"`, the advertised comms address, the external runtime's Ed25519
public identity, and a typed `bootstrap_token` for supervisor bridge binding.

For a remote `rkat` member, generate that binding from the target process:

```bash
rkat run \
  --comms-name remote-worker \
  --comms-listen-tcp 0.0.0.0:4200 \
  --comms-advertise-tcp 203.0.113.7:4200 \
  --comms-binding-out ./remote-worker.binding.json \
  --keep-alive \
  "You are a remote worker."
```
`--comms-advertise-tcp`
takes `HOST:PORT` or `tcp://HOST:PORT` (0.9.1); HOST is a DNS name, an IPv4
address or a bracketed IPv6 address, resolved when a peer connects, and it is
advertised as `tcp://HOST:PORT`. A wildcard host and a missing or zero port
are refused (0.9.0 accepted only IP literals). A wildcard
`--comms-listen-tcp 0.0.0.0:...` needs the flag or a realm `[comms]
advertise_address`.

Use `rkat run --comms-listen-tcp` for the signed Meerkat peer channel.
`rkat-rpc --tcp` is only JSON-RPC host transport and does not make an agent
reachable as a signed peer or external mob member.

### Spawn startup policy

- Mob member spawn uses deferred initial turn semantics.
- Session creation for spawn registers the session without immediately running a model turn.
- Fresh runtime-backed `autonomous_host` members start their host loops and
  automatically admit an initial kickoff after activation/readiness. The
  kickoff uses `initial_message` when supplied, otherwise a fallback prompt.
  This is real runtime prompt dispatch and can call the provider without any
  later user message, peer message, or flow step.
- `turn_driven` members have a separate dispatch path: an explicitly supplied
  initial message can run an initial turn, but an omitted message does not
  imply the autonomous fallback kickoff. Recovery that resumes an
  authoritative transcript suppresses a manufactured autonomous kickoff.
- Concurrent spawns provision in parallel; actor finalization stays serialized for deterministic state transitions.
- `spawn_many(Vec<SpawnMemberSpec>)` exposes this as first-class runtime API.

## Multi-surface examples

### CLI tool-driven (primary)

```bash
rkat run --tools full "Create a mob with one lead and three workers, wire lead to all workers, and report status."
rkat run --tools full --resume <session_id> "Retire worker-2 and add worker-4, then summarize."
```

### CLI direct commands (explicit operational)

```bash
rkat mob spawn-helper team-mob "Join as lead-1" --profile lead --agent-identity lead-1 --result-label join-report --max-text-bytes 4096
rkat mob fork-helper team-mob lead-1 "Investigate the failing test cluster." --agent-identity worker-2 --profile worker --result-label investigation --max-text-bytes 4096 --json
rkat mob member-status team-mob lead-1 --json
rkat mob force-cancel team-mob worker-1
rkat mob respawn team-mob worker-1 --initial-message "restart"
rkat mob run ./dist/release-triage.mobpack --prompt "triage latest regressions" --trust-policy permissive --json
rkat mob runs team-mob --json
rkat mob status team-mob <run_id> --json
rkat mob attach team-mob <run_id> --json
rkat mob run-flow team-mob --flow triage --stream
```

### CLI artifact + web deployment

```bash
rkat mob pack ./mobs/release-triage -o ./dist/release-triage.mobpack \
  --sign ./keys/release.key --signer-id team@example.com   # --sign requires --signer-id
rkat mob inspect ./dist/release-triage.mobpack
rkat mob validate ./dist/release-triage.mobpack --trust-policy permissive
rkat mob run ./dist/release-triage.mobpack --flow main --trust-policy permissive
rkat mob web build ./dist/release-triage.mobpack -o ./dist/release-triage-web \
  --wasm <PKG_DIR|name_bg.wasm> --trust-policy permissive
```

Packing signs the artifact but does not install its signer. The explicit
permissive policy is therefore the local posture until the signer is installed;
signature verification still runs and warns that the signer is unknown.
`--wasm` is required and names prebuilt wasm-pack `--target web` output.

### WASM browser surface

The web build is a surface over the shared `MeerkatMachine`. Direct sessions and mob members use `SessionOwned` bindings for the same admission, keep-alive, cancellation, terminality, and streaming semantics as other runtime-backed surfaces.

**How it works:**
- `meerkat-core` + `meerkat-client` compile to wasm32 via `tokio_with_wasm` (drop-in tokio replacement)
- `reqwest` uses browser `fetch` on wasm32 — no custom JS bridge
- `web-time` replaces `std::time` types (SystemTime, Instant) for browser compatibility
- Anthropic CORS header added automatically on wasm32 targets

The shared `BrowserRuntimeProfile` controls availability. In-memory sessions,
foreground execution, keep-alive, in-process comms, transient turn context, and
embedded skills are supported. Excluded capabilities include durable storage,
background execution after page teardown, host processes, configured hooks,
runtime skill discovery, MCP clients, TCP/UDS comms, and remote placement.
Requests for exclusions return `CAPABILITY_UNAVAILABLE` with typed clearing
actions; inspect the profile for the complete current capability set.

**WASM API:**
Key mob-related exports (a partial list; the full WASM export reference is not part of `rkat help`):
```
mob_create(definition_json) → mob_id string  [async]
mob_spawn(mob_id, specs_json) → result JSON  [async]
mob_wire / mob_unwire / mob_wire_peer / mob_unwire_peer / mob_retire / mob_respawn  [async]
mob_list_members / mob_member_send / mob_events(mob_id, after_cursor: string, limit: u32) / mob_status / mob_list  [async]
mob_member_status / mob_spawn_helper / mob_fork_helper / mob_force_cancel / mob_stop_member_run / mob_append_system_context  [async]
mob_lifecycle(mob_id, action)  [async]
mob_run_flow → run_id string / mob_flow_status / mob_cancel_flow  [async]
mob_member_subscribe / mob_subscribe_events → stream_id string  [async]; poll_subscription / close_subscription  [sync]
```
There is no `wire_cross_mob` export.
There is also no browser `mob_run_result` export: `mob_flow_status` returns a
`MobFlowStatusResult` JSON envelope with `run` (state and ledgers, or null).
The Web SDK's `mob.flowStatus(runId)` unwraps it to `FlowStatus | null`.
Native RPC `mob/run_result` and native SDK run-result helpers are separate
surface contracts.

### RPC

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"session/create","params":{"prompt":"Use mob_* tools to create a lead/worker mob and return status.","enable_mob":true}}
```

A mob's members reach the callback tools registered on the JSON-RPC
connection that holds the mob's callback route: the connection whose
`mob/create` created it, or the one that claimed it with
`mob/claim_callback_route {mob_id, claim_token}`, presenting the claim token
`mob/create` returned once to the creator (the mob keeps only its SHA-256).
A mob restored after a restart, created any other way, or whose connection
closed, has no route: its members' callbacks fail `tool_unavailable` until a
connection claims it. A mob created other than by `mob/create` has no token
and is not claimable unless its host mints one
(`MobMcpState::mint_callback_claim_token`, Rust only). The claim reaches live
and later members alike; another token is refused with `-32002`
(`claim_token_mismatch`), and so is a claim while the holding connection is
open (`held_by_open_connection`).

### REST

```bash
curl -X POST http://127.0.0.1:8080/sessions \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Use mob_* tools to create a lead/worker mob and return status.","enable_mob":true}'
```

### MCP

```json
{
  "name": "meerkat_run",
  "arguments": {
    "prompt": "Use mob_* tools to create a lead/worker mob and return status.",
    "enable_mob": true
  }
}
```

### Python SDK

```python
from meerkat import MeerkatClient

client = MeerkatClient()
await client.connect(realm_id="team-alpha")
result = await client.create_session("Design a mob topology for release triage.")
print(result.text)
await client.close()
```

### TypeScript SDK

```typescript
import { MeerkatClient } from "@rkat/sdk";

const client = new MeerkatClient();
await client.connect({ realmId: "team-alpha" });
const result = await client.createSession(
  "Use mob_* tools to create a lead/worker mob and return status.",
  { enableMob: true },
);
console.log(result.text);
await client.close();
```

These execution examples require a host with mob support and a composed
`MobToolsFactory`; `enable_mob` / `enableMob` explicitly requests generated
create-only operator capability. It does not grant arbitrary existing-mob
scopes or bypass per-call authorization. The Python design-only prompt does
not require the model to execute mob tools.

## Flows (subfeature)

Flows add DAG orchestration to mobs.

### v1 flows (flat step DAG)

Flow essentials:

- `depends_on` + `depends_on_mode` (`all`/`any`)
- `dispatch_mode` (`one_to_one`/`fan_out`/`fan_in`)
- `collection_policy` (`any`/`all`/`quorum`)
- optional `condition` and `branch`
- persisted `step_ledger` and `failure_ledger`

### v2 flows (frame-based execution with loops)

All flows execute through `FlowFrameEngine` with a canonical
`FlowSpec.root: FrameSpec`. Flat step declarations normalize to that root at
decode/construction; an explicitly authored `root` takes precedence over the
generated structure. These are two authoring forms, not separate engines.

Key types:

- `FrameSpec` — a set of `FlowNodeSpec` nodes forming a dependency graph within a frame
- `FlowNodeSpec` — either `Step(FrameStepSpec)` or `RepeatUntil(RepeatUntilSpec)`
- `FrameStepSpec` — references a named `step_id`, with `depends_on` and
  `depends_on_mode`; role/message stay in the flow's `steps` map
- `RepeatUntilSpec` — loop with `loop_id`, `depends_on`, `depends_on_mode`,
  `body: FrameSpec`, `until: ConditionExpr`, `max_iterations: u32`

Execution model:

- `MobMachine` owns per-frame state (node readiness, completion tracking)
- `MobMachine` owns loop body/evaluate lifecycle
- `MobMachine` owns scheduler grants (`GrantNodeSlot`, `GrantBodyFrameStart`), frame-step projection, and terminalization
- `flow_run`, `flow_frame`, and `loop_iteration` are MobMachine-owned fail-closed projection reducers used to materialize `MobRun` snapshots. They are not standalone machines.
- Frame-step outcomes route back through MobMachine-owned transitions; direct mutation from executor code is prohibited
- Recovery handles ready-frame / pending-body-frame drift and returns typed incompatibility for pre-v2 active runs

Definition example inside a mob with `tester` and `lead` profiles:

```toml
[flows.release_flow]
description = "Iterative release check"

[flows.release_flow.steps.run_tests]
role = "tester"
message = 'Run tests and return only JSON with an "all_pass" boolean: true if all pass, false otherwise.'
output_format = "json"

[flows.release_flow.steps.ship]
role = "lead"
message = "Ship it."

[flows.release_flow.root.nodes.check_quality]
kind = "repeat_until"
loop_id = "quality_loop"
depends_on = []
depends_on_mode = "all"
max_iterations = 5
until = { op = "eq", path = "steps.run_tests.all_pass", value = true }

[flows.release_flow.root.nodes.check_quality.body.nodes.run_tests]
kind = "step"
step_id = "run_tests"
depends_on = []
depends_on_mode = "all"

[flows.release_flow.root.nodes.ship]
kind = "step"
step_id = "ship"
depends_on = ["check_quality"]
depends_on_mode = "all"
```

### Operational flow controls

- list flows
- run flow
- check flow status
- cancel flow

### Agent-facing delegation tools

With generated authority, `AgentMobToolSurface`
(`crates/meerkat-mob-mcp/src/agent_tools.rs`) exposes thirteen base definitions:

| Tool | Purpose |
|------|---------|
| `delegate` | Quick helper spawn — creates implicit mob on first use, spawns member, auto-wires comms |
| `conclude_objective` | Supply only `outcome` to conclude the kickoff objective pre-addressed by the current turn |
| `mob_create` | Create a mob from a definition |
| `mob_destroy` | Destroy a mob and archive all members |
| `mob_spawn_member` | Spawn a member into an authorized mob |
| `fork_off` | Fork the current durable member's committed transcript prefix through the resume path into a child the caller owns, and run the task in the background; the outcome is recorded in the forker's transcript as a durable `BackgroundJob` notice (blocking on one-shot hosts) |
| `council` | Fork existing specialist members into a temporary discussion mob, run bounded rounds/merge, and clean up; the sealed outcome is recorded in the convener's transcript as a durable `BackgroundJob` notice (blocking on one-shot hosts) |
| `mob_retire_member` | Archive a member and its session (manage scope, or the caller owns the member) |
| `mob_check_member` | Check a member's execution status and output (manage scope, or the caller owns the member) |
| `mob_list_members` | List members of a mob (without manage scope: only the caller's own descendants) |
| `mob_list` | List all mobs |
| `mob_wire` | Wire a member to a local or external peer (creates comms trust) |
| `mob_unwire` | Remove a wiring relationship between a member and a peer |

`conclude_objective` requires an objective-correlated turn and its resolved
lead principal. `fork_off` requires the caller to be a durable mob member with
spawn authority; supply `member_id` and `task`, optionally `message_count`,
`expected_output` (prompt guidance, not a validated schema), `result_label`,
`max_text_bytes`, and `max_run_secs`. Unknown arguments are rejected. The
child runs turn-driven whatever its role's default runtime mode. This
durable transcript fork is distinct from `MemberLaunchMode::Fork` /
`fork_helper`, which seed a fresh session's prompt with rendered history;
low-level `Session::fork()` / `fork_at()` / `fork_replacing()` are separate
structural primitives. Every fork starts with zero usage: the source's
lifetime token counters are not copied into the child.

`fork_off` is detached where the host can deliver a later completion: it
declares `DetachedCompletionDelivery::Available` and has a runtime adapter
(the default for hosts built with one: the JSON-RPC, REST and MCP servers,
Meta; also `rkat run --keep-alive` and `rkat mob deploy --surface rpc`), and
it has bound its runtime delivery inbox with `MobMcpState::bind_continuations`
(the Core hosts above do). It returns once the child is seated and its turn
admitted: `status: "running"`,
`agent_identity`, `member_ref`, `fork_session_id`, a `job_id`, and a `note`
(no `cache_inheritance`: that host-side accounting stays on
`ForkMemberResult`). When the child's turn ends, its outcome is recorded
once in the forker's transcript as a durable `BackgroundJob` system notice
(header "Background fork_off job <id> finished (<status>):" with status
`completed`, `terminated` for an autokill, or `failed`; the outcome JSON is the
typed block's `detail`, with `persisted: true`). The outcome is committed once
as a `ForkJobTerminal` event in the child's mob, then submitted to the forker
as a durable continuation under the idempotency key `fork_off:<job_id>` and
applied as a runtime input with steer handling: a forker mid-turn sees it at
that turn's next model call (a durable in-turn append, saved once, no
second turn; one follow-up turn only if the turn ends first), an idle forker
runs one wake turn that sees it, and a non-live forker is revived through its
mob first. It is
readable in later turns and through session history even after the child is
retired. The outcome status is `completed` (with `bounded_result`, `usage`,
`turns`, `tool_calls`), `failed` (child retired), `max_run_elapsed` (run
cancelled, child retired), `supervisor_stopped`, or `restart_interrupted`. The
child's durable `ForkJobRecord` lets a restarted host re-link it: a one-time
pass after restore (or when Meta inserts a restored handle) delivers a reply
already in the child's durable transcript, observes a still-running child with
`max_run` measured from the original start, or delivers `restart_interrupted`,
under the same idempotency key, and wakes an idle forker; a job whose
completion was already admitted is never re-linked, and a respawned child
carries no job. The re-link routes by the job's `owner_session_id` (never the
child's roster entry); a limit already elapsed delivers `max_run_elapsed` at
once and retires the child only after delivery settles (no
`retirement_error` on the re-linked outcome); a respawned forker is
`OwnerGone`; a forker that cannot be served yet (its mob not running, or its
resume still in progress) receives the durable continuation once it is. The
`rkat` CLI declares `Unavailable` unless it stays alive, so `rkat run` without
`--keep-alive` and one-shot `rkat mob` commands block and return the child's
result directly, with `blocked_because` (`host_declared_unavailable`,
`no_runtime_adapter`, `no_owner_revival_host`, or `no_continuation_owner` on a
host that never bound its delivery inbox). Neither form has a default deadline, and the agent
loop's default tool deadline does not cut it (an explicit
`tools.tool_timeouts.fork_off` still does); `max_run_secs` is an opt-in
autokill, honored in both forms, that cancels the run and retires the child.

The forker owns its child, and transitively every member that child forks
(ownership never flows upward). Ownership is durable spawn provenance
(`RosterEntry::spawned_by`, kept across resume, respawn and successor-spec
respawn) checked against the caller's own session binding, never against
arguments. Without manage scope the forker can still observe its descendants
with `mob_check_member` (a status read never waits on the member's turn; when
it could not observe the member's session it still answers, with a typed
`preview_unavailable` of `observation_deadline`, `read_failed`,
`session_absent` or `not_observed_while_retiring`, meaning `output_preview`
and `tokens_used` are missing rather than empty, and a `note` that says
whether checking again helps), retire them with `mob_retire_member`, and see
them, and only them, in `mob_list_members`; the member operator tools
`member_status`, `retire_member`, `force_cancel_member`, and `list_members`
apply the same rule. Retirement cascades to descendants, deepest first, in
the core retire (`MobHandle::retire`; `retire_with_descendants` is an alias):
the retire tools, autokill, failed-child cleanup, host `mob/retire` /
`meerkat_mob_retire`, and Meta's retire paths all take it, and a child
forked mid-cascade is retired too. A child whose own turn fails is
retired automatically; a child whose turn completes stays seated until its
forker retires it. Meerkat adds no retention limit; Meta applies its
`idle_retire_secs` policy.

`council` requires creation authority and scope over each source mob. Supply a
`topic` and participants with `mob_id`, `member_id`, and discussion `role`;
optional controls include `council_id`, `max_rounds`, `max_exchanges`,
`max_result_bytes`, `timeout_seconds`, and `merge`. Source profiles are
resolved by the tool; participants are forks, not the original members. Like
`fork_off`, a council is detached where the host can deliver a later
completion: the call returns `status: "running"`, the `council_id`, and a
`job_id`, and the sealed outcome (`result`, `cleanup`, `replayed`, or `error`
when it fails after the call returned) is recorded and delivered the same way
as a "Background council job ... finished" notice; a failure exit reason gives
it status `failed`. A detached council also re-links after a restart: its
custody record carries the convener's job, and the outcome (or
`coordinator_interrupted` once the dead coordinator's lease expires) is
delivered once. The council recovery sweep that does this runs only on a
`MobMcpState` built with `into_shared()` (Meta 0.8.43 does; a plain
`Arc::new` never runs it). `council_id` may contain only ASCII alphanumerics,
`-` and `_` (the derived default is `agent-<uuid>`). On a
one-shot host, or for a plain-session convener on a host without a
`DetachedOwnerHost` (a top-level REST, MCP-server or keep-alive CLI session;
`blocked_because: "no_owner_revival_host"`), the call blocks and returns the
sealed outcome, so no detached job is owed.
`timeout_seconds` bounds it; the agent loop's default tool deadline does not.

Visibility alone satisfies none of these per-call prerequisites.

Model-facing inputs decode through the public contract: `mob_create` takes a
`MobDefinitionInput`, and the profile tools and an inline `tooling` profile
take a `MobProfileInput`, so host-only fields such as `mcp_servers` or
`rust_bundles` are refused with `InvalidArguments`, as are host-path skill
sources. `tooling` on `delegate` and `mob_spawn_member` is `inherit_parent`
(default; `allow_overlay`/`deny_overlay` narrow it), `minimal` (comms tools
only), or `profile` (`realm_profile` or `inline`), and a profile is capped by
the spawning agent's visible tools. On a managed host
(`MobMcpState::with_tool_consequence_policy_registry`), members of
agent-created child mobs and `delegate` helpers run under
`MobMcpState::with_child_application_tool_policy`; without one, `mob_create`,
`delegate` and spawns into child mobs are refused
(`child_tool_policy_required`). A host registers each Rust tool bundle for
child mobs as `HostOnly` (default), `ChildAvailable` (supplied to every inline
child profile) or, since 0.9.4, `ChildOffered`: supplied to no profile, and
mounted only for an inline child profile that names it in `tools.bundles`.
Any other name is refused with `MobError::ToolBundleNotOffered`, which lists
the offered names; an MCP server cannot be `ChildOffered`
(`ChildMcpServerRegistrationError::OfferNotSupported`). The opt-in persists
with the definition and the offer is its ceiling at every build:
`MobHandle::withheld_tool_bundles` reports opted bundles the host no longer
offers, and the member is built without them.

A realm profile store adds five profile-management tools for reusable,
versioned member templates:

| Tool | Purpose |
|------|---------|
| `mob_profile_create` | Register a named profile in the realm |
| `mob_profile_get` | Read a profile (with revision) |
| `mob_profile_list` | List profiles in the realm |
| `mob_profile_update` | Update a profile with `expected_revision` for CAS |
| `mob_profile_delete` | Delete a profile with `expected_revision` for CAS |

With both that store and a parent tool snapshot provider,
`mob_profile_list_sources` additionally lists visible tool sources grouped by
provenance. A profile store alone does not expose it.

These tools are composed into the agent's tool dispatcher via `MobToolsFactory` late-binding. Operator authority is injected at runtime through `MobToolAuthorityContext`; ambient mob enablement alone does not surface operator tools on resume.

## Practical guidance

Use mobs when you need:

- long-lived role-based multi-agent systems,
- explicit peer graph control,
- durable operational history.

Use plain sessions when:

- single-agent execution is sufficient,
- no shared graph/lifecycle state is required.
