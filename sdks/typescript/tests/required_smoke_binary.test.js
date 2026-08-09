import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";

import { resolveRequiredSmokeBinary } from "./support/required_smoke_binary.mjs";

describe("TypeScript SDK smoke binary authority", () => {
  it("requires an explicitly named runtime and never falls back to PATH", () => {
    assert.throws(
      () => resolveRequiredSmokeBinary({ PATH: process.env.PATH }),
      /require MEERKAT_BIN_PATH or MEERKAT_RPC_BINARY/,
    );
  });

  it("accepts the exact executable named by MEERKAT_BIN_PATH", () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-smoke-bin-"));
    const binary = path.join(root, "rkat-rpc");
    writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    chmodSync(binary, 0o755);

    try {
      assert.equal(
        resolveRequiredSmokeBinary({ MEERKAT_BIN_PATH: binary }),
        binary,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("rejects a named binary that is missing", () => {
    const missing = path.join(os.tmpdir(), "missing-meerkat-ts-smoke-rkat-rpc");
    assert.throws(
      () => resolveRequiredSmokeBinary({ MEERKAT_RPC_BINARY: missing }),
      /missing or not executable/,
    );
  });
});
