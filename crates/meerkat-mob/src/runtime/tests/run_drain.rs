//! A bounded drain of a Stopped mob's runs: the verb's refusal, the
//! snapshot mapping and the refusal's classification.

use super::*;
use crate::runtime::run_drain::{
    DrainMemberFacts, DrainSnapshot, DrainSnapshotMember, DrainTarget, drain, drain_target,
};
use crate::runtime::stop_report::{
    MemberRunDrain, MemberRunDrainOutcome, MemberRunStarts, NotHoldableReason,
};
use meerkat_core::lifecycle::RunId;

fn run_id() -> RunId {
    RunId(uuid::Uuid::new_v4())
}

fn local(session_id: &SessionId) -> DrainMemberFacts {
    DrainMemberFacts {
        local_session: Some(session_id.clone()),
        ..DrainMemberFacts::default()
    }
}

#[tokio::test]
async fn the_drain_refuses_a_running_mob() {
    let (handle, _service) = create_test_mob_with_runtime_adapter(sample_definition()).await;
    let refused = handle
        .drain_stopped_runs(meerkat_core::time_compat::Instant::now() + Duration::from_secs(1))
        .await;
    assert!(
        matches!(
            refused,
            Err(MobError::DrainRequiresStoppedMob {
                state: MobState::Running
            })
        ),
        "a drain is refused, typed, on a Running mob: {refused:?}"
    );
    assert_eq!(
        handle.status().await.expect("status"),
        MobState::Running,
        "the refusal leaves the mob as it was"
    );

    handle.stop().await.expect("stop");
    let report = handle
        .drain_stopped_runs(meerkat_core::time_compat::Instant::now() + Duration::from_secs(1))
        .await
        .expect("a Stopped mob drains");
    assert!(
        report.members.is_empty(),
        "a mob without members reports none: {report:?}"
    );
}

#[test]
fn the_drain_maps_each_member_by_what_the_snapshot_saw() {
    let session_id = SessionId::new();
    let run = run_id();

    assert_eq!(
        drain_target(DrainMemberFacts::default(), Some(&MemberRunStarts::Held)),
        DrainTarget::Report(MemberRunDrain::NotLocal),
        "a member without a local session is not local"
    );
    assert_eq!(
        drain_target(
            DrainMemberFacts {
                reload_required: true,
                current_run: Some(run.clone()),
                ..local(&session_id)
            },
            Some(&MemberRunStarts::Held),
        ),
        DrainTarget::Report(MemberRunDrain::ReloadRequired),
        "a durability-degraded registration records no run outcome"
    );
    assert_eq!(
        drain_target(local(&session_id), Some(&MemberRunStarts::ReloadRequired)),
        DrainTarget::Report(MemberRunDrain::ReloadRequired),
        "so does one the Stop reported reload-required"
    );
    assert_eq!(
        drain_target(
            DrainMemberFacts {
                live_channel_active: true,
                current_run: Some(run.clone()),
                ..local(&session_id)
            },
            Some(&MemberRunStarts::Held),
        ),
        DrainTarget::Report(MemberRunDrain::LiveChannelOpen {
            run_id: Some(run.clone())
        }),
        "an open live channel is reported at once, with its run, and not waited on"
    );
    assert_eq!(
        drain_target(local(&session_id), Some(&MemberRunStarts::Held)),
        DrainTarget::Report(MemberRunDrain::NoRun),
        "a member without a current run has nothing to drain"
    );
    for starts in [
        Some(MemberRunStarts::Held),
        Some(MemberRunStarts::NotHoldable {
            reason: NotHoldableReason::ProvisionerLacksCapability,
        }),
        None,
    ] {
        assert_eq!(
            drain_target(
                DrainMemberFacts {
                    current_run: Some(run.clone()),
                    ..local(&session_id)
                },
                starts.as_ref(),
            ),
            DrainTarget::Wait {
                session_id: session_id.clone(),
                run_id: run.clone(),
            },
            "the captured run is waited for whatever the starts ({starts:?})"
        );
    }
}

#[tokio::test]
async fn the_drain_reports_each_member_with_the_starts_its_stop_recorded() {
    let held = AgentIdentity::from("held");
    let not_holdable = AgentIdentity::from("not-holdable");
    let unrecorded = AgentIdentity::from("unrecorded");
    let not_holdable_starts = MemberRunStarts::NotHoldable {
        reason: NotHoldableReason::ProvisionerLacksCapability,
    };
    let report = drain(
        DrainSnapshot {
            members: vec![
                DrainSnapshotMember {
                    identity: held.clone(),
                    starts: Some(MemberRunStarts::Held),
                    target: DrainTarget::Report(MemberRunDrain::NoRun),
                },
                DrainSnapshotMember {
                    identity: not_holdable.clone(),
                    starts: Some(not_holdable_starts.clone()),
                    target: DrainTarget::Report(MemberRunDrain::NoRun),
                },
                DrainSnapshotMember {
                    identity: unrecorded.clone(),
                    starts: None,
                    target: DrainTarget::Report(MemberRunDrain::NotLocal),
                },
            ],
            machine: None,
        },
        meerkat_core::time_compat::Instant::now(),
    )
    .await;
    let outcome = |identity: &AgentIdentity| report.members.get(identity).cloned();
    assert_eq!(
        outcome(&held),
        Some(MemberRunDrainOutcome {
            run: MemberRunDrain::NoRun,
            starts: Some(MemberRunStarts::Held),
        })
    );
    assert_eq!(
        outcome(&not_holdable),
        Some(MemberRunDrainOutcome {
            run: MemberRunDrain::NoRun,
            starts: Some(not_holdable_starts),
        }),
        "a member whose starts are not held says so: it may start runs the drain does not cover"
    );
    assert_eq!(
        outcome(&unrecorded),
        Some(MemberRunDrainOutcome {
            run: MemberRunDrain::NotLocal,
            starts: None,
        }),
        "no Stop outcome recorded in this process is None, not a guess"
    );
}

#[test]
fn the_drain_refusal_is_an_in_process_lifecycle_refusal() {
    let refusal = MobError::DrainRequiresStoppedMob {
        state: MobState::Running,
    };
    assert_eq!(
        refusal.to_string(),
        "draining runs requires a stopped mob, but the mob is Running"
    );
    assert_eq!(
        refusal.wire_error_code(),
        None,
        "like InvalidTransition, the refusal has no wire code: the verb has no wire surface"
    );
    assert!(refusal.wire_detail().is_none());
}
