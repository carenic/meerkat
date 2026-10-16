#!/usr/bin/env node
// Supply-chain guard for GitHub Actions: every third-party action is pinned
// to a full commit SHA, and Renovate never automerges an action update.
//
// usage: ci-action-pins.mjs [--root DIR]
//   Reads every .github/**/*.yml and *.yaml under DIR (default: the
//   repository) and renovate.json. Each `uses:` that is not a local `./`
//   action or a `docker://` image must read `OWNER/REPO[/PATH]@<40-hex sha>
//   # <ref>`: the commit, and the tag or branch it was resolved from, which
//   Renovate's helpers:pinGitHubActionDigests keeps current. renovate.json
//   must extend that preset, give github-actions updates a 7-day
//   minimumReleaseAge, and end with a github-actions rule that turns
//   automerge off, after every rule that could turn it on for them. Every
//   offender is reported as file:line (or renovate.json: ...); exit 1 on
//   any.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const USES = /^\s*(?:-\s*)?uses:\s*(\S+)(.*)$/;
const PINNED = /^[^@\s]+\/[^@\s]+@[0-9a-f]{40}$/;
const REF_COMMENT = /^\s+#\s+\S+\s*$/;
const MANAGER = "github-actions";
const PIN_PRESET = "helpers:pinGitHubActionDigests";
const MINIMUM_RELEASE_AGE = "7 days";

function workflowFiles(dir) {
  const out = [];
  const walk = (path) => {
    for (const name of readdirSync(path).sort()) {
      const child = join(path, name);
      if (statSync(child).isDirectory()) walk(child);
      else if (/\.ya?ml$/.test(name)) out.push(child);
    }
  };
  walk(dir);
  return out;
}

export function usesProblems(root) {
  const problems = [];
  for (const path of workflowFiles(join(root, ".github"))) {
    const lines = readFileSync(path, "utf8").split("\n");
    lines.forEach((line, index) => {
      const match = USES.exec(line);
      if (!match) return;
      const token = match[1].replace(/^['"]|['"]$/g, "");
      if (token.startsWith("./") || token.startsWith("docker://")) return;
      const where = `${relative(root, path)}:${index + 1}`;
      if (!PINNED.test(token)) {
        problems.push(`${where}: \`${token}\` is not pinned to a full commit SHA`);
      } else if (!REF_COMMENT.test(match[2])) {
        problems.push(`${where}: \`${token}\` has no \`# <ref>\` comment naming the tag or branch it pins`);
      }
    });
  }
  return problems;
}

// Does a package rule apply to github-actions updates, for at least some
// of them (true), and to all of them (all)? A rule with any criterion other
// than matchManagers applies to some updates only.
function ruleScope(rule) {
  const managers = rule.matchManagers;
  if (Array.isArray(managers) && !managers.includes(MANAGER)) return { some: false, all: false };
  const criteria = Object.keys(rule).filter((key) => key.startsWith("match") || key.startsWith("exclude"));
  const onlyManager = criteria.every((key) => key === "matchManagers");
  return { some: true, all: onlyManager && Array.isArray(managers) && managers.length === 1 };
}

export function renovateProblems(config) {
  const problems = [];
  if (!(config.extends ?? []).includes(PIN_PRESET)) {
    problems.push(`renovate.json: extends lacks ${PIN_PRESET}, which keeps the action pins current`);
  }
  if (config.automerge === true) problems.push("renovate.json: automerge is on for every update");
  const rules = config.packageRules ?? [];
  let lastOff = -1;
  let lastOn = -1;
  let age = false;
  rules.forEach((rule, index) => {
    const scope = ruleScope(rule);
    if (!scope.some) return;
    if (rule.automerge === true) lastOn = index;
    if (rule.automerge === false && scope.all) lastOff = index;
    if (scope.all && rule.minimumReleaseAge === MINIMUM_RELEASE_AGE) age = true;
  });
  if (lastOff < 0) {
    problems.push(`renovate.json: no packageRule turns automerge off for every ${MANAGER} update`);
  } else if (lastOn > lastOff) {
    problems.push(`renovate.json: packageRules[${lastOn}] can turn automerge on for ${MANAGER} after the rule that turns it off`);
  }
  if (!age) problems.push(`renovate.json: ${MANAGER} updates lack a minimumReleaseAge of "${MINIMUM_RELEASE_AGE}"`);
  return problems;
}

export function checkRepository(root) {
  const problems = usesProblems(root);
  let config;
  try {
    config = JSON.parse(readFileSync(join(root, "renovate.json"), "utf8"));
  } catch (error) {
    return [...problems, `renovate.json: unreadable (${error.message})`];
  }
  return [...problems, ...renovateProblems(config)];
}

function main(argv) {
  let root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--root" && argv[i + 1]) root = resolve(argv[++i]);
    else {
      process.stderr.write(`ci-action-pins: unknown argument ${argv[i]}\n`);
      return 2;
    }
  }
  const problems = checkRepository(root);
  if (problems.length > 0) {
    process.stderr.write(`${problems.join("\n")}\n${problems.length} action supply-chain problem(s)\n`);
    return 1;
  }
  process.stdout.write("ci-action-pins: every third-party action is SHA-pinned and Renovate never automerges one\n");
  return 0;
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
