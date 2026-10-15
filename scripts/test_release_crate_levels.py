#!/usr/bin/env python3
"""Unit tests for scripts/release_crate_levels.py (the publish levels).

Synthetic `cargo metadata` documents pin the derivation: normal, build and
target-specific dependencies order publication, dev-dependencies and
non-release crates do not, and an order the metadata contradicts, a duplicate
or an unknown crate is refused. The last case derives the levels of this
repository and checks every crate's release dependencies sit in lower levels.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))

import release_crate_levels as levels  # noqa: E402


def package(name: str, *dependencies: tuple[str, str | None, str | None]) -> dict:
    return {
        "name": name,
        "version": "1.2.3",
        "dependencies": [
            {"name": dep, "kind": kind, "target": target} for dep, kind, target in dependencies
        ],
    }


def metadata(*packages: dict) -> dict:
    return {"packages": list(packages)}


class ComputeLevelsTest(unittest.TestCase):
    def test_levels_follow_normal_build_and_target_dependencies(self) -> None:
        doc = metadata(
            package("core"),
            package("derive"),
            package("store", ("core", None, None)),
            package("schema", ("derive", "build", None)),
            package("native", ("core", None, "cfg(unix)")),
            package("facade", ("store", None, None), ("schema", None, None), ("serde", None, None)),
        )
        rows = levels.compute_levels(["core", "derive", "store", "schema", "native", "facade"], doc)
        self.assertEqual(
            rows,
            [
                (0, "core", "1.2.3"),
                (0, "derive", "1.2.3"),
                (1, "store", "1.2.3"),
                (1, "schema", "1.2.3"),
                (1, "native", "1.2.3"),
                (2, "facade", "1.2.3"),
            ],
        )

    def test_dev_dependencies_do_not_order_publication(self) -> None:
        doc = metadata(package("core", ("testkit", "dev", None)), package("testkit", ("core", None, None)))
        self.assertEqual(
            levels.compute_levels(["core", "testkit"], doc),
            [(0, "core", "1.2.3"), (1, "testkit", "1.2.3")],
        )

    def test_a_dependency_listed_later_is_refused(self) -> None:
        doc = metadata(package("core"), package("store", ("core", None, None)))
        with self.assertRaisesRegex(levels.LevelError, "store appears before its dependency core"):
            levels.compute_levels(["store", "core"], doc)

    def test_duplicates_and_unknown_crates_are_refused(self) -> None:
        doc = metadata(package("core"))
        with self.assertRaisesRegex(levels.LevelError, "more than once"):
            levels.compute_levels(["core", "core"], doc)
        with self.assertRaisesRegex(levels.LevelError, "no workspace package for ghost"):
            levels.compute_levels(["core", "ghost"], doc)

    def test_cli_summary_and_tsv(self) -> None:
        doc = metadata(package("core"), package("store", ("core", None, None)))
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
            json.dump(doc, handle)
        try:
            for fmt, expected in (
                ("summary", "level 0 (1): core\nlevel 1 (1): store\n"),
                ("tsv", "0\tcore\t1.2.3\n1\tstore\t1.2.3\n"),
            ):
                result = subprocess.run(
                    [sys.executable, str(SCRIPTS / "release_crate_levels.py"), "--metadata", handle.name, "--format", fmt],
                    input="core\nstore\n",
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual((result.returncode, result.stdout), (0, expected), result.stderr)
            refused = subprocess.run(
                [sys.executable, str(SCRIPTS / "release_crate_levels.py"), "--metadata", handle.name],
                input="store\ncore\n",
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(refused.returncode, 1)
            self.assertIn("store appears before its dependency core", refused.stderr)
        finally:
            Path(handle.name).unlink()

    def test_repository_levels_respect_every_release_dependency(self) -> None:
        root = SCRIPTS.parent
        order = subprocess.run(
            [str(SCRIPTS / "release-rust-crates.sh")], capture_output=True, text=True, check=True
        ).stdout.split()
        doc = levels.load_metadata(root, None)
        rows = levels.compute_levels(order, doc)
        level = {crate: number for number, crate, _ in rows}
        packages = {entry["name"]: entry for entry in doc["packages"]}
        release = set(order)
        for crate in order:
            for dependency in levels.release_dependencies(packages[crate], release):
                self.assertLess(level[dependency], level[crate], f"{crate} -> {dependency}")
        self.assertEqual([crate for _, crate, _ in rows], order)


if __name__ == "__main__":
    unittest.main()
