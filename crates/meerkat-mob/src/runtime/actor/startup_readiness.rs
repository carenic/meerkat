//! The startup readiness pass of a resumed mob.
//!
//! The members a crash-path start materialized (its boot set) leave the
//! builder bound and Waking: their readiness and topology settle before
//! MobMachine holds them Live (R7). The topology settled in the builder;
//! this pass runs their readiness with the readiness of every other member
//! materialized in this process, then applies each boot member's
//! `ResolveMemberRevivalSucceeded` under its open attempt.
//!
//! A member's readiness failure is that member's outcome, never the
//! mob's (X6): a boot member's attempt is abandoned with the typed
//! readiness failure, which records it Broken; a Live member whose comms
//! runtime is gone is woken again through the drift arm; any other Live
//! member failure records that member Broken. The mob keeps serving every
//! other member.

use super::*;

impl MobActor {
    /// Run the startup readiness of every member materialized in this
    /// process and settle the start's boot set (see the module docs).
    /// Errors only for failures that are not one member's: a lifecycle that
    /// fences readiness, or a refused machine input.
    ///
    /// It runs in `prepare_actor_run`, before the command loop exists: every
    /// member's bounded readiness step runs concurrently, so the pass is
    /// bounded by one `RESUME_MEMBER_READINESS_BOUND` rather than by the
    /// member count, and commands sent meanwhile queue until the loop starts.
    /// It fires no synthetic kickoff turn. Once a local autonomous member's
    /// drain and injector were observed, its exact runtime and fence are
    /// published through MobMachine's startup readiness transition: durable
    /// membership alone is not readiness.
    pub(super) async fn ensure_startup_member_readiness(&mut self) -> Result<(), MobError> {
        let boot_attempts = std::mem::take(&mut self.startup_boot_attempts);
        let Some(mut targets) = self.collect_member_readiness_targets(false).await? else {
            return Ok(());
        };
        {
            // The boot set is Waking, so the Live-only target set leaves it
            // out: its readiness is this start's to run.
            let roster = self.roster.read().await;
            for identity in boot_attempts.keys() {
                if let Some(entry) = roster.get(identity) {
                    targets.push(MemberReadinessTarget {
                        entry: entry.clone(),
                        stage: member_readiness_stage(entry),
                    });
                }
            }
        }
        #[cfg(feature = "runtime-adapter")]
        let targets = self
            .break_members_whose_runtime_ended_outside_the_mob(targets)
            .await;
        let expected = member_readiness_expectations(&targets);
        let readiness = self.spawn_member_readiness_tasks(targets, None);
        let outcomes =
            collect_member_readiness_outcomes(readiness, expected, &self.definition.id).await;
        let mut settled = Vec::with_capacity(outcomes.len());
        for outcome in outcomes {
            let boot_attempt = boot_attempts.get(&outcome.entry.agent_identity).copied();
            match (&outcome.result, boot_attempt) {
                (Ok(()), Some(attempt)) => {
                    self.settle_startup_boot_member_live(&outcome.entry, attempt)
                        .await;
                }
                (Err(error), Some(attempt)) => {
                    self.abandon_startup_boot_member(&outcome.entry, attempt, error)
                        .await;
                    continue;
                }
                (Err(error), None) => {
                    self.resolve_startup_live_member_failure(&outcome.entry, error)
                        .await;
                    continue;
                }
                (Ok(()), None) => {}
            }
            settled.push(outcome);
        }
        // Every outcome left is a success: the kept-kickoff re-arm and the
        // autonomous startup marks run as for any readiness pass.
        self.apply_member_readiness_outcomes(settled).await
    }

    /// A boot member's readiness settled: MobMachine holds it Live under the
    /// attempt its plan opened (R7: Live means ready). A stale attempt (the
    /// member's attempt closed meanwhile) is the machine's no-op.
    async fn settle_startup_boot_member_live(&mut self, entry: &RosterEntry, attempt: u64) {
        let identity = mob_dsl::AgentIdentity::from_domain(&entry.agent_identity);
        match self.apply_dsl_signal_collect_transition(
            mob_dsl::MobMachineSignal::ResolveMemberRevivalSucceeded {
                agent_identity: identity.clone(),
                attempt,
            },
            "startup_boot_member_ready",
        ) {
            Ok(transition)
                if super::super::boot_plan::wake_feedback_applied(
                    transition.effects(),
                    &identity,
                    attempt,
                ) =>
            {
                self.report_startup_boot_member_built(entry);
            }
            Ok(_) => {
                tracing::info!(
                    mob_id = %self.definition.id,
                    agent_identity = %entry.agent_identity,
                    attempt,
                    "a boot member's attempt closed before its startup readiness settled it"
                );
            }
            Err(error) => {
                tracing::error!(
                    mob_id = %self.definition.id,
                    agent_identity = %entry.agent_identity,
                    %error,
                    "MobMachine refused the startup readiness of a boot member"
                );
            }
        }
    }

    /// Tell the host's build preparer that a boot member was built, off the
    /// actor (the visible tool record of the member's build).
    fn report_startup_boot_member_built(&mut self, entry: &RosterEntry) {
        #[cfg(feature = "runtime-adapter")]
        {
            let Some(preparer) = self.member_residency.build_preparer.clone() else {
                return;
            };
            let Some(session_id) = entry.member_ref.bridge_session_id().cloned() else {
                return;
            };
            let session_service = Arc::clone(&self.session_service);
            let identity = entry.agent_identity.clone();
            self.actor_io_tasks.spawn(async move {
                crate::runtime::member_build_preparer::report_member_built(
                    preparer,
                    session_service,
                    identity,
                    session_id,
                )
                .await;
            });
        }
        #[cfg(not(feature = "runtime-adapter"))]
        let _ = entry;
    }

    /// A boot member's readiness failed: its attempt is abandoned with the
    /// typed readiness failure, which records it Broken (member-scoped).
    async fn abandon_startup_boot_member(
        &mut self,
        entry: &RosterEntry,
        attempt: u64,
        error: &MobError,
    ) {
        let identity = mob_dsl::AgentIdentity::from_domain(&entry.agent_identity);
        let reason = format!(
            "member '{}' was restored at this start but its readiness failed: {error}",
            entry.agent_identity
        );
        tracing::warn!(
            mob_id = %self.definition.id,
            agent_identity = %entry.agent_identity,
            %error,
            "boot member readiness failed; the member is Broken and the mob keeps running"
        );
        match self.apply_dsl_signal_collect_transition(
            mob_dsl::MobMachineSignal::AbandonMemberWake {
                agent_identity: identity.clone(),
                attempt,
                kind: mob_dsl::MemberWakeAbandonKind::ReadinessFailed,
                reason: reason.clone(),
            },
            "startup_boot_member_readiness_failed",
        ) {
            Ok(transition)
                if super::super::boot_plan::wake_feedback_applied(
                    transition.effects(),
                    &identity,
                    attempt,
                ) =>
            {
                self.record_startup_member_broken(entry, reason).await;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(
                    mob_id = %self.definition.id,
                    agent_identity = %entry.agent_identity,
                    %error,
                    "MobMachine refused to abandon a boot member's failed readiness"
                );
            }
        }
    }

    /// A Live member's startup readiness failed. A member whose comms runtime
    /// is gone is not materialized any more: the drift arm makes it Waking and
    /// the wake path materializes it again. Any other failure records the
    /// member Broken. Neither stops the mob.
    async fn resolve_startup_live_member_failure(&mut self, entry: &RosterEntry, error: &MobError) {
        let identity = mob_dsl::AgentIdentity::from_domain(&entry.agent_identity);
        let lost_materialization = matches!(
            error,
            MobError::MissingMemberCapability {
                capability: crate::error::MobMemberCapability::OutboundCommsRuntime,
                ..
            }
        );
        tracing::warn!(
            mob_id = %self.definition.id,
            agent_identity = %entry.agent_identity,
            %error,
            lost_materialization,
            "member readiness failed at startup; the outcome is the member's, the mob keeps running"
        );
        if lost_materialization {
            if let Err(classify_error) = self.apply_dsl_signal(
                mob_dsl::MobMachineSignal::ClassifyMemberLiveMaterialization {
                    agent_identity: identity,
                    observation:
                        mob_dsl::MemberLiveMaterializationObservationKind::DurableSnapshotPresent,
                    reason: format!("the member's comms runtime is missing at startup: {error}"),
                },
                "startup_member_materialization_lost",
            ) {
                tracing::error!(
                    mob_id = %self.definition.id,
                    agent_identity = %entry.agent_identity,
                    error = %classify_error,
                    "MobMachine refused the startup drift observation of a member"
                );
            }
            return;
        }
        let reason = format!(
            "member '{}' readiness failed at startup: {error}",
            entry.agent_identity
        );
        if let Err(recover_error) = self.apply_dsl_signal(
            mob_dsl::MobMachineSignal::RecoverMemberRestoreFailure {
                agent_identity: identity,
                reason: reason.clone(),
            },
            "startup_member_readiness_failed",
        ) {
            tracing::error!(
                mob_id = %self.definition.id,
                agent_identity = %entry.agent_identity,
                error = %recover_error,
                "MobMachine refused the startup readiness failure of a member"
            );
            return;
        }
        self.record_startup_member_broken(entry, reason).await;
    }

    /// The handle diagnostic of a member the startup readiness recorded
    /// Broken, with its projected comms identity cleared, as every other
    /// restore failure leaves it.
    async fn record_startup_member_broken(&mut self, entry: &RosterEntry, reason: String) {
        {
            let mut roster = self.roster.write().await;
            let mut snapshot = roster.snapshot();
            let _ = snapshot.set_comms_identity(&entry.agent_identity, None, None);
            *roster = super::super::roster_authority::RosterAuthority::from_roster(snapshot);
        }
        self.record_member_restore_failure(
            &entry.agent_identity,
            super::super::handle::RestoreFailureDiagnostic {
                bridge_session_id: entry.member_ref.bridge_session_id().cloned(),
                reason,
                hold: None,
                registration_refusal: None,
            },
        )
        .await;
        self.publish_machine_state_projection();
    }
}
