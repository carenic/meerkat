# Meerkat 0.8 -> 0.9 Migration Notes

> Scope: what an integrator changes when moving a 0.8.52 system to 0.9.0
> (released 2026-10-13). The source is the `## [0.9.0]` section of
> `CHANGELOG.md` (`### Breaking`, `### Changed`, `### Known issues`); read it
> for the full list. Coming from 0.8.50 or earlier, also follow the per-release
> sections of `docs/guides/upgrading.mdx` first.

## Before you upgrade

- Upgrade everything together: `rkat`, `rkat-rpc`, `rkat-rest`, `rkat-mcp`,
  PyPI `meerkat-sdk`, npm `@rkat/sdk` and `@rkat/web`, and every Rust
  `meerkat*` dependency exact-pinned to `=0.9.0`. The SDK handshake accepts any
  `rkat-rpc` of the same major and minor version, so a mismatched patch
  connects without complaint.
- 0.9.0 writes state that older binaries cannot read, and there is no
  downgrade path:
  - stored runtime input state is version 6 (rows at versions 3-5 are read and
    rewritten as 6; older binaries refuse version 6 rows), so stop every
    process that shares a runtime store and start 0.9.0 on all of them
    together;
  - a mob event log holding `OrchestratorResumeNotificationFailed`;
  - a realm or `mcp.toml` server definition carrying `access` or `secret_env`
    (upgrade every reader of a shared realm document before writing one);
  - a grant selection or work association naming a checked MCP route.
- Back up the realm state root and keep `rkat storage doctor` reports from
  before and after.

## Renames: MobKit is now Meta (no aliases)

| 0.8 | 0.9 |
|---|---|
| JSON-RPC `mobkit/jobs/{heartbeat,progress,checkpoint,complete,fail,cancel_ack}` | `meta/jobs/{heartbeat,progress,checkpoint,complete,fail,cancel_ack}` |
| Python `MeerkatClient.mobkit_job_*` | `meta_job_*` |
| TypeScript `MeerkatClient.mobkitJob*` | `metaJob*` |
| Rust/schema `MobkitJob{Heartbeat,Progress,Checkpoint,Complete,Fail,CancelAck}Params`, `MobkitJobMutationResult` | `MetaJob*Params`, `MetaJobMutationResult` |
| `ToolDeadlineOwner::MobkitPublicCallback` ("mobkit public callback") | `ToolDeadlineOwner::MetaPublicCallback` ("meta public callback") |

Meta 0.9 hosts use the new names; a host still calling the old ones gets
method-not-found.

## Hooks: config that no longer loads

Mandatory model-request hooks are now operation-local: a denying foreground
Guardrail at `pre_llm_request` / `post_llm_response` withholds only that model
operation, sends `mandatory_hook_prerequisite_failed` to the retained
controller, and the run continues. Configuration validation (and run
admission again) rejects:

- an enabled Guardrail at `pre_llm_request` or `post_llm_response` with the
  default `model_operations = "all"`;
- any enabled Guardrail at `run_started` or `turn_boundary`;
- `model_operations` on a point that carries no model operation (even on a
  disabled entry).

A config file with such an entry fails to load, so the host does not start
until it is migrated; a rejected config update keeps the previous valid
config. Run admission additionally rejects a model-request Guardrail on work
without a retained authorized controller (`AgentError::HookConfigInvalid`, no
`RunStarted`, no user append). That config loads, but every such run is
refused; ordinary (ungoverned) sessions have no retained controller. To
migrate:

- For each model-request Guardrail, decide whether its prerequisite also
  governs the separate controller-feedback request. If not, set
  `model_operations = "ordinary_only"` (nothing sets it for you) and run that
  work with a retained authorized controller. Without a controller, move the
  policy to `pre_tool_execution` / `post_tool_execution`, or change it to an
  `observe` hook that returns no decision (`{}`).
- For a `run_started` or `turn_boundary` Guardrail there is no enforcing
  migration yet: remove the gate, move the policy to a supported point (for
  example `pre_tool_execution`), or change it to `observe` and make it return
  no decision (`{}`). The engine still applies a `deny` returned by any
  foreground entry, `observe` included, and such a deny fails the run.

## MCP

- MCP OAuth credentials are per realm. A credential stored before 0.9 is left
  in place but unused: run `rkat mcp login <server>` (or the host login) once
  in each realm.
- New optional server fields: `access = { allowed_operations = [...] }` (an
  access ceiling; `rkat mcp add --allow-operation`) and, on stdio servers,
  `secret_env` (realm service secrets). Older readers reject documents that
  carry them.
- A grant covers an MCP server's installation and raw operations only when it
  lists the installation's connector approval, as the action
  `mcp_connector/<server>/<token>` or the domain
  `mcp-connector/<server>/<token>`; a wildcard approves no connector. Grants
  issued before 0.9 must be reissued to approve a binding.
- Stdio servers now close gracefully within bounds (stdin closed, then
  SIGTERM and SIGKILL to the process group after `eof_grace` 5 s and
  `term_grace` 3 s by default; hosts set them with
  `McpRouter::with_stdio_close_bounds`).

## Governed realms

- `rkat realm grant --controller` requires one or more `--route <key>`
  (`model-route/v1/<sha256 hex>`). A label-only controller grant issued before
  0.9 admits no route: issue a grant for each route (a refused governed prompt
  names the exact command).
- `rkat-rpc` enables `local-authorization` by default, so it serves a governed
  realm as governed JSONL over stdio instead of refusing it. Ordinary realms
  are unchanged and every stock operation stays R1 until review rules exist.
- Ordinary config writes (RPC, REST and MCP `config/set|patch`, the CLI) that
  would change, remove or restate the `governance` section are refused (RPC
  and MCP: invalid params with `data: {"type": "governance_administration_required"}`).

## Approvals

- RPC approval methods report store outcomes as `APPROVAL_STORE_UNAVAILABLE`
  (-32033, nothing committed), `APPROVAL_CONFLICT` (-32034) and
  `APPROVAL_COMMIT_UNKNOWN` (-32035, reconcile before reuse).
- The file approval store needs a Unix directory durability barrier and read
  access to every directory on its path, and refuses a symlinked store path.
  On other platforms (including Windows) a persisted `rkat-rpc` host's
  approval service is unavailable and fails closed; ephemeral hosts keep the
  in-memory store.
- `approval/decide` refuses a consent approval (`AuthenticationRequired`); R3
  consent is decided only through an authenticated host route.

## Mobs (Rust hosts)

- Admission closes once the last exported `MobHandle` clone drops and the mob
  is idle; later spawns, work submission, flow runs, wiring and resume answer
  `MobError::AdmissionClosed`. `read_handle()`, `events()` and the mob's own
  internal clones do not count as owners: keep a `MobHandle` clone for as long
  as the mob must admit work.
- A destroy keeps the mob's database files and answers the retryable
  `MobMcpDestroyError::Incomplete` (`mob_destroy_incomplete`) while a store,
  storage bundle, handle or reader in the process still holds the mob's
  observer, including a handle the caller kept. Drop them and retry; the same
  mob id cannot be re-created until the cleanup is done.
- `MobHandle::submit_work_with_mode_and_delivery_identity_bounded` takes a
  `&MemberDeliveryScope` (from `capture_member_delivery_scope`) instead of a
  runtime id and fence. A member session that moved answers
  `MobError::StaleDeliveryScope` (wire `STALE_DELIVERY_SCOPE`, -32036):
  re-capture the scope.
- A custom `MobProvisioner::admit_governed_input` takes a
  `custody: Option<GovernedInputCustody>` and returns
  `Result<MemberInputAdmission, MobError>`.

## Rust API

Struct literals and exhaustive matches need the new fields and variants named
under `### Breaking`. The common ones:

- `McpServerConfig` gains `access`, `account` and `connector` (use `None`);
  `McpStdioConfig` gains `secret_env`.
- `ToolDef` gains `audience` (`Default::default()`); `ToolProvenance` gains
  `artifact` (`None`, or build it with `ToolProvenance::new(kind, source_id)`).
- `SessionBuildState` gains `mcp_attachment_intents` (`Vec::new()`).
- `HookEngine::mandatory_topology` is a required trait method;
  `HookEntryConfig` gains `model_operations`.
- `ApprovalStore::put` is replaced by `put_if_current(expected, record)` plus a
  required `get`, and `ApprovalStoreError::Backend` by `NotCommitted`,
  `Contended`, `Conflict` and `CommitUnknown`.
- `SessionAgentMandate::new(executor, servers, catalog, represented_subject)`
  selects a grant lineage per MCP server.
- New variants on `McpError`, `MobError`, `MobEventKind`,
  `meerkat_contracts::ErrorCode`, `AuthError` / `TokenStoreError` (typed
  credential access) and many generated machine-authority enums. Exact-pin the
  crate family and rebuild against 0.9.0.

## Other behaviour changes

- `rkat-rpc` shuts down gracefully on SIGTERM or an interrupt, as on stdin
  EOF.
- Keyring access is typed: public status never reads the OS keychain, and a
  refusal is never reported as an absent credential (auth reasons
  `credential_access_denied`, `credential_interaction_required`,
  `credential_access_pending`, `credential_store_unavailable`).
- Explicit `agent.max_turns` values now survive realm config inheritance; `0`
  ends the run before its first model request.

## Known issues in 0.9.0

- In a governed realm, sessions a schedule materializes are not bound to an
  agent yet, so their configured MCP servers are unavailable.
- `rkat-rpc`: a mob's members reach callback tools only through the
  connection whose `mob/create` created that mob; a restored mob has no
  callback route. Re-create it to bind a live connection.

## Checklist

1. Back up, stop every process sharing the state root, and upgrade binaries,
   SDKs and crates together.
2. Rename `mobkit/jobs/*` calls to `meta/jobs/*` (and the SDK methods).
3. Fix hook entries: `model_operations = "ordinary_only"` on model-request
   Guardrails, and no Guardrails at `run_started` / `turn_boundary`.
4. Log in to OAuth MCP servers once per realm.
5. Reissue controller grants per route key and MCP grants with their
   connector approvals.
6. Rust hosts: keep a `MobHandle` clone while a mob must admit work, retry
   `mob_destroy_incomplete`, and update struct literals and matches.
