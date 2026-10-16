#!/usr/bin/env node
// Fail-closed contract test for scripts/ci-cargo-lanes.mjs, the changed-path
// classifier behind the GitHub-hosted Cargo PR CI. Each fixture is a changed
// path list; the assertions pin the property the old `changed-paths`
// BuildBuddy mode violated: a build-relevant change always yields lanes.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import {
  CI_ROW_STEPS,
  HOOK_SCRIPT,
  bindRowSteps,
  checkHookCoverage,
  hookModel,
  featureClosure,
  parseFilter,
  profileFilter,
  workspaceFeatures,
} from "./ci-hook-coverage.mjs";
import { readMetadata, workspacePackages } from "./rust-test-selector.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const classifier = resolve(here, "ci-cargo-lanes.mjs");
const root = resolve(here, "..");

function run(args, { input } = {}) {
  const result = spawnSync("node", [classifier, ...args], {
    cwd: root,
    encoding: "utf8",
    input,
    maxBuffer: 64 * 1024 * 1024,
  });
  return result;
}

function planFor(paths, extra = []) {
  const result = run(["--format", "json", ...extra, "--", ...paths]);
  assert.equal(result.status, 0, `classifier failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}

function assertLanes(plan, label) {
  assert.equal(plan.rust_changed, true, `${label}: rust_changed`);
  assert.ok(plan.shards.length >= 1, `${label}: at least one clippy/unit shard`);
  const covered = new Set(plan.shards.flatMap((shard) => shard.packages));
  for (const name of plan.packages) {
    assert.ok(covered.has(name), `${label}: ${name} is covered by a shard`);
  }
  for (const shard of plan.shards) {
    assert.match(shard.packages_flags ?? shard.package_flags, /^-p \S+( -p \S+)*$/, `${label}: shard flags`);
  }
  assert.ok(plan.closure.length >= plan.packages.length, `${label}: closure contains the packages`);
  for (const name of plan.packages) {
    assert.ok(plan.closure.includes(name), `${label}: closure includes ${name}`);
  }
  // Unit plan: every changed package is either in a pull-request unit shard
  // or named as deferred to the push-to-main run, never dropped. The only
  // other unit package is a chain exception that a changed trigger selected.
  const unitCovered = plan.unit_shards.flatMap((shard) => shard.packages);
  assert.deepEqual(
    [...unitCovered, ...plan.unit_deferred].sort(),
    [...new Set([...plan.packages, ...plan.unit_chain_exceptions])].sort(),
    `${label}: unit shards plus deferred packages partition the changed packages and selected exceptions`,
  );
  for (const name of plan.unit_deferred) {
    assert.equal(plan.package_model[name].heavy_chain, true, `${label}: ${name} is deferred only because it compiles meerkat-mob`);
    assert.ok(!plan.unit_chain_exceptions.includes(name), `${label}: selected exception ${name} is not deferred`);
  }
  for (const shard of plan.unit_shards) {
    assert.ok(
      shard.estimated_minutes <= plan.pr_unit_budget_minutes,
      `${label}: pull-request unit lane ${shard.name} models ${shard.estimated_minutes} min, over the ${plan.pr_unit_budget_minutes} min budget`,
    );
    for (const name of shard.packages) {
      assert.ok(
        !plan.package_model[name].heavy_chain || plan.unit_chain_exceptions.includes(name),
        `${label}: ${name} must not run its unit tests in the pull request`,
      );
    }
  }
  // An exception covers its default-feature row only: no pull-request
  // feature suite compiles meerkat-mob.
  for (const suite of plan.unit_feature_shards) {
    assert.equal(plan.package_model[suite.packages[0]].heavy_chain, false, `${label}: ${suite.name} must not run in the pull request`);
  }
  // The push-to-main plan covers the whole workspace: the packed shards
  // plus the archived (build once, partitioned run) lanes, never both.
  const archivedPackages = plan.main_archive_builds
    .filter((build) => !build.package_flags.includes("--features"))
    .map((build) => build.package_flags.replace(/^-p /, ""));
  const packed = plan.main_unit_shards.flatMap((shard) => shard.packages);
  for (const name of archivedPackages) {
    assert.ok(!packed.includes(name), `${label}: archived package ${name} is not also packed`);
  }
  assert.deepEqual(
    [...packed, ...archivedPackages].sort(),
    Object.keys(plan.package_model).sort(),
    `${label}: main unit shards and archived lanes cover every workspace package`,
  );
  assertArchivedLanes(plan, label);
  assertHookParity(plan, label);
}

const HOOK_STACK = 32 * 1024 * 1024;
const OTHER_E2E_LANES = ["e2e_auth_lane", "e2e_build_lane", "e2e_system_lane", "e2e_live_lane", "e2e_smoke_lane", "e2e_models_lane"];

// The closure-tests rows: the pre-push hook's cargo-test coverage for the
// test scope (the reverse-dependency closure plus the readers of changed
// non-Rust inputs), each row naming exactly the binaries it runs. The lib row
// exists only on a named runner; without one the plan leaves the hook's
// kind(lib) lane to the hook.
function assertHookParity(plan, label) {
  assert.deepEqual(
    plan.test_scope,
    [...new Set([...plan.closure, ...plan.reader_packages])].sort(),
    `${label}: the test scope is the closure plus the non-Rust readers`,
  );
  assert.deepEqual(plan.hook_coverage.missing, [], `${label}: every hook binary in scope has a row`);
  assert.deepEqual(plan.hook_coverage.errors, [], `${label}: every row builds`);
  const libRows = plan.closure_test_rows.filter((row) => row.lane === "lib");
  if (plan.lib_row_runner === null) {
    assert.deepEqual(libRows, [], `${label}: no lib row without a runner for it`);
    assert.deepEqual(plan.hook_retained_lanes, ["workspace unit lane"], `${label}: the hook keeps its kind(lib) lane`);
  } else {
    assert.deepEqual(plan.hook_retained_lanes, [], `${label}: a plan with a lib row leaves no lane to the hook`);
    assert.equal(plan.hook_coverage.hook_retained, 0, `${label}: and no binary`);
    for (const row of libRows) assert.equal(row.runner, plan.lib_row_runner, `${label}: the lib row runs on its runner`);
  }
  for (const row of plan.closure_test_rows.filter((candidate) => candidate.lane !== "lib")) {
    assert.equal(row.runner, undefined, `${label}: ${row.name} runs on its job's runner`);
  }
  const names = plan.closure_test_rows.map((row) => row.name);
  assert.equal(new Set(names).size, names.length, `${label}: closure test row names are unique`);
  const runs = [];
  for (const row of plan.closure_test_rows) {
    assert.ok(row.binaries.length > 0, `${label}: ${row.name} names its binaries`);
    assert.ok(["ci-unit", "ci-integration"].includes(row.profile), `${label}: ${row.name} profile ${row.profile}`);
    assert.ok(["warn", "fail"].includes(row.no_tests), `${label}: ${row.name} no-tests policy`);
    assert.equal(row.rust_min_stack, HOOK_STACK, `${label}: ${row.name} runs on the hook's stack`);
    for (const feature of row.package_flags.match(/--features (\S+)/)?.[1]?.split(",") ?? []) {
      assert.match(feature, /^[A-Za-z0-9_-]+\/[A-Za-z0-9_-]+$/, `${label}: ${row.name} names each feature's package`);
    }
    // Every row but HeadCanonical selects the whole workspace, so Cargo
    // resolves features exactly as the hook's workspace build does.
    if (row.lane !== "headcanonical") {
      assert.equal(row.package_flags, "--workspace", `${label}: ${row.name} selects the whole workspace`);
    }
    if (row.lane === "lib") {
      assert.equal(row.test_flags, "--lib", `${label}: ${row.name} runs lib tests`);
      // --workspace --lib also builds proc-macro crates' tests, which the
      // hook's kind(lib) lane does not run and the row does not name.
      assert.match(row.filter, /^kind\(lib\)( & \(.+\))?$/, `${label}: ${row.name} runs only kind(lib) binaries`);
      assert.deepEqual(row.binaries, row.packages, `${label}: ${row.name} lists one lib binary per package`);
    } else {
      // Every named target is one of the row's binaries, and the reverse (a
      // name shared by several members names all of their binaries).
      const named = [...new Set([...row.test_flags.matchAll(/--test (\S+)/g)].map((match) => match[1]))].sort();
      assert.deepEqual(named, [...new Set(row.binaries.map((binary) => binary.split("::")[1]))].sort(), `${label}: ${row.name} names exactly its binaries`);
      for (const binary of row.binaries) {
        assert.ok(row.packages.includes(binary.split("::")[0]), `${label}: ${row.name} selects the package of ${binary}`);
      }
    }
    if (row.lane !== "headcanonical") runs.push(...row.binaries);
  }
  assert.equal(new Set(runs).size, runs.length, `${label}: each hook binary runs in one row (HeadCanonical's own build aside)`);
  for (const lane of OTHER_E2E_LANES) {
    assert.ok(!runs.includes(`meerkat-integration-tests::${lane}`), `${label}: ${lane} is outside the hook's fast selection`);
  }
  assert.equal(
    plan.closure_test_rows.some((row) => row.lane === "headcanonical"),
    plan.test_scope.includes("meerkat-mob"),
    `${label}: HeadCanonical runs exactly when meerkat-mob is in scope`,
  );
  assert.equal(
    plan.closure_test_rows.some((row) => row.lane === "e2e-fast"),
    plan.test_scope.includes("meerkat-integration-tests"),
    `${label}: e2e-fast runs exactly when meerkat-integration-tests is in scope`,
  );
}

// Every archive build is executed by exactly its partitions, 1..n of n, and
// names unique archives (they name matrix rows and artifacts).
function assertArchivedLanes(plan, label) {
  const archives = plan.main_archive_builds.map((build) => build.archive);
  assert.equal(new Set(archives).size, archives.length, `${label}: archive names are unique`);
  for (const build of plan.main_archive_builds) {
    assert.match(build.archive, /^[A-Za-z0-9_-]+$/, `${label}: archive ${build.archive} is artifact-safe`);
    assert.ok(build.partitions >= 2, `${label}: ${build.name} fans out`);
    const partitions = plan.main_archive_runs
      .filter((run) => run.archive === build.archive)
      .map((run) => run.partition);
    assert.deepEqual(
      partitions,
      Array.from({ length: build.partitions }, (_, index) => `hash:${index + 1}/${build.partitions}`),
      `${label}: ${build.name} runs every partition exactly once`,
    );
  }
  assert.equal(
    plan.main_archive_runs.length,
    plan.main_archive_builds.reduce((sum, build) => sum + build.partitions, 0),
    `${label}: no partition run without its archive build`,
  );
}

// Core touch: the direct lanes are meerkat-core's and, because core is a
// trigger of meerkat-rpc's restart controls, the rpc exception lane; the
// closure is nearly the whole workspace (everything depends on core).
{
  const plan = planFor(["crates/meerkat-core/src/lib.rs"]);
  assert.equal(plan.mode, "packages");
  assert.deepEqual(plan.packages, ["meerkat-core"]);
  assertLanes(plan, "core touch");
  assert.ok(plan.closure.length >= 40, `core closure spans the workspace (${plan.closure.length})`);
  assert.ok(plan.closure.includes("meerkat-mob"), "core closure includes meerkat-mob");
  assert.ok(plan.closure.includes("rkat"), "core closure includes rkat");
  assert.equal(plan.shards.length, 1);
  assert.deepEqual(
    plan.unit_shards.map((shard) => shard.packages).sort(),
    [["meerkat-core"], ["meerkat-rpc"]],
    "core runs its unit tests in the pull request, and so does the rpc exception it selects",
  );
  assert.deepEqual(plan.unit_chain_exceptions, ["meerkat-rpc"]);
  assert.deepEqual(plan.unit_deferred, []);
  assert.equal(plan.wasm, true, "core is in the wasm runtime closure");
  assert.equal(plan.machine_authority, false);
}

// The meerkat-mob compile chain: a mob touch gets its clippy lane in the pull
// request and no pull-request unit lane; its unit tests are named as
// deferred to the push-to-main run. The chain is computed from metadata.
{
  const plan = planFor(["crates/meerkat-mob/src/lib.rs"]);
  assert.equal(plan.mode, "packages");
  assert.deepEqual(plan.packages, ["meerkat-mob"]);
  assert.equal(plan.shards.length, 1, "clippy lane for mob");
  assert.deepEqual(plan.unit_shards, [], "no pull-request unit lane may compile meerkat-mob");
  assert.deepEqual(plan.unit_deferred, ["meerkat-mob"]);
  assert.deepEqual(plan.unit_chain_exceptions, [], "a meerkat-mob change alone selects no exception");
  assert.ok(plan.package_model["meerkat-mob"].estimated_minutes > plan.pr_unit_budget_minutes, "mob's own lane models over budget");
  assertLanes(plan, "mob touch");
  for (const name of ["meerkat-mob", "meerkat-rpc", "meerkat-rest", "rkat", "meerkat-mob-mcp", "meerkat-integration-tests", "meerkat-web-runtime", "meerkat-machine-codegen"]) {
    assert.ok(plan.unit_deferred_chain.includes(name), `${name} compiles meerkat-mob in its unit lane and is in the deferred chain`);
  }
  // xtask reaches meerkat-mob only through meerkat-machine-codegen's
  // dev-dependency, which Cargo never builds for xtask: not in the chain.
  for (const name of ["meerkat-core", "meerkat-runtime", "meerkat-session", "meerkat", "xtask", "machine-dsl-tests"]) {
    assert.ok(!plan.unit_deferred_chain.includes(name), `${name} does not compile meerkat-mob in its unit lane`);
  }
}
{
  // A session change also selects the rpc exception lane; rkat stays deferred.
  const plan = planFor(["crates/meerkat-cli/src/main.rs", "crates/meerkat-session/src/lib.rs"]);
  assert.deepEqual(plan.unit_deferred, ["rkat"]);
  assert.deepEqual(plan.unit_shards.map((shard) => shard.packages).sort(), [["meerkat-rpc"], ["meerkat-session"]]);
  assertLanes(plan, "cli + session");
}

// Runtime's ordinary unit tests remain in the pull request even when its
// authorization dependency has native facade fixtures in its own dev graph.
{
  const plan = planFor(["crates/meerkat-runtime/src/lib.rs"]);
  assert.equal(plan.pr_unit_budget_minutes, 17, "runtime uses the 17-minute surrogate packing cap");
  assert.deepEqual(
    plan.unit_shards.map((shard) => shard.packages).sort(),
    [["meerkat-rpc"], ["meerkat-runtime"]],
    "runtime retains its pull-request unit lane, beside the rpc exception lane it selects",
  );
  assert.deepEqual(plan.unit_deferred, [], "runtime unit tests are not deferred");
  assert.ok(!plan.unit_deferred_chain.includes("meerkat-runtime"), "runtime remains outside the mob build chain");
  const runtimeLane = plan.unit_shards.find((shard) => shard.packages.includes("meerkat-runtime"));
  assert.ok(runtimeLane.estimated_minutes <= plan.pr_unit_budget_minutes, "runtime's unit lane fits the budget");
}

// Budget by construction: every package outside the chain models under the
// pull-request unit budget on its own, so no changed-package plan can
// exceed it; so does each chain exception; every other package inside the
// chain is deferred.
{
  const plan = planFor(["Cargo.lock"]);
  for (const [name, model] of Object.entries(plan.package_model)) {
    if (!model.heavy_chain) {
      assert.ok(model.estimated_minutes <= plan.pr_unit_budget_minutes, `${name} models ${model.estimated_minutes} min alone, over budget`);
    }
  }
  assert.deepEqual(plan.unit_chain_exceptions, ["meerkat-rpc"], "a workspace plan changes every exception trigger");
  for (const name of plan.unit_chain_exceptions) {
    assert.equal(plan.package_model[name].heavy_chain, true, `${name} is an exception to the chain`);
    assert.ok(plan.package_model[name].estimated_minutes <= plan.pr_unit_budget_minutes, `${name} models ${plan.package_model[name].estimated_minutes} min alone, over budget`);
  }
  assert.deepEqual(
    plan.unit_deferred,
    plan.unit_deferred_chain.filter((name) => !plan.unit_chain_exceptions.includes(name)),
    "workspace mode defers exactly the chain less its exceptions",
  );
  // An exception compiles meerkat-mob: its row runs alone, never packed with
  // another package's compile and tests.
  for (const name of plan.unit_chain_exceptions) {
    const rows = plan.unit_shards.filter((shard) => shard.packages.includes(name));
    assert.deepEqual(rows.map((shard) => shard.packages), [[name]], `${name}'s unit row runs alone`);
    assert.equal(rows[0].package_flags, `-p ${name}`);
  }
  assert.ok(plan.unit_shards.length <= 11, `workspace unit rows stay near the shard cap: ${plan.unit_shards.length}`);
  assertLanes(plan, "workspace with exceptions");
}

// The one chain exception: meerkat-rpc's existing default-feature unit row
// (its MCP restart controls) runs in the pull request when meerkat-rpc or one
// of its trigger packages changed directly. It is packed under the budget like
// any unit lane; its feature suites and the rest of the chain stay deferred.
{
  const rpc = planFor(["crates/meerkat-rpc/src/governed_jsonl/tests.rs"]);
  assert.deepEqual(rpc.packages, ["meerkat-rpc"]);
  assert.deepEqual(rpc.unit_chain_exceptions, ["meerkat-rpc"]);
  assert.deepEqual(rpc.unit_deferred, [], "an rpc change runs rpc's unit tests in the pull request");
  assert.deepEqual(rpc.unit_shards.map((shard) => shard.package_flags), ["-p meerkat-rpc"], "one default-feature unit row");
  assert.equal(rpc.package_model["meerkat-rpc"].heavy_chain, true, "rpc still compiles meerkat-mob");
  assert.ok(rpc.unit_deferred_chain.includes("meerkat-rpc"), "rpc stays in the computed chain");
  assert.deepEqual(rpc.unit_feature_shards, [], "rpc's feature suites stay on main");
  assert.ok(rpc.unit_shards[0].estimated_minutes <= rpc.pr_unit_budget_minutes, `rpc's lane models ${rpc.unit_shards[0].estimated_minutes} min`);
  assertLanes(rpc, "rpc touch");
  // The native packages its restart controls exercise select the same row.
  for (const path of [
    "crates/meerkat/src/governed_realm/stock/mod.rs",
    "crates/meerkat-core/src/mcp_attachment.rs",
    "crates/meerkat-store/src/lib.rs",
    "crates/meerkat-sqlite/src/lib.rs",
    "crates/meerkat-runtime/src/lib.rs",
    "crates/meerkat-session/src/lib.rs",
    "crates/meerkat-mcp/src/router.rs",
    "crates/meerkat-authorization/src/grant_policy.rs",
    "crates/meerkat-authorization-contracts/src/grant.rs",
  ]) {
    const plan = planFor([path]);
    assert.deepEqual(plan.unit_chain_exceptions, ["meerkat-rpc"], `${path} selects the rpc restart controls`);
    assert.ok(plan.unit_shards.some((shard) => shard.packages.includes("meerkat-rpc")), `${path}: rpc has a pull-request unit lane`);
    assert.ok(!plan.unit_feature_shards.some((suite) => suite.packages[0] === "meerkat-rpc"), `${path}: rpc's feature suites stay on main`);
    assertLanes(plan, `${path} selects rpc`);
  }
  // Other packages, including the rest of the chain, leave rpc deferred.
  for (const path of [
    "crates/meerkat-mob/src/lib.rs",
    "crates/meerkat-mob-mcp/src/lib.rs",
    "crates/meerkat-rest/src/lib.rs",
    "crates/meerkat-cli/src/main.rs",
    "crates/meerkat-openai/src/public_live.rs",
    "crates/meerkat-tools/src/lib.rs",
    "crates/xtask/src/main.rs",
  ]) {
    const plan = planFor([path]);
    assert.deepEqual(plan.unit_chain_exceptions, [], `${path} does not select rpc's unit lane`);
    assert.ok(!plan.unit_shards.some((shard) => shard.packages.includes("meerkat-rpc")), `${path}: no rpc unit lane`);
  }
  // Two chain packages changed together: rpc runs, rest stays deferred.
  const both = planFor(["crates/meerkat-rpc/src/lib.rs", "crates/meerkat-rest/src/lib.rs"]);
  assert.deepEqual(both.unit_deferred, ["meerkat-rest"]);
  assert.deepEqual(both.unit_shards.map((shard) => shard.packages), [["meerkat-rpc"]]);
  assertLanes(both, "rpc + rest");
  // GitHub output: the rpc row is an ordinary unit matrix row.
  const github = run(["--format", "github", "--", "crates/meerkat-rpc/src/lib.rs"]);
  assert.equal(github.status, 0, github.stderr);
  const lines = Object.fromEntries(github.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(lines.unit_shard_count, "1");
  assert.equal(lines.unit_deferred, "");
  assert.deepEqual(JSON.parse(lines.unit_shard_matrix).include, [{ name: "rpc", packages: "-p meerkat-rpc" }]);
}

// Docs-only: no Cargo lanes, and the plan says so explicitly rather than
// erroring or emitting a placeholder shard.
{
  const plan = planFor(["docs/guides/skills.mdx", "CHANGELOG.md", "README.md"]);
  assert.equal(plan.rust_changed, false);
  assert.equal(plan.mode, "none");
  assert.deepEqual(plan.shards, []);
  assert.deepEqual(plan.packages, []);
  assert.equal(plan.docs_only, true);
  assert.equal(plan.generated_contract, false);
}

// Documentation inside a crate directory is not a build input either.
{
  const plan = planFor(["crates/meerkat-core/README.md"]);
  assert.equal(plan.rust_changed, false);
  assert.equal(plan.mode, "none");
}

// Cargo.lock-only: the lock changes what every crate compiles against, so
// the plan is the whole workspace, packed into bounded shards that cover
// every member.
{
  const plan = planFor(["Cargo.lock"], ["--workspace-shards", "8"]);
  assert.equal(plan.mode, "workspace");
  assertLanes(plan, "Cargo.lock");
  assert.ok(plan.packages.length >= 45, `workspace mode lists every member (${plan.packages.length})`);
  assert.deepEqual(plan.closure, plan.packages);
  // The heads of the long check chains each get a fixed Clippy shard of
  // their own (CLIPPY_HEAD_SHARDS), so the pole is one head and their
  // dependencies' feature sets repeat between runs; the rest pack into five
  // more shards.
  const heads = [
    ["rkat"],
    ["meerkat-rpc"],
    ["meerkat-mob"],
    ["meerkat"],
    ["meerkat-mob-adaptive", "meerkat-mob-mcp", "meerkat-mob-pack"],
    ["meerkat-mcp-server", "meerkat-rest", "meerkat-web-runtime"],
    ["meerkat-integration-tests", "surface-build-fixtures"],
  ];
  for (const group of heads) {
    assert.ok(
      plan.shards.some((shard) => JSON.stringify(shard.packages) === JSON.stringify(group)),
      `the workspace plan lints ${group.join(" ")} in a shard of its own`,
    );
  }
  assert.equal(plan.shards.length, heads.length + 5, `head shards plus five packed (${plan.shards.length})`);
  // Feature checks keep the weight-packed shards (--workspace-shards).
  assert.ok(new Set(plan.feature_check_jobs.map((job) => job.name.replace(/ (bazel set|matrix)$/, ""))).size <= 8);
  // Cost-balanced, not count-balanced: no shard carries more than a quarter
  // of the estimated workspace cost, so the heaviest crates spread out.
  const total = plan.shards.reduce((sum, shard) => sum + shard.estimated_cost, 0);
  for (const shard of plan.shards) {
    assert.ok(shard.estimated_cost <= total / 4, `${shard.name} is not overloaded`);
  }
  const covered = plan.shards.flatMap((shard) => shard.packages).sort();
  assert.deepEqual(covered, plan.packages, "shards partition the workspace exactly once");
  assert.equal(plan.wasm, true);
}

// Root manifest, Cargo config, nextest config, the toolchain pin, the build
// wrapper, and the workflow itself are all global.
for (const path of [
  "Cargo.toml",
  ".cargo/config.toml",
  ".config/nextest.toml",
  "rust-toolchain.toml",
  "scripts/repo-cargo",
  "scripts/cargo-agent-gate",
  "scripts/ci-cargo-lanes.mjs",
  "scripts/ci-hook-coverage.mjs",
  "scripts/ci-verify-row-binaries.mjs",
  "scripts/ci-non-rust-readers.mjs",
  // They decide when a release projection reuses its parent's evidence.
  "scripts/ci-release-evidence.mjs",
  "scripts/ci-release-projection-selftest.mjs",
  "scripts/release-projection-only.mjs",
  "scripts/release-version-pins.mjs",
  "scripts/ci-clippy-sccache-wrapper",
  ".github/workflows/ci.yml",
  ".github/actions/setup-rust-ci/action.yml",
]) {
  const plan = planFor([path]);
  assert.equal(plan.mode, "workspace", `${path} escalates to the workspace`);
  assertLanes(plan, path);
}

// A path list is never a release projection: only a --base/--head diff with
// verified evidence is (scripts/ci-release-projection-selftest.mjs).
{
  const plan = planFor(["Cargo.toml", "Cargo.lock", "CHANGELOG.md", "README.md"]);
  assert.equal(plan.release_projection, null, "a release-shaped path list keeps the ordinary plan");
  assert.equal(plan.mode, "workspace");
  assertLanes(plan, "release-shaped path list");
}

// Machine-authority-only: TLA+ models and machine specs are not Cargo
// inputs, but the governance flag must fire so machine-check-drift runs.
{
  const plan = planFor(["specs/machines/meerkat_machine/model.tla", "specs/compositions/meerkat_mob_seam/ci.cfg"]);
  assert.equal(plan.rust_changed, false);
  assert.equal(plan.mode, "none");
  assert.equal(plan.machine_authority, true);
  assert.deepEqual(plan.shards, []);
}

// Poster-only: the poster gate runs, but the change is not machine
// authority, so it cannot start the machine-authority lanes (TLC).
{
  const plan = planFor(["docs/internal/machine-posters/mob_machine.html", "scripts/machine-posters/generate-machine-posters.mjs"]);
  assert.equal(plan.rust_changed, false);
  assert.equal(plan.machine_authority, false);
  assert.equal(plan.machine_posters, true);
}

// A generated machine kernel is both machine authority and a Rust source
// owned by meerkat-machine-kernels.
{
  const plan = planFor(["crates/meerkat-machine-kernels/src/generated/meerkat.rs"]);
  assert.equal(plan.mode, "packages");
  assert.deepEqual(plan.packages, ["meerkat-machine-kernels"]);
  assert.equal(plan.machine_authority, true);
  assertLanes(plan, "generated kernel");
}

// A Rust source outside every workspace package cannot be attributed, so it
// fails closed into the workspace instead of into nothing.
{
  const plan = planFor(["examples/001-hello-meerkat-rs/main.rs"]);
  assert.equal(plan.mode, "workspace");
  assert.deepEqual(plan.unmapped_rust_paths, ["examples/001-hello-meerkat-rs/main.rs"]);
  assertLanes(plan, "unmapped .rs");
}

// A crate manifest selects its crate (and the closure through dependents).
{
  const plan = planFor(["crates/meerkat-openai/Cargo.toml", "crates/meerkat/src/help.rs"]);
  assert.equal(plan.mode, "packages");
  assert.deepEqual(plan.packages, ["meerkat", "meerkat-openai"]);
  assertLanes(plan, "manifest + facade");
  assert.ok(plan.closure.includes("meerkat-mob"));
  assert.equal(plan.shards.length, 2, "one shard per changed package below the shard cap");
}

// Many changed packages are packed into the shard cap, never dropped.
{
  const paths = [
    "crates/meerkat-core/src/lib.rs",
    "crates/meerkat-runtime/src/lib.rs",
    "crates/meerkat-mob/src/lib.rs",
    "crates/meerkat-rpc/src/lib.rs",
    "crates/meerkat-rest/src/lib.rs",
    "crates/meerkat-cli/src/main.rs",
    "crates/meerkat-session/src/lib.rs",
    "crates/meerkat-store/src/lib.rs",
    "crates/xtask/src/main.rs",
  ];
  const plan = planFor(paths, ["--max-shards", "4"]);
  assert.equal(plan.mode, "packages");
  assert.equal(plan.packages.length, 9);
  assert.equal(plan.shards.length, 4);
  assertLanes(plan, "nine packages in four shards");
}

// A pull request that changes crates/xtask runs xtask's unit lane and its
// machine-authority feature suite (main went red through #1362 because both
// were deferred).
{
  const plan = planFor(["crates/xtask/src/machines.rs"]);
  assert.deepEqual(plan.unit_deferred, [], "xtask is not deferred to main");
  assert.deepEqual(plan.unit_shards.map((shard) => shard.packages), [["xtask"]]);
  assert.ok(
    plan.unit_feature_shards.some((suite) => suite.packages[0] === "xtask" && suite.features.includes("machine-authority")),
    "the xtask machine-authority suite runs in the pull request",
  );
  for (const shard of [...plan.unit_shards, ...plan.unit_feature_shards]) {
    assert.ok(shard.estimated_minutes <= plan.pr_unit_budget_minutes, `${shard.name} fits the pull-request budget`);
  }
}

// Embedded inputs the facade compiles in through include macros select the
// facade even though they are Markdown outside its directory.
{
  const plan = planFor([".claude/skills/meerkat-platform/SKILL.md"]);
  assert.equal(plan.mode, "packages");
  assert.deepEqual(plan.packages, ["meerkat"]);
  assertLanes(plan, "embedded skill doc");
}

// Generated-contract and SDK-host flags.
{
  const plan = planFor(["artifacts/schemas/version.json", "sdks/python/meerkat/client.py"]);
  assert.equal(plan.generated_contract, true);
  assert.equal(plan.sdk_host, true);
}

// No diff base at all: the plan is the whole workspace, never nothing.
{
  const result = run(["--format", "json"]);
  assert.equal(result.status, 0, result.stderr);
  const plan = JSON.parse(result.stdout);
  assert.equal(plan.mode, "workspace");
  assertLanes(plan, "no base");
}
{
  const result = run(["--format", "json", "--base", "0000000000000000000000000000000000000000"]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).mode, "workspace");
}
{
  const result = run(["--format", "json", "--base", "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", "--head", "HEAD"]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).mode, "workspace", "unknown base escalates to the workspace");
}

// GitHub output: every key the workflow reads, and a well-formed matrix.
{
  const result = run(["--format", "github", "--", "crates/meerkat-core/src/lib.rs"]);
  assert.equal(result.status, 0, result.stderr);
  const lines = Object.fromEntries(
    result.stdout.trim().split("\n").map((line) => {
      const index = line.indexOf("=");
      return [line.slice(0, index), line.slice(index + 1)];
    }),
  );
  for (const key of [
    "rust_changed",
    "mode",
    "reason",
    "generated_contract",
    "machine_authority",
    "machine_posters",
    "wasm",
    "sdk_host",
    "bazel_graph",
    "examples_browser",
    "example_web",
    "docs_only",
    "package_count",
    "closure_count",
    "closure_flags",
    "shard_count",
    "shard_matrix",
  ]) {
    assert.ok(key in lines, `github output has ${key}`);
  }
  assert.equal(lines.rust_changed, "true");
  const matrix = JSON.parse(lines.shard_matrix);
  assert.deepEqual(matrix.include, [{ name: "core", packages: "-p meerkat-core" }]);
  assert.match(lines.closure_flags, /-p meerkat-core/);
  for (const key of ["unit_shard_count", "unit_shard_matrix", "unit_deferred", "unit_deferred_count", "main_unit_shard_count", "main_unit_shard_matrix"]) {
    assert.ok(key in lines, `github output has ${key}`);
  }
  // Core's unit row and the rpc exception row a core change selects.
  assert.equal(lines.unit_shard_count, "2");
  assert.deepEqual(
    JSON.parse(lines.unit_shard_matrix).include.map((row) => row.packages).sort(),
    ["-p meerkat-core", "-p meerkat-rpc"],
  );
  assert.equal(lines.unit_deferred, "");
  assert.ok(Number(lines.main_unit_shard_count) >= 2);
  const integrationRows = JSON.parse(lines.integration_matrix).include;
  assert.deepEqual(integrationRows.find((row) => row.name === "standalone-turn-lifecycle"), {
    name: "standalone-turn-lifecycle",
    packages: "-p meerkat",
    tests: "--test standalone_turn_lifecycle",
  });
  assert.deepEqual(integrationRows.find((row) => row.name === "rpc-configured-turn-limit"), {
    name: "rpc-configured-turn-limit",
    packages: "-p meerkat-rpc",
    tests: "--test configured_turn_limit",
  });
}
{
  const result = run(["--format", "github", "--", "crates/meerkat-mob/src/lib.rs"]);
  const lines = Object.fromEntries(result.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(lines.unit_shard_count, "0");
  assert.equal(lines.unit_deferred, "meerkat-mob");
  assert.deepEqual(JSON.parse(lines.unit_shard_matrix).include, [{ name: "none", packages: "" }]);
  assert.equal(lines.shard_count, "1");
}
{
  const result = run(["--format", "github", "--", "docs/index.mdx"]);
  const lines = Object.fromEntries(result.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.deepEqual(JSON.parse(lines.shard_matrix).include, [{ name: "none", packages: "" }]);
  assert.equal(lines.rust_changed, "false");
  // A docs-only push to main has no unit lanes: the main-unit matrix is
  // empty (no "none" shard for the job to trip over) and its count is 0.
  assert.deepEqual(JSON.parse(lines.main_unit_shard_matrix), { include: [] });
  assert.equal(lines.main_unit_shard_count, "0");
  assert.equal(lines.unit_shard_count, "0");
  assert.equal(lines.unit_deferred, "");
}
{
  const plan = planFor(["CHANGELOG.md"]);
  assert.equal(plan.rust_changed, false);
  assert.deepEqual(plan.main_unit_shards, [], "a CHANGELOG-only merge yields no main unit lanes");
  assert.deepEqual(plan.main_archive_builds, [], "a CHANGELOG-only merge yields no archived lanes");
  assert.deepEqual(plan.main_archive_runs, []);
}

// Feature-gated unit suites: the default-feature unit lanes cannot run a
// test behind a non-default feature, so every Rust-relevant push to main
// carries every suite as an extra main unit row, and a pull request carries
// the suites of a changed package that runs its unit tests there.
{
  const featureFlags = /^-p (\S+) --features [a-z0-9-]+(,[a-z0-9-]+)*$/;
  const facade = planFor(["crates/meerkat/src/experimental_gpt_live.rs"]);
  assert.ok(facade.main_feature_unit_shards.length >= 2, "main carries the feature suites");
  for (const suite of facade.main_feature_unit_shards) {
    const match = featureFlags.exec(suite.package_flags);
    assert.ok(match, `feature suite ${suite.name} flags: ${suite.package_flags}`);
    assert.deepEqual(suite.packages, [match[1]], `${suite.name} runs exactly its package`);
    assert.ok(suite.packages[0] in facade.package_model, `${suite.name} names a workspace package`);
  }
  const names = facade.main_feature_unit_shards.map((suite) => suite.name);
  assert.equal(new Set(names).size, names.length, "feature suite names are unique (they name matrix rows and artifacts)");
  assert.ok(
    facade.main_feature_unit_shards.some((suite) => suite.packages[0] === "meerkat" && suite.features.includes("openai-live")),
    "the facade's public GPT Live suite is on main",
  );
  // The facade runs its unit tests in the pull request, so its suites do too.
  assert.deepEqual(
    facade.unit_feature_shards.map((suite) => suite.packages[0]).filter((name) => name !== "meerkat"),
    [],
    "only the changed package's suites join the pull request",
  );
  assert.ok(facade.unit_feature_shards.some((suite) => suite.features.includes("openai-live")), "the facade touch runs its GPT Live suite in the pull request");
  for (const suite of facade.unit_feature_shards) {
    assert.ok(suite.estimated_minutes <= facade.pr_unit_budget_minutes, `${suite.name} fits the pull-request budget`);
  }
  const github = run(["--format", "github", "--", "crates/meerkat/src/experimental_gpt_live.rs"]);
  assert.equal(github.status, 0, github.stderr);
  const lines = Object.fromEntries(github.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  const unitRows = JSON.parse(lines.unit_shard_matrix).include;
  const mainRows = JSON.parse(lines.main_unit_shard_matrix).include;
  assert.equal(Number(lines.unit_shard_count), unitRows.length);
  assert.equal(Number(lines.main_unit_shard_count), mainRows.length);
  assert.equal(unitRows.length, facade.unit_shards.length + facade.unit_feature_shards.length);
  assert.equal(mainRows.length, facade.main_unit_shards.length + facade.main_feature_unit_shards.length);
  for (const suite of facade.main_feature_unit_shards) {
    assert.ok(mainRows.some((row) => row.name === suite.name && row.packages === suite.package_flags), `main matrix row for ${suite.name}`);
  }

  // A heavy-chain package defers its suites with its unit tests.
  const mob = planFor(["crates/meerkat-mob/src/lib.rs"]);
  assert.deepEqual(mob.unit_feature_shards, [], "no pull-request feature suite may compile meerkat-mob");
  // meerkat-mob's default lane and its feature suite run as archived lanes:
  // built once, executed in partitions.
  assert.ok(
    !mob.main_feature_unit_shards.some((suite) => suite.packages[0] === "meerkat-mob"),
    "mob's suite leaves the packed feature rows",
  );
  assert.deepEqual(
    mob.main_archive_builds.map((build) => build.package_flags),
    ["-p meerkat-mob", "-p meerkat-mob --features experimental-gpt-live,schema"],
    "mob's default lane and feature suite build archives on main",
  );
  const mobGithub = run(["--format", "github", "--", "crates/meerkat-mob/src/lib.rs"]);
  assert.equal(mobGithub.status, 0, mobGithub.stderr);
  const mobLines = Object.fromEntries(mobGithub.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(Number(mobLines.main_archive_build_count), JSON.parse(mobLines.main_archive_build_matrix).include.length);
  assert.equal(Number(mobLines.main_archive_run_count), JSON.parse(mobLines.main_archive_run_matrix).include.length);
  // An unrelated leaf change carries no pull-request suite; docs carry none anywhere.
  assert.deepEqual(planFor(["crates/meerkat-sqlite/src/lib.rs"]).unit_feature_shards, []);
  const docs = planFor(["docs/index.mdx"]);
  assert.deepEqual(docs.unit_feature_shards, []);
  assert.deepEqual(docs.main_feature_unit_shards, []);
}

// Integration suites: a directly changed trigger package runs the suite's
// tests/*.rs binaries; a leaf change and docs run none; workspace mode runs
// every suite; the github output carries the matrix rows.
{
  const names = (plan) => plan.integration_suites.map((suite) => suite.packages[0]);
  assert.deepEqual(
    names(planFor(["crates/meerkat-runtime/src/lib.rs"])),
    ["meerkat-runtime", "meerkat-machine-codegen", "meerkat", "meerkat", "meerkat-integration-tests", "meerkat-authorization"],
  );
  assert.deepEqual(
    names(planFor(["crates/meerkat-machine-schema/src/lib.rs"])),
    ["meerkat-runtime", "meerkat-machine-codegen", "meerkat-machine-kernels", "meerkat-authorization"],
    "a machine schema or DSL change runs the runtime, codegen parity, kernel oracle and authorization suites",
  );
  assert.deepEqual(
    names(planFor(["crates/meerkat-mob/src/lib.rs"])),
    ["meerkat-machine-codegen", "meerkat-integration-tests"],
    "mob runs the codegen parity suite and the gpt-live replays",
  );
  assert.deepEqual(names(planFor(["crates/meerkat-machine-codegen/tests/runtime_alphabet_parity.rs"])), ["meerkat-machine-codegen"]);
  // The store and cold-restart suites follow every store backend change.
  assert.deepEqual(names(planFor(["crates/meerkat-sqlite/src/lib.rs"])), ["meerkat"]);
  const store = planFor(["crates/meerkat-store/src/lib.rs"]);
  assert.deepEqual(names(store), ["meerkat", "meerkat"]);
  assert.equal(store.integration_suites[0].name, "meerkat-store-restart");
  // The governed realm suite follows its store, runtime and MCP owners.
  assert.equal(store.integration_suites[1].name, "meerkat-governed-realm");
  assert.match(store.integration_suites[1].package_flags, /^-p meerkat --features .*\blocal-authorization\b/);
  assert.match(store.integration_suites[1].test_flags, /^--test governed_realm$/);
  assert.match(store.integration_suites[0].package_flags, /^-p meerkat --features .*\bsession-store\b/);
  assert.match(store.integration_suites[0].test_flags, /--test cold_restart_resume_after_compaction\b/);
  assert.deepEqual(names(planFor(["crates/meerkat-tools/src/lib.rs"])), ["meerkat-authorization"]);
  assert.deepEqual(names(planFor(["docs/index.mdx"])), []);
  assert.deepEqual(
    names(planFor(["Cargo.toml"])),
    ["meerkat-runtime", "meerkat-machine-codegen", "xtask", "meerkat-machine-kernels", "meerkat", "meerkat", "meerkat", "meerkat-rpc", "meerkat-integration-tests", "meerkat-authorization"],
    "workspace mode runs every suite",
  );
  // Config layering, core construction and facade changes must execute both
  // regressions; an RPC-only change selects its own test binary.
  const limitNames = (plan) => plan.integration_suites
    .map((suite) => suite.name)
    .filter((name) => ["standalone-turn-lifecycle", "rpc-configured-turn-limit"].includes(name));
  for (const path of [
    "crates/meerkat-core/src/config.rs",
    "crates/meerkat-core/src/agent/builder.rs",
    "crates/meerkat/src/factory.rs",
    "crates/meerkat/tests/standalone_turn_lifecycle.rs",
  ]) {
    assert.deepEqual(limitNames(planFor([path])), ["standalone-turn-lifecycle", "rpc-configured-turn-limit"], `${path} runs both limit regressions`);
  }
  for (const path of [
    "crates/meerkat-rpc/src/session_runtime.rs",
    "crates/meerkat-rpc/tests/configured_turn_limit.rs",
  ]) {
    assert.deepEqual(limitNames(planFor([path])), ["rpc-configured-turn-limit"], `${path} runs the RPC limit regression`);
  }
  for (const path of ["crates/meerkat-openai/src/lib.rs", "docs/index.mdx"]) {
    assert.deepEqual(limitNames(planFor([path])), [], `${path} does not add limit regression lanes`);
  }
  // The gpt-live replays: a public Live path package or a fixture change
  // runs exactly the replay target with its feature.
  const replay = planFor(["crates/meerkat-openai/src/public_live.rs"]);
  assert.deepEqual(names(replay), ["meerkat-integration-tests"]);
  assert.equal(replay.integration_suites[0].name, "gpt-live-replay");
  assert.equal(replay.integration_suites[0].package_flags, "-p meerkat-integration-tests --features gpt-live-replay");
  assert.equal(replay.integration_suites[0].test_flags, "--test gpt_live_replay");
  const fixture = planFor(["tests/integration/fixtures/gpt_live_replay/s104.provider-stream.jsonl"]);
  assert.deepEqual(names(fixture), ["meerkat-integration-tests"], "a fixture-only change runs the replays");
  // Authorization's native model/tool loops and ordinary cost controls are
  // integration binaries, so its default-feature unit row cannot run them.
  for (const path of [
    "crates/meerkat-authorization/src/grant_policy.rs",
    "crates/meerkat-authorization/tests/native_governed_loop.rs",
    "crates/meerkat-authorization/tests/native_cost.rs",
    "crates/meerkat-authorization-contracts/src/grant.rs",
    "crates/meerkat-core/src/agent/state.rs",
    "crates/meerkat-runtime/src/store/execution_custody.rs",
    "crates/meerkat/src/session_factory.rs",
    "crates/meerkat-tools/src/lib.rs",
    "crates/meerkat-llm-core/src/lib.rs",
    "crates/meerkat-anthropic/src/lib.rs",
    "crates/meerkat-auth-core/src/lib.rs",
    "crates/meerkat-models/src/lib.rs",
    "crates/meerkat-machine-schema/src/lib.rs",
    "crates/meerkat-machine-dsl/src/lib.rs",
    "crates/meerkat-machine-dsl-core/src/lib.rs",
    "crates/meerkat-machine-derive/src/lib.rs",
    "crates/meerkat-machine-kernels/src/lib.rs",
  ]) {
    const authorization = planFor([path]);
    const selected = authorization.integration_suites.filter((suite) => suite.packages[0] === "meerkat-authorization");
    assert.equal(selected.length, 1, `${path} selects exactly one native authorization suite`);
    assert.equal(selected[0].package_flags, "-p meerkat-authorization", "ordinary native fixtures keep default features and ignored acceptance opt-in");
  }
  const authorizationGithub = run(["--format", "github", "--", "crates/meerkat-authorization/tests/native_governed_loop.rs"]);
  assert.equal(authorizationGithub.status, 0, authorizationGithub.stderr);
  const authorizationLines = Object.fromEntries(authorizationGithub.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(authorizationLines.integration_count, "1");
  assert.deepEqual(JSON.parse(authorizationLines.integration_matrix).include.map((row) => row.packages), ["-p meerkat-authorization"]);
  // xtask's integration tests pin the workflows: an xtask change or a
  // workflow-only edit (a plan with no Rust change) runs them, with the
  // feature its machines_contracts target requires.
  const xtask = planFor(["crates/xtask/src/machines.rs"]);
  assert.deepEqual(names(xtask), ["xtask"]);
  assert.equal(xtask.integration_suites[0].package_flags, "-p xtask --features machine-authority");
  const workflow = planFor([".github/workflows/nightly.yml"]);
  assert.equal(workflow.rust_changed, false);
  assert.deepEqual(names(workflow), ["xtask"], "a workflow-only edit runs the xtask workflow pins");
  // buildbuddy_static_lanes reads the SDK manifests and generated types.
  for (const path of ["sdks/web/package.json", "sdks/typescript/src/generated/types.ts", "sdks/python/meerkat/generated/types.py"]) {
    assert.ok(names(planFor([path])).includes("xtask"), `${path} runs the xtask integration suite`);
  }
  const github = run(["--format", "github", "--", "crates/meerkat-runtime/src/lib.rs"]);
  assert.equal(github.status, 0, github.stderr);
  const lines = Object.fromEntries(github.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  const rows = JSON.parse(lines.integration_matrix).include;
  assert.equal(Number(lines.integration_count), 6);
  assert.deepEqual(rows.map((row) => row.packages.split(" --features")[0]), [
    "-p meerkat-runtime",
    "-p meerkat-machine-codegen",
    "-p meerkat",
    "-p meerkat",
    "-p meerkat-integration-tests",
    "-p meerkat-authorization",
  ]);
  assert.equal(rows[3].packages, "-p meerkat --features session-store,local-authorization,mcp");
  assert.equal(rows[4].packages, "-p meerkat-integration-tests --features gpt-live-replay");
  assert.deepEqual(rows.map((row) => row.tests), [
    undefined,
    undefined,
    "--test cold_restart_resume_after_compaction --test cold_restart_resume --test persistence_contract --test storage_provider_seam --test brain_swap_surface_parity",
    "--test governed_realm",
    "--test gpt_live_replay",
    undefined,
  ]);
  const docsGithub = run(["--format", "github", "--", "docs/index.mdx"]);
  const docsLines = Object.fromEntries(docsGithub.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(docsLines.integration_count, "0");
  assert.deepEqual(JSON.parse(docsLines.integration_matrix).include, [{ name: "none", packages: "" }]);
}

// Bazel graph check selection: Bazel-relevant paths, any Cargo manifest, and
// moved or deleted Rust files select it; documentation does not.
{
  const plan = planFor(["crates/meerkat-core/BUILD.bazel"]);
  assert.equal(plan.bazel_graph, true, "a BUILD-only diff selects the Bazel graph check");
  assert.equal(plan.examples_browser, false);
}
{
  for (const path of ["MODULE.bazel", "MODULE.bazel.lock", ".bazelrc", ".bazelversion", "tools/bazel/defs.bzl", "tools/buildbuddy/README.md", "scripts/generate-bazel-rust-builds.mjs", "Cargo.lock"]) {
    assert.equal(planFor([path]).bazel_graph, true, `${path} selects the Bazel graph check`);
  }
}
{
  const plan = planFor(["docs/index.mdx", "README.md"]);
  assert.equal(plan.bazel_graph, false, "a docs-only diff does not select the Bazel graph check");
  assert.equal(plan.examples_browser, false, "a docs-only diff does not select the example suites");
  assert.equal(plan.example_web, false, "a docs-only diff does not select the example web suites");
}
{
  const plan = planFor(["crates/meerkat-core/Cargo.toml"]);
  assert.equal(plan.bazel_graph, true, "a Cargo.toml change selects the Bazel graph check");
}
{
  const plan = planFor(["crates/meerkat-core/src/lib.rs"]);
  assert.equal(plan.bazel_graph, false, "an in-place Rust edit leaves the graph unchanged");
}
{
  // A rename inside crates/: the old path no longer exists, the new one does.
  const plan = planFor(["crates/meerkat-core/src/renamed_away_for_selftest.rs", "crates/meerkat-core/src/lib.rs"]);
  assert.equal(plan.bazel_graph, true, "a crates/ rename selects the Bazel graph check");
  assert.deepEqual(plan.removed_paths, ["crates/meerkat-core/src/renamed_away_for_selftest.rs"]);
}
{
  const result = run(["--format", "github", "--", "crates/meerkat-core/BUILD.bazel"]);
  const lines = Object.fromEntries(result.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(lines.bazel_graph, "true");
  assert.equal(lines.examples_browser, "false");
}

// Example suites selection: the TypeScript and Python SDKs and example
// sources select them; example documentation does not.
{
  for (const path of ["sdks/typescript/src/index.ts", "sdks/python/meerkat/__init__.py", "examples/037-live-webrtc-web/app.js", "examples/tests/sdk-examples.test.mjs", "examples/package-lock.json"]) {
    assert.equal(planFor([path]).examples_browser, true, `${path} selects the example suites`);
  }
  assert.equal(planFor(["examples/037-live-webrtc-web/README.md"]).examples_browser, false);
  assert.equal(planFor(["crates/meerkat-core/src/lib.rs"]).examples_browser, false);
}

// Example web suites selection: the sdks/web runtime, the crates it builds
// from, and the sources of examples 031, 032 and 033 select them; their
// documentation, other examples and unrelated crates do not.
{
  // Dependency-only Rust changes must run both the browser examples and
  // WASM timer tests, whose workflow jobs share the example_web flag.
  for (const path of [
    "crates/meerkat-core/src/time_compat/wasm.rs",
    "crates/meerkat-core/src/lib.rs",
    "crates/meerkat-runtime/src/lib.rs",
    "crates/meerkat-session/src/lib.rs",
    "crates/meerkat-mob/src/lib.rs",
    "crates/meerkat-machine-kernels/src/lib.rs",
  ]) {
    const plan = planFor([path]);
    assert.equal(plan.mode, "packages", `${path} selects its owning package`);
    assert.equal(plan.packages.length, 1, `${path} does not escalate to the workspace`);
    assert.ok(plan.closure.includes("meerkat-web-runtime"), `${path} reaches the web runtime through dependencies`);
    assert.equal(plan.wasm, true, `${path} selects the WASM build`);
    assert.equal(plan.example_web, true, `${path} selects the browser and WASM timer suites`);
  }
  for (const path of [
    "sdks/web/scripts/build-wasm.mjs",
    "sdks/web/src/index.ts",
    "crates/meerkat-web-runtime/src/lib.rs",
    "crates/meerkat-contracts/src/lib.rs",
    "examples/031-wasm-mini-diplomacy-sh/web/tests/regression.test.mjs",
    "examples/032-wasm-webcm-agent/web/package.json",
    "examples/033-the-office-demo-sh/web/tests/regression.cjs",
  ]) {
    assert.equal(planFor([path]).example_web, true, `${path} selects the example web suites`);
  }
  for (const path of [
    "examples/033-the-office-demo-sh/README.md",
    "examples/037-live-webrtc-web/app.js",
    "sdks/typescript/src/index.ts",
    "crates/meerkat-core/README.md",
    "CHANGELOG.md",
  ]) {
    assert.equal(planFor([path]).example_web, false, `${path} does not select the example web suites`);
  }
  for (const path of ["crates/meerkat-rpc/src/lib.rs", "crates/meerkat-cli/src/main.rs"]) {
    const plan = planFor([path]);
    assert.equal(plan.mode, "packages", `${path} still selects its Rust package`);
    assert.ok(!plan.closure.includes("meerkat-web-runtime"), `${path} has no reverse dependency on the web runtime`);
    assert.equal(plan.example_web, false, `${path} does not select the browser and WASM timer suites`);
  }
  for (const path of ["Cargo.lock", ".cargo/config.toml"]) {
    const plan = planFor([path]);
    assert.equal(plan.mode, "workspace", `${path} selects the workspace`);
    assert.equal(plan.example_web, true, `${path} selects the browser and WASM timer suites`);
  }
  for (const path of ["sdks/web/src/index.ts", "crates/meerkat-core/src/time_compat/wasm.rs"]) {
    const result = run(["--format", "github", "--", path]);
    assert.equal(result.status, 0, result.stderr);
    const lines = Object.fromEntries(result.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
    assert.equal(lines.example_web, "true", `${path} selects the suites in GitHub output`);
  }
}

// Feature-combination checks (#1687): each clippy shard gives a Bazel-set job
// (its packages under their Bazel test feature set) and a matrix job (their
// other Bazel library/binary sets and their Make feature-matrix rows).
{
  const rows = (plan) => plan.feature_check_jobs.flatMap((job) => job.commands);
  // #1595: meerkat-mob-mcp's lib tests under Bazel's `openai-live` alone.
  const mobMcp = planFor(["crates/meerkat-mob-mcp/src/lib.rs"]);
  assert.deepEqual(
    mobMcp.feature_check_jobs.map((job) => job.name),
    mobMcp.shards.flatMap((shard) => [`${shard.name} bazel set`, `${shard.name} matrix`]).filter((name) =>
      mobMcp.feature_check_jobs.some((job) => job.name === name),
    ),
  );
  assert.equal(mobMcp.feature_check_jobs[0].name, `${mobMcp.shards[0].name} bazel set`);
  assert.equal(mobMcp.feature_check_jobs[0].commands.length, 1, "the Bazel-set job holds only the Bazel row");
  assert.equal(
    rows(mobMcp)[0],
    "-p meerkat-mob-mcp --no-default-features --features meerkat-mob-mcp/openai-live --lib --bins --tests",
  );
  // #1558: `meerkat-mob --no-default-features` from the Make feature matrix,
  // and the production library variant without `test-support`.
  const mob = planFor(["crates/meerkat-mob/src/lib.rs"]);
  assert.match(rows(mob)[0], /^-p meerkat-mob --no-default-features --features \S*meerkat-mob\/test-support\S* --lib --tests$/);
  assert.ok(rows(mob).includes("-p meerkat-mob --no-default-features"), "the Make matrix row for meerkat-mob runs");
  assert.ok(rows(mob).includes("-p meerkat-mob --no-default-features --features runtime-adapter"));
  assert.ok(
    rows(mob).some((row) => /^-p meerkat-mob --no-default-features --features \S+ --lib$/.test(row) && !row.includes("test-support")),
    "the production library set without test-support is checked",
  );
  // A production target selects its release feature set (no test-only
  // feature) under //tools/bazel:release_build; both branches are checked.
  const core = planFor(["crates/meerkat-core/src/lib.rs"]);
  const coreRows = rows(core).filter((row) => row.startsWith("-p meerkat-core ") && row.endsWith(" --lib"));
  assert.ok(
    coreRows.some((row) => !row.includes("test-support")),
    "the release-configuration feature set of meerkat-core is checked",
  );
  assert.ok(
    coreRows.some((row) => row.includes("test-support")),
    "the default-configuration feature set of meerkat-core is checked",
  );
  // A bin-only package gets no --lib; weak dependency features stay out.
  const cli = planFor(["crates/meerkat-cli/src/main.rs"]);
  assert.match(rows(cli)[0], /^-p rkat --no-default-features --features \S+ --bins --tests$/);
  assert.ok(!rows(cli)[0].includes("?"), "weak dependency features are not passed to Cargo");
  for (const row of rows(cli)) {
    assert.match(row, /^-p rkat /, `${row} belongs to the changed package`);
  }
  // A workspace plan checks every package, shard by shard (the weight-packed
  // shards, not the Clippy head split), and runs every Make feature-matrix
  // check row with the shard that holds its package.
  const workspace = planFor(["Cargo.toml"]);
  assert.deepEqual(
    workspace.feature_check_shards.flatMap((shard) => shard.packages).sort(),
    workspace.packages,
    "feature-check shards partition the workspace",
  );
  assert.ok(workspace.feature_check_jobs.length <= 2 * workspace.feature_check_shards.length);
  for (const job of workspace.feature_check_jobs) {
    const shard = workspace.feature_check_shards.find(
      (candidate) => job.name === `${candidate.name} bazel set` || job.name === `${candidate.name} matrix`,
    );
    assert.ok(shard, `feature-check job ${job.name} follows a clippy shard`);
    for (const row of job.commands) {
      for (const [, name] of row.matchAll(/-p (\S+)/g)) {
        assert.ok(shard.packages.includes(name), `${job.name}: ${name} belongs to the shard`);
      }
    }
  }
  const bazelChecked = new Set(
    workspace.feature_check_jobs
      .filter((job) => job.name.endsWith(" bazel set"))
      .flatMap((job) => [...job.commands[0].matchAll(/-p (\S+)/g)].map((match) => match[1])),
  );
  for (const name of ["meerkat", "meerkat-mob", "meerkat-mob-mcp", "rkat", "meerkat-core"]) {
    assert.ok(bazelChecked.has(name), `the workspace plan checks ${name} under its Bazel test set`);
  }
  for (const row of [
    "-p meerkat-client --no-default-features",
    "-p meerkat-tools --no-default-features --features comms,mcp",
    "-p meerkat --no-default-features --features session-store,meerkat-store/jsonl",
  ]) {
    assert.ok(rows(workspace).includes(row), `workspace plan runs the Make row ${row}`);
  }
  // A plan without a Rust change has no feature checks; GitHub output keeps
  // a well-formed placeholder matrix behind a zero count.
  const docs = planFor(["docs/guides/deploying.mdx"]);
  assert.deepEqual(docs.feature_check_jobs, []);
  const github = run(["--format", "github", "--", "docs/guides/deploying.mdx"]);
  assert.equal(github.status, 0, github.stderr);
  const lines = Object.fromEntries(github.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(lines.feature_check_count, "0");
  assert.deepEqual(JSON.parse(lines.feature_check_matrix).include, [{ name: "none", commands: "" }]);
}

// A change to the lane definitions runs the lanes they define.
{
  for (const path of [
    ".github/workflows/ci.yml",
    "scripts/ci-cargo-lanes.mjs",
    "scripts/ci-cargo-lanes-selftest.mjs",
    "scripts/ci-hook-coverage.mjs",
    "scripts/ci-verify-row-binaries.mjs",
    "scripts/ci-non-rust-readers.mjs",
  ]) {
    const plan = planFor([path]);
    assert.equal(plan.bazel_graph, true, `${path} selects the Bazel graph check`);
    assert.equal(plan.examples_browser, true, `${path} selects the example suites`);
    assert.equal(plan.example_web, true, `${path} selects the example web suites`);
  }
}

// Hook parity: workspace-unified features. meerkat-mcp's form_elicitation
// and lifecycle_contract require test-support, which only the workspace's
// feature unification enables (a package-scoped build has none), so their
// row must carry it. The mob chain's lib tests run before the merge, never
// only on main: in the hook's kind(lib) lane without a lib-row runner, in the
// pull request's lib row with one.
{
  const plan = planFor(["crates/meerkat-mcp/src/lib.rs"]);
  assertLanes(plan, "mcp touch");
  const row = plan.closure_test_rows.find((candidate) => candidate.binaries.includes("meerkat-mcp::form_elicitation"));
  assert.ok(row, "a row runs meerkat-mcp's form_elicitation");
  assert.ok(row.binaries.includes("meerkat-mcp::lifecycle_contract"), "and lifecycle_contract");
  assert.equal(row.package_flags, "--workspace", "under the workspace's resolution, which enables test-support");
  assert.equal(
    checkHookCoverage(plan).map["meerkat-mob::lib::meerkat_mob"],
    "hook: workspace unit lane",
    "without a lib-row runner, the hook runs meerkat-mob's lib tests",
  );
  const withRunner = planFor(["crates/meerkat-mcp/src/lib.rs"], ["--lib-row-runner", "ubuntu-24.04"]);
  assertLanes(withRunner, "mcp touch, lib-row runner");
  assert.ok(
    withRunner.closure_test_rows.some((candidate) => candidate.lane === "lib" && candidate.packages.includes("meerkat-mob")),
    "with one, meerkat-mob's lib tests run in the pull request",
  );
}

// The hook's named lanes keep the hook's own flags.
{
  const plan = planFor(["crates/meerkat-mob/src/lib.rs"]);
  assertLanes(plan, "mob touch");
  const pick = (lane) => {
    const row = plan.closure_test_rows.find((candidate) => candidate.lane === lane);
    assert.ok(row, `the mob plan has a ${lane} row`);
    return { packages: row.package_flags, tests: row.test_flags, profile: row.profile, filter: row.filter, no_tests: row.no_tests };
  };
  assert.deepEqual(pick("headcanonical"), {
    packages: "-p meerkat-mob --features meerkat-mob/test-support",
    tests: "--test cold_restart_mob_resume",
    profile: "ci-integration",
    filter: "binary(cold_restart_mob_resume)",
    no_tests: "fail",
  });
  assert.deepEqual(pick("e2e-fast"), {
    packages: "--workspace",
    tests: "--test e2e_fast_lane",
    profile: "ci-unit",
    filter: "binary(e2e_fast_lane)",
    no_tests: "fail",
  });
}

// Readers of non-Rust inputs: a docs-only change has no Cargo lanes but runs
// the tests that read docs/ (and the whole-checkout audits), with the
// workspace feature set, as the hook did on every push.
{
  const plan = planFor(["docs/index.mdx"]);
  assert.equal(plan.rust_changed, false);
  assert.deepEqual(plan.closure, []);
  for (const name of ["rkat", "meerkat-machine-schema", "xtask"]) {
    assert.ok(plan.reader_packages.includes(name), `docs/index.mdx selects its reader ${name}`);
  }
  assertHookParity(plan, "docs reader");
  assert.ok(plan.closure_test_rows.length > 0, "a docs change runs its readers' rows");
  const github = run(["--format", "github", "--", "docs/index.mdx"]);
  assert.equal(github.status, 0, github.stderr);
  const lines = Object.fromEntries(github.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.equal(lines.closure_test_count, String(plan.closure_test_rows.length));
  assert.equal(JSON.parse(lines.closure_test_matrix).include.length, plan.closure_test_rows.length);
  assert.equal(lines.hook_retained_lanes, "workspace unit lane", "the plan log names the lane it leaves to the hook");
  assert.equal(lines.hook_retained_count, String(plan.hook_coverage.hook_retained));
}

// The coverage checker on mutated plans: each mutation is a way a row change
// could narrow what CI runs, and each must be reported. The plan has the lib
// row (a runner named for it), so the lib mutations have a row to change.
{
  const plan = planFor(["crates/meerkat-mcp/src/lib.rs"], ["--lib-row-runner", "ubuntu-24.04"]);
  const baseline = checkHookCoverage(plan);
  assert.deepEqual([baseline.missing, baseline.errors], [[], []], "the planned rows cover the hook");
  const mutate = (change) => {
    const copy = JSON.parse(JSON.stringify(plan));
    change(copy);
    return checkHookCoverage(copy);
  };
  const mcpRow = (copy) => copy.closure_test_rows.find((row) => row.binaries.includes("meerkat-mcp::form_elicitation"));
  // A dropped row leaves its binaries uncovered.
  const dropped = mutate((copy) => {
    copy.closure_test_rows = copy.closure_test_rows.filter((row) => row !== mcpRow(copy));
  });
  assert.ok(dropped.missing.includes("meerkat-mcp::test::form_elicitation"), "a dropped row is a gap");
  // A package-scoped row is not the hook's build: Cargo refuses the
  // required-features targets, and nothing in the row counts.
  const scoped = mutate((copy) => {
    mcpRow(copy).package_flags = "-p meerkat-mcp";
  });
  assert.ok(scoped.errors.some((error) => error.includes("form_elicitation requires test-support")), "Cargo would refuse the row");
  assert.ok(scoped.missing.includes("meerkat-mcp::test::account_selection"), "the scoped set does not cover the hook");
  // A package-scoped lib row is a gap even with the workspace's own
  // features: its dependencies are not the hook's.
  const libScoped = mutate((copy) => {
    const row = copy.closure_test_rows.find((candidate) => candidate.lane === "lib");
    row.package_flags = "-p meerkat-mob --features meerkat-mob/runtime-adapter,meerkat-mob/schema,meerkat-mob/test-support";
    delete row.filter;
  });
  assert.ok(libScoped.missing.includes("meerkat-mob::lib::meerkat_mob"), "meerkat-mob's lib under its scoped set is a gap");
  // HeadCanonical under the workspace set is not the hook's HeadCanonical.
  const headCanonical = mutate((copy) => {
    const row = copy.closure_test_rows.find((candidate) => candidate.lane === "headcanonical");
    row.package_flags = "-p meerkat-mob --features meerkat-mob/test-support,meerkat-mob/schema";
  });
  assert.ok(headCanonical.missing.includes("HeadCanonical::meerkat-mob::cold_restart_mob_resume"), "HeadCanonical keeps the hook's flags");
  // A profile whose default-filter drops a binary does not run it.
  const filtered = mutate((copy) => {
    copy.closure_test_rows.find((candidate) => candidate.lane === "e2e-fast").profile = "ci-integration";
  });
  assert.ok(filtered.missing.includes("meerkat-integration-tests::test::e2e_fast_lane"), "ci-integration drops e2e_fast_lane");
  // A row's binary list is what its job checks nextest against: it must be
  // exactly what the row runs.
  const overlisted = mutate((copy) => {
    copy.closure_test_rows.find((row) => row.lane === "lib").binaries.push("meerkat-machine-derive");
  });
  assert.ok(overlisted.errors.some((error) => error.includes("lists binaries it does not run (meerkat-machine-derive)")), "a row cannot list a binary it does not run");
  const underlisted = mutate((copy) => {
    const row = copy.closure_test_rows.find((candidate) => candidate.lane === "lib");
    delete row.filter;
  });
  assert.ok(underlisted.errors.some((error) => error.includes("runs binaries it does not list")), "a row cannot run binaries it does not list");
  // A narrower scope than the closure is caught by the selftest's own
  // test-scope assertion (the checker trusts plan.test_scope).
  assert.throws(() => assertHookParity({ ...plan, test_scope: plan.packages }, "narrowed scope"));
  // Every row-executing step is bound: with no workflow, each one is missing.
  const unbound = checkHookCoverage(plan, { workflowText: "" });
  assert.equal(unbound.errors.length, CI_ROW_STEPS.length, "every row-executing step is bound");
}

// What a plan may leave to the hook: the lib binaries of a lane the hook
// runs, nothing else; and a row runs where its bound job runs.
{
  const plan = planFor(["crates/meerkat-mcp/src/lib.rs"]);
  const baseline = checkHookCoverage(plan);
  assert.deepEqual([baseline.missing, baseline.errors], [[], []], "the rows and the hook's kind(lib) lane cover the hook");
  assert.ok(baseline.hook_retained > 0, "the plan leaves lib binaries to the hook");
  const mutate = (change) => {
    const copy = JSON.parse(JSON.stringify(plan));
    change(copy);
    return checkHookCoverage(copy);
  };
  const unretained = mutate((copy) => {
    copy.hook_retained_lanes = [];
  });
  assert.ok(unretained.missing.includes("meerkat-mcp::lib::meerkat_mcp"), "a lib binary that neither runs is a gap");
  // A retained lane exempts lib binaries only: leaving the integration lane to
  // the hook exempts none of its tests, and the lib binaries lose their lane.
  const integrationKept = mutate((copy) => {
    copy.hook_retained_lanes = ["workspace integration lane"];
    copy.closure_test_rows = copy.closure_test_rows.filter((row) => !row.binaries.includes("meerkat-mcp::form_elicitation"));
  });
  assert.ok(integrationKept.missing.includes("meerkat-mcp::test::form_elicitation"), "the integration lane's tests still need rows");
  assert.ok(integrationKept.missing.includes("meerkat-mcp::lib::meerkat_mcp"), "and the lib binaries their own lane");
  const unknownLane = mutate((copy) => {
    copy.hook_retained_lanes = ["workspace lib lane"];
  });
  assert.ok(
    unknownLane.errors.some((error) => error.includes("leaves \"workspace lib lane\" to the hook, which has no such workspace lane")),
    "a lane the hook does not have is refused",
  );
  assert.ok(unknownLane.missing.includes("meerkat-mcp::lib::meerkat_mcp"), "and exempts nothing");
  const elsewhere = mutate((copy) => {
    copy.closure_test_rows[0].runner = "r1-c3d-30";
  });
  assert.ok(
    elsewhere.errors.some((error) => error.includes("names runner r1-c3d-30, but job closure-tests runs on ubuntu-24.04")),
    "a row cannot name a runner its job does not run on",
  );
  // The planner fails the same way for a lib-row runner no bound job runs on,
  // and refuses an empty label.
  const r1 = run(["--format", "json", "--lib-row-runner", "r1-c3d-30", "--", "crates/meerkat-mcp/src/lib.rs"]);
  assert.equal(r1.status, 1, "a lib row on an unbound runner fails the plan");
  assert.match(r1.stderr, /row lib names runner r1-c3d-30, but job closure-tests runs on ubuntu-24\.04/);
  const empty = run(["--format", "json", "--lib-row-runner", "", "--", "crates/meerkat-mcp/src/lib.rs"]);
  assert.equal(empty.status, 1, "an empty lib-row runner is an error");
  assert.match(empty.stderr, /--lib-row-runner requires a runner label/);
}

// The bound steps: what a closure-tests row lists is what it runs, and no
// other nextest command, selection or missing binary check can hide in the
// steps the coverage model reads. Each mutation edits the real ci.yml text.
{
  const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
  assert.deepEqual(bindRowSteps(workflow), [], "the real workflow binds");
  const mutated = (from, to) => {
    assert.equal(workflow.split(from).length, 2, `mutation anchor is unique: ${from}`);
    return bindRowSteps(workflow.replace(from, to));
  };
  const run = '          ./scripts/repo-cargo nextest run "${selection[@]}" --no-tests="${NO_TESTS}" --no-fail-fast --status-level slow --final-status-level slow\n';
  const runOnly = mutated(run, run.replace('"${selection[@]}"', `"\${selection[@]}" -E 'none()'`));
  assert.ok(runOnly.some((error) => error.includes("job closure-tests: the step runs")), "a run-only selection change is refused");
  const ownSelection = mutated(run, run.replace('"${selection[@]}"', '${PACKAGES} "${test_flags[@]}" --profile "${PROFILE}"'));
  assert.ok(ownSelection.some((error) => error.includes("job closure-tests: the step runs")), "a run that does not use the listed selection is refused");
  const narrowed = mutated(
    '          selection=(${PACKAGES} "${test_flags[@]}" --profile "${PROFILE}" "${filter_args[@]}")\n',
    '          selection=(${PACKAGES} "${test_flags[@]}" --profile "${PROFILE}" "${filter_args[@]}")\n          [[ "${NO_TESTS}" == warn ]] && filter_args=(-E "none()")\n',
  );
  assert.ok(narrowed.some((error) => error.includes("job closure-tests: the step runs")), "a later selection change is refused");
  const unchecked = mutated('          node scripts/ci-verify-row-binaries.mjs "${RUNNER_TEMP}/closure-test-list.json" ${BINARIES}\n', "");
  assert.ok(unchecked.some((error) => error.includes("job closure-tests: the step runs")), "a step without the binary check is refused");
  // An input reassigned before it is read leaves every pinned line intact:
  // the line that reassigns it names it, and is refused.
  const selectionLine = '          selection=(${PACKAGES} "${test_flags[@]}" --profile "${PROFILE}" "${filter_args[@]}")\n';
  const repackaged = mutated(selectionLine, `          PACKAGES='-p meerkat-integration-tests'\n${selectionLine}`);
  assert.ok(repackaged.some((error) => error.includes("job closure-tests: the step runs")), "a PACKAGES reassignment is refused");
  const reprofiled = mutated(selectionLine, `          export PROFILE=ci-pr\n${selectionLine}`);
  assert.ok(reprofiled.some((error) => error.includes("job closure-tests: the step runs")), "a PROFILE export is refused");
  const listLine = '          ./scripts/repo-cargo nextest list "${selection[@]}" --message-format json >"${RUNNER_TEMP}/closure-test-list.json"\n';
  const reread = mutated(listLine, `          read -r NO_TESTS <<<"warn"\n${listLine}`);
  assert.ok(reread.some((error) => error.includes("job closure-tests: the step runs")), "a NO_TESTS read is refused");
  // The test-thread stack: its MIN_STACK mapping and the line that exports it
  // are pinned, and no later line can override RUST_MIN_STACK.
  const mapping = "          NO_TESTS: ${{ matrix.no_tests }}\n          MIN_STACK: ${{ matrix.rust_min_stack }}\n";
  const remapped = mutated(mapping, "          NO_TESTS: ${{ matrix.no_tests }}\n          MIN_STACK: 1048576\n");
  assert.ok(remapped.some((error) => error.includes("step env MIN_STACK")), "an altered MIN_STACK mapping is refused");
  const stackExport = '          # The hook ran every lane on a 32 MiB test-thread stack.\n          if [[ -n "${MIN_STACK}" ]]; then export RUST_MIN_STACK="${MIN_STACK}"; fi\n';
  const reexported = mutated(stackExport, stackExport.replace('RUST_MIN_STACK="${MIN_STACK}"', "RUST_MIN_STACK=1048576"));
  assert.ok(reexported.some((error) => error.includes("job closure-tests: the step runs")), "an altered RUST_MIN_STACK export is refused");
  const unexported = mutated(stackExport, "");
  assert.ok(unexported.some((error) => error.includes("job closure-tests: the step runs")), "a removed RUST_MIN_STACK export is refused");
  const overridden = mutated(listLine, `          export RUST_MIN_STACK=1048576\n${listLine}`);
  assert.ok(overridden.some((error) => error.includes("job closure-tests: the step runs")), "a later RUST_MIN_STACK override is refused");
  const moved = mutated("      - name: Closure tests (the row's targets, workspace feature set)\n", "      - name: Closure tests (moved)\n");
  assert.ok(moved.some((error) => error.includes("job closure-tests: step")), "the rows' step is bound by name");
  const wrongMatrix = mutated(
    "      matrix: ${{ fromJSON(needs.changes.outputs.closure_test_matrix) }}\n",
    "      matrix: ${{ fromJSON(needs.changes.outputs.integration_matrix) }}\n",
  );
  assert.ok(wrongMatrix.some((error) => error.includes("its matrix is not")), "the closure-tests job runs the closure matrix");
  const otherRunner = mutated(
    "    if: ${{ needs.changes.result == 'success' && needs.changes.outputs.closure_test_count != '0' }}\n    runs-on: ubuntu-24.04\n",
    "    if: ${{ needs.changes.result == 'success' && needs.changes.outputs.closure_test_count != '0' }}\n    runs-on: ubuntu-22.04\n",
  );
  assert.ok(otherRunner.some((error) => error.includes("job closure-tests: it does not run on ubuntu-24.04")), "the job's runner is bound");
  const rewired = mutated(
    "          TESTS: ${{ matrix.tests }}\n          PROFILE: ${{ matrix.profile }}\n",
    "          TESTS: ${{ matrix.packages }}\n          PROFILE: ${{ matrix.profile }}\n",
  );
  assert.ok(rewired.some((error) => error.includes("step env TESTS")), "the step reads each selection input from its own matrix field");
  const envSelection = mutated(
    "          BINARIES: ${{ matrix.binaries }}\n",
    "          BINARIES: ${{ matrix.binaries }}\n          NEXTEST_RUN_IGNORED: only\n",
  );
  assert.ok(envSelection.some((error) => error.includes("sets a NEXTEST_ variable")), "a NEXTEST_ variable in a bound job is refused");
}

// The hook's selection is read from the hook script, not copied: a changed
// hook filter changes what CI must cover, and a shape the model cannot read
// fails closed.
{
  const hookText = readFileSync(resolve(root, HOOK_SCRIPT), "utf8");
  const hook = hookModel(hookText);
  assert.equal(hook.buildFilter, "kind(lib) or kind(test)");
  assert.deepEqual(hook.headCanonical, { package: "meerkat-mob", test: "cold_restart_mob_resume", features: ["test-support"], profile: "fast" });
  assert.equal(hook.e2eFast.binary, "e2e_fast_lane");
  assert.deepEqual(
    hook.lanes.map((lane) => [lane.label, lane.profile, lane.filter]),
    [
      ["workspace unit lane", "default", "kind(lib)"],
      ["workspace integration lane", "fast", "kind(test)"],
      ["e2e-fast lane", "default", "binary(e2e_fast_lane)"],
    ],
  );
  const plan = planFor(["Cargo.lock"]);
  assert.deepEqual(checkHookCoverage(plan, { hookText }).missing, []);
  const mutatedHook = (from, to) => {
    assert.equal(hookText.split(from).length, 2, `hook mutation anchor is unique: ${from}`);
    return hookText.replace(from, to);
  };
  // Widening only a lane's filter changes nothing (the build inventory lists
  // lib and test binaries); widening both makes bin tests hook coverage.
  const laneOnly = checkHookCoverage(plan, {
    hookText: mutatedHook("    -E 'kind(lib)' --no-tests=fail --no-fail-fast \\\n", "    -E 'kind(lib) or kind(bin)' --no-tests=fail --no-fail-fast \\\n"),
  });
  assert.deepEqual(laneOnly.missing, [], "a lane cannot run binaries the hook's build does not list");
  const widened = checkHookCoverage(plan, {
    hookText: mutatedHook("    -E 'kind(lib)' --no-tests=fail --no-fail-fast \\\n", "    -E 'kind(lib) or kind(bin)' --no-tests=fail --no-fail-fast \\\n")
      .replace("-E 'kind(lib) or kind(test)' >", "-E 'kind(lib) or kind(test) or kind(bin)' >"),
  });
  assert.ok(widened.missing.some((key) => key.includes("::bin::")), "a hook that also builds and runs bin tests needs rows for them");
  const otherLane = checkHookCoverage(plan, {
    hookText: mutatedHook("    -E 'binary(e2e_fast_lane)' --no-tests=fail --no-fail-fast \\\n", "    -E 'binary(e2e_system_lane)' --no-tests=fail --no-fail-fast \\\n"),
  });
  assert.ok(otherLane.missing.includes("meerkat-integration-tests::test::e2e_system_lane"), "a different e2e lane in the hook needs its row");
  assert.throws(
    () => checkHookCoverage(plan, { hookText: mutatedHook("    -E 'kind(lib)' --no-tests=fail --no-fail-fast \\\n", "    -E 'kind(lib) and not test(=slow)' --no-tests=fail --no-fail-fast \\\n") }),
    /cannot be decided per binary/,
    "a hook filter the model cannot decide per binary fails closed",
  );
  assert.throws(
    () => hookModel(mutatedHook("    -E 'kind(lib)' --no-tests=fail --no-fail-fast \\\n", "    -E 'kind(lib)' --no-tests=fail --no-fail-fast --run-ignored all \\\n")),
    /shape the coverage model does not read/,
    "a hook lane with an extra selection flag fails closed",
  );
}

// The matrix the closure-tests job runs is the plan's rows, field by field.
{
  const plan = planFor(["crates/meerkat-mob/src/lib.rs"]);
  const github = run(["--format", "github", "--", "crates/meerkat-mob/src/lib.rs"]);
  assert.equal(github.status, 0, github.stderr);
  const lines = Object.fromEntries(github.stdout.trim().split("\n").map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]));
  assert.deepEqual(
    JSON.parse(lines.closure_test_matrix).include,
    plan.closure_test_rows.map((row) => ({
      name: row.name,
      packages: row.package_flags,
      tests: row.test_flags,
      profile: row.profile,
      filter: row.filter ?? "",
      binaries: row.binaries.join(" "),
      no_tests: row.no_tests,
      rust_min_stack: String(row.rust_min_stack),
    })),
  );
}

// The filter subset and the profiles: ci-integration selects exactly fast's
// binaries; ci-pr and ci-unit select every binary (nextest's `inherits`
// does not carry a default-filter); a test() predicate cannot be decided per
// binary and is refused rather than guessed.
{
  const nextest = readFileSync(resolve(root, ".config/nextest.toml"), "utf8");
  assert.equal(profileFilter(nextest, "ci-integration"), profileFilter(nextest, "fast"));
  assert.equal(profileFilter(nextest, "ci-pr"), "all()");
  assert.equal(profileFilter(nextest, "ci-unit"), "all()");
  const fast = parseFilter(profileFilter(nextest, "fast"));
  assert.equal(fast({ kind: "test", binary: "e2e_fast_lane", package: "meerkat-integration-tests" }), false);
  assert.equal(fast({ kind: "test", binary: "phase1_wiring", package: "meerkat-integration-tests" }), true);
  const both = parseFilter("kind(test) & (binary(=a) | package(b))");
  assert.equal(both({ kind: "test", binary: "a", package: "x" }), true);
  assert.equal(both({ kind: "lib", binary: "a", package: "x" }), false);
  assert.equal(both({ kind: "test", binary: "c", package: "b" }), true);
  assert.throws(() => parseFilter("test(=foo)"), /cannot be decided per binary/);
  assert.throws(() => parseFilter("binary(~e2e)"), /unsupported name matcher/);
  assert.throws(() => profileFilter(nextest, "no-such-profile"));
}

// featureClosure infers the features of a package-scoped build (HeadCanonical's
// own command, any package-scoped row the checker reads). That inference must
// never claim a feature Cargo does not enable in a package-scoped build.
{
  const metadata = readMetadata();
  const packages = workspacePackages(metadata);
  const unified = workspaceFeatures(metadata, packages);
  for (const pkg of packages) {
    const defaults = Object.hasOwn(pkg.features ?? {}, "default") ? ["default"] : [];
    const inferred = featureClosure(pkg, new Set(defaults));
    const tree = spawnSync(
      join(root, "scripts/repo-cargo"),
      ["tree", "-p", pkg.name, "--depth", "0", "-e", "normal,build,dev", "--prefix", "none", "-f", "{p}|{f}"],
      { cwd: root, encoding: "utf8" },
    );
    assert.equal(tree.status, 0, `cargo tree -p ${pkg.name}: ${tree.stderr}`);
    const first = tree.stdout.split("\n")[0];
    const actual = new Set(first.slice(first.indexOf("|") + 1).split(",").filter(Boolean));
    for (const feature of inferred) {
      assert.ok(actual.has(feature), `${pkg.name}: inferred default feature ${feature} is enabled by Cargo`);
      assert.ok(unified.get(pkg.name).has(feature), `${pkg.name}: ${feature} is in the workspace set`);
    }
  }
}

// The job's runtime check: nextest must list exactly the planned binaries,
// and no test of theirs may be filtered out by a profile.
{
  const verifier = resolve(here, "ci-verify-row-binaries.mjs");
  const scratch = mkdtempSync(join(tmpdir(), "ci-verify-row-binaries-"));
  const verify = (suites, expected) => {
    const listPath = join(scratch, "list.json");
    writeFileSync(listPath, JSON.stringify({ "rust-suites": suites }));
    return spawnSync("node", [verifier, listPath, ...expected], { encoding: "utf8" });
  };
  const suite = (testcases, status = "listed") => ({ status, testcases });
  const match = { "filter-match": { status: "matches" } };
  const ignored = { "filter-match": { status: "mismatch", reason: "ignored" } };
  const defaultFiltered = { "filter-match": { status: "mismatch", reason: "default-filter" } };
  try {
    assert.equal(verify({ "p::a": suite({ t1: match, t2: ignored }), p: suite({ t3: match }) }, ["p::a", "p"]).status, 0);
    assert.equal(verify({ "p::a": suite({ t1: match }) }, ["p::a", "p::b"]).status, 1, "a missing binary fails");
    assert.equal(verify({ "p::a": suite({ t1: match }), "q::a": suite({ t1: match }) }, ["p::a"]).status, 1, "an extra binary fails");
    assert.equal(
      verify({ "p::a": suite({ t1: match }), q: suite({ t1: { "filter-match": { status: "mismatch", reason: "expression" } } }), r: suite({}, "skipped") }, ["p::a"]).status,
      0,
      "a listed binary none of whose tests are selected (another member's lib under --workspace) passes",
    );
    assert.equal(verify({ "p::a": suite({ t1: defaultFiltered }) }, ["p::a"]).status, 1, "a profile-filtered test fails");
    assert.equal(verify({ "p::a": suite({}, "skipped") }, ["p::a"]).status, 1, "a skipped binary fails");
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

// Bad arguments fail loudly.
{
  const result = run(["--format", "yaml", "--", "crates/meerkat-core/src/lib.rs"]);
  assert.notEqual(result.status, 0, "invalid format is an error");
}

console.log("ci-cargo-lanes fail-closed contracts hold");
