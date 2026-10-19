#!/usr/bin/env bash
# Self-test for scripts/check-release-binary-sqlite-profile with a stub rkat
# that prints a chosen doctor report, so it runs on any host: the check must
# accept a binary whose report has every SQLite lock off and the allocator
# statistics switch finding them already off, refuse each defect, skip a
# release before 0.9.4 and a target the runner cannot run without running
# the binary, and run the binary against an empty state root and home.

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

echo "SQLite lock profile release check contract: all cases pass"
