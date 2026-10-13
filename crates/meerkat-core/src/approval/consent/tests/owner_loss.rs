//! Consent owner loss (3): under a borrow of the live reservation of a claim
//! lineage whose grant excluded every earlier holder of a session's hosting
//! claim, the issuing approval owner settles the unspent consent a lost
//! owner left in exactly that session's scope.

use std::collections::BTreeSet;

use super::*;
use crate::approval::{
    ApprovalConsentDeliveryState, ApprovalConsentTransition, ConsentOwnerLossSettlement,
};
use crate::session_hosting::{
    HostingAcquisition, HostingCapability, HostingClaim, HostingOwner, HostingPaths,
    HostingRefused, OwnerLossAttachment, OwnerLossReservation, grant_session_hosting,
};

/// Cross-process hosting claims in a hosting domain under `dir`.
fn os_lock(dir: &std::path::Path) -> HostingCapability {
    HostingCapability::OsLock(Arc::new(HostingPaths {
        hosting_lock_dir: dir.join("hosting"),
        cold_delivery_lock: dir.join("cold-delivery.lock"),
        database: None,
    }))
}

/// The live reservation of `claim`'s lineage, which owes its settlement.
fn reserved(claim: &HostingClaim) -> OwnerLossReservation {
    match claim.reserve_owner_loss() {
        OwnerLossAttachment::Reserved(reservation) => Some(reservation),
        OwnerLossAttachment::Settled | OwnerLossAttachment::InProgress => None,
    }
    .expect("the lineage owes its settlement")
}

/// The consent scope a reservation's proof names.
fn scope_of(reservation: &OwnerLossReservation) -> ApprovalConsentScope {
    ApprovalConsentScope {
        domain: reservation.proof().domain().clone(),
        session_id: reservation.proof().session_id().clone(),
    }
}

/// A new runtime owner's fresh claim of `session`, kept held, with the live
/// reservation of the settlement its lineage owes and the scope it names.
fn owner_loss_of(
    capability: &HostingCapability,
    session: &SessionId,
) -> (HostingClaim, OwnerLossReservation, ApprovalConsentScope) {
    let claim =
        grant_session_hosting(capability, &HostingOwner::mint(), session).expect("a fresh claim");
    let reservation = reserved(&claim);
    let scope = scope_of(&reservation);
    (claim, reservation, scope)
}

/// A world issuing consent into `store` from routed work scoped to `scope`,
/// as another runtime owner's process issues it (it holds no claim here).
fn scoped_world(store: &Arc<InMemoryApprovalStore>, scope: Option<ApprovalConsentScope>) -> World {
    let mut w = world_with(ApprovalService::with_store(store.clone()).expect("issuer"));
    w.work = w.work.clone().with_retained_work(origin_work("runtime-a"));
    if let Some(scope) = scope {
        w.work = w.work.clone().with_consent_scope(scope);
    }
    w
}

/// A world issuing consent through `approvals` from routed work that
/// carries `claim`'s lineage, as the runtime holding that claim issues it.
fn hosted_world(approvals: ApprovalService, claim: &HostingClaim) -> World {
    let mut w = world_with(approvals);
    w.work = w
        .work
        .clone()
        .with_consent_hosting(claim.clone())
        .with_retained_work(origin_work("runtime-b"));
    w
}

fn row(store: &InMemoryApprovalStore, id: &ApprovalId) -> ApprovalRecord {
    store.get(id).expect("read").expect("row")
}

fn withdrawal_reason(record: &ApprovalRecord) -> Option<ApprovalConsentWithdrawalReason> {
    record
        .consent
        .as_ref()
        .and_then(|consent| consent.withdrawal.as_ref())
        .map(|withdrawal| withdrawal.reason.clone())
}

fn withdrawn_owed() -> (ApprovalConsentTransition, ApprovalConsentDeliveryState) {
    (
        ApprovalConsentTransition::Withdrawn,
        ApprovalConsentDeliveryState::Owed,
    )
}

/// Move `id`'s stored deadline into the past, as if its validity ran out
/// while no owner observed it.
fn deadline_passed(store: &InMemoryApprovalStore, id: &ApprovalId) {
    let stored = row(store, id);
    let mut past = stored.clone();
    past.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
    store
        .put_if_current(Some(&stored), &past)
        .expect("a deadline already due in the store");
}

/// A lost owner's pending consent in the reservation's scope becomes
/// Cancelled and its approved consent keeps its decision but can no longer
/// be spent; both record an Abandoned withdrawal and owe their Withdrawn
/// notice in the same row write. Spent and already withdrawn consent of that
/// scope, and consent of another session, another domain or no scope at
/// all, are left exactly as they were. A second settlement settles nothing.
#[tokio::test]
async fn an_owner_loss_settles_the_unspent_consent_of_exactly_its_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let capability = os_lock(dir.path());
    let session = SessionId::new();
    let (_claim, reservation, scope) = owner_loss_of(&capability, &session);
    let store = Arc::new(InMemoryApprovalStore::new());

    let lost = scoped_world(&store, Some(scope.clone()));
    let pending = lost.request(&lost.check()).await;
    let approved = lost.request(&lost.check()).await;
    lost.approvals
        .decide_consent(&approved, decision(ApprovalDecision::Approve, "lead-1"))
        .expect("approved");
    let spent = lost.request(&lost.check()).await;
    lost.approvals
        .decide_consent(&spent, decision(ApprovalDecision::Approve, "lead-1"))
        .expect("approved");
    assert_eq!(lost.spend(&spent), Ok(()));
    // Work with a scope but no notice destination: its run's close withdraws
    // its consent instead of parking it.
    let mut closed = world_with(ApprovalService::with_store(store.clone()).expect("issuer"));
    closed.work = closed.work.clone().with_consent_scope(scope.clone());
    let withdrawn = closed.request(&closed.check()).await;
    closed.review.close_consent_for_work(&closed.work);
    assert!(
        withdrawal_reason(&row(&store, &withdrawn)).is_some(),
        "withdrawn at its run's close"
    );

    let other_session = scoped_world(
        &store,
        Some(ApprovalConsentScope {
            domain: scope.domain.clone(),
            session_id: SessionId::new(),
        }),
    );
    let elsewhere = other_session.request(&other_session.check()).await;
    let other_domain = scoped_world(
        &store,
        Some(consent_scope("realm-b/hosting", session.clone())),
    );
    let foreign = other_domain.request(&other_domain.check()).await;
    let unscoped_world = scoped_world(&store, None);
    let unscoped = unscoped_world.request(&unscoped_world.check()).await;
    let untouched: Vec<(ApprovalId, ApprovalRecord)> =
        [&spent, &withdrawn, &elsewhere, &foreign, &unscoped]
            .into_iter()
            .map(|id| (id.clone(), row(&store, id)))
            .collect();

    let owner = ApprovalService::with_store(store.clone()).expect("the new owner");
    let settled: BTreeSet<ApprovalId> = owner
        .settle_owner_loss(&reservation)
        .expect("settled")
        .into_iter()
        .collect();
    assert_eq!(
        settled,
        [pending.clone(), approved.clone()].into_iter().collect(),
        "exactly the lost owner's unspent consent of the reservation's scope is settled"
    );

    let cancelled = row(&store, &pending);
    assert_eq!(cancelled.status, ApprovalStatus::Cancelled);
    assert_eq!(
        withdrawal_reason(&cancelled),
        Some(ApprovalConsentWithdrawalReason::Abandoned)
    );
    assert_eq!(
        deliveries_of(&store, &pending),
        vec![withdrawn_owed()],
        "the withdrawal owes its notice"
    );
    let unspendable = row(&store, &approved);
    assert_eq!(unspendable.status, ApprovalStatus::Approved);
    assert!(unspendable.decision.is_some(), "the decision history stays");
    assert_eq!(
        withdrawal_reason(&unspendable),
        Some(ApprovalConsentWithdrawalReason::Abandoned)
    );
    assert!(
        deliveries_of(&store, &approved).contains(&withdrawn_owed()),
        "the withdrawal owes its own notice besides the decision's"
    );
    for (id, before) in untouched {
        assert_eq!(row(&store, &id), before, "{id} is left as it was");
    }
    assert!(
        owner
            .settle_owner_loss(&reservation)
            .expect("settled again")
            .is_empty(),
        "settled consent is never settled twice"
    );
}

/// A lost owner's pending consent whose deadline already passed is still
/// settled through the owner-loss transition (no custody remains to
/// withdraw it on expiry), keeping its deadline. Consent already recorded
/// Expired keeps its history.
#[tokio::test]
async fn an_owner_loss_settles_expired_pending_consent_and_keeps_expired_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let capability = os_lock(dir.path());
    let session = SessionId::new();
    let (_claim, reservation, scope) = owner_loss_of(&capability, &session);
    let store = Arc::new(InMemoryApprovalStore::new());
    let lost = scoped_world(&store, Some(scope));
    let overdue = lost.request(&lost.check()).await;
    let expired = lost.request(&lost.check()).await;
    deadline_passed(&store, &expired);
    ApprovalService::with_store(store.clone())
        .expect("an observer")
        .list(ApprovalListFilter::default())
        .expect("an observer records the expiry");
    let observed = row(&store, &expired);
    assert_eq!(observed.status, ApprovalStatus::Expired);
    // The other deadline passes after that observation: the row is still
    // Pending in the store.
    deadline_passed(&store, &overdue);
    let still_pending = row(&store, &overdue);
    assert_eq!(still_pending.status, ApprovalStatus::Pending);

    let owner = ApprovalService::with_store(store.clone()).expect("the new owner");
    let settled = owner.settle_owner_loss(&reservation).expect("settled");
    assert_eq!(settled, vec![overdue.clone()]);
    let cancelled = row(&store, &overdue);
    assert_eq!(cancelled.status, ApprovalStatus::Cancelled);
    assert_eq!(
        withdrawal_reason(&cancelled),
        Some(ApprovalConsentWithdrawalReason::Abandoned)
    );
    assert_eq!(
        cancelled.expires_at, still_pending.expires_at,
        "the deadline is kept"
    );
    assert_eq!(deliveries_of(&store, &overdue), vec![withdrawn_owed()]);
    assert_eq!(
        row(&store, &expired),
        observed,
        "Expired history is never rewritten"
    );
}

/// The issuing service settles through itself, composed as the runtime's
/// settlement: custody it holds live on the attaching claim's own lineage is
/// left as it is, a lost owner's rows written after it read its store are
/// read from the store and settled, and its own records hold what it
/// settled.
#[tokio::test]
async fn the_issuing_service_settles_through_itself() {
    let dir = tempfile::tempdir().expect("tempdir");
    let capability = os_lock(dir.path());
    let session = SessionId::new();
    let (claim, reservation, scope) = owner_loss_of(&capability, &session);
    let store = Arc::new(InMemoryApprovalStore::new());
    let issuer = hosted_world(
        ApprovalService::with_store(store.clone()).expect("issuer"),
        &claim,
    );
    let own = issuer.request(&issuer.check()).await;
    let lost = scoped_world(&store, Some(scope));
    let pending = lost.request(&lost.check()).await;
    let approved = lost.request(&lost.check()).await;
    lost.approvals
        .decide_consent(&approved, decision(ApprovalDecision::Approve, "lead-1"))
        .expect("approved");
    assert!(
        !issuer.approvals.state.read().records.contains_key(&pending),
        "the issuer never loaded the lost owner's rows"
    );

    let settlement: Arc<dyn ConsentOwnerLossSettlement> = Arc::new(issuer.approvals.clone());
    assert_eq!(settlement.settle(&reservation), Ok(()));

    let cancelled = row(&store, &pending);
    assert_eq!(cancelled.status, ApprovalStatus::Cancelled);
    assert_eq!(
        withdrawal_reason(&cancelled),
        Some(ApprovalConsentWithdrawalReason::Abandoned)
    );
    assert_eq!(
        issuer.record(&pending),
        cancelled,
        "the issuing service's own records hold what it settled"
    );
    let unspendable = row(&store, &approved);
    assert_eq!(unspendable.status, ApprovalStatus::Approved);
    assert_eq!(
        withdrawal_reason(&unspendable),
        Some(ApprovalConsentWithdrawalReason::Abandoned)
    );
    let kept = row(&store, &own);
    assert_eq!(
        kept.status,
        ApprovalStatus::Pending,
        "custody on the attaching claim's own lineage is left as it is"
    );
    assert!(withdrawal_reason(&kept).is_none());
    assert!(
        issuer.approvals.state.read().consent.contains_key(&own),
        "and stays live"
    );
}

/// The same issuing service approves its own consent, and the approval's
/// write commits with its outcome unknown: the row is durably Approved and
/// unspent, the candidate custody is gone, and the issuance record and the
/// unknown mark stay. Once every claim of that lineage is released and a new
/// grant takes actual exclusion, settling never reports an empty success:
/// it stays incomplete for as long as the row is unknown and writes
/// nothing. After the owner's ordinary reconciliation reads the row, it is
/// settled with its approval history kept and one withdrawal owed; settling
/// again adds no obligation.
#[tokio::test]
async fn an_unknown_approved_row_keeps_the_settlement_incomplete_until_reconciled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let capability = os_lock(dir.path());
    let owner = HostingOwner::mint();
    let session = SessionId::new();
    let store = Arc::new(FaultStore::default());
    let first = grant_session_hosting(&capability, &owner, &session).expect("the issuer's claim");
    let mut issuer = hosted_world(
        ApprovalService::with_store(store.clone()).expect("issuer"),
        &first,
    );
    {
        let reservation = reserved(&first);
        assert_eq!(
            issuer.approvals.settle_owner_loss(&reservation),
            Ok(Vec::new()),
            "nothing was left before the issuer took the session"
        );
        let _settled = reservation.complete();
    }
    let consent = issuer.request(&issuer.check()).await;
    *store.fault.lock() = Some(Fault::AfterWrite);
    assert_eq!(
        issuer
            .approvals
            .decide_consent(&consent, decision(ApprovalDecision::Approve, "lead-1")),
        Err(ApprovalError::StoreCommitUnknown {
            approval_id: consent.clone()
        })
    );
    let durable = row(&store.inner, &consent);
    assert_eq!(durable.status, ApprovalStatus::Approved, "durably approved");
    assert!(
        durable
            .consent
            .as_ref()
            .is_some_and(|consent| consent.consumed_at.is_none() && consent.withdrawal.is_none()),
        "and unspent"
    );
    {
        let state = issuer.approvals.state.read();
        assert!(
            !state.consent.contains_key(&consent),
            "no candidate custody"
        );
        assert!(
            state.issued_consent.contains(&consent),
            "the issuance record stays"
        );
        assert!(
            state.unreconciled.contains_key(&consent),
            "the row is unknown here"
        );
    }

    // Every claim of the issuer's lineage is released; a new grant takes
    // actual exclusion of the session.
    issuer.work = WorkAuthorizationContext::new(
        Arc::new(OwnerWork(Arc::clone(&issuer.owner))),
        OperationExecutionScope::Domain,
    );
    drop(first);
    let second = grant_session_hosting(&capability, &owner, &session).expect("a new claim");
    assert_eq!(
        second.acquisition(),
        HostingAcquisition::AcquiredFresh,
        "no claim of the earlier lineage is left"
    );
    let reservation = reserved(&second);

    store.unreadable.store(true, Ordering::Release);
    for _ in 0..2 {
        assert_eq!(
            issuer.approvals.settle_owner_loss(&reservation),
            Err(ApprovalError::Unreconciled {
                approval_id: consent.clone()
            }),
            "never an empty success while the row is unknown"
        );
    }
    assert_eq!(
        row(&store.inner, &consent),
        durable,
        "an unknown row is never written"
    );

    store.unreadable.store(false, Ordering::Release);
    assert_eq!(
        issuer.approvals.settle_owner_loss(&reservation),
        Ok(vec![consent.clone()]),
        "settled once the ordinary reconciliation knows the row"
    );
    let settled = row(&store.inner, &consent);
    assert_eq!(
        settled.status,
        ApprovalStatus::Approved,
        "the approval history is kept"
    );
    assert_eq!(settled.decision, durable.decision);
    assert_eq!(
        withdrawal_reason(&settled),
        Some(ApprovalConsentWithdrawalReason::Abandoned)
    );
    assert_eq!(
        deliveries_of(&store.inner, &consent),
        vec![
            (
                ApprovalConsentTransition::Approved,
                ApprovalConsentDeliveryState::Owed
            ),
            withdrawn_owed(),
        ],
        "one withdrawal is owed beside the decision's notice"
    );
    assert_eq!(
        issuer.approvals.settle_owner_loss(&reservation),
        Ok(Vec::new()),
        "settling again settles nothing"
    );
    assert_eq!(
        row(&store.inner, &consent),
        settled,
        "and adds no obligation"
    );
    let _settled = reservation.complete();
}

/// Owner A settles its lineage, keeps that lineage's proof as history and
/// releases its claim. Owner B then acquires the same session and issues
/// approved, unspent consent whose custody it keeps alive. A's proof names
/// exactly B's scope, but a proof is not a live reservation, and settling
/// takes a borrow of one: no API accepts the proof. A third approval owner
/// holding A's proof cannot reach a reservation of the session either: A
/// and any other owner are refused the claim while B holds it, B's own
/// lineage owes nothing, and a live reservation of another session settles
/// only that session's scope. B's row stays exactly as B committed it, and
/// B spends its consent once.
#[tokio::test]
async fn an_earlier_owners_proof_cannot_withdraw_a_later_owners_live_consent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let capability = os_lock(dir.path());
    let session = SessionId::new();
    let store = Arc::new(InMemoryApprovalStore::new());

    let owner_a = HostingOwner::mint();
    let claim_a = grant_session_hosting(&capability, &owner_a, &session).expect("A's claim");
    let reservation_a = reserved(&claim_a);
    let approvals_a = ApprovalService::with_store(store.clone()).expect("A's approval owner");
    assert_eq!(
        approvals_a.settle_owner_loss(&reservation_a),
        Ok(Vec::new())
    );
    let proof_a = reservation_a.complete();
    drop(claim_a);

    let owner_b = HostingOwner::mint();
    let claim_b = grant_session_hosting(&capability, &owner_b, &session)
        .expect("B acquires the session A let go");
    assert_eq!(claim_b.acquisition(), HostingAcquisition::AcquiredFresh);
    let b = hosted_world(
        ApprovalService::with_store(store.clone()).expect("B's approval owner"),
        &claim_b,
    );
    {
        let reservation_b = reserved(&claim_b);
        assert_eq!(
            b.approvals.settle_owner_loss(&reservation_b),
            Ok(Vec::new()),
            "A left nothing unsettled"
        );
        let _settled = reservation_b.complete();
    }
    let consent = b.request(&b.check()).await;
    b.approvals
        .decide_consent(&consent, decision(ApprovalDecision::Approve, "lead-1"))
        .expect("approved");
    let raw = row(&store, &consent);
    assert_eq!(
        raw.consent
            .as_ref()
            .and_then(|consent| consent.scope.clone()),
        Some(ApprovalConsentScope {
            domain: proof_a.domain().clone(),
            session_id: proof_a.session_id().clone(),
        }),
        "A's old proof names exactly B's scope"
    );

    let third = ApprovalService::with_store(store.clone()).expect("a third approval owner");
    assert!(
        matches!(
            grant_session_hosting(&capability, &owner_a, &session),
            Err(HostingRefused::ServedElsewhere(_))
        ),
        "A cannot take the session's claim back while B holds it"
    );
    assert!(
        matches!(
            grant_session_hosting(&capability, &HostingOwner::mint(), &session),
            Err(HostingRefused::ServedElsewhere(_))
        ),
        "nor can any other owner"
    );
    assert!(
        matches!(claim_b.reserve_owner_loss(), OwnerLossAttachment::Settled),
        "B's own lineage owes nothing"
    );
    let (_other_claim, other, _other_scope) = owner_loss_of(&capability, &SessionId::new());
    assert_eq!(
        third.settle_owner_loss(&other),
        Ok(Vec::new()),
        "a live reservation of another session settles nothing of B's"
    );

    assert_eq!(
        row(&store, &consent),
        raw,
        "B's row is exactly as B committed it"
    );
    assert_eq!(b.spend(&consent), Ok(()), "B spends its consent");
    assert!(b.spend(&consent).is_err(), "once");
}
