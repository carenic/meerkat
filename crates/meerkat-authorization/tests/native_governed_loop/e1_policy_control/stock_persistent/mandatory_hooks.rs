//! Configured mandatory model-request hooks for the stock persistent cases.
//! The host configures two `ordinary_only` Guardrails and the stock factory
//! composes them like any other configured entry. A loopback HTTP endpoint is
//! the policy owner's hook runtime: it decides, and keeps what it was actually
//! offered. Nothing here stages, appends or fabricates a protected record.
use super::*;
use meerkat_core::authorization::{
    MandatoryHookCause, MandatoryHookWithheldEffect, MandatoryHookWithholding,
};
use meerkat_core::{
    HookAdapterConfig, HookCapability, HookDecision, HookEntryConfig, HookId, HookInvocation,
    HookModelOperation, HookModelOperationScope, HookPoint, HookReasonCode,
};

pub(super) const PRE_LLM_GATE: &str = "stock-pre-llm-gate";
pub(super) const POST_LLM_GATE: &str = "stock-post-llm-gate";
const NOTICE_CATEGORY: &str = "mandatory_hook_prerequisite_failed";
/// The hook's own free text and payload. Neither the protected record nor the
/// controller feedback may carry them.
const PRIVATE_RATIONALE: &str = "stock-gate-private-rationale";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdicts {
    /// Deny the first invocation each gate is offered, then allow.
    WithholdFirstAtEachGate,
    AllowAll,
}

struct Gate {
    verdicts: Verdicts,
    offered: Mutex<Vec<(Value, bool)>>,
}

pub(super) struct HookEndpoint {
    url: String,
    gate: Arc<Gate>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for HookEndpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn decide(State(gate): State<Arc<Gate>>, Json(invocation): Json<Value>) -> Json<Value> {
    let decision = {
        let mut offered = gate.offered.lock().unwrap();
        let deny = gate.verdicts == Verdicts::WithholdFirstAtEachGate
            && !offered
                .iter()
                .any(|(seen, _)| seen["point"] == invocation["point"]);
        let decision = match (deny, invocation["point"].as_str()) {
            (false, _) => HookDecision::Allow,
            (true, Some("pre_llm_request")) => HookDecision::deny(
                HookId::new(PRE_LLM_GATE),
                HookReasonCode::PolicyViolation,
                PRIVATE_RATIONALE,
                Some(json!({ "private": PRIVATE_RATIONALE })),
            ),
            (true, _) => HookDecision::deny(
                HookId::new(POST_LLM_GATE),
                HookReasonCode::SafetyViolation,
                PRIVATE_RATIONALE,
                Some(json!({ "private": PRIVATE_RATIONALE })),
            ),
        };
        offered.push((invocation, deny));
        decision
    };
    Json(json!({ "decision": decision }))
}

impl HookEndpoint {
    pub(super) async fn start(verdicts: Verdicts) -> Self {
        let gate = Arc::new(Gate {
            verdicts,
            offered: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/gate", post(decide))
            .with_state(gate.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/gate", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, gate, task }
    }

    /// The host's configuration: two foreground mandatory Guardrails that
    /// state they do not govern the controller-feedback request.
    pub(super) fn config(&self) -> Config {
        let mut config = Config::default();
        config.hooks.entries = [
            (PRE_LLM_GATE, HookPoint::PreLlmRequest),
            (POST_LLM_GATE, HookPoint::PostLlmResponse),
        ]
        .into_iter()
        .map(|(id, point)| HookEntryConfig {
            id: HookId::new(id),
            point,
            capability: HookCapability::Guardrail,
            timeout_ms: Some(30_000),
            model_operations: HookModelOperationScope::OrdinaryOnly,
            runtime: HookAdapterConfig::http(self.url.clone(), "POST", HashMap::new()),
            ..Default::default()
        })
        .collect();
        config
    }

    /// Every invocation the configured gates were actually offered, decoded
    /// from the wire, with the verdict returned.
    pub(super) fn offered(&self) -> Vec<(HookInvocation, bool)> {
        self.gate
            .offered
            .lock()
            .unwrap()
            .iter()
            .map(|(invocation, denied)| {
                (
                    serde_json::from_value(invocation.clone()).expect("typed hook invocation"),
                    *denied,
                )
            })
            .collect()
    }
}

/// The gates were offered exactly these ordinary operations of the actual
/// run, in order. An `ordinary_only` gate is never offered controller
/// feedback.
pub(super) fn assert_offered(
    endpoint: &HookEndpoint,
    session_id: &SessionId,
    run_id: &meerkat_core::RunId,
    expected: &[(HookPoint, u64, bool)],
) -> Vec<HookInvocation> {
    let offered = endpoint.offered();
    let actual: Vec<_> = offered
        .iter()
        .map(|(invocation, denied)| {
            assert_eq!(&invocation.session_id, session_id);
            assert_eq!(invocation.run_id.as_ref(), Some(run_id));
            let Some(HookModelOperation::Ordinary { sequence }) = invocation.model_operation()
            else {
                panic!(
                    "an ordinary_only gate was offered {:?}",
                    invocation.model_operation()
                );
            };
            (invocation.point, *sequence, *denied)
        })
        .collect();
    assert_eq!(actual, expected);
    offered
        .into_iter()
        .map(|(invocation, _)| invocation)
        .collect()
}

fn withholding(record: &StoredAuthorizationAuditObservation) -> Option<&MandatoryHookWithholding> {
    match &record.observation.observation {
        AuditObservation::MandatoryHookWithheld { withholding } => Some(withholding),
        _ => None,
    }
}

fn prepared_model(record: &StoredAuthorizationAuditObservation) -> bool {
    matches!(&record.observation.observation,
        AuditObservation::Prepared { target, .. } if matches!(target.as_ref(), AuditTarget::Model(_)))
}

fn candidate_withheld() -> MandatoryHookWithholding {
    MandatoryHookWithholding {
        sequence: 1,
        hook_id: Some(HookId::new(PRE_LLM_GATE)),
        hook_id_omitted: false,
        point: HookPoint::PreLlmRequest,
        cause: MandatoryHookCause::Denied {
            reason_code: HookReasonCode::PolicyViolation,
        },
        effect: MandatoryHookWithheldEffect::RequestNotSent,
    }
}

fn response_withheld() -> MandatoryHookWithholding {
    MandatoryHookWithholding {
        sequence: 3,
        hook_id: Some(HookId::new(POST_LLM_GATE)),
        hook_id_omitted: false,
        point: HookPoint::PostLlmResponse,
        cause: MandatoryHookCause::Denied {
            reason_code: HookReasonCode::SafetyViolation,
        },
        // The withheld response is the plain final text: no local call, no
        // provider-native tool.
        effect: MandatoryHookWithheldEffect::LocalDispatchAndCommitWithheld {
            local_tool_calls: 0,
            text_output_produced: true,
            provider_native_tools: Vec::new(),
            provider_native_tools_total: 0,
            provider_native_tools_omitted: 0,
        },
    }
}

/// While the first allowed ordinary request is held: the gate withheld the
/// first ordinary candidate before any request existed, and the stock owner
/// staged that record in live protected custody before the controller
/// feedback replacement was prepared.
pub(super) fn assert_candidate_withheld_before_feedback(
    endpoint: &HookEndpoint,
    live_audit: &[StoredAuthorizationAuditObservation],
    session_id: &SessionId,
    run_id: &meerkat_core::RunId,
) {
    assert_offered(
        endpoint,
        session_id,
        run_id,
        &[
            (HookPoint::PreLlmRequest, 1, true),
            (HookPoint::PreLlmRequest, 3, false),
        ],
    );
    let staged: Vec<_> = live_audit
        .iter()
        .enumerate()
        .filter_map(|(at, record)| withholding(record).map(|withheld| (at, record, withheld)))
        .collect();
    assert_eq!(staged.len(), 1, "only the candidate is withheld so far");
    let (at, record, withheld) = staged[0];
    assert_eq!(withheld, &candidate_withheld());
    let first_model = live_audit
        .iter()
        .position(prepared_model)
        .expect("the controller-feedback replacement was prepared");
    assert!(at < first_model);
    assert_eq!(
        live_audit
            .iter()
            .filter(|other| other.observation.operation_id == record.observation.operation_id)
            .count(),
        1,
        "the withheld candidate is never prepared, entered or observed"
    );
}

/// The hooked writer turn on an actual stored row: the withheld first
/// ordinary candidate, the controller-feedback replacement that carried the
/// sibling batch, the withheld actual responder and the final feedback.
pub(super) fn assert_withheld_stock_turn_audit(
    audit: &[StoredAuthorizationAuditObservation],
    session_id: &SessionId,
    input_id: &meerkat_core::InputId,
    run_id: &meerkat_core::RunId,
) {
    let withheld: Vec<_> = audit
        .iter()
        .enumerate()
        .filter(|(_, record)| withholding(record).is_some())
        .collect();
    assert_eq!(withheld.len(), 2, "exactly the two actual withholdings");
    for (_, record) in &withheld {
        // The same row-bound contributors, run and execution scope as every
        // other observation of this input.
        assert_stock_record_scope(record, session_id, input_id, run_id);
    }
    let entered: Vec<_> = audit
        .iter()
        .filter(|record| withholding(record).is_none())
        .cloned()
        .collect();
    // Truthful prior effects: the sibling refusal and the entered read keep
    // their exact records, and every entered model request keeps its actual
    // HTTP outcome. Three requests entered: feedback, responder, feedback.
    assert_stock_turn_audit_with_models(
        &entered,
        session_id,
        input_id,
        run_id,
        DENIED_CALL,
        PERMITTED_CALL,
        3,
    );
    let models: Vec<_> = audit
        .iter()
        .enumerate()
        .filter(|(_, record)| prepared_model(record))
        .map(|(at, record)| (at, &record.observation.operation_id))
        .collect();
    assert_eq!(models.len(), 3);
    let tools: Vec<_> = audit
        .iter()
        .enumerate()
        .filter(|(_, record)| match &record.observation.observation {
            AuditObservation::Prepared { target, .. }
            | AuditObservation::Refused { target, .. } => {
                matches!(target.as_ref(), AuditTarget::Tool { .. })
            }
            _ => false,
        })
        .map(|(at, _)| at)
        .collect();
    assert_eq!(tools.len(), 2);
    assert!(
        tools
            .iter()
            .all(|at| models[0].0 < *at && *at < models[1].0),
        "the replacement's sibling batch ran before the withheld responder"
    );

    // PreLlm: recorded under the reserved candidate, before the replacement
    // was prepared. No other record, prepared or otherwise, uses that id.
    let (candidate_at, candidate) = withheld[0];
    assert_eq!(withholding(candidate), Some(&candidate_withheld()));
    assert!(candidate_at < models[0].0);
    assert_eq!(
        audit
            .iter()
            .filter(|record| record.observation.operation_id == candidate.observation.operation_id)
            .count(),
        1,
        "the withheld candidate never reached preparation or the provider"
    );
    assert!(
        models
            .iter()
            .all(|(_, id)| **id != candidate.observation.operation_id),
        "the replacement is a distinct operation"
    );

    // PostLlm: recorded under the request that actually responded, after its
    // truthful outcome and before the second feedback was prepared.
    let (responder_at, responder) = withheld[1];
    assert_eq!(withholding(responder), Some(&response_withheld()));
    assert_eq!(&responder.observation.operation_id, models[1].1);
    assert!(models[1].0 < responder_at && responder_at < models[2].0);
    let responder_sequence: Vec<_> = audit
        .iter()
        .filter(|record| record.observation.operation_id == responder.observation.operation_id)
        .map(|record| &record.observation.observation)
        .collect();
    assert!(
        matches!(
            responder_sequence.as_slice(),
            [
                AuditObservation::Prepared { .. },
                AuditObservation::Entry,
                AuditObservation::Outcome {
                    outcome: OperationObservedOutcome::HttpResponse { status: 200 }
                },
                AuditObservation::MandatoryHookWithheld { .. },
            ]
        ),
        "the withholding follows the actual responder's outcome and rewrites none of it"
    );
    assert_ne!(models[0].1, models[1].1);
    assert_ne!(models[1].1, models[2].1);
    assert_ne!(models[0].1, models[2].1);
}

/// The committed mandatory-prerequisite notices, each with the exact detail
/// the model receives. The hook's own text and payload are never feedback.
fn committed_notices(messages: &[Message]) -> Vec<(Value, String)> {
    let mut notices = Vec::new();
    for message in messages {
        let Message::SystemNotice(notice) = message else {
            continue;
        };
        for block in &notice.blocks {
            let meerkat_core::types::SystemNoticeBlock::RuntimeNotice {
                category,
                payload: Some(payload),
                ..
            } = block
            else {
                continue;
            };
            if category != NOTICE_CATEGORY {
                continue;
            }
            let detail = block.model_projection_text();
            assert_eq!(serde_json::from_str::<Value>(&detail).unwrap(), *payload);
            let text = notice.model_projection_text();
            assert!(text.contains(&detail));
            assert!(!text.contains(PRIVATE_RATIONALE));
            notices.push((payload.clone(), detail));
        }
    }
    notices
}

/// User-role text one actual provider request carried.
fn user_texts(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .expect("actual Anthropic request messages")
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| match &message["content"] {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        })
        .collect()
}

/// After completion: the gates' complete offers, the controller feedback each
/// withholding owed, and what actually crossed the provider wire.
pub(super) fn assert_completed_writer_turn(
    endpoint: &HookEndpoint,
    session_id: &SessionId,
    run_id: &meerkat_core::RunId,
    messages: &[Message],
    bodies: &[Value],
) {
    let offered = assert_offered(
        endpoint,
        session_id,
        run_id,
        &[
            (HookPoint::PreLlmRequest, 1, true),
            (HookPoint::PreLlmRequest, 3, false),
            (HookPoint::PostLlmResponse, 3, true),
        ],
    );
    let response = offered[2]
        .llm_response
        .as_ref()
        .expect("PostLlm sees the actual response");
    assert_eq!(response.assistant_text, FINISHED);
    assert!(response.tool_call_names.is_empty());
    assert!(response.server_tool_content.is_empty());

    let notices = committed_notices(messages);
    let payloads: Vec<_> = notices.iter().map(|(payload, _)| payload.clone()).collect();
    assert_eq!(
        payloads,
        [
            json!({
                "attribution": {
                    "session_id": session_id, "run_id": run_id,
                    "turn_number": offered[0].turn_number, "hook_id": PRE_LLM_GATE,
                    "point": HookPoint::PreLlmRequest,
                },
                "requirement": "mandatory",
                "disposition": "denied",
                "reason": { "reason_code": HookReasonCode::PolicyViolation },
                "operation_sequence": 1,
                "withheld": { "model_request": true },
                "already_occurred": null,
                "attribution_truncated": false,
                "protected_record": "staged",
            }),
            json!({
                "attribution": {
                    "session_id": session_id, "run_id": run_id,
                    "turn_number": offered[2].turn_number, "hook_id": POST_LLM_GATE,
                    "point": HookPoint::PostLlmResponse,
                },
                "requirement": "mandatory",
                "disposition": "denied",
                "reason": { "reason_code": HookReasonCode::SafetyViolation },
                "operation_sequence": 3,
                "withheld": { "local_tool_calls": 0, "commit": true },
                "already_occurred": {
                    "model_response": true, "text_output": true,
                    "provider_native_tools": [], "provider_native_tools_total": 0,
                    "provider_native_tools_omitted": 0,
                },
                "attribution_truncated": false,
                "protected_record": "staged",
            }),
        ],
        "each withholding feeds the retained controller exactly once, from the staged record"
    );

    // The first request ever sent is the controller-feedback replacement, so
    // the withheld candidate never reached the provider. The rejected
    // response is neither committed nor sent back.
    assert_eq!(bodies.len(), 3);
    let carried: Vec<Vec<usize>> = bodies
        .iter()
        .map(|body| {
            let texts = user_texts(body);
            notices
                .iter()
                .map(|(_, detail)| texts.iter().filter(|text| text.contains(detail)).count())
                .collect()
        })
        .collect();
    assert_eq!(carried, [vec![1, 0], vec![1, 0], vec![1, 1]]);
    for body in bodies {
        assert!(
            !serde_json::to_string(body)
                .unwrap()
                .contains(PRIVATE_RATIONALE)
        );
    }
    assert!(
        !bodies[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "assistant"
                && serde_json::to_string(&message["content"])
                    .unwrap()
                    .contains(FINISHED)),
        "the withheld response was not committed"
    );
}

/// Fresh permitted work: both gates were offered every ordinary operation of
/// the new run and allowed it; nothing was withheld.
pub(super) fn assert_permitted_fresh_turn(
    endpoint: &HookEndpoint,
    audit: &[StoredAuthorizationAuditObservation],
    session_id: &SessionId,
    run_id: &meerkat_core::RunId,
) {
    assert_offered(
        endpoint,
        session_id,
        run_id,
        &[
            (HookPoint::PreLlmRequest, 1, false),
            (HookPoint::PostLlmResponse, 1, false),
            (HookPoint::PreLlmRequest, 2, false),
            (HookPoint::PostLlmResponse, 2, false),
        ],
    );
    assert!(audit.iter().all(|record| withholding(record).is_none()));
}
