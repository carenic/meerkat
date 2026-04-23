//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task that wires release-binding into the
//! overlay sweep.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.5): releasing a member's runtime binding must remove that
//! member from the peer-overlay projection — no stale overlay entry
//! survives the release. This is the session-lifecycle cleanup arm of
//! the peer-projection reducer.
//!
//! Status at c.0: `#[ignore]`. Expected API: DSL reducer path for the
//! `MemberBindingReleased` effect (or equivalent post-C-6r) that
//! sweeps the overlay.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when release-binding drives the \
            overlay sweep through the DSL reducer"]
fn release_binding_removes_member_from_overlay_projection() {
    // TODO(C-T): wire M1↔M2, release M1's binding, poll the
    // peer_projection; assert M1 has no overlay entry and M2's
    // overlay no longer references M1.
    unreachable!(
        "release-binding overlay sweep tripwire: un-ignore with the \
         DSL reducer path"
    );
}
