#![cfg(all(feature = "runtime-adapter", not(target_arch = "wasm32")))]
//! Member wake rows: a seated member that is not materialized in its process
//! (dormant) is woken by its first delivery, through MobMachine's wake
//! verdict and the cold restore recipe, and every refusal is typed.
//!
//! Every test runs real turn-driven members with real in-process comms over a
//! real factory, a real SQLite session store and a real head-canonical
//! SQLite runtime store. A member is made dormant inside the process with
//! `MobHandle::release_member`, so the rows do not depend on how a start
//! plans its members. The member's residency is read from the machine state
//! the actor publishes; a recording `MemberBuildPreparer` counts the builds
//! (one call per wake), and the model double records every request.

use super::*;
use crate::machines::mob_machine as mob_dsl;
use crate::runtime::{
    MemberBuildPreparation, MemberBuildPreparationRefusal, MemberBuildPrepared,
    MemberBuildPreparer, MemberBuildReport,
};

const WAIT: Duration = Duration::from_secs(30);
/// How long a test waits for a step the row says must happen soon (a stop
/// landing while a wake is held).
const STEP: Duration = Duration::from_secs(5);

type Service = meerkat_session::PersistentSessionService<meerkat::FactoryAgentBuilder>;

/// Records every model request and holds each reply until released.
#[derive(Clone)]
struct ScriptedClient {
    requests: Arc<Mutex<Vec<meerkat_client::LlmRequest>>>,
    released: tokio::sync::watch::Sender<bool>,
    started: tokio::sync::watch::Sender<usize>,
}

impl ScriptedClient {
    fn new() -> Self {
        let (released, _) = tokio::sync::watch::channel(true);
        let (started, _) = tokio::sync::watch::channel(0);
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            released,
            started,
        }
    }

    fn block(&self) {
        self.released.send_replace(false);
    }

    fn release(&self) {
        self.released.send_replace(true);
    }

    fn started_count(&self) -> usize {
        *self.started.borrow()
    }

    /// Wait until `count` model requests have started.
    async fn wait_started(&self, count: usize) {
        let mut started = self.started.subscribe();
        tokio::time::timeout(WAIT, started.wait_for(|seen| *seen >= count))
            .await
            .expect("the model request starts")
            .expect("started channel open");
    }

    /// How many recorded model requests mention `token` anywhere.
    fn requests_mentioning(&self, token: &str) -> usize {
        self.requests
            .lock()
            .expect("recorded requests")
            .iter()
            .filter(|request| format!("{:?}", request.messages).contains(token))
            .count()
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
        self.started.send_replace(sequence);
        let mut released = self.released.subscribe();
        Box::pin(async_stream::try_stream! {
            while !*released.borrow_and_update() {
                if released.changed().await.is_err() {
                    break;
                }
            }
            yield meerkat_client::LlmEvent::TextDelta {
                delta: format!("answer-{sequence}"),
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

/// What the recording preparer answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreparerAnswer {
    Prepared,
    Retryable,
    Final,
}

/// One `prepare_member_build` call, as the preparer saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedBuild {
    identity: AgentIdentity,
    session_id: SessionId,
    wired_peers: BTreeSet<AgentIdentity>,
}

/// Records every build preparation and build report; can hold a build at
/// its preparation until the test opens the gate.
struct RecordingPreparer {
    answer: Mutex<PreparerAnswer>,
    prepared: Mutex<Vec<PreparedBuild>>,
    built: Mutex<Vec<MemberBuildReport>>,
    /// Open (`true`) lets a preparation answer; closed holds it.
    gate: tokio::sync::watch::Sender<bool>,
    /// Preparations that reached the gate.
    entered: tokio::sync::watch::Sender<usize>,
}

impl RecordingPreparer {
    fn new() -> Self {
        let (gate, _) = tokio::sync::watch::channel(true);
        let (entered, _) = tokio::sync::watch::channel(0);
        Self {
            answer: Mutex::new(PreparerAnswer::Prepared),
            prepared: Mutex::new(Vec::new()),
            built: Mutex::new(Vec::new()),
            gate,
            entered,
        }
    }

    fn answer_with(&self, answer: PreparerAnswer) {
        *self.answer.lock().expect("preparer answer") = answer;
    }

    fn prepared(&self) -> Vec<PreparedBuild> {
        self.prepared.lock().expect("preparations").clone()
    }

    fn prepared_count(&self) -> usize {
        self.prepared.lock().expect("preparations").len()
    }

    fn built(&self) -> Vec<MemberBuildReport> {
        self.built.lock().expect("build reports").clone()
    }

    fn close_gate(&self) {
        self.gate.send_replace(false);
    }

    fn open_gate(&self) {
        self.gate.send_replace(true);
    }

    async fn wait_entered(&self, count: usize) {
        let mut entered = self.entered.subscribe();
        tokio::time::timeout(WAIT, entered.wait_for(|seen| *seen >= count))
            .await
            .expect("the build preparation is asked")
            .expect("entered channel open");
    }

    /// Wait until `count` build reports arrived (they are delivered off the
    /// actor after the wake answered).
    async fn wait_built(&self, count: usize) {
        tokio::time::timeout(WAIT, async {
            while self.built.lock().expect("build reports").len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the build is reported");
    }
}

#[async_trait]
impl MemberBuildPreparer for RecordingPreparer {
    async fn prepare_member_build(
        &self,
        request: MemberBuildPreparation,
    ) -> Result<MemberBuildPrepared, MemberBuildPreparationRefusal> {
        self.prepared
            .lock()
            .expect("preparations")
            .push(PreparedBuild {
                identity: request.agent_identity.clone(),
                session_id: request.session_id.clone(),
                wired_peers: request.wired_peers.clone(),
            });
        self.entered.send_modify(|entered| *entered += 1);
        let mut gate = self.gate.subscribe();
        while !*gate.borrow_and_update() {
            if gate.changed().await.is_err() {
                break;
            }
        }
        match *self.answer.lock().expect("preparer answer") {
            PreparerAnswer::Prepared => Ok(MemberBuildPrepared::Prepared),
            PreparerAnswer::Retryable => Err(MemberBuildPreparationRefusal::Retryable {
                reason: "the host is not ready for this member yet".to_string(),
            }),
            PreparerAnswer::Final => Err(MemberBuildPreparationRefusal::Final {
                reason: "the host refuses to build this member".to_string(),
            }),
        }
    }

    async fn member_built(&self, report: MemberBuildReport) {
        self.built.lock().expect("build reports").push(report);
    }
}

/// A customizer whose resume answer is versioned: every resume records the
/// version it customized the member with.
struct VersionedCustomizer {
    version: AtomicU64,
    resumes: Mutex<Vec<(AgentIdentity, SessionId, u64)>>,
}

impl VersionedCustomizer {
    fn new() -> Self {
        Self {
            version: AtomicU64::new(1),
            resumes: Mutex::new(Vec::new()),
        }
    }

    fn set_version(&self, version: u64) {
        self.version.store(version, Ordering::SeqCst);
    }

    fn resumes(&self) -> Vec<(AgentIdentity, SessionId, u64)> {
        self.resumes.lock().expect("resumes").clone()
    }
}

impl SpawnMemberCustomizer for VersionedCustomizer {
    fn customize_spawn(
        &self,
        _ctx: &SpawnCustomizationContext,
        _spec: &mut SpawnMemberSpec,
    ) -> Result<(), MobError> {
        Ok(())
    }

    fn customize_resume(
        &self,
        ctx: &SpawnCustomizationContext,
        durable: &ResumedMemberView,
        spec: &mut SpawnMemberSpec,
    ) -> Result<(), MobError> {
        self.resumes.lock().expect("resumes").push((
            spec.identity.clone(),
            durable.session_id.clone(),
            self.version.load(Ordering::SeqCst),
        ));
        self.customize_spawn(ctx, spec)
    }
}

fn wake_rows_definition(label: &str) -> MobDefinition {
    let mut definition = with_unique_mob_id(sample_definition(), label);
    let worker = definition
        .profiles
        .get_mut(&ProfileName::from("worker"))
        .and_then(ProfileBinding::as_inline_mut)
        .expect("inline worker");
    worker.model = "gpt-5.5".to_string();
    worker.runtime_mode = crate::MobRuntimeMode::TurnDriven;
    worker.tools = ToolConfig {
        comms: true,
        ..ToolConfig::default()
    };
    definition
}

struct WakeFixture {
    _root: tempfile::TempDir,
    handle: MobHandle,
    service: Arc<Service>,
    client: ScriptedClient,
    preparer: Arc<RecordingPreparer>,
    customizer: Arc<VersionedCustomizer>,
}

impl WakeFixture {
    async fn new(label: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix("meerkat-member-wake-")
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
        let client = ScriptedClient::new();
        let mut builder = meerkat::FactoryAgentBuilder::new(factory, config);
        builder.default_llm_client = Some(Arc::new(client.clone()));
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
        let preparer = Arc::new(RecordingPreparer::new());
        let customizer = Arc::new(VersionedCustomizer::new());
        let handle = MobBuilder::new(wake_rows_definition(label), MobStorage::in_memory())
            .with_session_service(service.clone())
            .with_default_llm_client(Arc::new(client.clone()))
            .with_member_build_preparer(preparer.clone())
            .with_spawn_member_customizer(customizer.clone())
            .create()
            .await
            .expect("create the mob");
        Self {
            _root: root,
            handle,
            service,
            client,
            preparer,
            customizer,
        }
    }

    /// Spawn one durable turn-driven worker. Identities are unique per test:
    /// comms routes are process-wide.
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
        self.wait_residency(&identity, Some(mob_dsl::MemberMaterializationPhase::Live))
            .await;
        identity
    }

    async fn wire(&self, a: &AgentIdentity, b: &AgentIdentity) {
        tokio::time::timeout(WAIT, self.handle.wire(a.clone(), b.clone()))
            .await
            .expect("the wire answers")
            .expect("wire the members");
    }

    fn residency(&self, identity: &AgentIdentity) -> Option<mob_dsl::MemberMaterializationPhase> {
        self.handle
            .machine_state_watch_rx
            .borrow()
            .member_materialization
            .get(&mob_dsl::AgentIdentity::from_domain(identity))
            .cloned()
    }

    async fn wait_residency(
        &self,
        identity: &AgentIdentity,
        expected: Option<mob_dsl::MemberMaterializationPhase>,
    ) {
        let key = mob_dsl::AgentIdentity::from_domain(identity);
        let mut state = self.handle.machine_state_watch_rx.clone();
        let reached = tokio::time::timeout(
            WAIT,
            state.wait_for(|state| state.member_materialization.get(&key).cloned() == expected),
        )
        .await;
        assert!(
            matches!(reached, Ok(Ok(_))),
            "member {identity} never reached residency {expected:?}; it is {:?}",
            self.residency(identity)
        );
    }

    async fn session(&self, identity: &AgentIdentity) -> SessionId {
        self.handle
            .resolve_bridge_session_id(identity)
            .await
            .expect("a session-bound member")
    }

    async fn entry(&self, identity: &AgentIdentity) -> RosterEntry {
        self.handle
            .get_member(identity)
            .await
            .expect("roster read")
            .expect("a seated member")
    }

    /// Release a quiescent member and wait until MobMachine holds it
    /// Dormant (the runtime's unregister reaches the mob through the
    /// composition release).
    async fn release(&self, identity: &AgentIdentity) {
        tokio::time::timeout(WAIT, self.handle.release_member(identity))
            .await
            .expect("the release answers")
            .expect("release a quiescent member");
        self.wait_residency(identity, Some(mob_dsl::MemberMaterializationPhase::Dormant))
            .await;
        let session = self.session(identity).await;
        assert!(
            !MobSessionService::live_session_actor_registered(&*self.service, &session)
                .await
                .unwrap_or(false),
            "a released member keeps no live session actor"
        );
    }

    async fn ensure_live(&self, identity: &AgentIdentity) -> Result<(), MobError> {
        tokio::time::timeout(WAIT, self.handle.ensure_member_live(identity))
            .await
            .expect("the wake answers")
    }

    async fn comms(&self, identity: &AgentIdentity) -> Arc<dyn CoreCommsRuntime> {
        let session = self.session(identity).await;
        self.service
            .comms_runtime(&session)
            .await
            .expect("a live member's comms runtime")
    }

    fn comms_name(&self, identity: &AgentIdentity) -> String {
        super::actor::render_member_comms_name(
            self.handle.mob_id().as_str(),
            "worker",
            identity.as_str(),
        )
        .expect("member comms name")
    }

    /// A raw peer message from `from`'s own comms runtime to `to`, outside
    /// the mob's peer-message API (which refuses a broken or stopped
    /// recipient itself before any send).
    async fn raw_peer_send(
        &self,
        from: &AgentIdentity,
        to: &AgentIdentity,
        body: &str,
    ) -> Result<SendReceipt, SendError> {
        let comms = self.comms(from).await;
        let route = test_peer_route(&*comms, &self.comms_name(to)).await;
        tokio::time::timeout(
            WAIT,
            CoreCommsRuntime::send(
                &*comms,
                CommsCommand::PeerMessage {
                    objective_id: None,
                    content_taint: None,
                    to: route,
                    body: body.to_string(),
                    blocks: None,
                    handling_mode: HandlingMode::Queue,
                    in_reply_to: None,
                },
            ),
        )
        .await
        .expect("the send answers")
    }

    async fn transcript(&self, session: &SessionId) -> String {
        let page = SessionServiceHistoryExt::read_history(
            self.service.as_ref(),
            session,
            meerkat_core::service::SessionHistoryQuery {
                offset: 0,
                limit: None,
            },
        )
        .await
        .expect("read the transcript");
        serde_json::to_string(&page.messages).expect("transcript json")
    }

    /// Wait until `identity`'s durable transcript holds `token`, then
    /// answer how many times it holds it.
    async fn delivered_count(&self, identity: &AgentIdentity, token: &str) -> usize {
        let session = self.session(identity).await;
        tokio::time::timeout(WAIT, async {
            loop {
                if self.transcript(&session).await.contains(token) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the message reaches the member's transcript");
        self.transcript(&session).await.matches(token).count()
    }

    /// Run one host turn on `identity` with `handling_mode` and wait for it.
    async fn host_turn(&self, identity: &AgentIdentity, text: &str, handling_mode: HandlingMode) {
        let entry = self.entry(identity).await;
        let turn = tokio::time::timeout(
            WAIT,
            self.handle.start_work_with_mode(
                entry.agent_runtime_id,
                entry.fence_token,
                WorkRef::new(),
                WorkSpec::new(text, WorkOrigin::Internal),
                handling_mode,
            ),
        )
        .await
        .expect("the delivery is admitted in time")
        .expect("the delivery is admitted");
        tokio::time::timeout(WAIT, turn.wait())
            .await
            .expect("the turn finishes")
            .expect("the turn succeeds");
    }

    async fn finish(self) {
        self.client.release();
        self.preparer.open_gate();
        let _ = tokio::time::timeout(WAIT, self.handle.shutdown()).await;
    }
}

fn token(label: &str) -> String {
    format!("{label}-token-{}", Uuid::new_v4().simple())
}

/// R1
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_message_wakes_a_released_member_once_and_is_delivered_once() {
    let fixture = WakeFixture::new("wake-r1").await;
    let sender = fixture.spawn("r1-sender").await;
    let recipient = fixture.spawn("r1-recipient").await;
    fixture.wire(&sender, &recipient).await;
    fixture.release(&recipient).await;

    let body = token("r1");
    let receipt = tokio::time::timeout(
        WAIT,
        fixture.handle.send_peer_message(
            sender.clone(),
            recipient.clone(),
            body.as_str(),
            HandlingMode::Queue,
        ),
    )
    .await
    .expect("the send answers")
    .expect("the dormant recipient is woken and the message handed off");
    assert_eq!(receipt.to, recipient);

    assert_eq!(fixture.delivered_count(&recipient, &body).await, 1);
    fixture
        .wait_residency(&recipient, Some(mob_dsl::MemberMaterializationPhase::Live))
        .await;
    assert_eq!(
        fixture.preparer.prepared_count(),
        1,
        "one wake builds the member once"
    );
    fixture.finish().await;
}

/// R2
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_peers_messaging_one_released_member_share_one_wake() {
    let fixture = WakeFixture::new("wake-r2").await;
    let first = fixture.spawn("r2-first").await;
    let second = fixture.spawn("r2-second").await;
    let recipient = fixture.spawn("r2-recipient").await;
    fixture.wire(&first, &recipient).await;
    fixture.wire(&second, &recipient).await;
    fixture.release(&recipient).await;

    let first_body = token("r2-first");
    let second_body = token("r2-second");
    let (first_sent, second_sent) = tokio::join!(
        fixture.handle.send_peer_message(
            first.clone(),
            recipient.clone(),
            first_body.as_str(),
            HandlingMode::Queue,
        ),
        fixture.handle.send_peer_message(
            second.clone(),
            recipient.clone(),
            second_body.as_str(),
            HandlingMode::Queue,
        ),
    );
    first_sent.expect("the first sender gets its receipt");
    second_sent.expect("the second sender gets its receipt");

    assert_eq!(fixture.delivered_count(&recipient, &first_body).await, 1);
    assert_eq!(fixture.delivered_count(&recipient, &second_body).await, 1);
    assert_eq!(
        fixture.preparer.prepared_count(),
        1,
        "concurrent senders join one wake"
    );
    fixture.finish().await;
}

/// R112 (final refusal) and R4: a broken member answers peers typed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_final_preparation_refusal_breaks_the_member_and_peers_get_member_broken() {
    let fixture = WakeFixture::new("wake-r4").await;
    let sender = fixture.spawn("r4-sender").await;
    let recipient = fixture.spawn("r4-recipient").await;
    fixture.wire(&sender, &recipient).await;
    fixture.release(&recipient).await;

    fixture.preparer.answer_with(PreparerAnswer::Final);
    let refused = fixture.ensure_live(&recipient).await;
    assert!(
        matches!(refused, Err(MobError::MemberRestoreFailed { ref member_id, .. }) if member_id == &recipient),
        "a final refusal records the member broken, typed: {refused:?}"
    );
    assert_eq!(
        fixture.residency(&recipient),
        None,
        "a broken member holds no residency phase"
    );
    assert!(
        fixture
            .handle
            .get_member(&recipient)
            .await
            .expect("roster read")
            .is_some(),
        "a broken member stays seated"
    );

    let sent = fixture
        .raw_peer_send(&sender, &recipient, &token("r4"))
        .await;
    assert!(
        matches!(
            sent,
            Err(SendError::PeerWakeRefused {
                refusal: meerkat_core::comms::PeerWakeRefusal::MemberBroken { .. },
                ..
            })
        ),
        "a peer message to a broken member is refused member_broken, never PeerNotFound: {sent:?}"
    );
    fixture.finish().await;
}

/// R26
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_of_a_released_member_never_wake_it() {
    let fixture = WakeFixture::new("wake-r26").await;
    let member = fixture.spawn("r26-member").await;
    fixture.release(&member).await;

    let _ = fixture.handle.list_members().await;
    let _ = fixture.handle.roster().await;
    let _ = fixture.entry(&member).await;
    let _ = fixture.handle.member_status(&member).await;
    let _ = fixture.session(&member).await;

    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Dormant),
        "reads leave the member dormant"
    );
    assert_eq!(fixture.preparer.prepared_count(), 0, "no read builds it");
    fixture.finish().await;
}

/// R30
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_steer_to_a_released_member_wakes_it() {
    let fixture = WakeFixture::new("wake-r30").await;
    let member = fixture.spawn("r30-member").await;
    fixture.release(&member).await;

    let body = token("r30");
    fixture.host_turn(&member, &body, HandlingMode::Steer).await;

    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Live)
    );
    assert_eq!(fixture.preparer.prepared_count(), 1);
    assert!(
        fixture.client.requests_mentioning(&body) >= 1,
        "the steered content reaches the woken member's model"
    );
    fixture.finish().await;
}

/// R50
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_turn_to_a_released_member_wakes_it_and_runs_once() {
    let fixture = WakeFixture::new("wake-r50").await;
    let member = fixture.spawn("r50-member").await;
    fixture.release(&member).await;

    let body = token("r50");
    fixture.host_turn(&member, &body, HandlingMode::Queue).await;

    assert_eq!(fixture.delivered_count(&member, &body).await, 1);
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Live)
    );
    assert_eq!(fixture.preparer.prepared_count(), 1);
    fixture.finish().await;
}

/// R26 (operator wake) and the attempt-keyed waiters (G13).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ensure_member_live_wakes_a_released_member_once_for_concurrent_callers() {
    let fixture = WakeFixture::new("wake-ensure").await;
    let member = fixture.spawn("ensure-member").await;
    fixture.release(&member).await;

    let (first, second) = tokio::join!(fixture.ensure_live(&member), fixture.ensure_live(&member));
    first.expect("the first caller sees the member ready");
    second.expect("the second caller joins the same wake");
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Live)
    );
    assert_eq!(fixture.preparer.prepared_count(), 1, "one wake, one build");
    fixture.finish().await;
}

/// R65 and R66
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn release_keeps_the_seat_and_the_wake_restores_the_same_incarnation() {
    let fixture = WakeFixture::new("wake-r65").await;
    let member = fixture.spawn("r65-member").await;
    let before = fixture.entry(&member).await;
    let session_before = fixture.session(&member).await;

    fixture.release(&member).await;
    let seated = fixture.entry(&member).await;
    assert_eq!(
        seated.generation, before.generation,
        "the seat keeps its generation"
    );
    assert_eq!(
        seated.fence_token, before.fence_token,
        "the seat keeps its fence"
    );

    fixture
        .ensure_live(&member)
        .await
        .expect("the released member wakes");
    let after = fixture.entry(&member).await;
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.fence_token, before.fence_token);
    assert_eq!(after.agent_runtime_id, before.agent_runtime_id);
    assert_eq!(
        fixture.session(&member).await,
        session_before,
        "the wake restores the same session"
    );
    fixture.finish().await;
}

/// R65: a release has nothing to release on a dormant member.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_release_of_a_dormant_member_changes_nothing() {
    let fixture = WakeFixture::new("wake-release-twice").await;
    let member = fixture.spawn("release-twice").await;
    fixture.release(&member).await;

    tokio::time::timeout(WAIT, fixture.handle.release_member(&member))
        .await
        .expect("the release answers")
        .expect("releasing a dormant member is a no-op");
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Dormant)
    );
    assert_eq!(fixture.preparer.prepared_count(), 0);
    fixture.finish().await;
}

/// R65: a release never interrupts work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_release_with_a_run_in_flight_is_refused_retryably() {
    let fixture = WakeFixture::new("wake-release-busy").await;
    let member = fixture.spawn("release-busy").await;
    fixture.client.block();
    let started_before = fixture.client.started_count();
    let entry = fixture.entry(&member).await;
    let turn = fixture
        .handle
        .start_work_with_mode(
            entry.agent_runtime_id,
            entry.fence_token,
            WorkRef::new(),
            WorkSpec::new(token("busy"), WorkOrigin::Internal),
            HandlingMode::Queue,
        )
        .await
        .expect("the turn is admitted");
    fixture.client.wait_started(started_before + 1).await;

    let refused = tokio::time::timeout(WAIT, fixture.handle.release_member(&member))
        .await
        .expect("the release answers");
    assert!(
        matches!(refused, Err(MobError::LifecycleOperationPending { .. })),
        "a release with work in flight is refused retryably: {refused:?}"
    );

    fixture.client.release();
    tokio::time::timeout(WAIT, turn.wait())
        .await
        .expect("the turn finishes")
        .expect("the turn succeeds");
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Live),
        "the refused release changed nothing"
    );
    fixture.finish().await;
}

/// R98
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_mob_refuses_peer_messages_to_dormant_and_live_members_alike() {
    let fixture = WakeFixture::new("wake-r98").await;
    let sender = fixture.spawn("r98-sender").await;
    let live = fixture.spawn("r98-live").await;
    let dormant = fixture.spawn("r98-dormant").await;
    fixture.wire(&sender, &live).await;
    fixture.wire(&sender, &dormant).await;
    fixture.release(&dormant).await;

    tokio::time::timeout(WAIT, fixture.handle.stop())
        .await
        .expect("the stop answers")
        .expect("stop the mob");

    for recipient in [&dormant, &live] {
        let sent = fixture
            .raw_peer_send(&sender, recipient, &token("r98-stopped"))
            .await;
        assert!(
            matches!(
                sent,
                Err(SendError::PeerWakeRefused {
                    refusal: meerkat_core::comms::PeerWakeRefusal::MobNotRunning,
                    ..
                })
            ),
            "a stopped mob refuses {recipient} mob_not_running: {sent:?}"
        );
    }

    tokio::time::timeout(WAIT, fixture.handle.resume())
        .await
        .expect("the resume answers")
        .expect("resume the mob");
    for recipient in [&dormant, &live] {
        let body = token("r98-resumed");
        fixture
            .raw_peer_send(&sender, recipient, &body)
            .await
            .expect("after the resume the message is delivered");
        assert_eq!(fixture.delivered_count(recipient, &body).await, 1);
    }
    fixture.finish().await;
}

/// R109
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wake_prepares_the_member_build_once_and_reports_it_built() {
    let fixture = WakeFixture::new("wake-r109").await;
    let peer = fixture.spawn("r109-peer").await;
    let member = fixture.spawn("r109-member").await;
    fixture.wire(&peer, &member).await;
    assert_eq!(
        fixture.preparer.prepared_count(),
        0,
        "a spawn is no wake: the preparer is asked for wakes and restores"
    );
    fixture.release(&member).await;
    let session = fixture.session(&member).await;

    fixture
        .ensure_live(&member)
        .await
        .expect("the released member wakes");

    let prepared = fixture.preparer.prepared();
    assert_eq!(
        prepared,
        vec![PreparedBuild {
            identity: member.clone(),
            session_id: session.clone(),
            wired_peers: BTreeSet::from([peer.clone()]),
        }],
        "one preparation, with the member, its session and its wired peers"
    );
    fixture.preparer.wait_built(1).await;
    let built = fixture.preparer.built();
    assert_eq!(built.len(), 1, "one build report per build");
    assert_eq!(built[0].agent_identity, member);
    assert_eq!(built[0].session_id, session);
    fixture.finish().await;
}

/// R112 (retryable refusal)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retryable_preparation_refusal_leaves_the_member_dormant_until_the_next_delivery() {
    let fixture = WakeFixture::new("wake-r112").await;
    let member = fixture.spawn("r112-member").await;
    fixture.release(&member).await;

    fixture.preparer.answer_with(PreparerAnswer::Retryable);
    let refused = fixture.ensure_live(&member).await;
    assert!(
        matches!(
            refused,
            Err(MobError::MemberWakeRefused {
                refusal: crate::MemberWakeRefusal::HostPreparationPending,
                ..
            })
        ),
        "a retryable refusal is the typed retryable wake refusal: {refused:?}"
    );
    fixture
        .wait_residency(&member, Some(mob_dsl::MemberMaterializationPhase::Dormant))
        .await;

    fixture.preparer.answer_with(PreparerAnswer::Prepared);
    fixture
        .ensure_live(&member)
        .await
        .expect("the next delivery wakes the member");
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Live)
    );
    assert_eq!(fixture.preparer.prepared_count(), 2);
    fixture.finish().await;
}

/// R113
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_during_a_wake_abandons_the_wake_without_fail_stop() {
    let fixture = WakeFixture::new("wake-r113").await;
    let member = fixture.spawn("r113-member").await;
    fixture.release(&member).await;

    fixture.preparer.close_gate();
    let handle = fixture.handle.clone();
    let woken = member.clone();
    let wake = tokio::spawn(async move { handle.ensure_member_live(&woken).await });
    fixture.preparer.wait_entered(1).await;
    fixture
        .wait_residency(&member, Some(mob_dsl::MemberMaterializationPhase::Waking))
        .await;

    let handle = fixture.handle.clone();
    let stop = tokio::spawn(async move { handle.stop().await });
    let stopped = tokio::time::timeout(STEP, async {
        loop {
            if matches!(fixture.handle.status().await, Ok(MobState::Stopped)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    fixture.preparer.open_gate();
    assert!(
        stopped.is_ok(),
        "the stop lands while the wake is held, not after it"
    );

    let answer = tokio::time::timeout(WAIT, wake)
        .await
        .expect("the wake answers")
        .expect("the wake task joins");
    assert!(
        matches!(
            answer,
            Err(MobError::MemberWakeRefused {
                refusal: crate::MemberWakeRefusal::MobNotRunning,
                ..
            })
        ),
        "the abandoned wake answers mob_not_running: {answer:?}"
    );
    tokio::time::timeout(WAIT, stop)
        .await
        .expect("the stop answers")
        .expect("the stop task joins")
        .expect("the stop succeeds: no fail-stop");
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Dormant),
        "nothing was materialized"
    );

    tokio::time::timeout(WAIT, fixture.handle.resume())
        .await
        .expect("the resume answers")
        .expect("the mob resumes normally");
    fixture
        .ensure_live(&member)
        .await
        .expect("the next delivery wakes the member");
    assert_eq!(
        fixture.residency(&member),
        Some(mob_dsl::MemberMaterializationPhase::Live)
    );
    fixture.finish().await;
}

/// R123: a wake rebuilds the member through the customizer's resume hook,
/// with what the customizer answers now, never a previous build's spec.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wake_asks_the_customizer_again_with_its_current_answer() {
    let fixture = WakeFixture::new("wake-r123").await;
    let member = fixture.spawn("r123-member").await;
    fixture.release(&member).await;
    let session = fixture.session(&member).await;
    let resumes_before = fixture.customizer.resumes().len();

    fixture.customizer.set_version(2);
    fixture
        .ensure_live(&member)
        .await
        .expect("the released member wakes");

    let resumes = fixture.customizer.resumes();
    assert_eq!(
        resumes.len(),
        resumes_before + 1,
        "the wake asks the customizer's resume hook exactly once"
    );
    assert_eq!(
        resumes.last(),
        Some(&(member.clone(), session, 2)),
        "the member is rebuilt with the customizer's current answer"
    );
    fixture.finish().await;
}
