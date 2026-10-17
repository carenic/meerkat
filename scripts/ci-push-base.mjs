#!/usr/bin/env node
// The diff base of a push to main.
//
// A push's payload base (github.event.before) is the head of the previous
// push. On main a newer push cancels the still-running run of an older one
// (the `main-<ref>-superseded` concurrency group in ci.yml), and a run can
// also fail. Either way the lanes a pull request leaves to the push
// (closure-tests, main-unit, the closure and feature checks, the meerkat-mob
// chain's suites) never ran for that commit's changes, and the next push,
// diffed against event.before, would not plan them either. The base is
// instead the newest commit before HEAD whose push run of ci.yml on main
// completed successfully and left its exact-tree attestation: every change
// since the last green, attested main run is planned again.
//
// usage: ci-push-base.mjs --repository OWNER/REPO --head SHA [--depth N]
//   Walks HEAD's ancestors newest first (the commits API, at most N,
//   default 100; HEAD itself is never its own base) and prints the first
//   one with a completed, successful push run of ci.yml on main whose
//   attestation (meerkat-ci-attestation-<sha>-<attempt>) names that commit,
//   its tree, the run and attempt, and a successful validation. A run among
//   the newest 100 successful main-push runs qualifies. Exits 1 when no
//   ancestor qualifies or on any lookup failure: the caller then plans the
//   whole workspace, never less. Exits 2 on a usage error.
//
// The CI workflow runs the copy of this script from HEAD's first parent, so
// a commit cannot narrow its own push. The GitHub CLI is `gh`
// (MEERKAT_CI_PUSH_BASE_GH overrides it for the self-test's fake).

import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const CI_WORKFLOW_PATH = ".github/workflows/ci.yml";
export const CI_WORKFLOW_NAME = "CI";
export const ATTESTATION_BACKEND = "github-hosted-cargo";
const DEFAULT_DEPTH = 100;

export class PushBaseError extends Error {}

export function attestationArtifactName(commit, attempt) {
  return `meerkat-ci-attestation-${commit}-${attempt}`;
}

// Problems with a workflow run as a green main-push run of `commit`; empty
// when it qualifies.
export function runProblems(run, commit) {
  const problems = [];
  const expect = (field, actual, wanted) => {
    if (actual !== wanted) problems.push(`${field} is ${JSON.stringify(actual)}, not ${JSON.stringify(wanted)}`);
  };
  expect("workflow path", run?.path, CI_WORKFLOW_PATH);
  expect("workflow name", run?.name, CI_WORKFLOW_NAME);
  expect("event", run?.event, "push");
  expect("branch", run?.head_branch, "main");
  expect("head commit", run?.head_sha, commit);
  expect("status", run?.status, "completed");
  expect("conclusion", run?.conclusion, "success");
  if (!Number.isInteger(run?.run_attempt) || run.run_attempt < 1) {
    problems.push(`run attempt ${JSON.stringify(run?.run_attempt)} is not a positive integer`);
  }
  return problems;
}

// Problems with an attestation as the record of a green main run of
// `commit`/`tree`; empty when it qualifies. Any plan mode qualifies: a run
// that planned less than the workspace covered its own diff, which reached
// back to the base it found the same way.
export function attestationProblems(attestation, { repository, commit, tree, runId, runAttempt }) {
  const problems = [];
  const expect = (field, actual, wanted) => {
    if (actual !== wanted) problems.push(`${field} is ${JSON.stringify(actual)}, not ${JSON.stringify(wanted)}`);
  };
  expect("schema_version", attestation?.schema_version, 4);
  expect("repository", attestation?.repository, repository);
  expect("commit_sha", attestation?.commit_sha, commit);
  expect("tree_sha", attestation?.tree_sha, tree);
  expect("workflow", attestation?.workflow, CI_WORKFLOW_NAME);
  expect("workflow_run_id", attestation?.workflow_run_id, String(runId));
  expect("workflow_run_attempt", attestation?.workflow_run_attempt, String(runAttempt));
  expect("event", attestation?.event, "push");
  expect("ref", attestation?.ref, "refs/heads/main");
  expect("validation_backend", attestation?.validation_backend, ATTESTATION_BACKEND);
  expect("validation_result", attestation?.validation_result, "success");
  return problems;
}

function gh(args) {
  const command = process.env.MEERKAT_CI_PUSH_BASE_GH || "gh";
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) {
    throw new PushBaseError(`${command} ${args.join(" ")} failed: ${(result.stderr ?? "").trim() || result.error?.message || `status ${result.status}`}`);
  }
  return result.stdout;
}

function ghJson(path) {
  const text = gh(["api", path]);
  try {
    return JSON.parse(text);
  } catch {
    throw new PushBaseError(`gh api ${path} returned invalid JSON`);
  }
}

function downloadAttestation(repository, runId, artifact) {
  const scratch = mkdtempSync(join(tmpdir(), "meerkat-ci-push-base-"));
  try {
    gh(["run", "download", String(runId), "--repo", repository, "--name", artifact, "--dir", scratch]);
    return JSON.parse(readFileSync(join(scratch, "attestation.json"), "utf8"));
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

export function findPushBase({ repository, head, depth = DEFAULT_DEPTH, log = () => {} }) {
  const commits = ghJson(`repos/${repository}/commits?sha=${head}&per_page=${depth}`);
  if (!Array.isArray(commits) || commits.length === 0 || commits[0]?.sha !== head) {
    throw new PushBaseError(`the commits of ${head} could not be listed`);
  }
  const listing = ghJson(
    `repos/${repository}/actions/workflows/ci.yml/runs?branch=main&event=push&status=success&per_page=100`,
  );
  // Newest first: a later run of a commit supersedes an older one.
  const runsBySha = new Map();
  for (const run of [...(listing?.workflow_runs ?? [])].sort((a, b) => b.id - a.id)) {
    if (!runsBySha.has(run.head_sha)) runsBySha.set(run.head_sha, []);
    runsBySha.get(run.head_sha).push(run);
  }
  for (const [index, commit] of commits.entries()) {
    if (index === 0) continue;
    const tree = commit?.commit?.tree?.sha;
    for (const run of runsBySha.get(commit?.sha) ?? []) {
      const problems = runProblems(run, commit.sha);
      if (problems.length > 0) {
        log(`skipped CI run ${run.id} of ${commit.sha}: ${problems.join("; ")}`);
        continue;
      }
      const artifact = attestationArtifactName(commit.sha, run.run_attempt);
      let attestation;
      try {
        attestation = downloadAttestation(repository, run.id, artifact);
      } catch (error) {
        log(`skipped CI run ${run.id} of ${commit.sha}: no readable ${artifact} (${error.message})`);
        continue;
      }
      const attestationIssues = attestationProblems(attestation, {
        repository,
        commit: commit.sha,
        tree,
        runId: run.id,
        runAttempt: run.run_attempt,
      });
      if (attestationIssues.length > 0) {
        log(`skipped CI run ${run.id} of ${commit.sha}: ${artifact} does not qualify: ${attestationIssues.join("; ")}`);
        continue;
      }
      return { base: commit.sha, runId: run.id, runAttempt: run.run_attempt, behind: index, url: run.html_url ?? "" };
    }
  }
  throw new PushBaseError(`no green, attested main CI run among the ${commits.length - 1} newest ancestors of ${head}`);
}

function parseArgs(argv) {
  const args = { repository: "", head: "", depth: DEFAULT_DEPTH };
  for (let i = 0; i < argv.length; i += 1) {
    const value = () => {
      const next = argv[i + 1];
      if (next === undefined) throw new PushBaseError(`${argv[i]} needs a value`);
      i += 1;
      return next;
    };
    switch (argv[i]) {
      case "--repository": args.repository = value(); break;
      case "--head": args.head = value(); break;
      case "--depth": {
        args.depth = Number.parseInt(value(), 10);
        if (!Number.isInteger(args.depth) || args.depth < 2 || args.depth > 100) throw new PushBaseError("--depth needs an integer from 2 to 100");
        break;
      }
      default: throw new PushBaseError(`unknown argument: ${argv[i]}`);
    }
  }
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(args.repository)) throw new PushBaseError("--repository OWNER/REPO is required");
  if (!/^[0-9a-f]{40}$/.test(args.head)) throw new PushBaseError("--head needs a full commit sha");
  return args;
}

function main(argv) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (error) {
    process.stderr.write(`ci-push-base: ${error.message}\n`);
    return 2;
  }
  try {
    const found = findPushBase({ ...args, log: (line) => process.stderr.write(`ci-push-base: ${line}\n`) });
    process.stderr.write(
      `ci-push-base: ${found.base}, ${found.behind} commit(s) before HEAD: CI run ${found.runId} attempt ${found.runAttempt} ${found.url}\n`,
    );
    process.stdout.write(`${found.base}\n`);
    return 0;
  } catch (error) {
    if (!(error instanceof PushBaseError)) throw error;
    process.stderr.write(`ci-push-base: ${error.message}\n`);
    return 1;
  }
}

const invokedPath = process.argv[1] ? realpathSync(process.argv[1]) : "";
if (invokedPath === realpathSync(fileURLToPath(import.meta.url))) {
  process.exitCode = main(process.argv.slice(2));
}
