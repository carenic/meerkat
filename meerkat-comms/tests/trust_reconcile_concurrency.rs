//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task that ports the PR #340 concurrency
//! regression fix to the DSL reducer path.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.1): concurrent reconciles of the trust store serialize — if two
//! reconcile calls overlap, the second observes the first's committed
//! state. Stale-epoch reconciles short-circuit rather than racing.
//!
//! Status at c.0: `#[ignore]`, not yet wired. The expected API surface
//! is
//!   `meerkat_comms::trust_reconcile::reconcile(&store, &policy, epoch)`
//! plus a typed `TrustReconcileError`. Un-ignore when that module
//! lands and a mutex / actor-mailbox inside `reconcile` serialises
//! concurrent callers.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when `meerkat-comms` exposes a \
            concurrent-reconcile-serialising entry point (port of \
            PR #340)"]
fn concurrent_reconciles_are_serialised_and_stale_short_circuits() {
    // TODO(C-T): spawn N (= 8) concurrent reconcile calls against a
    // shared store; all must complete without a "CAS conflict" /
    // "stale epoch" panic. Drive a stale epoch through one caller and
    // assert the returned `TrustReconcileError::StaleEpoch` instead of
    // mutating the store.
    unreachable!(
        "trust_reconcile concurrency tripwire: un-ignore with the \
         serialised reconcile port"
    );
}
