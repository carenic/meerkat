#!/usr/bin/env bash
# Self-test for scripts/verify-release-asset-provenance's --source-run path
# with a stubbed `gh` (no network): the archives of an earlier release.yml run
# are accepted only when that run is a dispatch on main at a commit in the
# checked-out history whose named build gate succeeded, every archive and its
# stamp verify against release.yml at that commit on refs/heads/main, and
# each stamp names the tag commit. The default gate is the full
# "Release binary build gate"; --source-gate "Release critical binary build
# gate" accepts the critical archives of a prebuild whose trailing lanes are
# still running, and no other job name is accepted.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERIFIER="${REPO_ROOT}/scripts/verify-release-asset-provenance"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-asset-provenance.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

REPO="example/meerkat"
RUN_ID=4242
mkdir -p "${TEST_ROOT}/bin" "${TEST_ROOT}/repo"

fail() {
  echo "release asset provenance contract violated: $1" >&2
  shift
  for extra in "$@"; do
    echo "  ${extra}" >&2
  done
  exit 1
}

git_in_repo() {
  git -C "${TEST_ROOT}/repo" -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false -c tag.gpgsign=false "$@"
}

git_in_repo init -q -b main
echo release >"${TEST_ROOT}/repo/file"
git_in_repo add file
git_in_repo commit -q -m release
git_in_repo tag v0.0.0
SHA="$(git_in_repo rev-parse HEAD)"

# gh api runs/<id> and runs/<id>/jobs, through real jq; gh attestation verify
# passes for --source-digest ${SHA} with --source-ref refs/heads/main.
cat >"${TEST_ROOT}/bin/gh" <<STUB
#!/usr/bin/env bash
set -euo pipefail
if [[ "\$1" == "attestation" ]]; then
  args="\$*"
  [[ "\${args}" == *"--source-digest ${SHA}"* && "\${args}" == *"--source-ref refs/heads/main"* ]]
  exit
fi
[[ "\$1" == "api" ]] || exit 2
shift
url="" filter="."
while [[ "\$#" -gt 0 ]]; do
  case "\$1" in
    --paginate) shift ;;
    --jq) filter="\$2"; shift 2 ;;
    *) url="\$1"; shift ;;
  esac
done
case "\${url}" in
  "repos/${REPO}/actions/runs/${RUN_ID}")
    printf '{"path": ".github/workflows/release.yml", "event": "workflow_dispatch", "head_branch": "main", "head_sha": "${SHA}", "repository": {"full_name": "${REPO}"}}' | jq -r "\${filter}"
    ;;
  "repos/${REPO}/actions/runs/${RUN_ID}/jobs?per_page=100")
    jq -r "\${filter}" "${TEST_ROOT}/jobs.json"
    ;;
  *) echo "stub gh: unexpected \${url}" >&2; exit 1 ;;
esac
STUB
chmod +x "${TEST_ROOT}/bin/gh"

mkdir -p "${TEST_ROOT}/artifacts" "${TEST_ROOT}/stamps"
echo archive >"${TEST_ROOT}/artifacts/rkat-0.0.0-x86_64-unknown-linux-gnu.tar.gz"
echo "${SHA}" >"${TEST_ROOT}/stamps/rkat-0.0.0-x86_64-unknown-linux-gnu.tar.gz.source-commit"

# gates <full gate conclusion> <critical gate conclusion>
gates() {
  printf '{"jobs": [{"name": "Release binary build gate", "conclusion": %s}, {"name": "Release critical binary build gate", "conclusion": %s}]}\n' \
    "$1" "$2" >"${TEST_ROOT}/jobs.json"
}

# verify <label> <expected status> [extra verifier options]
verify() {
  local label="$1" expected="$2" status
  shift 2
  set +e
  (cd "${TEST_ROOT}/repo" && PATH="${TEST_ROOT}/bin:${PATH}" GITHUB_REPOSITORY="${REPO}" \
    "${VERIFIER}" --source-run "${RUN_ID}" "$@" --stamps "${TEST_ROOT}/stamps" \
    "${TEST_ROOT}/artifacts" v0.0.0) >"${TEST_ROOT}/${label}.log" 2>&1
  status=$?
  set -e
  [[ "${status}" -eq "${expected}" ]] || fail "${label}: expected exit ${expected}, got ${status}" "$(cat "${TEST_ROOT}/${label}.log")"
  echo "ok: ${label}"
}

gates '"success"' '"success"'
verify complete-run 0
gates null '"success"'
verify full-gate-pending 1
grep -Fq "Release binary build gate did not succeed (null)" "${TEST_ROOT}/full-gate-pending.log" ||
  fail "full-gate-pending: the pending gate is not named" "$(cat "${TEST_ROOT}/full-gate-pending.log")"
verify critical-gate-accepts-running-trailing-lanes 0 --source-gate "Release critical binary build gate"
gates null '"failure"'
verify critical-gate-failed 1 --source-gate "Release critical binary build gate"
gates '"success"' '"success"'
verify unknown-gate 2 --source-gate "Release prebuild gate"
echo "${SHA%?}0" >"${TEST_ROOT}/stamps/rkat-0.0.0-x86_64-unknown-linux-gnu.tar.gz.source-commit"
verify stamp-names-another-commit 1

echo "release asset provenance contract holds"
