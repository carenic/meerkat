#!/usr/bin/env node
// The diff base ci.yml's change classification uses for a push to main.
//
// scripts/ci-push-base.mjs finds the newest ancestor of HEAD whose main CI
// run completed green with its exact-tree attestation. A pushed commit must
// not choose its own base, so this runs the finder from HEAD's first parent,
// and only when that parent is the push's payload base (github.event.before):
// history that had landed on main before the push, which is the case for a
// one-commit push and for a merge. In a push of several commits (a rebase
// merge, a direct push of a stack) HEAD's first parent is itself one of the
// pushed commits, which could carry a finder that names any base, so such a
// push plans the whole workspace. The finder's answer must be one full
// commit sha, not HEAD, and an ancestor of HEAD (in the checkout's history,
// or by GitHub's compare API where the shallow checkout lacks it); anything
// else plans the whole workspace too.
//
// usage: ci-push-plan-base.mjs --repository OWNER/REPO --payload-base SHA
//   Run in the checkout of the pushed commit. Prints the base and exits 0;
//   exits 1 when the push must plan the whole workspace (the reason on
//   stderr) and 2 on a usage error. The GitHub CLI is `gh`
//   (MEERKAT_CI_PUSH_BASE_GH overrides it for the self-test's fake).

import { spawnSync } from "node:child_process";
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const FINDER_PATH = "scripts/ci-push-base.mjs";
const FULL_SHA = /^[0-9a-f]{40}$/;

class Refusal extends Error {}

function git(args) {
  const result = spawnSync("git", args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return { ok: result.status === 0, stdout: result.stdout ?? "" };
}

function commitOf(rev) {
  const result = git(["rev-parse", "--verify", "--quiet", `${rev}^{commit}`]);
  if (!result.ok) throw new Refusal(`${rev} names no commit in this checkout`);
  return result.stdout.trim();
}

// A local yes is final (a shallow checkout only lacks edges, it never adds
// one); a local no can be the missing history, so GitHub decides.
function isAncestor(repository, base, head) {
  if (git(["merge-base", "--is-ancestor", base, head]).ok) return true;
  const command = process.env.MEERKAT_CI_PUSH_BASE_GH || "gh";
  const result = spawnSync(command, ["api", `repos/${repository}/compare/${base}...${head}`], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Refusal(`GitHub could not compare ${base} with HEAD: ${(result.stderr ?? "").trim() || result.error?.message || `status ${result.status}`}`);
  }
  let comparison;
  try {
    comparison = JSON.parse(result.stdout);
  } catch {
    throw new Refusal(`GitHub's comparison of ${base} with HEAD is not JSON`);
  }
  return comparison?.status === "ahead" && comparison?.merge_base_commit?.sha === base;
}

// The finder's stdout as a base for `head`, or a refusal.
export function checkFound(stdout, { repository, head }) {
  const lines = stdout.split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (lines.length !== 1 || !FULL_SHA.test(lines[0])) {
    throw new Refusal(`the finder printed ${JSON.stringify(stdout.slice(0, 200))}, not one full commit sha`);
  }
  const [base] = lines;
  if (base === head) throw new Refusal("the finder named HEAD itself");
  if (!isAncestor(repository, base, head)) throw new Refusal(`the finder named ${base}, which is not an ancestor of HEAD`);
  return base;
}

function planBase({ repository, payloadBase }) {
  const head = commitOf("HEAD");
  const parent = commitOf("HEAD^1");
  if (payloadBase !== parent) {
    throw new Refusal(
      `HEAD's first parent ${parent} is not the push's payload base ${payloadBase || "(none)"}: ` +
        "in a push of several commits the parent's finder is one of the pushed commits",
    );
  }
  const finder = git(["show", `${parent}:${FINDER_PATH}`]);
  if (!finder.ok) throw new Refusal(`${parent} has no ${FINDER_PATH}`);
  const scratch = mkdtempSync(join(tmpdir(), "meerkat-ci-push-plan-base-"));
  let stdout;
  try {
    const copy = join(scratch, "ci-push-base.mjs");
    writeFileSync(copy, finder.stdout);
    const result = spawnSync("node", [copy, "--repository", repository, "--head", head], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    });
    if (result.status !== 0) {
      throw new Refusal(`the finder from ${parent} named no base (${result.error?.message ?? `exit ${result.status}`})`);
    }
    stdout = result.stdout;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  return checkFound(stdout, { repository, head });
}

function parseArgs(argv) {
  const args = { repository: "", payloadBase: null };
  for (let i = 0; i < argv.length; i += 1) {
    const next = argv[i + 1];
    if (next === undefined) throw new Error(`${argv[i]} needs a value`);
    switch (argv[i]) {
      case "--repository": args.repository = next; break;
      case "--payload-base": args.payloadBase = next; break;
      default: throw new Error(`unknown argument: ${argv[i]}`);
    }
    i += 1;
  }
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(args.repository)) throw new Error("--repository OWNER/REPO is required");
  if (args.payloadBase === null) throw new Error("--payload-base is required (it may be empty)");
  return args;
}

function main(argv) {
  let args;
  try {
    args = parseArgs(argv);
  } catch (error) {
    process.stderr.write(`ci-push-plan-base: ${error.message}\n`);
    return 2;
  }
  try {
    const base = planBase(args);
    process.stdout.write(`${base}\n`);
    return 0;
  } catch (error) {
    if (!(error instanceof Refusal)) throw error;
    process.stderr.write(`ci-push-plan-base: ${error.message}; the push plans the whole workspace\n`);
    return 1;
  }
}

const invokedPath = process.argv[1] ? realpathSync(process.argv[1]) : "";
if (invokedPath === realpathSync(fileURLToPath(import.meta.url))) {
  process.exitCode = main(process.argv.slice(2));
}
