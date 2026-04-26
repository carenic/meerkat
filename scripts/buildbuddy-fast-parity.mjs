#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  crateName,
  isFastTest,
  packageDir,
  readMetadata,
  root,
  workspacePackages,
} from "./rust-test-selector.mjs";

const metadata = readMetadata();
const rootBuild = readFileSync(resolve(root, "BUILD.bazel"), "utf8");
const missing = [];
const expected = [];

function hasTarget(pkg, name) {
  const buildPath = resolve(root, packageDir(pkg), "BUILD.bazel");
  if (!existsSync(buildPath)) return false;
  return readFileSync(buildPath, "utf8").includes(`name = "${name}"`);
}

function rootSuiteIncludes(label) {
  return rootBuild.includes(JSON.stringify(label));
}

for (const pkg of workspacePackages(metadata)) {
  for (const target of pkg.targets) {
    if (!target.kind.includes("test")) continue;
    if (!isFastTest(pkg, target)) continue;

    const targetName = `${crateName(target.name)}_test`;
    const label = `//${packageDir(pkg)}:${targetName}`;
    expected.push(label);

    if (!hasTarget(pkg, targetName)) {
      missing.push(`${label} missing generated target`);
    } else if (!rootSuiteIncludes(label)) {
      missing.push(`${label} missing from //:fast_tests`);
    }
  }
}

if (missing.length) {
  console.error("BuildBuddy fast parity failed:");
  for (const item of missing) console.error(`  ${item}`);
  process.exit(1);
}

console.log(`BuildBuddy fast parity ok: ${expected.length} fast Cargo test target(s) mapped`);
