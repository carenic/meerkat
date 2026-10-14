/**
 * Per-turn `authBinding` reaches `turn/start` on every turn entry point.
 *
 * `turn/start` carries `auth_binding` as the canonical Inherit/Set/Clear
 * tri-state (`StartTurnParams` in crates/meerkat-rpc/src/handlers/turn.rs):
 * omit it to keep the session's binding, pass a binding ref or
 * `{ action: "set", value: ref }` to switch, or `{ action: "clear" }` to drop
 * it.
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { DeferredSession, MeerkatClient, Session } from "../dist/index.js";

const RUN_RESULT = {
  session_id: "s1",
  text: "ok",
  turns: 1,
  tool_calls: 0,
  usage: { input_tokens: 1, output_tokens: 1 },
};
const REF = { realm: "team", binding: "work-anthropic" };
const OVERRIDES = [
  ["a binding ref", REF],
  ["an explicit set", { action: "set", value: { ...REF, profile: "default" } }],
  ["a clear", { action: "clear" }],
];

function recordingClient() {
  const client = new MeerkatClient();
  const sent = [];
  client.request = async (method, params) => {
    sent.push({ method, params });
    return structuredClone(RUN_RESULT);
  };
  client.process = {
    stdin: {
      write: (data) => {
        const { method, params } = JSON.parse(data);
        sent.push({ method, params });
      },
    },
  };
  client.registerRequest = async () => structuredClone(RUN_RESULT);
  return { client, sent };
}

function session(client) {
  return new Session(client, {
    sessionId: "s1",
    text: "ready",
    turns: 0,
    toolCalls: 0,
    usage: { inputTokens: 0, outputTokens: 0 },
  });
}

describe("turn/start auth_binding", () => {
  for (const [label, authBinding] of OVERRIDES) {
    it(`turn, stream and deferred startTurn send ${label}`, async () => {
      const { client, sent } = recordingClient();

      await session(client).turn("next", { authBinding });
      session(client).stream("next", { authBinding });
      await new DeferredSession(client, "s1").startTurn("first", { authBinding });

      assert.equal(sent.length, 3);
      for (const { method, params } of sent) {
        assert.equal(method, "turn/start");
        assert.deepEqual(params.auth_binding, authBinding);
      }
    });
  }

  it("omits auth_binding when the option is absent", async () => {
    const { client, sent } = recordingClient();

    await session(client).turn("next", {});

    assert.deepEqual(sent, [
      { method: "turn/start", params: { session_id: "s1", prompt: "next" } },
    ]);
  });
});
