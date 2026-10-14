#!/usr/bin/env bash
# Contract test for scripts/ci-clippy-sccache-wrapper, the RUSTC_WRAPPER of
# the CI Clippy lanes: only a crate Clippy does not lint (not a primary
# package, `--no-deps` in effect) reaches sccache as plain rustc; everything
# else keeps clippy-driver, so no lint is ever skipped.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WRAPPER="$REPO_ROOT/scripts/ci-clippy-sccache-wrapper"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ci-clippy-sccache-wrapper.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

# A stand-in sccache that prints the command it was asked to run.
cat > "$TEST_ROOT/sccache" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*"
EOF
chmod +x "$TEST_ROOT/sccache"
export SCCACHE_PATH="$TEST_ROOT/sccache"

failures=0
check() {
  local label="$1" expected="$2"
  shift 2
  local actual
  actual="$(env "$@")"
  if [[ "$actual" == "$expected" ]]; then
    printf 'PASS %s\n' "$label"
  else
    printf 'FAIL %s: ran "%s", expected "%s"\n' "$label" "$actual" "$expected" >&2
    failures=$((failures + 1))
  fi
}

# cargo-clippy's own arguments, as it hands them to clippy-driver.
no_deps='--no-deps__CLIPPY_HACKERY__-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__'
deps='-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__'

check "a dependency crate under --no-deps compiles as rustc" \
  "rustc --crate-name dep src/lib.rs" \
  -u CARGO_PRIMARY_PACKAGE CLIPPY_ARGS="$no_deps" \
  "$WRAPPER" /toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs
check "a primary package keeps clippy-driver" \
  "/toolchain/bin/clippy-driver rustc --crate-name linted src/lib.rs" \
  CARGO_PRIMARY_PACKAGE=1 CLIPPY_ARGS="$no_deps" \
  "$WRAPPER" /toolchain/bin/clippy-driver rustc --crate-name linted src/lib.rs
check "without --no-deps clippy-driver lints every workspace crate" \
  "/toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs" \
  -u CARGO_PRIMARY_PACKAGE CLIPPY_ARGS="$deps" \
  "$WRAPPER" /toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs
check "without CLIPPY_ARGS nothing is rewritten" \
  "/toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs" \
  -u CARGO_PRIMARY_PACKAGE -u CLIPPY_ARGS \
  "$WRAPPER" /toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs
check "a lint argument that merely contains --no-deps does not count" \
  "/toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs" \
  -u CARGO_PRIMARY_PACKAGE CLIPPY_ARGS='-A__CLIPPY_HACKERY__--no-deps-please__CLIPPY_HACKERY__' \
  "$WRAPPER" /toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs
check "--no-deps after other clippy arguments counts" \
  "rustc --crate-name dep src/lib.rs" \
  -u CARGO_PRIMARY_PACKAGE CLIPPY_ARGS='-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__--no-deps__CLIPPY_HACKERY__' \
  "$WRAPPER" /toolchain/bin/clippy-driver rustc --crate-name dep src/lib.rs
check "a registry crate (no workspace wrapper) is unchanged" \
  "rustc --crate-name serde src/lib.rs" \
  -u CARGO_PRIMARY_PACKAGE CLIPPY_ARGS="$no_deps" \
  "$WRAPPER" rustc --crate-name serde src/lib.rs
check "a lone clippy-driver argument is unchanged" \
  "/toolchain/bin/clippy-driver" \
  -u CARGO_PRIMARY_PACKAGE CLIPPY_ARGS="$no_deps" \
  "$WRAPPER" /toolchain/bin/clippy-driver

if [[ "$failures" -ne 0 ]]; then
  echo "${failures} ci-clippy-sccache-wrapper contract failures" >&2
  exit 1
fi
echo "ci-clippy-sccache-wrapper contracts hold"
