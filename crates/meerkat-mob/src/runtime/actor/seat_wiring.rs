//! The edges a mob definition declares, wired for seated members.
//!
//! A seat (`MobActor::commit_dormant_seats`) and a rotation's next
//! incarnation (`MobActor::commit_member_rotation`) wire their member to the
//! members a spawn of it would be wired to (`MobActor::declared_wiring_targets`:
//! the orchestrator's members under `auto_wire_orchestrator`, and the members
//! of every profile a `role_wiring` rule pairs with its own), once the seat is
//! committed, so the members seated in one chunk see each other. An identity
//! that was seated already, or that a spawn embodied, is wired by nothing
//! here: it holds the edges it holds.
//!
//! One step per chunk, linear in the chunk's edges: the new edges come from
//! one pass over MobMachine's active members, and their lifecycle notice
//! verdicts, their `WireMembers` inputs and, for an edge with a live end, its
//! `AuthorizeMemberTrustWiring` are applied on one prepared authority (one
//! copy of the machine state), with their `MembersWired` rows journaled in one
//! `append_batch` before the base-checked commit. A refused batch leaves the
//! prepared authority as it was, and its edges are applied again one at a
//! time, so an edge MobMachine refuses is left out alone.
//!
//! Every such edge joins a member that is not materialized (the seated
//! member, whose wake can only start after this step), so MobMachine
//! suppresses its lifecycle notices, and trust is installed only on a live
//! end, toward the other member's durable endpoint. A member that is not
//! materialized gets its own trust from its wake's topology step, from the
//! machine's edges. A member seated with its endpoint owed (`EndpointPending`,
//! on network comms) is joined by the machine edge and its row alone, with no
//! trust on either end: its boot wake registers the endpoint and its topology
//! step realizes the trust of both ends, as for the edges a restart replays.
//! A live end that cannot install its trust leaves the edge owed a repair,
//! which the owner retries. An edge with a placed or peer-only member, or
//! between two live members, takes the single-edge wire path
//! (`MobActor::handle_wire`), as a spawn's edges do.

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

impl MobActor {
    /// Wire the members a seat step just seated (in `seated`, in order) to
    /// the edges their definition declares (see the module docs). The seats
    /// are durable already, so nothing here fails them: an edge MobMachine
    /// refuses, or one the single-edge path cannot wire, is logged and left
    /// unwired.
    pub(super) async fn wire_seated_declared_edges(&mut self, seated: &[AgentIdentity]) {
        if seated.is_empty() || !Self::definition_declares_wiring(&self.definition) {
            return;
        }
        if self.dsl_authority.state().lifecycle_phase != mob_dsl::MobPhase::Running {
            tracing::warn!(
                mob_id = %self.definition.id,
                "the declared edges of newly seated members are not wired: the mob is not running"
            );
            return;
        }
        let pairs = self.seated_declared_pairs(seated);
        if pairs.is_empty() {
            return;
        }
        // Each member's end, resolved once for all its edges.
        let mut ends: BTreeMap<AgentIdentity, DeclaredEdgeEnd> = BTreeMap::new();
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
        let unsuppressed = self.commit_batched_declared_edges(batched).await;
        single.extend(unsuppressed);
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
    /// member, target) and each unordered pair once, from one pass over
    /// MobMachine's members: a seated member's targets are the ones a spawn
    /// of it computes (`declared_wiring_targets` over the active members of
    /// each profile the definition's wiring names). An edge MobMachine holds
    /// already is left out, as is a seated member that is not active (a
    /// Broken one, say).
    fn seated_declared_pairs(
        &self,
        seated: &[AgentIdentity],
    ) -> Vec<(AgentIdentity, AgentIdentity)> {
        let state = self.dsl_authority.state();
        let wiring = &self.definition.wiring;
        let mut wired_profiles = BTreeSet::new();
        if wiring.auto_wire_orchestrator
            && let Some(orchestrator) = &self.definition.orchestrator
        {
            wired_profiles.insert(orchestrator.profile.as_str());
        }
        for rule in &wiring.role_wiring {
            wired_profiles.insert(rule.a.as_str());
            wired_profiles.insert(rule.b.as_str());
        }
        let is_active = |identity: &mob_dsl::AgentIdentity| {
            MobMemberLifecycleProjection::is_active_machine_lifecycle(
                &state.member_lifecycle_for_identity(identity),
            )
        };
        let mut active: BTreeMap<&str, Vec<AgentIdentity>> = BTreeMap::new();
        for (identity, profile) in &state.member_profile_names {
            if wired_profiles.contains(profile.as_str()) && is_active(identity) {
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

    /// How `identity` joins the declared edges of a seat. MobMachine's facts
    /// decide first: a placed member takes the single-edge path, and a
    /// Dormant one joins from its durable endpoint, or with its endpoint owed.
    /// Any other member is resolved as every wire resolves it.
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

    /// Commit the batched declared edges in one step (see the module docs),
    /// then install each live end's trust. Returns the edges whose lifecycle
    /// notices MobMachine does not suppress: they take the single-edge path,
    /// which realizes that verdict.
    async fn commit_batched_declared_edges(
        &mut self,
        edges: Vec<BatchedDeclaredEdge>,
    ) -> Vec<(AgentIdentity, AgentIdentity)> {
        if edges.is_empty() {
            return Vec::new();
        }
        let mut authority = self.dsl_authority.prepare_authority();
        let mut effects = Vec::new();
        let mut phase_changed = false;

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
                return edges
                    .into_iter()
                    .map(|batched| (batched.local, batched.peer))
                    .collect();
            }
        };
        let mut unsuppressed = Vec::new();
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
                phase_changed |= transition.from_phase != transition.to_phase;
                effects.extend(transition.into_effects());
            }
            if suppressed {
                kept.push(batched);
            } else {
                unsuppressed.push((batched.local, batched.peer));
            }
        }

        // The edges, then the live ends' trust authorizations, which read the
        // topology the whole batch leaves.
        let applied = self.apply_declared_edges(&mut authority, kept);
        let mut journal = Vec::with_capacity(applied.len());
        let mut committed = Vec::with_capacity(applied.len());
        for AppliedDeclaredEdge {
            batched,
            wired,
            authorized,
        } in applied
        {
            let Some(wired) = wired else {
                continue;
            };
            let added = Self::wire_members_disposition_from_effects(
                wired.effects(),
                &batched.edge,
                SEAT_WIRING,
            );
            phase_changed |= wired.from_phase != wired.to_phase;
            effects.extend(wired.into_effects());
            if let Some(authorized) = &authorized {
                phase_changed |= authorized.from_phase != authorized.to_phase;
                effects.extend(authorized.effects().iter().cloned());
            }
            match added {
                Ok(true) => {
                    journal.push(NewMobEvent {
                        mob_id: self.definition.id.clone(),
                        timestamp: None,
                        kind: MobEventKind::MembersWired {
                            a: AgentIdentity::from(batched.edge.a.0.as_str()),
                            b: AgentIdentity::from(batched.edge.b.0.as_str()),
                        },
                    });
                    committed.push((batched, authorized));
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
        if journal.is_empty() {
            return unsuppressed;
        }

        // Journal before commit: MobMachine never holds an edge before its
        // MembersWired row is durable.
        let prepared = PreparedDslInput {
            authority,
            effects,
            phase_changed,
        };
        let events = Arc::clone(&self.events);
        let stored = match self
            .commit_prepared_dsl_input_after(prepared, move || async move {
                events.append_batch(journal).await.map_err(MobError::from)
            })
            .await
        {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    %error,
                    "the declared edges of newly seated members could not be committed; none is wired"
                );
                return unsuppressed;
            }
        };
        {
            let mut roster = self.roster.write().await;
            for event in &stored {
                roster.apply_event(event);
            }
        }

        // Each live end trusts its counterpart's durable endpoint, under the
        // authority the batch minted for the topology it committed. Every
        // authority is derived before any trust is installed.
        let mut installs = Vec::new();
        for (batched, authorized) in committed {
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
        unsuppressed
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
    /// with, from the trust authorization the batch applied.
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
