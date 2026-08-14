#!/usr/bin/env python3
"""Detect internally inconsistent Cargo.lock content.

A textual git merge can resolve a shared `[[package]]` block to one version
while a branch-only crate keeps referencing the version it was locked against.
Nothing conflicts, so no marker appears, and every cargo mode except `--locked`
silently reresolves the lock instead of complaining. The failure then surfaces
at registry publish, which is the most expensive place to learn it.

This checker reads the lock as data and names the exact offending reference.
`cargo metadata --locked` is the authority on whether the lock needs updating;
this is the diagnostic that says which line to look at.
"""

from __future__ import annotations

import pathlib
import sys
from collections import defaultdict

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised only on Python < 3.11
    import tomli as tomllib


def dangling_reference_errors(lock: dict) -> list[str]:
    versions_by_name: dict[str, set[str]] = defaultdict(set)
    for package in lock.get("package", []):
        name = package.get("name")
        version = package.get("version")
        if isinstance(name, str) and isinstance(version, str):
            versions_by_name[name].add(version)

    errors: list[str] = []
    for package in lock.get("package", []):
        owner = f"{package.get('name', '<unnamed>')} {package.get('version', '<unversioned>')}"
        for dependency in package.get("dependencies", []):
            if not isinstance(dependency, str):
                errors.append(f"{owner}: non-string dependency entry {dependency!r}")
                continue
            fields = dependency.split(" ")
            name = fields[0]
            known = versions_by_name.get(name)
            if not known:
                errors.append(
                    f"{owner}: dependency `{dependency}` has no [[package]] block named `{name}`"
                )
                continue
            if len(fields) == 1:
                if len(known) > 1:
                    errors.append(
                        f"{owner}: dependency `{dependency}` is unqualified while "
                        f"{name} is locked at {len(known)} versions "
                        f"({', '.join(sorted(known))})"
                    )
                continue
            version = fields[1]
            if version not in known:
                errors.append(
                    f"{owner}: dependency `{dependency}` references a version no "
                    f"[[package]] block provides (locked: {', '.join(sorted(known))})"
                )

    return errors


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: check_cargo_lock_consistency.py CARGO_LOCK", file=sys.stderr)
        return 2

    lock_path = pathlib.Path(sys.argv[1])
    try:
        lock = tomllib.loads(lock_path.read_text())
    except FileNotFoundError:
        print(f"{lock_path}: lock file is missing", file=sys.stderr)
        return 1
    except tomllib.TOMLDecodeError as error:
        print(f"{lock_path}: lock file is not valid TOML: {error}", file=sys.stderr)
        return 1

    errors = dangling_reference_errors(lock)
    if errors:
        print(f"{lock_path} is internally inconsistent:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    package_count = len(lock.get("package", []))
    print(f"{lock_path}: {package_count} locked packages, no dangling references")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
