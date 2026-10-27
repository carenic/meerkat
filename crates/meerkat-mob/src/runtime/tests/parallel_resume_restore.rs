//! A cold resume restores its members up to its restore concurrency at a
//! time, and the mob it leaves is the one a restore of one member at a time
//! leaves.
//!
//! Most fixtures spawn their members on a persistent session service, shut
//! the mob down and resume it on the same service and stores. The agent
//! builder holds the build of each restored member (its provision) until the
//! test releases it, so the store round trips of a restore are counted in
//! release rounds: a resume of N members at concurrency C takes ceil(N / C)
//! rounds. The restore pass probe (`builder::restore_pass_probe`) records
//! when each member's restore and turn start and finish, and holds or fails
//! a member's turn.

use super::*;
use crate::runtime::builder::restore_pass_probe::{self, RestorePassEvent};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bound for one step; it only turns a hang into a failure.
const STEP: Duration = Duration::from_secs(60);

const MEMBERS: usize = 8;

/// The member whose restore fails in the failure rows.
const FAILING: usize = 3;

/// Holds the agent build of each armed session (a restore's provision) until
/// the test releases that session.
struct RestoreGate {
    /// Sessions whose next build waits at the gate.
    armed: std::sync::Mutex<HashSet<SessionId>>,
    /// Each armed session's release.
    releases: std::sync::Mutex<HashMap<SessionId, Arc<tokio::sync::Semaphore>>>,
    /// The sessions whose build arrived since the last arming, in order.
    arrived: std::sync::Mutex<Vec<SessionId>>,
    arrival: tokio::sync::Notify,
    /// Builds waiting at the gate now.
    held: AtomicUsize,
    /// The most builds that waited at the gate at once since the last arming.
    max_held: AtomicUsize,
}

impl RestoreGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            armed: std::sync::Mutex::new(HashSet::new()),
            releases: std::sync::Mutex::new(HashMap::new()),
            arrived: std::sync::Mutex::new(Vec::new()),
            arrival: tokio::sync::Notify::new(),
            held: AtomicUsize::new(0),
            max_held: AtomicUsize::new(0),
        })
    }

    /// Hold the next build of each of `sessions`.
    fn arm(&self, sessions: impl IntoIterator<Item = SessionId>) {
        assert_eq!(
            self.held.load(Ordering::SeqCst),
            0,
            "no build waits at the gate"
        );
        self.arrived.lock().expect("arrived builds").clear();
        self.max_held.store(0, Ordering::SeqCst);
        let mut armed = self.armed.lock().expect("armed sessions");
        let mut releases = self.releases.lock().expect("build releases");
        for session in sessions {
            armed.insert(session.clone());
            releases.insert(session, Arc::new(tokio::sync::Semaphore::new(0)));
        }
    }

    async fn hold_if_armed(&self, session_id: &SessionId) {
        if !self
            .armed
            .lock()
            .expect("armed sessions")
            .remove(session_id)
        {
            return;
        }
        let release = self
            .releases
            .lock()
            .expect("build releases")
            .get(session_id)
            .cloned();
        let held = self.held.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_held.fetch_max(held, Ordering::SeqCst);
        self.arrived
            .lock()
            .expect("arrived builds")
            .push(session_id.clone());
        self.arrival.notify_one();
        if let Some(release) = release {
            let _released = release.acquire().await;
        }
        self.held.fetch_sub(1, Ordering::SeqCst);
    }

    /// Let `session`'s held build go on.
    fn release(&self, session: &SessionId) {
        if let Some(release) = self.releases.lock().expect("build releases").get(session) {
            release.add_permits(1);
        }
    }

    fn arrived(&self) -> Vec<SessionId> {
        self.arrived.lock().expect("arrived builds").clone()
    }

    /// Wait until `count` builds arrived since the last arming.
    async fn wait_for_arrivals(&self, count: usize) {
        let waited = tokio::time::timeout(STEP, async {
            loop {
                let arrival = self.arrival.notified();
                if self.arrived().len() >= count {
                    return;
                }
                arrival.await;
            }
        })
        .await;
        assert!(
            waited.is_ok(),
            "{count} builds arrive at the gate ({} arrived)",
            self.arrived().len()
        );
    }

    /// Release the held builds in rounds of `concurrency`, each round once
    /// all of its builds wait at the gate. Returns the number of rounds.
    async fn release_in_rounds(&self, members: usize, concurrency: usize) -> usize {
        let mut released = 0;
        let mut rounds = 0;
        while released < members {
            let round = concurrency.min(members - released);
            self.wait_for_arrivals(released + round).await;
            let arrived = self.arrived();
            assert_eq!(
                arrived.len(),
                released + round,
                "round {rounds}: no more member restores run at once than the bound"
            );
            for session in &arrived[released..] {
                self.release(session);
            }
            released += round;
            rounds += 1;
        }
        rounds
    }
}

/// [`PersistentMockBuilder`], holding each armed session's build at the
/// [`RestoreGate`].
struct HeldRestoreBuilder {
    gate: Arc<RestoreGate>,
}

#[async_trait]
impl SessionAgentBuilder for HeldRestoreBuilder {
    type Agent = PersistentMockAgent;

    async fn build_agent(
        &self,
        req: &CreateSessionRequest,
        event_tx: tokio::sync::mpsc::Sender<AgentEvent>,
    ) -> Result<Self::Agent, SessionError> {
        self.gate
            .hold_if_armed(&requested_test_session_id(req))
            .await;
        PersistentMockBuilder.build_agent(req, event_tx).await
    }
}

/// The host's customizer: it records which members' restores asked it, and
/// fails one member's restore request, alone (`refused`) or with the whole
/// resume (`renamed`, which a restore refuses as a changed identity).
struct RestoreCustomizer {
    refused: Option<AgentIdentity>,
    renamed: Option<AgentIdentity>,
    asked: std::sync::Mutex<Vec<AgentIdentity>>,
}

impl RestoreCustomizer {
    fn new(refused: Option<AgentIdentity>, renamed: Option<AgentIdentity>) -> Arc<Self> {
        Arc::new(Self {
            refused,
            renamed,
            asked: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<AgentIdentity> {
        self.asked.lock().expect("asked members").clone()
    }
}

impl SpawnMemberCustomizer for RestoreCustomizer {
    fn customize_spawn(
        &self,
        ctx: &SpawnCustomizationContext,
        spec: &mut SpawnMemberSpec,
    ) -> Result<(), MobError> {
        if ctx.spawn_source != SpawnSource::Resume {
            return Ok(());
        }
        self.asked
            .lock()
            .expect("asked members")
            .push(spec.identity.clone());
        if self.refused.as_ref() == Some(&spec.identity) {
            return Err(MobError::Internal(format!(
                "the host does not restore {} yet",
                spec.identity
            )));
        }
        if self.renamed.as_ref() == Some(&spec.identity) {
            spec.identity = AgentIdentity::from("impostor");
        }
        Ok(())
    }
}

fn worker(index: usize) -> AgentIdentity {
    AgentIdentity::from(format!("worker-{index:02}"))
}

/// A mob whose members were spawned on a persistent session service and
/// shut down, ready to resume on the same service and stores.
struct SpawnedMob {
    mob_id: MobId,
    service: Arc<meerkat_session::PersistentSessionService<HeldRestoreBuilder>>,
    storage: MobStorage,
    gate: Arc<RestoreGate>,
    /// Each member and the session it was spawned on, in roster order.
    sessions: Vec<(AgentIdentity, SessionId)>,
}

impl SpawnedMob {
    async fn spawn(members: usize) -> Self {
        let gate = RestoreGate::new();
        let session_store: Arc<dyn SessionStore> = Arc::new(MemoryStore::new());
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
            Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
        let blob_store: Arc<dyn meerkat_core::BlobStore> =
            Arc::new(meerkat_store::MemoryBlobStore::new());
        let service = Arc::new(meerkat_session::PersistentSessionService::new(
            HeldRestoreBuilder { gate: gate.clone() },
            64,
            session_store,
            runtime_store,
            blob_store,
        ));
        let mut definition = with_unique_mob_id(sample_definition(), "parallel-restore");
        definition.orchestrator = None;
        let mob_id = definition.id.clone();
        let storage = MobStorage::in_memory();
        let handle = MobBuilder::new(definition, storage.clone())
            .with_session_service(service.clone())
            .create()
            .await
            .expect("create the mob");
        let mut sessions = Vec::with_capacity(members);
        for index in 0..members {
            let identity = worker(index);
            let mut spec = SpawnMemberSpec::new("worker", identity.as_str());
            spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
            handle.spawn_spec(spec).await.expect("spawn a worker");
            let session = handle
                .resolve_bridge_session_id(&identity)
                .await
                .expect("session-backed worker");
            sessions.push((identity, session));
        }
        handle
            .shutdown()
            .await
            .expect("shut the first runtime down");
        Self {
            mob_id,
            service,
            storage,
            gate,
            sessions,
        }
    }

    /// A restart finds no live session: every member is restored through
    /// its provision.
    async fn drop_live_sessions(&self) {
        for (identity, session) in &self.sessions {
            if self.service.live_session_actor_registered(session).await {
                MobSessionService::discard_live_session(self.service.as_ref(), session)
                    .await
                    .unwrap_or_else(|error| {
                        panic!("{identity}: discard the live session the shutdown kept: {error}")
                    });
            }
        }
    }

    fn resume_builder(&self, concurrency: usize) -> MobBuilder {
        MobBuilder::for_resume(self.storage.clone())
            .with_session_service(self.service.clone())
            .notify_orchestrator_on_resume(false)
            .with_resume_restore_concurrency(concurrency)
    }

    /// Start a resume at `concurrency` in a task of its own.
    async fn spawn_resume(
        &self,
        concurrency: usize,
    ) -> tokio::task::JoinHandle<Result<MobHandle, MobError>> {
        self.drop_live_sessions().await;
        tokio::spawn(self.resume_builder(concurrency).resume())
    }

    /// Resume at `concurrency`, holding every member's provision at the
    /// gate and releasing them in rounds.
    async fn resume_held(&self, concurrency: usize) -> HeldResume {
        self.gate
            .arm(self.sessions.iter().map(|(_, session)| session.clone()));
        let journal = self.journal().await;
        let resume = self.spawn_resume(concurrency).await;
        let rounds = self
            .gate
            .release_in_rounds(self.sessions.len(), concurrency)
            .await;
        let handle = joined(resume).await.expect("the resume succeeds");
        let appended = self.journal().await.split_off(journal.len());
        HeldResume {
            rounds,
            max_held: self.gate.max_held.load(Ordering::SeqCst),
            mob: self.restored(&handle, appended).await,
            handle,
        }
    }

    /// Resume at `concurrency`, with `customizer` when given, nothing held.
    /// Returns the resumed mob with what it left, or the resume's error.
    async fn resume(
        &self,
        concurrency: usize,
        customizer: Option<Arc<RestoreCustomizer>>,
    ) -> Result<(MobHandle, RestoredMob), MobError> {
        self.drop_live_sessions().await;
        let journal = self.journal().await;
        let mut builder = self.resume_builder(concurrency);
        if let Some(customizer) = customizer {
            builder = builder.with_spawn_member_customizer(customizer);
        }
        let handle = tokio::time::timeout(STEP, Box::pin(builder.resume()))
            .await
            .expect("the resume answers")?;
        let appended = self.journal().await.split_off(journal.len());
        let restored = self.restored(&handle, appended).await;
        Ok((handle, restored))
    }

    /// The kind of every event in the mob's journal, in order.
    async fn journal(&self) -> Vec<String> {
        self.storage
            .events
            .replay_all()
            .await
            .expect("read the mob journal")
            .iter()
            .map(|event| format!("{:?}", std::mem::discriminant(&event.kind)))
            .collect()
    }

    /// What `handle`'s resume left, each member's session named by whether
    /// it is the session the member was spawned on. `appended` are the kinds
    /// of the events the resume appended to the journal.
    async fn restored(&self, handle: &MobHandle, appended: Vec<String>) -> RestoredMob {
        let listed = handle.list_members().await;
        let state = handle.machine_state_watch_rx.borrow().clone();
        let mut members = Vec::with_capacity(self.sessions.len());
        for (identity, spawned) in &self.sessions {
            let status = listed
                .iter()
                .find(|entry| &entry.agent_identity == identity)
                .map(|entry| format!("{:?}", entry.status));
            let dsl_identity = crate::machines::mob_machine::AgentIdentity::from_domain(identity);
            members.push(RestoredMember {
                identity: identity.clone(),
                status,
                on_its_own_session: handle.resolve_bridge_session_id(identity).await.as_ref()
                    == Some(spawned),
                machine: format!(
                    "{:?}",
                    (
                        state
                            .member_session_bindings
                            .get(&dsl_identity)
                            .map(|session| session.0 == spawned.to_string()),
                        state.member_profile_names.get(&dsl_identity),
                        state.member_runtime_modes.get(&dsl_identity),
                        state.member_revival_pending.contains(&dsl_identity),
                        state.member_restore_failures.get(&dsl_identity),
                        state
                            .spawn_profile_authority_profile_names
                            .get(&dsl_identity),
                        state
                            .spawn_profile_authority_material_digests
                            .get(&dsl_identity),
                    )
                ),
            });
        }
        let restore_failures = handle
            .restore_diagnostics
            .read()
            .await
            .iter()
            .map(|(identity, diagnostic)| (identity.clone(), diagnostic.reason.clone()))
            .collect();
        RestoredMob {
            members,
            restore_failures,
            appended,
        }
    }
}

/// The resume `task` answers, within the step bound, without a panic.
async fn joined(
    task: tokio::task::JoinHandle<Result<MobHandle, MobError>>,
) -> Result<MobHandle, MobError> {
    tokio::time::timeout(STEP, task)
        .await
        .expect("the resume answers")
        .expect("the resume task does not panic")
}

struct HeldResume {
    rounds: usize,
    max_held: usize,
    mob: RestoredMob,
    handle: MobHandle,
}

#[derive(Debug, PartialEq)]
struct RestoredMob {
    members: Vec<RestoredMember>,
    restore_failures: Vec<(AgentIdentity, String)>,
    /// The kinds of the events the resume appended to the journal, in order.
    appended: Vec<String>,
}

#[derive(Debug, PartialEq)]
struct RestoredMember {
    identity: AgentIdentity,
    status: Option<String>,
    on_its_own_session: bool,
    /// The member's MobMachine entries the restore writes.
    machine: String,
}

fn every_member_back(mob: &RestoredMob) {
    assert!(
        mob.restore_failures.is_empty(),
        "{:?}",
        mob.restore_failures
    );
    for member in &mob.members {
        assert!(member.on_its_own_session, "{member:?}");
        assert_eq!(member.status.as_deref(), Some("Active"), "{member:?}");
    }
}

/// A resume of 8 members at concurrency 1 takes 8 provision rounds, one
/// member at a time; at concurrency 4 it takes 2, with 4 members' provisions
/// waiting at once. Both resumes run on the same seeded stores and leave the
/// same mob: every member back on its own session, the same MobMachine
/// entries, no restore failure, and the same events appended.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resume_restores_members_up_to_its_concurrency_and_leaves_the_serial_mob() {
    let mob = SpawnedMob::spawn(MEMBERS).await;

    let serial = mob.resume_held(1).await;
    assert_eq!(serial.rounds, MEMBERS, "one member at a time");
    assert_eq!(serial.max_held, 1);
    every_member_back(&serial.mob);
    serial
        .handle
        .shutdown()
        .await
        .expect("shut the serial resume down");

    let concurrent = mob.resume_held(4).await;
    assert_eq!(
        concurrent.rounds,
        MEMBERS.div_ceil(4),
        "four members at a time"
    );
    assert_eq!(concurrent.max_held, 4);
    assert_eq!(
        concurrent.mob, serial.mob,
        "the concurrent resume leaves the mob the serial resume left"
    );
    concurrent
        .handle
        .shutdown()
        .await
        .expect("shut the concurrent resume down");
}

/// At concurrency 1 a member's restore starts only once the turn of the
/// member before it returned: while worker-00's turn is held, worker-01's
/// restore has asked for its permit and has not started. The whole pass is
/// one member after another: restore, turn, then the next member.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn at_concurrency_one_each_member_is_restored_and_applied_before_the_next_restore_starts() {
    let mob = SpawnedMob::spawn(3).await;
    let probe = restore_pass_probe::install(&mob.mob_id);
    probe.hold_turn(&worker(0));
    let resume = mob.spawn_resume(1).await;

    let waited = tokio::time::timeout(
        STEP,
        probe.wait_for(|events| {
            events.contains(&RestorePassEvent::TurnStarted(worker(0)))
                && events.contains(&RestorePassEvent::PermitRequested(worker(1)))
        }),
    )
    .await;
    assert!(
        waited.is_ok(),
        "worker-00's turn starts and worker-01's restore asks for its permit: {:?}",
        probe.events()
    );
    assert!(
        !probe
            .events()
            .contains(&RestorePassEvent::RestoreStarted(worker(1))),
        "worker-01's restore waits for worker-00's turn: {:?}",
        probe.events()
    );
    probe.release_turn(&worker(0));
    let handle = joined(resume).await.expect("the resume succeeds");

    let pass = probe
        .events()
        .into_iter()
        .filter(|event| !matches!(event, RestorePassEvent::PermitRequested(_)))
        .collect::<Vec<_>>();
    let one_after_another = (0..3)
        .flat_map(|index| {
            [
                RestorePassEvent::RestoreStarted(worker(index)),
                RestorePassEvent::RestoreFinished(worker(index)),
                RestorePassEvent::TurnStarted(worker(index)),
                RestorePassEvent::TurnFinished(worker(index)),
            ]
        })
        .collect::<Vec<_>>();
    assert_eq!(pass, one_after_another);
    handle.shutdown().await.expect("shut the resume down");
}

/// The members' turns apply in roster order however their restores finish:
/// with the four provisions released last member first, the restores finish
/// worker-03 first, and the turns still run worker-00 first. A pass that
/// applied turns in completion order fails this row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turns_apply_in_roster_order_however_the_restores_finish() {
    let mob = SpawnedMob::spawn(4).await;
    let probe = restore_pass_probe::install(&mob.mob_id);
    mob.gate
        .arm(mob.sessions.iter().map(|(_, session)| session.clone()));
    let resume = mob.spawn_resume(4).await;
    mob.gate.wait_for_arrivals(4).await;
    for (identity, session) in mob.sessions.iter().rev() {
        mob.gate.release(session);
        let finished = tokio::time::timeout(
            STEP,
            probe.wait_for(|events| {
                events.contains(&RestorePassEvent::RestoreFinished(identity.clone()))
            }),
        )
        .await;
        assert!(finished.is_ok(), "{identity}'s restore finishes");
    }
    let handle = joined(resume).await.expect("the resume succeeds");

    let events = probe.events();
    let finished = events
        .iter()
        .filter_map(|event| match event {
            RestorePassEvent::RestoreFinished(identity) => Some(identity.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished, (0..4).rev().map(worker).collect::<Vec<_>>());
    let turns = events
        .iter()
        .filter_map(|event| match event {
            RestorePassEvent::TurnStarted(identity) => Some(identity.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(turns, (0..4).map(worker).collect::<Vec<_>>());
    handle.shutdown().await.expect("shut the resume down");
}

/// What a resume whose turn of worker-02 failed left.
struct FailedTurnResume {
    error: String,
    /// Members after worker-02 whose restore started.
    started_after: Vec<AgentIdentity>,
    /// Members after worker-02 left with a live session.
    left_live: Vec<AgentIdentity>,
}

/// Resume `members` members at `concurrency` with worker-02's turn failing
/// the resume, then resume again without the failure, which brings every
/// member back.
async fn resume_failing_a_turn(members: usize, concurrency: usize) -> FailedTurnResume {
    const FAILING_TURN: usize = 2;
    let mob = SpawnedMob::spawn(members).await;
    let probe = restore_pass_probe::install(&mob.mob_id);
    probe.fail_turn(&worker(FAILING_TURN));
    let error = match mob.resume(concurrency, None).await {
        Ok(_) => panic!("the failed turn fails the resume"),
        Err(error) => error.to_string(),
    };
    let started_after = probe
        .events()
        .iter()
        .filter_map(|event| match event {
            RestorePassEvent::RestoreStarted(identity)
                if (FAILING_TURN + 1..members).any(|index| &worker(index) == identity) =>
            {
                Some(identity.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    // A released member's live session goes away with its attachment.
    let mut left_live = Vec::new();
    for (identity, session) in &mob.sessions[FAILING_TURN + 1..] {
        let released = tokio::time::timeout(Duration::from_secs(10), async {
            while mob.service.live_session_actor_registered(session).await {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        if released.is_err() {
            left_live.push(identity.clone());
        }
    }

    // The next resume, in the same process, finds the failed resume's live
    // sessions as it left them and brings every member back.
    probe.stop_failing_turns();
    let handle = tokio::time::timeout(STEP, Box::pin(mob.resume_builder(concurrency).resume()))
        .await
        .expect("the resume answers")
        .expect("a resume after the failed one succeeds");
    every_member_back(&mob.restored(&handle, Vec::new()).await);
    handle.shutdown().await.expect("shut the resume down");
    FailedTurnResume {
        error,
        started_after,
        left_live,
    }
}

/// A turn that fails the resume (worker-02's, failed by the probe) fails it
/// with the same error at concurrency 1 and 4. At 1 no member after it
/// started; at 4 at most 3 did, and none of them is left with a live session:
/// the resume released every provision it did not apply. The next resume
/// brings every member back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_fails_the_resume_fails_it_as_at_concurrency_one_and_leaves_nothing_unapplied()
{
    const CONCURRENCY: usize = 4;
    let serial = resume_failing_a_turn(MEMBERS, 1).await;
    let concurrent = resume_failing_a_turn(MEMBERS, CONCURRENCY).await;
    assert!(
        serial
            .error
            .contains("injected restore turn failure for 'worker-02'"),
        "{}",
        serial.error
    );
    assert_eq!(concurrent.error, serial.error, "the same error");
    assert_eq!(serial.started_after, Vec::<AgentIdentity>::new());
    assert!(
        concurrent.started_after.len() < CONCURRENCY,
        "at most {} members after the failed one started: {:?}",
        CONCURRENCY - 1,
        concurrent.started_after
    );
    assert_eq!(serial.left_live, Vec::<AgentIdentity>::new());
    assert_eq!(
        concurrent.left_live,
        Vec::<AgentIdentity>::new(),
        "every member restored but not applied was released"
    );
}

/// A member whose restore fails alone (its host customizer refuses it) is
/// restored by neither resume, with the same diagnostic, while every other
/// member comes back: the concurrent resume leaves the mob the serial one
/// does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_restore_that_fails_alone_fails_alone_as_in_a_serial_resume() {
    let mob = SpawnedMob::spawn(MEMBERS).await;

    let (serial, serial_mob) = mob
        .resume(1, Some(RestoreCustomizer::new(Some(worker(FAILING)), None)))
        .await
        .expect("the serial resume succeeds");
    assert_eq!(
        serial_mob
            .restore_failures
            .iter()
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>(),
        vec![worker(FAILING)],
        "{serial_mob:?}"
    );
    assert!(
        serial_mob.restore_failures[0]
            .1
            .contains("the host does not restore worker-03 yet"),
        "{serial_mob:?}"
    );
    for member in &serial_mob.members {
        if member.identity != worker(FAILING) {
            assert!(member.on_its_own_session, "{member:?}");
            assert_eq!(member.status.as_deref(), Some("Active"), "{member:?}");
        }
    }
    serial
        .shutdown()
        .await
        .expect("shut the serial resume down");

    let (concurrent, concurrent_mob) = mob
        .resume(4, Some(RestoreCustomizer::new(Some(worker(FAILING)), None)))
        .await
        .expect("the concurrent resume succeeds");
    assert_eq!(
        concurrent_mob, serial_mob,
        "the concurrent resume leaves the mob the serial resume left"
    );
    concurrent
        .shutdown()
        .await
        .expect("shut the concurrent resume down");
}

/// A member restore that fails the resume (its customizer changes its
/// identity) fails both resumes with the same error. The serial resume asks
/// the customizer for no member after the failing one. The concurrent resume
/// starts no member restore once that one has failed: it asked for every
/// member up to the failing one, and for none beyond the members that could
/// already be running alongside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_restore_that_fails_the_resume_fails_it_as_a_serial_resume_does() {
    const CONCURRENCY: usize = 4;

    let serial_mob = SpawnedMob::spawn(MEMBERS).await;
    let serial_customizer = RestoreCustomizer::new(None, Some(worker(FAILING)));
    let Err(serial_error) = serial_mob.resume(1, Some(serial_customizer.clone())).await else {
        panic!("the serial resume fails");
    };
    assert_eq!(
        serial_customizer.asked(),
        (0..=FAILING).map(worker).collect::<Vec<_>>(),
        "the serial resume restores the members in roster order and stops at the failure"
    );

    let concurrent_mob = SpawnedMob::spawn(MEMBERS).await;
    let concurrent_customizer = RestoreCustomizer::new(None, Some(worker(FAILING)));
    let Err(concurrent_error) = concurrent_mob
        .resume(CONCURRENCY, Some(concurrent_customizer.clone()))
        .await
    else {
        panic!("the concurrent resume fails");
    };
    assert_eq!(
        concurrent_error.to_string(),
        serial_error.to_string(),
        "the same error"
    );
    assert!(
        serial_error
            .to_string()
            .contains("cannot change resume restore identity from 'worker-03'"),
        "{serial_error}"
    );
    let asked = concurrent_customizer.asked();
    for index in 0..=FAILING {
        assert!(
            asked.contains(&worker(index)),
            "{} is restored before the failure: {asked:?}",
            worker(index)
        );
    }
    for identity in &asked {
        assert!(
            (0..FAILING + CONCURRENCY).any(|index| &worker(index) == identity),
            "{identity} started no later than the bound allows: {asked:?}"
        );
    }
}

/// A fork child of an in-mob source is restored in its own turn, after its
/// source, with the overlay its source was restored with, at concurrency 1
/// and at the default 16 alike.
#[tokio::test]
async fn a_fork_child_restored_after_a_crash_is_built_as_its_source_at_any_restore_concurrency() {
    for concurrency in [1, 16] {
        let (service, storage, source_identity, child_identity, first, expected_source) =
            seat_fork_child_then_crash(false).await;
        let resumed = MobBuilder::for_resume(storage)
            .with_session_service(service.clone())
            .with_spawn_member_customizer(Arc::new(IdentityOverlayCustomizer {
                source: source_identity.clone(),
            }))
            .with_resume_restore_concurrency(concurrency)
            .resume()
            .await
            .unwrap_or_else(|error| panic!("resume at concurrency {concurrency}: {error}"));
        assert_restored_child_built_as_its_source(
            &service,
            &resumed,
            &source_identity,
            &child_identity,
            &first,
            &expected_source,
        )
        .await;
        crash_stop_and_release_routes(resumed).await;
    }
}

/// What a resume on a service that keeps no durable sessions left: each
/// member recreated on a fresh session.
async fn recreated_members(concurrency: usize) -> Vec<(AgentIdentity, String, bool, bool)> {
    let service = Arc::new(MockSessionService::new());
    service.keep_no_durable_sessions();
    let _ = service.enable_runtime_adapter();
    let mut definition = with_unique_mob_id(sample_definition(), "parallel-recreate");
    definition.orchestrator = None;
    let storage = MobStorage::in_memory();
    let handle = MobBuilder::new(definition, storage.clone())
        .with_session_service(service.clone())
        .allow_ephemeral_sessions(true)
        .create()
        .await
        .expect("create a mob without durable sessions");
    let mut sessions = Vec::new();
    for index in 0..6 {
        let identity = worker(index);
        let mut spec = SpawnMemberSpec::new("worker", identity.as_str());
        spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
        handle.spawn_spec(spec).await.expect("spawn a worker");
        let session = handle
            .resolve_bridge_session_id(&identity)
            .await
            .expect("session-backed worker");
        sessions.push((identity, session));
    }
    handle
        .shutdown()
        .await
        .expect("shut the first runtime down");
    for (identity, session) in &sessions {
        if MobSessionService::live_session_actor_registered(service.as_ref(), session)
            .await
            .expect("read the live session registry")
        {
            MobSessionService::discard_live_session(service.as_ref(), session)
                .await
                .unwrap_or_else(|error| panic!("{identity}: discard the live session: {error}"));
        }
    }
    let resumed = tokio::time::timeout(
        STEP,
        Box::pin(
            MobBuilder::for_resume(storage)
                .with_session_service(service.clone())
                .allow_ephemeral_sessions(true)
                .notify_orchestrator_on_resume(false)
                .with_resume_restore_concurrency(concurrency)
                .resume(),
        ),
    )
    .await
    .expect("the resume answers")
    .expect("the resume succeeds");
    assert!(
        resumed.restore_diagnostics.read().await.is_empty(),
        "no member restore failed"
    );
    let state = resumed.machine_state_watch_rx.borrow().clone();
    let listed = resumed.list_members().await;
    let mut recreated = Vec::new();
    for (identity, spawned) in &sessions {
        let status = listed
            .iter()
            .find(|entry| &entry.agent_identity == identity)
            .map(|entry| format!("{:?}", entry.status))
            .unwrap_or_default();
        let current = resumed.resolve_bridge_session_id(identity).await;
        let bound = state
            .member_session_bindings
            .get(&crate::machines::mob_machine::AgentIdentity::from_domain(
                identity,
            ))
            .map(|session| session.0.clone());
        recreated.push((
            identity.clone(),
            status,
            current.as_ref().map(ToString::to_string) == bound,
            current.as_ref() != Some(spawned),
        ));
    }
    resumed.shutdown().await.expect("shut the resume down");
    recreated
}

/// A service that keeps no durable sessions recreates every member on a
/// fresh session, bound in MobMachine, at concurrency 4 as at concurrency 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_without_durable_sessions_recreates_members_as_it_does_one_at_a_time() {
    let serial = recreated_members(1).await;
    for (identity, status, bound, fresh) in &serial {
        assert_eq!(status, "Active", "{identity}");
        assert!(*bound && *fresh, "{identity}: bound to its fresh session");
    }
    assert_eq!(recreated_members(4).await, serial);
}
