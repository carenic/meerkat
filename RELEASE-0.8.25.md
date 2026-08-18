# Meerkat 0.8.25 release train

## Blocking train-opening item: migrate SDK wrappers to generated types

Owner: sdk-contracts

The first 0.8.25 change must advance
`BASELINE_HAND_ROLLED_CURRENT_TRAIN` in
`scripts/verify_rpc_signature_parity.py` from 0.8.24 to 0.8.25. That mutation
must remain failing until the migration below is complete.

Migrate all 239 entries in `BASELINE_HAND_ROLLED` to the generated SDK
parameter and result types. The reviewed inventory is 110 TypeScript sides and
129 Python sides. This work is complete only when the baseline is empty and
the RPC signature-parity verifier passes without grandfathered wrappers.

The 0.8.24 release may retain the exact reviewed 239-entry baseline and cap.
The verifier binds that exception to release trains before 0.8.25. Advancing
the explicit train marker to 0.8.25 makes the verifier fail while any baseline
entry remains, avoiding a calendar-date renewal and the package-version bump
that occurs during release closeout.

The migration must not weaken wrapper coverage, admit new hand-rolled shapes,
or suppress stale-entry detection. Delete each baseline entry only with the
corresponding generated-type wrapper migration.
