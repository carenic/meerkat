#!/usr/bin/env node
// Supply-chain guard for GitHub Actions: every third-party action is pinned
// to a full commit SHA, every container image the workflows and the Bazel
// execution platforms name is pinned by digest, and Renovate never
// automerges an action update.
//
// usage: ci-action-pins.mjs [--root DIR]
//   Reads every .github/**/*.yml and *.yaml under DIR (default: the
//   repository) and renovate.json. Each `uses:` that is not a local `./`
//   action or a `docker://` image must read `OWNER/REPO[/PATH]@<40-hex sha>
//   # <ref>` on one line: the commit, and the tag or branch it was resolved
//   from, which Renovate's helpers:pinGitHubActionDigests keeps current.
//   Any other spelling of a `uses` key (quoted, inside a flow mapping, its
//   value on the next line) fails as unreadable, so none slips past, and so
//   does any key whose name a line cannot show (an explicit `?` key, an
//   alias, an escape in a quoted key);
//   comments and block scalars (`run: |`) are not read. renovate.json must
//   extend that preset, give github-actions updates a 7-day
//   minimumReleaseAge (new versions), hold their digest updates (a tag
//   that moved, which the age does not hold back) for dependency dashboard
//   approval, and end with a github-actions rule that turns automerge off,
//   after every rule that could turn it on for them; a custom.regex rule
//   does the same for what its customManagers pin, and a cargo rule for
//   each crate in NEVER_AUTOMERGED_CRATES (the YAML parser); nothing outside
//   packageRules may turn automerge on (vulnerabilityAlerts, an update
//   type's settings). A `docker://` action, a `container:` or `image:`
//   value in .github, and a `docker://` reference in the Bazel files under
//   tools/bazel/ (and .bazelrc) must name its image by `@sha256:` digest;
//   UNPINNED_IMAGES lists the exceptions, each with its reason. A step
//   using an action in PINNED_INPUTS names an exact release in its own
//   `with` (sccache-action's `version`, which would otherwise install the
//   newest sccache at run time). Every offender is reported as file:line
//   (or renovate.json: ...); exit 1 on any.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The one spelling this check reads.
const USES = /^\s*(?:-\s*)?uses:\s*(\S+)(.*)$/;
// A `uses` key in any spelling: plain, quoted, or inside a flow mapping.
const USES_KEY = /(?:^|[\s{,-])(["']?)uses\1\s*:/;
// A key whose name no line reader can see: an explicit `?` key, an alias,
// or a double-quoted key with an escape (`"u\x73es"`). Any of them fails,
// whatever it names.
const OPAQUE_KEY = new RegExp(
  String.raw`(?:^\s*(?:-\s+)*|[{,[]\s*)` +
    String.raw`(?:\?(?:\s|$)|(?:[!&]\S*\s+)*(?:"(?:[^"\\]|\\.)*\\.(?:[^"\\]|\\.)*"\s*:|\*[^\s,[\]{}]+\s*:(?:\s|$)))`,
);
// An image reference that names its digest.
const DIGEST = /@sha256:[0-9a-f]{64}$/;
// A `container:` or `image:` key with an inline value.
const IMAGE_KEY = /^\s*(?:-\s*)?(container|image):\s*(\S.*?)\s*$/;
// Image references this check accepts without a digest, and why.
export const UNPINNED_IMAGES = new Map([
  [
    "docker://europe-west1-docker.pkg.dev/king-dnn-training-dev/meerkat-ci/meerkat-ci-rust:1.94.0",
    "the self-hosted CI image is in a private registry: its digest gets pinned once registry read access " +
      "exists (the repository variable MEERKAT_BUILDBUDDY_CI_IMAGE, which overrides it at run time, too)",
  ],
]);
// Actions that would resolve a moving "latest" at run time unless an input
// names an exact release, and that input.
export const PINNED_INPUTS = new Map([["mozilla-actions/sccache-action", { input: "version", value: /^v\d+\.\d+\.\d+$/ }]]);
// A key whose value is a block scalar (`run: |`, `description: >-`).
const BLOCK_SCALAR = /^(\s*)(-\s+)?[^\s#-][^#]*?:\s*[|>][-+0-9]*\s*$/;
const PINNED = /^[^@\s]+\/[^@\s]+@[0-9a-f]{40}$/;
const REF_COMMENT = /^\s+#\s+\S+\s*$/;
const MANAGER = "github-actions";
// The manager Renovate runs renovate.json's customManagers as.
const CUSTOM_MANAGER = "custom.regex";
const PIN_PRESET = "helpers:pinGitHubActionDigests";
const MINIMUM_RELEASE_AGE = "7 days";
// Crates whose updates are never automerged: the YAML parser, which reads
// SKILL.md frontmatter that arrives from HTTP and git sources.
export const NEVER_AUTOMERGED_CRATES = ["serde-saphyr", "granit-parser"];
const CARGO_MANAGER = "cargo";

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
      if (quote === '"' && char === "\\") i += 1;
      else if (char === quote) quote = null;
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

// The value of `with.<name>` in the step whose `uses:` key is at `usesIndex`,
// or null. The step opens with a `- ` item at two columns left of its keys.
function stepInput(lines, usesIndex, name) {
  const column = lines[usesIndex].indexOf("uses:");
  const indentOf = (line) => line.length - line.trimStart().length;
  const keyColumn = (line) => /^\s*(?:- )?/.exec(line)[0].length;
  const opensStep = (line) => indentOf(line) === column - 2 && line.trimStart().startsWith("- ");
  let start = usesIndex;
  while (!opensStep(lines[start]) && start > 0 && (lines[start - 1].trim() === "" || indentOf(lines[start - 1]) >= column - 2)) {
    start -= 1;
  }
  let end = usesIndex + 1;
  while (end < lines.length && (lines[end].trim() === "" || indentOf(lines[end]) >= column)) end += 1;
  const input = new RegExp(`^\\s+${name}:\\s*(.*?)\\s*$`);
  for (let i = start; i < end; i += 1) {
    if (keyColumn(lines[i]) !== column || withoutComment(lines[i]).slice(column).trimEnd() !== "with:") continue;
    for (let j = i + 1; j < end && (lines[j].trim() === "" || indentOf(lines[j]) > column); j += 1) {
      const found = input.exec(withoutComment(lines[j]));
      if (found) return found[1].replace(/^(['"])(.*)\1$/, "$2");
    }
  }
  return null;
}

export function usesProblems(root) {
  const problems = [];
  for (const path of workflowFiles(join(root, ".github"))) {
    const lines = readFileSync(path, "utf8").split("\n");
    for (const [index, line] of yamlLines(lines.join("\n"))) {
      const where = `${relative(root, path)}:${index + 1}`;
      if (OPAQUE_KEY.test(withoutComment(line))) {
        problems.push(`${where}: a key this check cannot read (an explicit \`?\` key, an alias, or an escape in a quoted key); write keys plainly`);
        continue;
      }
      if (!USES_KEY.test(withoutComment(line))) continue;
      const match = USES.exec(line);
      if (!match || match[1].startsWith("#")) {
        problems.push(`${where}: a \`uses\` this check cannot read; write it as \`uses: OWNER/REPO@<sha> # <ref>\` on one line`);
        continue;
      }
      const token = match[1].replace(/^['"]|['"]$/g, "");
      if (token.startsWith("docker://")) {
        if (!DIGEST.test(token) && !UNPINNED_IMAGES.has(token)) {
          problems.push(`${where}: \`${token}\` names its image without an @sha256: digest`);
        }
        continue;
      }
      if (token.startsWith("./")) continue;
      if (!PINNED.test(token)) {
        problems.push(`${where}: \`${token}\` is not pinned to a full commit SHA`);
      } else if (!REF_COMMENT.test(match[2])) {
        problems.push(`${where}: \`${token}\` has no \`# <ref>\` comment naming the tag or branch it pins`);
      }
      const required = PINNED_INPUTS.get(token.split("@", 1)[0]);
      if (required) {
        const value = stepInput(lines, index, required.input);
        if (value === null || !required.value.test(value)) {
          problems.push(`${where}: \`${token.split("@", 1)[0]}\` must pin \`with.${required.input}\` to an exact release (got ${value === null ? "none" : `\`${value}\``})`);
        }
      }
    }
  }
  return problems;
}

// Does a package rule apply to a manager's updates, for at least some of
// them (true), and to all of them (all)? A rule with any criterion other
// than matchManagers applies to some updates only.
function ruleScope(rule, manager = MANAGER) {
  const managers = rule.matchManagers;
  if (Array.isArray(managers) && !managers.includes(manager)) return { some: false, all: false };
  const criteria = Object.keys(rule).filter((key) => key.startsWith("match") || key.startsWith("exclude"));
  const onlyManager = criteria.every((key) => key === "matchManagers");
  return { some: true, all: onlyManager && Array.isArray(managers) && managers.length === 1 };
}

// The image references a `container:` or `image:` value names: the value
// itself, or the quoted literals of an expression that look like one; null
// when an expression names none this check can read.
function imageReferences(value) {
  const unquoted = value.replace(/^['"]|['"]$/g, "");
  if (!unquoted.includes("${{")) return [unquoted];
  const literals = [...unquoted.matchAll(/'([^']*)'/g)].map((found) => found[1]);
  const images = literals.filter((literal) => literal !== "" && !/\s/.test(literal) && /[/:]/.test(literal));
  return images.length > 0 ? images : null;
}

export function imageProblems(root) {
  const problems = [];
  for (const path of workflowFiles(join(root, ".github"))) {
    for (const [index, line] of yamlLines(readFileSync(path, "utf8"))) {
      const match = IMAGE_KEY.exec(withoutComment(line));
      if (!match) continue;
      const where = `${relative(root, path)}:${index + 1}`;
      const images = imageReferences(match[2]);
      if (images === null) {
        problems.push(`${where}: a \`${match[1]}\` this check cannot read; name the image and its @sha256: digest literally`);
        continue;
      }
      for (const image of images) {
        if (!DIGEST.test(image) && !UNPINNED_IMAGES.has(image)) {
          problems.push(`${where}: \`${image}\` names its image without an @sha256: digest`);
        }
      }
    }
  }
  const bazel = [];
  const tools = join(root, "tools", "bazel");
  try {
    const walk = (dir) => {
      for (const name of readdirSync(dir).sort()) {
        const child = join(dir, name);
        if (statSync(child).isDirectory()) walk(child);
        else if (name === "BUILD.bazel" || name === "BUILD" || name.endsWith(".bzl")) bazel.push(child);
      }
    };
    walk(tools);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const bazelrc = join(root, ".bazelrc");
  try {
    if (statSync(bazelrc).isFile()) bazel.push(bazelrc);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  for (const path of bazel) {
    readFileSync(path, "utf8").split("\n").forEach((line, index) => {
      for (const found of line.matchAll(/docker:\/\/[^\s"',)]+/g)) {
        const image = found[0];
        if (!DIGEST.test(image) && !UNPINNED_IMAGES.has(image)) {
          problems.push(`${relative(root, path)}:${index + 1}: \`${image}\` names its image without an @sha256: digest`);
        }
      }
    });
  }
  return problems;
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

// Does a rule apply to some, or to every, cargo update of one crate? A
// package name pattern (a regex or glob) may match it, so it counts as some.
function crateRuleScope(rule, crate) {
  const managers = rule.matchManagers;
  if (Array.isArray(managers) && !managers.includes(CARGO_MANAGER)) return { some: false, all: false };
  const names = rule.matchPackageNames;
  const exact = Array.isArray(names) && names.includes(crate);
  const patterned = Array.isArray(names) && names.some((name) => /^[/!]|[*?]/.test(name));
  if (Array.isArray(names) && !exact && !patterned) return { some: false, all: false };
  const criteria = Object.keys(rule).filter((key) => key.startsWith("match") || key.startsWith("exclude"));
  const onlyManagerAndName = criteria.every((key) => key === "matchManagers" || key === "matchPackageNames");
  return {
    some: true,
    all: onlyManagerAndName && exact && Array.isArray(managers) && managers.length === 1,
  };
}

// A manager's updates are never automerged: a rule for all of them turns
// automerge off after every rule that could turn it on for some, and one
// for all of them sets the release age. `scopeOf` narrows a rule to one
// crate's updates instead.
function managerRuleProblems(rules, manager, scopeOf = (rule) => ruleScope(rule, manager)) {
  const problems = [];
  let lastOff = -1;
  let lastOn = -1;
  let age = false;
  rules.forEach((rule, index) => {
    const scope = scopeOf(rule);
    if (!scope.some) return;
    if (rule.automerge === true) lastOn = index;
    if (rule.automerge === false && scope.all) lastOff = index;
    if (scope.all && rule.minimumReleaseAge === MINIMUM_RELEASE_AGE) age = true;
  });
  if (lastOff < 0) {
    problems.push(`renovate.json: no packageRule turns automerge off for every ${manager} update`);
  } else if (lastOn > lastOff) {
    problems.push(`renovate.json: packageRules[${lastOn}] can turn automerge on for ${manager} after the rule that turns it off`);
  }
  if (!age) problems.push(`renovate.json: ${manager} updates lack a minimumReleaseAge of "${MINIMUM_RELEASE_AGE}"`);
  return problems;
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
  problems.push(...managerRuleProblems(rules, MANAGER));
  // What the customManagers pin (container digests, a tool release) is
  // never automerged either.
  if ((config.customManagers ?? []).length > 0) problems.push(...managerRuleProblems(rules, CUSTOM_MANAGER));
  // Nor is the YAML parser.
  for (const crate of NEVER_AUTOMERGED_CRATES) {
    problems.push(...managerRuleProblems(rules, `${CARGO_MANAGER} ${crate}`, (rule) => crateRuleScope(rule, crate)));
  }
  let digestHeld = -1;
  let digestReleased = -1;
  rules.forEach((rule, index) => {
    if (!ruleScope(rule).some) return;
    if (rule.dependencyDashboardApproval === false) digestReleased = index;
    if (rule.dependencyDashboardApproval === true && coversAllDigests(rule)) digestHeld = index;
  });
  if (digestHeld < 0) {
    problems.push(`renovate.json: no packageRule holds every ${MANAGER} digest update for dependency dashboard approval`);
  } else if (digestReleased > digestHeld) {
    problems.push(`renovate.json: packageRules[${digestReleased}] can release ${MANAGER} digest updates from dashboard approval`);
  }
  return problems;
}

export function checkRepository(root) {
  const problems = [...usesProblems(root), ...imageProblems(root)];
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
  process.stdout.write(
    "ci-action-pins: every third-party action is SHA-pinned, every container image digest-pinned, every sccache-action step names an exact release, and Renovate never automerges an action, what a customManager pins, or the YAML parser crates\n",
  );
  return 0;
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
