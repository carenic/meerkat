Covers: governed realms (`rkat realm govern`, `grant`, `revoke-grant`,
`--as-agent`, route keys, limits, listing grants, governed `rkat-rpc`),
review tiers R1/R2/R3 and `[[governance.review.rules]]`, R3 human consent and
approvals (codes -32033 to -32035), native authorization and Rust host
composition, application tool policy, and secret redaction in Debug output.
Sandboxing, tool policy, credentials, MCP limits, errors: references/security.md

# Security in Meerkat 0.9: governed realms, review and approvals

How a governed realm decides whether an agent may perform an operation, when a
model or a human must review it first, and how approval decisions are
recorded. Process sandboxing, the shell command policy, credential storage,
MCP boundaries, audit and the full errors table are in
`references/security.md`.

## Governed realms and grants

Preview. A governed realm runs agent work only under grants that its local
operator issued. Authorization crates: `meerkat-authorization` (grant owner,
policies, audit) and `meerkat-authorization-contracts` (shared vocabulary).

Requirements and limits:

- Feature `local-authorization`: on by default in `rkat` and `rkat-rpc`; off by
  default on the `meerkat` facade. `rkat-rest`, `rkat-mcp` and every ordinary
  opener refuse a governed realm (`realm '<id>' is governed and cannot be
  opened as an ordinary realm`).
- Unix only: the operator is the local OS user (`uid`); elsewhere `govern`
  fails with "the local operator identity is unavailable on this platform".
- The realm must already exist (`RealmNotMaterialized` otherwise) and must not
  use the `memory` backend.
- Governing is permanent: there is no command to ungovern a realm, and no
  flag or run option selects governance. Every later process opens it governed.
- `govern` issues no grants; until the operator grants, governed prompts are
  refused.

### Set up a governed realm with rkat

```bash
rkat realm create team-alpha
rkat -r team-alpha mcp add github --scope realm --allow-operation search_issues --allow-operation get_issue -- /usr/local/bin/github-mcp
rkat realm govern team-alpha
rkat realm grant team-alpha --catalog                              # read the session tool catalog
rkat realm grant team-alpha --mcp github=search_issues,get_issue   # or --mcp github=* for every operation
# Learn the controller route key: grant a placeholder route, then run a prompt.
rkat realm grant team-alpha --controller --route model-route/v1/0000000000000000000000000000000000000000000000000000000000000000
rkat -r team-alpha run "hello"   # refused; the error names the exact command:
#   rkat realm grant team-alpha --agent rkat-agent --controller --route model-route/v1/<sha256 hex>
rkat realm grant team-alpha --agent rkat-agent --controller --route model-route/v1/<sha256 hex>
rkat -r team-alpha run "triage today's issues"
rkat realm show team-alpha        # prints "governance: governed"
```

A prompt needs a controller grant; without one it is refused before any model
call ("the operator has not selected the realm's stock grants"). What governed
work can use in 0.9.0:

- the controller model, on the granted route keys;
- the session tool catalog, with a `--catalog` grant;
- configured MCP servers that have an access ceiling (`access`, written by
  `--allow-operation`; see "MCP security boundaries" in
  `references/security.md`) and a `--mcp` grant; a server without a ceiling
  or a grant is unavailable while the session continues;
- tool bundle operations a Rust host registered, with their own grants.

Builtin tools (shell, files, tasks and the like) and provider-hosted tools have
no stock grant target and are refused (`operation_authorization_unavailable`).
The builtin `web_search` names its own operation target, which stock never
grants, so a per-target stock selection refuses it (`operation_refused`)
before any owner is asked.

### Commands

```bash
rkat realm govern <REALM_ID>
rkat realm grant <REALM_ID> [--agent <AGENT>] (--controller --route <ROUTE_KEY>... | --catalog | --mcp <SERVER>=<OP>[,<OP>...] | --mcp <SERVER>=*) [--expires-in-secs <N>]
rkat realm revoke-grant <REALM_ID> [--agent <AGENT>] (--controller | --catalog | --mcp <SERVER> | --worker)
rkat run --as-agent <AGENT> "..."
```

- `--agent` defaults to `rkat-agent`. One grant per target; a new grant
  supersedes only that target's previous grant, which is retired once no
  unfinished work references it. New selections apply to realms opened
  afterwards.
- `--controller` takes one or more `--route` keys
  (`model-route/v1/<sha256 hex>`, covering provider, model, binding, endpoint
  and wire model) and has no expiry. Changing the model or endpoint changes the
  key and needs a new grant. Controller grants from before 0.9 name no route
  and admit nothing.
- `--expires-in-secs` sets a lifetime for catalog and MCP grants.
- `revoke-grant` exits successfully only when revoked. Otherwise it reports
  why: unfinished work holds the controller, cleanup deferred because work
  references the grant, custody unavailable, refused, or uncertain.
  `--worker` revokes an older selection's single worker grant.
- `--as-agent <AGENT>` is a global flag and works before or after the command
  name (`rkat --as-agent x run ...` or `rkat run --as-agent x ...`); 0.9.0
  accepted it only after the command name.
- MCP grants pin the server's connector approval. A rebound account or a
  connector added to a no-connector server is denied; grants issued before 0.9
  must be reissued.
- Tool bundle operation grants and checked-route grants exist only through the
  Rust API (`grant_stock_agent` with `StockGrantTarget::ToolBundleOperation` or
  `CheckedMcpRoute`); `rkat realm grant` does not issue them.
- Listing grants: no CLI or RPC command lists them (there is no
  `rkat realm grants`); the CLI only grants and revokes. A Rust host reads a
  `StockGrantObservation` (`meerkat::governed_realm::stock::issuance`: realm,
  agent, executor, coverage and one entry per requested `StockGrantSlot`,
  such as `Controller`, `Catalog`, `Mcp(server)`). The offline
  `observe_stock_agent_grants(realms_root, realm_id, agent, slots)` reads the
  persisted selection and is refused while a host serves the realm; that host
  calls `StockGovernance::observe_retained_grants(slots)` instead. An
  observation reports grants only, not credential or account readiness.
- Files: grant owner `<realm root>/authorization/grant-owner.jsonl` and the
  selection `<realm root>/authorization/stock-grants.json`. A missing or
  corrupt grant owner refuses governed work; it is never recreated silently.
- `rkat run` (including `--resume`) works in a governed realm. Known issue in
  0.9.0: sessions created by schedules have no bound agent, so their MCP
  servers are unavailable.
- Restart recovery (0.9.0): a governed input recovered after a restart keeps
  the scoped destination it was admitted under, even where scoped
  destinations are optional. Its destination-scoped operations and the
  disclosure of its result are refused until that same destination is bound
  again (fail-closed if it never is): REST 403 `RESULT_DISCLOSURE_REFUSED`
  (`ApiError::ResultDisclosureRefused`), JSON-RPC -32025 `SCOPE_DENIED` with
  `data.reason` `result_disclosure_refused`.

### Serving a governed realm over JSON-RPC

`rkat-rpc --realm <id>` serves a governed realm only as governed JSONL over
stdio, acting as `rkat-agent` (it has no `--as-agent`). `--tcp`, `--live-ws`
and `--live-webrtc` are refused, skills and schedules are disabled, and the
realm needs a configured `default_binding` (the controller). Configured MCP
servers connect per granted server.

## Operation review (R1/R2/R3)

Preview. In a governed realm every operation the grants allow runs at R1 (no
review) unless a rule raises it:

- R2: the configured reviewer model reviews the exact operation before it runs.
- R3: a human consents to the exact operation (see the next section).

Rules live in the governed realm's own `config.toml`:

```toml
[[governance.review.rules]]
target = { kind = "mcp_operation", server = "mail", operation = "send" }
tier = "r3"
consent_assurance = "person_authenticated"   # or "device_trusted"

[[governance.review.rules]]
target = { kind = "mcp_server", server = "calendar" }
tier = "r2"

[governance.review.reviewer]
identity = { model = "claude-sonnet-5-5", provider = "anthropic" }
max_output_tokens = 1024
```

- `target.kind`: `mcp_server` (every operation of a configured server),
  `mcp_operation` (one raw operation) or `tool_bundle_operation` (`bundle`,
  `operation`). Names are the configured or registered names, never display
  names. Each target appears once.
- A rule only raises a tier; it never permits what grants refuse. The strictest
  matching rule applies.
- `consent_assurance` is required for `r3` and refused for `r2`.
  `device_trusted` accepts a declared local-operator posture or a person;
  `person_authenticated` needs a person authenticated on the decision event. A
  delegated service credential never decides consent.
- The reviewer `identity` takes `model`, `provider`, optional
  `self_hosted_server_id` and `auth_binding`; `provider_params` is refused.
  There is no default reviewer: an R2 operation without one is refused, never
  run at R1. Unknown fields anywhere in the section are refused.
- Reviewed operations must state the account they act with: an MCP server's
  connector binding, its `oauth_account`, or the host declaration
  `account = "none"` on the server entry. An operation with no stated account
  is refused, never reviewed without it.
- Administration: `config set`/`patch` over RPC, REST, the MCP server tool and
  the CLI cannot change, remove or restate the `governance` section. RPC and the
  MCP server answer invalid params with
  `data: {"type": "governance_administration_required"}`. Edit the realm's
  `config.toml` directly (for a project-local realm,
  `<project-root>/.rkat/realms/<realm>/config.toml`); rules take effect when the
  realm next opens. A malformed or unreadable section refuses the realm's
  governed work.
- Rust hosts attach review with `AgentFactory::with_operation_review` /
  `AgentBuilder::with_operation_review` (`BoundOperationReview`). A tool
  without a reviewed entry point (for example `web_search` and comms `send`)
  never runs under a required review: R2 settles as `reviewer_missing`
  without a reviewer and as `unsupported_entry` with one; R3 settles as
  `human_consent_required` without a human consent route and as
  `unsupported_entry` with one.
- What the reviewer sees (0.9.0): for an R2 or R3 operation the owners state
  its action account and current mandate (the grant that admits it, with its
  bounds and expiry), read once after R1 is ruled out and sent to the R2
  reviewer model or shown in the R3 consent presentation. This material
  grants nothing, and each field is bounded to 4 KiB. An owner that cannot
  supply it settles the call as `review_unavailable` (`context_unavailable`)
  before any review; material that changes makes a retained allow stale
  (`review_unsatisfied`, `context_changed`) and a fresh attempt is reviewed
  again. When only the owner's publication moved during an R2 review or
  before the tool started (same permission, tier and material), the dispatch
  itself reviews the call once more as a second attempt for the same call
  (0.9.4), so a review observer's `context_changed` retirement is not always
  the call's outcome. Rust owners
  answer through `WorkAuthorization::review_operation_context`
  (`LocalWorkPolicy::review_context`, `AdmittedWorkPolicyOwner::review_mandate`,
  `OperationPolicyOwner::review_action_account`); every default fails closed.

Feedback is an ordinary tool result: `review_unsatisfied` (data `kind`:
`denied`, `escalated`, `context_changed`, `human_consent_required`,
`already_consumed`, plus `approval_id` when a consent request was committed) or
`review_unavailable` (`deadline_expired`, `reviewer_failed`,
`reviewer_missing`, `unsupported_entry`, `owner_unavailable`, `dispatch_ended`,
`store_conflict`, `store_commit_unknown`, `context_unavailable`).

## Human consent and approvals

### Approval records (JSON-RPC)

`rkat-rpc` exposes approval audit records: `approval/request`,
`approval/list` (`filter`), `approval/get` (`approval_id`), `approval/decide`
(`approval_id`, `decision` = `approve`|`deny`, `actor`, optional `reason`,
`provenance`). Statuses: `pending`, `approved`, `denied`, `expired`,
`cancelled`. SDKs: Python `request_approval`, `list_approvals`,
`get_approval`, `decide_approval`; TypeScript `requestApproval`,
`listApprovals`, `getApproval`, `decideApproval`. REST, the MCP server and the
CLI have no approval methods.

These records gate nothing and execute nothing. `requester` and `actor` are
audit strings that `rkat-rpc` does not authenticate. Do not store secrets in
them. `approval/decide` refuses a consent approval (`AuthenticationRequired`).

Storage: one approval issuer per persistence. A persistent host keeps
`approvals.json` under the realm's store path; an explicitly ephemeral host
keeps them in memory. The file store holds an OS lock on `<path>.lock`, syncs
file and directory on every commit, and works only on Unix: elsewhere (for
example Windows) it refuses to open, so a persisted `rkat-rpc` host's approval
service is unavailable and fails closed. Symlinked store paths are refused.

| Code | JSON-RPC | HTTP | CLI exit | Meaning |
| --- | --- | --- | --- | --- |
| `APPROVAL_STORE_UNAVAILABLE` | -32033 | 503 | 53 | nothing committed; a fresh, rechecked retry is valid |
| `APPROVAL_CONFLICT` | -32034 | 409 | 54 | another owner changed the approval first |
| `APPROVAL_COMMIT_UNKNOWN` | -32035 | 500 | 55 | commit may have happened; reconcile, never resend |

### R3 human consent on a governed rkat-rpc pipe

When a governed realm's rules require R3, `rkat-rpc` makes the stdio pipe's
operator (the OS user who started it, assurance `device_trusted`) the human
route. A `person_authenticated` rule therefore refuses that operator.

1. The call needing consent settles locally with `review_unsatisfied`
   (`human_consent_required`, with `approval_id`); other work continues.
2. The pipe sends `approval/consent_notice` with `kind` = `requested` and a
   `presentation`: `action`, `risk`, `account`, `mandate`,
   `accepted_assurance`, `expires_at`.
3. The operator decides:

```json
{"jsonrpc":"2.0","id":31,"method":"approval/consent_decide",
 "params":{"approval_id":"01936f8a-7b2c-7000-8000-0000000000c1","decision":"approve"}}
```

   Only `approval_id` and `decision` are accepted; any other field is an
   invalid parameter. The method is listed by `initialize` only on a pipe that
   serves consent, and the pipe decides only approvals it presented.
4. Later notices report `approved`, `denied`, `used` or `withdrawn`.
5. An approval reaching its still-open run is spent once, at the run's next
   boundary, on a fresh attempt of the exact retained call; the outcome arrives
   as a `consent_attempt` runtime notice. Consent is spendable only in memory:
   when the run closes it is parked or withdrawn, and a restored row is
   history only.

The pipe's authority ends at EOF. SDKs receive notices with Python
`on_consent_notice` (`parse_consent_notice`) and TypeScript `onConsentNotice`
(`parseConsentNotice`); they have no consent-decision wrapper, so send
`approval/consent_decide` as a raw request.

Rust embedders build their own route: `BoundOperationReview::with_human_consent`,
`ApprovalService::decide_consent` with an `AuthenticatedConsentDecision`, or
`MethodRouter::dispatch_with_consent_event` with an
`AuthenticatedConsentEvent` built after the host authenticated the decision.

## Native authorization for Rust hosts

Preview. Enable the facade feature `local-authorization`. A host installs real
owners in `NativeGrantWorkConfiguration`: `grants` (`LocalGrantAuthority`),
`ingress` (`NativeIngressCheck`), `invocation_owner`
(`AdmittedWorkPolicyOwner`) and `operation_owner` (`OperationPolicyOwner`),
then `MeerkatMachine::ephemeral().with_local_grant_authorization(owners)`
before the machine is shared. Persistent governed construction requires
exclusive execution custody. Other entry points: `meerkat_rpc::governed_jsonl`
(`serve_governed_jsonl`, one fixed-host JSONL connection),
`meerkat::governed_realm::stock_host` (a host's own stores under a stock
governed realm), and `StockCallerPosture::HostAuthenticated` for callers a
host authenticated.

The builtin `web_search` is two operations, each mapped from typed facts and
never from the tool name. The call's plan carries the owner-produced
`ToolOperationTarget::BuiltinOperation { operation:
BuiltinToolOperation::WebSearch }`; its helper's model request is an
`Inference` whose binding carries a `ToolDelegateAttribution` (role
`HostedSearchInference`, naming the call), with the helper's own route and its
`WebSearch` or `GoogleSearch` capability, never the controller's. In a
`TargetedGrantLineages` basis select `GrantTarget::BuiltinToolOperation` for
the call and `GrantTarget::DelegatedHostedSearch { route, capability }` (the
helper's own route key) for the helper; the base, catalog and controller
lineages never cover either. The attribution grants nothing: revoking the
call's lineage refuses later calls but not one already entered, whose helper
still needs its own current lineage.

Typed outcomes (`OperationAuthorizationError`): `Refused` (ordinary denial,
local feedback), `Unavailable` (owner could not decide; repair it, do not
retry unchecked) and `ObservationUnavailable` (required audit failed; an
infrastructure error, never a denial). Grant issuance refusals:
`GrantRefusal::Denied`, `Duplicate`, `ControllerInUse`, `Unavailable`.
Continuations are refused on governed runtimes (`NoAdmissibleWorkBinding`).
Contract and limits: `docs/guides/authorization-integration.mdx` and
`docs/rust/native-authorization.mdx`.

## Rust host composition

Brief pointers for trusted Rust hosts; the stock `rkat` flows above need none
of this.

- Realm posture: the host picks `meerkat::governed_realm::RealmGovernance`
  before a realm opens: `Ungoverned`, `Governed(GovernedRealmBootstrap)` (the
  host's own owners), `Stock(StockGovernance)` (the composition `rkat` uses)
  or `GovernedWithoutOwners` (every governed effect refused as unavailable).
  The posture is read from the realm manifest, never inferred from a grant
  file. Errors: `PersistenceError::RealmGoverned` (an ordinary opener met a
  governed realm) and `StoreError::InvalidRealmGovernance` (the manifest's
  posture and format disagree; the realm does not open).
- Callers: `StockCallerPosture::LocalOperator` (default: the OS operator is
  the only requester) or `HostAuthenticated(StockCallerAccess)`, built with
  `StockGovernance::host_authenticated` or `host_authenticated_for_agent`.
  Each input is bound with its authenticated caller (`bind_caller_prompt`),
  the host decides each exact caller and session, and a refused caller gets
  `StockGovernanceError::CallerNotAdmitted`. In that posture the operator
  stays the owner of every session and is no requester.
- Prepared construction: `AgentFactory::prepare_agent` (or
  `prepare_agent_with_controller`) returns a `PreparedAgent` whose
  `tool_surface()` is the final composed tool surface. No agent exists and
  provider-native tools stay off until `finish_agent(prepared, selection)`
  binds the host's `ToolSurfaceSelection`; a selection made from another
  surface is refused. `MeerkatMachine::bind_session_tool_surface_gate`
  installs such a gate for a session before its agent is built.
- Grant issuance beyond the CLI: `grant_stock_agent` (including
  `StockGrantTarget::ToolBundleOperation`), `revoke_stock_agent` (including
  `StockGrantSlot::ToolBundleOperation`) and
  `grant_stock_agent_for_reviewed_installation`, which grants an MCP server
  only while the realm config still holds exactly the reviewed server at the
  reviewed generation (`StockGovernanceError::InstallationChanged`
  otherwise). `LocalGrantAuthority::issued_reference` returns the reference
  of an issued grant, which does not say the grant is usable;
  `revoke_expecting` refuses with `ExpectedRevokeRefusal::NotCurrent`
  (nothing changed) or `Grant(..)` (which does not prove that);
  `GrantRefusal::Duplicate` means the grant id was already issued, since ids
  are never reused.
- Reviewed connectors: `governed_realm::stock::finalize::finalize_reviewed_connector`
  is the last step of a reviewed connector install. Under one administrative
  custody it rechecks the reviewed account, stored credential and client
  profile without using a token, writes the exact reviewed MCP server to the
  realm config and issues the agent's `--mcp` grant with its connector
  approval. Outcome: `Refused` (nothing written), `PersistUncertain` or
  `Persisted`.

## Application tool policy

A host can attach a per-member application tool (consequence) policy that
decides each call the tool access policy already allows. It needs a Rust host
that installs a `ToolConsequencePolicyRegistry`
(`AgentBuildConfig.tool_consequence_policy_registry`); the stock binaries
install none. The policy content stays with the host's provider and is never
copied into specs.

- Binding (`application_tool_policy`, or `application_policy` in a member
  tool declaration): `{"kind":"unmanaged"}` (default, no policy),
  `{"kind":"inherit"}` (resolved from the parent when spawned) or
  `{"kind":"provider","provider_id":"...","policy_id":"..."}`.
- Set it per mob member with JSON-RPC `mob/apply_member_tool_declaration`
  (read it with `mob/member_tool_declaration`) or REST
  `PUT /mob/{id}/members/{agent_identity}/tool-declaration`, or on a portable
  member spec or `AgentBuildConfig` in Rust.
- The policy document is a `CompiledApplicationToolPolicy` in canonical JSON
  (`parse_canonical_json`): `default_deny` must be `true`, a tool absent from
  the member's grants is denied, and unknown fields, non-canonical bytes and
  digest mismatches are refused.
- At call time a denial is `policy_denied`; `policy_indeterminate` means the
  policy produced no authoritative verdict, so the call is refused. Tools
  stay visible, and a governing policy disables provider-native tools.
- Child mobs that agents create on such a host run under the host's child
  policy or are refused (`child_tool_policy_required`).

## Secret redaction in Debug output

Since 0.8.50, `Debug` output, and so any log line or error that prints it,
keeps names but shows `<redacted>` for values:

- MCP, skill-repository and hook configuration (`McpServerConfig`,
  `McpConfig`, `McpAddParams`, `SkillRepoTransport`, hook command and HTTP
  runtime configs): argument, env and header values, and URL userinfo, query
  and fragment.
- Auth types: inline secrets, command credential arguments and env values,
  refresh and id tokens, API keys, OAuth token results and the stored MCP
  OAuth client secret.
- Live sessions: WebRTC and WebSocket bootstrap tokens and the bootstrap URL
  query; WebRTC SDP offers and answers print only their length (they carry
  ICE credentials).

`Serialize` and equality are unchanged, so config files, wire payloads and
anything you serialize still contain the values. Rust hosts can reuse
`meerkat_core::redact::REDACTED` and `RedactedUrl` for their own types.

## Common questions

- Why is every prompt refused after `rkat realm govern`? `govern` issues no
  grants. Grant a controller route (learn the key with the placeholder route
  shown above), plus `--catalog` and `--mcp` grants for tools.
- Can I ungovern a realm? No. Governing is permanent; use a separate,
  ungoverned realm for ungoverned work.
- Why is the shell (or another builtin tool) refused in a governed realm?
  Builtin tools have no stock grant target in 0.9.0 and are refused with
  `operation_authorization_unavailable`.
- What can run work in a governed realm? `rkat run`, `rkat-rpc` (stdio JSONL
  only) and Rust hosts through the governed entry points above. `rkat-rest`,
  `rkat-mcp` and ordinary Rust openers refuse it.
- How do I list the grants my agent holds? No CLI or RPC command does; the
  CLI only grants and revokes. A Rust host reads a `StockGrantObservation`
  (`meerkat::governed_realm::stock::issuance`) through
  `observe_stock_agent_grants` or, while serving, `observe_retained_grants`.
- Do logs show my tokens? `Debug` output redacts them (since 0.8.50), but
  serialized config and wire payloads still carry them.
- How do I require approval before an action? Grant the operation, add an
  `r3` rule for it with `consent_assurance = "device_trusted"` (the stock
  pipe operator's assurance), serve the realm with `rkat-rpc` and answer the
  `approval/consent_notice` with `approval/consent_decide`.
