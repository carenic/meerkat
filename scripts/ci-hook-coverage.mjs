#!/usr/bin/env node
// Hook coverage equivalence: does a CI lane plan run every test binary that
// the pre-push hook's cargo-test body (scripts/pre-push-unit.sh) runs, under
// the feature set the hook builds it with?
//
// The hook builds every workspace test binary in one invocation,
//   cargo nextest list --workspace -E 'kind(lib) or kind(test)'
// so each member is compiled with the features the whole workspace unifies on
// it (cargo metadata's resolve graph; on 8e1f4f21, 18 of the 53 members get
// more than their package-scoped build, meerkat-mcp's test-support among
// them), and a test target whose required-features are outside that set is
// not built at all. It then runs four lanes over those binaries:
//   unit           -E 'kind(lib)'                      default profile
//   integration    -E 'kind(test)' --profile fast      fast's default-filter
//                                                      drops the e2e_*_lane
//                                                      binaries
//   e2e-fast       -E 'binary(e2e_fast_lane)'          default profile
//   HeadCanonical  -p meerkat-mob --test cold_restart_mob_resume
//                  --features test-support --profile fast
//                  (its own, package-scoped build)
// The required CI rows replace the hook for the plan's test scope: the
// reverse-dependency closure of the changed packages, or every member when a
// global input changed. This module derives the hook's selection for that
// scope from cargo metadata and .config/nextest.toml by itself, reads each
// planned CI row the way Cargo and nextest would (packages, features, target
// flags, profile, filter), and reports every hook binary that no row runs
// under the hook's feature set, and every row that could not build. The
// planner calls it on its own result and fails the plan on any gap, so a row
// construction change cannot silently narrow what CI runs.
//
// What it does not see: the features Cargo unifies on a row's dependencies.
// A row selects its own packages with the workspace's feature set, but the
// crates below them get the features of the row's graph, which can be fewer
// than the workspace's. Binary selection is exact; dependency configuration
// is the stated residual.
//
// usage: ci-hook-coverage.mjs [--plan <file>|-] [--map]
//   Reads a plan (ci-cargo-lanes.mjs --format json) and prints the coverage
//   summary; exits 1 on a gap. --map prints every hook binary with the CI row
//   that runs it.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { readMetadata, root, workspacePackages } from "./rust-test-selector.mjs";

// How ci.yml executes each plan field. Every command fragment is pinned
// against the workflow below, so a workflow edit that changes what a row runs
// fails this check instead of drifting from it.
export const CI_ROW_JOBS = [
  { field: "unit_shards", job: "unit", targets: ["--lib", "--bins"], profile: "ci-pr" },
  { field: "unit_feature_shards", job: "unit", targets: ["--lib", "--bins"], profile: "ci-pr" },
  { field: "closure_unit_shards", job: "unit", targets: ["--lib", "--bins"], profile: "ci-pr" },
  { field: "integration_suites", job: "integration", targets: null, profile: "ci-pr" },
  { field: "closure_test_rows", job: "closure-tests", targets: null, profile: null },
];
export const CI_COMMAND_PINS = [
  "./scripts/repo-cargo nextest run ${PACKAGES} --lib --bins --profile ci-pr",
  "test_flags=(--test '*')",
  "./scripts/repo-cargo nextest run ${PACKAGES} \"${test_flags[@]}\" --profile ci-pr",
  "./scripts/repo-cargo nextest run ${PACKAGES} \"${test_flags[@]}\" --profile \"${PROFILE}\" \"${filter_args[@]}\"",
];

// The hook's lanes, as data (scripts/pre-push-unit.sh).
export const HOOK_LANES = {
  unit: { filter: "kind(lib)", profile: "default" },
  integration: { filter: "kind(test)", profile: "fast" },
  e2eFast: { package: "meerkat-integration-tests", binary: "e2e_fast_lane", profile: "default" },
  headCanonical: {
    package: "meerkat-mob",
    test: "cold_restart_mob_resume",
    features: ["test-support"],
    profile: "fast",
  },
};

const LIB_KINDS = new Set(["lib", "rlib", "dylib", "cdylib", "staticlib"]);

// nextest binary kind of a Cargo target.
function nextestKind(target) {
  if (target.kind.includes("proc-macro")) return "proc-macro";
  if (target.kind.some((kind) => LIB_KINDS.has(kind))) return "lib";
  if (target.kind.length === 1) return target.kind[0];
  return target.kind.join("+");
}

// The `default-filter` of each nextest profile. nextest's `inherits` does not
// carry a parent's default-filter (measured on 0.9.143; see
// .config/nextest.toml), so a profile without its own selects every binary.
export function profileFilters(text) {
  const filters = new Map();
  const profiles = new Set(["default"]);
  let section = null;
  for (const line of text.split("\n")) {
    const header = line.match(/^\s*(\[\[?)([^\]]+)\]\]?\s*(#.*)?$/);
    if (header) {
      const name = header[2].trim();
      const profile = header[1] === "[" ? name.match(/^profile\.([A-Za-z0-9_-]+)$/) : null;
      section = profile ? profile[1] : null;
      if (section) profiles.add(section);
      continue;
    }
    if (section === null || !/^\s*default-filter\s*=/.test(line)) continue;
    const value = line.match(/^\s*default-filter\s*=\s*'([^']*)'\s*(#.*)?$/);
    if (!value) throw new Error(`unsupported default-filter syntax in profile ${section}: ${line.trim()}`);
    filters.set(section, value[1]);
  }
  return { filters, profiles };
}

export function profileFilter(nextestText, profile) {
  const { filters, profiles } = profileFilters(nextestText);
  if (!profiles.has(profile)) throw new Error(`.config/nextest.toml has no profile ${profile}`);
  return filters.get(profile) ?? "all()";
}

// A binary-level subset of nextest's filterset language: kind(), binary(),
// package(), all(), none(), not/!, and/&, or/|/+, and parentheses, with
// exact (`=name` or plain) name matchers. A test() predicate or any other
// matcher cannot be decided per binary, so it throws instead of guessing.
export function parseFilter(expr) {
  const tokens = [];
  const pattern = /\s*(\(|\)|!|&|\||\+|[A-Za-z_]+\([^()]*\)|[A-Za-z_]+)/y;
  let index = 0;
  const source = expr.trim();
  while (index < source.length) {
    pattern.lastIndex = index;
    const match = pattern.exec(source);
    if (!match) throw new Error(`unsupported filter syntax at ${JSON.stringify(source.slice(index))}`);
    tokens.push(match[1]);
    index = pattern.lastIndex;
    while (index < source.length && /\s/.test(source[index])) index += 1;
  }
  let position = 0;
  const peek = () => tokens[position];
  const next = () => tokens[position++];
  const parseOr = () => {
    let left = parseAnd();
    while (["or", "|", "+"].includes(peek())) {
      next();
      const right = parseAnd();
      const l = left;
      left = (binary) => l(binary) || right(binary);
    }
    return left;
  };
  const parseAnd = () => {
    let left = parseUnary();
    while (["and", "&"].includes(peek())) {
      next();
      const right = parseUnary();
      const l = left;
      left = (binary) => l(binary) && right(binary);
    }
    return left;
  };
  const parseUnary = () => {
    const token = next();
    if (token === "not" || token === "!") {
      const inner = parseUnary();
      return (binary) => !inner(binary);
    }
    if (token === "(") {
      const inner = parseOr();
      if (next() !== ")") throw new Error(`unbalanced parentheses in filter ${JSON.stringify(expr)}`);
      return inner;
    }
    const call = token?.match(/^([A-Za-z_]+)\(([^()]*)\)$/);
    if (!call) throw new Error(`unsupported filter term ${JSON.stringify(token)} in ${JSON.stringify(expr)}`);
    const [, name, rawArg] = call;
    const arg = rawArg.trim();
    if (name === "all" || name === "none") {
      if (arg !== "") throw new Error(`${name}() takes no argument in ${JSON.stringify(expr)}`);
      return () => name === "all";
    }
    if (!["kind", "binary", "package"].includes(name)) {
      throw new Error(`filter predicate ${name}() cannot be decided per binary: ${JSON.stringify(expr)}`);
    }
    const exact = arg.startsWith("=") ? arg.slice(1) : arg;
    if (!/^[A-Za-z0-9_.-]+$/.test(exact)) {
      throw new Error(`unsupported name matcher ${JSON.stringify(arg)} in ${JSON.stringify(expr)}`);
    }
    return (binary) => binary[name] === exact;
  };
  const predicate = parseOr();
  if (position !== tokens.length) throw new Error(`trailing filter tokens in ${JSON.stringify(expr)}`);
  return predicate;
}

// Features a package has with `enabled` requested, following its own feature
// table (`dep:x` enables no feature; `x/y` and `x?/y` name a dependency).
export function featureClosure(pkg, enabled) {
  const result = new Set();
  const stack = [...enabled];
  while (stack.length) {
    const feature = stack.pop();
    if (result.has(feature)) continue;
    if (!Object.hasOwn(pkg.features ?? {}, feature)) {
      throw new Error(`${pkg.name} has no feature ${feature}`);
    }
    result.add(feature);
    for (const entry of pkg.features[feature]) {
      if (entry.startsWith("dep:")) continue;
      const own = entry.includes("/") ? entry.split("/", 1)[0] : entry;
      if (own.endsWith("?")) continue;
      if (Object.hasOwn(pkg.features, own)) stack.push(own);
    }
  }
  return result;
}

// The features each workspace member is compiled with in a whole-workspace
// build: cargo metadata's resolve graph (equal to `cargo tree --workspace`'s
// per-member features on 8e1f4f21).
export function workspaceFeatures(metadata, packages) {
  const nodes = new Map((metadata.resolve?.nodes ?? []).map((node) => [node.id, node]));
  return new Map(packages.map((pkg) => {
    const node = nodes.get(pkg.id);
    if (!node) throw new Error(`cargo metadata has no resolve node for ${pkg.name}`);
    return [pkg.name, new Set(node.features ?? [])];
  }));
}

function sameSet(a, b) {
  return a.size === b.size && [...a].every((item) => b.has(item));
}

// Reads `-p NAME`, `--features LIST` and `--no-default-features` the way
// Cargo does for a multi-package invocation: a feature must name its package
// unless exactly one package is selected.
function parsePackageFlags(flags, byName) {
  const tokens = flags.split(/\s+/).filter(Boolean);
  const packages = [];
  const requested = [];
  let defaults = true;
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token === "-p" || token === "--package") {
      packages.push(tokens[++index]);
    } else if (token === "--features" || token === "-F") {
      requested.push(...tokens[++index].split(",").filter(Boolean));
    } else if (token === "--no-default-features") {
      defaults = false;
    } else {
      throw new Error(`unsupported package flag ${token} in ${JSON.stringify(flags)}`);
    }
  }
  for (const name of packages) {
    if (!byName.has(name)) throw new Error(`row selects unknown package ${name}`);
  }
  const explicit = new Map(packages.map((name) => [name, new Set(defaults ? ["default"] : [])]));
  for (const feature of requested) {
    const [owner, name] = feature.includes("/") ? feature.split("/", 2) : [null, feature];
    if (owner === null && packages.length !== 1) {
      throw new Error(`unqualified feature ${feature} in a multi-package row: ${JSON.stringify(flags)}`);
    }
    const target = owner ?? packages[0];
    if (!explicit.has(target)) throw new Error(`feature ${feature} names a package the row does not select`);
    explicit.get(target).add(name);
  }
  const enabled = new Map();
  for (const [name, features] of explicit) {
    const pkg = byName.get(name);
    // A package without a `default` feature simply has no default set.
    if (!Object.hasOwn(pkg.features ?? {}, "default")) features.delete("default");
    enabled.set(name, featureClosure(pkg, features));
  }
  return { packages, enabled };
}

// The binaries a row runs: Cargo's target selection (with its
// required-features refusal for named targets), then the profile's
// default-filter and the row's own filter.
function rowBinaries(row, job, byName, nextestText) {
  const { packages, enabled } = parsePackageFlags(row.package_flags, byName);
  const targetTokens = job.targets ?? (row.test_flags ? row.test_flags.split(/\s+/).filter(Boolean) : ["--test", "*"]);
  const profile = job.profile ?? row.profile;
  if (!profile) throw new Error(`row ${row.name} names no nextest profile`);
  const profileFilterFn = parseFilter(profileFilter(nextestText, profile));
  const rowFilterFn = row.filter ? parseFilter(row.filter) : () => true;
  const selected = [];
  const errors = [];
  const add = (pkg, target) => selected.push({ pkg, target });
  for (let index = 0; index < targetTokens.length; index += 1) {
    const token = targetTokens[index];
    if (token === "--lib") {
      for (const name of packages) {
        const pkg = byName.get(name);
        for (const target of pkg.targets.filter((candidate) => ["lib", "proc-macro"].includes(nextestKind(candidate)))) {
          add(pkg, target);
        }
      }
    } else if (token === "--bins") {
      for (const name of packages) {
        const pkg = byName.get(name);
        for (const target of pkg.targets.filter((candidate) => candidate.kind.includes("bin"))) {
          if ((target["required-features"] ?? []).every((feature) => enabled.get(name).has(feature))) add(pkg, target);
        }
      }
    } else if (token === "--test") {
      const wanted = targetTokens[++index];
      const glob = wanted === "*";
      if (!glob && !/^[A-Za-z0-9_-]+$/.test(wanted)) {
        errors.push(`row ${row.name}: unsupported --test pattern ${wanted}`);
        continue;
      }
      let matched = 0;
      for (const name of packages) {
        const pkg = byName.get(name);
        for (const target of pkg.targets) {
          if (!target.kind.includes("test") || (!glob && target.name !== wanted)) continue;
          matched += 1;
          const missing = (target["required-features"] ?? []).filter((feature) => !enabled.get(name).has(feature));
          if (missing.length > 0) {
            errors.push(`row ${row.name}: ${name} test target ${target.name} requires ${missing.join(",")}; Cargo refuses the row`);
            continue;
          }
          add(pkg, target);
        }
      }
      if (matched === 0) errors.push(`row ${row.name}: no test target named ${wanted} in ${packages.join(" ")}`);
    } else {
      errors.push(`row ${row.name}: unsupported target flag ${token}`);
    }
  }
  const runs = selected.filter(({ pkg, target }) => {
    const binary = { kind: nextestKind(target), binary: target.name, package: pkg.name };
    return profileFilterFn(binary) && rowFilterFn(binary);
  });
  return { packages, enabled, runs, errors, profile };
}

// The hook's selection for `scope`, and the CI rows that cover it.
export function checkHookCoverage(plan, { metadata = readMetadata(), nextestText, workflowText } = {}) {
  const nextest = nextestText ?? readFileSync(resolve(root, ".config/nextest.toml"), "utf8");
  const workflow = workflowText ?? readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  const errors = [];
  for (const pin of CI_COMMAND_PINS) {
    if (!workflow.includes(pin)) errors.push(`ci.yml no longer runs plan rows with: ${pin}`);
  }
  const packages = workspacePackages(metadata);
  const byName = new Map(packages.map((pkg) => [pkg.name, pkg]));
  const unified = workspaceFeatures(metadata, packages);
  const scope = [...(plan.test_scope ?? [])].sort();
  for (const name of scope) {
    if (!byName.has(name)) errors.push(`test scope names unknown package ${name}`);
  }

  // The hook's binaries for the scope, keyed package::kind::binary.
  const unitFilter = parseFilter(HOOK_LANES.unit.filter);
  const integrationFilter = parseFilter(HOOK_LANES.integration.filter);
  const fastFilter = parseFilter(profileFilter(nextest, HOOK_LANES.integration.profile));
  const expected = new Map();
  for (const name of scope.filter((candidate) => byName.has(candidate))) {
    const pkg = byName.get(name);
    for (const target of pkg.targets) {
      // `test = false` targets get no test binary; unmet required-features
      // keep a target out of the workspace build.
      if (target.test === false) continue;
      if (!(target["required-features"] ?? []).every((feature) => unified.get(name).has(feature))) continue;
      const binary = { kind: nextestKind(target), binary: target.name, package: name };
      const key = `${name}::${binary.kind}::${target.name}`;
      if (unitFilter(binary)) expected.set(key, { lane: "unit", package: name });
      else if (integrationFilter(binary) && fastFilter(binary)) expected.set(key, { lane: "integration", package: name });
      else if (name === HOOK_LANES.e2eFast.package && target.name === HOOK_LANES.e2eFast.binary) {
        expected.set(key, { lane: "e2e-fast", package: name });
      }
    }
  }
  const headCanonicalExpected = scope.includes(HOOK_LANES.headCanonical.package);
  if (headCanonicalExpected) {
    const pkg = byName.get(HOOK_LANES.headCanonical.package);
    if (!pkg?.targets.some((target) => target.kind.includes("test") && target.name === HOOK_LANES.headCanonical.test)) {
      errors.push(`HeadCanonical target ${HOOK_LANES.headCanonical.test} is missing from ${HOOK_LANES.headCanonical.package}`);
    }
  }

  // What the planned rows run under the hook's feature set.
  const covered = new Map();
  let headCanonical = null;
  for (const job of CI_ROW_JOBS) {
    for (const row of plan[job.field] ?? []) {
      let read;
      try {
        read = rowBinaries(row, job, byName, nextest);
      } catch (error) {
        errors.push(`row ${row.name} (${job.job}): ${error.message}`);
        continue;
      }
      errors.push(...read.errors);
      for (const { pkg, target } of read.runs) {
        const key = `${pkg.name}::${nextestKind(target)}::${target.name}`;
        if (sameSet(read.enabled.get(pkg.name), unified.get(pkg.name)) && !covered.has(key)) {
          covered.set(key, `${job.job}: ${row.name}`);
        }
      }
      const hc = HOOK_LANES.headCanonical;
      const runsHeadCanonical = read.runs.some(({ pkg, target }) => pkg.name === hc.package && target.name === hc.test);
      if (runsHeadCanonical && read.packages.length === 1 && read.packages[0] === hc.package) {
        const fast = parseFilter(profileFilter(nextest, hc.profile));
        const scoped = featureClosure(byName.get(hc.package), new Set(["default", ...hc.features]));
        const exact = sameSet(read.enabled.get(hc.package), scoped)
          && fast({ kind: "test", binary: hc.test, package: hc.package })
          && read.runs.length === 1;
        if (exact) headCanonical = `${job.job}: ${row.name}`;
      }
    }
  }

  const missing = [...expected.keys()].filter((key) => !covered.has(key)).sort();
  if (headCanonicalExpected && !headCanonical) missing.push("HeadCanonical::meerkat-mob::cold_restart_mob_resume");
  const lanes = {};
  for (const { lane } of expected.values()) lanes[lane] = (lanes[lane] ?? 0) + 1;
  if (headCanonicalExpected) lanes.headcanonical = 1;
  return {
    scope_packages: scope.length,
    hook_binaries: expected.size + (headCanonicalExpected ? 1 : 0),
    by_lane: lanes,
    missing,
    errors,
    map: Object.fromEntries([
      ...[...expected.keys()].sort().map((key) => [key, covered.get(key) ?? null]),
      ...(headCanonicalExpected ? [["HeadCanonical::meerkat-mob::cold_restart_mob_resume", headCanonical]] : []),
    ]),
  };
}

function main() {
  const args = process.argv.slice(2);
  let planPath = "-";
  let printMap = false;
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--plan") planPath = args[++index] ?? "-";
    else if (args[index] === "--map") printMap = true;
    else throw new Error(`unknown argument: ${args[index]}`);
  }
  const plan = JSON.parse(readFileSync(planPath === "-" ? 0 : planPath, "utf8"));
  const report = checkHookCoverage(plan);
  const summary = {
    scope_packages: report.scope_packages,
    hook_binaries: report.hook_binaries,
    by_lane: report.by_lane,
    missing: report.missing,
    errors: report.errors,
  };
  process.stdout.write(`${JSON.stringify(printMap ? { ...summary, map: report.map } : summary, null, 2)}\n`);
  if (report.missing.length > 0 || report.errors.length > 0) process.exit(1);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`ci-hook-coverage: ${error?.message ?? error}\n`);
    process.exit(2);
  }
}
