#!/usr/bin/env bash
# Contract test for the default-off gate on the self-hosted GCP BuildBuddy
# graph (repository variable MEERKAT_GCP_BUILDBUDDY_ENABLED).
#
# It plans the workflows the way GitHub Actions schedules jobs (job `if:`
# expressions, `needs`, the implicit success() check, status functions) for
# a scheduled nightly, a graph-only nightly dispatch, a release tag push and
# the manual release dispatches, with the variable unset and set to 'true',
# and runs the release validation gate's own shell step against each planned
# route. Nothing here calls GCP, BuildBuddy or GitHub.
#
#   variable unset: no job that starts, syncs or scales anything in GCP is
#     planned; nightly skips gcp-buildbuddy and still runs its chains; a tag
#     whose exact-tree CI attestation is schema 4 validates on that
#     attestation alone ("Validate release state with Cargo" is skipped), a
#     tag with a legacy attestation validates with Cargo, and the release
#     validation gate accepts exactly the planned route; buildbuddy.yml, if
#     called anyway, fails its gate instead of passing.
#   variable 'true': the previous route is unchanged (the full graph on tags,
#     every GCP job in buildbuddy.yml, Cargo skipped on tags).
#   always: the hosted BuildBuddy binary job does not depend on the gate.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/meerkat-gcp-bb-gate.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

cat >"${TEST_ROOT}/plan.py" <<'PY'
import os
import re
import subprocess
import sys

ROOT = sys.argv[1]
WORKFLOWS = os.path.join(ROOT, ".github", "workflows")
GATE_VAR = "MEERKAT_GCP_BUILDBUDDY_ENABLED"
GCP_JOBS = ("control-plane-up", "executors-up", "executors-down", "control-plane-down")
FAILURES = []


def fail(message):
    FAILURES.append(message)
    print(f"FAIL: {message}")


def ok(message):
    print(f"ok: {message}")


# --- A deliberately small YAML reader for this repository's workflow style.
# It reads only `jobs.<id>` blocks and their 4-space keys, and stops loudly on
# shapes it does not understand instead of guessing.

def read_jobs(name):
    path = os.path.join(WORKFLOWS, name)
    with open(path, encoding="utf-8") as handle:
        lines = handle.read().splitlines()
    if "jobs:" not in lines:
        sys.exit(f"{name}: no top-level jobs: key")
    jobs = {}
    order = []
    current = None
    for line in lines[lines.index("jobs:") + 1:]:
        if line and not line.startswith((" ", "#")):
            break
        match = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if match:
            current = match.group(1)
            order.append(current)
            jobs[current] = []
            continue
        if current is not None:
            jobs[current].append(line)
    return order, jobs


def indent(line):
    return len(line) - len(line.lstrip(" "))


def job_key(lines, key):
    for idx, line in enumerate(lines):
        match = re.match(r"^    " + re.escape(key) + r":(.*)$", line)
        if not match:
            continue
        rest = match.group(1).strip()
        if rest in (">-", ">", "|", "|-"):
            block = []
            for nxt in lines[idx + 1:]:
                if nxt.strip() and indent(nxt) <= 4:
                    break
                if nxt.strip():
                    block.append(nxt.strip())
            return " ".join(block)
        if rest == "":
            block = []
            for nxt in lines[idx + 1:]:
                if not nxt.strip():
                    continue
                if indent(nxt) <= 4:
                    break
                block.append(nxt.strip())
            if block and block[0].startswith("["):
                flow = " ".join(block)
                if not flow.endswith("]"):
                    sys.exit(f"unsupported flow sequence under {key}: {flow!r}")
                return [item.strip() for item in flow.strip("[]").split(",") if item.strip()]
            items = []
            for stripped in block:
                if not stripped.startswith("- "):
                    sys.exit(f"unsupported block under {key}: {stripped!r}")
                items.append(stripped[2:].strip())
            return items
        return rest
    return None


def job_needs(lines):
    value = job_key(lines, "needs")
    if value is None:
        return []
    if isinstance(value, list):
        return value
    value = value.strip()
    if value.startswith("["):
        return [item.strip() for item in value.strip("[]").split(",") if item.strip()]
    return [value]


def strip_expression(value):
    value = value.strip()
    if value.startswith("${{") and value.endswith("}}"):
        return value[3:-2].strip()
    return value


# --- GitHub Actions expression subset: literals, context paths, ==, !=, !,
# &&, ||, parentheses and the functions these workflows use in job `if:`.

TOKEN = re.compile(
    r"\s*(?:('(?:[^']|'')*')|(==|!=|&&|\|\||[!(),])|([A-Za-z_][A-Za-z0-9_.\-]*)|(\d+))"
)


def tokenize(text):
    tokens = []
    pos = 0
    text = text.strip()
    while pos < len(text):
        match = TOKEN.match(text, pos)
        if not match or match.end() == pos:
            sys.exit(f"cannot tokenize expression at {text[pos:]!r} in {text!r}")
        pos = match.end()
        string, op, ident, number = match.groups()
        if string is not None:
            tokens.append(("str", string[1:-1].replace("''", "'")))
        elif op is not None:
            tokens.append(("op", op))
        elif ident is not None:
            tokens.append(("id", ident))
        else:
            tokens.append(("num", int(number)))
    return tokens


class Parser:
    def __init__(self, text):
        self.tokens = tokenize(text)
        self.pos = 0
        self.text = text

    def peek(self):
        return self.tokens[self.pos] if self.pos < len(self.tokens) else (None, None)

    def take(self, kind=None, value=None):
        token = self.peek()
        if token[0] is None or (kind and token[0] != kind) or (value and token[1] != value):
            sys.exit(f"unexpected {token!r} in {self.text!r}")
        self.pos += 1
        return token

    def parse(self):
        node = self.parse_or()
        if self.pos != len(self.tokens):
            sys.exit(f"trailing tokens in {self.text!r}")
        return node

    def parse_or(self):
        node = self.parse_and()
        while self.peek() == ("op", "||"):
            self.take()
            node = ("or", node, self.parse_and())
        return node

    def parse_and(self):
        node = self.parse_unary()
        while self.peek() == ("op", "&&"):
            self.take()
            node = ("and", node, self.parse_unary())
        return node

    def parse_unary(self):
        if self.peek() == ("op", "!"):
            self.take()
            return ("not", self.parse_unary())
        return self.parse_compare()

    def parse_compare(self):
        node = self.parse_primary()
        if self.peek() in (("op", "=="), ("op", "!=")):
            op = self.take()[1]
            node = (op, node, self.parse_primary())
        return node

    def parse_primary(self):
        kind, value = self.peek()
        if (kind, value) == ("op", "("):
            self.take()
            node = self.parse_or()
            self.take("op", ")")
            return node
        if kind == "str" or kind == "num":
            self.take()
            return ("lit", value)
        if kind == "id":
            self.take()
            if self.peek() == ("op", "("):
                self.take()
                args = []
                while self.peek() != ("op", ")"):
                    args.append(self.parse_or())
                    if self.peek() == ("op", ","):
                        self.take()
                self.take("op", ")")
                return ("call", value, args)
            if value in ("true", "false"):
                return ("lit", value == "true")
            if value == "null":
                return ("lit", None)
            return ("ctx", value)
        sys.exit(f"unexpected {value!r} in {self.text!r}")


def truthy(value):
    return value not in (None, "", False, 0)


def loose_equal(left, right):
    if left is None or right is None:
        return left is right
    if isinstance(left, bool) or isinstance(right, bool):
        return str(left).lower() == str(right).lower()
    return str(left).lower() == str(right).lower()


STATUS_FUNCTIONS = re.compile(r"\b(always|success|failure|cancelled)\s*\(")


class Scenario:
    def __init__(self, event, gate, actor="lukacf", inputs=None, release_backend=None, outputs=None):
        self.event = event
        self.vars = {GATE_VAR: "true"} if gate else {}
        self.actor = actor
        self.inputs = dict(inputs or {})
        # job -> {output: value} for the jobs whose outputs a scenario models;
        # any other job that ran reports every output as "true" (change flags).
        self.outputs = {job: dict(values) for job, values in (outputs or {}).items()}
        self.dispatch_inputs = {}
        if event == "workflow_dispatch":
            self.dispatch_inputs = {k: ("true" if v is True else "false" if v is False else str(v)) for k, v in self.inputs.items()}
            if release_backend is not None:
                self.dispatch_inputs["release_backend"] = release_backend
        self.ref = "refs/tags/v0.0.0" if event == "push" else "refs/heads/main"

    def describe(self):
        gate = "set" if self.vars else "unset"
        extra = f" inputs={self.dispatch_inputs}" if self.dispatch_inputs else ""
        return f"event={self.event} {GATE_VAR}={gate} actor={self.actor}{extra}"


def evaluate(node, scenario, needs, results):
    kind = node[0]
    if kind == "lit":
        return node[1]
    if kind == "and":
        left = evaluate(node[1], scenario, needs, results)
        return evaluate(node[2], scenario, needs, results) if truthy(left) else left
    if kind == "or":
        left = evaluate(node[1], scenario, needs, results)
        return left if truthy(left) else evaluate(node[2], scenario, needs, results)
    if kind == "not":
        return not truthy(evaluate(node[1], scenario, needs, results))
    if kind in ("==", "!="):
        equal = loose_equal(
            evaluate(node[1], scenario, needs, results),
            evaluate(node[2], scenario, needs, results),
        )
        return equal if kind == "==" else not equal
    if kind == "call":
        name, args = node[1], node[2]
        if name == "always":
            return True
        if name == "cancelled":
            return False
        if name == "success":
            return all(results.get(need) == "success" for need in needs)
        if name == "failure":
            return any(results.get(need) == "failure" for need in needs)
        values = [evaluate(arg, scenario, needs, results) for arg in args]
        if name == "startsWith":
            return str(values[0] or "").lower().startswith(str(values[1] or "").lower())
        if name == "contains":
            return str(values[1] or "").lower() in str(values[0] or "").lower()
        sys.exit(f"unsupported function {name}()")
    if kind == "ctx":
        parts = node[1].split(".")
        if parts[0] == "vars" and len(parts) == 2:
            return scenario.vars.get(parts[1], "")
        if parts[0] == "github":
            if parts[1:] == ["event_name"]:
                return scenario.event
            if parts[1:] == ["actor"]:
                return scenario.actor
            if parts[1:] == ["ref"]:
                return scenario.ref
            if parts[1:3] == ["event", "inputs"] and len(parts) == 4:
                if scenario.event != "workflow_dispatch":
                    return None
                return scenario.dispatch_inputs.get(parts[3], "")
        if parts[0] == "inputs" and len(parts) == 2:
            return scenario.inputs.get(parts[1])
        if parts[0] == "needs" and len(parts) >= 3:
            job = parts[1]
            if parts[2] == "result":
                return results.get(job, "skipped")
            if parts[2] == "outputs" and len(parts) == 4:
                if results.get(job) != "success":
                    return ""
                if job in scenario.outputs:
                    return scenario.outputs[job].get(parts[3], "")
                # A job that ran reports every change flag as changed.
                return "true"
        sys.exit(f"unsupported context {node[1]!r}")
    sys.exit(f"unsupported node {node!r}")


def plan(name, scenario):
    order, jobs = read_jobs(name)
    results = {}
    remaining = list(order)
    while remaining:
        progressed = False
        for job in list(remaining):
            needs = job_needs(jobs[job])
            if any(need not in results for need in needs):
                continue
            remaining.remove(job)
            progressed = True
            expression = job_key(jobs[job], "if")
            expression = strip_expression(expression) if expression else ""
            if not expression or not STATUS_FUNCTIONS.search(expression):
                if any(results[need] != "success" for need in needs):
                    results[job] = "skipped"
                    continue
            runs = True if not expression else truthy(
                evaluate(Parser(expression).parse(), scenario, needs, results)
            )
            results[job] = "success" if runs else "skipped"
        if not progressed:
            sys.exit(f"{name}: unresolvable needs among {remaining}")
    return order, jobs, results


def substitute(script, scenario, results):
    def replace(match):
        value = evaluate(Parser(match.group(1)).parse(), scenario, [], results)
        if isinstance(value, bool):
            return "true" if value else "false"
        return "" if value is None else str(value)

    return re.sub(r"\$\{\{(.*?)\}\}", replace, script)


def step_run_script(lines, step_name):
    for idx, line in enumerate(lines):
        if line.strip() == f"- name: {step_name}":
            step_indent = indent(line)
            for jdx in range(idx + 1, len(lines)):
                candidate = lines[jdx]
                if candidate.strip() and indent(candidate) <= step_indent:
                    break
                if re.match(r"^\s*run: \|\s*$", candidate):
                    key_indent = indent(candidate)
                    body = []
                    for nxt in lines[jdx + 1:]:
                        if nxt.strip() and indent(nxt) <= key_indent:
                            break
                        body.append(nxt)
                    width = min(indent(b) for b in body if b.strip())
                    return "\n".join(b[width:] for b in body) + "\n"
    sys.exit(f"step {step_name!r} has no run: | block")


def step_if(lines, step_name):
    for idx, line in enumerate(lines):
        if line.strip() == f"- name: {step_name}":
            step_indent = indent(line)
            for candidate in lines[idx + 1:]:
                if candidate.strip() and indent(candidate) <= step_indent:
                    break
                match = re.match(r"^\s*if: (.*)$", candidate)
                if match:
                    return strip_expression(match.group(1))
            return None
    return None


RELEASE_GATE_STEP = "Check selected release validation backend"


def release_gate_accepts(scenario, results):
    _, jobs = read_jobs("release.yml")
    script = substitute(step_run_script(jobs["release_validate_gate"], RELEASE_GATE_STEP), scenario, results)
    proc = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    return proc.returncode == 0, proc.stdout + proc.stderr


def ran(results, job):
    return results.get(job) == "success"


def job_text(jobs, job):
    return "\n".join(jobs[job])


def touches_gcp(text):
    # Submitters authenticate and open their IAP tunnel inside the batch action.
    return any(marker in text for marker in (
        "google-github-actions/auth",
        "scripts/gcp-buildbuddy-",
        "uses: ./.github/actions/run-buildbuddy-ci-lane-batch",
    ))


# --- buildbuddy.yml -------------------------------------------------------

DISABLED_STEP = "Require the self-hosted graph to be enabled"


def buildbuddy_plan(scenario):
    """Plan the called workflow: the caller's event and variables, no inputs."""
    inner = Scenario(scenario.event, bool(scenario.vars), actor=scenario.actor)
    order, jobs, results = plan("buildbuddy.yml", inner)
    gcp = [job for job in order if ran(results, job) and touches_gcp(job_text(jobs, job))]
    fails_closed = False
    if ran(results, "gate"):
        expression = step_if(jobs["gate"], DISABLED_STEP)
        if expression is not None and truthy(evaluate(Parser(expression).parse(), inner, [], results)):
            script = step_run_script(jobs["gate"], DISABLED_STEP)
            fails_closed = subprocess.run(["bash", "-c", script], capture_output=True).returncode != 0
    return order, jobs, results, gcp, fails_closed


for gate in (False, True):
    scenario = Scenario("schedule", gate)
    order, jobs, results, gcp_running, fails_closed = buildbuddy_plan(scenario)
    running = [job for job in order if ran(results, job)]
    if not gate:
        if gcp_running:
            fail(f"buildbuddy.yml with {GATE_VAR} unset plans GCP jobs: {gcp_running}")
        else:
            ok(f"buildbuddy.yml with {GATE_VAR} unset plans no GCP job (running: {running})")
        for job in GCP_JOBS:
            if job in jobs and ran(results, job):
                fail(f"buildbuddy.yml with {GATE_VAR} unset plans {job}")
        if not fails_closed:
            fail("buildbuddy.yml called with the graph disabled must fail its gate (a step that runs and exits non-zero)")
        else:
            ok("buildbuddy.yml called with the graph disabled fails its gate instead of passing")
    else:
        for job in ("control-plane-up", "executors-up", "prebuild-submit", "native-submit",
                    "native-cargo-submit", "executors-down", "control-plane-down", "gate"):
            if not ran(results, job):
                fail(f"buildbuddy.yml with {GATE_VAR}=true no longer plans {job}")
        if fails_closed:
            fail("buildbuddy.yml disabled-graph step fails while the graph is enabled")
        ok(f"buildbuddy.yml with {GATE_VAR}=true keeps the GCP route: {running}")

_, bb_jobs = read_jobs("buildbuddy.yml")
for job in bb_jobs:
    if touches_gcp(job_text(bb_jobs, job)):
        expression = job_key(bb_jobs[job], "if") or ""
        if f"vars.{GATE_VAR} == 'true'" not in expression or "||" in expression:
            fail(f"buildbuddy.yml job {job} touches GCP without a conjunctive {GATE_VAR} gate: if={expression!r}")

# --- nightly.yml ----------------------------------------------------------
# Scheduled and ordinary dispatches skip the graph while it is disabled and
# keep their other jobs; a graph_only dispatch always calls buildbuddy.yml,
# which then starts nothing in GCP and fails, so a disabled graph never
# finishes as an empty green pre-tag check.

for gate in (False, True):
    for event, inputs in (
        ("schedule", {}),
        ("workflow_dispatch", {"graph_only": False}),
        ("workflow_dispatch", {"graph_only": True}),
    ):
        scenario = Scenario(event, gate, inputs=inputs)
        graph_only = bool(inputs.get("graph_only"))
        before = len(FAILURES)
        order, jobs, results = plan("nightly.yml", scenario)
        running = [job for job in order if ran(results, job)]
        callers = [job for job in order if "uses: ./.github/workflows/buildbuddy.yml" in job_text(jobs, job)]
        if not callers:
            fail("nightly.yml no longer calls buildbuddy.yml")
        expect_call = gate or graph_only
        for caller in callers:
            if ran(results, caller) != expect_call:
                fail(f"nightly {scenario.describe()}: {caller} planned={results[caller]}, expected {'run' if expect_call else 'skipped'}")
        chains = [job for job in running if job not in callers]
        if not graph_only and not chains:
            fail(f"nightly {scenario.describe()}: no other nightly job runs ({results})")
        if graph_only and chains:
            fail(f"nightly graph-only dispatch runs non-graph jobs: {chains}")
        outcome = "skipped"
        if any(ran(results, caller) for caller in callers):
            _, _, _, gcp, fails_closed = buildbuddy_plan(scenario)
            if gate and (not gcp or fails_closed):
                fail(f"nightly {scenario.describe()}: the enabled graph must start its GCP jobs and not fail closed")
            if not gate and (gcp or not fails_closed):
                fail(f"nightly {scenario.describe()}: the disabled graph must start nothing in GCP and fail (gcp={gcp}, fails_closed={fails_closed})")
            outcome = "run" if gate else "called, starts nothing in GCP, fails (red)"
        if len(FAILURES) == before:
            ok(f"nightly {scenario.describe()}: graph {outcome}; other jobs running={len(chains)}")

# --- release.yml ----------------------------------------------------------

_, release_jobs = read_jobs("release.yml")
VALIDATION = ("release_validate_cargo", "release_validate_buildbuddy", "release_validate_buildbuddy_full")

def release_route(scenario):
    order, jobs, results = plan("release.yml", scenario)
    return {job: results[job] for job in VALIDATION + ("release_validate_gate",)}, results


def attested(schema):
    """require_ci_green's outputs on a tag run whose attestation has `schema`."""
    return {"require_ci_green": {"ci_attestation_schema": schema}}


for gate in (False, True):
    for schema in ("4", "1"):
        scenario = Scenario("push", gate, outputs=attested(schema))
        label = f"{scenario.describe()} attestation schema {schema}"
        before = len(FAILURES)
        route, results = release_route(scenario)
        cargo_validates = not gate and schema != "4"
        expected = {
            "release_validate_cargo": "success" if cargo_validates else "skipped",
            "release_validate_buildbuddy": "skipped",
            "release_validate_buildbuddy_full": "success" if gate else "skipped",
            "release_validate_gate": "success",
        }
        if route != expected:
            fail(f"release tag push ({label}): route {route}, expected {expected}")
        accepted, output = release_gate_accepts(scenario, results)
        if not accepted:
            fail(f"release validation gate rejects the planned tag route ({label}):\n{output}")
        elif len(FAILURES) == before:
            validator = (
                "the full BuildBuddy graph" if gate
                else "Cargo" if cargo_validates
                else "the schema-4 exact-tree CI attestation (Cargo skipped)"
            )
            ok(f"release tag push ({label}) validates with {validator} and the gate accepts it")
        # The gate must not accept another route for the same state: each
        # validation job flipped between ran and skipped is refused.
        for job in ("release_validate_cargo", "release_validate_buildbuddy_full"):
            flipped = dict(results)
            flipped[job] = "skipped" if results[job] == "success" else "success"
            accepted, _ = release_gate_accepts(scenario, flipped)
            if accepted:
                fail(f"release validation gate accepts {job}={flipped[job]} for {label}")
        if gate or cargo_validates:
            failed = dict(results)
            failed["release_validate_buildbuddy_full" if gate else "release_validate_cargo"] = "failure"
            accepted, _ = release_gate_accepts(scenario, failed)
            if accepted:
                fail(f"release validation gate accepts a failed validation for {label}")
        else:
            # The attestation is the validation: the same skipped route is
            # refused when the verified attestation is not schema 4, and when
            # require_ci_green did not succeed.
            legacy = Scenario("push", gate, outputs=attested("1"))
            accepted, _ = release_gate_accepts(legacy, results)
            if accepted:
                fail(f"release validation gate accepts a skipped Cargo validation with a legacy attestation ({legacy.describe()})")
            red_ci = dict(results)
            red_ci["require_ci_green"] = "failure"
            accepted, _ = release_gate_accepts(scenario, red_ci)
            if accepted:
                fail(f"release validation gate accepts a tag whose exact-main CI did not succeed ({label})")

for gate in (False, True):
    for actor, backend, selected in (
        ("someone", "github-hosted", "release_validate_cargo"),
        ("lukacf", "github-hosted", "release_validate_cargo"),
        ("lukacf", "buildbuddy", "release_validate_buildbuddy"),
    ):
        scenario = Scenario("workflow_dispatch", gate, actor=actor, inputs={
            "publish_web_sdk_only": False,
            "publish_release_assets_only": False,
        }, release_backend=backend, outputs={"require_ci_green": {}})
        route, results = release_route(scenario)
        ran_jobs = [job for job in VALIDATION if route[job] == "success"]
        if ran_jobs != [selected]:
            fail(f"release dispatch {scenario.describe()}: validation jobs {ran_jobs}, expected [{selected}]")
            continue
        accepted, output = release_gate_accepts(scenario, results)
        if not accepted:
            fail(f"release validation gate rejects dispatch route {scenario.describe()}:\n{output}")
        else:
            ok(f"release dispatch ({scenario.describe()}) validates with {selected}")

for job, lines in release_jobs.items():
    text = "\n".join(lines)
    if "build_binaries_buildbuddy" == job and GATE_VAR in text:
        fail("the hosted BuildBuddy binary job must not depend on the self-hosted graph gate")
if "build_binaries_buildbuddy" not in release_jobs:
    fail("release.yml has no build_binaries_buildbuddy job")
else:
    ok("the hosted BuildBuddy binary job is independent of the self-hosted graph gate")

if FAILURES:
    print(f"{len(FAILURES)} failure(s)")
    sys.exit(1)
print("PASS: self-hosted GCP BuildBuddy workflow gate")
PY

python3 "${TEST_ROOT}/plan.py" "${REPO_ROOT}"
