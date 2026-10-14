/**
 * comms/send receipts: the SDK accepts exactly the server's shape.
 *
 * Fixtures are derived from `CommsSendResult` in
 * artifacts/schemas/wire-types.json, which `emit-schemas` generates from the
 * Rust `CommsSendResult` (crates/meerkat-contracts/src/wire/comms.rs) and its
 * `PeerDeliveryOutcome` (crates/meerkat-core/src/comms.rs).
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

import { MeerkatClient, MeerkatError } from "../dist/index.js";

const SCHEMA = JSON.parse(
  readFileSync(new URL("../../../artifacts/schemas/wire-types.json", import.meta.url), "utf8"),
).CommsSendResult;

// The command that yields each receipt kind.
const COMMANDS = {
  input_accepted: { kind: "input", body: "hello" },
  peer_message_sent: { kind: "peer_message", to: "agent-a", body: "hello" },
  peer_lifecycle_sent: {
    kind: "peer_lifecycle",
    to: "agent-a",
    lifecycle_kind: "mob.peer_added",
    params: { peer: "agent-b" },
  },
  peer_request_sent: {
    kind: "peer_request",
    to: "agent-a",
    intent: "checksum_token",
    params: { subject: "release-notes" },
  },
  peer_response_sent: {
    kind: "peer_response",
    to: "agent-a",
    in_reply_to: "00000000-0000-4000-8000-000000000002",
    status: "completed",
    result: {
      request_intent: "checksum_token",
      request_subject: "release-notes",
      token: "token-1",
    },
  },
};

// Every `PeerDeliveryOutcome` the server can serialize: externally tagged,
// snake_case, with a `PeerIngressTerminalOutcomeKind` inside
// `durably_resolved` (crates/meerkat-core/src/interaction.rs).
const DELIVERY_FORMS = [
  "acked",
  "volatile_handed_off",
  "queued",
  { durably_resolved: { outcome: "accepted" } },
  { durably_resolved: { outcome: "deduplicated" } },
  { durably_resolved: { outcome: "rejected" } },
];

const INVALID_DELIVERIES = [
  "handed_off",
  "durably_resolved",
  "ACKED",
  "",
  null,
  true,
  1,
  ["acked"],
  {},
  { acked: null },
  { durably_resolved: { outcome: "accepted" }, acked: null },
  { durably_resolved: "accepted" },
  { durably_resolved: null },
  { durably_resolved: {} },
  { durably_resolved: { outcome: "dropped" } },
  { durably_resolved: { outcome: null } },
  { durably_resolved: { outcome: ["accepted"] } },
  { durably_resolved: { outcome: { accepted: true } } },
  { durably_resolved: { outcome: "accepted", reason: "late" } },
];

function resolve(node) {
  return node.$ref === undefined ? node : SCHEMA.$defs[node.$ref.replace(/^#\/\$defs\//, "")];
}

/** Every value the schema node admits, one representative per string. */
function instances(schemaNode) {
  const node = resolve(schemaNode);
  if (node.oneOf !== undefined) return node.oneOf.flatMap(instances);
  if ("const" in node) return [node.const];
  if (node.enum !== undefined) return [...node.enum];
  if (node.type === "string") return ["00000000-0000-4000-8000-000000000001"];
  if (node.type === "boolean") return [false, true];
  assert.equal(node.type, "object");
  let values = [{}];
  for (const [name, prop] of Object.entries(node.properties)) {
    values = values.flatMap((value) => instances(prop).map((item) => ({ ...value, [name]: item })));
  }
  return values;
}

const RECEIPTS = instances(SCHEMA);
const ARMS = Object.fromEntries(SCHEMA.oneOf.map((arm) => [arm.properties.kind.const, arm]));
const PEER_KINDS = Object.keys(ARMS).filter((kind) => kind !== "input_accepted").sort();

function receiptId(receipt) {
  return ["kind", "delivery", "stream_reserved"]
    .filter((key) => key in receipt)
    .map((key) => (typeof receipt[key] === "string" ? receipt[key] : JSON.stringify(receipt[key])))
    .join("-");
}

function clientReturning(response) {
  const client = new MeerkatClient();
  client.request = async (method) => {
    assert.equal(method, "comms/send");
    return structuredClone(response);
  };
  return client;
}

async function assertRejected(response, kind = "peer_message_sent") {
  await assert.rejects(
    () => clientReturning(response).send("s1", COMMANDS[kind]),
    (error) => error instanceof MeerkatError && error.code === "INVALID_RESPONSE",
  );
}

describe("comms/send receipts match the server CommsSendResult", () => {
  it("schema fixtures cover every receipt kind and delivery form", () => {
    assert.deepEqual(
      [...new Set(RECEIPTS.map((receipt) => receipt.kind))].sort(),
      Object.keys(COMMANDS).sort(),
    );
    const peerReceipts = RECEIPTS.filter((receipt) => receipt.kind !== "input_accepted");
    assert.ok(peerReceipts.every((receipt) => "delivery" in receipt));
    for (const kind of PEER_KINDS) {
      assert.ok(ARMS[kind].required.includes("delivery"), kind);
    }
    assert.deepEqual(
      new Set(peerReceipts.map((receipt) => JSON.stringify(receipt.delivery))),
      new Set(DELIVERY_FORMS.map((form) => JSON.stringify(form))),
    );
  });

  for (const receipt of RECEIPTS) {
    it(`accepts ${receiptId(receipt)}`, async () => {
      const result = await clientReturning(receipt).send("s1", COMMANDS[receipt.kind]);
      assert.deepEqual(result, receipt);
    });
  }

  for (const [kind, arm] of Object.entries(ARMS)) {
    it(`rejects ${kind} with an unknown field`, async () => {
      const [receipt] = instances(arm);
      await assertRejected({ ...receipt, extra_behavior: true }, kind);
    });

    for (const field of arm.required) {
      it(`rejects ${kind} without ${field}`, async () => {
        const [receipt] = instances(arm);
        delete receipt[field];
        await assertRejected(receipt, kind);
      });
    }
  }

  for (const kind of PEER_KINDS) {
    for (const delivery of INVALID_DELIVERIES) {
      it(`rejects ${kind} with delivery ${JSON.stringify(delivery)}`, async () => {
        const [receipt] = instances(ARMS[kind]);
        await assertRejected({ ...receipt, delivery }, kind);
      });
    }
  }

  it("rejects non-object results and unknown kinds", async () => {
    for (const response of [
      null,
      [],
      "peer_message_sent",
      {},
      { kind: "peer_message", envelope_id: "env-1", delivery: "acked" },
      { kind: "legacy_sent", envelope_id: "env-1", delivery: "acked" },
    ]) {
      await assertRejected(response);
    }
  });
});
