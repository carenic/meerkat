#!/usr/bin/env bash
# Bounded TLC audit of the MobMachine member lifecycle.
#
# usage: member_lifecycle_audit.sh <max-steps> [--mutants] [extra tlc args...]
#
# Derives the TLC config from the generated ci.cfg next to this script (every
# generated constant and invariant, unchanged), then adds the audit's finite
# model values (one identity, two runtime ids, two fence tokens, generations
# 0..1, two sessions, the audit's strings), its action property and its step
# bound, and checks them over the whole bounded space with TLC coverage. The
# safety run must finish without an error, reach at least min_distinct_states
# distinct states (so a guard that silently disables arms fails the lane
# instead of shrinking the space), and fire every action of covered_actions
# (TLC coverage: each generated at least one successor state). It then reruns
# the same space once per reachability witness, with that witness's negation
# as an extra invariant (a state goal) or action property (a step goal), and
# requires TLC to report exactly that violation, so the properties are not
# vacuous. With --mutants it then seeds two defects into a copy of the
# generated model and requires TLC to refuse each: a progress arm without its
# observation_monotonic guard (AuditObservationMonotonic), and the same guard
# rendered as a plain `<=` between option records, which TLC cannot evaluate
# (the renderer now writes it through OptionLe). An anchor this script
# expects but cannot find fails the run. Each TLC run is held to the
# canonical lane's per-run cap (tlc_run_cap.sh) and fails closed as TLC
# INCOMPLETE.
set -euo pipefail

usage="usage: member_lifecycle_audit.sh <max-steps> [--mutants] [extra tlc args...]"
max_steps="${1:?${usage}}"
shift
run_mutants=false
if [[ "${1:-}" == "--mutants" ]]; then
  run_mutants=true
  shift
fi
# The lane's bound: every covered action fires and every witness is reached
# within 6 steps. The distinct-state floor is 90% of the 38738 states the
# first green run found there; a deeper bound only grows the space, so the
# floor holds there too.
lane_bound=6
states_at_lane_bound=38738
min_distinct_states=$(( states_at_lane_bound * 9 / 10 ))
if ! [[ "${max_steps}" =~ ^[0-9]+$ ]] || (( max_steps < lane_bound )); then
  echo "error: max-steps must be an integer >= ${lane_bound} (every covered action and witness needs ${lane_bound} steps)" >&2
  exit 2
fi

# Every arm the audit's domains can enable; each must fire.
covered_actions=(
  RecoverRosterMemberRunning
  RecoverMemberSessionBindingFreshRunning
  ObserveMemberProgressChangedOpenRunning
  ObserveMemberProgressChangedIdleRunning
  ObserveMemberProgressUnchangedIdleRunning
  ObserveMemberProgressUnchangedOpenHealthyRunning
  ObserveMemberProgressUnchangedOpenDegradedRunning
  ObserveMemberProgressUnchangedOpenWedgedRunning
  ObserveMemberProgressStaleRunning
  RetireRunningReleasing
  RetireRunningPreservingBinding
  RetireRunningNoBinding
  RetireStoppedReleasing
  RetireStoppedPreservingBinding
  RetireStoppedNoBinding
  RetireAbsentRunning
  RetireAbsentStopped
  RetireMember
  SessionIngressDetachedForMobDestroyRunning
  ObserveRuntimeRetired
  ObserveRuntimeRetiredStopped
  ObserveMemberRetirementArchivedLive
  ObserveMemberRetirementArchivedRetired
  ObserveMemberRetirementArchivedLiveStopped
  ObserveMemberRetirementArchivedRetiredStopped
  ObserveMemberRetirementArchivedAlreadyCleared
  ObserveMemberRetirementArchivedStaleRuntimeAlreadyCleared
  ObserveMemberRetirementArchivedAlreadyClearedStopped
  ObserveMemberRetirementArchivedStaleRuntimeAlreadyClearedStopped
  BeginPlacedCompletionLifecycleQuiesceFreshStop
  StopRunning
  ResumeStopped
)

spec_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ci_cfg="${spec_dir}/ci.cfg"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/member-lifecycle-audit.XXXXXX")"
# An interrupted or killed run ends its TLC child too (tlc_run_cap.sh).
trap 'if declare -F tlc_run_cap_reap >/dev/null; then tlc_run_cap_reap; fi; rm -rf "${work_dir}"' EXIT

replace_exact_line() {
  local from="$1" to="$2" file="$3"
  local count
  count="$(grep -cxF -- "${from}" "${file}" || true)"
  if [[ "${count}" != "1" ]]; then
    echo "error: expected exactly one line '${from}' in ${ci_cfg}, found ${count}" >&2
    exit 1
  fi
  FROM="${from}" TO="${to}" awk '$0 == ENVIRON["FROM"] { print ENVIRON["TO"]; next } { print }' \
    "${file}" > "${file}.next"
  mv "${file}.next" "${file}"
}

# usage: write_cfg <file> <extra invariant lines> <extra property lines>
write_cfg() {
  local cfg="$1" invariants="$2" properties="$3"
  cp "${ci_cfg}" "${cfg}"
  replace_exact_line "SPECIFICATION Spec" "SPECIFICATION AuditSpec" "${cfg}"
  replace_exact_line "  AgentIdentityValues = {}" '  AgentIdentityValues = {"agentidentity_1"}' "${cfg}"
  replace_exact_line "  AgentRuntimeIdValues = {}" '  AgentRuntimeIdValues = {"agentruntimeid_1", "agentruntimeid_2"}' "${cfg}"
  replace_exact_line "  FenceTokenValues = {1}" "  FenceTokenValues = {1, 2}" "${cfg}"
  replace_exact_line "  GenerationValues = {1}" "  GenerationValues = {0, 1}" "${cfg}"
  replace_exact_line "  MobIdValues = {}" '  MobIdValues = {"mobid_1"}' "${cfg}"
  replace_exact_line "  SessionIdValues = {}" '  SessionIdValues = {"sessionid_1", "sessionid_2"}' "${cfg}"
  replace_exact_line "  StringValues = {}" '  StringValues = {"profile_1", "token_1", "token_2"}' "${cfg}"
  replace_exact_line "CONSTANTS" "CONSTANTS
  AuditMaxSteps = ${max_steps}" "${cfg}"
  replace_exact_line "INVARIANTS" "PROPERTIES
  AuditObservationMonotonic${properties}
INVARIANTS${invariants}" "${cfg}"
  replace_exact_line "  CiStateConstraint" "  AuditStateConstraint" "${cfg}"
  printf 'CHECK_DEADLOCK FALSE\n' >> "${cfg}"
}

# The generated model's initial predicate needs a deep JVM stack on both the
# launcher (JDK_JAVA_OPTIONS) and the VM (JAVA_TOOL_OPTIONS); respect an
# explicit caller policy.
if [[ " ${JAVA_TOOL_OPTIONS:-} " != *" -Xss"* ]]; then
  export JAVA_TOOL_OPTIONS="-Xss512m -XX:+UseParallelGC${JAVA_TOOL_OPTIONS:+ ${JAVA_TOOL_OPTIONS}}"
fi
if [[ " ${JDK_JAVA_OPTIONS:-} " != *" -Xss"* ]]; then
  export JDK_JAVA_OPTIONS="-Xss512m${JDK_JAVA_OPTIONS:+ ${JDK_JAVA_OPTIONS}}"
fi
workers="${TLC_WORKERS:-auto}"
# shellcheck source=../meerkat_machine/tlc_run_cap.sh
source "${spec_dir}/../meerkat_machine/tlc_run_cap.sh"
cd "${spec_dir}"

# usage: run_tlc <name> <cfg> [extra tlc args...]; runs in the current
# directory and leaves the log at ${work_dir}/<name>.log
run_tlc() {
  local name="$1" cfg="$2"
  shift 2
  local status=0
  # Witness runs end in a violation by design: -noGenerateSpecTE keeps TLC
  # from writing trace-exploration files into the spec directory.
  tlc_run_capped member_lifecycle_audit "${name}" "${work_dir}/${name}.log" \
    -workers "${workers}" -metadir "${work_dir}/${name}-states" -noGenerateSpecTE \
    -config "${cfg}" "$@" member_lifecycle_audit.tla || status=$?
  return "${status}"
}

safety_cfg="${work_dir}/safety.cfg"
write_cfg "${safety_cfg}" "" ""
safety_status=0
run_tlc safety "${safety_cfg}" -coverage 1 "$@" || safety_status=$?
safety_log="${work_dir}/safety.log"
cat "${safety_log}"
if [[ "${safety_status}" != "0" ]] \
  || ! grep -q "Model checking completed. No error has been found." "${safety_log}"; then
  echo "error: member lifecycle audit failed (tlc exit ${safety_status}, bound ${max_steps})" >&2
  exit 1
fi

distinct="$(sed -nE 's/^[0-9]+ states generated, ([0-9]+) distinct states found, 0 states left on queue\.$/\1/p' \
  "${safety_log}" | tail -1)"
if [[ -z "${distinct}" ]]; then
  echo "error: the safety run reported no final distinct-state count" >&2
  exit 1
fi
if (( distinct < min_distinct_states )); then
  echo "error: ${distinct} distinct states at bound ${max_steps}, below the floor ${min_distinct_states} (90% of ${states_at_lane_bound} at bound ${lane_bound})" >&2
  exit 1
fi
echo "distinct states: ${distinct} (floor ${min_distinct_states})"

# TLC prints a coverage block every minute and one at the end; read the last.
# An action's line is
# `<Action line L, col C to line L, col C of module model>: D:T`, with D the
# new distinct states it found and T the successor states it generated; an
# action that fires only into known states has D = 0, so T counts.
coverage="${work_dir}/coverage.txt"
awk '/^The coverage statistics at / { block = ""; next } { block = block $0 "\n" } END { printf "%s", block }' \
  "${safety_log}" > "${coverage}"
for action in "${covered_actions[@]}"; do
  generated="$(awk -v head="<${action} line " '
    index($0, head) == 1 && $0 ~ / of module model>: [0-9]+:[0-9]+$/ {
      split($NF, counts, ":"); total += counts[2]; found = 1
    }
    END { if (found) print total }' "${coverage}")"
  if [[ -z "${generated}" || "${generated}" == "0" ]]; then
    echo "error: ${action} never fired at bound ${max_steps} (coverage '${generated:-absent}')" >&2
    exit 1
  fi
  echo "covered ${action}: ${generated} successor states"
done

# Each witness is <kind>:<name>: a state goal's negation joins the
# invariants, a step goal's negation ([][~step]_vars) joins the properties.
for witness in "Invariant:Bound" "Action property:ObservationAdvances" \
  "Invariant:Retiring" "Invariant:Archived" "Action property:Successor"; do
  kind="${witness%%:*}"
  name="${witness#*:}"
  cfg="${work_dir}/witness-${name}.cfg"
  if [[ "${kind}" == "Invariant" ]]; then
    write_cfg "${cfg}" "
  NotAuditWitness${name}" ""
  else
    write_cfg "${cfg}" "" "
  NotAuditWitness${name}"
  fi
  status=0
  run_tlc "witness-${name}" "${cfg}" "$@" || status=$?
  log="${work_dir}/witness-${name}.log"
  if ! grep -q "Error: ${kind} NotAuditWitness${name} is violated" "${log}"; then
    cat "${log}"
    echo "error: witness ${name} is not reachable at bound ${max_steps} (tlc exit ${status})" >&2
    exit 1
  fi
  depth="$(grep -cE '^State [0-9]+:' "${log}" || true)"
  echo "witness ${name} reached in ${depth} states"
done

if [[ "${run_mutants}" == "true" ]]; then
  # usage: mutate <name> <expected refusal regex>; reads a python edit on
  # stdin that rewrites model.tla in place and must find each anchor it names.
  mutate() {
    local name="$1" refusal="$2"
    shift 2
    local dir="${work_dir}/mutant-${name}"
    mkdir -p "${dir}"
    cp "${spec_dir}"/*.tla "${dir}/"
    MODEL="${dir}/model.tla" python3 -
    local cfg="${work_dir}/mutant-${name}.cfg"
    write_cfg "${cfg}" "" ""
    local status=0
    (cd "${dir}" && run_tlc "mutant-${name}" "${cfg}" "$@") || status=$?
    local log="${work_dir}/mutant-${name}.log"
    if ! grep -qE "${refusal}" "${log}"; then
      cat "${log}"
      echo "error: mutant ${name} was not refused by '${refusal}' (tlc exit ${status})" >&2
      exit 1
    fi
    echo "mutant ${name} refused by $(grep -oE "${refusal}" "${log}" | head -1)"
  }

  mutate observation_unguarded "Error: Action property AuditObservationMonotonic is violated" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
block = re.search(r"^ObserveMemberProgressChangedOpenRunning\(.*?\n\n", text, re.S | re.M)
assert block, "ObserveMemberProgressChangedOpenRunning not found"
body = block.group(0)
lines = body.split("\n")
guard = [i for i, line in enumerate(lines)
         if line.startswith("    /\\ OptionLe(") and "member_last_observed_at_ms" in line]
assert len(guard) == 1, "expected one observation_monotonic guard in ObserveMemberProgressChangedOpenRunning"
del lines[guard[0]]
text = text[: block.start()] + "\n".join(lines) + text[block.end():]
open(path, "w").write(text)
PY

  mutate plain_option_ordering "The first argument of <= should be an integer" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
block = re.search(r"^ObserveMemberProgressChangedOpenRunning\(.*?\n\n", text, re.S | re.M)
assert block, "ObserveMemberProgressChangedOpenRunning not found"
body = block.group(0)
match = re.search(r"^    /\\ OptionLe\((.*), Some\(observed_at_ms\)\)$", body, re.M)
assert match, "expected the observation_monotonic guard rendered through OptionLe"
plain = "    /\\ (" + match.group(1) + " <= Some(observed_at_ms))"
body = body[: match.start()] + plain + body[match.end():]
text = text[: block.start()] + body + text[block.end():]
open(path, "w").write(text)
PY
fi
echo "member lifecycle audit passed at model_step_count <= ${max_steps}"
