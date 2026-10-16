#!/usr/bin/env node
// Checks one closure-tests row before it runs: the binaries whose tests the
// row selects must be exactly the ones the plan names for it
// (scripts/ci-cargo-lanes.mjs, closure_test_rows), and no test of those
// binaries may be dropped by a filter. A row that selects the whole workspace
// lists other members' binaries too (a `--workspace --lib` row lists every
// lib); those may be listed, but none of their tests may be selected. The plan's static coverage check
// (scripts/ci-hook-coverage.mjs) proves the rows cover the pre-push hook's
// binaries; this proves the job built and selected what the row says, so a
// row cannot pass by running fewer binaries than planned.
//
// usage: ci-verify-row-binaries.mjs <nextest-list.json> [--partition count:K/N] <binary-id>...
//   <nextest-list.json> is `cargo nextest list ... --message-format json`
//   output; a binary id is nextest's (`package` for a lib, `package::test`).
//   --partition names the row's own `--partition`: its tests outside that
//   partition run in the row's sibling partitions, which the plan's coverage
//   check requires all of, so only they may be left out here.

import { readFileSync } from "node:fs";

function main() {
  const args = process.argv.slice(2);
  const listPath = args.shift();
  let partition = null;
  if (args[0] === "--partition") {
    args.shift();
    partition = args.shift() ?? "";
    if (!/^count:[1-9][0-9]*\/[1-9][0-9]*$/.test(partition)) {
      throw new Error(`unsupported --partition ${JSON.stringify(partition)}; expected count:K/N`);
    }
  }
  const expected = args;
  if (!listPath || expected.length === 0) {
    throw new Error("usage: ci-verify-row-binaries.mjs <nextest-list.json> [--partition count:K/N] <binary-id>...");
  }
  const list = JSON.parse(readFileSync(listPath, "utf8"));
  const suites = list["rust-suites"];
  if (!suites || typeof suites !== "object") throw new Error(`${listPath} has no rust-suites`);
  const wanted = new Set(expected);
  const problems = [];
  let matched = 0;
  let ignored = 0;
  let elsewhere = 0;
  for (const [id, suite] of Object.entries(suites)) {
    if (!wanted.has(id)) {
      const selected = Object.entries(suite.testcases ?? {}).filter(([, testCase]) => testCase["filter-match"]?.status === "matches");
      if (selected.length > 0) {
        problems.push(`nextest selects ${selected.length} test(s) of ${id}, which the row does not name`);
      }
      continue;
    }
    if (suite.status !== "listed") {
      problems.push(`${id} was not listed (status ${suite.status ?? "missing"})`);
      continue;
    }
    for (const [name, testCase] of Object.entries(suite.testcases ?? {})) {
      const match = testCase["filter-match"] ?? {};
      if (match.status === "matches") {
        matched += 1;
      } else if (match.reason === "ignored") {
        ignored += 1;
      } else if (partition !== null && match.reason === "partition") {
        elsewhere += 1;
      } else {
        problems.push(`${id} ${name} is filtered out (${match.reason ?? match.status ?? "unknown"})`);
      }
    }
  }
  for (const id of wanted) {
    if (!Object.hasOwn(suites, id)) problems.push(`nextest did not list ${id}`);
  }
  if (partition !== null && matched === 0) problems.push(`partition ${partition} selects no test of the row's binaries`);
  if (problems.length > 0) {
    for (const problem of problems) process.stderr.write(`::error::${problem}\n`);
    process.exit(1);
  }
  process.stdout.write(
    `row binaries verified: ${wanted.size} listed as planned, ${matched} tests selected, ${ignored} ignored` +
      (partition !== null ? `, ${elsewhere} in the other partitions of ${partition}` : "") +
      "\n",
  );
}

try {
  main();
} catch (error) {
  process.stderr.write(`ci-verify-row-binaries: ${error?.message ?? error}\n`);
  process.exit(2);
}
