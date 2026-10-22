import { test } from "node:test";
import assert from "node:assert/strict";
import { authErrorReason, MeerkatError, meerkatErrorFromJsonRpcCode } from "../dist/index.js";

// A graceful mob Shutdown that keeps a queued input answers its waiting
// `turn/start` with JSON-RPC -32003 (session not running) and
// `error.data.reason` "requeued_across_stop". The SDK has no accessor of its
// own for that reason: the error keeps its code and the whole data payload,
// and no typed accessor misreads it.
const REQUEUED = {
  session_id: "01936f8b-0000-7000-8000-000000000001",
  input_id: "01936f8b-0000-7000-8000-000000000002",
  reason: "requeued_across_stop",
  stopped_at: "2026-10-22T22:00:00Z",
};

test("a requeued wait keeps its code and whole data", () => {
  const error = meerkatErrorFromJsonRpcCode(
    -32003,
    undefined,
    "the input stays queued across the runtime stop and runs on the next start",
    REQUEUED,
  );
  assert.ok(error instanceof MeerkatError);
  assert.equal(error.constructor, MeerkatError);
  assert.equal(error.code, "-32003");
  assert.deepEqual(error.details, REQUEUED);
  assert.equal(authErrorReason(error), undefined);
});
