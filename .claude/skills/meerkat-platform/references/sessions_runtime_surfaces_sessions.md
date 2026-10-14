Covers: Meerkat 0.9 sessions on every surface (CLI, JSON-RPC, REST, Python,
TypeScript): create, continue, `rkat run --resume`, list, read, history and
summaries, archive; per-turn `turn/start` fields and refused build settings;
turn queueing and the 3-attempt retry cap; interrupt versus `turn/stop_run`;
forks, transcript edits and retention; system prompts; events, cursor replay,
ATIF export; durable resume holds, repair-wholeblob, tool-process recovery.

# Sessions on Every Surface (Meerkat 0.9)

Names are exact and checked against the 0.9.0 source. Storage, runtime,
WorkGraph, schedules and config: `references/sessions_runtime_surfaces_storage_config.md`.
API changes, error codes, SDKs, prebuilt binaries and Rust embedding:
`references/sessions_runtime_surfaces_api_changes.md`.

## Session lifecycle by surface

| Operation | CLI | JSON-RPC | REST | Python | TypeScript |
|---|---|---|---|---|---|
| Create and run | `rkat run "<prompt>"` (or `rkat "<prompt>"`) | `session/create` | `POST /sessions` | `client.create_session(prompt, ...)` | `client.createSession(prompt, opts)` |
| Create without running | none | `session/create` with `"initial_turn": "deferred"` | none | `create_deferred_session(...)` | `createDeferredSession(...)` |
| Continue | `rkat run --resume <target> "<prompt>"` | `turn/start` | `POST /sessions/{id}/messages` | `Session.turn(...)` | `Session.turn(...)` |
| Interrupt current turn | `rkat session interrupt <id>` | `turn/interrupt` | `POST /sessions/{id}/interrupt` | `Session.interrupt()` | `Session.interrupt()` |
| Stop one exact run | none | `turn/stop_run` | `POST /sessions/{id}/runs/{run_id}/stop` | `Session.stop_run(run_id, reason=...)` | `Session.stopRun(runId, reason)` |
| Read state | `rkat session show <id>` | `session/read` | `GET /sessions/{id}`, `GET /sessions/{id}/status` | `read_session(id)` | `readSession(id)` |
| History | none | `session/history` | `GET /sessions/{id}/history?offset=&limit=` | `read_session_history(id, offset=, limit=)` | `readSessionHistory(id, {offset, limit})` |
| List | `rkat session list [--limit N] [--offset N] [--label K=V]` | `session/list` | `GET /sessions?limit=&offset=&label=k%3Dv` | `list_sessions(labels=, limit=, offset=)` | `listSessions({labels, limit, offset})` |
| Archive | `rkat session delete <id>` | `session/archive` | `DELETE /sessions/{id}` | `Session.archive()` | `Session.archive()` |
| Fork | none | `session/fork_at`, `session/fork_replace` | none | `Session.fork_at(...)`, `fork_replace(...)` | `Session.forkAt(...)`, `forkReplace(...)` |
| Export ATIF | `rkat session export-atif <id> [--output FILE]`, `rkat run --export-atif` | `session/export_atif` | none | `export_session_atif(...)` | `exportSessionAtif(...)` |

- `rkat session list` defaults to `--limit 20`; repeated `--label` filters
  must all match (the same AND rule applies to RPC `labels` and REST `label`).
- `rkat session delete` archives the session (it prints `Deleted session:` and
  the `Session Ref:`); archive is a visibility change, persistent history stays
  readable.
- `GET /sessions/{id}/status` returns `{"state": ...}` with one of
  `initializing`, `idle`, `attached`, `running`, `retired`, `stopped`,
  `destroyed`.
- One run executes at a time. A `turn/start` (or REST message) sent during a
  run is queued first in, first out and runs next; the call returns when that
  turn completes. `SESSION_BUSY` (-32002) means the session cannot take the
  request now: it is still being materialized, served by another process, or
  tearing down.
- A turn that keeps failing is retried back to back up to 3 times within one
  wake (about three times the model spend), then its input ends abandoned:
  `session/input_status` shows `terminal_outcome: "abandoned"` (Rust
  `InputAbandonReason::MaxAttemptsExhausted {attempts}`). At the cap the whole
  refused batch ends this way, so other queued inputs can show it too.
  Provider-level retries are separate (0.8.23).
- Summaries (`message_count`, `total_tokens`, `last_assistant_text` in
  `session/read` and `session/list`) are seeded when the session actor is
  created: a new session reports `message_count` of at least 1 (its system
  prompt), and a resumed or forked session reports its committed transcript
  during its first turn (0.8.45).
- Reading a session (for example `GET /sessions/{id}`) no longer waits for an
  in-flight turn to finish (0.8.51).
- `rkat run` and `rkat run --resume` wait for the session event log
  (`.rkat/sessions/<id>/events.jsonl`) to finish writing before exiting
  (0.8.43).

## Resuming from the CLI

There is no `rkat resume`, `rkat continue` or `rkat c` command; those words
fail to parse. Resume with `rkat run --resume`:

```bash
rkat run --resume last "keep going"
rkat run --resume ~2 "go back to that"    # third most recent session
rkat run --resume 019c8b99 "continue"     # UUID prefix or tail handle
rkat run --resume team-alpha:<uuid> "hi"  # realm-qualified; must match the active realm
rkat --resume "keep going"                # root shorthand, rewritten to `run --resume last`
```

A bare `--resume` resumes `last`. When a prompt follows, name the target
first (`rkat run --resume last "keep going"`), or use the root form above.

Accepted targets: a full UUID, a UUID prefix or tail handle,
`<realm>:<uuid>`, `last`, `~`, `~0`, and `~N` (counted from the most
recently updated session).
A realm-qualified id for another realm fails with "Session belongs to realm
'X', but active realm is 'Y'. Use --realm X ...". Resume only finds sessions in
the realm the command resolves (see "Where state lives" in
`references/sessions_runtime_surfaces_storage_config.md`).

## Per-turn inputs

JSON-RPC `turn/start` (SDK `Session.turn`) accepts:

- `prompt`; `skill_refs` (the retired `skill_references` field returns a typed
  error); `additional_instructions`; `keep_alive`.
- `model`, `provider`, `self_hosted_server_id`, `provider_params` and
  `auth_binding`: per-turn LLM identity. For
  `provider_params` and `auth_binding` a plain value (or
  `{"action": "set", "value": ...}`) sets it, `{"action": "clear"}` removes
  it, and omitting it keeps the session value. Unknown fields are refused
  (`turn/start` uses `deny_unknown_fields`).
- `system_prompt` on a turn appends exactly one ordinary System message at
  that turn boundary (since 0.8.11). It does not replace earlier System
  messages. From the second turn on it sits mid-conversation, which only
  models with `supports_mid_conversation_system_messages` accept:
  claude-fable-5-1, claude-fable-5, claude-opus-5-5, claude-opus-5,
  claude-opus-4-8, claude-sonnet-5-5, and OpenAI API models except
  `gpt-live-1` and `gpt-live-1-codex`. Gemini, other Anthropic models and the
  ChatGPT-account backend refuse the request as a non-retryable invalid
  request (`InvalidInputShape`); history is not rewritten. On create, `system_prompt` is a policy: omit or `null` to inherit,
  a string to set, `{"action": "disable"}` to suppress every prompt source.
- `injected_context`: a list of content items stored as typed
  injected-context user rows immediately before the turn's user message;
  excluded from semantic-memory indexing.
- `transient_turn_context`: request-only text. It is kept with the pending
  input for crash retry and shown to the provider for that logical turn, but is
  never written to the transcript, compaction or extraction. It must be
  non-empty ("transient turn context must not be empty") and its whitespace is
  kept exactly. A deferred create does not accept it; attach it to the
  eventual turn.
- `turn_tool_overlay`: a per-turn tool overlay.
- Refused on every `turn/start`: `max_tokens`, `output_schema` and
  `structured_output_retries` are build settings. Sending any of them fails
  with -32602 ("Cannot override <fields> on a runtime-routed turn; set these
  at session/create time or use a deferred session"). Set them on
  `session/create` (a deferred create works too). The Python and TypeScript
  `turn` arguments with these names (`max_tokens`, `maxTokens`, ...) are
  refused the same way.

REST `POST /sessions/{id}/messages` differs: its body `session_id` must equal
the path id, `auth_binding` is a plain `{realm, binding, profile}` reference,
and it has no `provider_params` or `self_hosted_server_id`. A body that sets
`model`, `provider`, `auth_binding`, `max_tokens`, `output_schema`,
`structured_output_retries`, `hooks_override`, `enable_web_search`,
`comms_name` or `peer_meta` makes REST rebuild the session with those
settings before the turn, so REST can change build settings that RPC refuses.

## Interrupt versus stop_run

- `turn/interrupt` cancels the run in flight. Its result is
  `{"session_id", "interrupted": true|false, "result": "interrupted" |
  "staged_noop"}`. An idle session also answers `interrupted` (a successful
  no-op), so `interrupted: true` does not prove a run was cancelled.
  `staged_noop` (`interrupted: false`) is only for a deferred session whose
  first turn has not started. A session still being materialized answers
  `SESSION_BUSY`, an unknown or destroyed one `SESSION_NOT_FOUND`, and a
  runtime in a state that cannot be interrupted answers invalid request
  ("Session is not interruptible while runtime is <state>"). A cancelled
  run ends with a `run_failed` event of error class `cancelled` (0.8.49).
- `turn/stop_run` stops one exact run and terminalizes every input bound to
  it (added in 0.8.49). Take the `run_id` from the `run_started` event
  (`identity.run_id`) or from an input's `last_run_id`. Params:
  `{"session_id", "run_id", "reason"}` (unknown fields refused). REST:
  `POST /sessions/{id}/runs/{run_id}/stop` with body `{"reason": "..."}`.
- The receipt is `{"session_id", "receipt": {"outcome": ...}}` where `outcome`
  is `stopped` (with `run_id` and `contributors[{input_id, completion,
  terminal}]`), `not_current` (stale run id, with `current_run_id`; nothing is
  touched) or `not_stoppable` (a runtime stop or teardown owns the run; carries
  `state`).
- A malformed run id is invalid params on RPC and `400` on REST. REST maps a
  missing runtime to `404`; a not-ready runtime, stale authority or teardown in
  progress to `409`.
- Mob members have the same contract: `mob/stop_member_run`,
  `POST /mob/{id}/members/{agent_identity}/runs/{run_id}/stop`,
  `Mob.stop_member_run` / `Mob.stopMemberRun`.

## Forks and transcript edits

- `session/fork_at {session_id, message_index}` and
  `session/fork_replace {session_id, message_index, replacement}` create a new
  session from an idle source. `running_behavior` only accepts `reject` (the
  default): a busy source gets `SESSION_BUSY`. A fork point that would split
  a tool call from its result is refused (0.8.22). The fork result carries
  `cache_inheritance`, and forks start with zero usage (0.8.43). An optional
  `tool_access_policy` constrains the fork's calls.
- Same-session edits keep the session id and are append-audited:
  `session/rewrite_transcript`, `session/transcript_revisions` (list with the
  current head), `session/transcript_revision` (read one; `"revision":
  "current"` reads the head), `session/restore_transcript_revision`.
- Rewrites may keep the `injected_context` user role, but a rewrite can never
  mint a `compaction_summary` row, and new system-prompt versions are minted
  only by `session/update_system_prompt`. Rewritten rows lose any
  `assistant_message_id` (0.8.45).
- These transcript-edit methods are JSON-RPC and SDK only. REST deliberately
  has no `rewrite-transcript` / `transcript-revisions` routes.
- Retention (0.8.51): after each compaction only the most recent
  `[compaction] history_retained_rewrites` rewrites (default `4`) keep their
  message bodies; older ones keep their commits and digests but lose their
  bodies, so the session document stays bounded. Fork, rewind, restore or
  revision reads older than the window fail with `TranscriptRevisionRetired`
  ("transcript revision X was retired with N older rewrite(s); the oldest
  retained revision is Y"). Raise the setting to keep more history.

## System prompts, instructions and injected system context

- `session/update_system_prompt` (REST `POST /sessions/{id}/system_prompt`)
  replaces one durable, versioned system-prompt key (0.8.22). Boot, resume,
  rewrite and compaction cannot mint prompt versions.
- `session/activate_instruction` activates one immutable instruction revision
  at a safe boundary; `session/instruction_activations` reads the records
  (0.8.30). Once durable instruction activations are effective, system-prompt
  replacement for that target is refused.
- `session/inject_context {session_id, content, source?, idempotency_key?}`
  (REST `POST /sessions/{id}/system_context`) appends one ordinary durable
  System message at the admitted boundary; plain text content is accepted.
- `session/input_status {session_id, selector: {"by": "input_id" |
  "idempotency_key", "value": "..."}}` returns the stored runtime state of one
  input (`state` is absent when nothing matches).

## Events, streaming and replay

- During RPC turns the server sends `session/event` notifications. For a
  standing stream call `session/stream_open {session_id}` (returns
  `stream_id`), receive `session/stream_event` / `session/stream_end`, and
  close with `session/stream_close {stream_id}`. REST streams the same events
  as SSE on `GET /sessions/{id}/events`. SDKs: `subscribe_session_events` /
  `subscribeSessionEvents`, `Session.subscribe_events()` /
  `subscribeEvents()`.
- Cursor replay: `events/latest_cursor {scope}`,
  `events/list_since {scope, cursor?, limit?}` and `events/snapshot {scope}`
  with `scope = {"type": "session", "session_id": "..."}` and
  `cursor = {"scope", "sequence"}` (sequence `0` is the empty-source cursor).
  Cursor errors are `scope_mismatch`, `ahead_of_latest` and
  `sequence_overflow`. SDKs: `latest_event_cursor`, `list_events_since`,
  `event_snapshot` (TypeScript `latestEventCursor`, `listEventsSince`,
  `eventSnapshot`).
- A slow live subscriber receives a typed `StreamTruncated` marker instead of
  silent loss; refresh from history. Event sequence numbers continue across
  actor incarnations of the same session (0.8.49).
- `run_started`, `run_completed` and `run_failed` carry an `identity` with the
  run id (0.8.44). `turn_completed` is published for every committed provider
  call, including tool-loop calls (0.8.43). Assistant events and history rows
  carry an optional `assistant_message_id`; history rows from live speech also
  carry `realtime_origin` (0.8.45). Runtime-appended `system_notice` rows
  carry `runtime_origin {session_id, run_id, input_id, append_ordinal}` in
  history, revision exports and live boundary events (0.8.44); rewrites strip
  it and content digests exclude it.
- ATIF export: `rkat session export-atif <id>` writes
  `<state-root>/<realm>/trajectories/<session_id>.json` unless `--output` is
  given; `rkat run --export-atif` (also with `--resume`) writes one to the
  same place after the turn (a failed export only prints a warning). Each
  provider request is its own ATIF step with metrics (tool-call turns, the
  answer, structured extraction; 0.8.43). RPC
  `session/export_atif {session_id, agent_name?, agent_version?, model_name?}`
  returns the ATIF v1.7 document; more than 500,000 events is
  `INVALID_PARAMS`, and a host without event replay refuses with "event
  replay is not enabled for this runtime host". Rust embedders need the
  facade's `atif` feature (off by default) for `meerkat::atif`.

## Durable recovery holds and repair

A persistent session can be intact but not runnable. JSON-RPC answers
`SESSION_NOT_RUNNING` (-32003) with data
`{"durable_resume_hold": <hold>, "session_error_code": <code>, "session_id",
"content_retained": true}`. Treat these as "session preserved, not runnable
yet", not as a crash.

| `session_error_code` | `durable_resume_hold` | Meaning and action |
|---|---|---|
| `SESSION_DURABLE_TAIL_HELD_FOR_RECOVERY` | `tail_held_for_recovery` | A verified tail lies past the last committed boundary. The next start or attach of the session (a turn, resume or external event on a cold session) recovers it; plain reads (`session/read`, history, status) never do. A tail ending in a tool call without its result stays held, because the call may already have run (0.8.9) |
| `SESSION_DURABLE_TAIL_RECOVERY_REFUSED` | `recovery_refused` | Another live runtime or contradicting receipts blocked recovery; retry after it quiesces |
| `SESSION_DURABLE_EVIDENCE_QUARANTINED` | `evidence_quarantined` | Evidence is forked or unverifiable; investigate before repair |
| `SESSION_WHOLEBLOB_AUDITED_ENDPOINT_DIVERGENCE` | `audited_endpoint_divergence` | A WholeBlob document fails its audited-endpoint guard; repair it with the CLI below |

```bash
rkat session repair-wholeblob <session-id> --json          # diagnose only (default)
rkat session repair-wholeblob <session-id> --apply         # re-anchor, keeps every message
rkat session repair-wholeblob <uuid> --apply --accept-shorter   # only if the live transcript is shorter
rkat session repair-wholeblob <uuid> --runtime-store /path/runtime.sqlite --json
```

WholeBlob runtime databases are the `runtime.sqlite3` of JSONL-backed realms
(or a host's own runtime file named with `--runtime-store`, which then needs a
full session UUID). SQLite-backed realms keep HeadCanonical sessions in
`sessions.sqlite3`.

If the host dies while a tool process runs (a shell call, background shell
job, monitor or command hook), process custody (Linux and macOS) stops that
earlier incarnation's processes before the agent is rebuilt. The run's inputs are
settled as interrupted and not replayed, and the transcript gets a
`tool_process_recovery` notice with a `tool_process_interrupted` block (the
tool's effects may be partial or complete). When custody cannot be proven,
building the agent fails closed with `ProcessCustodyError`, whose message
names the operator action (0.8.49).
