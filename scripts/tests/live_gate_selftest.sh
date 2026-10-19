#!/usr/bin/env bash
# Contract test for the live gate's two scripts: scripts/live-gate-changed
# (which pull requests run the GPT Live scenarios) and scripts/live-gate-verdict
# (GREEN / RED / VOID from one single-attempt Bazel run).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
classifier="${ROOT}/scripts/live-gate-changed"
verdict="${ROOT}/scripts/live-gate-verdict"
work="$(mktemp -d "${TMPDIR:-/tmp}/live-gate-selftest.XXXXXX")"
trap 'rm -rf "$work"' EXIT
repo="${work}/repo"
mkdir -p "$repo"

g() { git -C "$repo" -c user.name=Meerkat -c user.email=meerkat@example.invalid "$@"; }
g init -q -b main
mkdir -p "$repo/crates/meerkat-live/src" "$repo/crates/meerkat-core/src" "$repo/docs"
echo "pub fn a() {}" > "$repo/crates/meerkat-core/src/lib.rs"
echo "pub fn b() {}" > "$repo/crates/meerkat-live/src/lib.rs"
echo "doc" > "$repo/docs/guide.md"
g add -A && g commit -qm base
base="$(g rev-parse HEAD)"

classify() {
  local head="$1"
  set +e
  (cd "$repo" && "$classifier" "$base" "$head" >/dev/null 2>&1)
  local status=$?
  set -e
  echo "$status"
}

fail() { echo "live_gate_selftest: $*" >&2; exit 1; }

# A docs-only change is not live.
g checkout -qb docs
echo "more" >> "$repo/docs/guide.md"; g commit -qam docs
[[ "$(classify "$(g rev-parse HEAD)")" == 1 ]] || fail "a docs-only change classified live"

# A Rust change that names no live state is not live.
g checkout -q "$base" && g checkout -qb plain
echo "pub fn c() {}" >> "$repo/crates/meerkat-core/src/lib.rs"; g commit -qam plain
[[ "$(classify "$(g rev-parse HEAD)")" == 1 ]] || fail "a non-live Rust change classified live"

# A file of the live stack is live by path.
g checkout -q "$base" && g checkout -qb livepath
echo "pub fn d() {}" >> "$repo/crates/meerkat-live/src/lib.rs"; g commit -qam livepath
[[ "$(classify "$(g rev-parse HEAD)")" == 0 ]] || fail "a live-stack path classified not live"

# A shared Rust file whose changed lines name live state is live by content.
g checkout -q "$base" && g checkout -qb livecontent
echo "pub fn e(live_channel_id: u32) -> u32 { live_channel_id }" >> "$repo/crates/meerkat-core/src/lib.rs"
g commit -qam livecontent
[[ "$(classify "$(g rev-parse HEAD)")" == 0 ]] || fail "a Rust change naming live state classified not live"

# xtask builds into no live gate target: its change is not live, even when
# its changed lines name live state.
g checkout -q "$base" && g checkout -qb xtask
mkdir -p "$repo/crates/xtask/src"
echo "pub fn live_docs_carry_no_stale_language() -> bool { true }" > "$repo/crates/xtask/src/audit.rs"
g add -A && g commit -qm xtask
[[ "$(classify "$(g rev-parse HEAD)")" == 1 ]] || fail "an xtask change classified live"

# An unknown revision is an error, never "not live".
[[ "$(classify deadbeefdeadbeef)" == 2 ]] || fail "an unknown revision did not fail closed"

# The workflow hands the verdict its log through a secret filter, so every
# verdict case reads its log through that same filter.
workflow="${ROOT}/.github/workflows/live-gate.yml"
gate_filter="$(grep -o "grep -v -E '[^']*'" "$workflow" | head -n 1 | sed -e "s/^grep -v -E '//" -e "s/'\$//" || true)"
[[ -n "$gate_filter" ]] || fail "live-gate.yml no longer filters the gate log with grep -v -E"
filtered() { grep -v -E "$gate_filter" "$1" > "$work/filtered.log" || true; }
run_verdict() {
  filtered "$1"
  set +e
  python3 "$verdict" "$work/filtered.log" "$2" >/dev/null 2>&1
  local status=$?
  set -e
  echo "$status"
}
section() { printf '==================== Test output for //:e2e_smoke_turbo_s_s%s (run 1 of 1):\n%s\n%s\n' "$1" "$2" "$(printf '=%.0s' $(seq 1 80))"; }

: > "$work/green.log"
[[ "$(run_verdict "$work/green.log" 0)" == 0 ]] || fail "a clean run was not GREEN"
[[ "$(run_verdict "$work/green.log" 1)" == 1 ]] || fail "a Bazel failure without test output was not RED"
section 104 "GPT_LIVE_VERDICT scenario=S104 verdict=provider_degraded exchange=e2" > "$work/void.log"
[[ "$(run_verdict "$work/void.log" 3)" == 3 ]] || fail "a provider-degraded-only run was not VOID"
{ section 104 "GPT_LIVE_VERDICT scenario=S104 verdict=provider_degraded exchange=e2"; section 101 "S101 deterministic checks failed"; } > "$work/red.log"
[[ "$(run_verdict "$work/red.log" 3)" == 1 ]] || fail "a real failure next to a void was not RED"
# The gate runs each target once, and Bazel then prints the bare target with
# no "(run N of M)" suffix (#1544's first full run reported its four real
# failures as "Bazel exited 3 without test output").
bare() { printf '==================== Test output for //:e2e_smoke_turbo_s_s%s:\n%s\n%s\n' "$1" "$2" "$(printf '=%.0s' $(seq 1 80))"; }
bare 103 "S103 deterministic checks failed" > "$work/bare-red.log"
[[ "$(run_verdict "$work/bare-red.log" 3)" == 1 ]] || fail "a single-run failure section was not parsed as RED"
bare_out="$(python3 "$verdict" "$work/bare-red.log" 3 2>/dev/null || true)"
grep -q '^RED  //:e2e_smoke_turbo_s_s103 run 1$' <<<"$bare_out" \
  || fail "a single-run failure section was not attributed to its target"
bare 104 "GPT_LIVE_VERDICT scenario=S104 verdict=provider_degraded exchange=e2" > "$work/bare-void.log"
[[ "$(run_verdict "$work/bare-void.log" 3)" == 3 ]] || fail "a single-run provider-degraded section was not VOID"

# The provider stalled after it started speaking: the recorded S98 section of
# run 39209102955 is VOID, and each near miss of its signature is RED.
stall="${ROOT}/scripts/tests/fixtures/live-gate/s98-provider-stall.log"
[[ "$(run_verdict "$stall" 3)" == 3 ]] || fail "the recorded S98 provider stall was not VOID"
filtered "$stall"
stall_out="$(python3 "$verdict" "$work/filtered.log" 3 2>/dev/null || true)"
grep -q '^VOID //:e2e_smoke_turbo_s_s98 run 1 (provider stalled after it started speaking)$' <<<"$stall_out" \
  || fail "the recorded S98 provider stall was not attributed to its target and reason"
{ cat "$stall"; section 101 "S101 deterministic checks failed"; } > "$work/stall-red.log"
[[ "$(run_verdict "$work/stall-red.log" 3)" == 1 ]] || fail "a real failure next to a provider stall was not RED"
{ cat "$stall"; section 104 "GPT_LIVE_VERDICT scenario=S104 verdict=provider_degraded exchange=e2"; } > "$work/stall-void.log"
[[ "$(run_verdict "$work/stall-void.log" 3)" == 3 ]] || fail "a provider stall next to a provider-degraded run was not VOID"

# A near miss is the recorded section with one edit: an awk program over the
# fixture, read twice so that `answer` holds the line of the last turn_started
# (the answer that stalled). An edit that changes nothing fails here rather
# than passing as RED for an unrelated reason. A control is an edit that keeps
# the signature, so it stays VOID.
edited() {
  local name="$1" program="$2" add="${3:-}"
  awk -v add="$add" 'NR == FNR { if (index($0, "kind: \"turn_started\"")) answer = FNR; next } '"$program" \
    "$stall" "$stall" > "$work/near-${name}.log"
  ! cmp -s "$stall" "$work/near-${name}.log" || fail "near miss ${name} left the recorded section unchanged"
}
near_miss() {
  edited "$1" "$3" "${4:-}"
  [[ "$(run_verdict "$work/near-$1.log" 3)" == 1 ]] || fail "near miss $1 was not RED: $2"
}
control() {
  edited "$1" "$3" "${4:-}"
  [[ "$(run_verdict "$work/near-$1.log" 3)" == 3 ]] || fail "control $1 was not VOID: $2"
}
# Insert `add` after everything the stalled answer logged, or at the start.
late='/ \.\.\. FAILED$/ { print add } { print }'
early='FNR == 2 { print add } { print }'
logged() { printf '2026-10-19T06:21:39.000000Z DEBUG %s: %s' "$1" "$2"; }
broker_target=meerkat_openai::public_live
observed() {
  logged "$broker_target" "public Live lowered a sideband observation observation=GptLiveBrokerObservation { kind: \"$1\", payload: \"<redacted>\" }"
}

# After the answer started, a provider that stalled leaves only the answer's
# transcript and snapshot deltas and its input clock. Any other observation
# the broker lowers is RED. The kinds are read from the broker's Debug arms,
# so a kind added later is covered when it is added.
broker=crates/meerkat-openai/src/gpt_live_broker.rs
pinned() { grep -qF -- "$2" "${ROOT}/$1" || fail "$1 no longer contains: $2 (update scripts/live-gate-verdict)"; }
pinned "$broker" '.debug_struct("GptLiveBrokerObservation")'
pinned "$broker" '.field("kind", &kind)'
arms="$(awk '/let kind = match self \{/ { f = 1; next } f && /^ *\};$/ { exit } f' "${ROOT}/${broker}")"
kinds="$(grep -o '"[a-z_]*"' <<<"$arms" | tr -d '"' || true)"
[[ -n "$kinds" && "$(wc -l <<<"$kinds")" -eq "$(grep -c 'Self::' <<<"$arms")" ]] \
  || fail "${broker}: expected one kind per GptLiveBrokerObservation arm (update this selftest)"
for kind in turn_started assistant_transcript_fragment turn_snapshot_delta provider_input_latency \
  user_speech_over_assistant assistant_playback_restorable; do
  grep -qx "$kind" <<<"$kinds" || fail "${broker} no longer lowers ${kind} (update scripts/live-gate-verdict)"
done
while read -r kind; do
  case "$kind" in
    assistant_transcript_fragment | turn_snapshot_delta | provider_input_latency)
      control "kind-${kind}" "the broker lowered ${kind} after the answer started" "$late" "$(observed "$kind")" ;;
    *) near_miss "kind-${kind}" "the broker lowered ${kind} after the answer started" "$late" "$(observed "$kind")" ;;
  esac
done <<<"$kinds"

# Any other line the broker logs after the answer started is RED too (a send,
# hold, release, acknowledgement, notice, transcription or input latency),
# except its reflected output audio. The messages are read from the broker's
# literals. The verdict knows a broker line by its tracing target or by its
# message prefix; each is checked without the other.
public_live=crates/meerkat-openai/src/public_live.rs
! grep -q 'target: "' "${ROOT}/${public_live}" \
  || fail "${public_live} sets a tracing target; its lines no longer carry ${broker_target} (update scripts/live-gate-verdict)"
messages="$(grep -o '"[^"]*public Live [^"]*"' "${ROOT}/${public_live}" | tr -d '"' | sed 's/{}/session context append/' | sort -u || true)"
[[ "$(wc -l <<<"$messages")" -ge 30 ]] || fail "${public_live}: fewer broker messages than expected (update this selftest)"
grep -qx 'public Live sideband reflected output audio' <<<"$messages" \
  || fail "${public_live} no longer logs reflected output audio (update scripts/live-gate-verdict)"
index=0
while read -r message; do
  index=$((index + 1))
  if [[ "$message" == "public Live sideband reflected output audio" ]]; then
    control "message-${index}" "$message" "$late" "$(logged "$broker_target" "$message frames=300")"
  else
    near_miss "message-${index}" "the broker logged after the answer started: $message" "$late" \
      "$(logged "$broker_target" "$message token=7")"
  fi
done <<<"$messages"
near_miss broker-target "a broker line without the message prefix" "$late" "$(logged "$broker_target" "a notice sent token=7")"
near_miss broker-prefix "a broker message from another module" "$late" \
  "$(logged "${broker_target}::notices" "public Live result cue sent token=7")"

# The answer itself: one fragment of it on the broker's own target, while the
# provider's reflected output kept arriving.
near_miss no-answer "the provider never started the answer" '!(FNR > answer && /kind: "assistant_transcript_fragment"/)'
near_miss answer-elsewhere "the answer's transcript was not the broker's" \
  'FNR > answer && /kind: "assistant_transcript_fragment"/ { sub(/ meerkat_openai::public_live: /, " meerkat_openai::public_live::replay: ") } { print }'
near_miss no-turn "the provider started no turn" '!/kind: "turn_started"/'
near_miss no-output-audio "no provider output audio after the answer started" \
  '!(FNR > answer && /public Live sideband reflected output audio /)'
near_miss ducked-before "the answer started into an unrestored duck" '!/kind: "assistant_playback_restorable"/'

# Anywhere in the run: a text-chat hold or release means the product measured
# the model's response as audible.
pinned "$public_live" "\"public Live text-chat context held: the model's response is audible\""
pinned "$public_live" '"public Live held text-chat context released"'
near_miss held-earlier "the product held text chat earlier in the run" "$early" \
  "$(logged "$broker_target" "public Live text-chat context held: the model's response is audible token=7")"
near_miss released-earlier "the product released held text chat earlier in the run" "$early" \
  "$(logged "$broker_target" "public Live held text-chat context released token=7")"

# The channel neither closed nor ended its observation stream. The run must log
# the adapter's target, and each scenario that enables it enables the close
# target too, so either end would show.
pinned crates/meerkat-live/src/lib.rs 'pub const LIVE_CLOSE_TRACE_TARGET: &str = "meerkat::live_close";'
pinned crates/meerkat/src/experimental_gpt_live.rs '"experimental live adapter observation stream ended"'
near_miss stream-ended "the observation stream ended during the answer" "$late" \
  "$(logged meerkat::experimental_gpt_live "experimental live adapter observation stream ended")"
near_miss closed "the channel closed during the answer" "$late" \
  "$(logged meerkat::live_close 'live close drain slice observed step="drain_slice"')"
near_miss no-adapter "the run does not log the adapter's target" '!/ meerkat::experimental_gpt_live: /'
filters="$(grep -o '"[^"]*meerkat::experimental_gpt_live=[^"]*"' "${ROOT}/tests/integration/tests/gpt_live_public_e2e.rs" || true)"
[[ -n "$filters" ]] || fail "gpt_live_public_e2e.rs: no scenario enables meerkat::experimental_gpt_live (update this selftest)"
! grep -v 'meerkat::live_close=' <<<"$filters" \
  || fail "gpt_live_public_e2e.rs: a scenario enables meerkat::experimental_gpt_live without meerkat::live_close"

# The browser's evidence: packets kept arriving and decoded to silence with a
# working track processor. Decoded frames are counted on the raw remote track,
# ahead of the playback gate, so a duck cannot silence them.
pinned tests/integration/tests/support/gpt_live_e2e.rs \
  '"timed out waiting for spoken output; decoded_frames={decoded_frames}, decoded_non_silent_frames={decoded_non_silent_frames}, max_decoded_rms={max_decoded_rms:.6}, processor_supported={processor_supported}, processor_errors={processor_errors}, sampled_frames={sampled_frames}, non_silent_frames={non_silent_frames}, max_rms={max_rms:.6}, bytes_received={bytes_received}, packets_received={packets_received}, total_audio_energy={total_audio_energy:?}, total_samples_received={total_samples_received:?}"'
harness=tests/live_smoke/browser/harness/gpt-live-peer-e2e.mjs
pinned "$harness" 'new MediaStreamTrackProcessor({ track: event.track })'
pinned "$harness" 'remoteAudio.decodedNonSilentFrames += audioData.numberOfFrames;'
pinned "$harness" 'inboundAudio.packets_received += Number(report.packetsReceived || 0);'
pinned "$harness" "['total_audio_energy', report.totalAudioEnergy],"
pinned "$harness" 'inboundAudio[key] = (inboundAudio[key] ?? 0) + value;'
near_miss audible "the browser decoded audible frames" '{ sub(/ decoded_non_silent_frames=0,/, " decoded_non_silent_frames=7200,"); print }'
near_miss audible-gated "the gated analyser heard audible frames" '{ sub(/ non_silent_frames=0,/, " non_silent_frames=3,"); print }'
near_miss no-processor "the browser could not decode the track" '{ sub(/ processor_supported=true,/, " processor_supported=false,"); print }'
near_miss processor-errors "the track processor failed" '{ sub(/ processor_errors=0,/, " processor_errors=1,"); print }'
near_miss no-track "the remote track delivered no frame" '{ sub(/ decoded_frames=[0-9]+,/, " decoded_frames=0,"); print }'
near_miss no-packets "no RTP packet arrived" '{ sub(/ packets_received=[0-9]+,/, " packets_received=0,"); print }'
near_miss energy "the received audio carried energy" \
  '{ sub(/ total_audio_energy=Some\(0\.0\),/, " total_audio_energy=Some(0.25),"); print }'
near_miss energy-unmeasured "the browser reported no audio energy" \
  '{ sub(/ total_audio_energy=Some\(0\.0\),/, " total_audio_energy=None,"); print }'
near_miss other-timeout "a timeout other than spoken output" \
  '{ sub(/timed out waiting for spoken output;/, "timed out waiting for provider events;"); print }'

# The setup-buildbuddy-ci composite action reads BUILDBUDDY_API_KEY from the
# job environment; a step-level env never reaches it (#1544's first runs
# failed at "Require BuildBuddy API key" with the secret configured).
job_env="$(awk '/^    env:$/{f=1; next} f && /^    [a-z]/{exit} f' "$workflow")"
grep -q '^      BUILDBUDDY_API_KEY: \${{ secrets.BUILDBUDDY_API_KEY }}$' <<<"$job_env" \
  || fail "live-gate.yml must set BUILDBUDDY_API_KEY in the job env for setup-buildbuddy-ci"

# Gate outputs live outside the workspace: the turbo-s targets take workspace
# files as inputs, and a log growing while Bazel hashes it failed every target
# with Exit 34 (#1544, digest mismatch on live-gate.log). Every redirect or
# tee in the workflow writes under $RUNNER_TEMP, and the runfiles glob never
# takes a log.
if grep -nE '(>>?|tee)[[:space:]]+"?[^"$[:space:]|]' "$workflow" | grep -v 'GITHUB_OUTPUT\|GITHUB_STEP_SUMMARY\|/dev/null' ; then
  fail "live-gate.yml writes a file outside \$RUNNER_TEMP (inside the workspace)"
fi
grep -q 'tee "\$out/live-gate.log"' "$workflow" \
  || fail "live-gate.yml must tee the gate log to \$RUNNER_TEMP/live-gate"
grep -q 'out="\${RUNNER_TEMP}/live-gate"' "$workflow" \
  || fail "live-gate.yml must root its outputs at \$RUNNER_TEMP/live-gate"
awk '/name = "workspace_runfiles"/{f=1} f&&/^\)/{exit} f' "${ROOT}/BUILD.bazel" | grep -q '"\*\*/\*.log"' \
  || fail "workspace_runfiles must exclude logs so a stray log is never a test input"

echo "live gate selftest: ok"
