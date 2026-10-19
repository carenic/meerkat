//! The later effect of helper setup: the helper's child controller grant is
//! issued (or recovered) only while every originating source and the
//! independent parent grant are current for the actual setup operation.
//! The grant owner's records, revision and resolved lineages are the oracle.

use super::*;
use crate::governed_realm::helper_admission::GovernedHelperAdmissionFence;
use crate::governed_realm::helper_child_issuance::{
    GovernedHelperChildIssuance, HelperChildIssuanceRefusal,
};
use meerkat_authorization::grants::{ChildGrantIntent, ChildGrantOutcome};
use meerkat_authorization::work::RetainedOriginal;
use meerkat_authorization_contracts::constraints::{ExactRestriction, LifetimeRestriction};
use meerkat_authorization_contracts::grant_mutation::{
    ControllerCustodyRefusal, ControllerGrantMutationCustody,
};
use meerkat_workgraph::{
    CreateWorkItemRequest, MemoryWorkGraphStore, WorkAdmissionKey, WorkAdmissionOutcome,
    WorkGraphService, WorkNamespace,
};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

fn retained_original(name: &str) -> RetainedOriginal {
    RetainedOriginal {
        namespace: namespace(name),
        original: work(name),
    }
}

/// The native clock, set explicitly; never a sleep. The wall and monotonic
/// readings move independently; `jump_at` moves the wall clock to `jump_to`
/// on that exact read (counted from the clock's creation), so an expiry can
/// fall between a check and the final boundary read.
struct Settable {
    wall: AtomicU64,
    base: meerkat_core::time_compat::Instant,
    monotonic_ms: AtomicU64,
    reads: AtomicUsize,
    jump_at: AtomicUsize,
    jump_to: AtomicU64,
}
impl Settable {
    fn new(wall: u64) -> Self {
        Self {
            wall: AtomicU64::new(wall),
            base: meerkat_core::time_compat::Instant::now(),
            monotonic_ms: AtomicU64::new(0),
            reads: AtomicUsize::new(0),
            jump_at: AtomicUsize::new(usize::MAX),
            jump_to: AtomicU64::new(0),
        }
    }
    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
    /// Move the wall clock to `to` on the read `nth` reads from now.
    fn jump_on(&self, nth: usize, to: u64) {
        self.jump_to.store(to, Ordering::SeqCst);
        self.jump_at.store(self.reads() + nth, Ordering::SeqCst);
    }
}
impl LocalAuthorizationClock for Settable {
    fn now(&self) -> Result<LocalAuthorizationTime, meerkat_authorization::clock::LocalClockError> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if read == self.jump_at.load(Ordering::SeqCst) {
            self.wall
                .store(self.jump_to.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        Ok(LocalAuthorizationTime {
            unix_ms: self.wall.load(Ordering::SeqCst),
            monotonic: self.base
                + std::time::Duration::from_millis(self.monotonic_ms.load(Ordering::SeqCst)),
        })
    }
}

struct NoController;
impl ControllerGrantMutationCustody for NoController {
    fn with_unreferenced_controller_grant<T, E>(
        &mut self,
        _: &GrantLineageRef,
        mutate: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, ControllerCustodyRefusal> {
        Ok(mutate())
    }
}

/// One native composition: the source ledger and grant owner on one
/// publication, the setup operation prepared in the child's work context,
/// the independent parent C and an unrelated frontend controller lineage.
struct Fixture {
    ledger: Arc<StockOriginalLedger>,
    publication: LocalAuthorizationPublication,
    grants: Arc<LocalGrantAuthority>,
    clock: Arc<Settable>,
    policy: Arc<GrantBackedWorkPolicy>,
    child: InputAuthorityAssociation,
    context: meerkat_core::WorkAuthorizationContext,
    setup: PreparedOperationCheck,
    root: PrincipalRef,
    parent: GrantLineageRef,
    frontend: GrantLineageRef,
}

impl Fixture {
    fn new() -> Self {
        Self::new_with(Arc::new(Records))
    }

    /// The fixture with `operation_owner` as the records operation owner.
    fn new_with(operation_owner: Arc<dyn OperationPolicyOwner>) -> Self {
        let (ledger, publication) = ledger();
        let root = principal("grant-owner");
        let clock = Arc::new(Settable::new(100));
        let grants = Arc::new(
            LocalGrantAuthority::new(
                LocalGrantConfiguration {
                    root: root.clone(),
                    namespace: id("controller"),
                    generation: 1,
                },
                publication.clone(),
                Arc::clone(&clock) as Arc<dyn LocalAuthorizationClock>,
            )
            .expect("grant owner on the ledger's publication"),
        );
        let configuration = ScopedGrantComposition {
            grants: Arc::clone(&grants),
            ingress_domain: namespace("x").ingress,
            originals: Arc::clone(&ledger) as Arc<dyn AuthenticatedOriginalSource>,
            invocation: ScopedInvocation::from_policy(Arc::new(Policy)),
            operation_owner,
            retained_resume: None,
        }
        .into_configuration();
        let policy = Arc::new(GrantBackedWorkPolicy::new(
            Arc::clone(&grants),
            configuration.invocation_owner,
            configuration.operation_owner,
        ));
        let controller = |name: &str| {
            grants
                .issue_root(
                    &root,
                    id(name),
                    principal("controller"),
                    None,
                    ExecutionRestrictions::unrestricted(),
                )
                .expect("controller grant")
        };
        let parent = controller("parent-c");
        let frontend = controller("frontend");
        ledger
            .record_candidate(&namespace("a"), &principal("requester"), original("a"))
            .expect("recorded")
            .admit()
            .expect("admitted");
        let child = association_of(&original("a"), &namespace("a"));
        let context = policy
            .work_context(Arc::from([child.clone()]), scope())
            .expect("work context");
        let setup = PreparedOperationCheck::prepare(
            context.clone(),
            PreparedAuthorizationBinding::new(OperationAuthorizationFacts {
                operation_id: meerkat_core::OperationId::new(),
                execution_scope: context.execution_scope().clone(),
                run_id: Some(meerkat_core::RunId::new()),
                context_revision: None,
                operation: AuthorizationOperation::Source(SourceAuthorizationFacts {
                    target: SourceAuthorizationTarget::External(OwnerQualifiedTarget {
                        authority: principal("owner"),
                        namespace: "records".into(),
                        id: "record".into(),
                    }),
                    usage: SourceAuthorizationUse::Read,
                }),
            }),
        )
        .expect("the setup operation is prepared");
        Self {
            ledger,
            publication,
            grants,
            clock,
            policy,
            child,
            context,
            setup,
            root,
            parent,
            frontend,
        }
    }

    /// Prepare the setup operation again, in a work context of `batch`.
    fn prepare_in(&mut self, batch: &[InputAuthorityAssociation]) {
        let context = self
            .policy
            .work_context(Arc::from(batch), scope())
            .expect("work context");
        self.setup = PreparedOperationCheck::prepare(
            context.clone(),
            PreparedAuthorizationBinding::new(OperationAuthorizationFacts {
                operation_id: meerkat_core::OperationId::new(),
                execution_scope: context.execution_scope().clone(),
                run_id: Some(meerkat_core::RunId::new()),
                context_revision: None,
                operation: AuthorizationOperation::Source(SourceAuthorizationFacts {
                    target: SourceAuthorizationTarget::External(OwnerQualifiedTarget {
                        authority: principal("owner"),
                        namespace: "records".into(),
                        id: "record".into(),
                    }),
                    usage: SourceAuthorizationUse::Read,
                }),
            }),
        )
        .expect("prepared");
    }

    /// `association` with its admitted ceiling ending at `expires_at_ms`.
    fn bounded(
        association: &InputAuthorityAssociation,
        expires_at_ms: u64,
    ) -> InputAuthorityAssociation {
        let mut claims = association.candidate().clone();
        claims.admitted_ceiling = ExecutionRestrictions {
            lifetime: LifetimeRestriction::window(0, expires_at_ms),
            ..ExecutionRestrictions::unrestricted()
        };
        InputAuthorityAssociation::new(claims).expect("well-formed claims")
    }

    /// A retained originating source `name` whose basis is a real grant
    /// with `restrictions`.
    fn source(&self, name: &str, restrictions: ExecutionRestrictions) {
        let lineage = self
            .grants
            .issue_root(
                &self.root,
                id(name),
                principal("executor"),
                None,
                restrictions,
            )
            .expect("source grant");
        let mut original = original(name);
        original.authority_basis = WorkAuthorityBasis::GrantLineage {
            lineage: vec![lineage],
        };
        self.ledger
            .record_candidate(&namespace(name), &principal("requester"), original)
            .expect("recorded")
            .admit()
            .expect("admitted");
    }

    fn intent(&self, name: &str) -> ChildGrantIntent {
        ChildGrantIntent {
            issuer: principal("controller"),
            parent_lineage: vec![self.parent.clone()],
            id: id(name),
            grantee: principal("helper"),
            represented_subject: None,
            requested: ExecutionRestrictions::unrestricted(),
        }
    }

    fn issuance(&self, name: &str, retained: &[&str]) -> GovernedHelperChildIssuance {
        GovernedHelperChildIssuance::new(
            Arc::clone(&self.grants),
            self.ledger.clone(),
            self.child.clone(),
            retained
                .iter()
                .map(|name| retained_original(name))
                .collect::<Vec<_>>()
                .into(),
            self.setup.clone(),
            Arc::clone(&self.policy),
            Arc::clone(&self.clock) as Arc<dyn LocalAuthorizationClock>,
            self.intent(name),
        )
    }

    fn issued(&self, name: &str) -> Option<GrantLineageRef> {
        self.grants.issued_reference(&id(name)).expect("lookup")
    }

    /// The publication is unchanged across `act`: no owner change landed.
    fn unchanged<T>(&self, act: impl FnOnce() -> T) -> T {
        let ((), stamp) = self.publication.observe(|| ()).expect("observed");
        let value = act();
        assert!(stamp.check_current().is_ok(), "no owner change published");
        value
    }

    fn resolves(&self, lineage: &[GrantLineageRef], executor: &str) -> bool {
        self.grants
            .resolve_controller_lineage(lineage, &principal(executor), None)
            .is_ok()
    }
}

/// Issued once with current sources, a permitted setup operation and a
/// current parent: one child record with the intended parent, issuer,
/// grantee and bounds whose controller lineage resolves for the helper.
/// The same intent recovers it without another append. After a WorkGraph
/// item is committed, withdrawing a source refuses a fresh issuance (no
/// record) and refuses usable recovery of the landed child, whose
/// historical reference stays; the committed item stays too.
#[tokio::test]
async fn a_helper_child_is_issued_only_while_its_sources_are_current() {
    let fixture = Fixture::new();
    fixture.source("p", ExecutionRestrictions::unrestricted());

    let issued = fixture
        .issuance("helper-child", &["p"])
        .issue_or_recover()
        .expect("issued");
    let ChildGrantOutcome::Issued { lineage } = issued.clone() else {
        panic!("first attempt issues: {issued:?}")
    };
    assert_eq!(&lineage[..1], std::slice::from_ref(&fixture.parent));
    assert_eq!(fixture.issued("helper-child").as_ref(), lineage.last());
    assert!(fixture.resolves(&lineage, "helper"), "the helper's lineage");
    assert!(
        !fixture.resolves(&lineage, "controller"),
        "the child is the helper's, not its issuer's"
    );
    let recovered = fixture.unchanged(|| {
        fixture
            .issuance("helper-child", &["p"])
            .issue_or_recover()
            .expect("recovered")
    });
    assert_eq!(
        recovered,
        ChildGrantOutcome::Recovered {
            lineage: lineage.clone()
        }
    );

    // A WorkGraph item is committed under the fence first.
    let service = WorkGraphService::new(Arc::new(MemoryWorkGraphStore::new()));
    let request = || CreateWorkItemRequest {
        title: "setup helper".to_string(),
        ..Default::default()
    };
    let key = WorkAdmissionKey::new("helper-2").expect("key");
    let fence = GovernedHelperAdmissionFence::new(
        Arc::clone(&fixture.grants),
        fixture.ledger.clone(),
        fixture.child.clone(),
        Arc::from([retained_original("p")]),
        fixture.setup.clone(),
        Arc::clone(&fixture.policy),
        Arc::clone(&fixture.clock) as Arc<dyn LocalAuthorizationClock>,
        "default",
        WorkNamespace::default(),
        service
            .admission_identity(key.clone(), request())
            .expect("identity"),
    );
    assert!(matches!(
        service
            .create_idempotent_with_fence(key, request(), fence)
            .await,
        Ok(WorkAdmissionOutcome::Created(_))
    ));
    fixture
        .ledger
        .cancel_operations(&namespace("p"), &work("p"), evidence("withdrawn"))
        .expect("the source is withdrawn");

    let refused = fixture.unchanged(|| {
        fixture
            .issuance("helper-child-2", &["p"])
            .issue_or_recover()
    });
    assert_eq!(refused, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(fixture.issued("helper-child-2"), None, "nothing appended");
    let historical =
        fixture.unchanged(|| fixture.issuance("helper-child", &["p"]).issue_or_recover());
    assert_eq!(
        historical,
        Err(HelperChildIssuanceRefusal::Denied),
        "a landed child is not usable once its source is withdrawn"
    );
    assert_eq!(
        fixture.issued("helper-child").as_ref(),
        lineage.last(),
        "its historical reference is kept, not deleted or reissued"
    );
    assert!(fixture.resolves(std::slice::from_ref(&fixture.parent), "controller"));
    assert_eq!(
        service
            .list(meerkat_workgraph::WorkItemFilter {
                include_terminal: true,
                ..Default::default()
            })
            .await
            .expect("list")
            .len(),
        1,
        "the committed WorkGraph item stays"
    );
}

/// Every originating source stays a conjunct: a source whose grant excludes
/// the operation refuses although the other allows it; a source whose
/// finite grant has expired on the native clock refuses although it allowed
/// the operation before. The independent parent is current or nothing is
/// issued: once it is revoked, no other controller lineage stands in.
#[tokio::test]
async fn every_originating_source_and_the_parent_are_conjuncts() {
    let fixture = Fixture::new();
    fixture.source("p", ExecutionRestrictions::unrestricted());
    fixture.source(
        "r",
        ExecutionRestrictions {
            actions: ExactRestriction::exact([ActionRef {
                feature: "stock-test".into(),
                action: "write".into(),
            }]),
            ..ExecutionRestrictions::unrestricted()
        },
    );
    fixture.source(
        "b",
        ExecutionRestrictions {
            lifetime: LifetimeRestriction::window(0, 500),
            ..ExecutionRestrictions::unrestricted()
        },
    );

    let restricted = fixture.unchanged(|| {
        fixture
            .issuance("with-restrictive", &["p", "r"])
            .issue_or_recover()
    });
    assert_eq!(restricted, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(fixture.issued("with-restrictive"), None);
    assert!(
        matches!(
            fixture.issuance("anchor-only", &["p"]).issue_or_recover(),
            Ok(ChildGrantOutcome::Issued { .. })
        ),
        "the permissive source alone permits it"
    );

    assert!(matches!(
        fixture
            .issuance("before-expiry", &["p", "b"])
            .issue_or_recover(),
        Ok(ChildGrantOutcome::Issued { .. })
    ));
    fixture.clock.wall.store(500, Ordering::SeqCst);
    let expired = fixture.unchanged(|| {
        fixture
            .issuance("after-expiry", &["p", "b"])
            .issue_or_recover()
    });
    assert_eq!(expired, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(fixture.issued("after-expiry"), None);
    assert!(fixture.resolves(std::slice::from_ref(&fixture.parent), "controller"));

    fixture
        .grants
        .revoke(&fixture.root, &fixture.parent, &mut NoController)
        .expect("the independent parent is revoked");
    let orphaned =
        fixture.unchanged(|| fixture.issuance("after-revoke", &["p"]).issue_or_recover());
    assert_eq!(orphaned, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(fixture.issued("after-revoke"), None);
    assert!(
        fixture.resolves(std::slice::from_ref(&fixture.frontend), "controller"),
        "the frontend controller lineage is current and is not a fallback"
    );
}

/// The window carried to the final append and recovery checks is the
/// exact native decision's: a batch member's admitted ceiling bounds it
/// although the source, the policy owner and every grant are unbounded; an
/// expiry that falls after the check, on the final boundary read, refuses
/// both a fresh issuance and a usable recovery; and the decision's own
/// monotonic cutoff is kept, not rebuilt from a later read.
#[tokio::test]
async fn the_exact_decision_window_bounds_the_final_boundary() {
    let mut fixture = Fixture::new();
    fixture.source("p", ExecutionRestrictions::unrestricted());
    // Native batch A+B: B is admitted with a ceiling ending at 500.
    ledger_admit(&fixture, "b");
    let b = Fixture::bounded(&association_of(&original("b"), &namespace("b")), 500);
    let a = fixture.child.clone();
    fixture.prepare_in(&[a, b]);

    let validity = fixture
        .issuance("probe", &["p"])
        .originating()
        .expect("current");
    assert_eq!(
        (validity.not_before_ms, validity.expires_at_ms),
        (0, 500),
        "B's admitted ceiling, although A, the policy and the grants are unbounded"
    );
    assert_eq!(
        validity.deadline,
        fixture.clock.base + std::time::Duration::from_millis(400),
        "the decision's own monotonic cutoff"
    );

    // A wall rollback with monotonic time passing: the cutoff stays the
    // decision's (base + 400), not a later read's.
    fixture.clock.monotonic_ms.store(399, Ordering::SeqCst);
    let later = fixture
        .issuance("probe", &["p"])
        .originating()
        .expect("still current");
    assert_eq!(later.deadline, validity.deadline);

    // Count the reads of a fresh issuance and of a recovery in the state the
    // later attempts run in (each issuance publishes, so every later attempt
    // re-prepares the setup operation): the last read is the final boundary.
    assert!(matches!(
        fixture.issuance("issued", &["p"]).issue_or_recover(),
        Ok(ChildGrantOutcome::Issued { .. })
    ));
    let before = fixture.clock.reads();
    assert!(matches!(
        fixture.issuance("issued-again", &["p"]).issue_or_recover(),
        Ok(ChildGrantOutcome::Issued { .. })
    ));
    let issue_reads = fixture.clock.reads() - before;
    let before = fixture.clock.reads();
    assert!(matches!(
        fixture.issuance("issued", &["p"]).issue_or_recover(),
        Ok(ChildGrantOutcome::Recovered { .. })
    ));
    let recovery_reads = fixture.clock.reads() - before;

    // Expiry on the final boundary read of a fresh issuance.
    let before = fixture.clock.reads();
    fixture.clock.jump_on(issue_reads, 500);
    let late = fixture.unchanged(|| fixture.issuance("late", &["p"]).issue_or_recover());
    assert_eq!(late, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(
        fixture.clock.reads() - before,
        issue_reads,
        "refused at the final boundary read, after every earlier check passed"
    );
    assert_eq!(fixture.issued("late"), None, "nothing appended");

    // Expiry on the final boundary read of a recovery.
    fixture.clock.wall.store(100, Ordering::SeqCst);
    let before = fixture.clock.reads();
    fixture.clock.jump_on(recovery_reads, 500);
    assert_eq!(
        fixture.issuance("issued", &["p"]).issue_or_recover(),
        Err(HelperChildIssuanceRefusal::Denied),
        "a landed child is not recovered as usable past the window"
    );
    assert_eq!(fixture.clock.reads() - before, recovery_reads);
    assert!(fixture.issued("issued").is_some(), "its record is kept");
}

/// A further originating grant's own lower bound stays in the window: with
/// the prepared decision usable from 0 but a retained original's grant only
/// from 100, a final boundary read at 50 refuses a fresh issuance (no
/// record) and a usable recovery (record kept).
#[tokio::test]
async fn a_further_grants_not_before_bounds_the_final_boundary() {
    let fixture = Fixture::new();
    fixture.source(
        "w",
        ExecutionRestrictions {
            lifetime: LifetimeRestriction::window(100, 1000),
            ..ExecutionRestrictions::unrestricted()
        },
    );
    fixture.clock.wall.store(200, Ordering::SeqCst);
    let validity = fixture
        .issuance("probe", &["w"])
        .originating()
        .expect("current at 200");
    assert_eq!(
        (validity.not_before_ms, validity.expires_at_ms),
        (100, 1000)
    );

    assert!(matches!(
        fixture.issuance("issued", &["w"]).issue_or_recover(),
        Ok(ChildGrantOutcome::Issued { .. })
    ));
    let before = fixture.clock.reads();
    assert!(matches!(
        fixture.issuance("issued-again", &["w"]).issue_or_recover(),
        Ok(ChildGrantOutcome::Issued { .. })
    ));
    let issue_reads = fixture.clock.reads() - before;
    let before = fixture.clock.reads();
    assert!(matches!(
        fixture.issuance("issued", &["w"]).issue_or_recover(),
        Ok(ChildGrantOutcome::Recovered { .. })
    ));
    let recovery_reads = fixture.clock.reads() - before;

    let before = fixture.clock.reads();
    fixture.clock.jump_on(issue_reads, 50);
    let early = fixture.unchanged(|| fixture.issuance("early", &["w"]).issue_or_recover());
    assert_eq!(early, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(fixture.clock.reads() - before, issue_reads);
    assert_eq!(fixture.issued("early"), None, "nothing appended");

    fixture.clock.wall.store(200, Ordering::SeqCst);
    let before = fixture.clock.reads();
    fixture.clock.jump_on(recovery_reads, 50);
    assert_eq!(
        fixture.issuance("issued", &["w"]).issue_or_recover(),
        Err(HelperChildIssuanceRefusal::Denied)
    );
    assert_eq!(fixture.clock.reads() - before, recovery_reads);
    assert!(fixture.issued("issued").is_some(), "its record is kept");
}

fn ledger_admit(fixture: &Fixture, name: &str) {
    fixture
        .ledger
        .record_candidate(&namespace(name), &principal("requester"), original(name))
        .expect("recorded")
        .admit()
        .expect("admitted");
}

/// The dispatch path's work owner issues the child through its own single
/// reservation: issued once, recovered on retry, refused for a mistyped
/// intent (nothing issued) and for a withdrawn source.
#[test]
fn the_work_owner_issues_the_child_under_its_own_reservation() {
    use meerkat_authorization::grants::ChildGrantRefusal;
    use meerkat_authorization::work::FurtherOriginals;
    use meerkat_core::authorization::OperationReviewTier;
    use meerkat_runtime::meerkat_machine::SourceOriginalBasis;

    let f = Fixture::new();
    f.source("s", ExecutionRestrictions::unrestricted());
    let further = FurtherOriginals {
        originals: Arc::from([retained_original("s")]),
        source: Arc::new(SourceOriginalBasis::new(f.ledger.clone())),
    };
    let issue = |intent: &(dyn std::any::Any + Send + Sync), tier| {
        f.context.authorization().issue_child_if_current_within(
            f.setup.binding(),
            f.setup.validity().as_ref(),
            &further,
            intent,
            tier,
        )
    };
    let outcome = |boxed: Box<dyn std::any::Any + Send>| {
        *boxed
            .downcast::<Result<ChildGrantOutcome, ChildGrantRefusal>>()
            .expect("the grant owner's typed outcome")
    };

    assert!(
        matches!(
            issue(&"not an intent", OperationReviewTier::R1),
            Err(meerkat_core::OperationAuthorizationError::Refused(_))
        ),
        "a mistyped intent is refused before any effect"
    );
    assert!(f.issued("helper-child").is_none());

    let issued =
        outcome(issue(&f.intent("helper-child"), OperationReviewTier::R1).expect("issued"));
    assert!(matches!(issued, Ok(ChildGrantOutcome::Issued { .. })));
    assert!(f.issued("helper-child").is_some());
    let recovered = f.unchanged(|| {
        outcome(issue(&f.intent("helper-child"), OperationReviewTier::R1).expect("recovered"))
    });
    assert!(matches!(recovered, Ok(ChildGrantOutcome::Recovered { .. })));

    f.ledger
        .cancel_operations(&namespace("s"), &work("s"), evidence("cancel"))
        .expect("the source is withdrawn");
    assert!(
        matches!(
            issue(&f.intent("helper-child-2"), OperationReviewTier::R1),
            Err(meerkat_core::OperationAuthorizationError::Refused(_))
        ),
        "a withdrawn source refuses before any append"
    );
    assert!(f.issued("helper-child-2").is_none());
}

/// A default dispatch context has no governed setup operation: no child
/// issuance can be built from it.
#[test]
fn a_default_dispatch_context_builds_no_child_issuance() {
    let fixture = Fixture::new();
    let args = serde_json::value::RawValue::from_string("{}".to_string()).expect("args");
    assert_eq!(
        GovernedHelperChildIssuance::from_dispatch(
            meerkat_core::ToolCallView {
                id: "setup",
                name: "setup",
                args: &args,
            },
            &meerkat_core::ToolDispatchContext::default(),
            fixture.ledger.clone(),
            Arc::from([]),
            fixture.intent("from-default"),
        )
        .err(),
        Some(HelperChildIssuanceRefusal::Denied)
    );
    assert_eq!(fixture.issued("from-default"), None);
}

/// The records owner whose review tier the host can raise after preparation.
struct TieredRecords(std::sync::atomic::AtomicBool);
impl OperationPolicyOwner for TieredRecords {
    fn authorize_operation(
        &self,
        association: &InputAuthorityAssociation,
        binding: &PreparedAuthorizationBinding,
        purpose: LocalPolicyPurpose,
        now_ms: u64,
    ) -> Result<LocalPolicyAllowance, meerkat_core::OperationAuthorizationError> {
        let mut allowance = Records.authorize_operation(association, binding, purpose, now_ms)?;
        if self.0.load(Ordering::SeqCst) {
            allowance.review_tier = meerkat_core::authorization::OperationReviewTier::R2;
        }
        Ok(allowance)
    }
}

/// A current decision that now needs R2 while the dispatch entered only R1
/// refuses the issuance, and nothing is issued.
#[test]
fn a_decision_needing_more_review_than_entered_issues_nothing() {
    use meerkat_authorization::work::FurtherOriginals;
    use meerkat_core::authorization::OperationReviewTier;
    use meerkat_runtime::meerkat_machine::SourceOriginalBasis;

    let tier = Arc::new(TieredRecords(std::sync::atomic::AtomicBool::new(false)));
    let f = Fixture::new_with(tier.clone());
    f.source("s", ExecutionRestrictions::unrestricted());
    tier.0.store(true, Ordering::SeqCst);
    let further = FurtherOriginals {
        originals: Arc::from([retained_original("s")]),
        source: Arc::new(SourceOriginalBasis::new(f.ledger.clone())),
    };
    let result = f.unchanged(|| {
        f.context.authorization().issue_child_if_current_within(
            f.setup.binding(),
            f.setup.validity().as_ref(),
            &further,
            &f.intent("over-tier"),
            OperationReviewTier::R1,
        )
    });
    assert!(
        matches!(
            result,
            Err(meerkat_core::OperationAuthorizationError::Refused(_))
        ),
        "a decision needing R2 refuses an R1 entry"
    );
    assert!(f.issued("over-tier").is_none(), "nothing was issued");
}

/// Every lower bound holds: a further original's grant valid over
/// [200, 500) admits at 250, and a wall rollback to 100 refuses although the
/// held decision itself started at 0.
#[test]
fn a_further_grant_lower_bound_survives_a_wall_rollback() {
    use meerkat_authorization::work::FurtherOriginals;
    use meerkat_runtime::meerkat_machine::SourceOriginalBasis;

    let f = Fixture::new();
    f.source(
        "g",
        ExecutionRestrictions {
            lifetime: LifetimeRestriction::window(200, 500),
            ..ExecutionRestrictions::unrestricted()
        },
    );
    let further = FurtherOriginals {
        originals: Arc::from([retained_original("g")]),
        source: Arc::new(SourceOriginalBasis::new(f.ledger.clone())),
    };
    let run = || {
        let mut ran = false;
        let result = f.context.authorization().run_if_current_within(
            f.setup.binding(),
            f.setup.validity().as_ref(),
            &further,
            &mut || ran = true,
        );
        (result, ran)
    };
    f.clock.wall.store(250, Ordering::SeqCst);
    assert_eq!(run(), (Ok(()), true), "inside the grant's window");
    f.clock.wall.store(100, Ordering::SeqCst);
    let (result, ran) = run();
    assert!(
        matches!(
            result,
            Err(meerkat_core::OperationAuthorizationError::Refused(_))
        ) && !ran,
        "before the grant's lower bound after a rollback"
    );
}

/// RED-3 (R4): a further original that is still its owner's pending
/// candidate leaves the child unissued and the issuance unavailable
/// (retried), never refused; once the owner promotes it, the child is
/// issued. CONTROL-cancelled (R4): cancelled while pending, it is refused.
#[test]
fn a_pending_further_original_leaves_the_child_unissued_and_unavailable() {
    use meerkat_authorization::work::FurtherOriginals;
    use meerkat_core::authorization::OperationReviewTier;
    use meerkat_runtime::meerkat_machine::SourceOriginalBasis;

    let f = Fixture::new();
    let issue = |source: &str, child: &str| {
        let further = FurtherOriginals {
            originals: Arc::from([retained_original(source)]),
            source: Arc::new(SourceOriginalBasis::new(f.ledger.clone())),
        };
        f.context.authorization().issue_child_if_current_within(
            f.setup.binding(),
            f.setup.validity().as_ref(),
            &further,
            &f.intent(child),
            OperationReviewTier::R1,
        )
    };
    let pending_s = f
        .ledger
        .record_candidate(&namespace("s"), &principal("requester"), original("s"))
        .expect("recorded");
    assert!(
        matches!(
            issue("s", "while-pending"),
            Err(meerkat_core::OperationAuthorizationError::Unavailable)
        ),
        "a pending further original is not yet, never refused"
    );
    assert!(f.issued("while-pending").is_none(), "nothing was issued");
    pending_s.admit().expect("promoted");
    assert!(issue("s", "once-promoted").is_ok(), "issued once promoted");
    assert!(f.issued("once-promoted").is_some());

    let _pending_t = f
        .ledger
        .record_candidate(&namespace("t"), &principal("requester"), original("t"))
        .expect("recorded");
    f.ledger
        .cancel_operations(&namespace("t"), &work("t"), evidence("cancel-t"))
        .expect("a pending candidate can be cancelled");
    assert!(
        matches!(
            issue("t", "cancelled"),
            Err(meerkat_core::OperationAuthorizationError::Refused(_))
        ),
        "a cancellation wins over a pending admission"
    );
    assert!(f.issued("cancelled").is_none());
}
