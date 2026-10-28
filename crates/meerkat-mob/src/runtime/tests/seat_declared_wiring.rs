//! Seated members get the edges their definition declares, as a spawn of
//! each does. One seat batch wires the members it seats to each other and to
//! the members seated before it, without waking anyone; the members trust
//! each other once woken; a member on network comms is joined by the machine
//! edge alone until its boot wake registers its endpoint and installs the
//! trust of both ends; and a rotation's next incarnation is wired again.
//! These rows run on the combined candidate: the machine, the wake path and
//! the boot path together.

use super::*;
use crate::machines::mob_machine as dsl;
use crate::runtime::DormantSeatOutcome;
use crate::runtime::actor::member_wake_turn::probe_member_wake_restores_for_test;

const STEP: Duration = Duration::from_secs(60);

/// The sample definition with a `member` and a `helper` profile (each a
/// worker otherwise) that `role_wiring` pairs.
fn member_helper_definition() -> MobDefinition {
    let mut definition = sample_definition();
    let worker = definition
        .profiles
        .get(&ProfileName::from("worker"))
        .cloned()
        .expect("the worker profile");
    definition
        .profiles
        .insert(ProfileName::from("member"), worker.clone());
    definition
        .profiles
        .insert(ProfileName::from("helper"), worker);
    definition.wiring.role_wiring = vec![RoleWiringRule {
        a: ProfileName::from("member"),
        b: ProfileName::from("helper"),
    }];
    definition
}

/// A fresh turn-driven spec of `profile`: seated, it owes no kickoff.
fn spec(profile: &str, identity: &AgentIdentity) -> SpawnMemberSpec {
    let mut spec = SpawnMemberSpec::new(profile, identity.clone());
    spec.runtime_mode = Some(crate::MobRuntimeMode::TurnDriven);
    spec
}

/// An unordered identity pair, as MobMachine orders an edge.
fn pair(a: &AgentIdentity, b: &AgentIdentity) -> (String, String) {
    let (a, b) = (a.to_string(), b.to_string());
    if a <= b { (a, b) } else { (b, a) }
}

/// A running mob over the mock durable session service. Its seats record
/// each member's endpoint from its session-scoped comms key, or, on network
/// comms, record none.
struct WiringMob {
    _keys: tempfile::TempDir,
    handle: MobHandle,
    service: Arc<MockSessionService>,
    events: Arc<dyn MobEventStore>,
}

impl WiringMob {
    async fn new(label: &str, definition: MobDefinition, network_comms: bool) -> Self {
        let keys = tempfile::tempdir().expect("a comms identity root");
        let definition = with_unique_mob_id(definition, label);
        let service = Arc::new(MockSessionService::new());
        let _adapter = service.enable_runtime_adapter();
        let storage = MobStorage::in_memory();
        let events = storage.events.clone();
        let builder = MobBuilder::new(definition, storage).with_session_service(service.clone());
        let builder = if network_comms {
            builder.with_network_member_comms()
        } else {
            builder.with_member_comms_identity_root(keys.path())
        };
        let handle = builder.create().await.expect("create the mob");
        Self {
            _keys: keys,
            handle,
            service,
            events,
        }
    }

    /// Seat `specs` in one call; every one is seated.
    async fn seat(&self, specs: Vec<SpawnMemberSpec>) {
        let outcomes = tokio::time::timeout(STEP, self.handle.seat_dormant_members(specs))
            .await
            .expect("the seat answers")
            .expect("the seat commits");
        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(outcome, DormantSeatOutcome::Seated { .. })),
            "every spec is seated: {outcomes:?}"
        );
    }

    /// MobMachine's member edges.
    fn edges(&self) -> BTreeSet<(String, String)> {
        self.handle
            .machine_state_watch_rx
            .borrow()
            .wiring_edges
            .iter()
            .map(|edge| (edge.a.0.clone(), edge.b.0.clone()))
            .collect()
    }

    /// The journal's member wiring rows, in order.
    async fn wired_rows(&self) -> Vec<(String, String)> {
        self.events
            .replay_all()
            .await
            .expect("read the journal")
            .into_iter()
            .flat_map(|event| match event.kind {
                MobEventKind::MembersWired { a, b } => vec![pair(&a, &b)],
                MobEventKind::MembersWiredBatch { edges } => {
                    edges.iter().map(|edge| pair(&edge.a, &edge.b)).collect()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    fn phase(&self, identity: &AgentIdentity) -> Option<dsl::MemberMaterializationPhase> {
        self.handle.member_materialization_for_test(identity)
    }

    /// The members the roster projects `identity` wired to.
    async fn wired_to(&self, identity: &AgentIdentity) -> BTreeSet<AgentIdentity> {
        self.handle
            .get_member(identity)
            .await
            .expect("read the roster")
            .expect("a member")
            .wired_to
    }

    /// The peer id MobMachine records for `identity`'s endpoint.
    fn recorded_peer_id(&self, identity: &AgentIdentity) -> Option<String> {
        self.handle
            .machine_state_watch_rx
            .borrow()
            .member_peer_ids
            .get(&dsl::AgentIdentity::from_domain(identity))
            .map(|peer_id| peer_id.0.clone())
    }

    /// The peer ids `identity`'s comms runtime trusts; none while it has no
    /// comms runtime.
    async fn trusted(&self, identity: &AgentIdentity) -> BTreeSet<String> {
        let Some(session) = self.handle.resolve_bridge_session_id(identity).await else {
            return BTreeSet::new();
        };
        let Some(comms) = self.service.comms_runtime(&session).await else {
            return BTreeSet::new();
        };
        comms
            .peers()
            .await
            .into_iter()
            .map(|peer| peer.peer_id.to_string())
            .collect()
    }

    /// Whether `a` and `b` are live and each trusts the other's recorded
    /// endpoint.
    async fn trust_each_other(&self, a: &AgentIdentity, b: &AgentIdentity) -> bool {
        let live = Some(dsl::MemberMaterializationPhase::Live);
        if self.phase(a) != live || self.phase(b) != live {
            return false;
        }
        let (Some(a_peer), Some(b_peer)) = (self.recorded_peer_id(a), self.recorded_peer_id(b))
        else {
            return false;
        };
        self.trusted(a).await.contains(&b_peer) && self.trusted(b).await.contains(&a_peer)
    }

    async fn wake(&self, identity: &AgentIdentity) {
        tokio::time::timeout(STEP, self.handle.ensure_member_live(identity))
            .await
            .expect("the wake answers")
            .expect("the member wakes");
    }
}

/// One seat batch of a member and a helper under `role_wiring` records
/// their declared edge in MobMachine and one MembersWired row, as spawning
/// them would, and wakes neither: both stay Dormant and nothing is built. A
/// re-sent seat answers both AlreadySeated and wires nothing again, so an
/// edge the host unwired stays unwired.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_seat_batch_wires_a_member_and_a_helper_and_wakes_neither() {
    let mob = WiringMob::new("seat-wiring-batch", member_helper_definition(), false).await;
    let member = AgentIdentity::from("seat-member");
    let helper = AgentIdentity::from("seat-helper");
    mob.seat(vec![spec("member", &member), spec("helper", &helper)])
        .await;

    assert_eq!(
        mob.edges(),
        BTreeSet::from([pair(&member, &helper)]),
        "the declared edge is MobMachine's"
    );
    assert_eq!(
        mob.wired_rows().await,
        vec![pair(&member, &helper)],
        "one MembersWired row"
    );
    assert_eq!(
        mob.wired_to(&member).await,
        BTreeSet::from([helper.clone()])
    );
    assert_eq!(
        mob.wired_to(&helper).await,
        BTreeSet::from([member.clone()])
    );
    for identity in [&member, &helper] {
        assert_eq!(
            mob.phase(identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity} stays Dormant"
        );
    }
    assert!(
        mob.service.recorded_create_requests().await.is_empty(),
        "the wiring builds nothing"
    );
    assert_eq!(mob.service.active_session_count().await, 0);

    mob.handle
        .unwire(member.clone(), helper.clone())
        .await
        .expect("the host unwires the pair");
    let again = mob
        .handle
        .seat_dormant_members(vec![spec("member", &member), spec("helper", &helper)])
        .await
        .expect("the re-sent seat answers");
    assert!(
        again
            .iter()
            .all(|outcome| matches!(outcome, DormantSeatOutcome::AlreadySeated { .. })),
        "{again:?}"
    );
    assert!(
        mob.edges().is_empty(),
        "an already-seated pair is not wired again"
    );
    assert_eq!(mob.wired_rows().await.len(), 1);
}

/// Seated members that wake trust each other: each one's wake realizes its
/// own trust from MobMachine's declared edge, so once both are live the
/// member trusts the helper's endpoint and the helper the member's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn woken_seated_members_trust_each_other() {
    let mob = WiringMob::new("seat-wiring-woken", member_helper_definition(), false).await;
    let member = AgentIdentity::from("seat-member");
    let helper = AgentIdentity::from("seat-helper");
    mob.seat(vec![spec("member", &member), spec("helper", &helper)])
        .await;
    mob.wake(&member).await;
    mob.wake(&helper).await;

    assert!(
        mob.trust_each_other(&member, &helper).await,
        "the woken members trust each other: the member trusts {:?}, the helper {:?}",
        mob.trusted(&member).await,
        mob.trusted(&helper).await
    );
    assert_eq!(mob.edges(), BTreeSet::from([pair(&member, &helper)]));
    assert_eq!(mob.wired_rows().await, vec![pair(&member, &helper)]);
}

/// A helper seated in a later batch is wired to the member seated before
/// it, as a spawn of the helper would be; the member seated alone had no
/// declared peer to wire to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_helper_seated_later_is_wired_to_the_seated_member() {
    let mob = WiringMob::new("seat-wiring-later", member_helper_definition(), false).await;
    let member = AgentIdentity::from("seat-member");
    let helper = AgentIdentity::from("seat-helper");
    mob.seat(vec![spec("member", &member)]).await;
    assert!(mob.edges().is_empty(), "no declared peer is seated yet");

    mob.seat(vec![spec("helper", &helper)]).await;
    assert_eq!(mob.edges(), BTreeSet::from([pair(&member, &helper)]));
    assert_eq!(mob.wired_rows().await, vec![pair(&member, &helper)]);
    for identity in [&member, &helper] {
        assert_eq!(
            mob.phase(identity),
            Some(dsl::MemberMaterializationPhase::Dormant),
            "{identity} stays Dormant"
        );
    }
}

/// Under `auto_wire_orchestrator`, a seated worker is wired to the
/// orchestrator exactly as a spawned worker is: the edge and its row, the
/// roster's projection on both ends, and the live orchestrator's trust in
/// the worker, here in its recorded endpoint while the worker stays Dormant.
/// The worker's wake then has it trust the orchestrator.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seated_worker_is_wired_to_the_orchestrator_as_a_spawned_one_is() {
    let mob = WiringMob::new(
        "seat-wiring-orchestrator",
        sample_definition_with_auto_wire(),
        false,
    )
    .await;
    let lead = AgentIdentity::from("seat-lead");
    let spawned = AgentIdentity::from("seat-spawned");
    let seated = AgentIdentity::from("seat-seated");
    mob.handle
        .spawn_spec(spec("lead", &lead))
        .await
        .expect("spawn the orchestrator");
    mob.handle
        .spawn_spec(spec("worker", &spawned))
        .await
        .expect("spawn a worker");
    assert_eq!(
        mob.wired_rows().await,
        vec![pair(&spawned, &lead)],
        "the spawn wires its worker to the orchestrator"
    );
    let spawned_peer = mob
        .recorded_peer_id(&spawned)
        .expect("the spawned worker's endpoint");
    assert!(mob.trusted(&lead).await.contains(&spawned_peer));

    mob.seat(vec![spec("worker", &seated)]).await;
    assert_eq!(
        mob.edges(),
        BTreeSet::from([pair(&spawned, &lead), pair(&seated, &lead)]),
        "the seat wires its worker to the orchestrator as the spawn did"
    );
    assert_eq!(
        mob.wired_rows().await,
        vec![pair(&spawned, &lead), pair(&seated, &lead)]
    );
    assert_eq!(mob.wired_to(&seated).await, BTreeSet::from([lead.clone()]));
    assert!(mob.wired_to(&lead).await.contains(&seated));
    assert_eq!(
        mob.phase(&seated),
        Some(dsl::MemberMaterializationPhase::Dormant),
        "the seated worker stays Dormant"
    );
    let seated_peer = mob
        .recorded_peer_id(&seated)
        .expect("the seat recorded the worker's endpoint");
    assert!(
        mob.trusted(&lead).await.contains(&seated_peer),
        "the live orchestrator trusts the seated worker's recorded endpoint"
    );

    mob.wake(&seated).await;
    let lead_peer = mob
        .recorded_peer_id(&lead)
        .expect("the orchestrator's endpoint");
    assert!(
        mob.trusted(&seated).await.contains(&lead_peer),
        "the woken worker trusts the orchestrator"
    );
}

/// On network comms a seat records no endpoint: the member and the helper
/// are seated owed theirs, and their declared edge is MobMachine's alone,
/// with its row and no trust on either end (neither has an endpoint to
/// trust, nor a comms runtime). Their boot wakes register their endpoints
/// and realize the edge: once both are live, each trusts the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_network_seat_pair_is_wired_and_trusts_each_other_after_its_boot_wakes() {
    let mob = WiringMob::new("seat-wiring-network", member_helper_definition(), true).await;
    let member = AgentIdentity::from("seat-member");
    let helper = AgentIdentity::from("seat-helper");
    let probe =
        probe_member_wake_restores_for_test(mob.handle.mob_id(), [member.clone(), helper.clone()]);
    mob.seat(vec![spec("member", &member), spec("helper", &helper)])
        .await;

    assert_eq!(mob.edges(), BTreeSet::from([pair(&member, &helper)]));
    assert_eq!(mob.wired_rows().await, vec![pair(&member, &helper)]);
    // Both boot wakes start, and are held at their restore.
    tokio::time::timeout(STEP, probe.wait_for_started(2))
        .await
        .expect("both boot wakes start");
    for identity in [&member, &helper] {
        assert_eq!(
            mob.recorded_peer_id(identity),
            None,
            "{identity} has no endpoint before its wake"
        );
        assert!(
            mob.trusted(identity).await.is_empty(),
            "{identity} trusts nothing before its wake"
        );
    }

    probe.release(&member);
    probe.release(&helper);
    let deadline = tokio::time::Instant::now() + STEP;
    while !mob.trust_each_other(&member, &helper).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the woken pair never trusted each other: member {:?} trusts {:?}, helper {:?} trusts {:?}",
            mob.phase(&member),
            mob.trusted(&member).await,
            mob.phase(&helper),
            mob.trusted(&helper).await
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(mob.edges(), BTreeSet::from([pair(&member, &helper)]));
}

/// A rotation's next incarnation holds none of its predecessor's edges
/// (CommitMemberRotation ends them): it is wired to the edges its definition
/// declares, as a spawn of it would be, journaled again, and the live helper
/// trusts the new incarnation's recorded endpoint while it stays Dormant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rotated_member_is_wired_again_to_its_declared_edges() {
    let mob = WiringMob::new("seat-wiring-rotation", member_helper_definition(), false).await;
    let member = AgentIdentity::from("seat-member");
    let helper = AgentIdentity::from("seat-helper");
    mob.handle
        .spawn_spec(spec("helper", &helper))
        .await
        .expect("spawn the helper");
    mob.handle
        .spawn_spec(spec("member", &member))
        .await
        .expect("spawn the member");
    assert_eq!(mob.edges(), BTreeSet::from([pair(&member, &helper)]));
    let old_peer = mob
        .recorded_peer_id(&member)
        .expect("the member's endpoint");

    tokio::time::timeout(STEP, mob.handle.rotate_member(spec("member", &member)))
        .await
        .expect("the rotation answers")
        .expect("the rotation succeeds");
    assert_eq!(
        mob.edges(),
        BTreeSet::from([pair(&member, &helper)]),
        "the next incarnation is wired to its declared edge"
    );
    assert_eq!(
        mob.wired_rows().await,
        vec![pair(&member, &helper), pair(&member, &helper)],
        "the rotation journals the edge again"
    );
    assert_eq!(
        mob.phase(&member),
        Some(dsl::MemberMaterializationPhase::Dormant)
    );
    let new_peer = mob
        .recorded_peer_id(&member)
        .expect("the rotation recorded the next incarnation's endpoint");
    assert_ne!(new_peer, old_peer, "the next incarnation has its own key");
    assert!(
        mob.trusted(&helper).await.contains(&new_peer),
        "the live helper trusts the next incarnation's recorded endpoint"
    );
}
