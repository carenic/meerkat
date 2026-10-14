#!/usr/bin/env node
// Changed-path classifier for the GitHub-hosted Cargo PR CI (ci.yml).
//
// Reads the changed paths of a push or pull request and emits the lane plan:
// which workspace packages get a clippy lane and a unit lane, how they are
// packed into parallel shards, the reverse-dependency closure for the
// closure-check lane, and the boolean flags for the conditional lanes.
//
// The plan fails closed. Any path that can change what Cargo compiles
// (Rust sources, manifests, the lock, Cargo/nextest configuration, the
// toolchain pin, the build wrapper scripts, this classifier, or ci.yml)
// yields at least one clippy and one unit lane. A Rust-relevant path that
// cannot be attributed to one workspace package, or a diff whose base cannot
// be established, escalates to the whole workspace instead of to nothing.
// The old `changed-paths` BuildBuddy mode reported unrun lanes as passed for
// three weeks; this script exits non-zero rather than emit an empty plan for
// a build-relevant change.
//
// usage:
//   ci-cargo-lanes.mjs [--base <rev> --head <rev>] [--max-shards N]
//                      [--workspace-shards N] [--closure-test-rows N]
//                      [--format json|github] [--] [changed-path ...]
//   ci-cargo-lanes.mjs --paths-from-stdin [...]
//
// Without explicit paths, --base/--head, or stdin, the plan is the whole
// workspace (no diff base means no evidence of what did not change).
//
// Shards are packed by an estimated lane cost, not by crate count: a crate's
// Rust line count (its lib-test binary compiles every inline test) plus a
// term per workspace crate in its build closure (each top-level crate's
// test binary links the whole graph; the cold hosted-runner shards that
// bundled several such crates ran 18-25 minutes while the line-count
// balanced shards of leaf crates ran 7-9). --max-shards bounds a
// changed-package plan, --workspace-shards a whole-workspace plan.
//
// Budget model. A pull-request lane models at
//   minutes = 1 + (lines + 8000 * build_closure) / 35000
// The constants came from earlier hosted 4-vCPU runs. Those historical source
// sizes and recursive closures do not calibrate today's build-closure input;
// current estimates require comparison with hosted results. On d1dda1482 the
// runtime unit lane models 15.1 minutes, not a measured completion guarantee.
// The pull-request unit plan must stay under PR_UNIT_BUDGET_MINUTES, a
// surrogate packing cap and not a measured timing. It is 17 since the runtime
// lane, whose closure counts its self dev-dependency, modelled 16.2 minutes.
// The enforced limits are the CI gate's: 1500 s of lane execution and 2700 s
// push-to-terminal, runner queue excluded (CI_MAX_EXEC_SECONDS and
// CI_MAX_TERMINAL_SECONDS in ci.yml); ordinary hosted runs supply the
// measured timing.
// Every unit lane that ever exceeded the push-to-terminal budget of its time
// (1200 s, since replaced by the gates above) compiled meerkat-mob's 442k
// lines: mob's own lane, and the lanes of crates
// that depend on mob and rebuild it under their own feature unification
// (rkat, rpc, rest, mcp-server, mob-mcp, mob-pack, ...). Those
// crates' unit tests therefore run on the push-to-main run (no budget) and
// on nightly; the chain is computed from cargo metadata, not listed here.
// The pull-request unit lane runs none of them except the package that
// PR_UNIT_CHAIN_EXCEPTIONS selects when one of its trigger packages changed.
// Clippy of a changed crate always runs in the pull request (worst measured
// lane 12:24 for mob).
//
// Hook-parity rows. The pre-push hook's cargo-test body
// (scripts/pre-push-unit.sh) ran the whole workspace's lib and tests/*.rs
// binaries, HeadCanonical and e2e-fast before every push. CI runs that
// coverage as required rows (closure_test_rows, the closure-tests job) for
// the test scope: the reverse-dependency closure of the changed packages,
// every member when a global input changed. Every row but HeadCanonical
// selects the whole workspace (`--workspace`) and names only its targets, so
// Cargo resolves every crate's features exactly as the hook's single
// workspace build does and builds only those targets' graphs: the row
// compiles the hook's own units (cargo's unit graph: identical for
// `--workspace --test ...` and `--workspace --lib`; a package-scoped row
// compiled 31-39 dependency units with fewer features, and so does
// `--workspace --exclude`). The rows are
//   lib        one `--workspace --lib` row: it builds every member's lib
//              test (nothing narrower keeps the resolution) and runs the
//              scope's through a package() filter
//   tests      every tests/*.rs target name the hook built, with every
//              member's target of that name, `--test NAME` by name, with
//              the hook's fast selection (ci-integration), split by
//              CLOSURE_TESTS_PER_ROW names
//   HeadCanonical and e2e-fast, from the hook's own lane commands
// Each row lists and runs one shared selection, and checks that nextest
// selects exactly the binaries the plan names before it runs.
// scripts/ci-hook-coverage.mjs reads the hook's lanes from the hook script
// and the executing step from ci.yml, credits only whole-workspace rows (and
// HeadCanonical's own command), and the plan fails when any hook binary in
// scope has no row.
// The test scope also holds the readers of changed non-Rust inputs
// (scripts/ci-non-rust-readers.json, kept complete by its guard): a test
// that reads docs/, specs/, sdks/, a fixture outside its crate or the whole
// checkout ran in the hook on every push, so a change to what it reads
// selects its package's rows, Rust change or not. Readers join the scope
// themselves; their dependents do not read the file.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import {
  HOOK_SCRIPT,
  checkHookCoverage,
  hookModel,
  parseFilter,
  profileFilter,
  workspaceFeatures,
} from "./ci-hook-coverage.mjs";
import { loadNonRustReaders, readerPackagesFor } from "./ci-non-rust-readers.mjs";
import {
  normalizePath,
  packageDir,
  packageDirs,
  packageForFile,
  readMetadata,
  root,
  workspacePackages,
} from "./rust-test-selector.mjs";

const NULL_SHA = "0000000000000000000000000000000000000000";
// Cost model constants (see the header comment).
const LINK_COST_PER_DEP = 8000;
const COST_UNITS_PER_MINUTE = 35000;
const LANE_SETUP_MINUTES = 1;
export const PR_UNIT_BUDGET_MINUTES = 17;
const HEAVY_ANCHOR = "meerkat-mob";

// Hook-parity rows (see the header). Every row runs on the hook's test-thread
// stack: scripts/pre-push-unit.sh exports 32 MiB for all of its lanes.
export const HOOK_TEST_STACK = 32 * 1024 * 1024;
export const CLOSURE_TESTS_PER_ROW = 12;
const CLOSURE_TEST_PROFILE = "ci-integration";
const CLOSURE_LIB_PROFILE = "ci-unit";
// Compile and link of one tests/*.rs binary, in cost units. Uncalibrated:
// the first hosted closure runs measure it.
const TEST_TARGET_COST = 15000;
const LIB_TARGET_KINDS = new Set(["lib", "rlib", "dylib", "cdylib", "staticlib"]);

// Feature-gated unit suites. Every unit lane builds its packages with their
// default features, so a lib or bin test behind a non-default feature ran in
// no lane at all: measured on 58023fbd, 342 lib/bin tests exist only under
// --all-features, and main CI stayed green while #1304 broke one of the
// facade's GPT Live tests. Each suite below is one more lane,
//   nextest run -p <package> --features <features> --lib --bins
// on every push to main, and in the pull request when <package> itself
// changed and is outside the meerkat-mob compile chain. Feature names are
// checked against cargo metadata, so a renamed feature fails the plan
// instead of silently dropping the suite.
//
// 22 of the facade's GPT Live tests overflow the 8 MiB test-thread stack that
// .cargo/config.toml gives every lane (debug build, measured on 82fddfc33);
// they pass at the 32 MiB the pre-push hook and the BuildBuddy unit lane use.
// Their rows carry that stack until the frames fit 8 MiB.
const GPT_LIVE_TEST_STACK = 32 * 1024 * 1024;
export const FEATURE_UNIT_SUITES = [
  // The shipping public GPT Live path (experimental_gpt_live::tests,
  // session_runtime::live_summary, service_factory) plus the realtime
  // credential and projection tests of the facade.
  { package: "meerkat", id: "openai-live", features: ["openai-live", "openai-realtime", "memory-store", "test-realtime-fixtures"], rustMinStack: GPT_LIVE_TEST_STACK },
  // The deprecated private broker adds its admission and broker tests.
  { package: "meerkat", id: "experimental-gpt-live", features: ["experimental-gpt-live", "openai-realtime", "memory-store", "test-realtime-fixtures"], rustMinStack: GPT_LIVE_TEST_STACK },
  { package: "meerkat-openai", id: "live", features: ["live", "experimental-gpt-live-gate0-harness", "test-realtime-fixtures"] },
  { package: "meerkat-live", id: "webrtc", features: ["webrtc"] },
  { package: "meerkat-session", id: "live", features: ["live", "session-store"] },
  { package: "meerkat-auth-core", id: "cloud", features: ["aws-sigv4", "gcp-auth", "azure-ad"] },
  { package: "meerkat-anthropic", id: "cloud", features: ["bedrock", "vertex", "foundry"] },
  { package: "meerkat-mob", id: "openai-live", features: ["experimental-gpt-live", "schema"] },
  { package: "meerkat-mob-mcp", id: "openai-live", features: ["experimental-gpt-live-gate0-harness"] },
  { package: "meerkat-rpc", id: "openai-live", features: ["experimental-gpt-live"] },
  { package: "xtask", id: "machine-authority", features: ["machine-authority"] },
  // The generated-kernel tests (runtime.rs) are behind `test-oracle`: their
  // hand-built MeerkatMachine inputs broke silently when RegisterSession
  // gained initial_run_start_holds, because no lane enabled the feature.
  { package: "meerkat-machine-kernels", id: "test-oracle", features: ["test-oracle"] },
];

// Integration-test suites. Every unit lane runs `--lib --bins`, so a crate's
// tests/*.rs binaries ran in no pull-request lane: #1349 merged with three
// failures there (runtime_alphabet_parity in meerkat-machine-codegen,
// gpt_live_generated_authority in meerkat-runtime). Each suite below is one
// lane,
//   nextest run -p <package> --test '*'
// on a pull request or main push whose directly changed packages include any
// of its triggers (a workspace-mode plan changes every package, so it runs
// every suite). The codegen parity tests compare the machine catalog with the
// runtime's and meerkat-mob's generated manifests, so the machine schema and
// DSL crates, the kernels, the runtime and meerkat-mob trigger them. Lane
// cost on 4 cores (measured on e20cc9ad9 after a meerkat-runtime change, deps
// cached): runtime 168 s, machine-codegen 396 s (meerkat-mob is compiled as
// a plain library, not its lib-test). Over the last 60 merges to main the
// runtime suite would have run on 17 and the codegen suite on 25.
const MACHINE_AUTHORITY_PACKAGES = [
  "meerkat-machine-schema",
  "meerkat-machine-dsl",
  "meerkat-machine-dsl-core",
  "meerkat-machine-derive",
  "meerkat-machine-kernels",
];
//
// A suite may also name `features` (a target behind a required feature
// makes `--test '*'` fail without it) and `paths`: changed-path prefixes
// that select it even when no Rust package changed. xtask's integration
// tests (ci_gate_requires_rmat and friends) pin the workflow files, so a
// workflow-only edit runs them; twice on 2026-10-01 a change merged that
// broke one with no PR lane running it. 163 tests, 169 s on 4 cores.
export const INTEGRATION_SUITES = [
  { package: "meerkat-runtime", triggers: [...MACHINE_AUTHORITY_PACKAGES, "meerkat-runtime"] },
  {
    package: "meerkat-machine-codegen",
    triggers: [...MACHINE_AUTHORITY_PACKAGES, "meerkat-runtime", "meerkat-mob", "meerkat-machine-codegen"],
  },
  // buildbuddy_static_lanes also reads the SDK manifests and generated types
  // and walks sdks/ (version parity, generated-type and banned-name checks).
  { package: "xtask", features: ["machine-authority"], triggers: ["xtask"], paths: [".github/workflows/", "sdks/"] },
  // kernel_typed_round_trip drives the generated kernels with hand-built
  // inputs, so any machine DSL change can break it; it needs `test-oracle`.
  { package: "meerkat-machine-kernels", features: ["test-oracle"], triggers: MACHINE_AUTHORITY_PACKAGES },
  // The facade's store and cold-restart suites, with the Bazel target's
  // features. No other lane ran them before a release tag: the Bazel graph
  // runs only nightly (on main) and at the tag, so a compaction plus
  // cold-restart store corruption reached a green release/0.8.51 (#1673).
  {
    package: "meerkat",
    name: "meerkat-store-restart",
    features: [
      "anthropic", "atif", "comms", "copilot", "gemini", "integration-real-tests", "jsonl-store", "live",
      "live-webrtc", "mcp", "memory-store", "memory-store-session", "openai", "openai-live", "openai-realtime",
      "schedule", "session-compaction", "session-store", "skills", "test-mcp-oauth-fixtures",
      "test-realtime-fixtures", "workgraph",
    ],
    tests: [
      "cold_restart_resume_after_compaction",
      "cold_restart_resume",
      "persistence_contract",
      "storage_provider_seam",
      "brain_swap_surface_parity",
    ],
    triggers: ["meerkat", "meerkat-core", "meerkat-store", "meerkat-session", "meerkat-runtime", "meerkat-sqlite"],
  },
  // A governed realm's composition: posture, restored grant owner, stock
  // composition and the realm's persisted MCP installations (the connection
  // control spawns the mcp-test-server fixture).
  {
    package: "meerkat",
    name: "meerkat-governed-realm",
    features: ["session-store", "local-authorization", "mcp"],
    tests: ["governed_realm"],
    triggers: ["meerkat", "meerkat-core", "meerkat-store", "meerkat-runtime", "meerkat-mcp"],
  },
  // These no-provider regressions exercise configured limits through the
  // factory and the RPC realm path. Select only their named test binaries.
  {
    package: "meerkat",
    name: "standalone-turn-lifecycle",
    tests: ["standalone_turn_lifecycle"],
    triggers: ["meerkat", "meerkat-core"],
  },
  {
    package: "meerkat-rpc",
    name: "rpc-configured-turn-limit",
    tests: ["configured_turn_limit"],
    triggers: ["meerkat-rpc", "meerkat", "meerkat-core"],
  },
  // Deterministic replays of recorded Turbo S gpt-live-1 provider streams
  // (tests/integration/fixtures/gpt_live_replay): the S104/S106 voice
  // contracts without a provider or a key. `tests` names the one target, so
  // the lane does not build every integration binary. Triggered by the
  // packages on the public Live path, its fixtures and its harness.
  {
    package: "meerkat-integration-tests",
    name: "gpt-live-replay",
    features: ["gpt-live-replay"],
    tests: ["gpt_live_replay"],
    triggers: [
      "meerkat",
      "meerkat-core",
      "meerkat-openai",
      "meerkat-live",
      "meerkat-runtime",
      "meerkat-session",
      "meerkat-mob",
      "meerkat-mob-mcp",
      "meerkat-rpc",
      "meerkat-integration-tests",
    ],
    paths: ["tests/integration/fixtures/gpt_live_replay/"],
  },
  // The ordinary native governed loops and cost correctness controls live in
  // tests/*.rs; unit rows only select lib/bin tests. Keep real/perf opt-ins.
  {
    package: "meerkat-authorization",
    triggers: [
      ...MACHINE_AUTHORITY_PACKAGES,
      "meerkat-authorization", "meerkat-authorization-contracts", "meerkat-core", "meerkat-runtime",
      "meerkat", "meerkat-tools", "meerkat-llm-core", "meerkat-anthropic", "meerkat-auth-core", "meerkat-models",
    ],
  },
];

// Packages whose own lib-test binary dominates their push-to-main lane. On
// the hosted 4-vCPU runners meerkat-mob's lanes took 12.5-20 min: 7.7-14.5
// min of cargo compile (one rustc unit, sccache-dependent) plus 4-5 min of
// nextest (measured on 6b2095989 and fe7fc7295). A single crate's compile
// cannot be split and nextest partitions that each rebuild it all miss the
// cache at once, so these packages build a nextest archive once (per
// default-feature lane and per feature suite) and fan the run out over
// `partitions` jobs that execute the archive with `--partition hash:k/n`.
export const ARCHIVED_UNIT_LANES = [{ package: "meerkat-mob", partitions: 2 }];

// The one exception to the chain deferral. meerkat-rpc compiles meerkat-mob,
// but its default features include local-authorization and mcp, and its lib
// tests carry the MCP restart controls (governed_jsonl::tests::stock_governed_*:
// a stock governed realm's MCP attachment is restored, or refused after a
// revoke, across a host restart). Those controls must run before merge, so
// meerkat-rpc's existing default-feature unit row joins the pull request when
// a trigger package changed directly: meerkat-rpc itself or a native package
// the controls exercise (the facade's governed realm, the core contracts, the
// store and its SQLite backend, the runtime and session owners, the MCP
// client, local authorization). It is packed and held to
// PR_UNIT_BUDGET_MINUTES like every other unit lane. The rest of the chain
// stays deferred (a meerkat-mob change alone selects nothing here), and
// meerkat-rpc's feature suites stay on main. Names are checked against cargo
// metadata, so a rename fails the plan instead of dropping the exception.
export const PR_UNIT_CHAIN_EXCEPTIONS = [
  {
    package: "meerkat-rpc",
    triggers: [
      "meerkat-rpc",
      "meerkat",
      "meerkat-core",
      "meerkat-store",
      "meerkat-sqlite",
      "meerkat-runtime",
      "meerkat-session",
      "meerkat-mcp",
      "meerkat-authorization",
      "meerkat-authorization-contracts",
    ],
  },
];
// Feature-combination checks (#1687). Every Cargo lane builds a package with
// its default features (plus FEATURE_UNIT_SUITES and --all-features), but the
// Bazel graph builds each crate with the feature set the generated BUILD.bazel
// gives it, and the nightly feature matrix checks further combinations. That
// graph runs only nightly on main and at the release tag, so a break under one
// of those sets reached release/0.8.51 three times: meerkat-mob-mcp's lib tests
// under `openai-live` alone (#1595) and `meerkat-mob --no-default-features`
// (#1558) among them. For each changed package the plan adds compile-only
// checks under:
//   - its Bazel test feature set (every rust_test of a package carries the
//     same set), in one `check --lib --bins --tests` per clippy shard over the
//     shard's packages, as Bazel's own unified graph builds them;
//   - every other Bazel library or binary feature set it has (production
//     variants that strip `test-support`, the surface feature variants);
//   - the `$(CARGO) check` rows of the Make feature-matrix targets below.
// The rows follow the clippy shards, whose packing is already sized for a
// pull-request lane; each shard gives a Bazel-set job and a matrix job.
// Feature names come from the generated BUILD.bazel (kept fresh by the
// bazel-locks-freshness gate) and the Makefile, and are validated against
// cargo metadata, so a renamed feature fails the plan.
export const MAKE_FEATURE_MATRIX_TARGETS = ["test-minimal", "test-feature-matrix-lib"];

// Whole-workspace Clippy shards. A Clippy lane checks the workspace chain
// below its packages (from sccache since scripts/ci-clippy-sccache-wrapper)
// and lints its own packages. The long chains end in a few heads: measured
// on runs 38471093009, 38415750191 and 38020614570 and main push
// 38473833907, tools -> meerkat-mob -> mob-mcp -> rpc -> rkat finished
// 1169-1363 s into a lane. Packed by weight, those heads shared shards with
// other crates, and the pole was a shard that linted meerkat-mob's lib and
// its 55 test targets after the chain. In a whole-workspace plan each group
// below gets a shard of its own, so the pole is one head, and a head's
// dependencies are checked under the same feature set on every run (stable
// sccache keys); the other packages pack into CLIPPY_PACKED_SHARDS more. A
// changed-package plan keeps one shard per changed package up to
// --max-shards. Names are checked against cargo metadata, so a renamed head
// fails the plan instead of falling back to packing.
export const CLIPPY_HEAD_SHARDS = [
  ["rkat"],
  ["meerkat-rpc"],
  ["meerkat-mob"],
  ["meerkat"],
  ["meerkat-mob-mcp", "meerkat-mob-pack", "meerkat-mob-adaptive"],
  ["meerkat-rest", "meerkat-mcp-server", "meerkat-web-runtime"],
  ["meerkat-integration-tests", "surface-build-fixtures"],
];
export const CLIPPY_PACKED_SHARDS = 5;

function parseBazelFeatureSets(pkg) {
  const buildPath = resolve(root, packageDir(pkg), "BUILD.bazel");
  if (!existsSync(buildPath)) {
    throw new Error(`no generated BUILD.bazel for ${pkg.name} (${relative(root, buildPath)})`);
  }
  const text = readFileSync(buildPath, "utf8");
  const known = new Set(Object.keys(pkg.features ?? {}));
  const testSets = new Set();
  const variants = new Map();
  for (const [, rule, body] of text.matchAll(/^(rust_test|rust_library|rust_binary)\(\n([\s\S]*?)^\)/gm)) {
    const list = body.match(/^ {4}crate_features = \[([\s\S]*?)\]/m);
    // Weak dependency features (`dep?/feature`) are rustc cfgs the generator
    // passes through; Cargo enables them from the package's own features.
    const features = list
      ? [...list[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]).filter((name) => !name.includes("/"))
      : [];
    for (const feature of features) {
      if (!known.has(feature)) {
        throw new Error(`${relative(root, buildPath)} enables ${feature}, which ${pkg.name} does not declare`);
      }
    }
    const key = [...new Set(features)].sort().join(",");
    if (rule === "rust_test") {
      testSets.add(key);
    } else {
      const variant = variants.get(key) ?? { lib: false, bins: false };
      if (rule === "rust_library") variant.lib = true;
      if (rule === "rust_binary") variant.bins = true;
      variants.set(key, variant);
    }
  }
  if (testSets.size > 1) {
    throw new Error(
      `${relative(root, buildPath)} gives ${pkg.name}'s tests ${testSets.size} feature sets; the feature-combination check expects one`,
    );
  }
  const testSet = testSets.size === 1 ? [...testSets][0] : null;
  if (testSet !== null) variants.delete(testSet);
  const split = (key) => (key === "" ? [] : key.split(","));
  return {
    test: testSet === null ? null : split(testSet),
    variants: [...variants]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, variant]) => ({ features: split(key), ...variant })),
  };
}

function parseMakeFeatureMatrix(byName) {
  const text = readFileSync(resolve(root, "Makefile"), "utf8");
  const rows = [];
  for (const target of MAKE_FEATURE_MATRIX_TARGETS) {
    const start = text.match(new RegExp(`^${target}:.*\\n((?:\\t.*\\n|#.*\\n)*)`, "m"));
    if (!start) throw new Error(`Makefile has no ${target} target`);
    let count = 0;
    for (const line of start[1].split("\n")) {
      const check = line.match(/^\t\$\(CARGO\) check (.+)$/);
      if (!check) continue;
      const args = check[1].trim();
      const pkgName = args.match(/(?:^|\s)-p (\S+)/)?.[1];
      if (!pkgName || !byName.has(pkgName)) {
        throw new Error(`Makefile ${target} row names no known package: ${args}`);
      }
      const features = args.match(/--features (\S+)/)?.[1]?.split(",") ?? [];
      for (const feature of features) {
        const [owner, name] = feature.includes("/") ? feature.split("/", 2) : [pkgName, feature];
        const ownerPkg = byName.get(owner);
        if (!ownerPkg || !Object.hasOwn(ownerPkg.features, name)) {
          throw new Error(`Makefile ${target} row enables unknown feature ${feature}: ${args}`);
        }
      }
      rows.push({ target, package: pkgName, args });
      count += 1;
    }
    if (count === 0) throw new Error(`Makefile ${target} has no $(CARGO) check rows`);
  }
  return rows;
}

function targetFlags(pkgs, { tests }) {
  const kinds = new Set(pkgs.flatMap((pkg) => pkg.targets.flatMap((target) => target.kind)));
  const flags = [];
  if (["lib", "rlib", "cdylib", "dylib", "staticlib", "proc-macro"].some((kind) => kinds.has(kind))) flags.push("--lib");
  if (kinds.has("bin")) flags.push("--bins");
  if (tests) flags.push("--tests");
  return flags;
}

function bazelTestRow(pkgs, sets) {
  const withTests = pkgs.filter((pkg) => sets.get(pkg.name).test !== null);
  if (withTests.length === 0) return null;
  const features = withTests.flatMap((pkg) => sets.get(pkg.name).test.map((feature) => `${pkg.name}/${feature}`));
  return [
    ...withTests.map((pkg) => `-p ${pkg.name}`),
    "--no-default-features",
    ...(features.length > 0 ? [`--features ${features.join(",")}`] : []),
    ...targetFlags(withTests, { tests: true }),
  ].join(" ");
}

function featureChecks(shards, byName) {
  const changedNames = shards.flatMap((shard) => shard.packages);
  const sets = new Map(changedNames.map((name) => [name, parseBazelFeatureSets(byName.get(name))]));
  const makeRows = parseMakeFeatureMatrix(byName);
  // Each shard gives two jobs that run in parallel: its Bazel test-set check
  // (one invocation over the shard, 534-796 s cold on #1763's workspace run)
  // and its other rows (variants and Make matrix rows, up to ~1000 s cold).
  // In one job they reached 1568 s, over the pull-request lane budget.
  return shards.flatMap((shard) => {
    const pkgs = shard.packages.map((name) => byName.get(name));
    const bazel = bazelTestRow(pkgs, sets);
    const commands = [];
    const add = (args) => {
      if (args && !commands.includes(args)) commands.push(args);
    };
    for (const pkg of pkgs) {
      for (const variant of sets.get(pkg.name).variants) {
        add([
          `-p ${pkg.name}`,
          "--no-default-features",
          ...(variant.features.length > 0 ? [`--features ${variant.features.join(",")}`] : []),
          ...(variant.lib ? ["--lib"] : []),
          ...(variant.bins ? ["--bins"] : []),
        ].join(" "));
      }
      for (const row of makeRows) {
        if (row.package === pkg.name) add(row.args);
      }
    }
    return [
      { name: `${shard.name} bazel set`, packages: shard.packages, commands: bazel ? [bazel] : [] },
      { name: `${shard.name} matrix`, packages: shard.packages, commands },
    ].filter((job) => job.commands.length > 0);
  });
}

function estimatedMinutes(cost) {
  return Math.round((LANE_SETUP_MINUTES + cost / COST_UNITS_PER_MINUTE) * 10) / 10;
}

function parseArgs(argv) {
  const args = {
    base: "",
    head: "HEAD",
    maxShards: 6,
    workspaceShards: 8,
    closureTestRows: 12,
    format: "json",
    pathsFromStdin: false,
    paths: [],
    explicitPaths: false,
    changedPathsOnly: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--base") {
      args.base = argv[++i] ?? "";
    } else if (arg === "--head") {
      args.head = argv[++i] ?? "HEAD";
    } else if (arg === "--max-shards") {
      args.maxShards = Number.parseInt(argv[++i] ?? "", 10);
      if (!Number.isInteger(args.maxShards) || args.maxShards < 1) {
        throw new Error("--max-shards requires a positive integer");
      }
    } else if (arg === "--workspace-shards") {
      args.workspaceShards = Number.parseInt(argv[++i] ?? "", 10);
      if (!Number.isInteger(args.workspaceShards) || args.workspaceShards < 1) {
        throw new Error("--workspace-shards requires a positive integer");
      }
    } else if (arg === "--closure-test-rows") {
      args.closureTestRows = Number.parseInt(argv[++i] ?? "", 10);
      if (!Number.isInteger(args.closureTestRows) || args.closureTestRows < 1) {
        throw new Error("--closure-test-rows requires a positive integer");
      }
    } else if (arg === "--format") {
      args.format = argv[++i] ?? "";
      if (!["json", "github"].includes(args.format)) {
        throw new Error("--format must be json or github");
      }
    } else if (arg === "--changed-paths-only") {
      args.changedPathsOnly = true;
    } else if (arg === "--paths-from-stdin") {
      args.pathsFromStdin = true;
      args.explicitPaths = true;
    } else if (arg === "--") {
      args.paths.push(...argv.slice(i + 1));
      args.explicitPaths = true;
      break;
    } else if (arg === "--help" || arg === "-h") {
      process.stdout.write(
        "usage: ci-cargo-lanes.mjs [--base <rev> --head <rev>] [--max-shards N] [--workspace-shards N] [--closure-test-rows N] [--format json|github] [--paths-from-stdin] [--changed-paths-only] [--] [changed-path ...]\n",
      );
      process.exit(0);
    } else if (arg.startsWith("-")) {
      throw new Error(`unknown argument: ${arg}`);
    } else {
      args.paths.push(arg);
      args.explicitPaths = true;
    }
  }
  return args;
}

function git(argsList, options = {}) {
  return execFileSync("git", argsList, { cwd: root, encoding: "utf8", ...options });
}

function revisionExists(rev) {
  const result = spawnSync("git", ["cat-file", "-e", `${rev}^{commit}`], { cwd: root });
  return result.status === 0;
}

// Returns { paths, reason }. `paths` is null when the diff cannot be
// established, which the caller treats as "everything changed".
function collectChangedPaths(args) {
  if (args.pathsFromStdin) {
    const text = readFileSync(0, "utf8");
    const paths = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
    const all = [...paths, ...args.paths];
    return { paths: all, removed: explicitRemoved(all), reason: "explicit path list" };
  }
  if (args.explicitPaths) {
    return { paths: args.paths, removed: explicitRemoved(args.paths), reason: "explicit path list" };
  }
  if (!args.base || args.base === NULL_SHA) {
    return { paths: null, reason: "no diff base supplied" };
  }
  if (!revisionExists(args.base)) {
    const fetched = spawnSync("git", ["fetch", "--no-tags", "--depth=1", "origin", args.base], {
      cwd: root,
      stdio: "inherit",
    });
    if (fetched.status !== 0 || !revisionExists(args.base)) {
      return { paths: null, reason: `diff base ${args.base} is not available` };
    }
  }
  if (!revisionExists(args.head)) {
    return { paths: null, reason: `diff head ${args.head} is not available` };
  }
  // --no-renames lists both sides of a rename (its old side as a deletion),
  // and T keeps type-only changes, such as a source file turned symlink.
  // Either can be the only path that names a package: a test moved from
  // crate A to crate B must still select A, and a source file renamed into
  // docs/ must still select its crate. -z keeps every name exact.
  const output = git([
    "diff", "--no-renames", "--name-only", "-z", "--diff-filter=ACMRDT", args.base, args.head, "--",
  ]);
  // Deleted paths and the old side of renames: a moved or removed source file
  // changes the generated Bazel data dependencies even when no BUILD file is
  // in the diff.
  const statusOutput = git(["diff", "--name-status", "--find-renames", "--diff-filter=RD", args.base, args.head, "--"]);
  const removed = [];
  for (const line of statusOutput.split(/\r?\n/)) {
    const fields = line.split("\t");
    if (fields.length >= 2 && fields[0].startsWith("D")) removed.push(fields[1].trim());
    if (fields.length >= 3 && fields[0].startsWith("R")) removed.push(fields[1].trim());
  }
  return {
    paths: output.split("\0").filter(Boolean),
    removed,
    reason: `git diff ${args.base}..${args.head}`,
  };
}

// For an explicit path list (self-test fixtures, local dry runs) a listed path
// that no longer exists in the working tree is a deletion or the old side of a
// move.
function explicitRemoved(paths) {
  return paths.filter((path) => !existsSync(resolve(root, path)));
}

// The CI workflow and its classifier define every lane, so a change to them
// runs every lane they define, as the Rust lanes do through isGlobalPath.
function isLaneDefinitionPath(path) {
  return (
    path === ".github/workflows/ci.yml" ||
    path === "scripts/ci-cargo-lanes.mjs" ||
    path === "scripts/ci-cargo-lanes-selftest.mjs" ||
    path === "scripts/ci-hook-coverage.mjs" ||
    path === "scripts/ci-verify-row-binaries.mjs" ||
    path === "scripts/ci-non-rust-readers.mjs"
  );
}

// Paths whose change can alter the Bazel graph without touching Rust code:
// generated or hand-written BUILD files, Starlark, module and lock inputs,
// the BUILD generator, the Bazel toolchain and lane definitions, and every
// Cargo manifest and the lockfile (crate_universe reads them).
function isBazelGraphPath(path) {
  const base = path.split("/").pop();
  return (
    isLaneDefinitionPath(path) ||
    base === "BUILD.bazel" ||
    base === "BUILD" ||
    path.endsWith(".bzl") ||
    path === "MODULE.bazel" ||
    path === "MODULE.bazel.lock" ||
    path === ".bazelrc" ||
    path === ".bazelversion" ||
    path === ".bazelignore" ||
    path === "Cargo.lock" ||
    base === "Cargo.toml" ||
    path === "scripts/generate-bazel-rust-builds.mjs" ||
    path.startsWith("tools/bazel/") ||
    path.startsWith("tools/buildbuddy/")
  );
}

// Node-only example suites: examples/037-live-webrtc-web (Playwright
// Chromium) and the examples SDK suite, which links sdks/typescript and runs
// the Python SDK examples against a fake host. Markdown is not an input.
function isExamplesBrowserPath(path) {
  if (isLaneDefinitionPath(path)) return true;
  if (path.startsWith("sdks/typescript/") || path.startsWith("sdks/python/")) return true;
  return path.startsWith("examples/") && !/\.(md|mdx)$/.test(path);
}

// Example web suites: examples 031, 032 and 033 execute the sdks/web wasm
// runtime in Chromium, so the runtime build and its inputs select them along
// with the example sources. Markdown is not an input.
const EXAMPLE_WEB_DIRS = [
  "examples/031-wasm-mini-diplomacy-sh/",
  "examples/032-wasm-webcm-agent/",
  "examples/033-the-office-demo-sh/",
];
function isExampleWebPath(path) {
  if (isLaneDefinitionPath(path)) return true;
  if (
    path.startsWith("sdks/web/") ||
    path.startsWith("crates/meerkat-web-runtime/") ||
    path.startsWith("crates/meerkat-contracts/")
  ) {
    return true;
  }
  return EXAMPLE_WEB_DIRS.some((dir) => path.startsWith(dir)) && !/\.(md|mdx)$/.test(path);
}

// Paths whose change alters the compile graph of every package.
function isGlobalPath(path) {
  if (
    path === "Cargo.toml" ||
    path === "Cargo.lock" ||
    path === "rust-toolchain.toml" ||
    path === "rust-toolchain" ||
    path === "Makefile" ||
    path === ".config/nextest.toml" ||
    path === "scripts/repo-cargo" ||
    path === "scripts/agent-gate" ||
    path === "scripts/rust-lane-doctor" ||
    path === "scripts/ci-cargo-lanes.mjs" ||
    path === "scripts/ci-cargo-lanes-selftest.mjs" ||
    path === "scripts/ci-hook-coverage.mjs" ||
    path === "scripts/ci-verify-row-binaries.mjs" ||
    path === "scripts/ci-non-rust-readers.mjs" ||
    path === "scripts/rust-test-selector.mjs" ||
    path === "scripts/ci-pin-rust-toolchain-env" ||
    path === "scripts/ci-clippy-sccache-wrapper" ||
    path === ".github/workflows/ci.yml"
  ) {
    return true;
  }
  return (
    path.startsWith(".cargo/") ||
    path.startsWith("scripts/cargo-") ||
    path.startsWith(".github/actions/setup-rust-ci/")
  );
}

function isRustSourcePath(path) {
  return path.endsWith(".rs") || path.endsWith("/Cargo.toml") || path === "Cargo.toml";
}

function runClassifier(script, paths) {
  if (paths.length === 0) return false;
  const result = spawnSync(join(root, script), ["--", ...paths], {
    cwd: root,
    encoding: "utf8",
  });
  if (result.status === 0) return true;
  if (result.status === 1) return false;
  throw new Error(
    `${script} failed (status ${result.status}): ${result.stderr || result.stdout || "no output"}`,
  );
}

function rustLineCount(dir) {
  let lines = 0;
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.name === "target" || entry.name === "node_modules" || entry.name.startsWith(".")) continue;
      const full = join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(full);
      } else if (entry.isFile() && entry.name.endsWith(".rs")) {
        try {
          const text = readFileSync(full, "utf8");
          lines += text.split("\n").length;
        } catch {
          // Unreadable sources are reported by rustc, not by the planner.
        }
      }
    }
  }
  return lines;
}

function shortName(name) {
  return name.startsWith("meerkat-") ? name.slice("meerkat-".length) : name;
}

// Longest-processing-time first bin packing of packages into at most
// `maxShards` lanes by Rust line count. Deterministic: ties break by name.
function packShards(pkgs, weights, maxShards, model = null) {
  const sorted = [...pkgs].sort((a, b) => weights.get(b) - weights.get(a) || a.localeCompare(b));
  const shardCount = Math.min(maxShards, sorted.length);
  const bins = Array.from({ length: shardCount }, () => ({ packages: [], weight: 0 }));
  for (const pkg of sorted) {
    bins.sort((a, b) => a.weight - b.weight || a.packages.length - b.packages.length);
    bins[0].packages.push(pkg);
    bins[0].weight += weights.get(pkg);
  }
  return bins
    .filter((bin) => bin.packages.length > 0)
    .map((bin) => {
      const names = [...bin.packages].sort();
      // Label by the heaviest member so a lane reads as what dominates it.
      const heaviest = [...names].sort((a, b) => weights.get(b) - weights.get(a) || a.localeCompare(b))[0];
      const label = names.length === 1
        ? shortName(names[0])
        : `${shortName(heaviest)}+${names.length - 1}`;
      const shard = {
        name: label,
        packages: names,
        package_flags: names.map((name) => `-p ${name}`).join(" "),
        estimated_cost: bin.weight,
      };
      if (model) {
        // A lane compiles each dependency once, so the graph term uses the
        // union of the shard's dependency closures, not their sum.
        const union = new Set();
        let lines = 0;
        for (const name of names) {
          lines += model.lines.get(name);
          for (const dep of model.closures.get(name)) union.add(dep);
        }
        shard.estimated_minutes = estimatedMinutes(lines + LINK_COST_PER_DEP * union.size);
      }
      return shard;
    })
    .sort((a, b) => b.estimated_cost - a.estimated_cost || a.name.localeCompare(b.name));
}

// The hook-parity rows for the test scope (see the header). Every row but
// HeadCanonical selects the whole workspace (`--workspace`) and names only
// its own targets: Cargo then resolves features exactly as the hook's single
// workspace build does, and builds only those targets' graphs, so a row's
// compilation units are the hook's own. Measured with cargo's unit graph on
// b45c8e60: `--workspace --test ...` rows 472/472 and 380/380 units identical
// to the hook's, `--workspace --lib` 572/572; package-scoped rows differed in
// 31-39 units (dependency features), and so does `--workspace --exclude`.
function hookParityRows({ scope, byName, unified, model, maxTestRows, nextestText, hook }) {
  const rows = [];
  const testTargets = (name) =>
    byName.get(name).targets.filter((target) => target.kind.length === 1 && target.kind[0] === "test");
  const hookBuilt = (name, target) =>
    target.test !== false && (target["required-features"] ?? []).every((feature) => unified.get(name).has(feature));
  const hasLib = (name) =>
    byName.get(name).targets.some((target) => target.test !== false && target.kind.some((kind) => LIB_TARGET_KINDS.has(kind)));

  // Lib tests: one row. `--workspace --lib` builds every member's lib test;
  // no stable selection builds fewer under the same resolution, so the scope
  // narrows what runs (a package() filter), not what builds.
  const libPackages = scope.filter(hasLib);
  if (libPackages.length > 0) {
    const everyLib = [...byName.keys()].filter(hasLib);
    const filter = libPackages.length === everyLib.length
      ? null
      : libPackages.map((name) => `package(${name})`).join(" | ");
    rows.push({
      name: "lib",
      lane: "lib",
      packages: libPackages,
      package_flags: "--workspace",
      test_flags: "--lib",
      profile: CLOSURE_LIB_PROFILE,
      ...(filter ? { filter } : {}),
      binaries: [...libPackages],
      no_tests: "warn",
      rust_min_stack: HOOK_TEST_STACK,
    });
  }

  // Integration tests: every hook-built tests/*.rs target name in scope that
  // the fast selection keeps. Under --workspace, `--test NAME` selects that
  // name in every member, so a name is one unit with all of its binaries;
  // a name a member cannot build would make Cargo refuse the row.
  const keep = parseFilter(profileFilter(nextestText, CLOSURE_TEST_PROFILE));
  const allNames = [...byName.keys()].sort();
  const home = new Map();
  for (const name of scope) {
    for (const target of testTargets(name)) {
      if (!hookBuilt(name, target) || !keep({ kind: "test", binary: target.name, package: name })) continue;
      if (!home.has(target.name)) home.set(target.name, name);
    }
  }
  const binariesOf = (test) => allNames.filter((name) => {
    const target = testTargets(name).find((candidate) => candidate.name === test);
    if (!target) return false;
    if (!hookBuilt(name, target)) {
      throw new Error(`test target name ${test} is hook-built in ${home.get(test)} but not in ${name}; --test ${test} would make Cargo refuse the row`);
    }
    return true;
  }).map((name) => `${name}::${test}`);
  const byHome = new Map();
  for (const [test, name] of home) byHome.set(name, [...(byHome.get(name) ?? []), test]);
  const units = [];
  for (const [name, tests] of [...byHome].sort(([a], [b]) => a.localeCompare(b))) {
    tests.sort();
    const parts = Math.ceil(tests.length / CLOSURE_TESTS_PER_ROW);
    for (let index = 0; index < parts; index += 1) {
      const slice = tests.slice(
        Math.floor((index * tests.length) / parts),
        Math.floor(((index + 1) * tests.length) / parts),
      );
      units.push({
        package: name,
        part: parts > 1 ? ` ${index + 1}/${parts}` : "",
        tests: slice,
        weight: model.lines.get(name) + LINK_COST_PER_DEP * model.closures.get(name).size + TEST_TARGET_COST * slice.length,
      });
    }
  }
  // Two parts of one package never share a row, or the split would not
  // parallelize; a target name lives in exactly one unit.
  units.sort((a, b) => b.weight - a.weight || a.package.localeCompare(b.package) || a.part.localeCompare(b.part));
  const bins = [];
  for (const unit of units) {
    const open = bins.filter((bin) => !bin.units.some((other) => other.package === unit.package));
    let bin;
    if (bins.length < maxTestRows || open.length === 0) {
      bin = { units: [], weight: 0 };
      bins.push(bin);
    } else {
      bin = open.sort((a, b) => a.weight - b.weight || a.units.length - b.units.length)[0];
    }
    bin.units.push(unit);
    bin.weight += unit.weight;
  }
  for (const bin of bins) {
    const tests = bin.units.flatMap((unit) => unit.tests).sort();
    const binaries = tests.flatMap(binariesOf);
    const packages = [...new Set(binaries.map((binary) => binary.split("::")[0]))].sort();
    const union = new Set();
    let lines = 0;
    for (const name of packages) {
      lines += model.lines.get(name);
      for (const dep of model.closures.get(name)) union.add(dep);
    }
    // Label by the heaviest part, which the packing placed first.
    const [heaviest] = bin.units;
    rows.push({
      name: `tests ${shortName(heaviest.package)}${heaviest.part}${bin.units.length > 1 ? `+${bin.units.length - 1}` : ""}`,
      lane: "tests",
      packages,
      package_flags: "--workspace",
      test_flags: tests.map((test) => `--test ${test}`).join(" "),
      profile: CLOSURE_TEST_PROFILE,
      binaries,
      no_tests: "warn",
      rust_min_stack: HOOK_TEST_STACK,
      estimated_minutes: estimatedMinutes(lines + LINK_COST_PER_DEP * union.size + TEST_TARGET_COST * binaries.length),
    });
  }

  // The hook's two named lanes: HeadCanonical with the hook's own
  // package-scoped flags, e2e-fast from the workspace build (both read from
  // the hook script by hookModel).
  const headCanonical = hook.headCanonical;
  if (scope.includes(headCanonical.package)) {
    if (!testTargets(headCanonical.package).some((target) => target.name === headCanonical.test)) {
      throw new Error(`HeadCanonical target ${headCanonical.test} is missing from ${headCanonical.package}`);
    }
    rows.push({
      name: "HeadCanonical",
      lane: "headcanonical",
      packages: [headCanonical.package],
      package_flags: `-p ${headCanonical.package} --features ${headCanonical.features.map((feature) => `${headCanonical.package}/${feature}`).join(",")}`,
      test_flags: `--test ${headCanonical.test}`,
      profile: CLOSURE_TEST_PROFILE,
      filter: `binary(${headCanonical.test})`,
      binaries: [`${headCanonical.package}::${headCanonical.test}`],
      no_tests: "fail",
      rust_min_stack: HOOK_TEST_STACK,
    });
  }
  const e2eOwners = allNames.filter((name) => testTargets(name).some((target) => target.name === hook.e2eFast.binary));
  if (e2eOwners.length !== 1) {
    throw new Error(`e2e-fast target ${hook.e2eFast.binary} belongs to ${e2eOwners.length} workspace packages`);
  }
  if (scope.includes(e2eOwners[0])) {
    rows.push({
      name: "e2e-fast",
      lane: "e2e-fast",
      packages: [e2eOwners[0]],
      package_flags: "--workspace",
      test_flags: `--test ${hook.e2eFast.binary}`,
      profile: CLOSURE_LIB_PROFILE,
      filter: `binary(${hook.e2eFast.binary})`,
      binaries: [`${e2eOwners[0]}::${hook.e2eFast.binary}`],
      no_tests: "fail",
      rust_min_stack: HOOK_TEST_STACK,
    });
  }
  const rowNames = rows.map((row) => row.name);
  if (new Set(rowNames).size !== rowNames.length) {
    throw new Error(`internal error: duplicate closure test row names: ${rowNames.join(", ")}`);
  }
  return rows;
}

// Clippy shards: per changed package (packed past the cap) for a
// changed-package plan, the fixed head groups plus packed shards for a
// whole-workspace plan (see CLIPPY_HEAD_SHARDS).
function clippyShards(pkgs, weights, mode, shardCap, byName) {
  if (mode !== "workspace") return packShards(pkgs, weights, shardCap);
  for (const name of CLIPPY_HEAD_SHARDS.flat()) {
    if (!byName.has(name)) throw new Error(`Clippy head shard names unknown package ${name}`);
  }
  const heads = CLIPPY_HEAD_SHARDS.map((group) => group.filter((name) => pkgs.includes(name))).filter((group) => group.length > 0);
  const inHeads = new Set(heads.flat());
  const headShards = heads.map((group) => packShards(group, weights, 1)[0]);
  const rest = packShards(pkgs.filter((name) => !inHeads.has(name)), weights, CLIPPY_PACKED_SHARDS);
  return [...headShards, ...rest].sort((a, b) => b.estimated_cost - a.estimated_cost || a.name.localeCompare(b.name));
}

function plan(args) {
  const metadata = readMetadata();
  const packages = workspacePackages(metadata);
  const byName = new Map(packages.map((pkg) => [pkg.name, pkg]));
  const byId = new Map(packages.map((pkg) => [pkg.id, pkg]));
  const dirs = packageDirs(packages);
  const allNames = packages.map((pkg) => pkg.name).sort();

  const reverse = new Map(packages.map((pkg) => [pkg.id, new Set()]));
  for (const pkg of packages) {
    for (const dep of pkg.dependencies) {
      if (dep.source !== null) continue;
      const depPkg = byName.get(dep.name);
      if (depPkg) reverse.get(depPkg.id)?.add(pkg.id);
    }
  }

  const { paths: rawPaths, removed: rawRemoved = [], reason: pathReason } = collectChangedPaths(args);
  const changed = rawPaths === null ? null : [...new Set(rawPaths.map(normalizePath))].sort();

  const result = {
    schema_version: 1,
    diff: pathReason,
    changed_paths: changed ?? [],
    rust_changed: false,
    mode: "none",
    reason: "",
    packages: [],
    closure: [],
    shards: [],
    unmapped_rust_paths: [],
    generated_contract: false,
    machine_authority: false,
    machine_posters: false,
    wasm: false,
    sdk_host: false,
    bazel_graph: false,
    examples_browser: false,
    example_web: false,
    docs_only: false,
  };

  let workspaceReason = "";
  const seeds = new Set();

  if (changed === null) {
    workspaceReason = pathReason;
  } else {
    for (const path of changed) {
      if (isGlobalPath(path)) {
        workspaceReason ||= `global build configuration changed: ${path}`;
        continue;
      }
      if (!isRustSourcePath(path)) {
        // Non-Rust paths can still select a package: fixtures, SQL, TOML,
        // JSON and snapshot data a crate reads at test time, and the
        // embedded inputs (for example the platform skill sources the facade
        // compiles in through include macros) that the selector maps to a
        // package from outside its directory. Only documentation that lives
        // inside the crate directory is not a build input.
        const owner = packageForFile(path, dirs);
        if (owner) {
          const insideOwner = path.startsWith(`${packageDir(owner)}/`);
          const isDoc = /\.(md|mdx)$/.test(path);
          if (!insideOwner || !isDoc) seeds.add(owner.name);
        }
        continue;
      }
      const owner = packageForFile(path, dirs);
      if (owner) {
        seeds.add(owner.name);
      } else {
        result.unmapped_rust_paths.push(path);
      }
    }
    if (result.unmapped_rust_paths.length > 0) {
      workspaceReason ||= `Rust path outside any workspace package: ${result.unmapped_rust_paths[0]}`;
    }
  }

  if (changed !== null) {
    result.generated_contract = runClassifier("scripts/generated-contract-ratchet-changed", changed);
    result.machine_authority = runClassifier("scripts/machine-authority-changed", changed);
    result.machine_posters = runClassifier("scripts/machine-posters-changed", changed);
    result.sdk_host = changed.some(
      (path) =>
        path.startsWith("sdks/python/") ||
        path.startsWith("sdks/typescript/") ||
        path.startsWith("sdks/test-fixtures/") ||
        path.startsWith("tools/sdk-codegen/") ||
        path.startsWith("tools/sdk-builder/"),
    );
    result.wasm = changed.some(
      (path) =>
        path.startsWith("crates/meerkat-web-runtime/") ||
        path.startsWith("crates/meerkat-contracts/") ||
        path.startsWith("crates/meerkat-cli/src/web_runtime_template/"),
    );
    const removed = rawRemoved.map(normalizePath);
    result.removed_paths = removed;
    result.bazel_graph =
      changed.some(isBazelGraphPath) || removed.some((path) => path.endsWith(".rs"));
    result.examples_browser = changed.some(isExamplesBrowserPath);
    result.example_web = changed.some(isExampleWebPath);
  } else {
    result.generated_contract = true;
    result.machine_authority = true;
    result.machine_posters = true;
    result.sdk_host = true;
    result.wasm = true;
    result.bazel_graph = true;
    result.examples_browser = true;
    result.example_web = true;
  }

  // A unit lane compiles the package's dependencies of every kind (its
  // dev-dependencies build its lib-test), then only normal and build
  // dependencies below that. Cargo never builds a dependency's dev fixtures.
  // Reuse this graph for both the link-cost estimate and the heavy chain.
  // Following nested dev edges put xtask in the mob chain (#1362) and priced
  // runtime as though it built authorization's native facade test fixtures.
  const buildClosure = (pkg) => {
    const seen = new Set();
    const queue = [];
    const visit = (dep) => {
      if (dep.source !== null) return;
      const depPkg = byName.get(dep.name);
      if (depPkg && !seen.has(depPkg.name)) {
        seen.add(depPkg.name);
        queue.push(depPkg);
      }
    };
    for (const dep of pkg.dependencies) visit(dep);
    while (queue.length) {
      for (const dep of queue.pop().dependencies) {
        if (dep.kind !== "dev") visit(dep);
      }
    }
    return seen;
  };
  // Estimated lane cost per package: Rust lines (the lib-test binary
  // compiles every inline test) plus a link term per compiled workspace crate.
  const closures = new Map(packages.map((pkg) => [pkg.name, buildClosure(pkg)]));
  const lineCounts = new Map(packages.map((pkg) => [pkg.name, rustLineCount(resolve(root, packageDir(pkg)))]));
  const weights = new Map(
    packages.map((pkg) => [pkg.name, lineCounts.get(pkg.name) + LINK_COST_PER_DEP * closures.get(pkg.name).size]),
  );
  const model = { lines: lineCounts, closures };
  // Crates whose unit lane compiles meerkat-mob still run their unit tests
  // on push to main, with no pull-request budget. Their clippy always runs.
  const heavyChain = new Set(
    packages
      .filter((pkg) => pkg.name === HEAVY_ANCHOR || closures.get(pkg.name).has(HEAVY_ANCHOR))
      .map((pkg) => pkg.name),
  );
  result.unit_deferred_chain = [...heavyChain].sort();
  result.package_model = Object.fromEntries(
    allNames.map((name) => [name, {
      lines: lineCounts.get(name),
      dependency_closure: closures.get(name).size,
      estimated_minutes: estimatedMinutes(weights.get(name)),
      heavy_chain: heavyChain.has(name),
    }]),
  );
  result.pr_unit_budget_minutes = PR_UNIT_BUDGET_MINUTES;
  const featureSuites = FEATURE_UNIT_SUITES.map((suite) => {
    const pkg = byName.get(suite.package);
    if (!pkg) throw new Error(`feature unit suite names unknown package ${suite.package}`);
    for (const feature of suite.features) {
      if (!Object.hasOwn(pkg.features, feature)) {
        throw new Error(`feature unit suite ${suite.package}[${suite.id}] names unknown feature ${feature}`);
      }
    }
    return {
      name: `${shortName(suite.package)}[${suite.id}]`,
      packages: [suite.package],
      features: [...suite.features],
      package_flags: `-p ${suite.package} --features ${suite.features.join(",")}`,
      estimated_minutes: estimatedMinutes(weights.get(suite.package)),
      ...(suite.rustMinStack ? { rust_min_stack: suite.rustMinStack } : {}),
    };
  });

  for (const exception of PR_UNIT_CHAIN_EXCEPTIONS) {
    for (const name of [exception.package, ...exception.triggers]) {
      if (!byName.has(name)) {
        throw new Error(`pull-request unit exception ${exception.package} names unknown package ${name}`);
      }
    }
  }

  const archivedPackages = new Map(
    ARCHIVED_UNIT_LANES.map((lane) => {
      if (!byName.has(lane.package)) {
        throw new Error(`archived unit lane names unknown package ${lane.package}`);
      }
      if (!Number.isInteger(lane.partitions) || lane.partitions < 2) {
        throw new Error(`archived unit lane ${lane.package} needs at least two partitions`);
      }
      return [lane.package, lane];
    }),
  );

  if (workspaceReason) {
    result.rust_changed = true;
    result.mode = "workspace";
    result.reason = workspaceReason;
    result.packages = allNames;
    result.closure = allNames;
    result.wasm = true;
  } else if (seeds.size > 0) {
    result.rust_changed = true;
    result.mode = "packages";
    result.reason = `${seeds.size} workspace package(s) own changed paths`;
    result.packages = [...seeds].sort();
    const seen = new Set(result.packages.map((name) => byName.get(name).id));
    const queue = [...seen];
    for (let i = 0; i < queue.length; i += 1) {
      for (const dependent of reverse.get(queue[i]) ?? []) {
        if (!seen.has(dependent)) {
          seen.add(dependent);
          queue.push(dependent);
        }
      }
    }
    result.closure = [...seen].map((id) => byId.get(id).name).sort();
  } else {
    result.mode = "none";
    result.reason = "no Rust build-relevant paths changed";
    result.docs_only = changed !== null && changed.every(
      (path) => /^(docs\/|docs\/internal\/|CHANGELOG\.md|README\.md|AGENTS\.md|CLAUDE\.md|.*\.mdx?$)/.test(path),
    );
  }

  // The browser examples and WASM timer tests execute this runtime. Select
  // them for any changed package that reaches it through the same reverse
  // dependency closure, including a whole-workspace build change.
  if (result.closure.includes("meerkat-web-runtime")) {
    result.wasm = true;
    result.example_web = true;
  }

  if (result.rust_changed) {
    const shardCap = result.mode === "workspace" ? args.workspaceShards : args.maxShards;
    // Clippy lanes: every changed package.
    result.shards = clippyShards(result.packages, weights, result.mode, shardCap, byName);
    if (result.shards.length === 0) {
      throw new Error("internal error: Rust-relevant change produced no lanes");
    }
    const covered = new Set(result.shards.flatMap((shard) => shard.packages));
    for (const name of result.packages) {
      if (!covered.has(name)) throw new Error(`internal error: package ${name} not covered by any shard`);
    }
    // Pull-request unit lanes: the changed packages outside the heavy chain,
    // plus each exception package one of whose triggers changed, packed so
    // that no lane models over the budget; the rest of the chain is deferred
    // to the push-to-main run and reported.
    result.unit_chain_exceptions = PR_UNIT_CHAIN_EXCEPTIONS.filter((exception) =>
      exception.triggers.some((name) => result.packages.includes(name)),
    ).map((exception) => exception.package);
    const runsInPullRequest = (name) => !heavyChain.has(name) || result.unit_chain_exceptions.includes(name);
    result.unit_deferred = result.packages.filter((name) => !runsInPullRequest(name));
    result.unit_packages = [...new Set([...result.packages, ...result.unit_chain_exceptions])]
      .filter(runsInPullRequest)
      .sort();
    result.unit_shards = packWithinBudget(result.unit_packages, weights, shardCap, model);
    for (const shard of result.unit_shards) {
      if (shard.estimated_minutes > PR_UNIT_BUDGET_MINUTES) {
        throw new Error(
          `internal error: pull-request unit lane ${shard.name} models ${shard.estimated_minutes} min, over the ${PR_UNIT_BUDGET_MINUTES} min budget`,
        );
      }
    }
    // Push-to-main unit lanes: the whole workspace, no budget. Archived
    // packages leave the packed shards for their build-once lanes.
    result.main_unit_shards = packShards(
      allNames.filter((name) => !archivedPackages.has(name)),
      weights,
      args.workspaceShards,
      model,
    );
    const archived = archivedLanes(archivedPackages, featureSuites);
    result.main_archive_builds = archived.builds;
    result.main_archive_runs = archived.runs;
    // Feature-gated suites: all of them on main (an archived package's
    // suites run as archived lanes); in the pull request those of a changed
    // package that runs its unit tests there. The heavy chain's suites stay
    // deferred with the package, an exception package's too: the exception
    // covers its default-feature row only.
    result.main_feature_unit_shards = featureSuites.filter(
      (suite) => !archivedPackages.has(suite.packages[0]),
    );
    result.unit_feature_shards = featureSuites.filter(
      (suite) => result.unit_packages.includes(suite.packages[0]) && !heavyChain.has(suite.packages[0]),
    );
    for (const suite of result.unit_feature_shards) {
      if (suite.estimated_minutes > PR_UNIT_BUDGET_MINUTES) {
        throw new Error(
          `internal error: pull-request feature suite ${suite.name} models ${suite.estimated_minutes} min, over the ${PR_UNIT_BUDGET_MINUTES} min budget`,
        );
      }
    }
    // Feature checks keep the weight-packed shards: they compile one Bazel
    // set and a few matrix rows per shard, and the head split would only
    // multiply their jobs.
    result.feature_check_shards = packShards(result.packages, weights, shardCap);
    result.feature_check_jobs = featureChecks(result.feature_check_shards, byName);
  } else {
    result.unit_chain_exceptions = [];
    result.unit_deferred = [];
    result.unit_packages = [];
    result.unit_shards = [];
    result.main_unit_shards = [];
    result.main_feature_unit_shards = [];
    result.unit_feature_shards = [];
    result.main_archive_builds = [];
    result.main_archive_runs = [];
    result.feature_check_shards = [];
    result.feature_check_jobs = [];
  }

  // Integration suites of the directly changed packages' triggers. Package
  // names are checked against cargo metadata, so a rename fails the plan
  // instead of silently dropping a suite.
  for (const suite of INTEGRATION_SUITES) {
    for (const name of [suite.package, ...suite.triggers]) {
      if (!byName.has(name)) throw new Error(`integration suite ${suite.package} names unknown package ${name}`);
    }
    for (const feature of suite.features ?? []) {
      if (!Object.hasOwn(byName.get(suite.package).features, feature)) {
        throw new Error(`integration suite ${suite.package} names unknown feature ${feature}`);
      }
    }
    for (const test of suite.tests ?? []) {
      const known = byName
        .get(suite.package)
        .targets.some((target) => target.kind.includes("test") && target.name === test);
      if (!known) throw new Error(`integration suite ${suite.package} names unknown test target ${test}`);
    }
  }
  const changedPaths = changed ?? [];
  result.integration_suites = INTEGRATION_SUITES.filter(
    (suite) =>
      (result.rust_changed && suite.triggers.some((name) => result.packages.includes(name))) ||
      (suite.paths ?? []).some((prefix) => changedPaths.some((path) => path.startsWith(prefix))),
  ).map((suite) => ({
    name: suite.name ?? shortName(suite.package),
    packages: [suite.package],
    package_flags: `-p ${suite.package}${suite.features?.length ? ` --features ${suite.features.join(",")}` : ""}`,
    ...(suite.tests?.length ? { test_flags: suite.tests.map((test) => `--test ${test}`).join(" ") } : {}),
  }));

  result.closure_flags = result.closure.map((name) => `-p ${name}`).join(" ");
  result.closure_beyond_packages = result.closure.filter((name) => !result.packages.includes(name));

  // Hook-parity rows for the test scope. scripts/ci-hook-coverage.mjs checks
  // them against the hook's own selection; a gap fails the plan.
  const nextestText = readFileSync(resolve(root, ".config/nextest.toml"), "utf8");
  const hookText = readFileSync(resolve(root, HOOK_SCRIPT), "utf8");
  result.reader_packages = changed === null ? [] : readerPackagesFor(changed, loadNonRustReaders());
  for (const name of result.reader_packages) {
    if (!byName.has(name)) throw new Error(`the non-Rust reader inventory names unknown package ${name}`);
  }
  result.test_scope = [...new Set([...result.closure, ...result.reader_packages])].sort();
  result.closure_test_rows = result.test_scope.length > 0
    ? hookParityRows({
      scope: result.test_scope,
      byName,
      unified: workspaceFeatures(metadata, packages),
      model,
      maxTestRows: args.closureTestRows,
      nextestText,
      hook: hookModel(hookText),
    })
    : [];
  const coverage = checkHookCoverage(result, { metadata, nextestText, hookText });
  result.hook_coverage = {
    scope_packages: coverage.scope_packages,
    hook_binaries: coverage.hook_binaries,
    by_lane: coverage.by_lane,
    missing: coverage.missing,
    errors: coverage.errors,
  };
  if (coverage.missing.length > 0 || coverage.errors.length > 0) {
    throw new Error(
      `the plan does not cover the pre-push hook's tests for its scope: missing ${coverage.missing.join(", ") || "none"}; errors ${coverage.errors.join("; ") || "none"}`,
    );
  }
  return result;
}

// Build-once lanes for ARCHIVED_UNIT_LANES: one archive build per package
// default-feature lane and per feature suite of that package, and
// `partitions` run rows per archive that execute it with
// `--partition hash:k/n`.
function archivedLanes(archivedPackages, featureSuites) {
  const builds = [];
  const runs = [];
  const add = (archive, name, packageFlags, partitions, rustMinStack) => {
    builds.push({
      name,
      archive,
      package_flags: packageFlags,
      partitions,
      ...(rustMinStack ? { rust_min_stack: rustMinStack } : {}),
    });
    for (let index = 1; index <= partitions; index += 1) {
      runs.push({
        name: `${name} ${index}/${partitions}`,
        archive,
        partition: `hash:${index}/${partitions}`,
        ...(rustMinStack ? { rust_min_stack: rustMinStack } : {}),
      });
    }
  };
  for (const [pkg, lane] of [...archivedPackages].sort(([a], [b]) => a.localeCompare(b))) {
    add(shortName(pkg), shortName(pkg), `-p ${pkg}`, lane.partitions, null);
    for (const suite of featureSuites.filter((candidate) => candidate.packages[0] === pkg)) {
      add(
        suite.name.replaceAll(/[^A-Za-z0-9_-]+/g, "-").replace(/-+$/, ""),
        suite.name,
        suite.package_flags,
        lane.partitions,
        suite.rust_min_stack,
      );
    }
  }
  return { builds, runs };
}

// Pack, then widen the shard count until every shard models under the
// pull-request unit budget (a single package over budget cannot be split and
// is reported by the caller).
function packWithinBudget(pkgs, weights, maxShards, model) {
  if (pkgs.length === 0) return [];
  let count = Math.min(maxShards, pkgs.length);
  for (;;) {
    const shards = packShards(pkgs, weights, count, model);
    const over = shards.some((shard) => shard.estimated_minutes > PR_UNIT_BUDGET_MINUTES && shard.packages.length > 1);
    if (!over || count >= pkgs.length) return shards;
    count += 1;
  }
}

// A consumer job that runs is guarded by `if:` on the corresponding count,
// so an empty plan gets a placeholder only where GitHub would otherwise
// reject an empty matrix on a job that still runs (never for the
// push-to-main unit lanes, which are skipped outright for a plan with no
// Rust-relevant change; run 35939276400 failed a "none" shard there).
function archiveMatrixOf(rows) {
  return JSON.stringify({
    include: rows.map((row) => ({
      name: row.name,
      archive: row.archive,
      ...(row.package_flags ? { packages: row.package_flags } : {}),
      ...(row.partitions ? { partitions: String(row.partitions) } : {}),
      ...(row.partition ? { partition: row.partition } : {}),
      ...(row.rust_min_stack ? { rust_min_stack: String(row.rust_min_stack) } : {}),
    })),
  });
}

function matrixOf(shards, { placeholder = true } = {}) {
  const include = shards.map((shard) => ({
    name: shard.name,
    packages: shard.package_flags,
    ...(shard.test_flags ? { tests: shard.test_flags } : {}),
    ...(shard.rust_min_stack ? { rust_min_stack: String(shard.rust_min_stack) } : {}),
  }));
  if (include.length === 0 && placeholder) include.push({ name: "none", packages: "" });
  return JSON.stringify({ include });
}

// The closure-tests job's rows: package and target flags, the nextest
// profile, an optional filter, the binaries the row must list before it runs,
// its no-tests policy and its test-thread stack.
function closureMatrixOf(rows) {
  const include = rows.map((row) => ({
    name: row.name,
    packages: row.package_flags,
    tests: row.test_flags,
    profile: row.profile,
    filter: row.filter ?? "",
    binaries: row.binaries.join(" "),
    no_tests: row.no_tests,
    rust_min_stack: String(row.rust_min_stack),
  }));
  if (include.length === 0) include.push({ name: "none", packages: "" });
  return JSON.stringify({ include });
}

function githubOutput(result) {
  const lines = [];
  const scalar = (key, value) => lines.push(`${key}=${value}`);
  scalar("rust_changed", String(result.rust_changed));
  scalar("mode", result.mode);
  scalar("reason", result.reason.replaceAll("\n", " "));
  scalar("generated_contract", String(result.generated_contract));
  scalar("machine_authority", String(result.machine_authority));
  scalar("machine_posters", String(result.machine_posters));
  scalar("wasm", String(result.wasm));
  scalar("sdk_host", String(result.sdk_host));
  scalar("bazel_graph", String(result.bazel_graph));
  scalar("examples_browser", String(result.examples_browser));
  scalar("example_web", String(result.example_web));
  scalar("docs_only", String(result.docs_only));
  scalar("package_count", String(result.packages.length));
  scalar("closure_count", String(result.closure.length));
  scalar("closure_flags", result.closure_flags);
  scalar("shard_count", String(result.shards.length));
  // A matrix must never be empty when the lanes are conditioned on it; keep a
  // placeholder so `fromJSON` stays well-formed and the consumer job's `if:`
  // decides whether it runs.
  scalar("shard_matrix", matrixOf(result.shards));
  // The feature-gated suites run in the same unit jobs as extra matrix rows.
  const unitLanes = [...result.unit_shards, ...result.unit_feature_shards];
  const mainUnitLanes = [...result.main_unit_shards, ...result.main_feature_unit_shards];
  scalar("unit_shard_count", String(unitLanes.length));
  scalar("unit_shard_matrix", matrixOf(unitLanes));
  scalar("unit_deferred", result.unit_deferred.join(" "));
  scalar("unit_deferred_count", String(result.unit_deferred.length));
  scalar("main_unit_shard_count", String(mainUnitLanes.length));
  scalar("main_unit_shard_matrix", matrixOf(mainUnitLanes, { placeholder: false }));
  scalar("main_archive_build_count", String(result.main_archive_builds.length));
  scalar("main_archive_build_matrix", archiveMatrixOf(result.main_archive_builds));
  scalar("main_archive_run_count", String(result.main_archive_runs.length));
  scalar("main_archive_run_matrix", archiveMatrixOf(result.main_archive_runs));
  scalar("integration_count", String(result.integration_suites.length));
  scalar("integration_matrix", matrixOf(result.integration_suites));
  scalar("test_scope_count", String(result.test_scope.length));
  scalar("hook_binary_count", String(result.hook_coverage.hook_binaries));
  scalar("closure_test_count", String(result.closure_test_rows.length));
  scalar("closure_test_matrix", closureMatrixOf(result.closure_test_rows));
  scalar("feature_check_count", String(result.feature_check_jobs.length));
  scalar(
    "feature_check_matrix",
    JSON.stringify({
      include: result.feature_check_jobs.length > 0
        ? result.feature_check_jobs.map((job) => ({ name: job.name, commands: job.commands.join("\n") }))
        : [{ name: "none", commands: "" }],
    }),
  );
  return `${lines.join("\n")}\n`;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.changedPathsOnly) {
    // The path collection alone, without Cargo metadata, for the actual-commit
    // controls in scripts/test-release-projection-pre-push.sh.
    const { paths, removed = [], reason } = collectChangedPaths(args);
    process.stdout.write(`${JSON.stringify({ diff: reason, changed_paths: paths, removed_paths: removed })}\n`);
    return;
  }
  const result = plan(args);
  if (args.format === "github") {
    process.stdout.write(githubOutput(result));
  } else {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  }
  process.stderr.write(
    `ci-cargo-lanes: mode=${result.mode} packages=${result.packages.length} closure=${result.closure.length} clippy_shards=${result.shards.length} unit_shards=${result.unit_shards.length} feature_suites=${result.unit_feature_shards.length}/${result.main_feature_unit_shards.length} unit_deferred=${result.unit_deferred.length} unit_chain_exceptions=${result.unit_chain_exceptions.length} integration=${result.integration_suites.length} feature_check_jobs=${result.feature_check_jobs.length} test_scope=${result.test_scope.length} closure_tests=${result.closure_test_rows.length} hook_binaries=${result.hook_coverage.hook_binaries} (${result.reason})\n`,
  );
}

try {
  main();
} catch (error) {
  process.stderr.write(`ci-cargo-lanes: ${error?.message ?? error}\n`);
  process.exit(1);
}
