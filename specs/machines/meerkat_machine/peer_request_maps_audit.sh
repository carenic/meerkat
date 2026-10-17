#!/usr/bin/env bash
# Bounded TLC audit of the peer request identity maps: the addressee each
# outbound request was sent to and the requester each inbound request came
# from.
#
# usage: peer_request_maps_audit.sh <max-steps> [--mutants] [extra tlc args...]
#
# Derives the TLC config from the generated ci.cfg next to this script (every
# generated constant and invariant, unchanged), then gives the audit one
# correlation id, two peers and one session, adds its invariant, its three
# action properties and its step bound, and checks them over the whole
# bounded space. It then reruns the same space once per reachability witness,
# with that witness's negation as an extra invariant, and requires TLC to
# report exactly that violation, so the invariants are not vacuous. With
# --mutants it then seeds five defects into a copy of the generated model (a
# terminal response that ignores its responder, swapped sender verdicts,
# swapped addressee verdicts, a send that records no addressee, a receipt that
# records no requester) and requires TLC to refuse each with the property or
# invariant it breaks. An anchor this script expects but cannot find fails
# the run. Each TLC run is held to the canonical lane's per-run cap
# (`TLC_RUN_CAP_SECS`, default 900 s, see tlc_run_cap.sh); a run that hits it
# fails closed as TLC INCOMPLETE (exit 3).
set -euo pipefail

usage="usage: peer_request_maps_audit.sh <max-steps> [--mutants] [extra tlc args...]"
max_steps="${1:?${usage}}"
shift
run_mutants=false
if [[ "${1:-}" == "--mutants" ]]; then
  run_mutants=true
  shift
fi
if ! [[ "${max_steps}" =~ ^[0-9]+$ ]] || (( max_steps < 5 )); then
  echo "error: max-steps must be an integer >= 5 (the longest witness is 5 steps)" >&2
  exit 2
fi

spec_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ci_cfg="${spec_dir}/ci.cfg"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/peer-request-maps-audit.XXXXXX")"
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
  replace_exact_line "  PeerCorrelationIdValues = {}" '  PeerCorrelationIdValues = {"corr_1"}' "${cfg}"
  replace_exact_line "  PeerIdValues = {}" '  PeerIdValues = {"peer_1", "peer_2"}' "${cfg}"
  replace_exact_line "  SessionIdValues = {}" '  SessionIdValues = {"sessionid_1"}' "${cfg}"
  replace_exact_line "CONSTANTS" "CONSTANTS
  AuditMaxSteps = ${max_steps}" "${cfg}"
  replace_exact_line "INVARIANTS" "PROPERTIES
  AuditResponsesOnlyFromAddressee
  AuditSenderVerdictExact
  AuditAddresseeVerdictExact
INVARIANTS
  AuditVerdictsTotal${extra}" "${cfg}"
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
# shellcheck source=tlc_run_cap.sh
source "${spec_dir}/tlc_run_cap.sh"
cd "${spec_dir}"

# usage: run_tlc <name> <cfg> [extra tlc args...]; runs in the current
# directory and leaves the log at ${work_dir}/<name>.log
run_tlc() {
  local name="$1" cfg="$2"
  shift 2
  local status=0
  # Witness runs end in a violation by design: -noGenerateSpecTE keeps TLC
  # from writing trace-exploration files into the spec directory.
  tlc_run_capped peer_request_maps_audit "${name}" "${work_dir}/${name}.log" \
    -workers "${workers}" -metadir "${work_dir}/${name}-states" -noGenerateSpecTE \
    -config "${cfg}" "$@" peer_request_maps_audit.tla || status=$?
  return "${status}"
}

safety_cfg="${work_dir}/safety.cfg"
write_cfg "${safety_cfg}" ""
safety_status=0
run_tlc safety "${safety_cfg}" "$@" || safety_status=$?
cat "${work_dir}/safety.log"
if [[ "${safety_status}" != "0" ]] \
  || ! grep -q "Model checking completed. No error has been found." "${work_dir}/safety.log"; then
  echo "error: peer request maps audit failed (tlc exit ${safety_status}, bound ${max_steps})" >&2
  exit 1
fi

for witness in Answered ProgressThenAnswered NotAddressee UnknownAfterAnswer Replied \
  NotRequester; do
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
    local name="$1" expected="$2"
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
    if ! grep -qE "${expected}" "${log}"; then
      cat "${log}"
      echo "error: mutant ${name} was not refused by '${expected}' (tlc exit ${status})" >&2
      exit 1
    fi
    echo "mutant ${name} refused: ${expected}"
  }

  mutate terminal_ignores_responder "property AuditResponsesOnlyFromAddressee is violated" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
guard = ("    /\\ ((IF ~((corr_id \\in DOMAIN pending_peer_request_addressees)) THEN TRUE ELSE "
         "(corr_id \\in DOMAIN pending_peer_request_addressees)) /\\ ((corr_id \\in DOMAIN "
         "pending_peer_request_addressees) /\\ ((pending_peer_request_addressees)[corr_id] = responder)))\n")
pattern = re.compile(
    r"^PeerResponseTerminalArrived(?:Completed|Failed)(?:Idle|Attached|Running|Retired|Stopped)\(.*?\n\n",
    re.S | re.M,
)
hits = 0
def drop(block):
    global hits
    body = block.group(0)
    assert body.count(guard) == 1, f"responder guard not found once in {body.splitlines()[0]}"
    hits += 1
    return body.replace(guard, "    /\\ TRUE\n")
text = pattern.sub(drop, text)
assert hits == 10, f"expected 10 terminal arms, found {hits}"
open(path, "w").write(text)
PY

  mutate sender_verdicts_swapped "property AuditSenderVerdictExact is violated" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
eq = "((pending_peer_request_addressees)[corr_id] = responder)"
ne = "((pending_peer_request_addressees)[corr_id] # responder)"
def swap(prefix, old, new, expected):
    global text
    pattern = re.compile(r"^" + prefix + r"(?:Idle|Attached|Running|Retired|Stopped|Destroyed)\(.*?\n\n", re.S | re.M)
    hits = 0
    def edit(block):
        nonlocal hits
        body = block.group(0)
        assert body.count(old) == 1, f"'{old}' not found once in {body.splitlines()[0]}"
        hits += 1
        return body.replace(old, new)
    text = pattern.sub(edit, text)
    assert hits == expected, f"expected {expected} {prefix} arms, found {hits}"
swap("ResolvePeerResponseSenderAddressee", eq, "__NE__", 6)
swap("ResolvePeerResponseSenderNotAddressee", ne, eq, 6)
text = text.replace("__NE__", ne)
open(path, "w").write(text)
PY

  mutate addressee_verdicts_swapped "property AuditAddresseeVerdictExact is violated" "$@" <<'PY'
import os, re
path = os.environ["MODEL"]
text = open(path).read()
eq = "((inbound_peer_request_requesters)[corr_id] = addressee)"
ne = "((inbound_peer_request_requesters)[corr_id] # addressee)"
def swap(prefix, old, new, expected):
    global text
    pattern = re.compile(r"^" + prefix + r"(?:Idle|Attached|Running|Retired|Stopped|Destroyed)\(.*?\n\n", re.S | re.M)
    hits = 0
    def edit(block):
        nonlocal hits
        body = block.group(0)
        assert body.count(old) == 1, f"'{old}' not found once in {body.splitlines()[0]}"
        hits += 1
        return body.replace(old, new)
    text = pattern.sub(edit, text)
    assert hits == expected, f"expected {expected} {prefix} arms, found {hits}"
swap("ResolvePeerResponseAddresseeRequester", eq, "__NE__", 6)
swap("ResolvePeerResponseAddresseeNotRequester", ne, eq, 6)
text = text.replace("__NE__", ne)
open(path, "w").write(text)
PY

  mutate send_records_no_addressee "Invariant pending_peer_request_addressees_match_requests is violated" "$@" <<'PY'
import os
path = os.environ["MODEL"]
text = open(path).read()
old = "pending_peer_request_addressees' = MapSet(pending_peer_request_addressees, corr_id, addressee)"
count = text.count(old)
assert count == 5, f"expected 5 send updates, found {count}"
text = text.replace(old, "pending_peer_request_addressees' = pending_peer_request_addressees")
open(path, "w").write(text)
PY

  mutate receipt_records_no_requester "Invariant inbound_peer_request_requesters_match_requests is violated" "$@" <<'PY'
import os
path = os.environ["MODEL"]
text = open(path).read()
old = "inbound_peer_request_requesters' = MapSet(inbound_peer_request_requesters, corr_id, requester)"
count = text.count(old)
assert count == 6, f"expected 6 receipt updates, found {count}"
text = text.replace(old, "inbound_peer_request_requesters' = inbound_peer_request_requesters")
open(path, "w").write(text)
PY
fi
echo "peer request maps audit passed at model_step_count <= ${max_steps}"
