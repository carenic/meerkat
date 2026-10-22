#!/usr/bin/env python3
"""Published crates' tests pass standalone.

A published crate's tests are the tests a downloader runs: plain `cargo test`
inside the unpacked package, with no sibling checkout, no workspace feature
unification and no repository files. This script proves that for every crate
scripts/release-rust-crates.sh lists.

  published_crates_standalone.py census --out DIR [--crate NAME ...]
      [--stack mirror|default] [--allow-dirty]

      Package every selected crate, unpack the archives into a synthetic
      workspace whose [patch.crates-io] points each crate at its unpacked copy,
      and then, one crate at a time (so sibling crates cannot turn features
      on):
        - `cargo test -p C --no-run` with default features (the command a
          downloader types must build);
        - `cargo test -p C --features <declared>` passes, where <declared> is
          the crate's [package.metadata.meerkat.standalone-tests] features;
        - test-name parity: per test target, the standalone test names equal
          the workspace's names for the same package and features, minus the
          workspace-only tests ci/standalone-workspace-only.toml declares.
      --stack mirror (default) sets RUST_MIN_STACK to the workspace's
      .cargo/config.toml value; --stack default leaves Rust's own default, to
      record which targets need the larger stack.

  published_crates_standalone.py guard [--crate NAME ...]

      Static check, no build. For every selected crate:
        - its [package] exclude and the manifest's `excluded` list agree;
        - no packaged Rust file reads outside the package: no include_* or
          #[path] that leaves the package root, no CARGO_MANIFEST_DIR walked
          up with parent() or ".." (inline, or through a same-file string
          constant), no workspace_root/repo_root helper;
        - no packaged test skips itself when a binary or artifact is missing;
        - code under a cfg that needs `workspace-tests` is not scanned: the
          package leaves that feature off, so the code never builds there;
        - the declared test features exist and leave `workspace-tests` off,
          a crate declaring `workspace-tests` has the path-only self
          dev-dependency that enables it and its Bazel unit-test target
          turns it on, and the packaged README.md prints the test command.
      A flagged line that is not a test escape is listed under the crate's
      `guard_allow` with its reason.

Exit 0 = proven, 1 = a crate fails, 2 = usage error or unreadable input.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised only on Python < 3.11
    import tomli as tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = pathlib.Path("ci") / "standalone-workspace-only.toml"
# .cargo/config.toml [env] of the workspace; mirrored, never read from the
# package (a downloader has no repository config).
MIRRORED_MIN_STACK = "8388608"
DOC_TARGET = "doc"


class UsageError(Exception):
    pass


# ---------------------------------------------------------------------------
# Inputs
# ---------------------------------------------------------------------------


def release_crates(root: pathlib.Path) -> list[str]:
    script = root / "scripts" / "release-rust-crates.sh"
    out = subprocess.run([str(script)], check=True, capture_output=True, text=True).stdout
    return [line.strip() for line in out.splitlines() if line.strip()]


def crate_dirs(root: pathlib.Path) -> dict[str, pathlib.Path]:
    out = subprocess.run(
        [cargo(), "metadata", "--no-deps", "--format-version", "1"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    meta = json.loads(out)
    return {pkg["name"]: pathlib.Path(pkg["manifest_path"]).parent for pkg in meta["packages"]}


def load_manifest(root: pathlib.Path) -> dict:
    path = root / MANIFEST
    try:
        data = tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as err:
        raise UsageError(f"cannot read {MANIFEST}: {err}") from err
    crates = data.get("crates") or {}
    for name, entry in crates.items():
        for key in ("excluded", "workspace_only", "guard_allow"):
            for item in entry.get(key) or []:
                if not str(item.get("reason", "")).strip():
                    raise UsageError(f"{MANIFEST}: {name}.{key} entry {item} has no reason")
        for item in entry.get("workspace_only") or []:
            names = [*(item.get("modules") or []), *(item.get("tests") or [])]
            if not item.get("target") or not names or not all(isinstance(n, str) and n for n in names):
                raise UsageError(
                    f"{MANIFEST}: {name}.workspace_only entry for {item.get('target')!r} needs a target "
                    "and at least one module or test name"
                )
    return crates


def standalone_tests(manifest: dict) -> dict:
    meta = ((manifest.get("package") or {}).get("metadata") or {}).get("meerkat") or {}
    return meta.get("standalone-tests") or {}


def standalone_features(manifest: dict) -> list[str]:
    return list(standalone_tests(manifest).get("features") or [])


def standalone_min_stack(manifest: dict) -> int | None:
    value = standalone_tests(manifest).get("min-stack")
    return int(value) if value is not None else None


WORKSPACE_TESTS = "workspace-tests"


def test_command(manifest: dict) -> str | None:
    """The command a consumer runs in the published package, as the crate's
    README.md prints it; None when plain `cargo test` is the whole story."""
    features = standalone_features(manifest)
    stack = standalone_min_stack(manifest)
    if not features and stack is None:
        return None
    command = "cargo test" + (f" --features {','.join(features)}" if features else "")
    return f"RUST_MIN_STACK={stack} {command}" if stack is not None else command


def published_test_contract_failures(crate: str, manifest: dict, package_root: pathlib.Path, packaged: set[str]) -> list[str]:
    """The published test contract: the declared feature set exists and never
    turns on `workspace-tests`; a crate that declares `workspace-tests` has the
    path-only self dev-dependency that enables it; and README.md, which the
    package carries, prints the exact test command."""
    failures = []
    features = (manifest.get("features") or {})
    for feature in standalone_features(manifest):
        if feature == WORKSPACE_TESTS:
            failures.append(f"{crate}: standalone-tests features must not include {WORKSPACE_TESTS}")
        elif feature not in features:
            failures.append(f"{crate}: standalone-tests feature {feature} is not declared in [features]")
    if WORKSPACE_TESTS in features:
        self_dep = (manifest.get("dev-dependencies") or {}).get(crate)
        if not (isinstance(self_dep, dict) and self_dep.get("path") == "." and "version" not in self_dep
                and WORKSPACE_TESTS in (self_dep.get("features") or [])):
            failures.append(
                f"{crate}: declares {WORKSPACE_TESTS} but no path-only self dev-dependency enables it"
            )
    command = test_command(manifest)
    if command is not None:
        readme = package_root / "README.md"
        if "README.md" not in packaged:
            failures.append(f"{crate}: README.md must ship with the package and print `{command}`")
        elif command not in readme.read_text():
            failures.append(f"{crate}: README.md must print the standalone test command `{command}`")
    return failures


def cargo() -> str:
    return os.environ.get("CARGO_BIN", "cargo")


def bazel_unit_test_features(build_text: str, target: str) -> list[str] | None:
    """The crate_features of the generated `rust_test` named `target` in a
    BUILD.bazel, or None when there is no such target."""
    rule = re.search(r'\nrust_test\(\n    name = "' + re.escape(target) + r'",\n(.*?)\n\)\n', build_text, re.S)
    if rule is None:
        return None
    features = re.search(r'\n    crate_features = \[\n(.*?)\n    \]', rule.group(1), re.S)
    return re.findall(r'"([^"]+)"', features.group(1)) if features else []


def bazel_workspace_tests_failures(crate: str, manifest: dict, package_root: pathlib.Path) -> list[str]:
    """A crate that declares `workspace-tests` gets it on its Bazel unit-test
    target (scripts/generate-bazel-rust-builds.mjs follows the self
    dev-dependency), or the Bazel lanes silently drop its workspace-only tests."""
    if WORKSPACE_TESTS not in (manifest.get("features") or {}):
        return []
    target = f"{crate.replace('-', '_')}_unit_test"
    try:
        build_text = (package_root / "BUILD.bazel").read_text()
    except OSError as err:
        return [f"{crate}: declares {WORKSPACE_TESTS} but its BUILD.bazel cannot be read: {err}"]
    features = bazel_unit_test_features(build_text, target)
    if features is None:
        return [f"{crate}: declares {WORKSPACE_TESTS} but BUILD.bazel has no {target}"]
    if WORKSPACE_TESTS not in features:
        return [f"{crate}: BUILD.bazel {target} does not enable {WORKSPACE_TESTS}; run make buildbuddy-generate"]
    return []


def toolchain(root: pathlib.Path) -> str | None:
    path = root / "rust-toolchain.toml"
    if not path.exists():
        return None
    return ((tomllib.loads(path.read_text()).get("toolchain") or {}).get("channel")) or None


# ---------------------------------------------------------------------------
# Guard (static)
# ---------------------------------------------------------------------------

INCLUDE_MACRO = re.compile(r'\b(include_str|include_bytes|include)!\s*\(\s*"([^"]+)"')
INCLUDE_FROM_MANIFEST = re.compile(
    r'\b(include_str|include_bytes|include)!\s*\(\s*concat!\s*\(\s*env!\s*\(\s*"CARGO_MANIFEST_DIR"\s*\)\s*,\s*"([^"]+)"'
)
PATH_ATTR = re.compile(r'#\[\s*path\s*=\s*"([^"]+)"\s*\]')
MANIFEST_DIR = re.compile(r'CARGO_MANIFEST_DIR')
# A same-file string constant: a statement that joins CARGO_MANIFEST_DIR with
# the constant's name walks wherever the constant's literal does.
STR_CONST = re.compile(r'\b(?:const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*&\s*(?:\'static\s+)?str\s*=\s*"([^"\\]*)"\s*;')
ROOT_HELPER = re.compile(r'\bfn\s+(workspace_root|repo_root|repository_root)\s*\(')
SKIP_MISSING = re.compile(r'"Skipping[^"]*\b(missing|not found|not built)\b', re.IGNORECASE)
STATEMENT_WINDOW = 400


def packaged_files(root: pathlib.Path, crate: str) -> list[str]:
    proc = subprocess.run(
        [cargo(), "package", "--list", "-p", crate, "--allow-dirty"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise UsageError(f"cargo package --list -p {crate} failed:\n{proc.stderr.strip()}")
    return [line.strip() for line in proc.stdout.splitlines() if line.strip()]


def strip_comments(text: str) -> str:
    """Blank Rust comments (keeping newlines, so line numbers hold) and keep
    string literals, so a pattern quoted in documentation is not a finding."""
    out: list[str] = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            out.append(" " * (end - i))
            i = end
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append("".join(ch if ch == "\n" else " " for ch in text[i:j]))
            i = j
        elif (c == "r" or text.startswith("br", i)) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + (2 if text.startswith("br", i) else 1)
            hashes = 0
            while j < n and text[j] == "#":
                hashes, j = hashes + 1, j + 1
            if j < n and text[j] == '"':
                close = '"' + "#" * hashes
                end = text.find(close, j + 1)
                end = n if end < 0 else end + len(close)
                out.append(text[i:end])
                i = end
            else:
                out.append(c)
                i += 1
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i : j + 1])
            i = j + 1
        elif c == "'" and i + 2 < n and (text[i + 2] == "'" or text[i + 1] == "\\"):
            end = text.find("'", i + 2)
            end = n if end < 0 else end + 1
            out.append(text[i:end])
            i = end
        else:
            out.append(c)
            i += 1
    return "".join(out)


CFG_ATTR = re.compile(r"#\s*(!?)\s*\[\s*cfg\s*\(")
CFG_TOKEN = re.compile(r'\s*(?:([A-Za-z_][A-Za-z0-9_]*)|("(?:[^"\\]|\\.)*")|([(),=]))')
ATTR_OPEN = re.compile(r"\s*#\s*!?\s*\[")
ITEM_START = re.compile(
    r"(?:pub\s*(?:\([^)]*\))?\s+)?(?:(?:async|unsafe|const|extern(?:\s+\"[^\"]*\")?)\s+)*"
    r"(?:(?:fn|mod|impl|struct|enum|union|trait|use|type|static|const|crate)\b|macro_rules\s*!)"
)


def cfg_requires(predicate: str, feature: str) -> bool:
    """Whether a `cfg(...)` predicate holds only with `feature` on: the feature
    itself, an all() with such a member, an any() whose members all need it.
    not(), other names and anything unparsed do not need it."""
    tokens: list[str] = []
    pos = 0
    while predicate[pos:].strip():
        match = CFG_TOKEN.match(predicate, pos)
        if not match:
            return False
        tokens.append(match.group(1) or match.group(2) or match.group(3))
        pos = match.end()

    def parse(i: int) -> tuple[bool, int]:
        name = tokens[i]
        if not (name[0].isalpha() or name[0] == "_"):
            raise ValueError(name)
        i += 1
        if i < len(tokens) and tokens[i] == "=":
            value = tokens[i + 1]
            if not value.startswith('"'):
                raise ValueError(value)
            return name == "feature" and value[1:-1] == feature, i + 2
        if i < len(tokens) and tokens[i] == "(":
            i += 1
            members: list[bool] = []
            while tokens[i] != ")":
                needs, i = parse(i)
                members.append(needs)
                if tokens[i] == ",":
                    i += 1
            if name == "all":
                return any(members), i + 1
            if name == "any":
                return bool(members) and all(members), i + 1
            return False, i + 1
        return False, i

    try:
        needs, end = parse(0)
    except (ValueError, IndexError):
        return False
    return needs and end == len(tokens)


def skip_literal(text: str, i: int) -> int | None:
    """The index after the string, raw string or char literal that starts at
    `i`, or None when none starts there (a lifetime is not a literal)."""
    n, c = len(text), text[i]
    if c in "rb" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
        j = i + 2 if text.startswith("br", i) else i + 1
        if text[j - 1] == "r":
            hashes = 0
            while j < n and text[j] == "#":
                hashes, j = hashes + 1, j + 1
            if j < n and text[j] == '"':
                end = text.find('"' + "#" * hashes, j + 1)
                return n if end < 0 else end + 1 + hashes
            return None
        return skip_literal(text, j) if j < n and text[j] in "\"'" else None
    if c == '"':
        j = i + 1
        while j < n and text[j] != '"':
            j += 2 if text[j] == "\\" else 1
        return min(j + 1, n)
    if c == "'" and i + 2 < n and text[i + 1] == "\\":
        end = text.find("'", i + 3)
        return n if end < 0 else end + 1
    if c == "'" and i + 2 < n and text[i + 2] == "'":
        return i + 3
    return None


def matching_close(text: str, i: int) -> int | None:
    """The index of the bracket that closes the one at `i`, skipping literals."""
    depth, n = 0, len(text)
    while i < n:
        end = skip_literal(text, i)
        if end is not None:
            i = end
            continue
        if text[i] in "([{":
            depth += 1
        elif text[i] in ")]}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return None


def skip_attributes(text: str, i: int) -> int:
    """The index of the first code after the whitespace and attributes at `i`."""
    while match := ATTR_OPEN.match(text, i):
        close = matching_close(text, match.end() - 1)
        if close is None:
            return len(text)
        i = close + 1
    while i < len(text) and text[i].isspace():
        i += 1
    return i


def item_end(text: str, i: int) -> int | None:
    """The index after the item whose attributes end at `i`: its block or its
    `;`. None when the attributes carry a statement, an expression or anything
    else that is not an item, or when the item does not close."""
    i = skip_attributes(text, i)
    if not ITEM_START.match(text, i):
        return None
    depth, n = 0, len(text)
    while i < n:
        end = skip_literal(text, i)
        if end is not None:
            i = end
            continue
        c = text[i]
        if c == "{" and depth == 0:
            close = matching_close(text, i)
            return None if close is None else close + 1
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth < 0:
                return None
        elif c == ";" and depth == 0:
            return i + 1
        i += 1
    return None


def blank_compiled_out(text: str, feature: str = WORKSPACE_TESTS) -> str:
    """Blank (keeping newlines, so line numbers hold) the code a cfg that needs
    `feature` compiles out: the whole file when its leading inner attribute
    needs it, and each item (fn, mod, impl, use, ...) whose outer attribute
    needs it. The package never turns `workspace-tests` on, so that code never
    builds there. A cfg on a statement or an expression keeps its code, so a
    misread extent can only keep code in the scan, never hide it."""
    out = list(text)
    pos = 0
    while match := CFG_ATTR.search(text, pos):
        close = matching_close(text, match.end() - 1)
        attr = re.compile(r"\s*\]").match(text, close + 1) if close is not None else None
        if attr is None:
            pos = match.end()
            continue
        pos = attr.end()
        if not cfg_requires(text[match.end() : close], feature):
            continue
        if match.group(1):
            if skip_attributes(text, 0) >= pos:
                return "".join(ch if ch == "\n" else " " for ch in text)
            continue
        end = item_end(text, pos)
        if end is None:
            continue
        for k in range(match.start(), end):
            if out[k] != "\n":
                out[k] = " "
        pos = end
    return "".join(out)


def ships(packaged: set[str], base: str, literal: str) -> bool:
    """Whether a path a source file names (relative to `base`, a package-relative
    directory) is a file the package ships. Normalised lexically: cargo package
    copies a symlink's target, so the link's place in the package is what counts."""
    joined = os.path.normpath(os.path.join(base, literal))
    return not joined.startswith("..") and joined in packaged


def scan_source(packaged: set[str], relative: str, text: str) -> list[tuple[str, int, str]]:
    """Return (kind, line, excerpt) for every guard finding in one packaged
    file, given with its comments stripped. Code the package compiles out is
    not scanned (see blank_compiled_out)."""
    text = blank_compiled_out(text)
    findings: list[tuple[str, int, str]] = []
    base = os.path.dirname(relative)
    consts = {match.group(1): match.group(2) for match in STR_CONST.finditer(text)}

    def line_of(offset: int) -> int:
        return text.count("\n", 0, offset) + 1

    for match in INCLUDE_MACRO.finditer(text):
        if not ships(packaged, base, match.group(2)):
            findings.append(("include-escape", line_of(match.start()), match.group(0)))
    for match in INCLUDE_FROM_MANIFEST.finditer(text):
        if not ships(packaged, "", match.group(2).lstrip("/")):
            findings.append(("include-escape", line_of(match.start()), match.group(0)))
    for match in PATH_ATTR.finditer(text):
        if not ships(packaged, base, match.group(1)):
            findings.append(("path-escape", line_of(match.start()), match.group(0)))
    for match in MANIFEST_DIR.finditer(text):
        window = text[match.start() : match.start() + STATEMENT_WINDOW]
        statement = window.split(";", 1)[0]
        named = [literal for name, literal in consts.items() if re.search(rf"\b{re.escape(name)}\b", statement)]
        if (".parent()" in statement or '".."' in statement or '"../' in statement
                or any(literal == ".." or literal.startswith("../") or "/../" in literal for literal in named)):
            findings.append(("manifest-dir-walk", line_of(match.start()), statement.strip()[:120]))
    for match in ROOT_HELPER.finditer(text):
        findings.append(("root-helper", line_of(match.start()), match.group(0)))
    for match in SKIP_MISSING.finditer(text):
        findings.append(("skip-when-missing", line_of(match.start()), match.group(0)))
    return findings


def guard(root: pathlib.Path, crates: list[str]) -> int:
    manifest = load_manifest(root)
    dirs = crate_dirs(root)
    failures: list[str] = []
    for crate in crates:
        entry = manifest.get(crate) or {}
        package_root = dirs[crate]
        cargo_manifest = tomllib.loads((package_root / "Cargo.toml").read_text())
        excludes = set((cargo_manifest.get("package") or {}).get("exclude") or [])
        declared = {item["path"] for item in entry.get("excluded") or []}
        for path in sorted(excludes - declared):
            failures.append(f"{crate}: [package] exclude {path} is not in {MANIFEST} excluded")
        for path in sorted(declared - excludes):
            failures.append(f"{crate}: {MANIFEST} excluded {path} is not in [package] exclude")
        for path in sorted(declared):
            if not (package_root / path).exists():
                failures.append(f"{crate}: excluded {path} does not exist")
        allowed = {(item["path"], item["kind"]) for item in entry.get("guard_allow") or []}
        used: set[tuple[str, str]] = set()
        packaged = set(packaged_files(root, crate))
        for relative in sorted(packaged):
            if not relative.endswith(".rs"):
                continue
            text = strip_comments((package_root / relative).read_text(errors="replace"))
            for kind, line, excerpt in scan_source(packaged, relative, text):
                if (relative, kind) in allowed:
                    used.add((relative, kind))
                    continue
                failures.append(f"{crate}: {relative}:{line}: {kind}: {excerpt}")
        for path, kind in sorted(allowed - used):
            failures.append(f"{crate}: guard_allow {path} {kind} matches nothing (stale entry)")
        failures.extend(published_test_contract_failures(crate, cargo_manifest, package_root, packaged))
        failures.extend(bazel_workspace_tests_failures(crate, cargo_manifest, package_root))
        print(f"  {crate:<36}{'FAIL' if any(f.startswith(crate + ':') for f in failures) else 'OK'}")
    for failure in failures:
        print(f"FAIL {failure}", file=sys.stderr)
    return 1 if failures else 0


# ---------------------------------------------------------------------------
# Census (build and run)
# ---------------------------------------------------------------------------

RUNNING = re.compile(r"^\s+Running (?:unittests )?(\S+) \(")
DOC_TESTS = re.compile(r"^\s+Doc-tests (\S+)")
LISTED = re.compile(r"^(.+): test$")


def parse_listing(output: str, package_dir: str = "") -> dict[str, set[str]]:
    """Map test target (source path, or "doc") to its test names.

    rustdoc names a doc test by its source path relative to the workspace
    root, which differs between the repository (`crates/<dir>/src/..`) and the
    unpacked package (`<name>-<version>/src/..`); `package_dir` is that prefix,
    stripped so both name the same test the same way."""
    names: dict[str, set[str]] = {}
    current: str | None = None
    prefix = package_dir.rstrip("/") + "/" if package_dir else ""
    for line in output.splitlines():
        running = RUNNING.match(line)
        if running:
            current = running.group(1)
            names.setdefault(current, set())
            continue
        if DOC_TESTS.match(line):
            current = DOC_TARGET
            names.setdefault(current, set())
            continue
        listed = LISTED.match(line)
        if listed and current is not None:
            name = listed.group(1)
            if current == DOC_TARGET and prefix and name.startswith(prefix):
                name = name[len(prefix) :]
            names[current].add(name)
    return names


def workspace_only_names(entry: dict, target: str, names: set[str]) -> set[str]:
    """The names in `target` the manifest declares workspace-only: each named
    test, and every test under each named module path."""
    out: set[str] = set()
    for item in entry.get("workspace_only") or []:
        if item.get("target") != target:
            continue
        out |= names & set(item.get("tests") or [])
        for module in item.get("modules") or []:
            prefix = module + "::"
            out |= {name for name in names if name.startswith(prefix)}
    return out


def stale_workspace_only(entry: dict, workspace: dict[str, set[str]]) -> list[str]:
    """Declared workspace-only names that no workspace test matches any more
    (renamed or removed): a stale declaration would hide nothing today but
    could hide a new test that takes the name."""
    problems: list[str] = []
    for item in entry.get("workspace_only") or []:
        target = item.get("target")
        names = workspace.get(target, set())
        for test in item.get("tests") or []:
            if test not in names:
                problems.append(f"{target}: declared workspace-only test {test} is not a workspace test")
        for module in item.get("modules") or []:
            if not any(name.startswith(module + "::") for name in names):
                problems.append(f"{target}: declared workspace-only module {module} holds no workspace test")
    return problems


def parity(entry: dict, workspace: dict[str, set[str]], standalone: dict[str, set[str]]) -> list[str]:
    """Differences between the standalone names and the workspace names the package keeps."""
    excluded_targets = {item["path"] for item in entry.get("excluded") or []}
    problems: list[str] = stale_workspace_only(entry, workspace)
    for target in sorted(set(workspace) | set(standalone)):
        expected = set() if target in excluded_targets else set(workspace.get(target, set()))
        expected -= workspace_only_names(entry, target, expected)
        got = standalone.get(target, set())
        for name in sorted(expected - got):
            problems.append(f"{target}: {name} runs in the workspace but not standalone")
        for name in sorted(got - expected):
            problems.append(f"{target}: {name} runs standalone but not in the workspace")
    return problems


def run(cmd: list[str], cwd: pathlib.Path, env: dict[str, str], log: pathlib.Path) -> tuple[int, str]:
    proc = subprocess.run(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log.write_text(f"$ {' '.join(cmd)}\n# cwd {cwd}\n{proc.stdout}\n# rc {proc.returncode}\n")
    return proc.returncode, proc.stdout


def build_env(stack: str, target_dir: pathlib.Path, rust_toolchain: str | None) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if k not in ("RUST_MIN_STACK", "CARGO_TARGET_DIR")}
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["CARGO_PROFILE_TEST_DEBUG"] = "0"
    env["CARGO_PROFILE_DEV_DEBUG"] = "0"
    env["CARGO_TERM_COLOR"] = "never"
    if rust_toolchain:
        env["RUSTUP_TOOLCHAIN"] = rust_toolchain
    if stack == "mirror":
        env["RUST_MIN_STACK"] = MIRRORED_MIN_STACK
    return env


def package_and_unpack(root: pathlib.Path, crates: list[str], all_crates: list[str], dirs: dict[str, pathlib.Path],
                       out: pathlib.Path, allow_dirty: bool, env: dict[str, str]) -> dict[str, pathlib.Path]:
    patch = out / "package-patch.toml"
    patch.write_text(
        "[patch.crates-io]\n"
        + "".join(f'{name} = {{ path = "{dirs[name]}" }}\n' for name in all_crates)
    )
    cmd = [cargo(), "package", "--no-verify", "--config", str(patch)]
    if allow_dirty:
        cmd.append("--allow-dirty")
    for name in crates:
        cmd += ["-p", name]
    rc, output = run(cmd, root, env, out / "package.log")
    if rc != 0:
        raise UsageError(f"cargo package failed (see {out / 'package.log'})")
    version = None
    package_dir = pathlib.Path(env["CARGO_TARGET_DIR"]) / "package"
    ws = out / "ws"
    ws.mkdir()
    unpacked: dict[str, pathlib.Path] = {}
    for name in crates:
        archives = sorted(package_dir.glob(f"{name}-[0-9]*.crate"))
        if len(archives) != 1:
            raise UsageError(f"expected one archive for {name} in {package_dir}, found {len(archives)}")
        with tarfile.open(archives[0], "r:gz") as bundle:
            bundle.extractall(ws, filter="data") if hasattr(tarfile, "data_filter") else bundle.extractall(ws)
        unpacked[name] = ws / archives[0].name[: -len(".crate")]
        version = version or archives[0].name[len(name) + 1 : -len(".crate")]
    members = "".join(f'    "{path.name}",\n' for path in unpacked.values())
    patches = "".join(f'{name} = {{ path = "{path.name}" }}\n' for name, path in unpacked.items())
    (ws / "Cargo.toml").write_text(
        f'[workspace]\nresolver = "2"\nmembers = [\n{members}]\n\n[patch.crates-io]\n{patches}'
    )
    shutil.copyfile(root / "Cargo.lock", ws / "Cargo.lock")
    return unpacked


def census(root: pathlib.Path, crates: list[str], out: pathlib.Path, stack: str, allow_dirty: bool) -> int:
    manifest = load_manifest(root)
    all_crates = release_crates(root)
    dirs = crate_dirs(root)
    rust_toolchain = toolchain(root)
    out.mkdir(parents=True, exist_ok=False)
    logs = out / "logs"
    logs.mkdir()
    standalone_env = build_env(stack, out / "target-standalone", rust_toolchain)
    workspace_env = build_env(stack, out / "target-workspace", rust_toolchain)
    # Every release crate is unpacked: the selected crates' dependencies must
    # resolve to the unpacked siblings, never to an older registry release.
    unpacked = package_and_unpack(root, all_crates, all_crates, dirs, out, allow_dirty, standalone_env)
    ws = out / "ws"
    rows: list[dict] = []
    failed = False
    for crate in crates:
        entry = manifest.get(crate) or {}
        packaged = tomllib.loads((unpacked[crate] / "Cargo.toml").read_text())
        features = standalone_features(packaged)
        feature_args = ["--features", ",".join(features)] if features else []
        row: dict = {"crate": crate, "features": features, "stack": stack, "problems": []}

        rc, _ = run([cargo(), "test", "-p", crate, "--no-run"], ws, standalone_env, logs / f"{crate}.naive.log")
        row["naive_build"] = rc == 0
        if rc != 0:
            row["problems"].append("default-feature `cargo test --no-run` does not build")

        rc, output = run([cargo(), "test", "-p", crate, *feature_args, "--no-fail-fast"], ws, standalone_env,
                         logs / f"{crate}.test.log")
        row["tests_pass"] = rc == 0
        row["results"] = re.findall(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", output)
        if rc != 0:
            row["problems"].append("`cargo test` with the declared features fails")

        rc_s, standalone_listing = run([cargo(), "test", "-p", crate, *feature_args, "--", "--list", "--format", "terse"],
                                       ws, standalone_env, logs / f"{crate}.list-standalone.log")
        rc_w, workspace_listing = run([cargo(), "test", "-p", crate, *feature_args, "--", "--list", "--format", "terse"],
                                      root, workspace_env, logs / f"{crate}.list-workspace.log")
        if rc_s != 0 or rc_w != 0:
            row["problems"].append(f"listing failed (standalone rc {rc_s}, workspace rc {rc_w})")
        else:
            standalone = parse_listing(standalone_listing, unpacked[crate].name)
            workspace = parse_listing(workspace_listing, str(dirs[crate].relative_to(root)))
            row["targets"] = {t: len(n) for t, n in sorted(standalone.items())}
            row["problems"] += parity(entry, workspace, standalone)
        failed |= bool(row["problems"])
        rows.append(row)
        print(f"  {crate:<36}{'FAIL' if row['problems'] else 'OK'}", flush=True)
        for problem in row["problems"]:
            print(f"      {problem}", flush=True)
    (out / "census.json").write_text(json.dumps(rows, indent=2) + "\n")
    return 1 if failed else 0


# ---------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    census_parser = sub.add_parser("census")
    census_parser.add_argument("--out", required=True, type=pathlib.Path)
    census_parser.add_argument("--crate", action="append", default=[])
    census_parser.add_argument("--stack", choices=("mirror", "default"), default="mirror")
    census_parser.add_argument("--allow-dirty", action="store_true")
    census_parser.add_argument("--shard", default="1/1", help="K/N: test every Nth crate starting at the Kth")
    guard_parser = sub.add_parser("guard")
    guard_parser.add_argument("--crate", action="append", default=[])
    args = parser.parse_args(argv)
    try:
        crates = release_crates(ROOT)
        if args.crate:
            unknown = sorted(set(args.crate) - set(crates))
            if unknown:
                raise UsageError(f"not release crates: {', '.join(unknown)}")
            crates = [name for name in crates if name in set(args.crate)]
        if args.command == "guard":
            return guard(ROOT, crates)
        match = re.fullmatch(r"([1-9][0-9]*)/([1-9][0-9]*)", args.shard)
        if not match or int(match.group(1)) > int(match.group(2)):
            raise UsageError(f"--shard must be K/N with 1 <= K <= N, got {args.shard}")
        k, n = int(match.group(1)), int(match.group(2))
        crates = crates[k - 1 :: n]
        return census(ROOT, crates, args.out, args.stack, args.allow_dirty)
    except UsageError as err:
        print(f"published_crates_standalone: {err}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
