//! Smoke test for `meerkat::session_runtime::staged_promotion`.
//!
//! Wider coverage of `PendingPromotionCleanup` lives in
//! `meerkat-rpc`'s integration tests because constructing a real
//! `PromotingSlot` + `StagedSessionRegistry` requires the runtime
//! plumbing the rpc crate already provides. Here we just assert the
//! `Mode` enum's discriminants compile and Copy/PartialEq hold.

#![cfg(all(feature = "session-store", not(target_arch = "wasm32")))]

use meerkat::session_runtime::staged_promotion::PendingPromotionCleanupMode;

#[test]
fn cleanup_mode_variants_are_distinct_and_copy() {
    let restore = PendingPromotionCleanupMode::Restore;
    let finish = PendingPromotionCleanupMode::Finish;
    assert_ne!(restore, finish);

    let restore_copy = restore;
    let finish_copy = finish;
    assert_eq!(restore, restore_copy);
    assert_eq!(finish, finish_copy);
}
