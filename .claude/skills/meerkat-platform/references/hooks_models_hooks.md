Covers: hooks in Meerkat 0.9: the 14 hook points and what a deny does at
each, `[hooks]` / `[[hooks.entries]]` config and defaults, command, http and
in_process runtimes and their JSON contract, `hooks_override` per surface,
ordering, background hooks and failures, 0.9 mandatory model-request hooks
(`model_operations`, controller feedback, configs that no longer load),
post-commit points and hook events. Related: references/hooks_models_model_operations.md, references/hooks_models_turn_control.md

# Hooks (Meerkat 0.9)

Hooks run your code at fixed points of the agent loop and return `allow` or
`deny`; they cannot rewrite requests, arguments, results or text (patch and
rewrite hooks were removed).

## Key facts

- `rkat run` has no hook flags; hooks come from config (`[[hooks.entries]]`)
  or a per-session `hooks_override` (RPC, REST, MCP, SDKs).
- A 0.9.0 config fails to load if it has a `guardrail` at `run_started` or
  `turn_boundary`, a `guardrail` at `pre_llm_request` / `post_llm_response`
  without `model_operations = "ordinary_only"`, or `model_operations` on any
  other point. A model-request guardrail also needs work with a retained
  authorized controller (in practice a governed realm); without one every run
  is refused with `HookConfigInvalid`.
- A `pre_tool_execution` deny refuses only that tool call; the run continues.
- Background hooks and the six post-commit points must be `observe`. At the
  other points a `deny` from any foreground hook takes effect, `observe`
  included.

## Hook points

| `point` | Fires | Effect of a `deny` | `guardrail` allowed in 0.9 |
|---|---|---|---|
| `run_started` | before the run is published and the user message is appended | run refused (`HookDenied`), no `run_started` event | no |
| `pre_llm_request` | before each model request | that request is withheld (see Mandatory model-request hooks) | only `ordinary_only` with a controller |
| `post_llm_response` | after a response arrives, before its tool calls run and before commit | the response's tool dispatch and commit are withheld | only `ordinary_only` with a controller |
| `pre_tool_execution` | before each tool call | only that call is refused; the model gets a `hook_denied` tool error | yes |
| `post_tool_execution` | after a tool call, before its result is published | the tool already ran; its result is replaced by a `hook_denied` "publication withheld" error | yes |
| `turn_boundary` | after a tool round, before the next model call | the run fails | no |
| `run_completed` | after the final answer, before completion is reported | the run fails | yes |
| `run_failed` | after a run fails | none: errors and denials are only logged | yes |
| `runtime_input_accepted`, `runtime_input_rejected`, `runtime_input_deduplicated`, `peer_ingress_committed`, `peer_egress_committed`, `interaction_completed` | after the fact is committed | none (observe-only) | no |

## Configuration

```toml
[hooks]
default_timeout_ms = 5000          # per invocation unless the entry sets timeout_ms
payload_max_bytes = 131072         # max serialized invocation and max hook reply
background_max_concurrency = 32    # background hook tasks at once (minimum 1)

[[hooks.entries]]
id = "tool-policy"                 # default "hook"; ids must be unique
enabled = true                     # default true
point = "pre_tool_execution"       # default turn_boundary
mode = "foreground"                # foreground (default) | background
capability = "guardrail"           # observe (default) | guardrail
priority = 10                      # default 100; lower runs first
timeout_ms = 2000                  # optional

[hooks.entries.runtime]
type = "command"                   # command | http | in_process
command = "python3"
args = ["hooks/tool_policy.py"]
env = { POLICY_FILE = "policy.json" }
```

Validation errors: `hook id cannot be empty`, `duplicate hook id: <id>`,
`background hooks must be observe-only: <id>`,
`post-commit hook points are observe-only: <id>`, plus the 0.9 topology rules
below. Always set `id`; two entries left at the default collide.

Entries come from the effective realm config (realm chain folded root first),
then the project `.rkat/config.toml`, then the user `~/.rkat/config.toml`
(where the surface has those roots, as the CLI does). For a repeated id the
first occurrence wins, so a child realm cannot replace or remove an inherited
entry; disable it per session instead. The three `[hooks]` settings come from
the effective realm config. Hooks are resolved when a session's agent is
built.

## Per-session overrides (`hooks_override`)

`hooks_override: {"disable": ["<id>"], "entries": [<entry>...]}` removes
configured entries by id and appends new ones; the result is validated again
(a duplicate id fails). To swap an entry, disable its id and add the
replacement under the same id. Override-only entries work with no configured
hooks.

- RPC `session/create.hooks_override` (`turn/start` has none).
- REST `POST /sessions` and `POST /sessions/{id}/messages` (on continue, any
  `hooks_override` rebuilds the session's agent).
- MCP `meerkat_run` and `meerkat_resume`.
- Python `create_session(..., hooks_override=...)`, `create_session_streaming`,
  `create_deferred_session`; TypeScript `SessionOptions.hooksOverride`.
- Rust `AgentBuildConfig.hooks_override: HookRunOverrides { entries, disable }`.

## Runtimes and the wire contract

- `command`: runs `command` with `args`; `env` adds variables to the inherited
  environment. The invocation JSON is written to stdin; the process must print
  one response object on stdout and exit 0. A non-zero exit (stderr is
  reported), unparsable or empty stdout (print `{}` to allow), an invocation or
  output larger than `payload_max_bytes`, or exceeding the timeout (the process
  group is killed) is a hook failure. Not supported on wasm32.
- `http`: sends the invocation JSON (`content-type: application/json`) to
  `url` with `method` (default `POST`) and literal `headers` (no environment
  expansion). A non-2xx status, oversized or unparsable body is a failure.
- `in_process`: `handler = "<name>"` (alias `name`) selects a Rust closure
  registered with `DefaultHookEngine::with_in_process_handler` or
  `register_in_process_handler`; an unregistered name fails when invoked.

Responses (`RuntimeHookResponse`; unknown fields such as a legacy `patches`
are rejected). `{}` or `{"decision": {"decision": "allow"}}` allows. A deny:

```json
{"decision": {"decision": "deny", "hook_id": "tool-policy",
  "reason_code": "policy_violation", "message": "rm -rf is not allowed",
  "payload": {"rule": "no-rm"}}}
```

`reason_code` is one of `policy_violation`, `safety_violation`,
`schema_violation`, `timeout`, `runtime_error`. `hook_id` is required but is
replaced by the configured entry id. `payload` is optional; an explicit `null`
is kept distinct from absent.

Example command hook for the entry above (`point = "pre_tool_execution"`):

```python
#!/usr/bin/env python3
import json, sys
inv = json.load(sys.stdin)
call = inv.get("tool_call") or {}
cmd = (call.get("args") or {}).get("command", "")
if call.get("name") == "shell" and "rm -rf" in str(cmd):
    print(json.dumps({"decision": {"decision": "deny", "hook_id": "tool-policy",
        "reason_code": "policy_violation", "message": "rm -rf is not allowed"}}))
else:
    print("{}")
```

An HTTP entry uses `[hooks.entries.runtime]` with `type = "http"`,
`url = "https://hooks.example.com/check"` and optional
`headers = { Authorization = "Bearer <token>" }`; it must answer with the same
JSON body.

Invocation fields: `point`, `session_id`, `run_id`, `turn_number`, plus per
point `prompt_input`/`prompt` (`run_started`), `error_report`/`error_class`/
`error` (`run_failed`), `llm_request` {`max_tokens`, `temperature`,
`provider_params`, `message_count`, `operation`}, `llm_response`
{`assistant_text`, `tool_call_names`, `stop_reason`, `usage`,
`server_tool_content`, `operation`}, `tool_call` {`tool_use_id`, `name`,
`args`, `provenance`}, `tool_result` {`tool_use_id`, `name`, `content`,
`content_blocks`, `is_error`, `provenance`}, `observation` (post-commit).

## Ordering, short-circuit and background hooks

- Foreground entries at a point run one at a time, by `priority` ascending,
  then registration order. The first `deny` stops the remaining foreground
  entries and skips every background entry for that invocation.
- The engine applies a `deny` from any foreground entry, `observe` included.
  Capability decides validation and which entries count as mandatory
  prerequisites, so an observer that must never block should return `{}`.
- Background entries must be `observe`; their decisions are discarded. They
  run only when nothing denied, can be skipped when capacity is exhausted
  (`concurrency_full`, `retention_full`, `attribution_too_large`,
  `ordinal_exhausted`; a notice records it), and report later as
  `background_hook_completion` notices plus hook events.
- Pre-tool hooks for one response's tool calls run concurrently.

## Failures

- Foreground timeouts, execution failures and invalid configuration fail
  closed: the run fails (`HookTimeout`, `HookExecutionFailed`,
  `HookConfigInvalid`; `run_failed.error_report.class = "hook"`).
- `pre_tool_execution`: a hook that cannot be launched (spawn failure or
  confinement refusal) refuses only that call. An `observe` entry refused by
  command-hook confinement is skipped with a notice at any point.
- `post_tool_execution` infrastructure failure: that call's result is
  withheld, the sibling results commit, then the run ends with the hook error.
- RPC `turn/start` reports a failed run as `INTERNAL_ERROR` (-32603, "turn
  abandoned: ...") with the typed error in `data.error`; `HOOK_DENIED`
  (-32012) is used where a hook denial surfaces as a session error. REST
  returns HTTP 500. The cause is in `run_failed.error_report`.

## Mandatory model-request hooks (0.9)

A foreground `guardrail` at `pre_llm_request` or `post_llm_response` is a
prerequisite for one model operation, not for the run.

```toml
[[hooks.entries]]
id = "prompt-policy"
point = "pre_llm_request"
mode = "foreground"
capability = "guardrail"
model_operations = "ordinary_only"   # all (default) | ordinary_only

[hooks.entries.runtime]
type = "command"
command = "/usr/local/bin/prompt-policy"
```

Rejected when the config loads (`Config::validate`) and again at run
admission, before anything is published:

- an enabled model-request `guardrail` left at `model_operations = "all"`
  ("... set model_operations = \"ordinary_only\" ...");
- an enabled `guardrail` at `run_started` or `turn_boundary` ("... a denial
  there cannot withhold only its dependent operation");
- `model_operations` on any other point, even on a disabled entry.

Rejected at run admission only, as `AgentError::HookConfigInvalid` (no
`run_started`, no user message): a model-request `guardrail` on work with no
retained authorized controller ("... requires a retained authorized controller
for same-run feedback, and this work has none"). Ordinary sessions have none;
governed-realm work admitted with a controller grant (`rkat realm grant
--controller --route <key>`) has one.

When such a guardrail denies, or confinement refuses its launch:

- `pre_llm_request`: the request is not sent. `post_llm_response`: the
  response was already transported, so streamed text and provider-native tools
  that already ran are not undone; its local tool calls and its commit are
  withheld and the body is dropped.
- The agent records the withholding in the work's protected audit, appends a
  `mandatory_hook_prerequisite_failed` notice (`disposition`, `reason`,
  `withheld`, `already_occurred`, `protected_record`; never the hook's message,
  payload or the rejected body) and sends one controller-feedback request to
  the retained controller's model. The run continues; its later model requests
  also go to the controller (no model fallback) and still pass their mandatory
  hooks. A feedback request is never itself recovered.
- `llm_request.operation` / `llm_response.operation` is `{"kind": "ordinary",
  "sequence": 3}` or `{"kind": "controller_feedback", "sequence": 4,
  "withheld": {"sequence": 3, "hook_id": "prompt-policy", "point":
  "pre_llm_request"}}`; `ordinary_only` entries skip feedback requests.
- Not recovered (the run fails): timeouts, execution failures, invalid
  configuration, and a deny from any entry that is not an admitted
  `ordinary_only` guardrail. During structured-output extraction any hook deny
  or failure ends extraction with `extraction_failed`.

Migrating a 0.8 config:

- Model-request guardrail: if its rule should not govern the feedback request,
  add `model_operations = "ordinary_only"` (nothing sets it for you) and run
  the work with a retained controller. Without a controller, move the policy
  to `pre_tool_execution` / `post_tool_execution`, or make it an `observe` hook
  that returns `{}`.
- `run_started` / `turn_boundary` guardrail: no enforcing replacement exists
  in 0.9.0. Remove it, move the policy, or make it an `observe` hook that
  returns `{}`.
- A rejected config update keeps the previous valid config and admitted work.

## Post-commit hook points

The six post-commit points react to committed input, peer and interaction
facts. They require `capability = "observe"` in either mode, carry no
`run_id`, and cannot change or delay the fact. Delivery is process-local best
effort (a crash can lose an invocation, recovery can repeat one), so
deduplicate side effects by ids such as `interaction_id`; use event streams
for ordered or replayable delivery.

## Hook events

`hook_started {hook_id, point}` (only for hooks that actually entered),
`hook_completed {hook_id, point, duration_ms}`, `hook_failed {hook_id, point,
reason}` with `reason.reason_code` `timeout` (`timeout_ms`),
`execution_failed` (`message`), `config_invalid` (`message`),
`observe_only_violation` or `confinement_refused` (`refusal`),
`hook_denied {hook_id, point, reason_code, message, payload?}`, and
`hook_launch_refused {hook_id, point, reason, tool_use_id?}`.

## Platform notes

- Browser/WASM builds compose no hook engine from config, so configured hooks
  do not run there.
- Rust hosts can require OS confinement for command hooks with
  `AgentFactory::with_command_hook_confinement(requirement, working_directory)`
  (absolute program path, no ambient environment, refused when custody is
  missing; not combinable with `hook_engine_override`). Otherwise command
  hooks run as trusted host processes.
- On Linux and macOS, factory builds with a realm runtime root run command
  hooks under durable process custody: a hook cut off by a host crash settles
  its run (the invocation carries `run_id`) instead of letting the run replay.
- A custom `HookEngine` must implement `mandatory_topology` (required in 0.9).
