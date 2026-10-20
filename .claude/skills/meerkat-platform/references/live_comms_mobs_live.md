Covers: Meerkat 0.9 live (realtime voice) channels: enabling live/* on
rkat-rpc (--live-ws, --live-webrtc), realtime models (gpt-realtime-2,
gpt-live-1), live/* methods, pcm_24k_mono audio and size limits, status,
close, playback and media health, SDK calls, the GPT Live preview, and mob
member live (rkat mob live). Comms: live_comms_mobs_comms.md. Mobs:
live_comms_mobs_mobs.md.

# Live Channels (0.9)

The 0.9.0 guide to live (realtime voice) channels on sessions and on mob
members. Agent-to-agent comms are in live_comms_mobs_comms.md and mob
changes in live_comms_mobs_mobs.md.

## Quick answers

- There is no `rkat live` command. Ordinary sessions use JSON-RPC `live/*` on
  `rkat-rpc --live-ws <ADDR>`. Mob members use `rkat mob live
  open|status|control|close` or RPC `mob/member_live_*`.
- WebRTC needs an `rkat-rpc` built with the `live-webrtc` Cargo feature (not
  in stock builds) and started with `--live-webrtc`. Mob member live channels
  never use WebRTC.
- Live audio is 16-bit signed little-endian PCM, 24 kHz, mono.
- GPT Live (`gpt-live-1`) is not in stock binaries: it needs the `openai-live`
  Cargo feature plus a Rust host that installs the public GPT Live authority.

## Live channels

A live channel is a low-latency audio, text and (model-gated) image channel
on an existing session. The session keeps one canonical history: committed
speech and text join it at turn boundaries like ordinary turns. Channels are
caller-initiated; nothing opens one for you. A session has at most one live
channel at a time.

### Enabling live on rkat-rpc

| Need | How |
|---|---|
| WebSocket transport | `rkat-rpc --live-ws 127.0.0.1:4900` (serves `/live/ws`; stock builds) |
| WebRTC signaling | build with `--features live-webrtc`, run `rkat-rpc --live-webrtc` (combine with `--live-ws` for both) |
| Advertised URL scheme | `--live-ws-scheme ws` (default) or `wss` behind a TLS-terminating proxy |
| Live tool-call timeout | `--live-tool-timeout-ms <MS>` (default 30 s) |
| Non-loopback bind | needs `--allow-remote` for `--live-ws` and `--tcp` (exposure opt-in, no auth) |

With neither `--live-ws` nor `--live-webrtc`, `live/*` is not registered.
`live/webrtc/answer`, `live/media_health` and the notifications
`live/assistant_output_available`, `live/media_health_requested` and
`live/assistant_playback_hint` exist only when WebRTC is enabled. SDKs can
spawn the server with these flags: Python
`connect(..., live_ws=True, live_webrtc=True)`, TypeScript
`connect({ liveWs: true, liveWebrtc: true, liveToolTimeoutMs: 60000 })`.

### Realtime models

| Model | Status | Notes |
|---|---|---|
| `gpt-realtime-2` | stable, recommended | Ordinary OpenAI Realtime adapter: both turning modes, image input, WebSocket or WebRTC |
| `gpt-live-1` | stable row; host composition is preview | Public OpenAI Live API: provider-managed turns, WebRTC only, needs `openai-live` and a host authority |
| `gpt-live-1-codex` | experimental, deprecated | Private path behind `experimental-gpt-live`; to be removed |

`ModelCapabilities.realtime` gates `live/open` (see RPC `models/catalog` or
REST `GET /models/catalog`); Anthropic and Gemini ship no realtime models.
`[agent] model = "gpt-realtime-2"` in realm config makes realtime the default
for unhinted sessions.

### JSON-RPC methods

| Method | Params |
|---|---|
| `live/open` | `session_id`, optional `turning_mode` (`provider_managed` default, or `explicit_commit`), `transport` (`websocket` or `webrtc`), `seed_max_chars` (positive), `execution_identity` |
| `live/send_input` | `channel_id`, `chunk` |
| `live/commit_input` | `channel_id`, optional `response_modality` (`{"modality":"text"}` or `{"modality":"audio"}`) |
| `live/interrupt` | `channel_id` (barge-in) |
| `live/truncate` | `channel_id`, `audio_played_ms`, optional `output_id`, `item_id`, `content_index`, `reported_playback_prefix` |
| `live/playback_complete` | `channel_id`, `output_id` |
| `live/refresh`, `live/status`, `live/close` | `channel_id` |
| `live/webrtc/answer` | `channel_id`, `token`, `offer_sdp`; returns `answer_sdp` |
| `live/media_health` | `channel_id`, `output_id`, `decoded_frames`, `audible_frames`, `max_rms`; returns `verdict` (`audible` or `media_fault`) and `reopen_recommended` |

Details that matter:

- `live/open` returns `channel_id`, `transport`, `capabilities` and
  `continuity`. Command results are typed `status` values (`sent`,
  `committed`, `interrupted`, `truncated`, `completed`, `queued`, `closed`);
  `sent` is queue acceptance only.
- Omitted `transport` prefers WebSocket when `--live-ws` is configured and
  otherwise WebRTC. The bootstrap is `{transport: "websocket", url, token}` or
  `{transport: "webrtc", token, answer_method, http_url?}`.
- `capabilities` is a set of booleans (`audio_in`, `image_in`, `video_in`,
  `barge_in_supported`, ...). `continuity.mode` is `fresh`,
  `transcript_only`, `degraded` or `provider_native_resume`.
- `seed_max_chars` bounds the history seed to a recent whole-turn suffix; if
  anything is left out, continuity is `degraded`. Zero is rejected.
- `live/commit_input` and a text-only `response_modality` need an
  `explicit_commit` channel; with provider-managed turns the provider owns
  commits. On the OpenAI Realtime adapter (`gpt-realtime-2`),
  `provider_managed` turns on server VAD at provider defaults, which starts
  and interrupts responses itself; `explicit_commit` turns detection off.
- `live/refresh` applies mutable settings such as visible tools without
  replaying history. Model or provider swaps and transcript rewrites need
  `live/close` plus `live/open` (`refresh_transcript_rewrite_requires_reopen`).
- `live/open` can promote a deferred session or recover an idle one. A
  deferred session that carries `injected_context` must run `turn/start`
  first.
- A GPT Live close converges within 15 seconds of the first close request,
  even when the remote side is already gone, and reports `closed`.

Minimal WebSocket flow:

```json
{"jsonrpc":"2.0","id":1,"method":"session/create","params":{"prompt":"Ready.","model":"gpt-realtime-2","provider":"openai"}}
{"jsonrpc":"2.0","id":2,"method":"live/open","params":{"session_id":"<SESSION_ID>","transport":"websocket","turning_mode":"explicit_commit"}}
{"jsonrpc":"2.0","id":3,"method":"live/send_input","params":{"channel_id":"<CHANNEL_ID>","chunk":{"kind":"audio","data":"<base64 pcm>","sample_rate_hz":24000,"channels":1}}}
{"jsonrpc":"2.0","id":4,"method":"live/commit_input","params":{"channel_id":"<CHANNEL_ID>"}}
{"jsonrpc":"2.0","id":5,"method":"live/close","params":{"channel_id":"<CHANNEL_ID>"}}
```

### Audio formats and limits

- Chunk kinds: `audio` (`data`, `sample_rate_hz`, `channels`), `text`,
  `image` (`idempotency_key`, `mime`, `data`) and `video_frame` (only when
  `capabilities.video_in` is true; never for OpenAI today).
- Direct WebSocket binary frames use `pcm_24k_mono`: 16-bit signed
  little-endian PCM, 24,000 Hz, one channel. The OpenAI adapter validates
  24 kHz mono, and reopening does not change the format.
- Direct WebSocket messages and frames are capped at 2 MiB each, with at most
  32 direct WebSocket connections per process. Images never travel on the
  direct WebSocket or the WebRTC data channel
  (`image_input_transport_unsupported`); send them with JSON-RPC
  `live/send_input`, whose frames may be up to 64 MiB.
- WebRTC carries the browser's Opus audio at 48 kHz, and the server
  resamples it to the provider's 24 kHz PCM. The WebRTC bootstrap token is
  single-use, bound to its channel, and expires after 60 seconds.
- Images: PNG or JPEG only, at most 20 MiB decoded. The MIME string is at
  most 255 bytes. The `idempotency_key` must be 1 to 128 bytes, with no
  control characters and no leading or trailing whitespace.
- Canonical live image history is capped at 40 MiB decoded
  (`image_input_history_budget_exceeded`). Commit staged audio or text before
  an image (`image_input_requires_commit`). A same-key replay with the same
  bytes returns the committed identity; different bytes fail with
  `image_input_idempotency_conflict`.
- Treat an image as durable only after the `user_content_committed`
  observation with its key; scoped failures arrive as `command_rejected`.

### Status, close events, playback and media health

- `live/status` returns `{status: "idle" | "opening" | "ready" | "degraded" |
  "closing" | "closed"}`; a typed close adds `reason` (`media_fault`) and
  `reopen_recommended`, and a closed channel answers `closed` until the
  session unregisters. GPT Live also reports `provider_input_latency`
  telemetry.
- Every committed close publishes a `live_channel_closed` session event with
  `session_id`, `channel_id`, `reason` (`client_requested`,
  `client_disconnected`, `provider_closed`, `error`, `media_fault`, `replaced`
  or `open_abandoned`) and `reopen_recommended`, so observers need not poll
  `live/status`.
- `live/playback_complete` reports finished client playback for one
  `output_id` (announced by `live/assistant_output_available`) and commits
  that transcript prefix without waiting for a provider final.
- Barge-in (GPT Live over WebRTC): the server sends
  `live/assistant_playback_hint {channel_id, hint}` with `duck` when the
  user's speech overlaps audible assistant audio, and `restore` when one side
  goes quiet. The hint is advisory; TypeScript ships
  `applyLiveAssistantPlaybackHint`.
- Media health (GPT Live over WebRTC): after a channel's first assistant
  output the server sends `live/media_health_requested {channel_id,
  output_id}`. The client answers `live/media_health` with raw counters, and
  the server judges them. `media_fault` (no audible frame and a peak below
  0.002) closes the channel before the reply. `reopen_recommended` is true
  only for a session's first media fault.
- Failed responses (since 0.9.4): a provider response that ends without
  completing arrives as a `turn_failed` observation. Route on
  `failure.status` (`failed`, `incomplete` or `in_progress`; a status without
  a wire name arrives as `{"unknown": {"debug": ...}}`); the provider's
  `reason`, `code` and `message` are diagnostics only. `incomplete` for
  `max_output_tokens` or `content_filter` keeps its earlier mapping. A failed
  response is not terminal: the channel stays open, Meerkat neither cancels
  nor retries it, nothing of it is committed (the transcript records
  `assistant_turn_failed`), and a WebRTC peer stops the audio still queued
  for it. Reported usage is charged; without it the session event stream
  carries `turn_usage_accounting_unmeasured`. 0.9.3 and earlier committed the
  partial text as a finished `end_turn` reply, and a failed response without
  usage closed the channel. Upgrade SDKs with the server: older SDKs reject
  the new observation. A session that recorded a failed response cannot be
  opened by 0.9.3 (realtime sidecar event schema 4).

### Transcript rows in session history

Committed live speech joins the session history as `block_assistant` rows
whose blocks are `{"block_type": "transcript", "data": {"text", "source"}}`.
`source` is `{"kind": "spoken"}`, `{"kind": "spoken_unmeasured"}` (speech
observed without measured playback, which is what GPT Live reports) or
`{"kind": "unknown", ...}` (treat as unrecognized, never as spoken). GPT Live
commits one assistant row per provider turn, when the turn ends, not one per
delta. `stop_reason` is optional: observation-only rows omit it. Rows
committed from a live channel carry `realtime_origin` (its channel and
`provider_item_ids`).

Rows wait behind a running turn of the member, so the persisted transcript
can lag the conversation by up to that turn's length. Since 0.9.4 a GPT Live
channel commits all the rows it already holds when a member turn ends at that
boundary (up to 64 rows or about a second at a time), before a member turn
queued behind them starts; before, they drained one row per turn, so speech
said before a typed message could land after its answer. A WebSocket or
WebRTC channel queues its waiting rows in arrival order off the media path, so
its voice, its other observations and the client's input keep flowing while a
member turn runs. It holds the boundary the same way across consecutive
transcript rows, and a turn completion or interruption row takes it on its
own. A transcript frame can reach the client before its row commits. If the
session refuses a row, the channel sends an error frame
(`reason: "projection_error"`, with `row`, `item_id` and `unapplied_rows`) and
closes; once 16 MiB of rows wait, it closes with
`reason: "projection_backlog_full"`, and every queued row still commits. Rows
still queued when the channel closes commit after the close, in order.

### SDK calls

- Python: `client.live_open(session_id, turning_mode=None, transport=None,
  seed_max_chars=None)`, `live_send_input_text`, `live_send_input_audio(channel_id,
  data_base64, sample_rate_hz, channels)`, `live_send_input_image(channel_id,
  idempotency_key, mime, data_base64)`, and `live_commit_input`,
  `live_interrupt`, `live_truncate`, `live_refresh`, `live_playback_complete`,
  `live_webrtc_answer`, `live_media_health`, `live_status`, `live_close`.
  `on_live_notification(callback)` returns an unsubscribe function.
- TypeScript: `client.liveOpen({ session_id, turning_mode, transport,
  seed_max_chars })`, `liveSendInput`, `liveSendInputImage`, and the same
  verbs in camelCase (`liveCommitInput`, `liveWebrtcAnswer`, ...), plus
  `onLiveNotification(listener)` and `LiveChannel.session(client, sessionId,
  { seedMaxChars })`. For WebRTC, wait for ICE gathering to finish and send
  `peer.localDescription.sdp` through `liveWebrtcAnswer`.
- The Web SDK (`@rkat/web`) has no live transport.

### GPT Live (public OpenAI Live API, preview)

`gpt-live-1` runs continuous voice with provider-managed turns. The voice
model has no tools: work that needs tools, files or reasoning is delegated to
the session's durable executor (up to 4 delegations per channel in parallel,
tracked as WorkGraph items; serial without a WorkGraph store) and the result
is spoken back. For a mob member's live channel the default execution policy
is `DurableFork`: each delegation runs in a durable fork of the member (worker
member `live-delegation-<operation>`); a Rust host can compose
`ExistingMember` (`compose_experimental_live_delegation_coordinator_with_policy`),
which runs delegations on the member itself, one at a time. GPT Live is compiled only with the `openai-live` Cargo feature (off
by default, absent from released binaries), and the host must install the
public authority (`ExperimentalGptLiveOpenAuthority::new_public`) with an
OpenAI API-key binding; stock `rkat-rpc` never does this. Callers open with
WebRTC and an execution profile, then answer the SDP offer with
`live/webrtc/answer`:

```json
{"method":"live/open","params":{"session_id":"<SESSION_ID>","transport":"webrtc","execution_identity":{"version":"v1","profile_id":"openai.gpt-live-1.client-context.v1"}}}
```

Capability atoms: `live.execution_identity.v1` and
`live.execution.client_context.v1`. The ordinary Realtime WebSocket factory
refuses `gpt-live-1`, and `live/open` has no top-level `profile_id` and no
`continuous` turning mode.

Turn-taking: the public Live API has no turn-detection setting, so the
default session instructions tell the voice not to greet, to let the user
finish (silent through mid-sentence pauses and fillers such as "um"), not to
backchannel or repeat details back while the user talks, and to act only
once the user has clearly finished. A Rust host replaces them with
`PublicGptLiveOpenAuthorityConfig.session_instructions`, or keeps them and
prepends its own knowledge with `session_instructions_preface`.

Held results: Meerkat holds a finished delegation's result while the user's
latest utterance is unanswered, and delivers one result at a time. Since
0.9.4 a held result first sends the voice a notice that the work has finished
but its result has not arrived, so it no longer answers "not yet". The voice
may say the work just finished but must not state or guess the outcome until
the result arrives. The notice waits while the voice is speaking and is
dropped if the result reaches the session first.

Known limitation: public GPT Live delegates through the client for the
whole session, and the provider's delegation events carry no typed reason,
so a question the user starts while the voice is still speaking (before the
cue for a delegated result is sent) can go to the executor even when it asks
about the conversation itself.

## Live channels on mob members

Each member has its own session, so live channels are per member. Give the
member's profile a realtime model, for example `model = "gpt-realtime-2"`
(it still needs `tools.comms = true`). Address the member by identity; the
owning host is resolved for you, local or placed.

```bash
rkat mob live open <MOB_ID> <AGENT_IDENTITY> [--turning-mode provider-managed|explicit-commit] [--transport websocket]
rkat mob live status <MOB_ID> <AGENT_IDENTITY> [--channel-id <ID>]
rkat mob live control <MOB_ID> <AGENT_IDENTITY> <CHANNEL_ID> commit-input|interrupt|refresh|truncate [--item-id <ID> --content-index <N> --audio-played-ms <MS>]
rkat mob live close <MOB_ID> <AGENT_IDENTITY> <CHANNEL_ID>
```

- `open` prints the `LiveOpenResult` JSON (the owning host's WebSocket URL and
  single-use token) on stdout. The WebSocket is the input plane, so there is
  deliberately no `send` verb.
- `status` without `--channel-id` discovers the member's active channel.
  `truncate` requires all three truncate flags.
- RPC: `mob/member_live_open {mob_id, agent_identity, turning_mode?,
  transport?}`, `mob/member_live_status` (optional `channel_id`),
  `mob/member_live_control` (`verb`: `commit_input`, `interrupt`, `truncate`
  or `refresh`) and `mob/member_live_close`. SDKs: Python
  `Mob.member_live_open/close/status/control`, TypeScript
  `Mob.memberLiveOpen/Close/Status/Control`.
- WebRTC refusal: `--transport webrtc` parses, but the member-live family
  rejects it for local and placed members alike with "webrtc is not supported
  for mob member live channels; use the session-scoped live/open". Use
  session `live/open` with the member's session id, which works only for a
  member local to that `rkat-rpc`. A controller cannot open a placed member's
  session through `session/*` or `live/*`.
- REST and the public MCP server have no member-live verbs.
- A member host serves live channels when started with `rkat mob host
  --live-ws <ADDR> --live-ws-advertise <ws(s)://...>`; `--live-ws` without the
  advertise URL is a startup error.

## Removed or renamed

- `session/realtime_attachment_status` and
  `mob/member_status.realtime_attachment_status` were removed in 0.6.5; use
  `live/*`.
- Do not use the TypeScript `answerLiveWebrtcOffer` helper (it sends the offer
  before ICE gathering finishes); use `liveWebrtcAnswer`.
