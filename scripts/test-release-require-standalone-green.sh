#!/usr/bin/env bash
# Self-test for scripts/release-require-standalone-green with a stubbed `gh`
# (no network).
#
# The release workflow publishes crates only after this check: it must accept
# exactly a completed, successful published-crates-standalone.yml run on main
# at the release commit whose gate job passed, wait for one still running,
# refuse every other run (another commit, another branch or repository, a
# pull-request event, a failed run, a missing gate), and fail loudly when the
# API fails instead of reporting "no evidence".

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECK="${REPO_ROOT}/scripts/release-require-standalone-green"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-standalone-green.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

SHA="0123456789abcdef0123456789abcdef01234567"
OTHER_SHA="89abcdef0123456789abcdef0123456789abcdef"
REPO="example/meerkat"
FIXTURES="${TEST_ROOT}/fixtures"
mkdir -p "${TEST_ROOT}/bin"

# gh api [--paginate] <url> --jq <filter>: answers from
# ${FIXTURES}/<url with non-alphanumerics as _>[.<call number>].json, the
# highest-numbered variant not above this call's number, through real jq. A
# missing fixture is an API failure.
cat >"${TEST_ROOT}/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == "api" ]] || { echo "stub gh: unsupported $*" >&2; exit 2; }
shift
url="" filter="."
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --paginate) shift ;;
    --jq) filter="$2"; shift 2 ;;
    *) url="$1"; shift ;;
  esac
done
key="$(printf '%s' "${url}" | tr -c 'A-Za-z0-9' '_')"
count_file="${FIXTURES}/.calls-${key}"
calls=$(( $(cat "${count_file}" 2>/dev/null || echo 0) + 1 ))
echo "${calls}" >"${count_file}"
fixture=""
for ((n = calls; n >= 1; n--)); do
  if [[ -f "${FIXTURES}/${key}.${n}.json" ]]; then
    fixture="${FIXTURES}/${key}.${n}.json"
    break
  fi
done
[[ -n "${fixture}" || ! -f "${FIXTURES}/${key}.json" ]] || fixture="${FIXTURES}/${key}.json"
[[ -n "${fixture}" ]] || { echo "stub gh: HTTP 502 for ${url}" >&2; exit 1; }
jq -r "${filter}" "${fixture}"
STUB
chmod +x "${TEST_ROOT}/bin/gh"

fail() {
  echo "release-require-standalone-green contract violated: $1" >&2
  shift
  for extra in "$@"; do
    echo "  ${extra}" >&2
  done
  exit 1
}

key_for() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9' '_'
}

runs_url="repos/${REPO}/actions/workflows/published-crates-standalone.yml/runs?branch=main&head_sha=${SHA}&per_page=100"

reset() {
  rm -rf "${FIXTURES}"
  mkdir -p "${FIXTURES}"
}

# run_json <id> <status> <conclusion> [event] [branch] [sha] [repo] [path] [started]
run_json() {
  local started="${9:-$(date -u -d '-1 minutes' +%Y-%m-%dT%H:%M:%SZ)}"
  local conclusion="null"
  [[ -z "$3" ]] || conclusion="\"$3\""
  printf '{"id": %s, "status": "%s", "conclusion": %s, "path": "%s", "event": "%s", "head_branch": "%s", "head_sha": "%s", "repository": {"full_name": "%s"}, "run_started_at": "%s", "created_at": "%s"}' \
    "$1" "$2" "${conclusion}" "${8:-.github/workflows/published-crates-standalone.yml}" "${4:-workflow_dispatch}" \
    "${5:-main}" "${6:-${SHA}}" "${7:-${REPO}}" "${started}" "${started}"
}

# runs <variant suffix or ""> <run json>...
runs() {
  local suffix="$1"
  shift
  local joined
  joined="$(IFS=,; printf '%s' "$*")"
  printf '{"workflow_runs": [%s]}\n' "${joined}" >"${FIXTURES}/$(key_for "${runs_url}")${suffix}.json"
}

# jobs <run id> <gate conclusion, "pending" or "absent">
jobs() {
  local gate=""
  case "$2" in
    absent) ;;
    pending) gate=', {"name": "Published crates standalone gate", "conclusion": null}' ;;
    *) gate=", {\"name\": \"Published crates standalone gate\", \"conclusion\": \"$2\"}" ;;
  esac
  printf '{"jobs": [{"name": "Standalone census (1/8)", "conclusion": "success"}%s]}\n' "${gate}" \
    >"${FIXTURES}/$(key_for "repos/${REPO}/actions/runs/$1/jobs?per_page=100").json"
}

# check <label> <expected status> [sha]
check() {
  local label="$1" expected_status="$2" sha="${3:-${SHA}}" output status
  set +e
  output="$(PATH="${TEST_ROOT}/bin:${PATH}" FIXTURES="${FIXTURES}" GITHUB_REPOSITORY="${REPO}" \
    MEERKAT_STANDALONE_POLL_SECONDS=0 MEERKAT_STANDALONE_WAIT_SECONDS="${WAIT_SECONDS:-600}" \
    "${CHECK}" "${sha}" 2>&1)"
  status=$?
  set -e
  [[ "${status}" -eq "${expected_status}" ]] ||
    fail "${label}: exit ${status}, expected ${expected_status}" "${output}"
  printf '%s\n' "${output}"
}

# 1. A completed, successful run at the release commit whose gate passed.
reset
runs "" "$(run_json 11 completed success)"
jobs 11 success
check "successful run" 0 >/dev/null

# 2. A scheduled run counts too.
reset
runs "" "$(run_json 12 completed success schedule)"
jobs 12 success
check "scheduled run" 0 >/dev/null

# 3. No run at all fails and names the dispatch.
reset
runs ""
out="$(check "no run" 1)"
[[ "${out}" == *"gh workflow run published-crates-standalone.yml --ref main"* ]] ||
  fail "no run: the refusal must name the dispatch" "${out}"

# 4. Runs that are not evidence for this commit are refused, each for its own
# field: another commit, another branch, another repository, another workflow
# file, a pull-request event.
for label_and_run in \
  "other sha|$(run_json 21 completed success workflow_dispatch main "${OTHER_SHA}")" \
  "other branch|$(run_json 22 completed success workflow_dispatch feature)" \
  "other repository|$(run_json 23 completed success workflow_dispatch main "${SHA}" fork/meerkat)" \
  "other workflow|$(run_json 24 completed success workflow_dispatch main "${SHA}" "${REPO}" .github/workflows/ci.yml)" \
  "pull request event|$(run_json 25 completed success pull_request)"; do
  reset
  label="${label_and_run%%|*}"
  run="${label_and_run#*|}"
  runs "" "${run}"
  for id in 21 22 23 24 25; do jobs "${id}" success; done
  check "${label}" 1 >/dev/null
done

# 5. A failed run, or a successful run whose gate did not pass, is refused.
reset
runs "" "$(run_json 31 completed failure)"
jobs 31 failure
check "failed run" 1 >/dev/null
reset
runs "" "$(run_json 32 completed success)"
jobs 32 absent
check "run without the gate" 1 >/dev/null
reset
runs "" "$(run_json 33 completed success)"
jobs 33 skipped
check "skipped gate" 1 >/dev/null

# 6. An older successful run still counts when a newer one failed.
reset
runs "" "$(run_json 41 completed failure)" "$(run_json 40 completed success)"
jobs 41 failure
jobs 40 success
check "older success" 0 >/dev/null

# 7. A run still going is waited for and accepted once it passes.
reset
runs "" "$(run_json 51 in_progress "")"
runs ".2" "$(run_json 51 completed success)"
jobs 51 success
out="$(check "waited run" 0)"
[[ "${out}" == *"waiting for it"* ]] || fail "waited run: no wait was logged" "${out}"

# 8. A run still going past the wait window is not waited for.
reset
runs "" "$(run_json 61 in_progress "" workflow_dispatch main "${SHA}" "${REPO}" \
  .github/workflows/published-crates-standalone.yml "$(date -u -d '-2 hours' +%Y-%m-%dT%H:%M:%SZ)")"
WAIT_SECONDS=60 check "stale running run" 1 >/dev/null

# 9. API failures fail: the run listing, and the jobs of a matching run.
reset
check "listing failure" 1 >/dev/null
reset
runs "" "$(run_json 71 completed success)"
check "jobs failure" 1 >/dev/null

# 10. Only a full commit sha is accepted.
reset
check "short sha" 2 "0123456" >/dev/null

echo "release-require-standalone-green contracts hold"
