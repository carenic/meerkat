//! The session-level error of a cold predecessor settlement
//! (`PersistentSessionService::settle_cold_predecessor_before`), shared by
//! every target that composes the runtime machine, so a mob service on any
//! target can carry it.

use meerkat_core::error::AgentError;
use meerkat_core::service::SessionError;
use meerkat_core::types::SessionId;
use meerkat_runtime::{
    ColdPredecessorSettlement, PredecessorSettlementRefusal, RuntimeDriverError,
};

/// A refused or stopped cold predecessor settlement, with the final evidence
/// it made durable before it stopped.
#[derive(Debug, thiserror::Error)]
#[error("cold predecessor settlement of session {session_id}: {refusal}")]
#[non_exhaustive]
pub struct ColdPredecessorSettleError {
    pub session_id: SessionId,
    pub refusal: ColdPredecessorSettleRefusal,
    /// Final evidence of what the settlement made durable before it stopped:
    /// the tail commit, the owner-loss outcome, the inputs that fully
    /// converged, and the ids of the inputs still owing a receipt or an
    /// append. Inputs the settlement did not reach, and a commit whose
    /// outcome is unknown, are not reported. Untouched when it refused before
    /// touching anything. Never a complete census: no closed-row count and
    /// no migrated rows are reported.
    pub progress: ColdPredecessorSettlement,
}

impl ColdPredecessorSettleError {
    /// A refusal that touched nothing.
    #[must_use]
    pub fn untouched(session_id: SessionId, refusal: ColdPredecessorSettleRefusal) -> Self {
        Self {
            progress: ColdPredecessorSettlement::untouched(session_id.clone()),
            session_id,
            refusal,
        }
    }

    /// A refusal with the final evidence the settlement made durable.
    #[must_use]
    pub fn with_progress(
        refusal: ColdPredecessorSettleRefusal,
        progress: ColdPredecessorSettlement,
    ) -> Self {
        Self {
            session_id: progress.session_id.clone(),
            refusal,
            progress,
        }
    }
}

/// Why a cold predecessor settlement refused, or stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ColdPredecessorSettleRefusal {
    /// The runtime settlement's own refusal.
    #[error(transparent)]
    Settlement(PredecessorSettlementRefusal),
    /// A session refusal: `Busy` (application work owns the actor, or a live
    /// actor is hosted here), `NotFound` (unknown or archived),
    /// `Unsupported` (no settlement on this service), or an event-store
    /// failure.
    #[error(transparent)]
    Session(SessionError),
}

impl From<ColdPredecessorSettleError> for SessionError {
    fn from(error: ColdPredecessorSettleError) -> Self {
        let id = error.session_id;
        match error.refusal {
            ColdPredecessorSettleRefusal::Session(error) => error,
            ColdPredecessorSettleRefusal::Settlement(refusal) => match refusal {
                PredecessorSettlementRefusal::Registered => SessionError::Busy { id },
                refusal @ (PredecessorSettlementRefusal::ClaimNotOwned
                | PredecessorSettlementRefusal::ExclusivityUnproven
                | PredecessorSettlementRefusal::ProfileUnsupported { .. }) => {
                    SessionError::Unsupported(format!(
                        "cold predecessor settlement of session {id}: {refusal}"
                    ))
                }
                PredecessorSettlementRefusal::TailHeld => {
                    SessionError::DurableTailHeldForRecovery { id }
                }
                PredecessorSettlementRefusal::TailRefused => {
                    SessionError::DurableTailRecoveryRefused { id }
                }
                PredecessorSettlementRefusal::Tail(error) => match error {
                    meerkat_runtime::recovery::DurableTailRecoveryError::Store(
                        meerkat_runtime::store::RuntimeStoreError::AuditedEndpointDivergence {
                            ..
                        },
                    ) => SessionError::WholeBlobAuditedEndpointDivergence { id },
                    error => SessionError::Agent(AgentError::InternalError(format!(
                        "durable-tail recovery for session {id}: {error}"
                    ))),
                },
                PredecessorSettlementRefusal::Runtime(error) => {
                    if let Some(hosting) = error.hosting_session_error() {
                        return hosting;
                    }
                    if let Some(hold) = error.durable_hold_session_error() {
                        return hold;
                    }
                    if let Some(teardown) = error.teardown_in_progress_session_error() {
                        return teardown;
                    }
                    SessionError::Agent(AgentError::InternalError(error.to_string()))
                }
                // Rows the settlement cannot account for read as the exact
                // receipt reader reports such rows: corruption, kept internal
                // at this surface with their ids in the typed refusal.
                refusal @ PredecessorSettlementRefusal::UnsettleableRows { .. } => {
                    internal(RuntimeDriverError::RecoveryCorruption {
                        reason: format!("cold predecessor settlement of session {id}: {refusal}"),
                    })
                }
                // A missing placement witness and an incomplete closing
                // census are a blocked repair, kept internal at this surface
                // with their ids in the typed refusal.
                refusal => internal(RuntimeDriverError::RecoveryRepairBlocked {
                    evidence_digest: None,
                    reason: format!("cold predecessor settlement of session {id}: {refusal}"),
                }),
            },
        }
    }
}

/// The session layer's internal mapping of a runtime error.
fn internal(error: RuntimeDriverError) -> SessionError {
    SessionError::Agent(AgentError::InternalError(error.to_string()))
}
