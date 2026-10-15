#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-release-projection.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

git -C "$TEST_ROOT" init -q
git -C "$TEST_ROOT" config user.name Meerkat
git -C "$TEST_ROOT" config user.email meerkat@example.invalid

mkdir -p \
  "$TEST_ROOT/crate" \
  "$TEST_ROOT/docs/api" \
  "$TEST_ROOT/crates/meerkat-contracts/src" \
  "$TEST_ROOT/sdks/web/src"

cat > "$TEST_ROOT/Cargo.toml" <<'EOF'
[workspace]
members = ["crate"]

[workspace.package]
version = "1.2.3"

[workspace.dependencies]
fixture = { version = "1.2.3", path = "crate" }
EOF
cat > "$TEST_ROOT/Cargo.lock" <<'EOF'
[[package]]
name = "fixture"
version = "1.2.3"
EOF
printf 'root digest before\n' > "$TEST_ROOT/MODULE.bazel.lock"
printf '## [Unreleased]\n\n' > "$TEST_ROOT/CHANGELOG.md"
printf 'fixture = "1.2.3"\n' > "$TEST_ROOT/README.md"
printf 'version = "1.2.3"\n' > "$TEST_ROOT/crate/BUILD.bazel"
printf '{"contract_version":{"major":1,"minor":2,"patch":3}}\n' \
  > "$TEST_ROOT/docs/api/contract.mdx"
cat > "$TEST_ROOT/crates/meerkat-contracts/src/version.rs" <<'EOF'
impl ContractVersion {
    pub const CURRENT: Self = Self {
        major: 1,
        minor: 2,
        patch: 3,
    };
    pub const PRERELEASE: Option<&'static str> = None;
}
EOF
printf 'export const VERSION = "1.2.3";\n' > "$TEST_ROOT/sdks/web/src/runtime.ts"
# Stamped prose also names past and upcoming releases; the stamper rewrites
# only its marker lines, so those mentions stay verbatim across the bump.
mkdir -p "$TEST_ROOT/docs/guides" "$TEST_ROOT/examples/demo"
cat > "$TEST_ROOT/docs/guides/install.md" <<'EOF'
meerkat = "=1.2.3"

Since 1.2.3 the flag takes a host name; 1.2.2 accepted only IP literals.
Planned for 1.2.4: the new flag. Tool 11.2.3 and build 1.2.3.1 are unrelated.
EOF
# A standalone example workspace lock (scripts/example-locks.py refreshes
# it in the release hook): only its path packages carry the release version.
cat > "$TEST_ROOT/examples/demo/Cargo.lock" <<'EOF'
[[package]]
name = "demo"
version = "0.1.0"

[[package]]
name = "fixture"
version = "1.2.3"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
EOF
# Over 1 MiB, like the real CHANGELOG.md: the classifier must read it whole.
changelog_filler() {
  awk 'BEGIN { for (i = 0; i < 24000; i++) printf "- Historical fixture entry %06d for 1.2.3.\n", i }'
}
{ printf '## [Unreleased]\n\n'; changelog_filler; } > "$TEST_ROOT/CHANGELOG.md"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm base
base="$(git -C "$TEST_ROOT" rev-parse HEAD)"

# The release hook's projection of 1.2.3 -> 1.2.4 on the checked-out tree.
apply_projection() {
  sed -i.bak 's/1\.2\.3/1.2.4/g' \
    "$TEST_ROOT/Cargo.toml" \
    "$TEST_ROOT/Cargo.lock" \
    "$TEST_ROOT/README.md" \
    "$TEST_ROOT/crate/BUILD.bazel" \
    "$TEST_ROOT/sdks/web/src/runtime.ts"
  rm -f "$TEST_ROOT"/*.bak "$TEST_ROOT/crate"/*.bak "$TEST_ROOT/sdks/web/src"/*.bak
  sed -i.bak \
    's/"major":1,"minor":2,"patch":3/"major":1,"minor":2,"patch":4/' \
    "$TEST_ROOT/docs/api/contract.mdx"
  rm -f "$TEST_ROOT/docs/api/contract.mdx.bak"
  sed -i.bak 's/patch: 3/patch: 4/' "$TEST_ROOT/crates/meerkat-contracts/src/version.rs"
  rm -f "$TEST_ROOT/crates/meerkat-contracts/src/version.rs.bak"
  sed -i.bak 's/^meerkat = "=1\.2\.3"$/meerkat = "=1.2.4"/' "$TEST_ROOT/docs/guides/install.md"
  rm -f "$TEST_ROOT/docs/guides/install.md.bak"
  sed -i.bak '/^name = "fixture"$/{n;s/1\.2\.3/1.2.4/;}' "$TEST_ROOT/examples/demo/Cargo.lock"
  rm -f "$TEST_ROOT/examples/demo/Cargo.lock.bak"
  printf 'root digest after\n' > "$TEST_ROOT/MODULE.bazel.lock"
  { printf '## [1.2.4] - 2026-08-28\n\n- Pipeline-only fixture.\n\n'; changelog_filler; } \
    > "$TEST_ROOT/CHANGELOG.md"
}
apply_projection
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm projection
projection="$(git -C "$TEST_ROOT" rev-parse HEAD)"
if git -C "$TEST_ROOT" diff --quiet "$base" "$projection" -- docs/guides/install.md examples/demo/Cargo.lock; then
  echo "the projection fixture must change the stamped prose and the example lock" >&2
  exit 1
fi
if [[ "$(git -C "$TEST_ROOT" cat-file -s "$projection:CHANGELOG.md")" -le $((1024 * 1024)) ]]; then
  echo "the projection fixture's CHANGELOG.md must exceed 1 MiB" >&2
  exit 1
fi

(
  cd "$TEST_ROOT"
  "$REPO_ROOT/scripts/release-projection-only.mjs" \
    --base "$base" --head "$projection" --verbose
)

assert_rejected() {
  local label="$1"
  local candidate="$2"
  set +e
  (
    cd "$TEST_ROOT"
    "$REPO_ROOT/scripts/release-projection-only.mjs" \
      --base "$base" --head "$candidate" >/dev/null 2>&1
  )
  local status=$?
  set -e
  if [[ "$status" -ne 1 ]]; then
    echo "${label} returned ${status}; expected classifier rejection status 1" >&2
    exit 1
  fi
}

git -C "$TEST_ROOT" checkout -q -b semantic-source "$base"
printf 'pub fn changed_behavior() {}\n' > "$TEST_ROOT/crate/src.rs"
sed -i.bak 's/1\.2\.3/1.2.4/g' "$TEST_ROOT/Cargo.toml"
rm -f "$TEST_ROOT/Cargo.toml.bak"
printf '## [1.2.4]\n' > "$TEST_ROOT/CHANGELOG.md"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm semantic-source
assert_rejected semantic-source "$(git -C "$TEST_ROOT" rev-parse HEAD)"

git -C "$TEST_ROOT" checkout -q -B semantic-manifest "$base"
sed -i.bak 's/1\.2\.3/1.2.4/g' "$TEST_ROOT/Cargo.toml"
rm -f "$TEST_ROOT/Cargo.toml.bak"
printf '\nserde = "1"\n' >> "$TEST_ROOT/Cargo.toml"
printf '## [1.2.4]\n' > "$TEST_ROOT/CHANGELOG.md"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm semantic-manifest
assert_rejected semantic-manifest "$(git -C "$TEST_ROOT" rev-parse HEAD)"

git -C "$TEST_ROOT" checkout -q -B equal-version-external "$base"
printf '\nexternal = "1.2.3"\n' >> "$TEST_ROOT/Cargo.toml"
git -C "$TEST_ROOT" add Cargo.toml
git -C "$TEST_ROOT" commit -qm external-base
external_base="$(git -C "$TEST_ROOT" rev-parse HEAD)"
sed -i.bak 's/1\.2\.3/1.2.4/g' "$TEST_ROOT/Cargo.toml"
rm -f "$TEST_ROOT/Cargo.toml.bak"
printf '## [1.2.4]\n' > "$TEST_ROOT/CHANGELOG.md"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm equal-version-external
set +e
(
  cd "$TEST_ROOT"
  "$REPO_ROOT/scripts/release-projection-only.mjs" \
    --base "$external_base" --head "$(git rev-parse HEAD)" >/dev/null 2>&1
)
external_status=$?
set -e
if [[ "$external_status" -ne 1 ]]; then
  echo "same-version external dependency drift escaped the classifier" >&2
  exit 1
fi

git -C "$TEST_ROOT" checkout -q -B stale-generated "$projection"
printf '\n# semantic rule drift\n' >> "$TEST_ROOT/crate/BUILD.bazel"
git -C "$TEST_ROOT" add crate/BUILD.bazel
git -C "$TEST_ROOT" commit -qm stale-generated
assert_rejected stale-generated "$(git -C "$TEST_ROOT" rev-parse HEAD)"

git -C "$TEST_ROOT" checkout -q -B dependency-lock-drift "$base"
sed -i.bak 's/1\.2\.3/1.2.4/g' "$TEST_ROOT/Cargo.toml"
rm -f "$TEST_ROOT/Cargo.toml.bak"
printf '## [1.2.4]\n' > "$TEST_ROOT/CHANGELOG.md"
cat >> "$TEST_ROOT/Cargo.lock" <<'EOF'

[[package]]
name = "unrelated"
version = "9.9.9"
EOF
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm dependency-lock-drift
assert_rejected dependency-lock-drift "$(git -C "$TEST_ROOT" rev-parse HEAD)"

# A projection plus one more byte change in a projected file is not a
# projection: prose edits, a version other than the release's, a version
# number that merely contains the old one, and an example lock that also
# moves a registry dependency.
assert_projection_plus_rejected() {
  local label="$1" path="$2" expression="$3"
  git -C "$TEST_ROOT" checkout -q -B "$label" "$base"
  apply_projection
  sed -i.bak "$expression" "$TEST_ROOT/$path"
  rm -f "$TEST_ROOT/$path.bak"
  if git -C "$TEST_ROOT" diff --quiet "$projection" -- "$path"; then
    echo "${label}: the deviation did not change ${path}" >&2
    exit 1
  fi
  git -C "$TEST_ROOT" add .
  git -C "$TEST_ROOT" commit -qm "$label"
  assert_rejected "$label" "$(git -C "$TEST_ROOT" rev-parse HEAD)"
}
assert_projection_plus_rejected prose-edit docs/guides/install.md 's/IP literals/IPv4 literals/'
assert_projection_plus_rejected prose-other-version docs/guides/install.md 's/1\.2\.2 accepted/1.2.4 accepted/'
assert_projection_plus_rejected prose-embedded-version docs/guides/install.md 's/Tool 11\.2\.3/Tool 11.2.4/'
assert_projection_plus_rejected prose-build-suffix docs/guides/install.md 's/build 1\.2\.3\.1/build 1.2.4.1/'
assert_projection_plus_rejected example-lock-registry-drift examples/demo/Cargo.lock 's/^version = "1\.0\.0"$/version = "1.0.1"/'
assert_projection_plus_rejected readme-edit README.md 's/^fixture/fixtures/'

# Only the documents the docs stamper owns may keep the old version; every
# other projection surface must carry the release version everywhere.
git -C "$TEST_ROOT" checkout -q -B partial-generated "$base"
printf 'export const BASELINE = "1.2.3";\n' >> "$TEST_ROOT/sdks/web/src/runtime.ts"
git -C "$TEST_ROOT" add sdks/web/src/runtime.ts
git -C "$TEST_ROOT" commit -qm partial-generated-base
partial_base="$(git -C "$TEST_ROOT" rev-parse HEAD)"
apply_projection
sed -i.bak 's/^export const BASELINE = "1\.2\.4";$/export const BASELINE = "1.2.3";/' \
  "$TEST_ROOT/sdks/web/src/runtime.ts"
rm -f "$TEST_ROOT/sdks/web/src/runtime.ts.bak"
if ! grep -q '^export const VERSION = "1\.2\.4";$' "$TEST_ROOT/sdks/web/src/runtime.ts" ||
   ! grep -q '^export const BASELINE = "1\.2\.3";$' "$TEST_ROOT/sdks/web/src/runtime.ts"; then
  echo "partial-generated: runtime.ts must carry one old and one new version" >&2
  exit 1
fi
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm partial-generated
set +e
(
  cd "$TEST_ROOT"
  "$REPO_ROOT/scripts/release-projection-only.mjs" \
    --base "$partial_base" --head "$(git rev-parse HEAD)" >/dev/null 2>&1
)
partial_status=$?
set -e
if [[ "$partial_status" -ne 1 ]]; then
  echo "a generated surface that keeps the old version escaped the classifier (status ${partial_status})" >&2
  exit 1
fi

set +e
(
  cd "$TEST_ROOT"
  "$REPO_ROOT/scripts/release-projection-only.mjs" \
    --base missing-revision --head "$projection" >/dev/null 2>&1
)
invalid_ref_status=$?
set -e
if [[ "$invalid_ref_status" -ne 1 && "$invalid_ref_status" -ne 2 ]]; then
  echo "invalid revision returned ${invalid_ref_status}; expected fail-closed status" >&2
  exit 1
fi

echo "release projection classifier contracts hold"
