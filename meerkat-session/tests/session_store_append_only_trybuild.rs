//! Tripwire for wave-c (Section 1.5 #9). Flipped green by **C-H1**
//! (type-level append-only `SessionStore` / `AppendOnlyMessages`
//! newtype — the compile-time form of F1 closure from the
//! state-scope-audit).
//!
//! Invariant: the public `SessionStore` / `Session` API must not
//! permit a caller to *shrink* a session's message history. Today
//! `SessionStore::save(&Session)` accepts any `Session`, including
//! ones with fewer messages than a previously-saved version — that's
//! the F1 gap. Post-C-H1, `session.messages_mut(): &mut Vec<Message>`
//! is replaced with an append-only witness, and truncating code fails
//! to compile.
//!
//! Status at c.0: marked `#[ignore]` — chose this over a `trybuild`
//! fixture today because (a) the `meerkat-core` / `meerkat-session`
//! tree does not compile at c.0, so `trybuild` would report unrelated
//! compile failures, and (b) the test's semantics are "this code used
//! to compile, must not after C-H1" which is exactly the thing a
//! runtime `#[ignore]` expresses without a fixture directory.
//!
//! Un-ignore strategy at C-H1: either
//!   (a) convert this to a `trybuild::TestCases::compile_fail(...)`
//!       over `tests/trybuild/shrink_session.rs`, OR
//!   (b) keep the `#[test]` but assert that a known-bad API call
//!       (e.g. `Session::replace_messages(vec![])`) no longer exists
//!       via a compile-probe in a submodule.
//! Either way, the catching assertion is: "shrink-a-session does not
//! type-check".

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
#[ignore = "un-ignore at C-H1: convert to trybuild compile-fail on a \
            fixture that attempts to shrink a session's message \
            history; today `save()` allows replacement so the \
            compile-fail would be a false positive"]
fn session_message_history_is_append_only_at_type_level() {
    // TODO(C-H1): establish one of the two shapes below.
    //
    // Shape A — trybuild fixture (preferred):
    //   use trybuild::TestCases;
    //   let t = TestCases::new();
    //   t.compile_fail("tests/trybuild/shrink_session.rs");
    //   // ^ fixture attempts to call
    //   //   `session.messages_mut().truncate(0)` or
    //   //   `store.save(&shrunk_session).await?` — both must fail
    //   //   to compile post-C-H1.
    //
    // Shape B — inline compile-probe:
    //   Keep a `#[cfg(any())]` stub referencing the forbidden API;
    //   remove the cfg guard as part of C-H1 to land the final
    //   compile-fail.
    unreachable!(
        "append-only SessionStore tripwire: un-ignore at C-H1 with a \
         trybuild compile-fail fixture"
    );
}
