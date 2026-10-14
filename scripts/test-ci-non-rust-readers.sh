#!/usr/bin/env bash
# Contract test for scripts/ci-non-rust-readers-guard.mjs, the guard that keeps
# scripts/ci-non-rust-readers.json (the Rust readers of non-Rust inputs) in
# step with the sources.
#
# GREEN: the committed inventory matches the scan of this checkout.
# RED:   an inventory with one reader row deleted fails, naming the reader's
#        file and line; an inventory with an invented row fails as stale; a
#        scratch copy of the checkout with a new, unlisted reader fails, naming
#        the new file and line. The scratch copy is a throwaway clone; nothing
#        synthetic touches this checkout.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GUARD="${REPO_ROOT}/scripts/ci-non-rust-readers-guard.mjs"
INVENTORY="${REPO_ROOT}/scripts/ci-non-rust-readers.json"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-non-rust-readers.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'test-ci-non-rust-readers: FAIL: %s\n' "$*" >&2
  exit 1
}

expect_red() {
  local label="$1" log="$2" needle="$3"
  shift 3
  local rc=0
  "$@" >"$log" 2>&1 || rc=$?
  [[ "$rc" -eq 1 ]] || { cat "$log" >&2; fail "${label}: guard exited ${rc}, expected 1"; }
  grep -qF -- "$needle" "$log" || { cat "$log" >&2; fail "${label}: output does not name ${needle}"; }
  printf 'RED   %s: %s\n' "$label" "$(grep -F -- "$needle" "$log" | head -1)"
}

# GREEN on this checkout.
node "$GUARD" >"${TEST_ROOT}/green.log" 2>&1 || { cat "${TEST_ROOT}/green.log" >&2; fail "guard is red on the committed inventory"; }
printf 'GREEN %s\n' "$(grep -F 'ci-non-rust-readers: OK' "${TEST_ROOT}/green.log")"

# RED: delete the help_mobpack_contract row for docs/docs.json.
node - "$INVENTORY" "${TEST_ROOT}/deleted.json" <<'NODE'
const fs = require("node:fs");
const [from, to] = process.argv.slice(2);
const doc = JSON.parse(fs.readFileSync(from, "utf8"));
const rows = doc.paths["docs/docs.json"];
const kept = (rows ?? []).filter((row) => !(row.package === "rkat" && row.target === "test:help_mobpack_contract"));
if (!rows || kept.length === rows.length) throw new Error("fixture: docs/docs.json has no rkat test:help_mobpack_contract row");
if (kept.length) doc.paths["docs/docs.json"] = kept;
else delete doc.paths["docs/docs.json"];
fs.writeFileSync(to, JSON.stringify(doc, null, 2));
NODE
expect_red "deleted inventory row" "${TEST_ROOT}/deleted.log" \
  "missing reader: crates/meerkat-cli/tests/help_mobpack_contract.rs:5 reads docs/docs.json" \
  node "$GUARD" --inventory "${TEST_ROOT}/deleted.json"

# RED: an invented row no source produces.
node - "$INVENTORY" "${TEST_ROOT}/stale.json" <<'NODE'
const fs = require("node:fs");
const [from, to] = process.argv.slice(2);
const doc = JSON.parse(fs.readFileSync(from, "utf8"));
doc.paths["docs/index.mdx"] = [
  { package: "rkat", target: "test:help_mobpack_contract", scope: "target", sources: ["crates/meerkat-cli/tests/help_mobpack_contract.rs"] },
];
doc.paths = Object.fromEntries(Object.entries(doc.paths).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
fs.writeFileSync(to, JSON.stringify(doc, null, 2));
NODE
expect_red "stale inventory row" "${TEST_ROOT}/stale.log" \
  'stale inventory entry: "docs/index.mdx" lists rkat test:help_mobpack_contract' \
  node "$GUARD" --inventory "${TEST_ROOT}/stale.json"

# RED: a new reader in a scratch copy. Clone the committed tree, overlay this
# checkout's guard files (so an uncommitted guard is what runs), and add a
# test target that reads docs/docs.json from outside its crate.
SCRATCH="${TEST_ROOT}/scratch"
git clone --quiet --shared --no-checkout "$REPO_ROOT" "$SCRATCH"
git -C "$SCRATCH" checkout --quiet --detach "$(git -C "$REPO_ROOT" rev-parse HEAD)"
for file in scripts/ci-non-rust-readers.json scripts/ci-non-rust-readers.mjs scripts/ci-non-rust-readers-guard.mjs scripts/rust-test-selector.mjs; do
  cp "${REPO_ROOT}/${file}" "${SCRATCH}/${file}"
done
SYNTHETIC="crates/meerkat-machine-schema/tests/synthetic_non_rust_reader.rs"
cat >"${SCRATCH}/${SYNTHETIC}" <<'RUST'
#[test]
fn synthetic_reader_of_the_docs_navigation() {
    let navigation = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/docs.json"));
    assert!(navigation.is_ok());
}
RUST
expect_red "new unlisted reader in a scratch copy" "${TEST_ROOT}/synthetic.log" \
  "missing reader: ${SYNTHETIC}:3 reads docs/docs.json (meerkat-machine-schema test:synthetic_non_rust_reader" \
  node "${SCRATCH}/scripts/ci-non-rust-readers-guard.mjs"

printf 'test-ci-non-rust-readers: OK\n'
