#!/usr/bin/env bash
# Contract test for the release crate enumeration gate.
#
# Adding one workspace member during 0.8.22 broke three hand-maintained lists
# at three different gates: the publish order, the package-verify patch map,
# and the documented order/count. Each failed later and further from the cause
# than the last. This test builds a scratch workspace, adds a member, and
# asserts one gate names the absence in every enumeration that must list it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PYTHON="${PYTHON:-$(command -v python3.11 2>/dev/null || command -v python3)}"
CHECKER="${REPO_ROOT}/scripts/check_rust_release_packaging.py"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-crate-enumeration.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

FIXTURE="${TEST_ROOT}/workspace"
FIXTURE_VERSION="0.0.0"

# The four release binaries the packaging check inspects by path, plus one
# ordinary library crate that stands in for a newly added member.
BAZEL_BINARY_DIRS=(meerkat-cli meerkat-rpc meerkat-rest meerkat-mcp-server)
BAZEL_BINARY_TARGETS=(rkat rkat_rpc_bin rkat_rest_bin rkat_mcp_bin)

write_manifest() {
  local crate_dir="$1"
  local crate_name="$2"
  mkdir -p "${FIXTURE}/${crate_dir}/src"
  : > "${FIXTURE}/${crate_dir}/src/lib.rs"
  cat > "${FIXTURE}/${crate_dir}/Cargo.toml" <<EOF
[package]
name = "${crate_name}"
version.workspace = true
description = "fixture crate"
license = "MIT"
repository = "https://example.invalid/repo"
homepage = "https://example.invalid"
documentation = "https://example.invalid/docs"
EOF
}

write_release_list() {
  local crates=("$@")
  mkdir -p "${FIXTURE}/scripts"
  {
    cat <<'LISTHEADER'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' \
LISTHEADER
    local index
    for index in "${!crates[@]}"; do
      if [[ "$index" -eq $(( ${#crates[@]} - 1 )) ]]; then
        printf '  %s\n' "${crates[$index]}"
      else
        printf '  %s \\\n' "${crates[$index]}"
      fi
    done
  } > "${FIXTURE}/scripts/release-rust-crates.sh"
  chmod +x "${FIXTURE}/scripts/release-rust-crates.sh"
}

write_claude_md() {
  local documented_count="$1"
  shift
  local crates=("$@")
  local listing=""
  local crate
  for crate in "${crates[@]}"; do
    if [[ -z "$listing" ]]; then
      listing="\`${crate}\`"
    else
      listing="${listing} → \`${crate}\`"
    fi
  done
  cat > "${FIXTURE}/CLAUDE.md" <<EOF
# Fixture

The canonical publish order lives in \`scripts/release-rust-crates.sh\` (${documented_count} crates, dependency order):
${listing}

| \`publish_registries\` | Tags | Publishes ${documented_count} Rust crates → crates.io |

make publish-dry-run              # Parallel dry-run for all ${documented_count} publishable Rust crates
EOF
}

build_fixture() {
  local release_crates=("$@")
  rm -rf "${FIXTURE}"
  mkdir -p "${FIXTURE}/scripts"

  local members=()
  local index
  for index in "${!BAZEL_BINARY_DIRS[@]}"; do
    local crate_dir="${BAZEL_BINARY_DIRS[$index]}"
    local crate_name="${crate_dir}"
    [[ "$crate_dir" == "meerkat-cli" ]] && crate_name="rkat"
    write_manifest "$crate_dir" "$crate_name"
    members+=("$crate_dir")
    cat > "${FIXTURE}/${crate_dir}/BUILD.bazel" <<EOF
rust_binary(
    name = "${BAZEL_BINARY_TARGETS[$index]}",
    rustc_env = {"CARGO_PKG_VERSION": "${FIXTURE_VERSION}"},
)
EOF
  done

  {
    echo '[workspace]'
    echo 'members = ['
    local member
    for member in "${members[@]}"; do
      printf '  "%s",\n' "$member"
    done
    echo '  "meerkat-scratch",'
    echo ']'
    echo ''
    echo '[workspace.package]'
    printf 'version = "%s"\n' "${FIXTURE_VERSION}"
  } > "${FIXTURE}/Cargo.toml"

  write_manifest meerkat-scratch meerkat-scratch
  cp "${REPO_ROOT}/scripts/generate-patch-config.sh" "${FIXTURE}/scripts/generate-patch-config.sh"
  chmod +x "${FIXTURE}/scripts/generate-patch-config.sh"
  write_release_list "${release_crates[@]}"
  write_claude_md "${#release_crates[@]}" "${release_crates[@]}"
}

run_gate() {
  local output_path="$1"
  shift
  set +e
  "$PYTHON" "$CHECKER" "$FIXTURE" "$@" >"$output_path" 2>&1
  local status=$?
  set -e
  printf '%s' "$status"
}

release_list() {
  "${FIXTURE}/scripts/release-rust-crates.sh"
}

fail() {
  echo "crate enumeration gate contract violated: $1" >&2
  shift
  for extra in "$@"; do
    echo "  ${extra}" >&2
  done
  exit 1
}

expect_named() {
  local label="$1"
  local log_path="$2"
  local needle="$3"
  if ! grep -Fq "$needle" "$log_path"; then
    fail "${label} failure does not name the defect (expected: ${needle})" \
      "$(cat "$log_path")"
  fi
}

ALL_CRATES=(rkat meerkat-rpc meerkat-rest meerkat-mcp-server meerkat-scratch)

# 1. Every enumeration agrees: the gate passes.
build_fixture "${ALL_CRATES[@]}"
mapfile -t fixture_crates < <(release_list)
status="$(run_gate "${TEST_ROOT}/consistent.log" "${fixture_crates[@]}")"
if [[ "$status" -ne 0 ]]; then
  fail "a consistent scratch workspace was rejected" "$(cat "${TEST_ROOT}/consistent.log")"
fi

# 2. The new member is absent from the documented order and count.
build_fixture "${ALL_CRATES[@]}"
write_claude_md 4 rkat meerkat-rpc meerkat-rest meerkat-mcp-server
status="$(run_gate "${TEST_ROOT}/docs.log" "${fixture_crates[@]}")"
if [[ "$status" -eq 0 ]]; then
  fail "a documented publish order missing a release crate was accepted"
fi
expect_named "documented order" "${TEST_ROOT}/docs.log" \
  "publish order omits \`meerkat-scratch\`"
expect_named "documented count" "${TEST_ROOT}/docs.log" \
  "claims 4 release crates, scripts/release-rust-crates.sh lists 5"

# 3. The new member is absent from the publish order itself.
build_fixture rkat meerkat-rpc meerkat-rest meerkat-mcp-server
mapfile -t short_crates < <(release_list)
status="$(run_gate "${TEST_ROOT}/release-list.log" "${short_crates[@]}")"
if [[ "$status" -eq 0 ]]; then
  fail "a publishable member missing from the release list was accepted"
fi
expect_named "release list" "${TEST_ROOT}/release-list.log" "  - meerkat-scratch"

# 4. The new member is absent from a hand-maintained patch map. Derivation
#    prevents this today; the gate is what keeps it prevented.
build_fixture "${ALL_CRATES[@]}"
cat > "${FIXTURE}/scripts/generate-patch-config.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
echo "[patch.crates-io]"
echo "rkat = { path = \"${FIXTURE}/meerkat-cli\" }"
echo "meerkat-rpc = { path = \"${FIXTURE}/meerkat-rpc\" }"
echo "meerkat-rest = { path = \"${FIXTURE}/meerkat-rest\" }"
echo "meerkat-mcp-server = { path = \"${FIXTURE}/meerkat-mcp-server\" }"
EOF
chmod +x "${FIXTURE}/scripts/generate-patch-config.sh"
status="$(run_gate "${TEST_ROOT}/patch-map.log" "${fixture_crates[@]}")"
if [[ "$status" -eq 0 ]]; then
  fail "a patch map missing a release crate was accepted"
fi
expect_named "patch map" "${TEST_ROOT}/patch-map.log" \
  "meerkat-scratch: release crate is absent from [patch.crates-io]"

echo "crate enumeration gate contract holds"
