//! The runtime wake turn of one attempt.
//!
//! A boot restores its members in roster order; a wake restores one member at
//! runtime, possibly while other members wake. The turn is the actor's commit
//! step for that one attempt, on the member effect lane:
//!
//! - Step A (on the actor, [`MobActor::start_member_materialization`]): the
//!   attempt MobMachine authorized is still open; take the member's own fork
//!   of the authority, its overlay rule and the overlays it may follow, its
//!   continuity intent, and dispatch the restore.
//! - Step B (off the actor, [`plan_member_wake`]): the cold restore recipe
//!   (`restore_resumed_member`) plans the member's restore on its own fork,
//!   writing nothing shared. A missing bound session searches its successor
//!   here and provisions it on a fork repointed to it. The woken comms
//!   runtime's endpoint is checked against the durable one.
//! - Step C (on the actor, [`MobActor::commit_member_wake_plan`]): the
//!   attempt is still this wake's (attempt and incarnation fences); a
//!   successor is repointed (journal first); `apply_member_restore_plan`
//!   applies the plan to the actor's authority, rosters and overlays in the
//!   member's turn, ending in `ResolveMemberRevivalBound` (the runtime
//!   binding is requested, the member is still Waking).
//! - Readiness (the member's comms drain) and topology (its wiring restored
//!   over the machine's edges, dormant peers included), then
//!   `ResolveMemberRevivalSucceeded` (Waking -> Live) and every caller of the
//!   attempt is answered.
//!
//! Every wake feedback is attempt-fenced and answered by a machine verdict:
//! a stale attempt is a no-op, never a fail-stop. A provision a stale or
//! failed wake leaves behind is rolled back through its `PendingProvision`.

use super::member_effect_lane::{
    MemberEffectAck, MemberEffectCommit, MemberEffectCommitFuture, MemberEffectRequest,
    MemberEffectRetention, MemberEffectSend, MemberEffectSettlement, MemberFence,
};
use super::member_wake::{
    MemberWakeAttempt, MemberWakeFailure, MemberWakeOutcome, MemberWiringEdge,
    PendingMemberMaterialization, RunningMemberWake,
};
use super::*;
use crate::runtime::builder::{
    MemberRestoreCommit, MemberRestorePlan, MemberRestoreProvisionOutcome, MemberSuccessorSearch,
    PersistedMemberSessionIndex, RecordedContinuity, ResumeMemberRestoreContext,
    ResumeRestoreCandidate, SeatedMemberRestoreOutcome, append_recovered_session_binding,
    apply_member_restore_plan, member_endpoint_mismatch_detail, member_restore_failure,
    provision_durable_member_restore, restore_resumed_member,
};

/// The wake one turn runs.
#[derive(Debug, Clone)]
struct WakeRun {
    identity: AgentIdentity,
    attempt: MemberWakeAttempt,
    cause: mob_dsl::MemberWakeCause,
}

/// What rolls back the provision a wake made, captured from its receipt
/// before the plan is applied: a wake that fails or goes stale after it
/// provisioned leaves no materialization the machine does not hold.
#[derive(Clone)]
struct WakeRollback {
    member_ref: MemberRef,
    operation_id: meerkat_core::ops::OperationId,
    session_origin: super::super::provisioner::ProvisionSessionOrigin,
    rollback_authority: Option<super::super::provisioner::ResumedMemberRollbackAuthority>,
}

impl WakeRollback {
    fn from_plan(plan: &MemberRestorePlan) -> Option<Self> {
        let MemberRestorePlan::Seated {
            outcome: SeatedMemberRestoreOutcome::Provision(provision),
            ..
        } = plan
        else {
            return None;
        };
        let receipt = match &provision.outcome {
            MemberRestoreProvisionOutcome::Durable { receipt, .. }
            | MemberRestoreProvisionOutcome::Fresh { receipt, .. } => receipt,
            MemberRestoreProvisionOutcome::Failed(_)
            | MemberRestoreProvisionOutcome::Retryable { .. } => return None,
        };
        Some(Self {
            member_ref: receipt.member_ref.clone(),
            operation_id: receipt.operation_id.clone(),
            session_origin: receipt.session_origin,
            rollback_authority: receipt.rollback_authority.clone(),
        })
    }

    fn pending(
        self,
        identity: &AgentIdentity,
        provisioner: Arc<dyn MobProvisioner>,
    ) -> PendingProvision {
        PendingProvision::new(
            self.member_ref,
            identity.clone(),
            provisioner,
            self.operation_id,
            self.session_origin,
            self.rollback_authority,
            // A failed wake keeps the member seated.
            super::super::provisioner::RollbackOrigin::RevivalOrRebuild,
        )
    }
}

/// The woken member's endpoint as its new comms runtime presents it.
enum WakeEndpoint {
    /// It matches the durable endpoint (or nothing was materialized).
    Matches,
    /// No durable endpoint is recorded yet (a legacy member, or a successor):
    /// record this one once the plan applied.
    Record(TrustedPeerDescriptor),
}

/// What Step B hands to the member's turn.
enum WakePlan {
    /// The bound session's restore plan.
    Restore {
        plan: MemberRestorePlan,
        endpoint: WakeEndpoint,
    },
    /// The bound session is absent: its persisted successor, provisioned on
    /// a fork repointed to it. The turn repoints the binding (journal first)
    /// and applies the plan.
    Successor {
        successor_session_id: SessionId,
        plan: MemberRestorePlan,
        endpoint: WakeEndpoint,
    },
    /// The bound session is absent and has no successor.
    NoSuccessor { bridge_session_id: SessionId },
    /// The woken comms runtime presents an endpoint other than the durable
    /// one; its provision was already rolled back.
    EndpointMismatch {
        bridge_session_id: SessionId,
        reason: String,
    },
}

/// The topology step's remaining edges.
struct WakeTopology {
    remaining: VecDeque<(
        super::super::handle::PeerTarget,
        RespawnTopologyPeerId,
        MemberWiringEdge,
    )>,
    failed: Vec<RespawnTopologyPeerId>,
}

/// One step of a wake turn, carried back to the actor.
enum WakeStep {
    Planned(Box<Result<WakePlan, MobError>>),
    Ready(Result<(), MobError>),
    Topology {
        topology: WakeTopology,
        peer: RespawnTopologyPeerId,
        edge: Box<MemberWiringEdge>,
        result: Result<(), MobError>,
    },
    /// A rollback of the wake's provision finished (`false`: it could not be
    /// settled and its custody was abandoned).
    RolledBack {
        settled: bool,
    },
    /// The effects never returned their own step.
    Unsettled,
}

struct WakeCommit {
    run: WakeRun,
    rollback: Option<WakeRollback>,
    step: WakeStep,
}

impl MemberEffectCommit for WakeCommit {
    fn commit(
        self: Box<Self>,
        actor: &mut MobActor,
        settlement: MemberEffectSettlement,
    ) -> MemberEffectCommitFuture<'_> {
        Box::pin(async move {
            if actor.durable_uncertainty_fail_stop.is_set() {
                return MemberEffectAck::Retained(MemberEffectRetention::unresumable(
                    MobError::Internal("member wake commit is held by fail-stop".to_string()),
                ));
            }
            boxed_arm_future(|| actor.commit_member_wake_step(*self, settlement)).await;
            MemberEffectAck::Settled
        })
    }
}

/// Everything Step B reads, owned: the effect future outlives the actor's
/// borrow (never hand a borrowed item into a `Send` future).
struct WakeRestoreInput {
    definition: Arc<MobDefinition>,
    session_service: Arc<dyn MobSessionService>,
    provisioner: Arc<super::super::provisioner::MultiBackendProvisioner>,
    provisioner_dyn: Arc<dyn MobProvisioner>,
    continuity: RecordedContinuity,
    spawn_member_customizer: Option<Arc<dyn super::super::SpawnMemberCustomizer>>,
    tool_bundles: BTreeMap<String, Arc<dyn AgentToolDispatcher>>,
    tool_handle: MobHandle,
    default_external_tools_provider: Option<crate::ExternalToolsProvider>,
    realm_profile_store: Option<Arc<dyn crate::store::RealmProfileStore>>,
    default_llm_client: Option<Arc<dyn LlmClient>>,
    tool_consequence_policy_registry: Option<Arc<meerkat_core::ToolConsequencePolicyRegistry>>,
    member_build_preparer: Option<Arc<dyn super::super::MemberBuildPreparer>>,
    candidate: ResumeRestoreCandidate,
    base: mob_dsl::MobMachineAuthority,
    fork_rule: super::super::fork_build::ForkOverlayRule,
    overlays: BTreeMap<AgentIdentity, Option<Arc<dyn AgentToolDispatcher>>>,
    durable_endpoint: Option<mob_dsl::MemberPeerEndpoint>,
    member_bound_sessions: HashSet<SessionId>,
}

impl MobActor {
    /// Start every materialization MobMachine handed off since the last loop
    /// boundary.
    pub(in crate::runtime) async fn start_pending_member_materializations(&mut self) {
        while let Some(pending) = self.member_wakes.pending.pop_front() {
            self.start_member_materialization(pending).await;
        }
    }

    /// Step A: dispatch the cold restore of one authorized attempt.
    async fn start_member_materialization(&mut self, pending: PendingMemberMaterialization) {
        let PendingMemberMaterialization {
            identity,
            cause,
            attempt,
        } = pending;
        if !self.member_wake_attempt_is_open(&identity, attempt) {
            // The machine closed it before it started (a stop, a retire):
            // its callers are answered from the state the closure left.
            self.finish_closed_member_wake(&identity, attempt).await;
            return;
        }
        let run = WakeRun {
            identity: identity.clone(),
            attempt,
            cause,
        };
        self.member_wakes.running.insert(
            attempt,
            RunningMemberWake {
                identity: identity.clone(),
                cause,
            },
        );
        self.arm_member_wake_bound(&identity, attempt);

        let Some(entry) = self.roster.read().await.get(&identity).cloned() else {
            self.abandon_member_wake(&run, mob_dsl::MemberWakeAbandonKind::NotCurrent, None)
                .await;
            return;
        };
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&identity);
        let (machine_session_id, durable_endpoint, member_bound_sessions) = {
            let state = self.dsl_authority.state();
            (
                state.member_session_bindings.get(&dsl_identity).cloned(),
                state.member_peer_endpoints.get(&dsl_identity).cloned(),
                state
                    .member_session_bindings
                    .values()
                    .filter_map(|session| SessionId::parse(&session.0).ok())
                    .collect::<HashSet<_>>(),
            )
        };
        let Some(machine_session_id) = machine_session_id else {
            self.abandon_member_wake(&run, mob_dsl::MemberWakeAbandonKind::NotCurrent, None)
                .await;
            return;
        };
        if let Err(error) = self
            .member_wakes
            .continuity
            .refresh(&self.events, &self.definition.id)
            .await
        {
            tracing::warn!(
                mob_id = %self.definition.id,
                agent_identity = %identity,
                %error,
                "the member's continuity intent could not be read; its wake is retried later"
            );
            self.abandon_member_wake(&run, mob_dsl::MemberWakeAbandonKind::Unsettled, None)
                .await;
            return;
        }
        let continuity = self.member_wakes.continuity.slice_for(&identity);
        let fork_rule = self.fork_overlay_rule(&entry).await;
        // The overlay a fork follows: its in-mob source's retained overlay,
        // when that source was built in this process (it is Live); a dormant
        // source was not rebuilt, so the fork takes its own rule.
        let mut overlays = BTreeMap::new();
        if let super::super::fork_build::ForkOverlayRule::FollowSource(source) = &fork_rule
            && matches!(
                self.dsl_authority
                    .state()
                    .member_materialization
                    .get(&mob_dsl::AgentIdentity::from_domain(source)),
                Some(mob_dsl::MemberMaterializationPhase::Live)
            )
        {
            let retained = self.per_spawn_external_tools.read().await;
            overlays.insert(source.clone(), retained.get(source).cloned());
        }
        let input = WakeRestoreInput {
            definition: Arc::clone(&self.definition),
            session_service: Arc::clone(&self.session_service),
            provisioner: Arc::clone(&self.member_wakes.provisioner),
            provisioner_dyn: Arc::clone(&self.provisioner),
            continuity,
            spawn_member_customizer: self.spawn_member_customizer.clone(),
            tool_bundles: self.tool_bundles.clone(),
            tool_handle: self.mob_handle_for_tools(),
            default_external_tools_provider: self.default_external_tools_provider.clone(),
            realm_profile_store: self.realm_profile_store.clone(),
            default_llm_client: self.default_llm_client.clone(),
            tool_consequence_policy_registry: self.tool_consequence_policy_registry.clone(),
            member_build_preparer: self.member_residency.build_preparer.clone(),
            candidate: ResumeRestoreCandidate {
                entry: entry.clone(),
                machine_session_id,
                in_turn: false,
            },
            base: self.dsl_authority.fork(),
            fork_rule,
            overlays,
            durable_endpoint,
            member_bound_sessions,
        };
        self.dispatch_member_wake_step(
            run,
            MemberFence::exact(&entry),
            None,
            "member_wake_restore",
            async move { WakeStep::Planned(Box::new(plan_member_wake(input).await)) },
        );
    }

    fn dispatch_member_wake_step<F>(
        &mut self,
        run: WakeRun,
        fence: MemberFence,
        rollback: Option<WakeRollback>,
        context: &'static str,
        effects: F,
    ) where
        F: std::future::Future<Output = WakeStep> + MemberEffectSend + 'static,
    {
        let worker_run = run.clone();
        let worker_rollback = rollback.clone();
        self.dispatch_member_effect(MemberEffectRequest {
            context,
            members: vec![fence],
            effects: Box::pin(async move {
                Box::new(WakeCommit {
                    run: worker_run,
                    rollback: worker_rollback,
                    step: effects.await,
                }) as Box<dyn MemberEffectCommit>
            }),
            unsettled_commit: Box::new(WakeCommit {
                run,
                rollback,
                step: WakeStep::Unsettled,
            }),
        });
    }

    async fn commit_member_wake_step(
        &mut self,
        commit: WakeCommit,
        settlement: MemberEffectSettlement,
    ) {
        let WakeCommit {
            run,
            rollback,
            step,
        } = commit;
        if settlement.unsettled.is_some() || matches!(step, WakeStep::Unsettled) {
            // The effects never published their own result: whether the
            // provision happened is unknown, so the mob fail-stops (its
            // state is discarded), after it abandoned the attempt
            // explicitly so no caller waits on it.
            self.abandon_member_wake(&run, mob_dsl::MemberWakeAbandonKind::Unsettled, None)
                .await;
            let _ = self
                .durable_uncertainty_fail_stop
                .record(MobError::Internal(
                    "member wake effect ended without owned settlement evidence".to_string(),
                ));
            return;
        }
        match step {
            WakeStep::Planned(planned) => match *planned {
                Ok(plan) => {
                    self.commit_member_wake_plan(run, plan, settlement.fence.is_current())
                        .await;
                }
                Err(error) => self.fail_member_wake(&run, None, &error).await,
            },
            WakeStep::Ready(result) => {
                self.member_wake_ready(run, rollback, settlement.fence.is_current(), result)
                    .await;
            }
            WakeStep::Topology {
                mut topology,
                peer,
                edge,
                result,
            } => {
                if !settlement.fence.is_current()
                    || !self.member_wake_attempt_is_open(&run.identity, run.attempt)
                {
                    self.roll_back_member_wake(run, rollback);
                    return;
                }
                if let Err(error) = &result {
                    tracing::warn!(
                        mob_id = %self.definition.id,
                        agent_identity = %run.identity,
                        peer = %peer,
                        %error,
                        "a woken member's wiring edge could not be realized; its repair is owed"
                    );
                    topology.failed.push(peer);
                }
                self.resolve_member_edge_realization(&edge, result.is_ok());
                self.advance_member_wake_topology(run, rollback, topology)
                    .await;
            }
            WakeStep::RolledBack { settled } => {
                if !settled {
                    tracing::error!(
                        mob_id = %self.definition.id,
                        agent_identity = %run.identity,
                        attempt = run.attempt,
                        "a failed member wake could not roll back its provision"
                    );
                }
                if self.member_wake_attempt_is_open(&run.identity, run.attempt) {
                    // Rolled back while the attempt stayed open (a fenced
                    // incarnation change): close it, nothing is materialized.
                    self.abandon_member_wake(
                        &run,
                        mob_dsl::MemberWakeAbandonKind::NotCurrent,
                        None,
                    )
                    .await;
                } else {
                    self.finish_closed_member_wake(&run.identity, run.attempt)
                        .await;
                }
            }
            WakeStep::Unsettled => {}
        }
    }

    /// Step C: apply the member's plan in its turn.
    async fn commit_member_wake_plan(&mut self, run: WakeRun, plan: WakePlan, fence_current: bool) {
        let plan_rollback = match &plan {
            WakePlan::Restore { plan, .. } | WakePlan::Successor { plan, .. } => {
                WakeRollback::from_plan(plan)
            }
            WakePlan::NoSuccessor { .. } | WakePlan::EndpointMismatch { .. } => None,
        };
        if !fence_current || !self.member_wake_attempt_is_open(&run.identity, run.attempt) {
            // Stale: the machine closed the attempt, or the member's
            // incarnation changed, while it planned. Nothing of the plan is
            // applied; its provision is rolled back.
            self.roll_back_member_wake(run, plan_rollback);
            return;
        }
        let (plan, endpoint, successor) = match plan {
            WakePlan::NoSuccessor { bridge_session_id } => {
                let diagnostic = member_restore_failure(
                    &bridge_session_id,
                    format!("missing durable session snapshot for '{bridge_session_id}'"),
                );
                self.fail_member_wake_with(&run, diagnostic).await;
                return;
            }
            WakePlan::EndpointMismatch {
                bridge_session_id,
                reason,
            } => {
                let diagnostic = member_restore_failure(&bridge_session_id, reason);
                self.fail_member_wake_with(&run, diagnostic).await;
                return;
            }
            WakePlan::Restore { plan, endpoint } => (plan, endpoint, None),
            WakePlan::Successor {
                successor_session_id,
                plan,
                endpoint,
            } => (plan, endpoint, Some(successor_session_id)),
        };
        // Outcomes the cold recipe already decided are resolved here,
        // through the machine, instead of being written as a boot restore
        // writes them (a boot records a diagnostic and seeds Broken later).
        match &plan {
            MemberRestorePlan::Unseated(diagnostic) => {
                self.fail_member_wake_with(&run, diagnostic.clone()).await;
                return;
            }
            MemberRestorePlan::Seated {
                outcome: SeatedMemberRestoreOutcome::Refused(diagnostic),
                ..
            } => {
                self.fail_member_wake_with(&run, diagnostic.clone()).await;
                return;
            }
            MemberRestorePlan::Seated {
                outcome: SeatedMemberRestoreOutcome::Provision(provision),
                ..
            } => match &provision.outcome {
                MemberRestoreProvisionOutcome::Failed(diagnostic) => {
                    self.fail_member_wake_with(&run, diagnostic.clone()).await;
                    return;
                }
                MemberRestoreProvisionOutcome::Retryable { reason } => {
                    tracing::info!(
                        mob_id = %self.definition.id,
                        agent_identity = %run.identity,
                        %reason,
                        "the host asked to retry the member's build preparation"
                    );
                    self.member_wakes.running.remove(&run.attempt);
                    self.resolve_member_wake_retryable(
                        &run.identity,
                        run.attempt,
                        mob_dsl::MemberWakeRetryableKind::BuildPreparationRefused,
                        crate::MemberWakeRefusal::HostPreparationPending,
                        "member_wake_build_preparation_retryable",
                    )
                    .await;
                    return;
                }
                MemberRestoreProvisionOutcome::Durable { .. }
                | MemberRestoreProvisionOutcome::Fresh { .. } => {}
            },
            MemberRestorePlan::Seated {
                outcome: SeatedMemberRestoreOutcome::SuccessorNeeded { .. },
                ..
            } => {
                // Step B runs the successor search; reaching here is a
                // planning fault, never a delivery answer.
                self.fail_member_wake(
                    &run,
                    plan_rollback,
                    &MobError::Internal(
                        "a member wake reached its turn without its successor search".to_string(),
                    ),
                )
                .await;
                return;
            }
            MemberRestorePlan::Kept => {}
        }
        let Some(entry) = self.roster.read().await.get(&run.identity).cloned() else {
            self.roll_back_member_wake(run, plan_rollback);
            return;
        };
        if let Some(successor) = successor.as_ref() {
            // The successor repoint is durable history before it is
            // authority (journal first), exactly as a boot repoints.
            match append_recovered_session_binding(
                &mut self.dsl_authority,
                &self.events,
                &self.definition.id,
                &entry,
                successor,
                None,
                "member_wake_repoint_missing_member_session_binding",
            )
            .await
            {
                Ok(event) => self.roster.write().await.apply_event(&event),
                Err(error) => {
                    self.fail_member_wake(&run, plan_rollback, &error).await;
                    return;
                }
            }
        }
        let applied = self.apply_member_wake_plan(&entry, plan).await;
        self.publish_machine_state_projection();
        // The plan's seeded inputs ran on the authority directly; realize
        // the member's wake route from the state they left (a successor
        // repoint moved its session).
        self.publish_member_wake_route(&run.identity);
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&run.identity);
        let bound = self
            .dsl_authority
            .state()
            .member_wake_bound
            .contains(&dsl_identity);
        match applied {
            Ok(()) if bound => {}
            Ok(()) => {
                // The turn recorded a per-member failure (its binding or its
                // runtime binding was refused): Broken, through the machine.
                match self.restore_failure_for(&run.identity).await {
                    Some(diagnostic) => {
                        self.roll_back_member_wake_provision(&run, plan_rollback.clone());
                        self.fail_member_wake_with(&run, diagnostic).await;
                    }
                    None => {
                        self.fail_member_wake(
                            &run,
                            plan_rollback,
                            &MobError::Internal(
                                "a member wake's plan applied without binding the member"
                                    .to_string(),
                            ),
                        )
                        .await;
                    }
                }
                return;
            }
            Err(error) => {
                // A restore input MobMachine admitted on the member's fork was
                // refused on the mob's authority (the lifecycle moved): never
                // a failed mob, only this wake's typed answer and rollback.
                tracing::warn!(
                    mob_id = %self.definition.id,
                    agent_identity = %run.identity,
                    %error,
                    "a member wake's plan was refused in its turn"
                );
                self.roll_back_member_wake_provision(&run, plan_rollback);
                if self.member_wake_attempt_is_open(&run.identity, run.attempt) {
                    self.abandon_member_wake(
                        &run,
                        mob_dsl::MemberWakeAbandonKind::NotCurrent,
                        None,
                    )
                    .await;
                } else {
                    self.finish_closed_member_wake(&run.identity, run.attempt)
                        .await;
                }
                return;
            }
        }
        // Register the woken member's endpoint where none was durable yet,
        // before its Ready verdict: a member owed EndpointPending is never
        // Live without one. A registration MobMachine refuses fails the wake.
        if let WakeEndpoint::Record(spec) = endpoint
            && let Err(error) = self.record_woken_member_endpoint(&run.identity, spec).await
        {
            self.fail_member_wake(&run, plan_rollback, &error).await;
            return;
        }
        self.dispatch_member_wake_readiness(run, plan_rollback)
            .await;
    }

    /// Apply a plan through `apply_member_restore_plan` over the
    /// actor's own state: its authority, its roster, its retained overlays.
    async fn apply_member_wake_plan(
        &mut self,
        entry: &RosterEntry,
        plan: MemberRestorePlan,
    ) -> Result<(), MobError> {
        let tool_handle = self.mob_handle_for_tools();
        let no_active_ids = HashSet::new();
        let continuity = self
            .member_wakes
            .continuity
            .slice_for(&entry.agent_identity);
        let mut roster = self.roster.write().await;
        let mut retained_overlays = self.per_spawn_external_tools.write().await;
        let mut restored_overlays = BTreeMap::new();
        let mut successor_index = None;
        let mut recovered_without_endpoint = HashSet::new();
        let no_listed_sessions = Vec::new();
        let no_bound_sessions = HashSet::new();
        let ctx = ResumeMemberRestoreContext {
            definition: &self.definition,
            session_service: &self.session_service,
            provisioner: &self.member_wakes.provisioner,
            active_ids: &no_active_ids,
            recorded_continuity: &continuity,
            spawn_member_customizer: &self.spawn_member_customizer,
            tool_bundles: &self.tool_bundles,
            tool_handle: &tool_handle,
            default_external_tools_provider: &self.default_external_tools_provider,
            realm_profile_store: self.realm_profile_store.as_ref(),
            default_llm_client: self.default_llm_client.as_ref(),
            tool_consequence_policy_registry: self.tool_consequence_policy_registry.as_ref(),
            member_build_preparer: None,
        };
        let mut commit = MemberRestoreCommit {
            authority: &mut self.dsl_authority,
            roster: roster.restore_projection_mut(),
            per_spawn_external_tools_seed: &mut retained_overlays,
            restored_overlays: &mut restored_overlays,
            tool_handle: &tool_handle,
            composition_binding: &self.composition_binding,
        };
        let mut successors = MemberSuccessorSearch {
            listed_sessions: &no_listed_sessions,
            member_bound_sessions: &no_bound_sessions,
            index: &mut successor_index,
            recovered_without_endpoint: &mut recovered_without_endpoint,
        };
        apply_member_restore_plan(&ctx, &mut commit, &mut successors, entry, plan).await
    }

    /// Register the woken member's endpoint where none was durable: journal
    /// first, then authority and rosters, exactly as a boot upgrades a
    /// recovered binding without one. A wired member's recovered endpoint is
    /// refused there (its wiring predates any durable endpoint), so it is
    /// registered on its spawned incarnation instead
    /// (`RecoverSpawnedMemberPeerEndpoint`, which ends its EndpointPending
    /// exception); that registration lives in this process, and a later
    /// start wakes the member again to register it. Both refused is an
    /// error: the member cannot be Live without an endpoint.
    async fn record_woken_member_endpoint(
        &mut self,
        identity: &AgentIdentity,
        spec: TrustedPeerDescriptor,
    ) -> Result<(), MobError> {
        let Some(entry) = self.roster.read().await.get(identity).cloned() else {
            return Ok(());
        };
        let Some(bridge_session_id) = entry.member_ref.bridge_session_id().cloned() else {
            return Ok(());
        };
        match append_recovered_session_binding(
            &mut self.dsl_authority,
            &self.events,
            &self.definition.id,
            &entry,
            &bridge_session_id,
            Some(spec.clone()),
            "member_wake_record_member_peer_endpoint",
        )
        .await
        {
            Ok(event) => self.roster.write().await.apply_event(&event),
            Err(recovered_error) => {
                self.apply_dsl_signal(
                    mob_dsl::MobMachineSignal::RecoverSpawnedMemberPeerEndpoint {
                        agent_identity: mob_dsl::AgentIdentity::from_domain(identity),
                        agent_runtime_id: mob_dsl::AgentRuntimeId::from_domain(
                            &entry.agent_runtime_id,
                        ),
                        fence_token: mob_dsl::FenceToken::from_domain(entry.fence_token),
                        generation: mob_dsl::Generation::from_domain(entry.generation),
                        peer_endpoint: mob_dsl::MemberPeerEndpoint::from(&spec),
                    },
                    "member_wake_register_spawned_member_peer_endpoint",
                )
                .map_err(|spawned_error| {
                    MobError::Internal(format!(
                        "the woken member '{identity}' could not register its endpoint: \
                         {recovered_error}; on its spawned incarnation: {spawned_error}"
                    ))
                })?;
            }
        }
        self.publish_machine_state_projection();
        self.publish_member_wake_route(identity);
        Ok(())
    }

    /// Readiness: the woken member's comms drain, as every readiness path
    /// requires it ([`super::member_requires_comms_drain`]).
    async fn dispatch_member_wake_readiness(
        &mut self,
        run: WakeRun,
        rollback: Option<WakeRollback>,
    ) {
        let Some(entry) = self.roster.read().await.get(&run.identity).cloned() else {
            self.roll_back_member_wake(run, rollback);
            return;
        };
        let drain_required = super::member_requires_comms_drain(self.dsl_authority.state(), &entry);
        let readiness = self.detached_member_readiness_context();
        let identity = run.identity.clone();
        let member_ref = entry.member_ref.clone();
        self.dispatch_member_wake_step(
            run,
            MemberFence::exact(&entry),
            rollback,
            "member_wake_readiness",
            async move {
                WakeStep::Ready(
                    readiness
                        .ensure_member_comms_drain(&identity, &member_ref, drain_required)
                        .await,
                )
            },
        );
    }

    async fn member_wake_ready(
        &mut self,
        run: WakeRun,
        rollback: Option<WakeRollback>,
        fence_current: bool,
        result: Result<(), MobError>,
    ) {
        if !fence_current || !self.member_wake_attempt_is_open(&run.identity, run.attempt) {
            self.roll_back_member_wake(run, rollback);
            return;
        }
        if let Err(error) = result {
            self.roll_back_member_wake_provision(&run, rollback);
            self.abandon_member_wake(
                &run,
                mob_dsl::MemberWakeAbandonKind::ReadinessFailed,
                Some(format!(
                    "the woken member's comms readiness failed: {error}"
                )),
            )
            .await;
            return;
        }
        let plan = match self.machine_restore_wiring_plan(&run.identity) {
            Ok(plan) => plan,
            Err(error) => {
                self.roll_back_member_wake_provision(&run, rollback);
                self.abandon_member_wake(
                    &run,
                    mob_dsl::MemberWakeAbandonKind::TopologyFailed,
                    Some(format!("the woken member's wiring plan failed: {error}")),
                )
                .await;
                return;
            }
        };
        let local = mob_dsl::AgentIdentity::from_domain(&run.identity);
        let mut remaining: VecDeque<_> = plan
            .local_peers
            .into_iter()
            .filter(|peer| peer != &run.identity)
            .map(|peer| {
                let key = RespawnTopologyPeerId::from(peer.as_str());
                let edge = MemberWiringEdge::Member(mob_dsl::WiringEdge::new(
                    local.clone(),
                    mob_dsl::AgentIdentity::from_domain(&peer),
                ));
                (super::super::handle::PeerTarget::Local(peer), key, edge)
            })
            .collect();
        // The member's external edges, keyed as MobMachine keys them.
        let external_edges: Vec<mob_dsl::ExternalPeerEdge> = self
            .dsl_authority
            .state()
            .external_peer_edges_by_key
            .values()
            .filter(|edge| edge.local == local)
            .cloned()
            .collect();
        for edge in external_edges {
            match Self::trusted_peer_descriptor_from_machine_endpoint(&edge.endpoint) {
                Ok(descriptor) => {
                    let key = RespawnTopologyPeerId::from(descriptor.peer_id.as_str());
                    remaining.push_back((
                        super::super::handle::PeerTarget::External(descriptor),
                        key,
                        MemberWiringEdge::External(edge),
                    ));
                }
                Err(error) => {
                    self.roll_back_member_wake_provision(&run, rollback);
                    self.abandon_member_wake(
                        &run,
                        mob_dsl::MemberWakeAbandonKind::TopologyFailed,
                        Some(format!("the woken member's wiring plan failed: {error}")),
                    )
                    .await;
                    return;
                }
            }
        }
        self.advance_member_wake_topology(
            run,
            rollback,
            WakeTopology {
                remaining,
                failed: Vec::new(),
            },
        )
        .await;
    }

    /// Topology: restore the woken member's wiring over the machine's edges,
    /// one edge at a time on the wiring lane, then resolve it Ready.
    async fn advance_member_wake_topology(
        &mut self,
        run: WakeRun,
        rollback: Option<WakeRollback>,
        mut topology: WakeTopology,
    ) {
        while let Some((target, peer, edge)) = topology.remaining.pop_front() {
            #[cfg(test)]
            if take_wake_edge_failure_for_test(&run.identity) {
                tracing::warn!(
                    mob_id = %self.definition.id,
                    agent_identity = %run.identity,
                    peer = %peer,
                    "test seam: a woken member's wiring edge fails; its repair is owed"
                );
                topology.failed.push(peer);
                self.resolve_member_edge_realization(&edge, false);
                continue;
            }
            match self.prepare_member_wire(run.identity.clone(), target).await {
                Ok(prepared) => {
                    if let Some(prepared) = prepared.into_prepared() {
                        let continuation = self.realize_wiring_detached(*prepared);
                        let Some(entry) = self.roster.read().await.get(&run.identity).cloned()
                        else {
                            self.roll_back_member_wake(run, rollback);
                            return;
                        };
                        self.dispatch_member_wake_step(
                            run,
                            MemberFence::exact(&entry),
                            rollback,
                            "member_wake_topology",
                            async move {
                                let result = continuation.settled().await;
                                WakeStep::Topology {
                                    topology,
                                    peer,
                                    edge: Box::new(edge),
                                    result,
                                }
                            },
                        );
                        return;
                    }
                    // Settled inside its preparation: the edge is realized.
                    self.resolve_member_edge_realization(&edge, true);
                }
                Err(error) => {
                    tracing::warn!(
                        mob_id = %self.definition.id,
                        agent_identity = %run.identity,
                        peer = %peer,
                        %error,
                        "a woken member's wiring edge could not be prepared; its repair is owed"
                    );
                    topology.failed.push(peer);
                    self.resolve_member_edge_realization(&edge, false);
                }
            }
        }
        if let Err(error) =
            self.resolve_respawn_topology_restore_result(&run.identity, topology.failed)
        {
            self.roll_back_member_wake_provision(&run, rollback);
            self.abandon_member_wake(
                &run,
                mob_dsl::MemberWakeAbandonKind::TopologyFailed,
                Some(format!("the woken member's topology failed: {error}")),
            )
            .await;
            return;
        }
        self.resolve_member_wake_ready(run, rollback).await;
    }

    /// Waking -> Live, after readiness and topology settled (Live means
    /// ready).
    async fn resolve_member_wake_ready(&mut self, run: WakeRun, rollback: Option<WakeRollback>) {
        let verdict = self.apply_member_wake_feedback(
            &run.identity,
            run.attempt,
            mob_dsl::MobMachineSignal::ResolveMemberRevivalSucceeded {
                agent_identity: mob_dsl::AgentIdentity::from_domain(&run.identity),
                attempt: run.attempt,
            },
            "member_wake_ready",
        );
        match verdict {
            mob_dsl::MemberWakeFeedbackVerdict::Applied => {
                self.member_wakes.running.remove(&run.attempt);
                self.clear_member_restore_failure(&run.identity).await;
                self.member_wakes
                    .answer(run.attempt, Ok(MemberWakeOutcome::Ready));
                self.report_member_built_after_wake(&run.identity).await;
                self.schedule_owed_member_wake_retry(&run.identity);
                tracing::debug!(
                    mob_id = %self.definition.id,
                    agent_identity = %run.identity,
                    attempt = run.attempt,
                    cause = ?run.cause,
                    "member woken"
                );
            }
            mob_dsl::MemberWakeFeedbackVerdict::StaleAttempt => {
                // The attempt ended while it settled (a stop): the machine
                // does not hold this materialization, so it is rolled back.
                self.roll_back_member_wake(run, rollback);
            }
        }
    }

    /// Report a woken member's build to the host's preparer (off the actor).
    async fn report_member_built_after_wake(&mut self, identity: &AgentIdentity) {
        let Some(session_id) = self
            .roster
            .read()
            .await
            .get(identity)
            .and_then(|entry| entry.member_ref.bridge_session_id().cloned())
        else {
            return;
        };
        self.report_member_built_to_host(identity, session_id);
    }

    /// Report a member Core built live on `session_id` (a wake's, or a
    /// spawn's once its activation committed) to the host's preparer, off
    /// the actor. Nothing happens without a preparer.
    pub(in crate::runtime) fn report_member_built_to_host(
        &mut self,
        identity: &AgentIdentity,
        session_id: SessionId,
    ) {
        let Some(preparer) = self.member_residency.build_preparer.clone() else {
            return;
        };
        let session_service = Arc::clone(&self.session_service);
        self.actor_io_tasks
            .spawn(super::super::member_build_preparer::report_member_built(
                Some(preparer),
                session_service,
                identity.clone(),
                session_id,
            ));
    }

    /// Resolve `run`'s attempt Broken for `error` (the recorded reason keeps a
    /// host refusal typed), roll back its provision, answer its callers.
    async fn fail_member_wake(
        &mut self,
        run: &WakeRun,
        rollback: Option<WakeRollback>,
        error: &MobError,
    ) {
        self.roll_back_member_wake_provision(run, rollback);
        let session_id = self
            .roster
            .read()
            .await
            .get(&run.identity)
            .and_then(|entry| entry.member_ref.bridge_session_id().cloned());
        let diagnostic = super::super::handle::RestoreFailureDiagnostic {
            bridge_session_id: session_id,
            reason: error.to_string(),
            hold: error.durable_resume_hold(),
            registration_refusal: match error {
                MobError::MemberRegistrationRefused { call, .. } => Some(*call),
                _ => None,
            },
        };
        self.fail_member_wake_with(run, diagnostic).await;
    }

    /// Resolve `run`'s attempt Broken with `diagnostic` and answer its
    /// callers with the Broken answer.
    async fn fail_member_wake_with(
        &mut self,
        run: &WakeRun,
        diagnostic: super::super::handle::RestoreFailureDiagnostic,
    ) {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&run.identity);
        let signal = match diagnostic.registration_refusal {
            Some(call) => mob_dsl::MobMachineSignal::ResolveMemberRevivalHostRefused {
                agent_identity: dsl_identity,
                refusal_code: call.as_code().to_string(),
                reason: diagnostic.reason.clone(),
                attempt: run.attempt,
            },
            None => mob_dsl::MobMachineSignal::ResolveMemberRevivalFailed {
                agent_identity: dsl_identity,
                reason: diagnostic.reason.clone(),
                attempt: run.attempt,
            },
        };
        let verdict = self.apply_member_wake_feedback(
            &run.identity,
            run.attempt,
            signal,
            "member_wake_failed",
        );
        self.member_wakes.running.remove(&run.attempt);
        match verdict {
            mob_dsl::MemberWakeFeedbackVerdict::Applied => {
                self.record_member_restore_failure(&run.identity, diagnostic)
                    .await;
                let failure = self.member_wake_broken(&run.identity).await;
                self.member_wakes.answer(run.attempt, Err(failure));
            }
            mob_dsl::MemberWakeFeedbackVerdict::StaleAttempt => {
                self.finish_closed_member_wake(&run.identity, run.attempt)
                    .await;
            }
        }
    }

    /// End `run`'s attempt without a Broken verdict of its own
    /// (`AbandonMemberWake`): NotCurrent, ReloadRefused and Unsettled leave
    /// the member Dormant; ReadinessFailed and TopologyFailed leave it Broken
    /// with `reason`.
    async fn abandon_member_wake(
        &mut self,
        run: &WakeRun,
        kind: mob_dsl::MemberWakeAbandonKind,
        reason: Option<String>,
    ) {
        let reason_text = reason.clone().unwrap_or_default();
        let verdict = self.apply_member_wake_feedback(
            &run.identity,
            run.attempt,
            mob_dsl::MobMachineSignal::AbandonMemberWake {
                agent_identity: mob_dsl::AgentIdentity::from_domain(&run.identity),
                attempt: run.attempt,
                kind,
                reason: reason_text.clone(),
            },
            "member_wake_abandoned",
        );
        self.member_wakes.running.remove(&run.attempt);
        if verdict == mob_dsl::MemberWakeFeedbackVerdict::StaleAttempt {
            self.finish_closed_member_wake(&run.identity, run.attempt)
                .await;
            return;
        }
        let broken = matches!(
            kind,
            mob_dsl::MemberWakeAbandonKind::ReadinessFailed
                | mob_dsl::MemberWakeAbandonKind::TopologyFailed
        );
        if broken {
            let session_id = self
                .roster
                .read()
                .await
                .get(&run.identity)
                .and_then(|entry| entry.member_ref.bridge_session_id().cloned());
            self.record_member_restore_failure(
                &run.identity,
                super::super::handle::RestoreFailureDiagnostic {
                    bridge_session_id: session_id,
                    reason: reason_text,
                    hold: None,
                    registration_refusal: None,
                },
            )
            .await;
            let failure = self.member_wake_broken(&run.identity).await;
            self.member_wakes.answer(run.attempt, Err(failure));
        } else {
            let refusal = match kind {
                mob_dsl::MemberWakeAbandonKind::ReloadRefused => {
                    crate::MemberWakeRefusal::ResumeGateUnavailable
                }
                mob_dsl::MemberWakeAbandonKind::Unsettled => {
                    crate::MemberWakeRefusal::MobUnavailable
                }
                _ => crate::MemberWakeRefusal::WakeTimedOut,
            };
            self.member_wakes
                .answer(run.attempt, Err(MemberWakeFailure::Refused(refusal)));
            self.schedule_owed_member_wake_retry(&run.identity);
        }
    }

    /// Roll back the provision a wake made (off the actor), then finish the
    /// attempt by the state the rollback leaves.
    fn roll_back_member_wake(&mut self, run: WakeRun, rollback: Option<WakeRollback>) {
        let Some(rollback) = rollback else {
            let identity = run.identity.clone();
            let attempt = run.attempt;
            // Nothing provisioned: finish at the next commit point.
            self.member_wakes.running.remove(&attempt);
            self.member_wakes.closed_answer_later(identity, attempt);
            return;
        };
        let provision = rollback.pending(&run.identity, Arc::clone(&self.provisioner));
        let fence = match self.member_wake_fence_or_absent(&run.identity) {
            Some(fence) => fence,
            None => MemberFence::ExpectedAbsent(run.identity.clone()),
        };
        self.dispatch_member_wake_step(run, fence, None, "member_wake_rollback", async move {
            match provision.rollback_retaining_custody().await {
                Ok(()) => WakeStep::RolledBack { settled: true },
                Err(retained) => {
                    let _ = retained.abandon("member wake rollback could not settle");
                    WakeStep::RolledBack { settled: false }
                }
            }
        });
    }

    /// Roll back a wake's provision without waiting for it (the caller
    /// resolves the attempt itself).
    fn roll_back_member_wake_provision(&mut self, run: &WakeRun, rollback: Option<WakeRollback>) {
        let Some(rollback) = rollback else {
            return;
        };
        let provision = rollback.pending(&run.identity, Arc::clone(&self.provisioner));
        let identity = run.identity.clone();
        let mob_id = self.definition.id.clone();
        self.actor_io_tasks.spawn(async move {
            if let Err(retained) = provision.rollback_retaining_custody().await {
                let _ = retained.abandon("member wake rollback could not settle");
                tracing::error!(
                    mob_id = %mob_id,
                    agent_identity = %identity,
                    "a failed member wake could not roll back its provision"
                );
            }
        });
    }

    /// The exact fence of `identity`'s current incarnation, if it is seated.
    fn member_wake_fence_or_absent(&self, identity: &AgentIdentity) -> Option<MemberFence> {
        self.roster
            .try_read()
            .ok()
            .and_then(|roster| roster.get(identity).cloned())
            .map(|entry| MemberFence::exact(&entry))
    }
}

/// Step B: plan the member's restore off the actor.
async fn plan_member_wake(input: WakeRestoreInput) -> Result<WakePlan, MobError> {
    let WakeRestoreInput {
        definition,
        session_service,
        provisioner,
        provisioner_dyn,
        continuity,
        spawn_member_customizer,
        tool_bundles,
        tool_handle,
        default_external_tools_provider,
        realm_profile_store,
        default_llm_client,
        tool_consequence_policy_registry,
        member_build_preparer,
        candidate,
        base,
        fork_rule,
        overlays,
        durable_endpoint,
        member_bound_sessions,
    } = input;
    let bridge_session_id = SessionId::parse(&candidate.machine_session_id.0).map_err(|error| {
        MobError::Internal(format!(
            "member wake found invalid session binding '{}' for '{}': {error}",
            candidate.machine_session_id.0, candidate.entry.agent_identity
        ))
    })?;
    // This member's own liveness observation (the wake path's drift
    // observation): a session actor still live in this process is kept.
    let mut active_ids = HashSet::new();
    if session_service
        .live_session_actor_registered(&bridge_session_id)
        .await
        .unwrap_or(false)
    {
        active_ids.insert(bridge_session_id.clone());
    }
    let ctx = ResumeMemberRestoreContext {
        definition: &definition,
        session_service: &session_service,
        provisioner: &provisioner,
        active_ids: &active_ids,
        recorded_continuity: &continuity,
        spawn_member_customizer: &spawn_member_customizer,
        tool_bundles: &tool_bundles,
        tool_handle: &tool_handle,
        default_external_tools_provider: &default_external_tools_provider,
        realm_profile_store: realm_profile_store.as_ref(),
        default_llm_client: default_llm_client.as_ref(),
        tool_consequence_policy_registry: tool_consequence_policy_registry.as_ref(),
        member_build_preparer: member_build_preparer.as_ref(),
    };
    let plan = restore_resumed_member(&ctx, &candidate, &base, fork_rule, &overlays).await?;
    let entry = &candidate.entry;
    let (seating, missing, mut seated) = match plan {
        MemberRestorePlan::Seated {
            seating,
            outcome:
                SeatedMemberRestoreOutcome::SuccessorNeeded {
                    bridge_session_id,
                    seated,
                },
        } => (seating, bridge_session_id, *seated),
        plan => {
            return match check_woken_member_endpoint(
                &provisioner,
                &definition,
                entry,
                &plan,
                durable_endpoint.as_ref(),
            )
            .await
            {
                Ok(endpoint) => Ok(WakePlan::Restore { plan, endpoint }),
                Err(reason) => {
                    if let Some(rollback) = WakeRollback::from_plan(&plan) {
                        let provision = rollback.pending(&entry.agent_identity, provisioner_dyn);
                        if let Err(retained) = provision.rollback_retaining_custody().await {
                            let _ = retained.abandon("member wake endpoint mismatch rollback");
                        }
                    }
                    Ok(WakePlan::EndpointMismatch {
                        bridge_session_id,
                        reason,
                    })
                }
            };
        }
    };
    // The bound session is absent: its latest persisted successor, found
    // the way a boot finds it, provisioned on a fork repointed to it.
    let listed = session_service
        .list(meerkat_core::service::SessionQuery::default())
        .await?;
    let index = PersistedMemberSessionIndex::scan(
        session_service.as_ref(),
        &listed,
        &member_bound_sessions,
        || {},
    )
    .await?;
    let Some((successor, authorized)) = index
        .latest_for_member(
            session_service.as_ref(),
            &missing,
            &definition.id,
            &entry.role,
            &entry.agent_identity,
            seated.restore_resume_from_role.as_ref(),
        )
        .await?
    else {
        return Ok(WakePlan::NoSuccessor {
            bridge_session_id: missing,
        });
    };
    let rebuilt = provisioner
        .retire_exact_attachment_for_explicit_resume(&successor)
        .await?;
    let successor_live = !rebuilt
        && session_service
            .live_session_actor_registered(&successor)
            .await
            .unwrap_or(false);
    if successor_live {
        // A successor live in this process keeps serving when it runs the
        // binding the member's spec names (otherwise the wake fails, typed,
        // and the live actor is retired, not reused): repoint and bind it,
        // nothing is provisioned.
        if let Some(requested) = seated.restore_spec.auth_binding.as_ref()
            && let Err(conflict) = super::super::live_actor_binding::verify_live_actor_auth_binding(
                session_service.as_ref(),
                &entry.agent_identity,
                &successor,
                requested,
            )
            .await
        {
            let reason = super::super::live_actor_binding::refuse_conflicting_live_actor(
                session_service.as_ref(),
                &successor,
                &conflict,
            )
            .await;
            return Ok(WakePlan::Restore {
                plan: MemberRestorePlan::Unseated(member_restore_failure(&successor, reason)),
                endpoint: WakeEndpoint::Matches,
            });
        }
        let endpoint = live_member_endpoint(&provisioner, &definition, entry, &successor).await;
        return Ok(WakePlan::Successor {
            successor_session_id: successor,
            plan: MemberRestorePlan::Kept,
            endpoint,
        });
    }
    if entry.fork_source.is_none() && spawn_member_customizer.is_some() {
        let view = match super::super::fork_build::load_resumed_member_view(
            session_service.as_ref(),
            &successor,
        )
        .await
        {
            Ok(view) => view,
            Err(error) => {
                return Ok(WakePlan::Restore {
                    plan: MemberRestorePlan::Unseated(member_restore_failure(
                        &successor,
                        error.to_string(),
                    )),
                    endpoint: WakeEndpoint::Matches,
                });
            }
        };
        seated.restore_spec.resume_provider_params =
            match super::super::fork_build::successor_resume_provider_params(
                &definition.id,
                spawn_member_customizer.as_ref(),
                entry,
                &view,
            ) {
                Ok(params) => params,
                Err(error) => {
                    return Ok(WakePlan::Restore {
                        plan: MemberRestorePlan::Unseated(member_restore_failure(
                            &successor,
                            error.to_string(),
                        )),
                        endpoint: WakeEndpoint::Matches,
                    });
                }
            };
    }
    let mut repointed = base.fork();
    let dsl_identity = mob_dsl::AgentIdentity::from_domain(&entry.agent_identity);
    let replacing = repointed
        .state()
        .member_session_bindings
        .get(&dsl_identity)
        .cloned();
    repointed
        .apply_signal(mob_dsl::MobMachineSignal::RecoverMemberSessionBinding {
            agent_identity: dsl_identity,
            agent_runtime_id: mob_dsl::AgentRuntimeId::from_domain(&entry.agent_runtime_id),
            bridge_session_id: mob_dsl::SessionId::from_domain(&successor),
            replacing,
        })
        .map_err(|error| {
            MobError::Internal(format!(
                "member wake could not repoint '{}' to its successor on its own fork: {error}",
                entry.agent_identity
            ))
        })?;
    let provision =
        provision_durable_member_restore(&ctx, entry, &seated, &successor, authorized, &repointed)
            .await?;
    let plan = MemberRestorePlan::Seated {
        seating,
        outcome: SeatedMemberRestoreOutcome::Provision(Box::new(provision)),
    };
    // The successor's endpoint is recorded after the repoint, as a boot
    // records a repointed member's endpoint.
    let endpoint = match plan_member_ref(&plan) {
        Some(member_ref) => {
            live_member_endpoint_for_ref(&provisioner, &definition, entry, &member_ref).await
        }
        None => WakeEndpoint::Matches,
    };
    Ok(WakePlan::Successor {
        successor_session_id: successor,
        plan,
        endpoint,
    })
}

/// The member ref a plan provisioned.
fn plan_member_ref(plan: &MemberRestorePlan) -> Option<MemberRef> {
    WakeRollback::from_plan(plan).map(|rollback| rollback.member_ref)
}

/// Check the woken comms runtime's endpoint against the durable one: a
/// changed endpoint is refused (the member's peers trust its durable key); a
/// missing durable one is recorded after the plan applied.
async fn check_woken_member_endpoint(
    provisioner: &super::super::provisioner::MultiBackendProvisioner,
    definition: &MobDefinition,
    entry: &RosterEntry,
    plan: &MemberRestorePlan,
    durable: Option<&mob_dsl::MemberPeerEndpoint>,
) -> Result<WakeEndpoint, String> {
    let Some(member_ref) = plan_member_ref(plan) else {
        return Ok(WakeEndpoint::Matches);
    };
    let WakeEndpoint::Record(spec) =
        live_member_endpoint_for_ref(provisioner, definition, entry, &member_ref).await
    else {
        return Ok(WakeEndpoint::Matches);
    };
    let candidate = mob_dsl::MemberPeerEndpoint::from(&spec);
    match durable {
        Some(durable) if durable != &candidate => {
            Err(member_endpoint_mismatch_detail(&candidate, durable))
        }
        Some(_) => Ok(WakeEndpoint::Matches),
        None => Ok(WakeEndpoint::Record(spec)),
    }
}

async fn live_member_endpoint(
    provisioner: &super::super::provisioner::MultiBackendProvisioner,
    definition: &MobDefinition,
    entry: &RosterEntry,
    session_id: &SessionId,
) -> WakeEndpoint {
    let member_ref = MemberRef::from_bridge_session_id(session_id.clone());
    live_member_endpoint_for_ref(provisioner, definition, entry, &member_ref).await
}

/// The endpoint the member's live comms runtime presents, as a boot reads
/// it; `Matches` when there is none to read.
async fn live_member_endpoint_for_ref(
    provisioner: &super::super::provisioner::MultiBackendProvisioner,
    definition: &MobDefinition,
    entry: &RosterEntry,
    member_ref: &MemberRef,
) -> WakeEndpoint {
    let Some(comms) = provisioner.comms_runtime(member_ref).await else {
        return WakeEndpoint::Matches;
    };
    let Some(key) = comms.public_key() else {
        return WakeEndpoint::Matches;
    };
    let Ok(name) = render_member_comms_name(
        definition.id.as_str(),
        entry.role.as_str(),
        entry.agent_identity.as_str(),
    ) else {
        return WakeEndpoint::Matches;
    };
    match provisioner.trusted_peer_spec(member_ref, &name, &key).await {
        Ok(spec) => WakeEndpoint::Record(spec),
        Err(_) => WakeEndpoint::Matches,
    }
}

/// Test seam: members whose next wake treats one wiring edge as failed (a
/// transient failure), consumed once per arm.
#[cfg(test)]
static WAKE_EDGE_FAILURES_FOR_TEST: std::sync::Mutex<Vec<AgentIdentity>> =
    std::sync::Mutex::new(Vec::new());

/// Make the next wake of `identity` fail one of its wiring edges.
#[cfg(test)]
pub(in crate::runtime) fn fail_next_wake_edge_for_test(identity: &AgentIdentity) {
    WAKE_EDGE_FAILURES_FOR_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(identity.clone());
}

#[cfg(test)]
fn take_wake_edge_failure_for_test(identity: &AgentIdentity) -> bool {
    let mut failures = WAKE_EDGE_FAILURES_FOR_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match failures.iter().position(|armed| armed == identity) {
        Some(index) => {
            failures.remove(index);
            true
        }
        None => false,
    }
}
