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
attested-archive verification, or making a job wait for the semver baseline
rustdoc all fail and name the defect.

Only the Python standard library is used, so the doctor and its contract test
run wherever `python3` does.
"""

from __future__ import annotations

import argparse
import json
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
# require_ci_green's output: the attestation proves the Cargo validation lanes ran.
COVERS_OUTPUT = "ci_covers_release_validation"
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
    def raw_run(self) -> str:
        """The `run:` block scalar with its relative indentation kept.

        `run` strips every line, which is enough for shell; an embedded
        heredoc in an indentation-sensitive language needs the original
        layout.
        """
        for index, line in enumerate(self.lines):
            match = KEY_LINE.match(line)
            if not (match and len(match.group(1)) == 8 and match.group(2) == "run"):
                continue
            body: list[str] = []
            for candidate in self.lines[index + 1 :]:
                if candidate.strip() and _indent(candidate) <= 8:
                    break
                body.append(candidate)
            width = min((_indent(line) for line in body if line.strip()), default=0)
            return "\n".join(line[width:] for line in body).strip("\n") + "\n"
        return ""

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


def _attested_tag(covers: str, schema: str = "4", **kwargs: object) -> EventContext:
    """A tag push whose require_ci_green verified an attestation of `schema`
    that does (covers == "true") or does not cover the Cargo lanes."""
    return EventContext(
        label=kwargs.pop("label", f"a tag push whose attestation covers the Cargo lanes ({covers})"),
        event_name="push",
        job_outputs={
            f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": schema,
            f"{CI_GREEN_JOB}.{COVERS_OUTPUT}": covers,
        },
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


def _attestation(**overrides: object) -> dict:
    """A schema-4 attestation for the binding scenario, every lane green."""
    document: dict = {
        "schema_version": 4,
        "repository": "example/meerkat",
        "commit_sha": "1" * 40,
        "tree_sha": "2" * 40,
        "workflow": "CI",
        "workflow_run_id": "11",
        "workflow_run_attempt": "1",
        "event": "push",
        "ref": "refs/heads/main",
        "validation_backend": "github-hosted-cargo",
        "validation_result": "success",
        "plan": {"mode": "workspace", "generated_contract": True},
        "component_results": {
            "changes": "success",
            "fmt_governance": "success",
            "ratchets": "success",
            "sdk_host": "success",
        },
    }
    for path, value in overrides.items():
        target = document
        keys = path.split("__")
        for key in keys[:-1]:
            target = target.setdefault(key, {})
        if value is None:
            target.pop(keys[-1], None)
        else:
            target[keys[-1]] = value
    return document


def _reused_evidence(**overrides: str) -> dict:
    """reused_test_evidence as CI writes it for a release-projection plan."""
    return {
        "classifier": "release-projection-only",
        "from_version": "0.0.0",
        "to_version": "0.0.1",
        "base_commit_sha": "3" * 40,
        "base_tree_sha": "4" * 40,
        "workflow": "CI",
        "workflow_run_id": "9",
        "workflow_run_attempt": "1",
        "attestation_artifact": "meerkat-ci-attestation-" + "3" * 40 + "-1",
        **overrides,
    }


def _coverage_of(step: Step, attestation: dict) -> tuple[int, dict[str, str]]:
    """Run the attestation step on `attestation`; its exit and outputs."""
    context = EventContext(
        label=TAG_PUSH.label,
        event_name="push",
        step_outputs={
            "ci_run.run_id": "11",
            "ci_run.run_attempt": "1",
            "release_ref.sha": "1" * 40,
            "release_ref.tree": "2" * 40,
        },
    )
    env = {key: render_strict(value, context) for key, value in step.env.items()}
    with tempfile.TemporaryDirectory(prefix="release-attestation-") as scratch:
        directory = Path(scratch, "meerkat-ci-attestation")
        directory.mkdir()
        Path(directory, "attestation.json").write_text(json.dumps(attestation), encoding="utf-8")
        output = Path(scratch, "github-output")
        output.write_text("", encoding="utf-8")
        result = subprocess.run(
            ["bash", "-c", render_strict(step.raw_run, context)],
            cwd=scratch,
            env={
                **_runner_env(context),
                "GITHUB_REPOSITORY": "example/meerkat",
                "RUNNER_TEMP": scratch,
                "GITHUB_OUTPUT": str(output),
                **env,
            },
            capture_output=True,
            text=True,
            check=False,
        )
        outputs = dict(
            line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines() if "=" in line
        )
        return result.returncode, outputs


def check_tag_validation(text: str) -> list[str]:
    """A tag skips Cargo validation only on an attestation that covers it.

    `release_validate_cargo` repeats fmt-governance, the generated-contract
    ratchets (which main CI runs only when its plan selects them) and the SDK
    suites. require_ci_green verifies the exact-tree attestation and exposes
    whether it proves all of those ran (`ci_covers_release_validation`); only
    then, while the self-hosted graph is off, is the job skipped and does the
    gate pass on the attestation. Otherwise, a legacy attestation included,
    the tag validates with Cargo, and manual dispatches keep their lane.
    """
    violations: list[str] = []

    # 1. The attestation step computes the coverage from the attestation it
    #    verified, and require_ci_green exposes it.
    ci_block = job_block(text, CI_GREEN_JOB)
    outputs = parse_mapping(ci_block, 4).get("outputs", "")
    wiring = re.search(
        rf"\b{COVERS_OUTPUT}:\s*\$\{{\{{\s*steps\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)\s*\}}\}}",
        outputs,
    )
    if wiring is None:
        return [f"job `{CI_GREEN_JOB}` does not expose `{COVERS_OUTPUT}` from a step output"]
    step_id, output = wiring.groups()
    producers = [step for step in job_steps(ci_block) if step.fields.get("id") == step_id]
    if len(producers) != 1:
        return [f"job `{CI_GREEN_JOB}` has {len(producers)} steps with `id: {step_id}`"]
    producer = producers[0]
    if not step_runs(producer, TAG_PUSH):
        violations.append(f"step `{producer.name}` does not run on {TAG_PUSH.label}")
    cases = (
        ("every lane ran", _attestation(), "true"),
        ("the generated-contract ratchets did not run", _attestation(plan__generated_contract=False), "false"),
        ("the plan does not record the ratchets", _attestation(plan__generated_contract=None), "false"),
        ("the SDK suites were skipped", _attestation(component_results__sdk_host="skipped"), "false"),
        (
            "a release-projection plan reused a verified parent run for the SDK lanes",
            _attestation(
                plan__mode="release-projection",
                component_results__sdk_host="skipped",
                reused_test_evidence=_reused_evidence(),
            ),
            "true",
        ),
        (
            "a release-projection plan names no reused run",
            _attestation(plan__mode="release-projection", component_results__sdk_host="skipped"),
            "false",
        ),
        (
            "a release-projection plan whose evidence comes from another classifier",
            _attestation(
                plan__mode="release-projection",
                component_results__sdk_host="skipped",
                reused_test_evidence=_reused_evidence(classifier="something-else"),
            ),
            "false",
        ),
        (
            "a release-projection plan whose evidence names no run attempt",
            _attestation(
                plan__mode="release-projection",
                component_results__sdk_host="skipped",
                reused_test_evidence=_reused_evidence(workflow_run_attempt=""),
            ),
            "false",
        ),
    )
    for label, attestation, expected in cases:
        status, written = _coverage_of(producer, attestation)
        if status != 0:
            violations.append(f"step `{producer.name}` rejects a valid attestation where {label}")
            continue
        if written.get(output) != expected:
            violations.append(
                f"step `{producer.name}` writes {output}={written.get(output, '<nothing>')} where {label}; "
                f"expected {expected}"
            )
    status, written = _coverage_of(producer, _attestation(component_results__ratchets="failure"))
    if status == 0 and written.get(output) == "true":
        violations.append(f"step `{producer.name}` reports coverage for an attestation it should refuse")

    # 2. The Cargo validation job is skipped exactly where the attestation
    #    stands in for it.
    cargo_if = parse_mapping(job_block(text, CARGO_VALIDATION_JOB), 4).get("if")
    graph_on = {GCP_GRAPH_VARIABLE: "true"}
    manual = EventContext(
        label="a package dispatch on the GitHub-hosted backend",
        event_name="workflow_dispatch",
        inputs={
            "release_tag": "v0.0.0",
            "publish_release_packages": "true",
            "release_backend": "github-hosted",
        },
        job_outputs={f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": "", f"{CI_GREEN_JOB}.{COVERS_OUTPUT}": ""},
    )
    routes = (
        (_attested_tag("true"), False),
        (_attested_tag("false", label="a tag push whose schema-4 attestation does not cover the Cargo lanes"), True),
        (_attested_tag("false", "1", label="a tag push with a verified legacy (schema 1) attestation"), True),
        (_attested_tag("true", label="a tag push while the self-hosted graph is enabled", variables=graph_on), False),
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
    ran = {**skipped, CARGO_VALIDATION_JOB: "success"}
    gate_cases = (
        (_attested_tag("true", job_results=skipped), True),
        (_attested_tag("true", label="a covering attestation with Cargo validation run", job_results=ran), False),
        (_attested_tag("false", label="a non-covering attestation with Cargo validation run", job_results=ran), True),
        (
            _attested_tag(
                "false",
                label="a tag push whose attestation does not cover the Cargo lanes and Cargo validation skipped",
                job_results=skipped,
            ),
            False,
        ),
        (
            _attested_tag(
                "false", "1",
                label="a tag push with a verified legacy (schema 1) attestation and Cargo validation skipped",
                job_results=skipped,
            ),
            False,
        ),
        (
            _attested_tag(
                "true",
                label="a tag push whose exact-main CI did not succeed",
                job_results={**skipped, CI_GREEN_JOB: "failure"},
            ),
            False,
        ),
    )
    for context, must_accept in gate_cases:
        status, output_text = _run_gate_script(gate, context)
        if must_accept and status != 0:
            violations.append(
                f"step `{gate.name}` refuses {context.label}: "
                + " | ".join(line for line in output_text.splitlines() if line.strip())
            )
        if not must_accept and status == 0:
            violations.append(f"step `{gate.name}` accepts {context.label}")
    return violations


PREBUILD_INPUT = "prebuild_release_artifacts"
PREBUILD_LOOKUP_JOB = "release_prebuild"
PREBUILD_GATE_JOB = "release_prebuild_gate"
PREBUILD_LOOKUP_SCRIPT = "scripts/release-find-prebuild"
PROVENANCE_VERIFIER = "scripts/verify-release-asset-provenance"
BINARY_BUILD_JOBS = (
    "build_binaries",
    "build_binaries_buildbuddy",
    "build_binaries_windows_cross",
    "build_binaries_macos_x86_cross",
)
CRITICAL_PUBLISH_JOB = "publish_critical_release_assets"
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
PREBUILD_ATTEMPT = "2"


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
        f"{PREBUILD_LOOKUP_JOB}.run_attempt": "",
        f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": "",
        f"{CI_GREEN_JOB}.{COVERS_OUTPUT}": "",
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
    for job in (
        "build_binaries",
        "build_binaries_windows_cross",
        "build_binaries_macos_x86_cross",
        WEB_SDK_BUILD_JOB,
        "build_binaries_critical_gate",
        "build_binaries_gate",
        PREBUILD_GATE_JOB,
    ):
        if not _job_runs(text, job, realistic):
            violations.append(f"job `{job}` is skipped on {prebuild.label}")
    for job in (*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB):
        ref = _checkout_ref(text, job, prebuild)
        if ref != prebuild.sha:
            violations.append(
                f"job `{job}` checks out `{ref}` on {prebuild.label}, not the dispatched commit "
                "(github.sha): a moving main would be built instead"
            )

    # 4. The prebuild gate passes only on the critical binaries and the Web
    #    SDK package, and the lookup requires exactly that job.
    gate_steps = [step for step in job_steps(job_block(text, PREBUILD_GATE_JOB)) if step.run]
    gate_name = parse_mapping(job_block(text, PREBUILD_GATE_JOB), 4).get("name", "")
    if len(gate_steps) != 1:
        violations.append(f"job `{PREBUILD_GATE_JOB}` must have exactly one run step")
    else:
        for failed in ("build_binaries_critical_gate", WEB_SDK_BUILD_JOB):
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
            step_outputs={"manifest.upload": "true"},
            job_outputs={
                f"{PREBUILD_LOOKUP_JOB}.run_id": run_id,
                f"{PREBUILD_LOOKUP_JOB}.run_attempt": PREBUILD_ATTEMPT if run_id else "",
                f"{CI_GREEN_JOB}.{ATTESTATION_SCHEMA_OUTPUT}": "4",
                f"{CI_GREEN_JOB}.{COVERS_OUTPUT}": "true",
                f"{CI_GREEN_JOB}.prebuild_tag": "",
            },
        )

    reused, fresh = tag_push(PREBUILD_RUN), tag_push("")

    # Reusing a prebuild never bypasses a gate: with exact-main CI, release
    # validation or the semver gate failed, nothing publishes.
    for failed in (CI_GREEN_JOB, VALIDATION_GATE_JOB, SEMVER_GATE_JOB):
        context = EventContext(
            **{**reused.__dict__, "label": f"{reused.label} and `{failed}` failed", "job_results": {failed: "failure"}}
        )
        for job in (CRITICAL_PUBLISH_JOB, ASSET_PUBLISH_JOB, WEB_SDK_PUBLISH_JOB, REGISTRY_JOB):
            if job in names and _job_runs(text, job, context):
                violations.append(f"job `{job}` publishes on {context.label}")
    for job in (*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB):
        if _job_runs(text, job, reused):
            violations.append(f"job `{job}` builds on {reused.label}")
    for job in ("build_binaries_windows_cross", "build_binaries_macos_x86_cross", WEB_SDK_BUILD_JOB):
        if not _job_runs(text, job, fresh):
            violations.append(f"job `{job}` is skipped on {fresh.label}")

    for job in (CRITICAL_PUBLISH_JOB, ASSET_PUBLISH_JOB, WEB_SDK_PUBLISH_JOB):
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

    for job in (CRITICAL_PUBLISH_JOB, ASSET_PUBLISH_JOB):
        verify_steps = [
            step for _, step in _running_steps(text, job, reused) if PROVENANCE_VERIFIER in step.run
        ]
        if len(verify_steps) != 1:
            violations.append(
                f"job `{job}` must run `{PROVENANCE_VERIFIER}` in exactly one step on {reused.label}"
            )
            continue
        calls = _verifier_invocations(verify_steps[0], reused)
        if (
            len(calls) != 1
            or calls[0][:2] != ["--source-run", PREBUILD_RUN]
            or "--stamps" not in calls[0]
            or "--prebuild" not in calls[0]
            or "--source-attempt" not in calls[0]
            or calls[0][calls[0].index("--source-attempt") + 1] != PREBUILD_ATTEMPT
        ):
            violations.append(
                f"step `{verify_steps[0].name}` verifies {reused.label} with {calls}; expected "
                f"`{PROVENANCE_VERIFIER} --source-run {PREBUILD_RUN} --source-attempt {PREBUILD_ATTEMPT} "
                "--prebuild --stamps ...`, the assets_run_id trust path for the tag's own prebuild, "
                "pinned to the attempt the lookup examined"
            )
        else:
            # The critical archives are verified against the prebuild's
            # critical gate (its trailing lanes may still run); the complete
            # set against its full binary build gate, the default.
            args = calls[0]
            gate = args[args.index("--source-gate") + 1] if "--source-gate" in args else "Release binary build gate"
            wanted = (
                "Release critical binary build gate" if job == CRITICAL_PUBLISH_JOB else "Release binary build gate"
            )
            if gate != wanted:
                violations.append(
                    f"step `{verify_steps[0].name}` requires the prebuild's `{gate}` on {reused.label}; "
                    f"expected `{wanted}`"
                )
        fresh_calls = _verifier_invocations(verify_steps[0], fresh)
        if len(fresh_calls) != 1 or "--source-run" in fresh_calls[0]:
            violations.append(
                f"step `{verify_steps[0].name}` verifies {fresh.label} with {fresh_calls}; "
                "expected the exact-tag verification"
            )

    # The trailing publisher downloads the prebuild's archives only after
    # that run's full binary build gate passed (the trailing lanes may still
    # be running when the prebuild is selected).
    publisher_steps = _running_steps(text, ASSET_PUBLISH_JOB, reused)
    wait_index = next(
        (
            index
            for index, step in publisher_steps
            if "Release binary build gate" in step.run
            and "PREBUILD_RUN_ID" in step.run
            and "PREBUILD_RUN_ATTEMPT" in step.run
        ),
        None,
    )
    download_index = next(
        (
            index
            for index, step in publisher_steps
            if "actions/download-artifact" in step.fields.get("uses", "")
        ),
        None,
    )
    if wait_index is None or download_index is None or wait_index > download_index:
        violations.append(
            f"job `{ASSET_PUBLISH_JOB}` must wait for the prebuild run's `Release binary build gate` "
            f"before downloading its archives on {reused.label}"
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

SEMVER_BASELINE_JOB = "publish_semver_baseline"
SEMVER_BASELINE_ATTACH_JOB = "attach_semver_baseline"


def _job_needs(text: str, job: str) -> list[str]:
    raw = parse_mapping(job_block(text, job), 4).get("needs", "").strip()
    if raw.startswith("[") and raw.endswith("]"):
        raw = raw[1:-1]
    return [item for item in re.split(r"[\s,]+", raw) if item and item != "-"]


def check_semver_baseline(text: str) -> list[str]:
    """The semver baseline rustdoc is off every path a user waits on.

    Its rustdoc build starts with the release (it waits for nothing that
    publishes), no job waits for it except the attach job, nothing waits for
    the attach job, and only the attach job writes to the GitHub release,
    after the release is published, from the artifact the build uploaded.
    """
    violations: list[str] = []
    names = _job_names(text)
    for job in (SEMVER_BASELINE_JOB, SEMVER_BASELINE_ATTACH_JOB):
        if job not in names:
            return [f"job `{job}` is not defined in the workflow"]
    needs = {job: _job_needs(text, job) for job in names}

    for job, upstream in needs.items():
        if job != SEMVER_BASELINE_ATTACH_JOB and SEMVER_BASELINE_JOB in upstream:
            violations.append(f"job `{job}` waits for the semver baseline rustdoc (`{SEMVER_BASELINE_JOB}`)")
        if SEMVER_BASELINE_ATTACH_JOB in upstream:
            violations.append(f"job `{job}` waits for the semver baseline attachment (`{SEMVER_BASELINE_ATTACH_JOB}`)")

    closure: set[str] = set()
    frontier = list(needs[SEMVER_BASELINE_JOB])
    while frontier:
        job = frontier.pop()
        if job in closure or job not in needs:
            continue
        closure.add(job)
        frontier.extend(needs[job])
    blocking = sorted(
        job
        for job in closure
        if any(marker in "\n".join(job_block(text, job)) for marker in PUBLISHING_MARKERS)
        or job.startswith("build_")
    )
    if blocking:
        violations.append(
            f"job `{SEMVER_BASELINE_JOB}` waits for {', '.join(f'`{job}`' for job in blocking)}; "
            "the rustdoc build must run in parallel with publication, not after it"
        )

    if "publish_github_release" not in needs[SEMVER_BASELINE_ATTACH_JOB]:
        violations.append(
            f"job `{SEMVER_BASELINE_ATTACH_JOB}` does not wait for `publish_github_release`, "
            "so it could attach to a release that is not published"
        )
    build_steps = job_steps(job_block(text, SEMVER_BASELINE_JOB))
    attach_steps = job_steps(job_block(text, SEMVER_BASELINE_ATTACH_JOB))
    if any("gh release upload" in step.run for step in build_steps):
        violations.append(f"job `{SEMVER_BASELINE_JOB}` writes to the GitHub release itself")
    if not any("gh release upload" in step.run for step in attach_steps):
        violations.append(f"job `{SEMVER_BASELINE_ATTACH_JOB}` does not attach the baseline")
    uploaded = {
        step.nested("with").get("name")
        for step in build_steps
        if "actions/upload-artifact" in step.fields.get("uses", "")
    }
    downloaded = {
        step.nested("with").get("name")
        for step in attach_steps
        if "actions/download-artifact" in step.fields.get("uses", "")
    }
    if not uploaded or not downloaded or not downloaded <= uploaded:
        violations.append(
            f"job `{SEMVER_BASELINE_ATTACH_JOB}` must attach the artifact `{SEMVER_BASELINE_JOB}` "
            f"uploads (uploads {sorted(filter(None, uploaded))}, downloads {sorted(filter(None, downloaded))})"
        )
    for job in (SEMVER_BASELINE_JOB, SEMVER_BASELINE_ATTACH_JOB):
        if not _job_runs(text, job, TAG_PUSH):
            violations.append(f"job `{job}` does not run on {TAG_PUSH.label}")
    return violations

CRITICAL_TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin")
TRAILING_TARGETS = ("x86_64-apple-darwin", "x86_64-pc-windows-msvc")
TRAILING_BUILD_JOBS = ("build_binaries_windows_cross", "verify_windows_binaries", "build_binaries_macos_x86_cross")
RELEASE_BINARIES = ("rkat", "rkat-rpc", "rkat-rest", "rkat-mcp")
GH_RELEASE_ACTION = "softprops/action-gh-release"


def _published_archives(text: str, job: str, context: EventContext, version: str) -> set[str]:
    """Which archives the job's release upload would publish under `context`.

    Every release archive name (critical and trailing targets) is laid out
    where the job's artifact downloads put them; the job's shell steps run in
    order up to its provenance verification (selection precedes
    verification), and the release upload's `files` patterns are then
    resolved against the result.
    """
    steps = [step for step in job_steps(job_block(text, job)) if step_runs(step, context)]
    with tempfile.TemporaryDirectory(prefix="release-trailing-") as scratch:
        for step in steps:
            if "actions/download-artifact" not in step.fields.get("uses", ""):
                continue
            root = Path(scratch, step.nested("with").get("path", "."))
            for target in (*CRITICAL_TARGETS, *TRAILING_TARGETS):
                extension = "zip" if "windows" in target else "tar.gz"
                for binary in RELEASE_BINARIES:
                    lane = root / f"meerkat-{target}-{binary}"
                    lane.mkdir(parents=True, exist_ok=True)
                    Path(lane, f"{binary}-{version}-{target}.{extension}").write_bytes(b"archive")
        for step in steps:
            if PROVENANCE_VERIFIER in step.run:
                break
            if not step.raw_run.strip():
                continue
            env = {key: render_strict(value, context) for key, value in step.env.items()}
            subprocess.run(
                ["bash", "-c", render_strict(step.raw_run, context)],
                cwd=scratch,
                env={**_runner_env(context), **env},
                capture_output=True,
                text=True,
                check=False,
            )
        published: set[str] = set()
        for step in steps:
            if GH_RELEASE_ACTION not in step.fields.get("uses", ""):
                continue
            for pattern in step.nested("with").get("files", "").split():
                published.update(path.name for path in Path(scratch).glob(pattern))
        return published


def _steps_of(text: str, job: str) -> list[Step]:
    """The job's steps; none for a job that calls a reusable workflow."""
    try:
        return job_steps(job_block(text, job))
    except ContractError:
        return []


def check_trailing_platforms(text: str) -> list[str]:
    """Intel macOS and Windows trail; nothing replaces a published asset.

    The critical publisher puts exactly the Linux (x86_64, aarch64) and macOS
    arm64 archives on the release as soon as their lane passes, waiting for no
    trailing (Intel macOS, Windows) job. Every archive upload is add-only, the
    complete manifest is written only by publish_github_release after every
    lane (and refuses to change a published one), and Homebrew is updated only
    after that trailing publisher.
    """
    violations: list[str] = []
    names = _job_names(text)
    for job in (CRITICAL_PUBLISH_JOB, ASSET_PUBLISH_JOB, "build_binaries_critical_gate", "update_homebrew"):
        if job not in names:
            return [f"job `{job}` is not defined in the workflow"]
    needs = {job: _job_needs(text, job) for job in names}

    def closure(job: str) -> set[str]:
        seen: set[str] = set()
        frontier = list(needs[job])
        while frontier:
            upstream = frontier.pop()
            if upstream in seen or upstream not in needs:
                continue
            seen.add(upstream)
            frontier.extend(needs[upstream])
        return seen

    for job in (CRITICAL_PUBLISH_JOB, PREBUILD_GATE_JOB):
        if job not in needs:
            continue
        waits = sorted(set(TRAILING_BUILD_JOBS + ("build_binaries_gate",)) & closure(job))
        if waits:
            violations.append(
                f"job `{job}` waits for {', '.join(f'`{upstream}`' for upstream in waits)}; "
                "the trailing Intel macOS and Windows lanes must not gate the critical assets"
            )
    if not _job_runs(text, CRITICAL_PUBLISH_JOB, TAG_PUSH):
        violations.append(f"job `{CRITICAL_PUBLISH_JOB}` does not run on {TAG_PUSH.label}")

    tag = EventContext(
        label=TAG_PUSH.label,
        event_name="push",
        job_outputs={f"{PREBUILD_LOOKUP_JOB}.run_id": ""},
    )
    expected = {
        f"{binary}-0.0.0-{target}.tar.gz" for binary in RELEASE_BINARIES for target in CRITICAL_TARGETS
    }
    published = _published_archives(text, CRITICAL_PUBLISH_JOB, tag, "0.0.0")
    if published != expected:
        violations.append(
            f"job `{CRITICAL_PUBLISH_JOB}` publishes {sorted(published - expected) or 'no extra archive'} "
            f"beyond the critical set and misses {sorted(expected - published) or 'none of it'}; "
            "it must publish exactly the Linux and macOS arm64 archives"
        )

    for job in names:
        for step in _steps_of(text, job):
            if GH_RELEASE_ACTION not in step.fields.get("uses", ""):
                continue
            files = step.nested("with").get("files", "")
            overwrite = step.nested("with").get("overwrite_files", "")
            manifest = "checksums.sha256" in files or "index.json" in files
            if manifest and job != ASSET_PUBLISH_JOB:
                violations.append(f"job `{job}` writes the release manifest; only `{ASSET_PUBLISH_JOB}` may")
            if overwrite != "false":
                violations.append(
                    f"step `{step.name}` uploads with overwrite_files `{overwrite or 'unset'}`; "
                    "release uploads are add-only (`overwrite_files: false`) and never replace an asset"
                )
            if manifest and "steps." not in (step.condition or ""):
                violations.append(
                    f"step `{step.name}` writes the manifest unconditionally; it must run only when "
                    "the manifest is absent from the release"
                )
    if "build_binaries_gate" not in closure(ASSET_PUBLISH_JOB):
        violations.append(f"job `{ASSET_PUBLISH_JOB}` writes the manifest before every lane finished")
    publisher = job_steps(job_block(text, ASSET_PUBLISH_JOB))
    guard = next((i for i, step in enumerate(publisher) if "cmp -s" in step.run and "gh release download" in step.run), None)
    upload = next(
        (
            i
            for i, step in enumerate(publisher)
            if GH_RELEASE_ACTION in step.fields.get("uses", "") and "checksums.sha256" in step.nested("with").get("files", "")
        ),
        None,
    )
    if guard is None or upload is None or guard > upload:
        violations.append(
            f"job `{ASSET_PUBLISH_JOB}` must refuse to change a published manifest before writing it"
        )

    for job in names:
        uses_homebrew = any("update-homebrew" in step.fields.get("uses", "") for step in _steps_of(text, job))
        if uses_homebrew and ASSET_PUBLISH_JOB not in closure(job):
            violations.append(f"job `{job}` updates Homebrew before the trailing publisher `{ASSET_PUBLISH_JOB}`")
    return violations

# Jobs allowed each elevated token scope (everything else is read-only).
WRITE_SCOPES_ALLOWED = {
    "contents": {"publish_critical_release_assets", "publish_github_release", "attach_semver_baseline"},
    "id-token": {*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB, "release_validate_buildbuddy_full"},
    "attestations": {*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB},
}


def _permission_scopes(lines: list[str], indent: int) -> dict[str, str]:
    """`scope: level` pairs of a permissions mapping at `indent`."""
    for index, line in enumerate(lines):
        if line == " " * indent + "permissions:":
            scopes: dict[str, str] = {}
            for candidate in lines[index + 1 :]:
                if candidate.strip() and _indent(candidate) <= indent:
                    break
                match = re.match(r"^\s+([a-z-]+):\s*([a-z]+)\s*$", candidate)
                if match:
                    scopes[match.group(1)] = match.group(2)
            return scopes
    return {}


def check_token_permissions(text: str) -> list[str]:
    """The workflow token is read-only except where a job needs more.

    The workflow default grants no write scope. Only the build jobs (and the
    full-graph caller, for buildbuddy.yml's OIDC) may mint OIDC tokens, only
    the build jobs may write attestations, and only the release publishers
    may write repository contents; every build checkout drops the token.
    """
    violations: list[str] = []
    lines = text.splitlines()
    top = _permission_scopes(lines[: lines.index("jobs:")], 0)
    for scope, level in sorted(top.items()):
        if level == "write":
            violations.append(f"the workflow default grants `{scope}: write` to every job")
    for job in _job_names(text):
        scopes = _permission_scopes(job_block(text, job), 4)
        for scope, level in sorted(scopes.items()):
            if level == "write" and job not in WRITE_SCOPES_ALLOWED.get(scope, set()):
                violations.append(f"job `{job}` is granted `{scope}: write`")
    for job in (*BINARY_BUILD_JOBS, WEB_SDK_BUILD_JOB):
        for step in _steps_of(text, job):
            inputs = step.nested("with")
            if CHECKOUT_ACTION in step.fields.get("uses", "") and inputs.get("persist-credentials") != "false":
                violations.append(f"job `{job}` step `{step.name}` persists the workflow token in its checkout")
    return violations

# Jobs that hold a registry or publish token (or write the GitHub release).
TOKEN_JOBS = (
    "registry_credentials_preflight",
    REGISTRY_JOB,
    WEB_SDK_PUBLISH_JOB,
    CRITICAL_PUBLISH_JOB,
    ASSET_PUBLISH_JOB,
    "update_homebrew",
    "attach_semver_baseline",
)
CACHE_ACTIONS = ("Swatinem/rust-cache", "actions/cache")


def check_release_caches(text: str) -> list[str]:
    """No release cache can be poisoned by a dispatch or reach a token job.

    A dispatch runs on main and saves into the default branch's cache scope,
    which every tag run restores from, so no release cache is saved by a
    dispatch; ~/.cargo/bin is never cached, so a restored binary cannot
    shadow a tool on PATH; and the jobs holding registry or publish tokens
    restore no cache at all.
    """
    violations: list[str] = []
    dispatch = EventContext(label="a dispatch", event_name="workflow_dispatch", ref="refs/heads/main")
    for job in _job_names(text):
        for step in _steps_of(text, job):
            uses = step.fields.get("uses", "")
            if not any(action in uses for action in CACHE_ACTIONS):
                continue
            if job in TOKEN_JOBS:
                violations.append(f"job `{job}` holds a registry or publish token and restores a cache (`{uses}`)")
                continue
            inputs = step.nested("with")
            if "rust-cache" in uses:
                if inputs.get("cache-bin") != "false":
                    violations.append(f"job `{job}` caches ~/.cargo/bin (`cache-bin` is not false)")
                save_if = inputs.get("save-if")
                if save_if is None or truthy(evaluate(save_if, dispatch)):
                    violations.append(f"job `{job}` saves its rust-cache on {dispatch.label}")
            else:
                violations.append(f"job `{job}` uses `{uses}`, which saves from any run")
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
    "semver-baseline": check_semver_baseline,
    "trailing-platforms": check_trailing_platforms,
    "token-permissions": check_token_permissions,
    "release-caches": check_release_caches,
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
