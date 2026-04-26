#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import {
  crateName,
  gitLines,
  isFastTest,
  normalizePath,
  packageDir,
  packageDirs,
  packageForFile,
  readMetadata,
  root,
  testSourcePaths,
  workspacePackages,
} from "./rust-test-selector.mjs";

function parseArgs() {
  const paths = [];
  let emptyIfNoLabels = false;
  let includeAll = false;
  let mode = "affected";
  let kind = "test";
  for (const arg of process.argv.slice(2)) {
    if (arg === "--empty-if-no-labels") {
      emptyIfNoLabels = true;
    } else if (arg === "--all") {
      includeAll = true;
    } else if (arg === "--owned") {
      mode = "owned";
    } else if (arg === "--affected") {
      mode = "affected";
    } else if (arg === "--build") {
      kind = "build";
    } else if (arg === "--clippy") {
      kind = "clippy";
    } else if (arg === "--test") {
      kind = "test";
    } else if (arg === "--help" || arg === "-h") {
      console.log("usage: bazel-affected-fast-tests.mjs [--owned|--affected] [--test|--build|--clippy] [--all] [--empty-if-no-labels] [changed-path ...]");
      process.exit(0);
    } else {
      paths.push(arg);
    }
  }
  return { emptyIfNoLabels, includeAll, kind, mode, paths };
}

const args = parseArgs();

function changedFiles() {
  if (args.paths.length) return args.paths;

  const files = new Set([
    ...gitLines(["diff", "--name-only", "HEAD", "--"]),
    ...gitLines(["diff", "--name-only", "--cached", "--"]),
  ]);
  return [...files].sort();
}

const metadata = readMetadata();

const packages = workspacePackages(metadata);
const byId = new Map(packages.map((pkg) => [pkg.id, pkg]));
const byName = new Map(packages.map((pkg) => [pkg.name, pkg]));
const dirs = packageDirs(packages);

const reverseDeps = new Map(packages.map((pkg) => [pkg.id, new Set()]));
for (const pkg of packages) {
  for (const dep of pkg.dependencies) {
    if (dep.source !== null) continue;
    const depPkg = byName.get(dep.name);
    if (depPkg) reverseDeps.get(depPkg.id)?.add(pkg.id);
  }
}

function hasFastSuite(pkg) {
  const buildFile = resolve(root, packageDir(pkg), "BUILD.bazel");
  return existsSync(buildFile) && readFileSync(buildFile, "utf8").includes('name = "fast_tests"');
}

const buildFileContents = new Map();

function hasBazelTarget(pkg, name) {
  const buildFile = resolve(root, packageDir(pkg), "BUILD.bazel");
  if (!existsSync(buildFile)) return false;
  let contents = buildFileContents.get(buildFile);
  if (contents === undefined) {
    contents = readFileSync(buildFile, "utf8");
    buildFileContents.set(buildFile, contents);
  }
  return contents.includes(`name = "${name}"`);
}

function exactTestLabelForFile(file, pkg, { fastOnly = false } = {}) {
  const normalized = normalizePath(file);
  const labels = [];
  for (const target of pkg.targets) {
    if (!target.kind.includes("test")) continue;
    if (target["required-features"]?.length) continue;
    if (fastOnly && !isFastTest(pkg, target)) continue;
    const targetName = `${crateName(target.name)}_test`;
    if (!hasBazelTarget(pkg, targetName)) continue;
    if (testSourcePaths(target, pkg).has(normalized)) {
      labels.push(`//${packageDir(pkg)}:${targetName}`);
    }
  }
  return labels;
}

function hasAnyTestTargetForFile(file, pkg) {
  const normalized = normalizePath(file);
  return pkg.targets.some((target) =>
    target.kind.includes("test") && testSourcePaths(target, pkg).has(normalized)
  );
}

function buildLabels(pkg) {
  const dir = packageDir(pkg);
  const libOrMacro = pkg.targets.find((target) =>
    target.kind.includes("proc-macro") || target.kind.includes("lib")
  );
  const labels = [];
  for (const target of pkg.targets) {
    if (target.kind.includes("bench") || target.kind.includes("example") || target.kind.includes("test")) {
      continue;
    }
    if (target["required-features"]?.length) continue;
    if (target.kind.includes("proc-macro") || target.kind.includes("lib")) {
      labels.push(`//${dir}:${crateName(pkg.name)}`);
    } else if (target.kind.includes("bin")) {
      const name = target.name !== pkg.name || libOrMacro ? `${crateName(target.name)}_bin` : crateName(pkg.name);
      labels.push(`//${dir}:${name}`);
    }
  }
  return [...new Set(labels)].sort();
}

function testLabels(pkg) {
  const dir = packageDir(pkg);
  const labels = [];
  for (const target of pkg.targets) {
    if (!target.kind.includes("test")) continue;
    if (target["required-features"]?.length) continue;
    const source = readFileSync(target.src_path, "utf8");
    if (source.includes("trybuild::")) continue;
    labels.push(`//${dir}:${crateName(target.name)}_test`);
  }
  return [...new Set(labels)].sort();
}

function clippyLabels(pkg) {
  return [...new Set([...buildLabels(pkg), ...testLabels(pkg)])].sort();
}

function affectedClosure(seedIds) {
  const seen = new Set(seedIds);
  const queue = [...seedIds];
  for (let i = 0; i < queue.length; i += 1) {
    for (const dep of reverseDeps.get(queue[i]) ?? []) {
      if (!seen.has(dep)) {
        seen.add(dep);
        queue.push(dep);
      }
    }
  }
  return seen;
}

const files = changedFiles();
if (args.includeAll) {
  const labels = args.kind === "build"
    ? packages.flatMap(buildLabels).sort()
    : args.kind === "clippy"
    ? packages.flatMap(clippyLabels).sort()
    : packages
      .filter(hasFastSuite)
      .map((pkg) => `//${packageDir(pkg)}:fast_tests`)
      .sort();
  console.log(labels.length ? labels.join(" ") : args.kind === "test" ? "//:fast_tests" : "//...");
  process.exit(0);
}
if (files.length === 0) {
  console.log(args.kind === "test" ? "//:fast_tests" : "//...");
  process.exit(0);
}

const seedIds = new Set();
const exactLabels = new Set();
const unmapped = [];
let suppressedOwnedTestTarget = false;
for (const file of files) {
  if (file.endsWith("BUILD.bazel") || file === "BUILD.bazel") continue;
  const pkg = packageForFile(file, dirs);
  if (pkg) {
    const exactTestLabels = args.mode === "owned"
      ? exactTestLabelForFile(file, pkg, { fastOnly: args.kind === "test" })
      : [];
    if (exactTestLabels.length) {
      for (const label of exactTestLabels) exactLabels.add(label);
    } else if (
      args.mode === "owned" &&
      args.kind === "test" &&
      hasAnyTestTargetForFile(file, pkg)
    ) {
      // Non-fast or not-yet-Bazlified test targets are still handled by the
      // build/clippy selectors, but the fast test selector should not broaden
      // to unrelated package fast tests just because one test source changed.
      suppressedOwnedTestTarget = true;
    } else {
      seedIds.add(pkg.id);
    }
  } else {
    unmapped.push(file);
  }
}

if (unmapped.length || (seedIds.size === 0 && exactLabels.size === 0)) {
  if (suppressedOwnedTestTarget && args.emptyIfNoLabels) {
    console.log("");
    process.exit(0);
  }
  console.log(args.kind === "test" ? "//:fast_tests" : "//...");
  process.exit(0);
}

const selectedIds = args.mode === "owned" ? seedIds : affectedClosure(seedIds);
const selectedPackages = [...selectedIds]
  .map((id) => byId.get(id))
  .filter(Boolean);
const labels = args.kind === "build"
  ? [
    ...exactLabels,
    ...selectedPackages.flatMap(buildLabels),
  ].sort()
  : args.kind === "clippy"
  ? [
    ...exactLabels,
    ...selectedPackages.flatMap(clippyLabels),
  ].sort()
  : [
    ...exactLabels,
    ...selectedPackages
    .filter(hasFastSuite)
      .map((pkg) => `//${packageDir(pkg)}:fast_tests`),
  ].sort();

const uniqueLabels = [...new Set(labels)].sort();

if (uniqueLabels.length) {
  console.log(uniqueLabels.join(" "));
} else if (args.emptyIfNoLabels) {
  console.log("");
} else {
  console.log(args.kind === "test" ? "//:fast_tests" : "//...");
}
