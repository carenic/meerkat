//! Tripwire (C-T blocker stub) for wave-c (Section 1.5 #7). Un-ignored
//! by the source-side C-T task that re-pins turn-boundary denial
//! semantics.
//!
//! Invariant (per `docs/wave-c-prep/test-coverage-audit.md` §5.3 +
//! §6.9): when the runtime denies a turn boundary (e.g. authority
//! refuses the boundary effect), the denied effect must NOT execute
//! its side-effects — no external tool call, no compaction write, no
//! peer emit. Denial is a hard stop, not an advisory.
//!
//! Status at c.0: `#[ignore]`. Expected API: `RuntimeDecision::Denied
//! { reason }` returned from the boundary authority, consumed by the
//! effect-dispatching handle without firing the underlying side-
//! effect.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "C-T blocker: un-ignore when turn-boundary denial is \
            wired through the effect dispatch path and proven to \
            block side-effects"]
fn turn_boundary_denial_blocks_all_side_effects() {
    // TODO(C-T): construct a turn where the boundary authority
    // returns `Denied`; wire a spy tool dispatcher that records every
    // invocation; drive the turn; assert the spy is never called and
    // the run result carries the denial reason verbatim.
    unreachable!(
        "turn-boundary denial tripwire: un-ignore with the boundary \
         denial effect-blocking landing"
    );
}
