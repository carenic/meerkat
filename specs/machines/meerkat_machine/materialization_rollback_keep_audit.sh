#!/usr/bin/env bash
# Bounded TLC audit of the Keep disposition: the rollback of a materialization
# that was refused before it served keeps every input its registration
# recovered from the store, and still stops an input admitted while the
# registration stood.
#
# usage: materialization_rollback_keep_audit.sh <max-steps> [--mutants] [extra tlc args...]
#
# Derives the TLC config from the generated ci.cfg next to this script (every
# generated constant and invariant, unchanged), then adds the audit's finite
# model values, its step bound and its invariants. Each restart is a new
# process, so the audit needs every generated variable's initial value as an
# action: it derives materialization_rollback_keep_fresh.tla
# (AuditFreshMachine) from the generated Init on every run and fails when
# Init and the declared variables disagree. TLC runs in a scratch copy of the
# specs. It checks the whole bounded space once (safety), then reruns it once
# per reachability witness with that witness's negation as an extra invariant
# and requires TLC to report exactly that violation, so the invariants are
# not vacuous. With --mutants it then seeds four defects into a copy of the
# generated model and requires TLC to refuse each with the invariant it
# breaks: the machine without the Keep arm (Keep stops like Stop), a Keep arm
# that ignores the recovery witness, an Abandon arm that overlaps Keep, and a
# Keep arm that ignores the disposition. An anchor this script expects but
# cannot find fails the run.
set -euo pipefail

usage="usage: materialization_rollback_keep_audit.sh <max-steps> [--mutants] [extra tlc args...]"
max_steps="${1:?${usage}}"
shift
run_mutants=false
if [[ "${1:-}" == "--mutants" ]]; then
  run_mutants=true
  shift
fi
if ! [[ "${max_steps}" =~ ^[0-9]+$ ]] || (( max_steps < 40 )); then
  echo "error: max-steps must be an integer >= 45 (the deepest witness is 40 steps)" >&2
  exit 2
fi

spec_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ci_cfg="${spec_dir}/ci.cfg"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/materialization-rollback-keep-audit.XXXXXX")"
# An interrupted or killed run ends its TLC child too (tlc_run_cap.sh).
trap 'if declare -F tlc_run_cap_reap >/dev/null; then tlc_run_cap_reap; fi; rm -rf "${work_dir}"' EXIT

# The scratch spec directory: the generated model, this audit, and the fresh
# machine derived from the generated Init.
model_dir="${work_dir}/specs"
mkdir -p "${model_dir}"
cp "${spec_dir}/model.tla" "${spec_dir}/materialization_rollback_keep_audit.tla" "${model_dir}/"
MODEL="${model_dir}/model.tla" OUT="${model_dir}/materialization_rollback_keep_fresh.tla" python3 - <<'PY'
import os, re
text = open(os.environ["MODEL"]).read()
declared = re.search(r"^VARIABLES (.*)$", text, re.M)
assert declared, "VARIABLES line not found in model.tla"
variables = [name.strip() for name in declared.group(1).split(",")]
init = re.search(r"^Init ==\n((?:    /\\ .*\n)+)\n", text, re.M)
assert init, "Init block not found in model.tla"
lines = init.group(1).rstrip("\n").split("\n")
assigned = []
fresh = []
for line in lines:
    match = re.fullmatch(r"    /\\ ([a-z_][a-z_0-9]*) = (.+)", line)
    assert match, f"Init line is not a plain assignment: {line[:120]}"
    name, value = match.groups()
    assigned.append(name)
    if name == "model_step_count":
        # The step bound counts every process.
        fresh.append("    /\\ model_step_count' = model_step_count + 1")
    else:
        fresh.append(f"    /\\ {name}' = {value}")
assert sorted(assigned) == sorted(variables), (
    "Init assigns a different variable set than VARIABLES declares: "
    f"missing {sorted(set(variables) - set(assigned))}, extra {sorted(set(assigned) - set(variables))}"
)
assert len(assigned) == len(set(assigned)), "Init assigns a variable twice"
with open(os.environ["OUT"], "w") as out:
    out.write("---- MODULE materialization_rollback_keep_fresh ----\n")
    out.write("\\* Derived by materialization_rollback_keep_audit.sh from the generated\n")
    out.write("\\* Init: a new process's machine, every variable at its initial value.\n")
    out.write("EXTENDS model\n\n")
    out.write("AuditFreshMachine ==\n")
    out.write("\n".join(fresh))
    out.write("\n====\n")
print(f"derived AuditFreshMachine from Init ({len(assigned)} variables)")
PY

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
  replace_exact_line "  InputIdValues = {}" '  InputIdValues = {"input_1", "input_2", "input_3"}' "${cfg}"
  replace_exact_line "  RunIdValues = {}" '  RunIdValues = {"runid_1"}' "${cfg}"
  replace_exact_line "  SessionIdValues = {}" '  SessionIdValues = {"sessionid_1"}' "${cfg}"
  replace_exact_line "  StringValues = {}" '  StringValues = {"input_1", "input_2", "input_3"}' "${cfg}"
  replace_exact_line "CONSTANTS" "CONSTANTS
  AuditMaxSteps = ${max_steps}" "${cfg}"
  # The verdict function comes first: an overlapping arm breaks it a state
  # before either answer is recorded.
  replace_exact_line "INVARIANTS" "INVARIANTS
  AuditRollbackVerdictIsAFunction
  AuditKeepEndsNoRecoveredInput
  AuditKeepStopsWhatItAdmitted
  AuditKeepOnlyUnderKeep
  AuditKeptInputSurvivesTheRollback${extra}" "${cfg}"
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
extra_tlc_args=("$@")
# Each TLC run is held to the canonical lane's per-run cap (fails closed as
# TLC INCOMPLETE, exit 3).
# shellcheck source=tlc_run_cap.sh
source "${spec_dir}/tlc_run_cap.sh"

# usage: run_tlc <name> <cfg> <dir>; leaves the log at ${work_dir}/<name>.log
run_tlc() {
  local name="$1" cfg="$2" dir="$3"
  tlc_status=0
  # Witness and mutant runs end in a violation by design: -noGenerateSpecTE
  # keeps TLC from writing trace-exploration files next to the specs.
  tlc_run_capped materialization_rollback_keep_audit "${name}" "${work_dir}/${name}.log" \
    -workers "${workers}" -metadir "${work_dir}/${name}-states" -noGenerateSpecTE \
    -config "${cfg}" "${extra_tlc_args[@]}" "${dir}/materialization_rollback_keep_audit.tla" || tlc_status=$?
}

safety_cfg="${work_dir}/safety.cfg"
write_cfg "${safety_cfg}" ""
run_tlc safety "${safety_cfg}" "${model_dir}"
cat "${work_dir}/safety.log"
if [[ "${tlc_status}" != "0" ]] \
  || ! grep -q "Model checking completed. No error has been found." "${work_dir}/safety.log"; then
  echo "error: materialization-rollback keep audit failed (tlc exit ${tlc_status}, bound ${max_steps})" >&2
  exit 1
fi

for witness in KeptQueuedInputRuns KeptRequeuedInputRuns AdmittedInputStoppedUnderKeep \
  StopRollbackEndsRecoveredInput; do
  cfg="${work_dir}/witness-${witness}.cfg"
  write_cfg "${cfg}" "
  NotAuditWitness${witness}"
  run_tlc "witness-${witness}" "${cfg}" "${model_dir}"
  log="${work_dir}/witness-${witness}.log"
  if ! grep -q "Error: Invariant NotAuditWitness${witness} is violated" "${log}"; then
    cat "${log}"
    echo "error: witness ${witness} is not reachable at bound ${max_steps} (tlc exit ${tlc_status})" >&2
    exit 1
  fi
  depth="$(grep -cE '^State [0-9]+:' "${log}" || true)"
  steps="$(grep -oE 'model_step_count = [0-9]+' "${log}" | tail -1 | grep -oE '[0-9]+$' || true)"
  echo "witness ${witness} reached in ${depth} states (model_step_count ${steps})"
done

if [[ "${run_mutants}" == "true" ]]; then
  keep_arms=""
  abandon_arms=""
  for arm_phase in Initializing Idle Attached Running Retired Stopped; do
    keep_arms+=" ClassifyStopInputDispositionKeep${arm_phase}"
    abandon_arms+=" ClassifyStopInputDispositionAbandon${arm_phase}"
  done

  # usage: mutate <name> <expected invariant>; reads a python edit on stdin
  # that rewrites model.tla in place (MODEL) and must find each anchor it
  # names. KEEP_ARMS and ABANDON_ARMS name the classifier arms.
  mutate() {
    local name="$1" property="$2"
    local dir="${work_dir}/mutant-${name}"
    mkdir -p "${dir}"
    cp "${model_dir}"/*.tla "${dir}/"
    MODEL="${dir}/model.tla" KEEP_ARMS="${keep_arms}" ABANDON_ARMS="${abandon_arms}" \
      PYTHONPATH="${work_dir}" python3 -
    local cfg="${work_dir}/mutant-${name}.cfg"
    write_cfg "${cfg}" ""
    run_tlc "mutant-${name}" "${cfg}" "${dir}"
    local log="${work_dir}/mutant-${name}.log"
    if ! grep -qE "Error: Invariant ${property} is violated" "${log}"; then
      cat "${log}"
      echo "error: mutant ${name} was not refused by ${property} (tlc exit ${tlc_status})" >&2
      exit 1
    fi
    echo "mutant ${name} refused by ${property}"
  }

  # Shared helper: replace the one guard line of each named arm that holds
  # every needle, exactly once per arm.
  cat > "${work_dir}/keep_mutation.py" <<'PY'
import os

KEEP_GUARD = (
    '((registration_phase = "Draining") /\\ (unregister_queued_inputs = "Keep") /\\ '
    '(input_id \\in DOMAIN input_phases) /\\ (input_id \\in recovered_admitted_inputs))'
)

def edit_arm_lines(text, arms_env, needle, replacement):
    for arm in os.environ[arms_env].split():
        start = text.index("\n" + arm + "(") + 1
        end = text.index("\n\n", start)
        lines = text[start:end].split("\n")
        hits = [i for i, line in enumerate(lines) if needle in line]
        assert len(hits) == 1, f"expected one {needle!r} line in {arm}, found {len(hits)}"
        lines[hits[0]] = replacement(lines[hits[0]])
        text = text[:start] + "\n".join(lines) + text[end:]
    return text
PY

  # The machine without Keep: the Keep arm never answers and the Abandon arm
  # answers every input, so a Keep rollback stops what its registration
  # recovered, as Stop does.
  mutate keep_stops_like_stop AuditKeepEndsNoRecoveredInput <<'PY'
import os
from keep_mutation import KEEP_GUARD, edit_arm_lines
path = os.environ["MODEL"]
text = open(path).read()
text = edit_arm_lines(text, "KEEP_ARMS", KEEP_GUARD, lambda line: "    /\\ FALSE")
text = edit_arm_lines(text, "ABANDON_ARMS", KEEP_GUARD, lambda line: "    /\\ TRUE")
open(path, "w").write(text)
PY

  # A Keep arm (and its Abandon exclusion) without the recovery witness keeps
  # the input the registration admitted.
  mutate keep_ignores_recovery AuditKeepStopsWhatItAdmitted <<'PY'
import os
from keep_mutation import KEEP_GUARD, edit_arm_lines
path = os.environ["MODEL"]
text = open(path).read()
drop = lambda line: line.replace(" /\\ (input_id \\in recovered_admitted_inputs)", "", 1)
text = edit_arm_lines(text, "KEEP_ARMS", KEEP_GUARD, drop)
text = edit_arm_lines(text, "ABANDON_ARMS", KEEP_GUARD, drop)
open(path, "w").write(text)
PY

  # An Abandon arm without the Keep exclusion overlaps the Keep arm.
  mutate abandon_overlaps_keep AuditRollbackVerdictIsAFunction <<'PY'
import os
from keep_mutation import KEEP_GUARD, edit_arm_lines
path = os.environ["MODEL"]
text = open(path).read()
text = edit_arm_lines(text, "ABANDON_ARMS", KEEP_GUARD, lambda line: "    /\\ TRUE")
open(path, "w").write(text)
PY

  # A Keep arm (and its Abandon exclusion) without the disposition keeps the
  # recovered inputs of a Stop rollback.
  mutate keep_ignores_disposition AuditKeepOnlyUnderKeep <<'PY'
import os
from keep_mutation import KEEP_GUARD, edit_arm_lines
path = os.environ["MODEL"]
text = open(path).read()
drop = lambda line: line.replace(' /\\ (unregister_queued_inputs = "Keep")', "", 1)
text = edit_arm_lines(text, "KEEP_ARMS", KEEP_GUARD, drop)
text = edit_arm_lines(text, "ABANDON_ARMS", KEEP_GUARD, drop)
open(path, "w").write(text)
PY
fi
echo "materialization-rollback keep audit passed at model_step_count <= ${max_steps}"
