//! Wiring with a member that is not materialized in this process.
//!
//! A seated local member that is Dormant (or Waking before its comms runtime
//! exists) resolves to [`WiringEndpoint::LocalDormant`]: its peer material is
//! MobMachine's durable endpoint. Wire, unwire and the idempotent repair
//! commit the machine edge and its durable event exactly as for a live
//! member. Trust is installed or removed only on a side that has a comms
//! runtime (a live member trusts the durable endpoint; a peer-only member's
//! overlay is pushed through its bridge); the dormant side's own trust is
//! realized by its wake's topology step, from machine state. No wiring verb
//! wakes a member.
//!
//! The lifecycle notices such an edge change would exchange are MobMachine's
//! verdict (`ClassifyPeerNotice`): a member that is not materialized can
//! neither send nor receive them, they are suppressed, and the trust its wake
//! installs carries their content.

use super::*;

/// The side of an edge with a comms runtime, trusting a counterpart that is
/// not materialized.
struct LiveSideTrust<'a> {
    comms: &'a Arc<dyn CoreCommsRuntime>,
    counterpart: &'a AgentIdentity,
    counterpart_spec: &'a TrustedPeerDescriptor,
}

/// A peer-only side's overlay toward a counterpart that is not materialized.
struct PeerOnlySideTrust<'a> {
    spec: &'a TrustedPeerDescriptor,
    binding: &'a crate::RuntimeBinding,
    counterpart_spec: &'a TrustedPeerDescriptor,
}

/// What an edge change with at least one member that is not materialized
/// realizes beyond the machine edge and its event.
enum DormantEdgeLane<'a> {
    /// Neither member is materialized: nothing to install now.
    Neither,
    /// One live member's trust toward the other's durable endpoint.
    Live(LiveSideTrust<'a>),
    /// One peer-only member's overlay toward the other's durable endpoint.
    PeerOnly(PeerOnlySideTrust<'a>),
}

fn dormant_edge_lane<'a>(
    local: &'a AgentIdentity,
    local_endpoint: &'a WiringEndpoint,
    peer: &'a AgentIdentity,
    peer_endpoint: &'a WiringEndpoint,
) -> Result<DormantEdgeLane<'a>, MobError> {
    match (local_endpoint, peer_endpoint) {
        (WiringEndpoint::LocalDormant { .. }, WiringEndpoint::LocalDormant { .. }) => {
            Ok(DormantEdgeLane::Neither)
        }
        (WiringEndpoint::Local { comms, .. }, WiringEndpoint::LocalDormant { spec, .. }) => {
            Ok(DormantEdgeLane::Live(LiveSideTrust {
                comms,
                counterpart: peer,
                counterpart_spec: spec,
            }))
        }
        (WiringEndpoint::LocalDormant { spec, .. }, WiringEndpoint::Local { comms, .. }) => {
            Ok(DormantEdgeLane::Live(LiveSideTrust {
                comms,
                counterpart: local,
                counterpart_spec: spec,
            }))
        }
        (
            WiringEndpoint::PeerOnly { spec, binding },
            WiringEndpoint::LocalDormant {
                spec: dormant_spec, ..
            },
        )
        | (
            WiringEndpoint::LocalDormant {
                spec: dormant_spec, ..
            },
            WiringEndpoint::PeerOnly { spec, binding },
        ) => Ok(DormantEdgeLane::PeerOnly(PeerOnlySideTrust {
            spec,
            binding,
            counterpart_spec: dormant_spec,
        })),
        _ => Err(MobError::Internal(format!(
            "the wiring lane of members that are not materialized was entered for '{local}' <-> '{peer}' without such a member"
        ))),
    }
}

impl MobActor {
    /// MobMachine's verdict on the lifecycle notices two members exchange
    /// when the edge between them changes and one of them is not
    /// materialized: each direction must be `Suppressed`. A member that is
    /// not materialized can neither send nor receive a notice, so any other
    /// verdict is refused typed rather than dropped.
    pub(super) fn require_suppressed_lifecycle_notices(
        &mut self,
        a: &AgentIdentity,
        b: &AgentIdentity,
    ) -> Result<(), MobError> {
        for (recipient, sender) in [(a, b), (b, a)] {
            let disposition =
                self.classify_peer_notice(recipient, sender, mob_dsl::PeerInputClass::Lifecycle)?;
            if !matches!(disposition, mob_dsl::PeerNoticeDisposition::Suppressed) {
                return Err(Self::unmaterialized_notice_drift(
                    recipient,
                    sender,
                    &format!("{disposition:?}"),
                ));
            }
        }
        Ok(())
    }

    /// MobMachine's verdict on one lifecycle notice from `sender` to
    /// `recipient` (R4), realized through the notice consumer: true only
    /// for `Deliver`. A notice that is not delivered is skipped because of
    /// that verdict, never because of the recipient's endpoint.
    pub(super) fn lifecycle_notice_delivers(
        &mut self,
        recipient: &AgentIdentity,
        sender: &AgentIdentity,
    ) -> Result<bool, MobError> {
        Ok(matches!(
            self.realize_peer_notice_disposition(
                recipient,
                sender,
                mob_dsl::PeerInputClass::Lifecycle,
            )?,
            super::member_wake::PeerNoticeRealization::Deliver
        ))
    }

    /// A lifecycle notice whose MobMachine verdict needs a comms runtime that
    /// a member that is not materialized does not have: the verdict and the
    /// member's materialization disagree.
    pub(super) fn unmaterialized_notice_drift(
        recipient: &AgentIdentity,
        sender: &AgentIdentity,
        verdict: &str,
    ) -> MobError {
        MobError::Internal(format!(
            "MobMachine's verdict {verdict} on the lifecycle notice from '{sender}' to '{recipient}' needs a comms runtime that a member that is not materialized does not have"
        ))
    }

    /// Wire a new edge one of whose members is not materialized (see the
    /// module docs). Every effect before the durable event is rolled back on
    /// a failure, the machine edge included.
    pub(super) async fn wire_new_edge_with_dormant_endpoint(
        &mut self,
        edge: &mob_dsl::WiringEdge,
        local: &AgentIdentity,
        peer: &AgentIdentity,
        local_endpoint: &WiringEndpoint,
        peer_endpoint: &WiringEndpoint,
    ) -> Result<(), MobError> {
        let lane = dormant_edge_lane(local, local_endpoint, peer, peer_endpoint)?;
        let local_spec = local_endpoint.spec();
        let peer_spec = peer_endpoint.spec();
        // The notices are a pure verdict: refused before anything moves.
        self.require_suppressed_lifecycle_notices(local, peer)?;
        let authority = self.apply_wire_members_idempotent(edge)?;
        let dsl_added = authority.dsl_added();
        let handoff = authority.member_handoff()?;

        let mut live_trust_created = false;
        match &lane {
            DormantEdgeLane::Neither => {}
            DormantEdgeLane::Live(side) => {
                let key = Self::trusted_peer_removal_key(side.counterpart_spec);
                let trust_authority = handoff
                    .require_peer_id_for(side.counterpart, &key)
                    .and_then(|()| {
                        handoff.add_authority_for(side.counterpart, &key, &self.dsl_authority)
                    });
                let added = match trust_authority {
                    Ok(trust_authority) => self
                        .apply_trusted_peer_add_report(
                            side.comms.as_ref(),
                            side.counterpart_spec.clone(),
                            trust_authority,
                        )
                        .await
                        .map_err(MobError::from),
                    Err(error) => Err(error),
                };
                match added {
                    Ok(created) => live_trust_created = created,
                    Err(error) => {
                        self.rollback_peer_only_wire(
                            edge,
                            dsl_added,
                            WiringSides::empty(),
                            local,
                            peer,
                            local_spec,
                            peer_spec,
                        )
                        .await;
                        return Err(error);
                    }
                }
            }
            DormantEdgeLane::PeerOnly(side) => {
                if let Err(error) = self
                    .wire_peer_only_recipient(
                        side.spec,
                        Some(side.binding),
                        side.counterpart_spec,
                        std::time::Duration::from_secs(10),
                    )
                    .await
                {
                    self.rollback_peer_only_wire(
                        edge,
                        dsl_added,
                        WiringSides::empty(),
                        local,
                        peer,
                        local_spec,
                        peer_spec,
                    )
                    .await;
                    return Err(error);
                }
            }
        }

        let event = NewMobEvent {
            mob_id: self.definition.id.clone(),
            timestamp: None,
            kind: MobEventKind::MembersWired {
                a: AgentIdentity::from(edge.a.0.as_str()),
                b: AgentIdentity::from(edge.b.0.as_str()),
            },
        };
        let stored = match self.events.append(event).await {
            Ok(stored) => stored,
            Err(error) => {
                match &lane {
                    DormantEdgeLane::Live(side) if live_trust_created => {
                        let key = Self::trusted_peer_removal_key(side.counterpart_spec);
                        self.rollback_peer_only_trust(
                            edge,
                            side.comms.as_ref(),
                            side.counterpart,
                            &key,
                            "wire_unmaterialized_member_event_rollback_trust_authority",
                        )
                        .await;
                    }
                    DormantEdgeLane::PeerOnly(side) => {
                        if let Err(cleanup) = self
                            .unwire_peer_only_recipient(
                                side.spec,
                                None,
                                side.counterpart_spec,
                                std::time::Duration::from_secs(10),
                            )
                            .await
                        {
                            tracing::warn!(
                                mob_id = %self.definition.id,
                                error = %cleanup,
                                "wire rollback: failed to unwire the peer-only recipient"
                            );
                        }
                    }
                    _ => {}
                }
                self.rollback_peer_only_wire(
                    edge,
                    dsl_added,
                    WiringSides::empty(),
                    local,
                    peer,
                    local_spec,
                    peer_spec,
                )
                .await;
                return Err(MobError::from(error));
            }
        };
        self.roster.write().await.apply_event(&stored);
        Ok(())
    }

    /// Unwire an edge one of whose members is not materialized (see the
    /// module docs). Idempotent absence is a no-op only when the live side's
    /// trust agrees. A failure before the durable event restores the machine
    /// edge (and the live side's trust, when it was removed).
    pub(super) async fn unwire_edge_with_dormant_endpoint(
        &mut self,
        edge: &mob_dsl::WiringEdge,
        local: &AgentIdentity,
        peer: &AgentIdentity,
        local_endpoint: &WiringEndpoint,
        peer_endpoint: &WiringEndpoint,
        dsl_has_edge: bool,
    ) -> Result<(), MobError> {
        let lane = dormant_edge_lane(local, local_endpoint, peer, peer_endpoint)?;
        if !dsl_has_edge {
            if let DormantEdgeLane::Live(side) = &lane {
                let key = Self::trusted_peer_removal_key(side.counterpart_spec);
                let stale = side
                    .comms
                    .peers()
                    .await
                    .iter()
                    .any(|trusted| trusted.peer_id.to_string() == key);
                if stale {
                    return Err(MobError::WiringError(format!(
                        "unwire for '{local}' <-> '{peer}' requires MobMachine wiring authority"
                    )));
                }
            }
            return Ok(());
        }
        // The notices are a pure verdict: refused before anything moves.
        self.require_suppressed_lifecycle_notices(local, peer)?;
        self.cancel_peer_deliveries_for_edge(local, peer, "members are unwiring")
            .await?;
        let unwire_handoff = self.apply_unwire_members_idempotent(edge)?;

        let mut live_trust_removed = false;
        let removed = match &lane {
            DormantEdgeLane::Neither => Ok(()),
            DormantEdgeLane::Live(side) => {
                let key = Self::trusted_peer_removal_key(side.counterpart_spec);
                let removal = unwire_handoff
                    .require_peer_id_for(side.counterpart, &key)
                    .and_then(|()| unwire_handoff.unwiring_authority_for(side.counterpart, &key));
                match removal {
                    Ok(authority) => {
                        let result = self
                            .apply_trusted_peer_remove(side.comms.as_ref(), key, authority)
                            .await
                            .map_err(MobError::from);
                        live_trust_removed = result.is_ok();
                        result
                    }
                    Err(error) => Err(error),
                }
            }
            DormantEdgeLane::PeerOnly(side) => {
                self.unwire_peer_only_recipient(
                    side.spec,
                    Some(side.binding),
                    side.counterpart_spec,
                    std::time::Duration::from_secs(10),
                )
                .await
            }
        };
        let appended = match removed {
            Ok(()) => {
                let event = NewMobEvent {
                    mob_id: self.definition.id.clone(),
                    timestamp: None,
                    kind: MobEventKind::MembersUnwired {
                        a: AgentIdentity::from(edge.a.0.as_str()),
                        b: AgentIdentity::from(edge.b.0.as_str()),
                    },
                };
                self.events.append(event).await.map_err(MobError::from)
            }
            Err(error) => Err(error),
        };
        match appended {
            Ok(stored) => {
                self.roster.write().await.apply_event(&stored);
                Ok(())
            }
            Err(error) => {
                self.restore_unmaterialized_member_edge(edge, &lane, live_trust_removed)
                    .await;
                Err(error)
            }
        }
    }

    /// Roll back a failed unwire of an edge with a member that is not
    /// materialized: the machine edge is wired again and the live side's
    /// trust reinstalled when it was removed. Best effort: each failure is
    /// logged, as the other unwire rollbacks do.
    async fn restore_unmaterialized_member_edge(
        &mut self,
        edge: &mob_dsl::WiringEdge,
        lane: &DormantEdgeLane<'_>,
        live_trust_removed: bool,
    ) {
        let authority = match self.apply_wire_members_idempotent(edge) {
            Ok(authority) => authority,
            Err(error) => {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    %error,
                    "unwire rollback: failed to restore the MobMachine wire"
                );
                return;
            }
        };
        let DormantEdgeLane::Live(side) = lane else {
            return;
        };
        if !live_trust_removed {
            return;
        }
        let key = Self::trusted_peer_removal_key(side.counterpart_spec);
        let restored = match authority.member_handoff().and_then(|handoff| {
            handoff
                .require_peer_id_for(side.counterpart, &key)
                .and_then(|()| {
                    handoff.add_authority_for(side.counterpart, &key, &self.dsl_authority)
                })
        }) {
            Ok(trust_authority) => self
                .apply_trusted_peer_add_report(
                    side.comms.as_ref(),
                    side.counterpart_spec.clone(),
                    trust_authority,
                )
                .await
                .map(|_| ())
                .map_err(MobError::from),
            Err(error) => Err(error),
        };
        if let Err(error) = restored {
            tracing::warn!(
                mob_id = %self.definition.id,
                %error,
                "unwire rollback: failed to reinstall the live side's trust"
            );
        }
    }
}
