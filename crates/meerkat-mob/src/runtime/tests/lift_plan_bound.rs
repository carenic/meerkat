#![cfg(all(feature = "runtime-adapter", not(target_arch = "wasm32")))]
//! The lift of a stopped mob dispatches its member plan at most the restore
//! concurrency at a time.
//!
//! A stopped mob is restarted (every member Dormant) and lifted under the
//! Eager policy, so the lift's plan materializes every member. The plan
//! applies `PlanBootMaterialization` to the next member only while fewer
//! than the restore concurrency of its wakes are open: a member it has not
//! reached stays Dormant, with no wake attempt (so no wake bound runs for
//! it), and a delivery wakes it at once through a wake of its own. The rows
//! hold the members' cold restores at the wake restore probe and read each
//! member's phase and wake attempt from the machine state the actor
//! publishes.
//!
//! Unrun until the candidate's validation.

use super::*;
use crate::machines::mob_machine as dsl;
use crate::runtime::MemberRestorePolicy;
use crate::runtime::actor::member_wake::MemberWakeOutcome;
use crate::runtime::actor::member_wake_turn::probe_member_wake_restores_for_test;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bound for one step; it only turns a hang into a failure.
const STEP: Duration = Duration::from_secs(30);

/// How long a row waits, with the bound's worth of restores held, for one
/// more to start. A lift without the bound starts every member's restore
/// within milliseconds of the first.
const SETTLE: Duration = Duration::from_millis(300);

/// The mob's restore concurrency.
const BOUND: usize = 2;

/// The members the lift's plan materializes: more than two rounds of the
/// bound.
const MEMBERS: usize = 5;

fn worker(index: usize) -> AgentIdentity {
    AgentIdentity::from(format!("plan-bound-{index}"))
}

/// A stopped mob of `MEMBERS` turn-driven workers after a restart: the
/// resumed handle is Stopped, every member Dormant, and its builder asked for
/// the Eager policy at restore concurrency `BOUND`.
struct StoppedMob {
    handle: MobHandle,
    service: Arc<MockSessionService>,
    /// Each worker and its session, in identity order (the plan's order).
    sessions: Vec<(AgentIdentity, SessionId)>,
}

impl StoppedMob {
    async fn new(label: &str) -> Self {
        let definition = with_unique_mob_id(sample_definition(), label);
        let service = Arc::new(MockSessionService::new());
        let _ = service.enable_runtime_adapter();
        let storage = MobStorage::in_memory();
        let events = storage.events.clone();
        let runtime_metadata = storage.runtime_metadata.clone();
        let handle = MobBuilder::new(definition, storage)
            .with_session_service(service.clone())
            .create()
            .await
            .expect("create the mob");
        let mut sessions = Vec::with_capacity(MEMBERS);
        for index in 0..MEMBERS {
            let identity = worker(index);
            let mut spec = SpawnMemberSpec::new("worker", identity.clone());
            spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
            handle.spawn_spec(spec).await.expect("spawn a worker");
            let session = handle
                .resolve_bridge_session_id(&identity)
                .await
                .expect("a session-backed worker");
            sessions.push((identity, session));
        }
        handle.stop().await.expect("stop the mob");
        let restarted = Arc::new(service.cold_restart_preserving_durable_state().await);
        let _ = restarted.enable_runtime_adapter();
        crash_stop_and_release_routes(handle).await;
        drop(service);
        let builder = MobBuilder::for_resume(MobStorage::with_events_and_runtime_metadata(
            events,
            runtime_metadata,
        ))
        .with_session_service(restarted.clone())
        .notify_orchestrator_on_resume(false)
        .with_member_restore_policy(MemberRestorePolicy::Eager)
        .with_resume_restore_concurrency(BOUND);
        let handle = tokio::time::timeout(STEP, Box::pin(builder.resume()))
            .await
            .expect("the restart answers")
            .expect("the restart succeeds");
        assert_eq!(
            handle.status().await.expect("status"),
            MobState::Stopped,
            "the restarted mob is stopped"
        );
        for (identity, _) in &sessions {
            assert_eq!(
                handle.member_materialization_for_test(identity),
                Some(dsl::MemberMaterializationPhase::Dormant),
                "{identity} is Dormant after the restart"
            );
        }
        Self {
            handle,
            service: restarted,
            sessions,
        }
    }

    /// Lift the mob in a task of its own.
    fn lift(&self) -> tokio::task::JoinHandle<Result<(), MobError>> {
        let handle = self.handle.clone();
        tokio::spawn(async move { handle.resume().await })
    }

    /// Join the lift and answer its result: the unit build's lift observer
    /// reports a stall after two seconds without progress, and the lift stays
    /// joinable. The rows read the result last, after the plan's own facts.
    async fn join_lift(
        &self,
        lift: tokio::task::JoinHandle<Result<(), MobError>>,
    ) -> Result<(), MobError> {
        let lifted = tokio::time::timeout(STEP, lift)
            .await
            .expect("the lift answers")
            .expect("lift task");
        if matches!(
            lifted,
            Err(MobError::LifecycleOperationProgressStalled { .. })
        ) {
            return tokio::time::timeout(STEP, self.handle.resume())
                .await
                .expect("the joined lift answers");
        }
        lifted
    }

    /// Each member's phase and whether it holds a wake attempt, as the actor
    /// last published them.
    fn residency(
        &self,
    ) -> BTreeMap<AgentIdentity, (Option<dsl::MemberMaterializationPhase>, bool)> {
        let state = self.handle.machine_state_watch_rx.borrow();
        self.sessions
            .iter()
            .map(|(identity, _)| {
                let id = dsl::AgentIdentity::from_domain(identity);
                (
                    identity.clone(),
                    (
                        state.member_materialization.get(&id).copied(),
                        state.member_wake_attempts.contains_key(&id),
                    ),
                )
            })
            .collect()
    }

    /// The machine's wake attempt sequence, as the actor last published it:
    /// every wake attempt opened so far.
    fn wake_attempts(&self) -> u64 {
        self.handle
            .machine_state_watch_rx
            .borrow()
            .member_wake_attempt_sequence
    }

    /// Wait, bounded by `STEP`, until the published state holds every member
    /// Live with no wake attempt open. A lift that answered before its plan's
    /// last wakes closed (a failed lift) is read after them.
    async fn settle_live(&self) {
        let members: Vec<dsl::AgentIdentity> = self
            .sessions
            .iter()
            .map(|(identity, _)| dsl::AgentIdentity::from_domain(identity))
            .collect();
        let mut published = self.handle.machine_state_watch_rx.clone();
        let _ = tokio::time::timeout(
            STEP,
            published.wait_for(|state| {
                members.iter().all(|id| {
                    state.member_materialization.get(id)
                        == Some(&dsl::MemberMaterializationPhase::Live)
                        && !state.member_wake_attempts.contains_key(id)
                })
            }),
        )
        .await;
    }

    /// How many times the restarted service built each member's session.
    async fn builds(&self) -> BTreeMap<AgentIdentity, usize> {
        let created = self.service.recorded_create_requests().await;
        self.sessions
            .iter()
            .map(|(identity, session)| {
                (
                    identity.clone(),
                    created
                        .iter()
                        .filter(|record| &record.session_id == session)
                        .count(),
                )
            })
            .collect()
    }
}

/// The most members the published machine state held Waking at once, from
/// now on, until the task is aborted.
fn watch_waking_peak(handle: &MobHandle) -> (Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let peak = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&peak);
    let mut published = handle.machine_state_watch_rx.clone();
    let task = tokio::spawn(async move {
        loop {
            let waking = published
                .borrow_and_update()
                .member_materialization
                .values()
                .filter(|phase| matches!(phase, dsl::MemberMaterializationPhase::Waking))
                .count();
            recorded.fetch_max(waking, Ordering::SeqCst);
            if published.changed().await.is_err() {
                return;
            }
        }
    });
    (peak, task)
}

/// A lift of `MEMBERS` Dormant members that its Eager plan materializes, at
/// restore concurrency `BOUND`, every restore held until the row releases
/// it: each round starts exactly the bound's worth of the next members in
/// identity order (the last round the rest), the members the plan has not
/// reached are Dormant without a wake attempt while a round is held, no more
/// than `BOUND` members are ever Waking at once, and each member wakes under
/// exactly one attempt (none timed out and was woken again), is restored and
/// built once, and ends Live. The lift's own result is read last.
///
/// RED on the base (e294b75f29, with the probe and the row applied): the
/// lift planned every Dormant member at once, so all `MEMBERS` were Waking
/// and their restores started together: the first round counts `MEMBERS`,
/// no member is left Dormant, and the peak is `MEMBERS`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lift_dispatches_its_plan_at_most_the_restore_concurrency_at_once() {
    let mob = StoppedMob::new("lift-plan-bound").await;
    let probe = probe_member_wake_restores_for_test(
        mob.handle.mob_id(),
        mob.sessions.iter().map(|(identity, _)| identity.clone()),
    );
    let (peak, watcher) = watch_waking_peak(&mob.handle);
    let attempts_before = mob.wake_attempts();
    let lift = mob.lift();

    let mut rounds = Vec::new();
    let mut released = 0;
    while released < MEMBERS {
        let round = BOUND.min(MEMBERS - released);
        let reached = tokio::time::timeout(STEP, probe.wait_for_started(released + round))
            .await
            .is_ok();
        // A lift without the bound starts another restore while these are
        // held; a bounded one cannot until one of these wakes closes.
        let _ = tokio::time::timeout(SETTLE, probe.wait_for_started(released + round + 1)).await;
        let started = probe.started();
        rounds.push((started[released..].to_vec(), mob.residency()));
        // Released whatever started, so a failing row ends instead of
        // hanging.
        for identity in &started[released..] {
            probe.release(identity);
        }
        if !reached || started.len() == released {
            break;
        }
        released = started.len();
    }
    let lifted = mob.join_lift(lift).await;
    mob.settle_live().await;
    watcher.abort();

    let mut reached = 0;
    for (index, (started, residency)) in rounds.iter().enumerate() {
        let expected: BTreeSet<AgentIdentity> = (reached..(reached + BOUND).min(MEMBERS))
            .map(worker)
            .collect();
        assert_eq!(
            started.iter().cloned().collect::<BTreeSet<_>>(),
            expected,
            "round {index} starts the next {BOUND} members in identity order, and no more"
        );
        for (position, (identity, _)) in mob.sessions.iter().enumerate() {
            let (phase, attempt) = residency[identity];
            if position < reached {
                assert_eq!(
                    (phase, attempt),
                    (Some(dsl::MemberMaterializationPhase::Live), false),
                    "round {index}: {identity}, of an earlier round, is Live"
                );
            } else if expected.contains(identity) {
                assert_eq!(
                    (phase, attempt),
                    (Some(dsl::MemberMaterializationPhase::Waking), true),
                    "round {index}: {identity} is Waking under its own attempt"
                );
            } else {
                assert_eq!(
                    (phase, attempt),
                    (Some(dsl::MemberMaterializationPhase::Dormant), false),
                    "round {index}: {identity}, not reached yet, is Dormant with no wake attempt"
                );
            }
        }
        reached += expected.len();
    }
    assert_eq!(
        reached, MEMBERS,
        "the rounds reached every member: {rounds:?}"
    );
    assert!(
        peak.load(Ordering::SeqCst) <= BOUND,
        "no more than {BOUND} members were ever Waking at once (peak {})",
        peak.load(Ordering::SeqCst)
    );
    assert_eq!(
        probe.max_running(),
        BOUND,
        "at most {BOUND} member restores ran at once"
    );
    let mut started = probe.started();
    started.sort();
    assert_eq!(
        started,
        mob.sessions
            .iter()
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>(),
        "each member was restored once"
    );
    for (identity, builds) in mob.builds().await {
        assert_eq!(builds, 1, "{identity} was built once");
    }
    for (identity, (phase, attempt)) in mob.residency() {
        assert_eq!(
            (phase, attempt),
            (Some(dsl::MemberMaterializationPhase::Live), false),
            "{identity} ends Live"
        );
    }
    assert_eq!(
        mob.wake_attempts() - attempts_before,
        MEMBERS as u64,
        "each member woke under exactly one attempt: none timed out and was woken again"
    );
    assert!(lifted.is_ok(), "the lift succeeds: {lifted:?}");
}

/// While the lift's first two plan wakes are held, a peer message to the
/// last member (the `WakeMember` its inproc wake route sends) wakes it at
/// once through a wake of its own: it is Live while the plan's wakes are
/// still held. When the plan reaches it, the plan applies nothing for it:
/// it is restored and built once, by its own wake, and every member wakes
/// under exactly one attempt. The lift's own result is read last.
///
/// RED on the base (e294b75f29, with the probe and the row applied): the
/// lift planned every member at once, so the last three were Waking (or
/// already Live through the plan's own restores) before the message came,
/// not Dormant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_message_wakes_a_member_the_lift_has_not_reached_at_once() {
    let mob = StoppedMob::new("lift-plan-bypass").await;
    let held = [worker(0), worker(1)];
    let queued = worker(MEMBERS - 1);
    let probe = probe_member_wake_restores_for_test(mob.handle.mob_id(), held.clone());
    let attempts_before = mob.wake_attempts();
    let lift = mob.lift();
    let reached = tokio::time::timeout(STEP, probe.wait_for_started(BOUND))
        .await
        .is_ok();
    let _ = tokio::time::timeout(SETTLE, probe.wait_for_started(BOUND + 1)).await;
    let before = mob.residency();

    let answer = mob
        .handle
        .enqueue_actor_command_for_test(|reply_tx| crate::runtime::state::MobCommand::WakeMember {
            agent_identity: queued.clone(),
            cause: dsl::MemberWakeCause::PeerDelivery,
            observed_missing: false,
            input_class: dsl::PeerInputClass::Content,
            sender_waking: false,
            reply_tx,
        })
        .await
        .expect("the peer wake reaches the actor");
    let woken = tokio::time::timeout(STEP, answer).await;
    let during = mob.residency();
    for identity in &held {
        probe.release(identity);
    }
    let lifted = mob.join_lift(lift).await;
    mob.settle_live().await;

    assert!(reached, "the plan's first {BOUND} restores start");
    for (identity, expected) in [
        (
            worker(0),
            (Some(dsl::MemberMaterializationPhase::Waking), true),
        ),
        (
            worker(1),
            (Some(dsl::MemberMaterializationPhase::Waking), true),
        ),
        (
            worker(2),
            (Some(dsl::MemberMaterializationPhase::Dormant), false),
        ),
        (
            worker(3),
            (Some(dsl::MemberMaterializationPhase::Dormant), false),
        ),
        (
            queued.clone(),
            (Some(dsl::MemberMaterializationPhase::Dormant), false),
        ),
    ] {
        assert_eq!(
            before[&identity], expected,
            "before the message, {identity} is as the bounded plan leaves it"
        );
    }
    assert!(
        matches!(woken, Ok(Ok(Ok(MemberWakeOutcome::Ready)))),
        "the peer message's wake makes the member ready: {woken:?}"
    );
    assert_eq!(
        during[&queued],
        (Some(dsl::MemberMaterializationPhase::Live), false),
        "the member is Live while the plan's wakes are still held"
    );
    for identity in &held {
        assert_eq!(
            during[identity],
            (Some(dsl::MemberMaterializationPhase::Waking), true),
            "{identity}'s plan wake is still held"
        );
    }
    assert_eq!(
        probe
            .started()
            .iter()
            .filter(|identity| **identity == queued)
            .count(),
        1,
        "the plan's later dispatch did not wake the member again"
    );
    for (identity, builds) in mob.builds().await {
        assert_eq!(builds, 1, "{identity} was built once");
    }
    for (identity, (phase, attempt)) in mob.residency() {
        assert_eq!(
            (phase, attempt),
            (Some(dsl::MemberMaterializationPhase::Live), false),
            "{identity} ends Live"
        );
    }
    assert_eq!(
        mob.wake_attempts() - attempts_before,
        MEMBERS as u64,
        "each member woke under exactly one attempt: the plan woke none a second time"
    );
    assert!(lifted.is_ok(), "the lift succeeds: {lifted:?}");
}
