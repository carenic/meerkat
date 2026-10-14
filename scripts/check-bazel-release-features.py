#!/usr/bin/env python3
"""Fail when a non-shipping (test-only) Cargo feature can reach a release binary
that Bazel builds.

The release roots are the targets every `release-build-*` case of
scripts/buildbuddy-bazel-poc builds; they must match the binaries
.github/workflows/release.yml packages from the BuildBuddy outputs.

(a) Every Bazel build of the release roots with -c opt, every
    `release-build-*` case of the launcher, passes
    --define=meerkat_release_build=1, which //tools/bazel:release_build
    matches; no workflow, Make target or script builds with -c opt any other
    way.
(b) Evaluated under that define, no target in the roots' dependency closure
    enables a feature listed in scripts/non-shipping-features.json or
    forwards one (`dep/feature`, `dep?/feature`), and none is a
    *_test_support test variant. A select keyed on //tools/bazel:release_build
    contributes every branch but its default (a more specific key could
    match beside the release one); any other select contributes every branch,
    since a platform or configuration key may match on some release target. A
    select the check cannot evaluate fails it.

Usage: check-bazel-release-features.py [repo-root]
Exit 0 = clean, 1 = violations, 2 = the inputs cannot be read.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

LAUNCHER = "scripts/buildbuddy-bazel-poc"
RELEASE_WORKFLOW = ".github/workflows/release.yml"
FEATURES_FILE = "scripts/non-shipping-features.json"
SETTING_PACKAGE = "tools/bazel"
SETTING_LABEL = "//tools/bazel:release_build"
DEFINE_NAME = "meerkat_release_build"
DEFINE_FLAG = f"--define={DEFINE_NAME}=1"
RUST_RULES = ("rust_library", "rust_binary", "rust_proc_macro", "rust_shared_library", "rust_static_library")
TEST_VARIANT_SUFFIXES = ("_test_support", "_with_runtime_test_support")
# Paths whose text names the opt flags without running a build: this guard and
# its contract test.
SELF_PATHS = {
    "scripts/check-bazel-release-features.py",
    "scripts/test_check_bazel_release_features.py",
}
OPT_FLAG = re.compile(r"(?:--compilation_mode[= ]opt\b|(?<![\w-])-c[= ]opt\b)")
SKIP_DIRS = {".git", "target", "node_modules", "bazel-out"}

RULE = re.compile(r"^([a-z_]+)\(\n(.*?)\n\)$", re.S | re.M)
ATTR = re.compile(r"^ {4}([a-z_]+) = ", re.M)
SELECT = re.compile(r"select\(\{(.*?)\}\)", re.S)
BRANCH = re.compile(r'"([^"]+)":\s*(\[[^\]]*\])', re.S)
BRANCH_KEY = re.compile(r'"([^"]+)"\s*:')
QUOTED = re.compile(r'"([^"]*)"')
LIST_EXPR = re.compile(r"^\[[^\[\]]*\](?:\s*\+\s*\[[^\[\]]*\])*$", re.S)
DEFAULT_CONDITION = "//conditions:default"


class GuardError(Exception):
    pass


def read(root: pathlib.Path, rel: str) -> str:
    try:
        return (root / rel).read_text(encoding="utf-8")
    except OSError as error:
        raise GuardError(f"cannot read {rel}: {error}") from error


def non_shipping_features(root: pathlib.Path) -> set[str]:
    try:
        features = json.loads(read(root, FEATURES_FILE))["features"]
    except (KeyError, TypeError, ValueError) as error:
        raise GuardError(f"{FEATURES_FILE} has no feature list: {error}") from error
    if not features or not all(isinstance(feature, str) and feature for feature in features):
        raise GuardError(f"{FEATURES_FILE} must list at least one feature name")
    return set(features)


def is_non_shipping(feature: str, features: set[str]) -> bool:
    return feature.rpartition("/")[2] in features


def release_alternatives(select_body: str, where: str) -> str:
    """Every branch a release build can take from one select(), as one list.

    Under the release define, a select keyed on //tools/bazel:release_build
    takes that branch or a more specific key that also matches, never its
    default; a select without that key takes whichever platform or
    configuration key matches on the target being built, or its default. The
    check keeps every alternative it cannot rule out, and refuses a branch
    that is not a literal list.
    """
    keys = BRANCH_KEY.findall(select_body)
    branches = BRANCH.findall(select_body)
    if not branches or [key for key, _ in branches] != keys:
        raise GuardError(f"{where}: cannot evaluate a select() whose branches are not all literal lists")
    if SETTING_LABEL in keys:
        branches = [(key, text) for key, text in branches if key != DEFAULT_CONDITION]
    items = [item for _, text in branches for item in QUOTED.findall(text)]
    return "[" + ", ".join(f'"{item}"' for item in items) + "]"


def evaluate(expr: str, where: str) -> str:
    """Replace every select() in an attribute value with its release alternatives."""
    if "select(" in expr and expr.count("select(") != len(SELECT.findall(expr)):
        raise GuardError(f"{where}: cannot evaluate a nested or malformed select()")
    return SELECT.sub(lambda match: release_alternatives(match.group(1), where), expr)


def attributes(body: str) -> dict[str, str]:
    marks = list(ATTR.finditer(body))
    values = {}
    for index, mark in enumerate(marks):
        end = marks[index + 1].start() if index + 1 < len(marks) else len(body)
        values[mark.group(1)] = body[mark.end():end].rstrip().rstrip(",")
    return values


def string_list(expr: str, where: str) -> list[str]:
    """A literal string list, or a sum of literal string lists."""
    expr = expr.strip()
    if not LIST_EXPR.match(expr):
        raise GuardError(f"{where}: cannot evaluate {expr[:60]!r} as a string list")
    return QUOTED.findall(expr)


def labels_in(expr: str, package: str) -> list[str]:
    labels = []
    for value in QUOTED.findall(expr):
        if value.startswith(":"):
            labels.append(f"//{package}{value}")
        elif value.startswith("//"):
            labels.append(value if ":" in value else f"{value}:{value.rsplit('/', 1)[-1]}")
    return labels


def build_files(root: pathlib.Path):
    for path in sorted(root.rglob("BUILD.bazel")):
        parts = path.relative_to(root).parts
        if any(part in SKIP_DIRS or part.startswith("bazel-") for part in parts[:-1]):
            continue
        yield path


def load_rules(root: pathlib.Path) -> dict[str, tuple[str, str, dict[str, str]]]:
    rules = {}
    for path in build_files(root):
        package = path.parent.relative_to(root).as_posix()
        package = "" if package == "." else package
        for match in RULE.finditer(path.read_text(encoding="utf-8")):
            kind, body = match.group(1), match.group(2)
            attrs = attributes(body)
            name = QUOTED.findall(attrs.get("name", ""))
            if not name:
                continue
            rules[f"//{package}:{name[0]}"] = (kind, package, attrs)
    return rules


def check_setting(rules) -> list[str]:
    """The selects' release branch applies exactly when the launcher's define is set."""
    entry = rules.get(SETTING_LABEL)
    if entry is None or entry[0] != "config_setting":
        return [f"{SETTING_LABEL} is not a config_setting in {SETTING_PACKAGE}/BUILD.bazel"]
    attrs = entry[2]
    raw = attrs.get("define_values", "")
    pairs = dict(re.findall(r'"([^"]+)":\s*"([^"]*)"', raw))
    extra = sorted(set(attrs) - {"name", "define_values", "visibility"})
    if pairs != {DEFINE_NAME: "1"} or extra:
        return [
            f"{SETTING_LABEL} must match exactly {DEFINE_FLAG} and nothing else; "
            f"found define_values {raw.strip()!r}, other conditions {extra}"
        ]
    return []


def launcher_cases(text: str) -> dict[str, str]:
    cases = {}
    for match in re.finditer(r"^  ([A-Za-z0-9_-]+)\)\n(.*?)^    ;;$", text, re.S | re.M):
        cases[match.group(1)] = match.group(2)
    return cases


def check_launcher(root: pathlib.Path) -> tuple[list[str], set[str], set[str]]:
    errors = []
    cases = launcher_cases(read(root, LAUNCHER))
    release_cases = {name for name in cases if name.startswith("release-build-")}
    if not release_cases:
        errors.append(f"{LAUNCHER} defines no release-build-* case")
    roots: set[str] | None = None
    for name, body in sorted(cases.items()):
        builds_opt = bool(OPT_FLAG.search(body))
        if name in release_cases and not builds_opt:
            errors.append(f"{LAUNCHER} {name} does not build with --compilation_mode=opt")
        if (builds_opt or name in release_cases) and DEFINE_FLAG not in body:
            errors.append(f"{LAUNCHER} {name} builds release binaries without {DEFINE_FLAG}")
        if name in release_cases:
            targets = re.search(r'default_target="([^"]*)"', body)
            labels = set(targets.group(1).split()) if targets else set()
            if not labels:
                errors.append(f"{LAUNCHER} {name} names no default_target")
            elif roots is None:
                roots = labels
            elif labels != roots:
                errors.append(f"{LAUNCHER} {name} builds {sorted(labels)}, other release cases {sorted(roots)}")
    return errors, roots or set(), release_cases


def check_workflow(root: pathlib.Path, roots: set[str], release_cases: set[str]) -> list[str]:
    errors = []
    text = read(root, RELEASE_WORKFLOW)
    for command in sorted(set(re.findall(r"bb_command:\s*([A-Za-z0-9_-]+)", text))):
        if command not in release_cases:
            errors.append(f"{RELEASE_WORKFLOW} builds release binaries with {command}, not a checked release-build-* case")
    packaged = set()
    for block in re.findall(r"declare -A BIN_PATHS=\((.*?)\)", text, re.S):
        for _, path in re.findall(r'\[([^\]]+)\]="([^"]+)"', block):
            package, _, name = path.rpartition("/")
            packaged.add(f"//{package}:{name}")
    if not packaged:
        errors.append(f"{RELEASE_WORKFLOW} packages no BuildBuddy BIN_PATHS")
    elif packaged != roots:
        errors.append(
            f"{RELEASE_WORKFLOW} packages {sorted(packaged)} but {LAUNCHER} release-build cases build {sorted(roots)}"
        )
    return errors


def check_other_opt_builds(root: pathlib.Path) -> list[str]:
    errors = []
    candidates = [root / "Makefile", root / ".bazelrc"]
    for directory in ("scripts", ".github", "tools"):
        base = root / directory
        if base.is_dir():
            candidates.extend(path for path in sorted(base.rglob("*")) if path.is_file())
    for path in candidates:
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if rel == LAUNCHER or rel in SELF_PATHS or any(part in SKIP_DIRS for part in path.parts):
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if OPT_FLAG.search(line) and DEFINE_FLAG not in line:
                errors.append(f"{rel}:{number} builds with -c opt outside the {LAUNCHER} release-build cases: {line.strip()}")
    return errors


def check_closure(rules, roots: set[str], features: set[str]) -> tuple[list[str], int, int]:
    errors = []
    seen: set[str] = set()
    selects = 0
    stack = sorted(roots)
    while stack:
        label = stack.pop()
        if label in seen:
            continue
        seen.add(label)
        entry = rules.get(label)
        if entry is None:
            errors.append(f"{label} is in the release closure but no BUILD.bazel defines it")
            continue
        kind, package, attrs = entry
        if kind == "alias":
            stack.extend(labels_in(evaluate(attrs.get("actual", ""), label), package))
            continue
        if kind not in RUST_RULES:
            errors.append(f"{label} is a {kind}, not a Rust library or binary, in the release closure")
            continue
        if label.endswith(TEST_VARIANT_SUFFIXES):
            errors.append(f"{label} is a test-support variant in the release closure")
        expr = attrs.get("crate_features", "[]")
        selects += expr.count("select(")
        for feature in string_list(evaluate(expr, label), label):
            if is_non_shipping(feature, features):
                errors.append(f"{label} enables non-shipping feature {feature!r} in the release configuration")
        for attr in ("deps", "proc_macro_deps"):
            stack.extend(labels_in(evaluate(attrs.get(attr, "[]"), label), package))
    return errors, len(seen), selects


def main() -> int:
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    try:
        features = non_shipping_features(root)
        rules = load_rules(root)
        errors = check_setting(rules)
        launcher_errors, roots, release_cases = check_launcher(root)
        errors += launcher_errors
        errors += check_workflow(root, roots, release_cases)
        errors += check_other_opt_builds(root)
        closure_errors, walked, selects = check_closure(rules, roots, features)
        errors += closure_errors
    except GuardError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    if errors:
        print("Release binaries can compile test-only code:", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1
    print(
        f"Release closure of {', '.join(sorted(roots))}: {walked} targets, "
        f"{selects} release selects evaluated, no non-shipping feature"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
