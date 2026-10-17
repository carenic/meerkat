#!/usr/bin/env node
// Self-test of the WASM lane selection in scripts/buildbuddy-edge-changes
// against cargo metadata: a change to any workspace crate that the web
// runtime's non-optional dependencies reach on wasm32 selects the WASM
// lanes, and a change to a crate outside all its dependencies (optional ones
// included) does not. The detector lists the crates by hand; this fails when
// that list drifts from the manifests.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { packageDir, readMetadata, root, workspacePackages } from "./rust-test-selector.mjs";

const WEB_RUNTIME = "meerkat-web-runtime";
const members = new Map(workspacePackages(readMetadata()).map((pkg) => [pkg.name, pkg]));

// Does a dependency's target filter hold on wasm32? Only the filters the
// workspace uses; another one fails here until this learns it.
function onWasm(target) {
  if (target === null || target === 'cfg(target_arch = "wasm32")') return true;
  if (target === 'cfg(not(target_arch = "wasm32"))') return false;
  throw new Error(`unknown dependency target filter ${target}`);
}

// The workspace crates the web runtime's normal and build dependencies reach
// on wasm32, through optional dependencies too or not.
function reach({ optional }) {
  const seen = new Set();
  const visit = (name) => {
    for (const dep of members.get(name).dependencies) {
      if (!dep.path || !members.has(dep.name) || dep.kind === "dev") continue;
      if ((dep.optional && !optional) || !onWasm(dep.target) || seen.has(dep.name)) continue;
      seen.add(dep.name);
      visit(dep.name);
    }
  };
  visit(WEB_RUNTIME);
  return seen;
}

const required = reach({ optional: false });
const possible = reach({ optional: true });
const selectsWasm = (pkg) =>
  /^wasm_changed=true$/m.test(
    execFileSync("scripts/buildbuddy-edge-changes", ["--paths-from-stdin"], {
      cwd: root,
      input: `${packageDir(pkg)}/src/lib.rs\n`,
      encoding: "utf8",
    }),
  );

const missing = [];
const extra = [];
for (const [name, pkg] of members) {
  if (name === WEB_RUNTIME) continue;
  if (required.has(name) && !selectsWasm(pkg)) missing.push(name);
  if (!possible.has(name) && selectsWasm(pkg)) extra.push(name);
}
assert.ok(required.size > 0, `${WEB_RUNTIME} reaches no workspace crate`);
assert.deepEqual(missing, [], "crates the web runtime builds on wasm32 that select no WASM lane");
assert.deepEqual(extra, [], "crates outside the web runtime's dependencies that select the WASM lanes");
console.log(`buildbuddy-edge-changes WASM selection matches cargo metadata (${required.size} required crates)`);
