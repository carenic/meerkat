//! Machine rows for the cold-predecessor settlement: the claim, profile and
//! registration refusals, the row scan, the placement witness, the marker on
//! the internal registration, its exact removal and the process-owned task;
//! then whole settlements over a HeadCanonical durable tail, driven step by
//! step as the session service drives them, with a recording publisher in
//! place of the service's stored-only one (the service's own rows are the
//! session crate's).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::input::Input;
use crate::input_state::{InputLifecycleState, InputStatePersistenceRecord};
use crate::session_hosting::HostingPaths;
use crate::store::RuntimeStore;
use meerkat_core::lifecycle::core_executor::{
    CoreExecutorError, CoreExecutorPublicationHandle, CoreInteractionTerminalPublicationReceipt,
};

const BOUND: std::time::Duration = std::time::Duration::from_secs(10);

fn deadline() -> meerkat_core::time_compat::Instant {
    meerkat_core::time_compat::Instant::now() + BOUND
}

fn memory_blob_store() -> Arc<dyn meerkat_core::BlobStore> {
    Arc::new(meerkat_store::MemoryBlobStore::new())
}

/// A HeadCanonical store whose sessions are hosted under cross-process OS
/// locks, and its machine.
struct Host {
    _dir: tempfile::TempDir,
    store: Arc<crate::store::SqliteRuntimeStore>,
    machine: MeerkatMachine,
}

impl Host {
    fn head_canonical() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(
            crate::store::SqliteRuntimeStore::new_head_canonical(
                dir.path().join("runtime.sqlite3"),
            )
            .unwrap()
            .with_hosting_paths(HostingPaths {
                hosting_lock_dir: dir.path().join("hosting"),
                cold_delivery_lock: dir.path().join("delivery").join("cold-delivery.lock"),
                database: None,
            }),
        );
        let machine = MeerkatMachine::persistent(
            Arc::clone(&store) as Arc<dyn RuntimeStore>,
            memory_blob_store(),
        )
        .unwrap();
        Self {
            _dir: dir,
            store,
            machine,
        }
    }

    fn claim(&self, session_id: &SessionId) -> HostingClaim {
        let claim = self
            .machine
            .grant_session_hosting(session_id)
            .expect("the host grants its own claim");
        assert!(claim.is_cross_process());
        claim
    }

    async fn persist(&self, session_id: &SessionId, stored: StoredInputState) {
        self.store
            .persist_input_state(
                &LogicalRuntimeId::for_session(session_id),
                &InputStatePersistenceRecord::stored_row_for_test(stored).expect("a stored row"),
            )
            .await
            .expect("the row is written");
    }

    fn write_corrupt_row(&self, session_id: &SessionId, key: &str) {
        let conn = rusqlite::Connection::open(self.store.path()).unwrap();
        conn.execute(
            "INSERT INTO runtime_input_states (runtime_id, input_id, state_json)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                LogicalRuntimeId::for_session(session_id).0,
                key,
                b"{\"not\":\"an input state\"}".as_slice()
            ],
        )
        .unwrap();
    }
}

fn prompt(text: &str) -> Input {
    Input::Prompt(crate::input::PromptInput::new(text, None))
}

fn directed_step(label: &str) -> Input {
    crate::mob_adapter::create_tracked_flow_step_input(
        label,
        meerkat_core::types::ContentInput::Text(format!("directed {label}")),
        "predecessor-settlement-flow",
        None,
        &uuid::Uuid::new_v4().to_string(),
    )
    .expect("a directed flow step")
}

/// A row of `input` as a predecessor left it: `phase`, with the run and
/// boundary it names.
fn row(
    input: &Input,
    phase: InputLifecycleState,
    run_id: Option<RunId>,
    boundary_sequence: Option<u64>,
) -> StoredInputState {
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
    let terminal = phase == InputLifecycleState::Consumed;
    StoredInputState {
        state,
        seed: crate::input_state::InputStateSeed {
            phase,
            last_run_id: run_id,
            last_boundary_sequence: boundary_sequence,
            terminal_outcome: terminal.then_some(InputTerminalOutcome::Consumed),
            attempt_count: u32::from(terminal),
            admission_sequence: None,
            recovery_lane: (!terminal).then_some(meerkat_core::types::HandlingMode::Queue),
        },
    }
}

/// A stored-only publisher that records what it publishes.
#[derive(Default)]
struct RecordingPublisher {
    published: std::sync::Mutex<Vec<meerkat_core::event::AgentEvent>>,
}

impl RecordingPublisher {
    fn events(&self) -> Vec<meerkat_core::event::AgentEvent> {
        self.published.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl CoreExecutorPublicationHandle for RecordingPublisher {
    async fn publish_interaction_terminals(
        &self,
        events: &[meerkat_core::event::AgentEvent],
    ) -> Result<Vec<CoreInteractionTerminalPublicationReceipt>, CoreExecutorError> {
        let mut published = self.published.lock().unwrap();
        events
            .iter()
            .map(|event| {
                published.push(event.clone());
                CoreInteractionTerminalPublicationReceipt::try_new(event, published.len() as u64)
            })
            .collect()
    }
}

/// Only this machine's own cross-process claim of the session settles
/// it. Another machine's claim is refused `ClaimNotOwned`, and a claim with
/// no cross-process OS lock `ExclusivityUnproven`; nothing is touched.
#[tokio::test]
async fn a_settlement_refuses_another_owners_claim_and_a_claim_without_an_os_lock() {
    let host = Host::head_canonical();
    let other = Host::head_canonical();
    let session_id = SessionId::new();
    let foreign = other.claim(&session_id);
    assert!(matches!(
        host.machine
            .begin_predecessor_settlement_before(&foreign, deadline())
            .await,
        Err(PredecessorSettlementRefusal::ClaimNotOwned)
    ));

    let dir = tempfile::tempdir().unwrap();
    let unlocked: Arc<dyn RuntimeStore> = Arc::new(
        crate::store::SqliteRuntimeStore::new_head_canonical(dir.path().join("runtime.sqlite3"))
            .unwrap(),
    );
    let machine = MeerkatMachine::persistent(unlocked, memory_blob_store()).unwrap();
    let unlocked_claim = machine
        .grant_session_hosting(&session_id)
        .expect("a store without OS locks still grants its own claim");
    assert!(!unlocked_claim.is_cross_process());
    assert!(matches!(
        machine
            .begin_predecessor_settlement_before(&unlocked_claim, deadline())
            .await,
        Err(PredecessorSettlementRefusal::ExclusivityUnproven)
    ));
    assert!(!host.machine.contains_session(&session_id).await);
    assert!(!machine.contains_session(&session_id).await);
}

/// A WholeBlob store refuses the settlement before anything is
/// touched.
#[tokio::test]
async fn a_whole_blob_store_refuses_the_settlement_before_anything_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn RuntimeStore> = Arc::new(
        crate::store::SqliteRuntimeStore::new(dir.path().join("runtime.sqlite3"))
            .unwrap()
            .with_hosting_paths(HostingPaths {
                hosting_lock_dir: dir.path().join("hosting"),
                cold_delivery_lock: dir.path().join("delivery").join("cold-delivery.lock"),
                database: None,
            }),
    );
    let machine = MeerkatMachine::persistent(store, memory_blob_store()).unwrap();
    let session_id = SessionId::new();
    let claim = machine.grant_session_hosting(&session_id).unwrap();
    assert!(matches!(
        machine
            .begin_predecessor_settlement_before(&claim, deadline())
            .await,
        Err(PredecessorSettlementRefusal::ProfileUnsupported { .. })
    ));
    assert!(!machine.contains_session(&session_id).await);
}

/// A live registration of the session refuses the settlement
/// `Registered`, and the registration is untouched.
#[tokio::test]
async fn a_registered_session_refuses_the_settlement() {
    let host = Host::head_canonical();
    let session_id = SessionId::new();
    let claim = host.claim(&session_id);
    host.machine
        .register_session(session_id.clone())
        .await
        .expect("the session registers under the host's lineage");
    assert!(matches!(
        host.machine
            .begin_predecessor_settlement_before(&claim, deadline())
            .await,
        Err(PredecessorSettlementRefusal::Registered)
    ));
    assert!(host.machine.contains_session(&session_id).await);
}

/// A row that does not decode, and a consumed row with a run whose
/// boundary receipt is not durable, are rows the settlement cannot
/// account for. It refuses `UnsettleableRows` naming them, before anything
/// is touched or registered; the rows are unchanged.
#[tokio::test]
async fn rows_the_settlement_cannot_account_for_refuse_it_untouched() {
    let host = Host::head_canonical();
    let session_id = SessionId::new();
    let claim = host.claim(&session_id);
    host.write_corrupt_row(&session_id, "zz-undecodable-row");
    let consumed = prompt("consumed without a boundary");
    host.persist(
        &session_id,
        row(
            &consumed,
            InputLifecycleState::Consumed,
            Some(RunId::new()),
            Some(1),
        ),
    )
    .await;

    match host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
    {
        Err(PredecessorSettlementRefusal::UnsettleableRows {
            undecodable,
            unproven,
        }) => {
            assert_eq!(undecodable, vec!["zz-undecodable-row".to_string()]);
            assert_eq!(unproven, vec![consumed.id().clone()]);
        }
        other => panic!("the settlement refuses the rows it cannot account for: {other:?}"),
    }
    assert!(!host.machine.contains_session(&session_id).await);
    let rows = host
        .store
        .load_input_states(&LogicalRuntimeId::for_session(&session_id))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "both rows stay as they were");
}

/// A queued directed input would need a new outbox, and the session's
/// lifecycle row records neither a complete binding nor a placement
/// witness: the settlement refuses `PlacementWitnessMissing` naming it,
/// before anything is touched.
#[tokio::test]
async fn directed_work_without_a_placement_witness_refuses_the_settlement() {
    let host = Host::head_canonical();
    let session_id = SessionId::new();
    let claim = host.claim(&session_id);
    let directed = directed_step("queued without a placement");
    host.persist(
        &session_id,
        row(&directed, InputLifecycleState::Queued, None, None),
    )
    .await;

    match host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
    {
        Err(PredecessorSettlementRefusal::PlacementWitnessMissing { directed: named }) => {
            assert_eq!(named, vec![directed.id().clone()]);
        }
        other => panic!("directed work without a placement witness refuses: {other:?}"),
    }
    assert!(!host.machine.contains_session(&session_id).await);
}

/// The internal registration's marker, its exact removal and registrable
/// afterwards. While the settlement's registration stands, the session's
/// entry is the settlement's, and every taker that reads it (a
/// registration, an adoption, an admission, a lifecycle mutation, an
/// unregister) is refused `UnregisterInProgress`; once finish's task ran,
/// the session holds no registration, the census found nothing owed, and a
/// fresh registration inserts in the caller's lineage.
#[tokio::test]
async fn the_settlement_registration_refuses_every_taker_and_leaves_exactly() {
    let host = Host::head_canonical();
    let session_id = SessionId::new();
    let claim = host.claim(&session_id);
    let mut lease = host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
        .expect("the settlement begins");
    let work = host
        .machine
        .end_predecessor_work_with_lease_before(&mut lease, deadline())
        .await
        .expect("the internal registration publishes and sweeps");
    assert_eq!(work.owner_loss, PredecessorOwnerLoss::NoConsentOwner);
    assert!(work.ended.is_empty());
    assert!(work.answered.is_empty());
    assert!(lease.registered());
    assert!(host.machine.contains_session(&session_id).await);

    // The lease holds the registration transaction and the mutation gate
    // here, so the takers are proven where every one of them reads the
    // entry.
    {
        let sessions = host.machine.sessions.read().await;
        let entry = sessions
            .get(&session_id)
            .expect("the settlement's registration stands");
        assert!(entry.is_predecessor_settlement());
        assert!(matches!(
            entry.registration_blocked_by_unregister(&session_id),
            Some(RuntimeDriverError::UnregisterInProgress { .. })
        ));
        assert!(matches!(
            entry.dsl_mutation_blocked_by_unregister(&session_id),
            Some(RuntimeDriverError::UnregisterInProgress { .. })
        ));
    }

    let publication = host
        .machine
        .finish_predecessor_settlement_before(
            lease,
            Some(Arc::new(RecordingPublisher::default())),
            deadline(),
        )
        .await
        .expect("the task drains, finds every row closed and removes");
    assert!(publication.converged.is_empty());
    assert!(publication.owed.is_empty());
    assert_eq!(publication.durable_rows_closed, 0);
    assert!(!host.machine.contains_session(&session_id).await);

    host.machine
        .register_session(session_id.clone())
        .await
        .expect("the session is registrable afterwards");
    assert_eq!(
        host.machine
            .registration_claim_same_lineage(&session_id, &claim)
            .await,
        Some(true),
        "the fresh registration joined the caller's lineage"
    );
}

/// A lease dropped after its internal registration published hands
/// the exact removal to the process-owned task, and a later settlement
/// waits for that task, holding no gate, before it begins.
#[tokio::test]
async fn a_dropped_lease_hands_its_removal_to_the_task_and_a_repeat_waits_for_it() {
    let host = Host::head_canonical();
    let session_id = SessionId::new();
    let claim = host.claim(&session_id);
    let mut lease = host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
        .expect("the settlement begins");
    host.machine
        .end_predecessor_work_with_lease_before(&mut lease, deadline())
        .await
        .expect("the internal registration publishes");
    drop(lease);

    let repeat = host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
        .expect("the repeat begins once the dropped lease's task removed the registration");
    assert!(!repeat.registered());
    assert!(!host.machine.contains_session(&session_id).await);
}

/// A queued nondirected input the predecessor left ends `Abandoned {
/// Stopped }` in the sweep, with its runless receipt, which the task
/// finalizes: the census lists it `Ended`, with no publication, and finds
/// its row closed.
#[tokio::test]
async fn a_queued_nondirected_input_is_ended_stopped_with_its_runless_receipt() {
    let host = Host::head_canonical();
    let session_id = SessionId::new();
    let claim = host.claim(&session_id);
    let queued = prompt("queued by the predecessor");
    let mut queued_row = row(&queued, InputLifecycleState::Queued, None, None);
    queued_row.seed.admission_sequence = Some(1);
    host.persist(&session_id, queued_row).await;

    let mut lease = host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
        .expect("the settlement begins");
    let work = host
        .machine
        .end_predecessor_work_with_lease_before(&mut lease, deadline())
        .await
        .expect("the sweep ends the queued input");
    assert_eq!(work.ended, vec![queued.id().clone()]);
    let publication = host
        .machine
        .finish_predecessor_settlement_before(
            lease,
            Some(Arc::new(RecordingPublisher::default())),
            deadline(),
        )
        .await
        .expect("the task finalizes the receipt and finds every row closed");
    let [settled] = publication.converged.as_slice() else {
        panic!("one input settled: {:?}", publication.converged);
    };
    assert_eq!(&settled.input_id, queued.id());
    assert_eq!(settled.settled_by, SettledBy::Ended);
    assert_eq!(settled.published_terminal_seq, None);
    assert_eq!(
        settled.receipt.terminal(),
        &InputTerminalOutcome::Abandoned {
            reason: InputAbandonReason::Stopped,
        }
    );
    assert_eq!(settled.receipt.run_id(), None);
    assert_eq!(publication.durable_rows_closed, 1);
    assert!(publication.owed.is_empty());
    assert!(!host.machine.contains_session(&session_id).await);
}

// ---------------------------------------------------------------------------
// Whole settlements over a HeadCanonical durable tail
// ---------------------------------------------------------------------------

use crate::completion::CompletionOutcome;
use crate::store::{
    MachineLifecycleBindingFacts, MachineLifecycleCommit, MachineLifecycleObservation,
    MachineLifecycleRunFacts, SupervisorAuthoritySnapshot,
};
use crate::terminal_status::InputTerminalReceiptScope;
use meerkat_core::event::{AgentEvent, InteractionFailureReason};
use meerkat_core::types::{
    AssistantBlock, BlockAssistantMessage, Message, StopReason, ToolResult,
    TranscriptMessageIdentity, UserMessage, message_timestamp_now,
};

const PLACED_RUNTIME: &str = "predecessor-runtime";
const PLACED_FENCE: u64 = 11;
const PLACED_GENERATION: u64 = 3;
const PLACED_EPOCH: &str = "predecessor-epoch";

/// The placement of the runtime that hosted the session before.
fn predecessor_placement() -> MachineLifecycleBindingFacts {
    MachineLifecycleBindingFacts::new(
        Some(PLACED_RUNTIME.to_string()),
        Some(PLACED_FENCE),
        Some(PLACED_GENERATION),
        Some(PLACED_EPOCH.to_string()),
    )
}

impl Host {
    /// The session's lifecycle row as a quiescent runtime left it: Idle,
    /// with `binding`, and `witness` as its placement witness.
    async fn lifecycle_row(
        &self,
        session_id: &SessionId,
        binding: MachineLifecycleBindingFacts,
        witness: Option<MachineLifecycleBindingFacts>,
    ) {
        self.store
            .commit_machine_lifecycle(
                &LogicalRuntimeId::for_session(session_id),
                MachineLifecycleCommit::new_with_binding_run_unregister_progress_and_live_bridge(
                    crate::runtime_state::RuntimeState::Idle,
                    binding,
                    MachineLifecycleRunFacts::default(),
                    SupervisorAuthoritySnapshot::UnboundNoReceipt,
                    None,
                    crate::live_execution::LiveBridgeRecoveryImage::default(),
                )
                .with_placement_witness(witness),
                &[],
            )
            .await
            .expect("the lifecycle row is written");
    }

    async fn rows(&self, session_id: &SessionId) -> Vec<StoredInputState> {
        self.store
            .load_input_states_strict(&LogicalRuntimeId::for_session(session_id))
            .await
            .expect("the session's input rows load")
    }

    /// The placement witness the session's lifecycle row carries.
    async fn placement_witness(
        &self,
        session_id: &SessionId,
    ) -> Option<MachineLifecycleBindingFacts> {
        match self
            .store
            .observe_machine_lifecycle(&LogicalRuntimeId::for_session(session_id))
            .await
            .expect("the lifecycle row reads")
        {
            MachineLifecycleObservation::Decoded { record, .. } => {
                record.placement_witness().cloned()
            }
            other => panic!("the lifecycle row decodes: {other:?}"),
        }
    }

    async fn provisional_tail_stands(&self, session_id: &SessionId) -> bool {
        self.store
            .load_head_canonical_provisional_tail(&LogicalRuntimeId::for_session(session_id))
            .await
            .expect("the provisional tail reads")
            .is_some()
    }
}

/// The tail a run leaves when it answered and its end-of-run save landed
/// but its boundary did not.
fn completed_tail(mut session: meerkat_core::Session, run_id: &RunId) -> meerkat_core::Session {
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
fn interrupted_tail(mut session: meerkat_core::Session, run_id: &RunId) -> meerkat_core::Session {
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

/// A session with one committed boundary (its first turn's prompt) on the
/// host's store and the co-located session store. With `tail`, run `run_id`
/// then left `tail` as the store's provisional tail and the physical head,
/// and the process died before the run's boundary commit.
async fn durable_session(
    host: &Host,
    tail: Option<fn(meerkat_core::Session, &RunId) -> meerkat_core::Session>,
) -> (SessionId, RunId) {
    use meerkat_core::lifecycle::core_executor::BoundSessionCommit;
    use meerkat_core::session_store::PreparedHeadCanonicalMutation;
    use meerkat_core::{IncrementalSessionStore as _, SessionStore as _};

    let session_store = meerkat_store::SqliteSessionStore::open(host.store.path())
        .expect("the co-located session store");
    let mut committed = meerkat_core::Session::new();
    committed.push(Message::User(UserMessage::text("the run's prompt")));
    let session_id = committed.id().clone();
    let runtime_id = LogicalRuntimeId::for_session(&session_id);
    let root = PreparedHeadCanonicalMutation::prepare(&committed, None)
        .expect("the committed head prepares");
    let document = BoundSessionCommit::sealed(Arc::new(committed))
        .expect("the committed document seals")
        .with_head_canonical_mutation(root)
        .expect("the committed head binds");
    let committed_authority = host
        .store
        .commit_prepared_session_boundary(
            &runtime_id,
            crate::store::PreparedRuntimeSessionCommit::snapshot_only(document),
        )
        .await
        .expect("the first boundary commits")
        .authority()
        .and_then(|authority| authority.head_canonical())
        .cloned()
        .expect("the boundary returns HeadCanonical authority");
    let run_id = RunId::new();
    if let Some(tail) = tail {
        let base = session_store
            .load(&session_id)
            .await
            .expect("the committed session loads")
            .expect("the committed session exists");
        let candidate = tail(base, &run_id);
        let observed = session_store
            .load_head(&session_id)
            .await
            .expect("the committed head loads")
            .expect("the committed head exists");
        let mutation = PreparedHeadCanonicalMutation::prepare(&candidate, Some(observed))
            .expect("the run's tail prepares");
        host.store
            .write_prepared_head_canonical_provisional_tail(
                &runtime_id,
                crate::store::PreparedHeadCanonicalProvisionalTail::prepare(
                    committed_authority,
                    run_id.clone(),
                    mutation.successor_head(),
                    mutation.successor_head_token(),
                    &candidate,
                )
                .expect("the provisional tail prepares"),
            )
            .await
            .expect("the provisional tail is written");
        session_store
            .apply_prepared_head_canonical_mutation(&mutation)
            .await
            .expect("the physical head moves past the boundary");
    }
    (session_id, run_id)
}

/// `input` applied by `run_id` and waiting for the run's boundary.
fn applied_to(input: &Input, run_id: &RunId) -> StoredInputState {
    let mut stored = row(
        input,
        InputLifecycleState::AppliedPendingConsumption,
        Some(run_id.clone()),
        Some(1),
    );
    stored.seed.attempt_count = 1;
    stored
}

/// `input` queued, never bound to a run, admitted `admission`-th.
fn queued(input: &Input, admission: u64) -> StoredInputState {
    let mut stored = row(input, InputLifecycleState::Queued, None, None);
    stored.seed.admission_sequence = Some(admission);
    stored
}

fn record(stored: StoredInputState) -> InputStatePersistenceRecord {
    InputStatePersistenceRecord::stored_row_for_test(stored).expect("a stored row")
}

fn stored_row<'a>(rows: &'a [StoredInputState], input: &Input) -> &'a StoredInputState {
    rows.iter()
        .find(|stored| &stored.state.input_id == input.id())
        .unwrap_or_else(|| panic!("the row of {}", input.id()))
}

fn settled_input<'a>(publication: &'a PredecessorPublication, input: &Input) -> &'a SettledInput {
    publication
        .converged
        .iter()
        .find(|settled| &settled.input_id == input.id())
        .unwrap_or_else(|| panic!("{} is settled: {:?}", input.id(), publication.converged))
}

/// The interaction terminal published for `input`'s requester.
fn published_for<'a>(events: &'a [AgentEvent], input: &Input) -> &'a AgentEvent {
    events
        .iter()
        .find(|event| {
            crate::input_state::interaction_terminal_event_id(event)
                .is_some_and(|interaction_id| interaction_id.0 == input.id().0)
        })
        .unwrap_or_else(|| panic!("a terminal is published for {}: {events:?}", input.id()))
}

fn sorted_ids(inputs: &[&Input]) -> Vec<InputId> {
    let mut ids = inputs
        .iter()
        .map(|input| input.id().clone())
        .collect::<Vec<_>>();
    ids.sort_by_key(|input_id| input_id.0);
    ids
}

fn sorted(mut ids: Vec<InputId>) -> Vec<InputId> {
    ids.sort_by_key(|input_id| input_id.0);
    ids
}

/// What a whole settlement made durable, step by step.
struct WholeSettlement {
    tail: Option<PredecessorTailCommit>,
    work: PredecessorWork,
    publication: Result<PredecessorPublication, PredecessorPublicationRefused>,
}

/// One whole settlement as the session service drives it: begin, the tail
/// drive, the internal registration and its sweep, the second drive when
/// the first held, and finish with `publisher`.
async fn settle_whole(
    host: &Host,
    claim: &HostingClaim,
    publisher: &Arc<RecordingPublisher>,
) -> WholeSettlement {
    let mut lease = host
        .machine
        .begin_predecessor_settlement_before(claim, deadline())
        .await
        .expect("the settlement begins");
    let mut tail = host
        .machine
        .settle_predecessor_tail_with_lease(&mut lease)
        .await
        .expect("the first drive answers");
    let work = host
        .machine
        .end_predecessor_work_with_lease_before(&mut lease, deadline())
        .await
        .expect("the internal registration publishes and sweeps");
    if lease.tail_held()
        && let Some(second) = host
            .machine
            .settle_predecessor_tail_with_lease(&mut lease)
            .await
            .expect("the second drive commits the tail")
    {
        tail = Some(second);
    }
    let publication = host
        .machine
        .finish_predecessor_settlement_before(
            lease,
            Some(Arc::clone(publisher) as Arc<dyn CoreExecutorPublicationHandle>),
            deadline(),
        )
        .await;
    WholeSettlement {
        tail,
        work,
        publication,
    }
}

/// A run whose answer reached its tail before its boundary: the writer
/// drive commits the tail (retaining the unbound queued inputs), and its
/// commit stages the run's batch for the contributors, the directed one's
/// outbox placed by the row's binding. The sweep ends the queued inputs
/// `Stopped`, the directed one's outbox placed by the witness while the
/// internal registration is unplaced, and the task publishes both directed
/// terminals and finds every row closed. The session is left with nothing
/// registered, no provisional tail and its lifecycle row without the
/// witness; a repeat finds nothing to settle and publishes nothing.
#[tokio::test]
async fn a_settlement_commits_the_tail_ends_the_rest_and_publishes_every_terminal() {
    let host = Host::head_canonical();
    let (session_id, run_id) = durable_session(&host, Some(completed_tail)).await;
    host.lifecycle_row(&session_id, predecessor_placement(), None)
        .await;
    let d1 = directed_step("the run's directed contributor");
    let p1 = prompt("the run's prompt contributor");
    let q1 = prompt("queued beside the run");
    let d2 = directed_step("queued directed beside the run");
    host.persist(&session_id, applied_to(&d1, &run_id)).await;
    host.persist(&session_id, applied_to(&p1, &run_id)).await;
    host.persist(&session_id, queued(&q1, 1)).await;
    host.persist(&session_id, queued(&d2, 2)).await;
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    let tail = settled.tail.expect("the tail is committed");
    assert_eq!(tail.run_id, run_id);
    assert_eq!(
        tail.disposition,
        DurableTailRecoveryDisposition::CommitCompletedRetainInputs
    );
    assert_eq!(sorted(settled.work.ended), sorted_ids(&[&q1, &d2]));
    assert!(settled.work.answered.is_empty());
    assert_eq!(
        settled.work.owner_loss,
        PredecessorOwnerLoss::NoConsentOwner
    );
    let publication = settled
        .publication
        .expect("the census finds every row closed");
    assert!(publication.owed.is_empty());
    assert!(publication.migrated_without_receipt.is_empty());
    assert_eq!(publication.durable_rows_closed, 4);
    assert_eq!(publication.converged.len(), 4);
    for input in [&d1, &p1] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::TailConsumed);
        assert_eq!(settled.receipt.terminal(), &InputTerminalOutcome::Consumed);
        assert_eq!(settled.receipt.run_id(), Some(&run_id));
        assert!(matches!(
            settled.receipt.outcome(),
            CompletionOutcome::CompletedWithoutResult
        ));
    }
    for input in [&q1, &d2] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::Ended);
        assert_eq!(
            settled.receipt.terminal(),
            &InputTerminalOutcome::Abandoned {
                reason: InputAbandonReason::Stopped,
            }
        );
        assert_eq!(
            settled.receipt.scope(),
            &InputTerminalReceiptScope::RuntimeTermination
        );
        assert!(matches!(
            settled.receipt.outcome(),
            CompletionOutcome::RuntimeTerminated { reason, .. } if reason == "runtime stopped"
        ));
    }
    assert_eq!(
        settled_input(&publication, &p1).published_terminal_seq,
        None
    );
    assert_eq!(
        settled_input(&publication, &q1).published_terminal_seq,
        None
    );
    assert!(
        settled_input(&publication, &d1)
            .published_terminal_seq
            .is_some()
    );
    assert!(
        settled_input(&publication, &d2)
            .published_terminal_seq
            .is_some()
    );

    let events = publisher.events();
    assert_eq!(
        events.len(),
        2,
        "only the directed inputs' requesters are answered: {events:?}"
    );
    assert!(matches!(
        published_for(&events, &d1),
        AgentEvent::InteractionComplete { result, .. } if result.is_empty()
    ));
    assert!(matches!(
        published_for(&events, &d2),
        AgentEvent::InteractionFailed {
            reason: InteractionFailureReason::Abandoned { detail },
            ..
        } if detail == "runtime stopped"
    ));
    let rows = host.rows(&session_id).await;
    for input in [&d1, &d2] {
        let outbox = stored_row(&rows, input)
            .state
            .interaction_terminal_outbox
            .as_ref()
            .expect("the directed input's outbox");
        assert_eq!(
            outbox.owner_agent_runtime_id.as_deref(),
            Some(PLACED_RUNTIME)
        );
        assert_eq!(outbox.owner_fence_token, Some(PLACED_FENCE));
        assert_eq!(outbox.owner_runtime_generation, Some(PLACED_GENERATION));
    }
    assert!(!host.machine.contains_session(&session_id).await);
    assert!(!host.provisional_tail_stands(&session_id).await);
    assert_eq!(
        host.placement_witness(&session_id).await,
        None,
        "the sweep commit wrote the row back without the witness"
    );

    let repeat_publisher = Arc::new(RecordingPublisher::default());
    let repeat = settle_whole(&host, &claim, &repeat_publisher).await;
    assert!(repeat.tail.is_none(), "the tail stands no more");
    assert!(repeat.work.ended.is_empty());
    assert!(repeat.work.answered.is_empty());
    let repeated = repeat
        .publication
        .expect("the repeat finds every row closed");
    assert!(repeated.converged.is_empty());
    assert!(repeated.owed.is_empty());
    assert_eq!(repeated.durable_rows_closed, 4);
    assert!(repeat_publisher.events().is_empty());
    assert!(!host.machine.contains_session(&session_id).await);
}

/// A run cut after a tool round, with a queued input beside it that no run
/// bound: the writer drive holds the tail. The sweep ends the run's inputs
/// and the queued one `Stopped` in one commit, and the second drive then
/// commits the tail as interrupted with no contributor left to consume.
#[tokio::test]
async fn a_tail_held_beside_unbound_work_commits_once_the_sweep_ended_it() {
    let host = Host::head_canonical();
    let (session_id, run_id) = durable_session(&host, Some(interrupted_tail)).await;
    host.lifecycle_row(&session_id, predecessor_placement(), None)
        .await;
    let b1 = directed_step("bound to the cut run");
    let b2 = prompt("also bound to the cut run");
    let q1 = prompt("queued beside the cut run");
    host.persist(&session_id, applied_to(&b1, &run_id)).await;
    host.persist(&session_id, applied_to(&b2, &run_id)).await;
    host.persist(&session_id, queued(&q1, 1)).await;
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let mut lease = host
        .machine
        .begin_predecessor_settlement_before(&claim, deadline())
        .await
        .expect("the settlement begins");
    assert_eq!(
        host.machine
            .settle_predecessor_tail_with_lease(&mut lease)
            .await
            .expect("the first drive answers"),
        None,
        "the first drive holds the tail beside the unbound input"
    );
    assert!(lease.tail_held());
    let work = host
        .machine
        .end_predecessor_work_with_lease_before(&mut lease, deadline())
        .await
        .expect("the sweep ends every input");
    assert_eq!(sorted(work.ended), sorted_ids(&[&b1, &b2, &q1]));
    let tail = host
        .machine
        .settle_predecessor_tail_with_lease(&mut lease)
        .await
        .expect("the second drive answers")
        .expect("the second drive commits the tail");
    assert_eq!(tail.run_id, run_id);
    assert_eq!(
        tail.disposition,
        DurableTailRecoveryDisposition::RepairAndCommitInterrupted
    );
    assert!(!lease.tail_held());
    let publication = host
        .machine
        .finish_predecessor_settlement_before(
            lease,
            Some(Arc::clone(&publisher) as Arc<dyn CoreExecutorPublicationHandle>),
            deadline(),
        )
        .await
        .expect("the census finds every row closed");

    assert_eq!(publication.durable_rows_closed, 3);
    for input in [&b1, &b2, &q1] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::Ended);
        assert_eq!(
            settled.receipt.terminal(),
            &InputTerminalOutcome::Abandoned {
                reason: InputAbandonReason::Stopped,
            }
        );
        assert_eq!(settled.receipt.run_id(), None);
    }
    let events = publisher.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(
        published_for(&events, &b1),
        AgentEvent::InteractionFailed {
            reason: InteractionFailureReason::Abandoned { .. },
            ..
        }
    ));
    assert!(!host.provisional_tail_stands(&session_id).await);
    assert!(!host.machine.contains_session(&session_id).await);
}

/// A run cut after a tool round with nothing else left: the writer drive
/// commits it interrupted, and its contributors' batch answers them
/// `Cancelled` under the run's scope; the directed one's requester gets the
/// cancellation.
#[tokio::test]
async fn an_interrupted_tail_alone_answers_its_inputs_cancelled() {
    let host = Host::head_canonical();
    let (session_id, run_id) = durable_session(&host, Some(interrupted_tail)).await;
    host.lifecycle_row(&session_id, predecessor_placement(), None)
        .await;
    let i1 = directed_step("cut directed");
    let i2 = prompt("cut prompt");
    host.persist(&session_id, applied_to(&i1, &run_id)).await;
    host.persist(&session_id, applied_to(&i2, &run_id)).await;
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    let tail = settled.tail.expect("the tail is committed");
    assert_eq!(tail.run_id, run_id);
    assert_eq!(
        tail.disposition,
        DurableTailRecoveryDisposition::RepairAndCommitInterrupted
    );
    assert!(settled.work.ended.is_empty());
    let publication = settled
        .publication
        .expect("the census finds every row closed");
    for input in [&i1, &i2] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::TailConsumed);
        assert_eq!(settled.receipt.terminal(), &InputTerminalOutcome::Consumed);
        assert_eq!(settled.receipt.run_id(), Some(&run_id));
        assert!(matches!(
            settled.receipt.outcome(),
            CompletionOutcome::Cancelled
        ));
    }
    let events = publisher.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(
        published_for(&events, &i1),
        AgentEvent::InteractionFailed {
            reason: InteractionFailureReason::Cancelled,
            ..
        }
    ));
}

/// A boundary of run `left_run` committed before this release left two
/// inputs consumed with no receipt, its receipt durable at their sequence.
/// The settlement answers them in one runless batch: terminal `Consumed`,
/// scope runtime termination, outcome unknown naming `left_run`; the
/// directed one's requester gets the outcome-unknown failure, its outbox
/// placed by the row's binding. The rows keep their run and sequence. A
/// repeat answers nothing again and publishes nothing.
#[tokio::test]
async fn inputs_a_recovered_boundary_left_without_a_receipt_are_answered_outcome_unknown() {
    let host = Host::head_canonical();
    let (session_id, _) = durable_session(&host, None).await;
    host.lifecycle_row(&session_id, predecessor_placement(), None)
        .await;
    let left_run = RunId::new();
    let l1 = directed_step("consumed without a receipt");
    let l2 = prompt("consumed without a receipt too");
    host.store
        .commit_prepared_session_boundary(
            &LogicalRuntimeId::for_session(&session_id),
            crate::store::PreparedRuntimeSessionCommit::success(
                None,
                meerkat_core::lifecycle::RunBoundaryReceipt {
                    run_id: left_run.clone(),
                    boundary:
                        meerkat_core::lifecycle::run_primitive::RunApplyBoundary::RunCheckpoint,
                    contributing_input_ids: vec![l1.id().clone(), l2.id().clone()],
                    conversation_digest: None,
                    message_count: 1,
                    sequence: 1,
                    owner_contributions: Vec::new(),
                },
                vec![
                    record(row(
                        &l1,
                        InputLifecycleState::Consumed,
                        Some(left_run.clone()),
                        Some(1),
                    )),
                    record(row(
                        &l2,
                        InputLifecycleState::Consumed,
                        Some(left_run.clone()),
                        Some(1),
                    )),
                ],
                Some(session_id.clone()),
            ),
        )
        .await
        .expect("the run's boundary commits its receipt and its consumed rows");
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    assert!(settled.tail.is_none());
    assert!(settled.work.ended.is_empty());
    assert_eq!(sorted(settled.work.answered), sorted_ids(&[&l1, &l2]));
    let publication = settled
        .publication
        .expect("the census finds every row closed");
    assert_eq!(publication.durable_rows_closed, 2);
    for input in [&l1, &l2] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::OutcomeUnknown);
        assert_eq!(settled.receipt.terminal(), &InputTerminalOutcome::Consumed);
        assert_eq!(
            settled.receipt.scope(),
            &InputTerminalReceiptScope::RuntimeTermination
        );
        assert!(matches!(
            settled.receipt.outcome(),
            CompletionOutcome::OutcomeUnknown { run_id } if run_id == &left_run
        ));
    }
    assert!(
        settled_input(&publication, &l1)
            .published_terminal_seq
            .is_some()
    );
    assert_eq!(
        settled_input(&publication, &l2).published_terminal_seq,
        None
    );
    let events = publisher.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(
        published_for(&events, &l1),
        AgentEvent::InteractionFailed {
            reason: InteractionFailureReason::OutcomeUnknown,
            ..
        }
    ));
    let rows = host.rows(&session_id).await;
    for input in [&l1, &l2] {
        let stored = stored_row(&rows, input);
        assert_eq!(stored.seed.phase, InputLifecycleState::Consumed);
        assert_eq!(stored.seed.last_run_id.as_ref(), Some(&left_run));
        assert_eq!(stored.seed.last_boundary_sequence, Some(1));
    }
    let outbox = stored_row(&rows, &l1)
        .state
        .interaction_terminal_outbox
        .as_ref()
        .expect("the directed input's outbox");
    assert_eq!(
        outbox.owner_agent_runtime_id.as_deref(),
        Some(PLACED_RUNTIME)
    );

    let repeat_publisher = Arc::new(RecordingPublisher::default());
    let repeat = settle_whole(&host, &claim, &repeat_publisher).await;
    assert!(repeat.work.answered.is_empty());
    let repeated = repeat
        .publication
        .expect("the repeat finds every row closed");
    assert!(repeated.converged.is_empty());
    assert_eq!(repeated.durable_rows_closed, 2);
    assert!(
        repeat_publisher.events().is_empty(),
        "an answered input is never published again"
    );
}

/// Beside another run's batch that has not finished, the writer drive
/// stages nothing for the tail's contributors, so they are left consumed
/// without a receipt: the sweep answers them outcome unknown, naming the
/// tail's run, and the task finishes the other run's batch too.
#[tokio::test]
async fn a_tail_beside_another_runs_open_batch_answers_its_contributors_outcome_unknown() {
    let host = Host::head_canonical();
    let (session_id, run_id) = durable_session(&host, Some(completed_tail)).await;
    host.lifecycle_row(&session_id, predecessor_placement(), None)
        .await;
    let d1 = directed_step("the tail's directed contributor");
    let p1 = prompt("the tail's prompt contributor");
    host.persist(&session_id, applied_to(&d1, &run_id)).await;
    host.persist(&session_id, applied_to(&p1, &run_id)).await;
    let earlier_run = RunId::new();
    let c0 = prompt("an earlier run's contributor");
    let mut earlier = row(
        &c0,
        InputLifecycleState::Consumed,
        Some(earlier_run.clone()),
        Some(1),
    );
    earlier.state.terminal_completion = Some(
        crate::meerkat_machine::driver::build_input_terminal_completion_rows(
            crate::meerkat_machine::driver::InteractionTerminalBatchScope::Run(&earlier_run),
            std::slice::from_ref(c0.id()),
            crate::input_state::InteractionTerminalCandidate::CompletedWithoutResult,
            false,
        )
        .expect("the earlier run's batch")
        .remove(0),
    );
    host.persist(&session_id, earlier).await;
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    let tail = settled.tail.expect("the tail is committed");
    assert_eq!(tail.run_id, run_id);
    assert_eq!(sorted(settled.work.answered), sorted_ids(&[&d1, &p1]));
    let publication = settled
        .publication
        .expect("the census finds every row closed");
    assert_eq!(publication.durable_rows_closed, 3);
    let converged = settled_input(&publication, &c0);
    assert_eq!(converged.settled_by, SettledBy::PendingTerminalConverged);
    assert_eq!(converged.receipt.run_id(), Some(&earlier_run));
    for input in [&d1, &p1] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::OutcomeUnknown);
        assert!(matches!(
            settled.receipt.outcome(),
            CompletionOutcome::OutcomeUnknown { run_id: answered } if answered == &run_id
        ));
    }
    let events = publisher.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(
        published_for(&events, &d1),
        AgentEvent::InteractionFailed {
            reason: InteractionFailureReason::OutcomeUnknown,
            ..
        }
    ));
}

/// An earlier start's recovery committed the tail with its contributors'
/// batch and the process died before its drain: the settlement finds the
/// tail aligned, rehydrates the batch, and the task finalizes and publishes
/// it.
#[tokio::test]
async fn a_tail_an_earlier_start_committed_has_its_batch_converged() {
    let host = Host::head_canonical();
    let (session_id, run_id) = durable_session(&host, Some(completed_tail)).await;
    host.lifecycle_row(&session_id, predecessor_placement(), None)
        .await;
    let d1 = directed_step("committed by an earlier start");
    let p1 = prompt("committed by an earlier start too");
    host.persist(&session_id, applied_to(&d1, &run_id)).await;
    host.persist(&session_id, applied_to(&p1, &run_id)).await;
    match crate::recovery::recover_durable_tail(host.store.as_ref(), &session_id)
        .await
        .expect("the earlier start's recovery answers")
    {
        crate::recovery::DurableTailRecoveryOutcome::Committed { disposition, .. } => {
            assert_eq!(disposition, DurableTailRecoveryDisposition::CommitCompleted);
        }
        other => panic!("the earlier start commits the tail: {other:?}"),
    }
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    assert!(settled.tail.is_none(), "the tail is aligned already");
    let publication = settled
        .publication
        .expect("the census finds every row closed");
    assert_eq!(publication.durable_rows_closed, 2);
    for input in [&d1, &p1] {
        let settled = settled_input(&publication, input);
        assert_eq!(settled.settled_by, SettledBy::PendingTerminalConverged);
        assert_eq!(settled.receipt.run_id(), Some(&run_id));
    }
    let events = publisher.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(
        published_for(&events, &d1),
        AgentEvent::InteractionComplete { .. }
    ));
}

/// A settlement that stopped after its internal registration wrote the
/// lifecycle row with the placement witness (clean, unbound) and before its
/// sweep: the repeat's begin takes the placement from the witness, the
/// sweep places the directed input's outbox by it and writes the row back
/// without it.
#[tokio::test]
async fn a_repeat_takes_the_placement_from_the_witness_a_stopped_settlement_left() {
    let host = Host::head_canonical();
    let (session_id, _) = durable_session(&host, Some(completed_tail)).await;
    host.lifecycle_row(
        &session_id,
        MachineLifecycleBindingFacts::default(),
        Some(predecessor_placement()),
    )
    .await;
    let d2 = directed_step("queued directed");
    host.persist(&session_id, queued(&d2, 1)).await;
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    assert!(settled.tail.is_some());
    assert_eq!(settled.work.ended, vec![d2.id().clone()]);
    let publication = settled
        .publication
        .expect("the census finds every row closed");
    assert_eq!(
        settled_input(&publication, &d2).settled_by,
        SettledBy::Ended
    );
    let rows = host.rows(&session_id).await;
    let outbox = stored_row(&rows, &d2)
        .state
        .interaction_terminal_outbox
        .as_ref()
        .expect("the directed input's outbox");
    assert_eq!(
        outbox.owner_agent_runtime_id.as_deref(),
        Some(PLACED_RUNTIME)
    );
    assert_eq!(outbox.owner_fence_token, Some(PLACED_FENCE));
    assert_eq!(outbox.owner_runtime_generation, Some(PLACED_GENERATION));
    assert_eq!(outbox.owner_runtime_epoch_id.as_deref(), Some(PLACED_EPOCH));
    assert_eq!(publisher.events().len(), 1);
    assert_eq!(host.placement_witness(&session_id).await, None);
}

/// A terminal row a migration left without its receipt (the marker set) is
/// reported apart and never changed or published.
#[tokio::test]
async fn a_migrated_row_is_reported_apart_and_left_as_it_is() {
    let host = Host::head_canonical();
    let (session_id, run_id) = durable_session(&host, None).await;
    let m1 = prompt("terminal before receipts existed");
    let mut migrated = row(&m1, InputLifecycleState::Consumed, Some(run_id), Some(1));
    migrated.state.terminal_completion_unavailable = true;
    host.persist(&session_id, migrated).await;
    let claim = host.claim(&session_id);
    let publisher = Arc::new(RecordingPublisher::default());

    let settled = settle_whole(&host, &claim, &publisher).await;

    let publication = settled
        .publication
        .expect("the census finds every row closed");
    assert!(publication.converged.is_empty());
    assert_eq!(publication.migrated_without_receipt, vec![m1.id().clone()]);
    assert_eq!(publication.durable_rows_closed, 1);
    assert!(publisher.events().is_empty());
    let rows = host.rows(&session_id).await;
    let stored = stored_row(&rows, &m1);
    assert!(stored.state.terminal_completion_unavailable);
    assert!(stored.state.terminal_completion.is_none());
}
