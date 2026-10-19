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
/// published is unregistered. Nobody took the registration over, so the
/// use gate lets the discard and the unregister through.
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
    let registration_use = mob.adapter.session_registration_use(&registration).await;
    assert!(
        registration_use.is_some(),
        "the current registration has a use"
    );
    let mut settlement =
        MemberUnregisterSettlement::new(registration, registration_use, None, &mob.adapter, None);
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

/// R1, the successor read after the fact: a settlement's reload-required
/// discard published a cold successor, and before the discard returned, a
/// later owner replaced that successor with a registration of its own. The
/// settlement follows only the successor its discard published, never the
/// registration current once the discard returned: the later owner's
/// registration stays, and the settlement settles on the published
/// successor's end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_degraded_unregister_follows_only_the_successor_its_discard_published() {
    let mob = create_isolation_mob(1).await;
    mob.degrade_member(0).await;
    // The store recovers; the registration stays degraded until disposed.
    mob.store.fail_commit(mob.session(0), false);
    let session = mob.session(0).clone();
    let registration = mob
        .adapter
        .current_session_registration_witness(&session)
        .await
        .expect("the degraded registration is current");
    let registration_use = mob.adapter.session_registration_use(&registration).await;
    let mut settlement =
        MemberUnregisterSettlement::new(registration, registration_use, None, &mob.adapter, None);
    let (published, release) = mob
        .adapter
        .arm_reload_required_discard_after_successor_publication_test_hook(session.clone());
    let settling = tokio::spawn(async move {
        let state = settlement.settle_within(Instant::now() + STEP).await;
        (settlement, state)
    });
    tokio::time::timeout(STEP, published)
        .await
        .expect("the settlement's discard publishes its successor")
        .expect("the discard reached its publication point");

    // A later owner replaces the published successor with its own
    // registration before the discard returns.
    let successor = mob
        .adapter
        .current_session_registration_witness(&session)
        .await
        .expect("the cold successor is current");
    assert!(
        mob.adapter
            .unregister_session_registration_until_terminal_if_current(&successor)
            .await
            .expect("unregister the successor"),
        "the successor is unregistered"
    );
    mob.adapter
        .register_session(session.clone())
        .await
        .expect("the later owner registers the session");
    let later = mob
        .adapter
        .current_session_registration_witness(&session)
        .await
        .expect("the later owner's registration is current");
    assert_ne!(later, successor);
    release.send(()).expect("release the paused discard");

    let (_settlement, state) = tokio::time::timeout(STEP + STEP, settling)
        .await
        .expect("the settlement returns")
        .expect("the settlement task");
    assert_eq!(
        mob.adapter
            .current_session_registration_witness(&session)
            .await
            .as_ref(),
        Some(&later),
        "the later owner's registration stays registered (settlement: {state:?})"
    );
    assert_eq!(
        state,
        MemberUnregisterState::Settled,
        "the successor the discard published is gone"
    );
}

/// A later owner's runtime executor that never runs anything.
struct IdleLaterOwnerExecutor;

#[async_trait::async_trait]
impl meerkat_core::lifecycle::CoreExecutor for IdleLaterOwnerExecutor {
    async fn apply(
        &mut self,
        _run_id: meerkat_core::RunId,
        _primitive: meerkat_core::lifecycle::run_primitive::RunPrimitive,
    ) -> Result<
        meerkat_core::lifecycle::core_executor::CoreApplyOutput,
        meerkat_core::lifecycle::core_executor::CoreExecutorError,
    > {
        Err(
            meerkat_core::lifecycle::core_executor::CoreExecutorError::Internal(
                "the later owner's idle fixture executor runs nothing".to_string(),
            ),
        )
    }

    async fn cancel_after_boundary(
        &mut self,
        _reason: String,
    ) -> Result<(), meerkat_core::lifecycle::core_executor::CoreExecutorError> {
        Ok(())
    }

    async fn stop_runtime_executor(
        &mut self,
        _reason: String,
    ) -> Result<(), meerkat_core::lifecycle::core_executor::CoreExecutorError> {
        Ok(())
    }
}

/// A later owner's runtime executor whose runs reach their boundary. With the
/// store's boundary commit failing, that boundary degrades the registration
/// and the runtime loop stops the executor and exits.
struct CommittingLaterOwnerExecutor {
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl meerkat_core::lifecycle::CoreExecutor for CommittingLaterOwnerExecutor {
    async fn apply(
        &mut self,
        run_id: meerkat_core::RunId,
        primitive: meerkat_core::lifecycle::run_primitive::RunPrimitive,
    ) -> Result<
        meerkat_core::lifecycle::core_executor::CoreApplyOutput,
        meerkat_core::lifecycle::core_executor::CoreExecutorError,
    > {
        Ok(
            meerkat_core::lifecycle::core_executor::CoreApplyOutput::with_untyped_snapshot(
                meerkat_core::lifecycle::run_receipt::RunBoundaryReceiptDraft {
                    run_id,
                    boundary: meerkat_core::lifecycle::run_primitive::RunApplyBoundary::RunStart,
                    contributing_input_ids: primitive.contributing_input_ids().to_vec(),
                    conversation_digest: None,
                    message_count: 0,
                },
                None,
                None,
            ),
        )
    }

    async fn cancel_after_boundary(
        &mut self,
        _reason: String,
    ) -> Result<(), meerkat_core::lifecycle::core_executor::CoreExecutorError> {
        Ok(())
    }

    async fn stop_runtime_executor(
        &mut self,
        _reason: String,
    ) -> Result<(), meerkat_core::lifecycle::core_executor::CoreExecutorError> {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

/// R1 at the discard step, with the later owner's runtime loop still live:
/// Shutdown left the member's registration without a runtime loop and its
/// unregister not admitted. A later owner adopted that registration in place,
/// and the registration then became durability-degraded while the later
/// owner's loop still runs. Settling must not go after the later owner's
/// registration with a reload-required discard: it reports the takeover, and
/// the later owner's loop stays attached.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_unregister_never_discards_a_later_owners_degraded_registration_with_a_live_loop()
{
    let runtime_store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let adapter = Arc::new(
        MeerkatMachine::persistent(runtime_store, blob_store).expect("construct runtime authority"),
    );
    let session = SessionId::new();
    adapter
        .register_session(session.clone())
        .await
        .expect("register a registration without a runtime loop");
    let registration = adapter
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    let registration_use = adapter.session_registration_use(&registration).await;
    let mut settlement = MemberUnregisterSettlement::new(
        registration.clone(),
        registration_use,
        None,
        &adapter,
        None,
    );

    adapter
        .ensure_session_with_executor(session.clone(), Box::new(IdleLaterOwnerExecutor))
        .await
        .expect("a later owner adopts the registration in place");
    let adopted = adapter
        .current_executor_attachment_witness(&session)
        .await
        .expect("the later owner's runtime loop is attached");
    assert!(
        adapter
            .force_session_durability_reload_required_for_test(&session)
            .await,
        "the later owner's registration degrades while its loop runs"
    );

    let state = settlement
        .settle_within(Instant::now() + Duration::from_secs(2))
        .await;
    assert_eq!(
        adapter
            .current_session_registration_witness(&session)
            .await
            .as_ref(),
        Some(&registration),
        "the later owner's registration stays registered (settlement: {state:?})"
    );
    assert_eq!(
        adapter
            .current_executor_attachment_witness(&session)
            .await
            .as_ref(),
        Some(&adopted),
        "the later owner's loop stays attached (settlement: {state:?})"
    );
    assert_eq!(
        state,
        MemberUnregisterState::HandedOver,
        "the settlement reports the takeover instead of discarding the later owner's registration"
    );
}

/// R1 at the discard step, after the later owner's runtime loop exited: as
/// above, but the later owner's turn degraded the registration through a
/// failed boundary commit and its loop has exited and published its
/// teardown handoff. Settling must neither discard the later owner's
/// registration nor unregister a successor of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pending_unregister_never_discards_a_later_owners_degraded_registration_after_its_loop_exited()
 {
    use meerkat_runtime::service_ext::SessionServiceRuntimeExt as _;

    let store = FaultInjectingRuntimeStore::new();
    let runtime_store: Arc<dyn RuntimeStore> = Arc::clone(&store) as Arc<dyn RuntimeStore>;
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let adapter = Arc::new(
        MeerkatMachine::persistent(runtime_store, blob_store).expect("construct runtime authority"),
    );
    let session = SessionId::new();
    adapter
        .register_session(session.clone())
        .await
        .expect("register a registration without a runtime loop");
    let registration = adapter
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    let registration_use = adapter.session_registration_use(&registration).await;
    let mut settlement = MemberUnregisterSettlement::new(
        registration.clone(),
        registration_use,
        None,
        &adapter,
        None,
    );

    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    adapter
        .ensure_session_with_executor(
            session.clone(),
            Box::new(CommittingLaterOwnerExecutor {
                stopped: Arc::clone(&stopped),
            }),
        )
        .await
        .expect("a later owner adopts the registration in place");
    store.fail_commit(&session, true);
    adapter
        .accept_input(
            &session,
            meerkat_runtime::Input::Prompt(meerkat_runtime::PromptInput::new(
                "a later owner's turn whose boundary commit fails",
                None,
            )),
        )
        .await
        .expect("the later owner's input is admitted");
    {
        let adapter = Arc::clone(&adapter);
        let session = session.clone();
        let stopped = Arc::clone(&stopped);
        wait_until(
            "the later owner's registration degraded and its loop exited",
            Duration::from_secs(5),
            move || {
                let adapter = Arc::clone(&adapter);
                let session = session.clone();
                let stopped = Arc::clone(&stopped);
                async move {
                    adapter.durability_reload_required(&session).await.is_some()
                        && stopped.load(std::sync::atomic::Ordering::SeqCst)
                }
            },
        )
        .await;
    }
    store.fail_commit(&session, false);

    let state = settlement.settle_within(Instant::now() + STEP).await;
    assert_eq!(
        adapter
            .current_session_registration_witness(&session)
            .await
            .as_ref(),
        Some(&registration),
        "the later owner's degraded registration is neither discarded nor unregistered (settlement: {state:?})"
    );
    assert_eq!(
        state,
        MemberUnregisterState::HandedOver,
        "the settlement reports the takeover and unregisters nothing"
    );
}

/// The no-handoff case: a durability-degraded registration that no runtime
/// loop ever held has no runtime-loop teardown handoff, and nothing retains
/// its mutation fence. Its exact unregister is refused while the session is
/// degraded (the durability gate refuses the unregister's first durable
/// write), so the reload-required discard is the only way it settles. The
/// discard takes the fence itself and publishes a durability-ready cold
/// successor, which Shutdown's teardown and a pending unregister's
/// settlement each unregister.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_degraded_registration_without_a_teardown_handoff_settles_through_the_discard() {
    use crate::runtime::provisioner::{DegradedUnregisterStep, MemberSessionDisposalArc};

    async fn degraded_without_a_handoff(
        adapter: &Arc<MeerkatMachine>,
    ) -> (
        SessionId,
        meerkat_runtime::RuntimeSessionRegistrationWitness,
    ) {
        let session = SessionId::new();
        adapter
            .register_session(session.clone())
            .await
            .expect("register a registration without a runtime loop");
        assert!(
            adapter
                .force_session_durability_reload_required_for_test(&session)
                .await,
            "the registration is durability-degraded"
        );
        let registration = adapter
            .current_session_registration_witness(&session)
            .await
            .expect("the degraded registration is current");
        (session, registration)
    }

    let runtime_store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let adapter = Arc::new(
        MeerkatMachine::persistent(runtime_store, blob_store).expect("construct runtime authority"),
    );

    // The discard itself.
    let (session, registration) = degraded_without_a_handoff(&adapter).await;
    let discard = match MemberSessionDisposalArc::discard_degraded_registration_first(
        &adapter,
        None,
        &registration,
        None,
        Instant::now() + STEP,
    )
    .await
    {
        DegradedUnregisterStep::Replaced { successor, .. } => {
            let current = adapter.current_session_registration_witness(&session).await;
            let ready = adapter.durability_reload_required(&session).await.is_none();
            format!(
                "replaced (successor current: {}, durability ready: {ready})",
                successor != registration && current.as_ref() == Some(&successor)
            )
        }
        DegradedUnregisterStep::Unchanged => "unchanged".to_string(),
        DegradedUnregisterStep::HandedOver => "handed over".to_string(),
        DegradedUnregisterStep::TimedOut => "timed out".to_string(),
        DegradedUnregisterStep::Failed(failure) => format!("failed: {failure}"),
    };

    // Shutdown's teardown.
    let (teardown_target, _) = degraded_without_a_handoff(&adapter).await;
    let teardown = match tokio::time::timeout(
        STEP,
        crate::runtime::actor::teardown_session(
            &adapter,
            &Arc::new(crate::runtime::ops_adapter::MobOpsAdapter::new()),
            &teardown_target,
            None,
            Instant::now() + Duration::from_secs(5),
        ),
    )
    .await
    .expect("the teardown returns within its deadline")
    {
        ShutdownUnregisterOutcome::Unregistered => "unregistered".to_string(),
        ShutdownUnregisterOutcome::Pending { stage, .. } => format!("pending at {stage}"),
        ShutdownUnregisterOutcome::Failed(failure) => format!("failed: {failure}"),
    };
    let teardown_left = adapter
        .current_session_registration_witness(&teardown_target)
        .await
        .is_some();

    // A settlement of an unregister Shutdown reported pending.
    let (settled_session, settled_registration) = degraded_without_a_handoff(&adapter).await;
    let registration_use = adapter
        .session_registration_use(&settled_registration)
        .await;
    let mut settlement = MemberUnregisterSettlement::new(
        settled_registration,
        registration_use,
        None,
        &adapter,
        None,
    );
    let settled = settlement
        .settle_within(Instant::now() + Duration::from_secs(5))
        .await;
    let settlement_left = adapter
        .current_session_registration_witness(&settled_session)
        .await
        .is_some();

    assert_eq!(
        (
            discard.as_str(),
            teardown.as_str(),
            teardown_left,
            &settled,
            settlement_left
        ),
        (
            "replaced (successor current: true, durability ready: true)",
            "unregistered",
            false,
            &MemberUnregisterState::Settled,
            false
        ),
        "discard: {discard}; teardown: {teardown} (registration left: {teardown_left}); \
         settlement: {settled:?} (registration left: {settlement_left})"
    );
}

/// The refusal stays for a runtime loop that may still hold the session's
/// mutation fence: a durability-degraded registration whose runtime loop is
/// live and has not handed its executor back is not taken over by a later
/// owner's executor registration (refused until a repair), and its discard
/// waits for that loop's teardown handoff instead of taking the fence (the
/// step times out and nothing is replaced).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_degraded_registration_whose_live_loop_holds_the_fence_is_not_taken_over() {
    use crate::runtime::provisioner::{DegradedUnregisterStep, MemberSessionDisposalArc};

    let runtime_store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let adapter = Arc::new(
        MeerkatMachine::persistent(runtime_store, blob_store).expect("construct runtime authority"),
    );
    let session = SessionId::new();
    adapter
        .register_session(session.clone())
        .await
        .expect("register the session");
    adapter
        .ensure_session_with_executor(session.clone(), Box::new(IdleLaterOwnerExecutor))
        .await
        .expect("attach a live runtime loop");
    let registration = adapter
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    let attachment = adapter
        .current_executor_attachment_witness(&session)
        .await
        .expect("the runtime loop is attached");
    assert!(
        adapter
            .force_session_durability_reload_required_for_test(&session)
            .await,
        "the registration degrades while its loop is live"
    );

    match adapter
        .ensure_session_with_executor(session.clone(), Box::new(IdleLaterOwnerExecutor))
        .await
    {
        Err(meerkat_runtime::RuntimeDriverError::RecoveryRepairBlocked { .. }) => {}
        other => panic!(
            "a later owner's executor registration must be refused while the loop holds the fence, got {other:?}"
        ),
    }
    assert!(
        matches!(
            MemberSessionDisposalArc::discard_degraded_registration_first(
                &adapter,
                None,
                &registration,
                None,
                Instant::now() + Duration::from_secs(1),
            )
            .await,
            DegradedUnregisterStep::TimedOut
        ),
        "the discard waits for the live loop's teardown handoff"
    );
    assert_eq!(
        adapter
            .current_session_registration_witness(&session)
            .await
            .as_ref(),
        Some(&registration),
        "the degraded registration is not replaced"
    );
    assert_eq!(
        adapter
            .current_executor_attachment_witness(&session)
            .await
            .as_ref(),
        Some(&attachment),
        "the live loop stays attached"
    );
}

/// A later owner's executor registration cold-reloads a durability-degraded
/// registration that no runtime loop holds, as it does one whose loop handed
/// its executor back: the reload-required discard publishes a cold
/// successor, and the later owner's executor attaches to that successor.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_later_owners_executor_cold_reloads_a_degraded_registration_without_a_teardown_handoff() {
    let runtime_store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let adapter = Arc::new(
        MeerkatMachine::persistent(runtime_store, blob_store).expect("construct runtime authority"),
    );
    let session = SessionId::new();
    adapter
        .register_session(session.clone())
        .await
        .expect("register a registration without a runtime loop");
    let registration = adapter
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    assert!(
        adapter
            .force_session_durability_reload_required_for_test(&session)
            .await,
        "the registration is durability-degraded"
    );

    adapter
        .ensure_session_with_executor(session.clone(), Box::new(IdleLaterOwnerExecutor))
        .await
        .expect("the later owner's executor registration cold-reloads the degraded registration");
    let successor = adapter
        .current_session_registration_witness(&session)
        .await
        .expect("a registration of the session is current");
    assert_ne!(
        successor, registration,
        "the executor attached to the cold successor, not to the degraded shell"
    );
    assert!(
        adapter.durability_reload_required(&session).await.is_none(),
        "the cold successor is durability-ready"
    );
    assert!(
        adapter
            .current_executor_attachment_witness(&session)
            .await
            .is_some(),
        "the later owner's executor is attached"
    );
}

/// The no-handoff discard revalidates under the registration's mutation
/// gate: a runtime-loop teardown slot that appears after the discard was
/// admitted and the registration transaction released, and before the
/// discard takes the gate (injected here: no public path attaches a loop to a
/// durability-degraded registration), leaves the registration as it is, and
/// the discard answers `NotCurrent` without discarding anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_no_handoff_discard_leaves_a_registration_a_loop_reached_before_the_fence() {
    let runtime_store: Arc<dyn RuntimeStore> = Arc::new(InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let adapter = Arc::new(
        MeerkatMachine::persistent(runtime_store, blob_store).expect("construct runtime authority"),
    );
    let session = SessionId::new();
    adapter
        .register_session(session.clone())
        .await
        .expect("register a registration without a runtime loop");
    let registration = adapter
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    assert!(
        adapter
            .force_session_durability_reload_required_for_test(&session)
            .await,
        "the registration is durability-degraded"
    );

    let (reached, release) =
        adapter.arm_reload_required_discard_before_mutation_gate_test_hook(session.clone());
    let discarding = {
        let adapter = Arc::clone(&adapter);
        let registration = registration.clone();
        tokio::spawn(async move {
            adapter
                .reload_required_registration_settlement_if_current(&registration, None, None)
                .await
        })
    };
    tokio::time::timeout(STEP, reached)
        .await
        .expect("the admitted discard reaches the mutation gate")
        .expect("the discard paused before the mutation gate");
    assert!(
        adapter
            .install_runtime_loop_teardown_slot_for_test(&session)
            .await,
        "a runtime-loop teardown slot appears before the discard takes the gate"
    );
    release.send(()).expect("release the paused discard");

    let settled = tokio::time::timeout(STEP, discarding)
        .await
        .expect("the discard returns")
        .expect("the discard task");
    assert!(
        matches!(
            settled,
            Ok(meerkat_runtime::ReloadRequiredRegistrationUseSettlement::NotCurrent)
        ),
        "the discard leaves a registration a loop reached before the fence: {settled:?}"
    );
    assert_eq!(
        adapter
            .current_session_registration_witness(&session)
            .await
            .as_ref(),
        Some(&registration),
        "nothing was discarded"
    );
    assert!(
        adapter.durability_reload_required(&session).await.is_some(),
        "the registration is still durability-degraded"
    );
}
