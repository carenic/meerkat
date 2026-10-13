import assert from "node:assert/strict";
import test from "node:test";

import { CONSENT_NOTICE_NOTIFICATION, parseConsentNotice } from "../dist/index.js";

test("a consent notice narrows to its typed params", () => {
  assert.equal(CONSENT_NOTICE_NOTIFICATION, "approval/consent_notice");
  for (const kind of ["requested", "approved", "denied", "used", "withdrawn"]) {
    assert.deepEqual(
      parseConsentNotice({
        approval_id: "01936f8a-7b2c-7000-8000-0000000000c1",
        tool_call_id: "toolu_01",
        kind,
      }),
      {
        approval_id: "01936f8a-7b2c-7000-8000-0000000000c1",
        tool_call_id: "toolu_01",
        kind,
      },
    );
  }
});

test("a malformed or unknown consent notice is ignored, never misread", () => {
  for (const params of [
    null,
    [],
    "requested",
    { tool_call_id: "toolu_01", kind: "requested" },
    { approval_id: "a", tool_call_id: 7, kind: "requested" },
    { approval_id: "a", tool_call_id: "toolu_01", kind: "granted" },
  ]) {
    assert.equal(parseConsentNotice(params), undefined);
  }
});

const PRESENTATION = {
  action: { kind: "tool_call", summary: "read", body: { folder: "inbox" } },
  risk: "high",
  account: "alice at https://issuer.example",
  mandate: "the work's mandate",
  accepted_assurance: "device_trusted",
  expires_at: "2030-01-02T03:04:05Z",
};

test("a requested notice carries its presentation", () => {
  const notice = { approval_id: "a", tool_call_id: "toolu_01", kind: "requested" };
  assert.deepEqual(parseConsentNotice({ ...notice, presentation: PRESENTATION }), {
    ...notice,
    presentation: PRESENTATION,
  });
  // An owner statement that is absent stays absent.
  const { account: _account, mandate: _mandate, ...unstated } = PRESENTATION;
  assert.deepEqual(
    parseConsentNotice({ ...notice, presentation: { ...unstated, account: null } }),
    { ...notice, presentation: unstated },
  );
  assert.deepEqual(parseConsentNotice({ ...notice, presentation: null }), notice);
});

test("a presentation this build cannot read makes the notice ignored", () => {
  const notice = { approval_id: "a", tool_call_id: "toolu_01", kind: "requested" };
  for (const presentation of [
    "high",
    [],
    { ...PRESENTATION, action: "read" },
    { ...PRESENTATION, action: { kind: "tool_call" } },
    { ...PRESENTATION, risk: "extreme" },
    { ...PRESENTATION, accepted_assurance: "hardware_key" },
    { ...PRESENTATION, expires_at: 7 },
    { ...PRESENTATION, account: 7 },
  ]) {
    assert.equal(parseConsentNotice({ ...notice, presentation }), undefined);
  }
});
