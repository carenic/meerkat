//! Member wake (B6): the runtime path that makes a seated member which is not
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
//! The wake turn itself (B4's cold recipe off the actor, its plan applied on
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
    /// member that is not live, without waking it (R4): its topology at its
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

/// A materialization MobMachine handed off (`MaterializeMember`), started at
/// the actor's next loop boundary.
#[derive(Debug, Clone)]
pub(in crate::runtime) struct PendingMemberMaterialization {
    pub(in crate::runtime) identity: AgentIdentity,
    pub(in crate::runtime) cause: mob_dsl::MemberWakeCause,
    pub(in crate::runtime) attempt: MemberWakeAttempt,
}

/// A stopped mob's peer-ingress change for its live members (R9).
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
/// without replaying the journal per wake).
pub(in crate::runtime) struct MemberContinuityIndex {
    last_cursor: u64,
    recorded: super::super::builder::RecordedContinuity,
}

impl MemberContinuityIndex {
    fn new() -> Self {
        Self {
            last_cursor: 0,
            recorded: HashMap::new(),
        }
    }

    const PAGE: usize = 256;

    /// Fold every journal event after the last cursor seen.
    async fn refresh(
        &mut self,
        events: &Arc<dyn MobEventStore>,
        mob_id: &MobId,
    ) -> Result<(), MobError> {
        loop {
            let batch = events.poll(self.last_cursor, Self::PAGE).await?;
            let complete = batch.len() < Self::PAGE;
            for event in batch {
                self.last_cursor = event.cursor;
                if &event.mob_id != mob_id {
                    continue;
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
            if complete {
                return Ok(());
            }
        }
    }

    /// The one-member slice a cold restore reads.
    fn slice_for(&self, identity: &AgentIdentity) -> super::super::builder::RecordedContinuity {
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
    /// The key of each wake route this mob published, by member, so a
    /// retraction removes exactly the route the projection holds.
    routes: BTreeMap<AgentIdentity, meerkat_comms::PubKey>,
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
    /// The concrete provisioner B4's cold recipe runs on.
    pub(in crate::runtime) provisioner: Arc<super::super::provisioner::MultiBackendProvisioner>,
    pub(in crate::runtime) continuity: MemberContinuityIndex,
    /// Consecutive owed-boot retries per member (backoff state).
    owed_retries: BTreeMap<AgentIdentity, u32>,
}

impl MemberWakes {
    pub(in crate::runtime) fn new(
        provisioner: Arc<super::super::provisioner::MultiBackendProvisioner>,
        mob_id: &MobId,
        command_tx: mpsc::Sender<RoutedMobCommand>,
        machine_state: tokio::sync::watch::Receiver<mob_dsl::MobMachineState>,
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
            continuity: MemberContinuityIndex::new(),
            owed_retries: BTreeMap::new(),
        })
    }

    fn waker_weak(&self) -> std::sync::Weak<dyn meerkat_comms::InprocPeerWaker> {
        let waker: Arc<dyn meerkat_comms::InprocPeerWaker> = self.waker.clone();
        Arc::downgrade(&waker)
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
        // channel close (answered `mob_unavailable`); the registry answers
        // the same for a route whose owner is gone, but retract them anyway
        // so the keys stop resolving to this mob.
        let registry = meerkat_comms::InprocRegistry::global();
        let waker = self.waker_weak();
        for pubkey in self.routes.values() {
            registry.retract_wake_route(&self.namespace, pubkey, &waker);
        }
    }
}

impl MobActor {
    /// Realize MobMachine's member-residency effects. Every apply seam calls
    /// this (from `queue_routed_effects_from`); the builder hands the effects
    /// of the transitions it applied before the actor existed to it once, at
    /// actor start. Wake routes are rebuilt from machine state, so realizing
    /// a route effect twice is the same projection.
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
    /// alone.
    async fn member_wake_answer_from_state(&self, identity: &AgentIdentity) -> MemberWakeAnswer {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        let (phase, running) = {
            let state = self.dsl_authority.state();
            (
                state.member_materialization.get(&dsl_identity).cloned(),
                matches!(state.lifecycle_phase, mob_dsl::MobPhase::Running),
            )
        };
        match phase {
            Some(mob_dsl::MemberMaterializationPhase::Live) => Ok(MemberWakeOutcome::Ready),
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
    // Wake routes (a declared projection of machine membership, G7).
    // ---------------------------------------------------------------

    /// The key and session of `identity`'s wake route, read from machine
    /// state: a local session-bound member with a durable endpoint.
    fn member_wake_route_source(&self, identity: &AgentIdentity) -> Option<meerkat_comms::PubKey> {
        let state = self.dsl_authority.state();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        if !state.identity_to_runtime.contains_key(&dsl_identity)
            || !state.member_session_bindings.contains_key(&dsl_identity)
            || state.member_placement.contains_key(&dsl_identity)
        {
            return None;
        }
        let endpoint = state.member_peer_endpoints.get(&dsl_identity)?;
        let pubkey = meerkat_comms::PubKey(endpoint.signing_key.0);
        (!pubkey.is_zero()).then_some(pubkey)
    }

    pub(in crate::runtime) fn publish_member_wake_route(&mut self, identity: &AgentIdentity) {
        let registry = meerkat_comms::InprocRegistry::global();
        let waker = self.member_wakes.waker_weak();
        let source = self.member_wake_route_source(identity);
        let previous = self.member_wakes.routes.get(identity).copied();
        if let Some(previous) = previous
            && Some(previous) != source
        {
            // The member's endpoint migrated: the old key is no longer its.
            registry.retract_wake_route(&self.member_wakes.namespace, &previous, &waker);
            self.member_wakes.routes.remove(identity);
        }
        if let Some(pubkey) = source {
            registry.publish_wake_route(
                &self.member_wakes.namespace,
                pubkey,
                identity.as_str(),
                waker,
            );
            self.member_wakes.routes.insert(identity.clone(), pubkey);
        }
    }

    fn retract_member_wake_route(&mut self, identity: &AgentIdentity) {
        if let Some(pubkey) = self.member_wakes.routes.remove(identity) {
            meerkat_comms::InprocRegistry::global().retract_wake_route(
                &self.member_wakes.namespace,
                &pubkey,
                &self.member_wakes.waker_weak(),
            );
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

    /// The counted drift path (G1): a routing miss or a liveness probe
    /// observed the live materialization of a member the machine holds Live.
    /// The machine's drift arm moves it to Waking under a new attempt, which
    /// this actor realizes (the drift arm hands nothing off).
    async fn wake_live_member_observed_missing(
        &mut self,
        identity: &AgentIdentity,
        cause: mob_dsl::MemberWakeCause,
    ) -> MemberWakeAdmission {
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
                self.member_wakes
                    .pending
                    .push_back(PendingMemberMaterialization {
                        identity: identity.clone(),
                        cause,
                        attempt,
                    });
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
    async fn member_wake_failure_from_state(&self, identity: &AgentIdentity) -> MemberWakeFailure {
        let state = self.dsl_authority.state();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        if state.member_restore_failures.contains_key(&dsl_identity) {
            return self.member_wake_broken(identity).await;
        }
        if !state.identity_to_runtime.contains_key(&dsl_identity) {
            return MemberWakeFailure::NotAMember;
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
    // Attempt custody for the explicit-resume lift (C2) and the turn.
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
    // Stopped-mob peer ingress (R9), at the loop boundary.
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
    // The wake bound (G11) and the owed-boot retry (lead decision b).
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
    /// read as stale: wake feedback never fail-stops the mob (F4).
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
            self.member_wakes.owed_retries.remove(identity);
            return;
        }
        let backoff = self.member_residency.owed_wake_retry;
        let retries = self
            .member_wakes
            .owed_retries
            .entry(identity.clone())
            .or_insert(0);
        let delay = backoff.delay(*retries);
        *retries = retries.saturating_add(1);
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
