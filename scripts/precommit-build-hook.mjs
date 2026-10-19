#!/usr/bin/env node
// The commit-stage hook that regenerates the Bazel BUILD files
// (`buildbuddy-generate-fix` in .pre-commit-config.yaml).
//
// The hook runs on every commit and decides here whether the commit changes
// an input of the BUILD generator. pre-commit leaves deletions out of the
// staged file list it matches a hook's `files` pattern against, so a commit
// that only removed a test file would skip a path-filtered hook and keep the
// file's target. This hook reads every staged path itself, deletions
// included.
//
// The generator reads the working tree, and pre-commit sets aside only the
// unstaged changes to tracked files. A Rust file under crates/ or tests/
// that is not in the index is still read: cargo metadata finds a tests/*.rs
// target and the srcs walk lists every .rs file on disk. The regenerated
// BUILD file would then name a file the commit does not contain, which every
// local check accepts and the CI Bazel build rejects. The hook refuses the
// commit while such a file exists.

import { execFileSync, spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// The generator's inputs: Cargo metadata, the Rust sources under crates/ and
// tests/ (a test's data and srcs follow its source text, and a new module
// file joins srcs), the generator and the modules and lists it reads.
export const GENERATOR_INPUT =
  /^(Cargo\.toml|Cargo\.lock|MODULE\.bazel|(crates\/[^/]+|tests\/integration|tests\/fixtures\/[^/]+)\/Cargo\.toml|(crates|tests)\/.+\.rs|scripts\/(generate-bazel-rust-builds|rust-source-text|rust-test-selector)\.mjs|scripts\/non-shipping-features\.json)$/;

// A Rust file on disk that the generator can read.
const RUST_SOURCE = /^(crates|tests)\/.+\.rs$/;

// The hook's decision for a commit, from its staged paths (every status) and
// the paths on disk that are not in the index:
//   { action: "skip" }                 no generator input changes;
//   { action: "refuse", files: [...] } Rust files outside the index would
//                                      enter the regenerated BUILD files;
//   { action: "run" }                  regenerate.
export function hookAction(staged, outsideIndex) {
  if (!staged.some((path) => GENERATOR_INPUT.test(path))) return { action: "skip" };
  const files = outsideIndex.filter((path) => RUST_SOURCE.test(path)).sort();
  if (files.length > 0) return { action: "refuse", files };
  return { action: "run" };
}

function gitPaths(root, args) {
  const output = execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  return output.split("\0").filter(Boolean);
}

function main() {
  const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
  // --no-renames lists both sides of a move; without --diff-filter every
  // status counts, deletions included.
  const staged = gitPaths(root, ["diff", "--cached", "--name-only", "--no-renames", "-z"]);
  if (hookAction(staged, []).action === "skip") return 0;
  // Untracked and ignored files alike: neither cargo nor the srcs walk reads
  // .gitignore.
  const outsideIndex = gitPaths(root, ["ls-files", "--others", "-z", "--", "crates", "tests"]);
  const decision = hookAction(staged, outsideIndex);
  if (decision.action === "refuse") {
    console.error("Rust files under crates/ or tests/ are not in the index:");
    for (const path of decision.files) console.error(`  ${path}`);
    console.error(
      "The BUILD generator reads them from disk, so the regenerated BUILD files would name files " +
        "this commit does not contain. Add them to the index, or move them out of crates/ and tests/, " +
        "and commit again.",
    );
    return 1;
  }
  const generator = resolve(dirname(fileURLToPath(import.meta.url)), "generate-bazel-rust-builds.mjs");
  const result = spawnSync(process.execPath, [generator], { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(`cannot run ${generator}: ${result.error.message}`);
    return 1;
  }
  return result.status ?? 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
