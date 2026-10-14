#!/usr/bin/env python3
"""Contract test for scripts/check-release-source-test-hooks.

Every archive the release publisher accepts is bound to the release tag's
tree; the check must refuse a tree whose built-binary test hooks (the
RKAT_TEST_CLIENT shim, the OAuth fixture endpoint override) reach release
builds, accept a tree that compiles them only under debug assertions or
tests, and fail when a listed hook disappears from where it is listed. A gate
counts only on the statement itself or a construct enclosing it, never on a
neighbouring item.
"""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "scripts" / "check-release-source-test-hooks"
# Git hook variables (GIT_DIR, GIT_INDEX_FILE, ...) would point the fixture
# repositories' commands at the repository running the hook.
GIT_FREE_ENV = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
# The base of the 0.9.1 gating: its release binaries honoured every hook.
PRE_FIX_BASE = "08d75606b85511e1d539f08440d3434019ff755f"

GATED_FACTORY = """
fn unrelated() {}

/// Built-binary test shim: `RKAT_TEST_CLIENT=1` swaps in the test client.
#[cfg(debug_assertions)]
fn test_client_shim_requested() -> bool {
    std::env::var("RKAT_TEST_CLIENT").ok().as_deref() == Some("1")
}

#[cfg(not(debug_assertions))]
const fn test_client_shim_requested() -> bool {
    false
}

/// An unrelated C string with a Unicode escape: valid Rust, so accepted.
fn unrelated_c_string() -> &'static core::ffi::CStr {
    c"\\u{41}"
}
"""

LEGACY_FACTORY = """
#[cfg(not(target_arch = "wasm32"))]
fn build() {
    if std::env::var("RKAT_TEST_CLIENT").ok().as_deref() == Some("1") {
        return;
    }
}
"""

GATED_OAUTH = """
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
const TEST_OAUTH_ENDPOINT_OVERRIDE_ENV: &str = "MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE";
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
const TEST_OAUTH_BASE_URL_ENV: &str = "MEERKAT_TEST_OAUTH_BASE_URL";

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn child() {
        let _ = ("MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE", "MEERKAT_TEST_OAUTH_BASE_URL");
    }
}
"""

LEGACY_OAUTH = """
const TEST_OAUTH_ENDPOINT_OVERRIDE_ENV: &str = "MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE";
const TEST_OAUTH_BASE_URL_ENV: &str = "MEERKAT_TEST_OAUTH_BASE_URL";
"""

# An ungated read right below a gated item: the gate is not on the read.
NEIGHBOUR_GATED_FACTORY = """
#[cfg(debug_assertions)]
fn debug_only() {}

fn build() {
    if std::env::var("RKAT_TEST_CLIENT").ok().as_deref() == Some("1") {
        return;
    }
}
"""

MULTI_LINE_GATED_FACTORY = """
impl Factory {
    #[cfg(all(
        debug_assertions,
        not(target_arch = "wasm32"),
    ))]
    fn test_client_shim_requested(&self) -> bool {
        let requested = std::env::var(
            "RKAT_TEST_CLIENT",
        );
        requested.is_ok()
    }
}
"""

# Column-0 lines of a raw string inside the gated module are not code.
RAW_STRING_IN_GATED_MODULE_FACTORY = """
#[cfg(test)]
mod tests {
    const CONFIG: &str = r#"
[server]
name = "fixture"
"#;

    #[test]
    fn reads_the_shim() {
        let _ = std::env::var("RKAT_TEST_CLIENT");
    }
}
"""

# The name only in comments and inside a longer string: no code reads it.
NAME_ONLY_IN_TEXT_FACTORY = """
// std::env::var("RKAT_TEST_CLIENT") used to live here.
/* "RKAT_TEST_CLIENT" */
const DOC: &str = r#"set "RKAT_TEST_CLIENT" to 1"#;
"""

# An ungated raw-string read beside a test-gated plain occurrence: the
# gated one must not vouch for the raw read.
RAW_UNGATED_READ_FACTORY = """
fn test_client_shim_requested() -> bool {
    std::env::var(r"RKAT_TEST_CLIENT").ok().as_deref() == Some("1")
}

#[cfg(test)]
mod tests {
    const SHIM: &str = "RKAT_TEST_CLIENT";
}
"""

GATED_RAW_READ_FACTORY = """
#[cfg(debug_assertions)]
fn test_client_shim_requested() -> bool {
    std::env::var(r#"RKAT_TEST_CLIENT"#).ok().as_deref() == Some("1")
}
"""

# An ungated read spelled with an escape is still the hook's name.
# Ungated reads spelled with escapes are still the hook's name, beside a
# test-gated plain occurrence; a Unicode escape may put any number of
# underscores after each of its hex digits.
ESCAPED_UNGATED_READ_FACTORY = """
fn test_client_shim_requested() -> bool {
    std::env::var("RKAT\\x5fTEST_CLIENT").ok().as_deref() == Some("1")
        || std::env::var("RKAT\\u{5_______f}TEST_CLIENT").is_ok()
}

#[cfg(test)]
mod tests {
    const SHIM: &str = "RKAT_TEST_CLIENT";
}
"""

# A C string may not hold NUL: the check cannot decode c"\\u{0}" either.
NUL_C_STRING_FACTORY = GATED_FACTORY + """
fn nul() -> &'static core::ffi::CStr {
    c"\\u{0}"
}
"""

# A literal whose escape the check cannot decode might spell the hook's name.
UNDECODABLE_LITERAL_FACTORY = GATED_FACTORY + """
fn other() -> &'static str {
    "RKAT\\u{_5f}TEST_CLIENT"
}
"""

RAW_UNGATED_OAUTH = """
const TEST_OAUTH_ENDPOINT_OVERRIDE_ENV: &str = r"MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE";
const TEST_OAUTH_BASE_URL_ENV: &str = r##"MEERKAT_TEST_OAUTH_BASE_URL"##;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn child() {
        let _ = ("MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE", "MEERKAT_TEST_OAUTH_BASE_URL");
    }
}
"""

FACTORY = "crates/meerkat/src/factory.rs"
OAUTH = "crates/meerkat-auth-core/src/oauth_flow.rs"


class SourceTestHookCheckTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="meerkat-source-hooks-")
        self.repo = pathlib.Path(self._tmp.name)
        self.git("init", "-q")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def git(self, *args: str) -> str:
        return subprocess.run(
            ["git", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", *args],
            cwd=self.repo,
            env=GIT_FREE_ENV,
            capture_output=True,
            text=True,
            check=True,
        ).stdout

    def commit(self, files: dict[str, str]) -> str:
        for rel, text in files.items():
            path = self.repo / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "fixture")
        return self.git("rev-parse", "HEAD").strip()

    def check(self, commit: str, cwd: pathlib.Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(CHECK), commit],
            cwd=cwd or self.repo,
            env=GIT_FREE_ENV,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_gated_hooks_pass(self) -> None:
        sha = self.commit({FACTORY: GATED_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('"RKAT_TEST_CLIENT" is compiled only for debug or test builds', result.stdout)

    def test_legacy_test_client_shim_fails(self) -> None:
        sha = self.commit({FACTORY: LEGACY_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{FACTORY}:4 at {sha} reads "RKAT_TEST_CLIENT" in release builds: neither it nor', result.stderr)
        self.assertIn("do not publish them", result.stderr)

    def test_legacy_oauth_override_fails(self) -> None:
        sha = self.commit({FACTORY: GATED_FACTORY, OAUTH: LEGACY_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{OAUTH}:2 at {sha} reads "MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE" in release builds', result.stderr)
        self.assertIn(f'{OAUTH}:3 at {sha} reads "MEERKAT_TEST_OAUTH_BASE_URL" in release builds', result.stderr)

    def test_negated_gate_fails(self) -> None:
        sha = self.commit({
            FACTORY: GATED_FACTORY.replace("#[cfg(debug_assertions)]", "#[cfg(not(debug_assertions))]", 1),
            OAUTH: GATED_OAUTH,
        })
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{FACTORY}:7 at {sha} reads "RKAT_TEST_CLIENT" in release builds', result.stderr)

    def test_gate_on_a_neighbouring_item_does_not_count(self) -> None:
        sha = self.commit({FACTORY: NEIGHBOUR_GATED_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{FACTORY}:6 at {sha} reads "RKAT_TEST_CLIENT" in release builds', result.stderr)

    def test_multi_line_gate_on_a_nested_item_passes(self) -> None:
        sha = self.commit({FACTORY: MULTI_LINE_GATED_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_ungated_raw_string_read_fails_beside_a_gated_plain_one(self) -> None:
        sha = self.commit({FACTORY: RAW_UNGATED_READ_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{FACTORY}:3 at {sha} reads "RKAT_TEST_CLIENT" in release builds', result.stderr)
        self.assertNotIn(f"{FACTORY}:9 ", result.stderr)

    def test_gated_raw_string_read_passes(self) -> None:
        sha = self.commit({FACTORY: GATED_RAW_READ_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_ungated_escaped_read_fails(self) -> None:
        sha = self.commit({FACTORY: ESCAPED_UNGATED_READ_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{FACTORY}:3 at {sha} reads "RKAT_TEST_CLIENT" in release builds', result.stderr)
        self.assertIn(f'{FACTORY}:4 at {sha} reads "RKAT_TEST_CLIENT" in release builds', result.stderr)
        self.assertNotIn(f"{FACTORY}:9 ", result.stderr)

    def test_undecodable_literal_fails_closed(self) -> None:
        sha = self.commit({FACTORY: UNDECODABLE_LITERAL_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"{FACTORY}:21 at {sha} has a string literal whose escapes this check cannot decode", result.stderr)

    def test_nul_c_string_escape_fails_closed(self) -> None:
        sha = self.commit({FACTORY: NUL_C_STRING_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"{FACTORY}:21 at {sha} has a string literal whose escapes this check cannot decode", result.stderr)

    def test_ungated_raw_oauth_reads_fail_beside_gated_plain_ones(self) -> None:
        sha = self.commit({FACTORY: GATED_FACTORY, OAUTH: RAW_UNGATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f'{OAUTH}:2 at {sha} reads "MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE" in release builds', result.stderr)
        self.assertIn(f'{OAUTH}:3 at {sha} reads "MEERKAT_TEST_OAUTH_BASE_URL" in release builds', result.stderr)
        self.assertNotIn(f"{OAUTH}:9 ", result.stderr)

    def test_raw_string_lines_inside_a_gated_module_are_not_code(self) -> None:
        sha = self.commit({FACTORY: RAW_STRING_IN_GATED_MODULE_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_name_in_comments_or_longer_strings_is_not_a_read(self) -> None:
        sha = self.commit({FACTORY: NAME_ONLY_IN_TEXT_FACTORY, OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('no longer reads "RKAT_TEST_CLIENT"; the hook moved', result.stderr)

    def test_gates_that_reach_release_builds_fail(self) -> None:
        for gate in (
            '#[cfg(any(test, feature = "fixtures"))]',
            "#[cfg_attr(test, allow(dead_code))]",
            "#[cfg(all(unix, not(debug_assertions)))]",
        ):
            with self.subTest(gate=gate):
                sha = self.commit({
                    FACTORY: GATED_FACTORY.replace("#[cfg(debug_assertions)]", gate, 1),
                    OAUTH: GATED_OAUTH,
                })
                result = self.check(sha)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f'{FACTORY}:7 at {sha} reads "RKAT_TEST_CLIENT" in release builds', result.stderr)

    def test_moved_hook_fails(self) -> None:
        sha = self.commit({FACTORY: "fn nothing_here() {}\n", OAUTH: GATED_OAUTH})
        result = self.check(sha)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('no longer reads "RKAT_TEST_CLIENT"; the hook moved', result.stderr)

    def test_unknown_commit_is_a_usage_error(self) -> None:
        self.commit({FACTORY: GATED_FACTORY, OAUTH: GATED_OAUTH})
        self.assertEqual(self.check("no-such-ref").returncode, 2)

    def test_repository_head_gates_every_hook(self) -> None:
        result = self.check("HEAD", cwd=ROOT)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pre_fix_base_is_refused(self) -> None:
        present = subprocess.run(
            ["git", "cat-file", "-e", f"{PRE_FIX_BASE}^{{commit}}"],
            cwd=ROOT,
            env=GIT_FREE_ENV,
            capture_output=True,
            check=False,
        )
        if present.returncode != 0:
            self.skipTest(f"{PRE_FIX_BASE} is not in this clone")
        result = self.check(PRE_FIX_BASE, cwd=ROOT)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('reads "RKAT_TEST_CLIENT" in release builds', result.stderr)
        self.assertIn('reads "MEERKAT_TEST_OAUTH_ENDPOINT_OVERRIDE" in release builds', result.stderr)


if __name__ == "__main__":
    unittest.main()
