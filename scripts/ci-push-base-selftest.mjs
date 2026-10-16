#!/usr/bin/env node
// Self-test of scripts/ci-push-base.mjs, the diff base of a push to main,
// and of scripts/ci-push-plan-base.mjs, which decides whether ci.yml may use
// the finder's answer, against a fake GitHub CLI (no network) and scratch
// Git histories, and of their wiring in ci.yml.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { attestationArtifactName } from "./ci-push-base.mjs";
import { FINDER_PATH } from "./ci-push-plan-base.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const finder = resolve(here, "ci-push-base.mjs");
const wrapper = resolve(here, "ci-push-plan-base.mjs");
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
  if (path.startsWith("repos/${REPO}/compare/")) {
    const answer = (fixture.compare ?? {})[path.slice("repos/${REPO}/compare/".length)];
    if (!answer) { process.stderr.write("HTTP 404"); process.exit(1); }
    process.stdout.write(JSON.stringify(answer));
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

  // ci-push-plan-base.mjs: the finder runs from HEAD's first parent only
  // when that parent is the push's payload base, and its answer counts only
  // as one full sha of an ancestor of HEAD other than HEAD. Every refusal
  // exits 1, so the caller plans the whole workspace.
  {
    assert.equal(FINDER_PATH, "scripts/ci-push-base.mjs");
    const gitEnv = {
      ...process.env,
      GIT_CONFIG_GLOBAL: "/dev/null",
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_AUTHOR_NAME: "ci",
      GIT_AUTHOR_EMAIL: "ci@example.invalid",
      GIT_COMMITTER_NAME: "ci",
      GIT_COMMITTER_EMAIL: "ci@example.invalid",
    };
    const git = (cwd, ...args) => {
      const result = spawnSync("git", args, { cwd, encoding: "utf8", env: gitEnv });
      assert.equal(result.status, 0, `git ${args.join(" ")}: ${result.stderr}`);
      return result.stdout.trim();
    };
    const commit = (cwd, message) => {
      git(cwd, "add", "-A");
      git(cwd, "commit", "-q", "--allow-empty", "-m", message);
      return git(cwd, "rev-parse", "HEAD");
    };
    // A finder that prints what the case asks for: the copy a pushed commit
    // could have planted. It records its arguments when it runs.
    const stub = [
      'import { writeFileSync } from "node:fs";',
      "writeFileSync(process.env.STUB_MARKER, process.argv.slice(2).join(\" \"));",
      'process.stdout.write(process.env.STUB_OUTPUT ?? "");',
      'process.exit(Number(process.env.STUB_STATUS ?? "0"));',
      "",
    ].join("\n");
    // main: c0 - c1 - c2 - c3 (HEAD); side: c0 - side; nofinder: c1 - n1
    // (removes the finder) - n2.
    const origin = join(scratch, "origin");
    mkdirSync(join(origin, "scripts"), { recursive: true });
    git(scratch, "init", "-q", "-b", "main", origin);
    writeFileSync(join(origin, FINDER_PATH), stub);
    const c0 = commit(origin, "c0");
    writeFileSync(join(origin, "a.txt"), "1\n");
    const c1 = commit(origin, "c1");
    git(origin, "checkout", "-q", "-b", "side", c0);
    writeFileSync(join(origin, "side.txt"), "side\n");
    const side = commit(origin, "side");
    git(origin, "checkout", "-q", "-b", "nofinder", c1);
    rmSync(join(origin, FINDER_PATH));
    const n1 = commit(origin, "n1");
    writeFileSync(join(origin, "b.txt"), "b\n");
    commit(origin, "n2");
    git(origin, "checkout", "-q", "main");
    writeFileSync(join(origin, "a.txt"), "2\n");
    const c2 = commit(origin, "c2");
    writeFileSync(join(origin, "a.txt"), "3\n");
    const c3 = commit(origin, "c3");
    // The CI checkout: HEAD and its first parent only.
    const shallow = join(scratch, "shallow");
    git(scratch, "clone", "-q", "--depth", "2", "--branch", "main", `file://${origin}`, shallow);
    assert.notEqual(spawnSync("git", ["cat-file", "-e", `${c1}^{commit}`], { cwd: shallow, env: gitEnv }).status, 0, "the shallow clone lacks c1");
    const nofinder = join(scratch, "nofinder");
    git(scratch, "clone", "-q", "--branch", "nofinder", origin, nofinder);
    const lone = join(scratch, "lone");
    git(scratch, "init", "-q", "-b", "main", lone);
    commit(lone, "root");

    const plan = (cwd, payloadBase, { output = "", status = 0, compare = {} } = {}) => {
      const fixture = join(scratch, `fixture-${nextId++}.json`);
      writeFileSync(fixture, JSON.stringify({ commits: [], runs: [], attestations: {}, compare }));
      const marker = join(scratch, `marker-${nextId++}`);
      const result = spawnSync("node", [wrapper, "--repository", REPO, "--payload-base", payloadBase], {
        cwd,
        encoding: "utf8",
        env: {
          ...gitEnv,
          MEERKAT_CI_PUSH_BASE_GH: fakeGh,
          MEERKAT_CI_PUSH_BASE_FIXTURE: fixture,
          STUB_OUTPUT: output,
          STUB_STATUS: String(status),
          STUB_MARKER: marker,
        },
      });
      return { ...result, finderArgs: existsSync(marker) ? readFileSync(marker, "utf8") : null };
    };
    const expectPlanned = (label, cwd, payloadBase, options, base) => {
      const result = plan(cwd, payloadBase, options);
      assert.equal(result.status, 0, `${label}: ${result.stderr}`);
      assert.equal(result.stdout, `${base}\n`, `${label}: base`);
      assert.equal(result.finderArgs, `--repository ${REPO} --head ${c3}`, `${label}: the finder walks back from HEAD`);
    };
    const expectRefused = (label, cwd, payloadBase, options, reason, { finderRuns = true } = {}) => {
      const result = plan(cwd, payloadBase, options);
      assert.equal(result.status, 1, `${label}: exit 1, so the caller plans the whole workspace (${result.stderr})`);
      assert.equal(result.stdout, "", `${label}: no base printed`);
      assert.match(result.stderr, reason, `${label}: says why`);
      assert.equal(result.finderArgs !== null, finderRuns, `${label}: the finder ${finderRuns ? "runs" : "never runs"}`);
    };
    const ahead = (base) => ({ status: "ahead", merge_base_commit: { sha: base } });

    // A one-commit push: the parent is the payload base.
    expectPlanned("the parent itself", shallow, c2, { output: `${c2}\n` }, c2);
    expectPlanned("a deeper ancestor in the local history", origin, c2, { output: `${c0}\n` }, c0);
    expectPlanned(
      "a deeper ancestor GitHub confirms where the checkout is shallow",
      shallow,
      c2,
      { output: `${c0}\n`, compare: { [`${c0}...${c3}`]: ahead(c0) } },
      c0,
    );
    // The parent is not the payload base (a push of several commits, a
    // branch creation), the checkout has no parent, or the parent has no
    // finder: the finder never runs.
    expectRefused("a push of several commits", shallow, c1, { output: `${c1}\n` }, /is not the push's payload base/, { finderRuns: false });
    expectRefused("no payload base", shallow, "", { output: `${c2}\n` }, /is not the push's payload base \(none\)/, { finderRuns: false });
    expectRefused("a branch creation", shallow, "0".repeat(40), { output: `${c2}\n` }, /is not the push's payload base/, { finderRuns: false });
    expectRefused("a root commit", lone, "0".repeat(40), {}, /HEAD\^1 names no commit/, { finderRuns: false });
    expectRefused("a parent without the finder", nofinder, n1, { output: `${n1}\n` }, /has no scripts\/ci-push-base\.mjs/, { finderRuns: false });
    // The finder fails or answers something that is not a base.
    expectRefused("a failed finder", shallow, c2, { output: `${c2}\n`, status: 1 }, /named no base \(exit 1\)/);
    for (const [label, output] of [
      ["nothing", ""],
      ["a revision expression", "HEAD~1\n"],
      ["a short sha", `${c2.slice(0, 12)}\n`],
      ["an upper-case sha", `${c2.toUpperCase()}\n`],
      ["two shas", `${c2}\n${c1}\n`],
      ["a sha with trailing text", `${c2} green\n`],
    ]) {
      expectRefused(`a finder that prints ${label}`, shallow, c2, { output }, /not one full commit sha/);
    }
    expectRefused("a finder that names HEAD", shallow, c2, { output: `${c3}\n` }, /named HEAD itself/);
    expectRefused(
      "a commit off HEAD's history",
      origin,
      c2,
      { output: `${side}\n`, compare: { [`${side}...${c3}`]: { status: "diverged", merge_base_commit: { sha: c0 } } } },
      /is not an ancestor of HEAD/,
    );
    expectRefused("an unknown commit GitHub cannot compare", shallow, c2, { output: `${"f".repeat(40)}\n` }, /GitHub could not compare/);
    expectRefused("a shallow ancestor GitHub cannot compare", shallow, c2, { output: `${c0}\n` }, /GitHub could not compare/);
    expectRefused(
      "a comparison whose merge base is another commit",
      shallow,
      c2,
      { output: `${c0}\n`, compare: { [`${c0}...${c3}`]: ahead(c1) } },
      /is not an ancestor of HEAD/,
    );
    // Usage errors exit 2.
    for (const args of [
      ["--payload-base", c2],
      ["--repository", REPO],
      ["--repository", REPO, "--payload-base"],
      ["--repository", REPO, "--payload-base", c2, "--bogus", "x"],
    ]) {
      assert.equal(spawnSync("node", [wrapper, ...args], { cwd: shallow, env: gitEnv }).status, 2, `usage error: ${args.join(" ")}`);
    }
  }

  // ci.yml: a push to main plans against the wrapper's base, given the
  // payload base (github.event.before), and plans the whole workspace
  // without one; it never runs the parent's finder itself; a release
  // projection keeps its verified first parent.
  const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  assert.match(
    workflow,
    /if found_base="\$\(node scripts\/ci-push-plan-base\.mjs --repository "\$\{GITHUB_REPOSITORY\}" --payload-base "\$\{BASE_SHA\}"\)"; then/,
    "a main push takes its base from ci-push-plan-base.mjs with the payload base",
  );
  assert.match(workflow, /github\.event_name == 'push' && github\.event\.before \|\|/, "BASE_SHA is a push's payload base");
  assert.doesNotMatch(workflow, /HEAD\^1:scripts\/ci-push-base\.mjs/, "ci.yml never runs the parent's finder directly");
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
