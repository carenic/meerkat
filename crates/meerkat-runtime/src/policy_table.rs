//! Generated admission-policy projection facade.
//!
//! Policy/default facts are resolved by `MeerkatMachine::ResolveAdmissionPlan`.
//! This module remains as a compatibility facade for tests and older callers
//! that ask for a `PolicyDecision` directly.

use crate::identifiers::{InputKind, KindId, PolicyVersion};
use crate::ingress_types::RuntimeInputSemantics;
use crate::input::Input;
use crate::meerkat_machine::dsl as mm_dsl;
use crate::policy::PolicyDecision;
use meerkat_core::lifecycle::RunId;
use meerkat_core::types::SessionId;

pub struct DefaultPolicyTable;

impl DefaultPolicyTable {
    #[allow(clippy::expect_used)]
    pub fn resolve(input: &Input, runtime_idle: bool) -> PolicyDecision {
        Self::try_resolve(input, runtime_idle)
            .expect("generated admission authority must resolve compatibility policy projection")
    }

    pub fn try_resolve(input: &Input, runtime_idle: bool) -> Result<PolicyDecision, String> {
        generated_admission_projection_for_input(input, runtime_idle)
            .map(|projection| projection.policy)
    }

    #[allow(clippy::expect_used)]
    pub fn resolve_by_kind(kind: KindId, runtime_idle: bool) -> PolicyDecision {
        Self::try_resolve_by_kind(kind, runtime_idle)
            .expect("generated admission authority must resolve compatibility policy projection")
    }

    pub fn try_resolve_by_kind(kind: KindId, runtime_idle: bool) -> Result<PolicyDecision, String> {
        generated_admission_projection_for_kind(kind, runtime_idle)
            .map(|projection| projection.policy)
    }
}

pub fn generated_default_policy_version() -> PolicyVersion {
    DefaultPolicyTable::resolve_by_kind(KindId::new(InputKind::Prompt), true).policy_version
}

pub(crate) struct GeneratedAdmissionProjection {
    pub policy: PolicyDecision,
    pub runtime_semantics: RuntimeInputSemantics,
}

pub(crate) fn generated_admission_projection_for_input(
    input: &Input,
    runtime_idle: bool,
) -> Result<GeneratedAdmissionProjection, String> {
    // The shell mirrors the machine's emitted projection: it threads the typed
    // input facts (kind, requested lane, continuation kind) into
    // `ResolveAdmissionPlan` and returns whatever lane/runtime-semantics the
    // machine emits. No shell-side reclassification or override.
    generated_admission_projection(
        input.id().to_string(),
        input.kind(),
        input.handling_mode().map(mm_dsl::InputLane::from),
        mm_dsl::AdmissionContinuationKind::from(input.continuation_kind()),
        crate::input::admission_turn_append_shape(input),
        runtime_idle,
    )
}

pub(crate) fn generated_admission_projection_for_kind(
    kind: KindId,
    runtime_idle: bool,
) -> Result<GeneratedAdmissionProjection, String> {
    generated_admission_projection(
        format!("policy-projection:{:?}", kind.kind()),
        kind.kind(),
        None,
        mm_dsl::AdmissionContinuationKind::Ordinary,
        kind_projection_turn_append_shape(kind.kind()),
        runtime_idle,
    )
}

/// The turn-append shape a per-kind projection reports, with no input at
/// hand: admission refuses `Untyped` for a content kind and a typed shape for
/// any other kind, so a content kind reports the fail-closed `FollowUpOnly`
/// and every other kind `Untyped`.
fn kind_projection_turn_append_shape(kind: InputKind) -> mm_dsl::AdmissionTurnAppendShape {
    match kind {
        InputKind::Prompt
        | InputKind::PeerMessage
        | InputKind::PeerRequest
        | InputKind::FlowStep
        | InputKind::ExternalEvent => mm_dsl::AdmissionTurnAppendShape::FollowUpOnly,
        InputKind::PeerResponseProgress
        | InputKind::PeerResponseTerminal
        | InputKind::Continuation
        | InputKind::Operation => mm_dsl::AdmissionTurnAppendShape::Untyped,
    }
}

fn generated_admission_projection(
    input_id: String,
    input_kind: InputKind,
    requested_lane: Option<mm_dsl::InputLane>,
    continuation_kind: mm_dsl::AdmissionContinuationKind,
    turn_append_shape: mm_dsl::AdmissionTurnAppendShape,
    runtime_idle: bool,
) -> Result<GeneratedAdmissionProjection, String> {
    let mut authority = projection_authority(runtime_idle)?;
    let transition = mm_dsl::MeerkatMachineMutator::apply(
        &mut authority,
        mm_dsl::MeerkatMachineInput::ResolveAdmissionPlan {
            authority_binding: None,
            authority_batch_key: None,
            input_id: input_id.clone(),
            input_kind: mm_dsl::AdmissionInputKind::from(input_kind),
            requested_lane,
            continuation_kind,
            turn_append_shape,
            silent_intent_match: false,
            existing_superseded_input_id: None,
            runtime_running: !runtime_idle,
            active_turn_boundary_available: false,
            without_wake: false,
        },
    )
    .map_err(|err| {
        format!("generated admission authority rejected policy projection for '{input_id}': {err}")
    })?;

    transition
        .into_effects()
        .into_iter()
        .find_map(|effect| match effect {
            mm_dsl::MeerkatMachineEffect::AdmissionResolved {
                input_id: effect_input_id,
                policy_version,
                policy_apply_mode,
                policy_wake_mode,
                policy_queue_mode,
                policy_consume_point,
                policy_drain_policy,
                policy_routing_disposition,
                runtime_boundary,
                runtime_execution_kind,
                runtime_peer_response_terminal_apply_intent,
                record_transcript,
                execution_handling_mode,
                live_interrupt_required,
                live_boundary_delivery,
                ..
            } if effect_input_id == input_id => {
                let apply_mode: crate::policy::ApplyMode = policy_apply_mode.into();
                let boundary: meerkat_core::lifecycle::run_primitive::RunApplyBoundary =
                    runtime_boundary.into();
                let routing_disposition: crate::policy::RoutingDisposition =
                    policy_routing_disposition.into();
                // #24: read the machine-emitted idle-steer normalization directly
                // (`Option<InputLane>` -> `HandlingMode`); the shell normalizer
                // `idle_steer_execution_handling_mode` is deleted.
                let execution_handling_mode = execution_handling_mode.map(|lane| match lane {
                    mm_dsl::InputLane::Queue => meerkat_core::types::HandlingMode::Queue,
                    mm_dsl::InputLane::Steer => meerkat_core::types::HandlingMode::Steer,
                });
                Some(GeneratedAdmissionProjection {
                    policy: PolicyDecision {
                        apply_mode,
                        wake_mode: policy_wake_mode.into(),
                        queue_mode: policy_queue_mode.into(),
                        consume_point: policy_consume_point.into(),
                        drain_policy: policy_drain_policy.into(),
                        routing_disposition,
                        record_transcript,
                        emit_operator_content: record_transcript,
                        policy_version: PolicyVersion(policy_version),
                    },
                    runtime_semantics: RuntimeInputSemantics {
                        boundary,
                        execution_kind: runtime_execution_kind.into(),
                        execution_handling_mode,
                        peer_response_terminal_apply_intent:
                            runtime_peer_response_terminal_apply_intent.map(Into::into),
                        live_interrupt_required,
                        live_boundary_delivery: live_boundary_delivery.map(Into::into),
                    },
                })
            }
            _ => None,
        })
        .ok_or_else(|| {
            format!("generated admission authority emitted no policy projection for '{input_id}'")
        })
}

fn projection_authority(runtime_idle: bool) -> Result<mm_dsl::MeerkatMachineAuthority, String> {
    let session_id = SessionId::new();
    let mut authority =
        crate::meerkat_machine::dsl_authority::new_registered_authority_without_runtime_entry(
            &session_id,
        )
        .map_err(|err| format!("generated admission authority registration failed: {err}"))?;
    if !runtime_idle {
        mm_dsl::MeerkatMachineMutator::apply(
            &mut authority,
            mm_dsl::MeerkatMachineInput::Prepare {
                session_id: mm_dsl::SessionId::from_domain(&session_id),
                run_id: mm_dsl::RunId::from_domain(&RunId::new()),
            },
        )
        .map_err(|err| format!("generated admission authority prepare failed: {err}"))?;
    }
    Ok(authority)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ApplyMode, WakeMode};

    #[test]
    fn compatibility_projection_uses_generated_admission_authority() {
        let idle = DefaultPolicyTable::resolve_by_kind(KindId::new(InputKind::PeerMessage), true);
        let running =
            DefaultPolicyTable::resolve_by_kind(KindId::new(InputKind::PeerMessage), false);

        assert_eq!(idle.apply_mode, ApplyMode::StageRunStart);
        assert_eq!(idle.wake_mode, WakeMode::WakeIfIdle);
        assert_eq!(running.apply_mode, ApplyMode::StageRunStart);
        assert_eq!(running.wake_mode, WakeMode::InterruptYielding);
        assert_eq!(generated_default_policy_version(), idle.policy_version);
    }

    /// Admission pairs the turn-append shape with the input kind: a content
    /// kind (prompt, peer message, request or lifecycle notice, external
    /// event, flow step) carries a typed shape, every other kind `Untyped`.
    /// A content kind admitted `Untyped` would resolve request-only as a
    /// steer and reach one model request without a transcript row.
    #[test]
    fn admission_refuses_an_untyped_shape_for_a_content_kind() {
        use mm_dsl::{AdmissionContinuationKind, AdmissionTurnAppendShape, InputLane};
        let admit = |kind: InputKind,
                     lane: Option<InputLane>,
                     shape: AdmissionTurnAppendShape,
                     runtime_idle: bool| {
            generated_admission_projection(
                format!("shape-guard:{kind:?}:{lane:?}:{shape:?}:{runtime_idle}"),
                kind,
                lane,
                AdmissionContinuationKind::Ordinary,
                shape,
                runtime_idle,
            )
        };
        for runtime_idle in [true, false] {
            for kind in [
                InputKind::Prompt,
                InputKind::PeerMessage,
                InputKind::PeerRequest,
                InputKind::ExternalEvent,
                InputKind::FlowStep,
            ] {
                for lane in [None, Some(InputLane::Queue), Some(InputLane::Steer)] {
                    assert!(
                        admit(kind, lane, AdmissionTurnAppendShape::Untyped, runtime_idle).is_err(),
                        "{kind:?} on {lane:?} (idle {runtime_idle}) admitted Untyped"
                    );
                    for shape in [
                        AdmissionTurnAppendShape::FollowUpOnly,
                        AdmissionTurnAppendShape::InTurnEligible,
                    ] {
                        assert!(
                            admit(kind, lane, shape, runtime_idle).is_ok(),
                            "{kind:?} on {lane:?} (idle {runtime_idle}) refused {shape:?}"
                        );
                    }
                }
            }
            for kind in [
                InputKind::PeerResponseProgress,
                InputKind::PeerResponseTerminal,
                InputKind::Continuation,
                InputKind::Operation,
            ] {
                assert!(
                    admit(kind, None, AdmissionTurnAppendShape::Untyped, runtime_idle).is_ok(),
                    "{kind:?} (idle {runtime_idle}) refused Untyped"
                );
                for shape in [
                    AdmissionTurnAppendShape::FollowUpOnly,
                    AdmissionTurnAppendShape::InTurnEligible,
                ] {
                    assert!(
                        admit(kind, None, shape, runtime_idle).is_err(),
                        "{kind:?} (idle {runtime_idle}) admitted {shape:?}"
                    );
                }
            }
        }
    }

    /// Every kind still resolves its per-kind policy projection.
    #[test]
    fn every_kind_resolves_its_policy_projection() {
        for kind in [
            InputKind::Prompt,
            InputKind::PeerMessage,
            InputKind::PeerRequest,
            InputKind::PeerResponseProgress,
            InputKind::PeerResponseTerminal,
            InputKind::FlowStep,
            InputKind::ExternalEvent,
            InputKind::Continuation,
            InputKind::Operation,
        ] {
            for runtime_idle in [true, false] {
                assert!(
                    DefaultPolicyTable::try_resolve_by_kind(KindId::new(kind), runtime_idle)
                        .is_ok(),
                    "{kind:?} (idle {runtime_idle})"
                );
            }
        }
    }
}
