#!/usr/bin/env node
// Writes the row-duration table scripts/ci-cargo-lanes.mjs plans its long
// rows first by (ROW_DURATIONS): the longest a lane row's job took on a
// hosted runner in the given green CI runs.
//
// usage: ci-row-durations.mjs --repository OWNER/REPO [--output FILE] RUN_ID...
//   Reads each run's jobs through `gh api` and keeps, per row, the longest
//   successful job. A row is `<lane>:<rowKey>`, the lane read from the job
//   name (LANE_PREFIXES) and the key from the row's name without the member
//   count of a packed row (rowKey in scripts/ci-cargo-lanes.mjs), so jobs of
//   one anchor in different packings share their row. Without --output the
//   table goes to stdout.

import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { checkRowDurations, rowKey } from "./ci-cargo-lanes.mjs";

export const LANE_PREFIXES = [
  ["Clippy (", "clippy"],
  ["Unit tests (", "unit"],
  ["Main unit tests (", "main-unit"],
  ["Closure tests (", "closure-tests"],
  ["Feature combinations (", "feature-check"],
];

// The table key of a job, or null for a job of no measured lane.
export function rowOf(jobName) {
  for (const [prefix, lane] of LANE_PREFIXES) {
    if (jobName.startsWith(prefix) && jobName.endsWith(")")) return `${lane}:${rowKey(jobName.slice(prefix.length, -1))}`;
  }
  return null;
}

// The longest successful duration of each row among `jobs` (GitHub's job
// objects), in whole seconds.
export function durationsOf(jobs) {
  const rows = {};
  for (const job of jobs) {
    const key = rowOf(job.name);
    if (!key || job.conclusion !== "success" || !job.started_at || !job.completed_at) continue;
    const seconds = Math.round((Date.parse(job.completed_at) - Date.parse(job.started_at)) / 1000);
    if (seconds > 0 && seconds > (rows[key] ?? 0)) rows[key] = seconds;
  }
  return Object.fromEntries(Object.entries(rows).sort(([a], [b]) => a.localeCompare(b)));
}

function main(argv) {
  let repository = "";
  let output = "";
  const runs = [];
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--repository") repository = argv[++i] ?? "";
    else if (argv[i] === "--output") output = argv[++i] ?? "";
    else if (/^[1-9][0-9]*$/.test(argv[i])) runs.push(argv[i]);
    else throw new Error(`unknown argument ${argv[i]}`);
  }
  if (!/^[^/\s]+\/[^/\s]+$/.test(repository) || runs.length === 0) {
    throw new Error("usage: ci-row-durations.mjs --repository OWNER/REPO [--output FILE] RUN_ID...");
  }
  const jobs = runs.flatMap((run) =>
    execFileSync(
      "gh",
      ["api", "--paginate", `repos/${repository}/actions/runs/${run}/jobs?per_page=100&filter=latest`, "--jq", ".jobs[]"],
      { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    )
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => JSON.parse(line)),
  );
  const table = {
    schema_version: 2,
    description:
      "Longest successful hosted-runner job time per lane row, in seconds, from the runs below " +
      "(scripts/ci-row-durations.mjs). A row is keyed by its name without the member count of a packed " +
      "row (rowKey), so a repack keeps its measurement while its anchor stays the same. " +
      "scripts/ci-cargo-lanes.mjs starts the rows at or over WAVE_FIRST_SECONDS, and rows missing here, " +
      "before the others.",
    runs: runs.map(Number),
    rows: durationsOf(jobs),
  };
  checkRowDurations(table, "the generated table");
  const text = `${JSON.stringify(table, null, 2)}\n`;
  if (output) writeFileSync(output, text);
  else process.stdout.write(text);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`ci-row-durations: ${error?.message ?? error}\n`);
    process.exit(1);
  }
}
