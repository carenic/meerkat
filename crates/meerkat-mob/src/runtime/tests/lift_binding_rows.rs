#![cfg(all(feature = "runtime-adapter", not(target_arch = "wasm32")))]
//! The auth binding a member's live actor runs across the lift of a mob
//! stopped in this process. Stop keeps each member's live actor, so the
//! explicit resume keeps it only when it runs the binding the member's
//! resume request names; otherwise the member is rebuilt on that binding.
//!
//! Real turn-driven members over a real factory, a real SQLite session store
//! and a real head-canonical SQLite runtime store, so the kept actor, its
//! exact runtime attachment and the binding its session recorded are the
//! production ones.

use super::*;
use crate::machines::mob_machine as mob_dsl;

const WAIT: Duration = Duration::from_secs(30);

type Service = meerkat_session::PersistentSessionService<meerkat::FactoryAgentBuilder>;

/// Answers every turn with one fixed text block (no row runs a turn).
#[derive(Clone)]
struct FixedAnswerClient;

#[async_trait]
impl meerkat_client::LlmClient for FixedAnswerClient {
    fn project_replay_messages(
        &self,
        messages: &[Message],
    ) -> Result<Vec<Message>, meerkat_client::LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(
        &'a self,
        request: &'a meerkat_client::LlmRequest,
    ) -> meerkat_client::types::LlmStream<'a> {
        Box::pin(async_stream::try_stream! {
            yield meerkat_client::LlmEvent::TextDelta {
                delta: "lift-answer".to_string(),
                meta: None,
            };
            yield meerkat_client::LlmEvent::UsageUpdate {
                usage: meerkat_core::TurnUsage::host_declared(
                    Provider::OpenAI,
                    &request.model,
                    Usage::default(),
                ),
            };
            yield meerkat_client::LlmEvent::Done {
                outcome: meerkat_client::LlmDoneOutcome::success(meerkat_core::StopReason::EndTurn),
            };
        })
    }

    fn provider(&self) -> Provider {
        Provider::OpenAI
    }

    async fn health_check(&self) -> Result<(), meerkat_client::LlmError> {
        Ok(())
    }
}

/// A host customizer whose answer names the auth binding it holds now, so a
/// row can change it while the mob is stopped. Its resume answer is its
/// spawn answer (the trait's default `customize_resume`).
#[derive(Default)]
struct BindingCustomizer {
    auth_binding: Mutex<Option<meerkat_core::AuthBindingRef>>,
}

impl BindingCustomizer {
    fn name_binding(&self, binding: meerkat_core::AuthBindingRef) {
        *self.auth_binding.lock().expect("auth binding") = Some(binding);
    }
}

impl SpawnMemberCustomizer for BindingCustomizer {
    fn customize_spawn(
        &self,
        _ctx: &SpawnCustomizationContext,
        spec: &mut SpawnMemberSpec,
    ) -> Result<(), MobError> {
        if let Some(binding) = self.auth_binding.lock().expect("auth binding").clone() {
            spec.auth_binding = Some(binding);
        }
        Ok(())
    }
}

/// The binding the rows move a member to: the realm's configured OpenAI key.
fn configured_openai_binding() -> meerkat_core::AuthBindingRef {
    meerkat_core::AuthBindingRef {
        realm: meerkat_core::RealmId::global(),
        binding: meerkat_core::BindingId::parse("default_openai").expect("binding id"),
        profile: None,
        origin: meerkat_core::BindingOrigin::Configured,
    }
}

fn lift_definition(label: &str) -> MobDefinition {
    let mut definition = with_unique_mob_id(sample_definition(), label);
    let worker = definition
        .profiles
        .get_mut(&ProfileName::from("worker"))
        .and_then(ProfileBinding::as_inline_mut)
        .expect("inline worker");
    worker.model = "gpt-5.5".to_string();
    worker.runtime_mode = crate::MobRuntimeMode::TurnDriven;
    definition
}

struct LiftFixture {
    _root: tempfile::TempDir,
    handle: MobHandle,
    service: Arc<Service>,
    customizer: Arc<BindingCustomizer>,
}

impl LiftFixture {
    async fn new(label: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix("meerkat-lift-binding-")
            .tempdir()
            .expect("fixture storage in the system temp dir");
        let root_path = root.path().canonicalize().expect("absolute fixture root");
        for directory in ["config", "runtime", "project", "context"] {
            std::fs::create_dir_all(root_path.join(directory)).expect("fixture root");
        }
        let factory = meerkat::AgentFactory::new(root_path.join("factory-store"))
            .user_config_root(root_path.join("config"))
            .runtime_root(root_path.join("runtime"))
            .project_root(root_path.join("project"))
            .context_root(root_path.join("context"))
            .builtins(false)
            .comms(true);
        let mut config = meerkat::Config::default();
        config.agent.model = "gpt-5.5".to_string();
        config.compaction.auto_compact_threshold = 1_000_000;
        config.realm.insert(
            "global".to_string(),
            meerkat_core::RealmConfigSection::from_inline_api_keys(&[("openai", "test-key")]),
        );
        let mut builder = meerkat::FactoryAgentBuilder::new(factory, config);
        builder.default_llm_client = Some(Arc::new(FixedAnswerClient));
        let db = root_path.join("realm.db");
        let session_store =
            Arc::new(meerkat_store::SqliteSessionStore::open(&db).expect("session store"));
        builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
            meerkat_runtime::SqliteRuntimeStore::new_head_canonical(&db)
                .expect("head-canonical runtime store"),
        );
        let service = Arc::new(Service::new(
            builder,
            16,
            session_store,
            runtime_store,
            Arc::new(meerkat_store::MemoryBlobStore::new()),
        ));
        let customizer = Arc::new(BindingCustomizer::default());
        let handle = MobBuilder::new(lift_definition(label), MobStorage::in_memory())
            .with_session_service(service.clone())
            .with_default_llm_client(Arc::new(FixedAnswerClient))
            .with_spawn_member_customizer(customizer.clone())
            .create()
            .await
            .expect("create the mob");
        Self {
            _root: root,
            handle,
            service,
            customizer,
        }
    }

    /// Spawn one durable turn-driven worker and wait until it is Live.
    /// Identities are unique per test: comms routes are process-wide.
    async fn spawn(&self, label: &str) -> AgentIdentity {
        let identity = AgentIdentity::from(format!("{label}-{}", Uuid::new_v4().simple()));
        let mut spec = SpawnMemberSpec::new("worker", identity.clone());
        spec.continuity_intent = SpawnContinuityIntent::DurableIdentity {
            continuity_key: identity.to_string(),
        };
        tokio::time::timeout(WAIT, self.handle.spawn_spec(spec))
            .await
            .expect("the spawn answers")
            .expect("spawn the worker");
        self.wait_live(&identity).await;
        identity
    }

    async fn wait_live(&self, identity: &AgentIdentity) {
        let key = mob_dsl::AgentIdentity::from_domain(identity);
        let mut state = self.handle.machine_state_watch_rx.clone();
        let reached = tokio::time::timeout(
            WAIT,
            state.wait_for(|state| {
                state.member_materialization.get(&key)
                    == Some(&mob_dsl::MemberMaterializationPhase::Live)
            }),
        )
        .await;
        assert!(
            matches!(reached, Ok(Ok(_))),
            "member {identity} never became Live"
        );
    }

    async fn session(&self, identity: &AgentIdentity) -> SessionId {
        self.handle
            .resolve_bridge_session_id(identity)
            .await
            .expect("a session-bound member")
    }

    /// Whether `session`'s live actor runs `binding`, by the one check every
    /// live-actor reuse runs.
    async fn live_actor_runs(
        &self,
        identity: &AgentIdentity,
        session: &SessionId,
        binding: &meerkat_core::AuthBindingRef,
    ) -> bool {
        crate::runtime::live_actor_binding::verify_live_actor_auth_binding(
            self.service.as_ref(),
            identity,
            session,
            binding,
        )
        .await
        .is_ok()
    }

    async fn finish(self) {
        let _ = tokio::time::timeout(WAIT, self.handle.shutdown()).await;
    }
}

/// A host changes a member's auth binding while its mob is stopped in this
/// process. Stop kept the member's live actor on the old binding; the lift
/// does not keep it: the member is rebuilt on the binding its resume request
/// names, serves Live, and its session records that binding.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lift_after_the_binding_changed_rebuilds_the_member_on_the_new_binding() {
    let fixture = LiftFixture::new("lift-binding").await;
    let member = fixture.spawn("lift-binding-member").await;
    let session = fixture.session(&member).await;
    let binding = configured_openai_binding();
    assert!(
        !fixture.live_actor_runs(&member, &session, &binding).await,
        "precondition: the member was not built on the binding the lift names"
    );

    tokio::time::timeout(WAIT, fixture.handle.stop())
        .await
        .expect("the stop answers")
        .expect("stop the mob");
    assert!(
        MobSessionService::live_session_actor_registered(&*fixture.service, &session)
            .await
            .unwrap_or(false),
        "precondition: the stop kept the member's live actor"
    );
    fixture.customizer.name_binding(binding.clone());
    tokio::time::timeout(WAIT, fixture.handle.resume())
        .await
        .expect("the lift answers")
        .expect("lift the mob");

    fixture.wait_live(&member).await;
    assert_eq!(
        fixture.session(&member).await,
        session,
        "the member is rebuilt on its own session"
    );
    assert!(
        MobSessionService::live_session_actor_registered(&*fixture.service, &session)
            .await
            .unwrap_or(false),
        "the rebuilt member serves from a live actor"
    );
    assert!(
        fixture.live_actor_runs(&member, &session, &binding).await,
        "the member's live actor runs the binding its resume request names"
    );
    fixture.finish().await;
}
