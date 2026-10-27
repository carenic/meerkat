//! The dormant seat on the actor: the plan and the commit steps of
//! [`super::super::MobHandle::seat_dormant_members`] (see
//! [`super::super::dormant_seat`] for the three steps).
//!
//! The commit is one actor step per chunk: one prepared authority, one
//! runtime obligation scan, one `append_batch` before the base-checked
//! commit. An identity the machine refuses is undone alone (its batch of
//! inputs restores the prepared authority) and answered typed; only a store
//! failure fails the chunk, before anything of it is committed.

use super::super::dormant_seat::{
    DormantSeatCandidate, DormantSeatCommitEntry, DormantSeatLaunch, DormantSeatOutcome,
    DormantSeatPlan, DormantSeatPlanEntry, MemberRotationPlan, fresh_seat_session_id,
};
use super::*;

/// What a plan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeatPlanMode {
    /// A dormant seat (`seat_dormant_members`).
    Seat,
    /// A rotation's next incarnation (`rotate_member`): a fresh life seated
    /// Dormant, owing the kickoff its spec owes, the identity seated now or
    /// not.
    Rotation,
}

/// A rotation's next incarnation (`rotate_member`): its seat material,
/// carried by the retirement of the current incarnation to the terminal
/// step that seats it.
pub(super) struct MemberRotation {
    pub(super) candidate: Box<DormantSeatCandidate>,
    pub(super) endpoint: Option<TrustedPeerDescriptor>,
}

/// One seated identity of a chunk, waiting for its plan and its outcome.
struct SeatedInChunk {
    index: usize,
    agent_identity: AgentIdentity,
    session_id: SessionId,
    endpoint: Option<TrustedPeerDescriptor>,
}

impl MobActor {
    /// Plan one seat chunk, in order: each spec's typed answer, the spawn of
    /// a fresh spec that owes a kickoff, or what its seat needs.
    pub(super) async fn plan_dormant_seats(
        &mut self,
        specs: Vec<super::super::handle::SpawnMemberSpec>,
    ) -> DormantSeatPlan {
        let mut named = BTreeSet::new();
        let mut entries = Vec::with_capacity(specs.len());
        for spec in specs {
            let agent_identity = spec.identity.clone();
            let entry = if named.insert(agent_identity.clone()) {
                match self.plan_dormant_seat(spec, SeatPlanMode::Seat).await {
                    Ok(entry) => entry,
                    Err(error) => DormantSeatPlanEntry::Done(DormantSeatOutcome::Refused {
                        agent_identity,
                        error,
                    }),
                }
            } else {
                DormantSeatPlanEntry::Done(DormantSeatOutcome::Refused {
                    error: MobError::Internal(format!(
                        "'{agent_identity}' is named twice in one seat batch"
                    )),
                    agent_identity,
                })
            };
            entries.push(entry);
        }
        DormantSeatPlan {
            entries,
            endpoint_source: self.member_residency.seat_endpoints.clone(),
        }
    }

    /// Plan a rotation (`rotate_member`): the next incarnation's seat, and
    /// whether the identity is seated now (its current incarnation is then
    /// retired and the next one seated in the retirement's terminal step).
    pub(super) async fn plan_member_rotation(
        &mut self,
        spec: super::super::handle::SpawnMemberSpec,
    ) -> Result<MemberRotationPlan, MobError> {
        let agent_identity = spec.identity.clone();
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&agent_identity);
        if self.retirements.contains_key(&agent_identity) {
            return Err(MobError::LifecycleOperationPending {
                intent: format!("retirement member {agent_identity}"),
            });
        }
        let seated = self
            .dsl_authority
            .state()
            .identity_to_runtime
            .contains_key(&dsl_identity);
        match self.plan_dormant_seat(spec, SeatPlanMode::Rotation).await? {
            DormantSeatPlanEntry::Seat(candidate) => Ok(MemberRotationPlan::Seat {
                candidate,
                seated,
                endpoint_source: self.member_residency.seat_endpoints.clone(),
            }),
            DormantSeatPlanEntry::Embody(spec) if !seated => Ok(MemberRotationPlan::Embody(spec)),
            _ => Err(MobError::Internal(format!(
                "the rotation of '{agent_identity}' planned no seat"
            ))),
        }
    }

    /// Plan one spec. Every check a spawn makes before anything exists is
    /// made here; the machine makes the rest at the commit.
    async fn plan_dormant_seat(
        &mut self,
        mut spec: super::super::handle::SpawnMemberSpec,
        mode: SeatPlanMode,
    ) -> Result<DormantSeatPlanEntry, MobError> {
        let agent_identity = spec.identity.clone();
        // CommitDormantSeat and BeginMemberRotation are Running-only: a
        // stopped mob seats and rotates nothing, refused here before any
        // session is created or any retirement starts.
        if self.state() != MobState::Running {
            return Err(self.invalid_transition_to(MobState::Running));
        }
        // A non-authoritative composition seats nothing (the machine refuses
        // the seat); refused here before a fresh seat's session is created in
        // the store it must not write.
        if self.dsl_authority.state().composition_authority
            == mob_dsl::CompositionAuthorityKind::NonAuthoritative
        {
            return Err(MobError::MobMachineRejected {
                context: "dormant_seat",
                reason: format!(
                    "'{agent_identity}' is not seated: this composition of the mob is not authoritative"
                ),
            });
        }
        if agent_identity.is_system_reserved() {
            return Err(MobError::WiringError(format!(
                "meerkat id '{agent_identity}' uses reserved system identifier namespace"
            )));
        }
        let launch = match &spec.launch_mode {
            crate::launch::MemberLaunchMode::Fresh => DormantSeatLaunch::Fresh,
            crate::launch::MemberLaunchMode::Resume { .. } if mode == SeatPlanMode::Seat => {
                DormantSeatLaunch::Resume
            }
            crate::launch::MemberLaunchMode::Resume { .. } => {
                return Err(MobError::Internal(format!(
                    "the rotation of '{agent_identity}' seats a fresh life: its spec must launch Fresh"
                )));
            }
            crate::launch::MemberLaunchMode::Fork { .. } => {
                return Err(MobError::Internal(format!(
                    "'{agent_identity}' cannot be seated dormant from a fork: a fork is spawned"
                )));
            }
        };
        if spec.placement.is_some()
            || spec.forked_participant_attachment.is_some()
            || spec.fork_build_inheritance.is_some()
            || spec.fork_source.is_some()
            || spec.fork_job.is_some()
            || spec.operator_authority_ceiling.is_some()
        {
            return Err(MobError::Internal(format!(
                "'{agent_identity}' cannot be seated dormant: a placed or fork-derived member is spawned"
            )));
        }
        if spec.resume_provider_params.is_some() && launch != DormantSeatLaunch::Resume {
            return Err(MobError::ResumeProviderParamsRequireResume {
                identity: agent_identity,
            });
        }
        if !self.definition.profiles.contains_key(&spec.role_name) {
            return Err(MobError::ProfileNotFound(spec.role_name.clone()));
        }

        // An identity already seated: an earlier seat of this very spec is
        // AlreadySeated; any other seat is the caller's rotation (a
        // rotation plans its next incarnation over the seated one).
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&agent_identity);
        let state = self.dsl_authority.state();
        let seated_now = state.identity_to_runtime.contains_key(&dsl_identity);
        if mode == SeatPlanMode::Seat && seated_now {
            let current_generation = state
                .identity_runtime_generations
                .get(&dsl_identity)
                .map(|generation| crate::ids::Generation::new(generation.0))
                .ok_or_else(|| {
                    MobError::Internal(format!(
                        "seated member '{agent_identity}' holds no generation"
                    ))
                })?;
            let bound = state
                .member_session_bindings
                .get(&dsl_identity)
                .map(|session| SessionId::parse(&session.0))
                .transpose()
                .map_err(|error| {
                    MobError::Internal(format!(
                        "'{agent_identity}' holds an invalid session binding: {error}"
                    ))
                })?;
            let seated_on = match launch {
                DormantSeatLaunch::Resume => spec.launch_mode.resume_bridge_session_id().cloned(),
                DormantSeatLaunch::Fresh => Some(fresh_seat_session_id(
                    &self.definition.id,
                    self.incarnation.incarnation(),
                    &agent_identity,
                    current_generation,
                )),
            };
            return match (bound, seated_on) {
                (Some(bound), Some(seated_on)) if bound == seated_on => {
                    Ok(DormantSeatPlanEntry::AlreadySeated {
                        agent_identity,
                        session_id: bound,
                    })
                }
                _ => Err(MobError::MemberAlreadyExists(agent_identity)),
            };
        }

        // The profile, resolved and authorized exactly as a spawn resolves
        // it on the actor.
        let full_profile_override_requested = spec.override_profile.is_some();
        let effective_model_override = spec.model_override.clone();
        let mut profile = match spec.override_profile.take() {
            Some(profile) => profile,
            None => {
                self.definition
                    .resolve_profile(&spec.role_name, self.realm_profile_store.as_ref())
                    .await?
            }
        };
        if let Some(model) = spec.model_override.clone() {
            profile.model = model;
        }
        super::super::fork_build::apply_resume_provider_params(
            &mut profile,
            spec.resume_provider_params.as_ref(),
        );
        super::super::spec_compiler::apply_tool_category_overrides(
            &mut profile,
            spec.tool_category_overrides,
        );
        let effective_profile_override = (full_profile_override_requested
            || spec.tool_category_overrides != meerkat_core::ToolCategoryOverrides::default())
        .then(|| profile.clone());
        let explicit_workgraph = profile.tools.workgraph;
        if spec.inherited_tool_filter.is_some() && effective_profile_override.is_none() {
            build::open_profile_tool_categories_for_inherited_filter(&mut profile);
        }
        let binding = resolve_binding(
            spec.binding.clone(),
            spec.backend,
            profile.backend,
            self.definition.backend.default,
            &agent_identity,
        )?;
        if !matches!(binding, crate::RuntimeBinding::Session) {
            return Err(MobError::Internal(format!(
                "'{agent_identity}' cannot be seated dormant: only a session-backed member is seated"
            )));
        }
        if profile.durable && !self.session_service.keeps_durable_member_sessions() {
            return Err(MobError::NonDurableService {
                member_id: agent_identity,
                profile: spec.role_name.clone(),
            });
        }
        let runtime_mode = spec.runtime_mode.unwrap_or(profile.runtime_mode);
        let generation = self.mint_spawn_generation(&agent_identity)?;

        // A fresh member that owes a kickoff (an AutonomousHost member's one
        // kickoff turn, or the initial message a fresh spawn runs as its
        // first turn) is embodied through the ordinary spawn path, as a 0.9.6
        // spawn is: it would be woken at once for the kickoff anyway. Its
        // session is the seat's name-based one. The rotation of a seated
        // member re-seats it Dormant and records the kickoff it owes in the
        // same step, which wakes the next incarnation once and runs exactly
        // once; the rotation of an identity that is not seated is a seat. A
        // resume owes none.
        let kickoff_owed = launch == DormantSeatLaunch::Fresh
            && (runtime_mode == crate::MobRuntimeMode::AutonomousHost
                || spec.initial_message.is_some());
        if kickoff_owed && (mode == SeatPlanMode::Seat || !seated_now) {
            spec.fresh_session_id = Some(fresh_seat_session_id(
                &self.definition.id,
                self.incarnation.incarnation(),
                &agent_identity,
                generation,
            ));
            return Ok(DormantSeatPlanEntry::Embody(Box::new(spec)));
        }

        let observations = observe_spawn_exec_facts(
            &profile,
            explicit_workgraph,
            spec.external_tools.is_some(),
            spec.shell_env.as_ref(),
            spec.inherited_tool_filter.is_some(),
            None,
        );
        let (authorize_input, authorized) =
            authorize_spawn_profile_input(&agent_identity, &spec.role_name, &profile, None)?;
        let session_id = match launch {
            DormantSeatLaunch::Resume => spec
                .launch_mode
                .resume_bridge_session_id()
                .cloned()
                .ok_or_else(|| {
                    MobError::Internal(format!(
                        "the resume seat of '{agent_identity}' names no session"
                    ))
                })?,
            DormantSeatLaunch::Fresh => fresh_seat_session_id(
                &self.definition.id,
                self.incarnation.incarnation(),
                &agent_identity,
                generation,
            ),
        };
        let continuity_intent = spec
            .continuity_intent
            .clone()
            .for_profile(&profile, &agent_identity);
        Ok(DormantSeatPlanEntry::Seat(Box::new(DormantSeatCandidate {
            agent_identity,
            profile_name: spec.role_name.clone(),
            launch,
            session_id,
            generation,
            // Decided by the preparation, from the session owner's
            // observation: a fresh seat's absent session is unborn.
            session_unborn: false,
            runtime_mode,
            labels: spec.labels.clone().unwrap_or_default(),
            continuity_intent,
            creation_origin: spec.creation_origin.clone(),
            effective_profile_override,
            effective_model_override,
            authorize_input,
            authorized,
            observations,
            kickoff_owed,
            kickoff_prompt: kickoff_owed.then(|| spec.initial_message.clone()).flatten(),
            kickoff_objective_id: spec.objective_id.unwrap_or_default(),
        })))
    }

    /// Commit one prepared seat chunk in one actor step, answering each
    /// entry in order.
    pub(super) async fn commit_dormant_seats(
        &mut self,
        entries: Vec<DormantSeatCommitEntry>,
    ) -> Result<Vec<DormantSeatOutcome>, MobError> {
        let mut outcomes: Vec<Option<DormantSeatOutcome>> = Vec::with_capacity(entries.len());
        let mut authority = self.dsl_authority.prepare_authority();
        let mut effects = Vec::new();
        let mut journal = Vec::new();
        let mut seated = Vec::new();
        // Identities planned after the scan: each new seat, and each
        // already-seated identity that is still Dormant.
        let mut plan = Vec::new();
        for (index, entry) in entries.into_iter().enumerate() {
            outcomes.push(None);
            match entry {
                DormantSeatCommitEntry::AlreadySeated {
                    agent_identity,
                    session_id,
                } => {
                    let dsl_identity = mob_dsl::AgentIdentity::from_domain(&agent_identity);
                    let still_seated = authority
                        .state()
                        .identity_to_runtime
                        .contains_key(&dsl_identity)
                        && authority.state().member_session_bindings.get(&dsl_identity)
                            == Some(&mob_dsl::SessionId::from_domain(&session_id));
                    if !still_seated {
                        outcomes[index] = Some(DormantSeatOutcome::Refused {
                            error: MobError::MemberNotFound(agent_identity.clone()),
                            agent_identity,
                        });
                        continue;
                    }
                    outcomes[index] = Some(
                        match Self::clear_host_preparation(&mut authority, &dsl_identity) {
                            Ok(cleared) => {
                                effects.extend(cleared);
                                plan.push(dsl_identity);
                                DormantSeatOutcome::AlreadySeated {
                                    agent_identity,
                                    session_id,
                                }
                            }
                            Err(error) => DormantSeatOutcome::Refused {
                                agent_identity,
                                error,
                            },
                        },
                    );
                }
                DormantSeatCommitEntry::Embodied {
                    agent_identity,
                    session_id,
                } => {
                    let dsl_identity = mob_dsl::AgentIdentity::from_domain(&agent_identity);
                    outcomes[index] = Some(
                        match Self::clear_host_preparation(&mut authority, &dsl_identity) {
                            Ok(cleared) => {
                                effects.extend(cleared);
                                DormantSeatOutcome::Embodied {
                                    agent_identity,
                                    session_id,
                                }
                            }
                            Err(error) => DormantSeatOutcome::Refused {
                                agent_identity,
                                error,
                            },
                        },
                    );
                }
                DormantSeatCommitEntry::Seat {
                    candidate,
                    endpoint,
                } => {
                    let agent_identity = candidate.agent_identity.clone();
                    let creation = self.seat_creation_record(&candidate).await;
                    match self.apply_dormant_seat(
                        &mut authority,
                        &candidate,
                        endpoint.as_ref(),
                        creation,
                    ) {
                        Ok(SeatApplied::Seated { transitions, event }) => {
                            for transition in transitions {
                                effects.extend(transition.into_effects());
                            }
                            journal.push(NewMobEvent {
                                mob_id: self.definition.id.clone(),
                                timestamp: None,
                                kind: MobEventKind::MemberSpawned(*event),
                            });
                            plan.push(mob_dsl::AgentIdentity::from_domain(&agent_identity));
                            seated.push(SeatedInChunk {
                                index,
                                agent_identity,
                                session_id: candidate.session_id.clone(),
                                endpoint,
                            });
                        }
                        Ok(SeatApplied::Refused(error)) => {
                            outcomes[index] = Some(DormantSeatOutcome::Refused {
                                agent_identity,
                                error,
                            });
                        }
                        // The machine accepted the seat without its typed
                        // authority: a defect, nothing of the chunk commits.
                        Err(error) => return Err(error),
                    }
                }
            }
        }

        // Each identity's boot verdict, from one runtime obligation scan
        // over the chunk's seated state. Exactly the Wake verdicts wake,
        // cause Boot, once the chunk is committed.
        if !plan.is_empty() {
            #[cfg(feature = "runtime-adapter")]
            let obligations = super::super::boot_plan::scan_boot_obligations(
                self.runtime_adapter.as_ref(),
                authority.state(),
                &self.definition.id,
                self.incarnation.incarnation(),
            )
            .await?;
            #[cfg(not(feature = "runtime-adapter"))]
            let obligations = super::super::boot_plan::BootObligations::none();
            let not_notified = BTreeSet::new();
            for dsl_identity in plan {
                if !super::super::boot_plan::member_is_dormant(authority.state(), &dsl_identity) {
                    continue;
                }
                let input = super::super::boot_plan::boot_materialization_input(
                    &dsl_identity,
                    &obligations,
                    &self.member_residency.host_required,
                    &not_notified,
                );
                match authority.apply_batch([input]) {
                    Ok(transitions) => {
                        for transition in transitions {
                            effects.extend(transition.into_effects());
                        }
                    }
                    // The member stays seated and Dormant: its first
                    // delivery wakes it.
                    Err(error) => tracing::warn!(
                        mob_id = %self.definition.id,
                        agent_identity = %dsl_identity.0,
                        %error,
                        "MobMachine refused a seated member's boot plan"
                    ),
                }
            }
        }

        let verify = effects
            .iter()
            .filter_map(|effect| match effect {
                mob_dsl::MobMachineEffect::VerifyMember { agent_identity } => {
                    Some(AgentIdentity::from(agent_identity.0.as_str()))
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let prepared = PreparedDslInput {
            authority,
            effects,
            phase_changed: false,
        };
        // Journal before commit: the machine never reports a member seated
        // before its MemberSpawned row is durable.
        let stored = if journal.is_empty() {
            self.commit_prepared_dsl_input(prepared)?;
            Vec::new()
        } else {
            let events = Arc::clone(&self.events);
            self.commit_prepared_dsl_input_after(prepared, move || async move {
                events.append_batch(journal).await.map_err(MobError::from)
            })
            .await?
        };
        {
            let mut roster = self.roster.write().await;
            for event in &stored {
                roster.apply_event(event);
            }
        }
        for seat in seated {
            tracing::debug!(
                mob_id = %self.definition.id,
                agent_identity = %seat.agent_identity,
                session_id = %seat.session_id,
                endpoint = seat.endpoint.is_some(),
                "seated a member dormant"
            );
            outcomes[seat.index] = Some(DormantSeatOutcome::Seated {
                agent_identity: seat.agent_identity,
                session_id: seat.session_id,
            });
        }
        // OnDemandVerified: the read-only verdict of each seated member the
        // plan leaves Dormant. The seats are durable already, so a pass that
        // cannot read is reported, not the chunk's failure: the members stay
        // Dormant and their wake classifies them as the pass would have.
        #[cfg(feature = "runtime-adapter")]
        if !verify.is_empty()
            && let Err(error) = self.verify_dormant_member_set(&verify).await
        {
            tracing::warn!(
                mob_id = %self.definition.id,
                %error,
                "the verified pass over newly seated members could not complete"
            );
        }
        #[cfg(not(feature = "runtime-adapter"))]
        let _ = verify;
        outcomes
            .into_iter()
            .map(|outcome| {
                outcome.ok_or_else(|| {
                    MobError::Internal("a seat chunk entry was left without an outcome".into())
                })
            })
            .collect()
    }

    /// Clear an identity's host preparation (its host's carry-out is this
    /// seat step): total, a no-op for an identity that is not pending.
    fn clear_host_preparation(
        authority: &mut mob_dsl::MobMachinePreparedAuthority,
        dsl_identity: &mob_dsl::AgentIdentity,
    ) -> Result<Vec<mob_dsl::MobMachineEffect>, MobError> {
        authority
            .apply_batch([mob_dsl::MobMachineInput::ResolveMemberHostPreparation {
                agent_identity: dsl_identity.clone(),
            }])
            .map(|transitions| {
                transitions
                    .into_iter()
                    .flat_map(mob_dsl::MobMachineTransition::into_effects)
                    .collect()
            })
            .map_err(|error| MobError::MobMachineRejected {
                context: "dormant_seat_resolve_host_preparation",
                reason: error.to_string(),
            })
    }

    /// Apply one seat on the chunk's prepared authority: the spawn ladder's
    /// admission (`AuthorizeSpawnProfile`, `BeginSpawnExec`) and
    /// `CommitDormantSeat`, as one batch. A refusal restores the prepared
    /// authority to before this identity and is answered alone.
    fn apply_dormant_seat(
        &self,
        authority: &mut mob_dsl::MobMachinePreparedAuthority,
        candidate: &DormantSeatCandidate,
        endpoint: Option<&TrustedPeerDescriptor>,
        creation: crate::MemberCreationRecord,
    ) -> Result<SeatApplied, MobError> {
        let agent_identity = &candidate.agent_identity;
        let fence_token = self.issue_fence_token()?;
        let agent_runtime_id =
            crate::ids::AgentRuntimeId::new(agent_identity.clone(), candidate.generation);
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(agent_identity);
        let dsl_runtime_id = mob_dsl::AgentRuntimeId::from_domain(&agent_runtime_id);
        let dsl_fence = mob_dsl::FenceToken::from_domain(fence_token);
        let dsl_generation = mob_dsl::Generation::from_domain(candidate.generation);
        let dsl_session = mob_dsl::SessionId::from_domain(&candidate.session_id);
        let runtime_mode = mob_dsl::SpawnPolicyRuntimeMode::from(candidate.runtime_mode);
        let external_addressable = candidate.authorized.external_addressable;
        let observations = &candidate.observations;
        let begin = mob_dsl::MobMachineInput::BeginSpawnExec {
            agent_identity: dsl_identity.clone(),
            agent_runtime_id: dsl_runtime_id.clone(),
            fence_token: dsl_fence,
            generation: dsl_generation,
            profile_material_digest: candidate.authorized.profile_material_digest.clone(),
            external_addressable,
            runtime_mode,
            bridge_session_id: Some(dsl_session.clone()),
            replacing: None,
            placement: None,
            workgraph_required: observations.workgraph_required,
            rust_bundles_present: observations.rust_bundles_present,
            per_spawn_external_tools_present: observations.per_spawn_external_tools_present,
            mob_default_external_tools_present: self.default_external_tools_provider.is_some(),
            default_llm_client_override_present: self.default_llm_client.is_some(),
            host_surface_mcp_allowlist_present: observations.host_surface_mcp_allowlist_present,
            inherited_tool_filter_present: observations.inherited_tool_filter_present,
            shell_env_present: observations.shell_env_present,
            mcp_stdio_env_present: observations.mcp_stdio_env_present,
            mcp_http_headers_present: observations.mcp_http_headers_present,
            memory_required: observations.memory_required,
            mcp_required: observations.mcp_required,
            resume_session_id: None,
            placed_spawn_id: None,
            placed_provision_operation_id: None,
            placed_operation_owner_session_id: None,
            effective_profile_override_present: false,
            effective_model_override_present: false,
        };
        let seat = mob_dsl::MobMachineInput::CommitDormantSeat {
            agent_identity: dsl_identity,
            agent_runtime_id: dsl_runtime_id,
            fence_token: dsl_fence,
            generation: dsl_generation,
            external_addressable,
            runtime_mode,
            bridge_session_id: dsl_session.clone(),
            member_peer_endpoint: endpoint.map(mob_dsl::MemberPeerEndpoint::from),
            // An archived session was answered at preparation, typed.
            session_archived: false,
            // A spec that owes a kickoff is embodied at the plan; the machine
            // refuses a seat that owes one.
            kickoff_owed: candidate.kickoff_owed,
            // The seat created nothing: a fresh seat's absent session is
            // unborn, and the member's first wake creates it.
            session_unborn: candidate.session_unborn,
        };
        let transitions =
            match authority.apply_batch([candidate.authorize_input.clone(), begin, seat]) {
                Ok(transitions) => transitions,
                Err(error) => {
                    return Ok(SeatApplied::Refused(MobError::MobMachineRejected {
                        context: "dormant_seat",
                        reason: error.to_string(),
                    }));
                }
            };
        let [authorized, _, committed] = transitions.as_slice() else {
            return Err(MobError::Internal(format!(
                "the dormant seat of '{agent_identity}' applied {} transitions for its three inputs",
                transitions.len()
            )));
        };
        require_authorized_effect(authorized, &candidate.authorized, "dormant_seat")?;
        Self::require_member_lifecycle_journal_effect(
            committed,
            mob_dsl::MobLifecycleJournalKind::MemberSpawned,
            agent_identity,
            &agent_runtime_id,
            Some(fence_token),
            candidate.generation,
            Some(dsl_session),
            "dormant_seat",
        )?;
        let mut event = crate::event::MemberSpawnedEvent::new(
            agent_identity.clone(),
            candidate.generation,
            fence_token,
            agent_runtime_id,
            candidate.profile_name.clone(),
        )
        .with_bridge_member_ref(Some(MemberRef::Session {
            session_id: candidate.session_id.clone(),
        }))
        .with_member_peer_endpoint(endpoint.cloned())
        .with_seat_session_unborn(candidate.session_unborn);
        event.creation = creation;
        event.runtime_mode = candidate.runtime_mode;
        event.labels = candidate.labels.clone();
        event.continuity_intent = candidate.continuity_intent.clone();
        event.effective_profile_override = candidate.effective_profile_override.clone();
        event.effective_model_override = candidate.effective_model_override.clone();
        Ok(SeatApplied::Seated {
            transitions,
            event: Box::new(event),
        })
    }

    /// The creation record of a seat, as a spawn records it: a resumed
    /// session keeps the creation its earlier member recorded; otherwise the
    /// spec's origin decides (a host-root seat is a root creation).
    async fn seat_creation_record(
        &self,
        candidate: &DormantSeatCandidate,
    ) -> crate::MemberCreationRecord {
        use crate::MemberCreationProvenance as Provenance;
        use crate::member_creation::MemberCreationOrigin as Origin;
        let unproven = || crate::MemberCreationRecord {
            creation_id: Some(crate::MemberCreationId::new()),
            provenance: Provenance::Unproven,
        };
        if candidate.launch == DormantSeatLaunch::Resume {
            match self
                .roster
                .read()
                .await
                .member_creation_for_session(&candidate.session_id)
            {
                Ok(Some(previous))
                    if previous.member_binding.member == candidate.agent_identity.as_str() =>
                {
                    return previous.creation;
                }
                Ok(None) => {}
                Ok(Some(_)) | Err(_) => return unproven(),
            }
        }
        let provenance = match &candidate.creation_origin {
            Origin::Source(witness) => match witness.source.as_ref() {
                Some(source) if witness.successor => {
                    if source.member_binding.mob_id != self.definition.id.as_str()
                        || source.member_binding.member != candidate.agent_identity.as_str()
                        || source.session_id == candidate.session_id
                    {
                        return unproven();
                    }
                    Provenance::Successor {
                        predecessor_session_id: source.session_id.clone(),
                        predecessor_member_binding: source.member_binding.clone(),
                        predecessor_creation_id: source.creation_id,
                    }
                }
                Some(source) => Provenance::Spawn {
                    source: source.clone(),
                },
                None => Provenance::Unproven,
            },
            Origin::HostRoot => Provenance::Root,
            Origin::Unproven => Provenance::Unproven,
        };
        let creation_id = match &provenance {
            Provenance::Successor {
                predecessor_creation_id,
                ..
            } => *predecessor_creation_id,
            _ => crate::MemberCreationId::new(),
        };
        crate::MemberCreationRecord {
            creation_id: Some(creation_id),
            provenance,
        }
    }
}

impl MobActor {
    /// Start a rotation of a seated identity (`rotate_member`): its current
    /// incarnation's retirement, which records the rotation at its admission
    /// (`BeginMemberRotation`) and seats the next incarnation in its terminal
    /// step (`CommitMemberRotation`). The caller is answered with the new
    /// session once the retirement settles. A retirement of the identity that
    /// is stuck is re-driven by it.
    pub(super) async fn start_member_rotation(
        &mut self,
        candidate: Box<DormantSeatCandidate>,
        endpoint: Option<TrustedPeerDescriptor>,
        reply: oneshot::Sender<Result<SessionId, MobError>>,
    ) {
        let identity = candidate.agent_identity.clone();
        let redrive = self.stuck_retirements.contains_key(&identity);
        Box::pin(self.start_retirement(
            identity,
            Instant::now() + super::super::provisioner::MEMBER_RETIRE_TOTAL_TIMEOUT,
            None,
            super::retirement_io::RetirementReply::Rotate {
                rotation: Box::new(MemberRotation {
                    candidate,
                    endpoint,
                }),
                reply,
            },
            redrive,
        ))
        .await;
    }

    /// Record the rotation on the retirement it admits, onto `authority`:
    /// the next session and endpoint (`BeginMemberRotation`) and the next
    /// incarnation's profile authority (`AuthorizeSpawnProfile`, consumed by
    /// the terminal step). The retirement's admission applies it in the same
    /// prepared step as its `Retire`, before the retirement's durable start,
    /// so a refused rotation journals nothing. A re-drive of the same
    /// rotation keeps the intent it recorded. Returns the effects to realize,
    /// or `None` when the intent and the authority are already recorded.
    pub(super) fn apply_member_rotation_begin(
        authority: &mut mob_dsl::MobMachinePreparedAuthority,
        entry: &RosterEntry,
        rotation: &MemberRotation,
    ) -> Result<Option<Vec<mob_dsl::MobMachineEffect>>, MobError> {
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(&entry.agent_identity);
        let next_session = mob_dsl::SessionId::from_domain(&rotation.candidate.session_id);
        let state = authority.state();
        let mut inputs = Vec::with_capacity(2);
        match state.member_rotation_session_ids.get(&dsl_identity) {
            Some(recorded) if recorded == &next_session => {}
            Some(_) => {
                return Err(MobError::LifecycleOperationPending {
                    intent: format!("rotation of member {}", entry.agent_identity),
                });
            }
            None => inputs.push(mob_dsl::MobMachineInput::BeginMemberRotation {
                agent_identity: dsl_identity.clone(),
                agent_runtime_id: mob_dsl::AgentRuntimeId::from_domain(&entry.agent_runtime_id),
                fence_token: mob_dsl::FenceToken::from_domain(entry.fence_token),
                generation: mob_dsl::Generation::from_domain(entry.generation),
                session_id: next_session,
                peer_endpoint: rotation
                    .endpoint
                    .as_ref()
                    .map(mob_dsl::MemberPeerEndpoint::from),
                // An archived session was answered at preparation, typed.
                session_archived: false,
            }),
        }
        let authorize = !state
            .spawn_profile_authority_profile_names
            .contains_key(&dsl_identity);
        if authorize {
            inputs.push(rotation.candidate.authorize_input.clone());
        }
        if inputs.is_empty() {
            return Ok(None);
        }
        let transitions =
            authority
                .apply_batch(inputs)
                .map_err(|error| MobError::MobMachineRejected {
                    context: "begin_member_rotation",
                    reason: error.to_string(),
                })?;
        if authorize {
            let authorized = transitions.last().ok_or_else(|| {
                MobError::Internal("the rotation's profile authority applied nothing".into())
            })?;
            require_authorized_effect(
                authorized,
                &rotation.candidate.authorized,
                "begin_member_rotation",
            )?;
        }
        Ok(Some(
            transitions
                .into_iter()
                .flat_map(mob_dsl::MobMachineTransition::into_effects)
                .collect(),
        ))
    }

    /// [`Self::apply_member_rotation_begin`] on its own, for a rotation
    /// whose retirement already started durably (a re-drive), committed in
    /// one step.
    pub(super) fn begin_member_rotation(
        &mut self,
        entry: &RosterEntry,
        rotation: &MemberRotation,
    ) -> Result<(), MobError> {
        let mut authority = self.dsl_authority.prepare_authority();
        let Some(effects) = Self::apply_member_rotation_begin(&mut authority, entry, rotation)?
        else {
            return Ok(());
        };
        self.commit_prepared_dsl_input(PreparedDslInput {
            authority,
            effects,
            phase_changed: false,
        })
    }

    /// The next sessions of the rotations a destroy's admission abandoned
    /// (`RotationSessionsAbandoned`): sessions no member will own.
    pub(super) fn abandoned_rotation_sessions(
        effects: &[mob_dsl::MobMachineEffect],
    ) -> Vec<(AgentIdentity, SessionId)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                mob_dsl::MobMachineEffect::RotationSessionsAbandoned { rotations } => {
                    Some(rotations)
                }
                _ => None,
            })
            .flat_map(|rotations| rotations.iter())
            .filter_map(|(identity, session)| match SessionId::parse(&session.0) {
                Ok(session_id) => Some((AgentIdentity::from(identity.0.as_str()), session_id)),
                Err(error) => {
                    tracing::warn!(
                        agent_identity = %identity.0,
                        session = %session.0,
                        %error,
                        "an abandoned rotation names an invalid session id"
                    );
                    None
                }
            })
            .collect()
    }

    /// Archive the sessions of rotations a destroy abandoned. Each holds no
    /// member and no runtime, and is usually unborn (absent, which archives
    /// as nothing): a failure is logged and leaves the session behind; the
    /// destroy goes on.
    pub(super) async fn archive_abandoned_rotation_sessions(
        &mut self,
        abandoned: Vec<(AgentIdentity, SessionId)>,
    ) {
        for (identity, session_id) in abandoned {
            match self.session_service.archive(&session_id).await {
                Ok(()) | Err(meerkat_core::service::SessionError::NotFound { .. }) => {}
                Err(error) => tracing::warn!(
                    mob_id = %self.definition.id,
                    agent_identity = %identity,
                    %session_id,
                    %error,
                    "could not archive the session of a rotation the destroy abandoned"
                ),
            }
        }
    }

    /// The rotation's archive terminal: the current incarnation retired
    /// and the next one seated Dormant on the recorded session and endpoint,
    /// in one transition. Its `MemberRetired` and `MemberSpawned` rows go to
    /// the journal in one `append_batch` before the commit, so the identity is
    /// never a non-member. The roster takes the next incarnation at once.
    pub(super) async fn commit_member_rotation(
        &mut self,
        ctx: &DisposalContext,
        disposal: mob_dsl::MemberSessionDisposal,
        rotation: &MemberRotation,
    ) -> Result<(), MobError> {
        let entry = &ctx.entry;
        let identity = &ctx.agent_identity;
        let dsl_identity = mob_dsl::AgentIdentity::from_domain(identity);
        if self.retire_event_exists(identity, entry.generation).await? {
            return Err(MobError::Internal(format!(
                "the rotation of '{identity}' found its retirement already journaled without its next incarnation"
            )));
        }
        let session_id = self
            .dsl_authority
            .state()
            .member_session_bindings
            .get(&dsl_identity)
            .cloned()
            .or_else(|| {
                entry
                    .member_ref
                    .bridge_session_id()
                    .map(mob_dsl::SessionId::from_domain)
            });
        let candidate = &rotation.candidate;
        let next_generation = candidate.generation;
        let next_fence = self.issue_fence_token()?;
        let next_runtime_id = crate::ids::AgentRuntimeId::new(identity.clone(), next_generation);
        let next_session = mob_dsl::SessionId::from_domain(&candidate.session_id);
        let rotation_input = mob_dsl::MobMachineInput::CommitMemberRotation {
            agent_identity: dsl_identity.clone(),
            agent_runtime_id: mob_dsl::AgentRuntimeId::from_domain(&entry.agent_runtime_id),
            fence_token: mob_dsl::FenceToken::from_domain(entry.fence_token),
            generation: mob_dsl::Generation::from_domain(entry.generation),
            session_id: session_id.clone(),
            disposal,
            next_agent_runtime_id: mob_dsl::AgentRuntimeId::from_domain(&next_runtime_id),
            next_fence_token: mob_dsl::FenceToken::from_domain(next_fence),
            next_generation: mob_dsl::Generation::from_domain(next_generation),
            runtime_mode: mob_dsl::SpawnPolicyRuntimeMode::from(candidate.runtime_mode),
            external_addressable: candidate.authorized.external_addressable,
            // The kickoff the spec owes, recorded pending on the next
            // incarnation in the same step.
            kickoff_owed: candidate.kickoff_owed,
            kickoff_objective_id: candidate.kickoff_objective_id.to_string(),
        };
        let mut authority = self.dsl_authority.prepare_authority();
        let mut transitions = authority.apply_batch([rotation_input]).map_err(|error| {
            MobError::MobMachineRejected {
                context: "commit_member_rotation",
                reason: error.to_string(),
            }
        })?;
        let committed = transitions.first().ok_or_else(|| {
            MobError::Internal("the rotation's terminal applied no transition".into())
        })?;
        Self::require_member_lifecycle_journal_effect(
            committed,
            mob_dsl::MobLifecycleJournalKind::MemberRetired,
            identity,
            &entry.agent_runtime_id,
            None,
            entry.generation,
            session_id,
            "commit_member_rotation",
        )?;
        Self::require_member_lifecycle_journal_effect(
            committed,
            mob_dsl::MobLifecycleJournalKind::MemberSpawned,
            identity,
            &next_runtime_id,
            Some(next_fence),
            next_generation,
            Some(next_session),
            "commit_member_rotation",
        )?;
        // The owed kickoff's row, journaled after the re-seat in the same
        // batch: replay records it pending on the next incarnation.
        let kickoff_row = if candidate.kickoff_owed {
            let recorded = committed.effects().iter().any(|effect| {
                matches!(
                    effect,
                    mob_dsl::MobMachineEffect::PersistKickoffUpdate {
                        member_id,
                        phase: mob_dsl::KickoffPhase::Pending,
                    } if member_id == &dsl_identity
                )
            });
            if !recorded {
                return Err(MobError::Internal(format!(
                    "the rotation of '{identity}' owes a kickoff its terminal step did not record"
                )));
            }
            Some(crate::roster::MobMemberKickoffSnapshot {
                objective_id: Some(required_machine_kickoff_objective_id(
                    authority.state(),
                    identity,
                )?),
                phase: crate::roster::MobMemberKickoffPhase::Pending,
                error: None,
                updated_at: SystemTime::now(),
            })
        } else {
            None
        };
        // The next incarnation's boot verdict, in the same step. Its session
        // is new and empty, so it holds no runtime work; a kickoff it owes
        // makes the verdict Wake. A stopped mob plans it at its lift.
        if authority.state().lifecycle_phase == mob_dsl::MobPhase::Running {
            let plan = super::super::boot_plan::boot_materialization_input(
                &dsl_identity,
                &super::super::boot_plan::BootObligations::none(),
                &self.member_residency.host_required,
                &BTreeSet::new(),
            );
            match authority.apply_batch([plan]) {
                Ok(planned) => transitions.extend(planned),
                // The member stays seated and Dormant: its first delivery
                // wakes it.
                Err(error) => tracing::warn!(
                    mob_id = %self.definition.id,
                    agent_identity = %identity,
                    %error,
                    "MobMachine refused a rotated member's boot plan"
                ),
            }
        }
        let phase_changed = transitions
            .iter()
            .any(|transition| transition.from_phase != transition.to_phase);
        let effects: Vec<mob_dsl::MobMachineEffect> = transitions
            .into_iter()
            .flat_map(mob_dsl::MobMachineTransition::into_effects)
            .collect();
        let kickoff_notices = effects
            .iter()
            .filter_map(|effect| match effect {
                mob_dsl::MobMachineEffect::EmitKickoffLifecycleNotice { member_id, intent }
                    if member_id == &dsl_identity =>
                {
                    Some(Self::kickoff_notice_intent(*intent))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let prepared = PreparedDslInput {
            authority,
            effects,
            phase_changed,
        };
        let creation = self.seat_creation_record(candidate).await;
        let mut spawned = crate::event::MemberSpawnedEvent::new(
            identity.clone(),
            next_generation,
            next_fence,
            next_runtime_id,
            candidate.profile_name.clone(),
        )
        .with_bridge_member_ref(Some(MemberRef::Session {
            session_id: candidate.session_id.clone(),
        }))
        .with_member_peer_endpoint(rotation.endpoint.clone())
        .with_kickoff_prompt(candidate.kickoff_prompt.clone())
        // The next incarnation's session is unborn (CommitMemberRotation
        // records it so): its first wake creates it.
        .with_seat_session_unborn(true);
        spawned.creation = creation;
        spawned.runtime_mode = candidate.runtime_mode;
        spawned.labels = candidate.labels.clone();
        spawned.continuity_intent = candidate.continuity_intent.clone();
        spawned.effective_profile_override = candidate.effective_profile_override.clone();
        spawned.effective_model_override = candidate.effective_model_override.clone();
        let mut batch = vec![
            NewMobEvent {
                mob_id: self.definition.id.clone(),
                timestamp: None,
                kind: MobEventKind::MemberRetired {
                    agent_identity: identity.clone(),
                    generation: entry.generation,
                    role: entry.role.clone(),
                },
            },
            NewMobEvent {
                mob_id: self.definition.id.clone(),
                timestamp: None,
                kind: MobEventKind::MemberSpawned(spawned),
            },
        ];
        if let Some(kickoff) = kickoff_row {
            batch.push(NewMobEvent {
                mob_id: self.definition.id.clone(),
                timestamp: None,
                kind: MobEventKind::MemberKickoffUpdated {
                    member: identity.clone(),
                    kickoff,
                },
            });
        }
        let events = Arc::clone(&self.events);
        let stored = self
            .commit_prepared_dsl_input_after(prepared, move || async move {
                events.append_batch(batch).await.map_err(MobError::from)
            })
            .await?;
        self.retired_event_index
            .write()
            .await
            .insert(Self::retire_event_key(identity, entry.generation));
        {
            let mut roster = self.roster.write().await;
            for event in &stored {
                roster.apply_event(event);
            }
        }
        // The owed kickoff's notices, effects of the now-durable step; a
        // failure is presentation-only, as for every kickoff notice.
        for intent in kickoff_notices {
            if let Err(error) = self.notify_kickoff_event(identity, intent).await {
                tracing::warn!(
                    agent_identity = %identity,
                    %error,
                    intent,
                    "failed to emit the rotated member's kickoff notice"
                );
            }
        }
        Ok(())
    }
}

/// One seat applied on the chunk's prepared authority.
enum SeatApplied {
    /// Seated: its transitions' effects and its `MemberSpawned` row.
    Seated {
        transitions: Vec<mob_dsl::MobMachineTransition>,
        event: Box<crate::event::MemberSpawnedEvent>,
    },
    /// The machine refused it; the prepared authority is as before it.
    Refused(MobError),
}
