#!/usr/bin/env bash
# Bounded TLC audit of the member generation high-water.
#
# usage: generation_highwater_audit.sh <max-steps> [--mutants] [extra tlc args...]
#
# Derives the TLC config from the generated ci.cfg next to this script (every
# generated constant and invariant, unchanged), then adds the audit's finite
# model values (one identity, two runtime ids, two fence tokens, one profile,
# generations 0..2), its two action properties and its step bound, and checks
# them over the whole bounded space. It then reruns the same space once per
# reachability witness, with that witness's negation as an extra invariant,
# and requires TLC to report exactly that violation, so the properties are
# not vacuous. With --mutants it then seeds three defects into a copy of the
# generated model (a mint helper that ignores the high-water, a recovery
# without its seal guard, a recovery without its not-lowered guard) and
# requires TLC to refuse each with the property it breaks. A fourth mutant
# restores the exact-successor replay rule and requires both replay witnesses
# (a first generation above zero, a member reset gap) to become unreachable.
# An anchor this script expects but cannot find fails the run. Each TLC run is
# held to the canonical lane's per-run cap (tlc_run_cap.sh) and fails closed
# as TLC INCOMPLETE.
set -euo pipefail

usage="usage: generation_highwater_audit.sh <max-steps> [--mutants] [extra tlc args...]"
max_steps="${1:?${usage}}"
shift
run_mutants=false
if [[ "${1:-}" == "--mutants" ]]; then
  run_mutants=true
  shift
fi
if ! [[ "${max_steps}" =~ ^[0-9]+$ ]] || (( max_steps < 2 )); then
  echo "error: max-steps must be an integer >= 2 (the longest witness is 2 steps)" >&2
  exit 2
fi

spec_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ci_cfg="${spec_dir}/ci.cfg"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/generation-highwater-audit.XXXXXX")"
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

# usage: write_cfg <file> <extra invariant lines>
write_cfg() {
  local cfg="$1" extra="$2"
  cp "${ci_cfg}" "${cfg}"
  replace_exact_line "SPECIFICATION Spec" "SPECIFICATION AuditSpec" "${cfg}"
  replace_exact_line "  AgentIdentityValues = {}" '  AgentIdentityValues = {"agentidentity_1"}' "${cfg}"
  replace_exact_line "  AgentRuntimeIdValues = {}" '  AgentRuntimeIdValues = {"agentruntimeid_1", "agentruntimeid_2"}' "${cfg}"
  replace_exact_line "  FenceTokenValues = {1}" "  FenceTokenValues = {1, 2}" "${cfg}"
  replace_exact_line "  GenerationValues = {1}" "  GenerationValues = {0, 1, 2}" "${cfg}"
  replace_exact_line "  StringValues = {}" '  StringValues = {"profile_1"}' "${cfg}"
  replace_exact_line "CONSTANTS" "CONSTANTS
  AuditMaxSteps = ${max_steps}" "${cfg}"
  replace_exact_line "INVARIANTS" "PROPERTIES
  AuditSealClosesRecovery
  AuditHighwaterNeverLowers
INVARIANTS${extra}" "${cfg}"
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
  tlc_run_capped generation_highwater_audit "${name}" "${work_dir}/${name}.log" \
    -workers "${workers}" -metadir "${work_dir}/${name}-states" -noGenerateSpecTE \
    -config "${cfg}" "$@" generation_highwater_audit.tla || status=$?
  return "${status}"
}

safety_cfg="${work_dir}/safety.cfg"
write_cfg "${safety_cfg}" ""
safety_status=0
run_tlc safety "${safety_cfg}" "$@" || safety_status=$?
cat "${work_dir}/safety.log"
if [[ "${safety_status}" != "0" ]] \
  || ! grep -q "Model checking completed. No error has been found." "${work_dir}/safety.log"; then
  echo "error: generation high-water audit failed (tlc exit ${safety_status}, bound ${max_steps})" >&2
  exit 1
fi

for witness in Recorded Raised SealedWithHighwater RetainedBelowHighwater \
  FirstRecoveredAboveZero ResetGap; do
  cfg="${work_dir}/witness-${witness}.cfg"
  write_cfg "${cfg}" "
  NotAuditWitness${witness}"
  status=0
  run_tlc "witness-${witness}" "${cfg}" "$@" || status=$?
  log="${work_dir}/witness-${witness}.log"
  if ! grep -q "Error: Invariant NotAuditWitness${witness} is violated" "${log}"; then
    cat "${log}"
    echo "error: witness ${witness} is not reachable at bound ${max_steps} (tlc exit ${status})" >&2
    exit 1
  fi
  depth="$(grep -cE '^State [0-9]+:' "${log}" || true)"
  echo "witness ${witness} reached in ${depth} states"
done

if [[ "${run_mutants}" == "true" ]]; then
  # usage: mutate <name> <expected violation regex>; reads a python edit on
  # stdin that rewrites model.tla in place and must find each anchor it names.
  mutate() {
    local name="$1" violation="$2"
    shift 2
    local dir="${work_dir}/mutant-${name}"
    mkdir -p "${dir}"
    cp "${spec_dir}"/*.tla "${dir}/"
    MODEL="${dir}/model.tla" python3 -
    local cfg="${work_dir}/mutant-${name}.cfg"
    write_cfg "${cfg}" ""
    local status=0
    (cd "${dir}" && run_tlc "mutant-${name}" "${cfg}" "$@") || status=$?
    local log="${work_dir}/mutant-${name}.log"
    if ! grep -qE "Error: (${violation}) is violated" "${log}"; then
      cat "${log}"
      echo "error: mutant ${name} was not refused by ${violation} (tlc exit ${status})" >&2
      exit 1
    fi
    echo "mutant ${name} refused by $(grep -oE "Error: (${violation}) is violated" "${log}" | head -1)"
  }

  mutate minted_ignores_highwater "Invariant next_generation_above_highwater" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
block = re.search(r"^mob_machine_next_minted_generation\(.*?\n(?=\S)", text, re.S | re.M)
assert block, "mob_machine_next_minted_generation not found"
head = block.group(0).split("\n", 1)[0]
assert head.endswith("=="), "unexpected mob_machine_next_minted_generation header"
body = head + "\n    mob_machine_next_respawn_generation(arg_identity_runtime_generations, arg_agent_identity)\n"
text = text[: block.start()] + body + text[block.end():]
open(path, "w").write(text)
PY

  mutate recovery_ignores_seal "Action property AuditSealClosesRecovery" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
block = re.search(r"^RecoverIdentityGenerationHighwaterRunning\(.*?\n\n", text, re.S | re.M)
assert block, "RecoverIdentityGenerationHighwaterRunning not found"
old = "    /\\ (identity_generation_highwater_sealed = FALSE)\n"
body = block.group(0)
assert body.count(old) == 1, "expected one seal guard in RecoverIdentityGenerationHighwaterRunning"
body = body.replace(old, "")
text = text[: block.start()] + body + text[block.end():]
open(path, "w").write(text)
PY

  mutate recovery_lowers "Action property AuditHighwaterNeverLowers" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
block = re.search(r"^RecoverIdentityGenerationHighwaterRunning\(.*?\n\n", text, re.S | re.M)
assert block, "RecoverIdentityGenerationHighwaterRunning not found"
body = block.group(0)
lines = body.split("\n")
guard = [i for i, line in enumerate(lines)
         if "generation >= (identity_generation_highwater)[agent_identity]" in line]
assert len(guard) == 1, "expected one not-lowered guard in RecoverIdentityGenerationHighwaterRunning"
lines[guard[0]] = "    /\\ TRUE"
body = "\n".join(lines)
text = text[: block.start()] + body + text[block.end():]
open(path, "w").write(text)
PY

  # usage: mutate_unreachable <name> <witness...>; reads a python edit on
  # stdin like mutate, then requires every named witness to be unreachable
  # in the mutated model: TLC finishes without an error.
  mutate_unreachable() {
    local name="$1"
    shift
    local witnesses=()
    while [[ $# -gt 0 && "$1" != "--" ]]; do
      witnesses+=("$1")
      shift
    done
    [[ "${1:-}" == "--" ]] && shift
    local dir="${work_dir}/mutant-${name}"
    mkdir -p "${dir}"
    cp "${spec_dir}"/*.tla "${dir}/"
    MODEL="${dir}/model.tla" python3 -
    local witness
    for witness in "${witnesses[@]}"; do
      local cfg="${work_dir}/mutant-${name}-${witness}.cfg"
      write_cfg "${cfg}" "
  NotAuditWitness${witness}"
      local status=0
      (cd "${dir}" && run_tlc "mutant-${name}-${witness}" "${cfg}" "$@") || status=$?
      local log="${work_dir}/mutant-${name}-${witness}.log"
      if [[ "${status}" != "0" ]] \
        || ! grep -q "Model checking completed. No error has been found." "${log}"; then
        cat "${log}"
        echo "error: mutant ${name} still reaches witness ${witness} (tlc exit ${status})" >&2
        exit 1
      fi
      echo "mutant ${name} makes witness ${witness} unreachable"
    done
  }

  mutate_unreachable recovery_exact_successor FirstRecoveredAboveZero ResetGap -- "$@" <<'PY'
import os
path = os.environ["MODEL"]
text = open(path).read()
relaxed = (
    "    /\\ ((mob_machine_next_respawn_generation(identity_runtime_generations, agent_identity) # None)"
    " /\\ (generation >= (IF \"value\" \\in DOMAIN mob_machine_next_respawn_generation("
    "identity_runtime_generations, agent_identity) THEN mob_machine_next_respawn_generation("
    "identity_runtime_generations, agent_identity)[\"value\"] ELSE None)))\n"
)
exact = "    /\\ (Some(generation) = mob_machine_next_respawn_generation(identity_runtime_generations, agent_identity))\n"
count = text.count(relaxed)
assert count == 3, f"expected three ascending replay guards, found {count}"
text = text.replace(relaxed, exact)
open(path, "w").write(text)
PY
fi
echo "generation high-water audit passed at model_step_count <= ${max_steps}"
