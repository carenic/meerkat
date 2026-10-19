//! Gemini stop reasons through a real agent loop.
//!
//! These tests run the real `GeminiClient` against a local
//! `streamGenerateContent` stub that records every request body, and pin
//! what a prompt Gemini blocked before any candidate does to the run: it
//! fails as content filtered after exactly one provider call, never as an
//! incomplete response that is retried, keeps the prompt tokens it was
//! billed, and reports the raw block reason as the typed reason's provider
//! detail (#1888).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use meerkat::{AgentBuilder, AgentFactory, AgentToolDispatcher, GeminiClient, ToolDef, ToolError};
use meerkat_core::event::{AgentErrorClass, AgentErrorReason};
use meerkat_core::{
    AgentError, AgentEvent, ContentFilteredStop, GeminiStopStage, Message, ProviderStopDetail,
    RetryPolicy, ToolCallView, ToolDispatchOutcome, Usage,
};
use serde_json::Value;
use tokio::net::TcpListener;

#[path = "support/test_session_store.rs"]
mod test_session_store;
use test_session_store::TestSessionStore;

const MODEL: &str = "gemini-3.5-flash";

#[derive(Clone)]
struct StubState {
    reply: Arc<String>,
    requests: Arc<Mutex<Vec<Value>>>,
}

/// Serve the scripted reply to every request and record each request body.
async fn stream_generate_content(
    State(state): State<StubState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    state.requests.lock().unwrap().push(body);
    (
        [("content-type", "text/event-stream")],
        state.reply.as_str().to_owned(),
    )
}

async fn spawn_gemini_stub(
    reply: String,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            &format!("/v1beta/models/{MODEL}:streamGenerateContent"),
            post(stream_generate_content),
        )
        .with_state(StubState {
            reply: Arc::new(reply),
            requests: Arc::clone(&requests),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let addr = listener.local_addr().expect("stub addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve stub");
    });
    (format!("http://{addr}"), requests, handle)
}

struct NoTools;

#[async_trait]
impl AgentToolDispatcher for NoTools {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::from([])
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        Err(ToolError::not_found(call.name))
    }
}

struct StubRun {
    result: Result<meerkat::RunResult, AgentError>,
    events: Vec<AgentEvent>,
    requests: usize,
    assistant_rows: usize,
    usage: Usage,
}

impl StubRun {
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

/// Run one prompt on an agent whose client talks to the stub.
async fn run_prompt(reply: String) -> StubRun {
    let (base_url, requests, server) = spawn_gemini_stub(reply).await;
    let client = GeminiClient::new_with_base_url("test-key".to_string(), base_url);
    let factory = AgentFactory::new(".rkat/sessions");
    let llm_adapter = Arc::new(factory.build_llm_adapter(Arc::new(client), MODEL).await);
    let store_adapter = Arc::new(
        factory
            .build_store_adapter(Arc::new(TestSessionStore::new()))
            .await,
    );
    let tools: Arc<dyn AgentToolDispatcher> = Arc::new(NoTools);
    let mut agent = AgentBuilder::new()
        .model(MODEL)
        .max_tokens_per_turn(256)
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
    let result = agent
        .run_with_events("Describe the meerkat.".to_string().into(), tx)
        .await;
    server.abort();
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    let requests = requests.lock().unwrap().len();
    let assistant_rows = agent
        .session()
        .messages()
        .iter()
        .filter(|message| matches!(message, Message::BlockAssistant(_)))
        .count();
    let usage = agent.session().total_usage();
    StubRun {
        result,
        events,
        requests,
        assistant_rows,
        usage,
    }
}

#[tokio::test]
async fn prompt_blocked_before_any_candidate_fails_content_filtered_once() {
    for block_reason in ["SAFETY", "BLOCKLIST", "OTHER"] {
        let reply = format!(
            "data: {{\"promptFeedback\":{{\"blockReason\":\"{block_reason}\"}},\"usageMetadata\":{{\"promptTokenCount\":12,\"totalTokenCount\":12}}}}\n"
        );
        let run = run_prompt(reply).await;

        let expected =
            ProviderStopDetail::gemini(GeminiStopStage::Prompt, block_reason, Vec::new());
        match &run.result {
            Err(AgentError::ContentFiltered {
                stop: ContentFilteredStop::ContentFilter,
                detail,
                ..
            }) => assert_eq!(detail.as_deref(), Some(&expected), "{block_reason}"),
            other => panic!("{block_reason}: expected content filtered, got {other:?}"),
        }
        assert_eq!(
            run.requests, 1,
            "{block_reason}: a blocked prompt is not retried"
        );
        assert_eq!(run.assistant_rows, 0, "{block_reason}");
        assert_eq!(
            run.run_failed_class(),
            Some(AgentErrorClass::ContentFiltered),
            "{block_reason}"
        );
        assert_eq!(
            run.run_failed_reason(),
            Some(AgentErrorReason::ContentFiltered {
                stop: ContentFilteredStop::ContentFilter,
                detail: Some(expected),
            }),
            "{block_reason}: the raw block reason reaches the caller"
        );
        // The blocked prompt was billed, and its tokens stay counted.
        assert_eq!(
            run.usage.input_tokens, 12,
            "{block_reason}: {:?}",
            run.usage
        );
    }
}

#[tokio::test]
async fn prompt_feedback_without_block_reason_completes_the_run() {
    let reply = concat!(
        r#"data: {"promptFeedback":{"safetyRatings":[{"category":"HARM_CATEGORY_HARASSMENT","probability":"NEGLIGIBLE"}]},"candidates":[{"content":{"role":"model","parts":[{"text":"A small mammal."}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":4}}"#,
        "\n"
    )
    .to_string();
    let run = run_prompt(reply).await;

    assert!(run.result.is_ok(), "{:?}", run.result);
    assert_eq!(run.requests, 1);
    assert_eq!(run.assistant_rows, 1);
    assert_eq!(run.run_failed_class(), None);
}
