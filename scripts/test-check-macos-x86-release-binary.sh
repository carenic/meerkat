#!/usr/bin/env bash
# Self-test for scripts/check-macos-x86-release-binary with stubbed Apple
# tools (lipo, arch, otool, vtool, sudo), so it runs on any host: the check
# must accept an x86_64 binary that reports the release version under
# Rosetta, links only system libraries and needs at most the supported macOS,
# and refuse each defect.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECK="${REPO_ROOT}/scripts/check-macos-x86-release-binary"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-macos-x86-check.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "${TEST_ROOT}/bin"
STATE="${TEST_ROOT}/state"
mkdir -p "${STATE}"

# Each stub answers from ${STATE}/<tool>.
for tool in lipo otool vtool; do
  cat >"${TEST_ROOT}/bin/${tool}" <<STUB
#!/usr/bin/env bash
cat "${STATE}/${tool}"
STUB
done
cat >"${TEST_ROOT}/bin/arch" <<STUB
#!/usr/bin/env bash
shift
[[ "\$1" == /usr/bin/true ]] && exit 0
cat "${STATE}/version"
exit "\$(cat "${STATE}/version-status")"
STUB
cat >"${TEST_ROOT}/bin/sudo" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
chmod +x "${TEST_ROOT}/bin/"*
: >"${TEST_ROOT}/rkat"

good() {
  echo x86_64 >"${STATE}/lipo"
  echo "rkat 1.2.3" >"${STATE}/version"
  echo 0 >"${STATE}/version-status"
  printf '%s:\n\t/usr/lib/libSystem.B.dylib (compat)\n\t/System/Library/Frameworks/Security.framework/Versions/A/Security (compat)\n' rkat >"${STATE}/otool"
  printf 'Load command 9\n      cmd LC_BUILD_VERSION\n platform MACOS\n    minos 10.12\n      sdk 15.0\n' >"${STATE}/vtool"
}

# check <label> <expected status>
check() {
  local status
  set +e
  MEERKAT_MACOS_TOOL_DIR="${TEST_ROOT}/bin" "${CHECK}" "${TEST_ROOT}/rkat" 1.2.3 >"${TEST_ROOT}/$1.log" 2>&1
  status=$?
  set -e
  if [[ "${status}" -ne "$2" ]]; then
    echo "macOS x86 release binary check contract violated: $1 exited ${status}, expected $2" >&2
    cat "${TEST_ROOT}/$1.log" >&2
    exit 1
  fi
  echo "ok: $1"
}

good
check accepted 0
good; echo "x86_64 arm64" >"${STATE}/lipo"
check universal-binary 1
good; echo arm64 >"${STATE}/lipo"
check arm64-binary 1
good; echo "rkat 1.2.2" >"${STATE}/version"
check wrong-version 1
good; echo 1 >"${STATE}/version-status"
check does-not-run 1
good; printf '%s:\n\t/usr/lib/libSystem.B.dylib (compat)\n\t/opt/homebrew/lib/libopus.0.dylib (compat)\n' rkat >"${STATE}/otool"
check non-system-library 1
good; printf ' platform MACOS\n    minos 15.0\n' >"${STATE}/vtool"
check too-new-minimum 1
good; printf ' platform MACOS\n' >"${STATE}/vtool"
check no-minimum 1
echo "macOS x86 release binary check contract holds"
