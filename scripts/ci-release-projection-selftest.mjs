#!/usr/bin/env node
// Fail-closed contracts for release-projection evidence reuse in CI: the
// shared classifier (scripts/release-projection-only.mjs), the planner's
// release-projection plan (scripts/ci-cargo-lanes.mjs --reuse-release-evidence),
// the evidence finder and verifier (scripts/ci-release-evidence.mjs, driven
// through a fake `gh`), and the CI gate's own shell (read from ci.yml).
//
// The real v0.9.1 bump (a954c519, whose first parent is 1a4d3419) must
// classify as a projection and plan only the release checks and the readers
// of its changed files; the same bump plus one source edit must not, and the
// planner must refuse evidence for it. Only a completed, workspace-mode,
// all-success parent run is evidence, and `CI-Plan: workspace` gives a
// release's parent such a run. Missing commits are fetched from origin (CI
// checks out depth 2).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  WORKSPACE_LANES,
  attestationProblems,
  parseEvidenceArgument,
  runProblems,
} from "./ci-release-evidence.mjs";
import { loadNonRustReaders, readerPackagesFor } from "./ci-non-rust-readers.mjs";
import { classifyReleaseProjection, isForwardPatchBump } from "./release-projection-only.mjs";

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

// Hosted runners have no git identity, and the synthetic commits below need
// one: every git call of this test carries a fixed identity and fixed dates,
// so it also builds the same commits on every run.
const GIT_IDENTITY = {
  GIT_AUTHOR_NAME: "ci-selftest",
  GIT_AUTHOR_EMAIL: "ci-selftest@invalid",
  GIT_AUTHOR_DATE: "2026-10-15T00:00:00Z",
  GIT_COMMITTER_NAME: "ci-selftest",
  GIT_COMMITTER_EMAIL: "ci-selftest@invalid",
  GIT_COMMITTER_DATE: "2026-10-15T00:00:00Z",
};
const baseEnv = { ...process.env, ...GIT_IDENTITY };

function git(args, { cwd = root, env = baseEnv, input } = {}) {
  const result = spawnSync("git", args, { cwd, env: { ...env, ...GIT_IDENTITY }, input, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
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
const withScratch = { ...baseEnv, GIT_ALTERNATE_OBJECT_DIRECTORIES: join(store, ".git/objects") };

// A commit whose tree is `fromTree` with `edits` (path -> new text) and whose
// only parent is `parent`.
function synthesize({ fromTree, parent, parents = [parent], edits, message }) {
  const index = join(scratch, `index-${Math.random().toString(16).slice(2)}`);
  const env = { ...baseEnv, GIT_INDEX_FILE: index };
  git(["read-tree", fromTree], { cwd: store, env });
  for (const [path, text] of Object.entries(edits)) {
    const blob = git(["hash-object", "-w", "--stdin"], { cwd: store, input: text });
    git(["update-index", "--add", "--cacheinfo", `100644,${blob},${path}`], { cwd: store, env });
  }
  const tree = git(["write-tree"], { cwd: store, env });
  return git(["commit-tree", tree, ...parents.flatMap((one) => ["-p", one]), "-m", message], { cwd: store });
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
  // --- Only a forward patch release is a projection. ---
  assert.ok(isForwardPatchBump("0.9.0", "0.9.1"));
  assert.ok(isForwardPatchBump("0.9.9", "0.9.10"));
  for (const [from, to] of [["0.9.1", "0.10.0"], ["0.9.1", "1.0.0"], ["0.9.1", "0.9.3"], ["0.9.1", "0.9.0"], ["0.9.1", "0.9.2-rc.1"], ["0.9.1-rc.1", "0.9.2"], ["0.9.01", "0.9.2"]]) {
    assert.ok(!isForwardPatchBump(from, to), `${from} -> ${to} is not a forward patch release`);
  }

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
    "packages", "closure", "shards", "unit_feature_shards", "main_unit_shards",
    "main_feature_unit_shards", "main_archive_builds", "main_archive_runs", "feature_check_jobs",
  ]) {
    assert.deepEqual(projectionPlan[field], [], `a release projection plans no ${field}`);
  }
  // The readers of the changed files still run, exactly as any plan runs
  // them: their hook-parity rows and the integration suites the paths select.
  const changedPaths = git(["diff", "--name-only", "--no-renames", BUMP_PARENT, BUMP]).split("\n");
  const readers = readerPackagesFor(changedPaths, loadNonRustReaders());
  assert.ok(readers.includes("xtask"), "the CHANGELOG's retired-token reader (xtask) is in scope");
  assert.deepEqual(projectionPlan.reader_packages, readers, "the projection keeps every reader of its changed files");
  assert.deepEqual(projectionPlan.test_scope, readers, "the test scope is the readers alone");
  assert.ok(projectionPlan.closure_test_rows.length > 0, "the readers' hook-parity rows are planned");
  const rowBinaries = projectionPlan.closure_test_rows.flatMap((row) => row.binaries);
  assert.ok(rowBinaries.includes("xtask::buildbuddy_static_lanes"), "the retired-surface-tokens test that reads CHANGELOG.md runs");
  assert.deepEqual(projectionPlan.integration_suites.map((suite) => suite.name), ["xtask"], "the sdks/ paths select the xtask suite");
  assert.deepEqual(projectionPlan.hook_coverage.missing, [], "the readers' hook binaries are all covered");
  // Their lib and bin unit tests too (the hook reuses the parent's evidence
  // for a projection, so nothing else runs them), and meerkat-contracts'.
  assert.deepEqual(projectionPlan.unit_packages, [...new Set([...readers, "meerkat-contracts"])].sort());
  assert.deepEqual(
    projectionPlan.unit_shards.flatMap((shard) => shard.packages).sort(),
    projectionPlan.unit_packages,
    "every reader package and meerkat-contracts has a unit row",
  );
  assert.ok(projectionPlan.unit_packages.includes("rkat"), "rkat's unit row runs its bin's fenced docs example test");
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
  const github = spawnSync("node", [planner, "--format", "github", "--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP_PARENT}:${RUN_ID}:2`], {
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
    "release_evidence_run_attempt=2",
    `closure_test_count=${projectionPlan.closure_test_rows.length}`,
    `unit_shard_count=${projectionPlan.unit_shards.length}`,
    "integration_count=1",
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
    ["malformed evidence", ["--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP_PARENT}:latest:1`], /is not <commit>:<run id>:<attempt>/],
    ["evidence of a run still in progress", ["--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP_PARENT}:${RUN_ID}:pending`], /is not <commit>:<run id>:<attempt>/],
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

  // --- CI-Plan: workspace gives a release's parent workspace-mode evidence. ---
  const docsPath = "docs/guides/cd-and-distribution.md";
  const docsOnly = (message) => synthesize({
    fromTree: `${BUMP_PARENT}^{tree}`,
    parent: BUMP_PARENT,
    edits: { [docsPath]: `${git(["show", `${BUMP_PARENT}:${docsPath}`])}\nA docs-only edit.\n` },
    message,
  });
  const planOf = (head, extra = []) => {
    const result = plan(["--base", BUMP_PARENT, "--head", head, ...extra]);
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  assert.equal(planOf(docsOnly("docs: an ordinary edit")).mode, "none", "a docs-only commit plans no Rust lanes");
  const requested = planOf(docsOnly("docs: release preparation\n\nCI-Plan: workspace\n"));
  assert.equal(requested.mode, "workspace", "CI-Plan: workspace plans the whole workspace");
  assert.match(requested.reason, /requests a workspace plan \(CI-Plan: workspace\)/);
  assert.equal(planOf(docsOnly("docs: mention CI-Plan: workspace in prose")).mode, "none", "the trailer counts only on its own line");
  const branchHead = docsOnly("release: last commit of the release branch\n\nCI-Plan: workspace\n");
  const releaseMerge = synthesize({
    fromTree: `${branchHead}^{tree}`,
    parents: [BUMP_PARENT, branchHead],
    edits: {},
    message: "Merge pull request #1 from example/rel/0.9.1",
  });
  assert.equal(planOf(releaseMerge).mode, "workspace", "a merge of a branch whose head carries the trailer is planned as the workspace");
  const probe = (head) => spawnSync("node", [planner, "--workspace-plan-requested", "--head", head], { cwd: root, env: withScratch, encoding: "utf8" }).status;
  assert.equal(probe(releaseMerge), 0, "the classification's probe sees the trailer on a merged branch head");
  assert.equal(probe(BUMP), 1, "and nothing on a commit without it");
  const requestedBump = synthesize({
    fromTree: `${BUMP}^{tree}`,
    parent: BUMP_PARENT,
    edits: {},
    message: "chore: release v0.9.1\n\nCI-Plan: workspace\n",
  });
  const noReuse = planOf(requestedBump, ["--reuse-release-evidence", evidence]);
  assert.equal(noReuse.mode, "workspace", "an explicit workspace request outranks reuse");
  assert.equal(noReuse.release_projection, null);

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
    component_results: Object.fromEntries(WORKSPACE_LANES.map((lane) => [lane, "success"])),
  };
  const expected = { repository: REPOSITORY, commit: BUMP_PARENT, tree: BUMP_PARENT_TREE, runId: RUN_ID, runAttempt: 1 };
  assert.deepEqual(attestationProblems(goodAttestation, expected), []);
  // A push runs its unit rows in the main unit lane: its unit lane is
  // skipped, and the plan says how many rows ran on main.
  const unitOnMain = {
    ...goodAttestation,
    plan: { ...goodAttestation.plan, unit_rows_on_main_count: 19 },
    component_results: { ...goodAttestation.component_results, unit: "skipped" },
  };
  assert.deepEqual(attestationProblems(unitOnMain, expected), [], "a push whose unit rows ran on main is evidence");
  for (const [label, change] of [
    ["another tree", { tree_sha: "0".repeat(40) }],
    ["another commit", { commit_sha: BUMP }],
    ["another attempt", { workflow_run_attempt: "2" }],
    ["a pull request", { event: "pull_request" }],
    ["a release branch", { ref: "refs/heads/release/0.9" }],
    ["schema 3", { schema_version: 3 }],
    ["a failed lane", { component_results: { ...goodAttestation.component_results, clippy: "failure" } }],
    ["chained reuse", { plan: { mode: "release-projection" } }],
    // A docs-only main push after a Rust change whose own run was cancelled:
    // its plan tested only its own diff, so the earlier change is untested.
    ["a mode-none run with every Rust lane skipped", {
      plan: { mode: "none" },
      component_results: {
        ...goodAttestation.component_results,
        ...Object.fromEntries(["clippy", "unit", "integration", "governed_jsonl", "mcp_confinement_positive", "main_unit", "closure_check", "wasm_check", "sdk_host"].map((lane) => [lane, "skipped"])),
      },
    }],
    ["a packages-mode run", { plan: { mode: "packages" } }],
    ["a workspace run with one lane skipped", { component_results: { ...goodAttestation.component_results, sdk_host: "skipped" } }],
    ["a skipped unit lane whose rows ran nowhere", { component_results: { ...goodAttestation.component_results, unit: "skipped" } }],
    ["a skipped unit lane with no unit rows on main", {
      plan: { ...goodAttestation.plan, unit_rows_on_main_count: 0 },
      component_results: { ...goodAttestation.component_results, unit: "skipped" },
    }],
    ["unit rows on main whose main unit lane failed", {
      plan: { ...goodAttestation.plan, unit_rows_on_main_count: 19 },
      component_results: { ...goodAttestation.component_results, unit: "skipped", main_unit: "failure" },
    }],
    ["a workspace run without a lane", { component_results: Object.fromEntries(WORKSPACE_LANES.filter((lane) => lane !== "closure_tests").map((lane) => [lane, "success"])) }],
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
  assert.deepEqual(parseEvidenceArgument(`${BUMP_PARENT}:12:3`), { commit: BUMP_PARENT, runId: "12", runAttempt: "3" });
  assert.throws(() => parseEvidenceArgument(`${BUMP_PARENT}:12:pending`), /is not <commit>/);
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
  const attemptPath = `${runPath}/attempts/1`;
  const okFixture = { api: { [listPath]: { workflow_runs: [goodRun] }, [runPath]: goodRun, [attemptPath]: goodRun }, artifacts: { [artifactKey]: goodAttestation } };
  const withApi = (api, artifacts = okFixture.artifacts) => ({ api: { ...okFixture.api, ...api }, artifacts });

  let result = evidenceRun("find", bumpArgs, okFixture);
  assert.equal(result.status, 0, `find accepts a verified successful parent run: ${result.stderr}`);
  assert.equal(result.stdout.trim(), `${BUMP_PARENT}:${RUN_ID}:1`);

  const running = { ...goodRun, status: "in_progress", conclusion: null };
  result = evidenceRun("find", bumpArgs, { ...okFixture, api: { [listPath]: { workflow_runs: [running] } } });
  assert.equal(result.status, 1, "a running parent run is not evidence");
  assert.equal(result.stdout, "", "a running parent run prints no evidence");
  result = evidenceRun("find", [...bumpArgs, "--wait-seconds", "30"], { ...okFixture, api: { [listPath]: [{ workflow_runs: [running] }, { workflow_runs: [goodRun] }] } });
  assert.equal(result.status, 0, `find waits for the parent run to complete: ${result.stderr}`);
  assert.equal(result.stdout.trim(), `${BUMP_PARENT}:${RUN_ID}:1`);
  assert.match(result.stderr, /waiting for CI run/);
  result = evidenceRun("find", [...bumpArgs, "--wait-seconds", "30"], { ...okFixture, api: { [listPath]: [{ workflow_runs: [running] }, { workflow_runs: [{ ...goodRun, conclusion: "cancelled" }] }] } });
  assert.equal(result.status, 1, "a parent run that ends cancelled leaves the ordinary plan");

  for (const [label, fixture] of [
    ["no run", { api: { [listPath]: { workflow_runs: [] } }, artifacts: {} }],
    ["a failed run", { ...okFixture, api: { [listPath]: { workflow_runs: [{ ...goodRun, conclusion: "failure" }] } } }],
    ["a cancelled run", { ...okFixture, api: { [listPath]: { workflow_runs: [{ ...goodRun, conclusion: "cancelled" }] } } }],
    ["a missing attestation", { ...okFixture, artifacts: {} }],
    ["a reused attestation", { ...okFixture, artifacts: { [artifactKey]: { ...goodAttestation, plan: { mode: "release-projection" } } } }],
    ["a mode-none attestation", { ...okFixture, artifacts: { [artifactKey]: { ...goodAttestation, plan: { mode: "none" } } } }],
    ["an attestation of another tree", { ...okFixture, artifacts: { [artifactKey]: { ...goodAttestation, tree_sha: "1".repeat(40) } } }],
    ["a lookup failure", { api: {}, artifacts: {} }],
  ]) {
    result = evidenceRun("find", bumpArgs, fixture);
    assert.equal(result.status, 1, `find: ${label} is no evidence (stdout ${result.stdout}, stderr ${result.stderr})`);
    assert.equal(result.stdout, "", `find: ${label} prints no evidence`);
  }
  // The newest run decides: a newer cancelled or failed run is not passed
  // over for an older success.
  const newer = { ...goodRun, id: RUN_ID + 1 };
  for (const conclusion of ["cancelled", "failure"]) {
    result = evidenceRun("find", bumpArgs, withApi({ [listPath]: { workflow_runs: [goodRun, { ...newer, conclusion }] } }));
    assert.equal(result.status, 1, `find: a newer ${conclusion} run withdraws an older success`);
  }
  result = evidenceRun("find", bumpArgs, withApi(
    { [listPath]: { workflow_runs: [{ ...goodRun, conclusion: "failure" }, newer] } },
    { [artifactKey]: goodAttestation, [`${RUN_ID + 1}/meerkat-ci-attestation-${BUMP_PARENT}-1`]: { ...goodAttestation, workflow_run_id: String(RUN_ID + 1) } },
  ));
  assert.equal(result.stdout.trim(), `${BUMP_PARENT}:${RUN_ID + 1}:1`, `find takes the newest successful run: ${result.stderr}`);

  // A test, snapshot, fixture, SDK or Rust code line that pins the old or
  // new version (unchanged by the bump, so it ran on the parent with the old
  // version compiled in) sends the bump to the ordinary plan.
  const pinPath = "crates/meerkat-core/tests/release_version_pin.rs";
  const pinned = `#[test]\nfn pinned() { assert_eq!(env!("CARGO_PKG_VERSION"), "0.9.0"); }\n`;
  const pinnedParent = synthesize({ fromTree: `${BUMP_PARENT}^{tree}`, parent: BUMP_PARENT, edits: { [pinPath]: pinned }, message: "test: pin the version" });
  const pinnedBump = synthesize({ fromTree: `${BUMP}^{tree}`, parent: pinnedParent, edits: { [pinPath]: pinned }, message: "chore: release v0.9.1" });
  assert.equal(
    spawnSync(resolve(here, "release-projection-only.mjs"), ["--base", pinnedParent, "--head", pinnedBump], { cwd: root, env: withScratch }).status,
    0,
    "the pinned bump is still a projection",
  );
  const pinnedListPath = `repos/${REPOSITORY}/actions/workflows/ci.yml/runs?branch=main&event=push&head_sha=${pinnedParent}&per_page=100`;
  // Everything but the pin is reusable evidence, so the pin alone refuses it.
  const pinnedFixture = withApi(
    { [pinnedListPath]: { workflow_runs: [{ ...goodRun, head_sha: pinnedParent }] } },
    {
      [`${RUN_ID}/meerkat-ci-attestation-${pinnedParent}-1`]: {
        ...goodAttestation,
        commit_sha: pinnedParent,
        tree_sha: git(["rev-parse", `${pinnedParent}^{tree}`], { env: withScratch }),
      },
    },
  );
  result = evidenceRun("find", ["--base", pinnedParent, "--head", pinnedBump], pinnedFixture);
  assert.equal(result.status, 1, "find refuses reuse when a test pins the version");
  assert.match(result.stderr, /pin 0\.9\.0 or 0\.9\.1 \(first: crates\/meerkat-core\/tests\/release_version_pin\.rs:2\)/);
  const pinnedPlan = plan(["--base", pinnedParent, "--head", pinnedBump, "--reuse-release-evidence", `${pinnedParent}:${RUN_ID}:1`]);
  assert.notEqual(pinnedPlan.status, 0, "the planner refuses evidence for a pinned bump");
  const pinsCli = spawnSync(resolve(here, "release-version-pins.mjs"), ["--from", "0.9.0", "--to", "0.9.1", "--revision", pinnedBump], { cwd: root, env: withScratch, encoding: "utf8" });
  assert.equal(pinsCli.status, 1, "the pin ratchet reports the pinned test");
  assert.equal(spawnSync(resolve(here, "release-version-pins.mjs"), ["--from", "0.9.0", "--to", "0.9.1", "--revision", BUMP], { cwd: root }).status, 0, "the real v0.9.1 bump pins nothing");

  rmSync(join(scratch, "gh-calls.log"), { force: true });
  result = evidenceRun("find", ["--base", BUMP_PARENT, "--head", sourceEdit], okFixture);
  assert.equal(result.status, 1, "find refuses the bump plus one source edit");
  assert.match(result.stderr, /is not a release projection/);
  assert.ok(!existsSync(join(scratch, "gh-calls.log")), "a non-projection never reaches the API");

  // verify checks the named attempt, the run's latest attempt and the attestation.
  const outputFile = join(scratch, "github-output");
  writeFileSync(outputFile, "");
  result = evidenceRun("verify", [...bumpArgs, "--run-id", String(RUN_ID), "--run-attempt", "1", "--github-output", outputFile], {
    ...okFixture,
    api: { ...okFixture.api },
  });
  assert.equal(result.status, 0, `verify accepts the completed parent attempt: ${result.stderr}`);
  const outputs = Object.fromEntries(readFileSync(outputFile, "utf8").trim().split("\n").map((line) => line.split(/=(.*)/s).slice(0, 2)));
  assert.deepEqual(
    { run_id: outputs.run_id, run_attempt: outputs.run_attempt, commit: outputs.commit, tree: outputs.tree, attestation_artifact: outputs.attestation_artifact, from_version: outputs.from_version, to_version: outputs.to_version },
    { run_id: String(RUN_ID), run_attempt: "1", commit: BUMP_PARENT, tree: BUMP_PARENT_TREE, attestation_artifact: `meerkat-ci-attestation-${BUMP_PARENT}-1`, from_version: "0.9.0", to_version: "0.9.1" },
  );
  const attemptArgs = ["--run-id", String(RUN_ID), "--run-attempt", "1"];
  for (const [label, args, fixture] of [
    ["a cancelled attempt", attemptArgs, withApi({ [attemptPath]: { ...goodRun, conclusion: "cancelled" } })],
    ["an attempt still running", attemptArgs, withApi({ [attemptPath]: running })],
    ["another commit's run", attemptArgs, withApi({ [attemptPath]: { ...goodRun, head_sha: BUMP } })],
    ["a named attempt that failed", attemptArgs, withApi({ [attemptPath]: { ...goodRun, conclusion: "failure" } })],
    ["another attempt's record", attemptArgs, withApi({ [attemptPath]: { ...goodRun, run_attempt: 2 } })],
    ["a later re-run that failed", attemptArgs, withApi({ [runPath]: { ...goodRun, run_attempt: 2, conclusion: "failure" } })],
    ["a later re-run still running", attemptArgs, withApi({ [runPath]: { ...running, run_attempt: 2 } })],
    ["a later successful re-run (another attempt)", attemptArgs, withApi({ [runPath]: { ...goodRun, run_attempt: 2 } })],
    ["an attestation of another tree", attemptArgs, withApi({}, { [artifactKey]: { ...goodAttestation, tree_sha: "1".repeat(40) } })],
    ["a mode-none attestation", attemptArgs, withApi({}, { [artifactKey]: { ...goodAttestation, plan: { mode: "none" } } })],
    ["no attempt", ["--run-id", String(RUN_ID)], okFixture],
  ]) {
    result = evidenceRun("verify", [...bumpArgs, ...args], fixture);
    assert.notEqual(result.status, 0, `verify: ${label} fails closed (stderr ${result.stderr})`);
  }
  result = evidenceRun("verify", ["--base", BUMP_PARENT, "--head", sourceEdit, ...attemptArgs], okFixture);
  assert.equal(result.status, 1, "verify refuses the bump plus one source edit");

  // --- The CI gate's own shell, read from ci.yml. ---
  const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  const gateCheck = extractStepRun(workflow, "Check lane results");
  const gateEnv = {
    CHANGES_RESULT: "success", FMT_GOVERNANCE_RESULT: "success", RATCHETS_RESULT: "success", STAGGER_RESULT: "success",
    TLC_AUDITS_RESULT: "skipped", MACHINE_AUTHORITY: "false", CLIPPY_RESULT: "skipped",
    GOVERNANCE_GATES_RESULT: "skipped", GOVERNANCE_GATES: "false",
    UNIT_RESULT: "success", INTEGRATION_RESULT: "skipped", GOVERNED_JSONL_RESULT: "skipped",
    MCP_CONFINEMENT_POSITIVE_RESULT: "skipped", INTEGRATION_COUNT: "0", CLOSURE_TESTS_RESULT: "skipped",
    CLOSURE_TEST_COUNT: "0", TEST_SCOPE_COUNT: "0", HOOK_BINARY_COUNT: "0", HOOK_RETAINED_COUNT: "0",
    MAIN_UNIT_PACKED_RESULT: "skipped", MAIN_UNIT_ARCHIVE_BUILD_RESULT: "skipped", MAIN_UNIT_ARCHIVE_RUN_RESULT: "skipped",
    MAIN_ARCHIVE_BUILD_COUNT: "0", MAIN_ARCHIVE_RUN_COUNT: "0",
    CLOSURE_CHECK_RESULT: "skipped", FEATURE_CHECK_RESULT: "skipped", FEATURE_CHECK_COUNT: "0",
    WASM: "false", WASM_CHECK_RESULT: "skipped", SDK_HOST_RESULT: "skipped", EXAMPLE_WEB_RESULT: "skipped",
    WASM_TIMERS_RESULT: "skipped", BAZEL_GRAPH_RESULT: "success", RUST_CHANGED: "false", IS_MAIN_PUSH: "true",
    IS_PULL_REQUEST: "false", PLAN_EVENT: "push", CLOSURE_CHECK: "false", GOVERNED_JSONL: "false",
    MCP_CONFINEMENT_POSITIVE: "false", DEFERRED_TO_PUSH: "", READER_NOT_RUN: "",
    SDK_HOST: "false", BAZEL_GRAPH: "true", EXAMPLES_BROWSER: "false", EXAMPLE_WEB: "false",
    MODE: "release-projection", REASON: "release projection", SHARD_COUNT: "0", UNIT_SHARD_COUNT: "6",
    UNIT_DEFERRED: "", UNIT_DEFERRED_COUNT: "0", PACKAGE_COUNT: "0", CLOSURE_COUNT: "0",
    GENERATED_CONTRACT: "true", RELEASE_PROJECTION: "true",
    RELEASE_EVIDENCE_COMMIT: BUMP_PARENT, RELEASE_EVIDENCE_RUN_ID: String(RUN_ID), RELEASE_EVIDENCE_RUN_ATTEMPT: "1",
  };
  // The gate reads each two-wave lane (long rows first) as two jobs and their
  // row counts. The cases below name a lane's result and count once: all its
  // rows in the first wave, none in the rest.
  const waveEnv = (env) => {
    const out = { ...env };
    for (const [lane, prefix, count] of [
      ["CLIPPY", "SHARD", "SHARD_COUNT"],
      ["CLOSURE_TESTS", "CLOSURE_TEST", "CLOSURE_TEST_COUNT"],
      ["FEATURE_CHECK", "FEATURE_CHECK", "FEATURE_CHECK_COUNT"],
      ["MAIN_UNIT", "MAIN_UNIT", null],
    ]) {
      const key = lane === "MAIN_UNIT" ? "MAIN_UNIT_PACKED_RESULT" : `${lane}_RESULT`;
      if (!(key in out)) continue;
      const rows = count ? out[count] : out[key] === "skipped" ? "0" : "1";
      out[`${lane}_FIRST_RESULT`] = out[key];
      out[`${lane}_REST_RESULT`] = "skipped";
      out[`${prefix}_FIRST_COUNT`] = rows;
      out[`${prefix}_REST_COUNT`] = "0";
      delete out[key];
    }
    return out;
  };
  const gate = (change) => spawnSync("bash", ["-c", gateCheck], { env: { ...process.env, ...waveEnv({ ...gateEnv, ...change }) }, encoding: "utf8" });
  assert.equal(gate({}).status, 0, `the gate accepts a release-projection main push: ${gate({}).stdout}`);
  assert.equal(gate({ IS_MAIN_PUSH: "false", IS_PULL_REQUEST: "true" }).status, 0, "and a release-projection pull request");
  assert.equal(
    gate({ CLOSURE_TEST_COUNT: "13", CLOSURE_TESTS_RESULT: "success", INTEGRATION_COUNT: "1", INTEGRATION_RESULT: "success" }).status,
    0,
    "a release projection whose reader rows ran",
  );
  for (const [label, change] of [
    ["a failed ratchet", { RATCHETS_RESULT: "failure" }],
    ["a failed stagger job", { STAGGER_RESULT: "failure" }],
    ["a failed governance gate", { GOVERNANCE_GATES_RESULT: "failure" }],
    ["a governance change whose gates did not run", { GOVERNANCE_GATES: "true" }],
    ["a failed Bazel graph check", { BAZEL_GRAPH_RESULT: "failure" }],
    ["a projection plan with Rust lanes", { RUST_CHANGED: "true" }],
    ["a projection plan whose reader rows did not run", { CLOSURE_TEST_COUNT: "2" }],
    ["a projection plan whose integration suite failed", { INTEGRATION_COUNT: "1", INTEGRATION_RESULT: "failure" }],
    ["a projection plan without a completed attempt", { RELEASE_EVIDENCE_RUN_ATTEMPT: "pending" }],
    ["a projection plan without the ratchets", { GENERATED_CONTRACT: "false" }],
    ["a projection plan without evidence", { RELEASE_EVIDENCE_RUN_ID: "" }],
    ["the projection mode without its flag", { RELEASE_PROJECTION: "false" }],
    ["an ordinary main push without the example web suites", { RELEASE_PROJECTION: "false", MODE: "none", RELEASE_EVIDENCE_COMMIT: "", RELEASE_EVIDENCE_RUN_ID: "", UNIT_SHARD_COUNT: "0", UNIT_RESULT: "skipped" }],
    ["a projection whose unit rows did not run", { UNIT_RESULT: "skipped" }],
    ["a projection whose unit rows failed", { UNIT_RESULT: "failure" }],
    ["a projection plan without unit rows", { UNIT_SHARD_COUNT: "0", UNIT_RESULT: "skipped" }],
  ]) {
    assert.equal(gate(change).status, 1, `the gate refuses ${label}`);
  }
  // An ordinary plan: a pull request passes without the lanes it left to
  // the push and names them; the push must run them, and the gate refuses a
  // push plan that left any.
  const rustGate = {
    ...gateEnv, MODE: "packages", REASON: "1 workspace package(s) own changed paths", RELEASE_PROJECTION: "false",
    RELEASE_EVIDENCE_COMMIT: "", RELEASE_EVIDENCE_RUN_ID: "", RELEASE_EVIDENCE_RUN_ATTEMPT: "", RUST_CHANGED: "true",
    SHARD_COUNT: "1", CLIPPY_RESULT: "success", UNIT_SHARD_COUNT: "1", UNIT_RESULT: "success", PACKAGE_COUNT: "1",
    GENERATED_CONTRACT: "false", BAZEL_GRAPH: "false", BAZEL_GRAPH_RESULT: "skipped",
  };
  const pullRequestGate = {
    ...rustGate, IS_MAIN_PUSH: "false", IS_PULL_REQUEST: "true", PLAN_EVENT: "pull_request",
    DEFERRED_TO_PUSH: "17 closure-tests rows; the closure check; governed JSONL; MCP confinement positive",
  };
  const pushGate = {
    ...rustGate, CLOSURE_CHECK: "true", GOVERNED_JSONL: "true", MCP_CONFINEMENT_POSITIVE: "true",
    CLOSURE_CHECK_RESULT: "success", GOVERNED_JSONL_RESULT: "success", MCP_CONFINEMENT_POSITIVE_RESULT: "success",
    CLOSURE_TEST_COUNT: "17", CLOSURE_TESTS_RESULT: "success", MAIN_UNIT_PACKED_RESULT: "success",
    WASM_CHECK_RESULT: "success", SDK_HOST_RESULT: "success", EXAMPLE_WEB_RESULT: "success",
    WASM_TIMERS_RESULT: "success", BAZEL_GRAPH_RESULT: "success",
  };
  const runGate = (env) => spawnSync("bash", ["-c", gateCheck], { env: { ...process.env, ...waveEnv(env) }, encoding: "utf8" });
  const pullRequestRun = runGate(pullRequestGate);
  assert.equal(pullRequestRun.status, 0, `the gate accepts a pull request without the lanes it left to the push: ${pullRequestRun.stdout}`);
  assert.match(pullRequestRun.stdout, /left to the push \(main or release\/\*\*\): 17 closure-tests rows/);
  // The readers no CI lane runs (live-provider credentials) are named, and
  // pass.
  const notRunRun = runGate({ ...pullRequestGate, READER_NOT_RUN: "integration-tests:test:gpt_live_public_e2e" });
  assert.equal(notRunRun.status, 0, notRunRun.stdout);
  assert.match(notRunRun.stdout, /::notice::readers of changed files that need live-provider credentials run in no CI lane: integration-tests:test:gpt_live_public_e2e/);
  assert.equal(runGate(pushGate).status, 0, `the gate accepts a push that ran them: ${runGate(pushGate).stdout}`);
  // A push runs its unit rows in the packed main unit lane, not in a unit
  // lane of its own: the gate requires that lane instead.
  const unitOnMainGate = {
    ...pushGate, UNIT_SHARD_COUNT: "0", UNIT_RESULT: "skipped", UNIT_ROWS_ON_MAIN: "core", UNIT_ROWS_ON_MAIN_COUNT: "1",
  };
  const unitOnMainRun = runGate(unitOnMainGate);
  assert.equal(unitOnMainRun.status, 0, `the gate accepts a push whose unit rows ran on main: ${unitOnMainRun.stdout}`);
  assert.match(unitOnMainRun.stdout, /the push runs its unit rows in the main unit lane: core/);
  for (const [label, env] of [
    ["a push plan without the closure check", { ...pushGate, CLOSURE_CHECK: "false", CLOSURE_CHECK_RESULT: "skipped" }],
    ["a push plan without governed JSONL", { ...pushGate, GOVERNED_JSONL: "false", GOVERNED_JSONL_RESULT: "skipped" }],
    ["a push plan without the confinement lane", { ...pushGate, MCP_CONFINEMENT_POSITIVE: "false", MCP_CONFINEMENT_POSITIVE_RESULT: "skipped" }],
    ["a push plan that left lanes to a later push", { ...pushGate, DEFERRED_TO_PUSH: "the closure check" }],
    ["a pull-request plan on a push", { ...pushGate, PLAN_EVENT: "pull_request" }],
    ["a push whose closure check did not run", { ...pushGate, CLOSURE_CHECK_RESULT: "skipped" }],
    ["a pull request whose skipped closure check failed", { ...pullRequestGate, CLOSURE_CHECK_RESULT: "failure" }],
    ["a pull request whose governed JSONL lane failed", { ...pullRequestGate, GOVERNED_JSONL_RESULT: "failure" }],
    ["unit rows on main whose packed main unit lane did not run", { ...unitOnMainGate, MAIN_UNIT_PACKED_RESULT: "skipped" }],
    ["unit rows on main whose packed main unit lane failed", { ...unitOnMainGate, MAIN_UNIT_PACKED_RESULT: "failure" }],
    ["a push with neither unit rows nor a deferral", { ...unitOnMainGate, UNIT_ROWS_ON_MAIN: "", UNIT_ROWS_ON_MAIN_COUNT: "0" }],
    ["unit rows on main beside a failed unit lane", { ...unitOnMainGate, UNIT_RESULT: "failure" }],
  ]) {
    assert.equal(runGate(env).status, 1, `the gate refuses ${label}`);
  }
  // A two-wave lane ran when every wave that holds rows succeeded.
  const waved = { ...waveEnv(pushGate), SHARD_COUNT: "5", SHARD_FIRST_COUNT: "2", SHARD_REST_COUNT: "3", CLIPPY_REST_RESULT: "success" };
  const rawGate = (env) => spawnSync("bash", ["-c", gateCheck], { env: { ...process.env, ...env }, encoding: "utf8" });
  assert.equal(rawGate(waved).status, 0, `the gate accepts both waves of a lane: ${rawGate(waved).stdout}`);
  const restOnly = { ...waved, SHARD_FIRST_COUNT: "0", CLIPPY_FIRST_RESULT: "skipped" };
  assert.equal(rawGate(restOnly).status, 0, `and a lane whose rows are all in the rest wave: ${rawGate(restOnly).stdout}`);
  for (const [label, change] of [
    ["a rest wave with rows that did not run", { CLIPPY_REST_RESULT: "skipped" }],
    ["a failed rest wave", { CLIPPY_REST_RESULT: "failure" }],
    ["a cancelled first wave", { CLIPPY_FIRST_RESULT: "cancelled" }],
    ["a first wave with rows that did not run", { CLIPPY_FIRST_RESULT: "skipped" }],
    ["a lane whose wave counts are missing", { SHARD_REST_COUNT: "" }],
    ["a main unit rest wave that failed", { MAIN_UNIT_REST_COUNT: "4", MAIN_UNIT_REST_RESULT: "failure" }],
    ["a closure-tests rest wave with rows that did not run", { CLOSURE_TEST_COUNT: "20", CLOSURE_TEST_REST_COUNT: "3", CLOSURE_TESTS_REST_RESULT: "skipped" }],
  ]) {
    assert.equal(rawGate({ ...waved, ...change }).status, 1, `the gate refuses ${label}`);
  }
  // Main unit coverage is the packed waves plus the archived lanes: each that
  // has rows must succeed, and the others' success never stands in for it.
  const archived = {
    ...waved, MAIN_ARCHIVE_BUILD_COUNT: "1", MAIN_UNIT_ARCHIVE_BUILD_RESULT: "success",
    MAIN_ARCHIVE_RUN_COUNT: "2", MAIN_UNIT_ARCHIVE_RUN_RESULT: "success",
  };
  assert.equal(rawGate(archived).status, 0, `the gate accepts the packed and archived main unit lanes: ${rawGate(archived).stdout}`);
  for (const [label, change] of [
    ["a packed main unit wave with rows that did not run, beside green archived lanes", { MAIN_UNIT_REST_COUNT: "4", MAIN_UNIT_REST_RESULT: "skipped" }],
    ["a packed main unit wave with rows and no result, beside green archived lanes", { MAIN_UNIT_FIRST_RESULT: "" }],
    ["archived lanes with rows that did not run, beside a green packed lane", { MAIN_UNIT_ARCHIVE_BUILD_RESULT: "skipped", MAIN_UNIT_ARCHIVE_RUN_RESULT: "skipped" }],
    ["an archived run with rows that did not run", { MAIN_UNIT_ARCHIVE_RUN_RESULT: "skipped" }],
    ["archived lanes without a row count", { MAIN_ARCHIVE_BUILD_COUNT: "" }],
  ]) {
    const run = rawGate({ ...archived, ...change });
    assert.equal(run.status, 1, `the gate refuses ${label}: ${run.stdout}`);
    assert.match(run.stdout, /::error::Main unit tests must run and succeed/, `main unit coverage refuses ${label}`);
  }

  // The lane execution budget, read from ci.yml and run against a fake gh.
  // A lane that waited for the stagger job (the rest wave of a two-wave lane,
  // the short lanes) counts the stagger's run time in its execution path and
  // its runner queue in its path queue, so waiting for runners alone never
  // fails the runaway ceiling.
  const budgetStep = extractStepRun(workflow, "Enforce lane execution budget");
  const budgetEnv = extractStepLiteralEnv(workflow, "Enforce lane execution budget");
  const jobName = (id) => workflow.match(new RegExp(`\\n  ${id}:\\n    name: (.+)\\n`))?.[1];
  assert.equal(budgetEnv.STAGGER_JOB_NAME, jobName("stagger"), "the budget names the stagger job");
  assert.equal(budgetEnv.CLASSIFICATION_JOB_NAME, jobName("changes"), "and the change classification");
  const workflowSeconds = (name) => {
    const value = workflow.match(new RegExp(`^  ${name}: "(\\d+)"$`, "m"))?.[1];
    assert.ok(value, `ci.yml sets ${name}`);
    return value;
  };
  const budgetBin = join(scratch, "budget-bin");
  mkdirSync(budgetBin);
  const budgetFixture = join(scratch, "budget-fixture.json");
  writeFileSync(
    join(budgetBin, "gh"),
    `#!/usr/bin/env node
const fs = require("node:fs");
const fixture = JSON.parse(fs.readFileSync(${JSON.stringify(budgetFixture)}, "utf8"));
const path = process.argv[process.argv.length - 1];
if (!(path in fixture)) { process.stderr.write("gh: Not Found (HTTP 404)\\n"); process.exit(1); }
process.stdout.write(JSON.stringify(fixture[path]));
`,
  );
  chmodSync(join(budgetBin, "gh"), 0o755);
  const at = (seconds) => new Date(Date.UTC(2026, 9, 17) + seconds * 1000).toISOString().replace(".000Z", "Z");
  const job = (name, created, started, completed) => ({
    name, created_at: at(created), started_at: at(started), completed_at: at(completed), conclusion: "success",
  });
  const runBudget = (attemptStarts, jobs) => {
    const run = `/repos/${REPOSITORY}/actions/runs/${RUN_ID}`;
    const pages = { [`${run}/attempts/${attemptStarts.length}/jobs?per_page=100`]: [{ jobs }] };
    attemptStarts.forEach((start, index) => {
      pages[`${run}/attempts/${index + 1}`] = [{ run_started_at: at(start) }];
    });
    writeFileSync(budgetFixture, JSON.stringify(pages));
    const outputFile = join(scratch, "budget-output");
    writeFileSync(outputFile, "");
    const result = spawnSync("bash", ["-c", budgetStep], {
      env: {
        ...process.env, ...budgetEnv, PATH: `${budgetBin}:${process.env.PATH}`,
        GITHUB_REPOSITORY: REPOSITORY, GITHUB_RUN_ID: String(RUN_ID), GITHUB_RUN_ATTEMPT: String(attemptStarts.length),
        MAX_EXEC_SECONDS: workflowSeconds("CI_MAX_EXEC_SECONDS"), MAX_TERMINAL_SECONDS: workflowSeconds("CI_MAX_TERMINAL_SECONDS"),
        QUEUE_WARN_SECONDS: workflowSeconds("CI_QUEUE_WARN_SECONDS"), ENFORCE_BUDGET: "true",
        GITHUB_OUTPUT: outputFile, GITHUB_STEP_SUMMARY: "",
      },
      encoding: "utf8",
    });
    const outputs = Object.fromEntries(
      readFileSync(outputFile, "utf8").split("\n").filter(Boolean).map((line) => line.split("=")),
    );
    return { ...result, outputs };
  };
  // A pull request on a busy account: the stagger job waited 700 s for a
  // runner, the long first-wave row ran 900 s, the rest-wave row 470 s.
  const stagger = job(budgetEnv.STAGGER_JOB_NAME, 36, 736, 740);
  const busy = [
    job(budgetEnv.CLASSIFICATION_JOB_NAME, 0, 5, 35),
    job("Format + governance", 0, 5, 400),
    stagger,
    job("Clippy (mob)", 36, 46, 946),
    job("Clippy (core+12)", 741, 745, 1215),
    { ...job("Unit tests (xtask)", 36, 36, 36), conclusion: "skipped" },
    { name: budgetEnv.GATE_JOB_NAME, created_at: at(1216), started_at: at(1220), completed_at: null, conclusion: null },
  ];
  let budget = runBudget([0], busy);
  assert.equal(budget.status, 0, `the stagger job's runner queue never fails a pull request: ${budget.stdout}${budget.stderr}`);
  assert.deepEqual(
    budget.outputs,
    {
      exec_path_seconds: "930", terminal_seconds: "1215", terminal_excluding_queue_seconds: "931",
      queue_seconds: "709", budget_verdict: "queue_delayed",
    },
    `the rest-wave row's path holds the stagger job's queue: ${budget.stdout}`,
  );
  assert.match(budget.stdout, /^\| Clippy \(core\+12\) \| 1 \| 4 \| 470 \| 504 \| 1215 \| 506 \| success \|$/m, "a rest-wave row waits for the stagger job");
  assert.match(budget.stdout, /^\| Clippy \(mob\) \| 1 \| 10 \| 900 \| 930 \| 946 \| 931 \| success \|$/m, "a first-wave row does not");
  assert.match(budget.stdout, /::warning title=CI runner queue wait::Clippy \(core\+12\) waited 709 s/);
  // A row re-run in a later attempt did not wait for the carried-over
  // stagger job.
  budget = runBudget([0, 2000], [...busy.slice(0, 4), job("Clippy (core+12)", 2000, 2003, 2473)]);
  assert.equal(budget.status, 0, `a re-run rest-wave row: ${budget.stdout}${budget.stderr}`);
  assert.match(budget.stdout, /^\| Clippy \(core\+12\) \| 2 \| 3 \| 470 \| 500 \| 473 \| 470 \| success \|$/m, "a re-run row is not behind the stagger job");
  // The execution overrun still fails: the stagger job's run time counts.
  budget = runBudget([0], [...busy.slice(0, 2), { ...stagger, completed_at: at(741) }, job("Clippy (core+12)", 741, 745, 1791)]);
  assert.equal(budget.status, 1, `a rest-wave row over the execution budget fails: ${budget.stdout}`);
  assert.equal(budget.outputs.exec_path_seconds, "1081", "1046 s of the row, 30 s of classification, 5 s of stagger");
  assert.equal(budget.outputs.budget_verdict, "exec_over_budget");

  // The gate verifies the evidence after the lanes and records it.
  const verifyStep = extractStepRun(workflow, "Verify reused release-projection evidence");
  assert.match(verifyStep, /git show "\$\{parent\}:scripts\/\$\{script\}"/, "the gate runs the parent's scripts");
  assert.match(verifyStep, /node "\$\{tools\}\/ci-release-evidence\.mjs" verify /);
  assert.match(verifyStep, /--run-attempt "\$\{RELEASE_EVIDENCE_RUN_ATTEMPT\}"/);
  assert.doesNotMatch(verifyStep, /--wait-seconds/, "the gate never waits: the plan reused a completed run");
  const attestationStep = extractStepRun(workflow, "Build exact-tree CI attestation");
  assert.match(attestationStep, /reused_test_evidence: \$reused/);
  assert.match(workflow, /git show "\$\{classification_base\}:scripts\/ci-release-evidence\.mjs"/, "the classification runs the parent's finder");
  assert.match(workflow, /git show "\$\{classification_base\}:scripts\/release-version-pins\.mjs"/, "with the parent's pin ratchet");
  assert.match(workflow, /&& ! node scripts\/ci-cargo-lanes\.mjs --workspace-plan-requested --head HEAD \\\n/, "CI-Plan: workspace skips the finder and its wait");
  assert.match(workflow, /for script in ci-release-evidence\.mjs release-projection-only\.mjs release-version-pins\.mjs; do/, "the gate runs the parent's three scripts");
  assert.equal((workflow.match(/python-version: "3\.14"/g) ?? []).length >= 2, true, "the classification and the gate pin Python 3.14");
  assert.match(workflow, /needs\.changes\.outputs\.release_projection == 'true'\) && needs\.changes\.outputs\.unit_shard_count != '0'/, "the unit job runs a projection's rows");
  assert.match(workflow, /node "\$\{evidence_tools\}\/ci-release-evidence\.mjs" find --repository "\$\{GITHUB_REPOSITORY\}"/);
  assert.match(workflow, /EVIDENCE_WAIT_SECONDS: \$\{\{ github\.event_name == 'push' && '2400' \|\| '0' \}\}/, "only a push waits for its parent's run");
  const release = readFileSync(resolve(root, ".github/workflows/release.yml"), "utf8");
  assert.match(release, /\.reused_test_evidence\.base_commit_sha == \$parent/, "require_ci_green binds the evidence to the release commit's first parent");
  assert.match(release, /for script in ci-release-evidence\.mjs release-projection-only\.mjs release-version-pins\.mjs; do/, "require_ci_green runs the parent's three scripts");
  assert.match(release, /node "\$\{tools\}\/ci-release-evidence\.mjs" verify --repository "\$\{GITHUB_REPOSITORY\}"/, "require_ci_green re-verifies the reused run");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// The literal `env:` entries of the named step (no expressions).
function extractStepLiteralEnv(text, name) {
  const lines = text.split("\n");
  const at = lines.findIndex((line) => line.trim() === `- name: ${name}`);
  assert.ok(at >= 0, `ci.yml has a step named ${name}`);
  const stepIndent = lines[at].indexOf("-");
  const env = {};
  let envIndent = -1;
  for (let index = at + 1; index < lines.length; index += 1) {
    const indent = lines[index].search(/\S/);
    if (indent >= 0 && indent <= stepIndent) break;
    if (envIndent < 0) {
      if (/^\s+env:\s*$/.test(lines[index])) envIndent = indent;
      continue;
    }
    if (indent >= 0 && indent <= envIndent) break;
    const entry = lines[index].match(/^\s+([A-Z_]+): (.+)$/);
    if (entry && !entry[2].includes("${{")) env[entry[1]] = entry[2];
  }
  assert.ok(Object.keys(env).length > 0, `step ${name} has literal env entries`);
  return env;
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
