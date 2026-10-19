/**
 * `close()` leaves nothing behind that keeps the Node process alive.
 *
 * `close()` ends the child's stdin, signals it, and waits for it to exit, up
 * to 5 s before it escalates to SIGKILL. A timer left running after the child
 * exited first kept every caller's process alive for those 5 s. Waiting for
 * the child's pipes to close instead of its exit let a descendant that holds
 * them keep `close()` waiting for the whole 5 s, or for ever after the kill.
 */

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";

import { CONTRACT_VERSION } from "../dist/generated/types.js";

const CLIENT_URL = new URL("../dist/client.js", import.meta.url).href;

/**
 * A fake rkat-rpc that answers the handshake. `stubborn` ignores SIGTERM and
 * stdin EOF; `holder` leaves a descendant that inherited its stdout and
 * stderr; `exitAfterHandshake` exits once the handshake is answered.
 */
function writeFixture(root, { stubborn = false, holder = false, exitAfterHandshake = false }) {
  const bin = path.join(root, "rkat-rpc.cjs");
  writeFileSync(bin, `#!/usr/bin/env node
const rl = require("node:readline").createInterface({ input: process.stdin });
rl.on("line", (line) => {
  const msg = JSON.parse(line);
  const result = msg.method === "initialize"
    ? { contract_version: ${JSON.stringify(CONTRACT_VERSION)}, methods: [] }
    : { capabilities: [] };
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result }) + "\\n");
  ${exitAfterHandshake ? 'if (msg.method !== "initialize") process.exit(0);' : ""}
});
${stubborn ? 'process.on("SIGTERM", () => {}); setInterval(() => {}, 1000);' : ""}
${holder ? 'require("node:child_process").spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: ["ignore", "inherit", "inherit"] }).unref();' : ""}
`);
  chmodSync(bin, 0o755);
  return bin;
}

/** Past this the closer and the fixture it spawned are killed, so a stuck close() cannot hang the suite. */
const CLOSER_DEADLINE_MS = 15_000;

/**
 * Run a separate Node process that connects to `bin`, closes the client and
 * then has nothing left to do; with `afterExit`, it closes only once the
 * child has exited. Reports how long close() took, how long the process lived
 * after close() returned, what it wrote to stderr, and its process group.
 */
function runCloser(root, bin, { afterExit = false } = {}) {
  const closer = path.join(root, "closer.mjs");
  writeFileSync(closer, `
import { MeerkatClient } from ${JSON.stringify(CLIENT_URL)};
const client = new MeerkatClient(${JSON.stringify(bin)});
await client.connect();
const child = client.process;
const pid = child.pid;
${afterExit ? 'if (child.exitCode === null && child.signalCode === null) await new Promise((resolve) => child.once("exit", resolve));' : ""}
const started = performance.now();
await client.close();
process.stdout.write(JSON.stringify({ pid, closeMs: performance.now() - started }) + "\\n");
`);
  const env = { ...process.env };
  delete env.MEERKAT_BIN_PATH;
  return new Promise((resolve, reject) => {
    // Its own process group: the fixture joins it, so one kill takes both.
    const child = spawn(process.execPath, [closer], {
      env,
      stdio: ["ignore", "pipe", "pipe"],
      detached: true,
    });
    let stdout = "";
    let stderr = "";
    let closedAt;
    let killed = false;
    const deadline = setTimeout(() => {
      killed = true;
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
    }, CLOSER_DEADLINE_MS);
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
      if (closedAt === undefined && stdout.includes("\n")) closedAt = performance.now();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.once("error", (error) => {
      clearTimeout(deadline);
      reject(error);
    });
    // "close", not "exit": every byte the closer wrote has been read by then.
    child.once("close", (code, signal) => {
      clearTimeout(deadline);
      const exitedAt = performance.now();
      if (killed) {
        const state = closedAt === undefined ? "before close() returned" : "after close() returned";
        reject(new Error(`killed the closer and its fixture ${CLOSER_DEADLINE_MS} ms in, ${state}`));
        return;
      }
      if (code !== 0 || closedAt === undefined) {
        reject(new Error(`closer failed (code ${code}, signal ${signal}): ${stderr}`));
        return;
      }
      resolve({ ...JSON.parse(stdout.trim()), lingerMs: exitedAt - closedAt, stderr, group: child.pid });
    });
  });
}

/** Kill what is left of a closer's process group, such as a fixture's descendant. */
function killGroup(group) {
  try {
    process.kill(-group, "SIGKILL");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}

describe("MeerkatClient.close()", () => {
  it("lets the process exit as soon as the child has closed", { timeout: 20_000 }, async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-close-"));
    try {
      const run = await runCloser(root, writeFixture(root, { stubborn: false }));
      assert.ok(run.closeMs < 2_000, `close() took ${Math.round(run.closeMs)} ms`);
      assert.ok(
        run.lingerMs < 1_000,
        `the process stayed alive ${Math.round(run.lingerMs)} ms after close() returned`,
      );
      assert.equal(run.stderr, "");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("still waits 5 s, then kills a child that ignores SIGTERM", { timeout: 20_000 }, async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-close-"));
    try {
      const run = await runCloser(root, writeFixture(root, { stubborn: true }));
      assert.ok(
        run.closeMs >= 4_500 && run.closeMs < 9_000,
        `close() took ${Math.round(run.closeMs)} ms, not the 5 s cap`,
      );
      assert.equal(alive(run.pid), false, "close() must kill the stubborn child");
      assert.ok(
        run.lingerMs < 1_000,
        `the process stayed alive ${Math.round(run.lingerMs)} ms after close() returned`,
      );
      assert.equal(run.stderr, "");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("returns once a killed child has exited, even when a descendant holds its pipes", { timeout: 20_000 }, async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-close-"));
    let group;
    try {
      const run = await runCloser(root, writeFixture(root, { stubborn: true, holder: true }));
      group = run.group;
      assert.ok(
        run.closeMs >= 4_500 && run.closeMs < 9_000,
        `close() took ${Math.round(run.closeMs)} ms, not the 5 s cap`,
      );
      assert.equal(alive(run.pid), false, "close() must kill the stubborn child");
      assert.ok(
        run.lingerMs < 1_000,
        `the process stayed alive ${Math.round(run.lingerMs)} ms after close() returned`,
      );
      assert.equal(run.stderr, "");
    } finally {
      if (group !== undefined) killGroup(group);
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("returns as soon as the child exits, even when a descendant holds its pipes", { timeout: 20_000 }, async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-close-"));
    let group;
    try {
      const run = await runCloser(root, writeFixture(root, { holder: true }));
      group = run.group;
      assert.ok(run.closeMs < 1_000, `close() took ${Math.round(run.closeMs)} ms`);
      assert.equal(alive(run.pid), false, "the child must have exited");
      assert.ok(
        run.lingerMs < 1_000,
        `the process stayed alive ${Math.round(run.lingerMs)} ms after close() returned`,
      );
      assert.equal(run.stderr, "");
    } finally {
      if (group !== undefined) killGroup(group);
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("returns at once for a child that has already exited while a descendant holds its pipes", { timeout: 20_000 }, async () => {
    const root = mkdtempSync(path.join(os.tmpdir(), "meerkat-ts-close-"));
    let group;
    try {
      const fixture = writeFixture(root, { holder: true, exitAfterHandshake: true });
      const run = await runCloser(root, fixture, { afterExit: true });
      group = run.group;
      assert.ok(run.closeMs < 1_000, `close() took ${Math.round(run.closeMs)} ms`);
      assert.ok(
        run.lingerMs < 1_000,
        `the process stayed alive ${Math.round(run.lingerMs)} ms after close() returned`,
      );
      assert.equal(run.stderr, "");
    } finally {
      if (group !== undefined) killGroup(group);
      rmSync(root, { recursive: true, force: true });
    }
  });
});
