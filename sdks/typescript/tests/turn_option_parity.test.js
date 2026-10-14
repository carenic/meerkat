/**
 * A deferred session's first turn sends the same per-turn host options as any
 * other turn, and a mob member turn sends its auth-binding override as given.
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { DeferredSession, MeerkatClient } from "../dist/index.js";

const RUN_RESULT = {
  session_id: "s1",
  text: "ok",
  turns: 1,
  tool_calls: 0,
  usage: { input_tokens: 1, output_tokens: 1 },
};

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

describe("turn option parity", () => {
  it("deferred startTurn and stream send the per-turn host options", async () => {
    const { client, sent } = recordingClient();
    const options = {
      selfHostedServerId: "local-gpu",
      injectedContext: ["ambient host context"],
      transientTurnContext: "request-only host facts",
    };

    await new DeferredSession(client, "s1").startTurn("first", options);
    new DeferredSession(client, "s1").stream("first", options);

    assert.equal(sent.length, 2);
    for (const { method, params } of sent) {
      assert.equal(method, "turn/start");
      assert.equal(params.self_hosted_server_id, "local-gpu");
      assert.deepEqual(params.injected_context, ["ambient host context"]);
      assert.equal(params.transient_turn_context, "request-only host facts");
    }
  });

  it("mob turn start sends the auth-binding override as given", async () => {
    const { client, sent } = recordingClient();

    await client.mobTurnStart("mob-1", "worker", "go", { authBinding: { action: "clear" } });

    assert.equal(sent.length, 1);
    assert.equal(sent[0].method, "mob/turn_start");
    assert.deepEqual(sent[0].params.auth_binding, { action: "clear" });
  });
});
