use std::collections::BTreeMap;

use crate::{
    ActorKind, ActorSchema, ClosurePolicy, CompositionInvariant, CompositionInvariantKind,
    CompositionSchema, CompositionStateLimits, CompositionTransactionPlan, CompositionWitness,
    EffectHandoffProtocol, EntryInput, FeedbackFieldBinding, FeedbackFieldSource, FeedbackInputRef,
    MachineInstance, ProtocolGenerationMode, ProtocolHelperReturnShape, ProtocolRustBinding, Route,
    RouteBindingSource, RouteDelivery, RouteFieldBinding, RouteTarget, RouteTargetKind,
};

pub fn schedule_bundle_composition() -> CompositionSchema {
    CompositionSchema {
        name: "schedule_bundle".into(),
        machines: vec![
            MachineInstance {
                instance_id: "schedule".into(),
                machine_name: "ScheduleLifecycleMachine".into(),
                actor: "schedule_authority".into(),
            },
            MachineInstance {
                instance_id: "occurrence".into(),
                machine_name: "OccurrenceLifecycleMachine".into(),
                actor: "occurrence_authority".into(),
            },
        ],
        actors: vec![
            machine_actor("schedule_authority"),
            machine_actor("occurrence_authority"),
        ],
        handoff_protocols: vec![],
        entry_inputs: vec![],
        routes: vec![route(
            "revision_supersede_enters_occurrence_authority",
            "schedule",
            "SupersedePendingOccurrences",
            "occurrence",
            RouteTargetKind::Input,
            "Supersede",
            &[bind("superseded_by_revision", "superseding_revision")],
        )],
        route_target_selectors: vec![],
        driver: None,
        transaction_plans: vec![
            transaction_plan(
                "transactional_claim",
                "claim_due_occurrences",
                "store-backed claim uses authoritative store time plus durable lease state",
                "ScheduleStore::claim_due_occurrences",
            ),
            transaction_plan(
                "revision_supersede_and_replan",
                "update_schedule_revision",
                "revision-affecting schedule updates supersede pending future occurrences before replanning",
                "ScheduleStore::commit_schedule_mutation",
            ),
        ],
        actor_priorities: vec![],
        scheduler_rules: vec![],
        invariants: vec![
            CompositionInvariant {
                name: "schedule_revision_supersede_route_present".into(),
                kind: CompositionInvariantKind::RoutePresent {
                    from_machine: "schedule".into(),
                    effect_variant: "SupersedePendingOccurrences".into(),
                    to_machine: "occurrence".into(),
                    input_variant: "Supersede".into(),
                },
                statement: "revision-affecting schedule edits enter occurrence authority through the explicit supersede route".into(),
                references_machines: vec!["schedule".into(), "occurrence".into()],
                references_actors: vec!["schedule_authority".into(), "occurrence_authority".into()],
            },
            CompositionInvariant {
                name: "superseded_occurrence_originates_from_schedule_revision".into(),
                kind: CompositionInvariantKind::ObservedRouteInputOriginatesFromEffect {
                    route_name: "revision_supersede_enters_occurrence_authority".into(),
                    to_machine: "occurrence".into(),
                    input_variant: "Supersede".into(),
                    from_machine: "schedule".into(),
                    effect_variant: "SupersedePendingOccurrences".into(),
                },
                statement: "pending future occurrences are superseded only by the schedule revision route rather than by ad hoc shell mutation".into(),
                references_machines: vec!["schedule".into(), "occurrence".into()],
                references_actors: vec!["schedule_authority".into(), "occurrence_authority".into()],
            },
        ],
        witnesses: vec![
            witness(
                "revision_supersede_route",
                &["revision_supersede_enters_occurrence_authority"],
            ),
            witness("pause_resume_without_revision", &[]),
        ],
        deep_domain_cardinality: 3,
        deep_domain_overrides: std::collections::BTreeMap::new(),
        witness_domain_cardinality: 2,
        ci_limits: Some(default_ci_limits()),
        closed_world: true,
    }
}

pub fn schedule_runtime_bundle_composition() -> CompositionSchema {
    CompositionSchema {
        name: "schedule_runtime_bundle".into(),
        machines: vec![MachineInstance {
            instance_id: "occurrence".into(),
            machine_name: "OccurrenceLifecycleMachine".into(),
            actor: "occurrence_authority".into(),
        }],
        actors: vec![machine_actor("occurrence_authority")],
        handoff_protocols: vec![],
        entry_inputs: vec![],
        routes: vec![],
        route_target_selectors: vec![],
        driver: None,
        transaction_plans: vec![transaction_plan(
            "transactional_runtime_claim",
            "claim_and_runtime_handoff",
            "transactional claim establishes the durable lease before runtime delivery begins",
            "ScheduleStore::claim_due_occurrences",
        )],
        actor_priorities: vec![],
        scheduler_rules: vec![],
        invariants: vec![],
        witnesses: vec![
            witness("runtime_delivery_feedback", &[]),
            witness("runtime_lease_expiry", &[]),
        ],
        deep_domain_cardinality: 3,
        deep_domain_overrides: std::collections::BTreeMap::new(),
        witness_domain_cardinality: 2,
        ci_limits: Some(default_ci_limits()),
        closed_world: true,
    }
}

pub fn schedule_mob_bundle_composition() -> CompositionSchema {
    CompositionSchema {
        name: "schedule_mob_bundle".into(),
        machines: vec![MachineInstance {
            instance_id: "occurrence".into(),
            machine_name: "OccurrenceLifecycleMachine".into(),
            actor: "occurrence_authority".into(),
        }],
        actors: vec![machine_actor("occurrence_authority")],
        handoff_protocols: vec![],
        entry_inputs: vec![],
        routes: vec![],
        route_target_selectors: vec![],
        driver: None,
        transaction_plans: vec![transaction_plan(
            "transactional_mob_claim",
            "claim_and_mob_handoff",
            "transactional claim establishes the durable lease before mob delivery begins",
            "ScheduleStore::claim_due_occurrences",
        )],
        actor_priorities: vec![],
        scheduler_rules: vec![],
        invariants: vec![],
        witnesses: vec![
            witness("mob_delivery_feedback", &[]),
            witness("materialization_failure_classification", &[]),
        ],
        deep_domain_cardinality: 3,
        deep_domain_overrides: std::collections::BTreeMap::new(),
        witness_domain_cardinality: 2,
        ci_limits: Some(default_ci_limits()),
        closed_world: true,
    }
}

pub fn meerkat_mob_seam_composition() -> CompositionSchema {
    CompositionSchema {
        name: "meerkat_mob_seam".into(),
        machines: vec![
            MachineInstance {
                instance_id: "meerkat".into(),
                machine_name: "MeerkatMachine".into(),
                actor: "meerkat_kernel".into(),
            },
            MachineInstance {
                instance_id: "mob".into(),
                machine_name: "MobMachine".into(),
                actor: "mob_kernel".into(),
            },
        ],
        actors: vec![machine_actor("meerkat_kernel"), machine_actor("mob_kernel")],
        handoff_protocols: vec![],
        entry_inputs: vec![
            EntryInput {
                name: "spawn_member".into(),
                machine: "mob".into(),
                input_variant: "Spawn".into(),
            },
            EntryInput {
                name: "submit_work".into(),
                machine: "mob".into(),
                input_variant: "SubmitWork".into(),
            },
            EntryInput {
                name: "retire_member".into(),
                machine: "mob".into(),
                input_variant: "Retire".into(),
            },
            EntryInput {
                name: "destroy_mob".into(),
                machine: "mob".into(),
                input_variant: "Destroy".into(),
            },
        ],
        routes: vec![
            route(
                "binding_request_reaches_meerkat",
                "mob",
                "RequestRuntimeBinding",
                "meerkat",
                RouteTargetKind::Input,
                "PrepareBindings",
                &[
                    bind("agent_runtime_id", "agent_runtime_id"),
                    bind("fence_token", "fence_token"),
                    bind("generation", "generation"),
                ],
            ),
            route(
                "work_request_reaches_meerkat",
                "mob",
                "RequestRuntimeIngress",
                "meerkat",
                RouteTargetKind::Input,
                "Ingest",
                &[
                    bind("runtime_id", "agent_runtime_id"),
                    bind("work_id", "work_id"),
                    bind("origin", "origin"),
                ],
            ),
            route(
                "retire_request_reaches_meerkat",
                "mob",
                "RequestRuntimeRetire",
                "meerkat",
                RouteTargetKind::Input,
                "Retire",
                &[],
            ),
            route(
                "destroy_request_reaches_meerkat",
                "mob",
                "RequestRuntimeDestroy",
                "meerkat",
                RouteTargetKind::Input,
                "Destroy",
                &[],
            ),
            route(
                "runtime_bound_reaches_mob",
                "meerkat",
                "RuntimeBound",
                "mob",
                RouteTargetKind::Signal,
                "ObserveRuntimeReady",
                &[
                    bind("agent_runtime_id", "agent_runtime_id"),
                    bind("fence_token", "fence_token"),
                ],
            ),
            route(
                "runtime_retired_reaches_mob",
                "meerkat",
                "RuntimeRetired",
                "mob",
                RouteTargetKind::Signal,
                "ObserveRuntimeRetired",
                &[
                    bind("agent_runtime_id", "agent_runtime_id"),
                    bind("fence_token", "fence_token"),
                ],
            ),
            route(
                "runtime_destroyed_reaches_mob",
                "meerkat",
                "RuntimeDestroyed",
                "mob",
                RouteTargetKind::Signal,
                "ObserveRuntimeDestroyed",
                &[
                    bind("agent_runtime_id", "agent_runtime_id"),
                    bind("fence_token", "fence_token"),
                ],
            ),
        ],
        route_target_selectors: vec![],
        driver: None,
        transaction_plans: vec![],
        actor_priorities: vec![],
        scheduler_rules: vec![],
        invariants: vec![],
        witnesses: vec![
            witness(
                "basic_round_trip",
                &[
                    "binding_request_reaches_meerkat",
                    "work_request_reaches_meerkat",
                    "runtime_bound_reaches_mob",
                ],
            ),
            witness(
                "retire_runtime_path",
                &[
                    "retire_request_reaches_meerkat",
                    "runtime_retired_reaches_mob",
                ],
            ),
            witness(
                "destroy_runtime_path",
                &[
                    "destroy_request_reaches_meerkat",
                    "runtime_destroyed_reaches_mob",
                ],
            ),
        ],
        deep_domain_cardinality: 3,
        deep_domain_overrides: std::collections::BTreeMap::new(),
        witness_domain_cardinality: 2,
        ci_limits: Some(default_ci_limits()),
        closed_world: true,
    }
}

fn machine_actor(name: &str) -> ActorSchema {
    ActorSchema {
        name: name.into(),
        kind: ActorKind::Machine,
    }
}

fn route(
    name: &str,
    from_machine: &str,
    effect_variant: &str,
    to_machine: &str,
    target_kind: RouteTargetKind,
    input_variant: &str,
    bindings: &[RouteFieldBinding],
) -> Route {
    Route {
        name: name.into(),
        from_machine: from_machine.into(),
        effect_variant: effect_variant.into(),
        to: RouteTarget {
            machine: to_machine.into(),
            kind: target_kind,
            input_variant: input_variant.into(),
        },
        bindings: bindings.to_vec(),
        delivery: RouteDelivery::Immediate,
    }
}

fn bind(to_field: &str, from_field: &str) -> RouteFieldBinding {
    RouteFieldBinding {
        to_field: to_field.into(),
        source: RouteBindingSource::Field {
            from_field: from_field.into(),
            allow_named_alias: false,
        },
    }
}

fn transaction_plan(
    name: &str,
    trigger: &str,
    description: &str,
    store_primitive: &str,
) -> CompositionTransactionPlan {
    CompositionTransactionPlan {
        name: name.into(),
        trigger: trigger.into(),
        description: description.into(),
        store_primitive: store_primitive.into(),
        route_names: vec![],
        protocol_names: vec![],
    }
}

fn witness(name: &str, expected_routes: &[&str]) -> CompositionWitness {
    CompositionWitness {
        name: name.into(),
        preload_inputs: vec![],
        expected_routes: expected_routes
            .iter()
            .map(|route| (*route).into())
            .collect(),
        expected_scheduler_rules: vec![],
        expected_states: vec![],
        expected_transitions: vec![],
        expected_transition_order: vec![],
        state_limits: default_ci_limits(),
    }
}

fn default_ci_limits() -> CompositionStateLimits {
    CompositionStateLimits {
        step_limit: 8,
        pending_input_limit: 8,
        pending_route_limit: 8,
        delivered_route_limit: 0,
        emitted_effect_limit: 0,
        seq_limit: 0,
        set_limit: 0,
        map_limit: 0,
    }
}

/// Compositions declared to host cross-machine handoff protocols whose
/// producer side is backed either by a compat machine (flow/loop) or by
/// the canonical `MeerkatMachine` with an absorbed handoff effect that
/// the runtime authority fills in. They sit alongside
/// `canonical_composition_schemas()` in the codegen iteration.
///
/// Populated as each handoff protocol's producer side is wired up.
pub fn compat_composition_schemas() -> Vec<CompositionSchema> {
    vec![mob_bundle_composition()]
}

/// Host composition for the `ops_barrier_satisfaction` handoff protocol.
///
/// The producer is the canonical `MeerkatMachine` which declares
/// `WaitAllSatisfied { wait_request_id, operation_ids }` as a
/// local-disposition effect. The realizing owner — the runtime's ops
/// lifecycle shell — observes the barrier closure and feeds back
/// through the `OpsBarrierSatisfied { operation_ids }` input; the
/// `HandleBridge` mode routes the same payload through
/// `TurnStateHandle::ops_barrier_satisfied` for runtime-backed sessions.
///
/// Modes: primary `ShellBridge` (accept + authority.apply submitters),
/// secondary `HandleBridge` (handle-driven submitter suffixed `_handle`).
fn mob_bundle_composition() -> CompositionSchema {
    let mut handle_methods = BTreeMap::new();
    handle_methods.insert("OpsBarrierSatisfied".into(), "ops_barrier_satisfied".into());
    // Handle method takes `operation_ids: BTreeSet<String>` — the obligation
    // field is `Set<OperationId>` which renders as `Vec<OperationId>`. The
    // accessor rewrites the reference to stringify each operation id.
    let mut handle_accessors = BTreeMap::new();
    handle_accessors.insert(
        "OpsBarrierSatisfied.operation_ids".into(),
        ".iter().map(ToString::to_string).collect()".into(),
    );
    // Handle method takes only `operation_ids`; the obligation carries
    // a `wait_request_id` correlation token that the turn-state handle
    // never consumes (the ops-lifecycle owner matches on it internally,
    // not through the handle).
    let mut handle_forwarded_fields = BTreeMap::new();
    handle_forwarded_fields.insert("OpsBarrierSatisfied".into(), vec!["operation_ids".into()]);

    CompositionSchema {
        name: "mob_bundle".into(),
        // The producer is the compat `OpsBarrierBridgeMachine` which hosts
        // the handoff-annotated `WaitAllSatisfied` effect declaration.
        // Its shape mirrors the runtime-owned effect; the canonical
        // `MeerkatMachine` also declares `WaitAllSatisfied` (without the
        // handoff annotation the DSL macro cannot emit) so the runtime
        // shell still observes the effect through its own reducer.
        machines: vec![MachineInstance {
            instance_id: "ops_barrier_bridge".into(),
            machine_name: "OpsBarrierBridgeMachine".into(),
            actor: "ops_barrier_bridge_authority".into(),
        }],
        actors: vec![
            machine_actor("ops_barrier_bridge_authority"),
            owner_actor("ops_lifecycle_owner"),
        ],
        handoff_protocols: vec![EffectHandoffProtocol {
            name: "ops_barrier_satisfaction".into(),
            producer_instance: "ops_barrier_bridge".into(),
            effect_variant: "WaitAllSatisfied".into(),
            realizing_actor: "ops_lifecycle_owner".into(),
            correlation_fields: vec!["wait_request_id".into()],
            obligation_fields: vec!["wait_request_id".into(), "operation_ids".into()],
            allowed_feedback_inputs: vec![FeedbackInputRef {
                machine_instance: "ops_barrier_bridge".into(),
                input_variant: "OpsBarrierSatisfied".into(),
                field_bindings: vec![
                    FeedbackFieldBinding {
                        input_field: "wait_request_id".into(),
                        source: FeedbackFieldSource::ObligationField(
                            "wait_request_id".into(),
                        ),
                    },
                    FeedbackFieldBinding {
                        input_field: "operation_ids".into(),
                        source: FeedbackFieldSource::ObligationField("operation_ids".into()),
                    },
                ],
            }],
            closure_policy: ClosurePolicy::AckRequired,
            liveness_annotation: Some(
                "eventual feedback under task-scheduling fairness".into(),
            ),
            rust: ProtocolRustBinding {
                module_path: "meerkat-core/src/generated/protocol_ops_barrier_satisfaction.rs"
                    .into(),
                // Primary mode: HandleBridge. The obligation is built
                // from the shell-owned `WaitAllSatisfied` struct via the
                // shared `accept_<effect>` emission; feedback flows
                // through the `TurnStateHandle::ops_barrier_satisfied`
                // trait method. The compat `OpsBarrierBridgeMachine`
                // hosts the handoff annotation purely so the protocol
                // passes composition validation — it has no runtime
                // authority of its own, so stacking ShellBridge with an
                // `authority.apply` submitter would point at nothing.
                generation_mode: ProtocolGenerationMode::HandleBridge,
                required_imports: vec![
                    "use crate::handles::TurnStateHandle;".into(),
                    "use crate::lifecycle::identifiers::WaitRequestId;".into(),
                    "use crate::ops::OperationId;".into(),
                    "use crate::ops_lifecycle::WaitAllSatisfied;".into(),
                ],
                authority_type_path: None,
                mutator_trait_path: None,
                input_enum_path: None,
                effect_enum_path: None,
                transition_type_path: None,
                error_type_path: None,
                executor_trigger_input_variant: None,
                bridge_source_type_path: Some("crate::ops_lifecycle::WaitAllSatisfied".into()),
                helper_return_shape: ProtocolHelperReturnShape::Obligations,
                handle_trait_path: Some("meerkat_core::handles::TurnStateHandle".into()),
                handle_method_names: handle_methods,
                handle_arg_accessors: handle_accessors,
                handle_method_forwarded_fields: handle_forwarded_fields,
                additional_modes: vec![],
            },
        }],
        entry_inputs: vec![],
        routes: vec![],
        route_target_selectors: vec![],
        driver: None,
        transaction_plans: vec![],
        actor_priorities: vec![],
        scheduler_rules: vec![],
        invariants: vec![CompositionInvariant {
            name: "ops_barrier_satisfaction_protocol_covered".into(),
            kind: CompositionInvariantKind::HandoffProtocolCovered {
                producer_instance: "ops_barrier_bridge".into(),
                effect_variant: "WaitAllSatisfied".into(),
                protocol_name: "ops_barrier_satisfaction".into(),
            },
            statement: "wait-all barrier satisfaction crosses from the ops lifecycle owner back into turn-state authority only through the explicit `ops_barrier_satisfaction` protocol".into(),
            references_machines: vec!["ops_barrier_bridge".into()],
            references_actors: vec![
                "ops_barrier_bridge_authority".into(),
                "ops_lifecycle_owner".into(),
            ],
        }],
        witnesses: vec![witness("ops_barrier_close_round_trip", &[])],
        deep_domain_cardinality: 3,
        deep_domain_overrides: std::collections::BTreeMap::new(),
        witness_domain_cardinality: 2,
        ci_limits: Some(default_ci_limits()),
        closed_world: true,
    }
}

fn owner_actor(name: &str) -> ActorSchema {
    ActorSchema {
        name: name.into(),
        kind: ActorKind::Owner,
    }
}
