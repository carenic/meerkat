#!/usr/bin/env bash
# Contract test for scripts/release-projection-only.mjs, the fail-closed
# classifier the pre-push hook and CI use to decide that a commit is only the
# release hook's version projection. The fixture repository carries the real
# docs and changelog stampers, and its projection is produced by them, so the
# accepted case is exactly what scripts/release-hook.sh writes.
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
  "$TEST_ROOT/docs/guides" \
  "$TEST_ROOT/crates/meerkat-contracts/src" \
  "$TEST_ROOT/sdks/web/src/generated" \
  "$TEST_ROOT/examples/demo" \
  "$TEST_ROOT/scripts" \
  "$TEST_ROOT/.claude/skills/meerkat-platform"
cp "$REPO_ROOT/scripts/stamp-docs-contract-version.sh" "$TEST_ROOT/scripts/"
cp "$REPO_ROOT/scripts/stamp-changelog-release.py" "$TEST_ROOT/scripts/"

cat > "$TEST_ROOT/Cargo.toml" <<'EOF'
[workspace]
members = ["crate"]

[workspace.package]
version = "1.2.3"

[workspace.dependencies]
fixture = { version = "=1.2.3", path = "crate" }
EOF
cat > "$TEST_ROOT/Cargo.lock" <<'EOF'
[[package]]
name = "fixture"
version = "1.2.3"
EOF
printf 'root digest before\n' > "$TEST_ROOT/MODULE.bazel.lock"
# Stamped prose also names past and upcoming releases; the stamper rewrites
# only its marker lines, so those mentions stay verbatim across the bump.
cat > "$TEST_ROOT/README.md" <<'EOF'
meerkat = "1.2.3"

Fixture prose: 1.2.3 is the current release.
EOF
cat > "$TEST_ROOT/.claude/skills/meerkat-platform/SKILL.md" <<'EOF'
cargo install rkat --version "=1.2.3"

Since 1.2.3 the skill names the fixture flag.
EOF
cat > "$TEST_ROOT/docs/guides/install.md" <<'EOF'
meerkat = "=1.2.3"

Since 1.2.3 the flag takes a host name; 1.2.2 accepted only IP literals.
Planned for 1.2.4: the new flag. Tool 11.2.3 and build 1.2.3.1 are unrelated.
EOF
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
# Over 1 MiB, like the real CHANGELOG.md, with the shape the changelog
# stamper requires: pending notes, the previous release and its links.
changelog_filler() {
  awk 'BEGIN { for (i = 0; i < 24000; i++) printf "- Historical fixture entry %06d for 1.2.3.\n", i }'
}
{
  printf '# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- Pipeline-only fixture note.\n\n## [1.2.3] - 2026-08-01\n\n'
  changelog_filler
  printf '\n[Unreleased]: https://github.com/example/meerkat/compare/v1.2.3...HEAD\n'
  printf '[1.2.3]: https://github.com/example/meerkat/compare/v1.2.2...v1.2.3\n'
} > "$TEST_ROOT/CHANGELOG.md"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm base
base="$(git -C "$TEST_ROOT" rev-parse HEAD)"

# The release hook's projection of 1.2.3 -> 1.2.4 on the checked-out tree:
# the version-bearing manifests and generated surfaces, then the real docs
# and changelog stampers.
apply_projection() {
  sed -i.bak 's/1\.2\.3/1.2.4/g' \
    "$TEST_ROOT/Cargo.toml" \
    "$TEST_ROOT/Cargo.lock" \
    "$TEST_ROOT/crate/BUILD.bazel" \
    "$TEST_ROOT/sdks/web/src/runtime.ts"
  rm -f "$TEST_ROOT"/*.bak "$TEST_ROOT/crate"/*.bak "$TEST_ROOT/sdks/web/src"/*.bak
  sed -i.bak 's/patch: 3/patch: 4/' "$TEST_ROOT/crates/meerkat-contracts/src/version.rs"
  rm -f "$TEST_ROOT/crates/meerkat-contracts/src/version.rs.bak"
  sed -i.bak '/^name = "fixture"$/{n;s/1\.2\.3/1.2.4/;}' "$TEST_ROOT/examples/demo/Cargo.lock"
  rm -f "$TEST_ROOT/examples/demo/Cargo.lock.bak"
  printf 'root digest after\n' > "$TEST_ROOT/MODULE.bazel.lock"
  bash "$TEST_ROOT/scripts/stamp-docs-contract-version.sh" 1.2.4 >/dev/null
  MEERKAT_RELEASE_DATE=2026-08-28 python3 "$TEST_ROOT/scripts/stamp-changelog-release.py" \
    "$TEST_ROOT/CHANGELOG.md" 1.2.4 >/dev/null
}
apply_projection
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm projection
projection="$(git -C "$TEST_ROOT" rev-parse HEAD)"
for path in docs/guides/install.md docs/api/contract.mdx README.md .claude/skills/meerkat-platform/SKILL.md examples/demo/Cargo.lock; do
  if git -C "$TEST_ROOT" diff --quiet "$base" "$projection" -- "$path"; then
    echo "the projection fixture must change ${path}" >&2
    exit 1
  fi
done
if ! git -C "$TEST_ROOT" show "$projection:docs/guides/install.md" | grep -q '^Since 1\.2\.3 the flag'; then
  echo "the docs stamper must leave historical mentions alone" >&2
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

# Rejected with status 1 and, when given, for the named reason.
assert_rejected() {
  local label="$1"
  local candidate="$2"
  local reason="${3:-}"
  local from="${4:-$base}"
  local output status
  set +e
  output="$(
    cd "$TEST_ROOT" &&
      "$REPO_ROOT/scripts/release-projection-only.mjs" \
        --base "$from" --head "$candidate" --verbose 2>&1
  )"
  status=$?
  set -e
  if [[ "$status" -ne 1 ]]; then
    echo "${label} returned ${status}; expected classifier rejection status 1: ${output}" >&2
    exit 1
  fi
  if [[ -n "$reason" && "$output" != *"$reason"* ]]; then
    echo "${label} was rejected for the wrong reason: ${output} (expected: ${reason})" >&2
    exit 1
  fi
}

git -C "$TEST_ROOT" checkout -q -b semantic-source "$base"
apply_projection
printf 'pub fn changed_behavior() {}\n' > "$TEST_ROOT/crate/src.rs"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm semantic-source
assert_rejected semantic-source "$(git -C "$TEST_ROOT" rev-parse HEAD)" "crate/src.rs has non-modification status A"

git -C "$TEST_ROOT" checkout -q -B semantic-manifest "$base"
apply_projection
printf '\nserde = "1"\n' >> "$TEST_ROOT/Cargo.toml"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm semantic-manifest
assert_rejected semantic-manifest "$(git -C "$TEST_ROOT" rev-parse HEAD)" "Cargo.toml contains changes beyond the version projection"

# Internal requirements publish exact (`=X.Y.Z`); a bump that also drops the
# `=` changes what every published crate requires, so it is no projection.
git -C "$TEST_ROOT" checkout -q -B dropped-exact-requirement "$projection"
sed -i.bak 's/version = "=1\.2\.4", path/version = "1.2.4", path/' "$TEST_ROOT/Cargo.toml"
rm -f "$TEST_ROOT/Cargo.toml.bak"
grep -Fq 'fixture = { version = "1.2.4", path = "crate" }' "$TEST_ROOT/Cargo.toml" || {
  echo "dropped-exact-requirement fixture did not drop the exact operator" >&2
  exit 1
}
git -C "$TEST_ROOT" add Cargo.toml
git -C "$TEST_ROOT" commit -qm dropped-exact-requirement
assert_rejected dropped-exact-requirement "$(git -C "$TEST_ROOT" rev-parse HEAD)"

git -C "$TEST_ROOT" checkout -q -B equal-version-external "$base"
printf '\nexternal = "1.2.3"\n' >> "$TEST_ROOT/Cargo.toml"
git -C "$TEST_ROOT" add Cargo.toml
git -C "$TEST_ROOT" commit -qm external-base
external_base="$(git -C "$TEST_ROOT" rev-parse HEAD)"
apply_projection
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm equal-version-external
assert_rejected equal-version-external "$(git -C "$TEST_ROOT" rev-parse HEAD)" \
  "Cargo.toml contains changes beyond the version projection" "$external_base"

git -C "$TEST_ROOT" checkout -q -B stale-generated "$projection"
printf '\n# semantic rule drift\n' >> "$TEST_ROOT/crate/BUILD.bazel"
git -C "$TEST_ROOT" add crate/BUILD.bazel
git -C "$TEST_ROOT" commit -qm stale-generated
assert_rejected stale-generated "$(git -C "$TEST_ROOT" rev-parse HEAD)" "crate/BUILD.bazel contains changes beyond the version projection"

git -C "$TEST_ROOT" checkout -q -B dependency-lock-drift "$base"
apply_projection
cat >> "$TEST_ROOT/Cargo.lock" <<'EOF'

[[package]]
name = "unrelated"
version = "9.9.9"
EOF
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm dependency-lock-drift
assert_rejected dependency-lock-drift "$(git -C "$TEST_ROOT" rev-parse HEAD)" "Cargo.lock contains changes beyond the version projection"

# A projection plus one more byte change in a projected file is not a
# projection: in a stamped document, any byte the stamper would not write
# (prose, another version, a longer number, a build suffix, a historical
# mention moved to the new version); in an example lock, a registry move; in
# the CHANGELOG, a hand edit next to the stamp.
assert_projection_plus_rejected() {
  local label="$1" path="$2" expression="$3" reason="$4"
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
  assert_rejected "$label" "$(git -C "$TEST_ROOT" rev-parse HEAD)" "$reason"
}
stamper_reason="is not the docs stamper's projection of the base file"
assert_projection_plus_rejected prose-edit docs/guides/install.md 's/IP literals/IPv4 literals/' "$stamper_reason"
assert_projection_plus_rejected prose-other-version docs/guides/install.md 's/1\.2\.2 accepted/1.2.4 accepted/' "$stamper_reason"
assert_projection_plus_rejected prose-embedded-version docs/guides/install.md 's/Tool 11\.2\.3/Tool 11.2.4/' "$stamper_reason"
assert_projection_plus_rejected prose-build-suffix docs/guides/install.md 's/build 1\.2\.3\.1/build 1.2.4.1/' "$stamper_reason"
assert_projection_plus_rejected prose-historical-mention docs/guides/install.md 's/^Since 1\.2\.3 /Since 1.2.4 /' "$stamper_reason"
assert_projection_plus_rejected skill-historical-mention .claude/skills/meerkat-platform/SKILL.md 's/^Since 1\.2\.3 /Since 1.2.4 /' "$stamper_reason"
assert_projection_plus_rejected readme-prose README.md 's/^Fixture prose: 1\.2\.3/Fixture prose: 1.2.4/' "$stamper_reason"
assert_projection_plus_rejected example-lock-registry-drift examples/demo/Cargo.lock 's/^version = "1\.0\.0"$/version = "1.0.1"/' \
  "examples/demo/Cargo.lock contains changes beyond the version projection"
assert_projection_plus_rejected changelog-hand-edit CHANGELOG.md 's/^- Pipeline-only fixture note\.$/- Pipeline-only fixture note, edited./' \
  "CHANGELOG.md is not the changelog stamper's projection of the base file"

# Only the stamped documents may keep the old version; every other
# projection surface must carry the release version everywhere.
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
assert_rejected partial-generated "$(git -C "$TEST_ROOT" rev-parse HEAD)" \
  "sdks/web/src/runtime.ts contains changes beyond the version projection" "$partial_base"

# A mode-only change is a change: an executable bit on a projected file.
git -C "$TEST_ROOT" checkout -q -B mode-change "$base"
apply_projection
chmod +x "$TEST_ROOT/sdks/web/src/runtime.ts"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm mode-change
assert_rejected mode-change "$(git -C "$TEST_ROOT" rev-parse HEAD)" "sdks/web/src/runtime.ts is mode 100644 -> 100755"

# Blobs are decoded strictly: lossy decoding would turn two different
# invalid bytes into the same replacement character.
git -C "$TEST_ROOT" checkout -q -B invalid-utf8 "$base"
printf 'export const VERSION = "1.2.3"; // \xff\n' > "$TEST_ROOT/sdks/web/src/generated/bytes.ts"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm invalid-utf8-base
invalid_base="$(git -C "$TEST_ROOT" rev-parse HEAD)"
apply_projection
printf 'export const VERSION = "1.2.4"; // \xfe\n' > "$TEST_ROOT/sdks/web/src/generated/bytes.ts"
git -C "$TEST_ROOT" add .
git -C "$TEST_ROOT" commit -qm invalid-utf8
assert_rejected invalid-utf8 "$(git -C "$TEST_ROOT" rev-parse HEAD)" "is not valid UTF-8" "$invalid_base"

# Only a forward patch release: never a minor, major, skipped, prerelease or
# backward version, even with every surface projected.
assert_version_rejected() {
  local label="$1" version="$2"
  git -C "$TEST_ROOT" checkout -q -B "$label" "$base"
  sed -i.bak "s/1\\.2\\.3/${version}/g" "$TEST_ROOT/Cargo.toml"
  rm -f "$TEST_ROOT/Cargo.toml.bak"
  printf '\n## [%s] - 2026-08-28\n' "$version" >> "$TEST_ROOT/CHANGELOG.md"
  git -C "$TEST_ROOT" add .
  git -C "$TEST_ROOT" commit -qm "$label"
  assert_rejected "$label" "$(git -C "$TEST_ROOT" rev-parse HEAD)" "is not a forward patch release"
}
assert_version_rejected minor-bump 1.3.0
assert_version_rejected major-bump 2.0.0
assert_version_rejected skipped-patch 1.2.5
assert_version_rejected downgrade 1.2.2
assert_version_rejected prerelease 1.2.4-rc.1

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
