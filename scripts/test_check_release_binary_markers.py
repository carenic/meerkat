#!/usr/bin/env python3
"""Contract test for scripts/check-release-binary-markers.

The artifact guard must refuse a binary carrying any of its test-only markers,
name the marker, accept a clean binary, and fail closed on unreadable input.
One marker, tokio's test-util, passes in the binaries of a release before
0.9.2 (named by VERSION), whose Bazel release builds compiled that feature;
with VERSION unset, unparsable, or 0.9.2 and later it fails like every other
marker, and it never masks another.
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
# tokio's test-util marker: allowed only for a release before 0.9.2.
TOKIO_TEST_UTIL = "time is not frozen"
# VERSION values that name a release before 0.9.2, and values that do not
# (0.9.2 and later, a 0.9.2 prerelease, or not a release version at all).
BEFORE_0_9_2 = ("0.9.1", "v0.9.1", "0.9.1-rc.1", "0.9.0", "0.8.24", "0.1.0")
NOT_BEFORE_0_9_2 = (
    "0.9.2",
    "v0.9.2",
    "0.9.2-rc.1",
    "0.9.3",
    "0.10.0",
    "1.0.0",
    "1.0.0-alpha.1",
    "",
    "0.9",
    "0.9.1.1",
    "00.9.1",
    "0.09.1",
    "0.9.1+build.1",
    "V0.9.1",
    " 0.9.1",
    "release-0.9.1",
    "latest",
)


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

    def guard(self, *args: str, version: str | None = None) -> subprocess.CompletedProcess[str]:
        # The caller's environment never decides: VERSION is unset unless given.
        env = {key: value for key, value in os.environ.items() if key != "VERSION"}
        if version is not None:
            env["VERSION"] = version
        return subprocess.run([str(GUARD), *args], capture_output=True, text=True, check=False, env=env)

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

    def test_tokio_test_util_marker_fails_without_a_release_before_0_9_2(self) -> None:
        # Since 0.9.2 the Bazel release builds drop tokio's test-util (a
        # select on //tools/bazel:release_build): its marker fails a binary
        # alone unless VERSION names an older release.
        self.assertIn(TOKIO_TEST_UTIL, [marker for marker, _, _ in markers()])
        path = self.binary("rkat-mcp", TOKIO_TEST_UTIL.encode())
        for version in (None, *NOT_BEFORE_0_9_2):
            with self.subTest(version=version):
                result = self.guard(str(path), version=version)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f'carries test-only code: "{TOKIO_TEST_UTIL}"', result.stderr)
                self.assertIn("only a release before 0.9.2 may carry it", result.stderr)
                self.assertNotIn("carries the allowed marker", result.stdout + result.stderr)

    def test_tokio_test_util_marker_passes_in_a_release_before_0_9_2(self) -> None:
        # Asset recovery of an older tag runs the current guard on binaries
        # whose Bazel release build compiled tokio's test-util.
        path = self.binary("rkat-mcp", TOKIO_TEST_UTIL.encode())
        for version in BEFORE_0_9_2:
            with self.subTest(version=version):
                result = self.guard(str(path), version=version)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(f'carries the allowed marker "{TOKIO_TEST_UTIL}"', result.stdout)
                self.assertIn(f"beyond the one allowed for {version}", result.stdout)

    def test_no_release_allows_any_other_marker(self) -> None:
        for version in BEFORE_0_9_2:
            for marker, _, _ in markers():
                if marker == TOKIO_TEST_UTIL:
                    continue
                with self.subTest(version=version, marker=marker):
                    alone = self.binary("rkat", marker.encode())
                    result = self.guard(str(alone), version=version)
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(f'carries test-only code: "{marker}"', result.stderr)
                    # The allowed marker never masks another.
                    both = self.binary("rkat-rest", TOKIO_TEST_UTIL.encode() + b"\0" + marker.encode())
                    result = self.guard(str(both), version=version)
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn(f'carries test-only code: "{marker}"', result.stderr)
                    self.assertNotIn(f'carries test-only code: "{TOKIO_TEST_UTIL}"', result.stderr)

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
