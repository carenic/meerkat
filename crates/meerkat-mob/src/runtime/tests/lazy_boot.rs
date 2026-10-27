//! Boot-side rows of member residency (design section 13): the lazy start
//! (R20, R21, R25), reads that never wake (R26, R47), the verified pass
//! (R48, R48n), wiring with dormant members (R28, H-D), seats (R13, R64,
//! R94, R115) and rotation (R62). They run on the combined candidate: the
//! machine, the wake path and the boot path together.

use super::*;
use crate::machines::mob_machine as dsl;
use crate::runtime::dormant_seat::fresh_seat_session_id;
use crate::runtime::{DormantSeatOutcome, MemberRestorePolicy, MobBootSetBasis};

const LAZY_STEP: Duration = Duration::from_secs(60);

fn worker(index: usize) -> AgentIdentity {
    AgentIdentity::from(format!("lazy-{index:02}"))
}

fn turn_driven(identity: &AgentIdentity) -> SpawnMemberSpec {
    let mut spec = SpawnMemberSpec::new("worker", identity.clone());
    spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
    spec
}

fn phase_of(
    handle: &MobHandle,
    identity: &AgentIdentity,
) -> Option<dsl::MemberMaterializationPhase> {
    handle.member_materialization_for_test(identity)
}

/// A mob whose turn-driven workers were spawned on a mock durable session
/// service and crash-stopped, with the durable state a restart finds.
struct CrashedMob {
    events: Arc<dyn MobEventStore>,
    runtime_metadata: Arc<dyn MobRuntimeMetadataStore>,
    /// The restarted service: durable session state only.
    service: Arc<MockSessionService>,
    /// Each worker and its session, in spawn order.
    sessions: Vec<(AgentIdentity, SessionId)>,
}

impl CrashedMob {
    /// Spawn `members` workers (the first two wired when `wire`), then crash
    /// (or, when `stop`, stop first) and restart the session service.
    async fn new(label: &str, members: usize, wire: bool, stop: bool) -> Self {
        let definition = with_unique_mob_id(sample_definition(), label);
        let service = Arc::new(MockSessionService::new());
        let _adapter = service.enable_runtime_adapter();
        let storage = MobStorage::in_memory();
        let events = storage.events.clone();
        let runtime_metadata = storage.runtime_metadata.clone();
        let handle = MobBuilder::new(definition, storage)
            .with_session_service(service.clone())
            .create()
            .await
            .expect("create the mob");
        let mut sessions = Vec::with_capacity(members);
        for index in 0..members {
            let identity = worker(index);
            handle
                .spawn_spec(turn_driven(&identity))
                .await
                .expect("spawn a worker");
            let session = handle
                .resolve_bridge_session_id(&identity)
                .await
                .expect("a session-backed worker");
            sessions.push((identity, session));
        }
        if wire {
            handle
                .wire(worker(0), worker(1))
                .await
                .expect("wire the first two workers");
        }
        if stop {
            handle.stop().await.expect("stop before the restart");
        }
        let restarted = Arc::new(service.cold_restart_preserving_durable_state().await);
        let _ = restarted.enable_runtime_adapter();
        crash_stop_and_release_routes(handle).await;
        drop(service);
        Self {
            events,
            runtime_metadata,
            service: restarted,
            sessions,
        }
    }

    fn builder(&self) -> MobBuilder {
        MobBuilder::for_resume(MobStorage::with_events_and_runtime_metadata(
            self.events.clone(),
            self.runtime_metadata.clone(),
        ))
        .with_session_service(self.service.clone())
        .notify_orchestrator_on_resume(false)
    }

    async fn resume(&self, builder: MobBuilder) -> MobHandle {
        tokio::time::timeout(LAZY_STEP, Box::pin(builder.resume()))
            .await
            .expect("the resume answers")
            .expect("the resume succeeds")
    }

    /// The sessions the restarted service built an agent for, in order.
    async fn materialized(&self) -> Vec<SessionId> {
        self.service
            .recorded_create_requests()
            .await
            .into_iter()
            .map(|record| record.session_id)
            .collect()
    }

    fn session(&self, index: usize) -> SessionId {
        self.sessions[index].1.clone()
    }
}

/// R20: a crash-path start under the default policy materializes no member
/// that holds no work, reads every member Dormant, and its plan was based on
/// an observed store. One host delivery to one member then materializes
/// exactly that member, on its own session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r20_a_crash_path_start_materializes_no_member_and_one_delivery_wakes_one() {
    let mob = CrashedMob::new("r20-lazy-crash-start", 6, false, false).await;
    let handle = mob.resume(mob.builder()).await;
    assert!(
        mob.materialized().await.is_empty(),
        "the start built no member: {:?}",
        mob.materialized().await
    );
    assert_eq!(mob.service.active_session_count().await, 0);
    for (identity, _) in &mob.sessions {
        assert_eq!(
            phase_of(&handle, identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity}"
        );
    }
    assert_eq!(handle.boot_set_basis(), MobBootSetBasis::Observed);

    let target = worker(3);
    handle
        .member(&target)
        .await
        .expect("a member handle")
        .send("first touch", HandlingMode::Queue)
        .await
        .expect("the delivery wakes the member and is accepted");
    assert_eq!(mob.materialized().await, vec![mob.session(3)]);
    assert_eq!(
        phase_of(&handle, &target),
        Some(dsl::MemberMaterializationPhase::Live)
    );
    for (identity, _) in mob
        .sessions
        .iter()
        .filter(|(identity, _)| identity != &target)
    {
        assert_eq!(
            phase_of(&handle, identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity} stays dormant"
        );
    }
}

/// R21: the lift of a stopped mob plans its members exactly as a crash-path
/// start does: none that holds no work is materialized.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r21_the_lift_of_a_stopped_mob_materializes_no_member_without_work() {
    let mob = CrashedMob::new("r21-lazy-lift", 4, false, true).await;
    let handle = mob.resume(mob.builder()).await;
    assert_eq!(handle.status().await.unwrap(), MobState::Stopped);
    tokio::time::timeout(LAZY_STEP, handle.resume())
        .await
        .expect("the lift answers")
        .expect("the lift succeeds");
    assert_eq!(handle.status().await.unwrap(), MobState::Running);
    assert!(mob.materialized().await.is_empty());
    for (identity, _) in &mob.sessions {
        assert_eq!(
            phase_of(&handle, identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity}"
        );
    }
}

/// R25: the Eager policy restores every member at start, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r25_the_eager_policy_materializes_every_member() {
    let mob = CrashedMob::new("r25-eager-start", 4, false, false).await;
    let handle = mob
        .resume(
            mob.builder()
                .with_member_restore_policy(MemberRestorePolicy::Eager),
        )
        .await;
    let mut materialized = mob.materialized().await;
    materialized.sort();
    let mut expected = mob
        .sessions
        .iter()
        .map(|(_, session)| session.clone())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(materialized, expected);
    for (identity, _) in &mob.sessions {
        assert_eq!(
            phase_of(&handle, identity),
            Some(dsl::MemberMaterializationPhase::Live),
            "{identity}"
        );
    }
}

/// R26 and R47: reads of dormant members do no work. Member lists, member
/// status and roster reads leave every member Dormant and build nothing;
/// a dormant member's status still reports its peer connectivity and its
/// resolved capabilities, and lists it Active.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r26_r47_reads_leave_dormant_members_dormant_and_report_them_whole() {
    let mob = CrashedMob::new("r26-reads", 3, true, false).await;
    let handle = mob.resume(mob.builder()).await;
    for _ in 0..3 {
        let listed = handle.list_members().await;
        assert_eq!(listed.len(), mob.sessions.len());
        for entry in &listed {
            assert_eq!(format!("{:?}", entry.status), "Active", "{entry:?}");
        }
        for (identity, session) in &mob.sessions {
            let member = handle
                .get_member(identity)
                .await
                .expect("read the roster")
                .expect("a member");
            assert_eq!(member.bridge_session_id(), Some(session));
            let status = handle
                .member_status(identity)
                .await
                .expect("a dormant member's status");
            assert_eq!(status.current_session_id.as_ref(), Some(session));
            assert!(
                status.peer_connectivity.is_some(),
                "T1: peer connectivity comes from membership: {status:?}"
            );
            assert!(
                status.resolved_capabilities.is_some(),
                "T2: capabilities come from the durable session and profile: {status:?}"
            );
        }
    }
    assert!(mob.materialized().await.is_empty());
    for (identity, _) in &mob.sessions {
        assert_eq!(
            phase_of(&handle, identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity}"
        );
    }
}

/// R48: under the verified policy a dormant member whose durable session is
/// gone reads Broken at ready, with nothing materialized for it; every other
/// member stays Dormant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r48_the_verified_policy_records_an_unrecoverable_member_broken_without_waking_it() {
    let mob = CrashedMob::new("r48-verified", 3, false, false).await;
    let lost = worker(1);
    mob.service.delete_persisted_session(&mob.session(1)).await;
    let handle = mob
        .resume(
            mob.builder()
                .with_member_restore_policy(MemberRestorePolicy::OnDemandVerified),
        )
        .await;
    assert!(mob.materialized().await.is_empty());
    assert_eq!(phase_of(&handle, &lost), None, "Broken holds no phase");
    assert!(
        handle.restore_diagnostics.read().await.contains_key(&lost),
        "the verdict is recorded as the member's restore failure"
    );
    for identity in [worker(0), worker(2)] {
        assert_eq!(
            phase_of(&handle, &identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity}"
        );
    }
}

/// R48n: under the default policy the same member reads Dormant at ready,
/// and the start read nothing of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r48n_the_default_policy_leaves_an_unrecoverable_member_dormant_until_touched() {
    let mob = CrashedMob::new("r48n-on-demand", 3, false, false).await;
    let lost = worker(1);
    mob.service.delete_persisted_session(&mob.session(1)).await;
    let handle = mob.resume(mob.builder()).await;
    assert!(mob.materialized().await.is_empty());
    assert_eq!(
        phase_of(&handle, &lost),
        Some(dsl::MemberMaterializationPhase::Dormant)
    );
    assert!(!handle.restore_diagnostics.read().await.contains_key(&lost));
}

/// R28, R100 and H-D: wiring and unwiring two dormant members commits the
/// edge and its event without waking either, and a wired pair restarted
/// lazily keeps its edge with both members Dormant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r28_wiring_dormant_members_wakes_neither() {
    let mob = CrashedMob::new("r28-dormant-wiring", 4, true, false).await;
    let handle = mob.resume(mob.builder()).await;
    let edges = |handle: &MobHandle| handle.machine_state_watch_rx.borrow().wiring_edges.len();
    assert_eq!(edges(&handle), 1, "the wired pair keeps its edge");
    handle
        .wire(worker(2), worker(3))
        .await
        .expect("wire two dormant members");
    assert_eq!(edges(&handle), 2);
    handle
        .unwire(worker(0), worker(1))
        .await
        .expect("unwire two dormant members");
    assert_eq!(edges(&handle), 1);
    assert!(mob.materialized().await.is_empty());
    for (identity, _) in &mob.sessions {
        assert_eq!(
            phase_of(&handle, identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity}"
        );
    }
}

/// A running mob over the mock service, for seats and rotations.
async fn seat_mob(
    label: &str,
    builder: impl FnOnce(MobBuilder) -> MobBuilder,
) -> (MobHandle, Arc<MockSessionService>, Arc<dyn MobEventStore>) {
    let definition = with_unique_mob_id(sample_definition(), label);
    let service = Arc::new(MockSessionService::new());
    let _adapter = service.enable_runtime_adapter();
    let storage = MobStorage::in_memory();
    let events = storage.events.clone();
    let handle =
        builder(MobBuilder::new(definition, storage).with_session_service(service.clone()))
            .create()
            .await
            .expect("create the mob");
    (handle, service, events)
}

/// The kinds of the journal's events, in order.
async fn journal_kinds(events: &Arc<dyn MobEventStore>) -> Vec<String> {
    events
        .replay_all()
        .await
        .expect("read the journal")
        .iter()
        .map(|event| match &event.kind {
            MobEventKind::MemberSpawned(spawned) => {
                format!("MemberSpawned({})", spawned.agent_identity)
            }
            MobEventKind::MemberRetired { agent_identity, .. } => {
                format!("MemberRetired({agent_identity})")
            }
            other => format!("{:?}", std::mem::discriminant(other)),
        })
        .collect()
}

/// R115 (F8, R5): a seat batch in which one identity's peer id is already
/// held seats the others in one commit and refuses that identity alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r115_a_seat_batch_refuses_only_the_identity_whose_peer_id_is_held() {
    let keys = tempfile::tempdir().expect("a comms identity root");
    let (handle, service, events) = seat_mob("r115-seat-batch", |builder| {
        builder.with_member_comms_identity_root(keys.path())
    })
    .await;
    let shared = SessionId::new();
    service
        .replace_live_session(Session::with_id(shared.clone()))
        .await;
    let own = SessionId::new();
    service
        .replace_live_session(Session::with_id(own.clone()))
        .await;
    let resume = |identity: &AgentIdentity, session: &SessionId| {
        let mut spec = turn_driven(identity);
        spec.launch_mode = crate::launch::MemberLaunchMode::Resume {
            bridge_session_id: session.clone(),
            resume_from_role: None,
        };
        spec
    };
    let before = journal_kinds(&events).await.len();
    let outcomes = handle
        .seat_dormant_members(vec![
            resume(&worker(0), &shared),
            resume(&worker(1), &shared),
            resume(&worker(2), &own),
        ])
        .await
        .expect("the batch commits");
    assert!(
        matches!(&outcomes[0], DormantSeatOutcome::Seated { session_id, .. } if session_id == &shared),
        "{outcomes:?}"
    );
    assert!(
        matches!(&outcomes[1], DormantSeatOutcome::Refused { .. }),
        "the second identity on the same key is refused: {outcomes:?}"
    );
    assert!(
        matches!(&outcomes[2], DormantSeatOutcome::Seated { session_id, .. } if session_id == &own),
        "{outcomes:?}"
    );
    assert_eq!(
        journal_kinds(&events).await.split_off(before),
        vec![
            format!("MemberSpawned({})", worker(0)),
            format!("MemberSpawned({})", worker(2)),
        ]
    );
    for identity in [worker(0), worker(2)] {
        assert_eq!(
            phase_of(&handle, &identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity}"
        );
    }
    assert_eq!(phase_of(&handle, &worker(1)), None);
    assert!(
        service.recorded_create_requests().await.is_empty(),
        "a resume seat builds nothing"
    );

    // A re-sent batch answers the seated identities AlreadySeated and writes
    // nothing.
    let journal = journal_kinds(&events).await.len();
    let again = handle
        .seat_dormant_members(vec![resume(&worker(0), &shared)])
        .await
        .expect("the re-sent batch");
    assert!(
        matches!(&again[0], DormantSeatOutcome::AlreadySeated { session_id, .. } if session_id == &shared),
        "{again:?}"
    );
    assert_eq!(journal_kinds(&events).await.len(), journal);
}

/// A seat needs the members' comms transport: without it every spec is
/// refused typed and nothing is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seat_without_a_comms_identity_source_is_refused_and_writes_nothing() {
    let (handle, service, events) = seat_mob("seat-no-identity-source", |builder| builder).await;
    let session = SessionId::new();
    service
        .replace_live_session(Session::with_id(session.clone()))
        .await;
    let mut spec = turn_driven(&worker(0));
    spec.launch_mode = crate::launch::MemberLaunchMode::Resume {
        bridge_session_id: session,
        resume_from_role: None,
    };
    let before = journal_kinds(&events).await;
    let outcomes = handle
        .seat_dormant_members(vec![spec])
        .await
        .expect("the batch answers");
    assert!(
        matches!(
            &outcomes[0],
            DormantSeatOutcome::Refused {
                error: MobError::WiringError(_),
                ..
            }
        ),
        "{outcomes:?}"
    );
    assert_eq!(journal_kinds(&events).await, before);
}

/// R6: an archived session is never bound: its seat is answered typed and
/// writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seat_on_an_archived_session_is_answered_typed() {
    let keys = tempfile::tempdir().expect("a comms identity root");
    let (handle, service, events) = seat_mob("seat-archived", |builder| {
        builder.with_member_comms_identity_root(keys.path())
    })
    .await;
    let session = SessionId::new();
    service
        .replace_live_session(Session::with_id(session.clone()))
        .await;
    SessionService::archive(service.as_ref(), &session)
        .await
        .expect("archive the session");
    let mut spec = turn_driven(&worker(0));
    spec.launch_mode = crate::launch::MemberLaunchMode::Resume {
        bridge_session_id: session.clone(),
        resume_from_role: None,
    };
    let before = journal_kinds(&events).await;
    let outcomes = handle
        .seat_dormant_members(vec![spec])
        .await
        .expect("the batch answers");
    assert!(
        matches!(&outcomes[0], DormantSeatOutcome::SessionArchived { session_id, .. } if session_id == &session),
        "{outcomes:?}"
    );
    assert_eq!(journal_kinds(&events).await, before);
}

/// R13: a fresh spec that owes a kickoff is not seated: it
/// is spawned, live, on the seat's name-based session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r13_a_fresh_autonomous_spec_is_embodied_on_its_name_based_session() {
    let (handle, _service, _events) =
        seat_mob("r13-embodied", MobBuilder::with_network_member_comms).await;
    let identity = worker(0);
    let mut spec = SpawnMemberSpec::new("worker", identity.clone());
    spec.runtime_mode = Some(crate::MobRuntimeMode::AutonomousHost);
    let outcomes = handle
        .seat_dormant_members(vec![spec])
        .await
        .expect("the batch answers");
    let expected = fresh_seat_session_id(
        handle.mob_id(),
        handle.incarnation(),
        &identity,
        crate::ids::Generation::INITIAL,
    );
    assert!(
        matches!(&outcomes[0], DormantSeatOutcome::Embodied { session_id, .. } if session_id == &expected),
        "{outcomes:?}"
    );
    assert_eq!(
        phase_of(&handle, &identity),
        Some(dsl::MemberMaterializationPhase::Live)
    );
}

/// R64: a fresh turn-driven spec with no initial message is seated, its
/// empty session created once through the fresh-spawn create path with no
/// runtime registration, under its name-based id. On network comms it is
/// seated owed its endpoint, which its boot wake registers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r64_a_fresh_seat_creates_its_empty_session_once_under_its_name_based_id() {
    let (handle, service, events) =
        seat_mob("r64-fresh-seat", MobBuilder::with_network_member_comms).await;
    let identity = worker(0);
    let before = journal_kinds(&events).await.len();
    let outcomes = handle
        .seat_dormant_members(vec![turn_driven(&identity)])
        .await
        .expect("the batch answers");
    let expected = fresh_seat_session_id(
        handle.mob_id(),
        handle.incarnation(),
        &identity,
        crate::ids::Generation::INITIAL,
    );
    assert!(
        matches!(&outcomes[0], DormantSeatOutcome::Seated { session_id, .. } if session_id == &expected),
        "{outcomes:?}"
    );
    let creates = service.recorded_create_requests().await;
    let seat_create = creates
        .iter()
        .find(|record| record.session_id == expected)
        .expect("the seat created its session");
    assert!(
        !seat_create.runtime_build_mode_session_owned,
        "the seat's create registers no runtime"
    );
    assert_eq!(
        journal_kinds(&events).await.split_off(before),
        vec![format!("MemberSpawned({identity})")]
    );
}

/// R94 (G12): a seat whose journal append failed after its empty session was
/// created is retried onto the same session, created once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r94_a_seat_retried_after_a_failed_journal_reuses_its_fresh_session() {
    let events = Arc::new(FaultInjectedMobEventStore::new());
    let storage = MobStorage::with_events_and_runtime_metadata(
        events.clone(),
        Arc::new(InMemoryMobRuntimeMetadataStore::new()),
    );
    let service = Arc::new(MockSessionService::new());
    let _adapter = service.enable_runtime_adapter();
    let handle = MobBuilder::new(
        with_unique_mob_id(sample_definition(), "r94-seat-retry"),
        storage,
    )
    .with_session_service(service.clone())
    .with_network_member_comms()
    .create()
    .await
    .expect("create the mob");
    let identity = worker(0);
    events.fail_appends_for("MemberSpawned").await;
    let failed = handle
        .seat_dormant_members(vec![turn_driven(&identity)])
        .await;
    assert!(
        failed.is_err(),
        "a store failure fails the batch: {failed:?}"
    );
    assert_eq!(phase_of(&handle, &identity), None, "nothing was seated");
    events.allow_appends_for("MemberSpawned").await;
    let outcomes = handle
        .seat_dormant_members(vec![turn_driven(&identity)])
        .await
        .expect("the retried batch commits");
    let expected = fresh_seat_session_id(
        handle.mob_id(),
        handle.incarnation(),
        &identity,
        crate::ids::Generation::INITIAL,
    );
    assert!(
        matches!(&outcomes[0], DormantSeatOutcome::Seated { session_id, .. } if session_id == &expected),
        "{outcomes:?}"
    );
    let created = service
        .recorded_create_requests()
        .await
        .into_iter()
        .filter(|record| record.session_id == expected && !record.runtime_build_mode_session_owned)
        .count();
    assert_eq!(
        created, 1,
        "the retry found the session its first attempt created"
    );
}

/// R62 (F6): a rotation retires the member's incarnation and seats the next
/// one on a new name-based session in one journal batch: MemberRetired is
/// followed at once by the next MemberSpawned, and the old session is
/// archived.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r62_a_rotation_reseats_the_member_on_a_new_session_in_one_batch() {
    let (handle, service, events) =
        seat_mob("r62-rotation", MobBuilder::with_network_member_comms).await;
    let identity = worker(0);
    handle
        .spawn_spec(turn_driven(&identity))
        .await
        .expect("spawn the member");
    let old = handle
        .resolve_bridge_session_id(&identity)
        .await
        .expect("a session-backed member");
    let new = tokio::time::timeout(LAZY_STEP, handle.rotate_member(turn_driven(&identity)))
        .await
        .expect("the rotation answers")
        .expect("the rotation succeeds");
    assert_eq!(
        new,
        fresh_seat_session_id(
            handle.mob_id(),
            handle.incarnation(),
            &identity,
            crate::ids::Generation::new(1),
        )
    );
    assert_ne!(new, old);
    assert_eq!(handle.resolve_bridge_session_id(&identity).await, Some(new));
    assert!(service.test_is_session_archived(&old).await);
    let kinds = journal_kinds(&events).await;
    let retired = kinds
        .iter()
        .position(|kind| kind == &format!("MemberRetired({identity})"))
        .expect("the retirement is journaled");
    assert_eq!(
        kinds.get(retired + 1),
        Some(&format!("MemberSpawned({identity})")),
        "the re-seat is journaled with the retirement: {kinds:?}"
    );
    let member = handle
        .get_member(&identity)
        .await
        .expect("read the roster")
        .expect("still a member");
    assert_eq!(member.generation, crate::ids::Generation::new(1));
}

/// R96 (R15): 10,000 dormant members restart with zero materializations and
/// every member Dormant, within the coarse bound. Deterministic counters are
/// the assertion; the wall clock is a smoke bound. Run explicitly, in the
/// release profile.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "scale row R96: run explicitly in the release profile"]
async fn r96_ten_thousand_dormant_members_restart_without_materializing_any() {
    const MEMBERS: usize = 10_000;
    let keys = tempfile::tempdir().expect("a comms identity root");
    let definition = with_unique_mob_id(sample_definition(), "r96-scale");
    let service = Arc::new(MockSessionService::new());
    let _adapter = service.enable_runtime_adapter();
    let storage = MobStorage::in_memory();
    let events = storage.events.clone();
    let runtime_metadata = storage.runtime_metadata.clone();
    let handle = MobBuilder::new(definition, storage)
        .with_session_service(service.clone())
        .with_member_comms_identity_root(keys.path())
        .create()
        .await
        .expect("create the mob");
    let mut specs = Vec::with_capacity(MEMBERS);
    for index in 0..MEMBERS {
        let session = SessionId::new();
        service
            .replace_live_session(Session::with_id(session.clone()))
            .await;
        let mut spec = turn_driven(&AgentIdentity::from(format!("scale-{index:05}")));
        spec.launch_mode = crate::launch::MemberLaunchMode::Resume {
            bridge_session_id: session,
            resume_from_role: None,
        };
        specs.push(spec);
    }
    let outcomes = handle
        .seat_dormant_members(specs)
        .await
        .expect("seat every member");
    assert!(
        outcomes
            .iter()
            .all(|outcome| matches!(outcome, DormantSeatOutcome::Seated { .. })),
        "every member is seated"
    );
    let restarted = Arc::new(service.cold_restart_preserving_durable_state().await);
    let _ = restarted.enable_runtime_adapter();
    crash_stop_and_release_routes(handle).await;
    drop(service);
    let started = std::time::Instant::now();
    let resumed = MobBuilder::for_resume(MobStorage::with_events_and_runtime_metadata(
        events,
        runtime_metadata,
    ))
    .with_session_service(restarted.clone())
    .with_member_comms_identity_root(keys.path())
    .notify_orchestrator_on_resume(false)
    .resume()
    .await
    .expect("restart the mob");
    let elapsed = started.elapsed();
    assert!(restarted.recorded_create_requests().await.is_empty());
    assert_eq!(restarted.active_session_count().await, 0);
    let state = resumed.machine_state_watch_rx.borrow().clone();
    assert_eq!(
        state
            .member_materialization
            .values()
            .filter(|phase| matches!(phase, dsl::MemberMaterializationPhase::Dormant))
            .count(),
        MEMBERS
    );
    assert!(
        elapsed < Duration::from_secs(60),
        "the restart took {elapsed:?}"
    );
}

/// R114 (F6): a crash after a rotation's retirement durably started and
/// before its terminal batch. The restart finds the incarnation Retiring
/// with no rotation intent; the host passes the identity as host-pending (so
/// the plain retirement cannot complete it) and calls `rotate_member` again,
/// which records the rotation on that retirement and seats the next
/// incarnation on the same name-based session the interrupted rotation
/// targeted. The journal never holds the retirement without the re-seat.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r114_a_rotation_interrupted_after_its_retirement_started_completes_on_redrive() {
    let definition = with_unique_mob_id(sample_definition(), "r114-rotation-crash");
    let service = Arc::new(MockSessionService::new());
    let _adapter = service.enable_runtime_adapter();
    let storage = MobStorage::in_memory();
    let events = storage.events.clone();
    let runtime_metadata = storage.runtime_metadata.clone();
    let handle = MobBuilder::new(definition, storage)
        .with_session_service(service.clone())
        .with_network_member_comms()
        .create()
        .await
        .expect("create the mob");
    let identity = worker(0);
    handle
        .spawn_spec(turn_driven(&identity))
        .await
        .expect("spawn the member");
    let target = fresh_seat_session_id(
        handle.mob_id(),
        handle.incarnation(),
        &identity,
        crate::ids::Generation::new(1),
    );
    let hold =
        crate::runtime::actor::retirement_stage_hold::install(&identity, "retire-runtime-quiesce");
    let rotating = {
        let handle = handle.clone();
        let spec = turn_driven(&identity);
        tokio::spawn(async move { handle.rotate_member(spec).await })
    };
    tokio::time::timeout(LAZY_STEP, hold.entered())
        .await
        .expect("the rotation's retirement starts");
    let restarted = Arc::new(service.cold_restart_preserving_durable_state().await);
    let _ = restarted.enable_runtime_adapter();
    crash_stop_and_release_routes(handle).await;
    hold.release();
    let _ = tokio::time::timeout(LAZY_STEP, rotating).await;
    drop(service);

    let storage = MobStorage::with_events_and_runtime_metadata(events.clone(), runtime_metadata);
    let resumed = tokio::time::timeout(
        LAZY_STEP,
        Box::pin(
            MobBuilder::for_resume(storage)
                .with_session_service(restarted.clone())
                .with_network_member_comms()
                .with_host_pending_members([identity.clone()])
                .notify_orchestrator_on_resume(false)
                .resume(),
        ),
    )
    .await
    .expect("the restart answers")
    .expect("the restart succeeds");
    // The restart's own re-drive of the retirement cannot complete it while
    // the identity is host-pending; the host's re-issued rotation does.
    let deadline = tokio::time::Instant::now() + LAZY_STEP;
    let session = loop {
        match resumed.rotate_member(turn_driven(&identity)).await {
            Ok(session) => break session,
            Err(MobError::LifecycleOperationPending { .. })
                if tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => panic!("the re-issued rotation fails: {error}"),
        }
    };
    assert_eq!(
        session, target,
        "the re-drive targets the interrupted rotation's session"
    );
    assert_eq!(
        resumed.resolve_bridge_session_id(&identity).await,
        Some(target)
    );
    let kinds = journal_kinds(&events).await;
    let retirements = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| *kind == &format!("MemberRetired({identity})"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(retirements.len(), 1, "{kinds:?}");
    assert_eq!(
        kinds.get(retirements[0] + 1),
        Some(&format!("MemberSpawned({identity})")),
        "{kinds:?}"
    );
}

/// X6: a member whose comms runtime is missing at start fails alone; the mob
/// starts Running and every other member is live.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn x6_a_member_whose_comms_runtime_is_missing_at_start_fails_alone() {
    let mob = CrashedMob::new("x6-missing-comms", 3, false, false).await;
    mob.service.set_missing_comms_runtime(&mob.session(1)).await;
    let handle = mob
        .resume(
            mob.builder()
                .with_member_restore_policy(MemberRestorePolicy::Eager),
        )
        .await;
    assert_eq!(handle.status().await.unwrap(), MobState::Running);
    for identity in [worker(0), worker(2)] {
        assert_eq!(
            phase_of(&handle, &identity),
            Some(dsl::MemberMaterializationPhase::Live),
            "{identity}"
        );
    }
}
