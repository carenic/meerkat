//! A boundary commit or run terminalization whose store reports no durable
//! outcome within the store's declared commit outcome deadline degrades its
//! session instead of keeping the session's mutation gate and driver forever.
//! The commit keeps running, and its late outcome settles the run's recovery
//! carriers, so the reload-required discard replaces the registration once
//! the store has answered. An input admission whose store write reports no
//! durable outcome within the same deadline degrades its session the same
//! way.
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

/// Reads of the session's driver that take no gate answer while a commit
/// past its deadline holds the driver: the active-input read answers busy,
/// the wake and the uncommitted-run-input read refuse typed, and the input's
/// terminal completion refuses as not readable yet. Before, each waited for the driver,
/// and so for the store.
#[tokio::test]
async fn driver_reads_without_the_gate_answer_while_a_commit_past_its_deadline_holds_the_driver() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let (run, hold) = commit_past_its_deadline(&machine, &store).await;
    let session_id = &run.session_id;

    let busy = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.session_has_active_inputs(session_id),
    )
    .await
    .expect("the active-input read answers");
    assert!(busy, "the degraded session answers busy");
    let woke = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.wake_runtime_if_active_inputs(session_id),
    )
    .await
    .expect("the wake answers");
    assert!(
        matches!(woke, Err(RuntimeDriverError::RecoveryRepairBlocked { .. })),
        "the wake refuses typed: {woke:?}"
    );
    let uncommitted = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.session_has_uncommitted_run_input(session_id),
    )
    .await
    .expect("the uncommitted-run-input read answers");
    assert!(
        matches!(
            uncommitted,
            Err(RuntimeDriverError::RecoveryRepairBlocked { .. })
        ),
        "the uncommitted-run-input read refuses typed: {uncommitted:?}"
    );
    let terminal = tokio::time::timeout(
        TYPED_END_BOUND,
        crate::SessionServiceRuntimeExt::input_terminal_completion(
            machine.as_ref(),
            session_id,
            &run.input_id,
        ),
    )
    .await
    .expect("the terminal read answers");
    assert!(
        matches!(
            terminal,
            Err(RuntimeDriverError::InputTerminalNotYetReadable { .. })
        ),
        "the terminal read refuses as not readable yet: {terminal:?}"
    );
    hold.release(HeldStoreCommitRelease::Commit);
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

/// Register a session with a runtime loop over an executor that holds any
/// run it is given and is never released: the admission tests stop at the
/// store.
async fn register_session(machine: &Arc<MeerkatMachine>) -> SessionId {
    let session_id = SessionId::new();
    machine
        .register_session_with_executor(
            session_id.clone(),
            Box::new(GatedExecutor {
                gate: Arc::new(ExecutorGate {
                    apply_entered: Notify::new(),
                    release_apply: Notify::new(),
                }),
                fail: false,
            }),
        )
        .await
        .expect("register a session with a runtime loop");
    session_id
}

/// The held admission's prompt and idempotency key; its resubmission
/// carries both, as an exact replay must.
const HELD_ADMISSION_TEXT: &str = "an admission whose store write is held";
const HELD_ADMISSION_KEY: &str = "held-admission";

/// A prompt that carries `key` as its idempotency key.
fn keyed_prompt(text: &str, key: &str) -> crate::Input {
    let mut input = prompt(text);
    input.header_mut().idempotency_key = Some(crate::identifiers::IdempotencyKey::new(key));
    input
}

/// The admission's typed, retryable refusal once its store write is past
/// its deadline.
fn admission_outcome_unknown(error: &RuntimeDriverError, input_id: &InputId) -> bool {
    matches!(
        error,
        RuntimeDriverError::RecoveryRepairBlocked { reason, .. }
            if reason.contains(&input_id.to_string())
                && reason.contains("has not reported its durable outcome")
                && reason.contains("same idempotency key")
    )
}

/// How many of the store's input rows are `input_id`'s.
async fn stored_rows(
    store: &RuntimeCommitAtomicityStore,
    session_id: &SessionId,
    input_id: &InputId,
) -> usize {
    RuntimeStore::load_input_states(
        store,
        &crate::identifiers::LogicalRuntimeId::for_session(session_id),
    )
    .await
    .expect("read the store's inputs")
    .iter()
    .filter(|row| {
        matches!(
            row,
            crate::store::InputStateRow::Decoded(stored) if &stored.state.input_id == input_id
        )
    })
    .count()
}

/// Hold an admission's store write and let the deadline pass. The admission
/// answers the typed refusal within the bound, the session needs a reload
/// for it, and the write keeps the gate while the store has not answered.
async fn admission_past_its_deadline(
    machine: &Arc<MeerkatMachine>,
    store: &RuntimeCommitAtomicityStore,
    session_id: &SessionId,
    input: crate::Input,
) -> super::tests::HeldStoreCommitControl {
    let input_id = input.id().clone();
    let hold = store.hold_next_admission_commit(&input_id);
    let admitted = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.accept_input_with_completion(session_id, input),
    )
    .await
    .expect("the admission answers while the store has not");
    let refused = match admitted {
        Ok((outcome, _)) => panic!("the admission answered before its store did: {outcome:?}"),
        Err(error) => error,
    };
    assert!(
        admission_outcome_unknown(&refused, &input_id),
        "the admission refuses typed and retryable: {refused:?}"
    );
    tokio::time::timeout(TYPED_END_BOUND, hold.entered.notified())
        .await
        .expect("the admission's write reached the store");
    let required = machine
        .durability_reload_required(session_id)
        .await
        .expect("the deadline degraded the session");
    assert_eq!(required.operation, "runtime_admission_deadline");
    assert!(
        gate_is_held(machine, session_id).await,
        "the admission's write keeps the gate while the store has not answered"
    );
    hold
}

/// An admission whose store write never answers. At the store's declared
/// deadline the admission answers a typed, retryable refusal and the session
/// needs a reload (runtime_admission_deadline); a later admission's live
/// wait for the gate ends typed, and a reload refuses typed while the write
/// still holds the gate. Once the store commits late, the reload replaces
/// the registration, the store holds the input, and a resubmission with the
/// same idempotency key is deduplicated against it: admitted exactly once.
/// Before, the admission held the gate and the driver until the store
/// answered.
#[tokio::test]
async fn an_admission_the_store_never_answers_refuses_typed_at_the_deadline_and_lands_once() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let session_id = register_session(&machine).await;
    let input = keyed_prompt(HELD_ADMISSION_TEXT, HELD_ADMISSION_KEY);
    let input_id = input.id().clone();
    let hold = admission_past_its_deadline(&machine, &store, &session_id, input).await;

    let later = tokio::time::timeout(
        TYPED_END_BOUND,
        machine.accept_input_with_completion(&session_id, prompt("a later input")),
    )
    .await
    .expect("a later admission's wait for the gate ends");
    match later {
        Ok((outcome, _)) => {
            panic!("a later admission passed the degraded gate: {outcome:?}")
        }
        Err(error) => assert!(
            matches!(error, RuntimeDriverError::RecoveryRepairBlocked { .. }),
            "the later gate wait ends typed: {error:?}"
        ),
    }
    let witness = machine
        .current_session_registration_witness(&session_id)
        .await
        .expect("exact registration");
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
    eventually("the late admission write hands the gate back", || {
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
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
        machine.is_durability_ready(&session_id).await,
        "the cold install is ready"
    );
    assert_eq!(
        stored_rows(&store, &session_id, &input_id).await,
        1,
        "the late write admitted the input"
    );
    let (resubmitted, _) = tokio::time::timeout(
        Duration::from_secs(30),
        machine.accept_input_with_completion(
            &session_id,
            keyed_prompt(HELD_ADMISSION_TEXT, HELD_ADMISSION_KEY),
        ),
    )
    .await
    .expect("the resubmission ends")
    .expect("the resubmission answers");
    assert!(
        matches!(
            &resubmitted,
            crate::accept::AcceptOutcome::Deduplicated { existing_id, .. }
                if existing_id == &input_id
        ),
        "the resubmission is deduplicated against the late admission: {resubmitted:?}"
    );
}

/// The store answers a past-deadline admission with an error. Once it has,
/// the reload replaces the registration and the store holds no row for the
/// input.
#[tokio::test]
async fn a_late_admission_failure_leaves_no_input_after_the_reload() {
    let store = declaring_store();
    let machine = machine_over(&store, HUNG_BUSY_WINDOW);
    let session_id = register_session(&machine).await;
    let input = prompt("an admission whose store write fails late");
    let input_id = input.id().clone();
    let hold = admission_past_its_deadline(&machine, &store, &session_id, input).await;
    let witness = machine
        .current_session_registration_witness(&session_id)
        .await
        .expect("exact registration");

    hold.release(HeldStoreCommitRelease::Fail);
    eventually("the failed admission write hands the gate back", || {
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
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
    assert_eq!(
        stored_rows(&store, &session_id, &input_id).await,
        0,
        "the failed write admitted nothing"
    );
}

/// Control: an admission whose store write takes most of the default
/// deadline is admitted, and its session stays ready.
#[tokio::test]
async fn a_slow_admission_under_the_deadline_is_admitted() {
    let store = declaring_store();
    let machine = machine_over(&store, SLOW_BUSY_WINDOW);
    let session_id = register_session(&machine).await;
    let input = prompt("a slow admission");
    let input_id = input.id().clone();
    // The deadline is two busy windows; the write takes 1.6 of them.
    store.delay_next_admission_commit(&input_id, SLOW_BUSY_WINDOW.mul_f64(1.6));
    let (outcome, _completion) = tokio::time::timeout(
        Duration::from_secs(30),
        machine.accept_input_with_completion(&session_id, input),
    )
    .await
    .expect("the slow admission ends")
    .expect("the slow admission is admitted");
    assert!(
        matches!(outcome, crate::accept::AcceptOutcome::Accepted { .. }),
        "{outcome:?}"
    );
    assert!(
        machine.is_durability_ready(&session_id).await,
        "an admission under the deadline does not degrade the session"
    );
    assert_eq!(stored_rows(&store, &session_id, &input_id).await, 1);
}
