//! Machine rows for the cold-predecessor settlement: the claim, profile and
//! registration refusals, the row scan, the placement witness, the marker on
//! the internal registration, its exact removal and the process-owned task.
//! The service's rows (the tail drives, the sweep's terminals, publication
//! and the census of every row) are the session crate's.

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
