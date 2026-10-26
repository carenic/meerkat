//! Rows for the terminal a recovered boundary stages for the contributors it
//! consumes: one run-scoped terminal-completion batch in the boundary's own
//! fenced commit, and a directed outbox for each directed contributor when
//! the session's lifecycle row records a runtime placement.
//!
//! Before it, a recovered boundary left each contributor consumed with no
//! receipt, so its exact receipt read answered corruption and a directed
//! contributor's requester never got a terminal.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::input::Input;
use crate::input_state::{
    InputTerminalCompletionBatchKey, InputTerminalCompletionPhase, InputTerminalOutcome,
    InteractionTerminalBatchKey, InteractionTerminalCandidate, InteractionTerminalOutboxPhase,
};
use crate::store::{InMemoryRuntimeStore, PreparedWholeBlobProvisionalTail};
use crate::terminal_status::InputTerminalReceiptRead;
use meerkat_core::types::{
    AssistantBlock, BlockAssistantMessage, TranscriptMessageIdentity, UserMessage,
    message_timestamp_now,
};

const PLACED_RUNTIME: &str = "placed-member-runtime";
const PLACED_FENCE: u64 = 7;
const PLACED_GENERATION: u64 = 2;
const PLACED_EPOCH: &str = "placed-member-epoch";

struct Seeded {
    store: Arc<InMemoryRuntimeStore>,
    session_id: SessionId,
    runtime_id: LogicalRuntimeId,
    run_id: RunId,
}

/// The tail a run leaves when it answered and its end-of-run save landed
/// but its boundary did not.
fn completed_tail(mut session: Session, run_id: &RunId) -> Session {
    session.push(Message::BlockAssistant(BlockAssistantMessage {
        blocks: vec![AssistantBlock::Text {
            text: "the answer".to_string(),
            meta: None,
        }],
        stop_reason: Some(StopReason::EndTurn),
        identity: TranscriptMessageIdentity::default().with_run_id(run_id.clone()),
        created_at: message_timestamp_now(),
        assistant_message_id: None,
    }));
    session
}

/// The tail a run leaves when it was cut after a tool round: its call is
/// answered, but no final answer and no boundary followed.
fn interrupted_tail(mut session: Session, run_id: &RunId) -> Session {
    session.push(Message::BlockAssistant(BlockAssistantMessage {
        blocks: vec![AssistantBlock::ToolUse {
            id: "cut-call".to_string(),
            name: "cut_tool".to_string(),
            args: serde_json::value::RawValue::from_string("{}".to_string()).unwrap(),
            meta: None,
        }],
        stop_reason: Some(StopReason::ToolUse),
        identity: TranscriptMessageIdentity::default().with_run_id(run_id.clone()),
        created_at: message_timestamp_now(),
        assistant_message_id: None,
    }));
    session.push(Message::tool_results(vec![ToolResult::new(
        "cut-call".to_string(),
        "the tool's result".to_string(),
        false,
    )]));
    session
}

/// A prompt row applied by `run_id` and waiting for its missing boundary.
fn applied_to(input: &Input, run_id: &RunId) -> StoredInputState {
    let mut state = crate::input_state::InputState::new_accepted(input.id().clone());
    state.persisted_input = Some(input.clone());
    state.durability = Some(crate::input::InputDurability::Durable);
    let policy = crate::DefaultPolicyTable::resolve(input, true);
    state.runtime_semantics = Some(
        crate::ingress_types::RuntimeInputSemantics::try_from_generated_admission(input, true)
            .expect("generated admission semantics"),
    );
    state.policy = Some(crate::input_state::PolicySnapshot {
        version: policy.policy_version,
        decision: policy,
    });
    StoredInputState {
        state,
        seed: crate::input_state::InputStateSeed {
            phase: InputLifecycleState::AppliedPendingConsumption,
            last_run_id: Some(run_id.clone()),
            last_boundary_sequence: Some(1),
            terminal_outcome: None,
            attempt_count: 1,
            admission_sequence: None,
            recovery_lane: Some(meerkat_core::types::HandlingMode::Queue),
        },
    }
}

fn prompt(text: &str) -> Input {
    Input::Prompt(crate::input::PromptInput::new(text, None))
}

/// A tracked flow step: a directed input whose terminal its requester waits
/// for.
fn directed_step(label: &str) -> Input {
    crate::mob_adapter::create_tracked_flow_step_input(
        label,
        meerkat_core::types::ContentInput::Text(format!("directed {label}")),
        "recovered-boundary-flow",
        None,
        &uuid::Uuid::new_v4().to_string(),
    )
    .expect("a directed flow step")
}

/// A session whose run's tail (`tail`) stands as the store's provisional
/// tail, each of `inputs` applied by that run and waiting for its boundary.
/// With `placed`, the session's lifecycle row is the quiescent Idle row of a
/// runtime that held a placement.
async fn seeded(inputs: &[Input], tail: fn(Session, &RunId) -> Session, placed: bool) -> Seeded {
    let store = Arc::new(InMemoryRuntimeStore::new());
    let run_id = RunId::new();
    let mut committed = Session::new();
    committed.push(Message::User(UserMessage::text("the run's prompt")));
    let session_id = committed.id().clone();
    let runtime_id = LogicalRuntimeId::for_session(&session_id);
    let base = store
        .inject_committed_whole_blob_bytes_for_test(
            &runtime_id,
            &session_id,
            serde_json::to_vec(&committed).expect("the committed session serializes"),
        )
        .await
        .expect("the committed base");
    let candidate = tail(committed, &run_id);
    store
        .write_prepared_whole_blob_provisional_tail(
            &runtime_id,
            PreparedWholeBlobProvisionalTail::prepare_from_session(
                base,
                run_id.clone(),
                1,
                &candidate,
            )
            .expect("the run's tail prepares"),
        )
        .await
        .expect("the run's tail is written");
    if placed {
        store
            .commit_machine_lifecycle(
                &runtime_id,
                MachineLifecycleCommit::new_with_binding_run_unregister_progress_and_live_bridge(
                    RuntimeState::Idle,
                    MachineLifecycleBindingFacts::new(
                        Some(PLACED_RUNTIME.to_string()),
                        Some(PLACED_FENCE),
                        Some(PLACED_GENERATION),
                        Some(PLACED_EPOCH.to_string()),
                    ),
                    MachineLifecycleRunFacts::default(),
                    SupervisorAuthoritySnapshot::UnboundNoReceipt,
                    None,
                    crate::live_execution::LiveBridgeRecoveryImage::default(),
                ),
                &[],
            )
            .await
            .expect("the placed runtime's quiescent lifecycle row");
    }
    for input in inputs {
        store
            .persist_input_state(
                &runtime_id,
                &InputStatePersistenceRecord::stored_row_for_test(applied_to(input, &run_id))
                    .expect("an applied row"),
            )
            .await
            .expect("the applied row is written");
    }
    Seeded {
        store,
        session_id,
        runtime_id,
        run_id,
    }
}

impl Seeded {
    async fn rows(&self) -> Vec<StoredInputState> {
        self.store
            .load_input_states_strict(&self.runtime_id)
            .await
            .expect("the session's input rows load")
    }

    async fn recover(&self) -> DurableTailRecoveryDisposition {
        match recover_durable_tail(self.store.as_ref(), &self.session_id)
            .await
            .expect("the recovery answers")
        {
            DurableTailRecoveryOutcome::Committed { disposition, .. } => disposition,
            other => panic!("the recovery must commit the run's tail, answered {other:?}"),
        }
    }
}

fn row<'a>(rows: &'a [StoredInputState], input: &Input) -> &'a StoredInputState {
    rows.iter()
        .find(|row| &row.state.input_id == input.id())
        .expect("the input's row")
}

/// A completed tail's contributors get one run-scoped batch in the
/// boundary's commit: consumed, each with a pending receipt of the batch
/// keyed by the recovered run, its owner the first contributor in canonical
/// order holding `CompletedWithoutResult` and every recipient. The directed
/// contributor gets a `Candidate` outbox naming the placement the lifecycle
/// row records. Their exact receipt reads answer pending (the next drain
/// finalizes them), where before they answered corruption.
#[tokio::test]
async fn a_completed_boundary_stages_one_run_batch_for_its_contributors() {
    let nondirected = prompt("the run's prompt");
    let directed = directed_step("joined");
    let seeded = seeded(
        &[nondirected.clone(), directed.clone()],
        completed_tail,
        true,
    )
    .await;

    assert_eq!(
        seeded.recover().await,
        DurableTailRecoveryDisposition::CommitCompleted
    );

    let rows = seeded.rows().await;
    let mut contributors = vec![nondirected.id().clone(), directed.id().clone()];
    contributors.sort_by_key(|input_id| input_id.0);
    let owner_input_id = contributors[0].clone();
    for input in [&nondirected, &directed] {
        let stored = row(&rows, input);
        assert_eq!(stored.seed.phase, InputLifecycleState::Consumed);
        assert_eq!(
            stored.seed.terminal_outcome,
            Some(InputTerminalOutcome::Consumed)
        );
        assert_eq!(stored.seed.last_run_id.as_ref(), Some(&seeded.run_id));
        let completion = stored
            .state
            .terminal_completion
            .as_ref()
            .expect("the contributor's receipt is staged with the boundary");
        assert_eq!(
            completion.batch_key,
            InputTerminalCompletionBatchKey::Run {
                run_id: seeded.run_id.clone()
            }
        );
        assert!(matches!(
            completion.phase,
            InputTerminalCompletionPhase::Pending
        ));
        assert_eq!(completion.owner_input_id, owner_input_id);
        assert!(!completion.requires_session_checkpoint);
        assert!(
            stored.state.persisted_input.is_some(),
            "a contributor keeps its payload while its receipt is pending"
        );
        match crate::terminal_status::input_terminal_receipt_read(&rows, input.id()) {
            Ok(Some(InputTerminalReceiptRead::Pending {
                terminal: Some(InputTerminalOutcome::Consumed),
                last_run_id: Some(run_id),
                ..
            })) if run_id == seeded.run_id => {}
            other => panic!(
                "the exact receipt read must answer the staged receipt as pending, got {other:?}"
            ),
        }
    }
    let owner = row(
        &rows,
        if nondirected.id() == &owner_input_id {
            &nondirected
        } else {
            &directed
        },
    );
    let owner_completion = owner.state.terminal_completion.as_ref().unwrap();
    assert!(matches!(
        owner_completion.candidate,
        Some(InteractionTerminalCandidate::CompletedWithoutResult)
    ));
    assert_eq!(
        owner_completion.completion_input_ids.as_ref(),
        Some(&contributors)
    );

    assert!(
        row(&rows, &nondirected)
            .state
            .interaction_terminal_outbox
            .is_none(),
        "a nondirected contributor has no requester to answer"
    );
    let outbox = row(&rows, &directed)
        .state
        .interaction_terminal_outbox
        .as_ref()
        .expect("the directed contributor's requester terminal is staged");
    assert!(matches!(
        outbox.phase,
        InteractionTerminalOutboxPhase::Candidate
    ));
    assert_eq!(
        outbox.batch_key,
        InteractionTerminalBatchKey::Run {
            run_id: seeded.run_id.clone()
        }
    );
    assert_eq!(&outbox.candidate_owner_input_id, directed.id());
    assert_eq!(outbox.owner_session_id, seeded.session_id);
    assert_eq!(
        outbox.owner_agent_runtime_id.as_deref(),
        Some(PLACED_RUNTIME)
    );
    assert_eq!(outbox.owner_fence_token, Some(PLACED_FENCE));
    assert_eq!(outbox.owner_runtime_generation, Some(PLACED_GENERATION));
    assert_eq!(outbox.owner_runtime_epoch_id.as_deref(), Some(PLACED_EPOCH));
    assert_eq!(outbox.candidate_digest, owner_completion.candidate_digest);

    let pending_owners = seeded
        .store
        .load_pending_terminal_owner_ids_page(&seeded.runtime_id, None, 16)
        .await
        .expect("the pending-owner index reads");
    assert!(pending_owners.contains(&owner_input_id));
    assert!(pending_owners.contains(directed.id()));
}

/// An interrupted tail's contributors get the batch too, answered
/// `Cancelled`: the class the live path gives a run that ends before it
/// completes.
#[tokio::test]
async fn an_interrupted_boundary_answers_its_contributors_cancelled() {
    let directed = directed_step("interrupted");
    let seeded = seeded(&[directed.clone()], interrupted_tail, true).await;

    assert_eq!(
        seeded.recover().await,
        DurableTailRecoveryDisposition::RepairAndCommitInterrupted
    );

    let rows = seeded.rows().await;
    let stored = row(&rows, &directed);
    assert_eq!(stored.seed.phase, InputLifecycleState::Consumed);
    let completion = stored
        .state
        .terminal_completion
        .as_ref()
        .expect("the contributor's receipt is staged with the boundary");
    assert!(matches!(
        completion.candidate,
        Some(InteractionTerminalCandidate::Cancelled)
    ));
    assert!(matches!(
        completion.phase,
        InputTerminalCompletionPhase::Pending
    ));
    assert!(
        stored.state.interaction_terminal_outbox.is_some(),
        "the directed contributor's requester gets the cancellation"
    );
}

/// Without a placement in the session's lifecycle row (here: no row), the
/// boundary still stages every contributor's receipt, but no outbox: a
/// directed contributor's requester stays unanswered, as before.
#[tokio::test]
async fn a_boundary_without_a_recorded_placement_stages_receipts_but_no_outbox() {
    let directed = directed_step("unplaced");
    let seeded = seeded(&[directed.clone()], completed_tail, false).await;

    assert_eq!(
        seeded.recover().await,
        DurableTailRecoveryDisposition::CommitCompleted
    );

    let rows = seeded.rows().await;
    let stored = row(&rows, &directed);
    assert!(stored.state.terminal_completion.is_some());
    assert!(stored.state.interaction_terminal_outbox.is_none());
    assert!(
        stored.state.persisted_input.is_some(),
        "a directed row without its outbox keeps the payload that owns its interaction"
    );
}

/// Beside another run's batch that has not finished, the boundary stages
/// nothing: the generated completion authority recovers one pending
/// run-scoped batch at a time, so a second one would block the first. The
/// contributors are left as before (consumed with no receipt), and the other
/// run's batch is untouched.
#[tokio::test]
async fn a_boundary_beside_another_runs_pending_batch_stages_nothing() {
    let nondirected = prompt("the run's prompt");
    let seeded = seeded(&[nondirected.clone()], completed_tail, true).await;

    // An earlier run's contributor, consumed, whose receipt is still pending.
    let earlier_run = RunId::new();
    let earlier = prompt("the earlier run's prompt");
    let mut earlier_row = applied_to(&earlier, &earlier_run);
    earlier_row.seed.phase = InputLifecycleState::Consumed;
    earlier_row.seed.terminal_outcome = Some(InputTerminalOutcome::Consumed);
    earlier_row.seed.recovery_lane = None;
    earlier_row.state.terminal_completion = Some(
        crate::meerkat_machine::driver::build_input_terminal_completion_rows(
            crate::meerkat_machine::driver::InteractionTerminalBatchScope::Run(&earlier_run),
            std::slice::from_ref(earlier.id()),
            InteractionTerminalCandidate::CompletedWithoutResult,
            false,
        )
        .expect("the earlier run's batch")
        .remove(0),
    );
    seeded
        .store
        .persist_input_state(
            &seeded.runtime_id,
            &InputStatePersistenceRecord::stored_row_for_test(earlier_row)
                .expect("the earlier run's row"),
        )
        .await
        .expect("the earlier run's row is written");

    assert_eq!(
        seeded.recover().await,
        DurableTailRecoveryDisposition::CommitCompleted
    );

    let rows = seeded.rows().await;
    let stored = row(&rows, &nondirected);
    assert_eq!(stored.seed.phase, InputLifecycleState::Consumed);
    assert!(
        stored.state.terminal_completion.is_none(),
        "no second pending run batch is staged beside the earlier one"
    );
    assert!(
        crate::terminal_status::input_terminal_receipt_read(&rows, nondirected.id()).is_err(),
        "the contributor's receipt read answers as before"
    );
    let earlier_stored = row(&rows, &earlier);
    assert!(matches!(
        earlier_stored
            .state
            .terminal_completion
            .as_ref()
            .map(|completion| &completion.phase),
        Some(InputTerminalCompletionPhase::Pending)
    ));
}
