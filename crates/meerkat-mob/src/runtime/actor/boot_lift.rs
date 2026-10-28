//! The clean-stop lift's member plan.
//!
//! A mob that was stopped and is lifted again ([`super::super::MobHandle::resume`])
//! plans its members exactly as a resume of a running mob does: once the
//! durable Resume made the mob Running again, one `PlanBootMaterialization`
//! per Dormant member, with the start's restore policy and host inputs and
//! one fresh runtime obligation scan. A member the plan selects is Waking
//! under the plan's wake attempt, and the lift realizes that attempt itself
//! through the explicit-resume work path, as an explicit resume rebuilds
//! every member: its rebuild is prepared off the actor (its customized
//! request, its view and snapshot reads; no takeover, which the seam ran
//! before the Resume), the explicit resume reserves it
//! (`AuthorizeExplicitResumeMember` answers Join), and the work path
//! observes, provisions (an occupied claim awaits its release, a retained
//! unattached actor is reclaimed, uncertain custody is parked) and accepts
//! it; its own readiness then settles it Live, or Broken. A seat whose
//! session is unborn, or a member with no durable endpoint yet, is realized
//! by the actor's wake path instead (the cold recipe creates the session,
//! or registers the endpoint), and the explicit resume joins that wake. The
//! lift answers its caller only after the members it materializes settled.
//! A member is never materialized twice: one attempt per member, and every
//! other wake of it joins that attempt. Every other member stays Dormant and
//! is materialized by its first delivery.
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

/// One armed hold of the hand-off of a watched wake's close to the actor:
/// the watcher reports reaching it, then waits for its release.
#[cfg(test)]
type ExplicitResumeBootMemberSettleHold = (oneshot::Sender<()>, oneshot::Receiver<()>);

#[cfg(test)]
static HOLD_EXPLICIT_RESUME_BOOT_MEMBER_SETTLE_FOR_TEST: std::sync::Mutex<
    BTreeMap<(MobId, AgentIdentity), ExplicitResumeBootMemberSettleHold>,
> = std::sync::Mutex::new(BTreeMap::new());

/// Test seam: the next wake of `member` a lift of `mob_id` watches hands its
/// close to the actor only once released. The receiver resolves once the
/// wake closed and the hand-off reached the hold; sending on, or dropping,
/// the sender releases it.
#[cfg(test)]
pub(in crate::runtime) fn hold_explicit_resume_boot_member_settle_for_test(
    mob_id: &MobId,
    member: &AgentIdentity,
) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    HOLD_EXPLICIT_RESUME_BOOT_MEMBER_SETTLE_FOR_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert((mob_id.clone(), member.clone()), (entered_tx, release_rx));
    (entered_rx, release_tx)
}

/// The hold armed for the hand-off of `member`'s next watched wake in
/// `mob_id`, taken once.
#[cfg(test)]
fn take_explicit_resume_boot_member_settle_hold_for_test(
    mob_id: &MobId,
    member: &AgentIdentity,
) -> Option<ExplicitResumeBootMemberSettleHold> {
    HOLD_EXPLICIT_RESUME_BOOT_MEMBER_SETTLE_FOR_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&(mob_id.clone(), member.clone()))
}

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
    /// The members of `open` whose rebuild is being prepared for the
    /// explicit-resume work path: the explicit resume reserves each once its
    /// rebuild is ready.
    preparing: BTreeSet<mob_dsl::AgentIdentity>,
    /// The members the plan verified (`VerifyMember`), read once it has
    /// reached every member (those still Dormant then).
    verify: BTreeSet<AgentIdentity>,
    /// How many of the plan's wakes may be open at once: the restore
    /// concurrency.
    bound: usize,
    /// The lift's progress: the plan's store reads report to it.
    progress: super::super::state::LifecycleProgressSignal,
    /// The lift's admission: a rebuild the plan prepares joins a takeover
    /// under it (a successor session's).
    admission: super::super::state::LifecycleAdmissionSignal,
}

/// One member of a dispatch round whose rebuild the lift prepared off the
/// actor, for the explicit-resume work path, under the plan's open
/// `attempt`.
pub(in crate::runtime) struct ExplicitResumeBootMemberPrepared {
    identity: AgentIdentity,
    attempt: u64,
    result: Result<ExplicitResumeMemberRebuild, MobError>,
}

/// Whether the lift realizes `identity`'s planned attempt through the
/// explicit-resume work path, as an explicit resume rebuilds a member: its
/// session is born and its endpoint is durable. A seat whose session is
/// unborn is created by its first wake, and a member with no durable
/// endpoint yet registers one in its wake before it is Live, so the actor's
/// wake path realizes those.
fn lift_rebuilds_member(
    state: &mob_dsl::MobMachineState,
    identity: &mob_dsl::AgentIdentity,
) -> bool {
    !state.member_seat_session_unborn.contains(identity)
        && state.member_peer_endpoints.contains_key(identity)
}

impl MobActor {
    /// Start the lift's member plan on the live authority, at most the
    /// restore concurrency of its wakes open at once (see the module
    /// documentation). Every effect goes through the actor's apply seam, so
    /// the wake path realizes each `MaterializeMember` it emits; this returns
    /// the members it made Waking now, for the explicit resume to join. The
    /// rest are dispatched as those wakes close
    /// ([`Self::dispatch_explicit_resume_boot_members`]).
    ///
    /// The plan's store reads report to the lift's `progress` as units of
    /// work: the obligation scan, and each member's read in the verified
    /// pass. A slow store then never reads as a stalled lift; only one read
    /// that outlasts the inactivity budget does.
    pub(super) async fn plan_explicit_resume_boot_set(
        &mut self,
        admission: &super::super::state::LifecycleAdmissionSignal,
        progress: &super::super::state::LifecycleProgressSignal,
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
        let obligations = {
            // The scan is a unit of work of its own: its two store-wide
            // reads start a new inactivity budget, and so does what follows
            // them.
            progress
                .stage_progress(super::super::state::LifecycleProgressStage::BootObligationScan);
            let obligations = super::super::boot_plan::scan_boot_obligations(
                self.runtime_adapter.as_ref(),
                self.dsl_authority.state(),
                &self.definition.id,
                self.incarnation.incarnation(),
            )
            .await?;
            progress
                .stage_progress(super::super::state::LifecycleProgressStage::BootObligationScan);
            obligations
        };
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
            preparing: BTreeSet::new(),
            verify: BTreeSet::new(),
            bound,
            progress: progress.clone(),
            admission: admission.clone(),
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
        let mut rebuilt = Vec::new();
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
                        if lift_rebuilds_member(self.dsl_authority.state(), agent_identity) {
                            dispatch.preparing.insert(agent_identity.clone());
                            rebuilt.push((agent_identity.clone(), *attempt));
                        } else {
                            planned.push((agent_identity.clone(), *attempt));
                        }
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
        if !rebuilt.is_empty() {
            // The lift realizes these attempts itself, through the
            // explicit-resume work path: the actor's wake path starts none of
            // them.
            self.member_wakes.pending.retain(|pending| {
                !rebuilt.iter().any(|(identity, attempt)| {
                    pending.attempt == *attempt && identity.0 == pending.identity.as_str()
                })
            });
        }
        let finished = dispatch.queue.is_empty()
            && dispatch.watched.is_empty()
            && dispatch.preparing.is_empty();
        #[cfg(feature = "runtime-adapter")]
        if finished {
            // A verified member a delivery has woken since reads its durable
            // session in its own wake: the pass reads only the members still
            // Dormant.
            let state = self.dsl_authority.state();
            dispatch.verify.retain(|identity| {
                super::super::boot_plan::domain_member_is_dormant(state, identity)
            });
            self.verify_dormant_member_set(&dispatch.verify, Some(&dispatch.progress))
                .await?;
            // The pass reported its own reads. The lift still waits for the
            // plan's wakes that are open: that wait is their host
            // materialization again, as every watch reports it.
            if !dispatch.verify.is_empty()
                && let Some(first) = dispatch.open.keys().next()
            {
                dispatch.progress.awaiting_owed_members(
                    &AgentIdentity::from(first.0.as_str()),
                    dispatch.open.len(),
                    super::super::state::LifecycleProgressStage::MemberLiveMaterialization,
                );
            }
        }
        #[cfg(not(feature = "runtime-adapter"))]
        let _ = (&dispatch.verify, &dispatch.progress);
        if !rebuilt.is_empty() {
            self.prepare_explicit_resume_boot_members(
                &dispatch.resume_attempt,
                &dispatch.admission,
                &dispatch.progress,
                rebuilt,
            )
            .await;
        }
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
    /// listed once and scanned at most once. A lift passes its `progress`:
    /// each member's read is reported as that member's progress.
    #[cfg(feature = "runtime-adapter")]
    pub(super) async fn verify_dormant_member_set(
        &mut self,
        verify: &BTreeSet<AgentIdentity>,
        progress: Option<&super::super::state::LifecycleProgressSignal>,
    ) -> Result<(), MobError> {
        if verify.is_empty() {
            return Ok(());
        }
        // The pass's reads are units of work of the lift, its first read
        // included: that read gets a budget of its own even when the lift
        // was waiting on a host build (whose stage has none), so a read that
        // never returns is answered as a stall instead of holding the lift
        // for good.
        if let Some(progress) = progress {
            progress.stage_progress(
                super::super::state::LifecycleProgressStage::MemberSessionMetadataProbe,
            );
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
        // The realm listing is a read of its own, as the rebuild path
        // reports it.
        if let Some(progress) = progress {
            progress.stage_progress(
                super::super::state::LifecycleProgressStage::MemberSessionMetadataProbe,
            );
        }
        let mut successor_index = None;
        let verdicts = super::super::boot_plan::verify_dormant_members(
            &super::super::boot_plan::VerifiedPassContext {
                mob_id: &self.definition.id,
                session_service: &self.session_service,
                listed_sessions: &listed_sessions,
                member_bound_sessions: &member_bound_sessions,
                concurrency: self.member_residency.verify_concurrency,
                progress,
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
            self.watch_explicit_resume_boot_member_wake(member);
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Watch `member`'s wake attempt (`wake_attempt`) until it closes, then
    /// hand the close to the actor (`ResumeLifecycleBootMemberSettled`).
    ///
    /// The lift waits for this wake, to settle the member or to free its slot
    /// of the plan, and the member's materialization is the host's to finish
    /// or fail: the wait is reported as the member's host materialization, so
    /// no platform inactivity budget answers the lift as stalled while the
    /// host builds (the rebuild path reports its members the same way).
    fn watch_explicit_resume_boot_member_wake(&mut self, member: ExplicitResumeBootMember) {
        if let Some(pending) = self.pending_resume_lifecycle.as_ref() {
            pending.progress.awaiting_member(
                &member.agent_identity,
                super::super::state::LifecycleProgressStage::MemberLiveMaterialization,
            );
        }
        let outcome_rx = self.join_member_wake(&member.agent_identity, member.wake_attempt);
        let command_tx = self.command_tx.clone();
        #[cfg(test)]
        let settle_hold = take_explicit_resume_boot_member_settle_hold_for_test(
            &self.definition.id,
            &member.agent_identity,
        );
        self.actor_io_tasks.spawn(async move {
            let joined = outcome_rx.await.is_ok_and(|outcome| outcome.is_ok());
            #[cfg(test)]
            if let Some((entered, release)) = settle_hold {
                let _ = entered.send(());
                let _ = release.await;
            }
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

    /// The wake attempt the lift must still wait for before it can settle
    /// `member`'s resume work: the member's open attempt, while MobMachine
    /// still holds that work for this lift. `None` once the work is not this
    /// lift's (it settled, or the lift ended) or when no attempt is open.
    fn explicit_resume_boot_member_open_wake(
        &self,
        member: &ExplicitResumeBootMember,
    ) -> Option<u64> {
        let state = self.dsl_authority.state();
        let identity = mob_dsl::AgentIdentity::from_domain(&member.agent_identity);
        if state.explicit_resume_attempt.as_ref() != Some(&member.resume_attempt)
            || state.explicit_resume_member_work.get(&identity) != Some(&member.binding)
        {
            return None;
        }
        state.member_wake_attempts.get(&identity).copied()
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
        let dispatched = self.advance_explicit_resume_boot_plan().await;
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
            // MobMachine settles the member's resume work only once no wake
            // attempt of the member is open. A wake opened after the watched
            // one closed (a delivery's, or an owed retry) holds the member
            // until it closes too: the lift watches that attempt and settles
            // then, so the member's work is never dropped. A dropped settle
            // left the lift waiting for good, with every Stop, Shutdown and
            // Destroy parked behind it.
            if let Some(open_attempt) = self.explicit_resume_boot_member_open_wake(&member) {
                tracing::debug!(
                    mob_id = %self.definition.id,
                    agent_identity = %member.agent_identity,
                    closed_attempt = member.wake_attempt,
                    open_attempt,
                    "the lift waits for the member's open wake before it settles the member"
                );
                let mut member = *member;
                member.wake_attempt = open_attempt;
                self.watch_explicit_resume_boot_member_wake(member);
                return;
            }
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

    /// Dispatch the next members of the lift's plan while it still has
    /// members to reach (each closed wake frees its slot) and join the wakes
    /// it opened or watches. Whether the plan was still dispatching.
    pub(super) async fn advance_explicit_resume_boot_plan(&mut self) -> bool {
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
        dispatched
    }

    /// Prepare, off the actor, the rebuilds of the members of a dispatch
    /// round the lift realizes through the explicit-resume work path, each
    /// member on its own (one member's failure is that member's), and hand
    /// them back ([`Self::explicit_resume_boot_members_prepared`]).
    async fn prepare_explicit_resume_boot_members(
        &mut self,
        resume_attempt: &mob_dsl::ResumeAttemptId,
        admission: &super::super::state::LifecycleAdmissionSignal,
        progress: &super::super::state::LifecycleProgressSignal,
        members: Vec<(mob_dsl::AgentIdentity, u64)>,
    ) {
        let mut prepared = Vec::new();
        let mut candidates = Vec::new();
        {
            let roster = self.roster.read().await;
            let state = self.dsl_authority.state();
            for (identity, attempt) in members {
                let domain = AgentIdentity::from(identity.0.as_str());
                let candidate = roster.get(&domain).cloned().and_then(|entry| {
                    let session_id = state
                        .member_session_bindings
                        .get(&identity)
                        .and_then(|session| SessionId::parse(&session.0).ok())?;
                    let member_ref = Self::project_member_ref_session_binding(
                        &entry.member_ref,
                        Some(session_id.clone()),
                    )?;
                    Some((
                        entry,
                        member_ref,
                        session_id,
                        ExplicitResumePreparation::Planned,
                    ))
                });
                match candidate {
                    Some(candidate) => candidates.push((attempt, candidate)),
                    None => prepared.push(ExplicitResumeBootMemberPrepared {
                        result: Err(MobError::Internal(format!(
                            "the lift cannot rebuild '{domain}': it has no roster entry or session binding"
                        ))),
                        identity: domain,
                        attempt,
                    }),
                }
            }
        }
        let context = Arc::new(self.explicit_resume_preparation_context());
        let admission = admission.clone();
        let progress = progress.clone();
        let resume_attempt = resume_attempt.clone();
        let command_tx = self.command_tx.clone();
        #[cfg(all(test, feature = "runtime-adapter"))]
        let probe_mob = self.definition.id.clone();
        self.actor_io_tasks.spawn(async move {
            let rebuilds =
                futures::future::join_all(candidates.into_iter().map(|(attempt, candidate)| {
                    let context = Arc::clone(&context);
                    let admission = admission.clone();
                    let progress = progress.clone();
                    #[cfg(all(test, feature = "runtime-adapter"))]
                    let probe_mob = probe_mob.clone();
                    Box::pin(async move {
                        let identity = candidate.0.agent_identity.clone();
                        // The rows' restore probe holds the member's restore at
                        // its start, as it holds a wake's.
                        #[cfg(all(test, feature = "runtime-adapter"))]
                        let probed = super::member_wake_turn::enter_member_wake_restore_probe(
                            &probe_mob,
                            identity.clone(),
                        )
                        .await;
                        #[cfg(all(test, feature = "runtime-adapter"))]
                        if probed.as_ref().is_some_and(|restore| restore.aborted) {
                            return ExplicitResumeBootMemberPrepared {
                                identity,
                                attempt,
                                result: Err(MobError::Internal(
                                    "the test probe ended this member restore at its start"
                                        .to_string(),
                                )),
                            };
                        }
                        let result = context
                            .prepare(vec![candidate], admission, &progress)
                            .await
                            .and_then(|mut rebuild| {
                                rebuild.pop().ok_or_else(|| {
                                    MobError::Internal(format!(
                                        "the explicit resume prepared no rebuild of '{identity}'"
                                    ))
                                })
                            });
                        #[cfg(all(test, feature = "runtime-adapter"))]
                        drop(probed);
                        ExplicitResumeBootMemberPrepared {
                            identity,
                            attempt,
                            result,
                        }
                    })
                }))
                .await;
            prepared.extend(rebuilds);
            let _ = command_tx
                .send(RoutedMobCommand::internal(
                    MobCommand::ResumeLifecycleBootMembersPrepared {
                        resume_attempt,
                        prepared,
                    },
                ))
                .await;
        });
    }

    /// The rebuilds of a dispatch round's members are prepared. The explicit
    /// resume reserves each prepared member (Join: the plan's attempt is
    /// open) and the work path materializes it. A member whose rebuild could
    /// not be prepared, or that the explicit resume could not reserve, ends
    /// its plan attempt as a wake would ([`Self::end_explicit_resume_boot_member`]).
    /// A member whose attempt closed meanwhile (a stop, a retire) was not
    /// provisioned and is left as its closure left it. Then the plan goes on.
    pub(super) async fn explicit_resume_boot_members_prepared(
        &mut self,
        resume_attempt: mob_dsl::ResumeAttemptId,
        prepared: Vec<ExplicitResumeBootMemberPrepared>,
    ) {
        if let Some(dispatch) = self
            .explicit_resume_boot_dispatch
            .as_mut()
            .filter(|dispatch| dispatch.resume_attempt == resume_attempt)
        {
            for member in &prepared {
                dispatch
                    .preparing
                    .remove(&mob_dsl::AgentIdentity::from_domain(&member.identity));
            }
        }
        let lift_current = {
            let state = self.dsl_authority.state();
            state.lifecycle_phase == mob_dsl::MobPhase::Running
                && !state.explicit_resume_cancel_requested
                && state.explicit_resume_attempt.as_ref() == Some(&resume_attempt)
        };
        let mut rebuild = Vec::new();
        let mut reserving = Vec::new();
        for ExplicitResumeBootMemberPrepared {
            identity,
            attempt,
            result,
        } in prepared
        {
            if !self.member_wake_attempt_is_open(&identity, attempt) {
                continue;
            }
            match result {
                Ok(member) if lift_current => {
                    reserving.push((identity, attempt));
                    rebuild.push(member);
                }
                Ok(_) => self.abandon_explicit_resume_boot_member(
                    &identity,
                    attempt,
                    "the lift that planned the member ended before its rebuild".to_string(),
                ),
                Err(error) => {
                    self.end_explicit_resume_boot_member(&identity, attempt, error)
                        .await;
                }
            }
        }
        if !rebuild.is_empty() {
            let progress = self
                .pending_resume_lifecycle
                .as_ref()
                .map(|pending| pending.progress.clone())
                .or_else(|| {
                    self.explicit_resume_boot_dispatch
                        .as_ref()
                        .map(|dispatch| dispatch.progress.clone())
                });
            let refusal = match progress {
                Some(progress) => self
                    .rebuild_explicit_resume_member_sessions(rebuild, &progress)
                    .await
                    .err()
                    .map(|error| error.to_string()),
                None => Some("the lift lost its explicit resume".to_string()),
            };
            // A member the explicit resume did not reserve still holds its
            // plan attempt, which nothing would realize: it ends here.
            for (identity, attempt) in reserving {
                let reserved = self
                    .dsl_authority
                    .state()
                    .explicit_resume_member_work
                    .contains_key(&mob_dsl::AgentIdentity::from_domain(&identity));
                if !reserved && self.member_wake_attempt_is_open(&identity, attempt) {
                    let reason = refusal
                        .clone()
                        .unwrap_or_else(|| "the explicit resume did not reserve it".to_string());
                    self.end_explicit_resume_boot_member(
                        &identity,
                        attempt,
                        MobError::Internal(format!(
                            "the explicit resume could not rebuild '{identity}': {reason}"
                        )),
                    )
                    .await;
                }
            }
        }
        // The lift goes on only while it still waits on its members: one a
        // lifecycle control superseded has already been answered.
        let rebuilding = matches!(
            self.pending_resume_lifecycle
                .as_ref()
                .map(|pending| &pending.phase),
            Some(ResumeLifecyclePhase::RebuildingMembers { .. })
        );
        if rebuilding {
            self.advance_explicit_resume_boot_plan().await;
            self.continue_explicit_resume_after_rebuild().await;
        }
    }

    /// End a planned member's open attempt that the lift could not rebuild,
    /// as a wake ends one: retryable for a cause that clears on its own
    /// ([`crate::runtime::builder::restore_error_is_retryable`]; the member
    /// is Dormant with its boot obligation owed and its retry armed, and the
    /// explicit resume answers the typed error), otherwise Broken with the
    /// error, that member's own outcome.
    async fn end_explicit_resume_boot_member(
        &mut self,
        identity: &AgentIdentity,
        attempt: u64,
        error: MobError,
    ) {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        if crate::runtime::builder::restore_error_is_retryable(&error) {
            match self.apply_dsl_signal_collect_transition(
                mob_dsl::MobMachineSignal::ResolveMemberRevivalRetryable {
                    agent_identity: dsl_identity,
                    attempt,
                    kind: mob_dsl::MemberWakeRetryableKind::ResumeGateUnavailable,
                },
                "resolve_retryable_explicit_resume_boot_member",
            ) {
                Ok(_) => {
                    self.schedule_owed_member_wake_retry(identity);
                    self.retain_explicit_resume_member_failure(error);
                }
                Err(refused) => {
                    self.abandon_explicit_resume_boot_member(
                        identity,
                        attempt,
                        refused.to_string(),
                    );
                }
            }
            return;
        }
        let session_id = self
            .dsl_authority
            .state()
            .member_session_bindings
            .get(&dsl_identity)
            .and_then(|session| SessionId::parse(&session.0).ok());
        let reason = error.to_string();
        let registration_refusal = match &error {
            MobError::MemberRegistrationRefused { call, .. } => Some(*call),
            _ => None,
        };
        let signal = match registration_refusal {
            Some(call) => mob_dsl::MobMachineSignal::ResolveMemberRevivalHostRefused {
                agent_identity: dsl_identity.clone(),
                refusal_code: call.as_code().to_string(),
                reason: reason.clone(),
                attempt,
            },
            None => mob_dsl::MobMachineSignal::ResolveMemberRevivalFailed {
                agent_identity: dsl_identity.clone(),
                reason: reason.clone(),
                attempt,
            },
        };
        match self.apply_dsl_signal_collect_transition(
            signal,
            "resolve_failed_explicit_resume_boot_member",
        ) {
            Ok(transition)
                if super::super::boot_plan::wake_feedback_applied(
                    transition.effects(),
                    &dsl_identity,
                    attempt,
                ) =>
            {
                self.record_member_restore_failure(
                    identity,
                    super::super::handle::RestoreFailureDiagnostic {
                        bridge_session_id: session_id,
                        reason,
                        hold: error.durable_resume_hold(),
                        registration_refusal,
                    },
                )
                .await;
            }
            Ok(_) => {}
            Err(refused) => {
                self.abandon_explicit_resume_boot_member(identity, attempt, refused.to_string());
            }
        }
    }

    /// Close a planned member's open attempt that nothing realizes (the lift
    /// that planned it ended, or its resolution was refused): the member is
    /// Dormant again and its next delivery wakes it.
    fn abandon_explicit_resume_boot_member(
        &mut self,
        identity: &AgentIdentity,
        attempt: u64,
        reason: String,
    ) {
        if let Err(error) = self.apply_dsl_signal(
            mob_dsl::MobMachineSignal::AbandonMemberWake {
                agent_identity: mob_dsl::AgentIdentity::from_domain(identity),
                attempt,
                kind: mob_dsl::MemberWakeAbandonKind::NotCurrent,
                reason,
            },
            "abandon_unrealized_explicit_resume_boot_member",
        ) {
            tracing::warn!(
                mob_id = %self.definition.id,
                agent_identity = %identity,
                %error,
                "a planned member's attempt the lift does not realize could not be closed"
            );
        }
    }
}
