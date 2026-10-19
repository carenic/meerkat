#!/usr/bin/env node
// Self-test of the commit-stage hook that regenerates Bazel BUILD files
// (`buildbuddy-generate-fix` in .pre-commit-config.yaml): it runs the
// generator, and it runs on every kind of file the generator reads, so a
// commit cannot leave a generated BUILD.bazel stale for CI to find.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const lines = readFileSync(resolve(root, ".pre-commit-config.yaml"), "utf8").split("\n");

// The hook's own block: from its id line to the next hook or repo entry.
const start = lines.findIndex((line) => /^\s*- id: buildbuddy-generate-fix\s*$/.test(line));
assert.ok(start >= 0, ".pre-commit-config.yaml has the buildbuddy-generate-fix hook");
let end = start + 1;
while (end < lines.length && !/^\s*- (id|repo):/.test(lines[end])) end += 1;
const block = lines.slice(start, end);
const field = (name) => {
  const values = block.map((line) => new RegExp(`^\\s*${name}:\\s*(.*?)\\s*$`).exec(line)).filter(Boolean);
  assert.equal(values.length, 1, `the hook names ${name} once`);
  return values[0][1];
};
assert.equal(field("entry"), "node scripts/generate-bazel-rust-builds.mjs", "the hook runs the BUILD generator");
assert.equal(field("pass_filenames"), "false", "the generator sees the whole workspace, not the staged files");
// No stages of its own: it runs at the commit stage, the config's default.
assert.ok(!block.some((line) => /^\s*stages:/.test(line)), "the hook keeps the default commit stage");
assert.ok(lines.some((line) => /^default_stages: \[pre-commit\]\s*$/.test(line)), "the default stage is commit");
const quoted = /^'(.*)'$/.exec(field("files"));
assert.ok(quoted, "the files pattern is one single-quoted YAML scalar");
// pre-commit applies `files` with Python's re.search; this pattern means the
// same in JavaScript.
const files = new RegExp(quoted[1].replaceAll("''", "'"));

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
  "examples/031-web-mob/src/main.rs",
];
for (const path of SELECTED) assert.ok(files.test(path), `the BUILD hook runs on ${path}`);
for (const path of NOT_SELECTED) assert.ok(!files.test(path), `the BUILD hook does not run on ${path}`);

console.log("precommit BUILD hook contracts hold");
