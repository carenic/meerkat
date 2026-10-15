#!/usr/bin/env bash
# Self-test for scripts/release-find-prebuild with a stubbed `gh` (no network).
#
# The lookup decides whether a tag run rebuilds every release artifact or
# publishes a pre-tag prebuild's: it must select only a complete prebuild of
# exactly the release commit (release.yml, workflow_dispatch, main, this
# repository, a successful "Release prebuild gate", downloadable artifacts),
# prefer the newest, wait for one still running, and fail loudly when the API
# fails instead of reporting "no prebuild".

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOOKUP="${REPO_ROOT}/scripts/release-find-prebuild"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-find-prebuild.XXXXXX")"
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
  echo "release-find-prebuild contract violated: $1" >&2
  shift
  for extra in "$@"; do
    echo "  ${extra}" >&2
  done
  exit 1
}

key_for() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9' '_'
}

runs_url="repos/${REPO}/actions/workflows/release.yml/runs?event=workflow_dispatch&branch=main&head_sha=${SHA}&per_page=100"

reset() {
  rm -rf "${FIXTURES}"
  mkdir -p "${FIXTURES}"
}

# run_json <id> <status> [path] [event] [branch] [sha] [repo] [started]
run_json() {
  local started="${8:-$(date -u -d '-1 minutes' +%Y-%m-%dT%H:%M:%SZ)}"
  printf '{"id": %s, "status": "%s", "path": "%s", "event": "%s", "head_branch": "%s", "head_sha": "%s", "repository": {"full_name": "%s"}, "run_started_at": "%s", "created_at": "%s"}' \
    "$1" "$2" "${3:-.github/workflows/release.yml}" "${4:-workflow_dispatch}" "${5:-main}" \
    "${6:-${SHA}}" "${7:-${REPO}}" "${started}" "${started}"
}

# runs <variant suffix or ""> <run json>...
runs() {
  local suffix="$1"
  shift
  local joined
  joined="$(IFS=,; printf '%s' "$*")"
  printf '{"workflow_runs": [%s]}\n' "${joined}" >"${FIXTURES}/$(key_for "${runs_url}")${suffix}.json"
}

# jobs <run id> <prebuild gate conclusion or "absent">
jobs() {
  local gate=""
  if [[ "$2" != "absent" ]]; then
    gate=", {\"name\": \"Release prebuild gate\", \"conclusion\": \"$2\"}"
  fi
  printf '{"jobs": [{"name": "Release binary build gate", "conclusion": "success"}%s]}\n' "${gate}" \
    >"${FIXTURES}/$(key_for "repos/${REPO}/actions/runs/$1/jobs?per_page=100").json"
}

# artifacts <run id> <name:expired>...
artifacts() {
  local run_id="$1"
  shift
  local entries=() entry
  for entry in "$@"; do
    entries+=("{\"name\": \"${entry%%:*}\", \"expired\": ${entry##*:}}")
  done
  local joined
  joined="$(IFS=,; printf '%s' "${entries[*]}")"
  printf '{"artifacts": [%s]}\n' "${joined}" \
    >"${FIXTURES}/$(key_for "repos/${REPO}/actions/runs/${run_id}/artifacts?per_page=100").json"
}

complete_artifacts() {
  artifacts "$1" \
    "meerkat-x86_64-unknown-linux-gnu-buildbuddy:false" \
    "meerkat-aarch64-unknown-linux-gnu-buildbuddy:false" \
    "meerkat-aarch64-apple-darwin-buildbuddy:false" \
    "meerkat-x86_64-apple-darwin-buildbuddy:false" \
    "meerkat-x86_64-pc-windows-msvc-windows-latest:false" \
    "rkat-web-package:false" \
    "buildbuddy-release-validate-1-1:true"
}

# lookup <label> <expected stdout> <expected status>
lookup() {
  local label="$1" expected="$2" expected_status="$3" output status
  set +e
  output="$(PATH="${TEST_ROOT}/bin:${PATH}" FIXTURES="${FIXTURES}" GITHUB_REPOSITORY="${REPO}" \
    MEERKAT_PREBUILD_POLL_SECONDS=0 MEERKAT_PREBUILD_WAIT_SECONDS="${WAIT_SECONDS:-600}" \
    "${LOOKUP}" "${SHA}" 2>"${TEST_ROOT}/${label}.err")"
  status=$?
  set -e
  if [[ "${status}" -ne "${expected_status}" || "${output}" != "${expected}" ]]; then
    fail "${label}: expected status ${expected_status} and output '${expected}', got ${status} and '${output}'" \
      "$(cat "${TEST_ROOT}/${label}.err")"
  fi
  echo "ok: ${label}"
}

export FIXTURES

# 1. No release.yml dispatch on main at the commit: no prebuild.
reset
runs ""
lookup no-runs "" 0

# 2. A complete prebuild of the commit is selected.
reset
runs "" "$(run_json 101 completed)"
jobs 101 success
complete_artifacts 101
lookup complete-prebuild 101 0

# 3. A dispatch without the prebuild gate (an asset or package recovery run
#    on main at the same commit) is not a prebuild.
reset
runs "" "$(run_json 102 completed)"
jobs 102 absent
complete_artifacts 102
lookup recovery-run "" 0

# 4. A prebuild whose gate failed is not reused.
reset
runs "" "$(run_json 103 completed)"
jobs 103 failure
complete_artifacts 103
lookup failed-gate "" 0

# 5. An expired archive, or a missing Windows or Web SDK artifact, is not
#    reusable.
reset
runs "" "$(run_json 104 completed)"
jobs 104 success
artifacts 104 "meerkat-x86_64-unknown-linux-gnu-buildbuddy:true" \
  "meerkat-x86_64-pc-windows-msvc-windows-latest:false" "rkat-web-package:false"
lookup expired-archive "" 0
artifacts 104 "meerkat-x86_64-unknown-linux-gnu-buildbuddy:false" "rkat-web-package:false"
lookup missing-windows "" 0
artifacts 104 "meerkat-x86_64-unknown-linux-gnu-buildbuddy:false" \
  "meerkat-x86_64-pc-windows-msvc-windows-latest:false"
lookup missing-web-sdk "" 0

# 6. Runs the API returns that are not exactly this commit's release.yml
#    dispatch on main in this repository are ignored, whatever their jobs.
reset
runs "" \
  "$(run_json 201 completed .github/workflows/ci.yml)" \
  "$(run_json 202 completed .github/workflows/release.yml push)" \
  "$(run_json 203 completed .github/workflows/release.yml workflow_dispatch release/v0.0.0)" \
  "$(run_json 204 completed .github/workflows/release.yml workflow_dispatch main "${OTHER_SHA}")" \
  "$(run_json 205 completed .github/workflows/release.yml workflow_dispatch main "${SHA}" fork/meerkat)"
for run_id in 201 202 203 204 205; do
  jobs "${run_id}" success
  complete_artifacts "${run_id}"
done
lookup foreign-runs "" 0

# 7. The newest complete prebuild wins.
reset
runs "" "$(run_json 301 completed)" "$(run_json 305 completed)" "$(run_json 303 completed)"
for run_id in 301 303 305; do
  jobs "${run_id}" success
  complete_artifacts "${run_id}"
done
lookup newest-prebuild 305 0

# 8. A prebuild still running is waited for and then selected.
reset
runs ".1" "$(run_json 401 in_progress)"
runs ".3" "$(run_json 401 completed)"
jobs 401 success
complete_artifacts 401
lookup waits-for-running 401 0
calls="$(cat "${FIXTURES}/.calls-$(key_for "${runs_url}")")"
[[ "${calls}" -eq 3 ]] || fail "waits-for-running: expected three listings while waiting, got ${calls}"

# 9. A run still going past the wait budget is not waited for forever.
reset
runs "" "$(run_json 501 in_progress .github/workflows/release.yml workflow_dispatch main "${SHA}" "${REPO}" \
  "$(date -u -d '-3 hours' +%Y-%m-%dT%H:%M:%SZ)")"
WAIT_SECONDS=600 lookup stale-running "" 0

# 10. An API failure fails the lookup instead of reading as "no prebuild".
reset
lookup api-failure "" 1
runs "" "$(run_json 601 completed)"
lookup jobs-api-failure "" 1
jobs 601 success
lookup artifacts-api-failure "" 1

# 11. A malformed commit is refused.
set +e
PATH="${TEST_ROOT}/bin:${PATH}" GITHUB_REPOSITORY="${REPO}" "${LOOKUP}" main >/dev/null 2>&1
status=$?
set -e
[[ "${status}" -eq 2 ]] || fail "malformed commit: expected status 2, got ${status}"
echo "ok: malformed-commit"

echo "release-find-prebuild contract holds"
