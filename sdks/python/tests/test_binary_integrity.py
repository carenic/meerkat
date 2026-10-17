"""The auto-downloaded rkat-rpc runs only when it matches its release digests.

The package pins the sha256 of every release archive it may download and of
the binary inside it (``meerkat._rkat_rpc_digests``, stamped by the release
workflow). Nothing here touches the network: the release download is
monkeypatched, and the redirect tests serve canned responses through urllib.
"""

from __future__ import annotations

import email.message
import hashlib
import http.client
import io
import os
import tarfile
import urllib.request
import urllib.response
import zipfile
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

import meerkat
import meerkat.client as client_module
from meerkat import _rkat_rpc_digests
from meerkat.client import MeerkatClient
from meerkat.errors import BinaryIntegrityError, MeerkatError
from meerkat.generated.types import CONTRACT_VERSION

LINUX = ("x86_64-unknown-linux-gnu", "tar.gz", "rkat-rpc")
WINDOWS = ("x86_64-pc-windows-msvc", "zip", "rkat-rpc.exe")
BINARY = b"#!/bin/sh\necho genuine rkat-rpc\n"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def tar_gz(name: str, data: bytes) -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
        member = tarfile.TarInfo(name)
        member.size = len(data)
        member.mode = 0o755
        archive.addfile(member, io.BytesIO(data))
    return buffer.getvalue()


def zip_archive(name: str, data: bytes) -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr(name, data)
    return buffer.getvalue()


def archive_for(platform: tuple[str, str, str], data: bytes) -> bytes:
    _target, ext, binary_name = platform
    return tar_gz(binary_name, data) if ext == "tar.gz" else zip_archive(binary_name, data)


def asset_name(platform: tuple[str, str, str]) -> str:
    target, ext, _binary_name = platform
    return f"rkat-rpc-{CONTRACT_VERSION}-{target}.{ext}"


def stamped(platform, archive: bytes, binary: bytes, version=CONTRACT_VERSION):
    return {
        "version": version,
        "archives": {
            platform[0]: {
                "asset": asset_name(platform),
                "archive_sha256": sha256(archive),
                "binary_sha256": sha256(binary),
            }
        },
    }


class Release:
    """One platform's release asset, served by a fake download."""

    def __init__(self, monkeypatch, tmp_path: Path, platform=LINUX):
        self.platform = platform
        self.archive = archive_for(platform, BINARY)
        self.served = self.archive
        self.urls: list[str] = []
        self.cache_root = tmp_path / "cache"
        monkeypatch.setattr(client_module, "_MEERKAT_BINARY_CACHE_ROOT", self.cache_root)
        monkeypatch.setattr(
            MeerkatClient,
            "_platform_target",
            staticmethod(lambda system=None, machine=None: platform),
        )
        monkeypatch.setattr(
            _rkat_rpc_digests,
            "RKAT_RPC_DIGESTS",
            stamped(platform, self.archive, BINARY),
        )
        monkeypatch.setattr(
            MeerkatClient, "_fetch_rkat_rpc_archive", staticmethod(self._fetch)
        )

    def _fetch(self, url: str) -> bytes:
        self.urls.append(url)
        return self.served

    @property
    def cache_dir(self) -> Path:
        return self.cache_root / f"v{CONTRACT_VERSION}" / self.platform[0]

    @property
    def cached(self) -> Path:
        return self.cache_dir / self.platform[2]

    @property
    def url(self) -> str:
        return (
            "https://github.com/lukacf/meerkat/releases/download/"
            f"v{CONTRACT_VERSION}/{asset_name(self.platform)}"
        )

    def cache_entries(self) -> list[str]:
        if not self.cache_dir.exists():
            return []
        return sorted(path.name for path in self.cache_dir.iterdir())


@pytest.fixture
def release(monkeypatch, tmp_path):
    monkeypatch.delenv("MEERKAT_BIN_PATH", raising=False)
    return Release(monkeypatch, tmp_path)


@pytest.mark.asyncio
async def test_matching_digests_install_the_binary_and_the_cache_is_reused(release):
    path = await MeerkatClient._download_rkat_rpc_binary()

    assert path == str(release.cached)
    assert release.cached.read_bytes() == BINARY
    if os.name != "nt":
        assert os.access(release.cached, os.X_OK)
    assert release.urls == [release.url]
    assert release.cache_entries() == ["rkat-rpc"], "no staging leftovers"

    again = await MeerkatClient._download_rkat_rpc_binary()

    assert again == str(release.cached)
    assert release.urls == [release.url], "a verified cache hit makes no request"


@pytest.mark.asyncio
async def test_a_zip_release_is_verified_and_installed(monkeypatch, tmp_path):
    monkeypatch.delenv("MEERKAT_BIN_PATH", raising=False)
    release = Release(monkeypatch, tmp_path, WINDOWS)

    path = await MeerkatClient._download_rkat_rpc_binary()

    assert path == str(release.cached)
    assert Path(path).name == "rkat-rpc.exe"
    assert release.cached.read_bytes() == BINARY


@pytest.mark.asyncio
async def test_a_tampered_archive_is_refused_before_anything_is_extracted(release):
    release.served = archive_for(release.platform, b"#!/bin/sh\necho tampered\n")

    def no_extraction(*_args, **_kwargs):
        raise AssertionError("a refused archive must not be opened")

    with patch.object(client_module.tarfile, "open", no_extraction), patch.object(
        client_module.zipfile, "ZipFile", no_extraction
    ):
        with pytest.raises(BinaryIntegrityError) as excinfo:
            await MeerkatClient._download_rkat_rpc_binary()

    error = excinfo.value
    assert isinstance(error, MeerkatError)
    assert error.code == "BINARY_INTEGRITY_MISMATCH"
    assert error.asset == asset_name(release.platform)
    assert error.expected_sha256 == sha256(release.archive)
    assert error.actual_sha256 == sha256(release.served)
    assert error.path is None
    assert error.details == {
        "asset": error.asset,
        "path": None,
        "expected_sha256": error.expected_sha256,
        "actual_sha256": error.actual_sha256,
    }
    assert release.cache_entries() == []


@pytest.mark.asyncio
async def test_a_truncated_download_is_refused(release):
    release.served = release.archive[: len(release.archive) // 2]

    with pytest.raises(BinaryIntegrityError) as excinfo:
        await MeerkatClient._download_rkat_rpc_binary()

    assert excinfo.value.actual_sha256 == sha256(release.served)
    assert release.cache_entries() == []


@pytest.mark.asyncio
async def test_a_binary_that_does_not_match_its_pinned_digest_is_not_installed(
    release, monkeypatch
):
    # The archive matches its pin, but the binary inside it does not match
    # the binary pin: refuse rather than trust either digest alone.
    table = stamped(release.platform, release.archive, b"some other binary")
    monkeypatch.setattr(_rkat_rpc_digests, "RKAT_RPC_DIGESTS", table)

    with pytest.raises(BinaryIntegrityError) as excinfo:
        await MeerkatClient._download_rkat_rpc_binary()

    assert excinfo.value.expected_sha256 == sha256(b"some other binary")
    assert excinfo.value.actual_sha256 == sha256(BINARY)
    assert release.cache_entries() == []


@pytest.mark.asyncio
async def test_a_tampered_cached_binary_is_deleted_and_refused_then_redownloaded(
    release,
):
    release.cache_dir.mkdir(parents=True)
    release.cached.write_bytes(b"#!/bin/sh\necho tampered\n")

    with pytest.raises(BinaryIntegrityError) as excinfo:
        await MeerkatClient._download_rkat_rpc_binary()

    error = excinfo.value
    assert error.code == "BINARY_INTEGRITY_MISMATCH"
    assert error.path == str(release.cached)
    assert error.expected_sha256 == sha256(BINARY)
    assert error.actual_sha256 == sha256(b"#!/bin/sh\necho tampered\n")
    assert not release.cached.exists(), "the tampered binary is removed"
    assert release.urls == [], "a refused cache entry is not replaced in the same call"

    path = await MeerkatClient._download_rkat_rpc_binary()

    assert path == str(release.cached)
    assert release.cached.read_bytes() == BINARY
    assert release.urls == [release.url]


@pytest.mark.asyncio
async def test_no_pinned_digest_refuses_without_a_request(release, monkeypatch):
    monkeypatch.setattr(
        _rkat_rpc_digests, "RKAT_RPC_DIGESTS", {"version": "", "archives": {}}
    )

    with pytest.raises(MeerkatError) as excinfo:
        await MeerkatClient._download_rkat_rpc_binary()

    assert not isinstance(excinfo.value, BinaryIntegrityError)
    assert excinfo.value.code == "BINARY_DIGEST_UNAVAILABLE"
    assert "MEERKAT_BIN_PATH" in excinfo.value.message
    assert "rkat_path" in excinfo.value.message
    assert release.url in excinfo.value.message, "the refusal names the exact asset"
    assert release.urls == []


@pytest.mark.asyncio
async def test_a_cached_binary_is_not_run_without_a_pinned_digest(release, monkeypatch):
    release.cache_dir.mkdir(parents=True)
    release.cached.write_bytes(BINARY)
    monkeypatch.setattr(
        _rkat_rpc_digests, "RKAT_RPC_DIGESTS", {"version": "", "archives": {}}
    )

    with pytest.raises(MeerkatError) as excinfo:
        await MeerkatClient._download_rkat_rpc_binary()

    assert excinfo.value.code == "BINARY_DIGEST_UNAVAILABLE"
    assert release.urls == []


@pytest.mark.parametrize(
    "table",
    [
        pytest.param(
            {"version": "0.0.1", "archives": {}}, id="digests-for-another-version"
        ),
        pytest.param(
            {
                "version": CONTRACT_VERSION,
                "archives": {
                    "aarch64-apple-darwin": {
                        "asset": f"rkat-rpc-{CONTRACT_VERSION}-aarch64-apple-darwin.tar.gz",
                        "archive_sha256": "0" * 64,
                        "binary_sha256": "1" * 64,
                    }
                },
            },
            id="no-entry-for-this-target",
        ),
        pytest.param(
            {
                "version": CONTRACT_VERSION,
                "archives": {
                    LINUX[0]: {
                        "asset": "rkat-rpc-0.0.1-x86_64-unknown-linux-gnu.tar.gz",
                        "archive_sha256": "0" * 64,
                        "binary_sha256": "1" * 64,
                    }
                },
            },
            id="entry-names-another-asset",
        ),
        pytest.param(
            {
                "version": CONTRACT_VERSION,
                "archives": {
                    LINUX[0]: {
                        "asset": asset_name(LINUX),
                        "archive_sha256": "not-a-digest",
                        "binary_sha256": "1" * 64,
                    }
                },
            },
            id="malformed-digest",
        ),
    ],
)
@pytest.mark.asyncio
async def test_digests_that_do_not_pin_this_asset_refuse(release, monkeypatch, table):
    monkeypatch.setattr(_rkat_rpc_digests, "RKAT_RPC_DIGESTS", table)

    with pytest.raises(MeerkatError) as excinfo:
        await MeerkatClient._download_rkat_rpc_binary()

    assert excinfo.value.code == "BINARY_DIGEST_UNAVAILABLE"
    assert release.urls == []


def test_a_source_tree_pins_nothing():
    # Only the release workflow stamps the digests; a source or git install
    # must fail closed instead of downloading an unverifiable binary.
    assert _rkat_rpc_digests.RKAT_RPC_DIGESTS == {"version": "", "archives": {}}


@pytest.mark.asyncio
async def test_an_explicit_binary_path_is_used_as_is(monkeypatch, tmp_path):
    monkeypatch.delenv("MEERKAT_BIN_PATH", raising=False)
    own = tmp_path / "my-rkat-rpc"
    own.write_text("#!/bin/sh\n")
    download = AsyncMock(side_effect=AssertionError("must not download"))

    with patch.object(MeerkatClient, "_download_rkat_rpc_binary", new=download):
        path, legacy = await MeerkatClient()._resolve_rkat_binary(str(own))
        monkeypatch.setenv("MEERKAT_BIN_PATH", str(own))
        env_path, env_legacy = await MeerkatClient()._resolve_rkat_binary("rkat-rpc")

    assert (path, legacy) == (str(own), False)
    assert (env_path, env_legacy) == (str(own), False)
    download.assert_not_awaited()


def _which_only_legacy(command: str) -> str | None:
    return "/usr/local/bin/rkat" if command == "rkat" else None


@pytest.mark.asyncio
async def test_an_integrity_failure_never_falls_back_to_legacy_rkat(monkeypatch):
    monkeypatch.delenv("MEERKAT_BIN_PATH", raising=False)
    refusal = BinaryIntegrityError(
        "tampered",
        asset="rkat-rpc.tar.gz",
        path=None,
        expected_sha256="0" * 64,
        actual_sha256="1" * 64,
    )

    with patch("meerkat.client.shutil.which", side_effect=_which_only_legacy), patch.object(
        MeerkatClient,
        "_download_rkat_rpc_binary",
        new=AsyncMock(side_effect=refusal),
    ):
        with pytest.raises(BinaryIntegrityError):
            await MeerkatClient()._resolve_rkat_binary("rkat-rpc")


@pytest.mark.asyncio
async def test_no_pinned_digest_keeps_the_legacy_fallback(monkeypatch):
    monkeypatch.delenv("MEERKAT_BIN_PATH", raising=False)
    unavailable = MeerkatError("BINARY_DIGEST_UNAVAILABLE", "no pinned digest")

    with patch("meerkat.client.shutil.which", side_effect=_which_only_legacy), patch.object(
        MeerkatClient,
        "_download_rkat_rpc_binary",
        new=AsyncMock(side_effect=unavailable),
    ):
        path, legacy = await MeerkatClient()._resolve_rkat_binary("rkat-rpc")

    assert (path, legacy) == ("rkat", True)


def test_binary_integrity_error_is_exported():
    assert meerkat.BinaryIntegrityError is BinaryIntegrityError
    assert "BinaryIntegrityError" in meerkat.__all__
    assert issubclass(BinaryIntegrityError, MeerkatError)


# -- The download itself: https only, redirects included ----------------------


class _CannedHttps:
    """Serve canned responses for https requests; any http request fails."""

    def __init__(self, monkeypatch, responses):
        self.responses = dict(responses)
        self.requested: list[str] = []
        canned = self
        for proxy in ("https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"):
            monkeypatch.delenv(proxy, raising=False)

        def https_open(_handler, request):
            url = request.full_url
            canned.requested.append(url)
            status, location, body, *extra = canned.responses[url]
            headers = email.message.Message()
            if location is not None:
                headers["Location"] = location
            for name, value in (extra[0] if extra else {}).items():
                headers[name] = value
            source = io.BytesIO(body) if isinstance(body, bytes) else body
            response = urllib.response.addinfourl(source, headers, url, status)
            response.msg = "Found" if location is not None else "OK"
            return response

        def http_open(_handler, request):
            canned.requested.append(request.full_url)
            raise AssertionError(f"plain http request made: {request.full_url}")

        monkeypatch.setattr(urllib.request.HTTPSHandler, "https_open", https_open)
        monkeypatch.setattr(urllib.request.HTTPHandler, "http_open", http_open)


RELEASE_URL = (
    "https://github.com/lukacf/meerkat/releases/download/v0.0.1/"
    "rkat-rpc-0.0.1-x86_64-unknown-linux-gnu.tar.gz"
)


def test_the_download_follows_an_https_redirect(monkeypatch):
    cdn = "https://release-assets.example/rkat-rpc.tar.gz"
    canned = _CannedHttps(
        monkeypatch,
        {RELEASE_URL: (302, cdn, b""), cdn: (200, None, b"archive bytes")},
    )

    assert MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL) == b"archive bytes"
    assert canned.requested == [RELEASE_URL, cdn]


def test_the_download_refuses_a_redirect_to_plain_http(monkeypatch):
    canned = _CannedHttps(
        monkeypatch,
        {RELEASE_URL: (302, "http://release-assets.example/rkat-rpc.tar.gz", b"")},
    )

    with pytest.raises(MeerkatError) as excinfo:
        MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL)

    assert excinfo.value.code == "BINARY_DOWNLOAD_FAILED"
    assert "http://release-assets.example" in excinfo.value.message
    assert canned.requested == [RELEASE_URL]


def test_the_download_refuses_a_plain_http_url(monkeypatch):
    canned = _CannedHttps(monkeypatch, {})

    with pytest.raises(MeerkatError) as excinfo:
        MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL.replace("https:", "http:"))

    assert excinfo.value.code == "BINARY_DOWNLOAD_FAILED"
    assert canned.requested == []


class _CutOffBody(io.RawIOBase):
    """A body whose connection drops: the read fails like http.client's."""

    def read(self, size=-1):
        raise http.client.IncompleteRead(b"partial", 100)


class _UnreadableBody(io.RawIOBase):
    def read(self, size=-1):
        raise AssertionError("an oversized archive must be refused before it is read")


def test_a_dropped_connection_is_a_download_failure_not_tamper(monkeypatch):
    _CannedHttps(monkeypatch, {RELEASE_URL: (200, None, _CutOffBody())})

    with pytest.raises(MeerkatError) as excinfo:
        MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL)

    assert not isinstance(excinfo.value, BinaryIntegrityError)
    assert excinfo.value.code == "BINARY_DOWNLOAD_FAILED"


def test_a_body_shorter_than_its_declared_length_is_a_download_failure(monkeypatch):
    # The connection closed early without the HTTP layer raising: still a
    # transport fault, never a digest mismatch.
    _CannedHttps(
        monkeypatch,
        {RELEASE_URL: (200, None, b"x" * 10, {"Content-Length": "100"})},
    )

    with pytest.raises(MeerkatError) as excinfo:
        MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL)

    assert not isinstance(excinfo.value, BinaryIntegrityError)
    assert excinfo.value.code == "BINARY_DOWNLOAD_FAILED"
    assert "10 of 100" in excinfo.value.message


class _RecordingBody(io.RawIOBase):
    """A body that records the size of every read it serves."""

    def __init__(self, data: bytes):
        self.data = data
        self.sizes: list[int] = []

    def read(self, size=-1):
        self.sizes.append(size)
        if size is None or size < 0:
            chunk, self.data = self.data, b""
        else:
            chunk, self.data = self.data[:size], self.data[size:]
        return chunk


@pytest.mark.parametrize(
    "declared",
    ["\u00b2", "1\u00b2", "\u0661\u0660", "1" * 5000, "0" * 4999 + "13"],
    ids=["superscript", "digit-superscript", "arabic-indic", "5000-digits", "5001-digits"],
)
def test_an_unreadable_content_length_declares_no_length(monkeypatch, declared):
    # str.isdigit() admits all of these: int() refuses the superscript, reads
    # the Arabic-Indic digits as 10, and refuses more than 4300 digits. The
    # header is ignored and the body is read under the cap, as when no length
    # is declared.
    _CannedHttps(
        monkeypatch,
        {RELEASE_URL: (200, None, b"archive bytes", {"Content-Length": declared})},
    )

    assert MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL) == b"archive bytes"


@pytest.mark.parametrize("headers", [{"Content-Length": "13"}, {}])
def test_every_read_is_bounded_at_one_byte_past_the_cap(monkeypatch, headers):
    monkeypatch.setattr(client_module, "_RKAT_RPC_ARCHIVE_MAX_BYTES", 16)
    body = _RecordingBody(b"archive bytes")
    _CannedHttps(monkeypatch, {RELEASE_URL: (200, None, body, headers)})

    assert MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL) == b"archive bytes"
    assert body.sizes == [17]


def test_an_archive_declared_larger_than_the_cap_is_refused_unread(monkeypatch):
    too_large = str(256 * 1024 * 1024 + 1)
    _CannedHttps(
        monkeypatch,
        {RELEASE_URL: (200, None, _UnreadableBody(), {"Content-Length": too_large})},
    )

    with pytest.raises(MeerkatError) as excinfo:
        MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL)

    assert excinfo.value.code == "BINARY_DOWNLOAD_TOO_LARGE"
    assert "256 MiB" in excinfo.value.message


def test_an_archive_that_streams_past_the_cap_is_refused(monkeypatch):
    monkeypatch.setattr(client_module, "_RKAT_RPC_ARCHIVE_MAX_BYTES", 16)
    _CannedHttps(monkeypatch, {RELEASE_URL: (200, None, b"x" * 17)})

    with pytest.raises(MeerkatError) as excinfo:
        MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL)

    assert excinfo.value.code == "BINARY_DOWNLOAD_TOO_LARGE"


def test_an_archive_at_the_cap_is_accepted(monkeypatch):
    monkeypatch.setattr(client_module, "_RKAT_RPC_ARCHIVE_MAX_BYTES", 16)
    _CannedHttps(monkeypatch, {RELEASE_URL: (200, None, b"x" * 16, {"Content-Length": "16"})})

    assert MeerkatClient._fetch_rkat_rpc_archive(RELEASE_URL) == b"x" * 16
