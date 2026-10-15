#!/usr/bin/env python3
"""Semantic release-workflow contract checks for the release doctor.

The doctor used to assert release-workflow behaviour by grepping literal lines
out of `.github/workflows/release.yml`. Two of those greps went stale the
moment the workflow was reflowed (a folded `if: >-` condition and a
`--slo-seconds ${{ ... }}` expression), so `make release-doctor` failed on a
main branch whose behaviour had not changed (#1091).

This module asserts what the workflow DOES instead of how it is spelled. It
extracts a job, splits it into steps, and evaluates each step's `if:` and the
`${{ }}` expressions in its `run:` body under concrete event contexts (a tag
push, a package-recovery dispatch, an explicit historical-evidence dispatch).
Reflowing a condition across lines, collapsing it onto one line, or rewriting
an expression into an equivalent one all pass; gating a step off tag pushes,
re-enabling the long measurement on tags, relaxing the 30 minute publication
SLO, making the SDK packages wait on it, re-running Cargo validation on a
tag whose exact-tree CI attestation already covers it, or letting a pre-tag
prebuild publish anything or a tag publish a prebuild's artifacts without the
attested-archive verification all fail and name the defect.

Only the Python standard library is used, so the doctor and its contract test
run wherever `python3` does.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_WORKFLOW = Path(".github/workflows/release.yml")
READINESS_WORKFLOW = Path(".github/workflows/release-semver-readiness.yml")
TAG_SLO_SECONDS = 1800

SEMVER_GATE_JOB = "release_semver_gate"
CI_GREEN_JOB = "require_ci_green"
CARGO_VALIDATION_JOB = "release_validate_cargo"
VALIDATION_GATE_JOB = "release_validate_gate"
# require_ci_green's output naming the verified exact-tree attestation schema.
ATTESTATION_SCHEMA_OUTPUT = "ci_attestation_schema"
GCP_GRAPH_VARIABLE = "MEERKAT_GCP_BUILDBUDDY_ENABLED"
RELEASE_VERSION_STEP_ID = "release_version"
REGISTRY_JOB = "publish_registries"
# The evidence step is the one that resolves the exact-tree readiness artifact.
EVIDENCE_ARTIFACT_PREFIX = "meerkat-semver-attestation-main-"
# The long measurement the tag path must never rerun.
MEASUREMENT_COMMAND = "make semver-breaks"
# The release's own measurement must refuse a post-release tree: measured
# against its own tag it would pass and publish main's tip as that version.
RELEASE_TREE_ENV = "MEERKAT_SEMVER_REQUIRE_RELEASE_TREE"
READINESS_JOB = "semver"
NOTES_STEP_ID = "notes"
NOTES_OUTPUT = "baseline"
PUBLIC_VERIFIER = "scripts/verify-rust-release-public.py"


class ContractError(Exception):
    """A structural precondition the checker cannot see past."""


class UnsupportedExpression(ContractError):
    """The workflow uses expression syntax this evaluator does not model."""


# --------------------------------------------------------------------------
# Workflow extraction (line-oriented, no YAML dependency)
# --------------------------------------------------------------------------

JOB_HEADER = re.compile(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$")
STEP_START = re.compile(r"^      - ")
KEY_LINE = re.compile(r"^(\s*)([A-Za-z0-9_-]+):(.*)$")


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def job_block(text: str, job_name: str) -> list[str]:
    lines = text.splitlines()
    start = None
    for index, line in enumerate(lines):
        match = JOB_HEADER.match(line)
        if match and match.group(1) == job_name:
            start = index
            break
    if start is None:
        raise ContractError(f"job `{job_name}` is not defined in the workflow")
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if JOB_HEADER.match(lines[index]):
            end = index
            break
    return lines[start + 1 : end]


def parse_mapping(lines: list[str], indent: int) -> dict[str, str]:
    """Collect `key: value` pairs at exactly `indent`, folding nested scalars.

    A value on the key line is kept verbatim (plus any more-indented plain
    continuation lines). A block scalar (`|`, `>`, with optional chomping
    indicator) or a nested mapping is folded into one string: literal blocks
    keep newlines, everything else is joined with single spaces. Comment and
    blank lines between keys are skipped.
    """
    mapping: dict[str, str] = {}
    index = 0
    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            index += 1
            continue
        match = KEY_LINE.match(line)
        if not match or len(match.group(1)) != indent:
            index += 1
            continue
        key = match.group(2)
        remainder = match.group(3).strip()
        index += 1
        continuation: list[str] = []
        while index < len(lines):
            candidate = lines[index]
            if candidate.strip() and _indent(candidate) <= indent:
                break
            continuation.append(candidate)
            index += 1
        if re.fullmatch(r"[|>][+-]?", remainder):
            joiner = "\n" if remainder.startswith("|") else " "
            body = [entry.strip() for entry in continuation if entry.strip()]
            mapping[key] = joiner.join(body)
        else:
            parts = [remainder] if remainder else []
            parts.extend(entry.strip() for entry in continuation if entry.strip())
            mapping[key] = " ".join(parts)
    return mapping


@dataclass
class Step:
    fields: dict[str, str]
    lines: list[str] = field(default_factory=list)

    @property
    def name(self) -> str:
        return self.fields.get("name", "<unnamed step>")

    @property
    def condition(self) -> str | None:
        return self.fields.get("if")

    @property
    def run(self) -> str:
        return self.fields.get("run", "")

    @property
    def env(self) -> dict[str, str]:
        """The step's `env:` mapping, values unquoted."""
        return self.nested("env")

    def nested(self, name: str) -> dict[str, str]:
        """The step's `<name>:` mapping (`env`, `with`), values unquoted."""
        for index, line in enumerate(self.lines):
            match = KEY_LINE.match(line)
            if match and len(match.group(1)) == 8 and match.group(2) == name:
                nested: list[str] = []
                for candidate in self.lines[index + 1 :]:
                    if candidate.strip() and _indent(candidate) <= 8:
                        break
                    nested.append(candidate)
                return {
                    key: value.strip().strip("'\"")
                    for key, value in parse_mapping(nested, 10).items()
                }
        return {}

    def written_outputs(self) -> set[str]:
        """Output names the step's `run:` writes into `$GITHUB_OUTPUT`."""
        names: set[str] = set()
        for line in self.run.splitlines():
            if "GITHUB_OUTPUT" not in line:
                continue
            match = re.search(r"""echo\s+["']?([A-Za-z_][A-Za-z0-9_-]*)=""", line)
            if match:
                names.add(match.group(1))
        return names


def job_steps(block: list[str]) -> list[Step]:
    steps: list[Step] = []
    in_steps = False
    current: list[str] | None = None
    for line in block:
        if re.match(r"^    steps:\s*$", line):
            in_steps = True
            continue
        if not in_steps:
            continue
        if line.strip() and _indent(line) < 6:
            break
        if STEP_START.match(line):
            if current is not None:
                steps.append(Step(parse_mapping(current, 8), current))
            current = ["        " + line[8:]]
            continue
        if current is not None:
            current.append(line)
    if current is not None:
        steps.append(Step(parse_mapping(current, 8), current))
    if not steps:
        raise ContractError("job defines no steps")
    return steps


# --------------------------------------------------------------------------
# GitHub Actions expression evaluation (the subset release.yml uses)
# --------------------------------------------------------------------------

TOKEN = re.compile(
    r"\s*(?:"
    r"(?P<string>'(?:[^']|'')*')"
    r"|(?P<op>&&|\|\||==|!=|!|\(|\)|,)"
    r"|(?P<number>\d+(?:\.\d+)?)"
    r"|(?P<ident>[A-Za-z_][A-Za-z0-9_.-]*)"
    r")"
)


def github_ref_name(ref: str) -> str:
    """`github.ref_name` / `GITHUB_REF_NAME` as GitHub sets it: the ref with
    its `refs/heads/` or `refs/tags/` prefix removed (for other refs, its
    `refs/<kind>/` prefix), so `refs/tags/alpha/v1.2.3` is `alpha/v1.2.3`,
    not its last path segment."""
    for prefix in ("refs/heads/", "refs/tags/"):
        if ref.startswith(prefix):
            return ref[len(prefix) :]
    parts = ref.split("/", 2)
    if len(parts) == 3 and parts[0] == "refs":
        return parts[2]
    return ref


@dataclass(frozen=True)
class EventContext:
    """The `github` and `needs` contexts of one hypothetical workflow run."""

    label: str
    event_name: str
    ref: str = "refs/tags/v0.0.0"
    inputs: dict[str, str] = field(default_factory=dict)
    needs_result: str = "success"
    # `<step id>.<output>` -> value, for the outputs this context models. Any
    # other `steps.*.outputs.*` reference raises UnsupportedExpression.
    step_outputs: dict[str, str] = field(default_factory=dict)
    actor: str = "release-bot"
    sha: str = "0" * 40
    run_id: str = "1"
    # Repository variables; an unset variable reads as empty, as on GitHub.
    variables: dict[str, str] = field(default_factory=dict)
    # job -> result, for upstream jobs whose result differs from needs_result.
    job_results: dict[str, str] = field(default_factory=dict)
    # `<job>.<output>` -> value, for the upstream outputs this context models.
    # Any other `needs.*.outputs.*` reference raises UnsupportedExpression.
    job_outputs: dict[str, str] = field(default_factory=dict)

    def resolve(self, path: str) -> str:
        if path == "github.event_name":
            return self.event_name
        if path == "github.ref":
            return self.ref
        if path == "github.ref_name":
            return github_ref_name(self.ref)
        if path == "github.actor":
            return self.actor
        if path == "github.sha":
            return self.sha
        if path == "github.run_id":
            return self.run_id
        if path == "github.token":
            # Never a real credential: scripts run here only against stubs.
            return "contract-check-token"
        if path.startswith("github.event.inputs."):
            # Unset dispatch inputs and push events both read as empty.
            return self.inputs.get(path[len("github.event.inputs.") :], "")
        variable = re.fullmatch(r"vars\.([A-Za-z0-9_]+)", path)
        if variable:
            return self.variables.get(variable.group(1), "")
        needs = re.fullmatch(r"needs\.([A-Za-z0-9_-]+)\.result", path)
        if needs:
            return self.job_results.get(needs.group(1), self.needs_result)
        output = re.fullmatch(r"needs\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)", path)
        if output and f"{output.group(1)}.{output.group(2)}" in self.job_outputs:
            return self.job_outputs[f"{output.group(1)}.{output.group(2)}"]
        # Earlier steps of the same job are evaluated on the happy path, the
        # same assumption `needs_result` makes for upstream jobs.
        if re.fullmatch(r"steps\.[A-Za-z0-9_-]+\.(?:outcome|conclusion)", path):
            return self.needs_result
        # Only the step outputs a context models resolve; anything else stays
        # unsupported, so a gate added on an unmodelled output fails the check
        # closed instead of reading as an empty string.
        output = re.fullmatch(r"steps\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)", path)
        if output and f"{output.group(1)}.{output.group(2)}" in self.step_outputs:
            return self.step_outputs[f"{output.group(1)}.{output.group(2)}"]
        raise UnsupportedExpression(
            f"context `{path}` is not modelled by the release doctor"
        )


def truthy(value: object) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    return value != ""


def _equal(left: object, right: object) -> bool:
    # GitHub compares strings case-insensitively and coerces null to ''.
    def norm(value: object) -> str:
        if value is None:
            return ""
        if isinstance(value, bool):
            return "true" if value else "false"
        return str(value).lower()

    return norm(left) == norm(right)


FUNCTIONS: dict[str, Callable[[list[object]], object]] = {
    "always": lambda args: True,
    "success": lambda args: True,
    "failure": lambda args: False,
    "cancelled": lambda args: False,
    "startsWith": lambda args: str(args[0]).lower().startswith(str(args[1]).lower()),
    "endsWith": lambda args: str(args[0]).lower().endswith(str(args[1]).lower()),
    "contains": lambda args: str(args[1]).lower() in str(args[0]).lower(),
}


class _Parser:
    def __init__(self, expression: str, context: EventContext) -> None:
        self.context = context
        self.tokens: list[tuple[str, str]] = []
        position = 0
        expression = expression.strip()
        while position < len(expression):
            match = TOKEN.match(expression, position)
            if not match or match.end() == position:
                raise UnsupportedExpression(
                    f"cannot tokenise expression near `{expression[position : position + 20]}`"
                )
            position = match.end()
            kind = match.lastgroup
            if kind is None:
                continue
            self.tokens.append((kind, match.group(kind)))
        self.index = 0

    def peek(self) -> tuple[str, str] | None:
        return self.tokens[self.index] if self.index < len(self.tokens) else None

    def take(self) -> tuple[str, str]:
        token = self.peek()
        if token is None:
            raise UnsupportedExpression("unexpected end of expression")
        self.index += 1
        return token

    def expect_op(self, op: str) -> None:
        token = self.take()
        if token != ("op", op):
            raise UnsupportedExpression(f"expected `{op}`, found `{token[1]}`")

    def parse(self) -> object:
        value = self.parse_or()
        if self.peek() is not None:
            raise UnsupportedExpression(f"trailing token `{self.peek()[1]}`")
        return value

    def parse_or(self) -> object:
        left = self.parse_and()
        while self.peek() == ("op", "||"):
            self.take()
            right = self.parse_and()
            left = left if truthy(left) else right
        return left

    def parse_and(self) -> object:
        left = self.parse_equality()
        while self.peek() == ("op", "&&"):
            self.take()
            right = self.parse_equality()
            left = right if truthy(left) else left
        return left

    def parse_equality(self) -> object:
        left = self.parse_unary()
        while self.peek() in (("op", "=="), ("op", "!=")):
            _, op = self.take()
            right = self.parse_unary()
            equal = _equal(left, right)
            left = equal if op == "==" else not equal
        return left

    def parse_unary(self) -> object:
        if self.peek() == ("op", "!"):
            self.take()
            return not truthy(self.parse_unary())
        return self.parse_primary()

    def parse_primary(self) -> object:
        kind, text = self.take()
        if kind == "op" and text == "(":
            value = self.parse_or()
            self.expect_op(")")
            return value
        if kind == "string":
            return text[1:-1].replace("''", "'")
        if kind == "number":
            return text
        if kind == "ident":
            lowered = text.lower()
            if lowered in ("true", "false"):
                return lowered == "true"
            if lowered == "null":
                return None
            if self.peek() == ("op", "("):
                self.take()
                args: list[object] = []
                if self.peek() != ("op", ")"):
                    args.append(self.parse_or())
                    while self.peek() == ("op", ","):
                        self.take()
                        args.append(self.parse_or())
                self.expect_op(")")
                function = FUNCTIONS.get(text)
                if function is None:
                    raise UnsupportedExpression(f"function `{text}()` is not modelled")
                return function(args)
            return self.context.resolve(text)
        raise UnsupportedExpression(f"unexpected token `{text}`")


EXPRESSION = re.compile(r"\$\{\{(.*?)\}\}", re.DOTALL)


def evaluate(expression: str, context: EventContext) -> object:
    """Evaluate one expression, with or without the `${{ }}` wrapper."""
    expression = " ".join(expression.split())
    match = re.fullmatch(r"\$\{\{(.*)\}\}", expression)
    if match:
        expression = match.group(1)
    return _Parser(expression, context).parse()


def step_runs(step: Step, context: EventContext) -> bool:
    condition = step.condition
    if condition is None:
        return True
    return truthy(evaluate(condition, context))


def render(template: str, context: EventContext) -> str:
    """Substitute every evaluable `${{ }}` in a run body; leave the rest."""

    def substitute(match: re.Match[str]) -> str:
        try:
            value = evaluate(match.group(1), context)
        except UnsupportedExpression:
            return match.group(0)
        if value is None:
            return ""
        if isinstance(value, bool):
            return "true" if value else "false"
        return str(value)

    return EXPRESSION.sub(substitute, template)


def render_strict(template: str, context: EventContext) -> str:
    """Substitute every `${{ }}`; raise if any cannot be evaluated.

    For checks that must fail closed: an expression the checker does not
    model is a contract error, never a literal left in place.
    """

    def substitute(match: re.Match[str]) -> str:
        value = evaluate(match.group(1), context)
        if value is None:
            return ""
        if isinstance(value, bool):
            return "true" if value else "false"
        return str(value)

    return EXPRESSION.sub(substitute, template)


# --------------------------------------------------------------------------
# Event contexts
# --------------------------------------------------------------------------

TAG_PUSH = EventContext(label="a tag push", event_name="push")
PACKAGE_RECOVERY = EventContext(
    label="a package-recovery dispatch",
    event_name="workflow_dispatch",
    inputs={"release_tag": "v0.0.0", "publish_release_packages": "true"},
)
HISTORICAL_EVIDENCE = EventContext(
    label="an explicit historical-evidence dispatch",
    event_name="workflow_dispatch",
    inputs={
        "release_tag": "v0.0.0",
        "publish_release_packages": "true",
        "semver_evidence_run_id": "1",
        "semver_evidence_job_id": "2",
    },
)


BRANCH_DISPATCHES = [
    EventContext(
        label=f"{'an' if mode[0] in 'aeiou' else 'a'} {mode} dispatch from {ref} without release_tag",
        event_name="workflow_dispatch",
        ref=ref,
        inputs=inputs,
    )
    # Branches named after the version, including exactly the allowed tag
    # names (github.ref_name is then an allowed tag, so a binding that reads
    # ref_name without checking refs/tags/ would take the branch for the tag).
    for ref in (
        "refs/heads/main",
        "refs/heads/release/v0.0.0",
        "refs/heads/hotfix/0.0.0",
        "refs/heads/v0.0.0",
        "refs/heads/alpha/v0.0.0",
    )
    for mode, inputs in (
        ("package", {"publish_release_packages": "true"}),
        ("alpha crate", {"publish_release_packages": "true", "alpha_crates_only": "true"}),
        ("Web-SDK-only", {"publish_web_sdk_only": "true"}),
        ("asset-only", {"publish_release_assets_only": "true"}),
    )
]


# --------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------


def _job_enabled(block: list[str], job_name: str, context: EventContext) -> list[str]:
    job_fields = parse_mapping(block, 4)
    condition = job_fields.get("if")
    if condition is not None and not truthy(evaluate(condition, context)):
        return [f"job `{job_name}` is skipped on {context.label}"]
    return []


def check_semver_evidence(text: str) -> list[str]:
    """Tag releases consume exact-tree pre-tag evidence, never re-measure."""
    block = job_block(text, SEMVER_GATE_JOB)
    violations = _job_enabled(block, SEMVER_GATE_JOB, TAG_PUSH)
    steps = job_steps(block)

    evidence_steps = [step for step in steps if EVIDENCE_ARTIFACT_PREFIX in step.run]
    if not evidence_steps:
        violations.append(
            f"job `{SEMVER_GATE_JOB}` has no step that resolves the exact-tree "
            f"`{EVIDENCE_ARTIFACT_PREFIX}<tree>` readiness artifact"
        )
    for step in evidence_steps:
        for context in (TAG_PUSH, PACKAGE_RECOVERY):
            if not step_runs(step, context):
                violations.append(
                    f"step `{step.name}` does not run on {context.label}, so the "
                    "release would not reuse exact-tree pre-tag semver evidence"
                )
        if step_runs(step, HISTORICAL_EVIDENCE):
            violations.append(
                f"step `{step.name}` also runs on {HISTORICAL_EVIDENCE.label}, "
                "which must verify the explicit measurement instead"
            )

    for step in steps:
        if MEASUREMENT_COMMAND not in step.run:
            continue
        if step.env.get(RELEASE_TREE_ENV) != "1":
            violations.append(
                f"step `{step.name}` runs `{MEASUREMENT_COMMAND}` without "
                f"`{RELEASE_TREE_ENV}: \"1\"`, so a post-release tree (notes above the "
                "stamped version) would pass and publish as the tagged version"
            )
        for context in (TAG_PUSH, PACKAGE_RECOVERY):
            if step_runs(step, context):
                violations.append(
                    f"step `{step.name}` reruns `{MEASUREMENT_COMMAND}` on "
                    f"{context.label}; the long measurement belongs before the tag"
                )
    return violations


# The version the binding scenarios run against.
BINDING_VERSION = "0.0.0"
CHECKOUT_ACTION = "actions/checkout"


def _run_binding_step(
    step: Step,
    context: EventContext,
    tags: tuple[str, ...],
    checkout_ref: str | None,
    later_refs: tuple[str, ...] = (),
) -> int:
    """Run the step's script as the runner would, in a scratch checkout.

    The repository has two commits: the release commit, which every tag in
    `tags` points at, and a later commit, which every branch points at, as do
    the `refs/tags/` and `refs/heads/` refs in `later_refs`. HEAD
    is what the job's checkout step selects for `context` (`checkout_ref`,
    rendered from its `with.ref`); `None` puts HEAD on the later commit, a
    checkout that did not land on the tag. A ref that does not resolve fails
    the checkout, which refuses the run as the runner would. Every `${{ }}`
    in the step's env and run must evaluate under `context`; one that does
    not raises UnsupportedExpression, so the check fails closed.
    """
    env = {key: render_strict(value, context) for key, value in step.env.items()}
    script = render_strict(step.run, context)
    with tempfile.TemporaryDirectory(prefix="release-binding-") as checkout:
        git_env = {
            "PATH": os.environ.get("PATH", ""),
            "HOME": checkout,
            "GIT_AUTHOR_NAME": "binding",
            "GIT_AUTHOR_EMAIL": "binding@example.invalid",
            "GIT_COMMITTER_NAME": "binding",
            "GIT_COMMITTER_EMAIL": "binding@example.invalid",
        }

        def git(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                ["git", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", *args],
                cwd=checkout,
                env=git_env,
                check=check,
                capture_output=True,
                text=True,
            )

        cargo = Path(checkout, "Cargo.toml")
        cargo.write_text(f'[workspace.package]\nversion = "{BINDING_VERSION}"\n', encoding="utf-8")
        git("init", "-q", "-b", "binding-base")
        git("add", "Cargo.toml")
        git("commit", "-q", "-m", "release")
        for tag in tags:
            git("tag", tag)
        cargo.write_text(
            f'[workspace.package]\nversion = "{BINDING_VERSION}"\n# later\n', encoding="utf-8"
        )
        git("commit", "-q", "-am", "later")
        for later_ref in (context.ref, *later_refs):
            if later_ref.startswith("refs/heads/"):
                git("branch", "-f", later_ref[len("refs/heads/") :], "HEAD")
            elif later_ref.startswith("refs/tags/") and later_ref != context.ref:
                git("tag", "-f", later_ref[len("refs/tags/") :], "HEAD")
        if checkout_ref is not None:
            if git("checkout", "-q", "--detach", checkout_ref, check=False).returncode != 0:
                return 1
        result = subprocess.run(
            ["bash", "-c", script],
            cwd=checkout,
            env={
                **git_env,
                **env,
                "GITHUB_REF": context.ref,
                "GITHUB_REF_NAME": github_ref_name(context.ref),
                "GITHUB_EVENT_NAME": context.event_name,
            },
            capture_output=True,
            text=True,
            check=False,
        )
        return result.returncode


def check_dispatch_binding(text: str) -> list[str]:
    """Every publishing run is bound to an allowed TAG of its version.

    `require_ci_green` gates everything that publishes. Its version step must
    run on a tag push, on a dispatch that names release_tag, and on every
    publishing dispatch that names none, and it must refuse unless the run is
    bound to an allowed tag: `v<version>` in every mode, `alpha/v<version>`
    only for the crates-only alpha canary. The step's script runs against
    scratch checkouts in which HEAD is whatever the job's checkout step
    selects, so what is checked is what the job does, not how it is spelled:
    a branch named after the version, an alpha tag outside the alpha lane,
    and a named tag that is not the checked-out commit must not pass.
    """
    block = job_block(text, CI_GREEN_JOB)
    steps = job_steps(block)
    binding = [step for step in steps if step.fields.get("id") == RELEASE_VERSION_STEP_ID]
    if len(binding) != 1:
        return [
            f"job `{CI_GREEN_JOB}` has {len(binding)} steps with `id: {RELEASE_VERSION_STEP_ID}`; "
            "exactly one must bind the release ref to the workspace version"
        ]
    step = binding[0]
    checkouts = [
        candidate
        for candidate in steps[: steps.index(step)]
        if CHECKOUT_ACTION in candidate.fields.get("uses", "")
    ]
    if len(checkouts) != 1 or "ref" not in checkouts[0].nested("with"):
        return [
            f"job `{CI_GREEN_JOB}` must check out the release ref with exactly one "
            f"`{CHECKOUT_ACTION}` step carrying `with.ref` before `{step.name}`"
        ]
    checkout_expression = checkouts[0].nested("with")["ref"]

    def checkout_for(context: EventContext) -> str:
        return render_strict(checkout_expression, context)

    tag = f"v{BINDING_VERSION}"
    alpha_tag = f"alpha/{tag}"

    def dispatch(label: str, ref: str, **inputs: str) -> EventContext:
        return EventContext(label=label, event_name="workflow_dispatch", ref=ref, inputs=inputs)

    tag_push = EventContext(label=f"a {tag} tag push", event_name="push", ref=f"refs/tags/{tag}")
    named_tag = dispatch(
        f"a package dispatch naming release_tag {tag}",
        "refs/heads/main",
        release_tag=tag,
        publish_release_packages="true",
    )
    alpha_on_tag = dispatch(
        f"an alpha crate dispatch on the {alpha_tag} tag",
        f"refs/tags/{alpha_tag}",
        publish_release_packages="true",
        alpha_crates_only="true",
    )
    alpha_named = dispatch(
        f"an alpha crate dispatch naming release_tag {alpha_tag}",
        "refs/heads/main",
        release_tag=alpha_tag,
        publish_release_packages="true",
        alpha_crates_only="true",
    )
    alpha_without_lane = dispatch(
        f"a package dispatch on the {alpha_tag} tag without alpha_crates_only",
        f"refs/tags/{alpha_tag}",
        publish_release_packages="true",
    )
    alpha_named_without_lane = dispatch(
        f"a package dispatch naming release_tag {alpha_tag} without alpha_crates_only",
        "refs/heads/main",
        release_tag=alpha_tag,
        publish_release_packages="true",
    )
    other_tag_push = EventContext(
        label="a tag push of another version", event_name="push", ref="refs/tags/v9.9.9"
    )
    # A run on the version's tag that names another release_tag: the named
    # tag is what is checked out and published, so it must bind. A binding
    # that preferred github.ref would pass these and publish the named ref.
    tag_ref_naming_branch = dispatch(
        f"a package dispatch on the {tag} tag naming release_tag main (a branch)",
        f"refs/tags/{tag}",
        release_tag="main",
        publish_release_packages="true",
    )
    tag_ref_naming_other_tag = dispatch(
        f"a package dispatch on the {tag} tag naming release_tag v9.9.9 "
        "(a tag on another commit)",
        f"refs/tags/{tag}",
        release_tag="v9.9.9",
        publish_release_packages="true",
    )
    # The same with both tags on one commit (as an alpha tag and its
    # version's tag usually are): outside the alpha lane the named alpha tag
    # must not bind, even though the version's tag is the checked-out commit.
    tag_ref_naming_alpha_same_commit = dispatch(
        f"a package dispatch on the {tag} tag naming release_tag {alpha_tag} "
        "(both tags on one commit, no alpha lane)",
        f"refs/tags/{tag}",
        release_tag=alpha_tag,
        publish_release_packages="true",
    )
    # Near misses of the allowed names: exact matches only.
    near_miss_push = EventContext(
        label=f"a tag push of {tag}1 (a near miss of {tag})",
        event_name="push",
        ref=f"refs/tags/{tag}1",
    )
    near_miss_alpha_rc = dispatch(
        f"an alpha crate dispatch on the {alpha_tag}-rc.1 tag (a near miss of {alpha_tag})",
        f"refs/tags/{alpha_tag}-rc.1",
        publish_release_packages="true",
        alpha_crates_only="true",
    )
    near_miss_alpha = dispatch(
        f"an alpha crate dispatch on the {alpha_tag}1 tag (a near miss of {alpha_tag})",
        f"refs/tags/{alpha_tag}1",
        publish_release_packages="true",
        alpha_crates_only="true",
    )
    # The alpha lane binds only its own version's alpha tag.
    other_alpha_on_tag = dispatch(
        "an alpha crate dispatch on the alpha/v9.9.9 tag (another version)",
        "refs/tags/alpha/v9.9.9",
        publish_release_packages="true",
        alpha_crates_only="true",
    )
    other_alpha_named = dispatch(
        "an alpha crate dispatch naming release_tag alpha/v9.9.9 (another version)",
        "refs/heads/main",
        release_tag="alpha/v9.9.9",
        publish_release_packages="true",
        alpha_crates_only="true",
    )
    # (context, tags, head) where head None means "HEAD off the tag".
    must_accept = [
        (tag_push, (tag,), checkout_for(tag_push)),
        (named_tag, (tag,), checkout_for(named_tag)),
        (alpha_on_tag, (alpha_tag,), checkout_for(alpha_on_tag)),
        (alpha_named, (alpha_tag,), checkout_for(alpha_named)),
    ]
    must_refuse = [
        (other_tag_push, ("v9.9.9",), checkout_for(other_tag_push), ""),
        (
            tag_ref_naming_alpha_same_commit,
            (tag, alpha_tag),
            checkout_for(tag_ref_naming_alpha_same_commit),
            "",
        ),
        (near_miss_push, (f"{tag}1",), checkout_for(near_miss_push), ""),
        (near_miss_alpha_rc, (f"{alpha_tag}-rc.1",), checkout_for(near_miss_alpha_rc), ""),
        (near_miss_alpha, (f"{alpha_tag}1",), checkout_for(near_miss_alpha), ""),
        (other_alpha_on_tag, ("alpha/v9.9.9",), checkout_for(other_alpha_on_tag), ""),
        (other_alpha_named, ("alpha/v9.9.9",), checkout_for(other_alpha_named), ""),
        (alpha_without_lane, (alpha_tag,), checkout_for(alpha_without_lane), ""),
        (alpha_named_without_lane, (alpha_tag,), checkout_for(alpha_named_without_lane), ""),
        # release_tag names the version but no such tag exists (a branch).
        (named_tag, (), None, " (no such tag exists)"),
        # The tag exists but the checked-out commit is a later one.
        (named_tag, (tag,), None, " (the tag is not the checked-out commit)"),
        # Every branch dispatch runs with the allowed tags present on the
        # release commit (the branch is on the later one), so a binding that
        # takes a branch named like a tag for that tag, because the tag
        # exists, is refused too.
        *(
            (context, (tag, alpha_tag), checkout_for(context), "")
            for context in BRANCH_DISPATCHES
        ),
    ]
    violations: list[str] = []
    for context in [
        *(entry[0] for entry in must_accept + must_refuse),
        tag_ref_naming_branch,
        tag_ref_naming_other_tag,
    ]:
        if not step_runs(step, context):
            violations.append(
                f"step `{step.name}` does not run on {context.label}, so nothing binds "
                "that publication to its version's tag"
            )
    if violations:
        return violations
    for context, tags, head in must_accept:
        if _run_binding_step(step, context, tags, head) != 0:
            violations.append(f"step `{step.name}` refuses {context.label}")
    for context, tags, head, detail in must_refuse:
        if _run_binding_step(step, context, tags, head) == 0:
            violations.append(
                f"step `{step.name}` accepts {context.label}{detail}; only an allowed tag "
                f"({tag}, or {alpha_tag} in the alpha lane) may publish {BINDING_VERSION}"
            )
    # The named ref sits on the later commit, where the checkout lands.
    for context, later_ref in (
        (tag_ref_naming_branch, "refs/heads/main"),
        (tag_ref_naming_other_tag, "refs/tags/v9.9.9"),
    ):
        if _run_binding_step(step, context, (tag,), checkout_for(context), (later_ref,)) == 0:
            violations.append(
                f"step `{step.name}` accepts {context.label}; the named release_tag is "
                f"what is checked out, and only an allowed tag ({tag}, or {alpha_tag} in "
                f"the alpha lane) may publish {BINDING_VERSION}"
            )
    return violations


READBACK_FLAG = "--readback-only"


def _is_sdk_publish(step: Step) -> bool:
    return step.name.startswith("Publish ") and step.name.endswith(" SDK")


def check_registry_slo(text: str) -> list[str]:
    """Tag releases read every crate back before the SDK packages publish, then
    enforce the 30 minute crates.io publication SLO without blocking them."""
    block = job_block(text, REGISTRY_JOB)
    violations = _job_enabled(block, REGISTRY_JOB, TAG_PUSH)
    steps = job_steps(block)

    verifier = [
        (index, step) for index, step in enumerate(steps) if PUBLIC_VERIFIER in step.run
    ]
    readback = [(i, s) for i, s in verifier if READBACK_FLAG in s.run]
    slo = [(i, s) for i, s in verifier if READBACK_FLAG not in s.run]
    sdk_indexes = [i for i, s in enumerate(steps) if _is_sdk_publish(s)]

    if not readback:
        violations.append(
            f"job `{REGISTRY_JOB}` has no step that reads every crate back with "
            f"`{PUBLIC_VERIFIER} {READBACK_FLAG}` before the SDK packages publish"
        )
    for index, step in readback:
        if not step_runs(step, TAG_PUSH):
            violations.append(f"step `{step.name}` does not run on {TAG_PUSH.label}")
        if sdk_indexes and index > min(sdk_indexes):
            violations.append(
                f"step `{step.name}` runs after an SDK publish step; the SDK "
                "packages must follow the crate readback"
            )

    if not slo:
        violations.append(
            f"job `{REGISTRY_JOB}` has no step that enforces the publication SLO "
            f"with `{PUBLIC_VERIFIER} --slo-seconds`"
        )
    for index, step in slo:
        if not step_runs(step, TAG_PUSH):
            violations.append(f"step `{step.name}` does not run on {TAG_PUSH.label}")
            continue
        if sdk_indexes and index < max(sdk_indexes):
            violations.append(
                f"step `{step.name}` runs before an SDK publish step; SDK "
                "publication must not wait on the publication SLO"
            )
        rendered = render(step.run, TAG_PUSH)
        if "--window-started-at" not in rendered and "--tag-pushed-at" not in rendered:
            violations.append(
                f"step `{step.name}` gives `{PUBLIC_VERIFIER}` no SLO window start"
            )
        match = re.search(r"--slo-seconds[\s=]+(\S+)", rendered)
        if not match:
            violations.append(
                f"step `{step.name}` invokes `{PUBLIC_VERIFIER}` without `--slo-seconds`"
            )
            continue
        value = match.group(1)
        if "${{" in value:
            violations.append(
                f"step `{step.name}` passes `--slo-seconds` as an expression the "
                f"release doctor cannot evaluate: `{value}`"
            )
        elif value != str(TAG_SLO_SECONDS):
            violations.append(
                f"step `{step.name}` passes `--slo-seconds {value}` on {TAG_PUSH.label}; "
                f"the publication SLO is {TAG_SLO_SECONDS} seconds"
            )
    return violations


def _attested_tag(schema: str, **kwargs: object) -> EventContext:
    """A tag push whose require_ci_green verified an attestation of `schema`."""
    return EventContext(
        label=kwargs.pop("label", f"a tag push with a verified schema-{schema} attestation"),
        event_name="push",
        job_outputs={f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": schema},
        **kwargs,
    )


def _runner_env(context: EventContext) -> dict[str, str]:
    """The default environment a runner gives every step of `context`."""
    return {
        "PATH": os.environ.get("PATH", ""),
        "GITHUB_SHA": context.sha,
        "GITHUB_REF": context.ref,
        "GITHUB_REF_NAME": github_ref_name(context.ref),
        "GITHUB_EVENT_NAME": context.event_name,
        "GITHUB_REPOSITORY": "example/meerkat",
        "GITHUB_RUN_ID": context.run_id,
    }


def _run_gate_script(step: Step, context: EventContext) -> tuple[int, str]:
    """Run a gate job's shell step as the runner would."""
    env = {key: render_strict(value, context) for key, value in step.env.items()}
    script = render_strict(step.run, context)
    result = subprocess.run(
        ["bash", "-c", script],
        env={**_runner_env(context), **env},
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode, result.stdout + result.stderr


def check_tag_validation(text: str) -> list[str]:
    """A tag validates on its verified schema-4 exact-tree CI attestation.

    The successful exact-main CI run on the tag's tree already ran everything
    `release_validate_cargo` repeats, so while the self-hosted graph is off a
    tag whose attestation `require_ci_green` verified as schema 4 skips that
    job and `release_validate_gate` passes on the attestation alone. A legacy
    attestation (schema 1-3) still validates with Cargo, manual dispatches
    keep their selected validation lane, and the gate refuses a skipped Cargo
    validation unless the verified attestation is schema 4.
    """
    violations: list[str] = []

    # 1. require_ci_green exposes the schema of the attestation it verified,
    #    written only after the verification passed.
    ci_block = job_block(text, CI_GREEN_JOB)
    outputs = parse_mapping(ci_block, 4).get("outputs", "")
    wiring = re.search(
        rf"\b{ATTESTATION_SCHEMA_OUTPUT}:\s*\$\{{\{{\s*steps\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)\s*\}}\}}",
        outputs,
    )
    if wiring is None:
        violations.append(
            f"job `{CI_GREEN_JOB}` does not expose `{ATTESTATION_SCHEMA_OUTPUT}` from a step output"
        )
    else:
        step_id, output = wiring.groups()
        producers = [
            step for step in job_steps(ci_block) if step.fields.get("id") == step_id
        ]
        if len(producers) != 1:
            violations.append(
                f"job `{CI_GREEN_JOB}` has {len(producers)} steps with `id: {step_id}`; "
                f"exactly one must write `{ATTESTATION_SCHEMA_OUTPUT}`"
            )
        else:
            producer = producers[0]
            run = producer.run
            if ".schema_version == 4" not in run or "jq -e" not in run:
                violations.append(
                    f"step `{producer.name}` writes `{ATTESTATION_SCHEMA_OUTPUT}` but does not "
                    "verify the exact-tree attestation (`jq -e` over its schema-4 fields)"
                )
            elif output not in producer.written_outputs():
                violations.append(
                    f"step `{producer.name}` does not write `{output}` to $GITHUB_OUTPUT"
                )
            else:
                write = re.search(rf"""echo\s+["']?{re.escape(output)}=""", run)
                if write is not None and write.start() < run.index("jq -e"):
                    violations.append(
                        f"step `{producer.name}` writes `{output}` before it verifies the "
                        "attestation, so an unverified schema would reach the gate"
                    )
            if not step_runs(producer, TAG_PUSH):
                violations.append(f"step `{producer.name}` does not run on {TAG_PUSH.label}")

    # 2. The Cargo validation job is skipped exactly where the attestation
    #    stands in for it.
    cargo_block = job_block(text, CARGO_VALIDATION_JOB)
    cargo_if = parse_mapping(cargo_block, 4).get("if")
    graph_on = {GCP_GRAPH_VARIABLE: "true"}
    manual = EventContext(
        label="a package dispatch on the GitHub-hosted backend",
        event_name="workflow_dispatch",
        inputs={
            "release_tag": "v0.0.0",
            "publish_release_packages": "true",
            "release_backend": "github-hosted",
        },
        job_outputs={f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": ""},
    )
    routes = (
        (_attested_tag("4"), False),
        (_attested_tag("1", label="a tag push with a verified legacy (schema 1) attestation"), True),
        (_attested_tag("4", label="a tag push while the self-hosted graph is enabled", variables=graph_on), False),
        (manual, True),
    )
    for context, must_run in routes:
        runs = cargo_if is None or truthy(evaluate(cargo_if, context))
        if runs and not must_run:
            violations.append(
                f"job `{CARGO_VALIDATION_JOB}` runs on {context.label}; the exact-tree CI "
                "attestation already covers it"
            )
        if must_run and not runs:
            violations.append(f"job `{CARGO_VALIDATION_JOB}` is skipped on {context.label}")

    # 3. The gate accepts the attestation route and nothing weaker.
    gate_steps = [step for step in job_steps(job_block(text, VALIDATION_GATE_JOB)) if step.run]
    if len(gate_steps) != 1:
        violations.append(
            f"job `{VALIDATION_GATE_JOB}` has {len(gate_steps)} run steps; expected one gate script"
        )
        return violations
    gate = gate_steps[0]
    skipped = {
        CARGO_VALIDATION_JOB: "skipped",
        "release_validate_buildbuddy": "skipped",
        "release_validate_buildbuddy_full": "skipped",
    }
    cases = (
        (_attested_tag("4", job_results=skipped), True),
        (
            _attested_tag(
                "4",
                label="a tag push with a verified schema-4 attestation and Cargo validation run",
                job_results={**skipped, CARGO_VALIDATION_JOB: "success"},
            ),
            False,
        ),
        (
            _attested_tag(
                "1",
                label="a tag push with a verified legacy (schema 1) attestation",
                job_results={**skipped, CARGO_VALIDATION_JOB: "success"},
            ),
            True,
        ),
        (
            _attested_tag(
                "1",
                label="a tag push with a verified legacy (schema 1) attestation and Cargo validation skipped",
                job_results=skipped,
            ),
            False,
        ),
        (
            _attested_tag(
                "",
                label="a tag push with no verified attestation and Cargo validation skipped",
                job_results=skipped,
            ),
            False,
        ),
        (
            _attested_tag(
                "4",
                label="a tag push whose exact-main CI did not succeed",
                job_results={**skipped, CI_GREEN_JOB: "failure"},
            ),
            False,
        ),
    )
    for context, must_accept in cases:
        status, output = _run_gate_script(gate, context)
        if must_accept and status != 0:
            violations.append(
                f"step `{gate.name}` refuses {context.label}: "
                + " | ".join(line for line in output.splitlines() if line.strip())
            )
        if not must_accept and status == 0:
            violations.append(f"step `{gate.name}` accepts {context.label}")
    return violations


PREBUILD_INPUT = "prebuild_release_artifacts"
PREBUILD_LOOKUP_JOB = "release_prebuild"
PREBUILD_GATE_JOB = "release_prebuild_gate"
PREBUILD_LOOKUP_SCRIPT = "scripts/release-find-prebuild"
PROVENANCE_VERIFIER = "scripts/verify-release-asset-provenance"
BINARY_BUILD_JOBS = ("build_binaries", "build_binaries_buildbuddy", "build_binaries_windows_cross")
WEB_SDK_BUILD_JOB = "build_web_sdk_package"
WEB_SDK_PUBLISH_JOB = "publish_web_sdk"
ASSET_PUBLISH_JOB = "publish_github_release"
# A job whose block contains one of these writes to a public surface.
PUBLISHING_MARKERS = (
    "softprops/action-gh-release",
    "gh release upload",
    "update-homebrew",
    "release-publish-rust-crates.sh",
    "twine upload",
    "npm publish",
    "release-publish-web-sdk-package.sh",
)
PREBUILD_RUN = "424242"


def _job_names(text: str) -> list[str]:
    lines = text.splitlines()
    try:
        start = lines.index("jobs:")
    except ValueError as error:
        raise ContractError("the workflow has no top-level `jobs:`") from error
    return [
        match.group(1)
        for line in lines[start + 1 :]
        if (match := JOB_HEADER.match(line))
    ]


def _job_runs(text: str, job: str, context: EventContext) -> bool:
    condition = parse_mapping(job_block(text, job), 4).get("if")
    return condition is None or truthy(evaluate(condition, context))


def _running_steps(text: str, job: str, context: EventContext) -> list[tuple[int, Step]]:
    return [
        (index, step)
        for index, step in enumerate(job_steps(job_block(text, job)))
        if step_runs(step, context)
    ]


def _checkout_ref(text: str, job: str, context: EventContext) -> str | None:
    for step in job_steps(job_block(text, job)):
        if CHECKOUT_ACTION in step.fields.get("uses", "") and "ref" in step.nested("with"):
            return render_strict(step.nested("with")["ref"], context)
    return None


def _verifier_invocations(step: Step, context: EventContext) -> list[list[str]]:
    """Run the step with a stub verifier and return the argv of each call."""
    env = {key: render_strict(value, context) for key, value in step.env.items()}
    script = render_strict(step.run, context)
    with tempfile.TemporaryDirectory(prefix="release-prebuild-") as scratch:
        record = Path(scratch, "calls")
        stub = Path(scratch, PROVENANCE_VERIFIER)
        stub.parent.mkdir(parents=True)
        stub.write_text(
            "#!/usr/bin/env bash\n"
            f"printf '%s\\x1f' \"$@\" >> '{record}'\n"
            f"printf '\\n' >> '{record}'\n",
            encoding="utf-8",
        )
        stub.chmod(0o755)
        subprocess.run(
            ["bash", "-c", script],
            cwd=scratch,
            env={**_runner_env(context), **env},
            capture_output=True,
            text=True,
            check=False,
        )
        if not record.exists():
            return []
        return [
            [arg for arg in line.split("\x1f") if arg]
            for line in record.read_text(encoding="utf-8").splitlines()
        ]


def check_prebuild(text: str) -> list[str]:
    """A pre-tag prebuild publishes nothing; a tag reuses it only verified.

    A `prebuild_release_artifacts` dispatch on main builds every release
    binary and the Web SDK package of the exact dispatched commit, runs no
    release validation or semver measurement, and publishes nothing. A tag
    push looks the prebuild of its own commit up (`release_prebuild`); when
    one is found it builds nothing, publishes that run's archives only after
    `verify-release-asset-provenance --source-run` (the assets_run_id trust
    path) and the Web SDK package only after its attestation is verified;
    when none is found it builds as before.
    """
    violations: list[str] = []
    names = _job_names(text)
    for job in (PREBUILD_LOOKUP_JOB, PREBUILD_GATE_JOB, *BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB):
        if job not in names:
            return [f"job `{job}` is not defined in the workflow"]

    prebuild_outputs = {
        f"{PREBUILD_LOOKUP_JOB}.run_id": "",
        f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": "",
        f"{CI_GREEN_JOB}.prebuild_tag": "v0.0.0",
    }
    prebuild = EventContext(
        label="a prebuild dispatch on main",
        event_name="workflow_dispatch",
        ref="refs/heads/main",
        sha="1" * 40,
        inputs={PREBUILD_INPUT: "true", "release_backend": "github-hosted"},
        job_outputs=prebuild_outputs,
    )

    # 1. A prebuild publishes nothing, whatever its upstream jobs report.
    for job in names:
        block = "\n".join(job_block(text, job))
        if any(marker in block for marker in PUBLISHING_MARKERS) and _job_runs(text, job, prebuild):
            violations.append(f"job `{job}` publishes on {prebuild.label}")

    # 2. It runs no release validation or semver measurement, does not wait
    #    for CI, and verifies its request.
    for job in ("release_validate_cargo", "release_validate_buildbuddy", "release_validate_gate", SEMVER_GATE_JOB):
        if job in names and _job_runs(text, job, prebuild):
            violations.append(f"job `{job}` runs on {prebuild.label}, which validates nothing")
    ci_steps = job_steps(job_block(text, CI_GREEN_JOB))
    for step in ci_steps:
        if step.fields.get("id") == "ci_run" and step_runs(step, prebuild):
            violations.append(f"step `{step.name}` waits for exact-main CI on {prebuild.label}")
    requests = [step for step in ci_steps if step.fields.get("id") == "prebuild_request"]
    if len(requests) != 1 or not step_runs(requests[0], prebuild) or "tag" not in requests[0].written_outputs():
        violations.append(
            f"job `{CI_GREEN_JOB}` must verify the request with one step `id: prebuild_request` "
            f"that runs on {prebuild.label} and writes its `tag`"
        )
    elif "refs/heads/main" not in requests[0].run:
        violations.append(f"step `{requests[0].name}` does not refuse a prebuild off refs/heads/main")

    # 3. It builds every artifact from exactly the dispatched commit.
    realistic = EventContext(
        **{
            **prebuild.__dict__,
            "job_results": {
                "release_validate_gate": "skipped",
                SEMVER_GATE_JOB: "skipped",
                PREBUILD_LOOKUP_JOB: "skipped",
            },
        }
    )
    for job in ("build_binaries", "build_binaries_windows_cross", WEB_SDK_BUILD_JOB, "build_binaries_gate", PREBUILD_GATE_JOB):
        if not _job_runs(text, job, realistic):
            violations.append(f"job `{job}` is skipped on {prebuild.label}")
    for job in (*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB):
        ref = _checkout_ref(text, job, prebuild)
        if ref != prebuild.sha:
            violations.append(
                f"job `{job}` checks out `{ref}` on {prebuild.label}, not the dispatched commit "
                "(github.sha): a moving main would be built instead"
            )

    # 4. The prebuild gate passes only on every artifact, and the lookup
    #    requires exactly that job.
    gate_steps = [step for step in job_steps(job_block(text, PREBUILD_GATE_JOB)) if step.run]
    gate_name = parse_mapping(job_block(text, PREBUILD_GATE_JOB), 4).get("name", "")
    if len(gate_steps) != 1:
        violations.append(f"job `{PREBUILD_GATE_JOB}` must have exactly one run step")
    else:
        for failed in ("build_binaries_gate", WEB_SDK_BUILD_JOB):
            context = EventContext(**{**realistic.__dict__, "job_results": {failed: "failure"}})
            status, _ = _run_gate_script(gate_steps[0], context)
            if status == 0:
                violations.append(f"job `{PREBUILD_GATE_JOB}` passes although `{failed}` failed")
        status, output = _run_gate_script(gate_steps[0], realistic)
        if status != 0:
            violations.append(f"job `{PREBUILD_GATE_JOB}` refuses a complete prebuild: {output.strip()}")
    lookup_script = Path(__file__).resolve().with_name(Path(PREBUILD_LOOKUP_SCRIPT).name)
    try:
        lookup_text = lookup_script.read_text(encoding="utf-8")
    except OSError:
        lookup_text = ""
    if f'PREBUILD_GATE_JOB="{gate_name}"' not in lookup_text:
        violations.append(
            f"{PREBUILD_LOOKUP_SCRIPT} does not require the `{gate_name}` job that marks a complete prebuild"
        )

    # 5. A tag looks its own commit's prebuild up; dispatches never do.
    if not _job_runs(text, PREBUILD_LOOKUP_JOB, TAG_PUSH):
        violations.append(f"job `{PREBUILD_LOOKUP_JOB}` does not run on {TAG_PUSH.label}")
    for context in (PACKAGE_RECOVERY, prebuild):
        if _job_runs(text, PREBUILD_LOOKUP_JOB, context):
            violations.append(f"job `{PREBUILD_LOOKUP_JOB}` runs on {context.label}")
    if not any(PREBUILD_LOOKUP_SCRIPT in step.run for step in job_steps(job_block(text, PREBUILD_LOOKUP_JOB))):
        violations.append(f"job `{PREBUILD_LOOKUP_JOB}` does not run `{PREBUILD_LOOKUP_SCRIPT}`")

    # 6. A tag whose commit was prebuilt builds nothing and publishes the
    #    prebuild's artifacts only through the attested-archive verification.
    def tag_push(run_id: str) -> EventContext:
        return EventContext(
            label=f"a tag push {'with a prebuild found' if run_id else 'without a prebuild'}",
            event_name="push",
            job_outputs={
                f"{PREBUILD_LOOKUP_JOB}.run_id": run_id,
                f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": "4",
                f"{CI_GREEN_JOB}.prebuild_tag": "",
            },
        )

    reused, fresh = tag_push(PREBUILD_RUN), tag_push("")
    for job in (*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB):
        if _job_runs(text, job, reused):
            violations.append(f"job `{job}` builds on {reused.label}")
    for job in ("build_binaries_windows_cross", WEB_SDK_BUILD_JOB):
        if not _job_runs(text, job, fresh):
            violations.append(f"job `{job}` is skipped on {fresh.label}")

    for job in (ASSET_PUBLISH_JOB, WEB_SDK_PUBLISH_JOB):
        if not _job_runs(text, job, reused):
            violations.append(f"job `{job}` does not run on {reused.label}")
            continue
        steps = _running_steps(text, job, reused)
        downloads = [
            step for _, step in steps if "actions/download-artifact" in step.fields.get("uses", "")
        ]
        if not downloads:
            violations.append(f"job `{job}` downloads nothing on {reused.label}")
        for step in downloads:
            run_id = render_strict(step.nested("with").get("run-id", ""), reused)
            if run_id != PREBUILD_RUN:
                violations.append(
                    f"step `{step.name}` downloads from run `{run_id or 'this run'}` on "
                    f"{reused.label}, not the prebuild run"
                )

    verify_steps = [
        step for _, step in _running_steps(text, ASSET_PUBLISH_JOB, reused) if PROVENANCE_VERIFIER in step.run
    ]
    if len(verify_steps) != 1:
        violations.append(
            f"job `{ASSET_PUBLISH_JOB}` must run `{PROVENANCE_VERIFIER}` in exactly one step on {reused.label}"
        )
    else:
        calls = _verifier_invocations(verify_steps[0], reused)
        if len(calls) != 1 or calls[0][:2] != ["--source-run", PREBUILD_RUN] or "--stamps" not in calls[0]:
            violations.append(
                f"step `{verify_steps[0].name}` verifies {reused.label} with {calls}; expected "
                f"`{PROVENANCE_VERIFIER} --source-run {PREBUILD_RUN} --stamps ...`, the "
                "assets_run_id trust path"
            )
        fresh_calls = _verifier_invocations(verify_steps[0], fresh)
        if len(fresh_calls) != 1 or "--source-run" in fresh_calls[0]:
            violations.append(
                f"step `{verify_steps[0].name}` verifies {fresh.label} with {fresh_calls}; "
                "expected the exact-tag verification"
            )

    web_steps = _running_steps(text, WEB_SDK_PUBLISH_JOB, reused)
    verify_index = next(
        (
            index
            for index, step in web_steps
            if "gh attestation verify" in step.run and "--source-ref refs/heads/main" in step.run
        ),
        None,
    )
    publish_index = next(
        (index for index, step in web_steps if "release-publish-web-sdk-package.sh" in step.run),
        None,
    )
    if verify_index is None or publish_index is None or verify_index > publish_index:
        violations.append(
            f"job `{WEB_SDK_PUBLISH_JOB}` must verify the prebuilt package's attestation "
            f"(`gh attestation verify ... --source-ref refs/heads/main`) before publishing on {reused.label}"
        )
    return violations

def _readiness_push(needed: str, baseline: str) -> EventContext:
    return EventContext(
        label=f"a main push with needed={needed!r} and notes baseline {baseline!r}",
        event_name="push",
        ref="refs/heads/main",
        step_outputs={"unpublished.needed": needed, f"{NOTES_STEP_ID}.{NOTES_OUTPUT}": baseline},
    )


def check_readiness_attestation(text: str) -> list[str]:
    """Only a release tree becomes release evidence (release-semver-readiness.yml).

    A post-release tree is measured against its own tag, so its green result
    says nothing about the breaks since the release before. `release_semver_gate`
    trusts the main-push attestation by tree and version alone, so the
    attestation and its upload must run for a release tree and never for a
    post-release one, keyed on the output the classifying step really writes.
    """
    block = job_block(text, READINESS_JOB)
    steps = job_steps(block)
    violations: list[str] = []

    notes = [step for step in steps if step.fields.get("id") == NOTES_STEP_ID]
    if len(notes) != 1:
        violations.append(
            f"job `{READINESS_JOB}` has {len(notes)} steps with `id: {NOTES_STEP_ID}`; "
            "exactly one must classify the measured notes"
        )
    else:
        written = notes[0].written_outputs()
        if NOTES_OUTPUT not in written:
            violations.append(
                f"step `{notes[0].name}` writes {sorted(written) or 'no outputs'} to "
                f"$GITHUB_OUTPUT, not `{NOTES_OUTPUT}`, which the attestation is gated on"
            )
        if not step_runs(notes[0], _readiness_push("true", "published")):
            violations.append(f"step `{notes[0].name}` does not run when a measurement is needed")

    attestation = [step for step in steps if "attestation.json" in step.run]
    upload = [
        step
        for step in steps
        if "upload-artifact" in step.fields.get("uses", "")
        and EVIDENCE_ARTIFACT_PREFIX in step.fields.get("with", "")
    ]
    if not attestation:
        violations.append(f"job `{READINESS_JOB}` has no step that writes attestation.json")
    if not upload:
        violations.append(
            f"job `{READINESS_JOB}` has no step that uploads the "
            f"`{EVIDENCE_ARTIFACT_PREFIX}<tree>` artifact"
        )
    release_tree = _readiness_push("true", "published")
    post_release = _readiness_push("true", "workspace-version")
    not_needed = _readiness_push("false", "published")
    for step in attestation + upload:
        if not step_runs(step, release_tree):
            violations.append(f"step `{step.name}` does not run on {release_tree.label}")
        for context in (post_release, not_needed):
            if step_runs(step, context):
                violations.append(f"step `{step.name}` also runs on {context.label}")
    return violations


CHECKS: dict[str, Callable[[str], list[str]]] = {
    "semver-evidence": check_semver_evidence,
    "registry-slo": check_registry_slo,
    "dispatch-binding": check_dispatch_binding,
    "tag-validation": check_tag_validation,
    "prebuild": check_prebuild,
}

# Checks of release-semver-readiness.yml; run them by name with
# `--workflow .github/workflows/release-semver-readiness.yml`.
READINESS_CHECKS: dict[str, Callable[[str], list[str]]] = {
    "readiness-attestation": check_readiness_attestation,
}


def run_checks(text: str, names: Iterable[str]) -> list[str]:
    violations: list[str] = []
    for name in names:
        try:
            violations.extend({**CHECKS, **READINESS_CHECKS}[name](text))
        except ContractError as error:
            violations.append(f"{name}: {error}")
    return violations


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--workflow",
        type=Path,
        default=DEFAULT_WORKFLOW,
        help=f"release workflow to inspect (default: {DEFAULT_WORKFLOW})",
    )
    parser.add_argument(
        "checks",
        nargs="*",
        choices=[*CHECKS, *READINESS_CHECKS, "all"],
        default=["all"],
        help="which contract checks to run (default: all release.yml checks)",
    )
    args = parser.parse_args(argv)
    names = list(CHECKS) if "all" in args.checks else args.checks
    try:
        text = args.workflow.read_text(encoding="utf-8")
    except OSError as error:
        print(f"cannot read {args.workflow}: {error}")
        return 2
    violations = run_checks(text, names)
    for violation in violations:
        print(violation)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
