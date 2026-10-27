//! The clean-stop lift's member plan.
//!
//! A mob that was stopped and is lifted again ([`super::super::MobHandle::resume`])
//! plans its members exactly as a resume of a running mob does: once the
//! durable Resume made the mob Running again, one `PlanBootMaterialization`
//! per Dormant member, with the start's restore policy and host inputs and
//! one fresh runtime obligation scan. A member the plan selects is Waking
//! under the plan's wake attempt; the actor's wake path materializes it
//! through the one cold recipe every wake uses. The explicit resume joins
//! each such wake (`AuthorizeExplicitResumeMember` answers Join), waits for
//! its attempt to close (Live, Broken, or Dormant again for a retryable
//! cause) and settles it, so the lift answers its caller only after the
//! members it materializes settled. A member is never materialized twice:
//! one attempt per member, and every other wake of it joins that attempt.
//! Every other member stays Dormant and is materialized by its first
//! delivery.
//!
//! The plan is dispatched at most the mob's restore concurrency at a time
//! (`MobBuilder::with_resume_restore_concurrency`): the lift applies
//! `PlanBootMaterialization` to one Dormant member after another until that
//! many of its wakes are open, and to the next members only as those wakes
//! close. A member the plan has not reached yet stays Dormant, its true
//! state: it holds no wake attempt, so no wake bound runs for it, and a
//! delivery wakes it at once through a wake of its own, never queued behind
//! the plan. When the plan reaches a member a delivery woke meanwhile, it
//! applies nothing to it while that wake runs (never a second wake): it
//! watches that wake, counts it against its bound and waits for it, and if
//! the wake leaves the member Dormant again (it ended retryably) the plan
//! reaches the member again and applies its input then, so the member's
//! boot work is owed and retried as for any member the plan materializes. A
//! member the plan finds Live or Broken is left as it is. The plan
//! dispatches nothing more once the mob leaves Running or the lift is
//! cancelled or superseded. The members the
//! plan verifies (`OnDemandVerified`) are read once it has reached every
//! member, those still Dormant then: a verified member a delivery woke
//! meanwhile reads its durable session in its own wake. A start of a
//! running mob plans before the actor exists and restores its boot set at
//! the same bound (`restore_resumed_members`).

use super::*;

/// One wake of the lift's plan, as the explicit resume tracks it: a member
/// Waking under the plan's wake attempt, joined by the lift, or a member a
/// delivery woke before the plan reached it, whose own wake the plan
/// watches.
#[derive(Debug, Clone)]
pub(in crate::runtime) struct ExplicitResumeBootMember {
    pub(in crate::runtime) agent_identity: AgentIdentity,
    resume_attempt: mob_dsl::ResumeAttemptId,
    binding: mob_dsl::ResumeMemberBinding,
    wake_attempt: u64,
    /// Whether the explicit resume takes this wake over
    /// (`AuthorizeExplicitResumeMember`): a wake of the plan's own, until
    /// MobMachine refuses the take-over. A wake the plan only watches (a
    /// delivery's) gets no input. A wake the lift does not take over still
    /// frees its slot of the plan when it closes; the lift has nothing of it
    /// to settle.
    authorized: bool,
    /// Whether the plan only watches this wake: a delivery opened it before
    /// the plan reached the member, and the plan opened none.
    watched: bool,
}

/// The lift's plan between two dispatches: the members it has not reached
/// yet and its wakes still open (see the module documentation).
pub(in crate::runtime) struct ExplicitResumeBootDispatch {
    /// The lift attempt the plan belongs to.
    resume_attempt: mob_dsl::ResumeAttemptId,
    /// The lift's one runtime obligation scan.
    obligations: super::super::boot_plan::BootObligations,
    /// The orchestrators the lift notifies.
    notified: BTreeSet<AgentIdentity>,
    /// The members Dormant when the plan started, in identity order, that it
    /// has not reached yet.
    queue: VecDeque<mob_dsl::AgentIdentity>,
    /// The wakes the plan counts against its bound, by member, with their
    /// attempts: its own, and the ones it watches (`watched`).
    open: BTreeMap<mob_dsl::AgentIdentity, u64>,
    /// The members of `open` a delivery woke before the plan reached them:
    /// the plan applied nothing to them, and reaches them again once that
    /// wake closes.
    watched: BTreeSet<mob_dsl::AgentIdentity>,
    /// The members the plan verified (`VerifyMember`), read once it has
    /// reached every member (those still Dormant then).
    verify: BTreeSet<AgentIdentity>,
    /// How many of the plan's wakes may be open at once: the restore
    /// concurrency.
    bound: usize,
}

impl MobActor {
    /// Start the lift's member plan on the live authority, at most the
    /// restore concurrency of its wakes open at once (see the module
    /// documentation). Every effect goes through the actor's apply seam, so
    /// the wake path realizes each `MaterializeMember` it emits; this returns
    /// the members it made Waking now, for the explicit resume to join. The
    /// rest are dispatched as those wakes close
    /// ([`Self::dispatch_explicit_resume_boot_members`]).
    pub(super) async fn plan_explicit_resume_boot_set(
        &mut self,
    ) -> Result<Vec<ExplicitResumeBootMember>, MobError> {
        let resume_attempt = self
            .dsl_authority
            .state()
            .explicit_resume_attempt
            .clone()
            .ok_or_else(|| {
                MobError::Internal("the lift's member plan lost its resume attempt".to_string())
            })?;
        #[cfg(feature = "runtime-adapter")]
        let obligations = super::super::boot_plan::scan_boot_obligations(
            self.runtime_adapter.as_ref(),
            self.dsl_authority.state(),
            &self.definition.id,
            self.incarnation.incarnation(),
        )
        .await?;
        #[cfg(not(feature = "runtime-adapter"))]
        let obligations = super::super::boot_plan::BootObligations::none();
        self.boot_set_basis.set(obligations.basis);
        let notified = super::super::boot_plan::notified_orchestrators(
            &self.definition,
            self.notify_orchestrator_on_resume,
            self.dsl_authority.state(),
        );
        let queue: VecDeque<mob_dsl::AgentIdentity> =
            super::super::boot_plan::dormant_members(self.dsl_authority.state()).into();
        let bound = self.member_residency.verify_concurrency.max(1);
        self.explicit_resume_boot_dispatch = Some(ExplicitResumeBootDispatch {
            resume_attempt,
            obligations,
            notified,
            queue,
            open: BTreeMap::new(),
            watched: BTreeSet::new(),
            verify: BTreeSet::new(),
            bound,
        });
        self.dispatch_explicit_resume_boot_members().await
    }

    /// Apply the lift's plan to the next members while fewer than its bound
    /// of wakes are open, and return the wakes it opened or now watches, for
    /// the explicit resume to join. A member a delivery woke first gets no
    /// plan input (never a second wake): its wake is watched, and the member
    /// is reached again once that wake closes. Once the plan has reached
    /// every member and watches no wake, the members it verified that are
    /// still Dormant are read and the dispatch ends. A dispatch whose lift was
    /// cancelled or superseded, or whose mob left Running, is dropped with
    /// nothing applied; so is one that fails.
    pub(super) async fn dispatch_explicit_resume_boot_members(
        &mut self,
    ) -> Result<Vec<ExplicitResumeBootMember>, MobError> {
        let Some(mut dispatch) = self.explicit_resume_boot_dispatch.take() else {
            return Ok(Vec::new());
        };
        if !self.explicit_resume_boot_dispatch_is_current(&dispatch) {
            return Ok(Vec::new());
        }
        {
            // A wake the plan counts that closed (Live, Broken, or Dormant
            // again) frees its slot. A member whose watched wake closed goes
            // back to the front of the queue: the plan reaches it again next.
            let state = self.dsl_authority.state();
            let mut closed = Vec::new();
            dispatch.open.retain(|identity, attempt| {
                let open = state
                    .member_wake_attempts
                    .get(identity)
                    .is_some_and(|open| *open == *attempt);
                if !open {
                    closed.push(identity.clone());
                }
                open
            });
            for identity in closed.into_iter().rev() {
                if dispatch.watched.remove(&identity) {
                    dispatch.queue.push_front(identity);
                }
            }
        }
        let mut planned = Vec::new();
        let mut watching = Vec::new();
        while dispatch.open.len() < dispatch.bound {
            let Some(identity) = dispatch.queue.pop_front() else {
                break;
            };
            let state = self.dsl_authority.state();
            if !super::super::boot_plan::member_is_dormant(state, &identity) {
                // A delivery woke the member first. While that wake runs the
                // plan applies nothing to it, and watches it instead; a member
                // Live or Broken is left as it is.
                let waking = matches!(
                    super::super::boot_plan::member_materialization_phase(state, &identity),
                    Some(mob_dsl::MemberMaterializationPhase::Waking)
                );
                if waking && let Some(attempt) = state.member_wake_attempts.get(&identity).copied()
                {
                    dispatch.open.insert(identity.clone(), attempt);
                    dispatch.watched.insert(identity.clone());
                    watching.push((identity, attempt));
                }
                continue;
            }
            let input = super::super::boot_plan::boot_materialization_input(
                &identity,
                &dispatch.obligations,
                &self.member_residency.host_required,
                &dispatch.notified,
            );
            let transition = self.apply_dsl_input_collect_transition(
                input,
                "explicit_resume_plan_boot_materialization",
            )?;
            for effect in transition.effects() {
                match effect {
                    mob_dsl::MobMachineEffect::MaterializeMember {
                        agent_identity,
                        attempt,
                        ..
                    } => {
                        dispatch.open.insert(agent_identity.clone(), *attempt);
                        planned.push((agent_identity.clone(), *attempt));
                    }
                    mob_dsl::MobMachineEffect::VerifyMember { agent_identity } => {
                        dispatch
                            .verify
                            .insert(AgentIdentity::from(agent_identity.0.as_str()));
                    }
                    _ => {}
                }
            }
        }
        let finished = dispatch.queue.is_empty() && dispatch.watched.is_empty();
        #[cfg(feature = "runtime-adapter")]
        if finished {
            // A verified member a delivery has woken since reads its durable
            // session in its own wake: the pass reads only the members still
            // Dormant.
            let state = self.dsl_authority.state();
            dispatch.verify.retain(|identity| {
                super::super::boot_plan::domain_member_is_dormant(state, identity)
            });
            self.verify_dormant_member_set(&dispatch.verify).await?;
        }
        #[cfg(not(feature = "runtime-adapter"))]
        let _ = &dispatch.verify;
        let members = self
            .explicit_resume_boot_members(&dispatch.resume_attempt, planned, watching)
            .await?;
        if !finished {
            self.explicit_resume_boot_dispatch = Some(dispatch);
        }
        Ok(members)
    }

    /// Whether `dispatch` may still dispatch: its lift is the mob's current
    /// one and not cancelled, and the mob is Running.
    fn explicit_resume_boot_dispatch_is_current(
        &self,
        dispatch: &ExplicitResumeBootDispatch,
    ) -> bool {
        let state = self.dsl_authority.state();
        state.lifecycle_phase == mob_dsl::MobPhase::Running
            && !state.explicit_resume_cancel_requested
            && state.explicit_resume_attempt.as_ref() == Some(&dispatch.resume_attempt)
    }

    /// Whether the lift's plan still has members to reach. A dispatch that is
    /// no longer current is dropped here.
    pub(super) fn explicit_resume_boot_dispatch_pending(&mut self) -> bool {
        let current = self
            .explicit_resume_boot_dispatch
            .as_ref()
            .is_some_and(|dispatch| self.explicit_resume_boot_dispatch_is_current(dispatch));
        if !current {
            self.explicit_resume_boot_dispatch = None;
        }
        current
    }

    /// The explicit resume's record of each wake of a dispatch round: the
    /// ones the plan opened (`planned`, taken over) and the ones it watches
    /// (`watched`, no input).
    async fn explicit_resume_boot_members(
        &self,
        resume_attempt: &mob_dsl::ResumeAttemptId,
        planned: Vec<(mob_dsl::AgentIdentity, u64)>,
        watched: Vec<(mob_dsl::AgentIdentity, u64)>,
    ) -> Result<Vec<ExplicitResumeBootMember>, MobError> {
        if planned.is_empty() && watched.is_empty() {
            return Ok(Vec::new());
        }
        let state = self.dsl_authority.state();
        let roster = self.roster.read().await;
        planned
            .into_iter()
            .map(|wake| (wake, true))
            .chain(watched.into_iter().map(|wake| (wake, false)))
            .map(|((identity, wake_attempt), authorized)| {
                let agent_identity = AgentIdentity::from(identity.0.as_str());
                let entry = roster.get(&agent_identity).ok_or_else(|| {
                    MobError::Internal(format!(
                        "the lift's plan reached '{agent_identity}', which has no roster entry"
                    ))
                })?;
                let session_id = state
                    .member_session_bindings
                    .get(&identity)
                    .cloned()
                    .ok_or_else(|| {
                        MobError::Internal(format!(
                            "the lift's plan reached '{agent_identity}', which has no session binding"
                        ))
                    })?;
                Ok(ExplicitResumeBootMember {
                    binding: mob_dsl::ResumeMemberBinding {
                        agent_runtime_id: mob_dsl::AgentRuntimeId::from_domain(
                            &entry.agent_runtime_id,
                        ),
                        fence_token: mob_dsl::FenceToken::from_domain(entry.fence_token),
                        session_id,
                        definition_epoch: state.definition_epoch,
                    },
                    agent_identity,
                    resume_attempt: resume_attempt.clone(),
                    wake_attempt,
                    authorized,
                    watched: !authorized,
                })
            })
            .collect()
    }

    /// `on_demand_verified` after a plan on the live authority (the lift's,
    /// or a seat's): one read-only durable verdict for each member the plan
    /// leaves Dormant, before the caller is answered. A member whose durable
    /// session cannot be resumed is recorded Broken through the machine,
    /// with nothing materialized; the others stay Dormant. The realm is
    /// listed once and scanned at most once.
    #[cfg(feature = "runtime-adapter")]
    pub(super) async fn verify_dormant_member_set(
        &mut self,
        verify: &BTreeSet<AgentIdentity>,
    ) -> Result<(), MobError> {
        if verify.is_empty() {
            return Ok(());
        }
        let (targets, member_bound_sessions) = {
            let roster = self.roster.read().await;
            let state = self.dsl_authority.state();
            let member_bound_sessions = roster
                .list()
                .filter_map(|entry| entry.member_ref.bridge_session_id().cloned())
                .collect::<HashSet<_>>();
            let targets = roster
                .list()
                .filter(|entry| verify.contains(&entry.agent_identity))
                .filter_map(|entry| {
                    let session = state
                        .member_session_bindings
                        .get(&mob_dsl::AgentIdentity::from_domain(&entry.agent_identity))?;
                    Some(
                        SessionId::parse(&session.0)
                            .map_err(|error| {
                                MobError::Internal(format!(
                                    "the verified pass found an invalid session binding '{}' for '{}': {error}",
                                    session.0, entry.agent_identity
                                ))
                            })
                            .map(|session_id| super::super::boot_plan::VerifiedMemberTarget {
                                entry: entry.clone(),
                                session_id,
                            }),
                    )
                })
                .collect::<Result<Vec<_>, MobError>>()?;
            (targets, member_bound_sessions)
        };
        let listed_sessions = self
            .session_service
            .list(meerkat_core::service::SessionQuery::default())
            .await?;
        let mut successor_index = None;
        let verdicts = super::super::boot_plan::verify_dormant_members(
            &super::super::boot_plan::VerifiedPassContext {
                mob_id: &self.definition.id,
                session_service: &self.session_service,
                listed_sessions: &listed_sessions,
                member_bound_sessions: &member_bound_sessions,
                concurrency: self.member_residency.verify_concurrency,
            },
            targets,
            &mut successor_index,
        )
        .await?;
        for (target, verdict) in &verdicts {
            let Some((signal, diagnostic)) =
                super::super::boot_plan::verified_member_signal(target, verdict)
            else {
                continue;
            };
            self.apply_dsl_signal(signal, "explicit_resume_verified_dormant_member")?;
            self.record_member_restore_failure(&target.entry.agent_identity, diagnostic)
                .await;
        }
        Ok(())
    }

    /// Join the explicit resume to each planned member's wake: the member's
    /// resume work is recorded under the wake's attempt (Join), and the
    /// resume settles it once that attempt closes. The first refusal is
    /// returned after every member was tried; a member whose join was refused
    /// is still materialized by its wake, the resume only does not wait for
    /// it. A wake the plan only watches (a delivery's) gets no input. Every
    /// wake of the round is watched until it closes either way: its close
    /// frees its slot of the plan.
    pub(super) fn join_explicit_resume_boot_members(
        &mut self,
        members: Vec<ExplicitResumeBootMember>,
    ) -> Result<(), MobError> {
        let mut first_error = None;
        for mut member in members {
            if member.authorized
                && let Err(error) = self.apply_dsl_input(
                    mob_dsl::MobMachineInput::AuthorizeExplicitResumeMember {
                        attempt: member.resume_attempt.clone(),
                        agent_identity: mob_dsl::AgentIdentity::from_domain(&member.agent_identity),
                        binding: member.binding.clone(),
                    },
                    "join_explicit_resume_boot_member",
                )
            {
                first_error.get_or_insert(error);
                member.authorized = false;
            }
            let outcome_rx = self.join_member_wake(&member.agent_identity, member.wake_attempt);
            let command_tx = self.command_tx.clone();
            self.actor_io_tasks.spawn(async move {
                let joined = outcome_rx.await.is_ok_and(|outcome| outcome.is_ok());
                let _ = command_tx
                    .send(RoutedMobCommand::internal(
                        MobCommand::ResumeLifecycleBootMemberSettled {
                            member: Box::new(member),
                            joined,
                        },
                    ))
                    .await;
            });
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// One planned member's wake closed. The wake path resolved the member
    /// through the machine (Live; Broken with its typed reason; or Dormant
    /// again for a retryable cause, its obligation owed and retried), so the
    /// lift reports none of those as its own failure: it settles the
    /// member's resume work and goes on once every member settled. The
    /// closed wake frees its slot of the plan first, so the plan dispatches
    /// the members it has not reached yet, up to its bound.
    pub(super) async fn explicit_resume_boot_member_settled(
        &mut self,
        member: Box<ExplicitResumeBootMember>,
        joined: bool,
    ) {
        if !joined && !member.watched {
            tracing::warn!(
                mob_id = %self.definition.id,
                agent_identity = %member.agent_identity,
                "a member the lift materializes did not become live; MobMachine holds its outcome"
            );
        }
        let dispatched = self.explicit_resume_boot_dispatch_pending();
        if dispatched {
            match self.dispatch_explicit_resume_boot_members().await {
                Ok(members) => {
                    if let Err(error) = self.join_explicit_resume_boot_members(members) {
                        self.retain_explicit_resume_error(error);
                    }
                }
                Err(error) => self.retain_explicit_resume_error(error),
            }
        }
        if !member.authorized {
            // The lift never took this wake over, so it has nothing to
            // settle; it goes on only when this close moved its plan. The
            // lift waited for a watched wake: its close is the lift's
            // progress.
            if dispatched {
                if member.watched
                    && let Some(pending) = self.pending_resume_lifecycle.as_ref()
                {
                    pending.progress.member_progress(
                        &member.agent_identity,
                        super::super::state::LifecycleProgressStage::MemberLiveMaterialization,
                    );
                }
                self.continue_explicit_resume_after_rebuild().await;
            }
            return;
        }
        if let Err(error) = self.apply_dsl_input(
            mob_dsl::MobMachineInput::SettleExplicitResumeMember {
                attempt: member.resume_attempt.clone(),
                agent_identity: mob_dsl::AgentIdentity::from_domain(&member.agent_identity),
                binding: member.binding.clone(),
            },
            "settle_explicit_resume_boot_member",
        ) {
            tracing::warn!(%error, "stale resume boot-member settlement left current authority unchanged");
            return;
        }
        if let Some(pending) = self.pending_resume_lifecycle.as_ref() {
            pending.progress.member_progress(
                &member.agent_identity,
                super::super::state::LifecycleProgressStage::MemberLiveMaterialization,
            );
        }
        self.continue_explicit_resume_after_rebuild().await;
    }
}
