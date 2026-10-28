//! Retryable restore rows: a restore that waits on work still in flight on
//! its member's session (the takeover of the attachment an earlier handle
//! left), or on a store that cannot serve it now, never breaks the member;
//! and the explicit resume's seam retires a Dormant member's retained
//! attachment before its durable Resume, as it retires every foreign one.

use super::*;
use crate::machines::mob_machine as dsl;
use crate::runtime::MemberRestorePolicy;

const ROW_STEP: Duration = Duration::from_secs(30);

fn phase_of(
    handle: &MobHandle,
    identity: &AgentIdentity,
) -> Option<dsl::MemberMaterializationPhase> {
    handle.member_materialization_for_test(identity)
}

/// One turn-driven worker spawned live on a session service the whole row
/// shares, whose mob's actor then went away (after a stop when `stop`): the
/// runtime still serves the worker's session with that handle's attachment
/// when a new handle is reconstructed from the same storage.
struct RetainedAttachmentMob {
    service: Arc<MockSessionService>,
    adapter: Arc<meerkat_runtime::MeerkatMachine>,
    events: Arc<dyn MobEventStore>,
    runtime_metadata: Arc<dyn MobRuntimeMetadataStore>,
    identity: AgentIdentity,
    session: SessionId,
    foreign: meerkat_runtime::RuntimeExecutorAttachmentWitness,
}

impl RetainedAttachmentMob {
    async fn new(label: &str, stop: bool) -> Self {
        let service = Arc::new(MockSessionService::new());
        let adapter = service.enable_runtime_adapter();
        let storage = MobStorage::in_memory();
        let events = storage.events.clone();
        let runtime_metadata = storage.runtime_metadata.clone();
        let handle = MobBuilder::new(with_unique_mob_id(sample_definition(), label), storage)
            .with_session_service(service.clone())
            .create()
            .await
            .expect("create the mob");
        let identity = AgentIdentity::from("retained-worker");
        let session = handle
            .spawn_with_options(
                ProfileName::from("worker"),
                identity.clone(),
                None,
                Some(crate::MobRuntimeMode::TurnDriven),
                None,
            )
            .await
            .expect("spawn the worker")
            .bridge_session_id()
            .cloned()
            .expect("the worker is session-backed");
        if stop {
            handle.stop().await.expect("stop the mob");
        }
        crash_stop_and_release_routes(handle).await;
        let foreign = adapter
            .current_executor_attachment_witness(&session)
            .await
            .expect("the runtime still serves the worker with the old handle's attachment");
        Self {
            service,
            adapter,
            events,
            runtime_metadata,
            identity,
            session,
            foreign,
        }
    }

    /// A new handle of the mob over the same service, under `policy`.
    async fn reconstruct(&self, policy: MemberRestorePolicy) -> MobHandle {
        let storage = MobStorage::with_events_and_runtime_metadata(
            self.events.clone(),
            self.runtime_metadata.clone(),
        );
        tokio::time::timeout(
            ROW_STEP,
            Box::pin(
                MobBuilder::for_resume(storage)
                    .with_session_service(self.service.clone())
                    .notify_orchestrator_on_resume(false)
                    .with_member_restore_policy(policy)
                    .resume(),
            ),
        )
        .await
        .expect("the reconstruction answers")
        .expect("the reconstruction succeeds")
    }
}

/// A wake whose restore cannot take the member's retained attachment over
/// within its budget (another holder keeps the session's turn boundary)
/// ends retryable: the member is Dormant again, never Broken, and its caller
/// gets a retryable refusal. Once the boundary is free a later wake joins the
/// takeover still in flight and materializes the member once, on a new
/// attachment.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wake_whose_takeover_outlasts_its_budget_ends_retryable_not_broken() {
    let mob = RetainedAttachmentMob::new("retryable-takeover-wake", false).await;
    let resumed = mob.reconstruct(MemberRestorePolicy::OnDemand).await;
    resumed
        .status()
        .await
        .expect("the reconstructed mob answers");
    assert_eq!(
        phase_of(&resumed, &mob.identity),
        Some(dsl::MemberMaterializationPhase::Dormant),
        "the lazy start materialized nothing"
    );
    let creates_before = mob.service.recorded_create_requests().await.len();

    // Another holder keeps every session's turn boundary.
    let gate = mob.service.install_non_reentrant_turn_finalization_gate();
    let held = Arc::clone(&gate).lock_owned().await;
    let refused = tokio::time::timeout(ROW_STEP, resumed.ensure_member_live(&mob.identity))
        .await
        .expect("the wake answers once its takeover budget is spent");
    assert!(
        matches!(
            &refused,
            Err(MobError::MemberWakeRefused { refusal, .. }) if refusal.is_retryable()
        ),
        "a takeover still in flight ends the wake retryable: {refused:?}"
    );
    resumed.status().await.expect("the mob answers");
    assert_eq!(
        phase_of(&resumed, &mob.identity),
        Some(dsl::MemberMaterializationPhase::Dormant),
        "the member is Dormant again"
    );
    let status = resumed
        .member_status(&mob.identity)
        .await
        .expect("the member's status");
    assert_ne!(
        status.status,
        crate::runtime::handle::MobMemberStatus::Broken,
        "a retryable restore never breaks the member: {status:?}"
    );

    drop(held);
    tokio::time::timeout(ROW_STEP, resumed.ensure_member_live(&mob.identity))
        .await
        .expect("the later wake answers")
        .expect("the later wake joins the takeover and materializes the member");
    resumed.status().await.expect("the mob answers");
    assert_eq!(
        phase_of(&resumed, &mob.identity),
        Some(dsl::MemberMaterializationPhase::Live)
    );
    let replacement = mob
        .adapter
        .current_executor_attachment_witness(&mob.session)
        .await
        .expect("the member is served by a new attachment");
    assert_ne!(
        replacement, mob.foreign,
        "the old handle's attachment is replaced, never adopted"
    );
    assert_eq!(
        mob.service.recorded_create_requests().await.len(),
        creates_before + 1,
        "the member is materialized once"
    );
}

/// The explicit resume of a reconstructed stopped mob retires the attachment
/// an earlier handle left serving a Dormant member before its durable
/// Resume, and does nothing else for that member: under the on-demand policy
/// it stays Dormant and nothing is materialized.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_resume_retires_a_dormant_members_retained_attachment_and_keeps_it_dormant() {
    let mob = RetainedAttachmentMob::new("retained-attachment-lift", true).await;
    let resumed = mob.reconstruct(MemberRestorePolicy::OnDemand).await;
    assert_eq!(
        resumed
            .status()
            .await
            .expect("the reconstructed mob answers"),
        MobState::Stopped
    );
    assert_eq!(
        phase_of(&resumed, &mob.identity),
        Some(dsl::MemberMaterializationPhase::Dormant)
    );
    let creates_before = mob.service.recorded_create_requests().await.len();

    tokio::time::timeout(ROW_STEP, resumed.resume())
        .await
        .expect("the explicit resume answers")
        .expect("the explicit resume succeeds");
    assert_eq!(
        resumed.status().await.expect("the mob answers"),
        MobState::Running
    );
    assert!(
        mob.adapter
            .current_executor_attachment_witness(&mob.session)
            .await
            .is_none(),
        "the seam retired the old handle's attachment"
    );
    assert_eq!(
        phase_of(&resumed, &mob.identity),
        Some(dsl::MemberMaterializationPhase::Dormant),
        "the member stays Dormant"
    );
    assert_eq!(
        mob.service.recorded_create_requests().await.len(),
        creates_before,
        "nothing is materialized"
    );
}

/// `count` turn-driven workers built on a session service, their mob stopped
/// and its process gone: a fresh service holds their durable sessions only,
/// and the stopped mob is reconstructed from its storage under the Eager
/// policy, every member Dormant.
async fn built_stopped_mob(
    label: &str,
    count: usize,
) -> (
    MobHandle,
    Arc<MockSessionService>,
    Vec<(AgentIdentity, SessionId)>,
) {
    let service = Arc::new(MockSessionService::new());
    let _ = service.enable_runtime_adapter();
    let storage = MobStorage::in_memory();
    let events = storage.events.clone();
    let runtime_metadata = storage.runtime_metadata.clone();
    let handle = MobBuilder::new(with_unique_mob_id(sample_definition(), label), storage)
        .with_session_service(service.clone())
        .create()
        .await
        .expect("create the mob");
    let mut members = Vec::with_capacity(count);
    for index in 0..count {
        let identity = AgentIdentity::from(format!("lifted-{index}"));
        let mut spec = SpawnMemberSpec::new("worker", identity.clone());
        spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
        handle.spawn_spec(spec).await.expect("spawn a worker");
        let session = handle
            .resolve_bridge_session_id(&identity)
            .await
            .expect("a session-backed worker");
        members.push((identity, session));
    }
    handle.stop().await.expect("stop the mob");
    let restarted = Arc::new(service.cold_restart_preserving_durable_state().await);
    let _ = restarted.enable_runtime_adapter();
    crash_stop_and_release_routes(handle).await;
    drop(service);
    let resumed = tokio::time::timeout(
        ROW_STEP,
        Box::pin(
            MobBuilder::for_resume(MobStorage::with_events_and_runtime_metadata(
                events,
                runtime_metadata,
            ))
            .with_session_service(restarted.clone())
            .notify_orchestrator_on_resume(false)
            .with_member_restore_policy(MemberRestorePolicy::Eager)
            .resume(),
        ),
    )
    .await
    .expect("the reconstruction answers")
    .expect("the reconstruction succeeds");
    assert_eq!(
        resumed
            .status()
            .await
            .expect("the reconstructed mob answers"),
        MobState::Stopped
    );
    (resumed, restarted, members)
}

/// The lift of a stopped mob rebuilds the members its Eager plan
/// materializes through the explicit-resume work path. One member's durable
/// observation meets a store that cannot serve it now: that member's attempt
/// ends retryable, never Broken, the explicit resume answers the typed store
/// error, every other member is Live, and the member's owed retry
/// materializes it once the store serves it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lifted_member_whose_durable_observation_fails_is_retried_and_the_lift_answers_the_store_error()
 {
    let (resumed, service, members) = built_stopped_mob("lift-observation-store-failure", 3).await;
    let (failing, failing_session) = members[1].clone();
    service.fail_next_resume_authority_observes_for(&failing_session, 1);

    let lifted = tokio::time::timeout(ROW_STEP, resumed.resume())
        .await
        .expect("the explicit resume answers");
    let error = lifted.expect_err("the lift answers the member's store error");
    let cause = match &error {
        MobError::SharedLifecycleFailure(inner) => inner.as_ref(),
        other => other,
    };
    assert!(
        matches!(cause, MobError::SessionError(SessionError::Store(_))),
        "a typed store error: {error:?}"
    );
    assert_eq!(
        resumed.status().await.expect("the mob answers"),
        MobState::Running
    );
    let status = resumed
        .member_status(&failing)
        .await
        .expect("the member's status");
    assert_ne!(
        status.status,
        crate::runtime::handle::MobMemberStatus::Broken,
        "a store that cannot serve the member now never breaks it: {status:?}"
    );
    for (identity, _) in members.iter().filter(|(identity, _)| identity != &failing) {
        assert_eq!(
            phase_of(&resumed, identity),
            Some(dsl::MemberMaterializationPhase::Live),
            "{identity} is Live"
        );
    }
    let mut published = resumed.machine_state_watch_rx.clone();
    let failing_id = dsl::AgentIdentity::from_domain(&failing);
    let retried = tokio::time::timeout(
        ROW_STEP,
        published.wait_for(|state| {
            state.member_materialization.get(&failing_id)
                == Some(&dsl::MemberMaterializationPhase::Live)
        }),
    )
    .await
    .is_ok_and(|seen| seen.is_ok());
    assert!(
        retried,
        "the member's owed retry materializes it once the store serves it"
    );
}
