#!/usr/bin/env node
// Every case an exact-profile lane selects must be defined in a file whose
// change runs that lane on a pull request (GUARD_LANE_INPUTS in
// scripts/ci-cargo-lanes.mjs). The selections are read from the same extracts
// of the lanes' definitions that the planner compares between a pull
// request's base and head (GUARD_LANE_DEFINITIONS): the expected list and
// command of the governed-jsonl job in .github/workflows/ci.yml, and the
// suites and CONFINEMENT_* filters of `make test-mcp-confinement-positive`.
// Each selected test name or module is
// resolved to the Rust files of its target that define it; a selection that
// resolves to no file, a defining file outside the lane's inputs, a selected
// package whose manifest is not an input, or an input that no longer exists
// fails.

import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { GUARD_LANE_DEFINITIONS, GUARD_LANE_INPUTS, touchesGuardLane } from "./ci-cargo-lanes.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");
// The extract of `path` that defines `lane`.
function definition(lane, path) {
  const entry = GUARD_LANE_DEFINITIONS[lane].find(([file]) => file === path);
  assert.ok(entry, `GUARD_LANE_DEFINITIONS.${lane} has no ${path}`);
  const text = entry[1](read(path));
  assert.ok(text, `${path} does not define ${lane}`);
  return text;
}

function rustFiles(dir) {
  const absolute = join(root, dir);
  if (!existsSync(absolute)) return [];
  if (statSync(absolute).isFile()) return dir.endsWith(".rs") ? [dir] : [];
  return readdirSync(absolute, { withFileTypes: true }).flatMap((entry) =>
    rustFiles(join(dir, entry.name)),
  );
}

// Package name -> crate directory, from each crate manifest's [package] name.
const crateDirs = new Map();
for (const entry of readdirSync(join(root, "crates"), { withFileTypes: true })) {
  if (!entry.isDirectory()) continue;
  const manifest = join("crates", entry.name, "Cargo.toml");
  if (!existsSync(join(root, manifest))) continue;
  const name = read(manifest).match(/^\[package\][^[]*?^name\s*=\s*"([^"]+)"/ms)?.[1];
  if (name) crateDirs.set(name, join("crates", entry.name));
}

// The Rust files of the targets that cargo arguments such as
// "-p meerkat --lib --test command_hook_confinement" select.
function targetFiles(args) {
  const words = args.trim().split(/\s+/);
  const pkg = words[words.indexOf("-p") + 1];
  const dir = crateDirs.get(pkg);
  assert.ok(dir, `no crate directory for package ${pkg} (${args})`);
  const files = [];
  words.forEach((word, index) => {
    if (word === "--lib") files.push(...rustFiles(join(dir, "src")));
    if (word === "--test") {
      const name = words[index + 1];
      files.push(...rustFiles(join(dir, "tests", `${name}.rs`)), ...rustFiles(join(dir, "tests", name)));
    }
  });
  assert.ok(files.length > 0, `cargo arguments ${args} select no Rust files`);
  return { pkg, dir, files };
}

// The files that define a selected test (`a::b::name`, exact) or module
// (`a::b::module::`, prefix). A file holds a test when it defines `fn name`,
// and a module when it declares it inline or is that module's own file; when
// several files match, those named after the parent module win.
function definingFiles(files, path, kind) {
  const segments = path.split("::").filter(Boolean);
  const name = segments.at(-1);
  const parent = segments.at(-2);
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern =
    kind === "test" ? new RegExp(`\\bfn\\s+${escaped}\\s*[<(]`) : new RegExp(`\\bmod\\s+${escaped}\\s*\\{`);
  const stem = (file) => {
    const parts = file.split("/");
    const base = parts.at(-1).replace(/\.rs$/, "");
    return base === "mod" ? parts.at(-2) : base;
  };
  let found = files.filter((file) => pattern.test(read(file)));
  if (kind === "module") found.push(...files.filter((file) => stem(file) === name && !found.includes(file)));
  const narrowed = parent ? found.filter((file) => stem(file) === parent) : [];
  if (narrowed.length > 0) found = narrowed;
  return found;
}

// Nextest filters `test(=path)` and `test(/^path::/)`, joined by `|`.
function filterAtoms(expression) {
  return expression.split("|").map((atom) => {
    const exact = atom.trim().match(/^test\(=([A-Za-z0-9_:]+)\)$/);
    if (exact) return { path: exact[1], kind: "test" };
    const prefix = atom.trim().match(/^test\(\/\^([A-Za-z0-9_:]+?)::\/\)$/);
    if (prefix) return { path: prefix[1], kind: "module" };
    throw new Error(`unsupported confinement filter atom ${atom.trim()}`);
  });
}

const selections = { governed_jsonl: [], mcp_confinement_positive: [] };

// Governed JSONL: the job's expected list over its command's target.
{
  const step = definition("governed_jsonl", ".github/workflows/ci.yml");
  const list = step.match(/expected = sorted\(\[([\s\S]*?)\]\)/)?.[1];
  assert.ok(list, "ci.yml: the governed JSONL step has no expected list");
  const names = [...list.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  assert.ok(names.length > 0, "ci.yml: the governed JSONL expected list is empty");
  const command = step.match(/command = \[([\s\S]*?)\]/)?.[1];
  assert.ok(command, "ci.yml: the governed JSONL step has no command");
  const args = [...command.matchAll(/"([^"]+)"/g)].map((match) => match[1]).join(" ");
  const target = targetFiles(args.slice(args.indexOf("-p ")));
  for (const name of names) {
    selections.governed_jsonl.push({ label: name, pkg: target.pkg, dir: target.dir, files: definingFiles(target.files, name, "test") });
  }
}

// MCP confinement positive: each suite of the Make target over its filter.
{
  const make = definition("mcp_confinement_positive", "Makefile");
  const filters = new Map(
    [...make.matchAll(/^(CONFINEMENT_[A-Z_]+_POSITIVE) := (.+)$/gm)].map((match) => [match[1], match[2]]),
  );
  const suites = [...make.matchAll(/^\tsuite "([^"]+)" '\$\((CONFINEMENT_[A-Z_]+_POSITIVE)\)'/gm)];
  assert.ok(suites.length > 0, "Makefile: test-mcp-confinement-positive runs no suite");
  for (const [, args, variable] of suites) {
    assert.ok(filters.has(variable), `Makefile: ${variable} is not defined`);
    const target = targetFiles(args);
    for (const atom of filterAtoms(filters.get(variable))) {
      selections.mcp_confinement_positive.push({
        label: `${variable} ${atom.path}`,
        pkg: target.pkg,
        dir: target.dir,
        files: definingFiles(target.files, atom.path, atom.kind),
      });
    }
  }
}

const failures = [];
for (const [lane, selected] of Object.entries(selections)) {
  if (process.argv.includes("--list")) {
    for (const { label, files } of selected) console.log(`${lane}\t${label}\t${files.join(" ")}`);
  }
  for (const input of GUARD_LANE_INPUTS[lane]) {
    if (!existsSync(join(root, input))) failures.push(`${lane}: input ${input} does not exist`);
  }
  const manifests = new Set();
  for (const { label, dir, files } of selected) {
    manifests.add(join(dir, "Cargo.toml"));
    if (files.length === 0) failures.push(`${lane}: ${label} resolves to no defining file`);
    for (const file of files) {
      const path = relative(root, join(root, file));
      if (!touchesGuardLane(lane, [path])) {
        failures.push(`${lane}: ${label} is defined in ${path}, which is not in GUARD_LANE_INPUTS.${lane}`);
      }
    }
  }
  for (const manifest of manifests) {
    if (!touchesGuardLane(lane, [manifest])) {
      failures.push(`${lane}: ${manifest} decides the selected cases' features but is not in GUARD_LANE_INPUTS.${lane}`);
    }
  }
}
if (failures.length > 0) {
  for (const failure of failures) console.error(`guard lane inputs: ${failure}`);
  process.exit(1);
}
const counts = Object.entries(selections)
  .map(([lane, selected]) => `${lane} ${selected.length} selections in ${new Set(selected.flatMap((s) => s.files)).size} files`)
  .join(", ");
console.log(`exact-profile lane inputs cover every selected case (${counts})`);
