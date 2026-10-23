//! A mob's lifecycle as its handles observe it without asking its actor:
//! once the actor has stopped, the observation says why, as `status` does,
//! and the readers that keep the live phase say why that phase is right.

use super::*;
use crate::continuation::{MobContinuationResolver, MobHandleLookup};
use meerkat::{
    AddressObservation, ContinuationAddressResolver, ContinuationDelivery, ContinuationHandling,
    ContinuationKey, ContinuationOwner, ContinuationOwnerService, ContinuationProducer,
    ContinuationResultRef, ContinuationStatus,
};
use meerkat_runtime::{InMemoryRuntimeStore, LogicalRuntimeId, RuntimeDeliveryInbox};

struct OneMob(MobHandle);

#[async_trait::async_trait]
impl MobHandleLookup for OneMob {
    async fn mob_handle(&self, mob_id: &MobId) -> Option<MobHandle> {
        (self.0.mob_id() == mob_id).then(|| self.0.clone())
    }
}

/// A mob with one member, `w-1`, whose actor then fail-stops in its loop:
/// a flow's terminal event cannot be appended, so the durable outcome stays
/// uncertain (the in-loop fail-stop status() reports).
async fn mob_whose_actor_fail_stopped_in_its_loop() -> MobHandle {
    let events = Arc::new(FaultInjectedMobEventStore::new());
    events.fail_appends_for("FlowFailed").await;
    let (handle, service, runs, _specs, _runtime_metadata, _identity, _identity_status) =
        create_test_mob_with_recoverable_fault_events(
            sample_definition_with_single_step_flow(500, 8),
            events,
        )
        .await;
    handle
        .spawn(
            ProfileName::from("worker"),
            AgentIdentity::from("w-1"),
            None,
        )
        .await
        .expect("spawn worker");
    service.set_flow_turn_fail(true);
    let run_id = handle
        .run_flow(FlowId::from("demo"), serde_json::json!({}))
        .await
        .expect("run flow");
    wait_for_run_terminal_in_store(runs.as_ref(), &run_id, Duration::from_secs(3)).await;
    wait_for_actor_fail_stop(&handle).await;
    handle
}

/// The cause `status()` reports for a fail-stopped actor.
async fn reported_fail_stop_cause(handle: &MobHandle) -> Arc<MobError> {
    match handle.status().await {
        Err(MobError::SharedLifecycleFailure(cause)) => cause,
        other => panic!("a fail-stopped actor reports why, typed: {other:?}"),
    }
}

fn assert_observed_cause(
    reader: &str,
    observation: &crate::MobLifecycleObservation,
    reported: &Arc<MobError>,
) {
    assert!(
        matches!(observation.phase, MobState::Creating | MobState::Running),
        "{reader}: the phase stays the last one the actor published: {:?}",
        observation.phase
    );
    let failure = observation.failure.as_ref().unwrap_or_else(|| {
        panic!(
            "{reader}: a fail-stopped actor must be observed with why it stopped, not as a live \
             {:?}",
            observation.phase
        )
    });
    assert!(
        Arc::ptr_eq(failure, reported),
        "{reader}: the observed failure is the cause status() reports: {failure:?} vs \
         {reported:?}"
    );
}

/// The handle's own observation of an actor that fail-stopped in its loop
/// carries the recorded cause, the one `status()` reports.
#[tokio::test]
async fn a_handle_observes_an_in_loop_fail_stop_with_its_recorded_cause() {
    let handle = mob_whose_actor_fail_stopped_in_its_loop().await;
    let reported = reported_fail_stop_cause(&handle).await;
    assert!(
        matches!(
            reported.as_ref(),
            MobError::DurabilityFailStop { cause, .. }
                if cause.to_string().contains("FlowFailed append failed")
        ),
        "the in-loop fail-stop's record carries its own cause: {reported:?}"
    );
    assert_observed_cause("MobHandle", &handle.lifecycle_observation(), &reported);
}

/// A read handle (the host's preactivation view) observes an actor that
/// fail-stopped in its startup recovery with the recorded cause.
#[tokio::test]
async fn a_read_handle_observes_a_startup_fail_stop_with_its_recorded_cause() {
    let definition = with_unique_mob_id(sample_definition(), "observed-startup-fail-stop");
    crate::runtime::actor::fail_stop_next_actor_start_for_test(&definition.id);
    let (handle, _service) = create_test_mob(definition).await;
    let mut task = handle.actor_task_watch();
    assert_eq!(
        task.wait().await,
        crate::runtime::MobActorTaskWait::Returned
    );
    let reported = reported_fail_stop_cause(&handle).await;
    assert_observed_cause(
        "MobReadHandle",
        &handle.read_handle().lifecycle_observation(),
        &reported,
    );
}

/// An actor task that ended while its phase was live and recorded no cause
/// (here its loop panicked) is observed with the closed-actor error, the
/// error `status()` reports for it.
#[tokio::test]
async fn an_actor_task_that_ended_live_without_a_record_is_observed_with_the_closed_actor_error() {
    let definition = with_unique_mob_id(sample_definition(), "observed-lost-actor");
    let (handle, _service) = create_test_mob(definition).await;
    let mut task = handle.actor_task_watch();
    crate::runtime::actor::panic_actor_loop_for_test(handle.mob_id());
    // Any wake takes the loop to its next turn, where the seam panics.
    let _phase = handle
        .enqueue_actor_command_for_test(|reply_tx| MobCommand::QueryPhase { reply_tx })
        .await;
    assert_eq!(task.wait().await, crate::runtime::MobActorTaskWait::Lost);

    let reported = handle
        .status()
        .await
        .expect_err("a lost actor's live phase is not lifecycle truth");
    let observation = handle.lifecycle_observation();
    assert!(
        matches!(observation.phase, MobState::Creating | MobState::Running),
        "the phase stays the last one the actor published: {:?}",
        observation.phase
    );
    let failure = observation
        .failure
        .as_ref()
        .expect("an actor task that ended live must be observed as stopped");
    assert!(
        matches!(failure.as_ref(), MobError::Internal(_)),
        "no cause was recorded, so the failure is the closed-actor error: {failure:?}"
    );
    assert_eq!(
        failure.to_string(),
        reported.to_string(),
        "the observed failure is the error status() reports"
    );
}

/// The address of a fail-stopped mob's member is observed as `MayServe`,
/// not `Retired`: `Retired` means the records prove the incarnation never
/// serves again, and recovery resumes this one, so a continuation bound to
/// it stays pending for it.
#[tokio::test]
async fn a_fail_stopped_mobs_member_address_is_still_observed_as_may_serve() {
    let handle = mob_whose_actor_fail_stopped_in_its_loop().await;
    let identity = AgentIdentity::from("w-1");
    let entry = handle
        .roster()
        .await
        .get_by_identity(&identity)
        .cloned()
        .expect("the member stays in the roster");
    let address: LogicalRuntimeId = meerkat::member_delivery_address(
        handle.mob_id().as_str(),
        handle.incarnation(),
        identity.as_str(),
        entry.generation.get(),
    )
    .expect("member address");
    let resolver = MobContinuationResolver::new(Arc::new(OneMob(handle.clone())));
    assert!(
        matches!(
            handle.status().await,
            Err(MobError::SharedLifecycleFailure(_))
        ),
        "the mob's actor has fail-stopped"
    );
    assert_eq!(
        resolver
            .observe_address(&address)
            .await
            .expect("observe the address"),
        AddressObservation::MayServe
    );
}

/// A continuation submitted through a fail-stopped mob's handle commits
/// durably for the member's incarnation, which recovery resumes; refusing it
/// would drop the producer's completion.
#[tokio::test]
async fn a_continuation_submitted_through_a_fail_stopped_mobs_handle_still_commits() {
    let handle = mob_whose_actor_fail_stopped_in_its_loop().await;
    let identity = AgentIdentity::from("w-1");
    let resolver = Arc::new(MobContinuationResolver::new(Arc::new(OneMob(
        handle.clone(),
    ))));
    let continuations = ContinuationOwnerService::new(
        RuntimeDeliveryInbox::new(Arc::new(InMemoryRuntimeStore::new())),
        resolver,
        Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()),
    );
    let delivery = ContinuationDelivery {
        key: ContinuationKey::new("after-fail-stop").expect("key"),
        result: ContinuationResultRef {
            producer: ContinuationProducer::Host {
                namespace: "tasks".into(),
            },
            producer_id: "op-1".into(),
            result_digest: "sha256:result".into(),
            summary: None,
        },
        body: "result".into(),
        handling: ContinuationHandling::Queue,
    };
    let receipt = handle
        .submit_continuation(&continuations, &identity, delivery, 1)
        .await
        .expect("a fail-stopped mob is not retired, so its handle submits");
    let owner = ContinuationOwner::Member {
        mob_id: handle.mob_id().as_str().to_string(),
        identity: identity.as_str().to_string(),
    };
    let status = continuations
        .continuation_status(
            &owner,
            &ContinuationKey::new("after-fail-stop").expect("key"),
        )
        .await
        .expect("continuation status");
    assert!(
        !matches!(
            status,
            ContinuationStatus::NotCommitted | ContinuationStatus::Stranded { .. }
        ),
        "the continuation is committed for the member's incarnation: {status:?} ({receipt:?})"
    );
}
