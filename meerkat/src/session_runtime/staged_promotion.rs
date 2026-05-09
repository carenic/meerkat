//! Staged-session promotion lifecycle.
//!
//! Populated by W1-B (`PendingPromotionCleanup` + `Mode` enum + Drop)
//! and W2-D (the staged-session promotion methods on the runtime).

use std::collections::BTreeMap;
use std::sync::Arc;

use meerkat_core::service::SessionError;
use meerkat_core::types::SessionId;
use meerkat_core::{ContentInput, Session, SessionLlmIdentity, SessionSystemContextState};

use meerkat_session::{MachineSessionArchiveProtocol, PersistentSessionService};

use crate::session_runtime::admission::{
    ActiveCapacityGuard, StagedCapacityAdmissions, restore_staged_capacity_admission,
};
use crate::{AgentBuildConfig, FactoryAgentBuilder, PromotingSlot, StagedSessionRegistry};

/// Cleanup mode for [`PendingPromotionCleanup`].
///
/// `Restore` is the default for an in-flight staged session whose
/// promotion has not yet been finalized: Drop restores the slot to the
/// staged registry. `Finish` is set after the promotion materializes
/// successfully; Drop only reaps the promoting system-context state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingPromotionCleanupMode {
    /// Restore the staged slot back to the registry on Drop.
    Restore,
    /// Reap promoting metadata on Drop, but do not restore.
    Finish,
}

/// RAII bookkeeping around a staged session that is in the middle of
/// being promoted into a live session.
///
/// While `armed` and `mode == Restore`, Drop spawns a tokio task that
/// re-stages the slot in [`StagedSessionRegistry`] and returns the
/// reserved capacity to [`StagedCapacityAdmissions`]. After
/// `mark_materialized` flips `mode` to `Finish`, Drop only reaps the
/// promoting system-context state.
pub struct PendingPromotionCleanup {
    pub(crate) staged_sessions: Arc<StagedSessionRegistry>,
    pub(crate) staged_capacity_admissions: StagedCapacityAdmissions,
    pub(crate) session_id: SessionId,
    pub(crate) staged_capacity_admission: Option<ActiveCapacityGuard>,
    pub(crate) build_config: Option<AgentBuildConfig>,
    pub(crate) effective_llm_identity: SessionLlmIdentity,
    pub(crate) labels: Option<BTreeMap<String, String>>,
    pub(crate) deferred_prompt: Option<ContentInput>,
    pub(crate) created_at_secs: u64,
    pub(crate) updated_at_secs: u64,
    pub(crate) mode: PendingPromotionCleanupMode,
    pub(crate) machine_archived_resume_authorized: bool,
    pub(crate) armed: bool,
}

impl PendingPromotionCleanup {
    /// Build a fresh cleanup guard from a [`PromotingSlot`] snapshot.
    pub fn new(
        staged_sessions: Arc<StagedSessionRegistry>,
        staged_capacity_admissions: StagedCapacityAdmissions,
        session_id: &SessionId,
        slot: &PromotingSlot,
        staged_capacity_admission: Option<ActiveCapacityGuard>,
    ) -> Self {
        Self {
            staged_sessions,
            staged_capacity_admissions,
            session_id: session_id.clone(),
            staged_capacity_admission,
            build_config: Some((*slot.build_config).clone()),
            effective_llm_identity: slot.effective_llm_identity.clone(),
            labels: slot.labels.clone(),
            deferred_prompt: slot.deferred_prompt.clone(),
            created_at_secs: slot.created_at_secs,
            updated_at_secs: slot.updated_at_secs,
            mode: PendingPromotionCleanupMode::Restore,
            machine_archived_resume_authorized: slot.machine_archived_resume_authorized,
            armed: true,
        }
    }

    /// Replace the staged build config that would be re-staged on
    /// rollback. No-op once disarmed.
    pub fn update_build_config(&mut self, build_config: &AgentBuildConfig) {
        if self.armed {
            self.build_config = Some(build_config.clone());
        }
    }

    /// Replace the LLM identity that would be re-staged on rollback.
    pub fn update_effective_llm_identity(&mut self, effective_llm_identity: SessionLlmIdentity) {
        if self.armed {
            self.effective_llm_identity = effective_llm_identity;
        }
    }

    /// Flip into `Finish` mode after the promotion materialized
    /// successfully. The Drop will no longer attempt to re-stage.
    pub fn mark_materialized(&mut self) {
        if self.armed {
            self.mode = PendingPromotionCleanupMode::Finish;
            self.staged_capacity_admission = None;
        }
    }

    /// Detach the staged-capacity admission for the caller to consume.
    pub fn take_staged_capacity_admission(&mut self) -> Option<ActiveCapacityGuard> {
        self.staged_capacity_admission.take()
    }

    /// Reserve a fresh staged-capacity admission if the guard does not
    /// already hold one.
    pub async fn replenish_staged_capacity_admission(
        &mut self,
        service: &PersistentSessionService<FactoryAgentBuilder>,
    ) -> Result<(), SessionError> {
        if self.staged_capacity_admission.is_none() {
            self.staged_capacity_admission =
                Some(service.reserve_create_session_admission().await?);
        }
        Ok(())
    }

    /// Reserve a runtime-turn admission for the materialized session.
    pub async fn recover_materialized_staged_capacity_admission(
        &mut self,
        service: &PersistentSessionService<FactoryAgentBuilder>,
    ) -> Result<(), SessionError> {
        if self.staged_capacity_admission.is_none() {
            self.staged_capacity_admission = Some(
                service
                    .reserve_runtime_turn_admission(&self.session_id)
                    .await?,
            );
        }
        Ok(())
    }

    /// Abort restore when no admission is available; reaps promoting
    /// metadata and disarms.
    pub async fn abort_restore_without_capacity(&mut self) {
        tracing::warn!(
            session_id = %self.session_id,
            "aborting staged-session restore without a capacity admission"
        );
        let _ = self
            .staged_sessions
            .take_promoting_system_context_state(&self.session_id)
            .await;
        self.armed = false;
    }

    /// Restore the staged session after a materialized-side failure
    /// (e.g. a pre-run apply failure on the live session).
    pub async fn restore_after_materialized_failure(
        &mut self,
        service: &PersistentSessionService<FactoryAgentBuilder>,
        protocol: MachineSessionArchiveProtocol<'_>,
    ) -> Result<(), SessionError> {
        if let Err(error) = self
            .recover_materialized_staged_capacity_admission(service)
            .await
        {
            self.abort_restore_without_capacity().await;
            return Err(error);
        }

        if let Err(error) = service
            .archive_with_machine_protocol(&self.session_id, protocol)
            .await
        {
            let _ = service.discard_live_session(&self.session_id).await;
            self.restore_now().await;
            return Err(error);
        }
        self.finish_after_machine_archive().await;
        Ok(())
    }

    /// Reap promoting state and disarm after a successful machine
    /// archive.
    pub async fn finish_after_machine_archive(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self
            .staged_sessions
            .take_promoting_system_context_state(&self.session_id)
            .await;
        let _ = self.staged_sessions.abandon(&self.session_id).await;
        self.build_config = None;
        drop(self.staged_capacity_admission.take());
        self.armed = false;
    }

    /// Mark the session as authorized to resume from machine-archived
    /// state on its next promotion attempt.
    pub fn authorize_machine_archived_resume(&mut self) {
        if !self.armed {
            return;
        }
        self.machine_archived_resume_authorized = true;
    }

    /// Copy the current promoting system-context state into
    /// `build_config` so a re-stage preserves it.
    pub async fn preserve_promoting_system_context_state(
        staged_sessions: &StagedSessionRegistry,
        session_id: &SessionId,
        build_config: &mut AgentBuildConfig,
    ) {
        let Some((_starting_system_context_state, current_system_context_state)) = staged_sessions
            .promoting_system_context_state(session_id)
            .await
        else {
            return;
        };
        let session = build_config
            .resume_session
            .get_or_insert_with(|| Session::with_id(session_id.clone()));
        if let Err(err) = session.set_system_context_state(current_system_context_state) {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "failed to preserve promoting system-context state while restoring staged session"
            );
        }
    }

    /// Synchronously restore the staged session and return its
    /// admission to the staged-capacity ledger.
    pub async fn restore_now(&mut self) {
        if !self.armed {
            return;
        }
        let Some(mut build_config) = self.build_config.take() else {
            self.armed = false;
            return;
        };
        Self::preserve_promoting_system_context_state(
            &self.staged_sessions,
            &self.session_id,
            &mut build_config,
        )
        .await;
        let Some(admission) = self.staged_capacity_admission.take() else {
            self.abort_restore_without_capacity().await;
            return;
        };
        let restored = self
            .staged_sessions
            .abandon_promotion(
                self.session_id.clone(),
                build_config,
                self.effective_llm_identity.clone(),
                self.labels.clone(),
                self.deferred_prompt.clone(),
                self.created_at_secs,
                self.updated_at_secs,
                self.machine_archived_resume_authorized,
            )
            .await;
        if restored {
            restore_staged_capacity_admission(
                &self.staged_capacity_admissions,
                self.session_id.clone(),
                admission,
            );
        }
        self.armed = false;
    }

    /// Reap promoting system-context state synchronously when in
    /// `Finish` mode; returns the (starting, current) pair if any.
    pub async fn finish_now(
        &mut self,
    ) -> Option<(SessionSystemContextState, SessionSystemContextState)> {
        if !self.armed || self.mode != PendingPromotionCleanupMode::Finish {
            return None;
        }
        self.staged_sessions
            .take_promoting_system_context_state(&self.session_id)
            .await
    }

    /// Suppress all Drop-time cleanup. Used by callers that have
    /// committed the promotion via a different path.
    pub fn disarm(&mut self) {
        self.armed = false;
        self.build_config = None;
        self.staged_capacity_admission = None;
    }
}

impl Drop for PendingPromotionCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let staged_sessions = Arc::clone(&self.staged_sessions);
        let staged_capacity_admissions = Arc::clone(&self.staged_capacity_admissions);
        let session_id = self.session_id.clone();
        match self.mode {
            PendingPromotionCleanupMode::Restore => {
                let Some(mut build_config) = self.build_config.take() else {
                    return;
                };
                let staged_capacity_admission = self.staged_capacity_admission.take();
                let effective_llm_identity = self.effective_llm_identity.clone();
                let labels = self.labels.clone();
                let deferred_prompt = self.deferred_prompt.clone();
                let created_at_secs = self.created_at_secs;
                let updated_at_secs = self.updated_at_secs;
                let machine_archived_resume_authorized = self.machine_archived_resume_authorized;
                tokio::spawn(async move {
                    let Some(admission) = staged_capacity_admission else {
                        tracing::warn!(
                            session_id = %session_id,
                            "aborting staged-session drop restore without a capacity admission"
                        );
                        let _ = staged_sessions
                            .take_promoting_system_context_state(&session_id)
                            .await;
                        return;
                    };
                    Self::preserve_promoting_system_context_state(
                        staged_sessions.as_ref(),
                        &session_id,
                        &mut build_config,
                    )
                    .await;
                    let restored = staged_sessions
                        .abandon_promotion(
                            session_id.clone(),
                            build_config,
                            effective_llm_identity,
                            labels,
                            deferred_prompt,
                            created_at_secs,
                            updated_at_secs,
                            machine_archived_resume_authorized,
                        )
                        .await;
                    if restored {
                        restore_staged_capacity_admission(
                            &staged_capacity_admissions,
                            session_id,
                            admission,
                        );
                    }
                });
            }
            PendingPromotionCleanupMode::Finish => {
                tokio::spawn(async move {
                    let _ = staged_sessions
                        .take_promoting_system_context_state(&session_id)
                        .await;
                });
            }
        }
    }
}
