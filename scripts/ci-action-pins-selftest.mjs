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
    { matchUpdateTypes: ["patch"], automerge: true, minimumReleaseAge: "7 days" },
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

  // A key whose name a line cannot show fails wherever it appears: an
  // explicit `?` key (its value on the next line), an escape in a quoted
  // key (block or flow), an alias; values with escapes and a `?` inside a
  // value are not keys, and an escaped quote does not end a quoted key.
  {
    const opaque = [
      "on: push",
      "jobs:",
      "  a:",
      "    runs-on: ubuntu-24.04",
      "    steps:",
      "      - ? uses",
      "        : actions/checkout@v4",
      '      - "u\\x73es": actions/setup-node@v4',
      '      - { name: flow, "\\u0075ses": actions/setup-go@v5 }',
      "      - *step : actions/setup-python@v5",
      "      - !!str &key \"u\\x73es\": actions/cache@v4",
      `      - uses: actions/checkout@${SHA} # v6`,
      '        with: { path: "a\\tb", ref: "x\\"y" }',
      "      - name: Is it set ? yes",
      '        run: echo "a\\tb: c"',
      '      - "u\\" # not a comment": actions/setup-node@v4',
      "",
    ].join("\n");
    const problems = checkRepository(tree("opaque", opaque));
    assert.deepEqual(
      problems.map((problem) => problem.slice(0, problem.indexOf(": "))),
      [6, 8, 9, 10, 11, 16].map((line) => `.github/workflows/ci.yml:${line}`),
      problems.join("; "),
    );
    for (const problem of problems) assert.match(problem, /a key this check cannot read/);
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

  // A step using an action in PINNED_INPUTS names an exact release in its
  // own `with`, wherever the step puts that key.
  {
    const sccache = `mozilla-actions/sccache-action@${SHA} # v0.0.10`;
    const job = (body) => `jobs:\n  a:\n    steps:\n${body}`;
    for (const [label, body] of [
      ["with after uses", `      - uses: ${sccache}\n        with:\n          version: v0.18.0\n`],
      [
        "with before uses quoted with comments",
        "      - name: Set up sccache\n" +
          "        with:  # inputs\n" +
          "          # The release it installs.\n" +
          "          token: ${{ github.token }}\n" +
          '          version: "v0.18.0" # pinned\n' +
          `        uses: ${sccache}\n`,
      ],
    ]) {
      assert.deepEqual(checkRepository(tree(`sccache-${label.replaceAll(" ", "-")}`, job(body))), [], label);
    }
    for (const [label, body, line, got] of [
      ["no version", `      - uses: ${sccache}\n`, 4, "none"],
      ["the newest release", `      - uses: ${sccache}\n        with:\n          version: latest\n`, 4, "`latest`"],
      ["a minor version", `      - uses: ${sccache}\n        with:\n          version: v0.18\n`, 4, "`v0.18`"],
      ["an expression", `      - uses: ${sccache}\n        with:\n          version: \${{ vars.SCCACHE_VERSION }}\n`, 4, "`${{ vars.SCCACHE_VERSION }}`"],
      ["the next step's version", `      - uses: ${sccache}\n      - uses: actions/setup-node@${SHA} # v6\n        with:\n          version: v0.18.0\n`, 4, "none"],
      [
        "the previous step's version",
        `      - uses: actions/setup-node@${SHA} # v6\n        with:\n          version: v0.18.0\n      - name: Set up sccache\n        uses: ${sccache}\n`,
        8,
        "none",
      ],
    ]) {
      const problems = checkRepository(tree(`sccache-${label.replaceAll(" ", "-").replaceAll("'", "")}`, job(body)));
      assert.deepEqual(
        problems,
        [`.github/workflows/ci.yml:${line}: \`mozilla-actions/sccache-action\` must pin \`with.version\` to an exact release (got ${got})`],
        label,
      );
    }
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
      { ...goodRenovate, packageRules: [{ matchManagers: ["github-actions"], automerge: true, minimumReleaseAge: "7 days" }] },
      "no packageRule turns automerge off",
    ],
    [
      "a patch automerge after the off rule",
      { ...goodRenovate, packageRules: [...goodRenovate.packageRules, { matchUpdateTypes: ["patch"], automerge: true, minimumReleaseAge: "7 days" }] },
      "can turn automerge on",
    ],
    [
      "an off rule limited to some updates",
      {
        ...goodRenovate,
        packageRules: [
          { matchUpdateTypes: ["patch"], automerge: true, minimumReleaseAge: "7 days" },
          { matchManagers: ["github-actions"], matchUpdateTypes: ["major"], automerge: false, minimumReleaseAge: "7 days" },
        ],
      },
      "no packageRule turns automerge off",
    ],
    [
      "no cooldown",
      {
        ...goodRenovate,
        packageRules: [{ matchUpdateTypes: ["patch"], automerge: true, minimumReleaseAge: "7 days" }, { matchManagers: ["github-actions"], automerge: false }],
      },
      "github-actions updates lack a minimumReleaseAge",
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
    [
      "a patch automerge without the release age",
      { ...goodRenovate, packageRules: [{ matchUpdateTypes: ["patch"], automerge: true }, ...goodRenovate.packageRules.slice(1)] },
      'packageRules[0] turns automerge on without a minimumReleaseAge of "7 days"',
    ],
    [
      "an automerge rule with a shorter release age",
      { ...goodRenovate, packageRules: [{ matchUpdateTypes: ["patch"], automerge: true, minimumReleaseAge: "3 days" }, ...goodRenovate.packageRules.slice(1)] },
      'packageRules[0] turns automerge on without a minimumReleaseAge of "7 days"',
    ],
    [
      "a later rule that shortens the release age",
      { ...goodRenovate, packageRules: [...goodRenovate.packageRules, { matchPackageNames: ["serde"], minimumReleaseAge: "1 day" }] },
      "packageRules[3] can shorten the release age of updates packageRules[0] automerges",
    ],
    [
      "a later rule that clears the release age",
      { ...goodRenovate, packageRules: [...goodRenovate.packageRules, { matchManagers: ["npm"], minimumReleaseAge: null }] },
      "packageRules[3] can shorten the release age of updates packageRules[0] automerges",
    ],
  ]) {
    const problems = renovateProblems(config);
    assert.ok(problems.some((problem) => problem.includes(message)), `${label}: ${problems.join("; ")}`);
  }
  // What customManagers pin is never automerged: a rule for every
  // custom.regex update turns automerge off after the patch automerge, with
  // the release age.
  {
    const custom = {
      ...goodRenovate,
      customManagers: [{ customType: "regex", managerFilePatterns: ["/x/"], matchStrings: ["(?<currentValue>v\\S+)"], datasourceTemplate: "github-releases" }],
    };
    const off = { matchManagers: ["custom.regex"], automerge: false, minimumReleaseAge: "7 days" };
    const [patch, ...rest] = goodRenovate.packageRules;
    assert.deepEqual(renovateProblems({ ...custom, packageRules: [patch, off, ...rest] }), []);
    for (const [label, packageRules, message] of [
      ["no custom.regex rule", goodRenovate.packageRules, "no packageRule turns automerge off for every custom.regex update"],
      ["a patch automerge after it", [off, patch, ...rest], "can turn automerge on for custom.regex"],
      ["a rule for one dependency only", [patch, { ...off, matchDepNames: ["mozilla/sccache"] }, ...rest], "no packageRule turns automerge off for every custom.regex update"],
      ["no release age", [patch, { matchManagers: ["custom.regex"], automerge: false }, ...rest], "custom.regex updates lack a minimumReleaseAge"],
    ]) {
      const problems = renovateProblems({ ...custom, packageRules });
      assert.ok(problems.some((problem) => problem.includes(message)), `${label}: ${problems.join("; ")}`);
    }
  }
  // A rule for other managers may automerge after the github-actions rule,
  // with the release age; a later rule may lengthen the age, turn automerge
  // off for some packages, or shorten the age of updates it turns automerge
  // off for.
  assert.deepEqual(
    renovateProblems({
      ...goodRenovate,
      packageRules: [
        ...goodRenovate.packageRules,
        { matchManagers: ["cargo"], automerge: true, minimumReleaseAge: "7 days" },
        { matchPackageNames: ["serde"], minimumReleaseAge: "14 days" },
        { matchPackageNames: ["serde-saphyr", "granit-parser"], automerge: false },
        { matchPackageNames: ["hyper"], automerge: false, minimumReleaseAge: "1 day" },
      ],
    }),
    [],
  );
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// The repository itself.
const result = spawnSync("node", [checker], { encoding: "utf8" });
assert.equal(result.status, 0, `the repository fails the action pin check:\n${result.stderr}`);
console.log("ci-action-pins contracts hold");
