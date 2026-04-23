//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task that classifies restore-failure as a
//! terminal mob member state.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.7): when a mob member's runtime fails to restore from a
//! persisted snapshot, the member must be classified `Broken`
//! (terminal) — not retried silently, not left in a transient
//! "restoring" state. Terminal classification is what lets the
//! orchestrator detach the member cleanly without taking down
//! siblings.
//!
//! Status at c.0: `#[ignore]`. Expected API: `MobMemberStatus::Broken
//! { reason }` reachable from a `MobMemberRestoreFailed` effect in
//! the MobMachine DSL.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when restore-failure classification \
            to terminal `Broken` lands in the MobMachine DSL"]
fn restore_failure_classifies_member_as_terminal_broken() {
    // TODO(C-T): persist a minimal mob snapshot, corrupt the member
    // payload, drive the restore path, assert the resulting member
    // status is `Broken { reason }` and the orchestrator emits a
    // detach effect rather than a retry.
    unreachable!(
        "restore-failure terminal classification tripwire: un-ignore \
         with the MobMachine classification landing"
    );
}
