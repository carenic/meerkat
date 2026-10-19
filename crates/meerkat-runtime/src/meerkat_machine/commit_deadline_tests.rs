//! A boundary commit or run terminalization whose store reports no durable
//! outcome within the store's declared commit outcome deadline degrades its
//! session instead of keeping the session's mutation gate and driver forever.
//! The commit keeps running, and its late outcome settles the run's recovery
//! carriers, so the reload-required discard replaces the registration once
//! the store has answered.
//!
//! The tests measure the store's declaration in short busy windows through
//! the machine's test seam; the declaration itself is the store's.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::tests::{HeldStoreCommitRelease, RuntimeCommitAtomicityStore};
use super::*;
use std::time::Duration;

use meerkat_core::lifecycle::core_executor::{
    CoreApplyFailureCause, CoreApplyOutput, CoreExecutor, CoreExecutorError,
};
use meerkat_core::lifecycle::run_primitive::{RunApplyBoundary, RunPrimitive};
use meerkat_core::lifecycle::run_receipt::RunBoundaryReceiptDraft;
use meerkat_core::lifecycle::{InputId, RunId};
use tokio::sync::Notify;

use crate::input_state::InputLifecycleState;
use crate::store::{DEFAULT_COMMIT_OUTCOME_DEADLINE, RuntimeStore};

/// The busy window of the hung-commit tests: their store declares the default
/// two windows, so the deadline is half a second.
const HUNG_BUSY_WINDOW: Duration = Duration::from_millis(250);
/// The busy window of the slow-commit tests, long enough that a commit
/// meant to finish under the deadline does so on a loaded host.
const SLOW_BUSY_WINDOW: Duration = Duration::from_secs(1);
/// How long a typed end may take once the deadline has passed.
const TYPED_END_BOUND: Duration = Duration::from_secs(5);

fn prompt(text: &str) -> crate::Input {
    crate::Input::Prompt(crate::PromptInput::new(text, None))
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

/// A store that declares the default commit outcome deadline, as the SQLite
/// store does.
fn declaring_store() -> Arc<RuntimeCommitAtomicityStore> {
    let store = Arc::new(RuntimeCommitAtomicityStore::pass_through(Arc::new(
        crate::store::InMemoryRuntimeStore::new(),
    )));
    store.declare_commit_outcome_deadline(DEFAULT_COMMIT_OUTCOME_DEADLINE);
    store
}

fn machine_over(
    store: &Arc<RuntimeCommitAtomicityStore>,
    busy_window: Duration,
) -> Arc<MeerkatMachine> {
    let machine = Arc::new(
        MeerkatMachine::persistent(
            Arc::clone(store) as Arc<dyn RuntimeStore>,
            Arc::new(meerkat_store::MemoryBlobStore::new()),
        )
        .expect("persistent machine"),
    );
    machine.set_runtime_commit_busy_window_for_test(busy_window);
    machine
}

struct ExecutorGate {
    apply_entered: Notify,
    release_apply: Notify,
}

/// Holds each run inside `apply` until the test releases it, then completes
/// the run, or fails it when built to.
struct GatedExecutor {
    gate: Arc<ExecutorGate>,
    fail: bool,
}

#[async_trait::async_trait]
impl CoreExecutor for GatedExecutor {
    async fn apply(
        &mut self,
        run_id: RunId,
        primitive: RunPrimitive,
    ) -> Result<CoreApplyOutput, CoreExecutorError> {
        self.gate.apply_entered.notify_one();
        self.gate.release_apply.notified().await;
        if self.fail {
            return Err(CoreExecutorError::ApplyFailed {
                cause: CoreApplyFailureCause::executor_internal("the test fails this run"),
            });
        }
        Ok(run_start_output(run_id, &primitive))
    }

    async fn cancel_after_boundary(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }

    async fn stop_runtime_executor(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }
}

/// A run held in its executor, with a completion waiter on its input (an
/// ordinary prompt: no directed interaction, so the run keeps a non-directed
/// recovery carrier across its commit).
struct HeldRun {
    session_id: SessionId,
    input_id: InputId,
    gate: Arc<ExecutorGate>,
    completion: crate::completion::CompletionHandle,
}

async fn start_held_run(machine: &Arc<MeerkatMachine>, fail: bool) -> HeldRun {
    let session_id = SessionId::new();
    let gate = Arc::new(ExecutorGate {
        apply_entered: Notify::new(),
        release_apply: Notify::new(),
    });
    machine
        .register_session_with_executor(
            session_id.clone(),
            Box::new(GatedExecutor {
                gate: Arc::clone(&gate),
                fail,
            }),
        )
        .await
        .expect("register a session with a runtime loop");
    let input = prompt("a run whose store commit is held");
    let input_id = input.id().clone();
    let (outcome, completion) = machine
        .accept_input_with_completion(&session_id, input)
        .await
        .expect("admit the run's input");
    assert!(
        matches!(outcome, crate::accept::AcceptOutcome::Accepted { .. }),
        "{outcome:?}"
    );
    tokio::time::timeout(TYPED_END_BOUND, gate.apply_entered.notified())
        .await
        .expect("the run reaches its executor");
    HeldRun {
        session_id,
        input_id,
        gate,
        completion: completion.expect("the prompt keeps a completion waiter"),
    }
}

async fn teardown_slot(
    machine: &MeerkatMachine,
    session_id: &SessionId,
) -> Arc<crate::runtime_loop::RuntimeLoopTeardownSlot> {
    let sessions = machine.sessions.read().await;
    sessions
        .get(session_id)
        .expect("registered session entry")
        .runtime_loop_teardown
        .clone()
        .expect("the runtime loop's teardown slot")
}

/// Wait until `condition` holds, for at most the typed-end bound.
async fn eventually<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TYPED_END_BOUND;
    while !condition().await {
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The reload's typed, retryable refusal while a run's commit or
/// terminalization is past its deadline and its store has not answered.
fn refused_until_the_store_answers(
    refused: &Result<ReloadRequiredRegistrationDisposition, RuntimeDriverError>,
) -> bool {
    matches!(
        refused,
        Err(RuntimeDriverError::RecoveryRepairBlocked { reason, .. })
            if reason.contains("has not reported its durable outcome")
    )
}

async fn gate_is_held(machine: &MeerkatMachine, session_id: &SessionId) -> bool {
    machine
        .session_mutation_gate(session_id)
        .await
        .expect("registered session gate")
        .try_lock()
        .is_err()
}

async fn stored_input(
    store: &RuntimeCommitAtomicityStore,
    session_id: &SessionId,
    input_id: &InputId,
) -> crate::input_state::StoredInputState {
    RuntimeStore::load_input_state(
        store,
        &crate::identifiers::LogicalRuntimeId::for_session(session_id),
        input_id,
    )
    .await
    .expect("read the input from the store")
    .expect("the input stays durably admitted")
}

/// Hold the run's boundary commit at the store and let the deadline pass.
async fn commit_past_its_deadline(
    machine: &Arc<MeerkatMachine>,
    store: &RuntimeCommitAtomicityStore,
) -> (HeldRun, super::tests::HeldStoreCommitControl) {
    let run = start_held_run(machine, false).await;
    let hold = store.hold_next_success_boundary_commit(&run.input_id);
    run.gate.release_apply.notify_one();
    tokio::time::timeout(TYPED_END_BOUND, hold.entered.notified())
        .await
        .expect("the run's boundary commit reaches the store");
    eventually("the deadline degrades the session", || {
        let machine = Arc::clone(machine);
        let session_id = run.session_id.clone();
        async move {
            machine
                .durability_reload_required(&session_id)
                .await
                .is_some()
        }
    })
    .await;
    (run, hold)
}

/// R-c1: a run's boundary commit that the store never answers. At the
/// store's declared deadline the session degrades, a wait for its gate ends
/// typed, the run's completion waiter fails as outcome-unknown, and the loop
/// hands its executor off while the commit still holds the gate. Before the
/// deadline existed, the wait and the waiter waited for the store forever.
#[tokio::test]
async fn a_commit_the_store_never_answers_degrades_its_session_at_the_deadline() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let run = start_held_run(&machine, false).await;
    let hold = store.hold_next_success_boundary_commit(&run.input_id);
    run.gate.release_apply.notify_one();
    tokio::time::timeout(TYPED_END_BOUND, hold.entered.notified())
        .await
        .expect("the run's boundary commit reaches the store");

    let parked = machine.arm_mutation_gate_wait_observer_for_test(run.session_id.clone());
    let waiter = tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = run.session_id.clone();
        async move {
            machine
                .update_peer_ingress_context(&session_id, false, None)
                .await
        }
    });
    tokio::time::timeout(TYPED_END_BOUND, parked)
        .await
        .expect("a gate wait parks behind the commit")
        .expect("the wait observer stays armed until the wait");

    let waited = tokio::time::timeout(TYPED_END_BOUND, waiter)
        .await
        .expect("the deadline ends the gate wait")
        .expect("the waiter completes");
    assert!(
        matches!(
            waited,
            Err(RuntimeDriverError::RecoveryRepairBlocked { .. })
        ),
        "the gate wait ends typed: {waited:?}"
    );
    let required = machine
        .durability_reload_required(&run.session_id)
        .await
        .expect("the deadline degraded the session");
    assert_eq!(required.operation, "runtime_commit_deadline");
    let failed = tokio::time::timeout(TYPED_END_BOUND, run.completion.wait())
        .await
        .expect("the completion waiter ends")
        .expect_err("the completion waiter fails");
    assert!(failed.is_durable_outcome_unknown(), "{failed:?}");

    let slot = teardown_slot(&machine, &run.session_id).await;
    eventually("the loop hands its executor off", || {
        let slot = Arc::clone(&slot);
        async move { slot.exit_handoff_is_settled() }
    })
    .await;
    assert!(
        slot.owned_child_past_deadline_for_test(),
        "the run's commit carrier is past its deadline"
    );
    assert!(
        gate_is_held(&machine, &run.session_id).await,
        "the commit still holds the gate while the store has not answered"
    );
    hold.release(HeldStoreCommitRelease::Commit);
    eventually("the late commit hands the gate back", || {
        let machine = Arc::clone(&machine);
        let session_id = run.session_id.clone();
        async move { !gate_is_held(&machine, &session_id).await }
    })
    .await;
}

/// R-c2: while the commit is past its deadline, a reload refuses typed and
/// retryable. Once the store commits late, the run's carriers are settled
/// (its non-directed one included), the reload replaces the registration,
/// and the store holds the run's input as consumed.
#[tokio::test]
async fn a_reload_refuses_until_a_late_commit_lands_then_replaces_the_registration() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let (run, hold) = commit_past_its_deadline(&machine, &store).await;
    let witness = machine
        .current_session_registration_witness(&run.session_id)
        .await
        .expect("exact registration");
    let slot = teardown_slot(&machine, &run.session_id).await;
    eventually("the loop hands its executor off", || {
        let slot = Arc::clone(&slot);
        async move { slot.exit_handoff_is_settled() }
    })
    .await;

    let refused = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.recover_or_discard_reload_required_registration_if_current(&witness),
    )
    .await
    .expect("the reload answers while the store has not");
    assert!(
        refused_until_the_store_answers(&refused),
        "the reload refuses typed and retryable: {refused:?}"
    );

    hold.release(HeldStoreCommitRelease::Commit);
    eventually("the late commit hands the gate back", || {
        let machine = Arc::clone(&machine);
        let session_id = run.session_id.clone();
        async move { !gate_is_held(&machine, &session_id).await }
    })
    .await;
    let disposition = tokio::time::timeout(
        Duration::from_secs(30),
        machine.recover_or_discard_reload_required_registration_if_current(&witness),
    )
    .await
    .expect("the reload ends")
    .expect("the reload runs once the store has answered");
    assert_eq!(
        disposition,
        ReloadRequiredRegistrationDisposition::Discarded
    );
    assert!(
        machine.is_durability_ready(&run.session_id).await,
        "the cold install is ready"
    );
    let stored = stored_input(&store, &run.session_id, &run.input_id).await;
    assert_eq!(stored.seed.phase, InputLifecycleState::Consumed);
}

/// R-c3: the store answers the past-deadline commit with an error. The
/// carrier becomes a rejected abort, the reload aborts the run's projections
/// and replaces the registration, and the input is not terminal: it runs
/// again after the reload.
#[tokio::test]
async fn a_late_commit_failure_leaves_the_input_to_run_again_after_the_reload() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let (run, hold) = commit_past_its_deadline(&machine, &store).await;
    let witness = machine
        .current_session_registration_witness(&run.session_id)
        .await
        .expect("exact registration");
    hold.release(HeldStoreCommitRelease::Fail);
    eventually("the failed commit hands the gate back", || {
        let machine = Arc::clone(&machine);
        let session_id = run.session_id.clone();
        async move { !gate_is_held(&machine, &session_id).await }
    })
    .await;
    let disposition = tokio::time::timeout(
        Duration::from_secs(30),
        machine.recover_or_discard_reload_required_registration_if_current(&witness),
    )
    .await
    .expect("the reload ends")
    .expect("the reload runs once the store has answered");
    assert_eq!(
        disposition,
        ReloadRequiredRegistrationDisposition::Discarded
    );
    let stored = stored_input(&store, &run.session_id, &run.input_id).await;
    assert!(
        stored.seed.terminal_outcome.is_none(),
        "the rolled-back run left its input to run again: {:?}",
        stored.seed.phase
    );
}

/// R-c4: the terminalization twin. A failed run whose failure the store
/// never records degrades its session at the deadline, its waiter fails as
/// outcome-unknown, the reload refuses typed until the store answers, and
/// then replaces the registration.
#[tokio::test]
async fn a_run_terminalization_the_store_never_answers_degrades_its_session_at_the_deadline() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let run = start_held_run(&machine, true).await;
    let hold = store.hold_next_machine_lifecycle_commit(&run.input_id);
    run.gate.release_apply.notify_one();
    tokio::time::timeout(TYPED_END_BOUND, hold.entered.notified())
        .await
        .expect("the run's terminalization reaches the store");
    eventually("the deadline degrades the session", || {
        let machine = Arc::clone(&machine);
        let session_id = run.session_id.clone();
        async move {
            machine
                .durability_reload_required(&session_id)
                .await
                .is_some()
        }
    })
    .await;
    let required = machine
        .durability_reload_required(&run.session_id)
        .await
        .expect("degraded");
    assert_eq!(required.operation, "runtime_terminalization_deadline");
    let failed = tokio::time::timeout(TYPED_END_BOUND, run.completion.wait())
        .await
        .expect("the completion waiter ends")
        .expect_err("the completion waiter fails");
    assert!(failed.is_durable_outcome_unknown(), "{failed:?}");

    let witness = machine
        .current_session_registration_witness(&run.session_id)
        .await
        .expect("exact registration");
    let slot = teardown_slot(&machine, &run.session_id).await;
    eventually("the loop hands its executor off", || {
        let slot = Arc::clone(&slot);
        async move { slot.exit_handoff_is_settled() }
    })
    .await;
    let refused = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.recover_or_discard_reload_required_registration_if_current(&witness),
    )
    .await
    .expect("the reload answers while the store has not");
    assert!(
        refused_until_the_store_answers(&refused),
        "the reload refuses typed and retryable: {refused:?}"
    );

    hold.release(HeldStoreCommitRelease::Commit);
    eventually("the late terminalization hands the gate back", || {
        let machine = Arc::clone(&machine);
        let session_id = run.session_id.clone();
        async move { !gate_is_held(&machine, &session_id).await }
    })
    .await;
    let disposition = tokio::time::timeout(
        Duration::from_secs(30),
        machine.recover_or_discard_reload_required_registration_if_current(&witness),
    )
    .await
    .expect("the reload ends")
    .expect("the reload runs once the store has answered");
    assert_eq!(
        disposition,
        ReloadRequiredRegistrationDisposition::Discarded
    );
}

/// R-c6: a composed store whose commit runs one transaction of its own after
/// its inner store's declares one more busy window. A commit that takes
/// longer than the default deadline but less than that declaration completes
/// with no degradation, and its waiter gets the ordinary outcome.
#[tokio::test]
async fn a_composed_store_commit_within_its_own_declaration_completes() {
    let store = declaring_store();
    store.declare_commit_outcome_deadline(
        DEFAULT_COMMIT_OUTCOME_DEADLINE + crate::store::COMMIT_BUSY_WINDOW,
    );
    // Two transactions that each wait just under a busy window, plus I/O:
    // over the default two windows, under the declared three.
    store.delay_success_boundary_commits(SLOW_BUSY_WINDOW.mul_f64(2.3));
    let machine = machine_over(&store, SLOW_BUSY_WINDOW);
    let run = start_held_run(&machine, false).await;
    run.gate.release_apply.notify_one();
    let outcome = tokio::time::timeout(Duration::from_secs(30), run.completion.wait())
        .await
        .expect("the slow commit ends")
        .expect("the waiter gets the run's outcome");
    assert!(
        matches!(
            outcome,
            crate::completion::CompletionOutcome::Completed(_)
                | crate::completion::CompletionOutcome::CompletedWithoutResult
        ),
        "the run completes: {outcome:?}"
    );
    assert!(
        machine.is_durability_ready(&run.session_id).await,
        "a commit within the store's declaration does not degrade the session"
    );
}

/// Control: a commit that takes most of the default deadline completes with
/// no degradation.
#[tokio::test]
async fn a_slow_commit_under_the_deadline_completes() {
    let store = declaring_store();
    // The deadline is two busy windows; the commit takes 1.6 of them.
    store.delay_success_boundary_commits(SLOW_BUSY_WINDOW.mul_f64(1.6));
    let machine = machine_over(&store, SLOW_BUSY_WINDOW);
    let run = start_held_run(&machine, false).await;
    run.gate.release_apply.notify_one();
    tokio::time::timeout(Duration::from_secs(30), run.completion.wait())
        .await
        .expect("the slow commit ends")
        .expect("the waiter gets the run's outcome");
    assert!(
        machine.is_durability_ready(&run.session_id).await,
        "a commit under the deadline does not degrade the session"
    );
}
