//! A cold resume restores its members up to its restore concurrency at a
//! time, and the mob it leaves is the one a restore of one member at a time
//! leaves.
//!
//! Each fixture spawns its members on a persistent session service, shuts the
//! mob down and resumes it on the same service and stores. The agent builder
//! holds the build of each restored member (its provision) until the test
//! releases it, so the store round trips of a restore are counted in release
//! rounds: a resume of N members at concurrency C takes ceil(N / C) rounds.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bound for one step; it only turns a hang into a failure.
const STEP: Duration = Duration::from_secs(60);

const MEMBERS: usize = 8;

/// The member whose restore fails in the failure rows.
const FAILING: usize = 3;

/// Holds the agent build of each armed session (a restore's provision) until
/// the test releases it.
struct RestoreGate {
    armed: std::sync::Mutex<HashSet<SessionId>>,
    /// Armed builds that arrived since the last arming.
    arrivals: AtomicUsize,
    /// Builds waiting at the gate now.
    held: AtomicUsize,
    /// The most builds that waited at the gate at once since the last arming.
    max_held: AtomicUsize,
    releases: tokio::sync::Semaphore,
}

impl RestoreGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            armed: std::sync::Mutex::new(HashSet::new()),
            arrivals: AtomicUsize::new(0),
            held: AtomicUsize::new(0),
            max_held: AtomicUsize::new(0),
            releases: tokio::sync::Semaphore::new(0),
        })
    }

    /// Hold the next build of each of `sessions`.
    fn arm(&self, sessions: impl IntoIterator<Item = SessionId>) {
        assert_eq!(
            self.held.load(Ordering::SeqCst),
            0,
            "no build waits at the gate"
        );
        self.arrivals.store(0, Ordering::SeqCst);
        self.max_held.store(0, Ordering::SeqCst);
        self.armed.lock().expect("armed sessions").extend(sessions);
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
        self.arrivals.fetch_add(1, Ordering::SeqCst);
        let held = self.held.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_held.fetch_max(held, Ordering::SeqCst);
        self.releases
            .acquire()
            .await
            .expect("the gate stays open")
            .forget();
        self.held.fetch_sub(1, Ordering::SeqCst);
    }

    /// Release the held builds in rounds of `concurrency`, each round once
    /// all of its builds wait at the gate. Returns the number of rounds.
    async fn release_in_rounds(&self, members: usize, concurrency: usize) -> usize {
        let mut released = 0;
        let mut rounds = 0;
        while released < members {
            let round = concurrency.min(members - released);
            let waited = tokio::time::timeout(STEP, async {
                while self.arrivals.load(Ordering::SeqCst) < released + round {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await;
            assert!(
                waited.is_ok(),
                "round {rounds}: {round} member restores wait at once ({} arrived)",
                self.arrivals.load(Ordering::SeqCst) - released
            );
            assert_eq!(
                self.arrivals.load(Ordering::SeqCst),
                released + round,
                "round {rounds}: no more member restores run at once than the bound"
            );
            self.releases.add_permits(round);
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
            service,
            storage,
            gate,
            sessions,
        }
    }

    /// Resume at `concurrency`, holding every member's provision at the
    /// gate and releasing them in rounds.
    async fn resume_held(&self, concurrency: usize) -> HeldResume {
        // A restart finds no live session: every member is restored through
        // its provision.
        for (identity, session) in &self.sessions {
            if self.service.live_session_actor_registered(session).await {
                MobSessionService::discard_live_session(self.service.as_ref(), session)
                    .await
                    .unwrap_or_else(|error| {
                        panic!("{identity}: discard the live session the shutdown kept: {error}")
                    });
            }
        }
        self.gate
            .arm(self.sessions.iter().map(|(_, session)| session.clone()));
        let journal = self.journal().await;
        let resume = tokio::spawn(
            MobBuilder::for_resume(self.storage.clone())
                .with_session_service(self.service.clone())
                .notify_orchestrator_on_resume(false)
                .with_resume_restore_concurrency(concurrency)
                .resume(),
        );
        let rounds = self
            .gate
            .release_in_rounds(self.sessions.len(), concurrency)
            .await;
        let handle = tokio::time::timeout(STEP, resume)
            .await
            .expect("the resume answers")
            .expect("the resume task does not panic")
            .expect("the resume succeeds");
        let appended = self.journal().await.split_off(journal.len());
        HeldResume {
            rounds,
            max_held: self.gate.max_held.load(Ordering::SeqCst),
            mob: self.restored(&handle, appended).await,
            handle,
        }
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

    /// Resume at `concurrency` with `customizer`, nothing held. Returns the
    /// resumed mob with what it left, or the resume's error.
    async fn resume_customized(
        &self,
        concurrency: usize,
        customizer: Arc<RestoreCustomizer>,
    ) -> Result<(MobHandle, RestoredMob), MobError> {
        let journal = self.journal().await;
        let resume = MobBuilder::for_resume(self.storage.clone())
            .with_session_service(self.service.clone())
            .with_spawn_member_customizer(customizer)
            .notify_orchestrator_on_resume(false)
            .with_resume_restore_concurrency(concurrency)
            .resume();
        let handle = tokio::time::timeout(STEP, Box::pin(resume))
            .await
            .expect("the resume answers")?;
        let appended = self.journal().await.split_off(journal.len());
        let restored = self.restored(&handle, appended).await;
        Ok((handle, restored))
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
    assert!(
        serial.mob.restore_failures.is_empty(),
        "{:?}",
        serial.mob.restore_failures
    );
    for member in &serial.mob.members {
        assert!(member.on_its_own_session, "{member:?}");
        assert_eq!(member.status.as_deref(), Some("Active"), "{member:?}");
    }
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

/// A member whose restore fails alone (its host customizer refuses it) is
/// restored by neither resume, with the same diagnostic, while every other
/// member comes back: the concurrent resume leaves the mob the serial one
/// does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_restore_that_fails_alone_fails_alone_as_in_a_serial_resume() {
    let mob = SpawnedMob::spawn(MEMBERS).await;

    let (serial, serial_mob) = mob
        .resume_customized(1, RestoreCustomizer::new(Some(worker(FAILING)), None))
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
        .resume_customized(4, RestoreCustomizer::new(Some(worker(FAILING)), None))
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
    let Err(serial_error) = serial_mob
        .resume_customized(1, serial_customizer.clone())
        .await
    else {
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
        .resume_customized(CONCURRENCY, concurrent_customizer.clone())
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
