#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-ci-nextest-archive.XXXXXX")"
trap 'rm -rf -- "$TEST_ROOT"' EXIT

LOG="${TEST_ROOT}/commands.log"
FAKE_CARGO="${TEST_ROOT}/repo-cargo"
FAKE_NEXTEST="${TEST_ROOT}/cargo-nextest"

cat >"$FAKE_CARGO" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'cargo' >>"$COMMAND_LOG"
printf ' <%s>' "$@" >>"$COMMAND_LOG"
printf '\n' >>"$COMMAND_LOG"
archive_file=""
while (($#)); do
  if [[ "$1" == "--archive-file" ]]; then
    archive_file="$2"
    break
  fi
  shift
done
[[ -n "$archive_file" ]] || exit 91
printf 'archive\n' >"$archive_file"
EOF

cat >"$FAKE_NEXTEST" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "${MEERKAT_WORKSPACE_ROOT:-}" == "${EXPECTED_WORKSPACE_ROOT:?}" ]] || {
  echo "archived run omitted MEERKAT_WORKSPACE_ROOT" >&2
  exit 92
}
printf 'nextest' >>"$COMMAND_LOG"
printf ' <%s>' "$@" >>"$COMMAND_LOG"
printf '\n' >>"$COMMAND_LOG"
EOF

chmod +x "$FAKE_CARGO" "$FAKE_NEXTEST"
export COMMAND_LOG="$LOG"
export EXPECTED_WORKSPACE_ROOT="$ROOT"
export ROOT
export CARGO="$FAKE_CARGO"
export NEXTEST_BIN="$FAKE_NEXTEST"

families=(unit int-heavy int-mob int-everything-else)
for family in "${families[@]}"; do
  archive="${TEST_ROOT}/${family}.tar.zst"
  "$ROOT/scripts/ci-nextest-archive.sh" build "$family" "$archive"
  "$ROOT/scripts/ci-nextest-archive.sh" run "$family" "$archive" hash:1/1
done

assert_line_contains() {
  local pattern="$1"
  if ! grep -F -- "$pattern" "$LOG" >/dev/null; then
    echo "missing command fragment: ${pattern}" >&2
    cat "$LOG" >&2
    exit 1
  fi
}

assert_line_contains 'cargo <nextest> <archive> <--workspace> <--lib>'
assert_line_contains 'cargo <nextest> <archive> <-p> <meerkat-integration-tests> <--tests> <--profile> <fast>'
assert_line_contains '<-p> <meerkat-mob> <-p> <meerkat-mob-adaptive>'
assert_line_contains '<--workspace> <--exclude> <meerkat-integration-tests> <--exclude> <meerkat-mob>'

run_count="$(grep -c '^nextest ' "$LOG")"
[[ "$run_count" == 4 ]] || {
  echo "expected four archived runs, got ${run_count}" >&2
  exit 1
}
while IFS= read -r command; do
  [[ "$command" == *' <--no-tests=fail> '* ]] || {
    echo "archived run omitted --no-tests=fail: ${command}" >&2
    exit 1
  }
  [[ "$command" == *' <--workspace-remap> '* ]] || {
    echo "archived run omitted --workspace-remap: ${command}" >&2
    exit 1
  }
  [[ "$command" == *' <--partition> <hash:1/1>'* ]] || {
    echo "archived run omitted the requested partition: ${command}" >&2
    exit 1
  }
done < <(grep '^nextest ' "$LOG")

unit_run="$(grep '^nextest ' "$LOG" | head -n 1)"
[[ "$unit_run" != *' <--profile> <fast>'* ]] || {
  echo "unit archive run unexpectedly selected the fast profile" >&2
  exit 1
}
fast_run_count="$(grep '^nextest ' "$LOG" | grep -c -- '<--profile> <fast>')"
[[ "$fast_run_count" == 3 ]] || {
  echo "expected three fast-profile archive runs, got ${fast_run_count}" >&2
  exit 1
}

if "$ROOT/scripts/ci-nextest-archive.sh" build unknown "${TEST_ROOT}/unknown.tar.zst" >/dev/null 2>&1; then
  echo "unknown archive family was accepted" >&2
  exit 1
fi
if "$ROOT/scripts/ci-nextest-archive.sh" run unit "${TEST_ROOT}/unit.tar.zst" >/dev/null 2>&1; then
  echo "archive run without a partition was accepted" >&2
  exit 1
fi

BUILD_ACTION="$ROOT/.github/actions/build-nextest-archive/action.yml"
RUN_ACTION="$ROOT/.github/actions/run-nextest-archive/action.yml"
WORKFLOW="$ROOT/.github/workflows/cargo.yml"

assert_file_contains() {
  local file="$1"
  local pattern="$2"
  if ! grep -F -- "$pattern" "$file" >/dev/null; then
    echo "${file} is missing required contract: ${pattern}" >&2
    exit 1
  fi
}

stable_artifact_name="nextest-\${{ inputs.family }}-\${{ github.sha }}"
assert_file_contains "$BUILD_ACTION" "name: ${stable_artifact_name}"
assert_file_contains "$BUILD_ACTION" 'overwrite: true'
assert_file_contains "$RUN_ACTION" "name: ${stable_artifact_name}"
assert_file_contains "$RUN_ACTION" 'scripts/ci-nextest-archive.sh run'

for dependency in unit-archive int-archives; do
  assert_file_contains "$WORKFLOW" "      - ${dependency}"
  assert_file_contains "$WORKFLOW" "            \${{ needs.${dependency}.result }}"
done
for execution_job in unit int-heavy int-mob int-else int-rest; do
  assert_file_contains "$WORKFLOW" "      - ${execution_job}"
  assert_file_contains "$WORKFLOW" "            \${{ needs.${execution_job}.result }}"
done

unit_job="$(sed -n '/^  unit:$/,/^  int-archives:$/p' "$WORKFLOW")"
[[ "$unit_job" == *'      - unit-archive'* ]] || {
  echo "unit execution does not depend on unit-archive" >&2
  exit 1
}
archive_job="$(sed -n '/^  int-archives:$/,/^  int-heavy:$/p' "$WORKFLOW")"
for family in int-heavy int-mob int-everything-else; do
  [[ "$archive_job" == *"          family: ${family}"* ]] || {
    echo "integration archive builder omits ${family}" >&2
    exit 1
  }
done

heavy_job="$(sed -n '/^  int-heavy:$/,/^  int-mob:$/p' "$WORKFLOW")"
[[ "$heavy_job" == *'      - int-archives'* ]] || {
  echo "int-heavy execution does not depend on int-archives" >&2
  exit 1
}
mob_job="$(sed -n '/^  int-mob:$/,/^  int-else:$/p' "$WORKFLOW")"
[[ "$mob_job" == *'      - int-archives'* ]] || {
  echo "int-mob execution does not depend on int-archives" >&2
  exit 1
}
else_job="$(sed -n '/^  int-else:$/,/^  int-rest:$/p' "$WORKFLOW")"
[[ "$else_job" == *'      - int-archives'* ]] || {
  echo "int-else execution does not depend on int-archives" >&2
  exit 1
}

echo "CI nextest archive family and fail-closed contracts hold"
