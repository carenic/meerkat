#!/usr/bin/env node
// Self-test of scripts/ci-action-pins.mjs on synthetic trees, then on the
// repository itself.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkRepository, renovateProblems } from "./ci-action-pins.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const checker = resolve(here, "ci-action-pins.mjs");
const SHA = "0123456789abcdef0123456789abcdef01234567";
const goodRenovate = {
  extends: ["config:recommended", "helpers:pinGitHubActionDigests"],
  packageRules: [
    { matchUpdateTypes: ["patch"], automerge: true },
    { matchManagers: ["github-actions"], automerge: false, minimumReleaseAge: "7 days" },
  ],
};

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

  // Pinned third-party actions pass; local actions, reusable workflows and
  // docker images are not third-party refs.
  const good = tree(
    "good",
    steps(`actions/checkout@${SHA} # v6`, "./.github/actions/setup", "docker://alpine:3.20", `taiki-e/install-action@${SHA} # nextest`) +
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

  // Renovate: the pin preset, the cooldown, and a final github-actions rule
  // that turns automerge off after anything that could turn it on.
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
