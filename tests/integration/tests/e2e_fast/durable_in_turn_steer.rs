//! Durable in-turn Steer delivery on the full in-process RPC stack.
//!
//! A turn-driven mob member is held mid-turn (inside a tool call, or inside a
//! model call) by a scripted provider and a gate tool. A Steer prompt that
//! carries only a typed `SystemNotice` append (the shape of a detached job
//! completion) is admitted while the turn runs:
//!
//! - with a post-tool boundary still ahead, the NEXT model request of the SAME
//!   run carries the notice as a transcript row, every later request of the
//!   run carries it too, the committed transcript holds it exactly once
//!   between the boundary's tool results and the next assistant message, and
//!   no follow-up turn runs;
//! - when the model is streaming and then returns tool calls, the notice waits
//!   for that next boundary of the same run;
//! - when the model is streaming and then answers without another boundary,
//!   exactly one follow-up turn carries the notice.
//!
//! Every other content steer (a peer message, request or lifecycle notice, a
//! text-only prompt, an external event, a local flow step) joins the running
//! turn the same way, and a peer steer emits exactly one
//! `peer_content_ingested`. Three peer steers admitted inside one gated
//! boundary each land exactly once, in arrival order.
//!
//! The scripted client is installed through BOTH
//! `SessionRuntime::set_default_llm_client` and
//! `MobMcpState::with_default_llm_client`.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat::{AgentFactory, Config};
use meerkat_client::types::LlmStream;
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::lifecycle::{ConversationAppend, ConversationAppendRole, CoreRenderable};
use meerkat_core::types::SessionId;
use meerkat_core::{ConfigRuntime, ContentInput, HandlingMode, MemoryConfigStore, Message};
use meerkat_mob::{
    AgentIdentity, BoundedResultSpec, MobDefinition, MobHandle, SpawnMemberSpec, WorkOrigin,
    WorkSpec,
};
use meerkat_mob_mcp::MobMcpState;
use meerkat_rpc::protocol::{RpcId, RpcRequest};
use meerkat_rpc::router::{MethodRouter, NotificationSink};
use meerkat_rpc::session_runtime::SessionRuntime;
use serde_json::{Value, json};
use tokio::sync::mpsc;

const TARGET: &str = "durable-steer-target";
const TURN_PROMPT: &str = "DURABLE-STEER-TURN run several tools";
const FINAL_TEXT: &str = "DURABLE-STEER-FINAL";
const FOLLOW_UP_ACK: &str = "DURABLE-STEER-FOLLOW-UP";
const NOTICE_TOKEN: &str = "DURABLE-NOTICE-TOKEN-8R";
const PEER_MESSAGE_TOKEN: &str = "DURABLE-PEER-MESSAGE-TOKEN-5V";
const PEER_REQUEST_TOKEN: &str = "DURABLE-PEER-REQUEST-TOKEN-6W";
const PEER_LIFECYCLE_TOKEN: &str = "DURABLE-PEER-LIFECYCLE-TOKEN-7X";
const TEXT_PROMPT_TOKEN: &str = "DURABLE-TEXT-PROMPT-TOKEN-8Y";
const EXTERNAL_EVENT_TOKEN: &str = "DURABLE-EXTERNAL-EVENT-TOKEN-9Z";
const FLOW_STEP_TOKEN: &str = "DURABLE-FLOW-STEP-TOKEN-2A";
const LATE_PEER_TOKEN: &str = "DURABLE-LATE-PEER-TOKEN-3B";
const ORDERED_PEER_TOKENS: [&str; 3] = [
    "DURABLE-PEER-FIRST-4C",
    "DURABLE-PEER-SECOND-5D",
    "DURABLE-PEER-THIRD-6E",
];
const WAIT: Duration = Duration::from_secs(60);
/// How long each `probe_step` tool call takes in the three-steer scenario,
/// so a durable steer that prepares after the previous one joined usually
/// catches the following boundary. The assertions hold either way.
const ORDERED_STEP_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    /// Hold the turn inside the `probe_block` tool call: the post-tool model
    /// boundary is open.
    Tool,
    /// Hold the turn inside the second model request; the model then returns
    /// tool calls, so a later boundary of the same run opens.
    ModelThenTools,
    /// Hold the turn inside the second model request; the model then answers,
    /// so the run ends without another boundary.
    ModelThenAnswer,
}

#[derive(Default)]
struct Hold {
    reached: AtomicBool,
    released: AtomicBool,
}

impl Hold {
    async fn hold(&self) {
        self.reached.store(true, Ordering::SeqCst);
        while !self.released.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

struct GateTools {
    defs: Arc<[Arc<meerkat_core::ToolDef>]>,
    hold: Arc<Hold>,
    step_delay: Duration,
}

impl GateTools {
    fn new(hold: Arc<Hold>, step_delay: Duration) -> Self {
        let defs = ["probe_block", "probe_step"]
            .iter()
            .map(|name| {
                Arc::new(meerkat_core::ToolDef {
                    audience: Default::default(),
                    name: (*name).into(),
                    description: format!("{name} durable steer probe tool"),
                    input_schema: json!({ "type": "object" }),
                    provenance: None,
                })
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            defs,
            hold,
            step_delay,
        }
    }
}

#[async_trait::async_trait]
impl meerkat_core::AgentToolDispatcher for GateTools {
    fn tools(&self) -> Arc<[Arc<meerkat_core::ToolDef>]> {
        Arc::clone(&self.defs)
    }

    async fn dispatch(
        &self,
        call: meerkat_core::ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        if call.name == "probe_block" {
            self.hold.hold().await;
        } else if !self.step_delay.is_zero() {
            tokio::time::sleep(self.step_delay).await;
        }
        Ok(
            meerkat_core::ToolResult::new(call.id.to_string(), format!("{} ok", call.name), false)
                .into(),
        )
    }
}

struct GateToolsFactory {
    inner: Arc<dyn meerkat_core::service::MobToolsFactory>,
    gate_tools: Arc<dyn meerkat_core::AgentToolDispatcher>,
}

#[async_trait::async_trait]
impl meerkat_core::service::MobToolsFactory for GateToolsFactory {
    async fn build_mob_tools(
        &self,
        args: meerkat_core::service::MobToolsBuildArgs,
    ) -> Result<Arc<dyn meerkat_core::AgentToolDispatcher>, Box<dyn std::error::Error + Send + Sync>>
    {
        let inner = self.inner.build_mob_tools(args).await?;
        Ok(Arc::new(meerkat_core::DynamicToolComposite::new(vec![
            inner,
            Arc::clone(&self.gate_tools),
        ])))
    }
}

#[derive(Clone, Debug)]
struct RecordedRequest {
    in_turn: bool,
    messages: Vec<Message>,
}

struct ScriptedProvider {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    hold: Arc<Hold>,
    gate: Gate,
}

fn usage(model: &str) -> LlmEvent {
    LlmEvent::UsageUpdate {
        usage: meerkat_core::TurnUsage::host_declared(
            meerkat_core::Provider::Anthropic,
            model,
            meerkat_core::Usage::default(),
        ),
    }
}

fn text_events(model: &str, text: &str) -> Vec<LlmEvent> {
    vec![
        LlmEvent::TextDelta {
            delta: text.to_string(),
            meta: None,
        },
        usage(model),
        LlmEvent::Done {
            outcome: LlmDoneOutcome::Success {
                stop_reason: meerkat_core::StopReason::EndTurn,
            },
        },
    ]
}

fn tool_events(model: &str, id: &str, name: &str) -> Vec<LlmEvent> {
    vec![
        LlmEvent::ToolCallComplete {
            id: id.to_string(),
            name: name.to_string(),
            args: json!({}),
            meta: None,
        },
        usage(model),
        LlmEvent::Done {
            outcome: LlmDoneOutcome::Success {
                stop_reason: meerkat_core::StopReason::ToolUse,
            },
        },
    ]
}

#[async_trait::async_trait]
impl LlmClient for ScriptedProvider {
    fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(&'a self, request: &'a LlmRequest) -> LlmStream<'a> {
        let rendered = format!("{:?}", request.messages);
        let model = request.model.clone();
        let in_turn_prompt = rendered.contains(TURN_PROMPT);
        let turn_ended = rendered.contains(FINAL_TEXT);
        // Step 0: an unrelated member request; 1: a follow-up turn; 2..6:
        // the gated turn's requests, keyed by the last tool call they answer.
        let step: u8 = if !in_turn_prompt {
            0
        } else if turn_ended {
            1
        } else if rendered.contains("toolu_p3") {
            2
        } else if rendered.contains("toolu_p2") {
            3
        } else if rendered.contains("toolu_p1") {
            4
        } else if rendered.contains("toolu_p0") {
            5
        } else {
            6
        };
        self.requests.lock().unwrap().push(RecordedRequest {
            in_turn: in_turn_prompt && !turn_ended,
            messages: request.messages.clone(),
        });
        let hold = Arc::clone(&self.hold);
        let gate = self.gate;
        Box::pin(futures::StreamExt::flat_map(
            futures::stream::once(async move {
                match step {
                    0 => text_events(&model, "OTHER-ACK"),
                    1 => text_events(&model, FOLLOW_UP_ACK),
                    2 => text_events(&model, FINAL_TEXT),
                    3 => tool_events(&model, "toolu_p3", "probe_step"),
                    4 => tool_events(&model, "toolu_p2", "probe_step"),
                    5 => match gate {
                        Gate::Tool => tool_events(&model, "toolu_p1", "probe_block"),
                        Gate::ModelThenTools => {
                            hold.hold().await;
                            tool_events(&model, "toolu_p1", "probe_step")
                        }
                        Gate::ModelThenAnswer => {
                            hold.hold().await;
                            text_events(&model, FINAL_TEXT)
                        }
                    },
                    _ => tool_events(&model, "toolu_p0", "probe_step"),
                }
            }),
            |events| futures::stream::iter(events.into_iter().map(Ok)),
        ))
    }

    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::Anthropic
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

async fn make_stack(
    root: &std::path::Path,
    client: Arc<dyn LlmClient>,
    gate_tools: Arc<dyn meerkat_core::AgentToolDispatcher>,
) -> (MethodRouter, Arc<MobMcpState>) {
    let project_root = root.join("project-root");
    let context_root = root.join("context-root");
    for dir in [&project_root, &context_root] {
        std::fs::create_dir_all(dir).expect("create project/context root");
        std::fs::write(dir.join("AGENTS.md"), "# Durable steer\n").expect("write AGENTS.md");
    }
    let runtime_root = root.join("runtime-root");
    let factory = AgentFactory::new(runtime_root.join("factory-store"))
        .user_config_root(root.join("user-config"))
        .runtime_root(runtime_root.clone())
        .project_root(project_root)
        .context_root(context_root)
        .builtins(false)
        .shell(false)
        .comms(true)
        .mob(true);
    let config = Config::default();
    let store: Arc<dyn meerkat::SessionStore> = Arc::new(meerkat::MemoryStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let persistence = meerkat::PersistenceBundle::new(
        store,
        Arc::new(meerkat_runtime::InMemoryRuntimeStore::new()),
        blob_store,
    )
    .expect("construct runtime authority");
    let runtime = SessionRuntime::new(
        factory,
        config.clone(),
        64,
        persistence,
        NotificationSink::noop(),
    )
    .expect("compose the RPC session runtime");
    runtime.set_default_llm_client(Some(Arc::clone(&client)));
    let config_store: Arc<dyn meerkat_core::ConfigStore> =
        Arc::new(MemoryConfigStore::new(config, meerkat_models::canonical()));
    runtime.set_config_runtime(Arc::new(ConfigRuntime::new(
        Arc::clone(&config_store),
        runtime_root.join("config_state.json"),
    )));
    let mob_state = Arc::new(
        MobMcpState::new_with_runtime_adapter(
            runtime.session_service(),
            Some(runtime.runtime_adapter()),
            meerkat_mob::MobControlPrincipal::Owner,
        )
        .expect("construct runtime authority")
        .with_default_llm_client(Some(client)),
    );
    let runtime = Arc::new(runtime);
    let (notif_tx, _notif_rx) = mpsc::channel(256);
    let router = MethodRouter::new_with_mob_state(
        Arc::clone(&runtime),
        config_store,
        NotificationSink::new(notif_tx),
        Arc::clone(&mob_state),
    );
    // The router installs its own mob tool surface factory; wrap it after.
    runtime.set_mob_tools(Arc::new(GateToolsFactory {
        inner: Arc::new(meerkat_mob_mcp::AgentMobToolSurfaceFactory::new(
            Arc::clone(&mob_state),
        )),
        gate_tools,
    }));
    (router, mob_state)
}

fn mob_definition(mob_id: &str) -> MobDefinition {
    serde_json::from_value(json!({
        "id": mob_id,
        "profiles": {
            "keeper": {
                "model": "claude-sonnet-4-5",
                "tools": { "comms": true, "mob": true },
                "peer_description": "Durable steer target",
                "runtime_mode": "turn_driven"
            }
        },
        "wiring": { "auto_wire_orchestrator": false, "role_wiring": [] }
    }))
    .expect("durable steer mob definition")
}

async fn session_history(router: &MethodRouter, session_id: &SessionId) -> Vec<Value> {
    let params = serde_json::value::RawValue::from_string(
        json!({ "session_id": session_id.to_string(), "limit": 200 }).to_string(),
    )
    .unwrap();
    let response = router
        .dispatch(RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RpcId::Num(1)),
            method: "session/history".to_string(),
            params: Some(params),
        })
        .await
        .expect("session/history dispatch");
    assert!(
        response.error.is_none(),
        "session/history error: {:?}",
        response.error
    );
    let history: Value = serde_json::from_str(response.result.as_ref().expect("result").get())
        .expect("history JSON");
    history["messages"].as_array().cloned().unwrap_or_default()
}

/// A Steer prompt with no text and one typed Generic system notice.
fn durable_notice_steer() -> meerkat_runtime::Input {
    let mut prompt = meerkat_runtime::PromptInput::new(
        "",
        Some(
            meerkat_core::lifecycle::run_primitive::RuntimeTurnMetadata {
                handling_mode: Some(HandlingMode::Steer),
                ..Default::default()
            },
        ),
    );
    prompt.typed_turn_appends = vec![ConversationAppend {
        role: ConversationAppendRole::SystemNotice,
        content: CoreRenderable::SystemNotice {
            kind: meerkat_core::SystemNoticeKind::Generic,
            body: Some(NOTICE_TOKEN.to_string()),
            blocks: Vec::new(),
        },
        identity: None,
        runtime_source: None,
    }];
    meerkat_runtime::Input::Prompt(prompt)
}

fn steer_header(source: meerkat_runtime::InputOrigin) -> meerkat_runtime::InputHeader {
    meerkat_runtime::InputHeader {
        ingress_context: None,
        retained_resume: None,
        authority_association: None,
        id: meerkat_core::lifecycle::InputId::new(),
        timestamp: chrono::Utc::now(),
        source,
        durability: meerkat_runtime::InputDurability::Durable,
        visibility: meerkat_runtime::InputVisibility::default(),
        idempotency_key: None,
        supersession_key: None,
        correlation_id: None,
    }
}

fn steer_metadata() -> meerkat_core::lifecycle::run_primitive::RuntimeTurnMetadata {
    meerkat_core::lifecycle::run_primitive::RuntimeTurnMetadata {
        handling_mode: Some(HandlingMode::Steer),
        ..Default::default()
    }
}

/// A peer steer as the comms bridge admits it: a canonical peer id, no
/// directed interaction, no system prompts or injected context.
fn peer_steer(convention: meerkat_runtime::PeerConvention, token: &str) -> meerkat_runtime::Input {
    meerkat_runtime::Input::Peer(meerkat_runtime::PeerInput {
        directed_interaction_id: None,
        objective_id: None,
        system_prompts: Vec::new(),
        injected_context: Vec::new(),
        sender_taint: None,
        header: steer_header(meerkat_runtime::InputOrigin::Peer {
            peer_id: uuid::Uuid::new_v4().to_string(),
            display_identity: None,
            runtime_id: None,
        }),
        convention: Some(convention),
        content: ContentInput::Text(token.to_string()),
        payload: None,
        handling_mode: Some(HandlingMode::Steer),
    })
}

fn peer_message_steer(token: &str) -> meerkat_runtime::Input {
    peer_steer(meerkat_runtime::PeerConvention::Message, token)
}

fn peer_request_steer(token: &str) -> meerkat_runtime::Input {
    peer_steer(
        meerkat_runtime::PeerConvention::Request {
            request_id: format!("req-{}", uuid::Uuid::new_v4()),
            intent: "durable_steer_probe".into(),
        },
        token,
    )
}

fn peer_lifecycle_steer(token: &str) -> meerkat_runtime::Input {
    peer_steer(
        meerkat_runtime::PeerConvention::Lifecycle {
            kind: meerkat_core::comms::PeerLifecycleKind::PeerAdded,
        },
        token,
    )
}

fn text_prompt_steer(token: &str) -> meerkat_runtime::Input {
    meerkat_runtime::Input::Prompt(meerkat_runtime::PromptInput::new(
        token,
        Some(steer_metadata()),
    ))
}

fn external_event_steer(token: &str) -> meerkat_runtime::Input {
    meerkat_runtime::Input::ExternalEvent(meerkat_runtime::ExternalEventInput {
        header: steer_header(meerkat_runtime::InputOrigin::External {
            source_name: "webhook".into(),
        }),
        event_type: "webhook".into(),
        payload: json!({ "body": token }),
        blocks: None,
        handling_mode: HandlingMode::Steer,
        render_metadata: None,
        objective_id: None,
    })
}

fn flow_step_steer(token: &str) -> meerkat_runtime::Input {
    meerkat_runtime::mob_adapter::create_flow_step_input(
        "durable-steer-step",
        ContentInput::Text(token.to_string()),
        "durable-steer-flow",
        0,
        Some(steer_metadata()),
    )
}

fn notice_rows(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|message| {
            matches!(message, Message::SystemNotice(notice)
                if notice.body.as_deref() == Some(NOTICE_TOKEN))
        })
        .count()
}

/// Messages of one model request that carry `token`, whatever their role.
fn token_rows(messages: &[Message], token: &str) -> usize {
    messages
        .iter()
        .filter(|message| format!("{message:?}").contains(token))
        .count()
}

struct Outcome {
    requests: Vec<RecordedRequest>,
    /// Index of the first gated-turn request recorded after the steer.
    first_after_gate: usize,
    history: Vec<Value>,
    /// `peer_content_ingested` events the target session published.
    peer_ingested: usize,
}

async fn run_scenario(gate: Gate, steers: Vec<meerkat_runtime::Input>) -> Outcome {
    run_scenario_with(gate, steers, Duration::ZERO).await
}

async fn run_scenario_with(
    gate: Gate,
    steers: Vec<meerkat_runtime::Input>,
    step_delay: Duration,
) -> Outcome {
    let temp = tempfile::TempDir::new().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hold = Arc::new(Hold::default());
    let client: Arc<dyn LlmClient> = Arc::new(ScriptedProvider {
        requests: Arc::clone(&requests),
        hold: Arc::clone(&hold),
        gate,
    });
    let gate_tools = Arc::new(GateTools::new(Arc::clone(&hold), step_delay));
    let (router, mob_state) = make_stack(temp.path(), client, gate_tools).await;
    let runtime = mob_state
        .session_service()
        .acquire_runtime_adapter(None)
        .expect("acquire runtime adapter")
        .expect("runtime-backed stack");
    let mob_id = format!("durable-steer-{}", uuid::Uuid::new_v4().simple());
    let mob_id = mob_state
        .mob_create_definition(mob_definition(&mob_id))
        .await
        .expect("create mob");
    let handle: MobHandle = mob_state.handle_for(&mob_id).await.expect("mob handle");
    handle
        .spawn_spec(SpawnMemberSpec::new("keeper", AgentIdentity::from(TARGET)))
        .await
        .expect("spawn target");
    let session = handle
        .resolve_bridge_session_id(&AgentIdentity::from(TARGET))
        .await
        .expect("target session");
    let mut events = meerkat_mob::MobSessionService::subscribe_session_events(
        mob_state.session_service().as_ref(),
        &session,
    )
    .await
    .expect("subscribe to the target's session events");
    let peer_ingested = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ingested = Arc::clone(&peer_ingested);
    let event_reader = tokio::spawn(async move {
        while let Some(envelope) = futures::StreamExt::next(&mut events).await {
            if matches!(
                envelope.payload,
                meerkat_core::AgentEvent::PeerContentIngested { .. }
            ) {
                ingested.fetch_add(1, Ordering::SeqCst);
            }
        }
    });

    let spec = BoundedResultSpec::new("turn", 16 * 1024).expect("bounded result spec");
    let work = handle
        .start_work_for_identity_bounded(
            AgentIdentity::from(TARGET),
            WorkSpec::new(
                ContentInput::Text(TURN_PROMPT.to_string()),
                WorkOrigin::Internal,
            ),
            HandlingMode::Queue,
            spec.clone(),
        )
        .await
        .expect("start the gated turn");
    let mut turn = tokio::spawn(async move {
        tokio::time::timeout(WAIT, work.wait_bounded(spec))
            .await
            .map(|result| {
                result
                    .map(|done| done.result().result().text().to_string())
                    .map_err(|error| error.to_string())
            })
    });

    let deadline = tokio::time::Instant::now() + WAIT;
    while !hold.reached.load(Ordering::SeqCst) {
        if turn.is_finished() {
            let result = (&mut turn).await;
            panic!("the turn ended before the gate: {result:?}");
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the turn never reached the gate"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let first_after_gate = requests.lock().unwrap().len();

    for steer in steers {
        let (outcome, _completion) = runtime
            .accept_input_with_completion(&session, steer)
            .await
            .expect("steer admitted while the turn runs");
        assert!(outcome.is_accepted());
    }
    // Let the ingress prepare its boundary delivery before releasing.
    tokio::time::sleep(Duration::from_millis(300)).await;
    hold.released.store(true, Ordering::SeqCst);

    let result = turn.await.expect("turn task");
    let text = result
        .expect("turn completes in time")
        .expect("turn succeeds");
    assert_eq!(text, FINAL_TEXT);

    // Let any follow-up turn settle.
    let mut stable = 0;
    let mut last = requests.lock().unwrap().len();
    while stable < 20 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = requests.lock().unwrap().len();
        if now == last {
            stable += 1;
        } else {
            stable = 0;
            last = now;
        }
    }
    let history = session_history(&router, &session).await;
    let requests = requests.lock().unwrap().clone();
    event_reader.abort();
    let peer_ingested = peer_ingested.load(Ordering::SeqCst);
    let _ = mob_state.mob_destroy(&mob_id).await;
    Outcome {
        requests,
        first_after_gate,
        history,
        peer_ingested,
    }
}

fn history_role(row: &Value) -> &str {
    row["role"].as_str().unwrap_or_default()
}

fn history_notice_positions(history: &[Value]) -> Vec<usize> {
    history
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            history_role(row) == "system_notice" && row.to_string().contains(NOTICE_TOKEN)
        })
        .map(|(index, _)| index)
        .collect()
}

/// Transcript rows that carry `token`, whatever their role.
fn history_token_positions(history: &[Value], token: &str) -> Vec<usize> {
    history
        .iter()
        .enumerate()
        .filter(|(_, row)| row.to_string().contains(token))
        .map(|(index, _)| index)
        .collect()
}

fn assistant_run_ids(history: &[Value]) -> std::collections::BTreeSet<String> {
    history
        .iter()
        .filter(|row| history_role(row) == "block_assistant")
        .filter_map(|row| row["run_id"].as_str().map(str::to_string))
        .collect()
}

fn assert_in_turn_delivery(outcome: &Outcome) {
    let requests = &outcome.requests;
    assert!(
        requests.iter().all(|request| request.in_turn),
        "no follow-up turn runs"
    );
    for request in &requests[..outcome.first_after_gate] {
        assert_eq!(notice_rows(&request.messages), 0);
    }
    let after = &requests[outcome.first_after_gate..];
    assert!(!after.is_empty());
    for request in after {
        assert_eq!(
            notice_rows(&request.messages),
            1,
            "the next request of the same run and every later one carry the notice once"
        );
    }
    let positions = history_notice_positions(&outcome.history);
    assert_eq!(
        positions.len(),
        1,
        "one transcript row: {:?}",
        outcome.history
    );
    let at = positions[0];
    assert_eq!(history_role(&outcome.history[at - 1]), "tool_results");
    assert_eq!(history_role(&outcome.history[at + 1]), "block_assistant");
    assert_eq!(
        assistant_run_ids(&outcome.history).len(),
        1,
        "every assistant message belongs to the one running run"
    );
}

/// A content steer joined the running turn at the gated boundary: every
/// request of the run after the gate carries it exactly once, the transcript
/// holds it once between the boundary's tool results and the next assistant
/// message, no follow-up turn runs, and `peer_ingested` ingestion events were
/// published.
fn assert_steer_joins_the_running_turn(outcome: &Outcome, token: &str, peer_ingested: usize) {
    let requests = &outcome.requests;
    assert!(
        requests.iter().all(|request| request.in_turn),
        "no follow-up turn runs"
    );
    for request in &requests[..outcome.first_after_gate] {
        assert_eq!(token_rows(&request.messages, token), 0);
    }
    let after = &requests[outcome.first_after_gate..];
    assert!(after.len() >= 3, "three requests follow the gate");
    for (index, request) in after.iter().enumerate() {
        assert_eq!(
            token_rows(&request.messages, token),
            1,
            "request {index} after the gate carries the steer once, as every later one of the run"
        );
    }
    let positions = history_token_positions(&outcome.history, token);
    assert_eq!(
        positions.len(),
        1,
        "one transcript row: {:?}",
        outcome.history
    );
    let at = positions[0];
    assert_eq!(history_role(&outcome.history[at - 1]), "tool_results");
    assert_eq!(history_role(&outcome.history[at + 1]), "block_assistant");
    assert_eq!(
        assistant_run_ids(&outcome.history).len(),
        1,
        "every assistant message belongs to the one running run"
    );
    assert_eq!(
        outcome.peer_ingested, peer_ingested,
        "peer_content_ingested events"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_durable_steer_joins_the_running_turn_after_its_tool_results() {
    let outcome = run_scenario(Gate::Tool, vec![durable_notice_steer()]).await;
    assert_in_turn_delivery(&outcome);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_durable_steer_waits_while_the_model_streams_for_the_next_boundary() {
    let outcome = run_scenario(Gate::ModelThenTools, vec![durable_notice_steer()]).await;
    assert_in_turn_delivery(&outcome);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_durable_steer_without_a_later_boundary_takes_exactly_one_follow_up() {
    let outcome = run_scenario(Gate::ModelThenAnswer, vec![durable_notice_steer()]).await;
    let in_turn_hits = outcome
        .requests
        .iter()
        .filter(|request| request.in_turn && notice_rows(&request.messages) > 0)
        .count();
    assert_eq!(in_turn_hits, 0, "no boundary opened in the running turn");
    let follow_ups = outcome
        .requests
        .iter()
        .filter(|request| !request.in_turn)
        .collect::<Vec<_>>();
    assert_eq!(follow_ups.len(), 1, "exactly one follow-up turn");
    assert_eq!(notice_rows(&follow_ups[0].messages), 1);
    assert_eq!(history_notice_positions(&outcome.history).len(), 1);
    assert_eq!(assistant_run_ids(&outcome.history).len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_peer_message_steer_joins_the_running_turn_once() {
    let outcome = run_scenario(Gate::Tool, vec![peer_message_steer(PEER_MESSAGE_TOKEN)]).await;
    assert_steer_joins_the_running_turn(&outcome, PEER_MESSAGE_TOKEN, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_peer_request_steer_joins_the_running_turn_once() {
    let outcome = run_scenario(Gate::Tool, vec![peer_request_steer(PEER_REQUEST_TOKEN)]).await;
    assert_steer_joins_the_running_turn(&outcome, PEER_REQUEST_TOKEN, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_peer_lifecycle_steer_joins_the_running_turn_once() {
    let outcome = run_scenario(Gate::Tool, vec![peer_lifecycle_steer(PEER_LIFECYCLE_TOKEN)]).await;
    assert_steer_joins_the_running_turn(&outcome, PEER_LIFECYCLE_TOKEN, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_text_prompt_steer_joins_the_running_turn_once() {
    let outcome = run_scenario(Gate::Tool, vec![text_prompt_steer(TEXT_PROMPT_TOKEN)]).await;
    assert_steer_joins_the_running_turn(&outcome, TEXT_PROMPT_TOKEN, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_external_event_steer_joins_the_running_turn_once() {
    let outcome = run_scenario(Gate::Tool, vec![external_event_steer(EXTERNAL_EVENT_TOKEN)]).await;
    assert_steer_joins_the_running_turn(&outcome, EXTERNAL_EVENT_TOKEN, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_flow_step_steer_joins_the_running_turn_once() {
    let outcome = run_scenario(Gate::Tool, vec![flow_step_steer(FLOW_STEP_TOKEN)]).await;
    assert_steer_joins_the_running_turn(&outcome, FLOW_STEP_TOKEN, 0);
}

/// A peer steer admitted while the model streams its final answer has no
/// later boundary: exactly one follow-up turn writes it, and it is ingested
/// once.
#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_peer_steer_without_a_later_boundary_takes_exactly_one_follow_up() {
    let outcome = run_scenario(
        Gate::ModelThenAnswer,
        vec![peer_message_steer(LATE_PEER_TOKEN)],
    )
    .await;
    let in_turn_hits = outcome
        .requests
        .iter()
        .filter(|request| request.in_turn && token_rows(&request.messages, LATE_PEER_TOKEN) > 0)
        .count();
    assert_eq!(in_turn_hits, 0, "no boundary opened in the running turn");
    let follow_ups = outcome
        .requests
        .iter()
        .filter(|request| !request.in_turn)
        .collect::<Vec<_>>();
    assert_eq!(follow_ups.len(), 1, "exactly one follow-up turn");
    assert_eq!(token_rows(&follow_ups[0].messages, LATE_PEER_TOKEN), 1);
    assert_eq!(
        history_token_positions(&outcome.history, LATE_PEER_TOKEN).len(),
        1
    );
    assert_eq!(assistant_run_ids(&outcome.history).len(), 2);
    assert_eq!(outcome.peer_ingested, 1, "peer_content_ingested events");
}

/// Three peer steers admitted inside one gated boundary each
/// land exactly once, in arrival order, and once a request carries one, every
/// later request carries it once. Where each lands (one per boundary, the
/// last possibly in the follow-up turn) is pinned exactly by the runtime
/// test `three_peer_steers_in_one_gated_boundary_land_once_each_in_arrival_order`.
#[tokio::test(flavor = "multi_thread")]
async fn e2e_fast_three_peer_steers_in_one_gated_boundary_land_once_each_in_arrival_order() {
    let outcome = run_scenario_with(
        Gate::Tool,
        ORDERED_PEER_TOKENS
            .iter()
            .map(|token| peer_message_steer(token))
            .collect(),
        ORDERED_STEP_DELAY,
    )
    .await;
    let mut rows = Vec::new();
    let mut first_requests = Vec::new();
    for token in ORDERED_PEER_TOKENS {
        let positions = history_token_positions(&outcome.history, token);
        assert_eq!(
            positions.len(),
            1,
            "{token}: one transcript row: {:?}",
            outcome.history
        );
        rows.push(positions[0]);
        let first = outcome
            .requests
            .iter()
            .position(|request| token_rows(&request.messages, token) > 0)
            .unwrap_or_else(|| panic!("{token} reaches a request"));
        assert!(first >= outcome.first_after_gate);
        for request in &outcome.requests[first..] {
            assert_eq!(
                token_rows(&request.messages, token),
                1,
                "{token} stays in every later request exactly once"
            );
        }
        first_requests.push(first);
    }
    assert!(
        rows.windows(2).all(|pair| pair[0] < pair[1]),
        "transcript rows in arrival order: {rows:?}"
    );
    assert!(
        first_requests.windows(2).all(|pair| pair[0] <= pair[1]),
        "first delivery in arrival order: {first_requests:?}"
    );
    assert_eq!(outcome.peer_ingested, 3, "peer_content_ingested events");
}
