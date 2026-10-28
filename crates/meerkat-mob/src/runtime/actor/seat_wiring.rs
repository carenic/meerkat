//! The edges a mob definition declares, wired for seated members.
//!
//! A seat (`MobActor::commit_dormant_seats`) and a rotation's next
//! incarnation (`MobActor::commit_member_rotation`) wire their member to the
//! members a spawn of it would be wired to (`MobActor::declared_wiring_targets`:
//! under `auto_wire_orchestrator`, a member to the orchestrator's members, and
//! an orchestrator member to every member that auto-wires to it; and the
//! members of every profile a `role_wiring` rule pairs with its own). An
//! identity that was seated already, or that a spawn embodied, is wired by
//! nothing here: it holds the edges it holds.
//!
//! The edges are staged on the seat step's own prepared authority, after its
//! seats, so the members seated in one chunk see each other: their lifecycle
//! notice verdicts, their `WireMembers` inputs and, for an edge with a live
//! end, its `AuthorizeMemberTrustWiring`, with their `MembersWired` rows in
//! the step's one `append_batch` beside its seats' rows, before its one
//! base-checked commit. A seat is therefore never durable without its
//! declared edges, and a store failure fails the seats and their edges
//! together. The step stays linear in the chunk's edges: the new edges come
//! from one pass over MobMachine's active members, and a refused batch of
//! edges leaves the prepared authority as it was and is applied again one
//! edge at a time, so an edge MobMachine refuses is left out alone.
//!
//! Every such edge joins a member that is not materialized (the seated
//! member, whose wake can only start after this step), so MobMachine
//! suppresses its lifecycle notices, and trust is installed only on a live
//! end, toward the other member's durable endpoint, once the step committed.
//! A member that is not materialized gets its own trust from its wake's
//! topology step, from the machine's edges. A member seated with its endpoint
//! owed (`EndpointPending`, on network comms) is joined by the machine edge and
//! its row alone, with no trust on either end: its boot wake registers the
//! endpoint and its topology step realizes the trust of both ends, as for the
//! edges a restart replays. A live end that cannot install its trust leaves
//! the edge owed a repair, which the owner retries. An edge with a placed or
//! peer-only member takes the single-edge wire path (`MobActor::handle_wire`)
//! once the step committed, as a spawn's edges do.

use super::member_wake::MemberWiringEdge;
use super::*;

/// The context the machine inputs and the logs of the seat wiring name.
const SEAT_WIRING: &str = "seat_declared_wiring";

/// How one member joins the declared edges of a seat.
enum DeclaredEdgeEnd {
    /// Materialized in this process: it trusts its counterpart now.
    Live(Arc<dyn CoreCommsRuntime>),
    /// Not materialized, with its durable endpoint: a live counterpart
    /// trusts that endpoint.
    Dormant(TrustedPeerDescriptor),
    /// Not materialized, its endpoint owed (`EndpointPending`): the machine
    /// edge alone, whose trust its boot wake realizes.
    EndpointPending,
    /// Placed, peer-only, or not resolvable here: the single-edge path.
    Single,
}

/// The trust a live end of a declared edge installs toward its counterpart
/// that is not materialized.
struct LiveEndTrust {
    comms: Arc<dyn CoreCommsRuntime>,
    counterpart: AgentIdentity,
    counterpart_spec: TrustedPeerDescriptor,
}

/// One declared edge of a seat's batch.
struct BatchedDeclaredEdge {
    edge: mob_dsl::WiringEdge,
    /// The seated member, as a spawn of it is the local end of its edges.
    local: AgentIdentity,
    peer: AgentIdentity,
    trust: Option<LiveEndTrust>,
}

/// A declared edge applied on the prepared authority, waiting for the
/// commit: its `WireMembers` transition, and its trust authorization when it
/// has a live end.
struct AppliedDeclaredEdge {
    batched: BatchedDeclaredEdge,
    wired: Option<mob_dsl::MobMachineTransition>,
    authorized: Option<mob_dsl::MobMachineTransition>,
}

/// The declared edges of the members one seat step seats, staged on that
/// step's prepared authority: what is realized once the step committed.
pub(super) struct StagedDeclaredEdges {
    /// Whether a staged transition moved MobMachine's lifecycle phase.
    pub(super) phase_changed: bool,
    /// Each staged edge with a live end, with the trust authorization the
    /// step applied for it.
    trust: Vec<(BatchedDeclaredEdge, Option<mob_dsl::MobMachineTransition>)>,
    /// The edges the single-edge wire path wires once the step committed.
    single: Vec<(AgentIdentity, AgentIdentity)>,
}

impl StagedDeclaredEdges {
    fn none() -> Self {
        Self {
            phase_changed: false,
            trust: Vec::new(),
            single: Vec::new(),
        }
    }
}

impl MobActor {
    /// Stage the edges the definition declares for the members a seat step
    /// seats (`seated`, in order, each with the endpoint its seat records,
    /// `None` when it is owed) on the step's prepared `authority`, after its
    /// seats (see the module docs). Their effects go to `effects` and their
    /// `MembersWired` rows to `journal`, which the step commits and appends
    /// with its seats. Nothing here fails the step: an edge MobMachine
    /// refuses is logged and left out.
    pub(super) async fn stage_seated_declared_edges(
        &self,
        authority: &mut mob_dsl::MobMachinePreparedAuthority,
        seated: &[(AgentIdentity, Option<TrustedPeerDescriptor>)],
        effects: &mut Vec<mob_dsl::MobMachineEffect>,
        journal: &mut Vec<NewMobEvent>,
    ) -> StagedDeclaredEdges {
        if seated.is_empty() || !Self::definition_declares_wiring(&self.definition) {
            return StagedDeclaredEdges::none();
        }
        if authority.state().lifecycle_phase != mob_dsl::MobPhase::Running {
            tracing::warn!(
                mob_id = %self.definition.id,
                "the declared edges of newly seated members are not wired: the mob is not running"
            );
            return StagedDeclaredEdges::none();
        }
        let members = seated
            .iter()
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>();
        let pairs = self.seated_declared_pairs(authority.state(), &members);
        if pairs.is_empty() {
            return StagedDeclaredEdges::none();
        }
        // A member this step seats joins from the endpoint its seat records:
        // it is not committed yet, so neither the roster nor the committed
        // machine state holds it (a rotation's identity holds its retiring
        // incarnation there). Every other member's end is resolved once for
        // all its edges.
        let mut ends: BTreeMap<AgentIdentity, DeclaredEdgeEnd> = seated
            .iter()
            .map(|(identity, endpoint)| {
                let end = match endpoint {
                    Some(spec) => DeclaredEdgeEnd::Dormant(spec.clone()),
                    None => DeclaredEdgeEnd::EndpointPending,
                };
                (identity.clone(), end)
            })
            .collect();
        for (local, peer) in &pairs {
            for identity in [local, peer] {
                if !ends.contains_key(identity) {
                    let end = self.declared_edge_end(identity).await;
                    ends.insert(identity.clone(), end);
                }
            }
        }
        let mut batched = Vec::with_capacity(pairs.len());
        let mut single = Vec::new();
        for (local, peer) in pairs {
            match Self::batched_declared_edge(&local, &peer, &ends) {
                Some(edge) => batched.push(edge),
                None => single.push((local, peer)),
            }
        }
        let mut staged = self.stage_batched_declared_edges(authority, batched, effects, journal);
        staged.single.extend(single);
        staged
    }

    /// Realize the declared edges a seat step staged, once the step
    /// committed: each live end trusts its counterpart's durable endpoint,
    /// under the authority the step minted for the topology it committed
    /// (every authority is derived before any trust is installed), and the
    /// edges the batch left out take the single-edge wire path. A failure
    /// leaves the edge owed a repair, or unwired, logged: the step is
    /// committed already.
    pub(super) async fn realize_staged_declared_edges(&mut self, staged: StagedDeclaredEdges) {
        let StagedDeclaredEdges { trust, single, .. } = staged;
        let mut installs = Vec::new();
        for (batched, authorized) in trust {
            let Some(trust) = batched.trust else {
                continue;
            };
            let authority = match &authorized {
                Some(authorized) => {
                    self.declared_edge_trust_authority(authorized, &batched.edge, &trust)
                }
                None => Err(MobError::WiringError(
                    "MobMachine authorized no trust for the edge".to_string(),
                )),
            };
            match authority {
                Ok(authority) => installs.push((batched.edge, trust, authority)),
                Err(error) => {
                    tracing::warn!(
                        mob_id = %self.definition.id,
                        member = %batched.local,
                        peer = %batched.peer,
                        %error,
                        "the live end of a declared edge has no trust authority; its repair is owed"
                    );
                    self.resolve_member_edge_realization(
                        &MemberWiringEdge::Member(batched.edge),
                        false,
                    );
                }
            }
        }
        for (edge, trust, authority) in installs {
            if let Err(error) = self
                .apply_trusted_peer_add_report(
                    trust.comms.as_ref(),
                    trust.counterpart_spec,
                    authority,
                )
                .await
            {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    counterpart = %trust.counterpart,
                    %error,
                    "the live end of a declared edge could not trust its counterpart; its repair is owed"
                );
                self.resolve_member_edge_realization(&MemberWiringEdge::Member(edge), false);
            }
        }
        for (local, peer) in single {
            if let Err(error) = Box::pin(self.handle_wire(
                local.clone(),
                super::super::handle::PeerTarget::Local(peer.clone()),
            ))
            .await
            {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    member = %local,
                    peer = %peer,
                    %error,
                    "a declared edge of a newly seated member could not be wired"
                );
            }
        }
    }

    /// The new edges the definition declares for `seated`, each as (seated
    /// member, target) and each unordered pair once, from one pass over the
    /// members of `state` (the seat step's prepared state, which holds its
    /// seats): a seated member's targets are the ones a spawn of it computes
    /// (`declared_wiring_targets` over the active members of each profile the
    /// rule reads). An edge `state` holds already is left out, as is a seated
    /// member that is not active (a Broken one, say).
    fn seated_declared_pairs(
        &self,
        state: &mob_dsl::MobMachineState,
        seated: &[AgentIdentity],
    ) -> Vec<(AgentIdentity, AgentIdentity)> {
        let wiring = &self.definition.wiring;
        let orchestrator_profile = self
            .definition
            .orchestrator
            .as_ref()
            .filter(|_| wiring.auto_wire_orchestrator)
            .map(|orchestrator| orchestrator.profile.as_str());
        let mut wired_profiles = BTreeSet::new();
        if let Some(orchestrator) = orchestrator_profile {
            wired_profiles.insert(orchestrator);
        }
        for rule in &wiring.role_wiring {
            wired_profiles.insert(rule.a.as_str());
            wired_profiles.insert(rule.b.as_str());
        }
        // An orchestrator member auto-wires to the members of every other
        // profile, so a step that seats one indexes every active member.
        let seats_an_orchestrator = orchestrator_profile.is_some_and(|orchestrator| {
            seated.iter().any(|member| {
                state
                    .member_profile_names
                    .get(&mob_dsl::AgentIdentity::from_domain(member))
                    .is_some_and(|profile| profile.as_str() == orchestrator)
            })
        });
        let is_active = |identity: &mob_dsl::AgentIdentity| {
            MobMemberLifecycleProjection::is_active_machine_lifecycle(
                &state.member_lifecycle_for_identity(identity),
            )
        };
        let mut active: BTreeMap<&str, Vec<AgentIdentity>> = BTreeMap::new();
        for (identity, profile) in &state.member_profile_names {
            if (seats_an_orchestrator || wired_profiles.contains(profile.as_str()))
                && is_active(identity)
            {
                active
                    .entry(profile.as_str())
                    .or_default()
                    .push(AgentIdentity::from(identity.0.as_str()));
            }
        }
        let mut pairs = Vec::new();
        let mut paired = BTreeSet::new();
        for member in seated {
            let dsl_member = mob_dsl::AgentIdentity::from_domain(member);
            if !is_active(&dsl_member) {
                continue;
            }
            let Some(profile) = state.member_profile_names.get(&dsl_member) else {
                continue;
            };
            let targets = Self::declared_wiring_targets(
                &self.definition,
                &ProfileName::from(profile.as_str()),
                |target_profile| {
                    active
                        .get(target_profile.as_str())
                        .map(|members| {
                            members
                                .iter()
                                .filter(|candidate| *candidate != member)
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                },
            );
            for target in targets {
                let edge = mob_dsl::WiringEdge::new(
                    dsl_member.clone(),
                    mob_dsl::AgentIdentity::from_domain(&target),
                );
                if !state.wiring_edges.contains(&edge) && paired.insert(edge) {
                    pairs.push((member.clone(), target));
                }
            }
        }
        pairs
    }

    /// How `identity`, a member committed before the seat step, joins the
    /// step's declared edges. MobMachine's facts decide first: a placed
    /// member takes the single-edge path, and a Dormant one joins from its
    /// durable endpoint, or with its endpoint owed. Any other member is
    /// resolved as every wire resolves it.
    async fn declared_edge_end(&self, identity: &AgentIdentity) -> DeclaredEdgeEnd {
        let (placed, dormant, unmaterialized, endpoint_recorded) = {
            let state = self.dsl_authority.state();
            (
                super::super::member_runtime_is_host_owned(state, identity),
                super::super::boot_plan::domain_member_is_dormant(state, identity),
                super::super::boot_plan::domain_member_is_unmaterialized(state, identity),
                state
                    .member_peer_endpoints
                    .contains_key(&mob_dsl::AgentIdentity::from_domain(identity)),
            )
        };
        if placed {
            return DeclaredEdgeEnd::Single;
        }
        let Some(entry) = self.roster.read().await.get(identity).cloned() else {
            return DeclaredEdgeEnd::Single;
        };
        if !matches!(entry.member_ref, MemberRef::Session { .. }) {
            return DeclaredEdgeEnd::Single;
        }
        if dormant && !endpoint_recorded {
            return DeclaredEdgeEnd::EndpointPending;
        }
        match self.resolve_wiring_endpoint(&entry, SEAT_WIRING).await {
            Ok(WiringEndpoint::Local { comms, .. }) => DeclaredEdgeEnd::Live(comms),
            Ok(WiringEndpoint::LocalDormant { spec, .. }) => DeclaredEdgeEnd::Dormant(spec),
            Err(_) if unmaterialized && !endpoint_recorded => DeclaredEdgeEnd::EndpointPending,
            Ok(WiringEndpoint::PeerOnly { .. } | WiringEndpoint::Placed { .. }) | Err(_) => {
                DeclaredEdgeEnd::Single
            }
        }
    }

    /// The batched form of the declared edge `local` - `peer`, or `None`
    /// when it takes the single-edge path: an end that is placed, peer-only
    /// or unresolved, or two live ends.
    fn batched_declared_edge(
        local: &AgentIdentity,
        peer: &AgentIdentity,
        ends: &BTreeMap<AgentIdentity, DeclaredEdgeEnd>,
    ) -> Option<BatchedDeclaredEdge> {
        let trust = match (ends.get(local)?, ends.get(peer)?) {
            (DeclaredEdgeEnd::Single, _)
            | (_, DeclaredEdgeEnd::Single)
            | (DeclaredEdgeEnd::Live(_), DeclaredEdgeEnd::Live(_)) => return None,
            (DeclaredEdgeEnd::Live(comms), DeclaredEdgeEnd::Dormant(spec)) => Some(LiveEndTrust {
                comms: Arc::clone(comms),
                counterpart: peer.clone(),
                counterpart_spec: spec.clone(),
            }),
            (DeclaredEdgeEnd::Dormant(spec), DeclaredEdgeEnd::Live(comms)) => Some(LiveEndTrust {
                comms: Arc::clone(comms),
                counterpart: local.clone(),
                counterpart_spec: spec.clone(),
            }),
            // No live end, or an end whose endpoint is owed: the machine edge
            // alone.
            (
                DeclaredEdgeEnd::Dormant(_) | DeclaredEdgeEnd::EndpointPending,
                DeclaredEdgeEnd::Dormant(_) | DeclaredEdgeEnd::EndpointPending,
            )
            | (DeclaredEdgeEnd::Live(_), DeclaredEdgeEnd::EndpointPending)
            | (DeclaredEdgeEnd::EndpointPending, DeclaredEdgeEnd::Live(_)) => None,
        };
        Some(BatchedDeclaredEdge {
            edge: mob_dsl::WiringEdge::new(
                mob_dsl::AgentIdentity::from_domain(local),
                mob_dsl::AgentIdentity::from_domain(peer),
            ),
            local: local.clone(),
            peer: peer.clone(),
            trust,
        })
    }

    /// Stage the batched declared edges on the seat step's prepared
    /// `authority` (see the module docs): their effects to `effects`, a
    /// `MembersWired` row per new edge to `journal`. Returns what is realized
    /// once the step committed: the live ends' trust, and as its single-edge
    /// edges the ones whose lifecycle notices MobMachine does not suppress,
    /// which that path classifies and realizes.
    fn stage_batched_declared_edges(
        &self,
        authority: &mut mob_dsl::MobMachinePreparedAuthority,
        edges: Vec<BatchedDeclaredEdge>,
        effects: &mut Vec<mob_dsl::MobMachineEffect>,
        journal: &mut Vec<NewMobEvent>,
    ) -> StagedDeclaredEdges {
        let mut staged = StagedDeclaredEdges::none();
        if edges.is_empty() {
            return staged;
        }

        // The lifecycle notices each edge would exchange, MobMachine's pure
        // verdict: an edge that joins a member that is not materialized
        // exchanges none.
        let verdict_inputs = edges.iter().flat_map(|batched| {
            [
                mob_dsl::MobMachineInput::ClassifyPeerNotice {
                    recipient: batched.edge.a.clone(),
                    sender: batched.edge.b.clone(),
                    notice_class: mob_dsl::PeerInputClass::Lifecycle,
                },
                mob_dsl::MobMachineInput::ClassifyPeerNotice {
                    recipient: batched.edge.b.clone(),
                    sender: batched.edge.a.clone(),
                    notice_class: mob_dsl::PeerInputClass::Lifecycle,
                },
            ]
        });
        let verdicts = match authority.apply_batch(verdict_inputs) {
            Ok(verdicts) => verdicts,
            Err(error) => {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    %error,
                    "MobMachine classified no notice of the declared edges of newly seated members"
                );
                staged.single = edges
                    .into_iter()
                    .map(|batched| (batched.local, batched.peer))
                    .collect();
                return staged;
            }
        };
        let mut kept = Vec::with_capacity(edges.len());
        let mut verdicts = verdicts.into_iter();
        for batched in edges {
            let pair = verdicts.by_ref().take(2).collect::<Vec<_>>();
            let suppressed = pair.len() == 2
                && pair.iter().all(|transition| {
                    transition.effects().iter().any(|effect| {
                        matches!(
                            effect,
                            mob_dsl::MobMachineEffect::PeerNoticeClassified {
                                disposition: mob_dsl::PeerNoticeDisposition::Suppressed,
                                ..
                            }
                        )
                    })
                });
            for transition in pair {
                staged.phase_changed |= transition.from_phase != transition.to_phase;
                effects.extend(transition.into_effects());
            }
            if suppressed {
                kept.push(batched);
            } else {
                staged.single.push((batched.local, batched.peer));
            }
        }

        // The edges, then the live ends' trust authorizations, which read the
        // topology the whole batch leaves.
        for AppliedDeclaredEdge {
            batched,
            wired,
            authorized,
        } in self.apply_declared_edges(authority, kept)
        {
            let Some(wired) = wired else {
                continue;
            };
            let added = Self::wire_members_disposition_from_effects(
                wired.effects(),
                &batched.edge,
                SEAT_WIRING,
            );
            staged.phase_changed |= wired.from_phase != wired.to_phase;
            effects.extend(wired.into_effects());
            if let Some(authorized) = &authorized {
                staged.phase_changed |= authorized.from_phase != authorized.to_phase;
                effects.extend(authorized.effects().iter().cloned());
            }
            match added {
                Ok(true) => {
                    // Journal before commit: MobMachine never holds an edge
                    // before its MembersWired row is durable, which the
                    // step's one append makes it with the seats' rows.
                    journal.push(NewMobEvent {
                        mob_id: self.definition.id.clone(),
                        timestamp: None,
                        kind: MobEventKind::MembersWired {
                            a: AgentIdentity::from(batched.edge.a.0.as_str()),
                            b: AgentIdentity::from(batched.edge.b.0.as_str()),
                        },
                    });
                    if batched.trust.is_some() {
                        staged.trust.push((batched, authorized));
                    }
                }
                // The pass leaves out the edges MobMachine holds, so none is
                // only repaired here.
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    mob_id = %self.definition.id,
                    member = %batched.local,
                    peer = %batched.peer,
                    %error,
                    "a declared edge of a newly seated member was not wired"
                ),
            }
        }
        staged
    }

    /// Apply `edges` on the prepared `authority`: every `WireMembers`, then
    /// every live end's `AuthorizeMemberTrustWiring`, as one batch. A refused
    /// batch left the authority as it was: each edge is then applied alone,
    /// in order, and an edge MobMachine refuses is left out, logged.
    fn apply_declared_edges(
        &self,
        authority: &mut mob_dsl::MobMachinePreparedAuthority,
        edges: Vec<BatchedDeclaredEdge>,
    ) -> Vec<AppliedDeclaredEdge> {
        let wire = |batched: &BatchedDeclaredEdge| mob_dsl::MobMachineInput::WireMembers {
            edge: batched.edge.clone(),
        };
        let authorize =
            |batched: &BatchedDeclaredEdge| mob_dsl::MobMachineInput::AuthorizeMemberTrustWiring {
                edge: batched.edge.clone(),
                a_identity: batched.edge.a.clone(),
                b_identity: batched.edge.b.clone(),
            };
        let inputs = edges
            .iter()
            .map(wire)
            .chain(
                edges
                    .iter()
                    .filter(|batched| batched.trust.is_some())
                    .map(authorize),
            )
            .collect::<Vec<_>>();
        if let Ok(transitions) = authority.apply_batch(inputs) {
            let mut transitions = transitions.into_iter();
            let wired = edges.iter().map(|_| transitions.next()).collect::<Vec<_>>();
            return edges
                .into_iter()
                .zip(wired)
                .map(|(batched, wired)| {
                    let authorized = if batched.trust.is_some() {
                        transitions.next()
                    } else {
                        None
                    };
                    AppliedDeclaredEdge {
                        batched,
                        wired,
                        authorized,
                    }
                })
                .collect();
        }

        // One edge's refusal: each edge alone, in order, then each live
        // end's authorization over the topology all of them leave.
        let mut applied = Vec::with_capacity(edges.len());
        for batched in edges {
            let wired = match authority.apply_batch([wire(&batched)]) {
                Ok(mut transitions) => transitions.pop(),
                Err(error) => {
                    tracing::warn!(
                        mob_id = %self.definition.id,
                        member = %batched.local,
                        peer = %batched.peer,
                        %error,
                        "MobMachine refused a declared edge of a newly seated member"
                    );
                    None
                }
            };
            applied.push(AppliedDeclaredEdge {
                batched,
                wired,
                authorized: None,
            });
        }
        for entry in &mut applied {
            if entry.wired.is_none() || entry.batched.trust.is_none() {
                continue;
            }
            entry.authorized = match authority.apply_batch([authorize(&entry.batched)]) {
                Ok(mut transitions) => transitions.pop(),
                Err(error) => {
                    tracing::warn!(
                        mob_id = %self.definition.id,
                        member = %entry.batched.local,
                        peer = %entry.batched.peer,
                        %error,
                        "MobMachine authorized no trust for a declared edge of a newly seated member"
                    );
                    None
                }
            };
        }
        applied
    }

    /// The authority a live end installs its trust toward its counterpart
    /// with, from the trust authorization the seat step applied.
    fn declared_edge_trust_authority(
        &self,
        authorized: &mob_dsl::MobMachineTransition,
        edge: &mob_dsl::WiringEdge,
        trust: &LiveEndTrust,
    ) -> Result<CommsTrustMutationAuthority, MobError> {
        let handoff = self.member_trust_wiring_handoff_from_transition(
            authorized,
            edge,
            SEAT_WIRING,
            MemberTrustOperation::Wiring,
        )?;
        let key = Self::trusted_peer_removal_key(&trust.counterpart_spec);
        handoff.require_peer_id_for(&trust.counterpart, &key)?;
        handoff.add_authority_for(&trust.counterpart, &key, &self.dsl_authority)
    }
}

/// The row kinds of every append a seat chunk or a rotation made, per mob,
/// in order: the structural witness that a seat step journals its seats and
/// their declared edges in one append.
#[cfg(test)]
static SEAT_STEP_APPENDS_FOR_TEST: std::sync::Mutex<BTreeMap<MobId, Vec<Vec<&'static str>>>> =
    std::sync::Mutex::new(BTreeMap::new());

/// Record the row kinds of one seat step's append for `mob_id`.
#[cfg(test)]
pub(super) fn record_seat_step_append_for_test(mob_id: &MobId, journal: &[NewMobEvent]) {
    let kinds = journal
        .iter()
        .map(|event| match &event.kind {
            MobEventKind::MemberSpawned(_) => "MemberSpawned",
            MobEventKind::MemberRetired { .. } => "MemberRetired",
            MobEventKind::MembersWired { .. } => "MembersWired",
            MobEventKind::MemberKickoffUpdated { .. } => "MemberKickoffUpdated",
            _ => "other",
        })
        .collect();
    if let Ok(mut appends) = SEAT_STEP_APPENDS_FOR_TEST.lock() {
        appends.entry(mob_id.clone()).or_default().push(kinds);
    }
}

/// Test read: the row kinds of every append the seat chunks and rotations of
/// `mob_id` made, in order.
#[cfg(test)]
pub(in crate::runtime) fn seat_step_appends_for_test(mob_id: &MobId) -> Vec<Vec<&'static str>> {
    SEAT_STEP_APPENDS_FOR_TEST
        .lock()
        .ok()
        .and_then(|appends| appends.get(mob_id).cloned())
        .unwrap_or_default()
}
