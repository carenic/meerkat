//! Rows of a seat that built nothing, on the wake side: a session an earlier
//! wake created before a crash lost its born row, or one a wake that ended
//! retryable left, is adopted at the next wake or at an eager start and
//! resolved born once, never created again; a reload of a member whose
//! session was never created is refused, typed. They run on the combined
//! candidate: the machine, the wake path and the boot path together.

use super::*;
use crate::machines::mob_machine as dsl;
use crate::runtime::{DormantSeatOutcome, MemberRestorePolicy};

const STEP: Duration = Duration::from_secs(60);

/// A mob over stored member keys with one fresh turn-driven member seated
/// Dormant: its seat built nothing.
struct SeatedMob {
    keys: tempfile::TempDir,
    handle: MobHandle,
    events: Arc<dyn MobEventStore>,
    runtime_metadata: Arc<dyn MobRuntimeMetadataStore>,
    service: Arc<MockSessionService>,
    identity: AgentIdentity,
    /// The session the seat named.
    session: SessionId,
}

impl SeatedMob {
    async fn new(label: &str) -> Self {
        Self::with_storage(label, MobStorage::in_memory()).await
    }

    async fn with_storage(label: &str, storage: MobStorage) -> Self {
        let keys = tempfile::tempdir().expect("a comms identity root");
        let definition = with_unique_mob_id(sample_definition(), label);
        let service = Arc::new(MockSessionService::new());
        let _adapter = service.enable_runtime_adapter();
        let events = storage.events.clone();
        let runtime_metadata = storage.runtime_metadata.clone();
        let handle = MobBuilder::new(definition, storage)
            .with_session_service(service.clone())
            .with_member_comms_identity_root(keys.path())
            .create()
            .await
            .expect("create the mob");
        let identity = AgentIdentity::from("seated-worker");
        let mut spec = SpawnMemberSpec::new("worker", identity.clone());
        spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
        let outcomes = handle
            .seat_dormant_members(vec![spec])
            .await
            .expect("the seat batch answers");
        let session = match &outcomes[0] {
            DormantSeatOutcome::Seated { session_id, .. } => session_id.clone(),
            other => panic!("the fresh spec is seated: {other:?}"),
        };
        Self {
            keys,
            handle,
            events,
            runtime_metadata,
            service,
            identity,
            session,
        }
    }

    fn unborn(&self) -> bool {
        crate::runtime::boot_plan::domain_member_seat_session_unborn(
            &self.handle.machine_state_watch_rx.borrow(),
            &self.identity,
        )
    }

    fn phase(&self) -> Option<dsl::MemberMaterializationPhase> {
        self.handle.member_materialization_for_test(&self.identity)
    }

    /// Wait until the member's phase is `expected`, within [`STEP`].
    async fn wait_phase(&self, expected: dsl::MemberMaterializationPhase) {
        let deadline = tokio::time::Instant::now() + STEP;
        while self.phase() != Some(expected) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the member never reached {expected:?}: {:?}",
                self.phase()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// What an earlier wake's create leaves when the process crashes before
    /// its born row: the seat's session exists, and no row names it born.
    async fn create_the_session_without_its_born_row(&self) {
        self.service
            .replace_live_session(Session::with_id(self.session.clone()))
            .await;
    }

    /// Crash the mob, restart its session service and resume it under
    /// `policy`.
    async fn restart(&mut self, policy: MemberRestorePolicy) {
        crash_stop_and_release_routes(self.handle.clone()).await;
        let restarted = Arc::new(self.service.cold_restart_preserving_durable_state().await);
        let _ = restarted.enable_runtime_adapter();
        self.service = restarted;
        self.handle = tokio::time::timeout(
            STEP,
            Box::pin(
                MobBuilder::for_resume(MobStorage::with_events_and_runtime_metadata(
                    self.events.clone(),
                    self.runtime_metadata.clone(),
                ))
                .with_session_service(self.service.clone())
                .with_member_comms_identity_root(self.keys.path())
                .with_member_restore_policy(policy)
                .notify_orchestrator_on_resume(false)
                .resume(),
            ),
        )
        .await
        .expect("the restart answers")
        .expect("the restart succeeds");
    }

    /// The born rows the journal holds for the member's seat session.
    async fn born_rows(&self) -> usize {
        self.events
            .replay_all()
            .await
            .expect("read the journal")
            .iter()
            .filter(|event| {
                matches!(
                    &event.kind,
                    MobEventKind::MemberSessionBindingRecovered(recovered)
                        if recovered.agent_identity == self.identity
                            && recovered.records_seat_session_born()
                )
            })
            .count()
    }

    /// The creates of the seat's session the current service recorded.
    async fn creates_of_the_session(&self) -> usize {
        self.service
            .recorded_create_requests()
            .await
            .iter()
            .filter(|record| record.session_id == self.session)
            .count()
    }

    /// After the adoption: the member is live on the seat's session, resumed
    /// rather than created again, its session born, and one born row
    /// journaled.
    async fn assert_adopted(&self) {
        assert_eq!(
            self.phase(),
            Some(dsl::MemberMaterializationPhase::Live),
            "the member is live"
        );
        assert_eq!(
            self.handle.resolve_bridge_session_id(&self.identity).await,
            Some(self.session.clone()),
            "the member serves the session its seat named"
        );
        assert_eq!(
            self.creates_of_the_session().await,
            0,
            "a session that exists is resumed, never created again"
        );
        assert!(!self.unborn(), "the adopted session is born");
        assert_eq!(self.born_rows().await, 1, "one born row is journaled");
    }
}

/// A first wake that created the seat's session and crashed before its born
/// row leaves the session unborn after the restart, though it exists. The
/// next wake adopts it: it resumes the session and resolves it born, through
/// the creation's own transition, never as a plain resume.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seat_session_created_before_its_born_row_is_adopted_at_the_next_wake() {
    let mut mob = SeatedMob::new("unborn-adopt-wake").await;
    assert!(mob.unborn(), "the seat built nothing");
    mob.create_the_session_without_its_born_row().await;
    mob.restart(MemberRestorePolicy::OnDemand).await;
    assert!(
        mob.unborn(),
        "without a born row the restart holds the session unborn"
    );
    assert_eq!(
        mob.phase(),
        Some(dsl::MemberMaterializationPhase::Dormant),
        "the restart materialized nothing"
    );

    mob.handle
        .ensure_member_live(&mob.identity)
        .await
        .expect("the wake adopts the created session");

    mob.assert_adopted().await;
}

/// An eager start adopts such a session the same way, in the boot restore.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_eager_start_adopts_a_seat_session_created_before_its_born_row() {
    let mut mob = SeatedMob::new("unborn-adopt-eager").await;
    mob.create_the_session_without_its_born_row().await;
    mob.restart(MemberRestorePolicy::Eager).await;

    mob.assert_adopted().await;
}

/// An eager start of a seat whose session was never created creates it
/// there, under the id the seat named, and resolves it born.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_eager_start_creates_a_never_created_seat_session_under_its_seat_id() {
    let mut mob = SeatedMob::new("unborn-eager-create").await;
    mob.restart(MemberRestorePolicy::Eager).await;

    assert_eq!(
        mob.phase(),
        Some(dsl::MemberMaterializationPhase::Live),
        "the eager start materialized the member"
    );
    assert_eq!(
        mob.creates_of_the_session().await,
        1,
        "the start created the seat's session once, under the seat's id"
    );
    assert!(!mob.unborn(), "the created session is born");
    assert_eq!(mob.born_rows().await, 1, "one born row is journaled");
}

/// A first wake that created the seat's session and then ended retryable
/// (its born row could not be journaled, so its turn was refused) keeps the
/// session rather than archiving it: only its live materialization is
/// released, and the member is Dormant with its session unborn. The next
/// wake adopts that session, as it adopts one a crash left created, so the
/// session is created exactly once across both wakes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_wake_that_ends_retryable_after_the_create_leaves_the_session_to_the_next_wake() {
    let faults = Arc::new(FaultInjectedMobEventStore::new());
    let storage = MobStorage::with_events_and_runtime_metadata(
        faults.clone(),
        Arc::new(InMemoryMobRuntimeMetadataStore::new()),
    );
    let mob = SeatedMob::with_storage("unborn-retryable", storage).await;
    faults
        .fail_appends_for("MemberSessionBindingRecovered")
        .await;

    let refused = mob
        .handle
        .ensure_member_live(&mob.identity)
        .await
        .expect_err("the first wake ends retryable");
    assert!(
        matches!(
            &refused,
            MobError::MemberWakeRefused { refusal, .. } if refusal.is_retryable()
        ),
        "{refused:?}"
    );
    mob.wait_phase(dsl::MemberMaterializationPhase::Dormant)
        .await;
    assert_eq!(
        mob.creates_of_the_session().await,
        1,
        "the first wake created the seat's session"
    );
    assert!(mob.unborn(), "no born row was journaled: still unborn");
    assert_eq!(mob.born_rows().await, 0);
    assert!(
        !mob.service.has_live_session(&mob.session).await.unwrap(),
        "the session's live materialization was released"
    );
    assert!(
        !mob.service.test_is_session_archived(&mob.session).await,
        "the session was kept, not archived"
    );

    faults
        .allow_appends_for("MemberSessionBindingRecovered")
        .await;
    mob.handle
        .ensure_member_live(&mob.identity)
        .await
        .expect("the next wake adopts the session");
    assert_eq!(
        mob.phase(),
        Some(dsl::MemberMaterializationPhase::Live),
        "the member is live"
    );
    assert_eq!(
        mob.creates_of_the_session().await,
        1,
        "the session is created once across both wakes"
    );
    assert!(!mob.unborn(), "the adopted session is born");
    assert_eq!(mob.born_rows().await, 1, "one born row is journaled");
}

/// A reload of a member whose seat built nothing is refused, typed: it has
/// no session to reload until its first wake creates it. The refusal
/// changes nothing; the member stays Dormant with its session unborn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reload_of_a_member_whose_seat_built_nothing_is_refused_typed() {
    let mob = SeatedMob::new("unborn-reload").await;

    let refused = mob
        .handle
        .reload_member_registration(&mob.identity)
        .await
        .expect_err("there is nothing to reload");

    assert!(
        matches!(
            &refused,
            MobError::UnsupportedForMode { reason, .. }
                if reason.contains("created at its first wake")
        ),
        "{refused:?}"
    );
    assert!(mob.unborn(), "the session stays unborn");
    assert_eq!(mob.creates_of_the_session().await, 0, "nothing was created");
    assert_eq!(
        mob.phase(),
        Some(dsl::MemberMaterializationPhase::Dormant),
        "nothing was materialized"
    );
}
