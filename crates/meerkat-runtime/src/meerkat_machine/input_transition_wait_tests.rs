//! A cancellation that meets an input between lifecycle steps (neither queued
//! nor terminal, with no run to cancel) re-reads it when the input's next step
//! commits, not on a timer (#1655). The paused clock makes that observable:
//! no virtual time passes between the commit and the answer. A step committed
//! without the signal costs the cancellation its settle bound, never a false
//! settle error, and the subscription it takes under the driver lock it reads
//! with cannot miss a step committed by the lock's next holder.
//!
//! The persistent variants step and consume the input through the persistent
//! driver's inner driver, so they pin the delegation of the step subscription,
//! not a durable commit.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use std::time::Duration;

use chrono::Utc;

fn keyed_prompt(key: &str) -> Input {
    Input::Prompt(crate::input::PromptInput {
        injected_context: Vec::new(),
        header: crate::input::InputHeader {
            ingress_context: None,
            retained_resume: None,
            authority_association: None,
            id: InputId::new(),
            timestamp: Utc::now(),
            source: crate::input::InputOrigin::Operator,
            durability: crate::input::InputDurability::Durable,
            visibility: crate::input::InputVisibility::default(),
            idempotency_key: Some(crate::identifiers::IdempotencyKey::new(key)),
            supersession_key: None,
            correlation_id: None,
        },
        content: "held between lifecycle steps".into(),
        typed_turn_appends: Vec::new(),
        turn_metadata: None,
    })
}

fn persistent_machine() -> Arc<MeerkatMachine> {
    let store = Arc::new(crate::store::InMemoryRuntimeStore::new());
    Arc::new(
        MeerkatMachine::persistent(store, Arc::new(meerkat_store::MemoryBlobStore::new()))
            .expect("persistent machine"),
    )
}

async fn session_driver(
    machine: &MeerkatMachine,
    session_id: &SessionId,
) -> crate::meerkat_machine::driver::SharedDriver {
    machine
        .sessions
        .read()
        .await
        .get(session_id)
        .expect("registered session")
        .driver
        .clone()
}

/// Accept one keyed prompt (it stays queued: no executor serves the session)
/// and step it to applied with no run: the state a cancellation must wait out.
async fn input_between_steps(
    machine: &MeerkatMachine,
    session_id: &SessionId,
    key: &str,
) -> InputId {
    let input = keyed_prompt(key);
    let input_id = input.id().clone();
    <MeerkatMachine as SessionServiceRuntimeExt>::accept_input(machine, session_id, input)
        .await
        .expect("accept the prompt");
    session_driver(machine, session_id)
        .await
        .lock()
        .await
        .apply_without_run_for_test(&input_id)
        .expect("step the input to applied without a run");
    input_id
}

/// Let every other task run until it is idle. The paused clock advances only
/// then, so a cancellation started before this call has read the input and
/// parked by the time it returns.
async fn until_parked() {
    crate::tokio::time::sleep(Duration::from_millis(1)).await;
}

/// Commit the input's terminal: consume it, as a completed run's boundary
/// does. Returns the paused-clock instant of the commit.
async fn consume(
    machine: &MeerkatMachine,
    session_id: &SessionId,
    input_id: &InputId,
) -> crate::tokio::time::Instant {
    session_driver(machine, session_id)
        .await
        .lock()
        .await
        .consume_inputs_for_test(std::slice::from_ref(input_id), &RunId::new())
        .expect("consume the applied input");
    crate::tokio::time::Instant::now()
}

/// The cancellations' settle bound.
const SETTLE: Duration = Duration::from_secs(5);

/// Commit the input's terminal without the step signal: apply the generated
/// consume straight on the shared machine authority, as a lifecycle apply
/// outside the driver's commit point would.
async fn consume_without_signal(
    machine: &MeerkatMachine,
    session_id: &SessionId,
    input_id: &InputId,
) {
    let authority = session_driver(machine, session_id)
        .await
        .lock()
        .await
        .shared_dsl_authority();
    let mut authority = authority
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    dsl::MeerkatMachineMutator::apply(
        &mut *authority,
        dsl::MeerkatMachineInput::ConsumeInput {
            input_id: input_id.to_string(),
        },
    )
    .expect("consume on the shared machine authority");
}

/// Publish a member residency for the session, as a mob's host does before it
/// cancels a tracked input.
async fn resident_member(
    machine: &Arc<MeerkatMachine>,
    session_id: &SessionId,
) -> meerkat_contracts::wire::supervisor_bridge::BridgeMemberIncarnation {
    let member = meerkat_contracts::wire::supervisor_bridge::BridgeMemberIncarnation {
        mob_id: "input-transition-wait".to_string(),
        agent_identity: "worker".to_string(),
        host_id: "host-a".to_string(),
        binding_generation: 1,
        member_session_id: session_id.to_string(),
        generation: 0,
        fence_token: 1,
    };
    let residency = machine
        .begin_member_residency_update(session_id.clone())
        .await
        .commit(member.clone(), None)
        .await
        .expect("publish the member residency");
    drop(residency);
    member
}

async fn exact_cancellation_waits_for_the_committed_step(machine: Arc<MeerkatMachine>) {
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let input_id = input_between_steps(&machine, &session_id, "exact-between-steps").await;

    // The owned body that `cancel_input_if_present` runs on the process-owned
    // cleanup runtime, driven here on the test's own runtime so the paused
    // clock governs its waits.
    let cancel = crate::tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        let input_id = input_id.clone();
        async move {
            machine
                .cancel_input_if_present_owned(
                    &session_id,
                    &input_id,
                    "cancelled between steps".to_string(),
                )
                .await
        }
    });
    until_parked().await;
    assert!(
        !cancel.is_finished(),
        "an input between steps holds the cancellation until a step commits"
    );

    let committed = consume(&machine, &session_id, &input_id).await;
    let observed = cancel
        .await
        .expect("cancellation task")
        .expect("the cancellation settles on the committed terminal");
    assert!(observed, "the consumed input is observed");
    assert_eq!(
        crate::tokio::time::Instant::now() - committed,
        Duration::ZERO,
        "the committed terminal, not a timer, orders the re-read"
    );
}

#[tokio::test(start_paused = true)]
async fn ephemeral_exact_cancellation_waits_for_the_committed_step() {
    exact_cancellation_waits_for_the_committed_step(Arc::new(MeerkatMachine::ephemeral())).await;
}

#[tokio::test(start_paused = true)]
async fn persistent_exact_cancellation_waits_for_the_committed_step() {
    exact_cancellation_waits_for_the_committed_step(persistent_machine()).await;
}

async fn tracked_cancellation_waits_for_the_committed_step(machine: Arc<MeerkatMachine>) {
    const KEY: &str = "tracked-between-steps";
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let member = resident_member(&machine, &session_id).await;
    let input_id = input_between_steps(&machine, &session_id, KEY).await;

    let cancel = crate::tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .cancel_tracked_input_for_member_incarnation(&session_id, KEY, &member)
                .await
        }
    });
    until_parked().await;
    assert!(
        !cancel.is_finished(),
        "an input between steps holds the tracked cancellation until a step commits"
    );

    let committed = consume(&machine, &session_id, &input_id).await;
    cancel
        .await
        .expect("tracked cancellation task")
        .expect("the tracked cancellation settles on the committed terminal");
    assert_eq!(
        crate::tokio::time::Instant::now() - committed,
        Duration::ZERO,
        "the committed terminal, not a timer, orders the tracked re-read"
    );
}

#[tokio::test(start_paused = true)]
async fn ephemeral_tracked_cancellation_waits_for_the_committed_step() {
    tracked_cancellation_waits_for_the_committed_step(Arc::new(MeerkatMachine::ephemeral())).await;
}

#[tokio::test(start_paused = true)]
async fn persistent_tracked_cancellation_waits_for_the_committed_step() {
    tracked_cancellation_waits_for_the_committed_step(persistent_machine()).await;
}

async fn exact_cancellation_reads_again_at_the_bound(machine: Arc<MeerkatMachine>) {
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let input_id = input_between_steps(&machine, &session_id, "exact-unsignalled").await;

    let cancel = crate::tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        let input_id = input_id.clone();
        async move {
            machine
                .cancel_input_if_present_owned(
                    &session_id,
                    &input_id,
                    "cancelled between steps".to_string(),
                )
                .await
        }
    });
    until_parked().await;
    consume_without_signal(&machine, &session_id, &input_id).await;
    crate::tokio::time::sleep(SETTLE / 2).await;
    assert!(
        !cancel.is_finished(),
        "a terminal committed without the signal does not wake the cancellation"
    );

    let observed = cancel
        .await
        .expect("cancellation task")
        .expect("the read after the bound finds the terminal: no false settle error");
    assert!(observed, "the consumed input is observed");
}

/// R1: a terminal committed without the step signal costs the exact
/// cancellation its settle bound, then the read after it answers.
#[tokio::test(start_paused = true)]
async fn ephemeral_exact_cancellation_reads_again_at_the_bound() {
    exact_cancellation_reads_again_at_the_bound(Arc::new(MeerkatMachine::ephemeral())).await;
}

#[tokio::test(start_paused = true)]
async fn persistent_exact_cancellation_reads_again_at_the_bound() {
    exact_cancellation_reads_again_at_the_bound(persistent_machine()).await;
}

async fn tracked_cancellation_reads_again_at_the_bound(machine: Arc<MeerkatMachine>) {
    const KEY: &str = "tracked-unsignalled";
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let member = resident_member(&machine, &session_id).await;
    let input_id = input_between_steps(&machine, &session_id, KEY).await;

    let cancel = crate::tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .cancel_tracked_input_for_member_incarnation(&session_id, KEY, &member)
                .await
        }
    });
    until_parked().await;
    consume_without_signal(&machine, &session_id, &input_id).await;
    crate::tokio::time::sleep(SETTLE / 2).await;
    assert!(
        !cancel.is_finished(),
        "a terminal committed without the signal does not wake the tracked cancellation"
    );

    cancel
        .await
        .expect("tracked cancellation task")
        .expect("the read after the bound finds the terminal: no false settle error");
}

/// R1: a terminal committed without the step signal costs the tracked
/// cancellation its settle bound, then the read after it answers.
#[tokio::test(start_paused = true)]
async fn ephemeral_tracked_cancellation_reads_again_at_the_bound() {
    tracked_cancellation_reads_again_at_the_bound(Arc::new(MeerkatMachine::ephemeral())).await;
}

#[tokio::test(start_paused = true)]
async fn persistent_tracked_cancellation_reads_again_at_the_bound() {
    tracked_cancellation_reads_again_at_the_bound(persistent_machine()).await;
}

async fn exact_cancellation_after_the_bound_answers_a_settled_terminal(
    machine: Arc<MeerkatMachine>,
) {
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let input_id = input_between_steps(&machine, &session_id, "exact-after-the-bound").await;
    consume(&machine, &session_id, &input_id).await;

    let observed = machine
        .cancel_input_if_present_owned_before(
            &session_id,
            &input_id,
            "cancelled after the bound".to_string(),
            meerkat_core::time_compat::Instant::now(),
        )
        .await
        .expect("a terminal with nothing owed needs no wait, so a spent bound is no error");
    assert!(observed, "the consumed input is observed");
}

/// A read that starts after the settle deadline (the one a wait that reached
/// it falls through to) answers a terminal input with no runless terminal
/// owed its publication at once, instead of failing the publication drain's
/// spent deadline.
#[tokio::test]
async fn ephemeral_exact_cancellation_after_the_bound_answers_a_settled_terminal() {
    exact_cancellation_after_the_bound_answers_a_settled_terminal(Arc::new(
        MeerkatMachine::ephemeral(),
    ))
    .await;
}

#[tokio::test]
async fn persistent_exact_cancellation_after_the_bound_answers_a_settled_terminal() {
    exact_cancellation_after_the_bound_answers_a_settled_terminal(persistent_machine()).await;
}

/// Queue a consume behind a cancellation on the session's driver lock, which
/// the test holds, then release it. The driver mutex is FIFO, so the
/// cancellation reads the input between steps first and the consumer commits
/// its terminal right after. Returns the paused-clock instant of that commit.
async fn consume_after_the_queued_read(
    machine: &MeerkatMachine,
    session_id: &SessionId,
    input_id: &InputId,
    held: crate::tokio::sync::MutexGuard<'_, crate::meerkat_machine::driver::DriverEntry>,
) -> crate::tokio::time::Instant {
    let driver = session_driver(machine, session_id).await;
    let consumer = crate::tokio::spawn({
        let input_id = input_id.clone();
        async move {
            driver
                .lock()
                .await
                .consume_inputs_for_test(std::slice::from_ref(&input_id), &RunId::new())
                .expect("consume the applied input");
            crate::tokio::time::Instant::now()
        }
    });
    until_parked().await;
    assert!(
        !consumer.is_finished(),
        "the consumer queues on the held driver"
    );
    drop(held);
    consumer.await.expect("consumer task")
}

/// L2: the exact cancellation subscribes under the driver lock it reads the
/// input with, so the step the lock's next holder commits wakes it.
#[tokio::test(start_paused = true)]
async fn exact_cancellation_subscribes_under_its_read_lock() {
    let machine = Arc::new(MeerkatMachine::ephemeral());
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let input_id = input_between_steps(&machine, &session_id, "exact-fifo").await;
    let driver = session_driver(&machine, &session_id).await;
    let held = driver.lock().await;

    let cancel = crate::tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        let input_id = input_id.clone();
        async move {
            machine
                .cancel_input_if_present_owned(
                    &session_id,
                    &input_id,
                    "cancelled between steps".to_string(),
                )
                .await
        }
    });
    until_parked().await;
    assert!(
        !cancel.is_finished(),
        "the cancellation queues on the held driver"
    );

    let committed = consume_after_the_queued_read(&machine, &session_id, &input_id, held).await;
    let observed = cancel
        .await
        .expect("cancellation task")
        .expect("the cancellation settles on the committed terminal");
    assert!(observed, "the consumed input is observed");
    assert_eq!(
        crate::tokio::time::Instant::now() - committed,
        Duration::ZERO,
        "the step committed right after the read wakes the cancellation"
    );
}

/// L2: the tracked cancellation subscribes under the driver lock it reads
/// the input with, so the step the lock's next holder commits wakes it.
#[tokio::test(start_paused = true)]
async fn tracked_cancellation_subscribes_under_its_read_lock() {
    const KEY: &str = "tracked-fifo";
    let machine = Arc::new(MeerkatMachine::ephemeral());
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("register session");
    let member = resident_member(&machine, &session_id).await;
    let input_id = input_between_steps(&machine, &session_id, KEY).await;
    let driver = session_driver(&machine, &session_id).await;
    let held = driver.lock().await;

    let cancel = crate::tokio::spawn({
        let machine = Arc::clone(&machine);
        let session_id = session_id.clone();
        async move {
            machine
                .cancel_tracked_input_for_member_incarnation(&session_id, KEY, &member)
                .await
        }
    });
    until_parked().await;
    assert!(
        !cancel.is_finished(),
        "the tracked cancellation queues on the held driver"
    );

    let committed = consume_after_the_queued_read(&machine, &session_id, &input_id, held).await;
    cancel
        .await
        .expect("tracked cancellation task")
        .expect("the tracked cancellation settles on the committed terminal");
    assert_eq!(
        crate::tokio::time::Instant::now() - committed,
        Duration::ZERO,
        "the step committed right after the read wakes the tracked cancellation"
    );
}
