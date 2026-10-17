#!/usr/bin/env node
// Supply-chain guard for GitHub Actions: every third-party action is pinned
// to a full commit SHA, and Renovate never automerges an action update.
//
// usage: ci-action-pins.mjs [--root DIR]
//   Reads every .github/**/*.yml and *.yaml under DIR (default: the
//   repository) and renovate.json. Each `uses:` that is not a local `./`
//   action or a `docker://` image must read `OWNER/REPO[/PATH]@<40-hex sha>
//   # <ref>` on one line: the commit, and the tag or branch it was resolved
//   from, which Renovate's helpers:pinGitHubActionDigests keeps current.
//   Any other spelling of a `uses` key (quoted, inside a flow mapping, its
//   value on the next line) fails as unreadable, so none slips past;
//   comments and block scalars (`run: |`) are not read. renovate.json must
//   extend that preset, give github-actions updates a 7-day
//   minimumReleaseAge (new versions), hold their digest updates (a tag
//   that moved, which the age does not hold back) for dependency dashboard
//   approval, and end with a github-actions rule that turns automerge off,
//   after every rule that could turn it on for them; nothing outside
//   packageRules may turn automerge on (vulnerabilityAlerts, an update
//   type's settings). Every offender is reported as file:line (or
//   renovate.json: ...); exit 1 on any.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The one spelling this check reads.
const USES = /^\s*(?:-\s*)?uses:\s*(\S+)(.*)$/;
// A `uses` key in any spelling: plain, quoted, or inside a flow mapping.
const USES_KEY = /(?:^|[\s{,-])(["']?)uses\1\s*:/;
// A key whose value is a block scalar (`run: |`, `description: >-`).
const BLOCK_SCALAR = /^(\s*)(-\s+)?[^\s#-][^#]*?:\s*[|>][-+0-9]*\s*$/;
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

// The line without a trailing YAML comment (a `#` after whitespace, outside
// quotes).
function withoutComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i += 1) {
    const char = line[i];
    if (quote) {
      if (char === quote) quote = null;
    } else if (char === "'" || char === '"') {
      quote = char;
    } else if (char === "#" && (i === 0 || /\s/.test(line[i - 1]))) {
      return line.slice(0, i);
    }
  }
  return line;
}

// The lines of a YAML file outside block scalars, as [index, line].
export function* yamlLines(text) {
  const lines = text.split("\n");
  let blockKeyIndent = -1;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const indent = line.length - line.trimStart().length;
    if (blockKeyIndent >= 0) {
      if (line.trim() === "" || indent > blockKeyIndent) continue;
      blockKeyIndent = -1;
    }
    yield [index, line];
    const block = BLOCK_SCALAR.exec(withoutComment(line).trimEnd());
    if (block) blockKeyIndent = block[1].length + (block[2]?.length ?? 0);
  }
}

export function usesProblems(root) {
  const problems = [];
  for (const path of workflowFiles(join(root, ".github"))) {
    for (const [index, line] of yamlLines(readFileSync(path, "utf8"))) {
      if (!USES_KEY.test(withoutComment(line))) continue;
      const where = `${relative(root, path)}:${index + 1}`;
      const match = USES.exec(line);
      if (!match || match[1].startsWith("#")) {
        problems.push(`${where}: a \`uses\` this check cannot read; write it as \`uses: OWNER/REPO@<sha> # <ref>\` on one line`);
        continue;
      }
      const token = match[1].replace(/^['"]|['"]$/g, "");
      if (token.startsWith("./") || token.startsWith("docker://")) continue;
      if (!PINNED.test(token)) {
        problems.push(`${where}: \`${token}\` is not pinned to a full commit SHA`);
      } else if (!REF_COMMENT.test(match[2])) {
        problems.push(`${where}: \`${token}\` has no \`# <ref>\` comment naming the tag or branch it pins`);
      }
    }
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

// Every `automerge: true` below `value`, by path.
function automergePaths(value, path, out = []) {
  if (Array.isArray(value)) {
    value.forEach((item, index) => automergePaths(item, `${path}[${index}]`, out));
  } else if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      const here = path ? `${path}.${key}` : key;
      if (key === "automerge" && child === true) out.push(here);
      automergePaths(child, here, out);
    }
  }
  return out;
}

// Does a rule apply to every github-actions digest update (a moved tag)?
function coversAllDigests(rule) {
  const managers = rule.matchManagers;
  const types = rule.matchUpdateTypes;
  const criteria = Object.keys(rule).filter((key) => key.startsWith("match") || key.startsWith("exclude"));
  return (
    Array.isArray(managers) &&
    managers.includes(MANAGER) &&
    (types === undefined || (Array.isArray(types) && types.includes("digest"))) &&
    criteria.every((key) => key === "matchManagers" || key === "matchUpdateTypes")
  );
}

export function renovateProblems(config) {
  const problems = [];
  if (!(config.extends ?? []).includes(PIN_PRESET)) {
    problems.push(`renovate.json: extends lacks ${PIN_PRESET}, which keeps the action pins current`);
  }
  if (config.automerge === true) problems.push("renovate.json: automerge is on for every update");
  // packageRules' own automerge is ordered below; anything else applies to
  // github-actions updates whatever the rules say (vulnerabilityAlerts
  // settings override them).
  for (const path of automergePaths(config, "")) {
    if (path === "automerge" || /^packageRules\[\d+\]\.automerge$/.test(path)) continue;
    problems.push(`renovate.json: ${path} turns automerge on outside packageRules`);
  }
  const rules = config.packageRules ?? [];
  let lastOff = -1;
  let lastOn = -1;
  let age = false;
  let digestHeld = -1;
  let digestReleased = -1;
  rules.forEach((rule, index) => {
    const scope = ruleScope(rule);
    if (!scope.some) return;
    if (rule.automerge === true) lastOn = index;
    if (rule.automerge === false && scope.all) lastOff = index;
    if (scope.all && rule.minimumReleaseAge === MINIMUM_RELEASE_AGE) age = true;
    if (rule.dependencyDashboardApproval === false) digestReleased = index;
    if (rule.dependencyDashboardApproval === true && coversAllDigests(rule)) digestHeld = index;
  });
  if (lastOff < 0) {
    problems.push(`renovate.json: no packageRule turns automerge off for every ${MANAGER} update`);
  } else if (lastOn > lastOff) {
    problems.push(`renovate.json: packageRules[${lastOn}] can turn automerge on for ${MANAGER} after the rule that turns it off`);
  }
  if (!age) problems.push(`renovate.json: ${MANAGER} updates lack a minimumReleaseAge of "${MINIMUM_RELEASE_AGE}"`);
  if (digestHeld < 0) {
    problems.push(`renovate.json: no packageRule holds every ${MANAGER} digest update for dependency dashboard approval`);
  } else if (digestReleased > digestHeld) {
    problems.push(`renovate.json: packageRules[${digestReleased}] can release ${MANAGER} digest updates from dashboard approval`);
  }
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
