#!/usr/bin/env bash
# Contract for dropping the pre-push hook's cargo-test body.
#
# The hook ran scripts/pre-push-unit.sh on every push: the whole workspace's
# lib tests, the fast selection of every tests/*.rs binary, HeadCanonical and
# e2e-fast. That body is no longer a push hook because required CI runs the
# same binaries for the change's test scope: the closure-tests job, whose rows
# scripts/ci-cargo-lanes.mjs plans and scripts/ci-hook-coverage.mjs checks
# against the hook's own selection on every plan. This test pins each half of
# that hand-off, so neither can go without the other:
#   1. no push hook runs scripts/pre-push-unit.sh;
#   2. ci.yml runs the closure-tests rows and the gate requires them;
#   3. the planner covers the hook's binaries on sample changes;
#   4. the checker's model of the hook still matches pre-push-unit.sh.
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$REPO_ROOT"

fail() {
  echo "pre-push cargo-test hand-off contract: $*" >&2
  exit 1
}

config=.pre-commit-config.yaml
ci=.github/workflows/ci.yml

# 1. The hook no longer runs the cargo-test body.
if grep -Eq 'entry: scripts/pre-push-unit\.sh|id: cargo-test$' "$config"; then
  fail "a pre-push hook still runs scripts/pre-push-unit.sh"
fi

# 2. Required CI runs its replacement.
python3 - "$ci" <<'PY' || fail "ci.yml no longer runs and requires the closure-tests rows"
import re
import sys

text = open(sys.argv[1]).read()
job = re.search(r"\n  closure-tests:\n(.*?)\n  [a-z][a-z0-9-]*:\n", text, re.S)
assert job, "no closure-tests job"
body = job.group(1)
assert "needs.changes.outputs.closure_test_count != '0'" in body, "closure-tests condition"
assert "matrix: ${{ fromJSON(needs.changes.outputs.closure_test_matrix) }}" in body, "closure-tests matrix"
assert "node scripts/ci-verify-row-binaries.mjs" in body, "closure-tests binary check"
gate = re.search(r"\n  gate:\n(.*?)\n      - name: Enforce lane execution budget", text, re.S)
assert gate, "no gate"
assert "      - closure-tests\n" in gate.group(1), "the gate does not need closure-tests"
assert (
    '          if [[ "${CLOSURE_TEST_COUNT}" =~ ^[0-9]+$ ]] && (( CLOSURE_TEST_COUNT > 0 )); then\n'
    '            require_ran "Closure tests" "${CLOSURE_TESTS_RESULT}"\n'
) in gate.group(1), "the gate does not require the planned closure-tests rows"
PY

# 3. The planner covers the hook's binaries for the test scope; a gap fails
#    the plan, so a zero-gap report here is the planner's own check passing.
python3 - <<'PY' || fail "the CI plan does not cover the hook's cargo-test binaries"
import json
import subprocess

for path, at_least in [
    ("crates/meerkat-core/src/lib.rs", 400),
    ("crates/meerkat-mob/src/lib.rs", 100),
    ("docs/index.mdx", 1),
]:
    plan = json.loads(subprocess.run(
        ["node", "scripts/ci-cargo-lanes.mjs", "--format", "json", "--", path],
        check=True, capture_output=True, text=True,
    ).stdout)
    coverage = plan["hook_coverage"]
    assert coverage["missing"] == [] and coverage["errors"] == [], f"{path}: {coverage}"
    assert coverage["hook_binaries"] >= at_least, f"{path}: only {coverage['hook_binaries']} hook binaries"
    assert plan["closure_test_rows"], f"{path}: no closure-tests rows"
PY

# 4. HOOK_LANES (scripts/ci-hook-coverage.mjs) still describes the commands
#    of the body that left the hook; the script stays for manual runs.
unit=scripts/pre-push-unit.sh
# shellcheck disable=SC2016 # literal command text, not expansions
for command in \
  "-E 'kind(lib)' --no-tests=fail --no-fail-fast" \
  "--profile fast -E 'kind(test)' --no-tests=fail --no-fail-fast" \
  "-E 'binary(e2e_fast_lane)' --no-tests=fail --no-fail-fast" \
  '"$CARGO" nextest list -p meerkat-mob --test cold_restart_mob_resume' \
  "--features test-support --profile fast" \
  "--profile fast -E 'binary(cold_restart_mob_resume)' --no-tests=fail" \
  "-E 'kind(lib) or kind(test)'"; do
  grep -Fq -- "$command" "$unit" || fail "$unit no longer runs: $command"
done
node --input-type=module -e '
import { HOOK_LANES } from "./scripts/ci-hook-coverage.mjs";
const expected = {
  unit: { filter: "kind(lib)", profile: "default" },
  integration: { filter: "kind(test)", profile: "fast" },
  e2eFast: { package: "meerkat-integration-tests", binary: "e2e_fast_lane", profile: "default" },
  headCanonical: { package: "meerkat-mob", test: "cold_restart_mob_resume", features: ["test-support"], profile: "fast" },
};
if (JSON.stringify(HOOK_LANES) !== JSON.stringify(expected)) {
  console.error(JSON.stringify(HOOK_LANES));
  process.exit(1);
}
' || fail "HOOK_LANES no longer matches the hook body it replaced"

echo "pre-push cargo-test hand-off contracts hold"
