#![cfg(all(feature = "runtime-adapter", not(target_arch = "wasm32")))]

use super::*;
use meerkat_core::lifecycle::run_primitive::TurnRequestContext;
use meerkat_core::skills::{SkillKey, SkillName, SourceUuid};
use meerkat_core::types::TranscriptUserRole;
use meerkat_runtime::{LogicalRuntimeId, RuntimeStore};

// Shares this module's real-factory autonomous fixture.
#[path = "delivery_terminal_wait.rs"]
mod delivery_terminal_wait;

const WAIT: Duration = Duration::from_secs(30);
const HUMAN: &str = "Please compare these options.\nKeep my exact wording: café & <human>.";
const INJECTED: &str = "host-only retrieval context";
const TRANSIENT: &str = "request-only ephemeral context";
const SYSTEM: &str = "ordinary per-turn system instructions";

fn deadline() -> std::time::Instant {
    std::time::Instant::now() + WAIT
}

fn delivery(label: &str) -> MobDeliveryIdentity {
    MobDeliveryIdentity::new(label, Uuid::new_v4().to_string()).expect("stable delivery identity")
}

fn interaction(delivery: &MobDeliveryIdentity) -> InteractionId {
    InteractionId(Uuid::parse_str(&delivery.correlation_id).expect("correlation UUID"))
}

fn llm_identity(session: &Session) -> SessionLlmIdentity {
    session
        .try_session_metadata()
        .expect("valid typed session metadata")
        .expect("factory session has durable model metadata")
        .llm_identity()
}

#[derive(Clone)]
struct ScriptedClient {
    requests: Arc<Mutex<Vec<meerkat_client::LlmRequest>>>,
    released: tokio::sync::watch::Sender<bool>,
}

impl ScriptedClient {
    fn new(released: bool) -> Self {
        let (released, _) = tokio::sync::watch::channel(released);
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            released,
        }
    }

    fn requests(&self) -> Vec<meerkat_client::LlmRequest> {
        self.requests.lock().expect("recorded requests").clone()
    }

    fn block(&self) {
        self.released.send_replace(false);
    }

    fn release(&self) {
        self.released.send_replace(true);
    }

    async fn wait_for_requests(&self, count: usize) {
        tokio::time::timeout(WAIT, async {
            loop {
                if self.requests.lock().expect("recorded requests").len() >= count {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("actual LLM request must start");
    }
}

#[async_trait]
impl meerkat_client::LlmClient for ScriptedClient {
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
        let sequence = {
            let mut requests = self.requests.lock().expect("record actual request");
            requests.push(request.clone());
            requests.len()
        };
        let mut released = self.released.subscribe();
        Box::pin(async_stream::try_stream! {
            while !*released.borrow_and_update() {
                if released.changed().await.is_err() {
                    break;
                }
            }
            yield meerkat_client::LlmEvent::TextDelta {
                delta: format!("executor-answer-{sequence}"),
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

type Service = meerkat_session::PersistentSessionService<meerkat::FactoryAgentBuilder>;

#[cfg(feature = "openai-live")]
struct ContextHost {
    service: Arc<Service>,
    appends: Mutex<Vec<String>>,
}

#[cfg(feature = "openai-live")]
#[async_trait]
impl meerkat_runtime::live_context_mirror::LiveContextMirrorHost for ContextHost {
    async fn committed_boundary(
        &self,
        session_id: &SessionId,
    ) -> Result<
        (
            meerkat_core::lifecycle::core_executor::BoundSessionCommit,
            String,
        ),
        String,
    > {
        self.service
            .export_live_context_committed_boundary(session_id)
            .await
            .map_err(|error| error.to_string())
    }

    async fn append_context(
        &self,
        authority: meerkat_runtime::live_execution::LiveContextAppendAuthority,
        context: String,
    ) -> Result<
        (
            meerkat_runtime::live_execution::LiveContextAppendAuthority,
            meerkat_core::LiveAppendDeliveryOutcome,
        ),
        String,
    > {
        self.appends
            .lock()
            .expect("committed context")
            .push(context);
        Ok((
            authority,
            meerkat_core::LiveAppendDeliveryOutcome::Acknowledged,
        ))
    }

    async fn recover_ambiguous_append(
        &self,
        _authority: meerkat_runtime::live_execution::LiveContextAmbiguityRecoveryAuthority,
    ) -> Result<(), String> {
        panic!("acknowledged fixture context must not require recovery");
    }

    async fn recover_ambiguous_delegation_result(
        &self,
        _authority: meerkat_runtime::live_execution::LiveDelegationResultAmbiguityRecoveryAuthority,
    ) -> Result<(), String> {
        panic!("host human input is not a delegation result");
    }
}

struct Fixture {
    root: tempfile::TempDir,
    handle: MobHandle,
    service: Arc<Service>,
    runtime_store: Arc<dyn RuntimeStore>,
    entry: RosterEntry,
    session_id: SessionId,
    client: ScriptedClient,
    #[cfg(feature = "openai-live")]
    context: Arc<ContextHost>,
    #[cfg(feature = "openai-live")]
    binding: Option<meerkat_runtime::live_execution::LiveDelegationRuntimeBinding>,
}

impl Fixture {
    async fn new() -> Self {
        let mut fixture = Self::with_kickoff_blocked(false).await;
        fixture
            .handle
            .wait_for_kickoff_complete(Some(WAIT))
            .await
            .expect("real autonomous kickoff commits");
        #[cfg(feature = "openai-live")]
        {
            let (seed, _) = fixture
                .service
                .export_live_context_committed_boundary(&fixture.session_id)
                .await
                .expect("store-sealed autonomous seed");
            let cursor = seed
                .session()
                .expect("typed canonical seed")
                .messages()
                .len() as u64;
            let adapter = fixture
                .service
                .acquire_runtime_adapter(None)
                .expect("acquire runtime authority")
                .expect("runtime owner");
            adapter.set_live_context_mirror_host(fixture.context.clone());
            fixture.binding = Some(
                adapter
                    .__test_open_live_context_channel(&fixture.session_id, cursor)
                    .await
                    .expect("generated context channel"),
            );
        }
        // Without live support no field needs mutation, but the same fixture
        // still exercises the actual autonomous executor and durable store.
        #[cfg(not(feature = "openai-live"))]
        let _ = &mut fixture;
        fixture
    }

    async fn with_kickoff_blocked(blocked: bool) -> Self {
        Self::build(blocked, &[]).await
    }

    /// Settled fixture whose member agent has a native skill engine over one
    /// configured filesystem repository per `(source, skill, body)` entry.
    async fn with_skills(skills: &[(&SourceUuid, &SkillName, &str)]) -> Self {
        let fixture = Self::build(false, skills).await;
        fixture
            .handle
            .wait_for_kickoff_complete(Some(WAIT))
            .await
            .expect("real autonomous kickoff commits");
        fixture
    }

    async fn build(blocked: bool, skills: &[(&SourceUuid, &SkillName, &str)]) -> Self {
        // The system temp dir, not the source tree: a run killed before the
        // TempDir drops must not leave fixture state in the worktree.
        let root = tempfile::Builder::new()
            .prefix("meerkat-host-human-input-")
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
        for (index, (source_uuid, name, body)) in skills.iter().enumerate() {
            let repository = root_path.join(format!("skills-{index}"));
            let skill_dir = repository.join(name.as_str());
            std::fs::create_dir_all(&skill_dir).expect("skill fixture directory");
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: fixture skill\n---\n{body}\n"),
            )
            .expect("skill fixture document");
            config
                .skills
                .repositories
                .push(meerkat_core::skills_config::SkillRepositoryConfig {
                    name: format!("fixture-skills-{index}"),
                    source_uuid: (*source_uuid).clone(),
                    transport: meerkat_core::skills_config::SkillRepoTransport::Filesystem {
                        path: repository.to_string_lossy().into_owned(),
                    },
                });
        }
        let client = ScriptedClient::new(!blocked);
        let mut builder = meerkat::FactoryAgentBuilder::new(factory, config);
        // A second controlling Mob reuses this service without the local
        // actor override. Its peer-only bridge session must stay scripted too.
        builder.default_llm_client = Some(Arc::new(client.clone()));
        let db = root_path.join("realm.db");
        let session_store =
            Arc::new(meerkat_store::SqliteSessionStore::open(&db).expect("session store"));
        builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
            session_store.clone(),
        )));
        let runtime_store: Arc<dyn RuntimeStore> = Arc::new(
            meerkat_runtime::SqliteRuntimeStore::new_head_canonical(&db)
                .expect("co-tenant canonical runtime store"),
        );
        let service = Arc::new(Service::new(
            builder,
            16,
            session_store,
            runtime_store.clone(),
            Arc::new(meerkat_store::MemoryBlobStore::new()),
        ));
        let mut definition = with_unique_mob_id(sample_definition(), "host-human");
        let lead = definition
            .profiles
            .get_mut(&ProfileName::from("lead"))
            .and_then(ProfileBinding::as_inline_mut)
            .expect("inline lead");
        lead.model = "gpt-5.5".to_string();
        lead.runtime_mode = crate::MobRuntimeMode::AutonomousHost;
        lead.tools = ToolConfig {
            comms: true,
            ..ToolConfig::default()
        };
        let handle = MobBuilder::new(definition, MobStorage::in_memory())
            .with_session_service(service.clone())
            .with_default_llm_client(Arc::new(client.clone()))
            .create()
            .await
            .expect("create real autonomous mob");
        let identity = AgentIdentity::from("host-human-member");
        let session_id = handle
            .spawn(
                ProfileName::from("lead"),
                identity.clone(),
                Some("fixture kickoff".into()),
            )
            .await
            .expect("spawn autonomous member")
            .bridge_session_id()
            .expect("local session member")
            .clone();
        let entry = handle
            .get_member(&identity)
            .await
            .expect("roster read")
            .expect("member binding");
        assert_eq!(entry.runtime_mode, crate::MobRuntimeMode::AutonomousHost);
        Self {
            root,
            handle,
            #[cfg(feature = "openai-live")]
            context: Arc::new(ContextHost {
                service: service.clone(),
                appends: Mutex::new(Vec::new()),
            }),
            service,
            runtime_store,
            entry,
            session_id,
            client,
            #[cfg(feature = "openai-live")]
            binding: None,
        }
    }

    async fn start(
        &self,
        spec: WorkSpec,
        mode: HandlingMode,
        delivery: MobDeliveryIdentity,
    ) -> Result<WorkTurnHandle, MobError> {
        self.handle
            .start_host_human_input_bounded(
                self.entry.agent_runtime_id.clone(),
                self.entry.fence_token,
                spec,
                mode,
                delivery,
                deadline(),
            )
            .await
    }

    async fn start_with_options(
        &self,
        spec: WorkSpec,
        options: MemberTurnOptions,
        delivery: MobDeliveryIdentity,
    ) -> Result<WorkTurnHandle, MobError> {
        self.handle
            .start_host_human_input_with_options_bounded(
                self.entry.agent_runtime_id.clone(),
                self.entry.fence_token,
                spec,
                HandlingMode::Queue,
                options,
                delivery,
                deadline(),
            )
            .await
    }

    async fn submit_with_options(
        &self,
        spec: WorkSpec,
        options: MemberTurnOptions,
        delivery: MobDeliveryIdentity,
    ) -> Result<WorkDeliveryReceipt, MobError> {
        self.handle
            .submit_host_human_input_with_options_bounded(
                self.entry.agent_runtime_id.clone(),
                self.entry.fence_token,
                spec,
                HandlingMode::Queue,
                options,
                delivery,
                deadline(),
            )
            .await
    }

    async fn submit(
        &self,
        spec: WorkSpec,
        mode: HandlingMode,
        delivery: MobDeliveryIdentity,
    ) -> Result<WorkDeliveryReceipt, MobError> {
        self.handle
            .submit_host_human_input_bounded(
                self.entry.agent_runtime_id.clone(),
                self.entry.fence_token,
                spec,
                mode,
                delivery,
                deadline(),
            )
            .await
    }

    async fn durable(&self) -> Session {
        tokio::time::timeout(WAIT, self.service.load_persisted_session(&self.session_id))
            .await
            .expect("durable read finishes")
            .expect("canonical read succeeds")
            .expect("member is durable")
    }

    async fn input_count(&self) -> usize {
        self.runtime_store
            .load_input_states(&LogicalRuntimeId::for_session(&self.session_id))
            .await
            .expect("runtime-owned input rows")
            .len()
    }

    async fn input_for_delivery(
        &self,
        delivery: &MobDeliveryIdentity,
    ) -> meerkat_runtime::store::ExactInputStateObservation {
        self.runtime_store
            .load_input_state_by_idempotency_key(
                &LogicalRuntimeId::for_session(&self.session_id),
                &meerkat_runtime::identifiers::IdempotencyKey::new(
                    delivery.idempotency_key.clone(),
                ),
            )
            .await
            .expect("exact runtime-owned key lookup")
            .expect("delivery has a canonical input owner")
    }

    async fn submit_generic(
        &self,
        spec: WorkSpec,
        delivery: MobDeliveryIdentity,
    ) -> WorkDeliveryReceipt {
        let scope = self
            .handle
            .capture_member_delivery_scope(&self.entry.agent_identity)
            .await
            .expect("capture the member's delivery scope");
        self.handle
            .submit_work_with_mode_and_delivery_identity_bounded(
                &scope,
                spec,
                HandlingMode::Queue,
                delivery,
                deadline(),
            )
            .await
            .expect("generic ingress receipt is not an authorship or completion receipt")
    }

    #[cfg(feature = "openai-live")]
    async fn mirrored(&self) -> Vec<MirroredRow> {
        tokio::time::timeout(
            WAIT,
            self.service
                .acquire_runtime_adapter(None)
                .expect("acquire runtime authority")
                .expect("runtime owner")
                .drain_live_context_outbox(&self.session_id),
        )
        .await
        .expect("post-commit outbox drains")
        .expect("actual committed boundary reaches mirror");
        self.context
            .appends
            .lock()
            .expect("context appends")
            .iter()
            .map(|text| serde_json::from_str(text).expect("typed context row"))
            .collect()
    }

    /// Close the live context channel the fixture opened (with live
    /// support; nothing is open otherwise). The fixture composes no
    /// transport-neutral live cleanup host, so disposing of the member
    /// (retire, shutdown) while the channel is open is refused as a
    /// composition error: close it first.
    async fn close_channel(&mut self) {
        #[cfg(feature = "openai-live")]
        if let Some(binding) = self.binding.take() {
            self.service
                .acquire_runtime_adapter(None)
                .expect("acquire runtime authority")
                .expect("runtime owner")
                .__test_close_live_context_channel(&binding)
                .await
                .expect("close fixture channel without cancelling runtime work");
        }
    }

    async fn finish(mut self) {
        self.client.release();
        self.close_channel().await;
        tokio::time::timeout(WAIT, self.handle.shutdown())
            .await
            .expect("mob shutdown finishes")
            .expect("mob shutdown");
    }
}

async fn completed(turn: WorkTurnHandle) -> WorkDeliveryReceipt {
    tokio::time::timeout(WAIT, turn.wait())
        .await
        .expect("exact runtime completion arrives")
        .expect("exact runtime boundary commits")
}

fn assert_human(session: &Session, text: &str, id: InteractionId) {
    let users = session
        .messages()
        .iter()
        .filter_map(|message| match message {
            Message::User(user) if user.identity.interaction_id == Some(id) => Some(user),
            _ => None,
        })
        .filter(|user| user.transcript_role == TranscriptUserRole::Conversational)
        .collect::<Vec<_>>();
    assert_eq!(
        users.len(),
        1,
        "exactly one conversational human per delivery"
    );
    assert_eq!(users[0].text_content(), text);
    assert!(users[0].identity.realtime_origin.is_none());
    assert!(
        external_event_sources(session.messages(), text).is_empty(),
        "host authorship must not become an ExternalEvent notice"
    );
}

fn external_event_sources<'a>(messages: &'a [Message], text: &str) -> Vec<&'a str> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::SystemNotice(notice) => Some(notice),
            _ => None,
        })
        .flat_map(|notice| notice.blocks.iter())
        .filter_map(|block| match block {
            SystemNoticeBlock::ExternalEvent {
                source,
                body,
                content,
                ..
            } if body.as_deref() == Some(text)
                || meerkat_core::types::text_content(content) == text =>
            {
                Some(source.as_str())
            }
            _ => None,
        })
        .collect()
}

#[cfg(feature = "openai-live")]
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MirroredRow {
    role: MirrorRole,
    text: String,
}

#[cfg(feature = "openai-live")]
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum MirrorRole {
    User,
    Assistant,
}

#[cfg(feature = "openai-live")]
fn conversational_rows(messages: &[Message]) -> Vec<MirroredRow> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::User(user) if user.transcript_role.is_conversational() => Some(MirroredRow {
                role: MirrorRole::User,
                text: user.text_content(),
            }),
            Message::BlockAssistant(assistant) => Some(MirroredRow {
                role: MirrorRole::Assistant,
                text: assistant.to_string(),
            }),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn autonomous_queue_commits_exact_human_answer_and_shared_context() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let identity_before = llm_identity(&before);
    let requests_before = fixture.client.requests().len();
    let delivery = delivery("canonical-human");
    let id = interaction(&delivery);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External)
        .with_interaction_id(id)
        .with_system_prompt(SYSTEM)
        .with_injected_context(vec![INJECTED.into()]);
    let turn = fixture
        .start(spec, HandlingMode::Queue, delivery)
        .await
        .expect("host Queue supports separate System and injected-context slots");
    let result = tokio::time::timeout(
        WAIT,
        turn.wait_bounded(BoundedResultSpec::new("human answer", 1024).expect("bound")),
    )
    .await
    .expect("bounded completion")
    .expect("Queue owns its exact answer");
    assert_eq!(result.result().session_id(), &fixture.session_id);

    let durable = fixture.durable().await;
    assert_human(&durable, HUMAN, id);
    assert_eq!(llm_identity(&durable), identity_before);
    let appended = &durable.messages()[before.messages().len()..];
    let slots = appended
        .iter()
        .filter_map(|message| match message {
            Message::User(user) => Some((user.transcript_role, user.text_content())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        slots,
        [
            (TranscriptUserRole::InjectedContext, INJECTED.to_string()),
            (TranscriptUserRole::Conversational, HUMAN.to_string()),
        ]
    );
    assert!(
        appended
            .iter()
            .any(|message| matches!(message, Message::System(system) if system.content == SYSTEM))
    );
    let requests = fixture.client.requests();
    assert_eq!(requests.len(), requests_before + 1);
    let request = requests.last().expect("human request");
    assert_eq!(request.model, "gpt-5.5");
    assert!(
        request
            .messages
            .iter()
            .any(|message| matches!(message, Message::System(system) if system.content == SYSTEM))
    );
    assert!(
        request
            .messages
            .iter()
            .any(|message| matches!(message, Message::User(user)
            if user.transcript_role == TranscriptUserRole::InjectedContext
                && user.text_content() == INJECTED))
    );
    let answer = format!("executor-answer-{}", requests_before + 1);
    assert!(appended.iter().any(
        |message| matches!(message, Message::BlockAssistant(assistant)
            if assistant.to_string() == answer && assistant.identity.interaction_id == Some(id))
    ));
    assert_eq!(result.result().result().text(), answer);

    #[cfg(feature = "openai-live")]
    {
        let mirrored = fixture.mirrored().await;
        assert_eq!(mirrored, conversational_rows(appended));
        assert!(
            mirrored
                .iter()
                .all(|row| row.text != SYSTEM && row.text != INJECTED)
        );
    }
    let connection =
        rusqlite::Connection::open(fixture.root.path().join("realm.db")).expect("canonical DB");
    let (runtime_token, session_token): (String, String) = connection
        .query_row(
            "SELECT r.committed_head_token, s.cas_token \
             FROM runtime_session_authority r JOIN session_heads s USING (session_id) \
             WHERE r.session_id = ?1",
            [fixture.session_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("one exact store-owned canonical head");
    assert_eq!(runtime_token, session_token);
    drop(connection);
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn generic_autonomous_work_remains_external_event_not_mirrored_human() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let generic_delivery = delivery("generic-work");
    let id = interaction(&generic_delivery);
    let requests_before = fixture.client.requests().len();
    let scope = fixture
        .handle
        .capture_member_delivery_scope(&fixture.entry.agent_identity)
        .await
        .expect("capture the member's delivery scope");
    fixture
        .handle
        .submit_work_with_mode_and_delivery_identity_bounded(
            &scope,
            WorkSpec::new(HUMAN, WorkOrigin::External).with_interaction_id(id),
            HandlingMode::Queue,
            generic_delivery,
            deadline(),
        )
        .await
        .expect("generic autonomous ingress");
    fixture.client.wait_for_requests(requests_before + 1).await;
    // A real subsequent completion is an executor barrier, not a synthetic
    // event or a call that manually tells the mirror work has committed.
    let barrier = fixture
        .start(
            WorkSpec::new("generic settlement witness", WorkOrigin::Internal),
            HandlingMode::Queue,
            delivery("generic-settlement"),
        )
        .await
        .expect("settlement admission");
    completed(barrier).await;
    let durable = fixture.durable().await;
    let appended = &durable.messages()[before.messages().len()..];
    assert_eq!(
        external_event_sources(appended, HUMAN),
        ["rpc"],
        "generic transport provenance is unchanged"
    );
    assert!(
        !appended
            .iter()
            .any(|message| matches!(message, Message::User(user) if user.text_content() == HUMAN))
    );
    assert_eq!(llm_identity(&durable), llm_identity(&before));
    #[cfg(feature = "openai-live")]
    {
        let mirrored = fixture.mirrored().await;
        assert_eq!(mirrored, conversational_rows(appended));
        assert!(!mirrored.iter().any(|row| row.text == HUMAN));
    }
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_kind_generic_then_host_human_replay_refuses_reclassification() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let requests_before = fixture.client.requests().len();
    let key = delivery("generic-then-human");
    let id = interaction(&key);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External).with_interaction_id(id);
    fixture.submit_generic(spec.clone(), key.clone()).await;
    fixture.client.wait_for_requests(requests_before + 1).await;
    completed(
        fixture
            .start(
                WorkSpec::new("cross-kind settlement witness", WorkOrigin::Internal),
                HandlingMode::Queue,
                delivery("cross-kind-settlement"),
            )
            .await
            .expect("real executor completion settles the original external event"),
    )
    .await;
    let committed = fixture.durable().await;
    let original = fixture.input_for_delivery(&key).await;
    let inputs_before = fixture.input_count().await;
    let requests_before_replay = fixture.client.requests().len();
    assert!(original.state().state.prompt_replay_identity.is_none());
    assert!(
        original.state().state.persisted_input.is_none(),
        "cross-kind replay must still refuse after the original payload retires"
    );
    assert_eq!(
        external_event_sources(&committed.messages()[before.messages().len()..], HUMAN),
        ["rpc"]
    );
    #[cfg(feature = "openai-live")]
    let mirrored_before = fixture.mirrored().await;

    let error = fixture
        .submit(spec, HandlingMode::Queue, key.clone())
        .await
        .expect_err("a generic key is not proof of prior human admission");
    assert!(
        matches!(
            error,
            MobError::WorkInputIdempotencyConflict { session_id, input_id }
                if session_id == fixture.session_id
                    && input_id == original.state().state.input_id
        ),
        "strict host replay must return the original input's typed conflict"
    );
    let replayed = fixture.input_for_delivery(&key).await;
    assert_eq!(replayed.exact_row_digest(), original.exact_row_digest());
    assert_eq!(fixture.input_count().await, inputs_before);
    assert_eq!(fixture.client.requests().len(), requests_before_replay);
    let after = fixture.durable().await;
    assert_eq!(after.messages(), committed.messages());
    assert_eq!(llm_identity(&after), llm_identity(&before));
    assert!(
        !after
            .messages()
            .iter()
            .any(|message| matches!(message, Message::User(user) if user.text_content() == HUMAN))
    );
    #[cfg(feature = "openai-live")]
    {
        assert_eq!(fixture.mirrored().await, mirrored_before);
        assert!(!mirrored_before.iter().any(|row| row.text == HUMAN));
    }
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_kind_host_human_then_generic_replay_preserves_original_human_fate() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let key = delivery("human-then-generic");
    let id = interaction(&key);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External).with_interaction_id(id);
    let original_receipt = completed(
        fixture
            .start(spec.clone(), HandlingMode::Queue, key.clone())
            .await
            .expect("original human admission"),
    )
    .await;
    let committed = fixture.durable().await;
    assert_human(&committed, HUMAN, id);
    let original = fixture.input_for_delivery(&key).await;
    assert!(original.state().state.prompt_replay_identity.is_some());
    assert!(
        original.state().state.persisted_input.is_none(),
        "the typed prompt replay witness must outlive its payload"
    );
    let inputs_before = fixture.input_count().await;
    let requests_before = fixture.client.requests().len();
    #[cfg(feature = "openai-live")]
    let mirrored_before = fixture.mirrored().await;
    let comms = fixture
        .service
        .comms_runtime(&fixture.session_id)
        .await
        .expect("real autonomous ingress owner");
    let ingress_before = comms
        .peer_ingress_queue_snapshot()
        .await
        .expect("read ingress counters");

    let generic_receipt = fixture.submit_generic(spec, key.clone()).await;
    assert_eq!(generic_receipt.work_ref, original_receipt.work_ref);
    assert_eq!(generic_receipt.runtime_id, original_receipt.runtime_id);
    // Generic submission acknowledges the inbox, not runtime admission.
    // Its legacy key-only policy preserves the original human input rather
    // than reclassifying it as an ExternalEvent.
    // Observe the actual queue owner's terminal handover before asserting
    // absence of execution; otherwise a late duplicate could evade the test.
    let ingress_after = tokio::time::timeout(WAIT, async {
        loop {
            let snapshot = comms
                .peer_ingress_queue_snapshot()
                .await
                .expect("read actual generic handover");
            if snapshot.runtime_handover_count > ingress_before.runtime_handover_count {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("generic retry reaches authoritative runtime handover");
    assert_eq!(
        ingress_after.terminal_outcomes.deduplicated,
        ingress_before.terminal_outcomes.deduplicated + 1
    );
    assert_eq!(
        ingress_after.terminal_outcomes.accepted,
        ingress_before.terminal_outcomes.accepted
    );
    assert_eq!(
        ingress_after.terminal_outcomes.rejected,
        ingress_before.terminal_outcomes.rejected
    );
    let correlation = ingress_after
        .last_delivery_correlation
        .expect("exact runtime handover correlation");
    assert_eq!(
        correlation.outcome,
        meerkat_core::PeerIngressTerminalOutcomeKind::Deduplicated
    );
    assert_eq!(correlation.interaction_id, Some(id));
    assert_eq!(
        correlation.existing_runtime_input_id.as_ref(),
        Some(&original.state().state.input_id)
    );
    let replayed = fixture.input_for_delivery(&key).await;
    assert_eq!(replayed.exact_row_digest(), original.exact_row_digest());
    assert_eq!(fixture.input_count().await, inputs_before);
    assert_eq!(fixture.client.requests().len(), requests_before);
    let after = fixture.durable().await;
    assert_eq!(after.messages(), committed.messages());
    assert_human(&after, HUMAN, id);
    assert_eq!(llm_identity(&after), llm_identity(&before));
    #[cfg(feature = "openai-live")]
    {
        assert_eq!(fixture.mirrored().await, mirrored_before);
        assert_eq!(
            mirrored_before,
            conversational_rows(&committed.messages()[before.messages().len()..])
        );
    }
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_queue_admissions_return_before_llm_and_same_key_executes_once() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let requests_before = fixture.client.requests().len();
    fixture.client.block();
    let first_delivery = delivery("held-human");
    let first_id = interaction(&first_delivery);
    let first = fixture
        .start(
            WorkSpec::new("held first human", WorkOrigin::Internal),
            HandlingMode::Queue,
            first_delivery,
        )
        .await
        .expect("first admission");
    fixture.client.wait_for_requests(requests_before + 1).await;
    let second_delivery = delivery("queued-human");
    let second_id = interaction(&second_delivery);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let second = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.start(spec.clone(), HandlingMode::Queue, second_delivery.clone()),
    )
    .await
    .expect("active Queue admission must not wait for the blocked LLM")
    .expect("queued human accepted");
    let receipt = second.receipt().clone();
    let replay = fixture
        .handle
        .submit_host_human_input_for_identity_bounded(
            fixture.entry.agent_identity.clone(),
            spec.clone(),
            HandlingMode::Queue,
            second_delivery.clone(),
            deadline(),
        )
        .await
        .expect("identity-first same-key pending replay");
    assert_eq!(replay.work_ref, receipt.work_ref);
    assert_eq!(replay.runtime_id, receipt.runtime_id);
    assert_eq!(fixture.client.requests().len(), requests_before + 1);
    #[cfg(feature = "openai-live")]
    assert!(fixture.context.appends.lock().expect("context").is_empty());

    fixture.client.release();
    completed(first).await;
    let bound = BoundedResultSpec::new("original queued answer", 1024).expect("result bound");
    let original_answer = tokio::time::timeout(WAIT, second.wait_bounded(bound.clone()))
        .await
        .expect("original queued completion")
        .expect("original queued answer");
    let replay = fixture
        .submit(spec.clone(), HandlingMode::Queue, second_delivery.clone())
        .await
        .expect("same-key committed replay");
    assert_eq!(replay.work_ref, receipt.work_ref);
    assert_eq!(replay.runtime_id, receipt.runtime_id);
    let replay = fixture
        .start(spec, HandlingMode::Queue, second_delivery)
        .await
        .expect("completion-bearing terminal replay admission");
    let replayed_answer = tokio::time::timeout(WAIT, replay.wait_bounded(bound))
        .await
        .expect("terminal replay resolves")
        .expect("terminal replay retains its original answer, not CompletedWithoutResult");
    assert_eq!(replayed_answer.result(), original_answer.result());
    assert_eq!(replayed_answer.receipt().work_ref, receipt.work_ref);
    completed(
        fixture
            .start(
                WorkSpec::new("replay settlement witness", WorkOrigin::Internal),
                HandlingMode::Queue,
                delivery("replay-settlement"),
            )
            .await
            .expect("settlement barrier"),
    )
    .await;
    assert_eq!(fixture.client.requests().len(), requests_before + 3);
    let durable = fixture.durable().await;
    assert_human(&durable, "held first human", first_id);
    assert_human(&durable, HUMAN, second_id);
    assert_eq!(llm_identity(&durable), llm_identity(&before));
    #[cfg(feature = "openai-live")]
    assert_eq!(
        fixture.mirrored().await,
        conversational_rows(&durable.messages()[before.messages().len()..])
    );
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_queued_human_terminal_replay_cannot_become_success() {
    use meerkat_runtime::input_state::{
        InputAbandonReason, InputLifecycleState, InputTerminalOutcome,
    };

    const CANCELLATION: &str = "host cancelled this exact queued human";
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let requests_before = fixture.client.requests().len();
    fixture.client.block();
    let active = fixture
        .start(
            WorkSpec::new("uncancelled active work", WorkOrigin::Internal),
            HandlingMode::Queue,
            delivery("cancel-active"),
        )
        .await
        .expect("active predecessor");
    fixture.client.wait_for_requests(requests_before + 1).await;
    let key = delivery("cancel-queued-human");
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let queued = fixture
        .start(spec.clone(), HandlingMode::Queue, key.clone())
        .await
        .expect("queued human admission");
    let admitted = fixture.input_for_delivery(&key).await;
    assert_eq!(admitted.state().seed.phase, InputLifecycleState::Queued);
    assert!(
        tokio::time::timeout(
            WAIT,
            fixture
                .service
                .acquire_runtime_adapter(None)
                .expect("acquire runtime authority")
                .expect("runtime cancellation owner")
                .cancel_input_if_present(
                    &fixture.session_id,
                    &admitted.state().state.input_id,
                    CANCELLATION,
                ),
        )
        .await
        .expect("exact queued cancellation settles")
        .expect("cancel through runtime authority")
    );
    let cancelled = fixture.input_for_delivery(&key).await;
    assert!(matches!(
        cancelled.state().seed.terminal_outcome,
        Some(InputTerminalOutcome::Abandoned {
            reason: InputAbandonReason::Cancelled,
        })
    ));
    assert!(cancelled.state().state.persisted_input.is_none());
    assert!(cancelled.state().state.prompt_replay_identity.is_some());

    let original_failure = tokio::time::timeout(
        WAIT,
        queued.wait_bounded(BoundedResultSpec::new("cancelled human", 1024).expect("bound")),
    )
    .await
    .expect("original cancellation completion")
    .expect_err("queued cancellation cannot succeed");
    assert!(matches!(
        original_failure.failure(),
        BoundedTurnFailure::RuntimeTerminated { session_id, reason, .. }
            if session_id == &fixture.session_id && reason == CANCELLATION
    ));
    let replay = fixture
        .start(spec, HandlingMode::Queue, key.clone())
        .await
        .expect("same-key replay observes the known durable terminal");
    let replay_failure = tokio::time::timeout(
        WAIT,
        replay.wait_bounded(BoundedResultSpec::new("cancelled human", 1024).expect("bound")),
    )
    .await
    .expect("cancelled terminal replay resolves")
    .expect_err("replay must not fabricate CompletedWithoutResult success");
    assert!(matches!(
        replay_failure.failure(),
        BoundedTurnFailure::RuntimeTerminated { session_id, reason, .. }
            if session_id == &fixture.session_id && reason == CANCELLATION
    ));
    assert_eq!(
        fixture.input_for_delivery(&key).await.exact_row_digest(),
        cancelled.exact_row_digest()
    );
    assert_eq!(fixture.client.requests().len(), requests_before + 1);
    fixture.client.release();
    completed(active).await;
    completed(
        fixture
            .start(
                WorkSpec::new("cancellation settlement witness", WorkOrigin::Internal),
                HandlingMode::Queue,
                delivery("cancel-settlement"),
            )
            .await
            .expect("subsequent work remains usable"),
    )
    .await;
    let after = fixture.durable().await;
    assert_eq!(fixture.client.requests().len(), requests_before + 2);
    assert!(
        !after
            .messages()
            .iter()
            .any(|message| matches!(message, Message::User(user) if user.text_content() == HUMAN))
    );
    assert_eq!(llm_identity(&after), llm_identity(&before));
    #[cfg(feature = "openai-live")]
    assert_eq!(
        fixture.mirrored().await,
        conversational_rows(&after.messages()[before.messages().len()..])
    );
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_external_host_human_cannot_autospawn_through_enabled_policy() {
    let fixture = Fixture::new().await;
    fixture
        .handle
        .set_spawn_policy(Some(Arc::new(StaticLeadSpawnPolicy)))
        .await
        .expect("install real external auto-spawn policy");
    assert!(
        fixture
            .handle
            .query_machine_state()
            .await
            .expect("policy authority")
            .spawn_policy_enabled
    );
    let missing = AgentIdentity::from("missing-host-human");
    let runtime_id = AgentRuntimeId::initial(missing.clone());
    let requests_before = fixture.client.requests().len();
    let before = fixture.durable().await;
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let bound_error = fixture
        .handle
        .submit_host_human_input_bounded(
            runtime_id.clone(),
            FenceToken::new(0),
            spec.clone(),
            HandlingMode::Queue,
            delivery("missing-bound"),
            deadline(),
        )
        .await
        .expect_err("bound host input must not auto-provision a missing external target");
    assert!(matches!(bound_error, MobError::MemberNotFound(identity) if identity == missing));
    let start_error = fixture
        .handle
        .start_host_human_input_bounded(
            runtime_id,
            FenceToken::new(0),
            spec.clone(),
            HandlingMode::Queue,
            delivery("missing-start"),
            deadline(),
        )
        .await
        .expect_err("completion-bearing host input cannot retarget a missing binding");
    assert!(matches!(start_error, MobError::MemberNotFound(identity) if identity == missing));
    let identity_error = fixture
        .handle
        .submit_host_human_input_for_identity_bounded(
            missing.clone(),
            spec,
            HandlingMode::Queue,
            delivery("missing-identity"),
            deadline(),
        )
        .await
        .expect_err("identity-first host input must also refuse absence");
    assert!(matches!(identity_error, MobError::MemberNotFound(identity) if identity == missing));
    assert!(
        fixture
            .handle
            .get_member(&missing)
            .await
            .expect("roster observation after rejection")
            .is_none()
    );
    assert_eq!(fixture.client.requests().len(), requests_before);
    assert_eq!(fixture.durable().await.messages(), before.messages());
    #[cfg(feature = "openai-live")]
    assert!(fixture.mirrored().await.is_empty());
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_steer_commits_human_and_runtime_owned_answer() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let requests_before = fixture.client.requests().len();
    fixture.client.block();
    let active = fixture
        .start(
            WorkSpec::new("active executor work", WorkOrigin::Internal),
            HandlingMode::Queue,
            delivery("active-for-steer"),
        )
        .await
        .expect("active turn");
    fixture.client.wait_for_requests(requests_before + 1).await;
    let delivery = delivery("steered-human");
    let id = interaction(&delivery);
    let steer = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.start(
            WorkSpec::new(HUMAN, WorkOrigin::External).with_interaction_id(id),
            HandlingMode::Steer,
            delivery.clone(),
        ),
    )
    .await
    .expect("Steer admission returns before the LLM")
    .expect("active human Steer admission");
    let admitted = fixture.input_for_delivery(&delivery).await;
    let semantics = admitted
        .state()
        .state
        .runtime_semantics
        .as_ref()
        .expect("generated runtime admission semantics");
    assert_eq!(
        semantics.boundary(),
        meerkat_core::lifecycle::run_primitive::RunApplyBoundary::RunCheckpoint
    );
    assert_eq!(
        semantics.execution_kind(),
        meerkat_core::lifecycle::RuntimeExecutionKind::ContentTurn
    );
    assert_eq!(
        admitted
            .state()
            .state
            .persisted_input
            .as_ref()
            .expect("active Steer retains its admitted input")
            .handling_mode(),
        Some(HandlingMode::Steer)
    );
    assert_eq!(
        admitted
            .state()
            .state
            .policy
            .as_ref()
            .expect("generated admission policy")
            .decision
            .routing_disposition,
        meerkat_runtime::policy::RoutingDisposition::Steer
    );
    fixture.client.release();
    completed(active).await;
    let result = tokio::time::timeout(
        WAIT,
        steer.wait_bounded(BoundedResultSpec::new("steer", 1024).expect("bound")),
    )
    .await
    .expect("runtime-owned Steer terminal")
    .expect("generated ContentTurn Steer owns its actual committed answer");
    assert_eq!(result.result().session_id(), &fixture.session_id);
    let durable = fixture.durable().await;
    assert_human(&durable, HUMAN, id);
    assert_eq!(llm_identity(&durable), llm_identity(&before));
    let requests = fixture.client.requests();
    let answered_request = requests
        .iter()
        .rposition(|request| {
            request.messages.iter().any(
                |message| matches!(message, Message::User(user) if user.text_content() == HUMAN),
            )
        })
        .expect("the real executor requested an answer with the steered human");
    assert!(answered_request >= requests_before);
    assert_eq!(
        result.result().result().text(),
        format!("executor-answer-{}", answered_request + 1)
    );
    assert!(durable.messages()[before.messages().len()..].iter().any(
        |message| matches!(message, Message::BlockAssistant(assistant)
                if assistant.to_string() == result.result().result().text())
    ));
    #[cfg(feature = "openai-live")]
    assert_eq!(
        fixture.mirrored().await,
        conversational_rows(&durable.messages()[before.messages().len()..])
    );
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsupported_host_context_is_refused_before_runtime_admission() {
    let fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let input_count = fixture.input_count().await;
    let request_count = fixture.client.requests().len();
    let specs = [
        (
            HandlingMode::Steer,
            WorkSpec::new(HUMAN, WorkOrigin::External).with_injected_context(vec![INJECTED.into()]),
        ),
        (
            HandlingMode::Steer,
            WorkSpec::new(HUMAN, WorkOrigin::External)
                .with_transient_turn_context(TurnRequestContext::new(TRANSIENT).expect("context")),
        ),
        (
            HandlingMode::Queue,
            WorkSpec::new(HUMAN, WorkOrigin::External)
                .with_transient_turn_context(TurnRequestContext::new(TRANSIENT).expect("context")),
        ),
        (
            HandlingMode::Steer,
            WorkSpec::new(HUMAN, WorkOrigin::External).with_system_prompt(SYSTEM),
        ),
    ];
    for (index, (mode, spec)) in specs.into_iter().enumerate() {
        let error = fixture
            .submit(spec, mode, delivery(&format!("refused-{index}")))
            .await
            .expect_err("host input must reject unsupported context before admission");
        if index == 0 {
            assert!(matches!(
                error,
                MobError::InjectedContextUndeliverable { member_id, .. }
                    if member_id == fixture.entry.agent_identity
            ));
        } else {
            assert!(matches!(
                error,
                MobError::UnsupportedForMode {
                    mode: crate::MobRuntimeMode::AutonomousHost,
                    ..
                }
            ));
        }
        assert_eq!(fixture.input_count().await, input_count);
        assert_eq!(fixture.client.requests().len(), request_count);
        assert_eq!(fixture.durable().await.messages(), before.messages());
    }
    #[cfg(feature = "openai-live")]
    assert!(fixture.mirrored().await.is_empty());
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unresolved_kickoff_rejects_host_steer_instead_of_falling_back_to_queue() {
    let fixture = Fixture::with_kickoff_blocked(true).await;
    fixture.client.wait_for_requests(1).await;
    let inputs_before = fixture.input_count().await;
    let error = fixture
        .submit(
            WorkSpec::new(HUMAN, WorkOrigin::External),
            HandlingMode::Steer,
            delivery("unresolved-kickoff"),
        )
        .await
        .expect_err("human Steer cannot silently become Queue");
    assert!(matches!(
        error,
        MobError::UnsupportedForMode {
            mode: crate::MobRuntimeMode::AutonomousHost,
            ..
        }
    ));
    assert_eq!(fixture.input_count().await, inputs_before);
    fixture.client.release();
    fixture
        .handle
        .wait_for_kickoff_complete(Some(WAIT))
        .await
        .expect("kickoff completes normally");
    completed(
        fixture
            .start(
                WorkSpec::new("kickoff settlement witness", WorkOrigin::Internal),
                HandlingMode::Queue,
                delivery("kickoff-settlement"),
            )
            .await
            .expect("later Queue remains usable"),
    )
    .await;
    assert_eq!(fixture.client.requests().len(), 2);
    assert!(
        !fixture
            .durable()
            .await
            .messages()
            .iter()
            .any(|message| matches!(message, Message::User(user) if user.text_content() == HUMAN))
    );
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bound_host_input_refuses_wrong_fence_and_runtime_without_retargeting() {
    let fixture = Fixture::new().await;
    let input_count = fixture.input_count().await;
    let before = fixture.durable().await;
    let wrong_fence = FenceToken::new(fixture.entry.fence_token.get() + 1);
    let error = fixture
        .handle
        .submit_host_human_input_bounded(
            fixture.entry.agent_runtime_id.clone(),
            wrong_fence,
            WorkSpec::new(HUMAN, WorkOrigin::External),
            HandlingMode::Queue,
            delivery("wrong-fence"),
            deadline(),
        )
        .await
        .expect_err("stale host lease must not be reacquired");
    assert!(matches!(
        error,
        MobError::StaleFenceToken { runtime_id, expected, actual }
            if runtime_id == fixture.entry.agent_runtime_id
                && expected == fixture.entry.fence_token && actual == wrong_fence
    ));
    let wrong_runtime = AgentRuntimeId::new(
        fixture.entry.agent_identity.clone(),
        crate::ids::Generation::new(fixture.entry.generation.get() + 1),
    );
    let error = fixture
        .handle
        .submit_host_human_input_bounded(
            wrong_runtime.clone(),
            fixture.entry.fence_token,
            WorkSpec::new(HUMAN, WorkOrigin::External),
            HandlingMode::Queue,
            delivery("wrong-runtime"),
            deadline(),
        )
        .await
        .expect_err("a runtime incarnation is not silently retargeted");
    assert!(matches!(
        error,
        MobError::StaleFenceToken { runtime_id, expected, actual }
            if runtime_id == wrong_runtime
                && expected == fixture.entry.fence_token && actual == fixture.entry.fence_token
    ));
    assert_eq!(fixture.input_count().await, input_count);
    assert_eq!(fixture.durable().await.messages(), before.messages());
    #[cfg(feature = "openai-live")]
    assert!(fixture.mirrored().await.is_empty());
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_deadline_reports_typed_fate_and_same_identity_can_be_retried() {
    let fixture = Fixture::new().await;
    let delivery = delivery("expired-observation");
    let id = interaction(&delivery);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let before = fixture.client.requests().len();
    let error = fixture
        .handle
        .submit_host_human_input_bounded(
            fixture.entry.agent_runtime_id.clone(),
            fixture.entry.fence_token,
            spec.clone(),
            HandlingMode::Queue,
            delivery.clone(),
            std::time::Instant::now(),
        )
        .await
        .expect_err("elapsed observation budget");
    assert!(matches!(
        error,
        MobError::ActorCommandTimedOut {
            command_kind: "SubmitWork",
            ..
        }
    ));
    let data = error.structured_data().expect("typed timeout data");
    assert!(data.get("executed").is_none());
    assert!(data.get("retryable").is_none());
    completed(
        fixture
            .start(spec, HandlingMode::Queue, delivery)
            .await
            .expect("retry preserves the exact delivery identity"),
    )
    .await;
    assert_human(&fixture.durable().await, HUMAN, id);
    assert_eq!(fixture.client.requests().len(), before + 1);
    fixture.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_and_peer_only_members_refuse_host_human_before_transport_delivery() {
    use super::placement_support as support;

    let service = Arc::new(MockSessionService::new());
    service.set_runtime_adapter(Arc::new(meerkat_runtime::MeerkatMachine::ephemeral()));
    let mob_id = MobId::from(format!("host-human-remote-{}", Uuid::new_v4()));
    let mut definition = support::controlling_mob_definition(mob_id.clone());
    for profile in definition.profiles.values_mut() {
        if let Some(profile) = profile.as_inline_mut() {
            profile.model = "gpt-5.5".to_string();
        }
    }
    let handle = MobBuilder::new(definition, MobStorage::in_memory())
        .with_session_service(service)
        .with_owner_bridge_session_create_authority(SessionId::new(), false, false)
        .with_spawn_base_prompt_source(Arc::new(crate::StaticSpawnBasePromptSource(
            "Remote host-human refusal fixture".to_string(),
        )))
        .create()
        .await
        .expect("controlling mob without a non-portable client override");
    let host = support::spawn_scripted_host_peer(&format!("{mob_id}-host")).await;
    let endpoint =
        Arc::new(support::spawn_peer_comms_endpoint(&format!("{mob_id}-remote"), true, None).await);
    let responder = support::spawn_scripted_member_turn_responder(endpoint.clone());
    host.script_member_identity("remote", support::member_identity_of(&endpoint));
    host.bind_member_endpoint("remote", endpoint);
    let report = handle
        .bind_host(support::descriptor_to_bind_request(&host.descriptor))
        .await
        .expect("bind real scripted host");
    handle
        .spawn_spec(support::placed_spawn_spec(
            "worker",
            "remote",
            &report.host_id,
        ))
        .await
        .expect("spawn placed member through the actual host protocol");

    let peer = spawn_live_external_peer(&test_comms_name_for(&mob_id, "worker", "peer-only")).await;
    let member_ref = handle
        .spawn_with_binding(
            ProfileName::from("worker"),
            AgentIdentity::from("peer-only"),
            None,
            peer.binding(),
        )
        .await
        .expect("bind real peer-only member");
    assert!(matches!(
        member_ref,
        MemberRef::BackendPeer {
            session_id: None,
            ..
        }
    ));
    for name in ["remote", "peer-only"] {
        let entry = handle
            .get_member(&AgentIdentity::from(name))
            .await
            .expect("binding read")
            .expect("bound member");
        for mode in [HandlingMode::Queue, HandlingMode::Steer] {
            let error = handle
                .submit_host_human_input_bounded(
                    entry.agent_runtime_id.clone(),
                    entry.fence_token,
                    WorkSpec::new(HUMAN, WorkOrigin::Internal),
                    mode,
                    delivery(&format!("{name}-{mode:?}")),
                    deadline(),
                )
                .await
                .expect_err("host human semantics must not degrade to remote or peer notices");
            assert!(matches!(error, MobError::UnsupportedForMode { .. }));
        }
    }
    assert!(responder.received_deliveries().is_empty());
    assert!(peer.delivered_input_ids().await.is_empty());
    tokio::time::timeout(WAIT, handle.shutdown())
        .await
        .expect("remote fixture shutdown")
        .expect("remote mob stops");
    responder.shutdown_and_join().await;
    host.shutdown();
}

#[cfg(feature = "openai-live")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closing_live_channel_does_not_cancel_queued_host_human_input() {
    let mut fixture = Fixture::new().await;
    let before = fixture.durable().await;
    let requests_before = fixture.client.requests().len();
    fixture.client.block();
    let active = fixture
        .start(
            WorkSpec::new("work surviving channel close", WorkOrigin::Internal),
            HandlingMode::Queue,
            delivery("close-active"),
        )
        .await
        .expect("active work");
    fixture.client.wait_for_requests(requests_before + 1).await;
    let delivery = delivery("close-queued-human");
    let id = interaction(&delivery);
    let queued = fixture
        .start(
            WorkSpec::new(HUMAN, WorkOrigin::External),
            HandlingMode::Queue,
            delivery,
        )
        .await
        .expect("queued human");
    assert!(fixture.context.appends.lock().expect("context").is_empty());
    fixture.close_channel().await;
    fixture.client.release();
    completed(active).await;
    completed(queued).await;
    let durable = fixture.durable().await;
    assert_human(&durable, HUMAN, id);
    assert_eq!(fixture.client.requests().len(), requests_before + 2);
    assert_eq!(llm_identity(&durable), llm_identity(&before));
    assert!(fixture.context.appends.lock().expect("context").is_empty());
    fixture.finish().await;
}

/// A host-human delivery carries selected skills to the exact fenced target.
/// Two sources host a skill with the same name; the source-pinned key picks
/// exactly one. The member resolves it natively (typed `SkillsResolved`,
/// durable `SkillContext`, the selected body and only that body in the
/// provider request). A retry after a lost response (same delivery identity,
/// same selection) returns the original admission with no second provider
/// call, and a retry under the same identity with a changed selection is a
/// typed idempotency conflict.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn host_human_selected_skills_resolve_natively_and_replay_exactly() {
    use futures::StreamExt;
    use meerkat_core::AgentEvent;

    let chosen_source =
        SourceUuid::parse("5b0c7c2e-4c61-4f2d-9d0e-2f5a8c6b1e11").expect("fixture source uuid");
    let other_source =
        SourceUuid::parse("9e4d2a10-7b3c-4c8e-a1f5-6d2b0c9e8f77").expect("fixture source uuid");
    let name = SkillName::parse("house-style").expect("fixture skill name");
    let chosen_body = "Answer in the chosen house style: numbered, terse, no hedging.";
    let other_body = "Answer in the other house style: long prose with caveats.";
    let fixture = Fixture::with_skills(&[
        (&chosen_source, &name, chosen_body),
        (&other_source, &name, other_body),
    ])
    .await;
    let chosen = SkillKey::new(chosen_source, name.clone());
    let other = SkillKey::new(other_source, name);
    let options = MemberTurnOptions::new().with_skill_references(vec![chosen.clone()]);
    let delivery = delivery("selected-skill");
    let id = interaction(&delivery);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let mut events = fixture
        .handle
        .subscribe_agent_events(&fixture.entry.agent_identity)
        .await
        .expect("member agent events");
    let requests_before = fixture.client.requests().len();

    let receipt = completed(
        fixture
            .start_with_options(spec.clone(), options.clone(), delivery.clone())
            .await
            .expect("selected-skill host input admitted"),
    )
    .await;

    let mut resolved = Vec::new();
    tokio::time::timeout(WAIT, async {
        while let Some(envelope) = events.next().await {
            match envelope.payload {
                AgentEvent::SkillsResolved { skills, .. } => resolved.push(skills),
                AgentEvent::SkillResolutionFailed { reason, .. } => {
                    panic!("selected skill must resolve, got {reason}")
                }
                // The member's own spawn turn may still be draining from
                // this subscription; the selected turn ends at the first
                // completion after its activation.
                AgentEvent::RunCompleted { .. } if !resolved.is_empty() => break,
                _ => {}
            }
        }
    })
    .await
    .expect("the selected turn's events arrive");
    assert_eq!(
        resolved,
        vec![vec![chosen.clone()]],
        "one native activation"
    );

    let durable = fixture.durable().await;
    let skill_rows = durable
        .messages()
        .iter()
        .filter_map(|message| match message {
            Message::User(user) if user.identity.interaction_id == Some(id) => Some(user),
            _ => None,
        })
        .flat_map(|user| user.content.iter())
        .filter_map(|block| match block {
            meerkat_core::types::ContentBlock::SkillContext { skill_key, text } => {
                Some((skill_key.clone(), text.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(skill_rows.len(), 1, "exactly one durable SkillContext");
    assert_eq!(skill_rows[0].0, chosen);
    assert!(
        skill_rows[0].1.contains(chosen_body),
        "native rendered body"
    );
    let requests = fixture.client.requests();
    assert_eq!(requests.len(), requests_before + 1);
    let request_json =
        serde_json::to_string(&requests[requests_before].messages).expect("request serializes");
    assert!(
        request_json.contains(chosen_body),
        "selected body reaches the provider"
    );
    assert!(
        !request_json.contains(other_body),
        "the same-name skill is not selected"
    );
    let inputs = fixture.input_count().await;

    // The response is lost; the host replays the same delivery.
    let replay = fixture
        .submit_with_options(spec.clone(), options, delivery.clone())
        .await
        .expect("same identity and selection replays the original admission");
    assert_eq!(replay.work_ref, receipt.work_ref);
    assert_eq!(replay.runtime_id, receipt.runtime_id);
    assert_eq!(fixture.input_count().await, inputs);
    assert_eq!(fixture.client.requests().len(), requests_before + 1);

    let error = fixture
        .submit_with_options(
            spec,
            MemberTurnOptions::new().with_skill_references(vec![other]),
            delivery,
        )
        .await
        .expect_err("a changed selection under the same identity is not a replay");
    assert!(
        matches!(error, MobError::WorkInputIdempotencyConflict { .. }),
        "typed conflict, got {error:?}"
    );
    assert_eq!(fixture.input_count().await, inputs);
    assert_eq!(fixture.client.requests().len(), requests_before + 1);
    fixture.finish().await;
}

/// An empty skill selection is no selection (it keeps the metadata-less
/// replay identity), and an options interaction id that differs from the
/// delivery correlation is a typed refusal before admission.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn host_human_options_normalize_empty_selection_and_refuse_foreign_interaction() {
    let fixture = Fixture::new().await;
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let empty = delivery("empty-selection");
    let first = fixture
        .submit_with_options(
            spec.clone(),
            MemberTurnOptions::new().with_skill_references(Vec::new()),
            empty.clone(),
        )
        .await
        .expect("empty selection admitted");
    let replay = fixture
        .submit(spec.clone(), HandlingMode::Queue, empty)
        .await
        .expect("an empty selection replays as no selection");
    assert_eq!(replay.work_ref, first.work_ref);

    let inputs = fixture.input_count().await;
    let foreign = delivery("foreign-interaction");
    let error = fixture
        .submit_with_options(
            spec,
            MemberTurnOptions::new().with_interaction_id(InteractionId(Uuid::new_v4())),
            foreign.clone(),
        )
        .await
        .expect_err("a foreign interaction id is not the delivery's interaction");
    assert!(
        matches!(
            &error,
            MobError::DeliveryInteractionConflict { correlation_id, .. }
                if *correlation_id == foreign.correlation_id
        ),
        "typed conflict, got {error:?}"
    );
    assert_eq!(fixture.input_count().await, inputs);
    fixture.finish().await;
}

const QUEUED_HUMAN: &str = "A second question, queued behind the running turn.";

/// One host process over the stop fixture's durable files: the realm's
/// SQLite session and runtime stores, the mob's SQLite storage and the
/// session event log.
struct StopHost {
    handle: MobHandle,
    runtime_store: Arc<dyn RuntimeStore>,
    adapter: Arc<meerkat_runtime::MeerkatMachine>,
}

/// Open a host process over `root`: a new mob (`label`) or, without a label,
/// the same mob resumed from its durable history.
async fn open_stop_host(
    root: &std::path::Path,
    client: &ScriptedClient,
    label: Option<&str>,
) -> StopHost {
    let factory = meerkat::AgentFactory::new(root.join("factory-store"))
        .user_config_root(root.join("config"))
        .runtime_root(root.join("runtime"))
        .project_root(root.join("project"))
        .context_root(root.join("context"))
        .builtins(false)
        .comms(true);
    let mut config = meerkat::Config::default();
    config.agent.model = "gpt-5.5".to_string();
    config.compaction.auto_compact_threshold = 1_000_000;
    let mut builder = meerkat::FactoryAgentBuilder::new(factory, config);
    builder.default_llm_client = Some(Arc::new(client.clone()));
    let db = root.join("realm.db");
    let session_store =
        Arc::new(meerkat_store::SqliteSessionStore::open(&db).expect("session store"));
    builder.default_session_store = Some(Arc::new(meerkat_store::StoreAdapter::new(
        session_store.clone(),
    )));
    let runtime_store: Arc<dyn RuntimeStore> = Arc::new(
        meerkat_runtime::SqliteRuntimeStore::new_head_canonical(&db)
            .expect("co-tenant canonical runtime store"),
    );
    let service = Arc::new(
        Service::new(
            builder,
            16,
            session_store,
            runtime_store.clone(),
            Arc::new(meerkat_store::MemoryBlobStore::new()),
        )
        .with_event_projection(
            Arc::new(meerkat_session::event_store::FileEventStore::new(
                root.join("events"),
            )),
            Arc::new(meerkat_session::projector::SessionProjector::new(
                root.join("projection"),
            )),
        ),
    );
    let storage = MobStorage::persistent(root.join("mob.db")).expect("persistent mob storage");
    let handle = match label {
        Some(label) => {
            let mut definition = with_unique_mob_id(sample_definition(), label);
            let lead = definition
                .profiles
                .get_mut(&ProfileName::from("lead"))
                .and_then(ProfileBinding::as_inline_mut)
                .expect("inline lead");
            lead.model = "gpt-5.5".to_string();
            lead.runtime_mode = crate::MobRuntimeMode::AutonomousHost;
            lead.tools = ToolConfig {
                comms: true,
                ..ToolConfig::default()
            };
            MobBuilder::new(definition, storage)
                .with_session_service(service.clone())
                .with_default_llm_client(Arc::new(client.clone()))
                .create()
                .await
                .expect("create real autonomous mob")
        }
        None => tokio::time::timeout(
            WAIT,
            MobBuilder::for_resume(storage)
                .with_session_service(service.clone())
                .with_default_llm_client(Arc::new(client.clone()))
                .notify_orchestrator_on_resume(false)
                .resume(),
        )
        .await
        .expect("mob resume finishes")
        .expect("resume the same mob from its durable history"),
    };
    let adapter = service
        .acquire_runtime_adapter(None)
        .expect("acquire runtime authority")
        .expect("runtime owner");
    StopHost {
        handle,
        runtime_store,
        adapter,
    }
}

/// A fresh fixture root in the system temp dir.
fn stop_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::Builder::new()
        .prefix("meerkat-host-human-stop-")
        .tempdir()
        .expect("fixture storage in the system temp dir");
    let root_path = root.path().canonicalize().expect("absolute fixture root");
    for directory in ["config", "runtime", "project", "context"] {
        std::fs::create_dir_all(root_path.join(directory)).expect("fixture root");
    }
    (root, root_path)
}

/// Spawn the stop fixture's autonomous member and settle its kickoff.
async fn spawn_stop_member(host: &StopHost) -> (AgentIdentity, SessionId, RosterEntry) {
    // Unique per test: identity-keyed gates are process-wide.
    let identity = AgentIdentity::from(format!("host-human-stop-{}", Uuid::new_v4().simple()));
    let session_id = host
        .handle
        .spawn(
            ProfileName::from("lead"),
            identity.clone(),
            Some("fixture kickoff".into()),
        )
        .await
        .expect("spawn autonomous member")
        .bridge_session_id()
        .expect("local session member")
        .clone();
    host.handle
        .wait_for_kickoff_complete(Some(WAIT))
        .await
        .expect("real autonomous kickoff commits");
    let entry = host
        .handle
        .get_member(&identity)
        .await
        .expect("roster read")
        .expect("member binding");
    (identity, session_id, entry)
}

/// The Core input a delivery admitted. The mob admission returns no input
/// id, so the row is read once by the delivery's idempotency key and then
/// addressed by its own input id.
async fn admitted_input_id(
    runtime_store: &Arc<dyn RuntimeStore>,
    session_id: &SessionId,
    key: &MobDeliveryIdentity,
) -> meerkat_core::lifecycle::InputId {
    runtime_store
        .load_input_state_by_idempotency_key(
            &LogicalRuntimeId::for_session(session_id),
            &meerkat_runtime::identifiers::IdempotencyKey::new(key.idempotency_key.clone()),
        )
        .await
        .expect("exact runtime-owned key lookup")
        .expect("the delivery has a canonical input")
        .state()
        .state
        .input_id
        .clone()
}

/// The input's stored row by its input id, the exact digest of its stored
/// bytes, and whether it carries an interaction-terminal carrier or an
/// unfinished terminal-owner entry.
struct StopRow {
    stored: meerkat_runtime::input_state::StoredInputState,
    digest: String,
    has_outbox: bool,
    pending_terminal_owner: bool,
    /// The row's wire image, for facts a typed read cannot name yet.
    image: serde_json::Value,
}

async fn stop_row(
    runtime_store: &Arc<dyn RuntimeStore>,
    session_id: &SessionId,
    key: &MobDeliveryIdentity,
    input_id: &meerkat_core::lifecycle::InputId,
) -> StopRow {
    let runtime_id = LogicalRuntimeId::for_session(session_id);
    let stored = runtime_store
        .load_input_state(&runtime_id, input_id)
        .await
        .expect("runtime-owned input read")
        .expect("the input keeps its durable row");
    let exact = runtime_store
        .load_input_state_by_idempotency_key(
            &runtime_id,
            &meerkat_runtime::identifiers::IdempotencyKey::new(key.idempotency_key.clone()),
        )
        .await
        .expect("exact runtime-owned key lookup")
        .expect("the delivery still names its input");
    assert_eq!(&exact.state().state.input_id, input_id);
    let image = serde_json::to_value(&stored).expect("the row's stored wire image");
    let mut pending_terminal_owner = false;
    let mut after = None;
    loop {
        let page = runtime_store
            .load_pending_terminal_owner_ids_page(
                &runtime_id,
                after.as_ref(),
                meerkat_runtime::store::MAX_PENDING_TERMINAL_OWNER_PAGE,
            )
            .await
            .expect("unfinished terminal owners");
        pending_terminal_owner |= page.contains(input_id);
        if page.len() < meerkat_runtime::store::MAX_PENDING_TERMINAL_OWNER_PAGE {
            break;
        }
        after = page.last().cloned();
    }
    StopRow {
        has_outbox: !image["interaction_terminal_outbox"].is_null(),
        image: image.clone(),
        digest: exact.exact_row_digest().to_owned(),
        stored,
        pending_terminal_owner,
    }
}

/// The session's durable event log, read the way a fresh process reads it.
async fn stop_events(root: &std::path::Path, session_id: &SessionId) -> Vec<AgentEvent> {
    use meerkat_session::event_store::EventStore as _;
    meerkat_session::event_store::FileEventStore::new(root.join("events"))
        .read_from(session_id, 0)
        .await
        .expect("read the durable event log")
        .into_iter()
        .map(|stored| stored.event)
        .collect()
}

/// Every run the log started for `interaction_id`.
fn runs_started_for(
    events: &[AgentEvent],
    interaction_id: InteractionId,
) -> Vec<meerkat_core::lifecycle::RunId> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::RunStarted { identity, .. }
                if identity.interaction_id == Some(interaction_id) =>
            {
                identity.run_id.clone()
            }
            _ => None,
        })
        .collect()
}

/// Every failure the log recorded for `run_id`.
fn run_failures(
    events: &[AgentEvent],
    run_id: &meerkat_core::lifecycle::RunId,
) -> Vec<meerkat_core::event::AgentErrorReport> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::RunFailed {
                identity,
                error_report,
                ..
            } if identity.run_id.as_ref() == Some(run_id) => Some(error_report.clone()),
            _ => None,
        })
        .collect()
}

/// Every interaction failure in the session's log, under any interaction id.
/// No input in these rows is Core-directed, so a stop publishes none.
fn interaction_failed_events(events: &[AgentEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, AgentEvent::InteractionFailed { .. }))
        .count()
}

/// Events in the log that name `interaction_id` anywhere in their wire image.
fn events_naming(events: &[AgentEvent], interaction_id: InteractionId) -> usize {
    let id = interaction_id.0.to_string();
    events
        .iter()
        .filter(|event| {
            serde_json::to_string(event)
                .expect("event wire image")
                .contains(&id)
        })
        .count()
}

fn requests_carrying(requests: &[meerkat_client::LlmRequest], text: &str) -> usize {
    requests
        .iter()
        .filter(|request| {
            request.messages.iter().any(
                |message| matches!(message, Message::User(user) if user.text_content().contains(text)),
            )
        })
        .count()
}

/// How a stopped input's bounded waiter ended.
#[derive(Clone, Debug, PartialEq)]
enum StopAnswer {
    Succeeded,
    Cancelled,
    RuntimeTerminated {
        reason: String,
        kind: meerkat_core::TurnTerminalCauseKind,
        detail: Option<String>,
    },
    Other(String),
    Refused(String),
    Unresolved,
    RequeuedAcrossStop(meerkat_core::lifecycle::InputId),
    Abandoned(String),
    RequeueExpired(meerkat_core::lifecycle::InputId),
}

async fn stop_answer(turn: WorkTurnHandle, session_id: &SessionId) -> StopAnswer {
    let Ok(result) = tokio::time::timeout(
        WAIT,
        turn.wait_bounded(BoundedResultSpec::new("stopped human", 1024).expect("bound")),
    )
    .await
    else {
        return StopAnswer::Unresolved;
    };
    let Err(error) = result else {
        return StopAnswer::Succeeded;
    };
    match error.failure() {
        BoundedTurnFailure::Cancelled { session_id: failed } if failed == session_id => {
            StopAnswer::Cancelled
        }
        BoundedTurnFailure::RuntimeTerminated {
            session_id: failed,
            reason,
            error,
        } if failed == session_id => StopAnswer::RuntimeTerminated {
            reason: reason.clone(),
            kind: error.kind,
            detail: error.detail.clone(),
        },
        BoundedTurnFailure::RequeuedAcrossStop {
            session_id: failed,
            input_id,
        } if failed == session_id => StopAnswer::RequeuedAcrossStop(input_id.clone()),
        BoundedTurnFailure::Abandoned {
            session_id: failed,
            reason,
            ..
        } if failed == session_id => StopAnswer::Abandoned(reason.clone()),
        BoundedTurnFailure::RequeueExpired {
            session_id: failed,
            input_id,
        } if failed == session_id => StopAnswer::RequeueExpired(input_id.clone()),
        other => StopAnswer::Other(format!("{other:?}")),
    }
}

/// What the stopping process leaves for one input: its waiter's answer, its
/// row, and its runs and terminal events in the log.
#[derive(Debug, PartialEq)]
struct AtStop {
    waiter: StopAnswer,
    phase: meerkat_runtime::input_state::InputLifecycleState,
    outcome: Option<meerkat_runtime::input_state::InputTerminalOutcome>,
    attempt_count: u32,
    last_run_id: Option<meerkat_core::lifecycle::RunId>,
    payload_kept: bool,
    replay_identity_kept: bool,
    outbox: bool,
    pending_terminal_owner: bool,
    runs_started: usize,
    run_failures: Vec<(meerkat_core::event::AgentErrorClass, String)>,
    interaction_failed_events: usize,
}

fn observe_at_stop(
    waiter: StopAnswer,
    row: &StopRow,
    events: &[AgentEvent],
    interaction_id: InteractionId,
) -> AtStop {
    let runs = runs_started_for(events, interaction_id);
    AtStop {
        waiter,
        phase: row.stored.seed.phase,
        outcome: row.stored.seed.terminal_outcome.clone(),
        attempt_count: row.stored.seed.attempt_count,
        last_run_id: row.stored.seed.last_run_id.clone(),
        payload_kept: row.stored.state.persisted_input.is_some(),
        replay_identity_kept: row.stored.state.prompt_replay_identity.is_some(),
        outbox: row.has_outbox,
        pending_terminal_owner: row.pending_terminal_owner,
        runs_started: runs.len(),
        run_failures: runs
            .iter()
            .flat_map(|run| run_failures(events, run))
            .map(|report| (report.class, report.message))
            .collect(),
        interaction_failed_events: interaction_failed_events(events),
    }
}

/// What the restarted process does with a stopped input: recovery, a
/// same-key replay, and the log across both processes.
#[derive(Debug, PartialEq)]
struct AfterRestart {
    recovery_count: u32,
    row_unchanged_by_recovery: bool,
    pending_terminal_owner: bool,
    replay: StopAnswer,
    model_requests: usize,
    row_unchanged_by_replay: bool,
    runs_started: Vec<meerkat_core::lifecycle::RunId>,
    run_failures: usize,
    interaction_failed_events: usize,
}

/// Recovery's view of the row, then a same-key replay of `spec` in the
/// restarted process, then the row and the log after it.
async fn observe_after_restart(
    root: &std::path::Path,
    host: &StopHost,
    client: &ScriptedClient,
    at_exit: &StopRow,
    input: (
        &MobDeliveryIdentity,
        &meerkat_core::lifecycle::InputId,
        WorkSpec,
    ),
    identity: &AgentIdentity,
    session_id: &SessionId,
) -> AfterRestart {
    let (key, input_id, spec) = input;
    let after_recovery = stop_row(&host.runtime_store, session_id, key, input_id).await;
    let entry = host
        .handle
        .get_member(identity)
        .await
        .expect("roster read")
        .expect("the member survives the restart");
    let replay = match host
        .handle
        .start_host_human_input_bounded(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            spec,
            HandlingMode::Queue,
            key.clone(),
            deadline(),
        )
        .await
    {
        Ok(replay) => stop_answer(replay, session_id).await,
        Err(error) => StopAnswer::Refused(error.to_string()),
    };
    let after_replay = stop_row(&host.runtime_store, session_id, key, input_id).await;
    let events = stop_events(root, session_id).await;
    let runs = runs_started_for(&events, interaction(key));
    AfterRestart {
        recovery_count: after_recovery.stored.state.recovery_count,
        row_unchanged_by_recovery: after_recovery.digest == at_exit.digest,
        pending_terminal_owner: after_recovery.pending_terminal_owner,
        replay,
        model_requests: client.requests().len(),
        row_unchanged_by_replay: after_replay.digest == after_recovery.digest,
        run_failures: runs
            .iter()
            .map(|run| run_failures(&events, run).len())
            .sum(),
        runs_started: runs,
        interaction_failed_events: interaction_failed_events(&events),
    }
}

/// A Queue-mode host human input whose own turn is in flight (its model
/// call held) when the host stops gracefully (the stop a gateway runs on
/// SIGTERM), then a restart over the same durable files.
///
/// The stop cancels the in-flight run: the input is abandoned as cancelled
/// and its payload retired in the same lifecycle commit, and the run fails
/// once as cancelled. A host human input is not a Core-directed input, so it
/// has no interaction-terminal carrier and no interaction terminal event.
/// The restart leaves the terminal row untouched, runs nothing, and a
/// same-key replay answers the failed terminal. Both processes are observed
/// before one comparison, so a failure shows the whole shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_human_mid_turn_at_graceful_stop_fails_once_after_restart_and_keeps_row() {
    use meerkat_core::event::AgentErrorClass;
    use meerkat_runtime::input_state::{
        InputAbandonReason, InputLifecycleState, InputTerminalOutcome,
    };

    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-stop")).await;
    let mob_id = host.handle.mob_id().clone();
    let (identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let key = delivery("graceful-stop-in-flight");
    let interaction_id = interaction(&key);
    let spec = WorkSpec::new(HUMAN, WorkOrigin::External);
    let turn = host
        .handle
        .start_host_human_input_bounded(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            spec.clone(),
            HandlingMode::Queue,
            key.clone(),
            deadline(),
        )
        .await
        .expect("queued human admission");
    client.wait_for_requests(requests_before + 1).await;
    assert_eq!(
        requests_carrying(&client.requests()[requests_before..], HUMAN),
        1,
        "the held model call is the human input's own turn"
    );
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;
    assert_ne!(
        input_id.0, interaction_id.0,
        "a host human input is not Core-directed: its input id is not the interaction id"
    );
    let in_flight = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    assert!(in_flight.stored.seed.terminal_outcome.is_none());
    assert!(in_flight.stored.state.persisted_input.is_some());

    tokio::time::timeout(WAIT, host.handle.shutdown())
        .await
        .expect("the graceful stop finishes")
        .expect("graceful stop");
    let waiter = stop_answer(turn, &session_id).await;
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    let at_stop = observe_at_stop(waiter, &at_exit, &events, interaction_id);
    // The input's own run as the log names it; `runs_started: 1` pins that
    // it is the only one.
    let its_run = runs_started_for(&events, interaction_id).first().cloned();
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    // Restart: a fresh host over the same files, with a model that records
    // every request it gets.
    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let after_restart = observe_after_restart(
        &root_path,
        &restarted,
        &restarted_client,
        &at_exit,
        (&key, &input_id, spec),
        &identity,
        &session_id,
    )
    .await;
    assert_eq!(
        (at_stop, after_restart),
        (
            AtStop {
                waiter: StopAnswer::Cancelled,
                phase: InputLifecycleState::Abandoned,
                outcome: Some(InputTerminalOutcome::Abandoned {
                    reason: InputAbandonReason::Cancelled,
                }),
                attempt_count: 1,
                last_run_id: its_run.clone(),
                payload_kept: false,
                replay_identity_kept: true,
                outbox: false,
                pending_terminal_owner: false,
                runs_started: 1,
                run_failures: vec![(AgentErrorClass::Cancelled, "Run was cancelled".to_owned())],
                interaction_failed_events: 0,
            },
            AfterRestart {
                recovery_count: at_exit.stored.state.recovery_count,
                row_unchanged_by_recovery: true,
                pending_terminal_owner: false,
                replay: StopAnswer::Cancelled,
                model_requests: 0,
                row_unchanged_by_replay: true,
                runs_started: its_run.into_iter().collect(),
                run_failures: 1,
                interaction_failed_events: 0,
            },
        ),
        "the stop cancels the in-flight run once and retires the payload with the \
         terminal; the restart keeps that terminal and the replay answers it without a run"
    );
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

/// A Queue-mode host human input still queued behind a running turn when
/// the host stops gracefully, then a restart over the same durable files.
///
/// The stop terminalizes it through the runtime stop: abandoned as stopped
/// with its payload retired, and its waiter gets the runtime termination.
/// The stop runs while the member's runtime registration drains, so the
/// termination reason is the unregistered one. The input never ran, so the
/// log has no run for it, and as a non-directed input it has no
/// interaction-terminal carrier or event. The restart leaves the row
/// untouched, runs nothing, and a same-key replay answers the same
/// termination.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_human_behind_a_running_turn_at_graceful_stop_is_terminated_once_after_restart_and_keeps_row()
 {
    use meerkat_runtime::input_state::{
        InputAbandonReason, InputLifecycleState, InputTerminalOutcome,
    };

    // Generated Draining classification: shutdown stops inside the unregister drain.
    const UNREGISTERED: &str = "runtime session unregistered";
    let terminated = StopAnswer::RuntimeTerminated {
        reason: UNREGISTERED.to_owned(),
        kind: meerkat_core::TurnTerminalCauseKind::FatalFailure,
        detail: Some(UNREGISTERED.to_owned()),
    };
    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-stop-queued")).await;
    let mob_id = host.handle.mob_id().clone();
    let (identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running = host
        .handle
        .start_host_human_input_bounded(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            WorkSpec::new(HUMAN, WorkOrigin::External),
            HandlingMode::Queue,
            delivery("graceful-stop-running"),
            deadline(),
        )
        .await
        .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let key = delivery("graceful-stop-queued-behind");
    let interaction_id = interaction(&key);
    let spec = WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External);
    let queued = host
        .handle
        .start_host_human_input_bounded(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            spec.clone(),
            HandlingMode::Queue,
            key.clone(),
            deadline(),
        )
        .await
        .expect("queued human admission");
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;
    assert_ne!(input_id.0, interaction_id.0);
    let before_stop = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    assert_eq!(before_stop.stored.seed.phase, InputLifecycleState::Queued);

    tokio::time::timeout(WAIT, host.handle.shutdown())
        .await
        .expect("the graceful stop finishes")
        .expect("graceful stop");
    let waiter = stop_answer(queued, &session_id).await;
    assert_eq!(
        stop_answer(running, &session_id).await,
        StopAnswer::Cancelled,
        "the running turn ahead of it is cancelled"
    );
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    let at_stop = observe_at_stop(waiter, &at_exit, &events, interaction_id);
    let naming_at_stop = events_naming(&events, interaction_id);
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let after_restart = observe_after_restart(
        &root_path,
        &restarted,
        &restarted_client,
        &at_exit,
        (&key, &input_id, spec),
        &identity,
        &session_id,
    )
    .await;
    let naming_after_restart =
        events_naming(&stop_events(&root_path, &session_id).await, interaction_id);
    assert_eq!(
        (
            at_stop,
            after_restart,
            (naming_at_stop, naming_after_restart),
        ),
        (
            AtStop {
                waiter: terminated.clone(),
                phase: InputLifecycleState::Abandoned,
                outcome: Some(InputTerminalOutcome::Abandoned {
                    reason: InputAbandonReason::Stopped,
                }),
                attempt_count: 0,
                last_run_id: None,
                payload_kept: false,
                replay_identity_kept: true,
                outbox: false,
                pending_terminal_owner: false,
                runs_started: 0,
                run_failures: Vec::new(),
                interaction_failed_events: 0,
            },
            AfterRestart {
                recovery_count: at_exit.stored.state.recovery_count,
                row_unchanged_by_recovery: true,
                pending_terminal_owner: false,
                replay: terminated,
                model_requests: 0,
                row_unchanged_by_replay: true,
                runs_started: Vec::new(),
                run_failures: 0,
                interaction_failed_events: 0,
            },
            (0, 0),
        ),
        "the stop terminates the queued input without a run and retires its payload; \
         the restart keeps that terminal and the replay answers it without a run"
    );
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

const QUEUED_SECOND: &str = "A third question, queued behind the second.";
const AFTER_RESTART: &str = "A question sent after the restart.";

fn requeue_options(max_age: Duration) -> crate::ShutdownOptions {
    crate::ShutdownOptions::default().with_queued_inputs(crate::QueuedInputsOnShutdown::Requeue(
        crate::RequeueMaxAge::new(max_age).expect("an accepted requeue bound"),
    ))
}

/// Start a Queue- or Steer-mode host human input on the stop fixture's member.
async fn start_stop_input(
    host: &StopHost,
    entry: &RosterEntry,
    spec: WorkSpec,
    mode: HandlingMode,
    key: &MobDeliveryIdentity,
) -> Result<WorkTurnHandle, MobError> {
    host.handle
        .start_host_human_input_bounded(
            entry.agent_runtime_id.clone(),
            entry.fence_token,
            spec,
            mode,
            key.clone(),
            deadline(),
        )
        .await
}

/// A requeued input as the stopping process leaves it.
#[derive(Debug, PartialEq)]
struct RequeuedAtStop {
    waiter: StopAnswer,
    phase: meerkat_runtime::input_state::InputLifecycleState,
    outcome: serde_json::Value,
    payload_kept: bool,
    replay_identity_kept: bool,
    requeue_mark: bool,
    outbox: bool,
    terminal_completion: bool,
    pending_terminal_owner: bool,
    runs_started: usize,
}

fn observe_requeued_at_stop(
    waiter: StopAnswer,
    row: &StopRow,
    events: &[AgentEvent],
    interaction_id: InteractionId,
) -> RequeuedAtStop {
    RequeuedAtStop {
        waiter,
        phase: row.stored.seed.phase,
        outcome: serde_json::to_value(&row.stored.seed.terminal_outcome)
            .expect("terminal outcome wire image"),
        payload_kept: row.stored.state.persisted_input.is_some(),
        replay_identity_kept: row.stored.state.prompt_replay_identity.is_some(),
        requeue_mark: !row.image["stop_requeue"].is_null(),
        outbox: row.has_outbox,
        terminal_completion: !row.image["terminal_completion"].is_null(),
        pending_terminal_owner: row.pending_terminal_owner,
        runs_started: runs_started_for(events, interaction_id).len(),
    }
}

/// The restarted process's handling of a requeued input: a same-key replay,
/// then the row, the model requests and the log.
#[derive(Debug, PartialEq)]
struct RequeuedAfterRestart {
    replay: StopAnswer,
    phase: meerkat_runtime::input_state::InputLifecycleState,
    outcome: serde_json::Value,
    payload_kept: bool,
    requests_carrying: usize,
    runs_started: usize,
}

async fn observe_requeued_after_restart(
    root: &std::path::Path,
    host: &StopHost,
    client: &ScriptedClient,
    input: (
        &MobDeliveryIdentity,
        &meerkat_core::lifecycle::InputId,
        WorkSpec,
    ),
    identity: &AgentIdentity,
    session_id: &SessionId,
    text: &str,
) -> RequeuedAfterRestart {
    let (key, input_id, spec) = input;
    let entry = host
        .handle
        .get_member(identity)
        .await
        .expect("roster read")
        .expect("the member survives the restart");
    let replay = match start_stop_input(host, &entry, spec, HandlingMode::Queue, key).await {
        Ok(replay) => stop_answer(replay, session_id).await,
        Err(error) => StopAnswer::Refused(error.to_string()),
    };
    let row = stop_row(&host.runtime_store, session_id, key, input_id).await;
    let events = stop_events(root, session_id).await;
    RequeuedAfterRestart {
        replay,
        phase: row.stored.seed.phase,
        outcome: serde_json::to_value(&row.stored.seed.terminal_outcome)
            .expect("terminal outcome wire image"),
        payload_kept: row.stored.state.persisted_input.is_some(),
        requests_carrying: requests_carrying(&client.requests(), text),
        runs_started: runs_started_for(&events, interaction(key)).len(),
    }
}

/// Under a requeue Shutdown, a Queue-mode host human input still queued
/// behind a running turn stays durably queued: its waiter is told it was
/// requeued, its row keeps its payload, its replay identity and a requeue
/// mark, it has no terminal completion and no carrier, and nothing ran it.
/// The running input is still cancelled and is not run again. After the
/// restart the requeued input runs exactly once, its first model request
/// does not carry the cancelled turn's user message, and a same-key replay
/// re-attaches to its result.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_keeps_a_queued_human_input_and_runs_it_once_after_restart() {
    use meerkat_runtime::input_state::InputLifecycleState;

    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-requeue")).await;
    let mob_id = host.handle.mob_id().clone();
    let (identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running_key = delivery("requeue-running");
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &running_key,
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let key = delivery("requeue-queued-behind");
    let interaction_id = interaction(&key);
    let spec = WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External);
    let queued = start_stop_input(&host, &entry, spec.clone(), HandlingMode::Queue, &key)
        .await
        .expect("queued human admission");
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;
    let before_stop = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    assert_eq!(before_stop.stored.seed.phase, InputLifecycleState::Queued);

    tokio::time::timeout(
        WAIT,
        host.handle
            .shutdown_with_report(requeue_options(crate::RequeueMaxAge::DEFAULT)),
    )
    .await
    .expect("the requeue stop finishes")
    .expect("requeue stop");
    let waiter = stop_answer(queued, &session_id).await;
    let running_answer = stop_answer(running, &session_id).await;
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    let at_stop = observe_requeued_at_stop(waiter, &at_exit, &events, interaction_id);
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let after_restart = observe_requeued_after_restart(
        &root_path,
        &restarted,
        &restarted_client,
        (&key, &input_id, spec),
        &identity,
        &session_id,
        QUEUED_HUMAN,
    )
    .await;
    let running_runs = runs_started_for(
        &stop_events(&root_path, &session_id).await,
        interaction(&running_key),
    )
    .len();
    // The kept input's first model request after the restart shows whether
    // the cancelled turn's user message is still in the transcript.
    let restart_requests = restarted_client.requests();
    let first_kept_request_carries_cancelled_text = restart_requests
        .iter()
        .find(|request| requests_carrying(std::slice::from_ref(*request), QUEUED_HUMAN) == 1)
        .map(|request| requests_carrying(std::slice::from_ref(request), HUMAN) == 1);
    assert_eq!(
        first_kept_request_carries_cancelled_text,
        Some(false),
        "the kept input's first request after the restart does not carry the \
         cancelled turn's user message"
    );
    assert_eq!(
        (running_answer, at_stop, after_restart, running_runs),
        (
            StopAnswer::Cancelled,
            RequeuedAtStop {
                waiter: StopAnswer::RequeuedAcrossStop(input_id.clone()),
                phase: InputLifecycleState::Queued,
                outcome: serde_json::Value::Null,
                payload_kept: true,
                replay_identity_kept: true,
                requeue_mark: true,
                outbox: false,
                terminal_completion: false,
                pending_terminal_owner: false,
                runs_started: 0,
            },
            RequeuedAfterRestart {
                replay: StopAnswer::Succeeded,
                phase: InputLifecycleState::Consumed,
                outcome: serde_json::json!({"outcome_type": "consumed"}),
                payload_kept: false,
                requests_carrying: 1,
                runs_started: 1,
            },
            1,
        ),
        "the requeued input stays queued across the stop and runs once after the restart; \
         the cancelled running input is not run again"
    );
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

/// A requeued input older than the requeue bound when the next start
/// recovers it ends as expired instead of running: payload retired, no run,
/// no model request, and a same-key replay answers the expiry. A later start
/// serves nothing: the expiry's commit left the row terminal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_expires_a_queued_human_input_older_than_the_bound_at_restart() {
    use meerkat_runtime::input_state::InputLifecycleState;

    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-requeue-expiry")).await;
    let mob_id = host.handle.mob_id().clone();
    let (identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("requeue-expiry-running"),
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let key = delivery("requeue-expiry-queued");
    let interaction_id = interaction(&key);
    let spec = WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External);
    let queued = start_stop_input(&host, &entry, spec.clone(), HandlingMode::Queue, &key)
        .await
        .expect("queued human admission");
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;

    tokio::time::timeout(
        WAIT,
        host.handle
            .shutdown_with_report(requeue_options(Duration::from_millis(1))),
    )
    .await
    .expect("the requeue stop finishes")
    .expect("requeue stop");
    let waiter = stop_answer(queued, &session_id).await;
    let _ = stop_answer(running, &session_id).await;
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    let at_stop = observe_requeued_at_stop(waiter, &at_exit, &events, interaction_id);
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let after_restart = observe_requeued_after_restart(
        &root_path,
        &restarted,
        &restarted_client,
        (&key, &input_id, spec),
        &identity,
        &session_id,
        QUEUED_HUMAN,
    )
    .await;
    assert_eq!(
        (at_stop.waiter.clone(), at_stop.requeue_mark, after_restart),
        (
            StopAnswer::RequeuedAcrossStop(input_id.clone()),
            true,
            RequeuedAfterRestart {
                replay: StopAnswer::RequeueExpired(input_id.clone()),
                phase: InputLifecycleState::Abandoned,
                outcome: serde_json::json!({
                    "outcome_type": "abandoned",
                    "reason": "requeue_expired",
                }),
                payload_kept: false,
                requests_carrying: 0,
                runs_started: 0,
            },
        ),
        "a requeued input past its bound at restart ends as expired and never runs"
    );

    // Once the expiry has committed, the row is terminal: the next start
    // serves nothing, and the row stays expired. (The process stops
    // gracefully here; the store then holds exactly what the expiry's own
    // commit wrote, which is what a cut right after it leaves.)
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
    drop(restarted);
    await_supervisor_route_release(&mob_id).await;
    let third_client = ScriptedClient::new(true);
    let third = open_stop_host(&root_path, &third_client, None).await;
    let after_cut = stop_row(&third.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    assert_eq!(
        (
            after_cut.stored.seed.phase,
            serde_json::to_value(&after_cut.stored.seed.terminal_outcome)
                .expect("terminal outcome wire image"),
            runs_started_for(&events, interaction(&key)).len(),
            requests_carrying(&third_client.requests(), QUEUED_HUMAN),
        ),
        (
            InputLifecycleState::Abandoned,
            serde_json::json!({
                "outcome_type": "abandoned",
                "reason": "requeue_expired",
            }),
            0,
            0,
        ),
        "a start after the expiry committed serves nothing"
    );
    tokio::time::timeout(WAIT, third.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

/// Two inputs requeued in admission order, then one admitted right after the
/// restart: the restarted member runs them in that order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_runs_requeued_inputs_in_admission_order_before_new_work() {
    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-requeue-order")).await;
    let mob_id = host.handle.mob_id().clone();
    let (identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("requeue-order-running"),
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let first_key = delivery("requeue-order-first");
    let first_spec = WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External);
    let first = start_stop_input(
        &host,
        &entry,
        first_spec.clone(),
        HandlingMode::Queue,
        &first_key,
    )
    .await
    .expect("first queued admission");
    let second_key = delivery("requeue-order-second");
    let second_spec = WorkSpec::new(QUEUED_SECOND, WorkOrigin::External);
    let second = start_stop_input(
        &host,
        &entry,
        second_spec.clone(),
        HandlingMode::Queue,
        &second_key,
    )
    .await
    .expect("second queued admission");

    tokio::time::timeout(
        WAIT,
        host.handle
            .shutdown_with_report(requeue_options(crate::RequeueMaxAge::DEFAULT)),
    )
    .await
    .expect("the requeue stop finishes")
    .expect("requeue stop");
    let _ = stop_answer(first, &session_id).await;
    let _ = stop_answer(second, &session_id).await;
    let _ = stop_answer(running, &session_id).await;
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let restarted_entry = restarted
        .handle
        .get_member(&identity)
        .await
        .expect("roster read")
        .expect("the member survives the restart");
    let later = start_stop_input(
        &restarted,
        &restarted_entry,
        WorkSpec::new(AFTER_RESTART, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("requeue-order-after-restart"),
    )
    .await
    .expect("post-restart admission");
    let later_answer = stop_answer(later, &session_id).await;
    let first_replay = match start_stop_input(
        &restarted,
        &restarted_entry,
        first_spec,
        HandlingMode::Queue,
        &first_key,
    )
    .await
    {
        Ok(replay) => stop_answer(replay, &session_id).await,
        Err(error) => StopAnswer::Refused(error.to_string()),
    };
    let second_replay = match start_stop_input(
        &restarted,
        &restarted_entry,
        second_spec,
        HandlingMode::Queue,
        &second_key,
    )
    .await
    {
        Ok(replay) => stop_answer(replay, &session_id).await,
        Err(error) => StopAnswer::Refused(error.to_string()),
    };
    // The last request carries the whole transcript; read the order of the
    // three user messages from it.
    let requests = restarted_client.requests();
    let order = requests.last().map(|request| {
        [QUEUED_HUMAN, QUEUED_SECOND, AFTER_RESTART].map(|text| {
            request.messages.iter().position(|message| {
                matches!(message, Message::User(user) if user.text_content().contains(text))
            })
        })
    });
    let in_order = matches!(
        order,
        Some([Some(first), Some(second), Some(later)]) if first < second && second < later
    );
    assert_eq!(
        (first_replay, second_replay, later_answer, in_order),
        (
            StopAnswer::Succeeded,
            StopAnswer::Succeeded,
            StopAnswer::Succeeded,
            true
        ),
        "requeued inputs run in admission order, before work admitted after the restart: \
         {order:?}"
    );
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

/// Requeue keeps only eligible inputs: a Steer-mode input queued behind the
/// running turn targets the cancelled run, so it is not requeued and never
/// runs after the restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_does_not_keep_a_steer_input_queued_behind_the_running_turn() {
    use meerkat_runtime::input_state::InputLifecycleState;

    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-requeue-steer")).await;
    let mob_id = host.handle.mob_id().clone();
    let (_identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("requeue-steer-running"),
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let key = delivery("requeue-steer-queued");
    let interaction_id = interaction(&key);
    let steer = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External),
        HandlingMode::Steer,
        &key,
    )
    .await
    .expect("steer human admission");
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;

    tokio::time::timeout(
        WAIT,
        host.handle
            .shutdown_with_report(requeue_options(crate::RequeueMaxAge::DEFAULT)),
    )
    .await
    .expect("the requeue stop finishes")
    .expect("requeue stop");
    let _ = stop_answer(steer, &session_id).await;
    let _ = stop_answer(running, &session_id).await;
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let after_restart = stop_row(&restarted.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    assert_eq!(
        (
            at_exit.stored.seed.phase,
            !at_exit.image["stop_requeue"].is_null(),
            after_restart.digest == at_exit.digest,
            requests_carrying(&restarted_client.requests(), QUEUED_HUMAN),
            runs_started_for(&events, interaction_id).len(),
        ),
        (InputLifecycleState::Abandoned, false, true, 0, 0),
        "a steer input is not requeued: terminal at the stop, never run after the restart"
    );
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

/// Requeue keeps no input bound to an objective. A member's kickoff is one
/// (its input carries the kickoff objective), and the mob records the
/// kickoff's outcome in the stopping process, so a kept kickoff would run
/// after a restart that record already calls failed. An objective-bound
/// input queued behind the running turn therefore ends as a Stop Shutdown
/// leaves it: terminal and unmarked, its waiter answered with the
/// unregistered termination, and never run after the restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_does_not_keep_an_objective_bound_input_queued_behind_the_running_turn() {
    use meerkat_runtime::input_state::InputLifecycleState;

    const UNREGISTERED: &str = "runtime session unregistered";
    let terminated = StopAnswer::RuntimeTerminated {
        reason: UNREGISTERED.to_owned(),
        kind: meerkat_core::TurnTerminalCauseKind::FatalFailure,
        detail: Some(UNREGISTERED.to_owned()),
    };
    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-requeue-objective")).await;
    let mob_id = host.handle.mob_id().clone();
    let (_identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("requeue-objective-running"),
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let key = delivery("requeue-objective-queued");
    let interaction_id = interaction(&key);
    let bound = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External)
            .with_objective_id(meerkat_core::interaction::ObjectiveId::new()),
        HandlingMode::Queue,
        &key,
    )
    .await
    .expect("objective-bound human admission");
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;
    let before_stop = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    assert_eq!(before_stop.stored.seed.phase, InputLifecycleState::Queued);

    tokio::time::timeout(
        WAIT,
        host.handle
            .shutdown_with_report(requeue_options(crate::RequeueMaxAge::DEFAULT)),
    )
    .await
    .expect("the requeue stop finishes")
    .expect("requeue stop");
    let waiter = stop_answer(bound, &session_id).await;
    let _ = stop_answer(running, &session_id).await;
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    let after_restart = stop_row(&restarted.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    assert_eq!(
        (
            waiter,
            at_exit.stored.seed.phase,
            !at_exit.image["stop_requeue"].is_null(),
            after_restart.digest == at_exit.digest,
            requests_carrying(&restarted_client.requests(), QUEUED_HUMAN),
            runs_started_for(&events, interaction_id).len(),
        ),
        (
            terminated,
            InputLifecycleState::Abandoned,
            false,
            true,
            0,
            0
        ),
        "an objective-bound input is not requeued: terminal at the stop, never run after the restart"
    );
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
}

const BOUND_HUMAN: &str = "A host question bound to the member's objective.";

/// One objective-bound input as a Shutdown leaves it and as the restarted
/// process finds it. Ids and times differ between runs, so the row's
/// unchanged-by-restart fact is compared, not its bytes.
#[derive(Debug, PartialEq)]
struct ObjectiveBoundAfterShutdown {
    waiter: StopAnswer,
    phase: meerkat_runtime::input_state::InputLifecycleState,
    outcome: serde_json::Value,
    payload_kept: bool,
    requeue_mark: bool,
    row_unchanged_by_restart: bool,
    requests_after_restart: usize,
}

/// What one Shutdown left of a plain queued host human input and of one
/// bound to an objective.
struct ObjectiveShutdownScenario {
    plain_kept: bool,
    bound: ObjectiveBoundAfterShutdown,
}

/// The stored row of `input_id` and its wire image.
async fn row_image(
    runtime_store: &Arc<dyn RuntimeStore>,
    session_id: &SessionId,
    input_id: &meerkat_core::lifecycle::InputId,
) -> (
    meerkat_runtime::input_state::StoredInputState,
    serde_json::Value,
) {
    let stored = runtime_store
        .load_input_state(&LogicalRuntimeId::for_session(session_id), input_id)
        .await
        .expect("runtime-owned input read")
        .expect("the input keeps its durable row");
    let image = serde_json::to_value(&stored).expect("the row's stored wire image");
    (stored, image)
}

/// Run one Shutdown, `requeue` or the default Stop, over a plain host human
/// input and an objective-bound one queued behind one running turn, then
/// restart and let the restarted process run whatever was kept.
async fn objective_bound_shutdown(mob_name: &str, requeue: bool) -> ObjectiveShutdownScenario {
    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some(mob_name)).await;
    let mob_id = host.handle.mob_id().clone();
    let (identity, session_id, entry) = spawn_stop_member(&host).await;
    let objective = meerkat_core::interaction::ObjectiveId::new();

    client.block();
    let requests_before = client.requests().len();
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("objective-scenario-running"),
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let plain_key = delivery("objective-scenario-plain");
    let plain = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &plain_key,
    )
    .await
    .expect("plain human admission");
    let bound_key = delivery("objective-scenario-bound");
    let bound_human = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(BOUND_HUMAN, WorkOrigin::External).with_objective_id(objective),
        HandlingMode::Queue,
        &bound_key,
    )
    .await
    .expect("objective-bound human admission");
    let plain_id = admitted_input_id(&host.runtime_store, &session_id, &plain_key).await;
    let bound_id = admitted_input_id(&host.runtime_store, &session_id, &bound_key).await;

    let options = if requeue {
        requeue_options(crate::RequeueMaxAge::DEFAULT)
    } else {
        crate::ShutdownOptions::default()
    };
    tokio::time::timeout(WAIT, host.handle.shutdown_with_report(options))
        .await
        .expect("the stop finishes")
        .expect("graceful stop");
    let plain_waiter = stop_answer(plain, &session_id).await;
    let bound_waiter = stop_answer(bound_human, &session_id).await;
    let _ = stop_answer(running, &session_id).await;
    let plain_marked = !row_image(&host.runtime_store, &session_id, &plain_id)
        .await
        .1["stop_requeue"]
        .is_null();
    let (stored, image) = row_image(&host.runtime_store, &session_id, &bound_id).await;
    drop(host);
    await_supervisor_route_release(&mob_id).await;

    let restarted_client = ScriptedClient::new(true);
    let restarted = open_stop_host(&root_path, &restarted_client, None).await;
    // A same-key replay of the plain input waits for whatever the restart
    // made of it: the kept input's run, or the stop's answer again.
    let entry = restarted
        .handle
        .get_member(&identity)
        .await
        .expect("roster read")
        .expect("the member survives the restart");
    let plain_replay = match start_stop_input(
        &restarted,
        &entry,
        WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &plain_key,
    )
    .await
    {
        Ok(replay) => stop_answer(replay, &session_id).await,
        Err(error) => StopAnswer::Refused(error.to_string()),
    };
    let requests = restarted_client.requests();
    let after = row_image(&restarted.runtime_store, &session_id, &bound_id)
        .await
        .1;
    let bound = ObjectiveBoundAfterShutdown {
        waiter: bound_waiter,
        phase: stored.seed.phase,
        outcome: serde_json::to_value(&stored.seed.terminal_outcome)
            .expect("terminal outcome wire image"),
        payload_kept: stored.state.persisted_input.is_some(),
        requeue_mark: !image["stop_requeue"].is_null(),
        row_unchanged_by_restart: after == image,
        requests_after_restart: requests_carrying(&requests, BOUND_HUMAN),
    };
    let plain_kept = matches!(plain_waiter, StopAnswer::RequeuedAcrossStop(_))
        && plain_marked
        && plain_replay == StopAnswer::Succeeded
        && requests_carrying(&requests, QUEUED_HUMAN) == 1;
    tokio::time::timeout(WAIT, restarted.handle.shutdown())
        .await
        .expect("mob shutdown finishes")
        .expect("mob shutdown");
    ObjectiveShutdownScenario { plain_kept, bound }
}

/// Requeue keeps no input bound to an objective. A member's kickoff is one
/// (its input carries the kickoff objective), and so is a turn-driven
/// member's delegated or helper work while it works an objective; their
/// owners record their outcome in the stopping process, and nothing runs
/// them again after a restart. So an objective-bound input queued behind the
/// running turn ends under a requeue Shutdown exactly as under a Stop
/// Shutdown, compared field by field against that baseline: the same waiter
/// answer, terminal, unmarked, untouched by the restart and never run after
/// it. The plain input queued beside it is kept, and runs once after the
/// restart, only under the requeue Shutdown, so the comparison tells kept
/// from stopped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_ends_objective_bound_inputs_exactly_as_a_stop_shutdown() {
    use meerkat_runtime::input_state::InputLifecycleState;

    let stop = objective_bound_shutdown("host-human-objective-stop", false).await;
    let requeue = objective_bound_shutdown("host-human-objective-requeue", true).await;
    assert_eq!(
        (stop.plain_kept, requeue.plain_kept),
        (false, true),
        "only the requeue Shutdown keeps the plain input"
    );
    assert_eq!(
        requeue.bound, stop.bound,
        "an objective-bound input ends under requeue exactly as under Stop"
    );
    assert_eq!(
        (
            requeue.bound.phase,
            requeue.bound.requeue_mark,
            requeue.bound.row_unchanged_by_restart,
            requeue.bound.requests_after_restart,
        ),
        (InputLifecycleState::Abandoned, false, true, 0),
        "terminal at the stop, never run after the restart"
    );
}

#[test]
fn requeue_max_age_refuses_zero_and_over_the_cap() {
    use crate::{RequeueMaxAge, ShutdownOptionsError};

    let over = RequeueMaxAge::CAP + Duration::from_millis(1);
    assert_eq!(
        (
            RequeueMaxAge::new(Duration::ZERO),
            RequeueMaxAge::new(over),
            RequeueMaxAge::new(RequeueMaxAge::CAP).map(RequeueMaxAge::get),
            RequeueMaxAge::new(Duration::from_millis(1)).map(RequeueMaxAge::get),
            RequeueMaxAge::default().get(),
        ),
        (
            Err(ShutdownOptionsError::RequeueMaxAgeZero),
            Err(ShutdownOptionsError::RequeueMaxAgeOverCap {
                max_age: over,
                cap: RequeueMaxAge::CAP,
            }),
            Ok(Duration::from_hours(24)),
            Ok(Duration::from_millis(1)),
            Duration::from_mins(15),
        )
    );
}

/// A requeue Shutdown whose deadline passes before a member's unregister is
/// admitted (its registration transaction is held) reports the member
/// pending and hands the unregister to its settlement. The settlement begins
/// that unregister with the Shutdown's choice: once the transaction is free
/// and the settlement settles, the input queued behind the running turn is
/// kept durably queued and marked for the next start, exactly as when
/// Shutdown begins the unregister itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requeue_shutdown_whose_admission_misses_the_deadline_still_keeps_the_queued_input() {
    use crate::runtime::{MemberShutdownOutcome, MemberUnregisterState};
    use meerkat_core::time_compat::Instant;
    use meerkat_runtime::input_state::InputLifecycleState;

    let (_root, root_path) = stop_root();
    let client = ScriptedClient::new(true);
    let host = open_stop_host(&root_path, &client, Some("host-human-requeue-settlement")).await;
    let (identity, session_id, entry) = spawn_stop_member(&host).await;

    client.block();
    let requests_before = client.requests().len();
    let running = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &delivery("requeue-settlement-running"),
    )
    .await
    .expect("running human admission");
    client.wait_for_requests(requests_before + 1).await;
    let key = delivery("requeue-settlement-queued");
    let interaction_id = interaction(&key);
    let queued = start_stop_input(
        &host,
        &entry,
        WorkSpec::new(QUEUED_HUMAN, WorkOrigin::External),
        HandlingMode::Queue,
        &key,
    )
    .await
    .expect("queued human admission");
    let input_id = admitted_input_id(&host.runtime_store, &session_id, &key).await;

    let guard = host
        .adapter
        .hold_session_registration_transaction_for_test(&session_id)
        .await;
    let report = tokio::time::timeout(
        WAIT,
        host.handle.shutdown_with_report(
            requeue_options(crate::RequeueMaxAge::DEFAULT)
                .with_deadline(Instant::now() + Duration::from_secs(1)),
        ),
    )
    .await
    .expect("the requeue stop returns at its deadline")
    .expect("requeue stop");
    let mut settlement = match report.members.get(&identity) {
        Some(MemberShutdownOutcome::UnregisterPending {
            stage, settlement, ..
        }) => {
            assert_eq!(stage, "registration_transaction_admission");
            settlement.clone()
        }
        other => panic!("expected the member pending at its unregister admission, got {other:?}"),
    };
    drop(guard);
    assert_eq!(
        settlement.settle_within(Instant::now() + WAIT).await,
        MemberUnregisterState::Settled,
        "the settlement begins and finishes the member's unregister"
    );

    let waiter = stop_answer(queued, &session_id).await;
    let _ = stop_answer(running, &session_id).await;
    let at_exit = stop_row(&host.runtime_store, &session_id, &key, &input_id).await;
    let events = stop_events(&root_path, &session_id).await;
    assert_eq!(
        observe_requeued_at_stop(waiter, &at_exit, &events, interaction_id),
        RequeuedAtStop {
            waiter: StopAnswer::RequeuedAcrossStop(input_id.clone()),
            phase: InputLifecycleState::Queued,
            outcome: serde_json::Value::Null,
            payload_kept: true,
            replay_identity_kept: true,
            requeue_mark: true,
            outbox: false,
            terminal_completion: false,
            pending_terminal_owner: false,
            runs_started: 0,
        },
        "an unregister the settlement begins keeps the queued input, as Shutdown's own does"
    );
}
