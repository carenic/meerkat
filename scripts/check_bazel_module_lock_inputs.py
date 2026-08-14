#!/usr/bin/env python3
"""Verify MODULE.bazel.lock still matches the workspace files it recorded.

The crate_universe module extension records every workspace manifest and
Cargo.lock as an extension input, each with its sha256. Change Cargo.lock and
skip the module-lock refresh, and Bazel refuses with "MODULE.bazel.lock is no
longer up-to-date" - which is how a one-line lock heal took out all four
BuildBuddy release-binary lanes after tests and CI were already green.

`bb mod deps --lockfile_mode=error` is the authority, but it needs the pinned
CLI and network. This check needs neither: the recorded hashes are in the lock
file, so staleness of the workspace-file inputs is decidable offline, which is
what makes it runnable on every push and in the release preflight.

Usage: check_bazel_module_lock_inputs.py REPO_ROOT [MODULE_LOCK_PATH]
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised only on Python < 3.11
    import tomli as tomllib

MAIN_REPO_FILE_PREFIX = "FILE:@@//"


def recorded_main_repo_files(lock: dict) -> dict[str, str]:
    """Map repo-relative path -> recorded sha256 for main-repo file inputs."""

    recorded: dict[str, str] = {}
    for extension in lock.get("moduleExtensions", {}).values():
        for scope in extension.values():
            if not isinstance(scope, dict):
                continue
            inputs = scope.get("recordedInputs") or []
            entries = inputs.values() if isinstance(inputs, dict) else inputs
            for entry in entries:
                if not isinstance(entry, str) or not entry.startswith(MAIN_REPO_FILE_PREFIX):
                    continue
                body = entry[len(MAIN_REPO_FILE_PREFIX) :]
                path, _, digest = body.partition(" ")
                recorded[path] = digest
    return recorded


def workspace_input_files(root: pathlib.Path) -> list[str]:
    """Workspace files crate_universe must have recorded: root manifest, lock, members."""

    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    required = ["Cargo.toml", "Cargo.lock"]
    for member in workspace.get("members", []):
        if "*" in member:
            for path in sorted(root.glob(member)):
                if (path / "Cargo.toml").exists():
                    required.append(f"{path.relative_to(root).as_posix()}/Cargo.toml")
        else:
            required.append(f"{member}/Cargo.toml")
    return required


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(
            "Usage: check_bazel_module_lock_inputs.py REPO_ROOT [MODULE_LOCK_PATH]",
            file=sys.stderr,
        )
        return 2

    root = pathlib.Path(sys.argv[1])
    lock_path = pathlib.Path(sys.argv[2]) if len(sys.argv) == 3 else root / "MODULE.bazel.lock"

    try:
        lock = json.loads(lock_path.read_text())
    except FileNotFoundError:
        print(f"{lock_path}: module lockfile is missing", file=sys.stderr)
        return 1
    except json.JSONDecodeError as error:
        print(f"{lock_path}: module lockfile is not valid JSON: {error}", file=sys.stderr)
        return 1

    recorded = recorded_main_repo_files(lock)
    if not recorded:
        print(f"{lock_path}: records no main-repo file inputs", file=sys.stderr)
        print(
            "  crate_universe records every workspace manifest; an empty set means "
            "this check can no longer see staleness.",
            file=sys.stderr,
        )
        return 1

    stale: list[str] = []
    for relative_path, digest in sorted(recorded.items()):
        candidate = root / relative_path
        try:
            actual = hashlib.sha256(candidate.read_bytes()).hexdigest()
        except FileNotFoundError:
            stale.append(f"{relative_path}: recorded as an extension input but no longer exists")
            continue
        if actual != digest:
            stale.append(f"{relative_path}: recorded {digest[:12]}, on disk {actual[:12]}")

    missing = [path for path in workspace_input_files(root) if path not in recorded]

    if stale or missing:
        print(f"{lock_path} is out of date:", file=sys.stderr)
        for error in stale:
            print(f"  - {error}", file=sys.stderr)
        for path in missing:
            print(f"  - {path}: workspace file is not recorded as an extension input", file=sys.stderr)
        print("", file=sys.stderr)
        print("Refresh with: make buildbuddy-lock-update", file=sys.stderr)
        print("Then stage MODULE.bazel.lock. Bazel refuses stale module locks in", file=sys.stderr)
        print("error mode, which is every BuildBuddy release-binary lane.", file=sys.stderr)
        return 1

    print(f"{lock_path}: {len(recorded)} recorded workspace inputs match on-disk content")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
