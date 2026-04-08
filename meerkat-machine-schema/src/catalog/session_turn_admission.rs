use indexmap::IndexMap;

use crate::{
    EffectDisposition, EffectDispositionRule, EnumSchema, Expr, FieldInit, FieldSchema, InitSchema,
    InputMatch, InvariantSchema, MachineSchema, RustBinding, StateSchema, TransitionSchema,
    TypeRef, Update, VariantSchema,
};

pub fn session_turn_admission_machine() -> MachineSchema {
    MachineSchema {
        machine: "SessionTurnAdmissionMachine".into(),
        version: 1,
        rust: RustBinding {
            crate_name: "meerkat-session".into(),
            module: "generated::session_turn_admission".into(),
        },
        state: StateSchema {
            phase: EnumSchema {
                name: "SessionTurnAdmissionPhase".into(),
                variants: vec![
                    variant("Idle"),
                    variant("Admitted"),
                    variant("Running"),
                    variant("Completing"),
                    variant("ShuttingDown"),
                ],
            },
            fields: vec![
                field("interrupt_pending", TypeRef::Bool),
                field("shutdown_pending", TypeRef::Bool),
            ],
            init: InitSchema {
                phase: "Idle".into(),
                fields: vec![
                    init("interrupt_pending", Expr::Bool(false)),
                    init("shutdown_pending", Expr::Bool(false)),
                ],
            },
            terminal_phases: vec!["ShuttingDown".into()],
        },
        inputs: EnumSchema {
            name: "SessionTurnAdmissionInput".into(),
            variants: vec![
                variant("RequestStartTurn"),
                variant("AbortAdmittedTurn"),
                variant("BeginRun"),
                variant("ResolveRun"),
                variant("FinalizeTurn"),
                variant("RequestInterrupt"),
                variant("RequestShutdown"),
            ],
        },
        effects: EnumSchema {
            name: "SessionTurnAdmissionEffect".into(),
            variants: vec![variant("WakeInterrupt")],
        },
        helpers: vec![],
        derived: vec![],
        invariants: vec![InvariantSchema {
            name: "interrupt_pending_only_while_active".into(),
            expr: Expr::Or(vec![
                Expr::Eq(
                    Box::new(Expr::Field("interrupt_pending".into())),
                    Box::new(Expr::Bool(false)),
                ),
                Expr::Or(vec![
                    Expr::Eq(
                        Box::new(Expr::CurrentPhase),
                        Box::new(Expr::Phase("Running".into())),
                    ),
                    Expr::Eq(
                        Box::new(Expr::CurrentPhase),
                        Box::new(Expr::Phase("Completing".into())),
                    ),
                ]),
            ]),
        }],
        transitions: vec![
            TransitionSchema {
                name: "RequestStartTurn".into(),
                from: vec!["Idle".into()],
                on: input("RequestStartTurn"),
                guards: vec![],
                updates: vec![
                    assign("interrupt_pending", Expr::Bool(false)),
                    assign("shutdown_pending", Expr::Bool(false)),
                ],
                to: "Admitted".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "AbortAdmittedTurn".into(),
                from: vec!["Admitted".into()],
                on: input("AbortAdmittedTurn"),
                guards: vec![],
                updates: vec![
                    assign("interrupt_pending", Expr::Bool(false)),
                    assign("shutdown_pending", Expr::Bool(false)),
                ],
                to: "Idle".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "BeginRun".into(),
                from: vec!["Admitted".into()],
                on: input("BeginRun"),
                guards: vec![],
                updates: vec![],
                to: "Running".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "ShutdownFromAdmitted".into(),
                from: vec!["Admitted".into()],
                on: input("RequestShutdown"),
                guards: vec![],
                updates: vec![
                    assign("interrupt_pending", Expr::Bool(false)),
                    assign("shutdown_pending", Expr::Bool(true)),
                ],
                to: "ShuttingDown".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "ResolveRun".into(),
                from: vec!["Running".into()],
                on: input("ResolveRun"),
                guards: vec![],
                updates: vec![],
                to: "Completing".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "RequestInterrupt".into(),
                from: vec!["Running".into()],
                on: input("RequestInterrupt"),
                guards: vec![],
                updates: vec![assign("interrupt_pending", Expr::Bool(true))],
                to: "Running".into(),
                emit: vec![emit("WakeInterrupt")],
            },
            TransitionSchema {
                name: "RequestShutdownFromRunning".into(),
                from: vec!["Running".into()],
                on: input("RequestShutdown"),
                guards: vec![],
                updates: vec![assign("shutdown_pending", Expr::Bool(true))],
                to: "Running".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "RequestShutdownFromCompleting".into(),
                from: vec!["Completing".into()],
                on: input("RequestShutdown"),
                guards: vec![],
                updates: vec![assign("shutdown_pending", Expr::Bool(true))],
                to: "Completing".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "FinalizeTurnToIdle".into(),
                from: vec!["Completing".into()],
                on: input("FinalizeTurn"),
                guards: vec![guard_false("shutdown_pending")],
                updates: vec![
                    assign("interrupt_pending", Expr::Bool(false)),
                    assign("shutdown_pending", Expr::Bool(false)),
                ],
                to: "Idle".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "FinalizeTurnToShuttingDown".into(),
                from: vec!["Completing".into()],
                on: input("FinalizeTurn"),
                guards: vec![guard_true("shutdown_pending")],
                updates: vec![assign("interrupt_pending", Expr::Bool(false))],
                to: "ShuttingDown".into(),
                emit: vec![],
            },
            TransitionSchema {
                name: "RequestShutdownFromIdle".into(),
                from: vec!["Idle".into()],
                on: input("RequestShutdown"),
                guards: vec![],
                updates: vec![
                    assign("interrupt_pending", Expr::Bool(false)),
                    assign("shutdown_pending", Expr::Bool(true)),
                ],
                to: "ShuttingDown".into(),
                emit: vec![],
            },
        ],
        ci_step_limit: None,
        effect_dispositions: vec![disposition("WakeInterrupt", EffectDisposition::Local)],
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

fn init(name: &str, expr: Expr) -> FieldInit {
    FieldInit {
        field: name.into(),
        expr,
    }
}

fn input(variant: &str) -> InputMatch {
    InputMatch {
        variant: variant.into(),
        bindings: vec![],
    }
}

fn assign(field: &str, expr: Expr) -> Update {
    Update::Assign {
        field: field.into(),
        expr,
    }
}

fn emit(variant: &str) -> crate::EffectEmit {
    crate::EffectEmit {
        variant: variant.into(),
        fields: IndexMap::new(),
    }
}

fn guard_true(field: &str) -> crate::Guard {
    crate::Guard {
        name: format!("{field}_true"),
        expr: Expr::Eq(
            Box::new(Expr::Field(field.into())),
            Box::new(Expr::Bool(true)),
        ),
    }
}

fn guard_false(field: &str) -> crate::Guard {
    crate::Guard {
        name: format!("{field}_false"),
        expr: Expr::Eq(
            Box::new(Expr::Field(field.into())),
            Box::new(Expr::Bool(false)),
        ),
    }
}

fn disposition(name: &str, disposition: EffectDisposition) -> EffectDispositionRule {
    EffectDispositionRule {
        effect_variant: name.into(),
        disposition,
        handoff_protocol: None,
    }
}
