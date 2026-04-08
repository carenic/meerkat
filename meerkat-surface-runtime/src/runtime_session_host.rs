//! Shared runtime-backed session host.
//!
//! Owns the common assembly used by runtime-backed surfaces:
//! persistent session service, runtime bindings provider, executor attachment,
//! persisted-session access, and transport-neutral peer-input admission.

use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};

use meerkat::{
    AgentFactory, Config, CreateSessionRequest, FactoryAgentBuilder, LlmClient, PersistenceBundle,
    PersistentSessionService, RunResult, Session, SessionError, SessionFilter, SessionId,
    SessionService, SessionStore, SessionStoreError, StartTurnRequest,
    surface::{
        SurfaceSessionRecoveryContext, SurfaceSessionRecoveryOverrides, build_recovered_session,
    },
};
use meerkat_core::BlobStore;
use meerkat_core::comms::{EventStream, StreamError};
use meerkat_core::error::AgentError;
#[cfg(feature = "comms")]
use meerkat_core::interaction::PeerInputCandidate;
use meerkat_core::service::MobToolsFactory;
#[cfg(feature = "comms")]
use meerkat_runtime::SessionServiceRuntimeExt;
#[cfg(feature = "comms")]
use meerkat_runtime::accept::AcceptOutcome;
#[cfg(feature = "comms")]
use meerkat_runtime::comms_bridge::peer_input_candidate_to_runtime_input;
#[cfg(feature = "comms")]
use meerkat_runtime::identifiers::LogicalRuntimeId;
use meerkat_runtime::session_adapter::RuntimeBindingsError;
use meerkat_runtime::{RuntimeDriverError, RuntimeSessionAdapter};
use meerkat_store::{MemoryBlobStore, StoreAdapter};

#[cfg(feature = "comms")]
use meerkat::CommsRuntime;
#[cfg(feature = "mob")]
use meerkat_mob_mcp::{AgentMobToolSurfaceFactory, MobMcpState};
#[cfg(not(target_arch = "wasm32"))]
use meerkat_store::JsonlStore;

use meerkat_core::lifecycle::core_executor::{CoreApplyOutput, CoreExecutor, CoreExecutorError};
use meerkat_core::lifecycle::run_control::RunControlCommand;
use meerkat_core::lifecycle::run_primitive::RunPrimitive;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeSessionHostError {
    #[error("runtime host requires explicit persistence when no session directory is configured")]
    MissingPersistence,
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Store(#[from] SessionStoreError),
    #[error(transparent)]
    RuntimeBindings(#[from] RuntimeBindingsError),
    #[error(transparent)]
    RuntimeDriver(#[from] RuntimeDriverError),
    #[error("persisted session not found: {0}")]
    PersistedSessionNotFound(SessionId),
    #[error("session service returned mismatched session id: expected {expected}, got {actual}")]
    SessionIdMismatch {
        expected: SessionId,
        actual: SessionId,
    },
}

pub struct RuntimeSessionHostBuilder {
    session_dir: Option<PathBuf>,
    factory: AgentFactory,
    config: Config,
    max_sessions: usize,
    persistence: Option<PersistenceBundle>,
    default_llm_client: Option<Arc<dyn LlmClient>>,
}

impl RuntimeSessionHostBuilder {
    pub fn new(session_dir: impl Into<PathBuf>) -> Self {
        let session_dir = session_dir.into();
        Self {
            factory: AgentFactory::new(session_dir.clone()),
            session_dir: Some(session_dir),
            config: Config::default(),
            max_sessions: 64,
            persistence: None,
            default_llm_client: None,
        }
    }

    pub fn from_factory(factory: AgentFactory) -> Self {
        Self {
            session_dir: None,
            factory,
            config: Config::default(),
            max_sessions: 64,
            persistence: None,
            default_llm_client: None,
        }
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn max_sessions(mut self, max_sessions: usize) -> Self {
        self.max_sessions = max_sessions;
        self
    }

    pub fn persistence(mut self, persistence: PersistenceBundle) -> Self {
        self.persistence = Some(persistence);
        self
    }

    pub fn default_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.default_llm_client = Some(llm_client);
        self
    }

    pub fn project_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.factory = self.factory.project_root(path);
        self
    }

    pub fn context_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.factory = self.factory.context_root(path);
        self
    }

    pub fn user_config_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.factory = self.factory.user_config_root(path);
        self
    }

    pub fn runtime_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.factory = self.factory.runtime_root(path);
        self
    }

    pub fn builtins(mut self, enabled: bool) -> Self {
        self.factory = self.factory.builtins(enabled);
        self
    }

    pub fn shell(mut self, enabled: bool) -> Self {
        self.factory = self.factory.shell(enabled);
        self
    }

    pub fn memory(mut self, enabled: bool) -> Self {
        self.factory = self.factory.memory(enabled);
        self
    }

    pub fn mob(mut self, enabled: bool) -> Self {
        self.factory = self.factory.mob(enabled);
        self
    }

    #[cfg(feature = "comms")]
    pub fn comms_runtime(mut self, runtime: Arc<CommsRuntime>) -> Self {
        self.factory = self.factory.with_comms_runtime(runtime);
        self
    }

    pub async fn build(self) -> Result<RuntimeSessionHost, RuntimeSessionHostError> {
        let persistence = match self.persistence {
            Some(persistence) => persistence,
            None => {
                let session_dir = self
                    .session_dir
                    .clone()
                    .ok_or(RuntimeSessionHostError::MissingPersistence)?;
                build_default_persistence(session_dir).await?
            }
        };

        let mut builder = FactoryAgentBuilder::new(
            self.factory.session_store(persistence.session_store()),
            self.config,
        );
        builder.default_llm_client = self.default_llm_client;

        Ok(RuntimeSessionHost::from_builder(
            builder,
            self.max_sessions,
            persistence,
        ))
    }
}

pub struct RuntimeSessionHost {
    config: Config,
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    persistence: PersistenceBundle,
    runtime_adapter: Arc<RuntimeSessionAdapter>,
    builder_mob_tools_slot: Arc<StdRwLock<Option<Arc<dyn MobToolsFactory>>>>,
    #[cfg(feature = "mob")]
    mob_state: StdRwLock<Arc<MobMcpState>>,
}

impl RuntimeSessionHost {
    pub fn builder(session_dir: impl Into<PathBuf>) -> RuntimeSessionHostBuilder {
        RuntimeSessionHostBuilder::new(session_dir)
    }

    pub fn from_factory(
        factory: AgentFactory,
        config: Config,
        max_sessions: usize,
        persistence: PersistenceBundle,
    ) -> Self {
        let builder =
            FactoryAgentBuilder::new(factory.session_store(persistence.session_store()), config);
        Self::from_builder(builder, max_sessions, persistence)
    }

    pub fn from_builder(
        mut builder: FactoryAgentBuilder,
        max_sessions: usize,
        persistence: PersistenceBundle,
    ) -> Self {
        let config = builder.config().clone();
        let runtime_adapter = persistence.runtime_adapter();
        if builder.default_session_store.is_none() {
            builder.default_session_store =
                Some(Arc::new(StoreAdapter::new(persistence.session_store())));
        }
        let builder_mob_tools_slot = Arc::clone(&builder.default_mob_tools);
        let service = Arc::new(build_persistent_service(
            builder,
            max_sessions,
            persistence.clone(),
            &runtime_adapter,
        ));

        #[cfg(feature = "mob")]
        let mob_state = {
            let mob_state = Arc::new(MobMcpState::new_with_runtime_adapter(
                service.clone(),
                Some(runtime_adapter.clone()),
            ));
            *builder_mob_tools_slot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(
                AgentMobToolSurfaceFactory::new(Arc::clone(&mob_state)),
            ));
            mob_state
        };

        Self {
            config,
            service,
            persistence,
            runtime_adapter,
            builder_mob_tools_slot,
            #[cfg(feature = "mob")]
            mob_state: StdRwLock::new(mob_state),
        }
    }

    pub fn service(&self) -> Arc<PersistentSessionService<FactoryAgentBuilder>> {
        self.service.clone()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn persistence(&self) -> PersistenceBundle {
        self.persistence.clone()
    }

    pub fn session_store(&self) -> Arc<dyn SessionStore> {
        self.persistence.session_store()
    }

    pub fn blob_store(&self) -> Arc<dyn BlobStore> {
        self.persistence.blob_store()
    }

    pub fn runtime_adapter(&self) -> Arc<RuntimeSessionAdapter> {
        self.runtime_adapter.clone()
    }

    pub fn builder_mob_tools_slot(&self) -> Arc<StdRwLock<Option<Arc<dyn MobToolsFactory>>>> {
        Arc::clone(&self.builder_mob_tools_slot)
    }

    #[cfg(feature = "mob")]
    pub fn mob_state(&self) -> Arc<MobMcpState> {
        self.mob_state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[cfg(feature = "mob")]
    pub fn set_mob_state(&self, mob_state: Arc<MobMcpState>) {
        *self
            .mob_state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::clone(&mob_state);
        *self
            .builder_mob_tools_slot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(
            AgentMobToolSurfaceFactory::new(Arc::clone(&mob_state)),
        ));
    }

    pub fn set_mob_tools(&self, factory: Arc<dyn MobToolsFactory>) {
        *self
            .builder_mob_tools_slot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(factory);
    }

    pub async fn list_persisted_sessions(
        &self,
        filter: SessionFilter,
    ) -> Result<Vec<meerkat_core::SessionMeta>, RuntimeSessionHostError> {
        Ok(self.session_store().list(filter).await?)
    }

    pub async fn load_persisted(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<Session>, RuntimeSessionHostError> {
        Ok(self.service.load_persisted(session_id).await?)
    }

    pub async fn materialize_persisted_session(
        &self,
        session_id: &SessionId,
        overrides: &SurfaceSessionRecoveryOverrides,
        mut recovery_context: SurfaceSessionRecoveryContext,
    ) -> Result<SessionId, RuntimeSessionHostError> {
        let session = self
            .load_persisted(session_id)
            .await?
            .ok_or_else(|| RuntimeSessionHostError::PersistedSessionNotFound(session_id.clone()))?;
        recovery_context.runtime_build_mode = None;
        let recovered = build_recovered_session(session.clone(), overrides, recovery_context)
            .map_err(|error| SessionError::Agent(AgentError::InternalError(error.to_string())))?;
        let materialized_id = self
            .materialize_session(session, recovered.into_deferred_create_request())
            .await?;
        Ok(materialized_id)
    }

    pub async fn subscribe_events(
        &self,
        session_id: &SessionId,
    ) -> Result<EventStream, StreamError> {
        self.service.subscribe_session_events(session_id).await
    }

    pub async fn discard_live_session(
        &self,
        session_id: &SessionId,
    ) -> Result<(), RuntimeSessionHostError> {
        Ok(self.service.discard_live_session(session_id).await?)
    }

    #[cfg(feature = "mob")]
    pub async fn discard_live_session_with_mob_cleanup(
        &self,
        session_id: &SessionId,
    ) -> Result<(), RuntimeSessionHostError> {
        let discard_result = self.service.discard_live_session(session_id).await;
        if let Err(error) = self
            .mob_state()
            .destroy_session_mobs(&session_id.to_string())
            .await
        {
            tracing::warn!(
                session_id = %session_id,
                error = %error,
                "failed to clean up session mobs after live-session discard"
            );
        }
        Ok(discard_result?)
    }

    pub async fn create_or_resume_session(
        &self,
        resume_id: Option<SessionId>,
        request: CreateSessionRequest,
    ) -> Result<SessionId, RuntimeSessionHostError> {
        let result = self
            .create_or_resume_session_with_result(resume_id, request)
            .await?;
        Ok(result.session_id)
    }

    pub async fn create_or_resume_session_with_result(
        &self,
        resume_id: Option<SessionId>,
        request: CreateSessionRequest,
    ) -> Result<RunResult, RuntimeSessionHostError> {
        let session = match resume_id {
            Some(session_id) => self.load_persisted(&session_id).await?.ok_or(
                RuntimeSessionHostError::PersistedSessionNotFound(session_id),
            )?,
            None => Session::new(),
        };
        let service = Arc::clone(&self.service);
        #[cfg(feature = "mob")]
        let mob_state = self.mob_state();
        self.materialize_session_with_result_inner(
            session,
            request,
            Some(move |session_id| {
                #[cfg(feature = "mob")]
                {
                    let executor: Box<dyn CoreExecutor> = Box::new(
                        RuntimeSessionHostExecutor::new(Arc::clone(&service), session_id)
                            .with_mob_state(Arc::clone(&mob_state)),
                    );
                    executor
                }
                #[cfg(not(feature = "mob"))]
                {
                    let executor: Box<dyn CoreExecutor> = Box::new(
                        RuntimeSessionHostExecutor::new(Arc::clone(&service), session_id),
                    );
                    executor
                }
            }),
        )
        .await
    }

    pub async fn create_or_resume_session_and_attach_executor<F>(
        &self,
        resume_id: Option<SessionId>,
        request: CreateSessionRequest,
        executor_factory: F,
    ) -> Result<SessionId, RuntimeSessionHostError>
    where
        F: FnOnce(SessionId) -> Box<dyn CoreExecutor>,
    {
        let result = self
            .create_or_resume_session_with_result_and_executor(resume_id, request, executor_factory)
            .await?;
        Ok(result.session_id)
    }

    pub async fn create_or_resume_session_with_result_and_executor<F>(
        &self,
        resume_id: Option<SessionId>,
        request: CreateSessionRequest,
        executor_factory: F,
    ) -> Result<RunResult, RuntimeSessionHostError>
    where
        F: FnOnce(SessionId) -> Box<dyn CoreExecutor>,
    {
        let session = match resume_id {
            Some(session_id) => self.load_persisted(&session_id).await?.ok_or(
                RuntimeSessionHostError::PersistedSessionNotFound(session_id),
            )?,
            None => Session::new(),
        };
        self.materialize_session_with_result_inner(session, request, Some(executor_factory))
            .await
    }

    pub async fn materialize_session(
        &self,
        session: Session,
        request: CreateSessionRequest,
    ) -> Result<SessionId, RuntimeSessionHostError> {
        let result = self
            .materialize_session_with_result(session, request)
            .await?;
        Ok(result.session_id)
    }

    pub async fn materialize_session_with_result(
        &self,
        session: Session,
        request: CreateSessionRequest,
    ) -> Result<RunResult, RuntimeSessionHostError> {
        self.materialize_session_with_result_inner::<fn(SessionId) -> Box<dyn CoreExecutor>>(
            session, request, None,
        )
        .await
    }

    pub async fn materialize_session_and_attach_executor<F>(
        &self,
        session: Session,
        request: CreateSessionRequest,
        executor_factory: F,
    ) -> Result<SessionId, RuntimeSessionHostError>
    where
        F: FnOnce(SessionId) -> Box<dyn CoreExecutor>,
    {
        let result = self
            .materialize_session_with_result_and_executor(session, request, executor_factory)
            .await?;
        Ok(result.session_id)
    }

    pub async fn materialize_session_with_result_and_executor<F>(
        &self,
        session: Session,
        request: CreateSessionRequest,
        executor_factory: F,
    ) -> Result<RunResult, RuntimeSessionHostError>
    where
        F: FnOnce(SessionId) -> Box<dyn CoreExecutor>,
    {
        self.materialize_session_with_result_inner(session, request, Some(executor_factory))
            .await
    }

    async fn materialize_session_with_result_inner<F>(
        &self,
        session: Session,
        mut request: CreateSessionRequest,
        executor_factory: Option<F>,
    ) -> Result<RunResult, RuntimeSessionHostError>
    where
        F: FnOnce(SessionId) -> Box<dyn CoreExecutor>,
    {
        let prepared_session_id = session.id().clone();
        let bindings = self
            .runtime_adapter
            .prepare_bindings(prepared_session_id.clone())
            .await?;

        let mut build = request.build.unwrap_or_default();
        build.resume_session = Some(session);
        build.runtime_build_mode = meerkat_core::RuntimeBuildMode::SessionOwned(bindings);
        request.build = Some(build);

        let result = match self.service.create_session(request).await {
            Ok(result) => result,
            Err(error) => {
                self.runtime_adapter
                    .unregister_session(&prepared_session_id)
                    .await;
                return Err(RuntimeSessionHostError::Session(error));
            }
        };

        if result.session_id != prepared_session_id {
            self.runtime_adapter
                .unregister_session(&prepared_session_id)
                .await;
            return Err(RuntimeSessionHostError::SessionIdMismatch {
                expected: prepared_session_id,
                actual: result.session_id,
            });
        }

        if let Some(executor_factory) = executor_factory {
            self.attach_executor(
                &result.session_id,
                executor_factory(result.session_id.clone()),
            )
            .await;
        }

        #[cfg(feature = "comms")]
        if let Err(error) = self
            .configure_materialized_peer_ingress(&result.session_id)
            .await
        {
            let _ = self.service.discard_live_session(&result.session_id).await;
            self.runtime_adapter
                .unregister_session(&result.session_id)
                .await;
            return Err(error);
        }

        Ok(result)
    }

    pub async fn attach_executor(&self, session_id: &SessionId, executor: Box<dyn CoreExecutor>) {
        self.runtime_adapter
            .ensure_session_with_executor(session_id.clone(), executor)
            .await;
    }

    pub async fn ensure_default_runtime_executor(
        &self,
        session_id: &SessionId,
    ) -> Result<(), RuntimeSessionHostError> {
        let session_exists = if let Some(session) = self.load_persisted(session_id).await? {
            !session_metadata_marks_archived(&session)
        } else {
            self.service.read(session_id).await.is_ok()
        };
        if !session_exists {
            return Err(RuntimeSessionHostError::PersistedSessionNotFound(
                session_id.clone(),
            ));
        }

        let service = Arc::clone(&self.service);
        #[cfg(feature = "mob")]
        let mob_state = self.mob_state();
        let executor: Box<dyn CoreExecutor> = {
            #[cfg(feature = "mob")]
            {
                Box::new(
                    RuntimeSessionHostExecutor::new(service, session_id.clone())
                        .with_mob_state(mob_state),
                )
            }
            #[cfg(not(feature = "mob"))]
            {
                Box::new(RuntimeSessionHostExecutor::new(service, session_id.clone()))
            }
        };
        self.attach_executor(session_id, executor).await;
        Ok(())
    }

    pub async fn configure_peer_ingress(&self, session_id: &SessionId, keep_alive: bool) -> bool {
        let comms_rt = self.service.comms_runtime(session_id).await;
        self.runtime_adapter
            .update_peer_ingress_context(session_id, keep_alive, comms_rt)
            .await
    }

    #[cfg(feature = "comms")]
    pub async fn inject_peer_input_candidate(
        &self,
        session_id: &SessionId,
        candidate: &PeerInputCandidate,
    ) -> Result<AcceptOutcome, RuntimeSessionHostError> {
        let runtime_id = LogicalRuntimeId::new(session_id.to_string());
        let input = peer_input_candidate_to_runtime_input(candidate, &runtime_id);
        Ok(self.runtime_adapter.accept_input(session_id, input).await?)
    }

    #[cfg(feature = "comms")]
    async fn configure_materialized_peer_ingress(
        &self,
        session_id: &SessionId,
    ) -> Result<(), RuntimeSessionHostError> {
        let keep_alive = self
            .service
            .load_persisted(session_id)
            .await?
            .ok_or_else(|| RuntimeSessionHostError::PersistedSessionNotFound(session_id.clone()))?
            .session_metadata()
            .is_some_and(|metadata| metadata.keep_alive);
        self.configure_peer_ingress(session_id, keep_alive).await;
        Ok(())
    }
}

struct RuntimeSessionHostExecutor {
    service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
    session_id: SessionId,
    #[cfg(feature = "mob")]
    mob_state: Option<Arc<MobMcpState>>,
}

impl RuntimeSessionHostExecutor {
    fn new(
        service: Arc<PersistentSessionService<FactoryAgentBuilder>>,
        session_id: SessionId,
    ) -> Self {
        Self {
            service,
            session_id,
            #[cfg(feature = "mob")]
            mob_state: None,
        }
    }

    #[cfg(feature = "mob")]
    fn with_mob_state(mut self, mob_state: Arc<MobMcpState>) -> Self {
        self.mob_state = Some(mob_state);
        self
    }
}

#[async_trait::async_trait]
impl CoreExecutor for RuntimeSessionHostExecutor {
    async fn apply(
        &mut self,
        run_id: meerkat_core::lifecycle::RunId,
        primitive: RunPrimitive,
    ) -> Result<CoreApplyOutput, CoreExecutorError> {
        let prompt = primitive.extract_content_input();
        let req = StartTurnRequest {
            prompt,
            system_prompt: None,
            render_metadata: None,
            handling_mode: meerkat_core::types::HandlingMode::Queue,
            event_tx: None,
            skill_references: primitive
                .turn_metadata()
                .and_then(|meta| meta.skill_references.clone()),
            flow_tool_overlay: primitive
                .turn_metadata()
                .and_then(|meta| meta.flow_tool_overlay.clone()),
            additional_instructions: primitive
                .turn_metadata()
                .and_then(|meta| meta.additional_instructions.clone()),
            execution_kind: primitive.turn_metadata().and_then(|m| m.execution_kind),
        };

        self.service
            .apply_runtime_turn(
                &self.session_id,
                run_id,
                req,
                primitive.apply_boundary(),
                primitive.contributing_input_ids().to_vec(),
            )
            .await
            .map_err(|error| CoreExecutorError::ApplyFailed {
                reason: error.to_string(),
            })
    }

    async fn control(&mut self, command: RunControlCommand) -> Result<(), CoreExecutorError> {
        match command {
            RunControlCommand::CancelCurrentRun { .. } => self
                .service
                .interrupt(&self.session_id)
                .await
                .map_err(|error| CoreExecutorError::ControlFailed {
                    reason: error.to_string(),
                }),
            RunControlCommand::StopRuntimeExecutor { .. } => {
                let discard_result = self.service.discard_live_session(&self.session_id).await;
                #[cfg(feature = "mob")]
                if let Some(mob_state) = &self.mob_state
                    && let Err(error) = mob_state
                        .destroy_session_mobs(&self.session_id.to_string())
                        .await
                {
                    tracing::warn!(
                        session_id = %self.session_id,
                        error = %error,
                        "failed to clean up session mobs after runtime executor stop"
                    );
                }
                match discard_result {
                    Ok(()) | Err(SessionError::NotFound { .. }) => Ok(()),
                    Err(error) => Err(CoreExecutorError::ControlFailed {
                        reason: error.to_string(),
                    }),
                }
            }
            _ => Ok(()),
        }
    }
}

fn build_persistent_service(
    builder: FactoryAgentBuilder,
    max_sessions: usize,
    persistence: PersistenceBundle,
    runtime_adapter: &Arc<RuntimeSessionAdapter>,
) -> PersistentSessionService<FactoryAgentBuilder> {
    let (store, runtime_store, blob_store) = persistence.into_parts();
    let mut service =
        PersistentSessionService::new(builder, max_sessions, store, runtime_store, blob_store);
    let adapter = runtime_adapter.clone();
    service.set_runtime_bindings_provider(Arc::new(move |session_id| {
        let adapter = adapter.clone();
        Box::pin(async move { adapter.prepare_bindings(session_id).await.ok() })
    }));
    service
}

fn session_metadata_marks_archived(session: &Session) -> bool {
    session
        .metadata()
        .get("session_archived")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
async fn build_default_persistence(
    session_dir: PathBuf,
) -> Result<PersistenceBundle, RuntimeSessionHostError> {
    let jsonl_store = Arc::new(JsonlStore::new(session_dir));
    jsonl_store
        .init()
        .await
        .map_err(|error| SessionStoreError::Internal(error.to_string()))?;
    Ok(PersistenceBundle::new(
        jsonl_store as Arc<dyn SessionStore>,
        None,
        Arc::new(MemoryBlobStore::new()),
    ))
}

#[cfg(target_arch = "wasm32")]
async fn build_default_persistence(
    _session_dir: PathBuf,
) -> Result<PersistenceBundle, RuntimeSessionHostError> {
    Err(RuntimeSessionHostError::MissingPersistence)
}

#[cfg(all(test, feature = "comms", not(target_arch = "wasm32")))]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use meerkat::CommsCommand;
    use meerkat_client::TestClient;
    use meerkat_core::SessionBuildOptions;
    use meerkat_core::agent::CommsRuntime as _;
    use meerkat_core::comms::{InputSource, TrustedPeerSpec};
    use meerkat_core::lifecycle::RunId;
    use meerkat_core::lifecycle::core_executor::{
        CoreApplyOutput, CoreExecutor, CoreExecutorError,
    };
    use meerkat_core::lifecycle::run_control::RunControlCommand;
    use meerkat_core::lifecycle::run_primitive::{RunApplyBoundary, RunPrimitive};
    use meerkat_core::lifecycle::run_receipt::RunBoundaryReceipt;
    use meerkat_core::types::HandlingMode;
    use meerkat_runtime::completion::CompletionOutcome;
    use meerkat_runtime::{Input, PromptInput};
    use tempfile::TempDir;
    use tokio::time::{Duration, Instant, sleep};

    fn make_request(build: SessionBuildOptions) -> CreateSessionRequest {
        CreateSessionRequest {
            model: "gpt-5.2".to_string(),
            prompt: meerkat_core::ContentInput::Text(String::new()),
            render_metadata: None,
            system_prompt: Some("runtime host regression".to_string()),
            max_tokens: None,
            event_tx: None,
            skill_references: None,
            initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
            build: Some(build),
            labels: None,
        }
    }

    async fn build_test_host(
        temp: &TempDir,
        shared_runtime: Option<Arc<CommsRuntime>>,
    ) -> RuntimeSessionHost {
        let builder = RuntimeSessionHost::builder(temp.path().join("sessions"))
            .default_llm_client(Arc::new(TestClient::default()))
            .max_sessions(4);
        let builder = if let Some(runtime) = shared_runtime {
            builder.comms_runtime(runtime)
        } else {
            builder
        };
        builder.build().await.expect("build runtime host")
    }

    fn make_inproc_runtime(name: &str) -> Arc<CommsRuntime> {
        Arc::new(CommsRuntime::inproc_only(name).expect("create inproc comms runtime"))
    }

    struct CountingExecutor {
        apply_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl CoreExecutor for CountingExecutor {
        async fn apply(
            &mut self,
            run_id: RunId,
            primitive: RunPrimitive,
        ) -> Result<CoreApplyOutput, CoreExecutorError> {
            self.apply_count.fetch_add(1, Ordering::SeqCst);
            Ok(CoreApplyOutput {
                receipt: RunBoundaryReceipt {
                    run_id,
                    boundary: RunApplyBoundary::RunStart,
                    contributing_input_ids: primitive.contributing_input_ids().to_vec(),
                    conversation_digest: None,
                    message_count: 0,
                    sequence: 0,
                },
                session_snapshot: None,
                terminal: None,
                run_result: None,
            })
        }

        async fn control(&mut self, _cmd: RunControlCommand) -> Result<(), CoreExecutorError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn keep_alive_without_comms_name_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let host = build_test_host(&temp, None).await;

        let error = host
            .materialize_session_with_result(
                Session::new(),
                make_request(SessionBuildOptions {
                    keep_alive: true,
                    ..Default::default()
                }),
            )
            .await
            .expect_err("keep_alive without comms_name must fail");

        assert!(
            error.to_string().contains("keep_alive requires comms_name"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn create_or_resume_session_uses_host_default_executor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let host = build_test_host(&temp, None).await;

        let session_id = host
            .create_or_resume_session(
                None,
                make_request(SessionBuildOptions {
                    comms_name: Some(format!("default-executor-{}", SessionId::new())),
                    keep_alive: true,
                    ..Default::default()
                }),
            )
            .await
            .expect("create session with host default executor");

        let (_outcome, handle) = host
            .runtime_adapter()
            .accept_input_with_completion(
                &session_id,
                Input::Prompt(PromptInput::new("host default executor prompt", None)),
            )
            .await
            .expect("accept prompt input");
        let handle = handle.expect("completion handle");
        let outcome = tokio::time::timeout(Duration::from_secs(2), handle.wait())
            .await
            .expect("prompt should complete");
        assert!(
            matches!(outcome, CompletionOutcome::Completed(ref run) if run.text == "ok"),
            "unexpected completion outcome: {outcome:?}"
        );

        host.runtime_adapter().abort_comms_drain(&session_id).await;
        host.runtime_adapter().wait_comms_drain(&session_id).await;
        host.discard_live_session(&session_id)
            .await
            .expect("discard live session");
        host.runtime_adapter().unregister_session(&session_id).await;
    }

    #[tokio::test]
    async fn materialize_session_with_executor_configures_peer_ingress() {
        let temp = tempfile::tempdir().expect("tempdir");
        let shared_runtime = make_inproc_runtime(&format!("surface-shared-{}", SessionId::new()));
        let host = build_test_host(&temp, Some(shared_runtime)).await;
        let apply_count = Arc::new(AtomicUsize::new(0));

        let result = host
            .materialize_session_with_result_and_executor(
                Session::new(),
                make_request(SessionBuildOptions::default()),
                {
                    let apply_count = Arc::clone(&apply_count);
                    move |_| {
                        Box::new(CountingExecutor {
                            apply_count: Arc::clone(&apply_count),
                        })
                    }
                },
            )
            .await
            .expect("materialize session");
        let session_id = result.session_id;

        let session_runtime = host
            .service()
            .comms_runtime(&session_id)
            .await
            .expect("session comms runtime");
        session_runtime
            .send(CommsCommand::Input {
                session_id: session_id.clone(),
                body: "ingress ping".to_string(),
                blocks: None,
                handling_mode: HandlingMode::Queue,
                source: InputSource::Rpc,
                allow_self_session: true,
            })
            .await
            .expect("inject comms input");

        let deadline = Instant::now() + Duration::from_secs(2);
        while apply_count.load(Ordering::SeqCst) == 0 {
            assert!(
                Instant::now() <= deadline,
                "host materialization should wire peer ingress before returning"
            );
            sleep(Duration::from_millis(25)).await;
        }

        host.runtime_adapter().abort_comms_drain(&session_id).await;
        host.runtime_adapter().wait_comms_drain(&session_id).await;
        host.discard_live_session(&session_id)
            .await
            .expect("discard live session");
        host.runtime_adapter().unregister_session(&session_id).await;
    }

    #[tokio::test]
    async fn materialize_session_does_not_copy_shared_runtime_trust() {
        let temp = tempfile::tempdir().expect("tempdir");
        let shared_runtime = make_inproc_runtime(&format!("surface-shared-{}", SessionId::new()));
        let trusted_name = format!("trusted-peer-{}", SessionId::new());
        let trusted_runtime = make_inproc_runtime(&trusted_name);
        shared_runtime
            .add_trusted_peer(
                TrustedPeerSpec::new(
                    trusted_name.clone(),
                    trusted_runtime.public_key().to_peer_id(),
                    format!("inproc://{trusted_name}"),
                )
                .expect("trusted peer spec"),
            )
            .await
            .expect("add trusted peer to shared runtime");
        let host = build_test_host(&temp, Some(shared_runtime.clone())).await;

        let shared_peer_names: Vec<_> = shared_runtime
            .peers()
            .await
            .into_iter()
            .map(|peer| peer.name.as_string())
            .collect();
        assert!(
            shared_peer_names.iter().any(|name| name == &trusted_name),
            "shared runtime should expose the explicitly trusted peer"
        );

        let session_id = host
            .materialize_session(
                Session::new(),
                make_request(SessionBuildOptions {
                    comms_name: Some(format!("session-peer-{}", SessionId::new())),
                    ..Default::default()
                }),
            )
            .await
            .expect("materialize session with explicit comms_name");
        let session_runtime = host
            .service()
            .comms_runtime(&session_id)
            .await
            .expect("session comms runtime");
        let session_peer_names: Vec<_> = session_runtime
            .peers()
            .await
            .into_iter()
            .map(|peer| peer.name.as_string())
            .collect();

        assert!(
            session_peer_names.iter().all(|name| name != &trusted_name),
            "session-scoped runtime must not inherit shared runtime trust entries"
        );

        host.discard_live_session(&session_id)
            .await
            .expect("discard live session");
        host.runtime_adapter().unregister_session(&session_id).await;
    }
}
