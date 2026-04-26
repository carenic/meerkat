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

let includeNonFast = false;
const paths = [];
for (const arg of process.argv.slice(2)) {
  if (arg === "--include-non-fast") {
    includeNonFast = true;
  } else if (arg === "--help" || arg === "-h") {
    console.log("usage: cargo-exact-tests.mjs [--include-non-fast] <changed-path>...");
    process.exit(0);
  } else {
    paths.push(normalizePath(arg));
  }
}

if (paths.length === 0) process.exit(1);

const metadata = readMetadata();
const packages = workspacePackages(metadata);
const dirs = packageDirs(packages);

function exactTestsForFile(file, pkg) {
  const tests = [];
  for (const target of pkg.targets) {
    if (!target.kind.includes("test")) continue;
    if (!includeNonFast && target["required-features"]?.length) continue;
    if (!includeNonFast && !isFastTest(pkg, target)) continue;
    if (testSourcePaths(target, pkg).has(file)) {
      tests.push({
        features: target["required-features"] ?? [],
        name: target.name,
      });
    }
  }
  return tests;
}

let selectedPackage = null;
const selectedTests = new Set();
const selectedFeatures = new Set();

for (const file of paths) {
  const pkg = packageForFile(file, dirs);
  if (!pkg) process.exit(1);
  if (selectedPackage && selectedPackage.name !== pkg.name) process.exit(1);
  selectedPackage = pkg;

  const tests = exactTestsForFile(file, pkg);
  if (tests.length === 0) process.exit(1);
  for (const test of tests) {
    selectedTests.add(test.name);
    for (const feature of test.features) selectedFeatures.add(feature);
  }
}

if (!selectedPackage || selectedTests.size === 0) process.exit(1);

console.log(`package=${selectedPackage.name}`);
for (const feature of [...selectedFeatures].sort()) {
  console.log(`feature=${feature}`);
}
for (const test of [...selectedTests].sort()) {
  console.log(`test=${test}`);
}
