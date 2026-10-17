//! An adaptive layer decision that carries Anthropic `thinking: {"type":
//! "disabled"}` although the released layer-decision schema does not offer it
//! (a hand-built decision, or provider output that skipped the schema), taken
//! through the real agent factory the way a mobpack layer is provisioned:
//! parsed as the planning turn parses it, compiled, built into a layer mob and
//! spawned. On a model that refuses disabled thinking the member's spawn fails
//! with the typed request-shaping refusal; on a model that accepts it the
//! member's requests carry `Disabled`. It never becomes another thinking mode.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meerkat::{AgentFactory, Config, FactoryAgentBuilder, PersistentSessionService};
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::Message;
use meerkat_core::lifecycle::run_primitive::{AnthropicThinkingConfig, ProviderTag};
use meerkat_core::types::HandlingMode;
use meerkat_mob::adaptive::{
    AdaptivePolicy, AdaptiveRunId, CompileContext, LayerDecision, SchemaRegistry, compile_layer,
};
use meerkat_mob::{MemberTurnOptions, MobBuilder, MobStorage};

const STEP: Duration = Duration::from_secs(30);

/// Answers every request with a short end of turn and records the thinking
/// configuration each request carried.
#[derive(Default)]
struct RecordingClient {
    thinking: Mutex<Vec<Option<AnthropicThinkingConfig>>>,
}

#[async_trait::async_trait]
impl LlmClient for RecordingClient {
    fn project_replay_messages(&self, messages: &[Message]) -> Result<Vec<Message>, LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(
        &'a self,
        request: &'a LlmRequest,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<LlmEvent, LlmError>> + Send + 'a>>
    {
        let thinking = match request.provider_params.as_ref() {
            Some(ProviderTag::Anthropic(tag)) => tag.thinking.clone(),
            _ => None,
        };
        self.thinking.lock().unwrap().push(thinking);
        let events = vec![
            LlmEvent::TextDelta {
                delta: "done".to_string(),
                meta: None,
            },
            LlmEvent::UsageUpdate {
                usage: meerkat_core::TurnUsage::host_declared(
                    meerkat_core::Provider::Anthropic,
                    &request.model,
                    meerkat_core::Usage::default(),
                ),
            },
            LlmEvent::Done {
                outcome: LlmDoneOutcome::Success {
                    stop_reason: meerkat_core::StopReason::EndTurn,
                },
            },
        ];
        Box::pin(futures::stream::iter(events.into_iter().map(Ok)))
    }

    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::Anthropic
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

async fn build_service(
    root: &Path,
    client: Arc<RecordingClient>,
) -> (
    Arc<PersistentSessionService<FactoryAgentBuilder>>,
    Arc<meerkat_runtime::MeerkatMachine>,
) {
    let (_manifest, persistence) = meerkat::open_realm_persistence_in(
        root,
        "adaptive-disabled-thinking-realm",
        Some(meerkat_store::RealmBackend::Sqlite),
        Some(meerkat_store::RealmOrigin::Explicit),
    )
    .await
    .expect("open realm persistence");
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    let factory = AgentFactory::new(root.join("sessions"))
        .runtime_root(root.join("realm"))
        .project_root(&project)
        .comms(true);
    let mut builder = FactoryAgentBuilder::new(factory, Config::default());
    builder.default_llm_client = Some(client);
    meerkat::surface::build_runtime_backed_service_with_default_reconfigure_host(
        builder,
        8,
        persistence,
        root.join("config-state.json"),
    )
    .expect("compose the runtime-backed service")
}

/// A one-member layer decision whose inline profile runs `model` with
/// disabled thinking, as JSON the planning turn would receive.
fn decision_with_disabled_thinking(model: &str) -> serde_json::Value {
    serde_json::json!({
        "decision": "run_layer",
        "reason": "verify",
        "plan": {
            "id": "verify",
            "objective": "Verify the finding.",
            "shape": { "kind": "solo" },
            "profiles": {
                "solo": {
                    "inline": {
                        "model": model,
                        "provider": "anthropic",
                        "runtime_mode": "turn_driven",
                        "external_addressable": true,
                        "tools": { "comms": true },
                        "provider_params": {
                            "provider_tag": {
                                "provider": "anthropic",
                                "thinking": { "type": "disabled" }
                            }
                        }
                    }
                }
            },
            "collector": {
                "profile": "solo",
                "output_schema": { "inline": { "type": "object" } }
            }
        }
    })
}

/// Parse and compile the decision as the adaptive driver does, then build the
/// layer mob and spawn its members as mobpack layer provisioning does; each
/// row is the spawned member or the row's failure text.
async fn provision_layer(
    root: &Path,
    model: &str,
    client: Arc<RecordingClient>,
) -> (
    meerkat_mob::MobHandle,
    Vec<Result<meerkat_mob::SpawnResult, String>>,
) {
    let LayerDecision::RunLayer { plan, .. } =
        serde_json::from_value(decision_with_disabled_thinking(model))
            .expect("the decision parses")
    else {
        panic!("a run_layer decision");
    };
    // A run id of its own per layer: the layer mob's id, and its members'
    // comms names, derive from it.
    let context = CompileContext {
        adaptive_run_id: AdaptiveRunId::new(format!(
            "disabled-thinking-{}",
            uuid::Uuid::new_v4().simple()
        ))
        .expect("run id"),
        attempt: 1,
        schema_registry: SchemaRegistry::default(),
        profile_templates: BTreeMap::new(),
        previous_layer_result: None,
    };
    let policy = AdaptivePolicy {
        allow_inline_profiles: true,
        ..AdaptivePolicy::default()
    };
    let compiled = compile_layer(&plan, &context, &policy).expect("the layer compiles");

    let (service, adapter) = build_service(root, client).await;
    let handle = MobBuilder::from_mobpack(
        compiled.definition.clone(),
        BTreeMap::new(),
        MobStorage::in_memory(),
    )
    .expect("layer mob builder")
    .with_session_service(service)
    .with_runtime_adapter(adapter)
    .create()
    .await
    .expect("create the layer mob");
    let results = handle
        .spawn_many(compiled.spawn_specs.clone())
        .await
        .expect("the spawn rows return")
        .into_iter()
        .map(|row| row.map_err(|failure| failure.to_string()))
        .collect();
    (handle, results)
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_thinking_on_a_model_that_refuses_it_fails_the_member_spawn_typed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let client = Arc::new(RecordingClient::default());
    let (_handle, results) =
        provision_layer(temp.path(), "claude-opus-5-5", Arc::clone(&client)).await;
    assert_eq!(results.len(), 1, "one collector member");
    let error = results
        .into_iter()
        .next()
        .expect("one row")
        .expect_err("the spawn is refused");
    assert!(
        error.contains("does not accept thinking type 'disabled'; accepted: [adaptive]"),
        "{error}"
    );
    assert!(
        client.thinking.lock().unwrap().is_empty(),
        "nothing reached the model"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_thinking_on_a_model_that_accepts_it_reaches_every_request() {
    for model in ["claude-opus-4-8", "claude-opus-5"] {
        let temp = tempfile::tempdir().expect("temp dir");
        let client = Arc::new(RecordingClient::default());
        let (handle, results) = provision_layer(temp.path(), model, Arc::clone(&client)).await;
        let spawned = results
            .into_iter()
            .next()
            .expect("one row")
            .unwrap_or_else(|error| panic!("{model}: the member spawns: {error}"));
        let member = handle
            .member(&spawned.agent_identity)
            .await
            .expect("member handle");
        let turn = member
            .start_turn(
                "verify",
                HandlingMode::Queue,
                MemberTurnOptions::default(),
                None,
            )
            .await
            .expect("start a turn");
        tokio::time::timeout(STEP, turn.wait())
            .await
            .expect("the turn completes")
            .expect("the turn succeeds");
        let thinking = client.thinking.lock().unwrap().clone();
        assert!(!thinking.is_empty(), "{model}: the member called the model");
        assert!(
            thinking
                .iter()
                .all(|config| config == &Some(AnthropicThinkingConfig::Disabled)),
            "{model}: every request carries disabled thinking: {thinking:?}"
        );
    }
}
