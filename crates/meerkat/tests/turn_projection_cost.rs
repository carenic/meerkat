//! Opt-in end-to-end benchmark for fresh runtime-backed turns (#24).
//!
//! Each sample builds a fresh runtime-backed session outside the timed span:
//! an ephemeral `MeerkatMachine`, an `EphemeralSessionService` with a scripted
//! model and one no-op tool, and the controller credential publication the
//! runtime requires. The timed span is one fresh admission through its
//! completion: the prompt is admitted, the run starts, the model calls the
//! tool three times in one batch, the results go back, the model answers and
//! the run completes. `turn_projection_cost_fixture_is_valid` runs one sample
//! in the ordinary lane and checks its outcome; the timing test is ignored:
//!
//! ```text
//! ./scripts/repo-cargo test --locked --release -p meerkat \
//!   --test turn_projection_cost --no-run --message-format=json
//! "$BINARY" --exact turn_projection_cost_b2 --ignored --nocapture --test-threads=1
//! ```
//!
//! It prints one JSON line with the raw per-turn nanoseconds. A single run is
//! not speedup evidence: compare two binaries in interleaved pairs on the
//! same lane and CPU set.

#![cfg(all(feature = "anthropic", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use meerkat::{AgentFactory, FactoryAgentBuilder};
use meerkat_core::connection::AuthCredentialIdentity;
use meerkat_core::service::{CreateSessionRequest, DeferredPromptPolicy, InitialTurnPolicy};
use meerkat_core::{
    AgentError, AgentSessionStore, AgentToolDispatcher, Config, ControllerModelSelection, Message,
    Provider, Session, SessionId, SessionLlmIdentity, StopReason, ToolCallView, ToolDef,
    ToolDispatchOutcome, ToolError, ToolResult,
};
use meerkat_llm_core::{
    LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest, LlmStream, PreparedLlmRequest,
};
use meerkat_runtime::SessionServiceRuntimeExt;
use meerkat_runtime::completion::CompletionOutcome;
use meerkat_runtime::input::{Input, PromptInput};
use meerkat_runtime::meerkat_machine::MeerkatMachine;
use meerkat_session::EphemeralSessionService;

const MODEL: &str = "claude-sonnet-4-5";
const ENDPOINT: &str = "https://turn-projection-cost.invalid/messages";
const TOOL: &str = "noop_record";
const TOOL_CALLS: usize = 3;
const ANSWER: &str = "bench turn completed";
const WARMUP: usize = 10;
const SAMPLES: usize = 100;
const CREDENTIAL_PUBLICATION_WAIT: Duration = Duration::from_secs(30);
const SAMPLE_DEADLINE: Duration = Duration::from_secs(30);

fn identity() -> SessionLlmIdentity {
    SessionLlmIdentity {
        model: MODEL.into(),
        provider: Provider::Anthropic,
        self_hosted_server_id: None,
        provider_params: None,
        auth_binding: None,
    }
}

fn credential() -> AuthCredentialIdentity {
    serde_json::from_value(
        serde_json::json!({"realm":"turn-projection-cost", "account":"scripted-controller"}),
    )
    .expect("nonsecret identity of the scripted transport")
}

fn selection() -> ControllerModelSelection {
    ControllerModelSelection::new(
        identity(),
        credential(),
        "scripted-profile".into(),
        "scripted-transport".into(),
    )
}

fn done(reason: StopReason) -> Result<LlmEvent, LlmError> {
    Ok(LlmEvent::Done {
        outcome: LlmDoneOutcome::Success {
            stop_reason: reason,
        },
    })
}

/// First request: three tool calls in one batch. Second request: the answer.
struct ScriptedModel {
    requests: AtomicUsize,
}

impl ScriptedModel {
    fn script(&self) -> LlmStream<'_> {
        let index = self.requests.fetch_add(1, Ordering::Relaxed);
        assert!(index < 2, "one turn makes exactly two model requests");
        if index == 0 {
            let mut events = (0..TOOL_CALLS)
                .map(|i| {
                    Ok(LlmEvent::ToolCallComplete {
                        id: format!("noop-{i}"),
                        name: TOOL.into(),
                        args: serde_json::json!({}),
                        meta: None,
                    })
                })
                .collect::<Vec<_>>();
            events.push(done(StopReason::ToolUse));
            Box::pin(futures::stream::iter(events))
        } else {
            Box::pin(futures::stream::iter([
                Ok(LlmEvent::TextDelta {
                    delta: ANSWER.into(),
                    meta: None,
                }),
                done(StopReason::EndTurn),
            ]))
        }
    }
}

#[async_trait]
impl LlmClient for ScriptedModel {
    fn plain_model_route(
        &self,
        logical_model: &str,
    ) -> Result<meerkat_llm_core::PlainModelRoute, meerkat_core::ControllerFactsUnavailable> {
        if logical_model != MODEL {
            return Err(meerkat_core::ControllerFactsUnavailable);
        }
        meerkat_llm_core::PlainModelRoute::new(ENDPOINT, MODEL)
    }

    fn controller_model_selection(&self) -> Option<ControllerModelSelection> {
        Some(selection())
    }

    fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(&'a self, _: &'a LlmRequest) -> LlmStream<'a> {
        self.script()
    }

    fn stream_prepared<'a>(&'a self, _: &'a PreparedLlmRequest) -> LlmStream<'a> {
        self.script()
    }

    fn provider(&self) -> Provider {
        Provider::Anthropic
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

struct NoopTool {
    calls: AtomicUsize,
}

#[async_trait]
impl AgentToolDispatcher for NoopTool {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::from([Arc::new(ToolDef::new(
            TOOL,
            "Deterministic no-effect benchmark tool",
            serde_json::json!({"type": "object"}),
        ))])
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        if call.name != TOOL {
            return Err(ToolError::not_found(call.name));
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(ToolResult::new(call.id.to_owned(), "done".to_owned(), false).into())
    }
}

#[derive(Default)]
struct SessionStore(Mutex<HashMap<SessionId, Session>>);

#[async_trait]
impl AgentSessionStore for SessionStore {
    async fn save(&self, session: &Session) -> Result<(), AgentError> {
        self.0
            .lock()
            .expect("session store")
            .insert(session.id().clone(), session.clone());
        Ok(())
    }

    async fn load(&self, id: &str) -> Result<Option<Session>, AgentError> {
        Ok(SessionId::parse(id)
            .ok()
            .and_then(|id| self.0.lock().expect("session store").get(&id).cloned()))
    }
}

struct Fixture {
    machine: Arc<MeerkatMachine>,
    service: Arc<EphemeralSessionService<FactoryAgentBuilder>>,
    session: SessionId,
    model: Arc<ScriptedModel>,
    tool: Arc<NoopTool>,
}

impl Fixture {
    async fn new() -> Self {
        let machine = Arc::new(MeerkatMachine::ephemeral());
        let model = Arc::new(ScriptedModel {
            requests: AtomicUsize::new(0),
        });
        let tool = Arc::new(NoopTool {
            calls: AtomicUsize::new(0),
        });
        let mut config = Config::default();
        config.agent.max_turns = Some(4);
        let mut builder = FactoryAgentBuilder::new(AgentFactory::minimal(), config);
        builder.default_llm_client = Some(model.clone());
        builder.default_tool_dispatcher = Some(tool.clone());
        builder.default_session_store = Some(Arc::new(SessionStore::default()));
        let service = Arc::new(EphemeralSessionService::new(builder, 2));
        let created = meerkat::surface::materialize_ephemeral_runtime_session(
            &service,
            &machine,
            CreateSessionRequest {
                model: MODEL.into(),
                prompt: "".into(),
                injected_context: Vec::new(),
                system_prompt: meerkat_core::config::SystemPromptOverride::Inherit,
                max_tokens: None,
                event_tx: None,
                initial_turn: InitialTurnPolicy::Defer,
                deferred_prompt_policy: DeferredPromptPolicy::Discard,
                build: None,
                labels: None,
            },
            false,
        )
        .await
        .unwrap();
        let session = created.session_id;
        let actor = service.live_session_actor_witness(&session).await.unwrap();
        let pin = service
            .pin_controller_client_for_actor(&actor)
            .await
            .unwrap();
        assert_eq!(pin.selection(), &selection());
        let owner = machine.generated_auth_lease_handle();
        meerkat_core::publish_token_lifecycle_acquired_for_identity_within(
            &owner,
            pin.selection().credential(),
            &meerkat_core::auth::PersistedTokens::api_key("synthetic-turn-projection-credential"),
            tokio::time::Instant::now() + CREDENTIAL_PUBLICATION_WAIT,
        )
        .await
        .unwrap();
        Self {
            machine,
            service,
            session,
            model,
            tool,
        }
    }

    /// One fresh admission through its completion; returns the elapsed span.
    async fn fresh_turn(&self) -> Duration {
        let input = Input::Prompt(PromptInput::new("Read the record three times.", None));
        let start = Instant::now();
        let (accepted, completion) = self
            .machine
            .accept_input_with_completion(&self.session, input)
            .await
            .unwrap();
        assert!(accepted.is_accepted());
        let outcome = completion
            .expect("fresh admission completion handle")
            .wait()
            .await
            .expect("fresh turn completion");
        let elapsed = start.elapsed();
        let CompletionOutcome::Completed(result) = outcome else {
            panic!("the fresh turn must complete normally")
        };
        assert_eq!(result.text, ANSWER);
        assert_eq!(result.session_id, self.session);
        assert!(result.terminal_cause_kind.is_none());
        assert_eq!(result.tool_calls as usize, TOOL_CALLS);
        assert_eq!(self.model.requests.load(Ordering::Relaxed), 2);
        assert_eq!(self.tool.calls.load(Ordering::Relaxed), TOOL_CALLS);
        elapsed
    }

    async fn close(self) {
        self.machine.retire_runtime(&self.session).await.unwrap();
        self.service.try_shutdown().await.unwrap();
    }
}

async fn sample() -> Duration {
    let fixture = Fixture::new().await;
    let elapsed = fixture.fresh_turn().await;
    fixture.close().await;
    elapsed
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_projection_cost_fixture_is_valid() {
    tokio::time::timeout(SAMPLE_DEADLINE, sample())
        .await
        .expect("one fresh turn completes in time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "opt-in timing; see the module docs"]
async fn turn_projection_cost_b2() {
    for _ in 0..WARMUP {
        tokio::time::timeout(SAMPLE_DEADLINE, sample())
            .await
            .unwrap();
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let elapsed = tokio::time::timeout(SAMPLE_DEADLINE, sample())
            .await
            .unwrap();
        samples.push(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX));
    }
    println!(
        "{}",
        serde_json::json!({
            "bench": "b2",
            "case": "fresh_runtime_turn_three_tools",
            "warmup": WARMUP,
            "n": samples.len(),
            "ns": samples,
        })
    );
}
