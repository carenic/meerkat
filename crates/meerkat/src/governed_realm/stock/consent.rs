//! The stock host's human consent route for R3.
//!
//! A stock composition decides R3 consent through one route over its
//! surface ([`StockConsentSurface`]): the RPC stdio pipe, REST's bearer
//! logins, or another trusted host adapter. The surface presents the
//! candidate, states the current facts of the exact login an approver was
//! authenticated in, and delivers the consent notices. The route adds what
//! the composition owns:
//!
//! - the weakest approver assurance the owner's R3 rule accepts
//!   ([`meerkat_core::StockConsentAssurance`]). It is fixed when the approval
//!   is issued and stated on it, so an approver authenticated more weakly is
//!   never eligible, whatever the surface says;
//! - the account the operation acts with and the mandate it runs under,
//!   as the operation's own owners stated them in the native review
//!   context, so the human sees which account an approval lets act and
//!   under which mandate. A surface never states or hides either;
//! - eligibility evidence that stays current only while the composition's
//!   publication has not moved since the surface's facts were read (the
//!   surface publishes every retirement, replacement or permission change of
//!   a login on it) and while the login's own deadline has not passed. The
//!   approval owner checks it when the decision commits and again before it
//!   publishes the allowance.
//!
//! The route decides who may decide, never what runs: an approved decision
//! is only recorded and notified, and only a fresh explicit attempt of the
//! retained candidate spends it.

use std::sync::Arc;

use meerkat_authorization::publication::{
    LocalAuthorizationPublication, PublishedConsentEligibility,
};
use meerkat_core::approval::consent::{
    AuthenticatedApprover, ConsentEligibility, ConsentIneligible, ConsentNotice,
    ConsentPresentation, ConsentPresentationCandidate, ConsentRouteUnavailable, HumanConsentRoute,
};
use meerkat_core::authorization::{
    AuthorizationOperation, ToolAuthorizationFacts, ToolAuthorizationTarget,
};
use meerkat_core::{ApprovalRecord, StockConsentAssurance};

use super::caller::StockLoginLifetime;
use super::review::StockReviewRules;

/// The label of a stock consent approval naming the weakest approver
/// assurance its R3 rule accepts. The route writes it into the presentation
/// (over any value the surface put there) when the approval is issued, and
/// reads it back from the approval owner's own record, which the owner keeps
/// unchanged through the decision; an approval without it admits no
/// approver. It is outside the reserved `meerkat` label namespace, which no
/// presentation may use.
pub const STOCK_CONSENT_ASSURANCE_LABEL: &str = "stock.consent_assurance";

/// The label of a stock consent approval naming the account the operation
/// acts with (`<subject> at <authority>`), as the operation's own owner
/// stated it in the native review context. The route writes it when the
/// approval is issued, over any value the surface put there, and removes a
/// surface's value when the owner stated that no account applies. It is
/// presentation, never eligibility or permission.
pub const STOCK_ACTION_ACCOUNT_LABEL: &str = "stock.action_account";

/// The label of a stock consent approval stating the current mandate the
/// operation runs under, as the admitted work's owner stated it in the
/// native review context. The route writes it when the approval is issued,
/// over any value the surface put there, and removes a surface's value when
/// the owner stated none. It is presentation, never eligibility or
/// permission.
pub const STOCK_REVIEW_MANDATE_LABEL: &str = "stock.review_mandate";

/// A stock surface's part of R3 human consent: presentation, the current
/// login facts of the approvers it authenticated, and notice delivery.
pub trait StockConsentSurface: Send + Sync {
    /// Presentation facts for the approval of one exact candidate: who
    /// requested it, its risk and the surface metadata shown with it. The
    /// route adds the accepted assurance and the operation's account and
    /// mandate to the metadata. The requester a surface presents is the
    /// person the approval is presented to on its behalf, such as the
    /// operator of a pipe; the work's own requester (the operator, a
    /// caller, or the stock ingress for an outcome it reports) is stated in
    /// the mandate ([`STOCK_REVIEW_MANDATE_LABEL`]).
    ///
    /// # Errors
    /// [`ConsentRouteUnavailable`] when the surface cannot present the
    /// candidate readably; no approval is issued.
    fn present(
        &self,
        candidate: &ConsentPresentationCandidate<'_>,
    ) -> Result<ConsentPresentation, ConsentRouteUnavailable>;

    /// The current facts of the exact login (authentication scope) this
    /// surface authenticated `approver` in: whether that login is current
    /// (not expired, logged out, replaced or retired) and permitted to decide
    /// `approval`, and how long it stays current. `None` refuses. Another
    /// current login of the same person is not this login.
    ///
    /// It is called inside an observation of the composition's publication,
    /// so it reads the surface's own login owner without I/O, waiting or
    /// callbacks. A surface whose login facts can change joins their owner to
    /// that publication ([`super::StockGovernance::authorization_publication`])
    /// and retires, replaces or re-permits a login only inside
    /// [`LocalAuthorizationPublication::begin_owner_change`], so eligibility
    /// observed before the change stops being current. A login past its
    /// deadline is refused whether or not the surface has retired it yet.
    fn approver_login(
        &self,
        approver: &AuthenticatedApprover,
        approval: &ApprovalRecord,
    ) -> Option<StockLoginLifetime>;

    /// Deliver one recorded consent transition, after the approval owner
    /// released its locks. It never executes the retained action.
    fn notice(&self, notice: ConsentNotice);
}

/// The stock composition's human consent route over one surface.
pub(super) struct StockHumanConsentRoute {
    surface: Arc<dyn StockConsentSurface>,
    publication: LocalAuthorizationPublication,
    rules: StockReviewRules,
}

impl StockHumanConsentRoute {
    /// The route of `surface`, observing its login facts on the
    /// composition's `publication`, for the owner's review `rules`.
    pub(super) fn new(
        surface: Arc<dyn StockConsentSurface>,
        publication: LocalAuthorizationPublication,
        rules: StockReviewRules,
    ) -> Self {
        Self {
            surface,
            publication,
            rules,
        }
    }

    /// The weakest assurance the candidate's consent accepts: the strongest
    /// that the owner's R3 rules naming its operation state, and person
    /// authentication when none names it (R3 another owner required).
    fn accepted_assurance(
        &self,
        candidate: &ConsentPresentationCandidate<'_>,
    ) -> Result<StockConsentAssurance, ConsentRouteUnavailable> {
        let target = match &candidate.facts().operation {
            AuthorizationOperation::Tool(ToolAuthorizationFacts {
                target: ToolAuthorizationTarget::Dispatcher(plan),
                ..
            }) => meerkat_authorization::grant_policy::single_tool_operation_target(plan)
                .map_err(|_| ConsentRouteUnavailable)?,
            _ => None,
        };
        Ok(self
            .rules
            .consent_assurance_for(target)
            .unwrap_or(StockConsentAssurance::PersonAuthenticated))
    }
}

impl HumanConsentRoute for StockHumanConsentRoute {
    fn present(
        &self,
        candidate: &ConsentPresentationCandidate<'_>,
    ) -> Result<ConsentPresentation, ConsentRouteUnavailable> {
        let accepted = self.accepted_assurance(candidate)?;
        let presentation = state_accepted_assurance(self.surface.present(candidate)?, accepted);
        let context = candidate.operation_context();
        let presentation = state_action_account(
            presentation,
            context.and_then(|context| context.action_account()),
        );
        Ok(state_review_mandate(
            presentation,
            context.and_then(|context| context.mandate()),
        ))
    }

    fn eligibility(
        &self,
        approver: &AuthenticatedApprover,
        approval: &ApprovalRecord,
    ) -> Result<Arc<dyn ConsentEligibility>, ConsentIneligible> {
        let accepted = approval
            .metadata
            .labels
            .get(STOCK_CONSENT_ASSURANCE_LABEL)
            .and_then(|name| StockConsentAssurance::from_name(name))
            .ok_or(ConsentIneligible)?;
        if !accepted.accepts(approver.assurance()) {
            return Err(ConsentIneligible);
        }
        let mut login = None;
        let published = PublishedConsentEligibility::observe(&self.publication, || {
            login = self.surface.approver_login(approver, approval);
            login.is_some_and(StockLoginLifetime::is_current_now)
        })?;
        let login = login.ok_or(ConsentIneligible)?;
        Ok(Arc::new(LoginBoundEligibility { published, login }))
    }

    fn notice(&self, notice: ConsentNotice) {
        self.surface.notice(notice);
    }
}

/// `presentation` stating `accepted` as its approval's accepted assurance,
/// over whatever the surface put under the label: the issued approval
/// carries the rule's assurance, never a weaker one a surface stated.
fn state_accepted_assurance(
    mut presentation: ConsentPresentation,
    accepted: StockConsentAssurance,
) -> ConsentPresentation {
    presentation.metadata.labels.insert(
        STOCK_CONSENT_ASSURANCE_LABEL.to_owned(),
        accepted.as_str().to_owned(),
    );
    presentation
}

/// `presentation` stating `account`, the operation owner's own statement of
/// the account the operation acts with, over whatever the surface put under
/// the label; without one (the owner stated none applies) the label is
/// absent.
fn state_action_account(
    presentation: ConsentPresentation,
    account: Option<&str>,
) -> ConsentPresentation {
    state_owner_statement(presentation, STOCK_ACTION_ACCOUNT_LABEL, account)
}

/// `presentation` stating `mandate`, the admitted work owner's own statement
/// of the mandate the operation runs under, over whatever the surface put
/// under the label; without one the label is absent.
fn state_review_mandate(
    presentation: ConsentPresentation,
    mandate: Option<&str>,
) -> ConsentPresentation {
    state_owner_statement(presentation, STOCK_REVIEW_MANDATE_LABEL, mandate)
}

/// `presentation` with `label` stating exactly the owner's `statement`, or
/// without the label when the owner stated none.
fn state_owner_statement(
    mut presentation: ConsentPresentation,
    label: &str,
    statement: Option<&str>,
) -> ConsentPresentation {
    match statement {
        Some(statement) => {
            presentation
                .metadata
                .labels
                .insert(label.to_owned(), statement.to_owned());
        }
        None => {
            presentation.metadata.labels.remove(label);
        }
    }
    presentation
}

/// Eligibility observed on the composition's publication, for a login with
/// its own deadline: current while neither has moved on.
struct LoginBoundEligibility {
    published: Arc<dyn ConsentEligibility>,
    login: StockLoginLifetime,
}

impl ConsentEligibility for LoginBoundEligibility {
    fn check_current(&self) -> Result<(), ConsentIneligible> {
        self.published.check_current()?;
        if self.login.is_current_now() {
            Ok(())
        } else {
            Err(ConsentIneligible)
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use meerkat_core::{
        ApprovalActionKind, ApprovalAssurance, ApprovalDecision, ApprovalOwnerRef,
        ApprovalPrincipalId, ApprovalProposedAction, ApprovalRequest, ApprovalResourceId,
        ApprovalResourceKind, ApprovalResourceRef, ApprovalRisk, ApprovalService, PrincipalKind,
        PrincipalRef, SurfaceMetadata, ToolCallId, TrustDomainId,
    };

    use super::*;

    /// A surface whose login owner vouches for one fixed login.
    struct Surface {
        login: Mutex<Option<StockLoginLifetime>>,
        asked: AtomicUsize,
    }

    impl Surface {
        fn vouching(login: Option<StockLoginLifetime>) -> Arc<Self> {
            Arc::new(Self {
                login: Mutex::new(login),
                asked: AtomicUsize::new(0),
            })
        }

        fn asked(&self) -> usize {
            self.asked.load(Ordering::SeqCst)
        }
    }

    impl StockConsentSurface for Surface {
        fn present(
            &self,
            _candidate: &ConsentPresentationCandidate<'_>,
        ) -> Result<ConsentPresentation, ConsentRouteUnavailable> {
            Err(ConsentRouteUnavailable)
        }

        fn approver_login(
            &self,
            _approver: &AuthenticatedApprover,
            _approval: &ApprovalRecord,
        ) -> Option<StockLoginLifetime> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            *self.login.lock().unwrap()
        }

        fn notice(&self, _notice: ConsentNotice) {}
    }

    fn route(
        surface: &Arc<Surface>,
        publication: &LocalAuthorizationPublication,
    ) -> StockHumanConsentRoute {
        StockHumanConsentRoute::new(
            Arc::clone(surface) as _,
            publication.clone(),
            StockReviewRules::default(),
        )
    }

    /// A consent approval whose label states `accepted`, as the route
    /// presented it.
    fn approval(accepted: Option<&str>) -> ApprovalRecord {
        let mut metadata = SurfaceMetadata::default();
        if let Some(accepted) = accepted {
            metadata.labels.insert(
                STOCK_CONSENT_ASSURANCE_LABEL.to_owned(),
                accepted.to_owned(),
            );
        }
        issue(metadata)
    }

    /// The approval the owner issues for a presentation's `metadata`.
    fn issue(metadata: SurfaceMetadata) -> ApprovalRecord {
        ApprovalService::new()
            .request(ApprovalRequest {
                requester: ApprovalPrincipalId::new("human:operator").expect("requester"),
                owner: ApprovalOwnerRef::ToolCall {
                    tool_call_id: ToolCallId::new("call-1"),
                },
                resource: ApprovalResourceRef {
                    kind: ApprovalResourceKind::ToolCall,
                    id: ApprovalResourceId::new("mail_send"),
                },
                proposed_action: ApprovalProposedAction {
                    kind: ApprovalActionKind::ToolCall,
                    summary: "mail_send".to_owned(),
                    body: None,
                },
                risk: ApprovalRisk::High,
                request_body: None,
                allowed_decisions: [ApprovalDecision::Approve, ApprovalDecision::Deny]
                    .into_iter()
                    .collect(),
                expires_at: None,
                metadata,
                request_provenance: None,
            })
            .expect("approval")
    }

    fn approver(assurance: ApprovalAssurance) -> AuthenticatedApprover {
        AuthenticatedApprover::new(
            PrincipalRef::in_domain(
                PrincipalKind::Human,
                "alice",
                TrustDomainId::new("console-idp").expect("domain"),
            )
            .expect("qualified"),
            assurance,
        )
        .expect("approver")
    }

    #[test]
    fn only_an_approver_the_rule_accepts_is_asked_about() {
        let publication = LocalAuthorizationPublication::new();
        let surface = Surface::vouching(Some(StockLoginLifetime::UntilRetired));
        let route = route(&surface, &publication);
        let person = approval(Some("person_authenticated"));
        // Weaker than the rule accepts, a delegated service, or an approval
        // that states no accepted assurance: refused before the surface is
        // asked about any login.
        for (approver, approval) in [
            (approver(ApprovalAssurance::DeviceTrusted), &person),
            (approver(ApprovalAssurance::ServiceCredential), &person),
            (
                approver(ApprovalAssurance::PersonAuthenticated),
                &approval(None),
            ),
            (
                approver(ApprovalAssurance::PersonAuthenticated),
                &approval(Some("anyone")),
            ),
        ] {
            assert!(route.eligibility(&approver, approval).is_err());
        }
        assert_eq!(surface.asked(), 0);
        // An accepted assurance is asked about and eligible while its login
        // is current; a person also satisfies a device-trusted rule.
        let evidence = route
            .eligibility(&approver(ApprovalAssurance::PersonAuthenticated), &person)
            .expect("eligible");
        assert_eq!(evidence.check_current(), Ok(()));
        let device = approval(Some("device_trusted"));
        for assurance in [
            ApprovalAssurance::DeviceTrusted,
            ApprovalAssurance::PersonAuthenticated,
        ] {
            assert!(route.eligibility(&approver(assurance), &device).is_ok());
        }
        assert!(
            route
                .eligibility(&approver(ApprovalAssurance::ServiceCredential), &device)
                .is_err()
        );
        assert_eq!(surface.asked(), 3);
    }

    #[test]
    fn a_login_the_surface_does_not_vouch_for_or_past_its_deadline_is_not_eligible() {
        let publication = LocalAuthorizationPublication::new();
        let approval = approval(Some("person_authenticated"));
        let person = approver(ApprovalAssurance::PersonAuthenticated);
        for login in [None, Some(StockLoginLifetime::ExpiresAt { unix_ms: 1 })] {
            let surface = Surface::vouching(login);
            assert!(
                route(&surface, &publication)
                    .eligibility(&person, &approval)
                    .is_err(),
                "{login:?}"
            );
            assert_eq!(surface.asked(), 1);
        }
    }

    #[test]
    fn eligibility_stops_being_current_when_the_login_owner_publishes_a_change() {
        let publication = LocalAuthorizationPublication::new();
        let surface = Surface::vouching(Some(StockLoginLifetime::UntilRetired));
        let evidence = route(&surface, &publication)
            .eligibility(
                &approver(ApprovalAssurance::PersonAuthenticated),
                &approval(Some("person_authenticated")),
            )
            .expect("eligible");
        assert_eq!(evidence.check_current(), Ok(()));
        // The surface retires the login inside an owner change on the
        // composition's publication.
        drop(publication.begin_owner_change().expect("owner change"));
        assert_eq!(evidence.check_current(), Err(ConsentIneligible));
    }

    /// A surface cannot weaken the rule: whatever it states under the label,
    /// the issued approval carries the rule's assurance, and an approver the
    /// rule does not accept is refused on it.
    #[test]
    fn the_issued_approval_carries_the_rules_assurance_over_the_surfaces() {
        let mut metadata = SurfaceMetadata::default();
        metadata.labels.insert(
            STOCK_CONSENT_ASSURANCE_LABEL.to_owned(),
            "device_trusted".to_owned(),
        );
        let presented = state_accepted_assurance(
            ConsentPresentation {
                requester: ApprovalPrincipalId::new("human:operator").expect("requester"),
                risk: ApprovalRisk::High,
                metadata,
            },
            StockConsentAssurance::PersonAuthenticated,
        );
        assert_eq!(
            presented
                .metadata
                .labels
                .get(STOCK_CONSENT_ASSURANCE_LABEL)
                .map(String::as_str),
            Some("person_authenticated")
        );
        let issued = issue(presented.metadata);
        let publication = LocalAuthorizationPublication::new();
        let surface = Surface::vouching(Some(StockLoginLifetime::UntilRetired));
        assert!(
            route(&surface, &publication)
                .eligibility(&approver(ApprovalAssurance::DeviceTrusted), &issued)
                .is_err()
        );
    }

    /// The issued approval names the account the owner stated, over any
    /// surface value; when the owner stated none applies, a surface's value
    /// is removed rather than shown.
    #[test]
    fn the_issued_approval_names_the_owners_account_over_the_surfaces() {
        let mut metadata = SurfaceMetadata::default();
        metadata.labels.insert(
            STOCK_ACTION_ACCOUNT_LABEL.to_owned(),
            "someone-else at https://elsewhere.example".to_owned(),
        );
        let presentation = || ConsentPresentation {
            requester: ApprovalPrincipalId::new("human:operator").expect("requester"),
            risk: ApprovalRisk::High,
            metadata: metadata.clone(),
        };
        let stated = state_action_account(presentation(), Some("alice at https://mail.example"));
        assert_eq!(
            stated
                .metadata
                .labels
                .get(STOCK_ACTION_ACCOUNT_LABEL)
                .map(String::as_str),
            Some("alice at https://mail.example")
        );
        let issued = issue(stated.metadata);
        assert_eq!(
            issued
                .metadata
                .labels
                .get(STOCK_ACTION_ACCOUNT_LABEL)
                .map(String::as_str),
            Some("alice at https://mail.example"),
            "the owner keeps the label on its record"
        );
        let none = state_action_account(presentation(), None);
        assert!(
            !none
                .metadata
                .labels
                .contains_key(STOCK_ACTION_ACCOUNT_LABEL)
        );
    }

    /// The issued approval states the mandate the owner stated, over any
    /// surface value; when the owner stated none, a surface's value is
    /// removed rather than shown.
    #[test]
    fn the_issued_approval_states_the_owners_mandate_over_the_surfaces() {
        let mut metadata = SurfaceMetadata::default();
        metadata.labels.insert(
            STOCK_REVIEW_MANDATE_LABEL.to_owned(),
            "a mandate the surface made up".to_owned(),
        );
        let presentation = || ConsentPresentation {
            requester: ApprovalPrincipalId::new("human:operator").expect("requester"),
            risk: ApprovalRisk::High,
            metadata: metadata.clone(),
        };
        let stated = state_review_mandate(presentation(), Some("the owner's mandate"));
        let issued = issue(stated.metadata);
        assert_eq!(
            issued
                .metadata
                .labels
                .get(STOCK_REVIEW_MANDATE_LABEL)
                .map(String::as_str),
            Some("the owner's mandate"),
            "the owner keeps the label on its record"
        );
        let none = state_review_mandate(presentation(), None);
        assert!(
            !none
                .metadata
                .labels
                .contains_key(STOCK_REVIEW_MANDATE_LABEL)
        );
    }

    #[test]
    fn eligibility_stops_being_current_at_the_logins_deadline() {
        let publication = LocalAuthorizationPublication::new();
        let current =
            PublishedConsentEligibility::observe(&publication, || true).expect("observed");
        let expired = LoginBoundEligibility {
            published: Arc::clone(&current),
            login: StockLoginLifetime::ExpiresAt { unix_ms: 1 },
        };
        assert_eq!(expired.check_current(), Err(ConsentIneligible));
        let live = LoginBoundEligibility {
            published: current,
            login: StockLoginLifetime::ExpiresAt { unix_ms: u64::MAX },
        };
        assert_eq!(live.check_current(), Ok(()));
    }
}
