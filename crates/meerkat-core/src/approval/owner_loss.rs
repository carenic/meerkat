//! Consent owner loss (3): the unspent consent a lost runtime owner left in
//! one session's scope is settled row-only by the approval owner that issues
//! consent in the runtime now holding the session's live claim.
//!
//! Consent custody keeps its session's hosting claim lineage while it lives
//! (see [`crate::WorkAuthorizationContext::with_consent_hosting`]). A claim
//! lineage whose grant excluded every earlier holder of the session's claim
//! owes one settlement
//! ([`crate::session_hosting::HostingClaim::reserve_owner_loss`]): no earlier
//! runtime owner still holds custody of consent in that scope, so a pending
//! row there can never be decided into spendable custody, and an approved
//! one can never be spent. The settlement records that through the generated
//! approval owner's row-only owner-loss transition
//! ([`ApprovalService::settle_lost_owner_consent`]), without building a
//! candidate and without executing anything.
//!
//! Its authority is a borrow of the attaching registration's live
//! reservation of that lineage, for the whole settlement: the reservation
//! keeps the session's claim held. The proof the reservation carries only
//! names the scope; a proof alone is historical exclusion data and settles
//! nothing.

use std::collections::{BTreeMap, BTreeSet};

use super::consent::{OwnerLossRefusal, OwnerLossSettlement};
use super::{
    ApprovalConsentScope, ApprovalError, ApprovalId, ApprovalRecord, ApprovalService,
    ApprovalServiceState, ApprovalStatus, restore_record_into_authority,
};
use crate::session_hosting::OwnerLossReservation;

/// The issuing consent owner's settlement of what a lost runtime owner left
/// in one session's scope, composed into the runtime that hosts the
/// session's registrations. A registration whose claim lineage owes a
/// settlement reserves it, and has this owner settle while it holds the
/// reservation, before the session can admit anything.
pub trait ConsentOwnerLossSettlement: Send + Sync {
    /// Settle what a lost owner left in the scope of `reservation`'s claim
    /// lineage, under that live reservation. An error means the settlement
    /// is incomplete: what it already committed stays settled, and the
    /// registration is refused and settles the current rows again when it
    /// retries.
    fn settle(&self, reservation: &OwnerLossReservation) -> Result<(), ApprovalError>;
}

impl ConsentOwnerLossSettlement for ApprovalService {
    fn settle(&self, reservation: &OwnerLossReservation) -> Result<(), ApprovalError> {
        self.settle_owner_loss(reservation).map(|_| ())
    }
}

impl ApprovalService {
    /// Settle the consent a lost owner left in the scope of `reservation`:
    /// the hosting domain and session of the live claim lineage whose grant
    /// excluded every earlier holder of the session's claim.
    ///
    /// Every consent row committed in exactly that scope whose consent is
    /// still unspent and unwithdrawn (Pending, or Approved with no spend) is
    /// settled through the generated owner-loss transition, in one
    /// conditional row write each: a pending row becomes Cancelled, also
    /// past its deadline, which it keeps (Expired history is never
    /// rewritten); an approved row keeps its decision history and can no
    /// longer be spent; both record the withdrawal (Abandoned) with its
    /// Withdrawn notice owed. No candidate is built and nothing executes.
    ///
    /// Custody this owner holds live on the reservation's own claim lineage
    /// was admitted under the live claim: it is not lost and is left to that
    /// custody. Issuing a row is not custody: a row whose custody is gone
    /// (for example after an uncertain write) is settled like any other.
    ///
    /// The store is read outside the owner's reservation. A row of the
    /// scope whose stored form this owner no longer knows is first
    /// reconciled through the owner's ordinary reconciliation; a row this
    /// owner never loaded is restored as history before it is settled. A
    /// row that stays unknown, or that the generated owner or the store
    /// refuses to settle, leaves the settlement incomplete: this returns
    /// that error after settling every other row. Rows already settled stay
    /// settled, and a retry reads the current rows again and never owes a
    /// notice twice. Returns the approvals this call settled.
    pub fn settle_owner_loss(
        &self,
        reservation: &OwnerLossReservation,
    ) -> Result<Vec<ApprovalId>, ApprovalError> {
        self.ensure_available()?;
        let proof = reservation.proof();
        let scope = ApprovalConsentScope {
            domain: proof.domain().clone(),
            session_id: proof.session_id().clone(),
        };
        let stored: BTreeMap<ApprovalId, ApprovalRecord> = self
            .store
            .load_all()
            .map_err(|error| ApprovalError::StoreUnavailable(error.to_string()))?
            .into_iter()
            .filter(|record| in_scope(record, &scope))
            .map(|record| (record.approval_id.clone(), record))
            .collect();
        self.reconcile_unknown_rows(reservation, &scope, &stored);
        self.settle_owner_now().run();
        let mut state = self.state.write();
        self.count_review_lock();
        // Withdrawals whose custody is already retired are recorded first,
        // under their own reason.
        self.record_pending_withdrawals_locked(&mut state);
        let mut settled = Vec::new();
        let mut incomplete = None;
        for approval_id in rows_to_settle(&state, &scope, &stored) {
            match self.settle_lost_row_locked(
                &mut state,
                reservation,
                &scope,
                &approval_id,
                stored.get(&approval_id),
            ) {
                Ok(true) => settled.push(approval_id),
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        %approval_id,
                        %error,
                        "an owner-loss settlement left a consent row of its scope unsettled"
                    );
                    if incomplete.is_none() {
                        incomplete = Some(error);
                    }
                }
            }
        }
        self.release_state(state).run();
        match incomplete {
            Some(error) => Err(error),
            None => Ok(settled),
        }
    }

    /// Reconcile, through the owner's ordinary reconciliation, every row of
    /// `scope` whose stored form this owner no longer knows, except custody
    /// held on the reservation's own lineage. A row that cannot be
    /// reconciled now stays unknown; the settlement then reports it.
    fn reconcile_unknown_rows(
        &self,
        reservation: &OwnerLossReservation,
        scope: &ApprovalConsentScope,
        stored: &BTreeMap<ApprovalId, ApprovalRecord>,
    ) {
        let unknown: Vec<ApprovalId> = {
            let state = self.state.read();
            state
                .unreconciled
                .keys()
                .filter(|approval_id| {
                    (stored.contains_key(*approval_id)
                        || state
                            .records
                            .get(*approval_id)
                            .is_some_and(|record| in_scope(record, scope)))
                        && !held_on_lineage(&state, approval_id, reservation)
                })
                .cloned()
                .collect()
        };
        for approval_id in unknown {
            if let Err(error) = self.reconcile(&approval_id) {
                tracing::debug!(
                    %approval_id,
                    %error,
                    "an unknown consent row of an owner-loss scope is not reconciled yet"
                );
            }
        }
    }

    /// Settle one row of the scope under the owner's reservation. Returns
    /// whether its settled form committed; an error leaves the row
    /// unsettled and the settlement incomplete.
    fn settle_lost_row_locked(
        &self,
        state: &mut ApprovalServiceState,
        reservation: &OwnerLossReservation,
        scope: &ApprovalConsentScope,
        approval_id: &ApprovalId,
        stored: Option<&ApprovalRecord>,
    ) -> Result<bool, ApprovalError> {
        if held_on_lineage(state, approval_id, reservation) {
            return Ok(false);
        }
        // A row this owner never loaded (another owner wrote it after this
        // owner last read its store) is restored as history, as the store's
        // rows are when the owner starts.
        if let Some(stored) = stored
            && !state.records.contains_key(approval_id)
            && !state.unreconciled.contains_key(approval_id)
        {
            let mut authority = state.authority.clone();
            restore_record_into_authority(&mut authority, stored)?;
            state.authority = authority;
            state.records.insert(approval_id.clone(), stored.clone());
        }
        match self.settle_lost_owner_consent_locked(state, approval_id, scope)? {
            OwnerLossSettlement::Settled(_) => Ok(true),
            // Spent, expired or already settled: nothing is owed.
            OwnerLossSettlement::Refused(
                OwnerLossRefusal::Consumed | OwnerLossRefusal::Expired | OwnerLossRefusal::Settled,
            ) => Ok(false),
            OwnerLossSettlement::Refused(OwnerLossRefusal::NotFound) => {
                Err(ApprovalError::NotFound {
                    approval_id: approval_id.clone(),
                })
            }
            // Custody held here outside the reservation's lineage, or a row
            // the generated owner does not take for this scope's consent:
            // never counted as settled.
            OwnerLossSettlement::Refused(
                refusal @ (OwnerLossRefusal::HeldHere
                | OwnerLossRefusal::NotConsent
                | OwnerLossRefusal::OutOfScope),
            ) => Err(ApprovalError::LifecycleInvariant(format!(
                "owner-loss settlement of session {} refused consent {approval_id} as {refusal:?}",
                scope.session_id
            ))),
        }
    }
}

/// Whether this owner holds live custody of `approval_id` on the very claim
/// lineage `reservation` holds.
fn held_on_lineage(
    state: &ApprovalServiceState,
    approval_id: &ApprovalId,
    reservation: &OwnerLossReservation,
) -> bool {
    state
        .consent
        .get(approval_id)
        .is_some_and(|candidate| candidate.keeps_lineage_of(reservation))
}

/// The rows of `scope` the settlement accounts for, in id order: every row
/// of the scope this owner holds as unknown, and every row of the scope
/// whose stored or known form is still unspent and unwithdrawn.
fn rows_to_settle(
    state: &ApprovalServiceState,
    scope: &ApprovalConsentScope,
    stored: &BTreeMap<ApprovalId, ApprovalRecord>,
) -> BTreeSet<ApprovalId> {
    let known = state
        .records
        .values()
        .filter(|record| in_scope(record, scope));
    stored
        .values()
        .chain(known)
        .filter(|record| {
            state.unreconciled.contains_key(&record.approval_id) || left_unspent(record)
        })
        .map(|record| record.approval_id.clone())
        .collect()
}

/// A consent row committed in exactly `scope`.
fn in_scope(record: &ApprovalRecord, scope: &ApprovalConsentScope) -> bool {
    record
        .consent
        .as_ref()
        .is_some_and(|consent| consent.scope.as_ref() == Some(scope))
}

/// A consent row whose consent is still unspent and unwithdrawn: Pending, or
/// Approved with no spend.
fn left_unspent(record: &ApprovalRecord) -> bool {
    record.consent.as_ref().is_some_and(|consent| {
        consent.consumed_at.is_none()
            && consent.withdrawal.is_none()
            && matches!(
                record.status,
                ApprovalStatus::Pending | ApprovalStatus::Approved
            )
    })
}
