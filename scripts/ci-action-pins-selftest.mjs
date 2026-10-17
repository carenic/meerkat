#!/usr/bin/env node
// Self-test of scripts/ci-action-pins.mjs on synthetic trees, then on the
// repository itself.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { UNPINNED_IMAGES, checkRepository, renovateProblems } from "./ci-action-pins.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const checker = resolve(here, "ci-action-pins.mjs");
const SHA = "0123456789abcdef0123456789abcdef01234567";
const IMAGE_DIGEST = `sha256:${"ab".repeat(32)}`;
const goodRenovate = {
  extends: ["config:recommended", "helpers:pinGitHubActionDigests"],
  packageRules: [
    { matchUpdateTypes: ["patch"], automerge: true },
    { matchManagers: ["github-actions"], matchUpdateTypes: ["digest"], dependencyDashboardApproval: true },
    { matchManagers: ["github-actions"], automerge: false, minimumReleaseAge: "7 days" },
  ],
};
// A workflow written to slip `uses` keys past a line-by-line reader (a
// value on the next line, a flow mapping, a quoted key) beside a plain tag
// ref and two images named without a digest; each must fail.
const BYPASS = [
  "on: push",
  "jobs:",
  "  a:",
  "    runs-on: ubuntu-24.04",
  "    container: docker.io/library/buildpack-deps:bullseye",
  "    steps:",
  "      - name: next-line value",
  "        uses:",
  "          actions/checkout@v4",
  "      - { name: flow mapping, uses: actions/setup-node@v4 }",
  '      - "uses": actions/setup-python@v5',
  "      - uses: docker://alpine:latest",
  "  b:",
  "    runs-on: ubuntu-24.04",
  "    steps:",
  "      - uses: actions/checkout@v4",
  "",
].join("\n");

const scratch = mkdtempSync(join(tmpdir(), "meerkat-ci-action-pins-"));
try {
  const tree = (name, workflow, renovate = goodRenovate) => {
    const root = join(scratch, name);
    mkdirSync(join(root, ".github", "workflows"), { recursive: true });
    mkdirSync(join(root, ".github", "actions", "setup"), { recursive: true });
    writeFileSync(join(root, ".github", "workflows", "ci.yml"), workflow);
    writeFileSync(
      join(root, ".github", "actions", "setup", "action.yml"),
      `runs:\n  using: composite\n  steps:\n    - uses: actions/cache/restore@${SHA} # v5\n`,
    );
    writeFileSync(join(root, "renovate.json"), JSON.stringify(renovate));
    return root;
  };
  const steps = (...uses) => `jobs:\n  a:\n    steps:\n${uses.map((use) => `      - uses: ${use}\n`).join("")}`;

  // Pinned third-party actions pass; local actions and reusable workflows
  // are not third-party refs, and a docker image passes with its digest.
  const good = tree(
    "good",
    steps(`actions/checkout@${SHA} # v6`, "./.github/actions/setup", `docker://alpine:3.20@${IMAGE_DIGEST}`, `taiki-e/install-action@${SHA} # nextest`) +
      "  b:\n    uses: ./.github/workflows/other.yml\n",
  );
  assert.deepEqual(checkRepository(good), []);
  assert.equal(spawnSync("node", [checker, "--root", good]).status, 0, "the checker passes a pinned tree");

  // A tag ref, a short SHA or a pin without its ref comment fails, named by
  // file and line.
  for (const [label, use, message] of [
    ["a tag", "actions/checkout@v6", "is not pinned to a full commit SHA"],
    ["a short sha", "actions/checkout@0123456", "is not pinned to a full commit SHA"],
    ["a branch", "dtolnay/rust-toolchain@1.94.1", "is not pinned to a full commit SHA"],
    ["no ref comment", `actions/checkout@${SHA}`, "has no `# <ref>` comment"],
  ]) {
    const root = tree(label.replaceAll(" ", "-"), steps("./.github/actions/setup", use));
    const problems = checkRepository(root);
    assert.equal(problems.length, 1, `${label}: ${problems}`);
    assert.ok(problems[0].startsWith(".github/workflows/ci.yml:5: "), `${label}: names file:line (${problems[0]})`);
    assert.ok(problems[0].includes(message), `${label}: ${problems[0]}`);
    const result = spawnSync("node", [checker, "--root", root], { encoding: "utf8" });
    assert.equal(result.status, 1, `${label}: the checker exits 1`);
    assert.match(result.stderr, /\.github\/workflows\/ci\.yml:5: /);
  }

  // Every other spelling of a `uses` key fails as unreadable, named by file
  // and line; `uses` text in a block scalar or a comment is not a key.
  {
    const problems = checkRepository(tree("bypass", BYPASS));
    const at = (line) => problems.filter((problem) => problem.startsWith(`.github/workflows/ci.yml:${line}: `));
    assert.equal(problems.length, 6, problems.join("; "));
    assert.match(at(5)[0], /`docker\.io\/library\/buildpack-deps:bullseye` names its image without an @sha256: digest/);
    for (const line of [8, 10, 11]) assert.match(at(line)[0], /a `uses` this check cannot read/);
    assert.match(at(12)[0], /`docker:\/\/alpine:latest` names its image without an @sha256: digest/);
    assert.match(at(16)[0], /`actions\/checkout@v4` is not pinned/);
    const prose =
      steps(`actions/checkout@${SHA} # v6`) +
      "      - name: Say it\n" +
      "        run: |\n" +
      "          echo 'uses: actions/checkout@v4'\n" +
      "          echo \"uses: tj-actions/changed-files@v1\"\n" +
      "        # uses: actions/setup-node@v4 in a comment\n" +
      "      - name: Folded\n" +
      "        run: >-\n" +
      "          uses: actions/setup-go@v5\n";
    assert.deepEqual(checkRepository(tree("prose", prose)), [], "block scalars and comments are not keys");
  }

  // Container images: a `container:` or `image:` value (plain, or the
  // literals of an expression) and a `docker://` reference in the Bazel
  // files name a digest; UNPINNED_IMAGES are the named exceptions.
  {
    const jobs = (body) => `jobs:\n  a:\n    runs-on: ubuntu-24.04\n${body}    steps:\n      - uses: actions/checkout@${SHA} # v6\n`;
    const pinned = `docker.io/library/buildpack-deps:bullseye@${IMAGE_DIGEST}`;
    for (const [label, body] of [
      ["a pinned container", `    container: ${pinned}\n`],
      ["a pinned container in an expression", `    container: \${{ contains(matrix.target, 'linux-gnu') && '${pinned}' || '' }}\n`],
      ["a pinned service image", `    services:\n      db:\n        image: postgres:16@${IMAGE_DIGEST}\n`],
    ]) {
      assert.deepEqual(checkRepository(tree(label.replaceAll(" ", "-"), jobs(body))), [], label);
    }
    for (const [label, body, message] of [
      ["a container in an expression", "    container: ${{ contains(matrix.target, 'linux-gnu') && 'docker.io/library/buildpack-deps:bullseye' || '' }}\n", "names its image without"],
      ["a container mapping's image", "    container:\n      image: node:22\n", "`node:22` names its image without"],
      ["a service image", "    services:\n      db:\n        image: postgres:16\n", "`postgres:16` names its image without"],
      ["a container from the matrix", "    container: ${{ matrix.image }}\n", "a `container` this check cannot read"],
    ]) {
      const problems = checkRepository(tree(label.replaceAll(" ", "-"), jobs(body)));
      assert.equal(problems.length, 1, `${label}: ${problems.join("; ")}`);
      assert.ok(problems[0].includes(message), `${label}: ${problems[0]}`);
    }
    const bazel = (name, content) => {
      const root = tree(name, steps(`actions/checkout@${SHA} # v6`));
      mkdirSync(join(root, "tools", "bazel", "platforms"), { recursive: true });
      writeFileSync(join(root, "tools", "bazel", "platforms", "BUILD.bazel"), content);
      return checkRepository(root);
    };
    const platform = (image) => `platform(\n    exec_properties = {\n        "container-image": "${image}",\n    },\n)\n`;
    assert.deepEqual(bazel("bazel-pinned", platform(`docker://${pinned}`)), []);
    const [excepted] = UNPINNED_IMAGES.keys();
    assert.deepEqual(bazel("bazel-excepted", platform(excepted)), [], "a named exception passes");
    const problems = bazel("bazel-unpinned", platform("docker://docker.io/library/buildpack-deps:bullseye"));
    assert.deepEqual(problems, [
      "tools/bazel/platforms/BUILD.bazel:3: `docker://docker.io/library/buildpack-deps:bullseye` names its image without an @sha256: digest",
    ]);
  }

  // Renovate: the pin preset, the cooldown, digest updates held for
  // approval, and a final github-actions rule that turns automerge off
  // after anything that could turn it on.
  assert.deepEqual(renovateProblems(goodRenovate), []);
  for (const [label, config, message] of [
    ["no pin preset", { ...goodRenovate, extends: ["config:recommended"] }, "pinGitHubActionDigests"],
    ["global automerge", { ...goodRenovate, automerge: true }, "automerge is on for every update"],
    [
      "the old github-actions automerge rule",
      { ...goodRenovate, packageRules: [{ matchManagers: ["github-actions"], automerge: true }] },
      "no packageRule turns automerge off",
    ],
    [
      "a patch automerge after the off rule",
      { ...goodRenovate, packageRules: [...goodRenovate.packageRules, { matchUpdateTypes: ["patch"], automerge: true }] },
      "can turn automerge on",
    ],
    [
      "an off rule limited to some updates",
      {
        ...goodRenovate,
        packageRules: [
          { matchUpdateTypes: ["patch"], automerge: true },
          { matchManagers: ["github-actions"], matchUpdateTypes: ["major"], automerge: false, minimumReleaseAge: "7 days" },
        ],
      },
      "no packageRule turns automerge off",
    ],
    [
      "no cooldown",
      { ...goodRenovate, packageRules: [{ matchUpdateTypes: ["patch"], automerge: true }, { matchManagers: ["github-actions"], automerge: false }] },
      "minimumReleaseAge",
    ],
    [
      "digest updates not held",
      { ...goodRenovate, packageRules: goodRenovate.packageRules.filter((rule) => !rule.dependencyDashboardApproval) },
      "holds every github-actions digest update",
    ],
    [
      "digest updates held for one action only",
      {
        ...goodRenovate,
        packageRules: goodRenovate.packageRules.map((rule) =>
          rule.dependencyDashboardApproval ? { ...rule, matchPackageNames: ["actions/checkout"] } : rule,
        ),
      },
      "holds every github-actions digest update",
    ],
    [
      "a later rule that releases them",
      { ...goodRenovate, packageRules: [...goodRenovate.packageRules, { matchManagers: ["github-actions"], dependencyDashboardApproval: false }] },
      "can release github-actions digest updates",
    ],
    ["automerge for vulnerability fixes", { ...goodRenovate, vulnerabilityAlerts: { automerge: true } }, "vulnerabilityAlerts.automerge turns automerge on"],
    ["automerge for an update type", { ...goodRenovate, digest: { automerge: true } }, "digest.automerge turns automerge on"],
    ["automerge for lock file maintenance", { ...goodRenovate, lockFileMaintenance: { enabled: true, automerge: true } }, "lockFileMaintenance.automerge"],
  ]) {
    const problems = renovateProblems(config);
    assert.ok(problems.some((problem) => problem.includes(message)), `${label}: ${problems.join("; ")}`);
  }
  // A rule for other managers may automerge after the github-actions rule.
  assert.deepEqual(
    renovateProblems({ ...goodRenovate, packageRules: [...goodRenovate.packageRules, { matchManagers: ["cargo"], automerge: true }] }),
    [],
  );
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// The repository itself.
const result = spawnSync("node", [checker], { encoding: "utf8" });
assert.equal(result.status, 0, `the repository fails the action pin check:\n${result.stderr}`);
console.log("ci-action-pins contracts hold");
