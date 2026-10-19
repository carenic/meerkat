#!/usr/bin/env bash
# Self-test for scripts/check-release-binary-sqlite-profile with a stub rkat
# that prints a chosen doctor report, so it runs on any host: the check must
# accept a binary whose report has every SQLite lock off and the allocator
# statistics switch finding them already off, refuse each defect, skip a
# release before 0.9.4 and a target the runner cannot run without running
# the binary, and run the binary against an empty state root and home.
#
# Where pwsh is installed (GitHub's Ubuntu runners have it), the same
# contract is checked for the PowerShell copy of this check in release.yml's
# verify_windows_binaries job, the only guard on the cross-compiled Windows
# binaries: its lock-profile block runs the way a `shell: pwsh` step does,
# against the stub as rkat.exe. A GitHub Actions run without pwsh fails.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECK="${REPO_ROOT}/scripts/check-release-binary-sqlite-profile"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-sqlite-profile-check.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT
STATE="${TEST_ROOT}/state"
mkdir -p "${STATE}" "${TEST_ROOT}/bin"

# The stub answers `storage doctor --json` from ${STATE}/report with the
# status in ${STATE}/status, and records its arguments and home.
cat >"${TEST_ROOT}/bin/rkat" <<STUB
#!/usr/bin/env bash
printf '%s\n' "\$@" >"${STATE}/args"
printf '%s\n' "\${HOME}" >"${STATE}/home"
: >"${STATE}/ran"
cat "${STATE}/report"
exit "\$(cat "${STATE}/status")"
STUB
chmod +x "${TEST_ROOT}/bin/rkat"

report() {
  printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":%s,"shared_page_cache":%s,"page_cache_overflow_statistics":%s,"memory_statistics_switch":"%s"}}\n' \
    "$1" "$2" "$3" "$4" >"${STATE}/report"
  echo 0 >"${STATE}/status"
}

host_target() {
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) echo x86_64-unknown-linux-gnu ;;
    Linux-aarch64 | Linux-arm64) echo aarch64-unknown-linux-gnu ;;
    Darwin-arm64) echo aarch64-apple-darwin ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    *) echo unknown-host ;;
  esac
}

# check <label> <expected status> <expect the binary to run: yes|no> [VERSION] [args...]
check() {
  local label="$1" expected="$2" runs="$3" version="$4" status
  shift 4
  rm -f "${STATE}/ran"
  set +e
  if [[ -n "${version}" ]]; then
    VERSION="${version}" "${CHECK}" "$@" >"${TEST_ROOT}/${label}.log" 2>&1
  else
    env -u VERSION "${CHECK}" "$@" >"${TEST_ROOT}/${label}.log" 2>&1
  fi
  status=$?
  set -e
  if [[ "${status}" -ne "${expected}" ]]; then
    echo "SQLite lock profile release check contract violated: ${label} exited ${status}, expected ${expected}" >&2
    cat "${TEST_ROOT}/${label}.log" >&2
    exit 1
  fi
  if [[ "${runs}" == yes && ! -f "${STATE}/ran" ]]; then
    echo "SQLite lock profile release check contract violated: ${label} did not run the binary" >&2
    exit 1
  fi
  if [[ "${runs}" == no && -f "${STATE}/ran" ]]; then
    echo "SQLite lock profile release check contract violated: ${label} ran the binary" >&2
    exit 1
  fi
  echo "ok: ${label}"
}

RKAT="${TEST_ROOT}/bin/rkat"

report false false false already_off
check required-profile 0 yes 0.9.4 "${RKAT}"
# The binary sweeps one empty state root from an empty home.
grep -qx -- "--state-root" "${STATE}/args"
grep -qx -- "storage" "${STATE}/args"
grep -qx -- "doctor" "${STATE}/args"
grep -qx -- "--json" "${STATE}/args"
case "$(cat "${STATE}/home")" in
  "${TMPDIR:-/tmp}"/meerkat-sqlite-profile.*/home) ;;
  *) echo "SQLite lock profile release check contract violated: HOME was $(cat "${STATE}/home")" >&2; exit 1 ;;
esac
echo "ok: empty state root and home"
check prerelease-is-checked 0 yes v0.9.4-rc.1 "${RKAT}"
check later-release-is-checked 0 yes 0.10.0 "${RKAT}"
check unset-version-is-checked 0 yes "" "${RKAT}"

report false true false already_off
check shared-page-cache 1 yes 0.9.4 "${RKAT}"
report false false true already_off
check overflow-statistics 1 yes 0.9.4 "${RKAT}"
# Allocator statistics turned off at runtime: the build lacks the flag.
report false false false applied
check switch-applied 1 yes 0.9.4 "${RKAT}"
report true false false too_late
check switch-too-late 1 yes 0.9.4 "${RKAT}"
# Each lock is checked on its own: allocator statistics on is refused even
# when the switch found them already off.
report true false false already_off
check memory-statistics 1 yes 0.9.4 "${RKAT}"
# A lock the report leaves out, or reports as null, counts as on.
printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":false,"shared_page_cache":false,"memory_statistics_switch":"already_off"}}\n' >"${STATE}/report"
check lock-missing 1 yes 0.9.4 "${RKAT}"
printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":false,"shared_page_cache":null,"page_cache_overflow_statistics":false,"memory_statistics_switch":"already_off"}}\n' >"${STATE}/report"
check lock-null 1 yes 0.9.4 "${RKAT}"
# A report without the switch outcome is refused: only "already_off" passes.
printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":false,"shared_page_cache":false,"page_cache_overflow_statistics":false}}\n' >"${STATE}/report"
check switch-missing 1 yes 0.9.4 "${RKAT}"

printf '{"findings":[],"inventory":[]}\n' >"${STATE}/report"
check no-profile 1 yes 0.9.4 "${RKAT}"
printf 'not json\n' >"${STATE}/report"
check not-json 1 yes 0.9.4 "${RKAT}"
report false false false already_off
echo 3 >"${STATE}/status"
check doctor-fails 2 yes 0.9.4 "${RKAT}"

# Not run: a release before the profile, or a target this host cannot run.
check older-release 0 no 0.9.3 "${RKAT}"
check older-release-v-prefix 0 no v0.8.40 "${RKAT}"
check foreign-target 0 no 0.9.4 --if-runnable-target riscv64gc-unknown-linux-gnu "${RKAT}"
report false true false already_off
check host-target-is-checked 1 yes 0.9.4 --if-runnable-target "$(host_target)" "${RKAT}"

check no-binary 2 no 0.9.4 "${TEST_ROOT}/bin/missing"
check no-arguments 2 no 0.9.4
check runnable-target-without-binary 2 no 0.9.4 --if-runnable-target "$(host_target)"

# The PowerShell copy in release.yml's verify_windows_binaries job ("Run each
# binary"): the step's first three lines (error preference, VERSION, its
# guard) and its lock-profile block, run as GitHub runs a `shell: pwsh` step
# (`pwsh -command ". '<file>'"`, with $ErrorActionPreference = 'stop' before
# the script and the $LASTEXITCODE exit after it), from a directory whose
# extracted/rkat.exe is the stub.
if command -v pwsh >/dev/null 2>&1; then
  WIN="${TEST_ROOT}/windows"
  mkdir -p "${WIN}/extracted"
  cp "${RKAT}" "${WIN}/extracted/rkat.exe"
  chmod +x "${WIN}/extracted/rkat.exe"
  python3 - "${REPO_ROOT}/.github/workflows/release.yml" "${WIN}/block.ps1" <<'PY'
import pathlib
import re
import sys

workflow = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
job = re.search(r"(?ms)^  verify_windows_binaries:\n(.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)", workflow)
if job is None:
    raise SystemExit("release.yml has no verify_windows_binaries job")
lines = job.group(0).splitlines()
step = lines.index("      - name: Run each binary")
run = lines.index("        run: |", step)
body = []
for line in lines[run + 1:]:
    if line.strip() and not line.startswith(" " * 10):
        break
    body.append(line[10:])
script = "\n".join(body).rstrip() + "\n"
prelude = (
    "$ErrorActionPreference = 'Stop'\n"
    "$version = $env:VERSION\n"
    "if (-not $version) { throw 'release version output missing' }\n"
)
if not script.startswith(prelude):
    raise SystemExit("the Windows step no longer starts by reading and requiring VERSION")
marker = "# The SQLite lock profile the repository's builds declare"
if script.count(marker) != 1:
    raise SystemExit("the Windows step has no single SQLite lock-profile block")
block = script[script.index(marker):]
for needle in ("sqlite_lock_profile", "memory_statistics_switch", "'already_off'", "$LASTEXITCODE"):
    if needle not in block:
        raise SystemExit(f"the Windows lock-profile block no longer contains {needle}")
pathlib.Path(sys.argv[2]).write_text(prelude + block, encoding="utf-8")
PY
  {
    printf '%s\n' "\$ErrorActionPreference = 'stop'"
    cat "${WIN}/block.ps1"
    # shellcheck disable=SC2016 # PowerShell text, written as is.
    printf '%s\n' 'if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }'
  } >"${WIN}/step.ps1"

  # win_check <label> <pass|fail> <text it must print> <expect rkat.exe to run: yes|no> <VERSION>
  win_check() {
    local label="$1" outcome="$2" text="$3" runs="$4" version="$5" status log
    log="${TEST_ROOT}/windows-${label}.log"
    rm -f "${STATE}/ran"
    rm -rf "${WIN}/runner-temp"
    mkdir -p "${WIN}/runner-temp"
    set +e
    (
      cd "${WIN}" &&
        VERSION="${version}" RUNNER_TEMP="${WIN}/runner-temp" NO_COLOR=1 TERM=dumb \
          pwsh -NoLogo -NoProfile -NonInteractive -Command ". '${WIN}/step.ps1'"
    ) </dev/null >"${log}" 2>&1
    status=$?
    set -e
    if [[ "${outcome}" == pass && "${status}" -ne 0 ]] || [[ "${outcome}" == fail && "${status}" -eq 0 ]]; then
      echo "SQLite lock profile Windows check contract violated: ${label} exited ${status}, expected ${outcome}" >&2
      cat "${log}" >&2
      exit 1
    fi
    if ! grep -qF -- "${text}" "${log}"; then
      echo "SQLite lock profile Windows check contract violated: ${label} did not print: ${text}" >&2
      cat "${log}" >&2
      exit 1
    fi
    if [[ "${runs}" == yes && ! -f "${STATE}/ran" ]]; then
      echo "SQLite lock profile Windows check contract violated: ${label} did not run rkat.exe" >&2
      exit 1
    fi
    if [[ "${runs}" == no && -f "${STATE}/ran" ]]; then
      echo "SQLite lock profile Windows check contract violated: ${label} ran rkat.exe" >&2
      exit 1
    fi
    echo "ok: windows ${label}"
  }

  WIN_OK="rkat.exe: SQLite runs without process-wide locks (allocator statistics already off)"
  WIN_LOCK="runs SQLite with a process-wide lock"

  report false false false already_off
  win_check required-profile pass "${WIN_OK}" yes 0.9.4
  grep -qx -- "--state-root" "${STATE}/args"
  grep -qx -- "doctor" "${STATE}/args"
  grep -qx -- "--json" "${STATE}/args"
  win_check prerelease-is-checked pass "${WIN_OK}" yes v0.9.4-rc.1

  report true false false already_off
  win_check memory-statistics fail "${WIN_LOCK}" yes 0.9.4
  report false true false already_off
  win_check shared-page-cache fail "${WIN_LOCK}" yes 0.9.4
  report false false true already_off
  win_check overflow-statistics fail "${WIN_LOCK}" yes 0.9.4
  report false false false applied
  win_check switch-applied fail "${WIN_LOCK}" yes 0.9.4
  printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":false,"shared_page_cache":false,"page_cache_overflow_statistics":false}}\n' >"${STATE}/report"
  win_check switch-missing fail "${WIN_LOCK}" yes 0.9.4
  printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":false,"shared_page_cache":false,"memory_statistics_switch":"already_off"}}\n' >"${STATE}/report"
  win_check lock-missing fail "${WIN_LOCK}" yes 0.9.4
  printf '{"findings":[],"inventory":[],"sqlite_lock_profile":{"memory_statistics":false,"shared_page_cache":null,"page_cache_overflow_statistics":false,"memory_statistics_switch":"already_off"}}\n' >"${STATE}/report"
  win_check lock-null fail "${WIN_LOCK}" yes 0.9.4
  printf '{"findings":[],"inventory":[]}\n' >"${STATE}/report"
  win_check no-profile fail "${WIN_LOCK}" yes 0.9.4

  report false false false already_off
  echo 3 >"${STATE}/status"
  win_check doctor-fails fail "rkat storage doctor --json exited 3" yes 0.9.4

  report false true false already_off
  win_check older-release pass "rkat.exe: release 0.9.3 predates the SQLite lock profile report; not checked" no 0.9.3
  win_check version-missing fail "release version output missing" no ""
elif [[ -n "${GITHUB_ACTIONS-}" ]]; then
  echo "SQLite lock profile release check contract violated: pwsh is not installed on this runner, so the Windows check was not tested" >&2
  exit 1
else
  echo "skipped: the Windows check cases (pwsh is not installed here)"
fi

echo "SQLite lock profile release check contract: all cases pass"
