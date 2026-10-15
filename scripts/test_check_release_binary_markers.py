#!/usr/bin/env python3
"""Contract test for scripts/check-release-binary-markers.

The artifact guard must refuse a binary carrying any of its test-only markers,
name the marker, accept a clean binary, and fail closed on unreadable input.
No marker is allowed: tokio's test-util marker, the one exception until 0.9.2,
now fails a binary like every other marker.
Every workspace marker must still be a string that only gated test code
compiles in: it must occur in its declared source under a `#[cfg(...)]` that
names its gate. No marker may appear in the docs or in the help corpus the
release binaries embed, where a mention would block a release.
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
GUARD = ROOT / "scripts" / "check-release-binary-markers"
# The guard and this test name the markers; their gated sources compile them.
NAMING_FILES = {"scripts/check-release-binary-markers", "scripts/test_check_release_binary_markers.py"}
GATE_WINDOW = 60
# tokio's test-util marker: allowed until 0.9.2, refused like any other since.
TOKIO_TEST_UTIL = "time is not frozen"


def markers() -> list[tuple[str, str, str]]:
    listing = subprocess.run([str(GUARD), "--list"], capture_output=True, text=True, check=True).stdout
    entries = []
    for line in listing.splitlines():
        marker, source, gate = line.split("|")
        entries.append((marker, source, gate))
    return entries


def tracked_files() -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z"],
        capture_output=True,
        check=True,
    )
    return [name for name in result.stdout.decode().split("\0") if name]


def gate_pattern(gate: str) -> re.Pattern[str]:
    if gate == "debug_assertions":
        return re.compile(r"#\[cfg\((?!not\()[^\]]*\bdebug_assertions\b")
    feature = gate.removeprefix("feature ")
    return re.compile(r'#\[cfg\((?!not\()[^\]]*feature\s*=\s*"' + re.escape(feature) + '"')


class ReleaseBinaryMarkerTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="meerkat-release-markers-")
        self.dir = pathlib.Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def binary(self, name: str, payload: bytes = b"") -> pathlib.Path:
        path = self.dir / name
        path.write_bytes(os.urandom(4096) + payload + os.urandom(4096))
        return path

    def guard(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run([str(GUARD), *args], capture_output=True, text=True, check=False)

    def test_marker_list_is_well_formed(self) -> None:
        entries = markers()
        self.assertGreaterEqual(len(entries), 5)
        self.assertEqual(len({marker for marker, _, _ in entries}), len(entries))
        for marker, source, gate in entries:
            self.assertGreaterEqual(len(marker), 16, f"{marker!r} is too short to be unique")
            self.assertTrue(source and gate, marker)

    def test_each_marker_fails_the_binary_carrying_it(self) -> None:
        for marker, source, _ in markers():
            with self.subTest(marker=marker):
                path = self.binary("rkat", marker.encode())
                result = self.guard(str(path))
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f'carries test-only code: "{marker}"', result.stderr)
                self.assertIn(source, result.stderr)

    def test_tokio_test_util_marker_is_no_longer_allowed(self) -> None:
        # The Bazel release builds drop tokio's test-util (a select on
        # //tools/bazel:release_build), so its marker fails a binary alone and
        # the guard has no allowlist left to consult.
        self.assertIn(TOKIO_TEST_UTIL, [marker for marker, _, _ in markers()])
        path = self.binary("rkat-mcp", TOKIO_TEST_UTIL.encode())
        result = self.guard(str(path))
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'carries test-only code: "{TOKIO_TEST_UTIL}"', result.stderr)
        self.assertNotIn("allowed", result.stdout + result.stderr)
        self.assertEqual(self.guard("--allowed").returncode, 2)

    def test_clean_binaries_pass_and_only_the_dirty_one_is_named(self) -> None:
        clean = self.binary("rkat-rpc")
        result = self.guard(str(clean))
        self.assertEqual(result.returncode, 0, result.stderr)
        marker = next(marker for marker, _, _ in markers() if marker != TOKIO_TEST_UTIL)
        dirty = self.binary("rkat-rest", b"\0" + marker.encode() + b"\0")
        result = self.guard(str(clean), str(dirty))
        self.assertEqual(result.returncode, 1)
        self.assertIn(str(dirty), result.stderr)
        self.assertNotIn(str(clean), result.stderr)

    def test_unreadable_input_fails_closed(self) -> None:
        self.assertEqual(self.guard().returncode, 2)
        self.assertEqual(self.guard(str(self.dir / "missing")).returncode, 2)

    def test_workspace_markers_are_gated_test_code(self) -> None:
        for marker, source, gate in markers():
            if source.startswith("external:"):
                continue
            with self.subTest(marker=marker):
                lines = (ROOT / source).read_text(encoding="utf-8").splitlines()
                hits = [index for index, line in enumerate(lines) if marker in line]
                self.assertTrue(hits, f"{marker!r} no longer occurs in {source}; update the marker")
                pattern = gate_pattern(gate)
                first = hits[0]
                window = lines[max(0, first - GATE_WINDOW):first + 1]
                cfgs = [line for line in window if "#[cfg(" in line]
                self.assertTrue(cfgs and pattern.search(cfgs[-1]), (
                    f"{marker!r} in {source}:{first + 1} is not under a #[cfg] naming {gate}; "
                    f"nearest cfg: {cfgs[-1].strip() if cfgs else None}"
                ))

    def test_no_marker_in_docs_or_the_embedded_help_corpus(self) -> None:
        docs = [
            name
            for name in tracked_files()
            if name.endswith((".md", ".mdx")) or name.startswith(".claude/skills/")
        ]
        self.assertTrue(docs)
        for name in docs:
            path = ROOT / name
            # Tracked symlinks into a developer's own skill directories are not
            # part of the repository's content.
            if name in NAMING_FILES or path.is_symlink() or not path.is_file():
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            for marker, _, _ in markers():
                self.assertNotIn(marker, text, f"{name} mentions release marker {marker!r}")


if __name__ == "__main__":
    unittest.main()
