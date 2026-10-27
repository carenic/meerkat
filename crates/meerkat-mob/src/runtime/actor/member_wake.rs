//! Member wake: the runtime path that makes a seated member which is not
//! materialized in this process ready, for every cause.
//!
//! MobMachine owns the member's residency (`member_materialization`:
//! Dormant, Waking, Live) and every verdict about it: whether a wake is
//! authorized, joined, already live or refused (`WakeMember`), how each wake
//! attempt ends (`ResolveMemberRevival*`, `AbandonMemberWake`, fenced by the
//! machine's attempt id), and when a live materialization is released. This
//! module is the shell around those verdicts:
//!
//! - it realizes the machine's residency effects: the inproc wake routes
//!   (`PublishWakeRoute`, `RetractWakeRoute`, `RebuildWakeRoutes`), the
//!   materialization handoff (`MaterializeMember`), the bulk abandonment of a
//!   stopping mob's wakes (`MemberWakesAbandoned`) and a stopped mob's peer
//!   ingress (`CloseMemberPeerIngress`, `ReopenMemberPeerIngress`);
//! - it answers every caller waiting on an attempt, keyed by the attempt id
//!   (it holds no dedupe decision of its own: the machine's `Joined` is the
//!   dedupe);
//! - it publishes the mob's [`MobPeerWaker`], the owner of its wake routes:
//!   a peer send that finds no live inbox for a seated member reaches the
//!   actor through it as `WakeMember(PeerDelivery)`.
//!
//! The wake turn itself (the cold restore recipe off the actor, its plan applied on
//! the actor, readiness, topology, the Ready resolution) is in
//! `member_wake_turn`.

use super::*;

/// MobMachine's wake attempt id (`member_wake_attempts`).
pub(in crate::runtime) type MemberWakeAttempt = u64;

/// How a wake a caller waited on ended well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum MemberWakeOutcome {
    /// The member is ready: live, its trust and drain settled.
    Ready,
    /// The request was a silent lifecycle notice the machine consumed for a
    /// member that is not live, without waking it: its topology at its
    /// next wake carries the notice's content.
    LifecycleNoticeConsumed,
}

/// How a wake a caller waited on did not make the member ready. Cloneable:
/// every caller of one attempt gets its own copy.
#[derive(Debug, Clone)]
pub(in crate::runtime) enum MemberWakeFailure {
    /// The identity is not a member (it left, or never joined).
    NotAMember,
    /// MobMachine refused the wake, or the attempt ended retryably or was
    /// abandoned before the member was ready.
    Refused(crate::MemberWakeRefusal),
    /// The member is Broken: its restore failed, now or earlier.
    Broken {
        session_id: Option<SessionId>,
        reason: String,
        hold: Option<meerkat_core::service::DurableResumeHold>,
        registration_refusal: Option<crate::MemberRegistrationHookCall>,
    },
    /// A peer send from a member that is itself waking, to a member that is
    /// not live: refused instead of waiting (no wait cycle between builds).
    SenderWaking,
}

impl MemberWakeFailure {
    /// The typed error a host delivery gets.
    pub(in crate::runtime) fn into_mob_error(self, member_id: &AgentIdentity) -> MobError {
        match self {
            Self::NotAMember => MobError::MemberNotFound(member_id.clone()),
            Self::Refused(refusal) => MobError::MemberWakeRefused {
                member_id: member_id.clone(),
                refusal,
            },
            Self::Broken {
                session_id,
                reason,
                registration_refusal: Some(call),
                ..
            } => MobError::MemberRegistrationRefused {
                member_id: member_id.clone(),
                session_id,
                call,
                reason,
            },
            Self::Broken {
                session_id,
                reason,
                hold,
                registration_refusal: None,
            } => MobError::MemberRestoreFailed {
                member_id: member_id.clone(),
                session_id,
                reason,
                hold,
            },
            // Only a peer send carries a sender residency; a host delivery
            // never gets this answer.
            Self::SenderWaking => MobError::Internal(format!(
                "a host delivery to '{member_id}' was answered as a peer send from a waking member"
            )),
        }
    }

    /// The same answer on the peer-message path.
    pub(in crate::runtime) fn peer_denial(&self) -> meerkat_comms::PeerWakeDenial {
        use meerkat_comms::PeerWakeDenial;
        use meerkat_core::comms::PeerWakeRefusal;
        match self {
            Self::NotAMember => PeerWakeDenial::NotAMember,
            Self::Refused(refusal) => PeerWakeDenial::Refused(refusal.peer_refusal()),
            Self::Broken { reason, hold, .. } => {
                PeerWakeDenial::Refused(PeerWakeRefusal::MemberBroken {
                    reason: reason.clone(),
                    hold: *hold,
                })
            }
            Self::SenderWaking => PeerWakeDenial::Refused(PeerWakeRefusal::SenderWaking),
        }
    }

    /// The answer for a wake command the actor refused before handling it
    /// (its scope gate, or a closing actor).
    pub(in crate::runtime) fn from_actor_refusal(error: &MobError) -> Self {
        match error {
            MobError::AdmissionClosed { .. } => {
                Self::Refused(crate::MemberWakeRefusal::AdmissionClosed)
            }
            _ => Self::Refused(crate::MemberWakeRefusal::MobUnavailable),
        }
    }
}

/// The answer of one wake attempt.
pub(in crate::runtime) type MemberWakeAnswer = Result<MemberWakeOutcome, MemberWakeFailure>;
/// The reply channel of one wake caller.
pub(in crate::runtime) type MemberWakeReply = oneshot::Sender<MemberWakeAnswer>;

/// What a caller on the actor gets back for a wake it applied.
pub(in crate::runtime) enum MemberWakeAdmission {
    /// The member is already live: deliver now.
    Ready,
    /// A silent lifecycle notice was consumed without a wake.
    LifecycleNoticeConsumed,
    /// The member is being woken: wait for this answer (off the actor).
    Pending(oneshot::Receiver<MemberWakeAnswer>),
    /// Refused, typed.
    Failed(MemberWakeFailure),
}

impl MemberWakeAdmission {
    /// Wait for the wake (off the actor) and fold it into a host delivery's
    /// result.
    pub(in crate::runtime) async fn settled_for_host(
        self,
        member_id: &AgentIdentity,
    ) -> Result<(), MobError> {
        match self {
            Self::Ready | Self::LifecycleNoticeConsumed => Ok(()),
            Self::Failed(failure) => Err(failure.into_mob_error(member_id)),
            Self::Pending(answer) => match answer.await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(failure)) => Err(failure.into_mob_error(member_id)),
                // The actor exited with the wake in flight.
                Err(_) => Err(MobError::MemberWakeRefused {
                    member_id: member_id.clone(),
                    refusal: crate::MemberWakeRefusal::MobUnavailable,
                }),
            },
        }
    }
}

/// The owner of this mob's inproc wake routes.
///
/// A peer send that finds no live inbox for a key with a wake route asks this
/// owner; it forwards the request to the mob actor as `WakeMember` and waits
/// for the actor's typed answer. It decides nothing itself.
pub(in crate::runtime) struct MobPeerWaker {
    command_tx: mpsc::Sender<RoutedMobCommand>,
    machine_state: tokio::sync::watch::Receiver<mob_dsl::MobMachineState>,
}

impl MobPeerWaker {
    fn new(
        command_tx: mpsc::Sender<RoutedMobCommand>,
        machine_state: tokio::sync::watch::Receiver<mob_dsl::MobMachineState>,
    ) -> Self {
        Self {
            command_tx,
            machine_state,
        }
    }
}

impl meerkat_comms::InprocPeerWaker for MobPeerWaker {
    fn wake(&self, request: meerkat_comms::PeerWakeRequest) -> meerkat_comms::PeerWakeFuture {
        use meerkat_comms::{PeerWakeAnswer, PeerWakeDenial};
        use meerkat_core::comms::PeerWakeRefusal;
        let command_tx = self.command_tx.clone();
        Box::pin(async move {
            let (reply_tx, reply_rx) = oneshot::channel();
            let input_class = match request.input_class {
                meerkat_comms::PeerWakeInputClass::Content => mob_dsl::PeerInputClass::Content,
                meerkat_comms::PeerWakeInputClass::Lifecycle => mob_dsl::PeerInputClass::Lifecycle,
            };
            let command = MobCommand::WakeMember {
                agent_identity: AgentIdentity::from(request.member.as_str()),
                cause: mob_dsl::MemberWakeCause::PeerDelivery,
                observed_missing: request.observed_missing,
                input_class,
                sender_waking: request.sender_waking,
                reply_tx,
            };
            if command_tx
                .send(RoutedMobCommand::internal(command))
                .await
                .is_err()
            {
                return Err(PeerWakeDenial::Refused(PeerWakeRefusal::MobUnavailable));
            }
            match reply_rx.await {
                Ok(Ok(MemberWakeOutcome::Ready)) => Ok(PeerWakeAnswer::Ready),
                Ok(Ok(MemberWakeOutcome::LifecycleNoticeConsumed)) => {
                    Ok(PeerWakeAnswer::ControlNoticeConsumed)
                }
                Ok(Err(failure)) => Err(failure.peer_denial()),
                Err(_) => Err(PeerWakeDenial::Refused(PeerWakeRefusal::MobUnavailable)),
            }
        })
    }

    fn member_is_waking(&self, member: &str) -> bool {
        let identity = mob_dsl::AgentIdentity(member.to_string());
        matches!(
            self.machine_state
                .borrow()
                .member_materialization
                .get(&identity),
            Some(mob_dsl::MemberMaterializationPhase::Waking)
        )
    }
}

impl meerkat_core::session_owner_wake::SessionOwnerWaker for MobPeerWaker {
    /// A session-keyed live operation on a member's session: the member
    /// bound to it, per the machine state the actor last published, is woken
    /// as a host delivery wakes it. A session no member is bound to any more
    /// answers `NotOwned`. The caller found no live actor serving the
    /// session, and that observation goes with the wake: a member the
    /// machine still holds Live has lost its materialization, and the
    /// machine's drift verdict decides its wake (re-observed on the actor
    /// first).
    fn wake_session_owner(
        &self,
        session_id: &SessionId,
    ) -> meerkat_core::session_owner_wake::SessionOwnerWakeFuture {
        use meerkat_core::session_owner_wake::SessionOwnerWakeOutcome;
        let wanted = mob_dsl::SessionId::from_domain(session_id);
        let member = self
            .machine_state
            .borrow()
            .member_session_bindings
            .iter()
            .find_map(|(identity, bound)| (bound == &wanted).then(|| identity.0.clone()));
        let command_tx = self.command_tx.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let Some(member) = member else {
                return Ok(SessionOwnerWakeOutcome::NotOwned);
            };
            let identity = AgentIdentity::from(member.as_str());
            let (reply_tx, reply_rx) = oneshot::channel();
            let command = MobCommand::WakeMember {
                agent_identity: identity.clone(),
                cause: mob_dsl::MemberWakeCause::HostDelivery,
                observed_missing: true,
                input_class: mob_dsl::PeerInputClass::Content,
                sender_waking: false,
                reply_tx,
            };
            if command_tx
                .send(RoutedMobCommand::internal(command))
                .await
                .is_err()
            {
                return Err(session_owner_wake_error(
                    &session_id,
                    MobError::MemberWakeRefused {
                        member_id: identity,
                        refusal: crate::MemberWakeRefusal::MobUnavailable,
                    },
                ));
            }
            MemberWakeAdmission::Pending(reply_rx)
                .settled_for_host(&identity)
                .await
                .map(|()| SessionOwnerWakeOutcome::Ready)
                .map_err(|error| session_owner_wake_error(&session_id, error))
        })
    }
}

/// A member wake's typed refusal, as the session service reports it to a
/// session-keyed caller: retryable refusals stay retryable (the session is
/// busy now), the rest name the member's state.
fn session_owner_wake_error(
    session_id: &SessionId,
    error: MobError,
) -> meerkat_core::service::SessionError {
    use meerkat_core::service::SessionError;
    match &error {
        MobError::MemberWakeRefused { refusal, .. } if refusal.is_retryable() => {
            SessionError::Busy {
                id: session_id.clone(),
            }
        }
        _ => SessionError::Agent(meerkat_core::error::AgentError::InternalError(format!(
            "the member owning session {session_id} could not be woken: {error}"
        ))),
    }
}

/// A materialization MobMachine handed off (`MaterializeMember`), started at
/// the actor's next loop boundary.
#[derive(Debug, Clone)]
pub(in crate::runtime) struct PendingMemberMaterialization {
    pub(in crate::runtime) identity: AgentIdentity,
    pub(in crate::runtime) cause: mob_dsl::MemberWakeCause,
    pub(in crate::runtime) attempt: MemberWakeAttempt,
}

/// A stopped mob's peer-ingress change for its live members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum MemberPeerIngressChange {
    Close,
    Reopen,
}

/// One wake this actor runs (its cold restore and the steps after it).
#[derive(Debug, Clone)]
pub(in crate::runtime) struct RunningMemberWake {
    pub(in crate::runtime) identity: AgentIdentity,
    pub(in crate::runtime) cause: mob_dsl::MemberWakeCause,
}

/// Continuity intents by member, caught up from the mob's journal by cursor
/// (the same fold `recorded_continuity_intents` makes over a full replay,
/// without replaying the journal per wake). It starts caught up to the
/// journal the actor's start replayed, so a wake folds only the events
/// appended since.
pub(in crate::runtime) struct MemberContinuityIndex {
    last_cursor: u64,
    recorded: super::super::builder::RecordedContinuity,
}

impl MemberContinuityIndex {
    /// The index caught up to `replayed`, the journal the actor's start
    /// replayed.
    fn seeded(replayed: &[MobEvent], mob_id: &MobId) -> Self {
        let mut index = Self {
            last_cursor: 0,
            recorded: HashMap::new(),
        };
        for event in replayed {
            index.fold(event, mob_id);
        }
        index
    }

    const PAGE: usize = 256;

    /// Fold every journal event after the last cursor seen.
    pub(in crate::runtime) async fn refresh(
        &mut self,
        events: &Arc<dyn MobEventStore>,
        mob_id: &MobId,
    ) -> Result<(), MobError> {
        loop {
            let batch = events.poll(self.last_cursor, Self::PAGE).await?;
            let complete = batch.len() < Self::PAGE;
            for event in &batch {
                self.fold(event, mob_id);
            }
            if complete {
                return Ok(());
            }
        }
    }

    fn fold(&mut self, event: &MobEvent, mob_id: &MobId) {
        self.last_cursor = self.last_cursor.max(event.cursor);
        if &event.mob_id != mob_id {
            return;
        }
        match &event.kind {
            MobEventKind::MobReset => self.recorded.clear(),
            MobEventKind::MemberSpawned(spawned) => {
                self.recorded.insert(
                    spawned.agent_identity.clone(),
                    (spawned.generation, spawned.continuity_intent.clone()),
                );
            }
            MobEventKind::MemberReset {
                agent_identity,
                previous_generation,
                new_generation,
                ..
            } => {
                if let Some((generation, _)) = self.recorded.get_mut(agent_identity)
                    && *generation == *previous_generation
                {
                    *generation = *new_generation;
                }
            }
            _ => {}
        }
    }

    /// The one-member slice a cold restore reads.
    pub(in crate::runtime) fn slice_for(
        &self,
        identity: &AgentIdentity,
    ) -> super::super::builder::RecordedContinuity {
        self.recorded
            .get(identity)
            .map(|recorded| {
                let mut slice = HashMap::new();
                slice.insert(identity.clone(), recorded.clone());
                slice
            })
            .unwrap_or_default()
    }
}

/// The actor's wake custody: everything this module keeps between machine
/// verdicts. None of it is lifecycle truth; MobMachine holds that.
pub(in crate::runtime) struct MemberWakes {
    /// The inproc namespace this mob's members register in: the mob's realm
    /// (`mob.<id>`), the same one `build_agent_config` gives every member.
    namespace: String,
    waker: Arc<MobPeerWaker>,
    /// What this mob published for each member (its inproc wake route and
    /// its session-owner entry), so a retraction removes exactly what the
    /// projection holds.
    routes: BTreeMap<AgentIdentity, PublishedMemberRoutes>,
    /// Callers waiting on each attempt, with the attempt's member.
    waiters: BTreeMap<MemberWakeAttempt, (AgentIdentity, Vec<MemberWakeReply>)>,
    /// Attempts this actor is resolving right now: their callers are
    /// answered by the resolution itself, not by the closed-attempt sweep.
    resolving: BTreeSet<MemberWakeAttempt>,
    /// Attempts the machine closed while callers waited on them, answered
    /// at the next loop boundary from the member's state.
    closed: BTreeMap<MemberWakeAttempt, AgentIdentity>,
    /// Materializations to start at the next loop boundary.
    pub(in crate::runtime) pending: VecDeque<PendingMemberMaterialization>,
    /// Wakes this actor is running, by attempt.
    pub(in crate::runtime) running: BTreeMap<MemberWakeAttempt, RunningMemberWake>,
    /// Peer-ingress changes to realize at the next loop boundary.
    ingress: VecDeque<MemberPeerIngressChange>,
    /// The concrete provisioner the cold restore recipe runs on.
    pub(in crate::runtime) provisioner: Arc<super::super::provisioner::MultiBackendProvisioner>,
    pub(in crate::runtime) continuity: MemberContinuityIndex,
    /// Owed-boot retry state per member: the backoff step, and whether its
    /// one retry timer is armed.
    owed_retries: BTreeMap<AgentIdentity, OwedMemberWakeRetry>,
}

/// One member's owed-boot retry: consecutive retries (the backoff step) and
/// whether a retry timer is armed. A member has at most one armed timer, so
/// two refusals before it fires do not step the backoff twice.
#[derive(Debug, Default)]
struct OwedMemberWakeRetry {
    retries: u32,
    armed: bool,
}

impl MemberWakes {
    /// `replayed` is the journal the actor's start replayed: the continuity
    /// index starts caught up to it.
    pub(in crate::runtime) fn new(
        provisioner: Arc<super::super::provisioner::MultiBackendProvisioner>,
        mob_id: &MobId,
        command_tx: mpsc::Sender<RoutedMobCommand>,
        machine_state: tokio::sync::watch::Receiver<mob_dsl::MobMachineState>,
        replayed: &[MobEvent],
    ) -> Result<Self, MobError> {
        let namespace = meerkat_core::mob_realm_id(mob_id.as_str())
            .map_err(|error| {
                MobError::WiringError(format!(
                    "mob id '{mob_id}' cannot be used as a realm identity: {error}"
                ))
            })?
            .as_str()
            .to_string();
        Ok(Self {
            namespace,
            waker: Arc::new(MobPeerWaker::new(command_tx, machine_state)),
            routes: BTreeMap::new(),
            waiters: BTreeMap::new(),
            resolving: BTreeSet::new(),
            closed: BTreeMap::new(),
            pending: VecDeque::new(),
            running: BTreeMap::new(),
            ingress: VecDeque::new(),
            provisioner,
            continuity: MemberContinuityIndex::seeded(replayed, mob_id),
            owed_retries: BTreeMap::new(),
        })
    }

    fn waker_weak(&self) -> std::sync::Weak<dyn meerkat_comms::InprocPeerWaker> {
        let waker: Arc<dyn meerkat_comms::InprocPeerWaker> = self.waker.clone();
        Arc::downgrade(&waker)
    }

    fn session_owner_weak(
        &self,
    ) -> std::sync::Weak<dyn meerkat_core::session_owner_wake::SessionOwnerWaker> {
        let owner: Arc<dyn meerkat_core::session_owner_wake::SessionOwnerWaker> =
            self.waker.clone();
        Arc::downgrade(&owner)
    }

    /// Wait on `attempt` of `identity`.
    pub(in crate::runtime) fn join(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
    ) -> oneshot::Receiver<MemberWakeAnswer> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.waiters
            .entry(attempt)
            .or_insert_with(|| (identity.clone(), Vec::new()))
            .1
            .push(reply_tx);
        reply_rx
    }

    /// Answer `attempt`'s callers at the next loop boundary, from the
    /// member's state then.
    pub(in crate::runtime) fn closed_answer_later(
        &mut self,
        identity: AgentIdentity,
        attempt: MemberWakeAttempt,
    ) {
        if self.waiters.contains_key(&attempt) {
            self.closed.insert(attempt, identity);
        }
    }

    /// Answer every caller of `attempt`, once.
    pub(in crate::runtime) fn answer(
        &mut self,
        attempt: MemberWakeAttempt,
        answer: MemberWakeAnswer,
    ) {
        self.closed.remove(&attempt);
        if let Some((_, replies)) = self.waiters.remove(&attempt) {
            for reply in replies {
                let _ = reply.send(answer.clone());
            }
        }
    }
}

impl Drop for MemberWakes {
    fn drop(&mut self) {
        // The actor is gone: its routes can wake nothing. Waiters see their
        // channel close (answered `mob_unavailable`); the registries answer
        // the same for an entry whose owner is gone, but retract them anyway
        // so the keys and sessions stop resolving to this mob.
        let registry = meerkat_comms::InprocRegistry::global();
        let owners = meerkat_core::session_owner_wake::SessionOwnerWakeRegistry::global();
        let waker = self.waker_weak();
        let session_owner = self.session_owner_weak();
        for published in self.routes.values() {
            if let Some(pubkey) = &published.pubkey {
                registry.retract_wake_route(&self.namespace, pubkey, &waker);
            }
            owners.retract(&published.session, &session_owner);
        }
    }
}

/// What the wake-route projection published for one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct PublishedMemberRoutes {
    /// Its inproc wake route's key: absent while the member has no durable
    /// endpoint yet (a legacy member before its first wake).
    pubkey: Option<meerkat_comms::PubKey>,
    /// Its bound session, routed to this mob in the session-owner registry.
    session: SessionId,
}

impl MobActor {
    /// Realize MobMachine's member-residency effects. Every actor apply seam
    /// calls this (from `queue_routed_effects_from`). The transitions the
    /// builder applied before the actor existed are not replayed here: the
    /// boot restore realized their materializations itself, and the actor
    /// rebuilds every wake route from machine state when it starts
    /// ([`Self::start_member_wakes`]). Wake routes are a projection of
    /// machine state, so realizing a route effect twice is the same
    /// projection.
    pub(in crate::runtime) fn realize_member_residency_effects(
        &mut self,
        effects: &[mob_dsl::MobMachineEffect],
    ) {
        let mut residency_changed = false;
        for effect in effects {
            match effect {
                mob_dsl::MobMachineEffect::PublishWakeRoute { agent_identity, .. } => {
                    self.publish_member_wake_route(&AgentIdentity::from(agent_identity.0.as_str()));
                }
                mob_dsl::MobMachineEffect::RetractWakeRoute { agent_identity, .. } => {
                    self.retract_member_wake_route(&AgentIdentity::from(agent_identity.0.as_str()));
                }
                mob_dsl::MobMachineEffect::RebuildWakeRoutes { .. } => {
                    self.rebuild_member_wake_routes();
                }
                // A binding that moved (a successor repoint, a recovered
                // binding) moves the member's session-owner entry with it.
                mob_dsl::MobMachineEffect::MemberSessionBindingChanged {
                    agent_identity, ..
                } => {
                    let identity = AgentIdentity::from(agent_identity.0.as_str());
                    if self.member_wakes.routes.contains_key(&identity) {
                        self.publish_member_wake_route(&identity);
                    }
                }
                mob_dsl::MobMachineEffect::MaterializeMember {
                    agent_identity,
                    cause,
                    attempt,
                } => {
                    let identity = AgentIdentity::from(agent_identity.0.as_str());
                    // A placed member's materialization is its host's, and
                    // its revival path realizes its own attempt.
                    if super::super::member_runtime_is_host_owned(
                        self.dsl_authority.state(),
                        &identity,
                    ) {
                        continue;
                    }
                    self.member_wakes
                        .pending
                        .push_back(PendingMemberMaterialization {
                            identity,
                            cause: *cause,
                            attempt: *attempt,
                        });
                }
                mob_dsl::MobMachineEffect::MemberWakesAbandoned { .. } => {
                    residency_changed = true;
                }
                mob_dsl::MobMachineEffect::CloseMemberPeerIngress { .. } => {
                    self.member_wakes
                        .ingress
                        .push_back(MemberPeerIngressChange::Close);
                }
                mob_dsl::MobMachineEffect::ReopenMemberPeerIngress { .. } => {
                    self.member_wakes
                        .ingress
                        .push_back(MemberPeerIngressChange::Reopen);
                }
                mob_dsl::MobMachineEffect::MemberWakeFeedbackResolved { .. }
                | mob_dsl::MobMachineEffect::MemberWakeClassified { .. }
                | mob_dsl::MobMachineEffect::MemberMaterializationReleased { .. } => {
                    residency_changed = true;
                }
                _ => {}
            }
        }
        if residency_changed || !self.member_wakes.waiters.is_empty() {
            self.note_closed_member_wakes();
        }
    }

    /// Note every attempt the machine closed while callers waited on it and
    /// no resolution of this actor is answering them (a stopping mob's bulk
    /// abandonment, a retire start, a lift that ended its own attempt).
    /// They are answered at the next loop boundary, from the member's state.
    fn note_closed_member_wakes(&mut self) {
        let state = self.dsl_authority.state();
        let mut closed = Vec::new();
        for (attempt, (identity, _)) in &self.member_wakes.waiters {
            if self.member_wakes.resolving.contains(attempt) {
                continue;
            }
            let open = state
                .member_wake_attempts
                .get(&mob_dsl::AgentIdentity::from_domain(identity))
                == Some(attempt);
            if !open {
                closed.push((*attempt, identity.clone()));
            }
        }
        self.member_wakes.closed.extend(closed);
    }

    /// Answer the callers of the attempts [`Self::note_closed_member_wakes`]
    /// found closed. Runs at the loop boundary (the Broken answer reads the
    /// restore diagnostics).
    pub(in crate::runtime) async fn answer_closed_member_wakes(&mut self) {
        let closed = std::mem::take(&mut self.member_wakes.closed);
        for (attempt, identity) in closed {
            if !self.member_wakes.waiters.contains_key(&attempt) {
                continue;
            }
            let answer = self.member_wake_answer_from_state(&identity).await;
            self.member_wakes.answer(attempt, answer);
        }
    }

    /// The answer a closed attempt's callers get from the member's state
    /// alone: the closure already happened in the machine (a stop, a retire
    /// start, a resolution of another path), this only tells the callers.
    async fn member_wake_answer_from_state(&self, identity: &AgentIdentity) -> MemberWakeAnswer {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        let (phase, running, retiring) = {
            let state = self.dsl_authority.state();
            (
                state.member_materialization.get(&dsl_identity).cloned(),
                matches!(state.lifecycle_phase, mob_dsl::MobPhase::Running),
                state
                    .identity_to_runtime
                    .get(&dsl_identity)
                    .and_then(|runtime_id| state.member_state_markers.get(runtime_id))
                    == Some(&mob_dsl::MobMemberState::Retiring),
            )
        };
        match phase {
            Some(mob_dsl::MemberMaterializationPhase::Live) => Ok(MemberWakeOutcome::Ready),
            // A retire start closed the attempt.
            Some(_) if retiring => Err(MemberWakeFailure::Refused(
                crate::MemberWakeRefusal::Retiring,
            )),
            _ if !running => Err(MemberWakeFailure::Refused(
                crate::MemberWakeRefusal::MobNotRunning,
            )),
            // Dormant again, or a newer attempt is open: the wake this caller
            // joined ended without making the member ready; a retry wakes it
            // (or joins the newer attempt).
            Some(
                mob_dsl::MemberMaterializationPhase::Dormant
                | mob_dsl::MemberMaterializationPhase::Waking,
            ) => Err(MemberWakeFailure::Refused(
                crate::MemberWakeRefusal::WakeTimedOut,
            )),
            None => Err(self.member_wake_failure_from_state(identity).await),
        }
    }

    // ---------------------------------------------------------------
    // Wake routes (a declared projection of machine membership).
    // ---------------------------------------------------------------

    /// What `identity`'s routes are, read from machine state: a local
    /// session-bound member is routed by its session (session-owner
    /// registry), and by its durable endpoint's key once it has one
    /// (inproc wake route).
    fn member_wake_route_source(&self, identity: &AgentIdentity) -> Option<PublishedMemberRoutes> {
        let state = self.dsl_authority.state();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        if !state.identity_to_runtime.contains_key(&dsl_identity)
            || state.member_placement.contains_key(&dsl_identity)
        {
            return None;
        }
        let session = state
            .member_session_bindings
            .get(&dsl_identity)
            .and_then(|session| SessionId::parse(&session.0).ok())?;
        let pubkey = state
            .member_peer_endpoints
            .get(&dsl_identity)
            .map(|endpoint| meerkat_comms::PubKey(endpoint.signing_key.0))
            .filter(|pubkey| !pubkey.is_zero());
        Some(PublishedMemberRoutes { pubkey, session })
    }

    /// Publish `identity`'s routes as machine state names them now: a key
    /// or a session the member no longer has (an endpoint that migrated, a
    /// binding repointed to a successor) is retracted in the same step.
    pub(in crate::runtime) fn publish_member_wake_route(&mut self, identity: &AgentIdentity) {
        let source = self.member_wake_route_source(identity);
        let previous = self.member_wakes.routes.get(identity).cloned();
        if previous == source {
            return;
        }
        let registry = meerkat_comms::InprocRegistry::global();
        let owners = meerkat_core::session_owner_wake::SessionOwnerWakeRegistry::global();
        let waker = self.member_wakes.waker_weak();
        let session_owner = self.member_wakes.session_owner_weak();
        if let Some(previous) = previous {
            if let Some(pubkey) = previous.pubkey
                && source.as_ref().and_then(|source| source.pubkey) != Some(pubkey)
            {
                registry.retract_wake_route(&self.member_wakes.namespace, &pubkey, &waker);
            }
            if source.as_ref().map(|source| &source.session) != Some(&previous.session) {
                owners.retract(&previous.session, &session_owner);
            }
        }
        match source {
            Some(source) => {
                if let Some(pubkey) = source.pubkey {
                    registry.publish_wake_route(
                        &self.member_wakes.namespace,
                        pubkey,
                        identity.as_str(),
                        waker,
                    );
                }
                owners.publish(source.session.clone(), session_owner);
                self.member_wakes.routes.insert(identity.clone(), source);
            }
            None => {
                self.member_wakes.routes.remove(identity);
            }
        }
    }

    fn retract_member_wake_route(&mut self, identity: &AgentIdentity) {
        if let Some(published) = self.member_wakes.routes.remove(identity) {
            if let Some(pubkey) = &published.pubkey {
                meerkat_comms::InprocRegistry::global().retract_wake_route(
                    &self.member_wakes.namespace,
                    pubkey,
                    &self.member_wakes.waker_weak(),
                );
            }
            meerkat_core::session_owner_wake::SessionOwnerWakeRegistry::global()
                .retract(&published.session, &self.member_wakes.session_owner_weak());
        }
    }

    /// Recompute every wake route from machine state.
    pub(in crate::runtime) fn rebuild_member_wake_routes(&mut self) {
        let seated: Vec<AgentIdentity> = self
            .dsl_authority
            .state()
            .identity_to_runtime
            .keys()
            .map(|identity| AgentIdentity::from(identity.0.as_str()))
            .collect();
        let published: Vec<AgentIdentity> = self.member_wakes.routes.keys().cloned().collect();
        for identity in published {
            if !seated.contains(&identity) {
                self.retract_member_wake_route(&identity);
            }
        }
        for identity in seated {
            self.publish_member_wake_route(&identity);
        }
    }

    // ---------------------------------------------------------------
    // WakeMember: the machine's verdict, and the caller's answer.
    // ---------------------------------------------------------------

    /// Apply `WakeMember` for `identity` and turn its verdict into the
    /// caller's admission. Authorized or joined callers wait on the attempt;
    /// an already-live member is ready now (or, with `observed_missing`, its
    /// missing live materialization is classified and woken, the counted
    /// drift path); a refusal is typed.
    pub(in crate::runtime) async fn apply_member_wake(
        &mut self,
        identity: &AgentIdentity,
        cause: mob_dsl::MemberWakeCause,
        observed_missing: bool,
        input_class: mob_dsl::PeerInputClass,
        sender_waking: bool,
    ) -> MemberWakeAdmission {
        if self.admission_closed.is_some() {
            return MemberWakeAdmission::Failed(MemberWakeFailure::Refused(
                crate::MemberWakeRefusal::AdmissionClosed,
            ));
        }
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        let transition = match self.apply_dsl_input_collect_transition(
            mob_dsl::MobMachineInput::WakeMember {
                agent_identity: dsl_identity.clone(),
                cause,
                observed_missing,
                input_class,
                sender_waking,
            },
            "member_wake",
        ) {
            Ok(transition) => transition,
            Err(error) => {
                // WakeMember is total over the lifecycle phases; a refused
                // transition is a machine fault, never a delivery answer.
                tracing::error!(
                    mob_id = %self.definition.id,
                    agent_identity = %identity,
                    %error,
                    "MobMachine refused WakeMember"
                );
                return MemberWakeAdmission::Failed(MemberWakeFailure::Refused(
                    crate::MemberWakeRefusal::MobUnavailable,
                ));
            }
        };
        let classified = transition.effects().iter().find_map(|effect| match effect {
            mob_dsl::MobMachineEffect::MemberWakeClassified {
                agent_identity,
                verdict,
                refusal,
                attempt,
                ..
            } if agent_identity == &dsl_identity => Some((*verdict, *refusal, *attempt)),
            _ => None,
        });
        let Some((verdict, refusal, attempt)) = classified else {
            tracing::error!(
                mob_id = %self.definition.id,
                agent_identity = %identity,
                "MobMachine accepted WakeMember but emitted no wake verdict"
            );
            return MemberWakeAdmission::Failed(MemberWakeFailure::Refused(
                crate::MemberWakeRefusal::MobUnavailable,
            ));
        };
        match verdict {
            mob_dsl::MemberWakeVerdictKind::Authorized | mob_dsl::MemberWakeVerdictKind::Joined => {
                match attempt {
                    Some(attempt) => {
                        MemberWakeAdmission::Pending(self.member_wakes.join(identity, attempt))
                    }
                    None => {
                        tracing::error!(
                            mob_id = %self.definition.id,
                            agent_identity = %identity,
                            ?verdict,
                            "MobMachine authorized or joined a wake without an attempt"
                        );
                        MemberWakeAdmission::Failed(MemberWakeFailure::Refused(
                            crate::MemberWakeRefusal::MobUnavailable,
                        ))
                    }
                }
            }
            mob_dsl::MemberWakeVerdictKind::AlreadyLive if observed_missing => {
                self.wake_live_member_observed_missing(identity, cause)
                    .await
            }
            mob_dsl::MemberWakeVerdictKind::AlreadyLive => MemberWakeAdmission::Ready,
            mob_dsl::MemberWakeVerdictKind::Refused => match refusal {
                Some(mob_dsl::MemberWakeRefusalKind::SuppressedLifecycleNotice) => {
                    MemberWakeAdmission::LifecycleNoticeConsumed
                }
                Some(kind) => MemberWakeAdmission::Failed(
                    self.member_wake_refusal_failure(identity, kind).await,
                ),
                None => {
                    tracing::error!(
                        mob_id = %self.definition.id,
                        agent_identity = %identity,
                        "MobMachine refused a wake without a refusal kind"
                    );
                    MemberWakeAdmission::Failed(MemberWakeFailure::Refused(
                        crate::MemberWakeRefusal::MobUnavailable,
                    ))
                }
            },
        }
    }

    /// The counted drift path: a routing miss or a liveness probe
    /// observed the live materialization of a member the machine holds Live.
    /// The machine's drift arm moves it to Waking under a new attempt, which
    /// this actor realizes (the drift arm hands nothing off).
    async fn wake_live_member_observed_missing(
        &mut self,
        identity: &AgentIdentity,
        cause: mob_dsl::MemberWakeCause,
    ) -> MemberWakeAdmission {
        // The miss was observed before this command ran (off the actor, or
        // by a sender's route lookup), so it may be stale: a concurrent wake
        // may have served the member since. Observe again here, and a member
        // whose runtime registration serves it with an executor is ready,
        // not drifted.
        if self.member_materialization_served(identity).await {
            return MemberWakeAdmission::Ready;
        }
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        let transition = match self.apply_dsl_signal_collect_transition(
            mob_dsl::MobMachineSignal::ClassifyMemberLiveMaterialization {
                agent_identity: dsl_identity.clone(),
                observation:
                    mob_dsl::MemberLiveMaterializationObservationKind::DurableSnapshotPresent,
                reason: format!(
                    "the live materialization of member '{identity}' was observed missing"
                ),
            },
            "member_wake_observed_missing",
        ) {
            Ok(transition) => transition,
            Err(error) => {
                // A Live member the drift arm does not admit (a pending
                // explicit resume, a lifecycle origin that closed): the
                // member is not woken here; answer by its state.
                tracing::warn!(
                    mob_id = %self.definition.id,
                    agent_identity = %identity,
                    %error,
                    "the drift arm did not admit a missing live materialization"
                );
                return MemberWakeAdmission::Failed(
                    self.member_wake_failure_from_state(identity).await,
                );
            }
        };
        let classified = transition.effects().iter().find_map(|effect| match effect {
            mob_dsl::MobMachineEffect::MemberLiveMaterializationClassified {
                agent_identity,
                verdict,
                attempt,
                ..
            } if agent_identity == &dsl_identity => Some((*verdict, *attempt)),
            _ => None,
        });
        tracing::warn!(
            mob_id = %self.definition.id,
            agent_identity = %identity,
            "member live materialization was missing without a release (counted drift)"
        );
        match classified {
            Some((mob_dsl::MemberRevivalVerdictKind::ReviveAuthorized, Some(attempt))) => {
                // The drift arm hands nothing off itself; should it ever emit
                // the handoff, the apply seam already queued it.
                if !self
                    .member_wakes
                    .pending
                    .iter()
                    .any(|pending| pending.attempt == attempt)
                {
                    self.member_wakes
                        .pending
                        .push_back(PendingMemberMaterialization {
                            identity: identity.clone(),
                            cause,
                            attempt,
                        });
                }
                MemberWakeAdmission::Pending(self.member_wakes.join(identity, attempt))
            }
            Some((mob_dsl::MemberRevivalVerdictKind::BrokenRecorded, _)) => {
                MemberWakeAdmission::Failed(self.member_wake_broken(identity).await)
            }
            _ => MemberWakeAdmission::Failed(MemberWakeFailure::Refused(
                crate::MemberWakeRefusal::MobUnavailable,
            )),
        }
    }

    /// Whether `identity`'s session is served in this process right now: its
    /// runtime registration has an executor attached. The fresh observation
    /// the drift path takes on the actor before it trusts an earlier miss.
    async fn member_materialization_served(&self, identity: &AgentIdentity) -> bool {
        #[cfg(feature = "runtime-adapter")]
        {
            let Some(adapter) = self.runtime_adapter.as_ref() else {
                return false;
            };
            let Some(session_id) = self
                .roster
                .read()
                .await
                .get(identity)
                .and_then(|entry| entry.member_ref.bridge_session_id().cloned())
            else {
                return false;
            };
            adapter
                .session_has_executor(&session_id)
                .await
                .unwrap_or(false)
        }
        #[cfg(not(feature = "runtime-adapter"))]
        {
            let _ = identity;
            false
        }
    }

    /// The typed failure for a machine refusal kind.
    async fn member_wake_refusal_failure(
        &self,
        identity: &AgentIdentity,
        kind: mob_dsl::MemberWakeRefusalKind,
    ) -> MemberWakeFailure {
        use crate::MemberWakeRefusal as Refusal;
        match kind {
            mob_dsl::MemberWakeRefusalKind::Broken => self.member_wake_broken(identity).await,
            mob_dsl::MemberWakeRefusalKind::NotAMember => MemberWakeFailure::NotAMember,
            mob_dsl::MemberWakeRefusalKind::SenderWaking => MemberWakeFailure::SenderWaking,
            mob_dsl::MemberWakeRefusalKind::Retiring => {
                MemberWakeFailure::Refused(Refusal::Retiring)
            }
            mob_dsl::MemberWakeRefusalKind::HostOwned => {
                MemberWakeFailure::Refused(Refusal::HostOwned)
            }
            mob_dsl::MemberWakeRefusalKind::MobNotRunning => {
                MemberWakeFailure::Refused(Refusal::MobNotRunning)
            }
            mob_dsl::MemberWakeRefusalKind::LifecycleOriginFenced => {
                MemberWakeFailure::Refused(Refusal::LifecycleOriginFenced)
            }
            mob_dsl::MemberWakeRefusalKind::NonAuthoritativeComposition => {
                MemberWakeFailure::Refused(Refusal::NonAuthoritativeComposition)
            }
            mob_dsl::MemberWakeRefusalKind::HostPreparationPending => {
                MemberWakeFailure::Refused(Refusal::HostPreparationPending)
            }
            mob_dsl::MemberWakeRefusalKind::ResumeGateUnavailable => {
                MemberWakeFailure::Refused(Refusal::ResumeGateUnavailable)
            }
            // A notice the machine consumed is not a failure; the caller
            // never reaches here with it.
            mob_dsl::MemberWakeRefusalKind::SuppressedLifecycleNotice => {
                MemberWakeFailure::Refused(Refusal::MobUnavailable)
            }
        }
    }

    /// The Broken answer for `identity`: the machine's recorded reason, with
    /// the restore diagnostic's session, durable hold and host refusal.
    pub(in crate::runtime) async fn member_wake_broken(
        &self,
        identity: &AgentIdentity,
    ) -> MemberWakeFailure {
        let recorded = self
            .dsl_authority
            .state()
            .member_restore_failures
            .get(&mob_dsl::AgentIdentity::from_domain(identity))
            .cloned();
        let diagnostic = self.restore_failure_for(identity).await;
        let registration_refusal = self
            .dsl_authority
            .state()
            .member_registration_refusal_for_identity(&mob_dsl::AgentIdentity::from_domain(
                identity,
            ));
        MemberWakeFailure::Broken {
            session_id: diagnostic
                .as_ref()
                .and_then(|diagnostic| diagnostic.bridge_session_id.clone()),
            reason: recorded
                .or_else(|| {
                    diagnostic
                        .as_ref()
                        .map(|diagnostic| diagnostic.reason.clone())
                })
                .unwrap_or_else(|| "member restore failed".to_string()),
            hold: diagnostic.as_ref().and_then(|diagnostic| diagnostic.hold),
            registration_refusal,
        }
    }

    /// The answer by the member's state alone (no verdict to read).
    pub(in crate::runtime) async fn member_wake_failure_from_state(
        &self,
        identity: &AgentIdentity,
    ) -> MemberWakeFailure {
        let state = self.dsl_authority.state();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        if state.member_restore_failures.contains_key(&dsl_identity) {
            return self.member_wake_broken(identity).await;
        }
        if !state.identity_to_runtime.contains_key(&dsl_identity) {
            return MemberWakeFailure::NotAMember;
        }
        // A retirement start closes the member's wake and removes its phase:
        // the member is retiring, which is the answer.
        if state
            .identity_to_runtime
            .get(&dsl_identity)
            .and_then(|runtime_id| state.member_state_markers.get(runtime_id))
            == Some(&mob_dsl::MobMemberState::Retiring)
        {
            return MemberWakeFailure::Refused(crate::MemberWakeRefusal::Retiring);
        }
        if !matches!(state.lifecycle_phase, mob_dsl::MobPhase::Running) {
            return MemberWakeFailure::Refused(crate::MemberWakeRefusal::MobNotRunning);
        }
        MemberWakeFailure::Refused(crate::MemberWakeRefusal::LifecycleOriginFenced)
    }

    /// The `WakeMember` command: a peer send's wake route, or any caller that
    /// is not on the actor. The reply waits off the actor.
    pub(in crate::runtime) async fn handle_wake_member_command(
        &mut self,
        identity: AgentIdentity,
        cause: mob_dsl::MemberWakeCause,
        observed_missing: bool,
        input_class: mob_dsl::PeerInputClass,
        sender_waking: bool,
        reply_tx: MemberWakeReply,
    ) {
        match self
            .apply_member_wake(
                &identity,
                cause,
                observed_missing,
                input_class,
                sender_waking,
            )
            .await
        {
            MemberWakeAdmission::Ready => {
                let _ = reply_tx.send(Ok(MemberWakeOutcome::Ready));
            }
            MemberWakeAdmission::LifecycleNoticeConsumed => {
                let _ = reply_tx.send(Ok(MemberWakeOutcome::LifecycleNoticeConsumed));
            }
            MemberWakeAdmission::Failed(failure) => {
                let _ = reply_tx.send(Err(failure));
            }
            MemberWakeAdmission::Pending(answer) => {
                self.actor_io_tasks.spawn(async move {
                    let answer = answer.await.unwrap_or(Err(MemberWakeFailure::Refused(
                        crate::MemberWakeRefusal::MobUnavailable,
                    )));
                    let _ = reply_tx.send(answer);
                });
            }
        }
    }

    // ---------------------------------------------------------------
    // Attempt custody for the explicit-resume lift and the turn.
    // ---------------------------------------------------------------

    /// Wait on `attempt` of `identity` (an explicit resume that joined it).
    pub(in crate::runtime) fn join_member_wake(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
    ) -> oneshot::Receiver<MemberWakeAnswer> {
        self.member_wakes.join(identity, attempt)
    }

    /// Answer every caller that joined `attempt` of `identity`, which its
    /// realizer (the lift's own rebuild) settled.
    pub(in crate::runtime) fn settle_member_wake(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
        outcome: Result<(), MemberWakeFailure>,
    ) {
        let _ = identity;
        self.member_wakes
            .answer(attempt, outcome.map(|()| MemberWakeOutcome::Ready));
    }

    // ---------------------------------------------------------------
    // Stopped-mob peer ingress, at the loop boundary.
    // ---------------------------------------------------------------

    /// Close or reopen the live members' inproc routes for peer ingress, as
    /// a stop closed or a resume reopened it. A closed route makes a peer
    /// send meet the member's wake route, whose `WakeMember` the stopped
    /// machine answers `mob_not_running`, the same outcome a dormant member
    /// of a stopped mob gives.
    pub(in crate::runtime) async fn realize_member_peer_ingress_changes(&mut self) {
        while let Some(change) = self.member_wakes.ingress.pop_front() {
            let live: Vec<RosterEntry> = {
                let state = self.dsl_authority.state();
                let roster = self.roster.read().await;
                roster
                    .list()
                    .filter(|entry| {
                        matches!(
                            state
                                .member_materialization
                                .get(&mob_dsl::AgentIdentity::from_domain(&entry.agent_identity)),
                            Some(mob_dsl::MemberMaterializationPhase::Live)
                        )
                    })
                    .cloned()
                    .collect()
            };
            for entry in live {
                let Some(comms) = self.provisioner.comms_runtime(&entry.member_ref).await else {
                    continue;
                };
                let changed = match change {
                    MemberPeerIngressChange::Close => comms.close_inproc_route(),
                    MemberPeerIngressChange::Reopen => comms.reopen_inproc_route(),
                };
                tracing::debug!(
                    mob_id = %self.definition.id,
                    agent_identity = %entry.agent_identity,
                    ?change,
                    changed,
                    "realized a member's peer ingress change"
                );
            }
        }
    }

    // ---------------------------------------------------------------
    // The wake bound and the owed-boot retry.
    // ---------------------------------------------------------------

    /// Arm the mob's wake bound for one attempt: when it ends, the machine
    /// resolves the attempt retryable (a no-op when it already ended).
    pub(in crate::runtime) fn arm_member_wake_bound(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
    ) {
        let bound = self.member_residency.wake_timeout;
        let command_tx = self.command_tx.clone();
        let agent_identity = identity.clone();
        self.actor_io_tasks.spawn(async move {
            tokio::time::sleep(bound).await;
            let _ = command_tx
                .send(RoutedMobCommand::internal(MobCommand::MemberWakeTimedOut {
                    agent_identity,
                    attempt,
                }))
                .await;
        });
    }

    /// The wake bound of `attempt` ended.
    pub(in crate::runtime) async fn member_wake_timed_out(
        &mut self,
        identity: AgentIdentity,
        attempt: MemberWakeAttempt,
    ) {
        self.resolve_member_wake_retryable(
            &identity,
            attempt,
            mob_dsl::MemberWakeRetryableKind::WakeTimedOut,
            crate::MemberWakeRefusal::WakeTimedOut,
            "member_wake_bound",
        )
        .await;
    }

    /// Resolve `attempt` retryable (Waking -> Dormant) and answer its
    /// callers with `refusal`. Stale feedback (the attempt already ended) is
    /// a no-op by the machine's own verdict.
    pub(in crate::runtime) async fn resolve_member_wake_retryable(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
        kind: mob_dsl::MemberWakeRetryableKind,
        refusal: crate::MemberWakeRefusal,
        context: &'static str,
    ) -> mob_dsl::MemberWakeFeedbackVerdict {
        let verdict = self.apply_member_wake_feedback(
            identity,
            attempt,
            mob_dsl::MobMachineSignal::ResolveMemberRevivalRetryable {
                agent_identity: mob_dsl::AgentIdentity::from_domain(identity),
                attempt,
                kind,
            },
            context,
        );
        if verdict == mob_dsl::MemberWakeFeedbackVerdict::Applied {
            self.member_wakes.running.remove(&attempt);
            self.member_wakes
                .answer(attempt, Err(MemberWakeFailure::Refused(refusal)));
            self.schedule_owed_member_wake_retry(identity);
        }
        verdict
    }

    /// Apply one attempt-fenced wake feedback signal and read the machine's
    /// verdict. A transition the machine refuses is recorded as a fault and
    /// read as stale: wake feedback never fail-stops the mob.
    pub(in crate::runtime) fn apply_member_wake_feedback(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
        signal: mob_dsl::MobMachineSignal,
        context: &'static str,
    ) -> mob_dsl::MemberWakeFeedbackVerdict {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        // This resolution answers the attempt's callers itself.
        self.member_wakes.resolving.insert(attempt);
        let applied = self.apply_dsl_signal_collect_transition(signal, context);
        self.member_wakes.resolving.remove(&attempt);
        match applied {
            Ok(transition) => transition
                .effects()
                .iter()
                .find_map(|effect| match effect {
                    mob_dsl::MobMachineEffect::MemberWakeFeedbackResolved {
                        agent_identity,
                        attempt: resolved,
                        verdict,
                    } if agent_identity == &dsl_identity && *resolved == attempt => Some(*verdict),
                    _ => None,
                })
                .unwrap_or(mob_dsl::MemberWakeFeedbackVerdict::StaleAttempt),
            Err(error) => {
                tracing::error!(
                    mob_id = %self.definition.id,
                    agent_identity = %identity,
                    attempt,
                    context,
                    %error,
                    "MobMachine refused wake feedback; the attempt is treated as stale"
                );
                // A feedback the machine has no arm for (a false
                // precondition) never leaves its attempt open: it is closed
                // NotCurrent, so the member is Dormant and its next delivery
                // wakes it again. Should that close be refused too, the
                // attempt's bound still closes it.
                if self.member_wake_attempt_is_open(identity, attempt) {
                    self.member_wakes.resolving.insert(attempt);
                    let closed = self.apply_dsl_signal_collect_transition(
                        mob_dsl::MobMachineSignal::AbandonMemberWake {
                            agent_identity: dsl_identity,
                            attempt,
                            kind: mob_dsl::MemberWakeAbandonKind::NotCurrent,
                            reason: format!("{context} was refused: {error}"),
                        },
                        "member_wake_refused_feedback_closed",
                    );
                    self.member_wakes.resolving.remove(&attempt);
                    if let Err(close_error) = closed {
                        tracing::error!(
                            mob_id = %self.definition.id,
                            agent_identity = %identity,
                            attempt,
                            %close_error,
                            "a refused wake feedback's attempt could not be closed; its bound closes it"
                        );
                    }
                }
                mob_dsl::MemberWakeFeedbackVerdict::StaleAttempt
            }
        }
    }

    /// Schedule the next owed-boot retry for `identity` when the machine
    /// still owes its boot work and holds it Dormant. Backoff doubles from
    /// the configured initial delay up to the configured cap.
    pub(in crate::runtime) fn schedule_owed_member_wake_retry(&mut self, identity: &AgentIdentity) {
        let state = self.dsl_authority.state();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        let owed = state
            .member_boot_obligation_owed
            .contains_key(&dsl_identity);
        let dormant = matches!(
            state.member_materialization.get(&dsl_identity),
            Some(mob_dsl::MemberMaterializationPhase::Dormant)
        );
        if !owed || !dormant {
            // Nothing to retry now; the backoff restarts at the next owed
            // refusal. An armed timer stays the member's one timer.
            if let std::collections::btree_map::Entry::Occupied(mut entry) =
                self.member_wakes.owed_retries.entry(identity.clone())
            {
                if entry.get().armed {
                    entry.get_mut().retries = 0;
                } else {
                    entry.remove();
                }
            }
            return;
        }
        let backoff = self.member_residency.owed_wake_retry;
        let retry = self
            .member_wakes
            .owed_retries
            .entry(identity.clone())
            .or_default();
        if retry.armed {
            // Its timer is already armed: the retry it fires covers this one.
            return;
        }
        let delay = backoff.delay(retry.retries);
        retry.retries = retry.retries.saturating_add(1);
        retry.armed = true;
        let command_tx = self.command_tx.clone();
        let agent_identity = identity.clone();
        self.actor_io_tasks.spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = command_tx
                .send(RoutedMobCommand::internal(
                    MobCommand::RetryOwedMemberWake { agent_identity },
                ))
                .await;
        });
    }

    /// Schedule a retry for every member whose boot work is still owed and
    /// that is Dormant (a boot wake the builder resolved retryable). Called
    /// once at actor start.
    pub(in crate::runtime) fn schedule_owed_member_wake_retries(&mut self) {
        let owed: Vec<AgentIdentity> = self
            .dsl_authority
            .state()
            .member_boot_obligation_owed
            .keys()
            .map(|identity| AgentIdentity::from(identity.0.as_str()))
            .collect();
        for identity in owed {
            self.schedule_owed_member_wake_retry(&identity);
        }
    }

    /// An owed-boot retry is due: wake the member for its boot work again,
    /// unless its obligation is gone or a delivery already woke it.
    pub(in crate::runtime) async fn retry_owed_member_wake(&mut self, identity: AgentIdentity) {
        // This is the member's armed timer firing.
        if let Some(retry) = self.member_wakes.owed_retries.get_mut(&identity) {
            retry.armed = false;
        }
        let state = self.dsl_authority.state();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&identity);
        let owed = state
            .member_boot_obligation_owed
            .contains_key(&dsl_identity);
        let dormant = matches!(
            state.member_materialization.get(&dsl_identity),
            Some(mob_dsl::MemberMaterializationPhase::Dormant)
        );
        if !owed {
            self.member_wakes.owed_retries.remove(&identity);
            return;
        }
        if !dormant {
            // Waking: a delivery joined or superseded the retry. Live: done.
            return;
        }
        match self
            .apply_member_wake(
                &identity,
                mob_dsl::MemberWakeCause::Boot,
                false,
                mob_dsl::PeerInputClass::Content,
                false,
            )
            .await
        {
            MemberWakeAdmission::Pending(_) | MemberWakeAdmission::Ready => {}
            MemberWakeAdmission::LifecycleNoticeConsumed => {}
            MemberWakeAdmission::Failed(failure) => {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    agent_identity = %identity,
                    ?failure,
                    "an owed boot wake was refused"
                );
                // A retryable refusal (host preparation pending) keeps the
                // member owed: try again later.
                if matches!(
                    &failure,
                    MemberWakeFailure::Refused(refusal) if refusal.is_retryable()
                ) {
                    self.schedule_owed_member_wake_retry(&identity);
                }
            }
        }
    }

    /// Answer the callers of a running wake the machine closed under it, by
    /// the member's state, and forget the run.
    pub(in crate::runtime) async fn finish_closed_member_wake(
        &mut self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
    ) {
        self.member_wakes.running.remove(&attempt);
        if !self.member_wakes.waiters.contains_key(&attempt) {
            return;
        }
        let answer = self.member_wake_answer_from_state(identity).await;
        self.member_wakes.answer(attempt, answer);
    }

    /// Without the runtime adapter there is no cold restore: a handed-off
    /// materialization ends Broken with that reason, typed.
    #[cfg(not(feature = "runtime-adapter"))]
    pub(in crate::runtime) async fn start_pending_member_materializations(&mut self) {
        while let Some(pending) = self.member_wakes.pending.pop_front() {
            let reason = "on-demand member restore requires the runtime adapter".to_string();
            let verdict = self.apply_member_wake_feedback(
                &pending.identity,
                pending.attempt,
                mob_dsl::MobMachineSignal::ResolveMemberRevivalFailed {
                    agent_identity: mob_dsl::AgentIdentity::from_domain(&pending.identity),
                    reason: reason.clone(),
                    attempt: pending.attempt,
                },
                "member_wake_without_runtime_adapter",
            );
            if verdict == mob_dsl::MemberWakeFeedbackVerdict::Applied {
                let failure = self.member_wake_broken(&pending.identity).await;
                self.member_wakes.answer(pending.attempt, Err(failure));
            } else {
                self.finish_closed_member_wake(&pending.identity, pending.attempt)
                    .await;
            }
        }
    }

    /// Actor start: publish every seated member's wake route from machine
    /// state (`RebuildWakeRoutes` at actor start), and schedule the retry of
    /// every boot wake that ended retryably while its obligation is owed.
    pub(in crate::runtime) fn start_member_wakes(&mut self) {
        self.rebuild_member_wake_routes();
        self.schedule_owed_member_wake_retries();
    }

    /// A missing-live revival request (a host's liveness probe, a reload's
    /// re-attachment, a start's recovery of a member it left out): applied
    /// as `WakeMember` with the observed miss and answered in the revival
    /// vocabulary. The wake runs the cold restore recipe like every other wake.
    pub(in crate::runtime) async fn revive_missing_member_through_wake(
        &mut self,
        identity: AgentIdentity,
        reply_tx: oneshot::Sender<Result<MemberLiveRevivalOutcome, MobError>>,
    ) {
        match self
            .apply_member_wake(
                &identity,
                mob_dsl::MemberWakeCause::Operator,
                true,
                mob_dsl::PeerInputClass::Content,
                false,
            )
            .await
        {
            MemberWakeAdmission::Ready | MemberWakeAdmission::LifecycleNoticeConsumed => {
                let _ = reply_tx.send(Ok(MemberLiveRevivalOutcome::AlreadyLive));
            }
            MemberWakeAdmission::Failed(failure) => {
                let _ = reply_tx.send(Err(failure.into_mob_error(&identity)));
            }
            MemberWakeAdmission::Pending(answer) => {
                self.actor_io_tasks.spawn(async move {
                    let result = MemberWakeAdmission::Pending(answer)
                        .settled_for_host(&identity)
                        .await
                        .map(|()| MemberLiveRevivalOutcome::Materialized(None));
                    let _ = reply_tx.send(result);
                });
            }
        }
    }

    /// Wake `agent_identity` from off the actor and wait for the answer: the
    /// actor applies `WakeMember` for the cause (with the observed miss, the
    /// counted drift path for a member MobMachine holds Live) and answers
    /// once the member is ready, or with the typed refusal.
    pub(in crate::runtime) async fn request_member_wake(
        command_tx: &mpsc::Sender<RoutedMobCommand>,
        agent_identity: &AgentIdentity,
        cause: mob_dsl::MemberWakeCause,
        observed_missing: bool,
    ) -> Result<(), MobError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        command_tx
            .send(RoutedMobCommand::internal(MobCommand::WakeMember {
                agent_identity: agent_identity.clone(),
                cause,
                observed_missing,
                input_class: mob_dsl::PeerInputClass::Content,
                sender_waking: false,
                reply_tx,
            }))
            .await
            .map_err(|_| MobError::ActorCommandChannelClosed)?;
        MemberWakeAdmission::Pending(reply_rx)
            .settled_for_host(agent_identity)
            .await
    }

    // ---------------------------------------------------------------
    // Make dormant (`MobHandle::release_member`).
    // ---------------------------------------------------------------

    /// Release `identity`'s live materialization: its runtime registration is
    /// closed while it is quiescent (no run, no queued input), and the
    /// runtime's unregister publishes the release that MobMachine applies
    /// (`ReleaseMemberMaterialization`, Live -> Dormant, fenced by the
    /// registration's epoch). The member stays seated, at the same session,
    /// generation and fence, and every delivery wakes it again.
    ///
    /// A member that is not materialized here (Dormant, or Broken) has
    /// nothing to release. One being woken is refused retryably: its fresh
    /// registration is the wake's. So is one whose registration's binding
    /// has not reached MobMachine yet, and one with work in flight; nothing
    /// is closed or interrupted.
    pub(in crate::runtime) async fn handle_release_member(
        &mut self,
        identity: AgentIdentity,
        reply_tx: oneshot::Sender<Result<(), MobError>>,
    ) {
        match self.prepare_member_release(&identity).await {
            Err(error) => {
                let _ = reply_tx.send(Err(error));
            }
            Ok(None) => {
                let _ = reply_tx.send(Ok(()));
            }
            #[cfg(feature = "runtime-adapter")]
            Ok(Some((adapter, session_id))) => {
                self.actor_io_tasks.spawn(async move {
                    let result = release_member_registration(adapter, session_id, &identity).await;
                    let _ = reply_tx.send(result);
                });
            }
            #[cfg(not(feature = "runtime-adapter"))]
            Ok(Some(never)) => match never {},
        }
    }

    /// The release, as MobMachine classifies it (`ClassifyMemberRelease`),
    /// realized on the actor: the member's local session and its runtime
    /// owner when it releases, `None` when nothing is materialized.
    async fn prepare_member_release(
        &mut self,
        identity: &AgentIdentity,
    ) -> Result<Option<MemberReleaseTarget>, MobError> {
        let verdict = self.classify_member_release(identity)?;
        match verdict {
            mob_dsl::MemberReleaseVerdict::Release | mob_dsl::MemberReleaseVerdict::HostOwned => {}
            mob_dsl::MemberReleaseVerdict::NotAMember => {
                return Err(MobError::MemberNotFound(identity.clone()));
            }
            mob_dsl::MemberReleaseVerdict::WakeInFlight => {
                return Err(MobError::LifecycleOperationPending {
                    intent: format!("member wake of '{identity}'"),
                });
            }
            // Live, but MobMachine would not honour the release of its
            // registration yet (the routed binding has not recorded the
            // registration's epoch): nothing is closed, and a retry releases
            // it once the binding landed.
            mob_dsl::MemberReleaseVerdict::NotYetReleasable => {
                return Err(MobError::LifecycleOperationPending {
                    intent: format!("runtime binding of member '{identity}'"),
                });
            }
            // Wired to a placed member, which reaches it only while it is
            // live: it stays live, closing nothing, and a retry answers the
            // same until that wiring is gone.
            mob_dsl::MemberReleaseVerdict::PinnedByPlacedPeer => {
                let mode = self
                    .roster
                    .read()
                    .await
                    .get(identity)
                    .map(|entry| entry.runtime_mode)
                    .ok_or_else(|| MobError::MemberNotFound(identity.clone()))?;
                return Err(MobError::UnsupportedForMode {
                    mode,
                    reason: "a member wired to a placed member stays live while that wiring exists"
                        .to_string(),
                });
            }
            // Dormant, or not materialized at all (Broken, retiring): nothing
            // is materialized here to release.
            mob_dsl::MemberReleaseVerdict::NothingMaterialized => return Ok(None),
        }
        let entry = self
            .roster
            .read()
            .await
            .get(identity)
            .cloned()
            .ok_or_else(|| MobError::MemberNotFound(identity.clone()))?;
        if verdict == mob_dsl::MemberReleaseVerdict::HostOwned {
            return Err(MobError::UnsupportedForMode {
                mode: entry.runtime_mode,
                reason: "a placed member's materialization is owned by its member host".to_string(),
            });
        }
        let member_ref = self.machine_member_ref_for_behavior(&entry, "release member")?;
        let session_id = member_ref.bridge_session_id().cloned().ok_or_else(|| {
            MobError::UnsupportedForMode {
                mode: entry.runtime_mode,
                reason: "release requires a session-backed member".to_string(),
            }
        })?;
        #[cfg(feature = "runtime-adapter")]
        {
            let adapter =
                self.runtime_adapter
                    .clone()
                    .ok_or_else(|| MobError::UnsupportedForMode {
                        mode: entry.runtime_mode,
                        reason: "release requires a runtime-backed member".to_string(),
                    })?;
            Ok(Some((adapter, session_id)))
        }
        #[cfg(not(feature = "runtime-adapter"))]
        {
            let _ = session_id;
            Err(MobError::UnsupportedForMode {
                mode: entry.runtime_mode,
                reason: "release requires a runtime-backed member".to_string(),
            })
        }
    }

    // ---------------------------------------------------------------
    // Peer-notice dispositions: MobMachine's verdict, realized.
    // ---------------------------------------------------------------

    /// MobMachine's disposition of one lifecycle or kickoff notice from
    /// `sender` to `recipient` (`ClassifyPeerNotice`, a pure verdict legal in
    /// every phase). A notice to or from a member that is not live is not
    /// sent: its content reaches the member through the trust its wake
    /// realizes from machine state.
    pub(in crate::runtime) fn classify_peer_notice(
        &mut self,
        recipient: &AgentIdentity,
        sender: &AgentIdentity,
        notice_class: mob_dsl::PeerInputClass,
    ) -> Result<mob_dsl::PeerNoticeDisposition, MobError> {
        let dsl_recipient = mob_dsl::AgentIdentity::from_domain(recipient);
        let dsl_sender = mob_dsl::AgentIdentity::from_domain(sender);
        let transition = self.apply_dsl_input_collect_transition(
            mob_dsl::MobMachineInput::ClassifyPeerNotice {
                recipient: dsl_recipient.clone(),
                sender: dsl_sender.clone(),
                notice_class,
            },
            "classify_peer_notice",
        )?;
        transition
            .effects()
            .iter()
            .find_map(|effect| match effect {
                mob_dsl::MobMachineEffect::PeerNoticeClassified {
                    recipient,
                    sender,
                    notice_class: classified,
                    disposition,
                } if recipient == &dsl_recipient
                    && sender == &dsl_sender
                    && *classified == notice_class =>
                {
                    Some(*disposition)
                }
                _ => None,
            })
            .ok_or_else(|| {
                MobError::Internal(format!(
                    "MobMachine classified no notice from '{sender}' to '{recipient}'"
                ))
            })
    }

    /// [`Self::classify_peer_notice`], answered as what the caller does with
    /// the notice.
    pub(in crate::runtime) fn realize_peer_notice_disposition(
        &mut self,
        recipient: &AgentIdentity,
        sender: &AgentIdentity,
        notice_class: mob_dsl::PeerInputClass,
    ) -> Result<PeerNoticeRealization, MobError> {
        Ok(
            match self.classify_peer_notice(recipient, sender, notice_class)? {
                mob_dsl::PeerNoticeDisposition::Deliver => PeerNoticeRealization::Deliver,
                mob_dsl::PeerNoticeDisposition::Suppressed => PeerNoticeRealization::Suppressed,
            },
        )
    }

    /// MobMachine's release verdict for `identity` (`ClassifyMemberRelease`,
    /// a pure verdict legal in every phase the actor serves).
    fn classify_member_release(
        &mut self,
        identity: &AgentIdentity,
    ) -> Result<mob_dsl::MemberReleaseVerdict, MobError> {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        let transition = self.apply_dsl_input_collect_transition(
            mob_dsl::MobMachineInput::ClassifyMemberRelease {
                agent_identity: dsl_identity.clone(),
            },
            "classify_member_release",
        )?;
        transition
            .effects()
            .iter()
            .find_map(|effect| match effect {
                mob_dsl::MobMachineEffect::MemberReleaseClassified {
                    agent_identity,
                    verdict,
                } if agent_identity == &dsl_identity => Some(*verdict),
                _ => None,
            })
            .ok_or_else(|| {
                MobError::Internal(format!(
                    "MobMachine classified no release of member '{identity}'"
                ))
            })
    }

    /// Whether `attempt` is still `identity`'s open wake attempt.
    pub(in crate::runtime) fn member_wake_attempt_is_open(
        &self,
        identity: &AgentIdentity,
        attempt: MemberWakeAttempt,
    ) -> bool {
        self.dsl_authority
            .state()
            .member_wake_attempts
            .get(&mob_dsl::AgentIdentity::from_domain(identity))
            == Some(&attempt)
    }
}

/// What a notice sender does with one notice, per MobMachine's disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum PeerNoticeRealization {
    /// Send it.
    Deliver,
    /// Do not send it: the recipient's (or the sender's) wake realizes its
    /// content from machine state.
    Suppressed,
}

/// The runtime owner and session a release closes.
#[cfg(feature = "runtime-adapter")]
type MemberReleaseTarget = (Arc<meerkat_runtime::MeerkatMachine>, SessionId);
/// Without the runtime adapter nothing is released.
#[cfg(not(feature = "runtime-adapter"))]
type MemberReleaseTarget = std::convert::Infallible;

/// Close the member's current registration while it is quiescent. The
/// runtime's unregister saga emits the release that reaches MobMachine.
#[cfg(feature = "runtime-adapter")]
async fn release_member_registration(
    adapter: Arc<meerkat_runtime::MeerkatMachine>,
    session_id: SessionId,
    identity: &AgentIdentity,
) -> Result<(), MobError> {
    let Some(registration) = adapter
        .current_session_registration_witness(&session_id)
        .await
    else {
        // Nothing is registered: nothing is materialized to release.
        return Ok(());
    };
    match adapter
        .unregister_session_registration_until_terminal_if_quiescent(&registration)
        .await
    {
        Ok(meerkat_runtime::RuntimeSessionQuiescentClose::Busy) => {
            Err(MobError::LifecycleOperationPending {
                intent: format!("release of member '{identity}': its session has work in flight"),
            })
        }
        // Closed, or already closing, or replaced meanwhile: the release the
        // close publishes reaches MobMachine (a replaced registration is a
        // newer materialization, which a later release closes).
        Ok(_) => Ok(()),
        Err(error) => Err(MobError::Internal(format!(
            "release of member '{identity}' could not close its registration: {error}"
        ))),
    }
}

/// Test seam: a member-effect commit that applies an operator wake, so the
/// wake is queued from a completion rather than from a command.
#[cfg(test)]
struct WakeFromEffectForTest {
    identity: AgentIdentity,
}

#[cfg(test)]
impl super::member_effect_lane::MemberEffectCommit for WakeFromEffectForTest {
    fn commit(
        self: Box<Self>,
        actor: &mut MobActor,
        _settlement: super::member_effect_lane::MemberEffectSettlement,
    ) -> super::member_effect_lane::MemberEffectCommitFuture<'_> {
        Box::pin(async move {
            let _ = actor
                .apply_member_wake(
                    &self.identity,
                    mob_dsl::MemberWakeCause::Operator,
                    false,
                    mob_dsl::PeerInputClass::Content,
                    false,
                )
                .await;
            super::member_effect_lane::MemberEffectAck::Settled
        })
    }
}

#[cfg(test)]
impl MobActor {
    /// Dispatch a member effect whose completion applies an operator wake
    /// of `identity` (see [`MobCommand::WakeMemberFromEffectForTest`]).
    pub(in crate::runtime) fn dispatch_wake_member_from_effect_for_test(
        &mut self,
        identity: AgentIdentity,
    ) {
        let commit_identity = identity.clone();
        let _ = self.dispatch_member_effect(super::member_effect_lane::MemberEffectRequest {
            context: "wake_member_from_effect_for_test",
            members: Vec::new(),
            effects: Box::pin(async move {
                Box::new(WakeFromEffectForTest {
                    identity: commit_identity,
                }) as Box<dyn super::member_effect_lane::MemberEffectCommit>
            }),
            unsettled_commit: Box::new(WakeFromEffectForTest { identity }),
        });
    }
}

#[cfg(test)]
mod continuity_index_tests {
    use super::*;
    use crate::event::{MemberSpawnedEvent, NewMobEvent};
    use crate::ids::{FenceToken, Generation, ProfileName};
    use crate::store::InMemoryMobEventStore;

    fn spawned(identity: &AgentIdentity, generation: Generation) -> MobEventKind {
        let mut spawned = MemberSpawnedEvent::new(
            identity.clone(),
            generation,
            FenceToken::new(1),
            crate::ids::AgentRuntimeId::new(identity.clone(), generation),
            ProfileName::from("worker"),
        );
        spawned.continuity_intent = crate::runtime::SpawnContinuityIntent::DurableIdentity {
            continuity_key: identity.as_str().to_string(),
        };
        MobEventKind::MemberSpawned(spawned)
    }

    /// The index an actor start seeds from its replay holds what the full
    /// journal fold holds, at the replay's last cursor, so a wake after the
    /// start folds only the events appended since (here a reset carrying a
    /// member's spawn to its next generation).
    #[tokio::test]
    async fn a_seeded_continuity_index_folds_only_the_events_appended_after_the_start() {
        let mob_id = MobId::from("continuity-seed");
        let first = AgentIdentity::from("worker-1");
        let second = AgentIdentity::from("worker-2");
        let store: Arc<dyn MobEventStore> = Arc::new(InMemoryMobEventStore::new());
        for kind in [
            spawned(&first, Generation::new(0)),
            spawned(&second, Generation::new(0)),
        ] {
            store
                .append(NewMobEvent::for_test(mob_id.clone(), kind))
                .await
                .expect("append a spawn");
        }
        let replayed = store.replay_all().await.expect("replay the journal");
        let mut index = MemberContinuityIndex::seeded(&replayed, &mob_id);
        assert_eq!(
            index.last_cursor,
            store.latest_cursor().await.expect("latest cursor"),
            "the seeded index starts at the replay's last cursor"
        );
        assert_eq!(
            index.recorded,
            crate::runtime::builder::recorded_continuity_intents(&replayed),
            "the seed holds what the full journal fold holds"
        );

        store
            .append(NewMobEvent::for_test(
                mob_id.clone(),
                MobEventKind::MemberReset {
                    agent_identity: first.clone(),
                    previous_generation: Generation::new(0),
                    new_generation: Generation::new(1),
                    fence_token: FenceToken::new(2),
                    agent_runtime_id: crate::ids::AgentRuntimeId::new(
                        first.clone(),
                        Generation::new(1),
                    ),
                },
            ))
            .await
            .expect("append a reset");
        index
            .refresh(&store, &mob_id)
            .await
            .expect("refresh the index");
        let journal = store.replay_all().await.expect("replay the journal");
        assert_eq!(
            index.recorded,
            crate::runtime::builder::recorded_continuity_intents(&journal),
            "the refresh folds the appended reset"
        );
        assert_eq!(
            index
                .slice_for(&first)
                .get(&first)
                .map(|(generation, _)| *generation),
            Some(Generation::new(1))
        );
        assert_eq!(
            index.last_cursor,
            store.latest_cursor().await.expect("latest cursor")
        );
    }
}
