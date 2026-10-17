#!/usr/bin/env python3
"""Self-test for the SDK binary digest release tooling (#1739).

scripts/release-stamp-sdk-binary-digests, on synthetic releases and a stubbed
`gh` (no network): it writes nothing unless the manifest is the complete one
for the tag and every rkat-rpc archive is within the SDKs' 256 MiB cap,
re-hashes to its manifest entry, carries a build attestation from the
repository's release workflow and holds exactly one binary at its root, and
both SDK packages carry the tag's version. The pinned binary digest is the
binary's inside the verified archive. `verify`, which the release workflow runs
on each built package before publishing it, refuses an unstamped,
wrong-version, incomplete or different table. The committed modules are the
unstamped rendering, so a source tree pins nothing.

scripts/release-await-binary-manifest, on a stubbed `gh`: it proceeds once
the release carries checksums.sha256 and index.json, refuses a package
recovery whose release has none, fails at once when the trailing publisher
failed, counts a failed API call as "not yet", and gives up at its deadline.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
REPO_ROOT = SCRIPTS.parent
STAMP = SCRIPTS / "release-stamp-sdk-binary-digests"
AWAIT = SCRIPTS / "release-await-binary-manifest"
REPO = "example/meerkat"
SIGNER = f"{REPO}/.github/workflows/release.yml"

# `gh attestation verify <file> --repo R --signer-workflow W`: logs each call
# and refuses the files named in STUB_GH_UNATTESTED.
GH_ATTESTATION_STUB = """#!/bin/sh
printf '%s\\n' "$*" >> "$STUB_GH_LOG"
[ "$1 $2" = "attestation verify" ] || { echo "stub gh: unsupported $*" >&2; exit 2; }
for name in $STUB_GH_UNATTESTED; do
  case "$3" in */"$name") echo "Error: HTTP 404: Not Found" >&2; exit 1 ;; esac
done
exit 0
"""
PYTHON_MODULE = Path("sdks/python/meerkat/_rkat_rpc_digests.py")
TYPESCRIPT_MODULE = Path("sdks/typescript/src/rkat-rpc-digests.ts")
TS_ASSIGNMENT = "export const RKAT_RPC_DIGESTS: RkatRpcDigests = "

VERSION = "9.9.9"
TAG = f"v{VERSION}"
PREFIXES = ("rkat", "rkat-mcp", "rkat-rest", "rkat-rpc")
TARGETS = (
    ("x86_64-unknown-linux-gnu", "tar.gz"),
    ("aarch64-unknown-linux-gnu", "tar.gz"),
    ("aarch64-apple-darwin", "tar.gz"),
    ("x86_64-apple-darwin", "tar.gz"),
    ("x86_64-pc-windows-msvc", "zip"),
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def binary_name(target: str) -> str:
    return "rkat-rpc.exe" if "windows" in target else "rkat-rpc"


def binary_for(target: str) -> bytes:
    return f"rkat-rpc {VERSION} for {target}\n".encode()


def archive_bytes(extension: str, members: dict[str, bytes]) -> bytes:
    buffer = io.BytesIO()
    if extension == "tar.gz":
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            for name, data in members.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755
                archive.addfile(info, io.BytesIO(data))
    else:
        with zipfile.ZipFile(buffer, "w") as archive:
            for name, data in members.items():
                archive.writestr(name, data)
    return buffer.getvalue()


def all_archive_names() -> list[str]:
    return sorted(
        f"{prefix}-{VERSION}-{target}.{extension}"
        for prefix in PREFIXES
        for target, extension in TARGETS
    )


def rpc_asset(target: str, extension: str) -> str:
    return f"rkat-rpc-{VERSION}-{target}.{extension}"


class Fixture:
    """A complete synthetic release plus a minimal SDK tree to stamp."""

    def __init__(self, root: Path):
        self.root = root
        self.release = root / "release"
        self.repo = root / "repo"
        self.bin = root / "bin"
        self.gh_log = root / "gh.log"
        self.unattested: list[str] = []
        self.release.mkdir()
        self.bin.mkdir()
        (self.bin / "gh").write_text(GH_ATTESTATION_STUB, encoding="utf-8")
        (self.bin / "gh").chmod(0o755)
        digests = {name: sha256(name.encode()) for name in all_archive_names()}
        for target, extension in TARGETS:
            asset = rpc_asset(target, extension)
            data = archive_bytes(extension, {binary_name(target): binary_for(target)})
            (self.release / asset).write_bytes(data)
            digests[asset] = sha256(data)
        self.write_checksums(digests)
        self.write_index(all_archive_names())
        for module in (PYTHON_MODULE, TYPESCRIPT_MODULE):
            (self.repo / module).parent.mkdir(parents=True)
            shutil.copyfile(REPO_ROOT / module, self.repo / module)
        self.set_versions(VERSION, VERSION)

    def write_checksums(self, digests: dict[str, str]) -> None:
        lines = "".join(f"{digests[name]}  {name}\n" for name in sorted(digests))
        (self.release / "checksums.sha256").write_text(lines, encoding="utf-8")

    def checksums(self) -> dict[str, str]:
        lines = (self.release / "checksums.sha256").read_text(encoding="utf-8").splitlines()
        return {name: digest for digest, name in (line.split("  ", 1) for line in lines)}

    def write_index(self, artifacts: list[str], tag: str = TAG) -> None:
        index = {
            "version": tag.lstrip("v"),
            "tag": tag,
            "artifacts": artifacts,
            "checksums": "checksums.sha256",
        }
        (self.release / "index.json").write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")

    def set_versions(self, python: str, typescript: str) -> None:
        (self.repo / "sdks/python/pyproject.toml").write_text(
            f'[project]\nname = "meerkat-sdk"\nversion = "{python}"\n', encoding="utf-8"
        )
        (self.repo / "sdks/typescript/package.json").write_text(
            json.dumps({"name": "@rkat/sdk", "version": typescript}) + "\n", encoding="utf-8"
        )

    def replace_archive(self, target: str, extension: str, members: dict[str, bytes]) -> None:
        """Swap in a different archive and keep the manifest consistent with it."""
        asset = rpc_asset(target, extension)
        data = archive_bytes(extension, members)
        (self.release / asset).write_bytes(data)
        digests = self.checksums()
        digests[asset] = sha256(data)
        self.write_checksums(digests)

    def expected_table(self) -> dict:
        return {
            "version": VERSION,
            "archives": {
                target: {
                    "asset": rpc_asset(target, extension),
                    "archive_sha256": sha256((self.release / rpc_asset(target, extension)).read_bytes()),
                    "binary_sha256": sha256(binary_for(target)),
                }
                for target, extension in TARGETS
            },
        }

    def stamp(self, tag: str = TAG, *, with_gh: bool = True) -> subprocess.CompletedProcess:
        path = f"{self.bin}{os.pathsep}{os.environ.get('PATH', '')}" if with_gh else str(self.root)
        env = {
            **os.environ,
            "PATH": path,
            "STUB_GH_LOG": str(self.gh_log),
            "STUB_GH_UNATTESTED": " ".join(self.unattested),
        }
        return run(
            "stamp",
            "--release-dir", str(self.release),
            "--release-tag", tag,
            "--repo", REPO,
            "--repo-root", str(self.repo),
            "--table-out", str(self.root / "table.json"),
            env=env,
        )

    def attestation_checks(self) -> list[str]:
        if not self.gh_log.exists():
            return []
        return self.gh_log.read_text(encoding="utf-8").splitlines()

    def python_table(self) -> dict:
        namespace: dict = {}
        exec(compile((self.repo / PYTHON_MODULE).read_text(encoding="utf-8"), "module", "exec"), namespace)
        return namespace["RKAT_RPC_DIGESTS"]

    def typescript_table(self) -> dict:
        text = (self.repo / TYPESCRIPT_MODULE).read_text(encoding="utf-8")
        assert text.count(TS_ASSIGNMENT) == 1 and text.endswith(";\n"), text
        return json.loads(text.split(TS_ASSIGNMENT, 1)[1][: -len(";\n")])

    def modules_unchanged(self) -> bool:
        return all(
            (self.repo / module).read_bytes() == (REPO_ROOT / module).read_bytes()
            for module in (PYTHON_MODULE, TYPESCRIPT_MODULE)
        )


def run(*args: str, env: dict | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(STAMP), *args],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )


class StampTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.fixture = Fixture(Path(self.tmp.name))

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def assert_refused(self, result: subprocess.CompletedProcess, reason: str) -> None:
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(reason, result.stderr)
        self.assertTrue(self.fixture.modules_unchanged(), "a refused stamp writes nothing")
        self.assertFalse((self.fixture.root / "table.json").exists())

    def test_stamps_both_sdks_from_a_complete_release(self) -> None:
        result = self.fixture.stamp()

        self.assertEqual(result.returncode, 0, result.stderr)
        expected = self.fixture.expected_table()
        self.assertEqual(self.fixture.python_table(), expected)
        self.assertEqual(self.fixture.typescript_table(), expected)
        table = json.loads((self.fixture.root / "table.json").read_text(encoding="utf-8"))
        self.assertEqual(table, expected)
        # The binary digest is the binary's inside the verified archive, not
        # the archive's.
        linux = expected["archives"]["x86_64-unknown-linux-gnu"]
        self.assertEqual(linux["binary_sha256"], sha256(binary_for("x86_64-unknown-linux-gnu")))
        self.assertNotEqual(linux["binary_sha256"], linux["archive_sha256"])

    def test_refuses_a_manifest_that_lists_fewer_archives(self) -> None:
        self.fixture.write_index(
            [name for name in all_archive_names() if not name.startswith("rkat-mcp-")]
        )
        self.assert_refused(self.fixture.stamp(), "is not the complete v9.9.9 manifest")

    def test_refuses_checksums_missing_an_archive(self) -> None:
        digests = self.fixture.checksums()
        del digests[f"rkat-rest-{VERSION}-aarch64-apple-darwin.tar.gz"]
        self.fixture.write_checksums(digests)
        self.assert_refused(self.fixture.stamp(), "checksums.sha256 entries mismatch")

    def test_refuses_a_release_without_its_manifest(self) -> None:
        (self.fixture.release / "index.json").unlink()
        self.assert_refused(self.fixture.stamp(), "cannot read index.json")

    def test_refuses_another_tags_manifest(self) -> None:
        self.assert_refused(self.fixture.stamp("v9.9.8"), "is not the complete v9.9.8 manifest")

    def test_refuses_an_archive_that_does_not_rehash(self) -> None:
        asset = rpc_asset("aarch64-apple-darwin", "tar.gz")
        (self.fixture.release / asset).write_bytes(
            archive_bytes("tar.gz", {"rkat-rpc": b"tampered\n"})
        )
        self.assert_refused(self.fixture.stamp(), f"{asset} does not match checksums.sha256")

    def test_refuses_a_missing_archive(self) -> None:
        asset = rpc_asset("x86_64-pc-windows-msvc", "zip")
        (self.fixture.release / asset).unlink()
        self.assert_refused(self.fixture.stamp(), f"{asset} is missing")

    def test_refuses_an_archive_without_one_root_binary(self) -> None:
        for members in (
            {"bin/rkat-rpc": b"nested\n"},
            {"rkat": b"another binary\n"},
            {"rkat-rpc": b"one\n", "docs/rkat-rpc": b"two\n"},
        ):
            with self.subTest(members=sorted(members)):
                self.fixture.replace_archive("x86_64-unknown-linux-gnu", "tar.gz", members)
                self.assert_refused(
                    self.fixture.stamp(), "must hold exactly one regular file 'rkat-rpc' at its root"
                )
        linux = "x86_64-unknown-linux-gnu"
        self.fixture.replace_archive(linux, "tar.gz", {"rkat-rpc": binary_for(linux)})
        self.fixture.replace_archive("x86_64-pc-windows-msvc", "zip", {"rkat-rpc": b"no .exe\n"})
        self.assert_refused(
            self.fixture.stamp(), "must hold exactly one regular file 'rkat-rpc.exe' at its root"
        )

    def test_verifies_every_archive_attestation_against_the_release_workflow(self) -> None:
        self.assertEqual(self.fixture.stamp().returncode, 0)
        self.assertEqual(
            sorted(self.fixture.attestation_checks()),
            sorted(
                f"attestation verify {self.fixture.release / rpc_asset(target, extension)} "
                f"--repo {REPO} --signer-workflow {SIGNER}"
                for target, extension in TARGETS
            ),
        )

    def test_refuses_an_archive_without_a_build_attestation(self) -> None:
        asset = rpc_asset("aarch64-apple-darwin", "tar.gz")
        self.fixture.unattested = [asset]
        self.assert_refused(
            self.fixture.stamp(),
            f"{asset} has no build attestation from {REPO}'s release workflow: Error: HTTP 404",
        )

    def test_refuses_without_the_github_cli(self) -> None:
        self.assert_refused(self.fixture.stamp(with_gh=False), "the GitHub CLI (gh) is required")

    def test_refuses_an_archive_over_the_sdk_download_cap(self) -> None:
        asset = rpc_asset("x86_64-unknown-linux-gnu", "tar.gz")
        with (self.fixture.release / asset).open("wb") as sparse:
            sparse.truncate(256 * 1024 * 1024 + 1)
        self.assert_refused(self.fixture.stamp(), f"{asset} is larger than 256 MiB")
        self.assertEqual(self.fixture.attestation_checks(), [], "refused before any check")

    def test_refuses_sdk_packages_of_another_version(self) -> None:
        self.fixture.set_versions("9.9.8", VERSION)
        self.assert_refused(self.fixture.stamp(), "sdks/python/pyproject.toml is version '9.9.8'")
        self.fixture.set_versions(VERSION, "9.9.8")
        self.assert_refused(self.fixture.stamp(), "sdks/typescript/package.json is version '9.9.8'")


class VerifyTests(unittest.TestCase):
    """`verify` is the publish steps' gate on each built package."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        fixture = Fixture(self.root)
        result = fixture.stamp()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.expected_path = self.root / "table.json"
        self.table = json.loads(self.expected_path.read_text(encoding="utf-8"))

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def verify(self, contract_version: str, digests: object) -> subprocess.CompletedProcess:
        actual = self.root / "actual.json"
        actual.write_text(
            json.dumps({"contract_version": contract_version, "digests": digests}),
            encoding="utf-8",
        )
        return run(
            "verify",
            "--release-tag", TAG,
            "--expected", str(self.expected_path),
            "--actual", str(actual),
        )

    def assert_refused(self, result: subprocess.CompletedProcess, reason: str) -> None:
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(reason, result.stderr)

    def test_accepts_a_package_that_pins_the_stamped_table(self) -> None:
        result = self.verify(VERSION, self.table)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_refuses_an_unstamped_package(self) -> None:
        self.assert_refused(self.verify(VERSION, {"version": "", "archives": {}}), "is unstamped")

    def test_refuses_a_package_stamped_for_another_version(self) -> None:
        other = dict(self.table, version="9.9.8")
        self.assert_refused(self.verify(VERSION, other), "is stamped for '9.9.8', not '9.9.9'")

    def test_refuses_a_package_of_another_version(self) -> None:
        self.assert_refused(self.verify("9.9.8", self.table), "CONTRACT_VERSION '9.9.8'")

    def test_refuses_a_package_missing_a_target(self) -> None:
        archives = dict(self.table["archives"])
        del archives["x86_64-apple-darwin"]
        self.assert_refused(
            self.verify(VERSION, dict(self.table, archives=archives)), "targets mismatch"
        )

    def test_refuses_a_package_with_a_different_digest(self) -> None:
        archives = json.loads(json.dumps(self.table["archives"]))
        archives["aarch64-unknown-linux-gnu"]["binary_sha256"] = "0" * 64
        self.assert_refused(
            self.verify(VERSION, dict(self.table, archives=archives)), "differ from the ones stamped"
        )


class UnstampedTests(unittest.TestCase):
    def test_the_committed_modules_pin_nothing(self) -> None:
        result = run("check-unstamped", "--repo-root", str(REPO_ROOT))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_stamped_module_is_not_the_unstamped_rendering(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Fixture(Path(tmp))
            self.assertEqual(fixture.stamp().returncode, 0)
            result = run("check-unstamped", "--repo-root", str(fixture.repo))
        self.assertEqual(result.returncode, 1)
        self.assertIn("is not the unstamped rendering", result.stderr)



# `gh release view ... --jq ...` and `gh api ... --jq ...` for the wait
# script: the Nth call of a kind prints ${STUB_DIR}/<kind>.N, else
# <kind>.last; with neither it fails like an API error.
GH_MANIFEST_STUB = """#!/bin/sh
case "$1 $2" in
  "release view") kind=release ;;
  "api --paginate") kind=api ;;
  *) echo "stub gh: unsupported $*" >&2; exit 2 ;;
esac
calls=$(( $(cat "$STUB_DIR/$kind.calls" 2>/dev/null || echo 0) + 1 ))
echo "$calls" > "$STUB_DIR/$kind.calls"
for answer in "$STUB_DIR/$kind.$calls" "$STUB_DIR/$kind.last"; do
  [ -f "$answer" ] && { cat "$answer"; exit 0; }
done
echo "HTTP 502: Bad Gateway" >&2
exit 1
"""
ARCHIVES_ONLY = "\n".join(all_archive_names()) + "\n"
COMPLETE = ARCHIVES_ONLY + "checksums.sha256\nindex.json\n"


class AwaitManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "bin").mkdir()
        (self.root / "bin" / "gh").write_text(GH_MANIFEST_STUB, encoding="utf-8")
        (self.root / "bin" / "gh").chmod(0o755)

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def answer(self, name: str, text: str) -> None:
        (self.root / name).write_text(text, encoding="utf-8")

    def calls(self, kind: str) -> int:
        path = self.root / f"{kind}.calls"
        return int(path.read_text(encoding="utf-8")) if path.exists() else 0

    def wait(self, *, tag_run: bool = True, deadline: int = 9000) -> subprocess.CompletedProcess:
        env = {
            **os.environ,
            "PATH": f"{self.root / 'bin'}{os.pathsep}{os.environ.get('PATH', '')}",
            "STUB_DIR": str(self.root),
            "GITHUB_REPOSITORY": REPO,
            "GITHUB_RUN_ID": "1234",
            "GITHUB_RUN_ATTEMPT": "1",
            "WAIT_FOR_MANIFEST": "true" if tag_run else "false",
            "RELEASE_MANIFEST_DEADLINE_SECONDS": str(deadline),
            "RELEASE_MANIFEST_POLL_SECONDS": "0",
        }
        return subprocess.run(
            [str(AWAIT), TAG], capture_output=True, text=True, check=False, env=env, timeout=60
        )

    def assert_waited(self, result: subprocess.CompletedProcess, rc: int, text: str) -> None:
        self.assertEqual(result.returncode, rc, result.stdout + result.stderr)
        self.assertIn(text, result.stdout + result.stderr)

    def test_proceeds_once_the_manifest_is_published(self) -> None:
        self.answer("release.1", COMPLETE)
        self.assert_waited(self.wait(), 0, "carries its complete binary manifest")
        self.assertEqual(self.calls("api"), 0, "no job lookup when the manifest is there")

    def test_a_package_recovery_needs_the_manifest_already_published(self) -> None:
        self.answer("release.last", ARCHIVES_ONLY)
        self.assert_waited(self.wait(tag_run=False), 1, "publish its assets first")
        self.assertEqual(self.calls("release"), 1)

    def test_fails_at_once_when_the_trailing_publisher_failed(self) -> None:
        self.answer("release.last", ARCHIVES_ONLY)
        self.answer("api.last", "failure\n")
        self.assert_waited(self.wait(), 1, "Publish GitHub release assets failure")
        self.assertEqual(self.calls("release"), 1)

    def test_a_failed_api_call_counts_as_not_yet(self) -> None:
        # No release yet and no job listing: both calls fail, then the
        # manifest appears.
        self.answer("release.3", COMPLETE)
        self.assert_waited(self.wait(), 0, "carries its complete binary manifest")
        self.assertEqual(self.calls("release"), 3)
        self.assertEqual(self.calls("api"), 2)

    def test_waits_while_the_publisher_runs_or_has_just_succeeded(self) -> None:
        self.answer("release.1", ARCHIVES_ONLY)
        self.answer("api.1", "pending\n")
        self.answer("release.2", ARCHIVES_ONLY)
        self.answer("api.2", "success\n")
        self.answer("release.3", COMPLETE)
        self.assert_waited(self.wait(), 0, "carries its complete binary manifest")

    def test_gives_up_at_the_deadline(self) -> None:
        self.answer("release.last", ARCHIVES_ONLY)
        self.answer("api.last", "pending\n")
        self.assert_waited(self.wait(deadline=0), 1, f"no complete {TAG} manifest after 0 minutes")


if __name__ == "__main__":
    unittest.main()
