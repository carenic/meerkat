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

use super::*;

/// One member the lift's plan materializes, as the explicit resume tracks
/// it: Waking under the plan's wake attempt, joined by the lift.
#[derive(Debug, Clone)]
pub(in crate::runtime) struct ExplicitResumeBootMember {
    pub(in crate::runtime) agent_identity: AgentIdentity,
    resume_attempt: mob_dsl::ResumeAttemptId,
    binding: mob_dsl::ResumeMemberBinding,
    wake_attempt: u64,
}

impl MobActor {
    /// Plan the lift's member materialization on the live authority: one
    /// `PlanBootMaterialization` per Dormant member. Every effect goes
    /// through the actor's apply seam, so the wake path realizes each
    /// `MaterializeMember` it emits; this returns the members it made
    /// Waking, for the explicit resume to join.
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
        let mut planned = Vec::new();
        let mut verify = BTreeSet::new();
        for identity in super::super::boot_plan::dormant_members(self.dsl_authority.state()) {
            let input = super::super::boot_plan::boot_materialization_input(
                &identity,
                &obligations,
                &self.member_residency.host_required,
                &notified,
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
                    } => planned.push((agent_identity.clone(), *attempt)),
                    mob_dsl::MobMachineEffect::VerifyMember { agent_identity } => {
                        verify.insert(AgentIdentity::from(agent_identity.0.as_str()));
                    }
                    _ => {}
                }
            }
        }
        #[cfg(feature = "runtime-adapter")]
        self.verify_explicit_resume_dormant_members(&verify).await?;
        #[cfg(not(feature = "runtime-adapter"))]
        let _ = verify;
        let state = self.dsl_authority.state();
        let roster = self.roster.read().await;
        planned
            .into_iter()
            .map(|(identity, wake_attempt)| {
                let agent_identity = AgentIdentity::from(identity.0.as_str());
                let entry = roster.get(&agent_identity).ok_or_else(|| {
                    MobError::Internal(format!(
                        "the lift planned '{agent_identity}', which has no roster entry"
                    ))
                })?;
                let session_id = state
                    .member_session_bindings
                    .get(&identity)
                    .cloned()
                    .ok_or_else(|| {
                        MobError::Internal(format!(
                            "the lift planned '{agent_identity}', which has no session binding"
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
                })
            })
            .collect()
    }

    /// `on_demand_verified` on the lift: one read-only durable verdict for
    /// each member the lift's plan leaves Dormant, before the lift answers.
    /// A member whose durable session cannot be resumed is recorded Broken
    /// through the machine, with nothing materialized; the others stay
    /// Dormant. The realm is listed once and scanned at most once.
    #[cfg(feature = "runtime-adapter")]
    async fn verify_explicit_resume_dormant_members(
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
                                    "the lift's verified pass found an invalid session binding '{}' for '{}': {error}",
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
    /// it.
    pub(super) fn join_explicit_resume_boot_members(
        &mut self,
        members: Vec<ExplicitResumeBootMember>,
    ) -> Result<(), MobError> {
        let mut first_error = None;
        for member in members {
            if let Err(error) = self.apply_dsl_input(
                mob_dsl::MobMachineInput::AuthorizeExplicitResumeMember {
                    attempt: member.resume_attempt.clone(),
                    agent_identity: mob_dsl::AgentIdentity::from_domain(&member.agent_identity),
                    binding: member.binding.clone(),
                },
                "join_explicit_resume_boot_member",
            ) {
                first_error.get_or_insert(error);
                continue;
            }
            let outcome_rx =
                self.join_member_wake(&member.agent_identity, member.wake_attempt.into());
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
    /// member's resume work and goes on once every member settled.
    pub(super) async fn explicit_resume_boot_member_settled(
        &mut self,
        member: Box<ExplicitResumeBootMember>,
        joined: bool,
    ) {
        if !joined {
            tracing::warn!(
                mob_id = %self.definition.id,
                agent_identity = %member.agent_identity,
                "a member the lift materializes did not become live; MobMachine holds its outcome"
            );
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
