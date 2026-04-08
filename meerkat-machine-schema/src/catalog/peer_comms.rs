use indexmap::IndexMap;

use crate::{
    EffectDisposition, EffectDispositionRule, EffectEmit, EnumSchema, Expr, FieldSchema,
    HelperSchema, InitSchema, InputMatch, MachineSchema, RustBinding, StateSchema,
    TransitionSchema, TypeRef, VariantSchema,
};

pub fn peer_comms_machine() -> MachineSchema {
    MachineSchema {
        machine: "PeerCommsMachine".into(),
        version: 2,
        rust: RustBinding {
            crate_name: "meerkat-comms".into(),
            module: "generated::peer_comms".into(),
        },
        state: StateSchema {
            phase: EnumSchema {
                name: "PeerCommsPhase".into(),
                variants: vec![variant("Ready")],
            },
            fields: vec![],
            init: InitSchema {
                phase: "Ready".into(),
                fields: vec![],
            },
            terminal_phases: vec![],
        },
        inputs: EnumSchema {
            name: "PeerCommsInput".into(),
            variants: vec![
                VariantSchema {
                    name: "ClassifyExternalEnvelope".into(),
                    fields: vec![
                        field("raw_item_id", TypeRef::Named("RawItemId".into())),
                        field("require_peer_auth", TypeRef::Bool),
                        field("sender_name_known", TypeRef::Bool),
                        field("sender_name", TypeRef::String),
                        field("fallback_sender_name", TypeRef::String),
                        field("kind", TypeRef::Enum("PeerEnvelopeKind".into())),
                        field("intent", TypeRef::String),
                        field("lifecycle_peer_present", TypeRef::Bool),
                        field("lifecycle_peer", TypeRef::String),
                        field("handling_mode_present", TypeRef::Bool),
                        field("handling_mode", TypeRef::Named("HandlingMode".into())),
                        field("silent_intent", TypeRef::Bool),
                        field("dismiss_message", TypeRef::Bool),
                    ],
                },
                VariantSchema {
                    name: "ClassifyPlainEvent".into(),
                    fields: vec![
                        field("raw_item_id", TypeRef::Named("RawItemId".into())),
                        field("source_name", TypeRef::String),
                        field("handling_mode", TypeRef::Named("HandlingMode".into())),
                    ],
                },
            ],
        },
        effects: EnumSchema {
            name: "PeerCommsEffect".into(),
            variants: vec![
                variant("DropIngress"),
                variant("SetDismissFlag"),
                VariantSchema {
                    name: "EnqueueClassifiedEntry".into(),
                    fields: vec![
                        field("raw_item_id", TypeRef::Named("RawItemId".into())),
                        field("class", TypeRef::Named("PeerInputClass".into())),
                        field("from_peer", TypeRef::Option(Box::new(TypeRef::String))),
                        field("lifecycle_peer", TypeRef::Option(Box::new(TypeRef::String))),
                        field(
                            "normalized_handling_mode",
                            TypeRef::Named("HandlingMode".into()),
                        ),
                    ],
                },
            ],
        },
        helpers: vec![
            HelperSchema {
                name: "EffectiveSender".into(),
                params: vec![
                    field("sender_name_known", TypeRef::Bool),
                    field("sender_name", TypeRef::String),
                    field("fallback_sender_name", TypeRef::String),
                ],
                returns: TypeRef::Option(Box::new(TypeRef::String)),
                body: Expr::IfElse {
                    condition: Box::new(Expr::Binding("sender_name_known".into())),
                    then_expr: Box::new(Expr::Some(Box::new(Expr::Binding("sender_name".into())))),
                    else_expr: Box::new(Expr::Some(Box::new(Expr::Binding(
                        "fallback_sender_name".into(),
                    )))),
                },
            },
            HelperSchema {
                name: "EffectiveLifecyclePeer".into(),
                params: vec![
                    field("lifecycle_peer_present", TypeRef::Bool),
                    field("lifecycle_peer", TypeRef::String),
                    field("sender_name_known", TypeRef::Bool),
                    field("sender_name", TypeRef::String),
                    field("fallback_sender_name", TypeRef::String),
                ],
                returns: TypeRef::Option(Box::new(TypeRef::String)),
                body: Expr::IfElse {
                    condition: Box::new(Expr::Binding("lifecycle_peer_present".into())),
                    then_expr: Box::new(Expr::Some(Box::new(Expr::Binding(
                        "lifecycle_peer".into(),
                    )))),
                    else_expr: Box::new(Expr::Call {
                        helper: "EffectiveSender".into(),
                        args: vec![
                            Expr::Binding("sender_name_known".into()),
                            Expr::Binding("sender_name".into()),
                            Expr::Binding("fallback_sender_name".into()),
                        ],
                    }),
                },
            },
            HelperSchema {
                name: "NormalizedHandlingMode".into(),
                params: vec![
                    field("handling_mode_present", TypeRef::Bool),
                    field("handling_mode", TypeRef::Named("HandlingMode".into())),
                ],
                returns: TypeRef::Named("HandlingMode".into()),
                body: Expr::IfElse {
                    condition: Box::new(Expr::Binding("handling_mode_present".into())),
                    then_expr: Box::new(Expr::Binding("handling_mode".into())),
                    else_expr: Box::new(Expr::NamedVariant {
                        enum_name: "HandlingMode".into(),
                        variant: "Queue".into(),
                    }),
                },
            },
        ],
        derived: vec![],
        invariants: vec![],
        transitions: vec![
            TransitionSchema {
                name: "DropUntrustedExternal".into(),
                from: vec!["Ready".into()],
                on: InputMatch {
                    variant: "ClassifyExternalEnvelope".into(),
                    bindings: vec![
                        "require_peer_auth".into(),
                        "raw_item_id".into(),
                        "sender_name_known".into(),
                        "sender_name".into(),
                        "fallback_sender_name".into(),
                        "kind".into(),
                        "intent".into(),
                        "lifecycle_peer_present".into(),
                        "lifecycle_peer".into(),
                        "handling_mode_present".into(),
                        "handling_mode".into(),
                        "silent_intent".into(),
                        "dismiss_message".into(),
                    ],
                },
                guards: vec![
                    guard_eq("require_peer_auth", Expr::Bool(true)),
                    guard_eq("sender_name_known", Expr::Bool(false)),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![emit("DropIngress", IndexMap::new())],
            },
            TransitionSchema {
                name: "DropAckExternal".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![guard_eq_variant("kind", "PeerEnvelopeKind", "Ack")],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![emit("DropIngress", IndexMap::new())],
            },
            TransitionSchema {
                name: "DismissExternalMessage".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Message"),
                    guard_eq("dismiss_message", Expr::Bool(true)),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![emit("SetDismissFlag", IndexMap::new())],
            },
            TransitionSchema {
                name: "EnqueueLifecycleAdded".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("intent", Expr::String("mob.peer_added".into())),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("PeerLifecycleAdded", true)],
            },
            TransitionSchema {
                name: "EnqueueLifecycleRetired".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("intent", Expr::String("mob.peer_retired".into())),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("PeerLifecycleRetired", true)],
            },
            TransitionSchema {
                name: "EnqueueLifecycleUnwired".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("intent", Expr::String("mob.peer_unwired".into())),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("PeerLifecycleUnwired", true)],
            },
            TransitionSchema {
                name: "EnqueueLifecycleKickoffFailed".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("intent", Expr::String("mob.kickoff_failed".into())),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("PeerLifecycleKickoffFailed", true)],
            },
            TransitionSchema {
                name: "EnqueueLifecycleKickoffCancelled".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("intent", Expr::String("mob.kickoff_cancelled".into())),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("PeerLifecycleKickoffCancelled", true)],
            },
            TransitionSchema {
                name: "EnqueueSilentRequest".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("silent_intent", Expr::Bool(true)),
                    guard_not_mob_lifecycle_intent(),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("SilentRequest", false)],
            },
            TransitionSchema {
                name: "EnqueueActionableRequest".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Request"),
                    guard_eq("silent_intent", Expr::Bool(false)),
                    guard_not_mob_lifecycle_intent(),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("ActionableRequest", false)],
            },
            TransitionSchema {
                name: "EnqueueActionableMessage".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Message"),
                    guard_eq("dismiss_message", Expr::Bool(false)),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("ActionableMessage", false)],
            },
            TransitionSchema {
                name: "EnqueueResponse".into(),
                from: vec!["Ready".into()],
                on: external_input(),
                guards: vec![
                    guard_not_untrusted_external(),
                    guard_eq_variant("kind", "PeerEnvelopeKind", "Response"),
                ],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![enqueue_effect("Response", false)],
            },
            TransitionSchema {
                name: "EnqueuePlainEvent".into(),
                from: vec!["Ready".into()],
                on: InputMatch {
                    variant: "ClassifyPlainEvent".into(),
                    bindings: vec![
                        "raw_item_id".into(),
                        "source_name".into(),
                        "handling_mode".into(),
                    ],
                },
                guards: vec![],
                updates: vec![],
                to: "Ready".into(),
                emit: vec![EffectEmit {
                    variant: "EnqueueClassifiedEntry".into(),
                    fields: IndexMap::from([
                        ("raw_item_id".into(), Expr::Binding("raw_item_id".into())),
                        (
                            "class".into(),
                            Expr::NamedVariant {
                                enum_name: "PeerInputClass".into(),
                                variant: "PlainEvent".into(),
                            },
                        ),
                        ("from_peer".into(), Expr::None),
                        ("lifecycle_peer".into(), Expr::None),
                        (
                            "normalized_handling_mode".into(),
                            Expr::Binding("handling_mode".into()),
                        ),
                    ]),
                }],
            },
        ],
        ci_step_limit: None,
        effect_dispositions: vec![
            disposition("DropIngress", EffectDisposition::External),
            disposition("SetDismissFlag", EffectDisposition::Local),
            disposition("EnqueueClassifiedEntry", EffectDisposition::External),
        ],
    }
}

fn external_input() -> InputMatch {
    InputMatch {
        variant: "ClassifyExternalEnvelope".into(),
        bindings: vec![
            "raw_item_id".into(),
            "require_peer_auth".into(),
            "sender_name_known".into(),
            "sender_name".into(),
            "fallback_sender_name".into(),
            "kind".into(),
            "intent".into(),
            "lifecycle_peer_present".into(),
            "lifecycle_peer".into(),
            "handling_mode_present".into(),
            "handling_mode".into(),
            "silent_intent".into(),
            "dismiss_message".into(),
        ],
    }
}

fn enqueue_effect(class_variant: &str, lifecycle: bool) -> EffectEmit {
    let lifecycle_peer = if lifecycle {
        Expr::Call {
            helper: "EffectiveLifecyclePeer".into(),
            args: vec![
                Expr::Binding("lifecycle_peer_present".into()),
                Expr::Binding("lifecycle_peer".into()),
                Expr::Binding("sender_name_known".into()),
                Expr::Binding("sender_name".into()),
                Expr::Binding("fallback_sender_name".into()),
            ],
        }
    } else {
        Expr::None
    };
    EffectEmit {
        variant: "EnqueueClassifiedEntry".into(),
        fields: IndexMap::from([
            ("raw_item_id".into(), Expr::Binding("raw_item_id".into())),
            (
                "class".into(),
                Expr::NamedVariant {
                    enum_name: "PeerInputClass".into(),
                    variant: class_variant.into(),
                },
            ),
            (
                "from_peer".into(),
                Expr::Call {
                    helper: "EffectiveSender".into(),
                    args: vec![
                        Expr::Binding("sender_name_known".into()),
                        Expr::Binding("sender_name".into()),
                        Expr::Binding("fallback_sender_name".into()),
                    ],
                },
            ),
            ("lifecycle_peer".into(), lifecycle_peer),
            (
                "normalized_handling_mode".into(),
                Expr::Call {
                    helper: "NormalizedHandlingMode".into(),
                    args: vec![
                        Expr::Binding("handling_mode_present".into()),
                        Expr::Binding("handling_mode".into()),
                    ],
                },
            ),
        ]),
    }
}

fn variant(name: &str) -> VariantSchema {
    VariantSchema {
        name: name.into(),
        fields: vec![],
    }
}

fn field(name: &str, ty: TypeRef) -> FieldSchema {
    FieldSchema {
        name: name.into(),
        ty,
    }
}

fn guard_eq(binding: &str, expr: Expr) -> crate::Guard {
    crate::Guard {
        name: format!("{binding}_matches"),
        expr: Expr::Eq(Box::new(Expr::Binding(binding.into())), Box::new(expr)),
    }
}

fn guard_not_untrusted_external() -> crate::Guard {
    crate::Guard {
        name: "not_untrusted_external".into(),
        expr: Expr::Not(Box::new(Expr::And(vec![
            Expr::Eq(
                Box::new(Expr::Binding("require_peer_auth".into())),
                Box::new(Expr::Bool(true)),
            ),
            Expr::Eq(
                Box::new(Expr::Binding("sender_name_known".into())),
                Box::new(Expr::Bool(false)),
            ),
        ]))),
    }
}

fn guard_not_mob_lifecycle_intent() -> crate::Guard {
    crate::Guard {
        name: "not_mob_lifecycle_intent".into(),
        expr: Expr::And(vec![
            Expr::Neq(
                Box::new(Expr::Binding("intent".into())),
                Box::new(Expr::String("mob.peer_added".into())),
            ),
            Expr::Neq(
                Box::new(Expr::Binding("intent".into())),
                Box::new(Expr::String("mob.peer_retired".into())),
            ),
            Expr::Neq(
                Box::new(Expr::Binding("intent".into())),
                Box::new(Expr::String("mob.peer_unwired".into())),
            ),
            Expr::Neq(
                Box::new(Expr::Binding("intent".into())),
                Box::new(Expr::String("mob.kickoff_failed".into())),
            ),
            Expr::Neq(
                Box::new(Expr::Binding("intent".into())),
                Box::new(Expr::String("mob.kickoff_cancelled".into())),
            ),
        ]),
    }
}

fn disposition(name: &str, disposition: EffectDisposition) -> EffectDispositionRule {
    EffectDispositionRule {
        effect_variant: name.into(),
        disposition,
        handoff_protocol: None,
    }
}

fn guard_eq_variant(binding: &str, enum_name: &str, variant: &str) -> crate::Guard {
    guard_eq(
        binding,
        Expr::NamedVariant {
            enum_name: enum_name.into(),
            variant: variant.into(),
        },
    )
}

fn emit(variant: &str, fields: IndexMap<String, Expr>) -> crate::EffectEmit {
    crate::EffectEmit {
        variant: variant.into(),
        fields,
    }
}
