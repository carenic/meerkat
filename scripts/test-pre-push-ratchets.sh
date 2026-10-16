#!/usr/bin/env bash
# Contracts for the changed-path ratchets hook, which replaced the hook's
# Clippy body and is now a manual hook: no push-stage hook runs the ratchets,
# Clippy, the deterministic test lanes or the machine codegen/TLC lanes.
#
# Dropping them from the push keeps coverage because CI runs each before
# merge: Clippy is a required CI check (the CI planner,
# scripts/ci-cargo-lanes.mjs, gives every package the agent gate would lint a
# Clippy lane, or plans the whole workspace; those lanes run
# `clippy --no-deps <targets> --all-features -- -D warnings`, every target
# except a pull request's meerkat-mob chain shards, which lint their lib and
# bin targets; and the CI gate requires them whenever Rust changed), and the
# ratchets and governance-gates jobs, example-web and wasm-check run the
# ratchets for the same changed paths. This test pins those facts, checks the package claim on
# sample changes against the agent gate's own selection, and checks that the
# manual hook keeps the agent gate's ratchets and runs no Clippy and no
# tests.
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$REPO_ROOT"

fail() {
  echo "pre-push ratchets contract: $*" >&2
  exit 1
}

config=.pre-commit-config.yaml
ci=.github/workflows/ci.yml

# 1. The ratchets hook stays as a manual hook, the push stage runs none of
#    the lanes CI runs, and nothing runs the old Clippy body.
grep -Fq -- '- id: changed-path-ratchets' "$config" || fail "the changed-path-ratchets hook is missing"
grep -Fq 'entry: scripts/pre-push-ratchets.sh' "$config" || fail "the ratchets hook does not run scripts/pre-push-ratchets.sh"
if grep -Eq 'pre-push-clippy|id: cargo-clippy' "$config"; then
  fail "a pre-push hook still runs the Clippy body"
fi
python3 - "$config" <<'PY' || fail "a push-stage hook runs a lane CI runs before merge"
import sys

import yaml

config = yaml.safe_load(open(sys.argv[1]))
default = config.get("default_stages", ["pre-commit"])
stages = {hook["id"]: hook.get("stages", default) for repo in config["repos"] for hook in repo["hooks"]}
for hook_id in ("changed-path-ratchets", "machine-codegen-drift", "machine-codegen-verify", "cargo-test"):
    assert stages.get(hook_id) == ["manual"], f"{hook_id} runs in {stages.get(hook_id)}, not only manually"
PY
[[ ! -e scripts/pre-push-clippy.sh ]] || fail "scripts/pre-push-clippy.sh exists again"
grep -Fq 'exec "$AGENT_GATE" --committed --ratchets-only --machine-drift-by-hook "$@"' scripts/pre-push-ratchets.sh ||
  fail "pre-push-ratchets.sh does not run the agent gate in ratchets-only mode"

# 2. The required CI Clippy lanes the hook relies on: flags, trigger, gate.
grep -Fxq '          ./scripts/repo-cargo clippy ${PACKAGES} --no-deps ${TARGETS} --all-features -- -D warnings' "$ci" ||
  fail "the CI Clippy lanes no longer run --no-deps <targets> --all-features -- -D warnings"
grep -Fxq '          TARGETS: ${{ matrix.targets }}' "$ci" ||
  fail "the CI Clippy lanes no longer take their targets from the plan"
python3 - "$ci" <<'PY' || fail "the CI Clippy job or its gate requirement changed"
import re
import sys

text = open(sys.argv[1]).read()
job = re.search(r"\n  clippy:\n(.*?)\n  [a-z][a-z0-9-]*:\n", text, re.S)
assert job, "no clippy job"
assert "    if: ${{ needs.changes.outputs.rust_changed == 'true' }}\n" in job.group(1), "clippy job condition"
assert "      matrix: ${{ fromJSON(needs.changes.outputs.shard_matrix) }}\n" in job.group(1), "clippy matrix"
required = (
    '          if [[ "${RUST_CHANGED}" == "true" ]]; then\n'
    '            if ! [[ "${SHARD_COUNT}" =~ ^[0-9]+$ ]] || (( SHARD_COUNT < 1 )); then\n'
    "              echo \"::error::a build-relevant change produced no lanes (shard_count='${SHARD_COUNT}').\"\n"
    "              failed=1\n"
    "            fi\n"
    '            require_ran "Clippy" "${CLIPPY_RESULT}"\n'
)
assert required in text, "the gate does not require Clippy lanes on Rust changes"
PY

# 3. On sample changes, the CI Clippy lanes cover every package the agent
#    gate's Clippy selection (what the hook used to lint) would lint.
python3 - <<'PY' || fail "the CI planner does not cover the agent gate's Clippy selection"
import json
import re
import subprocess

def run(*args):
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"{' '.join(args)} failed ({result.returncode}): {result.stderr[-2000:]}")
    return result.stdout

def embedded_input_sample():
    # The selector's own list: (path, package, kind) per embedded input.
    for line in run("scripts/rust-embedded-inputs.mjs").splitlines():
        path, package, kind = line.split("\t")
        if kind == "file":
            return path
    raise SystemExit("scripts/rust-embedded-inputs.mjs lists no embedded input file")

samples = [
    "crates/meerkat-runtime/src/lib.rs",
    "crates/meerkat-openai/Cargo.toml",
    embedded_input_sample(),
    "Cargo.lock",
    ".cargo/config.toml",
    "scripts/repo-cargo",
]
for path in samples:
    gate = run("scripts/cargo-agent-gate", "--clippy-only", "--dry-run", "--", path)
    if "clippy --workspace" in gate:
        hook = "workspace"
    else:
        match = re.search(r"Cargo agent gate packages:\n((?:  \S+\n)+)", gate)
        hook = set(match.group(1).split()) if match else set()
    for event in ("pull_request", "push"):
        plan = json.loads(run("node", "scripts/ci-cargo-lanes.mjs", "--event", event, "--format", "json", "--", path))
        ci = {name for shard in plan["shards"] for name in shard["packages"]}
        # Every target on the push; a pull request's mob-chain shards lint
        # their lib and bin targets.
        for shard in plan["shards"]:
            chain = all(plan["package_model"][name]["heavy_chain"] for name in shard["packages"])
            allowed = {"--all-targets", "--lib", "--bins", "--lib --bins"} if event == "pull_request" and chain else {"--all-targets"}
            if shard["targets"] not in allowed:
                raise SystemExit(f"{path} ({event}): shard {shard['name']} lints {shard['targets']}")
        if plan["mode"] == "workspace":
            print(f"covered: {path} ({event}): CI plans the whole workspace")
            continue
        if hook == "workspace":
            raise SystemExit(f"{path} ({event}): the agent gate lints the workspace, the CI planner only {sorted(ci)}")
        missing = hook - ci
        if missing:
            raise SystemExit(f"{path} ({event}): CI Clippy lanes miss {sorted(missing)} (planned {sorted(ci)})")
        print(f"covered: {path} ({event}): hook {sorted(hook)} within CI {sorted(ci)}")
PY

# 4. --ratchets-only keeps the ratchets and runs no Clippy and no tests.
ratchets() {
  scripts/cargo-agent-gate --ratchets-only --machine-drift-by-hook --dry-run -- "$@" 2>&1
}
expect_quiet() {
  local output
  output="$(ratchets "$@")" || fail "ratchets-only failed for $*"
  [[ "$output" == *"ratchets only"* ]] || fail "ratchets-only did not report itself for $*: ${output}"
  if grep -Eq 'clippy|nextest|fast --no-run' <<<"$output"; then
    fail "ratchets-only ran Clippy or tests for $*: ${output}"
  fi
}
expect_ratchet() {
  local expected="$1" output
  shift
  output="$(ratchets "$@")" || fail "ratchets-only failed for $*"
  grep -Fq -- "$expected" <<<"$output" || fail "ratchets-only lost '${expected}' for $*: ${output}"
  if grep -Eq 'clippy|nextest|fast --no-run' <<<"$output"; then
    fail "ratchets-only ran Clippy or tests for $*: ${output}"
  fi
}
expect_quiet crates/meerkat-runtime/src/lib.rs
expect_quiet Cargo.lock
expect_ratchet 'DRY-RUN make test-sdk-web' sdks/web/src/index.ts
expect_ratchet 'DRY-RUN make wasm-check' crates/meerkat-web-runtime/src/lib.rs
expect_ratchet 'DRY-RUN make machine-authority-docs-gate runtime-authority-bypass seam-inventory rmat-audit' \
  docs/architecture/meerkat-dogma.md
expect_ratchet 'DRY-RUN make verify-schema-freshness' docs/api/rpc.mdx
if scripts/cargo-agent-gate --ratchets-only --clippy-only --dry-run -- Cargo.lock >/dev/null 2>&1; then
  fail "--ratchets-only combined with --clippy-only was accepted"
fi

echo "pre-push ratchets contracts hold"
