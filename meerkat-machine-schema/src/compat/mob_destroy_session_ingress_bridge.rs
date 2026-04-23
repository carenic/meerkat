//! Compat bridging machine that hosts the
//! `mob_destroying_session_ingress` handoff obligation — the C-F3
//! step-lock formalisation for the mob-destroy → session-ingress-detach
//! seam.
//!
//! State-scope audit row F3 flagged that `MeerkatMachine` carries a
//! `peer_ingress_mob_id: Option<MobId>` with no structural
//! "mob-exists" invariant: when a mob is destroyed, every session whose
//! ingress was `MobOwned` by that mob must receive a `DetachIngress`
//! input first, otherwise `peer_ingress_mob_id` is left dangling on a
//! now-retired mob. The `canonical meerkat_mob_seam` route
//! `destroy_request_reaches_meerkat` (`RequestRuntimeDestroy` →
//! `Destroy` input) does not by itself prove that ordering — the shell
//! (`meerkat-mob::runtime::actor::handle_destroy`) retires each member
//! before the `Destroy` input lands, but that ordering is convention,
//! not contract.
//!
//! C-F3 formalises the step-lock as a generated obligation pair on the
//! `mob_destroy_session_ingress_bundle` composition:
//!
//! - `mob_destroying_session_ingress` — producer emits
//!   `RequestSessionIngressDetachForMobDestroy { mob_id,
//!   agent_runtime_id }`; the realising actor
//!   (`mob_destroy_session_ingress_owner`) calls `DetachIngress` on the
//!   target `MeerkatMachine` session and feeds back one of
//!   `SessionIngressDetachedForMobDestroy` (success, mob may now
//!   request `RequestRuntimeDestroy`) or
//!   `SessionIngressDetachFailedForMobDestroy` (failure, surfaces
//!   through the mob's destroy-error report and holds off the destroy).
//!
//! The invariant documented in
//! `meerkat-machine-schema/src/catalog/dsl/meerkat_machine.rs:112` —
//! "MeerkatMachine tolerates a dangling `peer_ingress_mob_id` iff mob
//! destruction propagates `DetachIngress` before destroying the mob" —
//! becomes a compile-time obligation this pair enforces, together
//! with the `xtask seam-inventory` destroy-obligation-pairing check
//! (`## Destroy-obligation Pairing`).
//!
//! Intentionally excluded from the canonical catalog and TLC state
//! space; it exists only for the protocol-codegen producer lookup and
//! the seam-inventory handoff-protocol registration. The producer-side
//! effect is currently emitted by the runtime shell
//! (`meerkat-mob::runtime::actor::handle_destroy`); once the canonical
//! DSL macro grows `handoff_protocol` syntax the bridge can retire.

use crate::identity::{
    EffectVariantId, EnumVariantId, FieldId, InputVariantId, MachineId, NamedTypeId, PhaseId,
    ProtocolId,
};
use crate::{
    EffectDisposition, EffectDispositionRule, EnumSchema, FieldSchema, InitSchema, MachineSchema,
    NamedTypeBinding, RustBinding, StateSchema, TypeRef, VariantSchema,
};

/// Minimal compat machine hosting the mob-destroy → session-ingress-detach
/// handoff obligation's producer annotation.
///
/// One effect, two feedback inputs (success + failure). The effect is
/// emitted from the mob runtime's `handle_destroy` path; the realising
/// actor (`mob_destroy_session_ingress_owner`) calls `DetachIngress` on
/// the target session via `MeerkatMachine` input and emits the feedback
/// through the generated protocol helper.
pub fn mob_destroy_session_ingress_bridge_machine() -> MachineSchema {
    MachineSchema {
        machine: MachineId::parse("MobDestroySessionIngressBridgeMachine")
            .expect("valid machine slug"),
        version: 1,
        rust: RustBinding {
            // The effect is emitted by the mob runtime's destroy path;
            // the bridge points at that module.
            crate_name: "meerkat-mob".into(),
            module: "runtime::actor".into(),
        },
        state: StateSchema {
            phase: EnumSchema {
                name: "MobDestroySessionIngressBridgePhase".into(),
                variants: vec![variant("Idle")],
            },
            fields: vec![],
            init: InitSchema {
                phase: PhaseId::parse("Idle").expect("valid phase slug"),
                fields: vec![],
            },
            terminal_phases: vec![],
        },
        inputs: EnumSchema {
            name: "MobDestroySessionIngressBridgeInput".into(),
            variants: vec![
                VariantSchema {
                    name: EnumVariantId::parse("SessionIngressDetachedForMobDestroy")
                        .expect("valid variant slug"),
                    fields: vec![
                        FieldSchema {
                            name: FieldId::parse("mob_id").expect("valid field slug"),
                            ty: TypeRef::Named(
                                NamedTypeId::parse("MobId").expect("valid named-type slug"),
                            ),
                        },
                        FieldSchema {
                            name: FieldId::parse("agent_runtime_id").expect("valid field slug"),
                            ty: TypeRef::Named(
                                NamedTypeId::parse("AgentRuntimeId")
                                    .expect("valid named-type slug"),
                            ),
                        },
                    ],
                },
                VariantSchema {
                    name: EnumVariantId::parse("SessionIngressDetachFailedForMobDestroy")
                        .expect("valid variant slug"),
                    fields: vec![
                        FieldSchema {
                            name: FieldId::parse("mob_id").expect("valid field slug"),
                            ty: TypeRef::Named(
                                NamedTypeId::parse("MobId").expect("valid named-type slug"),
                            ),
                        },
                        FieldSchema {
                            name: FieldId::parse("agent_runtime_id").expect("valid field slug"),
                            ty: TypeRef::Named(
                                NamedTypeId::parse("AgentRuntimeId")
                                    .expect("valid named-type slug"),
                            ),
                        },
                        FieldSchema {
                            name: FieldId::parse("reason").expect("valid field slug"),
                            ty: TypeRef::String,
                        },
                    ],
                },
            ],
        },
        signals: EnumSchema {
            name: "MobDestroySessionIngressBridgeSignal".into(),
            variants: vec![],
        },
        effects: EnumSchema {
            name: "MobDestroySessionIngressBridgeEffect".into(),
            variants: vec![VariantSchema {
                name: EnumVariantId::parse("RequestSessionIngressDetachForMobDestroy")
                    .expect("valid variant slug"),
                fields: vec![
                    FieldSchema {
                        name: FieldId::parse("mob_id").expect("valid field slug"),
                        ty: TypeRef::Named(
                            NamedTypeId::parse("MobId").expect("valid named-type slug"),
                        ),
                    },
                    FieldSchema {
                        name: FieldId::parse("agent_runtime_id").expect("valid field slug"),
                        ty: TypeRef::Named(
                            NamedTypeId::parse("AgentRuntimeId").expect("valid named-type slug"),
                        ),
                    },
                ],
            }],
        },
        transitions: vec![],
        surface_only_inputs: vec![
            InputVariantId::parse("SessionIngressDetachedForMobDestroy")
                .expect("valid input-variant slug"),
            InputVariantId::parse("SessionIngressDetachFailedForMobDestroy")
                .expect("valid input-variant slug"),
        ],
        helpers: vec![],
        derived: vec![],
        invariants: vec![],
        ci_step_limit: None,
        effect_dispositions: vec![EffectDispositionRule {
            effect_variant: EffectVariantId::parse("RequestSessionIngressDetachForMobDestroy")
                .expect("valid effect-variant slug"),
            disposition: EffectDisposition::External,
            handoff_protocol: Some(
                ProtocolId::parse("mob_destroying_session_ingress").expect("valid protocol slug"),
            ),
        }],
        named_types: vec![
            NamedTypeBinding::string("MobId"),
            NamedTypeBinding::string("AgentRuntimeId"),
        ],
    }
}

fn variant(name: &str) -> VariantSchema {
    VariantSchema {
        name: EnumVariantId::parse(name).expect("valid variant slug"),
        fields: vec![],
    }
}
