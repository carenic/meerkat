use super::*;
use crate::runtime::provisioner::{
    MobProvisioner, ProvisionEffectSettlement, ProvisionMemberRequest, ProvisionSessionOrigin,
    RuntimeRevivalIntent, SessionBackend,
};
use crate::{MemberDestinationSelection, MemberRuntimeBuildIdentity};
use meerkat_authorization_contracts::constraints::AudienceRef;
use meerkat_authorization_contracts::evidence::EvidenceId;
use meerkat_core::auth::{PrincipalKind, PrincipalRef};
use meerkat_runtime::MeerkatMachine;
use meerkat_runtime::meerkat_machine::SessionAgentMandate;
use tokio::sync::Semaphore;

const GENERATION: u64 = 7;

/// A host that selects one destination per member from its native identity,
/// records what it saw, and can pause or refuse.
struct SelectingHost {
    seen: std::sync::Mutex<Vec<MemberRuntimeBuildIdentity>>,
    expected_calls: AtomicUsize,
    expected_generation: AtomicU64,
    foreign_context: AtomicBool,
    surfaces: std::sync::Mutex<Vec<(String, usize)>>,
    refuse_surface: AtomicBool,
    prepared_surfaces: std::sync::Mutex<Vec<(String, usize)>>,
    refuse_prepared: AtomicBool,
    mandate: std::sync::Mutex<Option<SessionAgentMandate>>,
    /// `Some` overrides the agent this host expects on recovery; `None`
    /// makes it expect the agent it selected (`mandate`), not an unbound
    /// registration.
    expected_mandate_override: std::sync::Mutex<Option<SessionAgentMandate>>,
    refuse_mandate: AtomicBool,
    entered: Semaphore,
    release: Semaphore,
    pause: bool,
    refuse: bool,
}

impl SelectingHost {
    fn new(pause: bool, refuse: bool) -> Arc<Self> {
        Arc::new(Self {
            seen: Default::default(),
            expected_calls: AtomicUsize::new(0),
            expected_generation: AtomicU64::new(GENERATION),
            foreign_context: AtomicBool::new(false),
            surfaces: Default::default(),
            refuse_surface: AtomicBool::new(false),
            prepared_surfaces: Default::default(),
            refuse_prepared: AtomicBool::new(false),
            mandate: Default::default(),
            expected_mandate_override: Default::default(),
            refuse_mandate: AtomicBool::new(false),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            pause,
            refuse,
        })
    }

    fn selection(identity: &MemberRuntimeBuildIdentity) -> MemberDestinationSelection {
        Self::selection_at(identity, GENERATION)
    }

    fn selection_at(
        identity: &MemberRuntimeBuildIdentity,
        generation: u64,
    ) -> MemberDestinationSelection {
        let owner = PrincipalRef::new(PrincipalKind::SharedAgent, identity.member().member.clone())
            .unwrap();
        MemberDestinationSelection {
            logical_owner: owner.clone(),
            context: identity.session_document().clone(),
            context_generation: generation,
            audience: AudienceRef::Principal { principal: owner },
        }
    }

    async fn reached(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

#[async_trait]
impl crate::MemberRuntimeRegistrationHook for SelectingHost {
    async fn select_destination(
        &self,
        identity: &MemberRuntimeBuildIdentity,
    ) -> Result<MemberDestinationSelection, MobError> {
        self.seen.lock().unwrap().push(identity.clone());
        self.entered.add_permits(1);
        if self.pause {
            self.release.acquire().await.unwrap().forget();
        }
        if self.refuse {
            return Err(MobError::WiringError(
                "host deliberately refused the member".into(),
            ));
        }
        let mut selection = Self::selection(identity);
        if self.foreign_context.load(Ordering::SeqCst) {
            selection.context = EvidenceId::new("another-document").unwrap();
        }
        Ok(selection)
    }

    async fn expected_destination(
        &self,
        identity: &MemberRuntimeBuildIdentity,
    ) -> Result<MemberDestinationSelection, MobError> {
        self.expected_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Self::selection_at(
            identity,
            self.expected_generation.load(Ordering::SeqCst),
        ))
    }

    async fn select_agent_mandate(
        &self,
        _identity: &MemberRuntimeBuildIdentity,
    ) -> Result<Option<SessionAgentMandate>, MobError> {
        if self.refuse_mandate.load(Ordering::SeqCst) {
            return Err(MobError::WiringError(
                "host refused to select the member's agent".into(),
            ));
        }
        Ok(self.mandate.lock().unwrap().clone())
    }

    async fn expected_agent_mandate(
        &self,
        _identity: &MemberRuntimeBuildIdentity,
    ) -> Result<Option<SessionAgentMandate>, MobError> {
        let expected = self.expected_mandate_override.lock().unwrap().clone();
        Ok(expected.or_else(|| self.mandate.lock().unwrap().clone()))
    }

    async fn verify_member_surface(
        &self,
        identity: &MemberRuntimeBuildIdentity,
        surface: &[meerkat_core::ToolDef],
    ) -> Result<(), MobError> {
        self.surfaces
            .lock()
            .unwrap()
            .push((identity.member().member.clone(), surface.len()));
        if self.refuse_surface.load(Ordering::SeqCst) {
            return Err(MobError::WiringError(
                "host refused the member's actual surface".into(),
            ));
        }
        Ok(())
    }

    async fn prepare_member_surface(
        &self,
        identity: &MemberRuntimeBuildIdentity,
        surface: &meerkat_core::PreparedToolSurface,
    ) -> Result<meerkat_core::ToolSurfaceSelection, MobError> {
        self.prepared_surfaces
            .lock()
            .unwrap()
            .push((identity.member().member.clone(), surface.tools.len()));
        if self.refuse_prepared.load(Ordering::SeqCst) {
            return Err(MobError::WiringError(
                "host refused the member's prepared surface".into(),
            ));
        }
        Ok(meerkat_core::ToolSurfaceSelection {
            binding: meerkat_core::ApplicationToolPolicyBinding::Provider {
                provider_id: meerkat_core::PolicyProviderId::new("host").unwrap(),
                policy_id: meerkat_core::PolicyId::new("member-tools").unwrap(),
            },
            compiled_against: surface.clone(),
        })
    }
}

fn stamp_identity(request: &mut ProvisionMemberRequest) {
    let member = meerkat_core::MobMemberBinding {
        mob_id: "hook-mob".into(),
        role: "worker".into(),
        member: "hook-member".into(),
    };
    request.peer_name = member.member.clone();
    let build = request.create_session.build.as_mut().unwrap();
    build.realm_id = Some(meerkat_core::mob_realm_id(&member.mob_id).unwrap());
    build.comms_name = Some(member.comms_name().unwrap().to_string());
    build.mob_member_binding = Some(member);
}

fn fresh_request(session: Session) -> ProvisionMemberRequest {
    let session_id = session.id().clone();
    let mut request = ProvisionMemberRequest {
        authorized_resume: None,
        direct_member_incarnation: None,
        session_origin: ProvisionSessionOrigin::Fresh,
        create_session: CreateSessionRequest {
            injected_context: Vec::new(),
            model: "claude-opus-5".into(),
            prompt: "deferred fixture".into(),
            system_prompt: meerkat_core::SystemPromptOverride::Inherit,
            max_tokens: None,
            event_tx: None,
            build: Some(meerkat_core::service::SessionBuildOptions {
                resume_session: Some(session),
                keep_alive: true,
                ..Default::default()
            }),
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
            labels: None,
        },
        binding: crate::RuntimeBinding::Session,
        peer_name: "hook-member".into(),
        owner_bridge_session_id: None,
        ops_registry: None,
        generated_self_owned_operation_owner: Some(session_id),
        runtime_revival_intent: RuntimeRevivalIntent::None,
        held_bound: None,
    };
    stamp_identity(&mut request);
    request
}

/// The machine's installed governed work host. A scoped destination is
/// governed custody, so a member destination can only be installed on a
/// machine that has one; this one admits no governed input, which these
/// tests never submit.
struct AdmitsNothing;

impl meerkat_runtime::input_authority::NativeWorkAuthorizationHost for AdmitsNothing {
    fn authenticate_association(
        &self,
        _runtime_id: &meerkat_runtime::LogicalRuntimeId,
        _input: &meerkat_runtime::input::Input,
        _ingress: &meerkat_runtime::input_authority::NativeIngressContext,
        _association: &meerkat_authorization_contracts::work_association::InputAuthorityAssociation,
    ) -> Result<(), meerkat_runtime::input_authority::NativeAdmissionError> {
        Err(meerkat_core::OperationRefused::new(meerkat_core::OperationRefusalKind::Denied).into())
    }

    fn work_context(
        &self,
        _batch: &meerkat_runtime::input_authority::NativeWorkBatch,
    ) -> Result<
        meerkat_core::authorization::WorkAuthorizationContext,
        meerkat_runtime::input_authority::NativeWorkContextError,
    > {
        Err(meerkat_runtime::input_authority::NativeWorkContextError::MalformedAcceptedWork)
    }
}

/// The service's runtime owner, with a governed work host installed.
fn governed_machine(service: &MockSessionService) -> Arc<MeerkatMachine> {
    let machine = Arc::new(
        MeerkatMachine::ephemeral()
            .with_native_work_authorization_host(Arc::new(AdmitsNothing))
            .expect("install the governed work host"),
    );
    service.set_runtime_adapter(Arc::clone(&machine));
    machine
}

fn backend(
    pause: bool,
    refuse: bool,
) -> (
    Arc<SessionBackend>,
    Arc<MockSessionService>,
    Arc<MeerkatMachine>,
    Arc<SelectingHost>,
) {
    let service = Arc::new(MockSessionService::new());
    let machine = governed_machine(&service);
    let host = SelectingHost::new(pause, refuse);
    let backend = Arc::new(
        SessionBackend::new(service.clone(), Some(Arc::clone(&machine)), None)
            .with_member_runtime_registration_hook(host.clone()),
    );
    (backend, service, machine, host)
}

async fn removed(machine: &MeerkatMachine, session: &SessionId) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while machine.contains_session(session).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("exact prepared rollback must settle");
}

/// The installed binding is observable only through the machine: retiring
/// exactly the selected generation succeeds once, and only when a binding of
/// that generation is current.
async fn assert_bound_at(machine: &MeerkatMachine, session: &SessionId, generation: u64) {
    let registration = machine
        .current_session_registration_witness(session)
        .await
        .expect("current registration");
    assert!(
        machine
            .retire_scoped_context(&registration, generation + 1)
            .await
            .is_err(),
        "no binding of another generation is current"
    );
    machine
        .retire_scoped_context(&registration, generation)
        .await
        .expect("the selected destination is bound at its generation");
    assert!(
        machine
            .retire_scoped_context(&registration, generation)
            .await
            .is_err(),
        "the retired binding is no longer current"
    );
}

/// A member spawned through the real builder has the host's selection
/// installed on its own registration before its actor exists, from the
/// member's native build identity.
#[tokio::test]
async fn a_spawned_member_has_its_selected_destination_installed() {
    let service = Arc::new(MockSessionService::new());
    let machine = governed_machine(&service);
    let host = SelectingHost::new(false, false);
    let definition = sample_definition();
    let expected_mob = definition.id.to_string();
    let handle = MobBuilder::new(definition, MobStorage::in_memory())
        .with_session_service(service.clone())
        .with_member_runtime_registration_hook(host.clone())
        .create()
        .await
        .unwrap();
    let member = handle
        .spawn_with_options(
            ProfileName::from("worker"),
            AgentIdentity::from("registered-worker"),
            None,
            Some(crate::MobRuntimeMode::TurnDriven),
            None,
        )
        .await
        .unwrap();
    let session = member.bridge_session_id().unwrap().clone();
    let identity = host
        .seen
        .lock()
        .unwrap()
        .iter()
        .find(|identity| identity.member().member == "registered-worker")
        .cloned()
        .expect("the host saw the spawned member");
    assert_eq!(identity.member().mob_id, expected_mob);
    assert_eq!(identity.member().role, "worker");
    assert_bound_at(&machine, &session, GENERATION).await;
    handle.stop().await.unwrap();
}

/// A provision that installed its selection leaves the binding on the exact
/// registration it prepared: a rebind of the same generation is refused.
#[tokio::test]
async fn a_provisioned_member_cannot_be_rebound_at_the_same_generation() {
    let (backend, _service, machine, host) = backend(false, false);
    let session = Session::new();
    let id = session.id().clone();
    backend
        .provision_member(fresh_request(session))
        .await
        .unwrap();
    let registration = machine
        .current_session_registration_witness(&id)
        .await
        .unwrap();
    let identity = host.seen.lock().unwrap()[0].clone();
    let selection = SelectingHost::selection(&identity);
    let target = meerkat_authorization_contracts::work_association::NativeWorkTarget {
        logical_owner: selection.logical_owner,
        logical_runtime: EvidenceId::new(
            meerkat_runtime::LogicalRuntimeId::for_session(&id).to_string(),
        )
        .unwrap(),
        context: EvidenceId::new(id.to_string()).unwrap(),
        context_generation: GENERATION,
        audience: selection.audience,
    };
    assert!(
        machine
            .bind_scoped_context(&registration, &target)
            .await
            .is_err(),
        "the selected destination is already bound at this generation"
    );
    assert_bound_at(&machine, &id, GENERATION).await;
    machine.unregister_session(&id).await.unwrap();
}

/// A host refusal at selection rolls the prepared registration back before
/// the settled provision returns; no actor is ever built.
#[tokio::test]
async fn a_host_refusal_rolls_the_registration_back_before_returning() {
    let (backend, service, machine, host) = backend(true, true);
    let session = Session::new();
    let id = session.id().clone();
    let task = tokio::spawn(async move {
        backend
            .provision_member_settled(fresh_request(session))
            .await
    });
    host.reached().await;
    assert!(!task.is_finished());
    assert!(machine.contains_session(&id).await);
    assert!(!service.has_live_session(&id).await.unwrap());
    host.release.add_permits(1);
    let failure = task
        .await
        .unwrap()
        .expect_err("a refused member is not a usable actor");
    assert_eq!(
        failure.settlement(),
        ProvisionEffectSettlement::ReleasedByOwner
    );
    assert!(
        failure
            .error()
            .to_string()
            .contains("host deliberately refused the member"),
        "{}",
        failure.error()
    );
    assert!(!machine.contains_session(&id).await);
    assert!(!service.has_live_session(&id).await.unwrap());
}

/// A cancelled observer leaves the prepared owner holding the registration
/// until the host returns, and no actor is built afterwards.
#[tokio::test]
async fn a_cancelled_observer_builds_no_actor_after_the_host_returns() {
    let (backend, service, machine, host) = backend(true, false);
    let session = Session::new();
    let id = session.id().clone();
    let task = tokio::spawn(async move {
        backend
            .provision_member_settled(fresh_request(session))
            .await
    });
    host.reached().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        machine.contains_session(&id).await,
        "the host selection is still pending"
    );
    assert!(!service.has_live_session(&id).await.unwrap());
    host.release.add_permits(1);
    removed(&machine, &id).await;
    assert!(
        !service.has_live_session(&id).await.unwrap(),
        "a cancelled observer must not cause actor construction"
    );
    assert_eq!(
        service.max_concurrent_create_session_calls(),
        0,
        "no actor was ever built, not even one cleaned up afterwards"
    );
}

/// A configured host without the canonical runtime adapter is refused
/// before anything is prepared.
#[tokio::test]
async fn a_hook_without_the_runtime_adapter_is_refused() {
    let service = Arc::new(MockSessionService::new());
    let backend = SessionBackend::new(service.clone(), None, None)
        .with_member_runtime_registration_hook(SelectingHost::new(false, false));
    let session = Session::new();
    let error = backend
        .provision_member(fresh_request(session))
        .await
        .expect_err("a hook needs the runtime owner it installs on");
    assert!(
        error
            .to_string()
            .contains("member registration hook requires the canonical runtime adapter"),
        "{error}"
    );
}

/// A host that selects a context other than the member's own session
/// document is refused, and the member is rolled back.
#[tokio::test]
async fn a_foreign_context_selection_is_refused() {
    let (backend, service, machine, host) = backend(false, false);
    host.foreign_context.store(true, Ordering::SeqCst);
    let session = Session::new();
    let id = session.id().clone();
    let error = backend
        .provision_member(fresh_request(session))
        .await
        .expect_err("a foreign context is refused");
    assert!(
        error
            .to_string()
            .contains("the selected context is not the member's own session document"),
        "{error}"
    );
    removed(&machine, &id).await;
    assert!(!service.has_live_session(&id).await.unwrap());
    assert_eq!(service.max_concurrent_create_session_calls(), 0);
}

async fn attached_member_lost_its_actor(
    backend: &SessionBackend,
    service: &Arc<MockSessionService>,
    machine: &Arc<MeerkatMachine>,
) -> (
    SessionId,
    meerkat_runtime::RuntimeExecutorAttachmentWitness,
    ProvisionMemberRequest,
) {
    let session = Session::new();
    let id = session.id().clone();
    backend
        .provision_member(fresh_request(session))
        .await
        .unwrap();
    let attachment = machine
        .current_executor_attachment_witness(&id)
        .await
        .unwrap();
    service.discard_live_session(&id).await.unwrap();
    let mut recovery = retained_resume_request(service, &id).await;
    stamp_identity(&mut recovery);
    recovery.runtime_revival_intent = RuntimeRevivalIntent::MissingLiveMaterialization;
    (id, attachment, recovery)
}

/// An attached member whose installed destination is still the host's
/// expectation recovers its actor on the same attachment; the host is asked
/// for its expectation and never to reinstall.
#[tokio::test]
async fn attached_recovery_verifies_the_installed_destination() {
    let (backend, service, machine, host) = backend(false, false);
    let (id, attachment, recovery) =
        attached_member_lost_its_actor(&backend, &service, &machine).await;
    backend
        .provision_member(recovery)
        .await
        .expect("the verified member recovers its actor");
    assert_eq!(host.expected_calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.seen.lock().unwrap().len(), 1, "no reinstall");
    assert_eq!(
        host.surfaces.lock().unwrap().len(),
        2,
        "the built member and the recovered actor each had their surface checked"
    );
    assert_eq!(
        machine.current_executor_attachment_witness(&id).await,
        Some(attachment),
        "the actor is recovered on the same attachment"
    );
    assert_bound_at(&machine, &id, GENERATION).await;
    machine.unregister_session(&id).await.unwrap();
}

/// A host whose expectation (here, a later generation) differs from the
/// member's installed destination refuses the actor-only recovery; the
/// attachment and binding are untouched. The expectation changes before
/// preparation: this is not a concurrent retirement witness, which the
/// shared mutation gate rules out in source.
#[tokio::test]
async fn attached_recovery_refuses_an_expectation_that_differs_from_the_installed_destination() {
    let (backend, service, machine, host) = backend(false, false);
    let (id, attachment, recovery) =
        attached_member_lost_its_actor(&backend, &service, &machine).await;
    host.expected_generation
        .store(GENERATION + 1, Ordering::SeqCst);
    let error = backend
        .provision_member(recovery)
        .await
        .expect_err("a changed destination refuses recovery");
    assert!(
        error
            .to_string()
            .contains("is not bound to the expected scoped destination"),
        "{error}"
    );
    assert!(!service.has_live_session(&id).await.unwrap());
    assert_eq!(
        machine.current_executor_attachment_witness(&id).await,
        Some(attachment),
        "the attachment is untouched"
    );
    assert_bound_at(&machine, &id, GENERATION).await;
    machine.unregister_session(&id).await.unwrap();
}

/// The host checks the built member's actual surface before the member is
/// published; a refusal discards the built member exactly.
#[tokio::test]
async fn a_surface_refusal_discards_the_built_member() {
    let (backend, service, machine, host) = backend(false, false);
    host.refuse_surface.store(true, Ordering::SeqCst);
    let session = Session::new();
    let id = session.id().clone();
    let error = backend
        .provision_member(fresh_request(session))
        .await
        .expect_err("a refused surface publishes no member");
    assert!(
        error
            .to_string()
            .contains("host refused the member's actual surface"),
        "{error}"
    );
    assert_eq!(
        host.surfaces.lock().unwrap().as_slice(),
        &[("hook-member".to_string(), 0)],
        "the host saw the built member's surface once"
    );
    removed(&machine, &id).await;
    assert!(!service.has_live_session(&id).await.unwrap());
}

/// A hooked member is created on the deferred route only: an eager create is
/// refused before the host is asked, so its first turn can never run on an
/// unverified surface.
#[tokio::test]
async fn an_eager_create_of_a_hooked_member_is_refused() {
    let (backend, service, machine, host) = backend(false, false);
    let session = Session::new();
    let id = session.id().clone();
    let mut request = fresh_request(session);
    request.create_session.initial_turn = meerkat_core::service::InitialTurnPolicy::RunImmediately;
    let error = backend
        .provision_member(request)
        .await
        .expect_err("an eager hooked create is refused");
    assert!(error.to_string().contains("deferred first turn"), "{error}");
    assert!(
        host.seen.lock().unwrap().is_empty(),
        "the host was not asked"
    );
    removed(&machine, &id).await;
    assert_eq!(service.max_concurrent_create_session_calls(), 0);
}

/// A host refusal of a recovered actor's surface discards that actor before
/// it is committed; the attachment and binding stay as they were.
#[tokio::test]
async fn attached_recovery_discards_an_actor_whose_surface_the_host_refuses() {
    let (backend, service, machine, host) = backend(false, false);
    let (id, attachment, recovery) =
        attached_member_lost_its_actor(&backend, &service, &machine).await;
    host.refuse_surface.store(true, Ordering::SeqCst);
    let error = backend
        .provision_member(recovery)
        .await
        .expect_err("a refused surface commits no recovered actor");
    assert!(
        error
            .to_string()
            .contains("host refused the member's actual surface"),
        "{error}"
    );
    assert_eq!(host.surfaces.lock().unwrap().len(), 2);
    assert!(!service.has_live_session(&id).await.unwrap());
    assert_eq!(
        machine.current_executor_attachment_witness(&id).await,
        Some(attachment),
        "the attachment is untouched"
    );
    assert_bound_at(&machine, &id, GENERATION).await;
    machine.unregister_session(&id).await.unwrap();
}

/// An eager actor-only recovery of a hooked member is refused before the
/// host is asked anything and before any actor is built.
#[tokio::test]
async fn an_eager_attached_recovery_of_a_hooked_member_is_refused() {
    let (backend, service, machine, host) = backend(false, false);
    let (id, attachment, mut recovery) =
        attached_member_lost_its_actor(&backend, &service, &machine).await;
    recovery.create_session.initial_turn = meerkat_core::service::InitialTurnPolicy::RunImmediately;
    let error = backend
        .provision_member(recovery)
        .await
        .expect_err("an eager hooked recovery is refused");
    assert!(error.to_string().contains("deferred first turn"), "{error}");
    assert_eq!(host.expected_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        host.surfaces.lock().unwrap().len(),
        1,
        "only the cold build"
    );
    assert!(!service.has_live_session(&id).await.unwrap());
    assert_eq!(
        machine.current_executor_attachment_witness(&id).await,
        Some(attachment)
    );
    machine.unregister_session(&id).await.unwrap();
}

/// Without an installed governed work host a member destination cannot be
/// installed: the member is refused and rolled back instead of registering
/// unscoped.
#[tokio::test]
async fn a_member_is_refused_where_no_governed_work_host_is_installed() {
    let service = Arc::new(MockSessionService::new());
    let machine = service.enable_runtime_adapter();
    let host = SelectingHost::new(false, false);
    let backend = SessionBackend::new(service.clone(), Some(Arc::clone(&machine)), None)
        .with_member_runtime_registration_hook(host.clone());
    let session = Session::new();
    let id = session.id().clone();
    let error = backend
        .provision_member(fresh_request(session))
        .await
        .expect_err("no governed host, no member");
    assert!(
        error
            .to_string()
            .contains("a scoped destination requires the governed work host"),
        "{error}"
    );
    removed(&machine, &id).await;
    assert_eq!(service.max_concurrent_create_session_calls(), 0);
}

/// A new member's registration on a governed machine carries its host's
/// surface decision before the member's agent is built: the gate the
/// provisioner installed asks this member's host about the prepared surface,
/// and carries the host's refusal.
#[tokio::test]
async fn a_new_member_registration_carries_its_hosts_surface_decision() {
    let (backend, _service, machine, host) = backend(false, false);
    let session = Session::new();
    let id = session.id().clone();
    backend
        .provision_member(fresh_request(session))
        .await
        .expect("provisioned");
    let gate = machine
        .session_tool_surface_gate(&id)
        .await
        .expect("the member's registration carries its host's gate");
    let read_record = meerkat_core::ToolIdentity {
        name: "read_record".into(),
        provenance: None,
    };
    let surface = meerkat_core::PreparedToolSurface {
        tools: vec![read_record.clone()],
        projected: vec![read_record],
        catalog: meerkat_core::PreparedToolCatalog::VisibleOnly,
    };
    let selection = gate.decide(&surface).await.expect("the host selects");
    assert_eq!(
        selection.compiled_against, surface,
        "the host's selection names the surface it was made from"
    );
    assert_eq!(
        host.prepared_surfaces.lock().unwrap().as_slice(),
        &[("hook-member".to_string(), 1)],
        "the decision is this member's host's, on the prepared surface"
    );
    host.refuse_prepared.store(true, Ordering::SeqCst);
    let refusal = gate.decide(&surface).await.expect_err("the host refuses");
    assert!(
        refusal
            .reason
            .contains("host refused the member's prepared surface"),
        "{refusal}"
    );
}

/// A host-selected agent mandate naming `agent` for one configured server.
fn agent_mandate(agent: &str) -> SessionAgentMandate {
    let principal = |name: &str| {
        PrincipalRef::in_domain(
            PrincipalKind::SharedAgent,
            name,
            meerkat_core::TrustDomainId::new("agents").unwrap(),
        )
        .unwrap()
    };
    let lineage = meerkat_authorization_contracts::grant::GrantLineageRef {
        root_authority: principal("operator"),
        authority_namespace: EvidenceId::new("installations").unwrap(),
        authority_generation: 1,
        authority_incarnation:
            meerkat_authorization_contracts::grant::GrantAuthorityIncarnation::from_uuid(
                uuid::Uuid::parse_str("6f1c2b9e-3a4d-4f8e-9c1b-2d3e4f5a6b7c").unwrap(),
            )
            .unwrap(),
        grant_id: EvidenceId::new(format!("{agent}-grant")).unwrap(),
        issued_revision: 1,
    };
    SessionAgentMandate::new(
        principal(agent),
        std::collections::BTreeMap::from([("configured".to_owned(), vec![lineage])]),
        None,
        None,
    )
    .unwrap()
}

/// The registration's agent binding is observable only through the machine:
/// another mandate is refused (so one is bound), then the bound mandate binds
/// again idempotently (so it is this one). The check never binds an unbound
/// registration.
async fn assert_agent_bound_to(
    machine: &MeerkatMachine,
    session: &SessionId,
    mandate: &SessionAgentMandate,
) {
    let registration = machine
        .current_session_registration_witness(session)
        .await
        .expect("current registration");
    // Refusal of another mandate first: it proves a binding already exists
    // without this check installing one.
    assert!(
        machine
            .bind_session_agent_mandate(&registration, agent_mandate("someone-else"))
            .await
            .is_err(),
        "the registration is already bound"
    );
    machine
        .bind_session_agent_mandate(&registration, mandate.clone())
        .await
        .expect("it is bound to exactly this mandate (an idempotent rebind)");
}

/// A new member's registration is bound to the agent its host selected
/// before the member's agent or configured servers are built.
#[tokio::test]
async fn a_new_member_registration_is_bound_to_its_hosts_agent_mandate() {
    let (backend, _service, machine, host) = backend(false, false);
    let mandate = agent_mandate("member-agent");
    *host.mandate.lock().unwrap() = Some(mandate.clone());
    let session = Session::new();
    let id = session.id().clone();
    backend
        .provision_member(fresh_request(session))
        .await
        .expect("provisioned");
    assert_agent_bound_to(&machine, &id, &mandate).await;
    machine.unregister_session(&id).await.unwrap();
}

/// A host that selects no agent leaves the registration unbound (its
/// configured servers stay unavailable); the provision itself succeeds.
#[tokio::test]
async fn a_host_without_an_agent_selection_leaves_the_registration_unbound() {
    let (backend, _service, machine, _host) = backend(false, false);
    let session = Session::new();
    let id = session.id().clone();
    backend
        .provision_member(fresh_request(session))
        .await
        .expect("provisioned");
    let registration = machine
        .current_session_registration_witness(&id)
        .await
        .expect("current registration");
    machine
        .bind_session_agent_mandate(&registration, agent_mandate("first"))
        .await
        .expect("an unbound registration accepts its first mandate");
    machine.unregister_session(&id).await.unwrap();
}

/// A host refusal at agent selection rolls the prepared registration back:
/// no actor is built.
#[tokio::test]
async fn an_agent_selection_refusal_rolls_the_registration_back() {
    let (backend, service, machine, host) = backend(false, false);
    host.refuse_mandate.store(true, Ordering::SeqCst);
    let session = Session::new();
    let id = session.id().clone();
    let failure = backend
        .provision_member_settled(fresh_request(session))
        .await
        .expect_err("a member without its agent is not usable");
    assert_eq!(
        failure.settlement(),
        ProvisionEffectSettlement::ReleasedByOwner
    );
    assert!(
        failure
            .error()
            .to_string()
            .contains("host refused to select the member's agent"),
        "{}",
        failure.error()
    );
    assert!(!machine.contains_session(&id).await);
    assert!(!service.has_live_session(&id).await.unwrap());
}

/// An attached member recovers its actor only while its registration is
/// still bound to the agent its host expects; recovery never rebinds. The
/// refused recovery leaves the attachment and the agent binding as they were
/// (the provisioner's earlier surface-gate install is not part of this
/// claim).
#[tokio::test]
async fn attached_recovery_checks_the_expected_agent_mandate() {
    let (backend, service, machine, host) = backend(false, false);
    let mandate = agent_mandate("member-agent");
    *host.mandate.lock().unwrap() = Some(mandate.clone());
    let (id, attachment, recovery) =
        attached_member_lost_its_actor(&backend, &service, &machine).await;
    *host.expected_mandate_override.lock().unwrap() = Some(agent_mandate("another-agent"));
    let error = backend
        .provision_member(recovery)
        .await
        .expect_err("a different expected agent refuses recovery");
    assert!(
        error
            .to_string()
            .contains("is not bound to the expected agent mandate"),
        "{error}"
    );
    assert!(!service.has_live_session(&id).await.unwrap());
    assert_eq!(
        machine.current_executor_attachment_witness(&id).await,
        Some(attachment.clone()),
        "the attachment is untouched"
    );
    assert_agent_bound_to(&machine, &id, &mandate).await;

    // Back to expecting the agent it selected.
    *host.expected_mandate_override.lock().unwrap() = None;
    let mut retry = retained_resume_request(&service, &id).await;
    stamp_identity(&mut retry);
    retry.runtime_revival_intent = RuntimeRevivalIntent::MissingLiveMaterialization;
    backend
        .provision_member(retry)
        .await
        .expect("the expected agent recovers its actor");
    assert_eq!(
        machine.current_executor_attachment_witness(&id).await,
        Some(attachment),
        "the actor is recovered on the same attachment"
    );
    assert_agent_bound_to(&machine, &id, &mandate).await;
    machine.unregister_session(&id).await.unwrap();
}

fn held_backend() -> (
    Arc<SessionBackend>,
    Arc<MockSessionService>,
    Arc<MeerkatMachine>,
    Arc<SelectingHost>,
) {
    let (backend, service, machine, host) = backend(false, false);
    service.decide_tool_surface_at_create();
    (backend, service, machine, host)
}

/// A held member is finished under its host's selection and created, but
/// not published: it has no serving attachment until it is published, and
/// publication is the existing one.
#[tokio::test]
async fn a_held_member_publishes_only_when_its_holder_resumes_it() {
    let (backend, _service, machine, host) = held_backend();
    let mut held = Vec::new();
    for _ in 0..2 {
        let session = Session::new();
        let id = session.id().clone();
        let member = backend
            .provision_member_held(fresh_request(session))
            .await
            .expect("held");
        assert_eq!(member.member_ref().bridge_session_id(), Some(&id));
        assert_eq!(
            member.finished_surface().binding,
            meerkat_core::ApplicationToolPolicyBinding::Provider {
                provider_id: meerkat_core::PolicyProviderId::new("host").unwrap(),
                policy_id: meerkat_core::PolicyId::new("member-tools").unwrap(),
            },
            "the member was finished under its host's selection"
        );
        assert!(
            machine
                .current_executor_attachment_witness(&id)
                .await
                .is_none(),
            "a held member does not serve"
        );
        held.push((id, member));
    }
    assert_eq!(
        host.prepared_surfaces.lock().unwrap().len(),
        2,
        "each member is decided once, when it is prepared"
    );
    // Every member is finished before any is published.
    for (id, member) in held {
        let receipt = member.publish().await.expect("published");
        assert_eq!(receipt.member_ref.bridge_session_id(), Some(&id));
        assert!(
            machine
                .current_executor_attachment_witness(&id)
                .await
                .is_some(),
            "the published member serves"
        );
    }
}

/// An aborted held member is settled: nothing of it stays registered.
#[tokio::test]
async fn an_aborted_held_member_is_settled() {
    let (backend, _service, machine, _host) = held_backend();
    let session = Session::new();
    let id = session.id().clone();
    let member = backend
        .provision_member_held(fresh_request(session))
        .await
        .expect("held");
    member.abort().await.expect("aborted");
    removed(&machine, &id).await;
}

/// A held member whose actor was closed while it was held is refused at its
/// final publication decision and settled.
#[tokio::test]
async fn a_held_member_closed_before_publication_is_refused() {
    let (backend, service, machine, _host) = held_backend();
    let session = Session::new();
    let id = session.id().clone();
    let member = backend
        .provision_member_held(fresh_request(session))
        .await
        .expect("held");
    service.discard_live_session(&id).await.expect("discard");
    let error = member.publish().await.expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("was closed before its publication"),
        "{error}"
    );
    assert!(
        machine
            .current_executor_attachment_witness(&id)
            .await
            .is_none()
    );
    removed(&machine, &id).await;
}

/// A member whose host refuses its prepared surface is never held; a
/// provision the held mode does not cover is refused before anything is
/// built.
#[tokio::test]
async fn only_a_finished_fresh_member_is_held() {
    let (backend, _service, machine, host) = held_backend();
    host.refuse_prepared.store(true, Ordering::SeqCst);
    let session = Session::new();
    let id = session.id().clone();
    assert!(
        backend
            .provision_member_held(fresh_request(session))
            .await
            .is_err()
    );
    removed(&machine, &id).await;

    let mut resumed = fresh_request(Session::new());
    resumed.session_origin = ProvisionSessionOrigin::ResumedDurable;
    assert!(backend.provision_member_held(resumed).await.is_err());
}

/// A service discard that competes with a held member's final serving
/// commit, completing after its liveness was read and before the commit
/// finishes, never leaves a published member whose actor is gone: either
/// the publication is refused and settled, or the discard waits for the
/// commit and closes a published member.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_discard_competing_with_the_final_commit_never_publishes_a_closed_actor() {
    let (backend, service, machine, _host) = held_backend();
    let session = Session::new();
    let id = session.id().clone();
    let member = backend
        .provision_member_held(fresh_request(session))
        .await
        .expect("held");
    let (revoked_tx, revoked_rx) = std::sync::mpsc::channel();
    let discarding = Arc::clone(&service);
    let discarded = id.clone();
    let landed_inside = Arc::new(AtomicBool::new(false));
    let landed = Arc::clone(&landed_inside);
    super::provisioner::HELD_FINAL_COMMIT_TEST_HOOKS
        .lock()
        .unwrap()
        .insert(
            id.clone(),
            Box::new(move || {
                // The real service table's revocation, on another thread.
                std::thread::spawn(move || {
                    discarding.actor_registry.remove_current(&discarded);
                    let _ = revoked_tx.send(());
                });
                // Give the discard every chance to land inside the interval.
                if revoked_rx.recv_timeout(Duration::from_secs(5)).is_ok() {
                    landed.store(true, Ordering::SeqCst);
                }
            }),
        );
    match member.publish().await {
        Ok(_) => {
            assert!(
                !landed_inside.load(Ordering::SeqCst),
                "a member whose actor was discarded inside its final commit was published"
            );
        }
        Err(error) => {
            assert!(
                error
                    .to_string()
                    .contains("was closed during its publication"),
                "{error}"
            );
            assert!(
                machine
                    .current_executor_attachment_witness(&id)
                    .await
                    .is_none()
            );
            removed(&machine, &id).await;
        }
    }
    // The mock service supplies no exclusion, so the competing discard does
    // land inside the interval: the refusal above is the path exercised.
    assert!(landed_inside.load(Ordering::SeqCst));
}
