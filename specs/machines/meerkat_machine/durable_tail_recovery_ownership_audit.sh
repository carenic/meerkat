#!/usr/bin/env bash
# TLC audit of the generated MeerkatMachine durable-tail recovery
# authorization over the drive's store-proven ownership: in both quiescent
# phases the AuthorizeDurableTailRecovery* arms partition the whole
# observation product, an unproven drive gates on the persisted facts as
# before ownership existed, ownership never admits another run or an
# undecodable row, an ExclusiveHost drive decides as a restart does over the
# same row read as quiescent, and an ExclusiveWriter drive decides as for a
# retired runtime and never continues a run.
#
# usage: durable_tail_recovery_ownership_audit.sh [--mutants] [extra tlc args...]
#
# Derives the TLC config from the generated ci.cfg next to this script (every
# generated constant and invariant, unchanged) and adds the audit's model
# values, invariants and walk bound. The audit evaluates ENABLED over every
# observation tuple in each state of a short walk (Idle and Retired, each
# with no terminal run, the candidate's run terminal, or another run
# terminal); that stands for every reachable state with no current run only
# while the arms are verdicts whose guards read no state but phase, the
# current run and the terminal run, so this script checks that premise on
# the generated model first and fails if it stops holding. It then checks
# the invariants (safety), and reruns once with the reachability witnesses'
# negations as the invariants, requiring TLC to report each violated. With
# --mutants it seeds eleven defects into a copy of the generated model and
# requires TLC to refuse each with the invariant it targets, checked alone
# beside AuditReportGaps, which names per state how many tuples the defect
# leaves with no arm or several, with an example.
# The defects:
#   - an unproven drive admitted over a non-quiescent row, or over a row
#     naming the candidate run (each alone);
#   - ownership admitting a row that names another run, or one that does not
#     decode;
#   - a writer continuing the run (the partition broken, and kept);
#   - the writer's retired cut dropped;
#   - the refusal of an unproven non-quiescent row dropped;
#   - a host refused over a row naming the candidate run;
#   - a host taking the writer's retired cuts (the partition kept);
#   - the Retired phase's continuable cut requiring a Retired lifecycle row
#     (a phase disjunct the schema lowering strips).
# An anchor this script expects but cannot find fails the run.
set -euo pipefail

run_mutants=false
if [[ "${1:-}" == "--mutants" ]]; then
  run_mutants=true
  shift
fi

spec_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ci_cfg="${spec_dir}/ci.cfg"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/durable-tail-recovery-ownership-audit.XXXXXX")"
# An interrupted or killed run ends its TLC child too (tlc_run_cap.sh).
trap 'if declare -F tlc_run_cap_reap >/dev/null; then tlc_run_cap_reap; fi; rm -rf "${work_dir}"' EXIT

# The premise: each authorization arm of the quiescent phases keeps its
# phase, and its guard reads no state variable but phase, the current run and
# the terminal run. The walk covers every value those take with no current
# run.
MODEL="${spec_dir}/model.tla" python3 - <<'PY'
import os, re
text = open(os.environ["MODEL"]).read()
declared = re.search(r"^VARIABLES (.*)$", text, re.M)
assert declared, "VARIABLES line not found in model.tla"
variables = {name.strip() for name in declared.group(1).split(",")}
allowed = {"phase", "current_run_id", "turn_terminal_run_id"}
arms = (
    "Commit", "Repair", "CommitCompletedRetainInputs", "Hold", "RefusePriorCommit",
    "HoldInputEvidence", "RepairContinuableRetired", "HoldContinuableRetired",
    "RefuseRunFacts", "RefusePersistedFacts",
)
names = [f"AuthorizeDurableTailRecovery{arm}{phase}" for arm in arms for phase in ("Idle", "Retired")]
names.append("AuthorizeDurableTailRecoveryContinueIdle")
defined = set(re.findall(r"(?m)^(AuthorizeDurableTailRecovery[A-Za-z]+)\(", text))
expected = set(names) | {
    f"AuthorizeDurableTailRecoveryRefuseNonQuiescent{phase}"
    for phase in ("Initializing", "Attached", "Running", "Stopped", "Destroyed")
}
assert defined == expected, f"the authorization arms changed: {sorted(defined ^ expected)}"
for arm in names:
    heads = re.findall(r"(?m)^" + arm + r"\(([^)]*)\) ==$", text)
    assert len(heads) == 1, f"expected one {arm} definition, found {len(heads)}"
    start = text.index("\n" + arm + "(") + 1
    end = text.index("\n\n", start)
    lines = text[start:end].split("\n")[1:]
    phase = "Idle" if arm.endswith("Idle") else "Retired"
    primed = [line for line in lines if "'" in line]
    assert f"    /\\ phase' = \"{phase}\"" in primed, f"{arm} no longer keeps phase {phase}"
    frames = [line for line in lines if re.fullmatch(r"    /\\ UnchangedFrame_[0-9a-f]+", line)]
    assert len(frames) == 1, f"{arm} has no single UNCHANGED frame"
    guards = [line for line in lines if "'" not in line and line not in frames]
    assert f"    /\\ phase = \"{phase}\"" in guards, f"{arm} is no longer guarded on phase {phase}"
    for line in guards:
        bare = re.sub(r'"[^"]*"', '""', line)
        read = set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", bare)) & variables
        assert read <= allowed, f"{arm} now reads state {sorted(read - allowed)}: the walk no longer stands for every state"
print("premise holds: the authorization arms are verdicts that read no state but phase, the current run and the terminal run")
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

audit_invariants="  AuditRecoveryIsTotalAndDisjoint
  AuditTerminalizedCandidateRefuses
  AuditUnprovenOwnershipGatesAsBefore
  AuditOwnershipNeverAdmitsAnotherRunOrAnUndecodableRow
  AuditHostRecoversAsARestartDoes
  AuditWriterRecoversAsRetired
  AuditWriterNeverContinues
  AuditEarlierIncarnationsRunOutcomes"

# usage: write_cfg <file> <invariant lines>; the audit's lines come before the
# generated invariants. TLC reports the first invariant a state violates, so a
# mutant run lists its target first.
write_cfg() {
  local cfg="$1" invariants="$2"
  cp "${ci_cfg}" "${cfg}"
  replace_exact_line "SPECIFICATION Spec" "SPECIFICATION AuditSpec" "${cfg}"
  replace_exact_line "  SessionIdValues = {}" '  SessionIdValues = {"session_1"}' "${cfg}"
  replace_exact_line "  RunIdValues = {}" '  RunIdValues = {"run_1", "run_2"}' "${cfg}"
  replace_exact_line "  StringValues = {}" '  StringValues = {"candidate_1"}' "${cfg}"
  replace_exact_line "INVARIANTS" "INVARIANTS
${invariants}" "${cfg}"
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

# usage: run_tlc <name> <cfg> <dir> [tlc args...]; leaves the log at
# ${work_dir}/<name>.log
run_tlc() {
  local name="$1" cfg="$2" dir="$3"
  shift 3
  tlc_status=0
  # Witness and mutant runs end in a violation by design: -noGenerateSpecTE
  # keeps TLC from writing trace-exploration files next to the specs.
  tlc_run_capped durable_tail_recovery_ownership_audit "${name}" "${work_dir}/${name}.log" \
    -workers "${workers}" -metadir "${work_dir}/${name}-states" -noGenerateSpecTE \
    -config "${cfg}" "$@" "${extra_tlc_args[@]}" "${dir}/durable_tail_recovery_ownership_audit.tla" \
    || tlc_status=$?
}

safety_cfg="${work_dir}/safety.cfg"
write_cfg "${safety_cfg}" "${audit_invariants}"
run_tlc safety "${safety_cfg}" "${spec_dir}"
cat "${work_dir}/safety.log"
if [[ "${tlc_status}" != "0" ]] \
  || ! grep -q "Model checking completed. No error has been found." "${work_dir}/safety.log"; then
  echo "error: durable-tail recovery ownership audit failed (tlc exit ${tlc_status})" >&2
  exit 1
fi

# One run for the four witnesses: each holds in different states, so with
# -continue TLC reports every negation the walk violates.
witness_cfg="${work_dir}/witnesses.cfg"
write_cfg "${witness_cfg}" "  NotAuditWitnessIdle
  NotAuditWitnessRetired
  NotAuditWitnessTerminalized
  NotAuditWitnessOtherTerminal"
run_tlc witnesses "${witness_cfg}" "${spec_dir}" -continue
for witness in Idle Retired Terminalized OtherTerminal; do
  if ! grep -q "Error: Invariant NotAuditWitness${witness} is violated" "${work_dir}/witnesses.log"; then
    cat "${work_dir}/witnesses.log"
    echo "error: witness ${witness} is not reached (tlc exit ${tlc_status})" >&2
    exit 1
  fi
  echo "witness ${witness} reached"
done

if [[ "${run_mutants}" == "true" ]]; then
  # usage: mutate <name> <target invariant> <arm> <old> <new> [<arm> <old> <new>...]
  # Copies the specs, replaces exactly one <old> by <new> in each named arm,
  # runs the target with AuditReportGaps (-continue, so every state reports),
  # and requires TLC to report the target violated.
  mutate() {
    local name="$1" target="$2"
    shift 2
    local dir="${work_dir}/mutant-${name}"
    mkdir -p "${dir}"
    cp "${spec_dir}/model.tla" "${spec_dir}/durable_tail_recovery_ownership_audit.tla" "${dir}/"
    MODEL="${dir}/model.tla" python3 - "$@" <<'PY'
import os, sys
path = os.environ["MODEL"]
text = open(path).read()
edits = sys.argv[1:]
assert edits and len(edits) % 3 == 0, "edits come in (arm, old, new) triples"
for i in range(0, len(edits), 3):
    arm, old, new = edits[i:i + 3]
    start = text.index("\n" + arm + "(") + 1
    end = text.index("\n\n", start)
    block = text[start:end]
    count = block.count(old)
    assert count == 1, f"expected one {old!r} in {arm}, found {count}"
    text = text[:start] + block.replace(old, new) + text[end:]
open(path, "w").write(text)
PY
    local cfg="${work_dir}/mutant-${name}.cfg"
    # The gap report first (always true, so every state prints it), then the
    # target alone: the safety run already checks the others.
    write_cfg "${cfg}" "  AuditReportGaps
  ${target}"
    run_tlc "mutant-${name}" "${cfg}" "${dir}" -continue
    local log="${work_dir}/mutant-${name}.log"
    if ! grep -q "Error: Invariant ${target} is violated" "${log}"; then
      cat "${log}"
      echo "error: mutant ${name} was not refused by ${target} (tlc exit ${tlc_status})" >&2
      exit 1
    fi
    local gaps
    # TLC pretty-prints a long PrintT value over several lines.
    gaps="$(LOG="${log}" python3 -c '
import os, re
text = open(os.environ["LOG"]).read()
found = re.findall(r"<<\s*\"tuples with.*?(?:>>\s*>>|\"none\">>)", text, re.S)
seen = sorted(set(" ".join(item.split()) for item in found))
print("; ".join(item for item in seen if not item.endswith("0, \"none\">>")) or "no tuple without an arm or with several")
')"
    echo "mutant ${name} refused by ${target}; ${gaps}"
  }

  P='AuthorizeDurableTailRecovery'
  unproven_nonquiescent='((observed_lifecycle = "NonQuiescent") /\ (ownership # "NotProven"))'
  unproven_candidate='((observed_current_run = "CandidateRun") /\ (ownership # "NotProven"))'
  current_run_admitted='(IF (observed_current_run = "NoRun") THEN TRUE ELSE ((observed_current_run = "CandidateRun") /\ (ownership # "NotProven")))'
  writer_retired='(IF (observed_lifecycle = "Retired") THEN TRUE ELSE (ownership = "ExclusiveWriter"))'

  # Ownership gates as before: an unproven drive admitted over an earlier
  # incarnation's row (each fact alone).
  mutate unproven_admits_non_quiescent_row AuditUnprovenOwnershipGatesAsBefore \
    "${P}CommitIdle" "${unproven_nonquiescent}" '(observed_lifecycle = "NonQuiescent")'
  mutate unproven_admits_candidate_run AuditUnprovenOwnershipGatesAsBefore \
    "${P}RepairIdle" "${unproven_candidate}" '(observed_current_run = "CandidateRun")'
  # Ownership admits a row naming another run, or one that does not decode.
  mutate ownership_admits_another_run AuditOwnershipNeverAdmitsAnotherRunOrAnUndecodableRow \
    "${P}ContinueIdle" "${current_run_admitted}" \
    '(IF (observed_current_run = "NoRun") THEN TRUE ELSE (ownership # "NotProven"))'
  mutate ownership_admits_an_undecodable_row AuditOwnershipNeverAdmitsAnotherRunOrAnUndecodableRow \
    "${P}ContinueIdle" "${unproven_nonquiescent}" '(ownership # "NotProven")'
  # A writer continues: with the retired cuts still taken (partition broken),
  # and with them dropped for it (partition kept).
  mutate writer_continues AuditWriterNeverContinues \
    "${P}ContinueIdle" '(ownership # "ExclusiveWriter")' 'TRUE'
  mutate writer_continues_alone AuditWriterRecoversAsRetired \
    "${P}ContinueIdle" '(ownership # "ExclusiveWriter")' 'TRUE' \
    "${P}RepairContinuableRetiredIdle" "${writer_retired}" '(observed_lifecycle = "Retired")' \
    "${P}HoldContinuableRetiredIdle" "${writer_retired}" '(observed_lifecycle = "Retired")'
  # The writer's retired cut dropped: its continuable tail has no arm.
  mutate writer_retired_cut_dropped AuditRecoveryIsTotalAndDisjoint \
    "${P}RepairContinuableRetiredIdle" "${writer_retired}" '(observed_lifecycle = "Retired")'
  # The refusal of an unproven non-quiescent row dropped.
  mutate refusal_drops_unproven_non_quiescent_row AuditRecoveryIsTotalAndDisjoint \
    "${P}RefusePersistedFactsIdle" \
    '(IF ((ownership = "NotProven") /\ (observed_lifecycle = "NonQuiescent")) THEN TRUE ELSE' \
    '(IF FALSE THEN TRUE ELSE'
  # A host refused over a row naming the candidate run.
  mutate host_refused_over_candidate_run AuditHostRecoversAsARestartDoes \
    "${P}RefusePersistedFactsIdle" '((ownership = "NotProven") /\ (observed_current_run = "CandidateRun"))' \
    '(observed_current_run = "CandidateRun")'
  # A host takes the writer's retired cuts (the partition kept).
  mutate host_takes_retired_cuts AuditHostRecoversAsARestartDoes \
    "${P}ContinueIdle" '(ownership # "ExclusiveWriter")' '(ownership = "NotProven")' \
    "${P}RepairContinuableRetiredIdle" '(ownership = "ExclusiveWriter")' '(ownership # "NotProven")' \
    "${P}HoldContinuableRetiredIdle" '(ownership = "ExclusiveWriter")' '(ownership # "NotProven")'
  # The Retired phase's continuable cut requires a Retired lifecycle row, as
  # the stripped `self.lifecycle_phase` disjunct left it.
  mutate retired_phase_cut_needs_retired_row AuditRecoveryIsTotalAndDisjoint \
    "${P}RepairContinuableRetiredRetired" '(class = "ContinuableCandidate")' \
    '(class = "ContinuableCandidate") /\ (observed_lifecycle = "Retired")'
fi
echo "durable-tail recovery ownership audit passed over the whole observation product"
