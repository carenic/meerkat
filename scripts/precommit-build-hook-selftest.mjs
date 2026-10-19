#!/usr/bin/env node
// Self-test of the commit-stage hook that regenerates the Bazel BUILD files
// (`buildbuddy-generate-fix` in .pre-commit-config.yaml, run by
// scripts/precommit-build-hook.mjs). The hook runs on every commit; it runs
// the generator when a staged path of any status, a deletion included, is a
// generator input; and it refuses the commit while a Rust file the generator
// would read from disk is not in the index. A commit can therefore neither
// leave a generated BUILD.bazel stale nor make one name a file the commit
// does not contain.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { GENERATOR_INPUT, hookAction } from "./precommit-build-hook.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const lines = readFileSync(resolve(root, ".pre-commit-config.yaml"), "utf8").split("\n");

// The hook's own block: from its id line to the next hook or repo entry.
const start = lines.findIndex((line) => /^\s*- id: buildbuddy-generate-fix\s*$/.test(line));
assert.ok(start >= 0, ".pre-commit-config.yaml has the buildbuddy-generate-fix hook");
let end = start + 1;
while (end < lines.length && !/^\s*- (id|repo):/.test(lines[end])) end += 1;
const block = lines.slice(start, end);
const values = (name) => block.map((line) => new RegExp(`^\\s*${name}:\\s*(.*?)\\s*$`).exec(line)).filter(Boolean);
const field = (name) => {
  const found = values(name);
  assert.equal(found.length, 1, `the hook names ${name} once`);
  return found[0][1];
};
assert.equal(field("entry"), "node scripts/precommit-build-hook.mjs", "the hook runs its runner, which runs the generator");
assert.equal(field("always_run"), "true", "the hook runs on every commit: pre-commit's file list has no deletions");
assert.equal(field("pass_filenames"), "false", "the generator sees the whole workspace, not the staged files");
assert.equal(values("files").length, 0, "the runner, not a files pattern, selects the commits");
// No stages of its own: it runs at the commit stage, the config's default.
assert.equal(values("stages").length, 0, "the hook keeps the default commit stage");
assert.ok(lines.some((line) => /^default_stages: \[pre-commit\]\s*$/.test(line)), "the default stage is commit");

// The generator's inputs.
const SELECTED = [
  // Cargo metadata.
  "Cargo.toml",
  "Cargo.lock",
  "MODULE.bazel",
  "crates/meerkat-core/Cargo.toml",
  "tests/integration/Cargo.toml",
  "tests/fixtures/mcp-test-server/Cargo.toml",
  // Rust sources: a comment or a string can change a test's data, and a new
  // module file changes a target's srcs.
  "crates/meerkat-store/src/sqlite_store.rs",
  "crates/meerkat-authorization/tests/native_governed_loop/owner_change_wait.rs",
  "crates/meerkat-core/build.rs",
  "tests/integration/tests/e2e_auth_lane.rs",
  "tests/integration/src/e2e_lanes.rs",
  "tests/fixtures/mcp-test-server/src/main.rs",
  // The generator and the files it reads.
  "scripts/generate-bazel-rust-builds.mjs",
  "scripts/rust-source-text.mjs",
  "scripts/rust-test-selector.mjs",
  "scripts/non-shipping-features.json",
];
const NOT_SELECTED = [
  "README.md",
  "CHANGELOG.md",
  "docs/index.mdx",
  "crates/meerkat-core/README.md",
  "crates/meerkat-core/BUILD.bazel",
  ".github/workflows/ci.yml",
  "sdks/web/src/runtime.ts",
  "scripts/ci-cargo-lanes.mjs",
  "scripts/precommit-build-hook.mjs",
  "examples/031-web-mob/src/main.rs",
];
for (const path of SELECTED) assert.ok(GENERATOR_INPUT.test(path), `the BUILD hook runs on ${path}`);
for (const path of NOT_SELECTED) assert.ok(!GENERATOR_INPUT.test(path), `the BUILD hook does not run on ${path}`);

// The decision, from the staged paths and the paths outside the index.
assert.deepEqual(hookAction(["README.md"], ["crates/a/tests/new.rs"]), { action: "skip" }, "no input: no run, whatever is on disk");
assert.deepEqual(hookAction(["crates/a/src/lib.rs"], []), { action: "run" });
assert.deepEqual(hookAction(["README.md", "Cargo.lock"], ["docs/notes.rs", "crates/a/notes.txt"]), { action: "run" }, "only Rust files under crates/ or tests/ are read");
assert.deepEqual(
  hookAction(["crates/a/src/lib.rs"], ["tests/x/tests/b.rs", "crates/a/tests/a.rs", "crates/a/notes.txt"]),
  { action: "refuse", files: ["crates/a/tests/a.rs", "tests/x/tests/b.rs"] },
  "a Rust file outside the index refuses the commit, listed by path",
);

// The runner in a scratch repository, with a stand-in generator that records
// each run in runs.log beside the repository and exits with the code in
// exit-code when that file exists.
const scratch = mkdtempSync(join(tmpdir(), "precommit-build-hook-"));
try {
  const repo = join(scratch, "repo");
  const runsLog = join(scratch, "runs.log");
  const write = (path, text) => {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), text);
  };
  write("Cargo.toml", "[workspace]\n");
  write("README.md", "scratch\n");
  write("crates/a/src/lib.rs", "pub fn a() {}\n");
  write("crates/a/tests/old.rs", "#[test]\nfn old() {}\n");
  mkdirSync(join(repo, "scripts"), { recursive: true });
  copyFileSync(resolve(root, "scripts/precommit-build-hook.mjs"), join(repo, "scripts/precommit-build-hook.mjs"));
  write(
    "scripts/generate-bazel-rust-builds.mjs",
    [
      'import { appendFileSync, existsSync, readFileSync } from "node:fs";',
      'appendFileSync(new URL("../../runs.log", import.meta.url), "run\\n");',
      'const code = new URL("../../exit-code", import.meta.url);',
      "if (existsSync(code)) process.exit(Number(readFileSync(code, \"utf8\")));",
      "",
    ].join("\n"),
  );
  // Git and the runner see only the scratch repository and no user or
  // system configuration, even when this test runs inside a Git hook.
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("GIT_")));
  Object.assign(env, { GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1" });
  const git = (...args) =>
    execFileSync(
      "git",
      [
        ...["-c", "init.defaultBranch=main", "-c", "user.name=Meerkat", "-c", "user.email=meerkat@example.invalid"],
        ...["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"],
        ...args,
      ],
      { cwd: repo, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
  const runs = () => (existsSync(runsLog) ? readFileSync(runsLog, "utf8").split("\n").filter(Boolean).length : 0);
  const hook = () => spawnSync(process.execPath, ["scripts/precommit-build-hook.mjs"], { cwd: repo, env, encoding: "utf8" });

  mkdirSync(repo, { recursive: true });
  git("init", "-q");
  git("add", "-A");
  git("commit", "-qm", "scratch");

  // A Rust change with a Rust file outside the index, untracked or ignored:
  // refused, each file named, the generator not run.
  write("crates/a/src/lib.rs", "pub fn a() -> u8 { 1 }\n");
  git("add", "crates/a/src/lib.rs");
  write("crates/a/tests/new.rs", "#[test]\nfn new() {}\n");
  write(".gitignore", "crates/a/tests/ignored.rs\n");
  write("crates/a/tests/ignored.rs", "#[test]\nfn ignored() {}\n");
  let result = hook();
  assert.equal(result.status, 1, `a Rust file outside the index refuses the commit: ${result.stderr}`);
  assert.match(result.stderr, /^ {2}crates\/a\/tests\/new\.rs$/m, "the untracked file is named");
  assert.match(result.stderr, /^ {2}crates\/a\/tests\/ignored\.rs$/m, "the ignored file is named: cargo does not read .gitignore");
  assert.equal(runs(), 0, "a refused commit does not run the generator");

  // Without them, the same commit runs the generator.
  unlinkSync(join(repo, "crates/a/tests/new.rs"));
  unlinkSync(join(repo, "crates/a/tests/ignored.rs"));
  result = hook();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(runs(), 1, "a staged Rust change runs the generator");
  git("add", ".gitignore");
  git("commit", "-qm", "change");

  // A commit that only deletes a test file runs the generator.
  git("rm", "-q", "crates/a/tests/old.rs");
  result = hook();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(runs(), 2, "a staged deletion runs the generator");
  git("commit", "-qm", "delete");

  // A commit that changes no input runs nothing and refuses nothing.
  write("crates/a/tests/other.rs", "#[test]\nfn other() {}\n");
  write("README.md", "changed\n");
  git("add", "README.md");
  result = hook();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(runs(), 2, "a commit without a generator input does not run the generator");
  unlinkSync(join(repo, "crates/a/tests/other.rs"));
  git("commit", "-qm", "readme");

  // The generator's failure fails the commit.
  write("Cargo.lock", "# lock\n");
  git("add", "Cargo.lock");
  writeFileSync(join(scratch, "exit-code"), "7");
  result = hook();
  assert.equal(result.status, 7, "the generator's exit status is the hook's");
  assert.equal(runs(), 3);
} finally {
  if (scratch.startsWith(`${tmpdir()}${sep}precommit-build-hook-`)) rmSync(scratch, { recursive: true, force: true });
}

console.log("precommit BUILD hook contracts hold");
