//! A wait for a session's mutation gate ends typed once the session degrades,
//! instead of waiting behind a holder that keeps the gate: the
//! post-stop fence an exited runtime loop retains after a failed boundary
//! commit. The tests hold the gate themselves as that holder.
//!
//! Covered waits: the ready-gated wait (the ingress detach a destroy's retire
//! takes, the mob Shutdown's run-start hold), the unregister saga's exact
//! gate waits before its final `UnregisterSession` is staged, and one waiter
//! of each class that took the gate plainly before it went through the live
//! accessor: the run-fenced interrupt, a control command, a comms
//! abort, an executor attachment's unregister and an explicit stop.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

use meerkat_core::lifecycle::RunId;
use meerkat_core::lifecycle::core_executor::{
    CoreApplyOutput, CoreExecutor, CoreExecutorError, CoreExecutorInterruptHandle,
};
use meerkat_core::lifecycle::run_primitive::{RunApplyBoundary, RunPrimitive};
use meerkat_core::lifecycle::run_receipt::RunBoundaryReceiptDraft;
use tokio::sync::Notify;

/// How long a wait may take to end once the session degrades.
const TYPED_END_BOUND: Duration = Duration::from_secs(5);

fn persistent_machine() -> Arc<MeerkatMachine> {
    let store: Arc<dyn crate::store::RuntimeStore> =
        Arc::new(crate::store::InMemoryRuntimeStore::new());
    Arc::new(
        MeerkatMachine::persistent(store, Arc::new(meerkat_store::MemoryBlobStore::new()))
            .expect("persistent machine"),
    )
}

fn run_start_output(run_id: RunId, primitive: &RunPrimitive) -> CoreApplyOutput {
    CoreApplyOutput::with_untyped_snapshot(
        RunBoundaryReceiptDraft {
            run_id,
            boundary: RunApplyBoundary::RunStart,
            contributing_input_ids: primitive.contributing_input_ids().to_vec(),
            conversation_digest: None,
            message_count: 0,
        },
        None,
        None,
    )
}

/// Gives the session a runtime loop and finishes every run at once.
struct IdleExecutor;

#[async_trait::async_trait]
impl CoreExecutor for IdleExecutor {
    async fn apply(
        &mut self,
        run_id: RunId,
        primitive: RunPrimitive,
    ) -> Result<CoreApplyOutput, CoreExecutorError> {
        Ok(run_start_output(run_id, &primitive))
    }

    async fn cancel_after_boundary(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }

    async fn stop_runtime_executor(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }
}

/// Records every exact hard cancel the degraded settlement sends.
#[derive(Default)]
struct RecordingInterrupt {
    requested_runs: std::sync::Mutex<Vec<RunId>>,
    entered: Notify,
}

#[async_trait::async_trait]
impl CoreExecutorInterruptHandle for RecordingInterrupt {
    async fn hard_cancel_run_if_current(
        &self,
        expected_run_id: &RunId,
        _reason: String,
    ) -> Result<bool, CoreExecutorError> {
        self.requested_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(expected_run_id.clone());
        self.entered.notify_one();
        Ok(true)
    }
}

/// Holds its run inside `apply` until the test releases it.
struct HeldRunExecutor {
    interrupt: Arc<RecordingInterrupt>,
    apply_started: Arc<Notify>,
    release_apply: Arc<Notify>,
}

#[async_trait::async_trait]
impl CoreExecutor for HeldRunExecutor {
    fn interrupt_handle(&self) -> Option<Arc<dyn CoreExecutorInterruptHandle>> {
        Some(self.interrupt.clone())
    }

    async fn apply(
        &mut self,
        run_id: RunId,
        primitive: RunPrimitive,
    ) -> Result<CoreApplyOutput, CoreExecutorError> {
        self.apply_started.notify_one();
        self.release_apply.notified().await;
        Ok(run_start_output(run_id, &primitive))
    }

    async fn cancel_after_boundary(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }

    async fn stop_runtime_executor(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }
}

/// Hold `session_id`'s mutation gate, as the post-stop fence an exited
/// runtime loop retains after a failed boundary commit.
async fn hold_mutation_gate(
    machine: &MeerkatMachine,
    session_id: &SessionId,
) -> crate::tokio::sync::OwnedMutexGuard<()> {
    machine
        .session_mutation_gate(session_id)
        .await
        .expect("registered session gate")
        .lock_settled(SettledGateWait::TestFixture)
        .await
}

async fn runtime_loop_attached(machine: &MeerkatMachine, session_id: &SessionId) -> bool {
    let sessions = machine.sessions.read().await;
    let entry = sessions.get(session_id).expect("registered session entry");
    !matches!(entry.attachment_slot, RuntimeLoopAttachmentSlot::Empty)
}

fn ended_typed<T: std::fmt::Debug>(result: &Result<T, RuntimeDriverError>) -> bool {
    matches!(
        result,
        Err(RuntimeDriverError::RecoveryRepairBlocked { .. })
    )
}

/// R-a1: the ingress detach a destroy's retire takes waits behind a holder of
/// the session's mutation gate, and the session degrades during that wait.
/// The wait ends typed at once, without the gate. At v0.9.3 it waited for the
/// holder, which never released.
#[tokio::test]
async fn a_gate_wait_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register a persistent session");
    let fence = hold_mutation_gate(&machine, &session_id).await;
    let parked = machine.arm_mutation_gate_wait_observer_for_test(session_id.clone());
    let detach = tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .update_peer_ingress_context(&session_id, false, None)
                .await
        }
    });
    tokio::time::timeout(TYPED_END_BOUND, parked)
        .await
        .expect("the detach parks behind the held gate")
        .expect("the wait observer stays armed until the wait");
    assert!(
        !detach.is_finished(),
        "a ready session's detach waits for the gate"
    );

    assert!(
        machine
            .force_session_durability_reload_required_for_test(&session_id)
            .await,
        "the fixture degrades the session during the wait"
    );
    let result = tokio::time::timeout(TYPED_END_BOUND, detach)
        .await
        .expect("the degradation ends the wait")
        .expect("the detach task completes");
    assert!(ended_typed(&result), "the wait ends typed: {result:?}");
    drop(fence);
}

/// R-a1, already degraded (C2): the session degraded before the call and
/// another holder keeps the gate. The wait reads the degradation under the
/// lock every degradation publishes under, so it ends typed at once instead
/// of missing a degradation that came before it subscribed.
#[tokio::test]
async fn a_gate_wait_on_an_already_degraded_session_behind_a_holder_ends_typed() {
    let machine = persistent_machine();
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register a persistent session");
    let fence = hold_mutation_gate(&machine, &session_id).await;
    assert!(
        machine
            .force_session_durability_reload_required_for_test(&session_id)
            .await,
        "the fixture degrades the session before the call"
    );

    let result = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.update_peer_ingress_context(&session_id, false, None),
    )
    .await
    .expect("an already degraded session's wait ends at once");
    assert!(ended_typed(&result), "the wait ends typed: {result:?}");
    drop(fence);
}

/// R-a4 (C1): with the gate free, a degraded session's settlement runs in the
/// caller every time, because the gate is polled before the degradation: the
/// caller detaches the runtime loop before it returns and hands nothing off.
/// An unbiased choice would leave it to a process-owned task about half the
/// time.
#[tokio::test]
async fn a_free_gate_settles_a_degraded_session_in_the_caller_every_time() {
    const SESSIONS: usize = 32;
    let machine = persistent_machine();
    for round in 0..SESSIONS {
        let session_id = SessionId::new();
        machine
            .register_session_with_executor(session_id.clone(), Box::new(IdleExecutor))
            .await
            .expect("register a session with a runtime loop");
        assert!(
            runtime_loop_attached(&machine, &session_id).await,
            "round {round}: the fixture needs an attached runtime loop"
        );
        assert!(
            machine
                .force_session_durability_reload_required_for_test(&session_id)
                .await,
            "round {round}: the fixture degrades the session"
        );

        let result = machine
            .lock_current_durability_ready_session_mutation_gate(&session_id)
            .await;
        assert!(ended_typed(&result), "round {round}: {result:?}");
        assert!(
            !runtime_loop_attached(&machine, &session_id).await,
            "round {round}: the caller detached the runtime loop before it returned"
        );
    }
    assert_eq!(
        machine
            .test_degraded_gate_settlement_hand_offs
            .load(Ordering::SeqCst),
        0,
        "a free gate is never handed off"
    );
}

/// R-a5 (C1): a waiter that left on the degradation hands the degraded
/// settlement to a process-owned task. Once the holder releases the gate,
/// that task detaches the runtime loop and hard-cancels the exact current
/// run, as a ready-gated caller holding the gate would have.
#[tokio::test]
async fn a_waiter_that_left_hands_the_settlement_to_a_task_that_cancels_the_exact_run() {
    let machine = persistent_machine();
    let session_id = SessionId::new();
    let interrupt = Arc::new(RecordingInterrupt::default());
    let apply_started = Arc::new(Notify::new());
    let release_apply = Arc::new(Notify::new());
    machine
        .register_session_with_executor(
            session_id.clone(),
            Box::new(HeldRunExecutor {
                interrupt: Arc::clone(&interrupt),
                apply_started: Arc::clone(&apply_started),
                release_apply: Arc::clone(&release_apply),
            }),
        )
        .await
        .expect("register a session whose run is held");
    let (outcome, _completion) = crate::SessionServiceRuntimeExt::accept_input_with_completion(
        machine.as_ref(),
        &session_id,
        crate::Input::Prompt(crate::PromptInput::new("held run", None)),
    )
    .await
    .expect("admit the held run");
    assert!(outcome.is_accepted());
    tokio::time::timeout(TYPED_END_BOUND, apply_started.notified())
        .await
        .expect("the run starts");
    let run_id = machine
        .meerkat_machine_spine_snapshot(&session_id)
        .await
        .expect("run snapshot")
        .control
        .current_run_id
        .expect("current run id");

    let fence = hold_mutation_gate(&machine, &session_id).await;
    let parked = machine.arm_mutation_gate_wait_observer_for_test(session_id.clone());
    let waiter = tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .lock_current_durability_ready_session_mutation_gate(&session_id)
                .await
                .map(|_gate| ())
        }
    });
    tokio::time::timeout(TYPED_END_BOUND, parked)
        .await
        .expect("the waiter parks behind the held gate")
        .expect("the wait observer stays armed until the wait");
    assert!(
        machine
            .force_session_durability_reload_required_for_test(&session_id)
            .await,
        "the fixture degrades the session during the wait"
    );
    let result = tokio::time::timeout(TYPED_END_BOUND, waiter)
        .await
        .expect("the degradation ends the wait")
        .expect("the waiter task completes");
    assert!(ended_typed(&result), "the wait ends typed: {result:?}");
    assert_eq!(
        machine
            .test_degraded_gate_settlement_hand_offs
            .load(Ordering::SeqCst),
        1,
        "the waiter handed the settlement off as it left"
    );
    assert!(
        runtime_loop_attached(&machine, &session_id).await,
        "nothing is settled while the holder keeps the gate"
    );
    assert!(
        interrupt
            .requested_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty(),
        "nothing is cancelled while the holder keeps the gate"
    );

    drop(fence);
    tokio::time::timeout(TYPED_END_BOUND, interrupt.entered.notified())
        .await
        .expect("the handed-off settlement hard-cancels once the gate frees");
    assert_eq!(
        *interrupt
            .requested_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![run_id],
        "the settlement cancels exactly the run that was current"
    );
    assert!(
        !runtime_loop_attached(&machine, &session_id).await,
        "the settlement detached the runtime loop before it cancelled"
    );
    release_apply.notify_one();
}

/// A mob Shutdown holds its members' run starts first, through the same
/// ready-gated wait. Behind a holder of the gate it ends typed once the
/// session degrades. At v0.9.3 a whole-mob Shutdown of a durability-degraded
/// member hung there before its teardown.
#[tokio::test]
async fn a_run_start_hold_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register a persistent session");
    let fence = hold_mutation_gate(&machine, &session_id).await;
    let parked = machine.arm_mutation_gate_wait_observer_for_test(session_id.clone());
    let hold = tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .hold_run_starts(&session_id, RunStartHoldReason::MobStop)
                .await
                .map(|_hold| ())
        }
    });
    tokio::time::timeout(TYPED_END_BOUND, parked)
        .await
        .expect("the hold parks behind the held gate")
        .expect("the wait observer stays armed until the wait");

    assert!(
        machine
            .force_session_durability_reload_required_for_test(&session_id)
            .await,
        "the fixture degrades the session during the wait"
    );
    let result = tokio::time::timeout(TYPED_END_BOUND, hold)
        .await
        .expect("the degradation ends the wait")
        .expect("the hold task completes");
    assert!(ended_typed(&result), "the hold ends typed: {result:?}");
    drop(fence);
}

/// An unregister saga whose coordinator was installed before the session
/// degraded waits behind a holder of the gate. Once the session degrades the
/// saga ends typed and releases its coordinator, so the reload-required
/// discard, which refuses while a teardown coordinator is installed, runs.
#[tokio::test]
async fn an_unregister_saga_behind_a_holder_ends_typed_and_frees_the_reload_discard() {
    let machine = persistent_machine();
    let session_id = SessionId::new();
    machine
        .register_session_with_executor(session_id.clone(), Box::new(IdleExecutor))
        .await
        .expect("register a session with a runtime loop");
    let witness = machine
        .current_session_registration_witness(&session_id)
        .await
        .expect("exact registration");
    let fence = hold_mutation_gate(&machine, &session_id).await;
    let parked = machine.arm_mutation_gate_wait_observer_for_test(session_id.clone());
    let unregister = tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move { machine.unregister_session(&session_id).await }
    });
    tokio::time::timeout(TYPED_END_BOUND, parked)
        .await
        .expect("the saga parks behind the held gate")
        .expect("the wait observer stays armed until the wait");
    {
        let sessions = machine.sessions.read().await;
        assert!(
            sessions
                .get(&session_id)
                .expect("registered session entry")
                .unregister_coordinator
                .is_some(),
            "the saga's coordinator is installed before the session degrades"
        );
    }

    assert!(
        machine
            .force_session_durability_reload_required_for_test(&session_id)
            .await,
        "the fixture degrades the session during the saga's wait"
    );
    let result = tokio::time::timeout(TYPED_END_BOUND, unregister)
        .await
        .expect("the degradation ends the saga")
        .expect("the unregister task completes");
    assert!(ended_typed(&result), "the saga ends typed: {result:?}");
    {
        let sessions = machine.sessions.read().await;
        let entry = sessions
            .get(&session_id)
            .expect("the degraded registration stays for the discard");
        assert!(
            entry.unregister_coordinator.is_none(),
            "the saga released its coordinator"
        );
        assert!(
            entry.pending_unregister_finalization.is_none(),
            "the saga staged no finalization"
        );
    }

    // The holder releases, and a degraded admission starts the exact runtime
    // teardown the discard claims.
    drop(fence);
    crate::SessionServiceRuntimeExt::accept_input(
        machine.as_ref(),
        &session_id,
        crate::Input::Prompt(crate::PromptInput::new("start the degraded teardown", None)),
    )
    .await
    .expect_err("a degraded session refuses admission");
    let disposition = tokio::time::timeout(
        Duration::from_secs(30),
        machine.recover_or_discard_reload_required_registration_if_current(&witness),
    )
    .await
    .expect("the discard ends")
    .expect("the discard runs");
    assert_eq!(
        disposition,
        ReloadRequiredRegistrationDisposition::Discarded,
        "the reload-required discard replaces the degraded registration"
    );
}

/// R-a6: idle sessions with a runtime loop are degraded. A gate waiter on
/// each is refused typed after settling the session itself (the gate was
/// free, so nothing is handed off and the loop is detached before the
/// refusal), and the reload-required discard then replaces the registration.
/// An unbiased or swapped choice hands the settlement off instead.
#[tokio::test]
async fn a_refused_waiter_on_a_free_degraded_session_leaves_it_to_the_discard() {
    const SESSIONS: usize = 16;
    let machine = persistent_machine();
    for round in 0..SESSIONS {
        let session_id = SessionId::new();
        machine
            .register_session_with_executor(session_id.clone(), Box::new(IdleExecutor))
            .await
            .expect("register a session with a runtime loop");
        let witness = machine
            .current_session_registration_witness(&session_id)
            .await
            .expect("exact registration");
        assert!(
            machine
                .force_session_durability_reload_required_for_test(&session_id)
                .await,
            "round {round}: the fixture degrades the session"
        );

        let refused = tokio::time::timeout(
            TYPED_END_BOUND,
            machine.update_peer_ingress_context(&session_id, false, None),
        )
        .await
        .expect("a free gate's wait ends at once");
        assert!(ended_typed(&refused), "round {round}: {refused:?}");
        assert!(
            !runtime_loop_attached(&machine, &session_id).await,
            "round {round}: the waiter detached the runtime loop before it was refused"
        );

        let disposition = tokio::time::timeout(
            TYPED_END_BOUND,
            machine.recover_or_discard_reload_required_registration_if_current(&witness),
        )
        .await
        .expect("the discard ends")
        .expect("the discard runs");
        assert_eq!(
            disposition,
            ReloadRequiredRegistrationDisposition::Discarded,
            "round {round}: the reload-required discard replaces the registration"
        );
    }
    assert_eq!(
        machine
            .test_degraded_gate_settlement_hand_offs
            .load(Ordering::SeqCst),
        0,
        "a waiter on a free gate settles the session itself"
    );
}

/// Run `operation` behind a holder of `session_id`'s mutation gate, degrade
/// the session once the operation parks behind that holder, and return what
/// it answered. It must answer within the bound, without the gate.
async fn degrade_during_live_gate_wait<T>(
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
    operation: impl std::future::Future<Output = T> + Send + 'static,
) -> T
where
    T: Send + 'static,
{
    let fence = hold_mutation_gate(machine, session_id).await;
    let parked = machine.arm_mutation_gate_wait_observer_for_test(session_id.clone());
    let waiter = tokio::spawn(operation);
    tokio::time::timeout(TYPED_END_BOUND, parked)
        .await
        .expect("the operation parks behind the held gate")
        .expect("the wait observer stays armed until the wait");
    assert!(
        !waiter.is_finished(),
        "a ready session's operation waits for the gate"
    );
    assert!(
        machine
            .force_session_durability_reload_required_for_test(session_id)
            .await,
        "the fixture degrades the session during the wait"
    );
    let answered = tokio::time::timeout(TYPED_END_BOUND, waiter)
        .await
        .expect("the degradation ends the wait")
        .expect("the operation completes");
    drop(fence);
    answered
}

async fn registered(machine: &MeerkatMachine) -> SessionId {
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register a persistent session");
    session_id
}

async fn registered_with_runtime_loop(machine: &Arc<MeerkatMachine>) -> SessionId {
    let session_id = SessionId::new();
    machine
        .register_session_with_executor(session_id.clone(), Box::new(IdleExecutor))
        .await
        .expect("register a session with a runtime loop");
    assert!(
        runtime_loop_attached(machine, &session_id).await,
        "the fixture needs an attached runtime loop"
    );
    session_id
}

/// A run-fenced interrupt (the bridge's exact hard cancel) waits for the
/// gate live. Behind a holder it ends typed once the session degrades; it
/// took the gate plainly before and waited for the holder.
#[tokio::test]
async fn a_run_fenced_interrupt_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = registered(&machine).await;
    let result = degrade_during_live_gate_wait(&machine, &session_id, {
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .hard_cancel_run_if_current(&session_id, &RunId::new(), "interrupt behind a holder")
                .await
        }
    })
    .await;
    assert!(ended_typed(&result), "the interrupt ends typed: {result:?}");
}

/// A control command (recover) waits for the gate live and answers the
/// repair-blocked refusal the ready-gated control wait reports.
#[tokio::test]
async fn a_control_command_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = registered(&machine).await;
    let runtime_id = crate::identifiers::LogicalRuntimeId::for_session(&session_id);
    let result = degrade_during_live_gate_wait(&machine, &session_id, {
        let machine = Arc::clone(&machine);
        async move { crate::traits::RuntimeControlPlane::recover(&*machine, &runtime_id).await }
    })
    .await;
    match result {
        Err(crate::traits::RuntimeControlPlaneError::StoreError(reason)) => assert!(
            reason.contains("repair-blocked"),
            "the refusal names the repair block: {reason}"
        ),
        other => panic!("the recover ends with the typed control refusal: {other:?}"),
    }
}

/// A comms abort waits for the gate live.
#[tokio::test]
async fn a_comms_abort_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = registered(&machine).await;
    let result = degrade_during_live_gate_wait(&machine, &session_id, {
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .execute_meerkat_machine_drain_local_command(MeerkatMachineCommand::Abort {
                    session_id,
                })
                .await
        }
    })
    .await;
    assert!(ended_typed(&result), "the abort ends typed: {result:?}");
}

/// Unregistering an executor attachment waits for the gate live, so a
/// retire of a degraded session goes to the reload instead of waiting for
/// the holder.
#[tokio::test]
async fn an_attachment_unregister_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = registered_with_runtime_loop(&machine).await;
    let witness = machine
        .current_executor_attachment_witness(&session_id)
        .await
        .expect("the attached runtime loop has a witness");
    let result = degrade_during_live_gate_wait(&machine, &session_id, {
        let machine = Arc::clone(&machine);
        async move {
            machine
                .unregister_executor_attachment_if_current(&witness)
                .await
        }
    })
    .await;
    assert!(
        ended_typed(&result),
        "the unregister ends typed: {result:?}"
    );
}

/// An explicit stop waits for the gate live, like every path to the
/// post-stop fence except the reload-required discard's own.
#[tokio::test]
async fn an_explicit_stop_behind_a_holder_ends_typed_once_the_session_degrades() {
    let machine = persistent_machine();
    let session_id = registered_with_runtime_loop(&machine).await;
    let result = degrade_during_live_gate_wait(&machine, &session_id, {
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .stop_runtime_executor(&session_id, "stop behind a holder")
                .await
        }
    })
    .await;
    assert!(ended_typed(&result), "the stop ends typed: {result:?}");
}
