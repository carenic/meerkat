#!/usr/bin/env node
import {
  isFastTest,
  normalizePath,
  packageDirs,
  packageForFile,
  readMetadata,
  testSourcePaths,
  workspacePackages,
} from "./rust-test-selector.mjs";

const paths = process.argv.slice(2).map(normalizePath);

if (paths.length === 0) process.exit(1);

const metadata = readMetadata();
const packages = workspacePackages(metadata);
const dirs = packageDirs(packages);

function exactTestsForFile(file, pkg) {
  const tests = [];
  for (const target of pkg.targets) {
    if (!target.kind.includes("test")) continue;
    if (target["required-features"]?.length) continue;
    if (!isFastTest(pkg, target)) continue;
    if (testSourcePaths(target, pkg).has(file)) tests.push(target.name);
  }
  return tests;
}

let selectedPackage = null;
const selectedTests = new Set();

for (const file of paths) {
  const pkg = packageForFile(file, dirs);
  if (!pkg) process.exit(1);
  if (selectedPackage && selectedPackage.name !== pkg.name) process.exit(1);
  selectedPackage = pkg;

  const tests = exactTestsForFile(file, pkg);
  if (tests.length === 0) process.exit(1);
  for (const test of tests) selectedTests.add(test);
}

if (!selectedPackage || selectedTests.size === 0) process.exit(1);

console.log(`package=${selectedPackage.name}`);
for (const test of [...selectedTests].sort()) {
  console.log(`test=${test}`);
}
