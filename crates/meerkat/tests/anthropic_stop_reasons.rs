//! Anthropic stop reasons through a real agent loop.
//!
//! These tests run the real `AnthropicClient` against a local Messages stub
//! that records every request body, and pin what each stop reason does to the
//! run:
//!
//! - a filled context window and a refusal fail the run typed: nothing of the
//!   turn is committed and none of its tool calls run, even a call the
//!   refusal cut mid-input;
//! - an unknown stop reason is retried, then fails typed;
//! - a paused server-tool turn is committed, and the next request replays its
//!   server-tool blocks.
//! - a native web search the provider refused comes back from the
//!   `web_search` executor failed, without its text as the answer (#1887);
//! - a refusal's `stop_details` reaches the run failure as the typed reason's
//!   provider detail, and a refusal without it still reports its typed stop.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use meerkat::{
    AgentBuilder, AgentFactory, AgentToolDispatcher, AnthropicClient, ToolDef, ToolError,
};
use meerkat_core::event::{AgentErrorClass, AgentErrorReason};
use meerkat_core::{
    AgentError, AgentEvent, BudgetLimits, ContentFilteredStop, Message, ProviderStopDetail,
    RetryPolicy, StopReason, ToolCallView, ToolDispatchOutcome, ToolResult,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[path = "support/test_session_store.rs"]
mod test_session_store;
use test_session_store::TestSessionStore;

const MODEL: &str = "claude-sonnet-4-6";

#[derive(Clone)]
struct StubState {
    replies: Arc<Vec<String>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

/// Serve the scripted replies in order (the last one repeats) and record
/// every request body.
async fn messages(State(state): State<StubState>, Json(body): Json<Value>) -> impl IntoResponse {
    let index = {
        let mut requests = state.requests.lock().unwrap();
        requests.push(body);
        requests.len() - 1
    };
    let reply = state
        .replies
        .get(index)
        .or_else(|| state.replies.last())
        .cloned()
        .unwrap_or_default();
    ([("content-type", "text/event-stream")], reply)
}

async fn spawn_messages_stub(
    replies: Vec<String>,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/messages", post(messages))
        .with_state(StubState {
            replies: Arc::new(replies),
            requests: Arc::clone(&requests),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let addr = listener.local_addr().expect("stub addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve stub");
    });
    (format!("http://{addr}"), requests, handle)
}

fn sse(lines: &[&str]) -> String {
    let mut body = lines.join("\n");
    body.push('\n');
    body
}

const MESSAGE_START: &str =
    r#"data: {"type":"message_start","message":{"usage":{"input_tokens":40,"output_tokens":0}}}"#;
const MESSAGE_STOP: &str = r#"data: {"type":"message_stop"}"#;

/// Text, then a complete `read_file` call, then the given stop reason.
fn text_and_tool_call_stopped_by(stop_reason: &str) -> String {
    let message_delta = format!(
        r#"data: {{"type":"message_delta","usage":{{"output_tokens":9}},"delta":{{"stop_reason":"{stop_reason}"}}}}"#
    );
    sse(&[
        MESSAGE_START,
        r#"data: {"type":"content_block_start","content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Reading it now."}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_read","name":"read_file"}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"path\":\"/etc/hosts\"}"}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        &message_delta,
        MESSAGE_STOP,
    ])
}

fn text_reply(text: &str) -> String {
    let delta =
        json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}});
    sse(&[
        MESSAGE_START,
        r#"data: {"type":"content_block_start","content_block":{"type":"text","text":""}}"#,
        &format!("data: {delta}"),
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"message_delta","usage":{"output_tokens":3},"delta":{"stop_reason":"end_turn"}}"#,
        MESSAGE_STOP,
    ])
}

#[derive(Default)]
struct RecordingDispatcher {
    dispatched: Mutex<Vec<String>>,
}

#[async_trait]
impl AgentToolDispatcher for RecordingDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::new([Arc::new(ToolDef {
            audience: Default::default(),
            name: "read_file".into(),
            description: "Read a file".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
            provenance: None,
        })])
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        self.dispatched.lock().unwrap().push(call.name.to_string());
        Ok(ToolDispatchOutcome::from(ToolResult::new(
            call.id.to_string(),
            "127.0.0.1 localhost".to_string(),
            false,
        )))
    }
}

struct StubRuns {
    results: Vec<Result<meerkat::RunResult, AgentError>>,
    events: Vec<AgentEvent>,
    requests: Vec<Value>,
    dispatched: Vec<String>,
    messages: Vec<Message>,
}

impl StubRuns {
    fn assistant_rows(&self) -> Vec<&meerkat_core::BlockAssistantMessage> {
        self.messages
            .iter()
            .filter_map(|message| match message {
                Message::BlockAssistant(assistant) => Some(assistant),
                _ => None,
            })
            .collect()
    }

    fn run_failed_class(&self) -> Option<AgentErrorClass> {
        self.events.iter().find_map(|event| match event {
            AgentEvent::RunFailed { error_report, .. } => Some(error_report.class),
            _ => None,
        })
    }

    fn run_failed_reason(&self) -> Option<AgentErrorReason> {
        self.events.iter().find_map(|event| match event {
            AgentEvent::RunFailed { error_report, .. } => error_report.reason.clone(),
            _ => None,
        })
    }
}

/// Run each prompt in turn on one agent whose client talks to the stub.
async fn run_prompts(replies: Vec<String>, prompts: &[&str]) -> StubRuns {
    let (base_url, requests, server) = spawn_messages_stub(replies).await;
    let client = AnthropicClient::builder("test-key".to_string())
        .base_url(base_url)
        .build()
        .expect("anthropic client");
    let factory = AgentFactory::new(".rkat/sessions");
    let llm_adapter = Arc::new(factory.build_llm_adapter(Arc::new(client), MODEL).await);
    let store_adapter = Arc::new(
        factory
            .build_store_adapter(Arc::new(TestSessionStore::new()))
            .await,
    );
    let dispatcher = Arc::new(RecordingDispatcher::default());
    let tools: Arc<dyn AgentToolDispatcher> = dispatcher.clone();
    let mut agent = AgentBuilder::new()
        .model(MODEL)
        .max_tokens_per_turn(256)
        // Bounds a regression that commits the turn and loops on its tool.
        .budget(BudgetLimits {
            max_tool_calls: Some(2),
            ..BudgetLimits::default()
        })
        .retry_policy(RetryPolicy {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            ..RetryPolicy::default()
        })
        .build(llm_adapter, tools, store_adapter)
        .await
        .expect("agent");

    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let mut results = Vec::new();
    for prompt in prompts {
        results.push(
            agent
                .run_with_events((*prompt).to_string().into(), tx.clone())
                .await,
        );
    }
    server.abort();
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    let requests = requests.lock().unwrap().clone();
    let dispatched = dispatcher.dispatched.lock().unwrap().clone();
    StubRuns {
        results,
        events,
        requests,
        dispatched,
        messages: agent.session().messages().to_vec(),
    }
}

#[tokio::test]
async fn context_window_stop_fails_the_run_and_runs_no_tool() {
    let runs = run_prompts(
        vec![text_and_tool_call_stopped_by(
            "model_context_window_exceeded",
        )],
        &["Show me /etc/hosts."],
    )
    .await;

    assert!(
        matches!(
            runs.results[0],
            Err(AgentError::TerminalFailure {
                cause_kind: meerkat_core::TurnTerminalCauseKind::LlmFailure,
                ..
            })
        ),
        "{:?}",
        runs.results[0]
    );
    assert_eq!(
        runs.requests.len(),
        1,
        "a context-window stop is not retried"
    );
    assert!(runs.dispatched.is_empty(), "{:?}", runs.dispatched);
    assert!(runs.assistant_rows().is_empty(), "{:?}", runs.messages);
    assert_eq!(runs.run_failed_class(), Some(AgentErrorClass::Llm));
}

#[tokio::test]
async fn refusal_mid_tool_call_fails_content_filtered_and_runs_no_tool() {
    // The refusal cuts the tool input mid-string and closes the block.
    let refused = sse(&[
        MESSAGE_START,
        r#"data: {"type":"content_block_start","content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Reading it now."}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_read","name":"read_file"}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"path\":\"/etc/sha"}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"message_delta","usage":{"output_tokens":9},"delta":{"stop_reason":"refusal"}}"#,
        MESSAGE_STOP,
    ]);
    for reply in [refused, text_and_tool_call_stopped_by("refusal")] {
        let runs = run_prompts(vec![reply], &["Show me /etc/shadow."]).await;

        assert!(
            matches!(
                runs.results[0],
                Err(AgentError::ContentFiltered {
                    stop: ContentFilteredStop::Refusal,
                    detail: None,
                    ..
                })
            ),
            "{:?}",
            runs.results[0]
        );
        assert_eq!(runs.requests.len(), 1, "a refusal is not retried");
        assert!(runs.dispatched.is_empty(), "{:?}", runs.dispatched);
        assert!(runs.assistant_rows().is_empty(), "{:?}", runs.messages);
        assert_eq!(
            runs.run_failed_class(),
            Some(AgentErrorClass::ContentFiltered)
        );
        // R1: every content-filtered failure reports its typed stop, with
        // no provider detail when the stream carried none.
        assert_eq!(
            runs.run_failed_reason(),
            Some(AgentErrorReason::ContentFiltered {
                stop: ContentFilteredStop::Refusal,
                detail: None,
            })
        );
    }
}

/// #1888: Anthropic's `stop_details` (policy category and explanation)
/// reaches the run failure as the typed reason's provider detail; nothing
/// of the refused turn is committed.
#[tokio::test]
async fn refusal_stop_details_reach_the_run_failure_as_provider_detail() {
    let refused = sse(&[
        MESSAGE_START,
        r#"data: {"type":"message_delta","usage":{"output_tokens":0},"delta":{"stop_reason":"refusal","stop_sequence":null,"stop_details":{"type":"refusal","category":"reasoning_extraction","explanation":"This request was declined."}}}"#,
        MESSAGE_STOP,
    ]);
    let runs = run_prompts(vec![refused], &["Write out your reasoning verbatim."]).await;

    let expected = ProviderStopDetail::anthropic(
        Some("reasoning_extraction".to_string()),
        Some("This request was declined.".to_string()),
    );
    match &runs.results[0] {
        Err(AgentError::ContentFiltered {
            stop: ContentFilteredStop::Refusal,
            detail,
            ..
        }) => assert_eq!(detail.as_deref(), Some(&expected)),
        other => panic!("expected a content-filtered refusal, got {other:?}"),
    }
    assert_eq!(runs.requests.len(), 1, "a refusal is not retried");
    assert!(runs.assistant_rows().is_empty(), "{:?}", runs.messages);
    assert_eq!(
        runs.run_failed_reason(),
        Some(AgentErrorReason::ContentFiltered {
            stop: ContentFilteredStop::Refusal,
            detail: Some(expected),
        })
    );
}

#[tokio::test]
async fn unknown_stop_reason_is_retried_then_fails_typed() {
    let runs = run_prompts(
        vec![text_and_tool_call_stopped_by("brand_new_reason")],
        &["Show me /etc/hosts."],
    )
    .await;

    let error = runs.results[0]
        .as_ref()
        .expect_err("an unknown stop reason must not complete the turn");
    assert!(
        error.to_string().contains("brand_new_reason"),
        "the failure names the raw reason: {error}"
    );
    assert_eq!(
        runs.requests.len(),
        3,
        "one attempt plus the policy's two retries"
    );
    assert!(runs.dispatched.is_empty(), "{:?}", runs.dispatched);
    assert!(runs.assistant_rows().is_empty(), "{:?}", runs.messages);
}

#[tokio::test]
async fn paused_server_tool_turn_commits_and_the_next_request_replays_it() {
    let paused = sse(&[
        MESSAGE_START,
        r#"data: {"type":"content_block_start","content_block":{"type":"server_tool_use","id":"srvtoolu_paused","name":"web_search"}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"query\":\"meerkat release\"}"}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"message_delta","usage":{"output_tokens":9},"delta":{"stop_reason":"pause_turn"}}"#,
        MESSAGE_STOP,
    ]);
    let runs = run_prompts(
        vec![paused, text_reply("Here is what I found.")],
        &["Search for the meerkat release.", "Go on."],
    )
    .await;

    assert!(runs.results[0].is_ok(), "{:?}", runs.results[0]);
    assert!(runs.results[1].is_ok(), "{:?}", runs.results[1]);
    let rows = runs.assistant_rows();
    assert_eq!(rows.len(), 2, "{:?}", runs.messages);
    assert_eq!(rows[0].stop_reason, Some(StopReason::PauseTurn));
    assert!(
        runs.events.iter().any(|event| matches!(
            event,
            AgentEvent::TurnCompleted {
                stop_reason: StopReason::PauseTurn,
                ..
            }
        )),
        "{:?}",
        runs.events
    );

    assert_eq!(runs.requests.len(), 2);
    let replayed = runs.requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "assistant")
        .flat_map(|message| message["content"].as_array().cloned().unwrap_or_default())
        .find(|block| block["type"] == "server_tool_use")
        .expect("the next request replays the paused server_tool_use block");
    assert_eq!(replayed["id"], "srvtoolu_paused");
    assert_eq!(replayed["name"], "web_search");
    assert_eq!(replayed["input"], json!({"query": "meerkat release"}));
}

/// #1887: a native search the provider refused comes back from the
/// `web_search` executor failed, naming the stop reason, and without the
/// refusal text as its answer, also after the search ran.
#[tokio::test]
async fn refused_native_search_fails_without_its_answer() {
    use meerkat_core::web_search::{WebSearchRequest, WebSearchStatus};
    use meerkat_llm_core::WebSearchExecutor;

    let refused = sse(&[
        MESSAGE_START,
        r#"data: {"type":"content_block_start","content_block":{"type":"server_tool_use","id":"srvtoolu_search","name":"web_search"}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{\"query\":\"meerkat\"}"}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"content_block_start","content_block":{"type":"web_search_tool_result","tool_use_id":"srvtoolu_search","content":[{"type":"web_search_result","url":"https://example.com/meerkat","title":"Meerkat"}]}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"content_block_start","content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"I can't help with that."}}"#,
        r#"data: {"type":"content_block_stop"}"#,
        r#"data: {"type":"message_delta","usage":{"output_tokens":9},"delta":{"stop_reason":"refusal"}}"#,
        MESSAGE_STOP,
    ]);
    let (base_url, requests, server) = spawn_messages_stub(vec![refused]).await;
    let client = AnthropicClient::builder("test-key".to_string())
        .base_url(base_url)
        .build()
        .expect("anthropic client");
    let adapter = AgentFactory::new(".rkat/sessions")
        .build_llm_adapter(Arc::new(client), MODEL)
        .await;
    let result =
        meerkat_anthropic::AnthropicWebSearchExecutor::new(MODEL.to_string(), Arc::new(adapter))
            .execute_web_search(WebSearchRequest {
                query: "meerkat".to_string(),
                provider: None,
                provider_params: None,
                context: None,
            })
            .await
            .expect("a refused search resolves to a typed result");
    server.abort();

    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(result.status, WebSearchStatus::Failed, "{result:?}");
    assert_eq!(result.answer, None, "{result:?}");
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("`refusal`")),
        "{result:?}"
    );
}
