#!/usr/bin/env node
// Fail-closed contracts for release-projection evidence reuse in CI: the
// shared classifier (scripts/release-projection-only.mjs), the planner's
// release-projection plan (scripts/ci-cargo-lanes.mjs --reuse-release-evidence),
// the evidence finder and verifier (scripts/ci-release-evidence.mjs, driven
// through a fake `gh`), and the CI gate's own shell (read from ci.yml).
//
// The real v0.9.1 bump (a954c519, whose first parent is 1a4d3419) must
// classify as a projection and plan only the release checks; the same bump
// plus one source edit must not, and the planner must refuse evidence for it.
// Missing commits are fetched from origin (CI checks out depth 2).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  attestationProblems,
  parseEvidenceArgument,
  runProblems,
} from "./ci-release-evidence.mjs";
import { classifyReleaseProjection, isVersionTokenSubstitution } from "./release-projection-only.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const planner = resolve(here, "ci-cargo-lanes.mjs");
const evidenceScript = resolve(here, "ci-release-evidence.mjs");

// The real v0.9.1 release bump and its first parent.
const BUMP_PARENT = "1a4d341922da75ce7524f36b84b667494615e8ed";
const BUMP = "a954c519f592fdad7f47c5bcc7e40a23750e790a";
const BUMP_PARENT_TREE = "ac72cfd357c09c068e9bc0c23b87371990866e2a";
const REPOSITORY = "lukacf/meerkat";
const RUN_ID = 4242;

function git(args, { cwd = root, env = process.env, input } = {}) {
  const result = spawnSync("git", args, { cwd, env, input, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  assert.equal(result.status, 0, `git ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout.trim();
}

function hasCommit(sha) {
  return spawnSync("git", ["cat-file", "-e", `${sha}^{commit}`], { cwd: root }).status === 0;
}

// Depth 2 brings the bump's first parent with it.
if (!hasCommit(BUMP) || !hasCommit(BUMP_PARENT)) {
  const fetched = spawnSync("git", ["fetch", "--no-tags", "--depth=2", "origin", BUMP], { cwd: root, stdio: "inherit" });
  assert.equal(fetched.status, 0, `could not fetch the v0.9.1 bump ${BUMP} from origin`);
}
assert.ok(hasCommit(BUMP) && hasCommit(BUMP_PARENT), "the v0.9.1 bump and its parent are available");
assert.equal(git(["rev-parse", `${BUMP}^1`]), BUMP_PARENT, "1a4d3419 is the bump's first parent");
assert.equal(git(["rev-parse", `${BUMP_PARENT}^{tree}`]), BUMP_PARENT_TREE);

// A scratch object store for synthetic commits; git in this checkout reads
// it through GIT_ALTERNATE_OBJECT_DIRECTORIES, so nothing is written here.
const scratch = mkdtempSync(join(tmpdir(), "ci-release-projection-selftest-"));
const store = join(scratch, "store");
git(["init", "-q", store], { cwd: scratch });
const commonDir = resolve(root, git(["rev-parse", "--git-common-dir"]));
writeFileSync(join(store, ".git/objects/info/alternates"), `${join(commonDir, "objects")}\n`);
const withScratch = { ...process.env, GIT_ALTERNATE_OBJECT_DIRECTORIES: join(store, ".git/objects") };

// A commit whose tree is `fromTree` with `edits` (path -> new text) and whose
// only parent is `parent`.
function synthesize({ fromTree, parent, edits, message }) {
  const index = join(scratch, `index-${Math.random().toString(16).slice(2)}`);
  const env = { ...process.env, GIT_INDEX_FILE: index };
  git(["read-tree", fromTree], { cwd: store, env });
  for (const [path, text] of Object.entries(edits)) {
    const blob = git(["hash-object", "-w", "--stdin"], { cwd: store, input: text });
    git(["update-index", "--cacheinfo", `100644,${blob},${path}`], { cwd: store, env });
  }
  const tree = git(["write-tree"], { cwd: store, env });
  return git(["commit-tree", tree, "-p", parent, "-m", message], { cwd: store });
}

function plan(args, env = withScratch) {
  return spawnSync("node", [planner, "--format", "json", ...args], {
    cwd: root,
    env,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
}

try {
  // --- The version-token alignment the stamped documents use. ---
  assert.ok(isVersionTokenSubstitution("a 0.9.0 b", "a 0.9.1 b", "0.9.0", "0.9.1"));
  assert.ok(isVersionTokenSubstitution("since 0.9.0; =0.9.0", "since 0.9.0; =0.9.1", "0.9.0", "0.9.1"), "a historical mention stays");
  assert.ok(isVersionTokenSubstitution("in 0.9.1 =0.9.0", "in 0.9.1 =0.9.1", "0.9.0", "0.9.1"), "an upcoming mention stays");
  assert.ok(isVersionTokenSubstitution("=0.9.1 x", "=0.9.10 x", "0.9.1", "0.9.10"), "one version may prefix the other");
  assert.ok(!isVersionTokenSubstitution("a 0.9.0 b", "a 0.9.2 b", "0.9.0", "0.9.1"), "another version is not the projection");
  assert.ok(!isVersionTokenSubstitution("v10.9.0", "v10.9.1", "0.9.0", "0.9.1"), "a longer number is not a token");
  assert.ok(!isVersionTokenSubstitution("0.9.0.1", "0.9.1.1", "0.9.0", "0.9.1"), "a dotted continuation is not a token");
  assert.ok(!isVersionTokenSubstitution("0.9.0-rc.1", "0.9.1-rc.1", "0.9.0", "0.9.1"), "a prerelease is another version");
  assert.ok(!isVersionTokenSubstitution("a 0.9.0 b", "a 0.9.1 c", "0.9.0", "0.9.1"), "a prose edit is not the projection");
  assert.ok(!isVersionTokenSubstitution("a 0.9.0", "a 0.9.1 ", "0.9.0", "0.9.1"), "a trailing byte is not the projection");

  // --- The real v0.9.1 bump against a bump plus one source edit. ---
  const real = classifyReleaseProjection({ base: BUMP_PARENT, head: BUMP, cwd: root });
  assert.deepEqual(
    { projection: real.projection, from: real.oldVersion, to: real.newVersion },
    { projection: true, from: "0.9.0", to: "0.9.1" },
    `the real v0.9.1 bump is a release projection (${real.reason ?? ""})`,
  );
  assert.equal(real.paths.length, 78, "the real bump touches 78 paths");
  const sourcePath = "crates/meerkat-core/src/lib.rs";
  const sourceEdit = synthesize({
    fromTree: `${BUMP}^{tree}`,
    parent: BUMP_PARENT,
    edits: { [sourcePath]: `${git(["show", `${BUMP}:${sourcePath}`])}\n// one source edit\n` },
    message: "chore: release v0.9.1 plus one source edit",
  });
  const editedCli = spawnSync(resolve(here, "release-projection-only.mjs"), ["--base", BUMP_PARENT, "--head", sourceEdit, "--verbose"], {
    cwd: root,
    env: withScratch,
    encoding: "utf8",
  });
  assert.equal(editedCli.status, 1, `the bump plus one source edit is not a projection: ${editedCli.stderr}`);
  assert.match(editedCli.stderr, /crates\/meerkat-core\/src\/lib\.rs is not a release projection surface/);
  const realCli = spawnSync(resolve(here, "release-projection-only.mjs"), ["--base", BUMP_PARENT, "--head", BUMP], {
    cwd: root,
    encoding: "utf8",
  });
  assert.equal(realCli.status, 0, `the hook's CLI accepts the real bump: ${realCli.stderr}`);

  // --- Plans: evidence gives the release-projection plan, nothing else does. ---
  const evidence = `${BUMP_PARENT}:${RUN_ID}:1`;
  const reduced = plan(["--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", evidence]);
  assert.equal(reduced.status, 0, `release-projection plan: ${reduced.stderr}`);
  const projectionPlan = JSON.parse(reduced.stdout);
  assert.equal(projectionPlan.mode, "release-projection");
  assert.equal(projectionPlan.rust_changed, false);
  for (const field of [
    "packages", "closure", "shards", "unit_shards", "unit_feature_shards", "main_unit_shards",
    "main_feature_unit_shards", "main_archive_builds", "main_archive_runs", "integration_suites",
    "feature_check_jobs", "test_scope", "closure_test_rows", "reader_packages",
  ]) {
    assert.deepEqual(projectionPlan[field], [], `a release projection plans no ${field}`);
  }
  assert.deepEqual(
    Object.fromEntries(["generated_contract", "bazel_graph", "machine_authority", "machine_posters", "wasm", "sdk_host", "examples_browser", "example_web"].map((key) => [key, projectionPlan[key]])),
    { generated_contract: true, bazel_graph: true, machine_authority: false, machine_posters: false, wasm: false, sdk_host: false, examples_browser: false, example_web: false },
    "a release projection runs the generation ratchets and the Bazel graph check only",
  );
  assert.deepEqual(projectionPlan.release_projection, {
    classifier: "release-projection-only",
    from_version: "0.9.0",
    to_version: "0.9.1",
    base_commit: BUMP_PARENT,
    base_tree: BUMP_PARENT_TREE,
    evidence: {
      workflow_run_id: String(RUN_ID),
      workflow_run_attempt: "1",
      attestation_artifact: `meerkat-ci-attestation-${BUMP_PARENT}-1`,
    },
  });
  const github = spawnSync("node", [planner, "--format", "github", "--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP_PARENT}:${RUN_ID}:pending`], {
    cwd: root,
    encoding: "utf8",
  });
  assert.equal(github.status, 0, github.stderr);
  for (const line of [
    "rust_changed=false",
    "mode=release-projection",
    "release_projection=true",
    `release_evidence_commit=${BUMP_PARENT}`,
    `release_evidence_run_id=${RUN_ID}`,
    "release_evidence_run_attempt=pending",
    "closure_test_count=0",
    "integration_count=0",
    "generated_contract=true",
    "bazel_graph=true",
    "example_web=false",
  ]) {
    assert.ok(github.stdout.split("\n").includes(line), `github output carries ${line}`);
  }

  const full = plan(["--base", BUMP_PARENT, "--head", BUMP]);
  assert.equal(full.status, 0, full.stderr);
  const fullPlan = JSON.parse(full.stdout);
  assert.equal(fullPlan.mode, "workspace", "without evidence the bump gets today's full plan");
  assert.equal(fullPlan.release_projection, null);
  assert.ok(fullPlan.shards.length > 0 && fullPlan.closure_test_rows.length > 0, "the full plan has Clippy and closure-test lanes");

  const refused = [
    ["the bump plus one source edit", ["--base", BUMP_PARENT, "--head", sourceEdit, "--reuse-release-evidence", evidence], /is not a release projection/],
    ["evidence for another commit", ["--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP}:${RUN_ID}:1`], /release evidence names/],
    ["a base that is not the first parent", ["--base", git(["rev-parse", `${BUMP}^{commit}`]), "--head", BUMP, "--reuse-release-evidence", `${BUMP}:${RUN_ID}:1`], /is not the first parent/],
    ["an explicit path list", ["--reuse-release-evidence", evidence, "--", "Cargo.toml"], /needs a --base\/--head diff/],
    ["malformed evidence", ["--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP_PARENT}:latest:1`], /is not <commit>:<run id>:<attempt\|pending>/],
  ];
  for (const [label, args, pattern] of refused) {
    const result = plan(args);
    assert.notEqual(result.status, 0, `${label}: the planner must refuse, not plan fewer lanes`);
    assert.match(result.stderr, pattern, `${label}: ${result.stderr}`);
  }
  const editedFull = plan(["--base", BUMP_PARENT, "--head", sourceEdit]);
  assert.equal(editedFull.status, 0, editedFull.stderr);
  const editedPlan = JSON.parse(editedFull.stdout);
  assert.equal(editedPlan.mode, "workspace", "the bump plus one source edit gets the full plan");
  assert.ok(editedPlan.changed_paths.includes(sourcePath));

  // --- Evidence records. ---
  const goodAttestation = {
    schema_version: 4,
    repository: REPOSITORY,
    commit_sha: BUMP_PARENT,
    tree_sha: BUMP_PARENT_TREE,
    workflow: "CI",
    workflow_run_id: String(RUN_ID),
    workflow_run_attempt: "1",
    event: "push",
    ref: "refs/heads/main",
    validation_backend: "github-hosted-cargo",
    validation_result: "success",
    plan: { mode: "workspace", package_count: 53 },
    component_results: { changes: "success", fmt_governance: "success", ratchets: "success", clippy: "success", sdk_host: "skipped" },
  };
  const expected = { repository: REPOSITORY, commit: BUMP_PARENT, tree: BUMP_PARENT_TREE, runId: RUN_ID, runAttempt: 1 };
  assert.deepEqual(attestationProblems(goodAttestation, expected), []);
  for (const [label, change] of [
    ["another tree", { tree_sha: "0".repeat(40) }],
    ["another commit", { commit_sha: BUMP }],
    ["another attempt", { workflow_run_attempt: "2" }],
    ["a pull request", { event: "pull_request" }],
    ["a release branch", { ref: "refs/heads/release/0.9" }],
    ["schema 3", { schema_version: 3 }],
    ["a failed lane", { component_results: { ...goodAttestation.component_results, clippy: "failure" } }],
    ["chained reuse", { plan: { mode: "release-projection" } }],
    ["recorded reuse", { reused_test_evidence: { workflow_run_id: "1" } }],
  ]) {
    assert.ok(attestationProblems({ ...goodAttestation, ...change }, expected).length > 0, `an attestation from ${label} is not evidence`);
  }
  const goodRun = {
    id: RUN_ID,
    name: "CI",
    path: ".github/workflows/ci.yml",
    event: "push",
    head_branch: "main",
    head_sha: BUMP_PARENT,
    status: "completed",
    conclusion: "success",
    run_attempt: 1,
    html_url: `https://github.com/${REPOSITORY}/actions/runs/${RUN_ID}`,
  };
  assert.deepEqual(runProblems(goodRun, BUMP_PARENT), []);
  for (const [label, change] of [
    ["a cancelled run", { conclusion: "cancelled" }],
    ["a running run", { status: "in_progress", conclusion: null }],
    ["a pull-request run", { event: "pull_request" }],
    ["a release-branch run", { head_branch: "release/0.9" }],
    ["another workflow", { path: ".github/workflows/nightly.yml" }],
    ["another commit", { head_sha: BUMP }],
  ]) {
    assert.ok(runProblems({ ...goodRun, ...change }, BUMP_PARENT).length > 0, `${label} is not evidence`);
  }
  assert.deepEqual(parseEvidenceArgument(`${BUMP_PARENT}:12:pending`), { commit: BUMP_PARENT, runId: "12", runAttempt: "pending" });
  assert.throws(() => parseEvidenceArgument(`${BUMP_PARENT}:12`), /is not <commit>/);

  // --- The finder and verifier through a fake gh. ---
  const fakeGh = join(scratch, "gh");
  const fixtures = join(scratch, "gh-fixture.json");
  writeFileSync(
    fakeGh,
    `#!/usr/bin/env node
const fs = require("node:fs");
const { createHash } = require("node:crypto");
const fixture = JSON.parse(fs.readFileSync(${JSON.stringify(fixtures)}, "utf8"));
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(join(scratch, "gh-calls.log"))}, args.join(" ") + "\\n");
if (args[0] === "api") {
  const path = args[1];
  const responses = fixture.api[path];
  if (responses === undefined) { process.stderr.write("gh: Not Found (HTTP 404)\\n"); process.exit(1); }
  const list = Array.isArray(responses) ? responses : [responses];
  const counter = ${JSON.stringify(join(scratch, "gh-counter-"))} + createHash("sha256").update(path).digest("hex");
  const seen = fs.existsSync(counter) ? Number(fs.readFileSync(counter, "utf8")) : 0;
  fs.writeFileSync(counter, String(seen + 1));
  process.stdout.write(JSON.stringify(list[Math.min(seen, list.length - 1)]));
} else if (args[0] === "run" && args[1] === "download") {
  const name = args[args.indexOf("--name") + 1];
  const dir = args[args.indexOf("--dir") + 1];
  const artifact = fixture.artifacts[args[2] + "/" + name];
  if (artifact === undefined) { process.stderr.write("no artifact matches " + name + "\\n"); process.exit(1); }
  fs.writeFileSync(dir + "/attestation.json", JSON.stringify(artifact));
} else {
  process.exit(2);
}
`,
  );
  chmodSync(fakeGh, 0o755);
  const listPath = `repos/${REPOSITORY}/actions/workflows/ci.yml/runs?branch=main&event=push&head_sha=${BUMP_PARENT}&per_page=100`;
  const runPath = `repos/${REPOSITORY}/actions/runs/${RUN_ID}`;
  const artifactKey = `${RUN_ID}/meerkat-ci-attestation-${BUMP_PARENT}-1`;
  const evidenceRun = (mode, args, fixture, extraEnv = {}) => {
    writeFileSync(fixtures, JSON.stringify(fixture));
    for (const name of readdirSync(scratch)) {
      if (name.startsWith("gh-counter-")) rmSync(join(scratch, name));
    }
    return spawnSync("node", [evidenceScript, mode, "--repository", REPOSITORY, ...args], {
      cwd: root,
      env: { ...withScratch, MEERKAT_CI_EVIDENCE_GH: fakeGh, MEERKAT_CI_EVIDENCE_POLL_SECONDS: "1", ...extraEnv },
      encoding: "utf8",
    });
  };
  const bumpArgs = ["--base", BUMP_PARENT, "--head", BUMP];
  const okFixture = { api: { [listPath]: { workflow_runs: [goodRun] }, [runPath]: goodRun }, artifacts: { [artifactKey]: goodAttestation } };

  let result = evidenceRun("find", bumpArgs, okFixture);
  assert.equal(result.status, 0, `find accepts a verified successful parent run: ${result.stderr}`);
  assert.equal(result.stdout.trim(), `${BUMP_PARENT}:${RUN_ID}:1`);

  result = evidenceRun("find", bumpArgs, { ...okFixture, api: { [listPath]: { workflow_runs: [{ ...goodRun, status: "in_progress", conclusion: null }] } } });
  assert.equal(result.status, 0, `find hands a running parent run to the gate: ${result.stderr}`);
  assert.equal(result.stdout.trim(), `${BUMP_PARENT}:${RUN_ID}:pending`);

  for (const [label, fixture] of [
    ["no run", { api: { [listPath]: { workflow_runs: [] } }, artifacts: {} }],
    ["a failed run", { ...okFixture, api: { [listPath]: { workflow_runs: [{ ...goodRun, conclusion: "failure" }] } } }],
    ["a cancelled run", { ...okFixture, api: { [listPath]: { workflow_runs: [{ ...goodRun, conclusion: "cancelled" }] } } }],
    ["a missing attestation", { ...okFixture, artifacts: {} }],
    ["a reused attestation", { ...okFixture, artifacts: { [artifactKey]: { ...goodAttestation, plan: { mode: "release-projection" } } } }],
    ["an attestation of another tree", { ...okFixture, artifacts: { [artifactKey]: { ...goodAttestation, tree_sha: "1".repeat(40) } } }],
    ["a lookup failure", { api: {}, artifacts: {} }],
  ]) {
    result = evidenceRun("find", bumpArgs, fixture);
    assert.equal(result.status, 1, `find: ${label} is no evidence (stdout ${result.stdout}, stderr ${result.stderr})`);
    assert.equal(result.stdout, "", `find: ${label} prints no evidence`);
  }
  rmSync(join(scratch, "gh-calls.log"), { force: true });
  result = evidenceRun("find", ["--base", BUMP_PARENT, "--head", sourceEdit], okFixture);
  assert.equal(result.status, 1, "find refuses the bump plus one source edit");
  assert.match(result.stderr, /is not a release projection/);
  assert.ok(!existsSync(join(scratch, "gh-calls.log")), "a non-projection never reaches the API");

  // verify waits for a running parent run, then checks it and its attestation.
  const outputFile = join(scratch, "github-output");
  writeFileSync(outputFile, "");
  result = evidenceRun("verify", [...bumpArgs, "--run-id", String(RUN_ID), "--wait-seconds", "30", "--github-output", outputFile], {
    ...okFixture,
    api: { [runPath]: [{ ...goodRun, status: "in_progress", conclusion: null }, goodRun] },
  });
  assert.equal(result.status, 0, `verify accepts the parent run once it succeeds: ${result.stderr}`);
  assert.match(result.stderr, /waiting for CI run/);
  const outputs = Object.fromEntries(readFileSync(outputFile, "utf8").trim().split("\n").map((line) => line.split(/=(.*)/s).slice(0, 2)));
  assert.deepEqual(
    { run_id: outputs.run_id, run_attempt: outputs.run_attempt, commit: outputs.commit, tree: outputs.tree, attestation_artifact: outputs.attestation_artifact, from_version: outputs.from_version, to_version: outputs.to_version },
    { run_id: String(RUN_ID), run_attempt: "1", commit: BUMP_PARENT, tree: BUMP_PARENT_TREE, attestation_artifact: `meerkat-ci-attestation-${BUMP_PARENT}-1`, from_version: "0.9.0", to_version: "0.9.1" },
  );
  const attemptPath = `${runPath}/attempts/1`;
  result = evidenceRun("verify", [...bumpArgs, "--run-id", String(RUN_ID), "--run-attempt", "1"], { ...okFixture, api: { [attemptPath]: goodRun } });
  assert.equal(result.status, 0, `verify accepts a named successful attempt: ${result.stderr}`);
  for (const [label, args, fixture] of [
    ["a run that ends cancelled", ["--run-id", String(RUN_ID), "--wait-seconds", "30"], { ...okFixture, api: { [runPath]: [{ ...goodRun, status: "in_progress", conclusion: null }, { ...goodRun, conclusion: "cancelled" }] } }],
    ["a run still running at the deadline", ["--run-id", String(RUN_ID), "--wait-seconds", "0"], { ...okFixture, api: { [runPath]: { ...goodRun, status: "in_progress", conclusion: null } } }],
    ["another commit's run", ["--run-id", String(RUN_ID)], { ...okFixture, api: { [runPath]: { ...goodRun, head_sha: BUMP } } }],
    ["a named attempt that failed", ["--run-id", String(RUN_ID), "--run-attempt", "1"], { ...okFixture, api: { [attemptPath]: { ...goodRun, conclusion: "failure" } } }],
    ["an attestation of another tree", ["--run-id", String(RUN_ID)], { ...okFixture, artifacts: { [artifactKey]: { ...goodAttestation, tree_sha: "1".repeat(40) } } }],
  ]) {
    result = evidenceRun("verify", [...bumpArgs, ...args], fixture);
    assert.equal(result.status, 1, `verify: ${label} fails closed (stderr ${result.stderr})`);
  }
  result = evidenceRun("verify", ["--base", BUMP_PARENT, "--head", sourceEdit, "--run-id", String(RUN_ID)], okFixture);
  assert.equal(result.status, 1, "verify refuses the bump plus one source edit");

  // --- The CI gate's own shell, read from ci.yml. ---
  const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  const gateCheck = extractStepRun(workflow, "Check lane results");
  const gateEnv = {
    CHANGES_RESULT: "success", FMT_GOVERNANCE_RESULT: "success", RATCHETS_RESULT: "success",
    TLC_AUDITS_RESULT: "skipped", MACHINE_AUTHORITY: "false", CLIPPY_RESULT: "skipped",
    UNIT_RESULT: "skipped", INTEGRATION_RESULT: "skipped", GOVERNED_JSONL_RESULT: "skipped",
    MCP_CONFINEMENT_POSITIVE_RESULT: "skipped", INTEGRATION_COUNT: "0", CLOSURE_TESTS_RESULT: "skipped",
    CLOSURE_TEST_COUNT: "0", TEST_SCOPE_COUNT: "0", HOOK_BINARY_COUNT: "0", HOOK_RETAINED_COUNT: "0",
    MAIN_UNIT_PACKED_RESULT: "skipped", MAIN_UNIT_ARCHIVE_BUILD_RESULT: "skipped", MAIN_UNIT_ARCHIVE_RUN_RESULT: "skipped",
    CLOSURE_CHECK_RESULT: "skipped", FEATURE_CHECK_RESULT: "skipped", FEATURE_CHECK_COUNT: "0",
    WASM: "false", WASM_CHECK_RESULT: "skipped", SDK_HOST_RESULT: "skipped", EXAMPLE_WEB_RESULT: "skipped",
    WASM_TIMERS_RESULT: "skipped", BAZEL_GRAPH_RESULT: "success", RUST_CHANGED: "false", IS_MAIN_PUSH: "true",
    SDK_HOST: "false", BAZEL_GRAPH: "true", EXAMPLES_BROWSER: "false", EXAMPLE_WEB: "false",
    MODE: "release-projection", REASON: "release projection", SHARD_COUNT: "0", UNIT_SHARD_COUNT: "0",
    UNIT_DEFERRED: "", UNIT_DEFERRED_COUNT: "0", PACKAGE_COUNT: "0", CLOSURE_COUNT: "0",
    GENERATED_CONTRACT: "true", RELEASE_PROJECTION: "true",
    RELEASE_EVIDENCE_COMMIT: BUMP_PARENT, RELEASE_EVIDENCE_RUN_ID: String(RUN_ID),
  };
  const gate = (change) => spawnSync("bash", ["-c", gateCheck], { env: { ...process.env, ...gateEnv, ...change }, encoding: "utf8" });
  assert.equal(gate({}).status, 0, `the gate accepts a release-projection main push: ${gate({}).stdout}`);
  assert.equal(gate({ IS_MAIN_PUSH: "false" }).status, 0, "and a release-projection pull request");
  for (const [label, change] of [
    ["a failed ratchet", { RATCHETS_RESULT: "failure" }],
    ["a failed Bazel graph check", { BAZEL_GRAPH_RESULT: "failure" }],
    ["a projection plan with Rust lanes", { RUST_CHANGED: "true" }],
    ["a projection plan with closure-test rows", { CLOSURE_TEST_COUNT: "2" }],
    ["a projection plan without the ratchets", { GENERATED_CONTRACT: "false" }],
    ["a projection plan without evidence", { RELEASE_EVIDENCE_RUN_ID: "" }],
    ["the projection mode without its flag", { RELEASE_PROJECTION: "false" }],
    ["an ordinary main push without the example web suites", { RELEASE_PROJECTION: "false", MODE: "none", RELEASE_EVIDENCE_COMMIT: "", RELEASE_EVIDENCE_RUN_ID: "" }],
  ]) {
    assert.equal(gate(change).status, 1, `the gate refuses ${label}`);
  }
  // The gate verifies the evidence after the lanes and records it.
  const verifyStep = extractStepRun(workflow, "Verify reused release-projection evidence");
  assert.match(verifyStep, /node scripts\/ci-release-evidence\.mjs verify /);
  assert.match(verifyStep, /--head HEAD --run-id "\$\{RELEASE_EVIDENCE_RUN_ID\}"/);
  const attestationStep = extractStepRun(workflow, "Build exact-tree CI attestation");
  assert.match(attestationStep, /reused_test_evidence: \$reused/);
  assert.match(workflow, /node scripts\/ci-release-evidence\.mjs find \\\n\s+--repository "\$\{GITHUB_REPOSITORY\}" --base "\$\{classification_base\}" --head HEAD/);
  const release = readFileSync(resolve(root, ".github/workflows/release.yml"), "utf8");
  assert.match(release, /\.reused_test_evidence\.base_commit_sha == \$parent/, "require_ci_green binds the evidence to the release commit's first parent");
  assert.match(release, /node scripts\/ci-release-evidence\.mjs verify --repository "\$\{GITHUB_REPOSITORY\}"/, "require_ci_green re-verifies the reused run");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// The `run: |` body of the named step: the indented block under it.
function extractStepRun(text, name) {
  const lines = text.split("\n");
  const at = lines.findIndex((line) => line.trim() === `- name: ${name}`);
  assert.ok(at >= 0, `ci.yml or release.yml has a step named ${name}`);
  const stepIndent = lines[at].indexOf("-");
  let runAt = -1;
  for (let index = at + 1; index < lines.length; index += 1) {
    const indent = lines[index].search(/\S/);
    if (indent >= 0 && indent <= stepIndent) break;
    if (/^\s+run: \|\s*$/.test(lines[index])) {
      runAt = index;
      break;
    }
  }
  assert.ok(runAt >= 0, `step ${name} has a run block`);
  const runIndent = lines[runAt].search(/\S/);
  const body = [];
  for (let index = runAt + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim() !== "" && line.search(/\S/) <= runIndent) break;
    body.push(line);
  }
  const bodyIndent = Math.min(...body.filter((line) => line.trim() !== "").map((line) => line.search(/\S/)));
  return body.map((line) => line.slice(bodyIndent)).join("\n");
}

console.log("release-projection evidence contracts hold");
