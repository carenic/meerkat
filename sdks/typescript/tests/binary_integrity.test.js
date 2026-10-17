// The auto-downloaded rkat-rpc runs only when it matches its release digests.
//
// The package pins the sha256 of every release archive it may download and of
// the binary inside it (src/rkat-rpc-digests.ts, stamped by the release
// workflow). Nothing here touches the network: `fetch` is replaced with canned
// responses and the cache lives in a temporary directory.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, it } from "node:test";

import { MeerkatClient } from "../dist/client.js";
import { BinaryIntegrityError, MeerkatError } from "../dist/index.js";
import { CONTRACT_VERSION } from "../dist/generated/types.js";

const LINUX = { target: "x86_64-unknown-linux-gnu", archiveExt: "tar.gz", binaryName: "rkat-rpc" };
const BINARY = Buffer.from("#!/bin/sh\necho genuine rkat-rpc\n");
const ASSET = `rkat-rpc-${CONTRACT_VERSION}-${LINUX.target}.tar.gz`;
const RELEASE_URL = `https://github.com/lukacf/meerkat/releases/download/v${CONTRACT_VERSION}/${ASSET}`;
const UNSTAMPED = { version: "", archives: {} };

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

function tarGz(root, binary) {
  const source = mkdtempSync(path.join(root, "archive-source-"));
  writeFileSync(path.join(source, LINUX.binaryName), binary, { mode: 0o755 });
  const archivePath = path.join(root, `archive-${sha256(binary).slice(0, 12)}.tar.gz`);
  execFileSync("tar", ["-czf", archivePath, "-C", source, LINUX.binaryName]);
  rmSync(source, { recursive: true, force: true });
  return readFileSync(archivePath);
}

function stamped(archive, binary, version = CONTRACT_VERSION) {
  return {
    version,
    archives: {
      [LINUX.target]: {
        asset: ASSET,
        archive_sha256: sha256(archive),
        binary_sha256: sha256(binary),
      },
    },
  };
}

const saved = {};
const SEAMS = [
  "platformTarget",
  "binaryCacheRoot",
  "pinnedDigests",
  "runCommand",
  "commandPath",
  "ensureDownloadedBinary",
  "releaseArchiveMaxBytes",
];
let root;
let release;

beforeEach(() => {
  for (const seam of SEAMS) saved[seam] = MeerkatClient[seam];
  saved.fetch = globalThis.fetch;
  saved.binPath = process.env.MEERKAT_BIN_PATH;
  delete process.env.MEERKAT_BIN_PATH;
  root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-integrity-"));
  const archive = tarGz(root, BINARY);
  release = {
    archive,
    served: archive,
    requests: [],
    table: stamped(archive, BINARY),
    cacheRoot: path.join(root, "cache"),
    extractions: 0,
  };
  release.cacheDir = path.join(release.cacheRoot, `v${CONTRACT_VERSION}`, LINUX.target);
  release.cached = path.join(release.cacheDir, LINUX.binaryName);
  MeerkatClient.platformTarget = () => LINUX;
  MeerkatClient.binaryCacheRoot = () => release.cacheRoot;
  MeerkatClient.pinnedDigests = () => release.table;
  const runCommand = saved.runCommand;
  MeerkatClient.runCommand = async (command, args) => {
    release.extractions += 1;
    return runCommand.call(MeerkatClient, command, args);
  };
  globalThis.fetch = async (url, init) => {
    release.requests.push({ url: String(url), redirect: init?.redirect });
    return new Response(release.served, { status: 200 });
  };
});

afterEach(() => {
  for (const seam of SEAMS) MeerkatClient[seam] = saved[seam];
  globalThis.fetch = saved.fetch;
  if (saved.binPath === undefined) delete process.env.MEERKAT_BIN_PATH;
  else process.env.MEERKAT_BIN_PATH = saved.binPath;
  rmSync(root, { recursive: true, force: true });
});

const cacheEntries = () => (existsSync(release.cacheDir) ? readdirSync(release.cacheDir).sort() : []);

describe("rkat-rpc auto-download integrity", () => {
  it("installs a binary whose digests match and reuses the verified cache", async () => {
    const installed = await MeerkatClient.ensureDownloadedBinary();

    assert.equal(installed, release.cached);
    assert.deepEqual(readFileSync(installed), BINARY);
    if (process.platform !== "win32") assert.ok(statSync(installed).mode & 0o100, "executable");
    assert.deepEqual(release.requests, [{ url: RELEASE_URL, redirect: "manual" }]);
    assert.deepEqual(cacheEntries(), ["rkat-rpc"], "no staging leftovers");

    assert.equal(await MeerkatClient.ensureDownloadedBinary(), release.cached);
    assert.equal(release.requests.length, 1, "a verified cache hit makes no request");
  });

  it("refuses a tampered archive before anything is written or extracted", async () => {
    release.served = tarGz(root, Buffer.from("#!/bin/sh\necho tampered\n"));

    await assert.rejects(MeerkatClient.ensureDownloadedBinary(), (error) => {
      assert.ok(error instanceof BinaryIntegrityError);
      assert.ok(error instanceof MeerkatError);
      assert.equal(error.code, "BINARY_INTEGRITY_MISMATCH");
      assert.equal(error.asset, ASSET);
      assert.equal(error.path, null);
      assert.equal(error.expectedSha256, sha256(release.archive));
      assert.equal(error.actualSha256, sha256(release.served));
      assert.deepEqual(error.details, {
        asset: ASSET,
        path: null,
        expected_sha256: sha256(release.archive),
        actual_sha256: sha256(release.served),
      });
      return true;
    });
    assert.equal(release.extractions, 0, "a refused archive is never extracted");
    assert.deepEqual(cacheEntries(), []);
  });

  it("refuses a truncated download", async () => {
    release.served = release.archive.subarray(0, Math.floor(release.archive.length / 2));

    await assert.rejects(MeerkatClient.ensureDownloadedBinary(), (error) => {
      assert.ok(error instanceof BinaryIntegrityError);
      assert.equal(error.actualSha256, sha256(release.served));
      return true;
    });
    assert.equal(release.extractions, 0);
    assert.deepEqual(cacheEntries(), []);
  });

  it("does not install a binary that does not match its pinned digest", async () => {
    // The archive matches its pin, but the binary inside it does not match
    // the binary pin: refuse rather than trust either digest alone.
    release.table = stamped(release.archive, Buffer.from("some other binary"));

    await assert.rejects(MeerkatClient.ensureDownloadedBinary(), (error) => {
      assert.ok(error instanceof BinaryIntegrityError);
      assert.equal(error.expectedSha256, sha256(Buffer.from("some other binary")));
      assert.equal(error.actualSha256, sha256(BINARY));
      return true;
    });
    assert.deepEqual(cacheEntries(), []);
  });

  it("deletes and refuses a tampered cached binary, then downloads a verified one", async () => {
    const tampered = Buffer.from("#!/bin/sh\necho tampered\n");
    mkdirSync(release.cacheDir, { recursive: true });
    writeFileSync(release.cached, tampered);

    await assert.rejects(MeerkatClient.ensureDownloadedBinary(), (error) => {
      assert.ok(error instanceof BinaryIntegrityError);
      assert.equal(error.code, "BINARY_INTEGRITY_MISMATCH");
      assert.equal(error.path, release.cached);
      assert.equal(error.expectedSha256, sha256(BINARY));
      assert.equal(error.actualSha256, sha256(tampered));
      return true;
    });
    assert.equal(existsSync(release.cached), false, "the tampered binary is removed");
    assert.deepEqual(release.requests, [], "a refused cache entry is not replaced in the same call");

    assert.equal(await MeerkatClient.ensureDownloadedBinary(), release.cached);
    assert.deepEqual(readFileSync(release.cached), BINARY);
    assert.equal(release.requests.length, 1);
  });

  it("refuses without a request when the package pins no digest", async () => {
    release.table = UNSTAMPED;

    await assert.rejects(MeerkatClient.ensureDownloadedBinary(), (error) => {
      assert.ok(error instanceof MeerkatError);
      assert.ok(!(error instanceof BinaryIntegrityError));
      assert.equal(error.code, "BINARY_DIGEST_UNAVAILABLE");
      assert.match(error.message, /MEERKAT_BIN_PATH/);
      assert.ok(error.message.includes(RELEASE_URL), "the refusal names the exact asset");
      return true;
    });
    assert.deepEqual(release.requests, []);
  });

  it("does not run a cached binary when the package pins no digest", async () => {
    mkdirSync(release.cacheDir, { recursive: true });
    writeFileSync(release.cached, BINARY);
    release.table = UNSTAMPED;

    await assert.rejects(MeerkatClient.ensureDownloadedBinary(), { code: "BINARY_DIGEST_UNAVAILABLE" });
    assert.deepEqual(release.requests, []);
  });

  for (const [name, table] of [
    ["digests for another version", { version: "0.0.1", archives: {} }],
    [
      "no entry for this target",
      {
        version: CONTRACT_VERSION,
        archives: {
          "aarch64-apple-darwin": {
            asset: `rkat-rpc-${CONTRACT_VERSION}-aarch64-apple-darwin.tar.gz`,
            archive_sha256: "0".repeat(64),
            binary_sha256: "1".repeat(64),
          },
        },
      },
    ],
    [
      "an entry naming another asset",
      {
        version: CONTRACT_VERSION,
        archives: {
          [LINUX.target]: {
            asset: "rkat-rpc-0.0.1-x86_64-unknown-linux-gnu.tar.gz",
            archive_sha256: "0".repeat(64),
            binary_sha256: "1".repeat(64),
          },
        },
      },
    ],
    [
      "a malformed digest",
      {
        version: CONTRACT_VERSION,
        archives: {
          [LINUX.target]: { asset: ASSET, archive_sha256: "not-a-digest", binary_sha256: "1".repeat(64) },
        },
      },
    ],
  ]) {
    it(`refuses ${name}`, async () => {
      release.table = table;

      await assert.rejects(MeerkatClient.ensureDownloadedBinary(), { code: "BINARY_DIGEST_UNAVAILABLE" });
      assert.deepEqual(release.requests, []);
    });
  }

  it("pins nothing in a source tree", () => {
    // Only the release workflow stamps the digests; a source or git install
    // must fail closed instead of downloading an unverifiable binary.
    MeerkatClient.pinnedDigests = saved.pinnedDigests;
    assert.deepEqual(MeerkatClient.pinnedDigests(), UNSTAMPED);
  });
});

describe("rkat-rpc binary resolution around the download", () => {
  it("uses an explicit binary path as-is, without downloading", async () => {
    const own = path.join(root, "my-rkat-rpc");
    writeFileSync(own, "#!/bin/sh\n");
    MeerkatClient.ensureDownloadedBinary = async () => assert.fail("must not download");

    assert.deepEqual(await MeerkatClient.resolveBinaryPath(own), { command: own, useLegacySubcommand: false });
    process.env.MEERKAT_BIN_PATH = own;
    assert.deepEqual(await MeerkatClient.resolveBinaryPath("rkat-rpc"), {
      command: own,
      useLegacySubcommand: false,
    });
  });

  it("never falls back to a legacy rkat after an integrity failure", async () => {
    MeerkatClient.commandPath = (command) => (command === "rkat" ? "/usr/local/bin/rkat" : null);
    MeerkatClient.ensureDownloadedBinary = async () => {
      throw new BinaryIntegrityError("tampered", {
        asset: ASSET,
        path: null,
        expectedSha256: "0".repeat(64),
        actualSha256: "1".repeat(64),
      });
    };

    await assert.rejects(MeerkatClient.resolveBinaryPath("rkat-rpc"), BinaryIntegrityError);
  });

  it("keeps the legacy rkat fallback when no digest is pinned", async () => {
    MeerkatClient.commandPath = (command) => (command === "rkat" ? "/usr/local/bin/rkat" : null);
    MeerkatClient.ensureDownloadedBinary = async () => {
      throw new MeerkatError("BINARY_DIGEST_UNAVAILABLE", "no pinned digest");
    };

    assert.deepEqual(await MeerkatClient.resolveBinaryPath("rkat-rpc"), {
      command: "/usr/local/bin/rkat",
      useLegacySubcommand: true,
    });
  });

  it("exports BinaryIntegrityError as a MeerkatError", () => {
    const error = new BinaryIntegrityError("tampered", {
      asset: ASSET,
      path: "/cache/rkat-rpc",
      expectedSha256: "0".repeat(64),
      actualSha256: "1".repeat(64),
    });
    assert.ok(error instanceof MeerkatError);
    assert.equal(error.name, "BinaryIntegrityError");
    assert.equal(error.code, "BINARY_INTEGRITY_MISMATCH");
    assert.equal(error.path, "/cache/rkat-rpc");
  });
});

describe("rkat-rpc release download transport", () => {
  const cdn = "https://release-assets.example/rkat-rpc.tar.gz";

  it("follows an https redirect", async () => {
    const responses = {
      [RELEASE_URL]: () => new Response(null, { status: 302, headers: { location: cdn } }),
      [cdn]: () => new Response(Buffer.from("archive bytes"), { status: 200 }),
    };
    globalThis.fetch = async (url, init) => {
      release.requests.push({ url: String(url), redirect: init?.redirect });
      return responses[String(url)]();
    };

    const body = await MeerkatClient.fetchReleaseArchive(RELEASE_URL);

    assert.deepEqual(Buffer.from(body), Buffer.from("archive bytes"));
    assert.deepEqual(release.requests, [
      { url: RELEASE_URL, redirect: "manual" },
      { url: cdn, redirect: "manual" },
    ]);
  });

  it("refuses a redirect to plain http", async () => {
    globalThis.fetch = async (url, init) => {
      release.requests.push({ url: String(url), redirect: init?.redirect });
      return new Response(null, { status: 302, headers: { location: "http://release-assets.example/x" } });
    };

    await assert.rejects(MeerkatClient.fetchReleaseArchive(RELEASE_URL), (error) => {
      assert.equal(error.code, "BINARY_DOWNLOAD_FAILED");
      assert.match(error.message, /http:\/\/release-assets\.example/);
      return true;
    });
    assert.deepEqual(release.requests, [{ url: RELEASE_URL, redirect: "manual" }]);
  });

  it("refuses a plain http URL without a request", async () => {
    await assert.rejects(
      MeerkatClient.fetchReleaseArchive(RELEASE_URL.replace("https:", "http:")),
      { code: "BINARY_DOWNLOAD_FAILED" },
    );
    assert.deepEqual(release.requests, []);
  });

  it("reports a dropped connection as a download failure, not tamper", async () => {
    globalThis.fetch = async () =>
      new Response(new ReadableStream({ start(controller) { controller.error(new Error("socket hang up")); } }), {
        status: 200,
      });

    await assert.rejects(MeerkatClient.fetchReleaseArchive(RELEASE_URL), (error) => {
      assert.ok(!(error instanceof BinaryIntegrityError));
      assert.equal(error.code, "BINARY_DOWNLOAD_FAILED");
      assert.match(error.message, /socket hang up/);
      return true;
    });
  });

  it("reports a body shorter than its declared length as a download failure", async () => {
    globalThis.fetch = async () =>
      new Response(Buffer.alloc(10, 120), { status: 200, headers: { "content-length": "100" } });

    await assert.rejects(MeerkatClient.fetchReleaseArchive(RELEASE_URL), (error) => {
      assert.ok(!(error instanceof BinaryIntegrityError));
      assert.equal(error.code, "BINARY_DOWNLOAD_FAILED");
      assert.match(error.message, /10 of 100/);
      return true;
    });
  });

  it("refuses an archive declared larger than the cap without reading it", async () => {
    globalThis.fetch = async () =>
      new Response(new ReadableStream({ pull() { throw new Error("an oversized archive was read"); } }), {
        status: 200,
        headers: { "content-length": String(256 * 1024 * 1024 + 1) },
      });

    await assert.rejects(MeerkatClient.fetchReleaseArchive(RELEASE_URL), (error) => {
      assert.equal(error.code, "BINARY_DOWNLOAD_TOO_LARGE");
      assert.match(error.message, /256 MiB/);
      return true;
    });
  });

  it("refuses an archive that streams past the cap", async () => {
    MeerkatClient.releaseArchiveMaxBytes = () => 16;
    globalThis.fetch = async () => new Response(Buffer.alloc(17, 120), { status: 200 });

    await assert.rejects(MeerkatClient.fetchReleaseArchive(RELEASE_URL), { code: "BINARY_DOWNLOAD_TOO_LARGE" });
  });

  it("accepts an archive at the cap", async () => {
    MeerkatClient.releaseArchiveMaxBytes = () => 16;
    globalThis.fetch = async () => new Response(Buffer.alloc(16, 120), { status: 200 });

    assert.deepEqual(Buffer.from(await MeerkatClient.fetchReleaseArchive(RELEASE_URL)), Buffer.alloc(16, 120));
  });

  it("refuses an endless redirect chain", async () => {
    globalThis.fetch = async (url, init) => {
      release.requests.push({ url: String(url), redirect: init?.redirect });
      return new Response(null, { status: 302, headers: { location: `${cdn}?hop=${release.requests.length}` } });
    };

    await assert.rejects(MeerkatClient.fetchReleaseArchive(RELEASE_URL), { code: "BINARY_DOWNLOAD_FAILED" });
    assert.ok(release.requests.length <= 6, `stopped after ${release.requests.length} requests`);
  });
});
