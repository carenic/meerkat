#!/usr/bin/env node
// Self-test of scripts/ci-push-base.mjs, the diff base of a push to main,
// against a fake GitHub CLI (no network), and of its wiring in ci.yml.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { attestationArtifactName } from "./ci-push-base.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const finder = resolve(here, "ci-push-base.mjs");
const REPO = "owner/repo";

const sha = (label) => label.padEnd(40, "0").replace(/[^0-9a-f]/g, "0").slice(0, 40);
const HEAD = sha("1111");
const P1 = sha("2222");
const P2 = sha("3333");
const P3 = sha("4444");
const ELSEWHERE = sha("5555");
const treeOf = (commit) => sha(`e${commit.slice(0, 6)}`);
const ancestry = [HEAD, P1, P2, P3];

const scratch = mkdtempSync(join(tmpdir(), "meerkat-ci-push-base-selftest-"));
try {
  // The fake `gh`: answers the commits and runs listings from a fixture file
  // and serves an attestation artifact only when the fixture holds it.
  const fakeGh = join(scratch, "fake-gh.mjs");
  writeFileSync(
    fakeGh,
    `#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const fixture = JSON.parse(readFileSync(process.env.MEERKAT_CI_PUSH_BASE_FIXTURE, "utf8"));
const args = process.argv.slice(2);
if (args[0] === "api") {
  const path = args[1];
  if (path.startsWith("repos/${REPO}/commits?")) {
    if (fixture.commitsFail) { process.stderr.write("HTTP 502"); process.exit(1); }
    process.stdout.write(JSON.stringify(fixture.commits));
    process.exit(0);
  }
  if (path.startsWith("repos/${REPO}/actions/workflows/ci.yml/runs?")) {
    if (!path.includes("branch=main") || !path.includes("event=push") || !path.includes("status=success")) process.exit(3);
    process.stdout.write(JSON.stringify({ workflow_runs: fixture.runs }));
    process.exit(0);
  }
  process.exit(4);
}
if (args[0] === "run" && args[1] === "download") {
  const name = args[args.indexOf("--name") + 1];
  const dir = args[args.indexOf("--dir") + 1];
  const attestation = fixture.attestations[name];
  if (!attestation) { process.stderr.write("no artifact " + name); process.exit(1); }
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "attestation.json"), JSON.stringify(attestation));
  process.exit(0);
}
process.exit(5);
`,
  );
  chmodSync(fakeGh, 0o755);

  const commitsOf = (list) => list.map((commit) => ({ sha: commit, commit: { tree: { sha: treeOf(commit) } } }));
  let nextId = 1000;
  const run = (commit, overrides = {}) => ({
    id: nextId++,
    path: ".github/workflows/ci.yml",
    name: "CI",
    event: "push",
    head_branch: "main",
    head_sha: commit,
    status: "completed",
    conclusion: "success",
    run_attempt: 1,
    html_url: `https://example.invalid/runs/${commit}`,
    ...overrides,
  });
  const attestationFor = (item, overrides = {}) => ({
    [attestationArtifactName(item.head_sha, item.run_attempt)]: {
      schema_version: 4,
      repository: REPO,
      commit_sha: item.head_sha,
      tree_sha: treeOf(item.head_sha),
      workflow: "CI",
      workflow_run_id: String(item.id),
      workflow_run_attempt: String(item.run_attempt),
      event: "push",
      ref: "refs/heads/main",
      validation_backend: "github-hosted-cargo",
      validation_result: "success",
      plan: { mode: "packages" },
      ...overrides,
    },
  });
  const find = (fixture, args = ["--repository", REPO, "--head", HEAD]) => {
    const path = join(scratch, `fixture-${nextId++}.json`);
    writeFileSync(path, JSON.stringify({ commits: commitsOf(ancestry), runs: [], attestations: {}, ...fixture }));
    return spawnSync("node", [finder, ...args], {
      encoding: "utf8",
      env: { ...process.env, MEERKAT_CI_PUSH_BASE_GH: fakeGh, MEERKAT_CI_PUSH_BASE_FIXTURE: path },
    });
  };
  const expectBase = (label, fixture, base) => {
    const result = find(fixture);
    assert.equal(result.status, 0, `${label}: ${result.stderr}`);
    assert.equal(result.stdout.trim(), base, `${label}: base (${result.stderr})`);
  };
  const expectNone = (label, fixture, args) => {
    const result = find(fixture, args);
    assert.equal(result.status, 1, `${label}: exit 1, so the caller plans the whole workspace (${result.stderr})`);
    assert.equal(result.stdout, "", `${label}: no base printed`);
  };

  // The previous main commit's run is green and attested: it is the base.
  {
    const p1 = run(P1);
    expectBase("a green, attested parent", { runs: [p1], attestations: attestationFor(p1) }, P1);
  }
  // The parent's run was superseded (cancelled) or failed, so it is not in
  // the successful listing: the base reaches back to the last green one, and
  // the parent's changes are planned again.
  {
    const p2 = run(P2);
    expectBase("a superseded parent", { runs: [p2], attestations: attestationFor(p2) }, P2);
  }
  // A green parent run without its attestation, or with one that names
  // another tree, run, ref or result, does not count.
  for (const [label, overrides] of [
    ["another tree", { tree_sha: sha("ffff") }],
    ["another run", { workflow_run_id: "1" }],
    ["another ref", { ref: "refs/heads/release/0.9" }],
    ["a failed validation", { validation_result: "failure" }],
    ["another backend", { validation_backend: "buildbuddy" }],
    ["an older schema", { schema_version: 3 }],
  ]) {
    const p1 = run(P1);
    const p2 = run(P2);
    expectBase(
      `a parent attestation with ${label}`,
      { runs: [p1, p2], attestations: { ...attestationFor(p1, overrides), ...attestationFor(p2) } },
      P2,
    );
  }
  {
    const p1 = run(P1);
    const p2 = run(P2);
    expectBase("a green parent without an attestation", { runs: [p1, p2], attestations: attestationFor(p2) }, P2);
  }
  // Runs of another workflow, branch, event or state never count.
  for (const [label, overrides] of [
    ["another workflow", { path: ".github/workflows/cargo.yml" }],
    ["another workflow name", { name: "Cargo" }],
    ["a release branch", { head_branch: "release/0.9" }],
    ["a pull request", { event: "pull_request" }],
    ["an unfinished run", { status: "in_progress", conclusion: null }],
    ["a cancelled run", { conclusion: "cancelled" }],
  ]) {
    const p1 = run(P1, overrides);
    const p2 = run(P2);
    expectBase(`a parent run of ${label}`, { runs: [p1, p2], attestations: { ...attestationFor(p1), ...attestationFor(p2) } }, P2);
  }
  // A re-run's attempt names its attestation; an older run of the same
  // commit still counts when the newest one does not qualify.
  {
    const p1 = run(P1, { run_attempt: 2 });
    expectBase("a re-run attempt", { runs: [p1], attestations: attestationFor(p1) }, P1);
    const older = run(P1);
    const newer = run(P1);
    expectBase("an older run of the parent", { runs: [older, newer], attestations: attestationFor(older) }, P1);
  }
  // A release projection's run (reused evidence) is green and attested too.
  {
    const p1 = run(P1);
    expectBase(
      "a release-projection parent",
      { runs: [p1], attestations: attestationFor(p1, { plan: { mode: "release-projection" }, reused_test_evidence: { base_commit_sha: P2 } }) },
      P1,
    );
  }
  // A green run of a commit outside HEAD's history never counts, and HEAD is
  // never its own base.
  {
    const elsewhere = run(ELSEWHERE);
    const head = run(HEAD);
    const p3 = run(P3);
    expectBase(
      "runs outside the ancestry and of HEAD itself",
      { runs: [elsewhere, head, p3], attestations: { ...attestationFor(elsewhere), ...attestationFor(head), ...attestationFor(p3) } },
      P3,
    );
  }
  // Nothing qualifies, or a lookup fails: exit 1 and print no base.
  {
    expectNone("no green run", {});
    const head = run(HEAD);
    expectNone("only HEAD's own run", { runs: [head], attestations: attestationFor(head) });
    const p1 = run(P1);
    expectNone("a failed commits lookup", { commitsFail: true, runs: [p1], attestations: attestationFor(p1) });
    expectNone("a commits listing that does not start at HEAD", { commits: commitsOf([P1, P2]), runs: [p1], attestations: attestationFor(p1) });
  }
  // Usage errors exit 2.
  for (const args of [
    ["--head", HEAD],
    ["--repository", REPO],
    ["--repository", REPO, "--head", "HEAD"],
    ["--repository", REPO, "--head", HEAD, "--depth", "1"],
    ["--repository", REPO, "--head", HEAD, "--bogus"],
  ]) {
    assert.equal(find({}, args).status, 2, `usage error: ${args.join(" ")}`);
  }

  // ci.yml: a push to main plans against the finder's base, run from HEAD's
  // first parent, and plans the whole workspace without one; a release
  // projection keeps its verified first parent.
  const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  assert.match(workflow, /git show "HEAD\^1:scripts\/ci-push-base\.mjs" > "\$\{push_base_tool\}"/, "the finder runs from HEAD's first parent");
  assert.match(
    workflow,
    /found_base="\$\(node "\$\{push_base_tool\}" --repository "\$\{GITHUB_REPOSITORY\}" --head "\$\(git rev-parse HEAD\)"\)"/,
    "the finder gets HEAD's full sha",
  );
  assert.match(
    workflow,
    /if \[\[ "\$\{#plan_args\[@\]\}" -eq 0 && "\$\{EVENT_NAME\}" == "push" && "\$\{GITHUB_REF\}" == "refs\/heads\/main" \]\]; then\n\s+plan_base=""/,
    "a main push without release evidence starts from no base, which plans the whole workspace",
  );
  assert.match(workflow, /--base "\$\{plan_base\}" --head HEAD "\$\{plan_args\[@\]\}"/, "the classifier diffs against the plan base");
  assert.doesNotMatch(workflow, /--base "\$\{classification_base\}" --head HEAD "\$\{plan_args\[@\]\}"/, "never against the payload base");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

console.log("ci-push-base contracts hold");
