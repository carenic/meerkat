//! The headline `brain_swap` proof: model A asks to become model B, and the
//! NEXT provider call is actually made on B.
//!
//! Everything else in this feature proves a fragment. The staging tests prove
//! the tool stages. The promotion tests prove a clean run boundary commits the
//! request. The realization saga proves the ordering of the rebinding steps.
//! None of them observe a provider call, so none of them can catch the failure
//! that matters to a user: the request is recorded, the log says `Realized`,
//! and the model answering the next turn is still A.
//!
//! This module closes that gap end to end, without credentials:
//!
//! * the `brain_swap` builtin is registered by the ORDINARY availability path
//!   — no manual tool registration, no injected dispatcher, no
//!   `default_llm_client`, no `llm_client_override`. Each of those makes the
//!   factory (correctly) advertise no reachable models, so a test that used
//!   one would be proving a tool production would never have offered;
//! * the provider layer is a fake `ProviderRuntime` registered in an otherwise
//!   empty `ProviderRuntimeRegistry`. It resolves an inline test secret and
//!   returns a scripted client, so resolution is real and the network is not;
//! * models A (`gpt-5.4`) and B (`gpt-5.5`) share one OpenAI auth binding, so
//!   the switch exercises the model-only/account-affinity route the tool
//!   promises rather than a provider change;
//! * the turns are submitted through `accept_input_with_completion`, so the
//!   real runtime loop — including its pre-dequeue realization pass — is what
//!   moves the identity. Nothing here calls the realization method directly.
//!
//! Both durable persistence profiles run: WholeBlob
//! (`SqliteRuntimeStore::new_whole_blob` over a `JsonlStore`) and HeadCanonical
//! (`SqliteRuntimeStore::new_head_canonical` sharing the `SqliteSessionStore`
//! file). They commit session state through genuinely different store
//! authorities, and the handoff must cross both.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};

use async_trait::async_trait;
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::image_generation::{SwitchTurnDuration, SwitchTurnOrigin, SwitchTurnRequestId};
use meerkat_core::lifecycle::{InputId, RunId};
use meerkat_core::service::{
    CreateSessionRequest, DeferredPromptPolicy, InitialTurnPolicy, SessionBuildOptions,
};
use meerkat_core::session::model_routing_control::{
    ModelRoutingIntentRecordDisposition, SessionModelRoutingControlRecord,
};
use meerkat_core::{
    AuthBindingRef, BindingId, Config, Provider, RealmConfigSection, RealmId, Session, SessionId,
    SessionLlmIdentity, StopReason,
};
use meerkat_llm_core::provider_runtime::{
    ProviderAuthError, ProviderClientError, ProviderRuntime, ProviderRuntimeRegistry,
    ResolvedConnection, ResolvedTextTarget, ResolverEnvironment, StaticLease, ValidatedBinding,
};
use meerkat_runtime::completion::CompletionOutcome;
use meerkat_runtime::store::RuntimeStore;
use meerkat_runtime::{
    Input, LogicalRuntimeId, MeerkatMachine, PromptInput, SessionServiceRuntimeExt,
};
use meerkat_tools::builtin::brain_swap::BRAIN_SWAP_TOOL_NAME;

use crate::session_runtime::llm_reconfigure::{
    SessionRuntimeLlmReconfigureHostBlueprint, SessionRuntimeLlmReconfigureService,
};
use crate::{
    AgentFactory, FactoryAgentBuilder, PersistenceBundle, PersistentSessionService, SessionStore,
};

/// The model the session starts on.
const MODEL_A: &str = "gpt-5.4";
/// The model the session asks to become, reachable on the SAME binding.
const MODEL_B: &str = "gpt-5.5";
const TEST_REALM: &str = "default";
const TEST_BINDING: &str = "default_openai";
const AUTHORING_TEXT: &str = "staged the swap";
const NEXT_TURN_TEXT: &str = "answered after the swap";
const SECOND_NEXT_TURN_TEXT: &str = "answered the second queued turn";

// ---------------------------------------------------------------------------
// Scripted provider
// ---------------------------------------------------------------------------

/// Scripted client for the deferred (cross-run) brain swap.
///
/// It records `request.model` for EVERY call, which is the only observation
/// that can distinguish "the log says B" from "B actually served the turn".
struct DeferredBrainSwapClient {
    requested_models: Arc<StdMutex<Vec<String>>>,
}

impl DeferredBrainSwapClient {
    fn record(&self, model: &str) -> usize {
        let mut recorded = self
            .requested_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        recorded.push(model.to_string());
        recorded.len() - 1
    }
}

fn usage_event(model: &str) -> LlmEvent {
    LlmEvent::UsageUpdate {
        usage: meerkat_core::TurnUsage::host_declared(
            Provider::OpenAI,
            model,
            meerkat_core::Usage::default(),
        ),
    }
}

fn done_event(stop_reason: StopReason) -> LlmEvent {
    LlmEvent::Done {
        outcome: LlmDoneOutcome::Success { stop_reason },
    }
}

#[async_trait]
impl LlmClient for DeferredBrainSwapClient {
    fn project_replay_messages(
        &self,
        messages: &[meerkat_core::Message],
    ) -> Result<Vec<meerkat_core::Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(
        &'a self,
        request: &'a LlmRequest,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>>
    {
        let call_index = self.record(&request.model);
        let events: Vec<Result<LlmEvent, LlmError>> = match call_index {
            // Call 0: the model asks, mid-run, to switch. The request is
            // staged; this run keeps answering on A.
            0 => vec![
                Ok(LlmEvent::ToolCallComplete {
                    id: "toolu_brain_swap".to_string(),
                    name: BRAIN_SWAP_TOOL_NAME.to_string(),
                    args: serde_json::json!({ "target_model": MODEL_B }),
                    meta: None,
                }),
                Ok(usage_event(MODEL_A)),
                Ok(done_event(StopReason::ToolUse)),
            ],
            // Call 1: the authoring turn reads its own tool result and
            // completes — still on A, which is the whole point of staging.
            1 => vec![
                Ok(LlmEvent::TextDelta {
                    delta: AUTHORING_TEXT.to_string(),
                    meta: None,
                }),
                Ok(usage_event(MODEL_A)),
                Ok(done_event(StopReason::EndTurn)),
            ],
            // Call 2: the next turn. Usage echoes whatever model the runtime
            // actually routed to, so a wrong route cannot hide behind a
            // hardcoded constant.
            2 => vec![
                Ok(LlmEvent::TextDelta {
                    delta: NEXT_TURN_TEXT.to_string(),
                    meta: None,
                }),
                Ok(usage_event(&request.model)),
                Ok(done_event(StopReason::EndTurn)),
            ],
            // Call 3 is used by the concurrent-admission proof. Distinct text
            // makes a queue-order reversal observable through the two exact
            // completion handles.
            3 => vec![
                Ok(LlmEvent::TextDelta {
                    delta: SECOND_NEXT_TURN_TEXT.to_string(),
                    meta: None,
                }),
                Ok(usage_event(&request.model)),
                Ok(done_event(StopReason::EndTurn)),
            ],
            // Anything past the script is a real finding, not noise to absorb.
            other => vec![Err(LlmError::InvalidRequest {
                message: format!(
                    "unscripted provider call #{other} on model {}",
                    request.model
                ),
            })],
        };
        Box::pin(futures::stream::iter(events))
    }

    fn provider(&self) -> Provider {
        Provider::OpenAI
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

/// Credential-free OpenAI provider runtime.
///
/// `resolve_binding` performs the real validated-binding → resolved-connection
/// step with an inline test secret, so the session resolves a registry-backed
/// provider identity — which is exactly what makes `brain_swap` available at
/// all. `build_client` hands back the scripted client instead of an HTTP one.
struct DeferredBrainSwapProviderRuntime {
    client: Arc<dyn LlmClient>,
    availability: Arc<ModelAvailability>,
    client_build_models: Arc<StdMutex<Vec<String>>>,
}

#[derive(Default)]
struct ModelAvailability {
    unavailable: StdRwLock<BTreeSet<String>>,
}

impl ModelAvailability {
    fn set_available(&self, model: &str, available: bool) {
        let mut unavailable = self
            .unavailable
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if available {
            unavailable.remove(model);
        } else {
            unavailable.insert(model.to_string());
        }
    }

    fn is_available(&self, model: &str) -> bool {
        !self
            .unavailable
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(model)
    }
}

#[async_trait]
impl ProviderRuntime for DeferredBrainSwapProviderRuntime {
    fn provider_id(&self) -> Provider {
        Provider::OpenAI
    }

    async fn resolve_binding(
        &self,
        binding: &ValidatedBinding,
        _env: &ResolverEnvironment,
    ) -> Result<ResolvedConnection, ProviderAuthError> {
        Ok(ResolvedConnection {
            provider: Provider::OpenAI,
            backend: binding.backend(),
            backend_profile: Arc::clone(binding.backend_profile()),
            credential_identity: binding.credential_identity().clone(),
            auth_lease: Arc::new(StaticLease::inline_secret(
                "unused-test-key".to_string(),
                meerkat_core::AuthMetadata::default(),
                None,
                "brain-swap-ab-test:openai".to_string(),
            )),
        })
    }

    fn build_client(
        &self,
        _connection: ResolvedConnection,
    ) -> Result<Arc<dyn LlmClient>, ProviderClientError> {
        Ok(Arc::clone(&self.client))
    }

    fn build_text_client(
        &self,
        target: ResolvedTextTarget,
    ) -> Result<Arc<dyn LlmClient>, ProviderClientError> {
        let model = target.identity().model.clone();
        self.client_build_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(model.clone());
        if !self.availability.is_available(&model) {
            return Err(ProviderClientError::ClientInit(format!(
                "model '{model}' is unavailable in the fake provider runtime"
            )));
        }
        Ok(Arc::clone(&self.client))
    }
}

struct CountingReconfigureService {
    inner: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    identity_applications: Arc<AtomicUsize>,
}

#[async_trait]
impl SessionRuntimeLlmReconfigureService for CountingReconfigureService {
    async fn acquire_runtime_turn_finalization_guard(
        &self,
        session_id: &SessionId,
    ) -> Result<
        Box<dyn meerkat_core::lifecycle::CoreExecutorTurnFinalizationGuard>,
        meerkat_core::service::SessionError,
    > {
        SessionRuntimeLlmReconfigureService::acquire_runtime_turn_finalization_guard(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn live_llm_identity(
        &self,
        session_id: &SessionId,
    ) -> Result<SessionLlmIdentity, meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::live_llm_identity(self.inner.as_ref(), session_id)
            .await
    }

    async fn live_session_has_instruction_activations(
        &self,
        session_id: &SessionId,
    ) -> Result<bool, meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::live_session_has_instruction_activations(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn live_realm_id(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<RealmId>, meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::live_realm_id(self.inner.as_ref(), session_id).await
    }

    async fn live_tool_visibility_state(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<meerkat_core::SessionToolVisibilityState>, meerkat_core::service::SessionError>
    {
        SessionRuntimeLlmReconfigureService::live_tool_visibility_state(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn live_web_search_override(
        &self,
        session_id: &SessionId,
    ) -> Result<meerkat_core::ToolCategoryOverride, meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::live_web_search_override(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn live_tool_scope_snapshot(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<meerkat_core::ToolScopeSnapshot>, meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::live_tool_scope_snapshot(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn apply_live_llm_identity_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
        client: Arc<dyn meerkat_core::AgentLlmClient>,
        identity: SessionLlmIdentity,
        request_policy: meerkat_core::SessionLlmRequestPolicy,
    ) -> Result<(), meerkat_core::service::SessionError> {
        self.identity_applications.fetch_add(1, Ordering::SeqCst);
        SessionRuntimeLlmReconfigureService::apply_live_llm_identity_under_runtime_turn_boundary(
            self.inner.as_ref(),
            session_id,
            client,
            identity,
            request_policy,
        )
        .await
    }

    async fn apply_live_tool_visibility_state_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
        state: Option<meerkat_core::SessionToolVisibilityState>,
    ) -> Result<(), meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::apply_live_tool_visibility_state_under_runtime_turn_boundary(
            self.inner.as_ref(),
            session_id,
            state,
        )
        .await
    }

    async fn persist_live_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
    ) -> Result<(), meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::persist_live_under_runtime_turn_boundary(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn discard_live_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
    ) -> Result<(), meerkat_core::service::SessionError> {
        SessionRuntimeLlmReconfigureService::discard_live_under_runtime_turn_boundary(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn live_model_routing_control_history(
        &self,
        session_id: &SessionId,
    ) -> Result<
        meerkat_core::session::model_routing_control::SessionModelRoutingControlHistory,
        meerkat_core::service::SessionError,
    > {
        SessionRuntimeLlmReconfigureService::live_model_routing_control_history(
            self.inner.as_ref(),
            session_id,
        )
        .await
    }

    async fn append_live_model_routing_control_record_under_runtime_turn_boundary(
        &self,
        session_id: &SessionId,
        record: SessionModelRoutingControlRecord,
    ) -> Result<
        meerkat_core::session::model_routing_control::ModelRoutingControlAppendOutcome,
        meerkat_core::service::SessionError,
    > {
        SessionRuntimeLlmReconfigureService::append_live_model_routing_control_record_under_runtime_turn_boundary(
            self.inner.as_ref(),
            session_id,
            record,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Which durable session-persistence authority the runtime store owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistenceProfile {
    WholeBlob,
    HeadCanonical,
}

/// One inline OpenAI key, minted as the realm's default binding.
///
/// Both A and B resolve through this single binding, so a switch that
/// silently rebound credentials or accounts shows up as a changed
/// `auth_binding` on the resulting identity.
fn ab_test_config() -> Config {
    let mut config = Config::default();
    config.realm.insert(
        TEST_REALM.to_string(),
        RealmConfigSection::from_inline_api_keys(&[("openai", "unused-test-key")]),
    );
    config
}

fn expected_auth_binding() -> AuthBindingRef {
    AuthBindingRef {
        realm: RealmId::parse(TEST_REALM).expect("valid test realm"),
        binding: BindingId::parse(TEST_BINDING).expect("valid test binding"),
        profile: None,
        origin: meerkat_core::BindingOrigin::Configured,
    }
}

fn create_request() -> CreateSessionRequest {
    CreateSessionRequest {
        injected_context: Vec::new(),
        model: MODEL_A.to_string(),
        prompt: meerkat_core::ContentInput::Text(String::new()),
        system_prompt: crate::SystemPromptOverride::Set("runtime-loop brain swap A->B".to_string()),
        max_tokens: None,
        event_tx: None,
        initial_turn: InitialTurnPolicy::Defer,
        deferred_prompt_policy: DeferredPromptPolicy::Discard,
        build: Some(SessionBuildOptions {
            provider: Some(Provider::OpenAI),
            realm_id: Some(RealmId::parse(TEST_REALM).expect("valid test realm")),
            ..Default::default()
        }),
        labels: None,
    }
}

struct AbHarness {
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    adapter: Arc<MeerkatMachine>,
    runtime_store: Arc<dyn RuntimeStore>,
    session_id: SessionId,
    runtime_id: LogicalRuntimeId,
    environment: Arc<AbEnvironment>,
    identity_applications: Arc<AtomicUsize>,
}

struct AbEnvironment {
    profile: PersistenceProfile,
    temp: tempfile::TempDir,
    requested_models: Arc<StdMutex<Vec<String>>>,
    availability: Arc<ModelAvailability>,
    client_build_models: Arc<StdMutex<Vec<String>>>,
}

impl AbHarness {
    fn requested_models(&self) -> Vec<String> {
        self.environment
            .requested_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set_available(&self, model: &str, available: bool) {
        self.environment
            .availability
            .set_available(model, available);
    }

    fn clear_client_build_models(&self) {
        self.environment
            .client_build_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn client_build_models(&self) -> Vec<String> {
        self.environment
            .client_build_models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn identity_application_count(&self) -> usize {
        self.identity_applications.load(Ordering::SeqCst)
    }

    async fn accept_prompt(&self, prompt: &str) -> (InputId, meerkat_runtime::CompletionHandle) {
        let (accepted, completion) = self
            .adapter
            .accept_input_with_completion(
                &self.session_id,
                Input::Prompt(PromptInput::new(prompt, None)),
            )
            .await
            .expect("runtime must accept the prompt input");
        let completion = completion.expect("an accepted prompt must yield a completion handle");
        let input_id = match accepted {
            meerkat_runtime::AcceptOutcome::Accepted { input_id, .. } => input_id,
            other => panic!("a fresh prompt must be accepted, got {other:?}"),
        };
        (input_id, completion)
    }

    async fn wait_completion(completion: meerkat_runtime::CompletionHandle) -> CompletionOutcome {
        tokio::time::timeout(std::time::Duration::from_secs(5), completion.wait())
            .await
            .expect("the turn must settle within the deterministic budget")
            .expect("the completion waiter must resolve")
    }

    async fn run_prompt(&self, prompt: &str) -> CompletionOutcome {
        let (_input_id, completion) = self.accept_prompt(prompt).await;
        Self::wait_completion(completion).await
    }

    async fn authoritative_session(&self) -> Session {
        self.service
            .load_authoritative_session(&self.session_id)
            .await
            .expect("load the authoritative session")
            .expect("a runtime-backed session must be durable")
    }

    /// The durable model-routing control log, read back off the authoritative
    /// session rather than off any live in-memory copy.
    async fn durable_routing_records(&self) -> Vec<SessionModelRoutingControlRecord> {
        self.authoritative_session()
            .await
            .model_routing_control()
            .records()
            .to_vec()
    }

    async fn durable_identity(&self) -> SessionLlmIdentity {
        self.authoritative_session()
            .await
            .session_metadata()
            .expect("a runtime-backed session must retain canonical metadata")
            .llm_identity()
    }

    async fn live_identity(&self) -> SessionLlmIdentity {
        self.service
            .live_session_llm_identity(&self.session_id)
            .await
            .expect("read the live identity watch")
    }

    /// Whether `run` actually committed a run-boundary receipt — the exact
    /// evidence realization requires before it will move routing.
    async fn origin_run_committed_a_boundary(&self, run: &RunId) -> bool {
        !self
            .runtime_store
            .load_committed_boundary_receipts(&self.runtime_id, run)
            .await
            .expect("read committed boundary receipts")
            .is_empty()
    }

    async fn cold_restart(self) -> Self {
        let environment = Arc::clone(&self.environment);
        let session_id = self.session_id.clone();
        self.adapter
            .unregister_session(&session_id)
            .await
            .expect("tear down the original runtime attachment");
        drop(self);
        attach_harness(environment, Some(session_id)).await
    }
}

async fn build_harness(profile: PersistenceProfile) -> AbHarness {
    let environment = Arc::new(AbEnvironment {
        profile,
        temp: tempfile::tempdir().expect("brain-swap A/B tempdir"),
        requested_models: Arc::new(StdMutex::new(Vec::new())),
        availability: Arc::new(ModelAvailability::default()),
        client_build_models: Arc::new(StdMutex::new(Vec::new())),
    });
    attach_harness(environment, None).await
}

async fn attach_harness(
    environment: Arc<AbEnvironment>,
    resume_session_id: Option<SessionId>,
) -> AbHarness {
    let scripted: Arc<dyn LlmClient> = Arc::new(DeferredBrainSwapClient {
        requested_models: Arc::clone(&environment.requested_models),
    });

    let mut factory = AgentFactory::new(environment.temp.path().join("factory-sessions"))
        .without_provider_auth_persistence()
        .builtins(true);
    factory.provider_registry = Arc::new(ProviderRuntimeRegistry::empty().with_runtime(Arc::new(
        DeferredBrainSwapProviderRuntime {
            client: scripted,
            availability: Arc::clone(&environment.availability),
            client_build_models: Arc::clone(&environment.client_build_models),
        },
    )));

    let builder = FactoryAgentBuilder::new(factory, ab_test_config());
    assert!(
        builder.default_llm_client.is_none(),
        "the A->B proof must resolve every client through the provider registry"
    );

    let sqlite_path = environment.temp.path().join("sessions.sqlite3");
    let (session_store, runtime_store): (Arc<dyn SessionStore>, Arc<dyn RuntimeStore>) =
        match environment.profile {
            PersistenceProfile::WholeBlob => (
                Arc::new(meerkat_store::JsonlStore::new(
                    environment.temp.path().join("jsonl"),
                )),
                Arc::new(
                    meerkat_runtime::store::SqliteRuntimeStore::new_whole_blob(
                        environment.temp.path().join("runtime.sqlite3"),
                    )
                    .expect("open the whole-blob runtime store"),
                ),
            ),
            PersistenceProfile::HeadCanonical => (
                Arc::new(
                    meerkat_store::SqliteSessionStore::open(sqlite_path.clone())
                        .expect("open the sqlite session store"),
                ),
                Arc::new(
                    meerkat_runtime::store::SqliteRuntimeStore::new_head_canonical(sqlite_path)
                        .expect("open the head-canonical runtime store"),
                ),
            ),
        };
    let persistence = PersistenceBundle::new(
        Arc::clone(&session_store),
        Arc::clone(&runtime_store),
        Arc::new(meerkat_store::MemoryBlobStore::new()),
    );

    // The blueprint's client slot stays empty on purpose. A populated slot
    // would short-circuit the reconfigure host and resolve target B without
    // ever consulting the provider registry, which is precisely the evidence
    // this test exists to produce. It is installed after the service exists
    // because the host needs that concrete service.
    let blueprint = SessionRuntimeLlmReconfigureHostBlueprint::new(
        &builder,
        environment.temp.path().join("config_state.json"),
        Arc::new(StdRwLock::new(None)),
    );

    let (service, adapter) = crate::surface::build_runtime_backed_service(builder, 4, persistence);
    let service = Arc::new(service);
    let identity_applications = Arc::new(AtomicUsize::new(0));
    blueprint.install(
        &adapter,
        Arc::new(CountingReconfigureService {
            inner: Arc::clone(&service),
            identity_applications: Arc::clone(&identity_applications),
        }) as Arc<dyn SessionRuntimeLlmReconfigureService>,
    );

    let session = match resume_session_id {
        Some(session_id) => service
            .load_authoritative_session(&session_id)
            .await
            .expect("load the retained authoritative session after restart")
            .expect("the retained session must remain durable"),
        None => Session::new(),
    };
    let session_id = session.id().clone();
    let runtime_id = LogicalRuntimeId::for_session(&session_id);
    let materialized = Box::pin(crate::surface::materialize_session(
        &service,
        &adapter,
        session,
        create_request(),
        {
            let service = Arc::clone(&service);
            let adapter = Arc::clone(&adapter);
            move |session_id| {
                crate::surface::default_persistent_executor(service, adapter, session_id)
            }
        },
    ))
    .await
    .expect("materialize the runtime-backed session with a real executor");
    assert_eq!(materialized.session_id, session_id);

    AbHarness {
        service,
        adapter,
        runtime_store,
        session_id,
        runtime_id,
        environment,
        identity_applications,
    }
}

struct CommittedHandoffProof {
    request_id: SwitchTurnRequestId,
    origin_run: RunId,
    auth_binding: Option<AuthBindingRef>,
}

async fn commit_brain_swap_request(harness: &AbHarness) -> CommittedHandoffProof {
    let auth_binding = harness.live_identity().await.auth_binding;
    assert_eq!(auth_binding.as_ref(), Some(&expected_auth_binding()));

    let authoring = harness.run_prompt("please switch after this turn").await;
    assert!(
        matches!(authoring, CompletionOutcome::Completed(ref run) if run.text == AUTHORING_TEXT),
        "the authoring turn must complete on A: {authoring:?}"
    );
    assert_eq!(
        harness.requested_models(),
        vec![MODEL_A.to_string(), MODEL_A.to_string()]
    );
    assert_eq!(harness.live_identity().await.model, MODEL_A);
    assert_eq!(harness.durable_identity().await.model, MODEL_A);

    let records = harness.durable_routing_records().await;
    assert_eq!(records.len(), 1);
    let (request_id, origin_run) = match &records[0] {
        SessionModelRoutingControlRecord::ModelRoutingIntentRequested {
            request_id,
            originating_run_id,
            intent,
        } => {
            assert_eq!(intent.target_model.as_str(), MODEL_B);
            (*request_id, originating_run_id.clone())
        }
        other => panic!("expected one committed Requested record, got {other:?}"),
    };
    assert!(
        harness.origin_run_committed_a_boundary(&origin_run).await,
        "the authoring run must have an exact committed boundary receipt"
    );
    CommittedHandoffProof {
        request_id,
        origin_run,
        auth_binding,
    }
}

// ---------------------------------------------------------------------------
// The proof
// ---------------------------------------------------------------------------

async fn brain_swap_moves_the_next_provider_call_from_a_to_b(profile: PersistenceProfile) {
    let harness = build_harness(profile).await;
    let binding_before = harness.live_identity().await.auth_binding;
    assert_eq!(
        binding_before.as_ref(),
        Some(&expected_auth_binding()),
        "both models must be reachable through one configured binding"
    );

    // ---- Authoring run: the model requests the switch and keeps answering on
    // ---- A for the rest of that run.
    let authoring = harness.run_prompt("please switch after this turn").await;
    assert!(
        matches!(authoring, CompletionOutcome::Completed(ref run) if run.text == AUTHORING_TEXT),
        "the authoring turn must complete on the original model: {authoring:?}"
    );
    assert_eq!(
        harness.requested_models(),
        vec![MODEL_A.to_string(), MODEL_A.to_string()],
        "every provider call in the requesting run must stay on model A"
    );
    assert_eq!(
        harness.live_identity().await.model,
        MODEL_A,
        "staging must not move live routing"
    );
    assert_eq!(
        harness.durable_identity().await.model,
        MODEL_A,
        "staging must not move durable routing"
    );

    let records = harness.durable_routing_records().await;
    assert_eq!(
        records.len(),
        1,
        "exactly one durable handoff record must exist after the authoring run: {records:?}"
    );
    let (request_id, origin_run) = match &records[0] {
        SessionModelRoutingControlRecord::ModelRoutingIntentRequested {
            request_id,
            originating_run_id,
            intent,
        } => {
            assert_eq!(intent.target_model.as_str(), MODEL_B);
            assert!(matches!(intent.duration, SwitchTurnDuration::UntilChanged));
            assert!(matches!(intent.origin, SwitchTurnOrigin::Model { .. }));
            (*request_id, originating_run_id.clone())
        }
        other => panic!("the authoring run must commit a Requested record, got {other:?}"),
    };
    assert_ne!(
        request_id,
        SwitchTurnRequestId::new(uuid::Uuid::nil()),
        "the tool must mint a real request identity"
    );
    assert_eq!(
        records[0].disposition(),
        ModelRoutingIntentRecordDisposition::Requested,
        "the request must not be terminal before the next input is admitted"
    );
    assert!(
        records[0].disposition().is_awaiting_decision(),
        "a staged-then-committed request is the one waiting disposition"
    );
    assert!(
        harness.origin_run_committed_a_boundary(&origin_run).await,
        "the durable request must be bound to a run that really committed a boundary"
    );

    let awaiting = harness
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&harness.session_id)
        .await
        .expect("read committed handoffs awaiting decision");
    assert_eq!(
        awaiting.len(),
        1,
        "the committed request must be visible to the loop as pending: {awaiting:?}"
    );
    assert_eq!(awaiting[0].request_id, request_id);
    assert_eq!(awaiting[0].originating_run_id, origin_run);

    // ---- Next input: nothing below calls realization. The runtime loop's
    // ---- pre-dequeue pass must do it before this input reaches a provider.
    let next = harness.run_prompt("continue").await;
    assert!(
        matches!(next, CompletionOutcome::Completed(ref run) if run.text == NEXT_TURN_TEXT),
        "the next turn must complete: {next:?}"
    );
    assert_eq!(
        harness.requested_models(),
        vec![
            MODEL_A.to_string(),
            MODEL_A.to_string(),
            MODEL_B.to_string(),
        ],
        "the first and only provider call after the handoff must be on model B"
    );

    assert_eq!(
        harness.live_identity().await.model,
        MODEL_B,
        "live routing must be model B"
    );
    let durable_identity = harness.durable_identity().await;
    assert_eq!(durable_identity.model, MODEL_B, "durable routing must be B");
    assert_eq!(durable_identity.provider, Provider::OpenAI);
    assert_eq!(
        durable_identity.auth_binding, binding_before,
        "a model-only switch must not rebind credentials or accounts"
    );

    let records = harness.durable_routing_records().await;
    assert_eq!(
        records.len(),
        2,
        "the append-only log must hold exactly the request and its one terminal: {records:?}"
    );
    assert_eq!(
        records[0].disposition(),
        ModelRoutingIntentRecordDisposition::Requested,
        "the original request record must survive verbatim"
    );
    let terminal = records
        .iter()
        .rfind(|record| *record.request_id() == request_id)
        .expect("the request must retain a durable record");
    match terminal {
        SessionModelRoutingControlRecord::ModelRoutingIntentRealized {
            originating_run_id,
            applied_identity,
            ..
        } => {
            assert_eq!(
                originating_run_id, &origin_run,
                "the terminal record must stay bound to the originating run"
            );
            assert_eq!(applied_identity.model, MODEL_B);
            assert_eq!(applied_identity.auth_binding, binding_before);
        }
        other => panic!("the realized handoff must record a Realized terminal, got {other:?}"),
    }
    assert_eq!(
        terminal.disposition(),
        ModelRoutingIntentRecordDisposition::Realized
    );
    assert!(
        terminal.disposition().is_terminal(),
        "the request must be terminal after realization"
    );

    let awaiting = harness
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&harness.session_id)
        .await
        .expect("read committed handoffs awaiting decision");
    assert!(
        awaiting.is_empty(),
        "no handoff may still await a decision after realization: {awaiting:?}"
    );
}

#[tokio::test]
async fn whole_blob_brain_swap_moves_the_next_provider_call_from_a_to_b() {
    brain_swap_moves_the_next_provider_call_from_a_to_b(PersistenceProfile::WholeBlob).await;
}

#[tokio::test]
async fn head_canonical_brain_swap_moves_the_next_provider_call_from_a_to_b() {
    brain_swap_moves_the_next_provider_call_from_a_to_b(PersistenceProfile::HeadCanonical).await;
}

fn assert_exact_realized_terminal(
    records: &[SessionModelRoutingControlRecord],
    proof: &CommittedHandoffProof,
) {
    assert_eq!(
        records.len(),
        2,
        "one Requested and one terminal must be durable: {records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.request_id() == &proof.request_id)
            .count(),
        2,
        "the request must not be duplicated or rotated twice"
    );
    match &records[1] {
        SessionModelRoutingControlRecord::ModelRoutingIntentRealized {
            request_id,
            originating_run_id,
            applied_identity,
            ..
        } => {
            assert_eq!(request_id, &proof.request_id);
            assert_eq!(originating_run_id, &proof.origin_run);
            assert_eq!(applied_identity.model, MODEL_B);
            assert_eq!(applied_identity.auth_binding, proof.auth_binding);
        }
        other => panic!("expected a durable Realized terminal, got {other:?}"),
    }
}

async fn cold_restart_realizes_with_a_unavailable(profile: PersistenceProfile) {
    let harness = build_harness(profile).await;
    let proof = commit_brain_swap_request(&harness).await;
    harness.set_available(MODEL_A, false);
    harness.set_available(MODEL_B, true);
    harness.clear_client_build_models();

    let harness = harness.cold_restart().await;
    assert!(
        harness
            .client_build_models()
            .starts_with(&[MODEL_A.to_string(), MODEL_B.to_string()]),
        "cold materialization must observe A unavailable, then bootstrap from exact Requested B: {:?}",
        harness.client_build_models()
    );
    assert_eq!(
        harness.requested_models(),
        vec![MODEL_A.to_string(), MODEL_A.to_string()],
        "cold materialization may construct B but must not call a provider"
    );
    assert_eq!(
        harness.durable_identity().await.model,
        MODEL_A,
        "bootstrap must not mark the request realized or rewrite durable identity"
    );
    assert_eq!(harness.identity_application_count(), 0);
    let waiting = harness.durable_routing_records().await;
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].disposition(),
        ModelRoutingIntentRecordDisposition::Requested
    );

    let next = harness.run_prompt("continue after cold restart").await;
    assert!(
        matches!(next, CompletionOutcome::Completed(ref run) if run.text == NEXT_TURN_TEXT),
        "the retained next input must complete after pre-dequeue realizes B: {next:?}"
    );
    assert_eq!(
        harness.requested_models(),
        vec![
            MODEL_A.to_string(),
            MODEL_A.to_string(),
            MODEL_B.to_string(),
        ]
    );
    assert_eq!(
        harness.identity_application_count(),
        1,
        "the cold successor must apply B exactly once"
    );
    assert_eq!(harness.live_identity().await.model, MODEL_B);
    assert_exact_realized_terminal(&harness.durable_routing_records().await, &proof);
}

#[tokio::test]
async fn whole_blob_cold_restart_bootstraps_from_requested_b_when_a_is_unavailable() {
    cold_restart_realizes_with_a_unavailable(PersistenceProfile::WholeBlob).await;
}

#[tokio::test]
async fn head_canonical_cold_restart_bootstraps_from_requested_b_when_a_is_unavailable() {
    cold_restart_realizes_with_a_unavailable(PersistenceProfile::HeadCanonical).await;
}

async fn unavailable_b_holds_the_real_next_input(profile: PersistenceProfile) {
    let harness = build_harness(profile).await;
    let proof = commit_brain_swap_request(&harness).await;
    harness.set_available(MODEL_B, false);
    harness.clear_client_build_models();

    let (input_id, completion) = harness.accept_prompt("must remain unattempted").await;
    let outcome = AbHarness::wait_completion(completion).await;
    assert!(
        !matches!(outcome, CompletionOutcome::Completed(_)),
        "an unavailable target must not report a successful completion: {outcome:?}"
    );
    assert_eq!(
        harness.requested_models(),
        vec![MODEL_A.to_string(), MODEL_A.to_string()],
        "the held input must make zero provider calls"
    );
    assert!(
        harness
            .client_build_models()
            .iter()
            .any(|model| model == MODEL_B),
        "the real pre-dequeue path must test B through provider-runtime availability"
    );
    let stored = harness
        .runtime_store
        .load_input_state(&harness.runtime_id, &input_id)
        .await
        .expect("read the held input row")
        .expect("the held input must retain durable state");
    assert_eq!(
        stored.seed.attempt_count, 0,
        "pre-dequeue must hold before the input gains an execution attempt"
    );

    let records = harness.durable_routing_records().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].request_id(), &proof.request_id);
    assert_eq!(
        records[0].disposition(),
        ModelRoutingIntentRecordDisposition::Requested,
        "a retriable target-unavailable hold must leave Requested waiting"
    );
    assert_eq!(
        harness.identity_application_count(),
        0,
        "target preflight must fail before the live identity host is invoked"
    );
}

#[tokio::test]
async fn whole_blob_unavailable_b_holds_the_real_next_input() {
    unavailable_b_holds_the_real_next_input(PersistenceProfile::WholeBlob).await;
}

#[tokio::test]
async fn head_canonical_unavailable_b_holds_the_real_next_input() {
    unavailable_b_holds_the_real_next_input(PersistenceProfile::HeadCanonical).await;
}

async fn concurrent_next_admissions_realize_once_in_order(profile: PersistenceProfile) {
    let harness = build_harness(profile).await;
    let proof = commit_brain_swap_request(&harness).await;

    let turn_boundary = harness
        .service
        .acquire_runtime_turn_finalization_guard(&harness.session_id)
        .await;
    let (first_id, first_completion) = harness.accept_prompt("queued first").await;
    let (second_id, second_completion) = harness.accept_prompt("queued second").await;
    assert_ne!(first_id, second_id);
    assert_eq!(
        harness
            .adapter
            .committed_model_routing_handoffs_awaiting_decision(&harness.session_id)
            .await
            .expect("the request remains visible while admission is fenced")
            .len(),
        1,
        "both inputs must be admitted while the one committed request is pending"
    );
    drop(turn_boundary);

    let first = AbHarness::wait_completion(first_completion).await;
    let second = AbHarness::wait_completion(second_completion).await;
    assert!(
        matches!(first, CompletionOutcome::Completed(ref run) if run.text == NEXT_TURN_TEXT),
        "the first admitted input must receive the first B result: {first:?}"
    );
    assert!(
        matches!(second, CompletionOutcome::Completed(ref run) if run.text == SECOND_NEXT_TURN_TEXT),
        "the second admitted input must receive the second B result: {second:?}"
    );
    assert_eq!(
        harness.requested_models(),
        vec![
            MODEL_A.to_string(),
            MODEL_A.to_string(),
            MODEL_B.to_string(),
            MODEL_B.to_string(),
        ],
        "both queued calls must run on B in admission order"
    );
    assert_eq!(
        harness.identity_application_count(),
        1,
        "one pending request must invoke the reconfigure host exactly once"
    );

    let first_state = harness
        .runtime_store
        .load_input_state(&harness.runtime_id, &first_id)
        .await
        .expect("read first input state")
        .expect("first input state remains durable");
    let second_state = harness
        .runtime_store
        .load_input_state(&harness.runtime_id, &second_id)
        .await
        .expect("read second input state")
        .expect("second input state remains durable");
    assert_eq!(first_state.seed.attempt_count, 1);
    assert_eq!(second_state.seed.attempt_count, 1);
    assert!(
        first_state.seed.admission_sequence < second_state.seed.admission_sequence,
        "generated admission sequence must preserve input order"
    );

    let user_inputs = harness
        .authoritative_session()
        .await
        .messages()
        .iter()
        .filter_map(|message| match message {
            meerkat_core::Message::User(user) => Some(user.text_content()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        user_inputs.ends_with(&["queued first".to_string(), "queued second".to_string()]),
        "durable transcript order must match admission order: {user_inputs:?}"
    );
    assert_exact_realized_terminal(&harness.durable_routing_records().await, &proof);
}

#[tokio::test]
async fn whole_blob_concurrent_next_admissions_realize_once_in_order() {
    concurrent_next_admissions_realize_once_in_order(PersistenceProfile::WholeBlob).await;
}

#[tokio::test]
async fn head_canonical_concurrent_next_admissions_realize_once_in_order() {
    concurrent_next_admissions_realize_once_in_order(PersistenceProfile::HeadCanonical).await;
}
