//! A consented (R3) governed call that continues with governed helper steps
//! after its single consuming entry. The native grant-backed work owner (with
//! its audit wrapper), the approval owner with a host human consent route, the
//! real fenced entry, the retained consent attempt, the helper WorkGraph fence
//! and the helper child issuance are production code; the human route, the
//! records policy and the original source are fixtures.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use meerkat::governed_realm::helper_admission::{
    GovernedHelperAdmissionFence, HelperAdmissionRefusal,
};
use meerkat::governed_realm::helper_child_issuance::{
    GovernedHelperChildIssuance, HelperChildIssuanceRefusal,
};
use meerkat_authorization::grant_policy::GrantBackedWorkPolicy;
use meerkat_authorization::grants::{ChildGrantIntent, ChildGrantOutcome};
use meerkat_authorization::work::{FurtherOriginals, RetainedOriginal};
use meerkat_authorization_contracts::audit::{
    AuthorizationAuditObservation, AuthorizationAuditSink,
};
use meerkat_core::agent::{
    dispatch_tool_execution_plan_fenced, resolve_tool_execution_plan_fenced,
};
use meerkat_core::approval::consent::{
    AuthenticatedApprover, AuthenticatedConsentDecision, ConsentEligibility, ConsentIneligible,
    ConsentNotice, ConsentNoticeKind, ConsentPresentation, ConsentPresentationCandidate,
    ConsentRouteUnavailable, HumanConsentRoute,
};
use meerkat_core::approval::review::{
    BoundOperationReview, OperationReviewObserver, OperationReviewer, ReviewAttemptStatus,
    ReviewCandidate, ReviewEntrySupport, ReviewObservation, ReviewUnsatisfiedKind, ReviewVerdict,
    ReviewerFailure,
};
use meerkat_core::authorization::{
    ControllerReadinessFailure, OperationObservationError, RefusalDispatch,
};
use meerkat_core::{
    ApprovalId, ApprovalRecord, EnteredDispatchOperation, dispatch_retained_consent_attempt,
};
use meerkat_runtime::meerkat_machine::{
    AuthenticatedOriginal, AuthenticatedOriginalSource, OriginalDisposition, RetainedCustody,
    SourceOriginalBasis,
};

/// The consented call's id, as the binding retains it.
const CONSENTED_CALL: &str = "consented-read";
/// Host-chosen validity of one consent request.
const CONSENT_VALIDITY: Duration = Duration::from_secs(4 * 60 * 60);

/// The records owner, rating every record operation R3: a fresh qualified
/// human decision is required. It selects no separate action account.
struct ConsentedRecords;
impl OperationPolicyOwner for ConsentedRecords {
    fn authorize_operation(
        &self,
        association: &InputAuthorityAssociation,
        binding: &PreparedAuthorizationBinding,
        purpose: LocalPolicyPurpose,
        now_ms: u64,
    ) -> Result<LocalPolicyAllowance, meerkat_core::OperationAuthorizationError> {
        let mut allowance =
            RecordOwner.authorize_operation(association, binding, purpose, now_ms)?;
        allowance.review_tier = meerkat_core::authorization::OperationReviewTier::R3;
        Ok(allowance)
    }

    fn review_action_account(
        &self,
        _: &InputAuthorityAssociation,
        _: &PreparedAuthorizationBinding,
        _: u64,
    ) -> Result<Option<String>, meerkat_core::OperationAuthorizationError> {
        Ok(None)
    }
}

/// R3 is never model-reviewed; counts every review it is asked for.
#[derive(Default)]
struct NoReviewer(AtomicUsize);
#[async_trait]
impl OperationReviewer for NoReviewer {
    async fn review(
        &self,
        _candidate: &ReviewCandidate<'_>,
    ) -> Result<ReviewVerdict, ReviewerFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(ReviewerFailure::Unavailable)
    }
}

/// The host's human route: `lead-1` is the eligible approver, published
/// through one local authorization publication; notices are recorded.
struct ConsentRoute {
    notices: Mutex<Vec<(ApprovalId, ConsentNoticeKind)>>,
    publication: LocalAuthorizationPublication,
}
impl ConsentRoute {
    fn new() -> Self {
        Self {
            notices: Mutex::new(Vec::new()),
            publication: LocalAuthorizationPublication::new(),
        }
    }

    fn notices(&self, approval_id: &ApprovalId) -> Vec<ConsentNoticeKind> {
        self.notices
            .lock()
            .expect("notices")
            .iter()
            .filter(|(id, _)| id == approval_id)
            .map(|(_, kind)| *kind)
            .collect()
    }
}
impl HumanConsentRoute for ConsentRoute {
    fn present(
        &self,
        _candidate: &ConsentPresentationCandidate<'_>,
    ) -> Result<ConsentPresentation, ConsentRouteUnavailable> {
        Ok(ConsentPresentation {
            requester: meerkat_core::ApprovalPrincipalId::new("human:requester")
                .expect("principal"),
            risk: meerkat_core::ApprovalRisk::High,
            metadata: meerkat_core::SurfaceMetadata::default(),
        })
    }

    fn eligibility(
        &self,
        approver: &AuthenticatedApprover,
        _approval: &ApprovalRecord,
    ) -> Result<Arc<dyn ConsentEligibility>, ConsentIneligible> {
        let eligible = approver.principal().id.as_str() == "lead-1";
        meerkat_authorization::publication::PublishedConsentEligibility::observe(
            &self.publication,
            || eligible,
        )
    }

    fn notice(&self, notice: ConsentNotice) {
        self.notices
            .lock()
            .expect("notices")
            .push((notice.approval_id, notice.kind));
    }
}

/// An authenticated approval by `id`, as the host's ingress adapter makes it.
fn approval_by(id: &str) -> AuthenticatedConsentDecision {
    AuthenticatedConsentDecision {
        approver: AuthenticatedApprover::new(
            PrincipalRef::in_domain(
                PrincipalKind::Human,
                id,
                TrustDomainId::new("team").expect("domain"),
            )
            .expect("principal"),
            meerkat_core::ApprovalAssurance::PersonAuthenticated,
        )
        .expect("qualified approver"),
        decision: meerkat_core::ApprovalDecision::Approve,
        reason: None,
    }
}

/// Every review observation, for counting the `Used` reports.
#[derive(Default)]
struct UsedObservations(Mutex<Vec<ReviewObservation>>);
impl OperationReviewObserver for UsedObservations {
    fn observe(&self, observation: ReviewObservation) {
        self.0
            .lock()
            .expect("review observations")
            .push(observation);
    }
}
impl UsedObservations {
    /// How many times the attempt of `call` was reported used.
    fn used(&self, call: &str) -> usize {
        self.0
            .lock()
            .expect("review observations")
            .iter()
            .filter(|observation| {
                observation.status == ReviewAttemptStatus::Used
                    && observation.tool_call_id.as_ref() == call
            })
            .count()
    }
}

/// The work's own audit sink: every observation is accepted.
#[derive(Default)]
struct AuditRecords(Mutex<Vec<AuthorizationAuditObservation>>);
impl AuthorizationAuditSink for AuditRecords {
    fn append(
        &self,
        observation: AuthorizationAuditObservation,
    ) -> Result<(), OperationObservationError> {
        self.0.lock().expect("audit records").push(observation);
        Ok(())
    }
}

/// The work's own original as its source owner holds it: active, and never
/// retained as a contributor. Armed, its next disposition read, which a
/// helper step makes inside the work owners' exclusion after the review
/// owner admitted that step, reports the step in flight and holds it until
/// released: a store publication blocked under the entered operation.
struct StepOriginals {
    armed: AtomicBool,
    in_flight: Notify,
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    release: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}
impl StepOriginals {
    fn new() -> Self {
        let (release, gate) = std::sync::mpsc::channel();
        Self {
            armed: AtomicBool::new(false),
            in_flight: Notify::new(),
            gate: Mutex::new(Some(gate)),
            release: Mutex::new(Some(release)),
        }
    }

    /// Let a held step go on. Idempotent.
    fn release(&self) {
        drop(self.release.lock().expect("step gate").take());
    }
}
impl AuthenticatedOriginalSource for StepOriginals {
    fn authenticated_original(
        &self,
        _: &NativeIngressContext,
        _: &QualifiedIngressNamespace,
        _: &OriginalWorkRef,
    ) -> Result<Option<AuthenticatedOriginal>, ControllerReadinessFailure> {
        Ok(None)
    }

    fn retained_custody(
        &self,
        _: &QualifiedIngressNamespace,
        _: &OriginalWorkRef,
    ) -> Result<RetainedCustody, ControllerReadinessFailure> {
        Ok(RetainedCustody::NotRetained)
    }

    fn original_disposition(
        &self,
        _: &QualifiedIngressNamespace,
        _: &OriginalWorkRef,
    ) -> Result<OriginalDisposition, ControllerReadinessFailure> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.in_flight.notify_one();
            let gate = self.gate.lock().expect("step gate").take();
            if let Some(gate) = gate {
                // Released, or the sender dropped: `recv` returns either way.
                let _ = gate.recv();
            }
        }
        Ok(OriginalDisposition::Active)
    }
}

/// Releases a held step when the test ends, whether it passed or not.
struct ReleaseOnDrop(Arc<StepOriginals>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// The consented call: `read_record` on `record-7`, rated R3.
struct ConsentedCall {
    id: String,
    name: String,
    args: Box<serde_json::value::RawValue>,
}
impl ConsentedCall {
    fn new() -> Self {
        Self {
            id: CONSENTED_CALL.into(),
            name: "read_record".into(),
            args: serde_json::value::RawValue::from_string(r#"{"record":"record-7"}"#.into())
                .expect("arguments"),
        }
    }

    fn view(&self) -> ToolCallView<'_> {
        ToolCallView {
            id: &self.id,
            name: &self.name,
            args: &self.args,
        }
    }
}

/// What one entered call's helper steps answered.
#[derive(Debug, Clone)]
struct HelperSteps {
    created: bool,
    fence_refusal: Option<HelperAdmissionRefusal>,
    children: Vec<Result<ChildGrantOutcome, HelperChildIssuanceRefusal>>,
    retried: Result<ChildGrantOutcome, HelperChildIssuanceRefusal>,
}

/// What the consented leaf does after its single consuming entry.
enum LeafMode {
    /// Admit the helper item, issue every intent's child, then retry the
    /// first intent: each a further step of the entered operation.
    Steps(Vec<ChildGrantIntent>),
    /// Admit the helper item from a worker thread, whose step the test holds
    /// in flight, and wait for that worker.
    Blocked,
}

/// The consented tool's effect leaf: it consumes review at its entry, makes
/// the call's single consuming entry with `enter_governed_operation`, then
/// takes the call's governed helper steps with the witness.
struct EnteredLeaf {
    mode: LeafMode,
    graph: meerkat::WorkGraphService,
    originals: Arc<StepOriginals>,
    bodies: AtomicUsize,
    steps: Mutex<Vec<HelperSteps>>,
    witness: Mutex<Option<EnteredDispatchOperation>>,
    worker: Mutex<Option<std::thread::JoinHandle<(bool, Option<HelperAdmissionRefusal>)>>>,
}

impl EnteredLeaf {
    fn helper_admission(&self) -> (meerkat::WorkAdmissionKey, meerkat::WorkAdmission) {
        let key = meerkat::WorkAdmissionKey::new("helper-for-consented-read").expect("key");
        let admission = self
            .graph
            .admission_identity(key.clone(), helper_request())
            .expect("identity");
        (key, admission)
    }

    /// The helper's WorkGraph item, then each intent's child, then the first
    /// intent again: every one a further step of the entered operation.
    async fn helper_steps(
        &self,
        entered: &EnteredDispatchOperation,
        intents: &[ChildGrantIntent],
    ) -> HelperSteps {
        let (key, admission) = self.helper_admission();
        let fence = GovernedHelperAdmissionFence::from_entered(
            entered,
            self.originals.clone(),
            no_contributors(),
            "default",
            meerkat::WorkNamespace::default(),
            admission,
        );
        let created = matches!(
            self.graph
                .create_idempotent_with_fence(key, helper_request(), fence.clone())
                .await,
            Ok(meerkat::WorkAdmissionOutcome::Created(_))
        );
        let issue = |intent: &ChildGrantIntent| {
            GovernedHelperChildIssuance::from_entered(
                entered,
                self.originals.clone(),
                no_contributors(),
                intent.clone(),
            )
            .issue_or_recover()
        };
        let children = intents.iter().map(&issue).collect();
        let retried = issue(&intents[0]);
        HelperSteps {
            created,
            fence_refusal: fence.refusal(),
            children,
            retried,
        }
    }
}

#[async_trait]
impl AgentToolDispatcher for EnteredLeaf {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        RecordingTools::default().tools()
    }

    fn review_entry_support(&self, _tool_name: &str) -> ReviewEntrySupport {
        ReviewEntrySupport::ConsumesAtEntry
    }

    async fn dispatch(&self, _: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        panic!("the consented leaf keeps its dispatch context")
    }

    async fn dispatch_with_context(
        &self,
        call: ToolCallView<'_>,
        context: &ToolDispatchContext,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        // The call's single consuming entry, at its first mutating step: the
        // human consent is spent here, once.
        let entered = context.enter_governed_operation(call)?;
        match &self.mode {
            LeafMode::Steps(intents) => {
                let steps = self.helper_steps(&entered, intents).await;
                self.steps.lock().expect("helper steps").push(steps);
            }
            LeafMode::Blocked => {
                *self.witness.lock().expect("witness") = Some(entered.clone());
                let (key, admission) = self.helper_admission();
                let fence = GovernedHelperAdmissionFence::from_entered(
                    &entered,
                    self.originals.clone(),
                    no_contributors(),
                    "default",
                    meerkat::WorkNamespace::default(),
                    admission,
                );
                let (done, finished) = tokio::sync::oneshot::channel();
                let graph = self.graph.clone();
                let runtime = tokio::runtime::Handle::current();
                let worker = std::thread::spawn(move || {
                    let created = matches!(
                        runtime.block_on(graph.create_idempotent_with_fence(
                            key,
                            helper_request(),
                            fence.clone(),
                        )),
                        Ok(meerkat::WorkAdmissionOutcome::Created(_))
                    );
                    let refusal = fence.refusal();
                    drop(fence);
                    let _ = done.send(());
                    (created, refusal)
                });
                *self.worker.lock().expect("helper worker") = Some(worker);
                // The dispatch waits for its helper publication; the host
                // cancels the dispatch here, while that step is held.
                let _ = finished.await;
            }
        }
        self.bodies.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::new(call.id.into(), "record-7 value".into(), false).into())
    }
}

fn helper_request() -> meerkat::CreateWorkItemRequest {
    meerkat::CreateWorkItemRequest {
        title: "setup helper".into(),
        ..Default::default()
    }
}

fn no_contributors() -> Arc<[RetainedOriginal]> {
    Arc::from([])
}

fn child_intent(parent: &GrantLineageRef, name: &str) -> ChildGrantIntent {
    ChildGrantIntent {
        issuer: principal("helper-operator"),
        parent_lineage: vec![parent.clone()],
        id: id(name),
        grantee: principal("helper"),
        represented_subject: None,
        requested: ExecutionRestrictions::unrestricted(),
    }
}

fn execution_scope() -> OperationExecutionScope {
    OperationExecutionScope::RuntimeInput {
        owner_session_id: SessionId::new(),
        runtime_epoch_id: serde_json::from_str(r#""00000000-0000-0000-0000-000000000002""#)
            .expect("epoch id"),
        submitted_input_id: serde_json::from_str(r#""00000000-0000-0000-0000-000000000003""#)
            .expect("input id"),
        canonical_input_id: serde_json::from_str(r#""00000000-0000-0000-0000-000000000003""#)
            .expect("input id"),
    }
}

/// Native work where the consented read is permitted at R3 by the actual
/// grant-backed owner, with a host human route composed; the helper's
/// independent parent grant is issued by the same grant owner.
struct ConsentedWorld {
    grants: Arc<LocalGrantAuthority>,
    approvals: meerkat_core::ApprovalService,
    route: Arc<ConsentRoute>,
    reviewer: Arc<NoReviewer>,
    observer: Arc<UsedObservations>,
    originals: Arc<StepOriginals>,
    leaf: Arc<EnteredLeaf>,
    context: ToolDispatchContext,
    parent: GrantLineageRef,
}

impl ConsentedWorld {
    /// `children` intents for the inline helper steps, or the blocked
    /// helper step when `blocked`.
    fn new(children: usize, blocked: bool) -> Self {
        let grants = Arc::new(
            LocalGrantAuthority::new(
                LocalGrantConfiguration {
                    root: principal("grant-owner"),
                    namespace: id("entered-consent-grants"),
                    generation: 1,
                },
                LocalAuthorizationPublication::new(),
                Arc::new(HostAuthorizationClock),
            )
            .expect("actual generated grant owner"),
        );
        let controller = grants
            .issue_root(
                &principal("grant-owner"),
                id("controller"),
                principal("executor"),
                None,
                ceiling("infer"),
            )
            .expect("actual controller lineage");
        let operation = grants
            .issue_root(
                &principal("grant-owner"),
                id("read-only"),
                principal("executor"),
                None,
                ceiling("read"),
            )
            .expect("actual read-only operation grant");
        let parent = grants
            .issue_root(
                &principal("grant-owner"),
                id("helper-parent"),
                principal("helper-operator"),
                None,
                ExecutionRestrictions::unrestricted(),
            )
            .expect("actual independent parent grant");
        let runtime = LogicalRuntimeId::for_session(&SessionId::new());
        let admitted = association(&runtime, controller, operation, selection());
        let policy = Arc::new(GrantBackedWorkPolicy::new(
            Arc::clone(&grants),
            Arc::new(InvocationOwner),
            Arc::new(ConsentedRecords),
        ));
        let work = policy
            .audited_work_context(
                Arc::from([admitted]),
                execution_scope(),
                Arc::new(AuditRecords::default()),
            )
            .expect("the native grant-backed work context");
        let approvals = meerkat_core::ApprovalService::new();
        let route = Arc::new(ConsentRoute::new());
        let reviewer = Arc::new(NoReviewer::default());
        let observer = Arc::new(UsedObservations::default());
        let review =
            BoundOperationReview::new(reviewer.clone(), approvals.clone(), Duration::from_secs(30))
                .with_observer(observer.clone())
                .with_human_consent(route.clone(), CONSENT_VALIDITY);
        let originals = Arc::new(StepOriginals::new());
        let mode = if blocked {
            LeafMode::Blocked
        } else {
            LeafMode::Steps(
                (1..=children)
                    .map(|n| child_intent(&parent, &format!("helper-child-{n}")))
                    .collect(),
            )
        };
        let leaf = Arc::new(EnteredLeaf {
            mode,
            graph: meerkat::WorkGraphService::new(Arc::new(meerkat::MemoryWorkGraphStore::new())),
            originals: Arc::clone(&originals),
            bodies: AtomicUsize::new(0),
            steps: Mutex::new(Vec::new()),
            witness: Mutex::new(None),
            worker: Mutex::new(None),
        });
        let context = ToolDispatchContext::default()
            .with_work_authorization(Some(work))
            .with_operation_review(Some(Arc::new(review)));
        Self {
            grants,
            approvals,
            route,
            reviewer,
            observer,
            originals,
            leaf,
            context,
            parent,
        }
    }

    fn plan(&self, call: &ConsentedCall) -> meerkat_core::ResolvedToolExecutionPlan {
        resolve_tool_execution_plan_fenced(
            &self.leaf,
            call.view(),
            &self.context,
            &meerkat_core::ToolExecutionResolutionContext::new(
                meerkat_core::ToolDeadlineChain::new(vec![
                    meerkat_core::ToolDeadlineContributor::finite(
                        meerkat_core::ToolDeadlineOwner::DirectCaller,
                        Duration::from_secs(600),
                    ),
                ])
                .expect("finite caller deadline"),
            ),
        )
        .expect("real plan resolution")
    }

    /// The consented call settles locally with its one consent approval;
    /// nothing entered.
    async fn request(&self, call: &ConsentedCall) -> ApprovalId {
        let plan = self.plan(call);
        let settled =
            dispatch_tool_execution_plan_fenced(&self.leaf, call.view(), &self.context, &plan)
                .await;
        let approval_id = match settled {
            Err(ToolError::ReviewUnsatisfied {
                kind: ReviewUnsatisfiedKind::HumanConsentRequired,
                approval_id: Some(approval_id),
            }) => approval_id,
            other => panic!("expected local human_consent_required, got {other:?}"),
        };
        assert_eq!(
            self.route.notices(&approval_id),
            [ConsentNoticeKind::Requested]
        );
        assert_eq!(
            self.leaf.bodies.load(Ordering::SeqCst),
            0,
            "nothing entered before the consent"
        );
        approval_id
    }

    fn approve(&self, approval_id: &ApprovalId) {
        let approved = self
            .approvals
            .decide_consent(approval_id, approval_by("lead-1"))
            .expect("the authenticated eligible approval commits");
        assert_eq!(approved.status, meerkat_core::ApprovalStatus::Approved);
    }

    /// The step's further originals: none beyond the work's own.
    fn further(&self) -> FurtherOriginals {
        FurtherOriginals {
            originals: no_contributors(),
            source: Arc::new(SourceOriginalBasis::new(self.originals.clone())),
        }
    }
}

/// The consented call is entered once by its retained attempt; the witness
/// then admits the helper item and issues `children` children, each a step
/// decided afresh, and a retry of the first intent recovers its child. The
/// consent is spent exactly once, the body runs once, nothing is refused as
/// already consumed, and a second entry finds the consent spent.
async fn consented_call_takes_its_helper_steps(children: usize) {
    let world = ConsentedWorld::new(children, false);
    let call = ConsentedCall::new();
    let approval_id = world.request(&call).await;
    world.approve(&approval_id);

    let entered =
        dispatch_retained_consent_attempt(&world.leaf, &world.context, &approval_id).await;
    assert!(
        entered.is_ok(),
        "the retained candidate enters: {entered:?}"
    );
    let recorded = world.leaf.steps.lock().expect("helper steps").clone();
    assert_eq!(recorded.len(), 1, "one entered call");
    let steps = &recorded[0];
    assert!(steps.created, "the helper item is committed: {steps:?}");
    assert_eq!(steps.fence_refusal, None);
    assert_eq!(steps.children.len(), children);
    for (index, child) in steps.children.iter().enumerate() {
        let Ok(ChildGrantOutcome::Issued { lineage }) = child else {
            panic!("child {index} is issued: {child:?}")
        };
        assert_eq!(
            world
                .grants
                .issued_reference(&id(&format!("helper-child-{}", index + 1)))
                .expect("lookup")
                .as_ref(),
            lineage.last(),
            "the grant owner retains exactly the issued child"
        );
        world
            .grants
            .resolve_controller_lineage(lineage, &principal("helper"), None)
            .expect("the child resolves as the helper's controller lineage");
    }
    let Ok(ChildGrantOutcome::Recovered { lineage: recovered }) = &steps.retried else {
        panic!("retrying an issued intent recovers its child: {steps:?}")
    };
    assert_eq!(
        Some(recovered.as_slice()),
        steps.children[0]
            .as_ref()
            .ok()
            .map(ChildGrantOutcome::lineage),
        "the same child, with no second spend"
    );
    assert_eq!(
        world.leaf.bodies.load(Ordering::SeqCst),
        1,
        "one body entry"
    );
    let consent = world
        .approvals
        .get(&approval_id)
        .expect("record")
        .consent
        .expect("consent history");
    assert!(consent.consumed_at.is_some(), "spent by the entry");
    assert!(consent.withdrawal.is_none(), "never withdrawn");
    assert_eq!(
        world.route.notices(&approval_id),
        [
            ConsentNoticeKind::Requested,
            ConsentNoticeKind::Approved,
            ConsentNoticeKind::Used,
        ],
        "decided, then used exactly once"
    );
    assert_eq!(
        world.observer.used(CONSENTED_CALL),
        1,
        "Used is reported once, after the dispatch ended"
    );
    assert_eq!(
        world.reviewer.0.load(Ordering::SeqCst),
        0,
        "R3 is never model-reviewed"
    );

    // The steps spent nothing more: a second entry finds the consent spent.
    let second = dispatch_retained_consent_attempt(&world.leaf, &world.context, &approval_id).await;
    assert!(
        matches!(
            second,
            Err(ToolError::ReviewUnsatisfied {
                kind: ReviewUnsatisfiedKind::AlreadyConsumed,
                ..
            })
        ),
        "a second entry finds the consent spent: {second:?}"
    );
    assert_eq!(world.leaf.bodies.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_consented_call_admits_its_helper_item_and_issues_two_children_with_one_spend() {
    consented_call_takes_its_helper_steps(2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_consented_call_admits_its_helper_item_and_issues_five_children_with_one_spend() {
    consented_call_takes_its_helper_steps(5).await;
}

/// The consented dispatch is cancelled while the helper publication it
/// admitted under its entered operation is still held in flight. The consent
/// stays spent and in use: a second spend is refused as already consumed,
/// and no Used is reported. A step started after the cancellation is refused
/// as denied, with no not-dispatched certificate, and runs nothing; the held
/// step then completes and commits its item. Only once the last witness is
/// dropped is Used reported, exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_consented_dispatch_keeps_its_review_in_use_until_its_held_step_commits() {
    let world = ConsentedWorld::new(0, true);
    let _release = ReleaseOnDrop(Arc::clone(&world.originals));
    let call = ConsentedCall::new();
    let approval_id = world.request(&call).await;
    world.approve(&approval_id);

    world.originals.armed.store(true, Ordering::SeqCst);
    let attempt = tokio::spawn({
        let leaf = Arc::clone(&world.leaf);
        let context = world.context.clone();
        let approval_id = approval_id.clone();
        async move { dispatch_retained_consent_attempt(&leaf, &context, &approval_id).await }
    });
    tokio::time::timeout(
        Duration::from_secs(20),
        world.originals.in_flight.notified(),
    )
    .await
    .expect("the helper step is admitted and in flight");
    // The host cancels the admitting dispatch while its helper step is held.
    attempt.abort();
    let cancelled = attempt.await;
    assert!(
        cancelled
            .as_ref()
            .is_err_and(tokio::task::JoinError::is_cancelled),
        "the admitting dispatch was cancelled: {cancelled:?}"
    );

    // The consent stays spent and in use while the admitted step is in
    // flight: a second spend is refused, and no Used is reported.
    let second = dispatch_retained_consent_attempt(&world.leaf, &world.context, &approval_id).await;
    assert!(
        matches!(
            second,
            Err(ToolError::ReviewUnsatisfied {
                kind: ReviewUnsatisfiedKind::AlreadyConsumed,
                ..
            })
        ),
        "a second spend is refused as already consumed: {second:?}"
    );
    assert!(
        world
            .approvals
            .get(&approval_id)
            .expect("record")
            .consent
            .and_then(|consent| consent.consumed_at)
            .is_some(),
        "spent once, at the entry"
    );
    assert_eq!(
        world.observer.used(CONSENTED_CALL),
        0,
        "no Used while the admitted step is in flight"
    );
    assert_eq!(
        world.route.notices(&approval_id),
        [ConsentNoticeKind::Requested, ConsentNoticeKind::Approved],
        "no Used notice while the admitted step is in flight"
    );

    // A step that starts after the cancellation is refused as ended.
    let witness = world
        .leaf
        .witness
        .lock()
        .expect("witness")
        .take()
        .expect("the entry minted its witness");
    let mut ran = false;
    let refused = witness.run_if_current_within(&world.further(), &mut || ran = true);
    match &refused {
        Err(ToolError::AuthorizationRefused { refusal }) => {
            assert_eq!(refusal.kind(), OperationRefusalKind::Denied);
            assert_eq!(refusal.dispatch(), RefusalDispatch::Unknown);
        }
        other => panic!("a step after the cancellation is refused as denied: {other:?}"),
    }
    assert!(!ran, "the refused step ran nothing");
    let error = refused.expect_err("refused");
    assert_eq!(error.to_error_payload()["data"]["tool_invoked"], "unknown");
    let after = GovernedHelperChildIssuance::from_entered(
        &witness,
        world.originals.clone(),
        no_contributors(),
        child_intent(&world.parent, "after-the-end"),
    )
    .issue_or_recover();
    assert_eq!(after, Err(HelperChildIssuanceRefusal::Denied));
    assert_eq!(
        world
            .grants
            .issued_reference(&id("after-the-end"))
            .expect("lookup"),
        None,
        "nothing issued after the cancellation"
    );

    // The held publication goes on: its step completes and commits.
    world.originals.release();
    let worker = world
        .leaf
        .worker
        .lock()
        .expect("helper worker")
        .take()
        .expect("the entry started its helper worker");
    let (created, refusal) = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::task::spawn_blocking(move || worker.join()),
    )
    .await
    .expect("the helper worker finishes")
    .expect("join task")
    .expect("helper worker");
    assert!(created, "the in-flight step committed its helper item");
    assert_eq!(refusal, None);

    // Released at its last step's finish, the attempt admits no step either.
    let mut ran_after_release = false;
    let refused_after_release =
        witness.run_if_current_within(&world.further(), &mut || ran_after_release = true);
    assert!(refused_after_release.is_err(), "{refused_after_release:?}");
    assert!(!ran_after_release, "the refused step ran nothing");
    assert_eq!(
        world.observer.used(CONSENTED_CALL),
        0,
        "the witness still holds the entered custody"
    );

    drop(witness);
    assert_eq!(
        world.observer.used(CONSENTED_CALL),
        1,
        "Used is reported once, after the last witness was dropped"
    );
    assert_eq!(
        world.route.notices(&approval_id),
        [
            ConsentNoticeKind::Requested,
            ConsentNoticeKind::Approved,
            ConsentNoticeKind::Used,
        ]
    );
}
