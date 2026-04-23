//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task that restores the PR #340
//! add-failure-typed-error behaviour.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.3): when the trust reconciler's underlying store returns an
//! error during an `add_peer` operation, the reconcile must surface a
//! typed `TrustReconcileError::AddFailed { .. }` and must NOT update
//! the in-memory "applied" view — otherwise later reconciles see a
//! phantom peer that was never actually committed.
//!
//! Status at c.0: `#[ignore]`. Expected API:
//!   `reconcile(...)` → `Result<ReconcileReport, TrustReconcileError>`
//! with `TrustReconcileError::AddFailed { peer, source }` variant.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when `TrustReconcileError::AddFailed` \
            exists and `reconcile` wires underlying store errors \
            through it without mutating the applied view"]
fn trust_reconcile_add_failure_surfaces_typed_error_and_preserves_applied_view() {
    // TODO(C-T): stub a `TrustStore` whose `add_peer` returns an
    // error; drive a reconcile; assert
    //   (a) the reconcile result is `Err(TrustReconcileError::AddFailed { .. })`
    //   (b) the reconciler's internal "applied" view is unchanged
    //   (c) a subsequent successful reconcile does not see the
    //       phantom add.
    unreachable!(
        "trust_reconcile add-failure tripwire: un-ignore with the \
         typed-error port"
    );
}
