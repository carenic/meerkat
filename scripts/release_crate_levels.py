#!/usr/bin/env python3
"""Group the release crates into dependency levels for parallel publishing.

The canonical publish order (scripts/release-rust-crates.sh, one crate per line
on stdin) stays the one hand-written enumeration. This derives, from that order
and `cargo metadata`, the levels a parallel publish walks: a level-0 crate has
no dependency among the release crates, and a level-N crate depends (normal or
build dependency, any target) on at least one level N-1 crate and only on
crates of lower levels. Every crate of one level can be published at the same
time once every lower level is visible in the registry index. Dev-dependencies
do not order publication, as in the canonical order check
(scripts/check_rust_release_packaging.py).

A release dependency that the canonical order lists later, an unknown or
duplicated crate, or a crate missing from the metadata is an error: the levels
can never contradict the order they are derived from.

Output formats:
  tsv      level<TAB>crate<TAB>version per crate, in canonical order (default)
  summary  one "level N (count): crate crate ..." line per level
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


class LevelError(Exception):
    pass


def load_metadata(root: Path, metadata_path: Path | None) -> dict:
    if metadata_path is not None:
        return json.loads(metadata_path.read_text(encoding="utf-8"))
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
            str(root / "Cargo.toml"),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise LevelError(f"cargo metadata failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def release_dependencies(package: dict, release: set[str]) -> set[str]:
    """Release crates `package` needs published first (not dev-dependencies)."""
    return {
        dependency["name"]
        for dependency in package.get("dependencies", [])
        if dependency.get("kind") != "dev"
        and dependency["name"] in release
        and dependency["name"] != package["name"]
    }


def compute_levels(order: list[str], metadata: dict) -> list[tuple[int, str, str]]:
    """(level, crate, version) for every crate of `order`, in that order."""
    seen: set[str] = set()
    duplicates = sorted({crate for crate in order if crate in seen or seen.add(crate)})
    if duplicates:
        raise LevelError(f"the canonical order lists {', '.join(duplicates)} more than once")
    packages = {package["name"]: package for package in metadata.get("packages", [])}
    missing = [crate for crate in order if crate not in packages]
    if missing:
        raise LevelError(f"cargo metadata has no workspace package for {', '.join(missing)}")

    release = set(order)
    position = {crate: index for index, crate in enumerate(order)}
    level: dict[str, int] = {}
    errors: list[str] = []
    for crate in order:
        dependencies = release_dependencies(packages[crate], release)
        later = sorted(dep for dep in dependencies if position[dep] > position[crate])
        for dependency in later:
            errors.append(f"{crate} appears before its dependency {dependency}")
        earlier = [level[dep] for dep in dependencies if dep in level]
        level[crate] = 1 + max(earlier) if earlier else 0
    if errors:
        raise LevelError("; ".join(errors))
    return [(level[crate], crate, packages[crate]["version"]) for crate in order]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument(
        "--metadata",
        type=Path,
        help="read `cargo metadata --no-deps --format-version 1` output from this file",
    )
    parser.add_argument("--format", choices=("tsv", "summary"), default="tsv")
    args = parser.parse_args(argv)

    order = [line.strip() for line in sys.stdin if line.strip()]
    if not order:
        print("release_crate_levels: no crates on stdin", file=sys.stderr)
        return 2
    try:
        rows = compute_levels(order, load_metadata(args.root, args.metadata))
    except (LevelError, json.JSONDecodeError, OSError) as error:
        print(f"release_crate_levels: {error}", file=sys.stderr)
        return 1

    if args.format == "tsv":
        for level, crate, version in rows:
            print(f"{level}\t{crate}\t{version}")
    else:
        levels: dict[int, list[str]] = {}
        for level, crate, _ in rows:
            levels.setdefault(level, []).append(crate)
        for level in sorted(levels):
            print(f"level {level} ({len(levels[level])}): {' '.join(levels[level])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
