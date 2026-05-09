//! Capacity admission helpers for the session runtime.
//!
//! Populated by W1-A (`StagedCapacityAdmissions` typedef + helpers)
//! and W1-C (`RuntimePreAdmission` family of RAII guards).

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use meerkat_core::types::SessionId;
use meerkat_session::RuntimeContextAdmissionGuard;

/// Type alias for a capacity guard issued by the session service for a
/// staged or running session. The actual guard type lives in
/// `meerkat-session`; we re-alias here so the runtime crate can use a
/// stable name without leaking the dependency path through every call
/// site.
pub type ActiveCapacityGuard = RuntimeContextAdmissionGuard;

/// Map of staged sessions to their reserved capacity admissions.
///
/// The shared mutex models the staged-session capacity ledger: while a
/// session is staged in `StagedSessionRegistry`, its admission is held
/// here and restored if promotion fails. The lock is `std::sync::Mutex`
/// because every operation is short and synchronous.
pub type StagedCapacityAdmissions = Arc<StdMutex<HashMap<SessionId, ActiveCapacityGuard>>>;

/// Restore a previously-taken admission back into the staged-capacity
/// ledger. Used by RAII guards (`RuntimePreAdmission`,
/// `PendingPromotionCleanup`, …) on rollback paths.
pub fn restore_staged_capacity_admission(
    admissions: &StagedCapacityAdmissions,
    session_id: SessionId,
    admission: ActiveCapacityGuard,
) {
    let mut admissions = admissions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    admissions.insert(session_id, admission);
}

/// Reason an `insert_staged_capacity_admission` call could not be
/// satisfied. Surface-agnostic: surfaces translate to their own error
/// shape (`RpcError::SESSION_BUSY`, HTTP 409, …) at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedCapacityCollision {
    /// The session id whose admission slot was already populated.
    pub session_id: SessionId,
}

/// Insert a fresh admission into the staged-capacity ledger.
///
/// Returns `Err(StagedCapacityCollision)` when the session id already
/// has an admission staged — surfaces map this onto their own
/// session-busy wire error.
pub fn insert_staged_capacity_admission(
    admissions: &StagedCapacityAdmissions,
    session_id: SessionId,
    admission: ActiveCapacityGuard,
) -> Result<(), StagedCapacityCollision> {
    let mut guard = admissions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.contains_key(&session_id) {
        return Err(StagedCapacityCollision { session_id });
    }
    guard.insert(session_id, admission);
    Ok(())
}

/// Remove and return the staged admission for `session_id`, if any.
pub fn take_staged_capacity_admission(
    admissions: &StagedCapacityAdmissions,
    session_id: &SessionId,
) -> Option<ActiveCapacityGuard> {
    admissions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(session_id)
}

/// Whether `session_id` currently holds a staged admission.
pub fn has_staged_capacity_admission(
    admissions: &StagedCapacityAdmissions,
    session_id: &SessionId,
) -> bool {
    admissions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_key(session_id)
}

/// Drop the staged admission for `session_id` if present. The guard's
/// `Drop` returns capacity to the pool.
pub fn discard_staged_capacity_admission(
    admissions: &StagedCapacityAdmissions,
    session_id: &SessionId,
) {
    drop(take_staged_capacity_admission(admissions, session_id));
}
