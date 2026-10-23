//! Early budget warnings through the real agent loop.
//!
//! `budget_warning` announces that a run is approaching a limit. It fires once
//! when a dimension reaches the agent's `budget_warning_threshold` fraction of
//! its limit, while the run still has room, and again (at 100%) only when the
//! limit itself stops the run.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::too_many_lines
)]

use crate as meerkat_core;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::budget::BudgetLimits;
use meerkat_core::event::BudgetType;
use meerkat_core::{
    AgentBuilder, AgentError, AgentEvent, AgentLlmClient, AgentSessionStore, AgentToolDispatcher,
    AssistantBlock, LlmStreamResult, Message, Provider, StopReason, ToolCallView, ToolDef,
    ToolResult, TurnUsage, Usage,
};
use serde_json::json;
use tokio::sync::mpsc;

const MODEL: &str = "test-anthropic-model";

/// One scripted provider reply, its accounting, and how long it takes.
struct ScriptedReply {
    blocks: Vec<AssistantBlock>,
    stop_reason: StopReason,
    input_tokens: u64,
    output_tokens: u64,
    latency: Duration,
}

fn text_reply(input_tokens: u64, output_tokens: u64, latency: Duration) -> ScriptedReply {
    ScriptedReply {
        blocks: vec![AssistantBlock::Text {
            text: "done".to_string(),
            meta: None,
        }],
        stop_reason: StopReason::EndTurn,
        input_tokens,
        output_tokens,
        latency,
    }
}

fn lookup_calls(ids: &[&str]) -> ScriptedReply {
    ScriptedReply {
        blocks: ids
            .iter()
            .map(|id| AssistantBlock::ToolUse {
                id: (*id).to_string(),
                name: "lookup".into(),
                args: serde_json::value::RawValue::from_string("{}".to_string()).unwrap(),
                meta: None,
            })
            .collect(),
        stop_reason: StopReason::ToolUse,
        input_tokens: 10,
        output_tokens: 1,
        latency: Duration::ZERO,
    }
}

struct ScriptedClient {
    replies: Mutex<VecDeque<ScriptedReply>>,
}

#[async_trait]
impl AgentLlmClient for ScriptedClient {
    async fn stream_response(
        &self,
        _messages: &[Message],
        _tools: &[Arc<ToolDef>],
        _max_tokens: u32,
        _temperature: Option<f32>,
        _provider_params: Option<&meerkat_core::lifecycle::run_primitive::ProviderParamsOverride>,
    ) -> Result<LlmStreamResult, AgentError> {
        let reply =
            self.replies.lock().unwrap().pop_front().ok_or_else(|| {
                AgentError::InternalError("scripted client exhausted".to_string())
            })?;
        if !reply.latency.is_zero() {
            tokio::time::sleep(reply.latency).await;
        }
        let usage = TurnUsage::host_declared(
            Provider::Anthropic,
            MODEL,
            Usage {
                input_tokens: reply.input_tokens,
                output_tokens: reply.output_tokens,
                ..Usage::default()
            },
        )
        .into_inner();
        Ok(LlmStreamResult::new(reply.blocks, reply.stop_reason, usage))
    }

    fn provider(&self) -> Provider {
        Provider::Anthropic
    }

    fn model(&self) -> &'static str {
        MODEL
    }
}

struct LookupTool;

#[async_trait]
impl AgentToolDispatcher for LookupTool {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::new([Arc::new(ToolDef {
            audience: Default::default(),
            name: "lookup".into(),
            description: "returns a fixed observation".to_string(),
            input_schema: json!({ "type": "object" }),
            provenance: None,
        })])
    }

    async fn dispatch(
        &self,
        call: ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        Ok(ToolResult::new(call.id.to_string(), "observation".to_string(), false).into())
    }
}

struct NoopStore;

#[async_trait]
impl AgentSessionStore for NoopStore {
    async fn save(&self, _session: &meerkat_core::Session) -> Result<(), AgentError> {
        Ok(())
    }

    async fn load(&self, _id: &str) -> Result<Option<meerkat_core::Session>, AgentError> {
        Ok(None)
    }
}

type ScriptedAgent = meerkat_core::Agent<ScriptedClient, LookupTool, NoopStore>;

async fn scripted_agent(
    replies: Vec<ScriptedReply>,
    limits: BudgetLimits,
    threshold: f32,
) -> ScriptedAgent {
    let mut builder = AgentBuilder::new()
        .with_turn_state_handle(Arc::new(
            crate::agent::test_turn_state_handle::TestTurnStateHandle::new(),
        ))
        .budget(limits);
    builder.config.budget_warning_threshold = threshold;
    builder
        .build_standalone(
            Arc::new(ScriptedClient {
                replies: Mutex::new(replies.into()),
            }),
            Arc::new(LookupTool),
            Arc::new(NoopStore),
        )
        .await
}

/// One `BudgetWarning` as `(budget_type, used, limit, percent)`.
type Warning = (BudgetType, u64, u64, f32);

/// Run one turn and return its terminal cause and the budget warnings it
/// published.
async fn run_collecting_warnings(
    agent: &mut ScriptedAgent,
    prompt: &str,
) -> (Option<meerkat_core::TurnTerminalCauseKind>, Vec<Warning>) {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
    let result = agent
        .run_with_events(prompt.to_string().into(), tx)
        .await
        .expect("the run completes");
    let mut warnings = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::BudgetWarning {
            budget_type,
            used,
            limit,
            percent,
        } = event
        {
            warnings.push((budget_type, used, limit, percent));
        }
    }
    (result.terminal_cause_kind, warnings)
}

fn assert_percent(warning: &Warning, expected: f32) {
    assert!(
        (warning.3 - expected).abs() < 1e-6,
        "expected percent {expected}, got {warning:?}"
    );
}

/// Tokens: a turn that reaches 85% of the token budget warns once, the run
/// goes on, and a later turn below the limit does not warn again.
#[tokio::test]
async fn token_budget_warns_once_at_the_threshold() {
    let mut agent = scripted_agent(
        vec![
            text_reply(800, 50, Duration::ZERO),
            text_reply(90, 10, Duration::ZERO),
        ],
        BudgetLimits::unlimited().with_max_tokens(1000),
        0.8,
    )
    .await;

    let (cause, warnings) = run_collecting_warnings(&mut agent, "first").await;
    assert_eq!(cause, None, "the budget still has room after the warning");
    let [warning] = warnings.as_slice() else {
        panic!("one early token warning is expected: {warnings:?}");
    };
    assert_eq!(
        (warning.0, warning.1, warning.2),
        (BudgetType::Tokens, 850, 1000)
    );
    assert_percent(warning, 0.85);

    let (cause, warnings) = run_collecting_warnings(&mut agent, "second").await;
    assert_eq!(cause, None);
    assert!(
        warnings.is_empty(),
        "the token warning fires once per budget: {warnings:?}"
    );
}

/// Tool calls: a batch that reaches half of the tool-call budget warns once
/// under a 0.5 threshold, before the next model call.
#[tokio::test]
async fn tool_call_budget_warns_once_at_the_threshold() {
    let mut agent = scripted_agent(
        vec![
            lookup_calls(&["call-1", "call-2"]),
            text_reply(10, 1, Duration::ZERO),
        ],
        BudgetLimits::unlimited().with_max_tool_calls(4),
        0.5,
    )
    .await;

    let (cause, warnings) = run_collecting_warnings(&mut agent, "look it up").await;
    assert_eq!(cause, None, "the tool-call budget still has room");
    let [warning] = warnings.as_slice() else {
        panic!("one early tool-call warning is expected: {warnings:?}");
    };
    assert_eq!(
        (warning.0, warning.1, warning.2),
        (BudgetType::ToolCalls, 2, 4)
    );
    assert_percent(warning, 0.5);
}

/// Time: a model call that runs past the threshold of the agent-lifetime
/// time budget warns once, while the run is still far from the limit.
#[tokio::test]
async fn time_budget_warns_once_at_the_threshold() {
    let mut agent = scripted_agent(
        vec![text_reply(10, 1, Duration::from_millis(600))],
        BudgetLimits::unlimited().with_max_duration(Duration::from_secs(10)),
        0.05,
    )
    .await;

    let (cause, warnings) = run_collecting_warnings(&mut agent, "take a while").await;
    assert_eq!(cause, None, "the time budget still has room");
    let [warning] = warnings.as_slice() else {
        panic!("one early time warning is expected: {warnings:?}");
    };
    assert_eq!((warning.0, warning.2), (BudgetType::Time, 10));
    assert!(
        (0.05..1.0).contains(&warning.3),
        "the time warning reports the elapsed fraction: {warning:?}"
    );
}
