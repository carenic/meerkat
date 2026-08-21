# Meerkat 0.8.25 release train

## Completed train-opening item: generated SDK wrapper migration

Owner: sdk-contracts

Completed on 2026-08-19 by PR #964. All public Python and TypeScript RPC
wrappers now use the generated parameter and result contracts at the request
boundary.

`scripts/verify_rpc_signature_parity.py` no longer contains
`BASELINE_HAND_ROLLED`, `BASELINE_HAND_ROLLED_CURRENT_TRAIN`, an entry cap, or
an expiry waiver. The verifier now rejects hand-rolled wrapper shapes for both
new and historical methods and requires exact catalog type references.

The post-migration release gate is:

- keep the generated RPC method artifact current;
- keep the documented Params and Result columns exactly aligned with it;
- keep SDK request sites bound to generated contract types;
- do not add a new grandfathered baseline or suppress stale-entry detection.
