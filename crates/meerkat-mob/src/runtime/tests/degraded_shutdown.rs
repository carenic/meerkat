//! Shutdown's teardown, and a settlement it reports, unregister a member whose
//! runtime registration is durability-degraded.
//!
//! A boundary commit that fails leaves the session `ReloadRequired`, and its
//! exited runtime loop can keep the session's mutation fence in its
//! published handoff. Only the runtime's reload-required discard releases
//! that fence, so an exact unregister admitted first waits on it for good.
//! Shutdown's teardown and a member's unregister settlement therefore run the
//! discard first, as retire and destroy do, and unregister the exact cold
//! successor it publishes.

use super::*;
use crate::runtime::actor::ShutdownUnregisterOutcome;
use crate::runtime::{MemberUnregisterSettlement, MemberUnregisterState};

/// Bound for test steps that must happen promptly; it only turns a hang into
/// a failure.
const STEP: Duration = Duration::from_secs(10);

/// Shutdown's teardown of a degraded member's session discards the degraded
/// registration and unregisters the cold successor within its deadline,
/// instead of reporting the member pending at an admission that waits on the
/// retained fence. (A whole Shutdown of a degraded member waits earlier, in
/// its run-start hold, on the same fence: that gate wait is bounded
/// separately, so the teardown is driven directly here.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_teardown_discards_a_degraded_registration_first() {
    let mob = create_isolation_mob(1).await;
    mob.degrade_member(0).await;
    // The store recovers; the registration stays degraded until disposed.
    mob.store.fail_commit(mob.session(0), false);

    let outcome = tokio::time::timeout(
        STEP,
        crate::runtime::actor::teardown_session(
            &mob.adapter,
            &Arc::new(crate::runtime::ops_adapter::MobOpsAdapter::new()),
            mob.session(0),
            None,
            Instant::now() + Duration::from_secs(5),
        ),
    )
    .await
    .expect("the teardown returns within its deadline");
    match outcome {
        ShutdownUnregisterOutcome::Unregistered => {}
        ShutdownUnregisterOutcome::Pending { stage, .. } => {
            panic!("the degraded member's unregister is pending at {stage}")
        }
        ShutdownUnregisterOutcome::Failed(failure) => {
            panic!("the degraded member's unregister failed: {failure}")
        }
    }
    assert!(
        mob.adapter
            .current_session_registration_witness(mob.session(0))
            .await
            .is_none(),
        "neither the degraded registration nor its cold successor is left"
    );
}

/// A settlement over a durability-degraded registration whose exact
/// unregister was never admitted (as Shutdown leaves it when its deadline
/// passed first) runs the reload-required discard itself: without it, the
/// unregister admission waits on the mutation fence the degraded session's
/// exited runtime loop holds. It settles once the cold successor the discard
/// published is unregistered.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_degraded_unregister_settles_through_the_discard() {
    let mob = create_isolation_mob(1).await;
    mob.degrade_member(0).await;
    // The store recovers; the registration stays degraded until disposed.
    mob.store.fail_commit(mob.session(0), false);
    let registration = mob
        .adapter
        .current_session_registration_witness(mob.session(0))
        .await
        .expect("the degraded registration is current");
    let mut settlement = MemberUnregisterSettlement::new(registration, None, &mob.adapter, None);
    assert_eq!(settlement.try_state(), MemberUnregisterState::Pending);

    assert_eq!(
        settlement.settle_within(Instant::now() + STEP).await,
        MemberUnregisterState::Settled
    );
    assert!(
        mob.adapter
            .current_session_registration_witness(mob.session(0))
            .await
            .is_none(),
        "neither the degraded registration nor its cold successor is left"
    );
}
