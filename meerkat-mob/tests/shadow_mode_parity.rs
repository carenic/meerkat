//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task (or its successor) that proves DSL
//! projection parity with actor-restored wiring.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.6): the DSL `peer_projection` must agree exactly with the
//! actor-restored wiring set after a restart/restore. This is the
//! cutover gate — shadow-mode parity OR its successor (DSL as the
//! only authority, actor restore consumes the DSL projection) must
//! be exercised by this test.
//!
//! Status at c.0: `#[ignore]`. Expected API: either (a) a shadow-mode
//! comparison surface in the MobStorage layer, or (b) a DSL-only
//! restore path where the actor-restored state is literally the DSL
//! projection.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when shadow-mode parity (or its \
            DSL-only successor) is wired; asserts projection == \
            actor-restored wiring"]
fn dsl_projection_equals_actor_restored_wiring_set() {
    // TODO(C-T): build a non-trivial wiring (≥3 members, ≥4 wires),
    // persist, restart the actor, and assert
    //   dsl_peer_projection(state) == actor_restored_wiring(snapshot)
    // as a set equality. Any drift is a cutover gate failure.
    unreachable!(
        "shadow-mode parity tripwire: un-ignore with the \
         parity-or-successor wiring"
    );
}
