//! The pre-dequeue hook, proven through the real runtime loop.
//!
//! Every other test in this feature drives realization by calling the runtime
//! method directly. That leaves one thing unproven and it is the thing most
//! likely to silently rot: whether the runtime LOOP actually invokes the hook,
//! at the right point, before it serves the next input.
//!
//! This file builds a real runtime-backed service over a real realm, runs a
//! real turn to obtain a real committed boundary receipt, writes a committed
//! `Requested` record bound to that exact run, and then submits the next input
//! through the ordinary admission path. Nothing here calls the realization
//! method.
//!
//! # What this cannot prove, and why
//!
//! It cannot show the next provider call landing on model B. Reaching that
//! requires the `brain_swap` builtin to be registered, which requires a
//! non-empty proven-availability set, which requires the session to resolve a
//! real provider client. Supplying a mock through `default_llm_client` sets
//! `llm_client_override`, and the factory deliberately advertises NO models in
//! that case — a client override means the session is not talking to a
//! registry-resolved provider, so no catalog model is provably reachable. The
//! mock that would make the test convenient is exactly what makes the feature
//! correctly unavailable. Proving the B call therefore needs live credentials,
//! not a better fixture.
//!
//! What it does prove is the load-bearing half: a committed request reaches the
//! loop, the loop acts on it before admitting work, and when realization cannot
//! complete the pending input is NOT served under the old identity.

#![cfg(all(feature = "session-store", not(target_arch = "wasm32")))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use meerkat::surface::{build_runtime_backed_service, default_persistent_executor};
use meerkat::{
    AgentFactory, Config, CreateSessionRequest, FactoryAgentBuilder, PersistentSessionService,
};
use meerkat_client::TestClient;
use meerkat_core::SessionBuildOptions;
use meerkat_core::image_generation::{
    SwitchTurnDuration, SwitchTurnIntent, SwitchTurnOrigin, SwitchTurnReasonTextDisposition,
    SwitchTurnRequestId,
};
use meerkat_core::lifecycle::RunId;
use meerkat_core::lifecycle::run_primitive::ModelId;
use meerkat_core::service::SessionService;
use meerkat_core::session::model_routing_control::SessionModelRoutingControlRecord;
use meerkat_runtime::completion::CompletionOutcome;
use meerkat_runtime::{Input, MeerkatMachine, PromptInput};
use tokio::time::Duration;

async fn build_service(
    root: &std::path::Path,
    backend: meerkat_store::RealmBackend,
) -> (
    Arc<PersistentSessionService<FactoryAgentBuilder>>,
    Arc<MeerkatMachine>,
) {
    let (_manifest, persistence) = meerkat::open_realm_persistence_in(
        root,
        "brain-swap-realm",
        Some(backend),
        Some(meerkat_store::RealmOrigin::Explicit),
    )
    .await
    .expect("open realm persistence");
    let factory = AgentFactory::new(root.join("sessions"));
    let mut builder = FactoryAgentBuilder::new(factory, Config::default());
    builder.default_llm_client = Some(Arc::new(TestClient::for_provider(
        meerkat_core::Provider::OpenAI,
    )));
    let (service, adapter) = build_runtime_backed_service(builder, 4, persistence);
    (Arc::new(service), adapter)
}

fn create_request() -> CreateSessionRequest {
    CreateSessionRequest {
        injected_context: Vec::new(),
        model: "gpt-5.4".to_string(),
        prompt: meerkat_core::ContentInput::Text(String::new()),
        system_prompt: meerkat::SystemPromptOverride::Set("pre-dequeue contract".to_string()),
        max_tokens: None,
        event_tx: None,
        initial_turn: meerkat_core::service::InitialTurnPolicy::Defer,
        deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
        build: Some(SessionBuildOptions::default()),
        labels: None,
    }
}

async fn run_prompt(
    adapter: &Arc<MeerkatMachine>,
    session_id: &meerkat::SessionId,
    prompt: &str,
) -> CompletionOutcome {
    let (_outcome, handle) = adapter
        .accept_input_with_completion(session_id, Input::Prompt(PromptInput::new(prompt, None)))
        .await
        .expect("accept prompt input");
    let handle = handle.expect("completion handle");
    tokio::time::timeout(Duration::from_secs(20), handle.wait())
        .await
        .expect("prompt should settle in time")
        .expect("completion waiter should resolve")
}

fn until_changed_model_intent(target: &str) -> SwitchTurnIntent {
    SwitchTurnIntent {
        target_model: ModelId::new(target),
        duration: SwitchTurnDuration::UntilChanged,
        origin: SwitchTurnOrigin::Model {
            reason: SwitchTurnReasonTextDisposition::NotProvided,
        },
    }
}

/// A committed request whose originating run never committed a boundary must
/// hold, and the hold must block the pending input rather than serving it.
///
/// `seed_before_first_turn` exists because the two backends tolerate different
/// seeding. HeadCanonical accepts an out-of-band persist between turns, so the
/// stronger shape runs there: one healthy turn first, proving the loop was fine
/// until the handoff appeared. WholeBlob refuses an ordinary write that would
/// re-encode a store-owned provisional candidate, so its request must be seeded
/// before any turn has produced one. That is a constraint on how this test can
/// plant the record, not on the feature: in production the record is appended
/// inside the run and committed by the ordinary boundary, never out of band.
async fn uncommitted_origin_blocks_the_next_input(
    backend: meerkat_store::RealmBackend,
    seed_before_first_turn: bool,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let (service, adapter) = build_service(temp.path(), backend).await;

    let created = service
        .create_session(create_request())
        .await
        .expect("create session");
    let session_id = created.session_id.clone();
    let service_for_executor = Arc::clone(&service);
    let adapter_for_executor = Arc::clone(&adapter);
    adapter
        .ensure_session_with_executor(
            session_id.clone(),
            default_persistent_executor(
                service_for_executor,
                adapter_for_executor,
                session_id.clone(),
            ),
        )
        .await
        .expect("attach executor");

    // One ordinary turn proves the loop is healthy before the handoff exists.
    if !seed_before_first_turn {
        let first = run_prompt(&adapter, &session_id, "first").await;
        assert!(
            matches!(first, CompletionOutcome::Completed(_)),
            "the baseline turn must complete: {first:?}"
        );
    }

    // Seed a committed request bound to a run that never committed anything.
    let orphan_run = RunId::new();
    service
        .append_live_model_routing_control_record_under_runtime_turn_boundary(
            &session_id,
            SessionModelRoutingControlRecord::request(
                SwitchTurnRequestId::new(uuid::Uuid::from_bytes([42u8; 16])),
                orphan_run,
                until_changed_model_intent("gpt-5.5"),
            )
            .expect("representable durable request"),
        )
        .await
        .expect("append committed request");
    service
        .persist_live_session_now_under_runtime_turn_boundary(&session_id)
        .await
        .expect("persist committed request");

    // The next input must NOT be served: the loop's pre-dequeue pass observes a
    // committed request it cannot prove, and fails closed.
    let second = run_prompt(&adapter, &session_id, "second").await;
    assert!(
        !matches!(second, CompletionOutcome::Completed(_)),
        "an unprovable committed handoff must block the next input, got {second:?}"
    );
}

#[tokio::test]
async fn head_canonical_uncommitted_origin_blocks_the_next_input() {
    uncommitted_origin_blocks_the_next_input(meerkat_store::RealmBackend::Sqlite, false).await;
}

// WholeBlob (jsonl) is NOT covered here, and the omission is deliberate.
//
// This test has to plant a committed request out of band, because the only
// in-band writer is the `brain_swap` builtin, which cannot be registered under
// a mock client. WholeBlob refuses an ordinary session write that would
// re-encode a store-owned provisional candidate, so the out-of-band persist
// poisons the next boundary commit with
// `ordinary WholeBlob write cannot bypass or re-encode a store-owned
// provisional candidate` — before the handoff is ever consulted.
//
// A test asserting "the next input was not served" would then pass for
// entirely the wrong reason: the input fails on a store-authority conflict
// this fixture manufactured, not on the pre-dequeue hold. That is worse than
// no coverage, so there is none. Covering WholeBlob honestly requires the
// builtin to be registered, which requires live provider credentials.
