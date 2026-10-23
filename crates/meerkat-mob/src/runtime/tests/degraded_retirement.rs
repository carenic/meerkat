//! A member retirement whose member session is durability-degraded when the
//! retirement reaches its ingress detach.
//!
//! A session that degrades after the retirement's quiesce stage (for example
//! while the quiesce waited on a boundary commit that then passed its
//! deadline) refuses the ingress detach until a reload replaces its
//! registration. The stage reloads it first, as the quiesce and a destroy do,
//! and detaches the cold successor. A reload the runtime refuses, or a
//! session that degrades after the stage's reload check, sticks the
//! retirement with the typed `MemberReloadRepairBlocked`, which a redrive
//! completes.

use super::*;
use crate::runtime::actor::retirement_stage_hold;
use crate::runtime::actor::retirement_stage_test_hooks as stages;

/// Bound for test steps that must happen promptly; it only turns a hang into
/// a failure.
const STEP: Duration = Duration::from_secs(10);

/// Degrade `session`'s registration and start its runtime loop's teardown,
/// as a boundary commit that failed would, so a reload can claim the loop's
/// handoff.
async fn degrade_with_teardown(mob: &IsolationMob, session: &SessionId) {
    assert!(
        mob.adapter
            .force_session_durability_reload_required_for_test(session)
            .await,
        "the member's session degrades"
    );
    start_degraded_teardown(mob, session).await;
}

/// A degraded admission starts the degraded runtime loop's teardown.
async fn start_degraded_teardown(mob: &IsolationMob, session: &SessionId) {
    meerkat_runtime::SessionServiceRuntimeExt::accept_input(
        mob.adapter.as_ref(),
        session,
        meerkat_runtime::Input::Prompt(meerkat_runtime::PromptInput::new(
            "start the degraded teardown",
            None,
        )),
    )
    .await
    .expect_err("a degraded admission is refused and starts the teardown");
}

/// Drive `member`'s retirement, a plain retire or a redrive, until it answers
/// other than in progress.
async fn settle(mob: &IsolationMob, member: &AgentIdentity, redrive: bool) -> Result<(), MobError> {
    tokio::time::timeout(STEP, async {
        loop {
            let answer = if redrive {
                mob.handle.redrive_retirement(member.clone()).await
            } else {
                mob.handle.retire(member.clone()).await
            };
            match answer {
                Err(MobError::MemberRetirementInProgress { .. }) => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                other => return other,
            }
        }
    })
    .await
    .expect("the retirement settles within the step bound")
}

/// Start `member`'s retirement and hold it at its ingress detach, before the
/// stage reads the session's durability gate; the quiesce ran first.
async fn retire_held_at_the_ingress_detach(
    mob: &IsolationMob,
    member: &AgentIdentity,
) -> (
    Arc<retirement_stage_hold::RetirementStageHold>,
    tokio::task::JoinHandle<Result<(), MobError>>,
) {
    let hold = retirement_stage_hold::install(member, "retire-ingress-detach");
    let retire = {
        let handle = mob.handle.clone();
        let member = member.clone();
        tokio::spawn(async move { handle.retire(member).await })
    };
    tokio::time::timeout(STEP, hold.entered())
        .await
        .expect("the retirement reaches its ingress detach");
    (hold, retire)
}

async fn assert_retired(mob: &IsolationMob, member: &AgentIdentity, session: &SessionId) {
    assert!(
        mob.handle
            .get_member(member)
            .await
            .expect("the roster answers")
            .is_none(),
        "the retired member left the roster"
    );
    assert!(
        mob.adapter
            .current_session_registration_witness(session)
            .await
            .is_none(),
        "no registration of the member's session remains"
    );
}

/// A retirement whose member degraded after its quiesce reloads the member
/// at its ingress detach and retires the cold successor: no manual reload
/// and no redrive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_that_degrades_after_its_quiesce_is_reloaded_at_its_ingress_detach() {
    let mob = create_isolation_mob(1).await;
    let member = mob.member(0).clone();
    let session = mob.session(0).clone();
    let (hold, retire) = retire_held_at_the_ingress_detach(&mob, &member).await;
    degrade_with_teardown(&mob, &session).await;
    hold.release();

    let first = tokio::time::timeout(STEP, retire)
        .await
        .expect("the retire answers")
        .expect("join the retire");
    match first {
        Ok(()) => {}
        Err(error) if error.is_retirement_in_progress() => settle(&mob, &member, false)
            .await
            .expect("the retirement reloads the member and retires it"),
        Err(error) => panic!("the retirement reloads the member and retires it: {error:?}"),
    }
    assert_eq!(
        stages::ingress_stage_reloads(&session),
        1,
        "the ingress detach reloaded the degraded member once"
    );
    assert_retired(&mob, &member, &session).await;
}

/// A retirement whose member is not degraded asks for no reload at its
/// ingress detach: the stage reads the gate and detaches as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retirement_of_a_member_that_is_not_degraded_asks_no_reload_at_its_ingress_detach() {
    let mob = create_isolation_mob(1).await;
    let member = mob.member(0).clone();
    let session = mob.session(0).clone();
    settle(&mob, &member, false)
        .await
        .expect("the retirement completes");
    assert_eq!(
        stages::ingress_stage_reloads(&session),
        0,
        "no reload was asked for"
    );
    assert_retired(&mob, &member, &session).await;
}

/// A reload the runtime refuses as repair-blocked at the ingress detach
/// sticks the retirement there with the typed `MemberReloadRepairBlocked`,
/// carrying the runtime's reason. A plain redrive then reloads the member and
/// completes the retirement: the variant's promise holds at this stage.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reload_refused_at_the_ingress_detach_sticks_repair_blocked_until_a_redrive() {
    let mob = create_isolation_mob(1).await;
    let member = mob.member(0).clone();
    let session = mob.session(0).clone();
    let (hold, retire) = retire_held_at_the_ingress_detach(&mob, &member).await;
    degrade_with_teardown(&mob, &session).await;
    stages::refuse_next_ingress_stage_reload(&session);
    hold.release();
    let _first = tokio::time::timeout(STEP, retire)
        .await
        .expect("the retire answers")
        .expect("join the retire");

    let stuck = settle(&mob, &member, false).await;
    let Err(MobError::MemberRetirementStuck { stage, cause, .. }) = &stuck else {
        panic!("a refused reload at the ingress detach sticks the retirement: {stuck:?}");
    };
    assert_eq!(stage, "retire-ingress-detach");
    match cause.as_ref() {
        MobError::MemberReloadRepairBlocked {
            session_id,
            reason,
            evidence_digest,
        } => {
            assert_eq!(session_id, &session);
            assert_eq!(reason, stages::INJECTED_RELOAD_REFUSAL);
            assert_eq!(evidence_digest, &None);
        }
        other => panic!("the stuck cause is MemberReloadRepairBlocked: {other:?}"),
    }
    assert_eq!(stages::ingress_stage_reloads(&session), 1);

    settle(&mob, &member, true)
        .await
        .expect("a plain redrive reloads the member and completes the retirement");
    assert_retired(&mob, &member, &session).await;
}

/// A session that degrades after the ingress stage's reload check, before
/// its detach, refuses the detach repair-blocked. The retirement sticks with
/// the typed `MemberReloadRepairBlocked` (the refusal's own reason), not an
/// internal error, and a plain redrive reloads the member and completes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_that_degrades_after_the_ingress_reload_check_sticks_until_a_redrive() {
    let mob = create_isolation_mob(1).await;
    let member = mob.member(0).clone();
    let session = mob.session(0).clone();
    stages::degrade_before_next_ingress_detach(&session);

    let stuck = settle(&mob, &member, false).await;
    let Err(MobError::MemberRetirementStuck { stage, cause, .. }) = &stuck else {
        panic!("a detach refused on a degraded session sticks the retirement: {stuck:?}");
    };
    assert_eq!(stage, "retire-ingress-detach");
    match cause.as_ref() {
        MobError::MemberReloadRepairBlocked {
            session_id, reason, ..
        } => {
            assert_eq!(session_id, &session);
            assert!(
                reason.contains("durability reload required"),
                "the reason is the runtime's refusal: {reason}"
            );
        }
        other => panic!("the stuck cause is MemberReloadRepairBlocked: {other:?}"),
    }
    assert_eq!(
        stages::ingress_stage_reloads(&session),
        0,
        "the session was not degraded at the stage's reload check"
    );

    start_degraded_teardown(&mob, &session).await;
    settle(&mob, &member, true)
        .await
        .expect("a plain redrive reloads the member and completes the retirement");
    assert_retired(&mob, &member, &session).await;
}
