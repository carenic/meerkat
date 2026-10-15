#!/usr/bin/env node
// Contract test for scripts/ci-non-rust-readers.mjs, the pure mapping from a
// changed path list to the Rust tests that read non-Rust inputs. Each case
// pins a selection the planner relies on; the inventory is the committed
// scripts/ci-non-rust-readers.json (kept complete by
// scripts/ci-non-rust-readers-guard.mjs) or a small synthetic one.
import assert from "node:assert/strict";
import {
  READER_DEFINITION_PATHS,
  WHOLE_CHECKOUT,
  inventoryPathCovers,
  loadNonRustReaders,
  parseNonRustReaders,
  readerPackagesFor,
  readerSelectionsFor,
} from "./ci-non-rust-readers.mjs";

const inventory = loadNonRustReaders();

// Rows a path selects through its own inventory keys, leaving out the
// readers of the whole checkout, which every path selects.
const pathSpecific = (paths) =>
  readerSelectionsFor(paths, inventory)
    .map((row) => ({ ...row, reads: row.reads.filter((read) => read !== WHOLE_CHECKOUT) }))
    .filter((row) => row.reads.length > 0);
const has = (rows, pkg, target) => rows.some((row) => row.package === pkg && row.target === target);

// A docs-only change selects the rkat doc-example test, xtask's docs readers
// and the machine-schema docs alignment test.
{
  const packages = readerPackagesFor(["docs/guides/skills.mdx"], inventory);
  for (const name of ["rkat", "xtask", "meerkat-machine-schema"]) {
    assert.ok(packages.includes(name), `a docs-only change selects ${name}: ${packages.join(" ")}`);
  }
  const rows = pathSpecific(["docs/guides/skills.mdx"]);
  const rkat = rows.find((row) => row.package === "rkat" && row.target === "bin:rkat");
  assert.ok(rkat, "the rkat binary's doc-example test reads docs/");
  assert.deepEqual(rkat.tests, ["fenced_rkat_doc_examples_parse_against_clap"], "only the doc-example test, not the whole binary suite");
  assert.equal(rkat.scope, "target");
  assert.ok(has(rows, "meerkat-machine-schema", "test:docs_machine_authority_alignment"));
  assert.ok(has(rows, "xtask", "test:buildbuddy_static_lanes"));
  assert.ok(!has(rows, "rkat", "test:help_mobpack_contract"), "help_mobpack_contract reads seven named docs, not this one");
}

// A docs file a test embeds with include_str! selects that test target.
{
  const rows = pathSpecific(["docs/guides/mobpack.mdx"]);
  assert.ok(has(rows, "rkat", "test:help_mobpack_contract"));
  // The doc-example walk and the static lane walk read it too.
  assert.ok(has(rows, "rkat", "bin:rkat"));
}

// An sdks/ change selects its readers: xtask's version-parity and
// generated-type checks and the integration-test SDK lanes.
{
  const rows = pathSpecific(["sdks/python/meerkat/generated/types.py"]);
  assert.ok(has(rows, "xtask", "test:buildbuddy_static_lanes"), "buildbuddy_static_lanes reads the generated Python types");
  assert.ok(has(rows, "xtask", "lib"), "public_contracts reads sdks/python/meerkat/generated/");
  assert.ok(has(rows, "meerkat-integration-tests", "lib"), "e2e_lanes runs the Python SDK suite from sdks/python");
  const web = pathSpecific(["sdks/web/package.json"]);
  assert.ok(has(web, "xtask", "test:buildbuddy_static_lanes"));
  assert.ok(has(web, "meerkat-integration-tests", "lib"));
}

// CHANGELOG.md is read by xtask's static lane scans only. The CHANGELOG
// mentions in meerkat-mob (cold_resume_digest_cost.rs) and meerkat-session
// (event_store.rs) are comments, not reads, so they select nothing.
{
  const rows = pathSpecific(["CHANGELOG.md"]);
  assert.deepEqual(
    [...new Set(rows.map((row) => row.package))],
    ["xtask"],
    `CHANGELOG.md selects xtask only: ${JSON.stringify(rows.map((row) => `${row.package} ${row.target}`))}`,
  );
  assert.ok(has(rows, "xtask", "test:buildbuddy_static_lanes"));
}

// An unmapped non-Rust path selects nothing new: only the readers of the
// whole checkout, which every path selects.
{
  for (const path of ["notes/unmapped-for-selftest.txt", "renovate.json"]) {
    assert.deepEqual(pathSpecific([path]), [], `${path} selects no path-specific reader`);
    for (const row of readerSelectionsFor([path], inventory)) {
      assert.deepEqual(row.reads, [WHOLE_CHECKOUT], `${path}: ${row.package} ${row.target} reads the whole checkout`);
    }
  }
  const whole = readerSelectionsFor(["notes/unmapped-for-selftest.txt"], inventory);
  assert.ok(has(whole, "xtask", "test:buildbuddy_static_lanes"), "the deprecated-backend scan walks the whole checkout");
  assert.ok(has(whole, "meerkat-integration-tests", "test:phase6_cleanup_sweep"));
  assert.deepEqual(readerSelectionsFor([], inventory), [], "no changed path, no selection");
}

// A deleted input still selects its readers: the path list carries the
// deletion (and both sides of a rename, as the planner's --no-renames diff
// lists them), and selection never consults the working tree.
{
  const deleted = pathSpecific(["docs/docs.json"]);
  assert.ok(has(deleted, "rkat", "test:help_mobpack_contract"), "deleting docs/docs.json selects its include_str! reader");
  const renamed = pathSpecific(["tests/fixtures/hooks/run_override.json", "tests/fixtures/hooks/run_override_renamed.json"]);
  for (const [pkg, target] of [["rkat", "bin:rkat"], ["meerkat-core", "test:hooks_contracts"], ["meerkat-mcp-server", "lib"], ["meerkat-rest", "lib"]]) {
    assert.ok(has(renamed, pkg, target), `renaming the hooks fixture away selects ${pkg} ${target}`);
  }
  const row = renamed.find((candidate) => candidate.package === "meerkat-core");
  assert.deepEqual(row.paths, ["tests/fixtures/hooks/run_override.json"], "the selecting path is the old side");
}

// Cross-crate inputs select the reader, not only the owning crate.
{
  const rows = pathSpecific(["crates/meerkat-core/tests/fixtures/compiled_application_tool_policy_valid_v1.json"]);
  assert.ok(has(rows, "meerkat-contracts", "lib"), "meerkat-contracts embeds meerkat-core's policy fixtures");
  const specs = pathSpecific(["specs/machines/meerkat_machine/model.tla"]);
  assert.ok(has(specs, "meerkat-runtime", "test:core_apply_terminal_truth"));
  assert.ok(has(specs, "xtask", "lib"));
  const skill = pathSpecific([".claude/skills/meerkat-platform/SKILL.md"]);
  const facade = skill.find((candidate) => candidate.package === "meerkat" && candidate.target === "lib");
  assert.equal(facade?.scope, "package", "the facade embeds the platform skill in library code");
  assert.ok(has(skill, "meerkat-machine-schema", "test:docs_machine_authority_alignment"));
}

// A change to the inventory or its model selects every reader it names.
{
  const all = new Set(inventory.entries.map((entry) => `${entry.package} ${entry.target}`));
  for (const path of READER_DEFINITION_PATHS) {
    const rows = readerSelectionsFor([path], inventory);
    assert.deepEqual(new Set(rows.map((row) => `${row.package} ${row.target}`)), all, `${path} selects every reader`);
  }
}

// Pure: the same input gives the same output and is not mutated.
{
  const paths = ["docs/index.mdx", "./sdks/web/package.json", "docs/index.mdx"];
  const copy = [...paths];
  const first = readerSelectionsFor(paths, inventory);
  assert.deepEqual(readerSelectionsFor(paths, inventory), first);
  assert.deepEqual(paths, copy);
  assert.ok(first.some((row) => row.paths.includes("sdks/web/package.json")), "./ prefixes are normalized");
}

// Synthetic inventory: prefix and exact keys, the whole-checkout key, test
// filters (a reader without one needs its whole target), features, scope.
{
  const synthetic = parseNonRustReaders({
    schema_version: 1,
    paths: {
      "./": [{ package: "audit", target: "test:live", scope: "target", tests: ["live_scan"], sources: ["crates/audit/tests/live.rs"] }],
      "docs/": [{ package: "cli", target: "bin:cli", scope: "target", tests: ["doc_examples"], sources: ["crates/cli/src/main.rs"] }],
      "docs/a.mdx": [
        { package: "cli", target: "bin:cli", scope: "target", sources: ["crates/cli/src/main.rs"] },
        { package: "lib", target: "lib", scope: "package", features: ["extra"], sources: ["crates/lib/src/lib.rs"] },
      ],
    },
    not_readers: [],
  });
  assert.equal(inventoryPathCovers("docs/", "docs/b/c.mdx"), true);
  assert.equal(inventoryPathCovers("docs/", "docsx/b.mdx"), false);
  assert.equal(inventoryPathCovers("docs/a.mdx", "docs/a.mdx.bak"), false);
  assert.equal(inventoryPathCovers(WHOLE_CHECKOUT, "anything/at/all"), true);
  const only = readerSelectionsFor(["docs/b.mdx"], synthetic);
  assert.deepEqual(only.map((row) => [row.package, row.target, row.tests]), [
    ["audit", "test:live", ["live_scan"]],
    ["cli", "bin:cli", ["doc_examples"]],
  ]);
  const both = readerSelectionsFor(["docs/a.mdx"], synthetic);
  assert.equal(both.find((row) => row.package === "cli").tests, null, "an unfiltered reader widens the row to the whole target");
  assert.deepEqual(both.find((row) => row.package === "lib"), {
    package: "lib",
    target: "lib",
    scope: "package",
    tests: null,
    features: ["extra"],
    paths: ["docs/a.mdx"],
    reads: ["docs/a.mdx"],
  });
  assert.deepEqual(readerPackagesFor(["docs/a.mdx"], synthetic), ["audit", "cli", "lib"]);
}

// A malformed inventory fails loudly instead of selecting nothing.
{
  const row = { package: "cli", target: "bin:cli", scope: "target", sources: ["crates/cli/src/main.rs"] };
  const bad = [
    ["unsorted keys", { schema_version: 1, paths: { "docs/b": [row], "docs/a": [row] } }],
    ["bad target", { schema_version: 1, paths: { "docs/": [{ ...row, target: "unit" }] } }],
    ["no sources", { schema_version: 1, paths: { "docs/": [{ ...row, sources: [] }] } }],
    ["bad scope", { schema_version: 1, paths: { "docs/": [{ ...row, scope: "workspace" }] } }],
    ["climbing key", { schema_version: 1, paths: { "../docs/": [row] } }],
    ["duplicate row", { schema_version: 1, paths: { "docs/": [row, row] } }],
    ["wrong schema", { schema_version: 2, paths: {} }],
    ["not_readers without why", { schema_version: 1, paths: {}, not_readers: [{ source: "a.rs", literal: "x" }] }],
  ];
  for (const [label, document] of bad) {
    assert.throws(() => parseNonRustReaders(document), Error, label);
  }
}

console.log("ci-non-rust-readers selection contracts hold");
