#![cfg(feature = "runtime-adapter")]
//! The lift's post-commit work and the work a member run can wait on.
//!
//! The lift (an explicit resume of a stopped mob) settles MobMachine's
//! topology fence when its topology reconciliation settles, before its
//! post-commit work (the members' operation bindings). Graph work a member's
//! run can wait on (a spawn, a wire, a retire) waits only for that
//! reconciliation; host repair work (a registration reload, a supervisor
//! rotation) still waits for the whole resume. These rows hold one member's
//! operation binding at a gated step and observe both sides of that split.

use super::*;

/// Bound for steps that must happen promptly; it only turns a hang into a
/// failure.
const STEP: Duration = Duration::from_secs(10);

/// The lift of a stopped mob whose operation binding of one member is held
/// (a gated step of its post-commit work): a Spawn is admitted and answered
/// while the binding is held, and a registration reload of another member
/// stays deferred until the lift completes and is answered after it.
///
/// RED on the base (8086477277): the lift's topology fence stayed up until
/// its post-commit work finished, and every Spawn was deferred behind it, so
/// the bounded Spawn timed out with the binding still held.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_lift_binding_admits_a_spawn_and_defers_a_reload_until_the_lift_completes() {
    let mob = create_isolation_mob(2).await;
    mob.handle.stop().await.expect("stop the mob");
    let (entered, release) = crate::runtime::actor::hold_explicit_resume_member_binding_for_test(
        mob.handle.mob_id(),
        mob.member(0),
    );
    let lift = tokio::spawn({
        let handle = mob.handle.clone();
        async move { handle.resume().await }
    });
    tokio::time::timeout(STEP, entered)
        .await
        .expect("the lift reaches the held binding")
        .expect("binding hold armed");

    let reload = tokio::spawn({
        let handle = mob.handle.clone();
        let member = mob.member(1).clone();
        async move { handle.reload_member_registration(&member).await }
    });
    let spawned = tokio::time::timeout(
        STEP,
        mob.handle.spawn(
            ProfileName::from("worker"),
            AgentIdentity::from("w-spawned-during-lift"),
            None,
        ),
    )
    .await;
    // The reload was sent before the Spawn, and an admitted reload of a
    // healthy member is a quick no-op: give it time to answer if it was not
    // deferred.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let reload_answered_while_held = reload.is_finished();
    let lift_answered_while_held = lift.is_finished();
    // Released whatever was observed, so a failing row ends instead of
    // hanging.
    let _ = release.send(());

    let spawned = spawned
        .expect("the Spawn is answered while the lift's binding is held")
        .expect("the Spawn is admitted while the lift's binding is held");
    assert!(
        spawned.bridge_session_id().is_some(),
        "the spawned member is session-backed"
    );
    assert!(
        !lift_answered_while_held,
        "the lift was still in its post-commit work while the binding was held"
    );
    assert!(
        !reload_answered_while_held,
        "a registration reload waits for the whole resume"
    );
    tokio::time::timeout(STEP, lift)
        .await
        .expect("the lift answers once the binding is released")
        .expect("lift task")
        .expect("the lift succeeds");
    let reloaded = tokio::time::timeout(STEP, reload)
        .await
        .expect("the deferred reload is answered after the lift")
        .expect("reload task");
    assert!(
        reloaded.is_ok(),
        "the deferred reload runs once the lift completed: {reloaded:?}"
    );
    assert_eq!(
        mob.handle.status().await.expect("status"),
        MobState::Running,
        "the lift reached ready"
    );
    mob.handle
        .resolve_bridge_session_id(&AgentIdentity::from("w-spawned-during-lift"))
        .await
        .expect("the member spawned during the lift is a member");
}
