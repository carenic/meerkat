#!/usr/bin/env node
// The CI gate and the jobs it judges must agree. The gate's "Check lane
// results" shell (ci.yml, job gate) decides which lanes a plan requires, and
// each lane's job-level if: decides whether the lane runs. The two are
// written apart: pull request 1904 first required the WASM timer lane on a
// plan whose job condition skipped it, and the same change ran that lane on
// release projections, whose gate contract runs no wasm lane.
//
// For each plan below this selftest runs the planner as the changes job
// does. It evaluates every job's if: with GitHub's needs semantics (a job
// runs only when every job it needs succeeded, unless its condition calls a
// status function), gives each job that runs a success, and runs the gate's
// own shell on those results: the gate must pass. Then, for each job that
// ran, the gate must fail when that job fails, and when it is skipped
// instead (its dependents re-evaluated), unless ALLOWED_UNNOTICED names it
// with the reason.
//
// The workflow is read as text, like the other CI selftests. A job-level
// if: or needs: this reader cannot parse, an expression it does not model,
// or a changes output that is not the planner's fails the selftest.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const planner = resolve(here, "ci-cargo-lanes.mjs");

// The real v0.9.1 release bump and its first parent (see
// ci-release-projection-selftest.mjs): the release-projection plan.
const BUMP_PARENT = "1a4d341922da75ce7524f36b84b667494615e8ed";
const BUMP = "a954c519f592fdad7f47c5bcc7e40a23750e790a";

// A lane that ran and that the gate does not miss when it is skipped
// instead, keyed by event and job.
const ALLOWED_UNNOTICED = new Map([
  [
    "workflow_dispatch:main-unit-archive-build",
    "a dispatch's gate requires the post-merge lanes only on a push; the archived main-unit lanes only forbid failure there",
  ],
  [
    "workflow_dispatch:main-unit-archive-run",
    "a dispatch's gate requires the post-merge lanes only on a push; the archived main-unit lanes only forbid failure there",
  ],
]);

const SCENARIOS = [];
for (const event of ["pull_request", "push"]) {
  for (const [label, paths] of [
    ["docs only", ["docs/reference/build-and-ci.mdx"]],
    ["meerkat-core", ["crates/meerkat-core/src/lib.rs"]],
    ["the web runtime", ["crates/meerkat-web-runtime/src/lib.rs"]],
    ["the web runtime and a crate it builds on", ["crates/meerkat-web-runtime/src/lib.rs", "crates/meerkat-core/src/lib.rs"]],
    ["meerkat-cli", ["crates/meerkat-cli/src/main.rs"]],
    ["example 031", ["examples/031-wasm-mini-diplomacy-sh/web/package.json"]],
    ["the Web SDK", ["sdks/web/src/index.ts"]],
    ["the CI workflow", [".github/workflows/ci.yml"]],
  ]) {
    SCENARIOS.push([`${event}: ${label}`, event, ["--", ...paths]]);
  }
  SCENARIOS.push([
    `${event}: release projection`,
    event,
    ["--base", BUMP_PARENT, "--head", BUMP, "--reuse-release-evidence", `${BUMP_PARENT}:4242:2`],
  ]);
}
SCENARIOS.push(["workflow_dispatch: no base (whole workspace)", "workflow_dispatch", ["--base", "", "--head", "HEAD"]]);

// --- The workflow, read as text. ---

function readJobs(text) {
  const lines = text.split("\n");
  const start = lines.indexOf("jobs:");
  assert.ok(start >= 0, "ci.yml has a jobs: map");
  const jobs = [];
  let job = null;
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\S/.test(line)) break;
    const header = line.match(/^ {2}([A-Za-z0-9_-]+):\s*$/);
    if (header) {
      job = { name: header[1], needs: [], if: null, outputs: null, line: index + 1 };
      jobs.push(job);
      continue;
    }
    if (!job) continue;
    const needs = line.match(/^ {4}needs:\s*(.*)$/);
    if (needs) {
      const value = needs[1].trim();
      if (value === "") {
        for (let next = index + 1; next < lines.length && /^ {6}- /.test(lines[next]); next += 1) {
          job.needs.push(lines[next].replace(/^ {6}- /, "").trim());
        }
      } else if (value.startsWith("[")) {
        assert.match(value, /^\[[A-Za-z0-9_, -]*\]$/, `${job.name}: needs: ${value} (line ${index + 1})`);
        job.needs = value.slice(1, -1).split(",").map((name) => name.trim()).filter(Boolean);
      } else {
        assert.match(value, /^[A-Za-z0-9_-]+$/, `${job.name}: needs: ${value} (line ${index + 1})`);
        job.needs = [value];
      }
      continue;
    }
    const condition = line.match(/^ {4}if:\s*(.*)$/);
    if (condition) {
      const wrapped = condition[1].match(/^\$\{\{(.*)\}\}\s*$/);
      assert.ok(wrapped, `${job.name}: this selftest reads a one-line if: \${{ ... }} only (line ${index + 1})`);
      job.if = wrapped[1];
      continue;
    }
    if (/^ {4}outputs:\s*$/.test(line)) {
      job.outputs = new Map();
      for (let next = index + 1; next < lines.length && /^ {6}\S/.test(lines[next]); next += 1) {
        const output = lines[next].match(/^ {6}([a-z0-9_]+): \$\{\{ steps\.plan\.outputs\.([a-z0-9_]+) \}\}\s*$/);
        assert.ok(output, `${job.name}: output ${lines[next].trim()} is not one of the planner's (line ${next + 1})`);
        job.outputs.set(output[1], output[2]);
      }
    }
  }
  const names = new Set(jobs.map((one) => one.name));
  const seen = new Set();
  for (const one of jobs) {
    for (const need of one.needs) {
      assert.ok(names.has(need), `${one.name} needs ${need}, which is not a job`);
      assert.ok(seen.has(need), `${one.name} needs ${need}, which comes after it: this selftest evaluates jobs in file order`);
    }
    seen.add(one.name);
  }
  return jobs;
}

// The env map and the run block of the named step of the named job.
function readStep(text, jobName, stepName) {
  const lines = text.split("\n");
  const jobAt = lines.indexOf(`  ${jobName}:`);
  assert.ok(jobAt >= 0, `ci.yml has a job ${jobName}`);
  let at = -1;
  for (let index = jobAt + 1; index < lines.length && !/^ {2}\S/.test(lines[index]); index += 1) {
    if (lines[index].trim() === `- name: ${stepName}`) {
      at = index;
      break;
    }
  }
  assert.ok(at >= 0, `job ${jobName} has a step named ${stepName}`);
  const stepIndent = lines[at].indexOf("-");
  const env = {};
  let run = null;
  for (let index = at + 1; index < lines.length; index += 1) {
    const indent = lines[index].search(/\S/);
    if (indent >= 0 && indent <= stepIndent) break;
    if (/^\s+env:\s*$/.test(lines[index])) {
      const envIndent = indent;
      for (index += 1; index < lines.length; index += 1) {
        const entryIndent = lines[index].search(/\S/);
        if (entryIndent >= 0 && entryIndent <= envIndent) break;
        const entry = lines[index].match(/^\s+([A-Z0-9_]+): (.*)$/);
        if (entry) env[entry[1]] = entry[2];
      }
      index -= 1;
      continue;
    }
    if (/^\s+run: \|\s*$/.test(lines[index])) {
      const runIndent = indent;
      const body = [];
      for (index += 1; index < lines.length; index += 1) {
        const line = lines[index];
        if (line.trim() !== "" && line.search(/\S/) <= runIndent) break;
        body.push(line);
      }
      const bodyIndent = Math.min(...body.filter((line) => line.trim() !== "").map((line) => line.search(/\S/)));
      run = body.map((line) => line.slice(bodyIndent)).join("\n");
      break;
    }
  }
  assert.ok(run !== null && Object.keys(env).length > 0, `step ${stepName} has an env map and a run block`);
  return { env, run };
}

// --- GitHub expressions: the subset ci.yml's conditions use. ---

const TOKEN = /\s*(?:(\|\||&&|==|!=|!|\(|\)|,)|'((?:[^']|'')*)'|([A-Za-z_][A-Za-z0-9_.-]*)|(\d+))/y;

function tokenize(text) {
  const tokens = [];
  const source = text.trim();
  let position = 0;
  while (position < source.length) {
    TOKEN.lastIndex = position;
    const match = TOKEN.exec(source);
    assert.ok(match && match[0].length > 0, `cannot read ${JSON.stringify(source.slice(position))} in ${JSON.stringify(source)}`);
    if (match[1]) tokens.push(["op", match[1]]);
    else if (match[2] !== undefined) tokens.push(["str", match[2].replaceAll("''", "'")]);
    else if (match[3]) tokens.push(["id", match[3]]);
    else tokens.push(["num", Number(match[4])]);
    position = TOKEN.lastIndex;
  }
  return tokens;
}

function truthy(value) {
  return !(value === false || value === null || value === undefined || value === 0 || value === "");
}

function equal(left, right) {
  if (typeof left === "string" && typeof right === "string") return left.toLowerCase() === right.toLowerCase();
  assert.equal(typeof left, typeof right, `this selftest does not model comparing ${typeof left} with ${typeof right}`);
  return left === right;
}

function evaluate(expression, context) {
  const tokens = tokenize(expression);
  let at = 0;
  const isOp = (value) => tokens[at]?.[0] === "op" && tokens[at][1] === value;
  const expectOp = (value) => {
    assert.ok(isOp(value), `expected ${value} in ${expression}`);
    at += 1;
  };
  const primary = () => {
    const [kind, value] = tokens[at] ?? [];
    if (kind === "op" && value === "(") {
      at += 1;
      const inner = or();
      expectOp(")");
      return inner;
    }
    if (kind === "str" || kind === "num") {
      at += 1;
      return value;
    }
    if (kind === "id") {
      at += 1;
      if (isOp("(")) {
        at += 1;
        assert.ok(isOp(")"), `this selftest does not model ${value}() with arguments, in ${expression}`);
        at += 1;
        return context.call(value);
      }
      return context.lookup(value);
    }
    assert.fail(`unexpected ${kind} ${value} in ${expression}`);
  };
  const compare = () => {
    let left = primary();
    while (isOp("==") || isOp("!=")) {
      const op = tokens[at][1];
      at += 1;
      const right = primary();
      left = op === "==" ? equal(left, right) : !equal(left, right);
    }
    return left;
  };
  const not = () => {
    if (isOp("!")) {
      at += 1;
      return !truthy(not());
    }
    return compare();
  };
  const and = () => {
    let left = not();
    while (isOp("&&")) {
      at += 1;
      const right = not();
      left = truthy(left) ? right : left;
    }
    return left;
  };
  const or = () => {
    let left = and();
    while (isOp("||")) {
      at += 1;
      const right = and();
      left = truthy(left) ? left : right;
    }
    return left;
  };
  const value = or();
  assert.equal(at, tokens.length, `trailing tokens in ${expression}`);
  return value;
}

function render(text, context) {
  return text.replace(/\$\{\{(.*?)\}\}/g, (_, expression) => {
    const value = evaluate(expression, context);
    if (typeof value === "boolean") return value ? "true" : "false";
    return value === null || value === undefined ? "" : String(value);
  });
}

function contextFor(event, outputNames, outputs, results) {
  return {
    lookup(path) {
      if (path === "github.event_name") return event;
      if (path === "needs.changes.result") return results.get("changes") ?? "success";
      const output = path.match(/^needs\.changes\.outputs\.([a-z0-9_]+)$/);
      if (output) {
        assert.ok(outputNames.has(output[1]), `${path} is not an output of the changes job`);
        return outputs.get(outputNames.get(output[1])) ?? "";
      }
      const result = path.match(/^needs\.([A-Za-z0-9_-]+)\.result$/);
      if (result) {
        assert.ok(results.has(result[1]), `${path} is read before that job is decided`);
        return results.get(result[1]);
      }
      assert.fail(`this selftest does not model ${path}`);
    },
    call(name) {
      if (name === "cancelled") return false;
      if (name === "always") return true;
      assert.fail(`this selftest does not model ${name}()`);
    },
  };
}

// --- The planner, the jobs and the gate. ---

function planOutputs(event, args) {
  const result = spawnSync("node", [planner, "--format", "github", "--event", event, ...args], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  assert.equal(result.status, 0, `the planner failed for ${event} ${args.join(" ")}: ${result.stderr}`);
  const outputs = new Map();
  const lines = result.stdout.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const heredoc = lines[index].match(/^([a-z0-9_]+)<<(\S+)$/);
    if (heredoc) {
      const body = [];
      for (index += 1; index < lines.length && lines[index] !== heredoc[2]; index += 1) body.push(lines[index]);
      assert.ok(index < lines.length, `the planner's ${heredoc[1]} output is not closed`);
      outputs.set(heredoc[1], body.join("\n"));
      continue;
    }
    const pair = lines[index].match(/^([a-z0-9_]+)=(.*)$/);
    if (pair) outputs.set(pair[1], pair[2]);
  }
  return outputs;
}

function simulate(jobs, event, outputNames, outputs, forced = new Map()) {
  const results = new Map();
  for (const job of jobs) {
    if (job.name === "gate") continue;
    if (forced.has(job.name)) {
      results.set(job.name, forced.get(job.name));
      continue;
    }
    const context = contextFor(event, outputNames, outputs, results);
    const statusAware = job.if !== null && /\b(always|cancelled|failure|success)\s*\(/.test(job.if);
    const needsMet = job.needs.every((need) => results.get(need) === "success");
    const runs = statusAware
      ? truthy(evaluate(job.if, context))
      : needsMet && (job.if === null || truthy(evaluate(job.if, context)));
    results.set(job.name, runs ? "success" : "skipped");
  }
  return results;
}

function runGate(step, event, outputNames, outputs, results) {
  const context = contextFor(event, outputNames, outputs, results);
  const env = { PATH: process.env.PATH, HOME: process.env.HOME ?? "/tmp" };
  for (const [key, value] of Object.entries(step.env)) env[key] = render(value, context);
  const run = spawnSync("bash", ["-c", step.run], { env, encoding: "utf8" });
  return { status: run.status, errors: run.stdout.split("\n").filter((line) => line.startsWith("::error::")) };
}

// The release-projection plan needs the real bump; CI checks out depth 2.
function hasCommit(sha) {
  return spawnSync("git", ["cat-file", "-e", `${sha}^{commit}`], { cwd: root }).status === 0;
}
if (!hasCommit(BUMP) || !hasCommit(BUMP_PARENT)) {
  const fetched = spawnSync("git", ["fetch", "--no-tags", "--depth=2", "origin", BUMP], { cwd: root, stdio: "inherit" });
  assert.equal(fetched.status, 0, `could not fetch the v0.9.1 bump ${BUMP} from origin`);
}

// The reader and the evaluator on known inputs.
assert.equal(evaluate("github.event_name == 'PUSH' && !cancelled()", contextFor("push", new Map(), new Map(), new Map())), true);
assert.equal(evaluate("'a' == 'b' || 'c'", contextFor("push", new Map(), new Map(), new Map())), "c");
assert.throws(() => evaluate("github.ref == 'x'", contextFor("push", new Map(), new Map(), new Map())), /does not model github\.ref/);
assert.throws(() => evaluate("contains('a', 'b')", contextFor("push", new Map(), new Map(), new Map())), /does not model contains/);

const workflow = readFileSync(resolve(root, ".github/workflows/ci.yml"), "utf8");
const jobs = readJobs(workflow);
const changes = jobs.find((job) => job.name === "changes");
assert.ok(changes?.outputs?.size > 0, "the changes job declares the planner's outputs");
const gateJob = jobs.find((job) => job.name === "gate");
assert.ok(gateJob, "ci.yml has a gate job");
assert.deepEqual(
  jobs.filter((job) => job.name !== "gate" && !gateJob.needs.includes(job.name)).map((job) => job.name),
  [],
  "the gate needs every other job, so it sees each result",
);
const gate = readStep(workflow, "gate", "Check lane results");

const unnoticedUsed = new Set();
for (const [label, event, args] of SCENARIOS) {
  const outputs = planOutputs(event, args);
  const results = simulate(jobs, event, changes.outputs, outputs);
  const consistent = runGate(gate, event, changes.outputs, outputs, results);
  assert.equal(
    consistent.status,
    0,
    `${label}: the gate refuses a run in which every lane the plan selected ran and passed: ${consistent.errors.join(" ")}`,
  );
  for (const [name, result] of results) {
    if (name === "changes" || result !== "success") continue;
    const failed = new Map(results);
    failed.set(name, "failure");
    assert.notEqual(runGate(gate, event, changes.outputs, outputs, failed).status, 0, `${label}: the gate passes a failed ${name}`);
    const skipped = simulate(jobs, event, changes.outputs, outputs, new Map([[name, "skipped"]]));
    if (runGate(gate, event, changes.outputs, outputs, skipped).status === 0) {
      const key = `${event}:${name}`;
      assert.ok(ALLOWED_UNNOTICED.has(key), `${label}: the gate passes a run in which ${name} did not run although its condition selected it`);
      unnoticedUsed.add(key);
    }
  }
}
for (const key of ALLOWED_UNNOTICED.keys()) {
  assert.ok(unnoticedUsed.has(key), `ALLOWED_UNNOTICED names ${key}, which the gate now notices: remove it`);
}
console.log(`CI gate agrees with the job conditions in ${SCENARIOS.length} plans`);
