#!/usr/bin/env python3
"""Verify every in-tree Cargo path patch is present in Bazel workspace runfiles."""

from __future__ import annotations

import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.9 on the oldest supported macOS hosts.
    tomllib = None


PACKAGE_RUNFILES_RULE = re.compile(
    r"filegroup\s*\(.*?name\s*=\s*[\"']package_runfiles[\"']",
    re.DOTALL,
)

PATCH_SECTION = re.compile(r"^\[patch\.([^\].]+)(?:\.([^\]]+))?\]$")
INLINE_PATCH = re.compile(r"^([A-Za-z0-9_-]+)\s*=\s*\{(.*)\}\s*$")
PATH_VALUE = re.compile(r"(?:^|,)\s*path\s*=\s*[\"']([^\"']+)[\"']")


def path_patches(manifest_text: str) -> list[tuple[str, str, str]]:
    if tomllib is not None:
        manifest = tomllib.loads(manifest_text)
        found: list[tuple[str, str, str]] = []
        for registry, patches in manifest.get("patch", {}).items():
            if not isinstance(patches, dict):
                continue
            for crate, spec in patches.items():
                if isinstance(spec, dict) and isinstance(spec.get("path"), str):
                    found.append((registry, crate, spec["path"]))
        return found

    # Python 3.9 fallback for Cargo's two path-patch forms. This deliberately
    # recognizes only the path field instead of pretending to be a TOML parser.
    found = []
    registry = None
    table_crate = None
    for raw_line in manifest_text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        section = PATCH_SECTION.match(line)
        if section:
            registry, table_crate = section.groups()
            continue
        if line.startswith("["):
            registry = None
            table_crate = None
            continue
        if registry is None:
            continue
        if table_crate is not None:
            value = re.match(r"^path\s*=\s*[\"']([^\"']+)[\"']\s*$", line)
            if value:
                found.append((registry, table_crate, value.group(1)))
            continue
        inline = INLINE_PATCH.match(line)
        if inline and (value := PATH_VALUE.search(inline.group(2))):
            found.append((registry, inline.group(1), value.group(1)))
    return found


def main() -> int:
    root = pathlib.Path(sys.argv[1] if len(sys.argv) == 2 else ".").resolve()
    cargo_toml = root / "Cargo.toml"
    root_build = root / "BUILD.bazel"
    try:
        manifest_text = cargo_toml.read_text(encoding="utf-8")
        root_build_text = root_build.read_text(encoding="utf-8")
        patches = path_patches(manifest_text)
    except (OSError, ValueError) as error:
        print(f"error: cannot inspect Bazel path-patch runfiles: {error}", file=sys.stderr)
        return 2

    errors: list[str] = []
    checked = 0
    for registry, crate, patch_path in patches:
        checked += 1
        dependency = (root / patch_path).resolve()
        try:
            relative_dependency = dependency.relative_to(root)
        except ValueError:
            errors.append(
                f"Cargo [patch.{registry}] {crate} points outside the workspace: {dependency}"
            )
            continue

        build_file = dependency / "BUILD.bazel"
        try:
            build_text = build_file.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{relative_dependency}/BUILD.bazel is unavailable: {error}")
            continue

        if not PACKAGE_RUNFILES_RULE.search(build_text):
            errors.append(
                f"{relative_dependency}/BUILD.bazel does not export package_runfiles"
            )

        label = f"//{relative_dependency.as_posix()}:package_runfiles"
        if f'"{label}"' not in root_build_text:
            errors.append(f"BUILD.bazel workspace_runfiles omits {label}")

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"Bazel path-patch runfiles ok: {checked} in-tree patch(es)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
