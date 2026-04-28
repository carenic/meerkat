#!/usr/bin/env python3
"""Validate release crate membership and required package metadata."""

from __future__ import annotations

import pathlib
import sys
import tomllib


def main() -> int:
    if len(sys.argv) < 2:
        print("Usage: check_rust_release_packaging.py REPO_ROOT [CRATE ...]", file=sys.stderr)
        return 2

    root = pathlib.Path(sys.argv[1])
    expected = set(sys.argv[2:])
    workspace = tomllib.loads((root / "Cargo.toml").read_text())

    paths: list[pathlib.Path] = []
    for member in workspace["workspace"]["members"]:
        if "*" in member:
            paths.extend(sorted(root.glob(member)))
        else:
            paths.append(root / member)

    publishable = set()
    metadata_errors = []
    for path in paths:
        manifest = path / "Cargo.toml"
        if not manifest.exists():
            continue
        data = tomllib.loads(manifest.read_text())
        package = data.get("package", {})
        name = package.get("name")
        if not name:
            continue
        if package.get("publish", "default") is False:
            continue
        publishable.add(name)

        for field in ("description", "license", "repository", "homepage", "documentation"):
            value = package.get(field)
            if value is None:
                metadata_errors.append(
                    f"{name}: missing required package metadata field `{field}`"
                )
            elif isinstance(value, str) and not value.strip():
                metadata_errors.append(f"{name}: empty required package metadata field `{field}`")

    missing = sorted(publishable - expected)
    unexpected = sorted(expected - publishable)
    if missing or unexpected or metadata_errors:
        if missing:
            print("Publishable workspace crates missing from release list:", file=sys.stderr)
            for name in missing:
                print(f"  - {name}", file=sys.stderr)
        if unexpected:
            print(
                "Release list contains crates that are not publishable workspace members:",
                file=sys.stderr,
            )
            for name in unexpected:
                print(f"  - {name}", file=sys.stderr)
        if metadata_errors:
            print("Publishable workspace crates with invalid release metadata:", file=sys.stderr)
            for err in metadata_errors:
                print(f"  - {err}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
