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
