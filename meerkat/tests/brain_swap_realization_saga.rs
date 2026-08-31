//! Runtime-backed realization saga for a committed model-routing handoff.
//!
//! These drive the real `MeerkatMachine` command path — the same one the
//! pre-dequeue hook calls — against a scripted reconfigure host and a scripted
//! `RuntimeStore`. They exist because the interesting failures live in the
//! ORDER of the saga, not in any single step:
//!
//! * a request whose originating run never committed must not move routing;
//! * the durable `Realized` record must land BEFORE the generated terminal, so
//!   a crash between them converges instead of claiming a change nothing
//!   recorded;
//! * a retry must converge rather than rotate identity twice;
//! * an unresolvable target must hold with no identity rebinding at all.
//!
//! What these do NOT cover, stated plainly: they assert identity REBINDING via
//! `apply_live_session_llm_identity`, not an observed provider call. Proving
//! "the next provider call used B" requires driving a full session service with
//! a scripted client through the runtime loop, which is a heavier harness than
//! this file builds.

#![cfg(all(feature = "session-store", not(target_arch = "wasm32")))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use meerkat_core::image_generation::{
    SwitchTurnDuration, SwitchTurnIntent, SwitchTurnOrigin, SwitchTurnReasonTextDisposition,
    SwitchTurnRequestId,
};
use meerkat_core::lifecycle::RunId;
use meerkat_core::lifecycle::run_primitive::ModelId;
use meerkat_core::session::model_routing_control::{
    ModelRoutingControlAppendOutcome, SessionModelRoutingControlHistory,
    SessionModelRoutingControlRecord,
};
use meerkat_core::types::SessionId;
use meerkat_core::{Provider, SessionLlmIdentity, SessionToolVisibilityState};
use meerkat_runtime::store::{InMemoryRuntimeStore, RuntimeStore};
use meerkat_runtime::{
    HydratedSessionLlmState, LogicalRuntimeId, MeerkatMachine, ModelRoutingHandoffHoldReason,
    ModelRoutingHandoffRealization, ResolvedSessionLlmReconfigure, RuntimeDriverError,
    SessionLlmCapabilitySurface, SessionLlmCapabilitySurfaceStatus, SessionLlmReconfigureHost,
    SessionLlmReconfigureRequest, SessionServiceRuntimeExt,
};

const MODEL_A: &str = "model-a";
const MODEL_B: &str = "model-b";

// ---------------------------------------------------------------------------
// Boundary-receipt seeding
// ---------------------------------------------------------------------------

/// Durably record a committed boundary receipt for `run` on the real store.
///
/// Deliberately not a store test double. The whole question these tests ask is
/// "does a REAL committed receipt authorize the handoff", so the receipt is
/// written through the store's ordinary atomic-apply path and read back through
/// the ordinary receipt read. A scripted store would have proved only that the
/// script was consulted.
async fn seed_committed_boundary_receipt(
    store: &InMemoryRuntimeStore,
    session_id: &SessionId,
    run: &RunId,
) {
    let runtime_id = LogicalRuntimeId::for_session(session_id);
    store
        .atomic_apply(
            &runtime_id,
            None,
            meerkat_core::RunBoundaryReceiptDraft {
                run_id: run.clone(),
                boundary: meerkat_core::RunApplyBoundary::RunCheckpoint,
                contributing_input_ids: Vec::new(),
                conversation_digest: None,
                message_count: 2,
            }
            .into_sequenced(1),
            Vec::new(),
            None,
        )
        .await
        .expect("seed a committed boundary receipt");
}

// ---------------------------------------------------------------------------
// Scripted reconfigure host: owns the committed log and records the saga
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostCall {
    Hydrate,
    Resolve,
    ApplyIdentity(String),
    AppendRecord(&'static str),
    Persist,
}

struct ScriptedReconfigureHost {
    calls: Mutex<Vec<HostCall>>,
    history: Mutex<SessionModelRoutingControlHistory>,
    current_identity: Mutex<SessionLlmIdentity>,
    resolve_fails: Mutex<Option<String>>,
    persist_fails: Mutex<Option<String>>,
}

fn identity(model: &str) -> SessionLlmIdentity {
    SessionLlmIdentity {
        model: model.to_string(),
        provider: Provider::OpenAI,
        self_hosted_server_id: None,
        provider_params: None,
        auth_binding: None,
    }
}

fn capability_surface() -> SessionLlmCapabilitySurface {
    SessionLlmCapabilitySurface {
        supports_temperature: false,
        supports_thinking: false,
        supports_reasoning: false,
        inline_video: false,
        vision: false,
        image_input: false,
        image_tool_results: false,
        supports_web_search: false,
        supports_mid_conversation_system_messages: false,
        image_generation: false,
        realtime: false,
        call_timeout_secs: None,
    }
}

impl ScriptedReconfigureHost {
    fn new(history: SessionModelRoutingControlHistory) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            history: Mutex::new(history),
            current_identity: Mutex::new(identity(MODEL_A)),
            resolve_fails: Mutex::new(None),
            persist_fails: Mutex::new(None),
        }
    }

    fn record(&self, call: HostCall) {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call);
    }

    fn calls(&self) -> Vec<HostCall> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn identity_applications(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                HostCall::ApplyIdentity(model) => Some(model),
                _ => None,
            })
            .collect()
    }

    fn history(&self) -> SessionModelRoutingControlHistory {
        self.history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn fail_resolve(&self, reason: &str) {
        *self
            .resolve_fails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reason.to_string());
    }

    fn fail_persist(&self, reason: &str) {
        *self
            .persist_fails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(reason.to_string());
    }
}

#[async_trait]
impl SessionLlmReconfigureHost for ScriptedReconfigureHost {
    async fn hydrate_session_llm_state(
        &self,
        _session_id: &SessionId,
    ) -> Result<HydratedSessionLlmState, RuntimeDriverError> {
        self.record(HostCall::Hydrate);
        Ok(HydratedSessionLlmState {
            current_identity: self
                .current_identity
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
            current_visibility_state: SessionToolVisibilityState {
                // The generated hydrate guard checks that the base filter is
                // the one this surface implies; a bare default is rejected.
                capability_base_filter: meerkat_core::capability_base_filter_for_image_tool_results(
                    capability_surface().image_tool_results,
                ),
                ..Default::default()
            },
            current_capability_surface: Some(capability_surface()),
            capability_surface_status: SessionLlmCapabilitySurfaceStatus::Resolved,
            base_tool_names: std::collections::BTreeSet::new(),
        })
    }

    async fn resolve_target_session_llm_identity(
        &self,
        request: &SessionLlmReconfigureRequest,
        _current_identity: &SessionLlmIdentity,
    ) -> Result<ResolvedSessionLlmReconfigure, RuntimeDriverError> {
        self.record(HostCall::Resolve);
        if let Some(reason) = self
            .resolve_fails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            return Err(RuntimeDriverError::Internal(reason));
        }
        let model = request.model.clone().unwrap_or_else(|| MODEL_B.to_string());
        Ok(ResolvedSessionLlmReconfigure {
            target_identity: identity(&model),
            target_capability_surface: capability_surface(),
        })
    }

    async fn apply_live_session_llm_identity(
        &self,
        _session_id: &SessionId,
        identity: &SessionLlmIdentity,
        _capability_surface: Option<&SessionLlmCapabilitySurface>,
    ) -> Result<(), RuntimeDriverError> {
        self.record(HostCall::ApplyIdentity(identity.model.clone()));
        *self
            .current_identity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = identity.clone();
        Ok(())
    }

    async fn apply_live_session_tool_visibility_state(
        &self,
        _session_id: &SessionId,
        _visibility_state: Option<SessionToolVisibilityState>,
    ) -> Result<(), RuntimeDriverError> {
        Ok(())
    }

    async fn persist_live_session(
        &self,
        _session_id: &SessionId,
    ) -> Result<(), RuntimeDriverError> {
        self.record(HostCall::Persist);
        if let Some(reason) = self
            .persist_fails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            return Err(RuntimeDriverError::Internal(reason));
        }
        Ok(())
    }

    async fn discard_live_session(
        &self,
        _session_id: &SessionId,
    ) -> Result<(), RuntimeDriverError> {
        Ok(())
    }

    async fn load_live_session_model_routing_control_history(
        &self,
        _session_id: &SessionId,
    ) -> Result<SessionModelRoutingControlHistory, RuntimeDriverError> {
        Ok(self.history())
    }

    async fn append_live_session_model_routing_control_record(
        &self,
        _session_id: &SessionId,
        record: SessionModelRoutingControlRecord,
    ) -> Result<ModelRoutingControlAppendOutcome, RuntimeDriverError> {
        let label = match &record {
            SessionModelRoutingControlRecord::ModelRoutingIntentRequested { .. } => "requested",
            SessionModelRoutingControlRecord::ModelRoutingIntentRealized { .. } => "realized",
            SessionModelRoutingControlRecord::ModelRoutingIntentDenied { .. } => "denied",
            _ => "unknown",
        };
        self.record(HostCall::AppendRecord(label));
        let mut history = self
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        history
            .append(record)
            .map_err(|error| RuntimeDriverError::Internal(error.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn request_id(byte: u8) -> SwitchTurnRequestId {
    SwitchTurnRequestId::new(uuid::Uuid::from_bytes([byte; 16]))
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

fn committed_request_history(
    request: SwitchTurnRequestId,
    run: &RunId,
    target: &str,
) -> SessionModelRoutingControlHistory {
    let mut history = SessionModelRoutingControlHistory::new();
    history
        .append(
            SessionModelRoutingControlRecord::request(
                request,
                run.clone(),
                until_changed_model_intent(target),
            )
            .expect("representable durable request"),
        )
        .expect("append");
    history
}

struct Fixture {
    adapter: Arc<MeerkatMachine>,
    host: Arc<ScriptedReconfigureHost>,
    session_id: SessionId,
}

async fn fixture_with(
    history: SessionModelRoutingControlHistory,
    committed_run: Option<RunId>,
) -> Fixture {
    let inner = Arc::new(InMemoryRuntimeStore::new());
    let session_id = SessionId::new();
    if let Some(run) = committed_run.as_ref() {
        seed_committed_boundary_receipt(inner.as_ref(), &session_id, run).await;
    }
    let store: Arc<dyn RuntimeStore> = inner;
    let adapter = Arc::new(MeerkatMachine::persistent_without_blobs(store));
    let host = Arc::new(ScriptedReconfigureHost::new(history));
    adapter.set_session_llm_reconfigure_host(host.clone());
    adapter
        .register_session(session_id.clone())
        .await
        .expect("register session");
    // The until-changed switch chain requires a known routing baseline; this is
    // the same call the ordinary build path makes.
    adapter
        .configure_model_routing_baseline(&session_id, ModelId::new(MODEL_A), false)
        .await
        .expect("baseline");
    Fixture {
        adapter,
        host,
        session_id,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The whole saga, from a freshly registered machine that has never seen the
/// request — which is exactly the cold-restart shape: the committed log is the
/// only carrier, and the machine learns the request by importing it.
#[tokio::test]
async fn a_committed_request_with_a_committed_origin_boundary_realizes_and_records() {
    let run = RunId::new();
    let fixture = fixture_with(
        committed_request_history(request_id(1), &run, MODEL_B),
        Some(run.clone()),
    )
    .await;

    let pending = fixture
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
        .await
        .expect("read committed log");
    assert_eq!(pending.len(), 1, "the committed log owes one decision");
    assert_eq!(pending[0].originating_run_id, run);
    assert_eq!(
        pending[0].intent.target_model,
        ModelId::new(MODEL_B),
        "the candidate must carry the exact committed target"
    );

    let realization = fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            pending[0].clone(),
        )
        .await
        .expect("realization succeeds");
    match realization {
        ModelRoutingHandoffRealization::Realized { applied_identity } => {
            assert_eq!(applied_identity.model, MODEL_B);
        }
        other => panic!("expected Realized, got {other:?}"),
    }

    assert_eq!(
        fixture.host.identity_applications(),
        vec![MODEL_B.to_string()],
        "the session identity must be rebound to B exactly once"
    );

    let calls = fixture.host.calls();
    let append_index = calls
        .iter()
        .position(|call| matches!(call, HostCall::AppendRecord("realized")))
        .expect("the Realized record must be appended");
    let persist_index = calls
        .iter()
        .rposition(|call| matches!(call, HostCall::Persist))
        .expect("the Realized record must be persisted");
    assert!(
        append_index < persist_index,
        "the durable Realized record must be persisted after it is appended: {calls:?}"
    );

    let history = fixture.host.history();
    assert_eq!(history.len(), 2, "Requested then Realized");
    assert!(
        history.awaiting_decision().next().is_none(),
        "the committed log must owe nothing after realization"
    );
    assert!(
        fixture
            .adapter
            .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
            .await
            .expect("reread")
            .is_empty(),
        "the next pre-dequeue pass must find nothing pending"
    );
}

/// The gate that makes a handoff actionable. A request whose originating run
/// left no committed boundary receipt must hold, change no identity, and write
/// nothing durable.
#[tokio::test]
async fn an_uncommitted_origin_boundary_holds_and_rebinds_nothing() {
    let run = RunId::new();
    let fixture = fixture_with(
        committed_request_history(request_id(2), &run, MODEL_B),
        None,
    )
    .await;
    let pending = fixture
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
        .await
        .expect("read committed log");

    let realization = fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            pending[0].clone(),
        )
        .await
        .expect("hold is a typed outcome, not an error");
    match realization {
        ModelRoutingHandoffRealization::Held {
            reason: ModelRoutingHandoffHoldReason::OriginatingBoundaryUncommitted { run_id },
        } => assert_eq!(run_id, run),
        other => panic!("expected a boundary hold, got {other:?}"),
    }
    assert!(
        fixture.host.identity_applications().is_empty(),
        "a held handoff must not rebind the session identity"
    );
    assert_eq!(
        fixture.host.history().len(),
        1,
        "a held handoff must leave the committed log unchanged"
    );
}

/// A receipt for a DIFFERENT run does not authorize this request.
#[tokio::test]
async fn a_receipt_for_another_run_does_not_authorize() {
    let run = RunId::new();
    let unrelated = RunId::new();
    let fixture = fixture_with(
        committed_request_history(request_id(3), &run, MODEL_B),
        Some(unrelated),
    )
    .await;
    let pending = fixture
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
        .await
        .expect("read committed log");
    let realization = fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            pending[0].clone(),
        )
        .await
        .expect("hold is typed");
    assert!(
        matches!(
            realization,
            ModelRoutingHandoffRealization::Held {
                reason: ModelRoutingHandoffHoldReason::OriginatingBoundaryUncommitted { .. }
            }
        ),
        "a foreign receipt must not satisfy this request's boundary proof"
    );
    assert!(fixture.host.identity_applications().is_empty());
}

/// An unresolvable target — the "B unavailable" case — holds typed and rebinds
/// nothing, so the pending input is served by nothing at all rather than by A
/// under a false success.
#[tokio::test]
async fn an_unresolvable_target_holds_typed_with_zero_identity_rebinding() {
    let run = RunId::new();
    let fixture = fixture_with(
        committed_request_history(request_id(4), &run, MODEL_B),
        Some(run),
    )
    .await;
    fixture.host.fail_resolve("model-b is unavailable");
    let pending = fixture
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
        .await
        .expect("read committed log");

    let realization = fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            pending[0].clone(),
        )
        .await
        .expect("hold is typed");
    match realization {
        ModelRoutingHandoffRealization::Held {
            reason: ModelRoutingHandoffHoldReason::TargetUnresolvable { target_model, .. },
        } => assert_eq!(target_model, ModelId::new(MODEL_B)),
        other => panic!("expected a target-unresolvable hold, got {other:?}"),
    }
    assert!(
        fixture.host.identity_applications().is_empty(),
        "an unresolvable target must never rebind identity"
    );
    assert_eq!(
        fixture.host.history().len(),
        1,
        "an unresolvable target must record no terminal"
    );
    assert!(
        !fixture
            .adapter
            .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
            .await
            .expect("reread")
            .is_empty(),
        "the request must remain pending so a later pass can retry it"
    );
}

/// Replaying the exact realization converges instead of rotating a second
/// time. This is the crash-after-realized recovery shape.
#[tokio::test]
async fn an_exact_replay_after_realization_converges_already_exact() {
    let run = RunId::new();
    let fixture = fixture_with(
        committed_request_history(request_id(5), &run, MODEL_B),
        Some(run),
    )
    .await;
    let pending = fixture
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
        .await
        .expect("read committed log");
    let handoff = pending[0].clone();

    fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            handoff.clone(),
        )
        .await
        .expect("first realization");

    let replay = fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            handoff,
        )
        .await
        .expect("replay converges");
    assert!(
        matches!(replay, ModelRoutingHandoffRealization::AlreadyExact),
        "an exact replay must converge, got {replay:?}"
    );
    assert_eq!(
        fixture.host.identity_applications(),
        vec![MODEL_B.to_string()],
        "convergence must not perform a second rotation"
    );
    assert_eq!(
        fixture.host.history().len(),
        2,
        "convergence must not append a second terminal record"
    );
}

/// The ordering proof. If persisting the durable `Realized` record fails, the
/// generated terminal must NOT be marked — otherwise the machine would claim a
/// routing change with nothing on disk to support it, and the next pass would
/// see a settled request that never settled.
#[tokio::test]
async fn a_persistence_failure_leaves_the_generated_terminal_unmarked() {
    let run = RunId::new();
    let fixture = fixture_with(
        committed_request_history(request_id(6), &run, MODEL_B),
        Some(run),
    )
    .await;
    fixture.host.fail_persist("disk full");
    let pending = fixture
        .adapter
        .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
        .await
        .expect("read committed log");
    let handoff = pending[0].clone();

    fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            handoff.clone(),
        )
        .await
        .expect_err("a persistence failure must surface");

    // Recovering: persistence works again, and the retry must still be able to
    // finish the request rather than finding it wedged.
    *fixture
        .host
        .persist_fails
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let retry = fixture
        .adapter
        .realize_committed_model_routing_handoff_under_turn_finalization_boundary(
            &fixture.session_id,
            handoff,
        )
        .await
        .expect("retry after a transient persistence failure");
    assert!(
        matches!(retry, ModelRoutingHandoffRealization::Realized { .. }),
        "the retry must complete the request, got {retry:?}"
    );
}

/// A log whose request already carries a terminal record owes nothing, so a
/// restart after the durable `Realized` landed finds no pending work at all.
#[tokio::test]
async fn a_settled_log_owes_no_decision_after_restart() {
    let run = RunId::new();
    let request = request_id(7);
    let mut history = committed_request_history(request, &run, MODEL_B);
    history
        .append(
            SessionModelRoutingControlRecord::ModelRoutingIntentRealized {
                request_id: request,
                originating_run_id: run.clone(),
                intent: until_changed_model_intent(MODEL_B),
                applied_identity: Box::new(identity(MODEL_B)),
            },
        )
        .expect("append realized");
    let fixture = fixture_with(history, Some(run)).await;

    assert!(
        fixture
            .adapter
            .committed_model_routing_handoffs_awaiting_decision(&fixture.session_id)
            .await
            .expect("read committed log")
            .is_empty(),
        "a settled request must not be replayed after restart"
    );
    assert!(
        fixture.host.identity_applications().is_empty(),
        "a settled log must trigger no rebinding"
    );
}
