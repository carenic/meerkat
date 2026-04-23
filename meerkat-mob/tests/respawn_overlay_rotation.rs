//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task that re-wires the respawn→overlay
//! rotation path through the DSL reducer.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.4): wire two members M1 and M2, respawn M1, and the
//! prior-session peer overlay entry for M1 must be cleared — the new
//! session id replaces the old in the overlay projection, so peers
//! that were wired to M1 see the rotated session immediately.
//!
//! Status at c.0: `#[ignore]`. Expected API is the DSL reducer path
//! exposing `peer_projection` under the `MeerkatMachine` actor with
//! the respawn effect emitting a `MemberRuntimeBinding` rotation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when the DSL peer-projection \
            rotation path is wired end-to-end through the actor"]
fn respawn_clears_prior_session_overlay_and_installs_rotated_session() {
    // TODO(C-T): build a two-member fixture, wire M1↔M2, respawn M1
    // through the actor, poll the peer_projection for M2, assert the
    // overlay for M1 references the *new* session id and the old
    // session id is absent from any overlay entry.
    unreachable!(
        "respawn overlay rotation tripwire: un-ignore with the DSL \
         rotation path"
    );
}
