//! A cancellation that meets an input between lifecycle steps (neither queued
//! nor terminal, with no run to cancel) re-reads it when the input's next step
//! commits, not on a timer (#1655). The paused clock makes that observable:
//! no virtual time passes between the commit and the answer.
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
