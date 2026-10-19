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
# than passing as RED for an unrelated reason.
near_miss() {
  local name="$1" why="$2" program="$3" add="${4:-}"
  awk -v add="$add" 'NR == FNR { if (index($0, "kind: \"turn_started\"")) answer = FNR; next } '"$program" \
    "$stall" "$stall" > "$work/near-${name}.log"
  ! cmp -s "$stall" "$work/near-${name}.log" || fail "near miss ${name} left the recorded section unchanged"
  [[ "$(run_verdict "$work/near-${name}.log" 3)" == 1 ]] || fail "near miss ${name} was not RED: ${why}"
}
# Insert `add` after everything the stalled answer logged.
late='/ \.\.\. FAILED$/ { print add } { print }'
observed() {
  printf '2026-10-19T06:21:39.000000Z DEBUG meerkat_openai::public_live: public Live lowered a sideband observation observation=GptLiveBrokerObservation { kind: "%s", payload: "<redacted>" }' "$1"
}
held="2026-10-19T06:21:39.000000Z  INFO meerkat_openai::public_live: public Live text-chat context held: the model's response is audible token=7"
near_miss duck-muted "the product ducked the answer" "$late" "$(observed user_speech_over_assistant)"
near_miss restored "the answer started ducked, then restored" "$late" "$(observed assistant_playback_restorable)"
near_miss ducked-before "the answer started into an unrestored duck" '!/kind: "assistant_playback_restorable"/'
near_miss held "the product held text chat behind an audible answer" "$late" "$held"
near_miss held-earlier "the product held text chat earlier in the run" 'FNR == 2 { print add } { print }' "$held"
near_miss released "the product released held text chat" "$late" \
  "2026-10-19T06:21:39.000000Z  INFO meerkat_openai::public_live: public Live held text-chat context released token=7"
near_miss no-answer "the provider never started the answer" '!(FNR > answer && /kind: "assistant_transcript_fragment"/)'
near_miss no-turn "the provider started no turn" '!/kind: "turn_started"/'
near_miss user-spoke "the user spoke after the answer started" "$late" "$(observed user_transcript_fragment)"
near_miss user-spoke-unlowered "the provider transcribed user speech after the answer started" "$late" \
  "2026-10-19T06:21:39.000000Z DEBUG meerkat_openai::public_live: public Live input transcript delta span start_ms=9000.0 end_ms=9200.0"
near_miss finished "the provider finished the answer" "$late" "$(observed turn_finished)"
near_miss finished-unlowered "the provider finished the answer, observation not lowered" "$late" \
  "2026-10-19T06:21:39.000000Z  INFO meerkat_openai::public_live: public Live provider turn finished; queued for the transcript projection turn=public-live-turn:2 role=Assistant"
near_miss no-output-audio "no provider output audio after the answer started" \
  '!(FNR > answer && /public Live sideband reflected output audio /)'
near_miss stream-ended "the observation stream ended during the answer" "$late" \
  "2026-10-19T06:21:39.000000Z  WARN meerkat::experimental_gpt_live: experimental live adapter observation stream ended"
near_miss closed "the channel closed during the answer" "$late" \
  '2026-10-19T06:21:39.000000Z  INFO meerkat::live_close: live close drain slice observed step="drain_slice"'
near_miss audible "the browser decoded audible frames" '{ sub(/ decoded_non_silent_frames=0,/, " decoded_non_silent_frames=7200,"); print }'
near_miss audible-gated "the gated analyser heard audible frames" '{ sub(/ non_silent_frames=0,/, " non_silent_frames=3,"); print }'
near_miss no-processor "the browser could not decode the track" '{ sub(/ processor_supported=true,/, " processor_supported=false,"); print }'
near_miss processor-errors "the track processor failed" '{ sub(/ processor_errors=0,/, " processor_errors=1,"); print }'
near_miss no-track "the remote track delivered no frame" '{ sub(/ decoded_frames=[0-9]+,/, " decoded_frames=0,"); print }'
near_miss other-timeout "a timeout other than spoken output" \
  '{ sub(/timed out waiting for spoken output;/, "timed out waiting for provider events;"); print }'

# The signature reads strings the product, the scenario harness and the
# browser harness write. Each must still be written where the verdict expects
# it: a renamed disqualifier (a hold, a duck, a finished turn, a channel end)
# would silently widen VOID.
pinned() { grep -qF -- "$2" "${ROOT}/$1" || fail "$1 no longer contains: $2 (update scripts/live-gate-verdict)"; }
public_live=crates/meerkat-openai/src/public_live.rs
pinned "$public_live" '"public Live lowered a sideband observation"'
pinned "$public_live" '"public Live provider turn finished; queued for the transcript projection"'
pinned "$public_live" '"public Live sideband reflected output audio"'
pinned "$public_live" "\"public Live text-chat context held: the model's response is audible\""
pinned "$public_live" '"public Live held text-chat context released"'
pinned "$public_live" '"public Live input transcript delta span"'
broker=crates/meerkat-openai/src/gpt_live_broker.rs
pinned "$broker" '.debug_struct("GptLiveBrokerObservation")'
pinned "$broker" '.field("kind", &kind)'
for kind in turn_started turn_finished assistant_transcript_fragment user_transcript_fragment user_speech_over_assistant \
  assistant_playback_restorable; do
  pinned "$broker" "=> \"${kind}\","
done
pinned crates/meerkat-live/src/lib.rs 'pub const LIVE_CLOSE_TRACE_TARGET: &str = "meerkat::live_close";'
pinned crates/meerkat/src/experimental_gpt_live.rs '"experimental live adapter observation stream ended"'
pinned tests/integration/tests/support/gpt_live_e2e.rs \
  '"timed out waiting for spoken output; decoded_frames={decoded_frames}, decoded_non_silent_frames={decoded_non_silent_frames}, max_decoded_rms={max_decoded_rms:.6}, processor_supported={processor_supported}, processor_errors={processor_errors}, sampled_frames={sampled_frames}, non_silent_frames={non_silent_frames},'
# Decoded frames are counted on the raw remote track, ahead of the playback
# gate: a duck cannot silence them.
pinned tests/live_smoke/browser/harness/gpt-live-peer-e2e.mjs 'new MediaStreamTrackProcessor({ track: event.track })'
pinned tests/live_smoke/browser/harness/gpt-live-peer-e2e.mjs 'remoteAudio.decodedNonSilentFrames += audioData.numberOfFrames;'

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
