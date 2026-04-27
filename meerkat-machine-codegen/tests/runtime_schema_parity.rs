use meerkat_machine_schema::catalog::dsl::{
    dsl_auth_machine, dsl_meerkat_machine, dsl_mob_machine, dsl_occurrence_lifecycle_machine,
    dsl_schedule_lifecycle_machine,
};
use meerkat_machine_schema::identity::{EnumVariantId, IdentityError, InputVariantId};
use meerkat_machine_schema::{
    EffectDispositionRule, FieldSchema, MachineSchema, NamedTypeBinding, TransitionSchema,
    VariantSchema, canonical_machine_schemas,
};
use std::collections::{BTreeMap, BTreeSet};

fn input_alphabet(schema: &MachineSchema) -> Result<BTreeSet<InputVariantId>, IdentityError> {
    schema
        .inputs
        .variants
        .iter()
        .map(|variant| InputVariantId::parse(variant.name.as_str()))
        .collect()
}

struct SchemaParityCase {
    machine: &'static str,
    catalog_schema: fn() -> MachineSchema,
    production_schema: fn() -> MachineSchema,
}

fn schema_shape_mismatches(case: SchemaParityCase) -> Vec<String> {
    let catalog = (case.catalog_schema)();
    let production = (case.production_schema)();
    schema_shape_mismatches_for_schemas(&catalog, &production)
}

fn schema_shape_mismatches_for_schemas(
    catalog: &MachineSchema,
    production: &MachineSchema,
) -> Vec<String> {
    let mut mismatches = Vec::new();

    if catalog.machine != production.machine {
        mismatches.push("machine id".to_owned());
    }
    if catalog.version != production.version {
        mismatches.push("version".to_owned());
    }
    if catalog.state != production.state {
        mismatches.push("state".to_owned());
    }
    if catalog.inputs != production.inputs {
        mismatches.push("inputs".to_owned());
    }
    if catalog.surface_only_inputs != production.surface_only_inputs {
        mismatches.push("surface-only input metadata".to_owned());
    }
    if catalog.runtime_internal_inputs != production.runtime_internal_inputs {
        mismatches.push("runtime-internal input metadata".to_owned());
    }
    if catalog.named_types != production.named_types {
        mismatches.push("named types".to_owned());
    }
    if catalog.signals != production.signals {
        mismatches.push("signals".to_owned());
    }
    if catalog.effects != production.effects {
        mismatches.push("effects".to_owned());
    }
    if catalog.helpers != production.helpers {
        mismatches.push("helpers".to_owned());
    }
    if catalog.derived != production.derived {
        mismatches.push("derived helpers".to_owned());
    }
    if catalog.invariants != production.invariants {
        mismatches.push("invariants".to_owned());
    }
    if catalog.transitions != production.transitions {
        mismatches.push("transitions".to_owned());
    }
    if catalog.effect_dispositions != production.effect_dispositions {
        mismatches.push("effect dispositions / handoff metadata".to_owned());
    }
    if catalog.ci_step_limit != production.ci_step_limit {
        mismatches.push("CI semantic-model step-limit metadata".to_owned());
    }

    mismatches
}

fn phase1_schema_parity_cases() -> [SchemaParityCase; 5] {
    [
        SchemaParityCase {
            machine: "MeerkatMachine",
            catalog_schema: dsl_meerkat_machine,
            production_schema: meerkat_runtime::machine_schema_exports::meerkat_machine_schema,
        },
        SchemaParityCase {
            machine: "AuthMachine",
            catalog_schema: dsl_auth_machine,
            production_schema: meerkat_runtime::machine_schema_exports::auth_machine_schema,
        },
        SchemaParityCase {
            machine: "MobMachine",
            catalog_schema: dsl_mob_machine,
            production_schema: meerkat_mob::machine_schema_exports::mob_machine_schema,
        },
        SchemaParityCase {
            machine: "ScheduleLifecycleMachine",
            catalog_schema: dsl_schedule_lifecycle_machine,
            production_schema: meerkat_schedule::machine_schema_exports::schedule_lifecycle_schema,
        },
        SchemaParityCase {
            machine: "OccurrenceLifecycleMachine",
            catalog_schema: dsl_occurrence_lifecycle_machine,
            production_schema:
                meerkat_schedule::machine_schema_exports::occurrence_lifecycle_schema,
        },
    ]
}

fn phase1_schema_drift_report() -> Vec<String> {
    let mut failures = Vec::new();
    for case in phase1_schema_parity_cases() {
        let machine = case.machine;
        let mismatches = schema_shape_mismatches(case);
        if !mismatches.is_empty() {
            failures.push(format!("{machine}: {}", mismatches.join(", ")));
        }
    }
    failures
}

fn fields_by_name(fields: &[FieldSchema]) -> BTreeMap<String, &FieldSchema> {
    fields
        .iter()
        .map(|field| (field.name.as_str().to_owned(), field))
        .collect()
}

fn variants_by_name(variants: &[VariantSchema]) -> BTreeMap<String, &VariantSchema> {
    variants
        .iter()
        .map(|variant| (variant.name.as_str().to_owned(), variant))
        .collect()
}

fn transitions_by_name(transitions: &[TransitionSchema]) -> BTreeMap<String, &TransitionSchema> {
    transitions
        .iter()
        .map(|transition| (transition.name.as_str().to_owned(), transition))
        .collect()
}

fn dispositions_by_name(
    dispositions: &[EffectDispositionRule],
) -> BTreeMap<String, &EffectDispositionRule> {
    dispositions
        .iter()
        .map(|rule| (rule.effect_variant.as_str().to_owned(), rule))
        .collect()
}

fn named_types_by_name(bindings: &[NamedTypeBinding]) -> BTreeMap<String, &NamedTypeBinding> {
    bindings
        .iter()
        .map(|binding| (binding.name.as_str().to_owned(), binding))
        .collect()
}

fn describe_key_diff<T: PartialEq + std::fmt::Debug>(
    label: &str,
    catalog: BTreeMap<String, &T>,
    production: BTreeMap<String, &T>,
    out: &mut Vec<String>,
) {
    for name in production.keys() {
        if !catalog.contains_key(name) {
            out.push(format!("{label}.production_only.{name}"));
        }
    }
    for name in catalog.keys() {
        if !production.contains_key(name) {
            out.push(format!("{label}.catalog_only.{name}"));
        }
    }
    for (name, catalog_item) in &catalog {
        if let Some(production_item) = production.get(name) {
            if *catalog_item != *production_item {
                out.push(format!("{label}.changed.{name}"));
            }
        }
    }
}

fn schema_drift_items_for_schemas(
    catalog: &MachineSchema,
    production: &MachineSchema,
) -> Vec<String> {
    let mut items = Vec::new();

    describe_key_diff(
        "state_field",
        fields_by_name(&catalog.state.fields),
        fields_by_name(&production.state.fields),
        &mut items,
    );
    describe_key_diff(
        "init_field",
        catalog
            .state
            .init
            .fields
            .iter()
            .map(|field| (field.field.as_str().to_owned(), field))
            .collect(),
        production
            .state
            .init
            .fields
            .iter()
            .map(|field| (field.field.as_str().to_owned(), field))
            .collect(),
        &mut items,
    );
    describe_key_diff(
        "phase",
        variants_by_name(&catalog.state.phase.variants),
        variants_by_name(&production.state.phase.variants),
        &mut items,
    );
    describe_key_diff(
        "input",
        variants_by_name(&catalog.inputs.variants),
        variants_by_name(&production.inputs.variants),
        &mut items,
    );
    describe_key_diff(
        "signal",
        variants_by_name(&catalog.signals.variants),
        variants_by_name(&production.signals.variants),
        &mut items,
    );
    describe_key_diff(
        "effect",
        variants_by_name(&catalog.effects.variants),
        variants_by_name(&production.effects.variants),
        &mut items,
    );
    describe_key_diff(
        "transition",
        transitions_by_name(&catalog.transitions),
        transitions_by_name(&production.transitions),
        &mut items,
    );
    describe_key_diff(
        "effect_disposition",
        dispositions_by_name(&catalog.effect_dispositions),
        dispositions_by_name(&production.effect_dispositions),
        &mut items,
    );
    describe_key_diff(
        "named_type",
        named_types_by_name(&catalog.named_types),
        named_types_by_name(&production.named_types),
        &mut items,
    );

    items
}

fn phase1_schema_drift_item_report() -> Vec<String> {
    let mut items = Vec::new();
    for case in phase1_schema_parity_cases() {
        let catalog = (case.catalog_schema)();
        let production = (case.production_schema)();
        for item in schema_drift_items_for_schemas(&catalog, &production) {
            items.push(format!("{}::{item}", case.machine));
        }
    }
    items
}

fn phase1_schema_drift_item_counts() -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for item in phase1_schema_drift_item_report() {
        let mut parts = item.split("::");
        let machine = parts.next().expect("machine");
        let rest = parts.next().expect("item");
        let mut item_parts = rest.split('.');
        let category = item_parts.next().expect("category");
        let side = item_parts.next().expect("side");
        *counts
            .entry(format!("{machine}::{category}.{side}"))
            .or_insert(0) += 1;
    }
    counts
}

const MOB_RUNTIME_PARITY_PROBED_INPUT_VARIANTS: &[&str] = &[
    "Spawn",
    "SubmitWork",
    "RunFlow",
    "CancelFlow",
    "Retire",
    "Respawn",
    "RetireAll",
    "CancelWork",
    "CancelAllWork",
    "Stop",
    "Resume",
    "Complete",
    "Reset",
    "Destroy",
    "TaskCreate",
    "TaskUpdate",
    "SubscribeAgentEvents",
    "SubscribeAllAgentEvents",
    "SubscribeMobEvents",
    "RecordOperatorActionProvenance",
    "SetSpawnPolicy",
    "Shutdown",
    "ForceCancel",
];

const MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS: &[&str] = &[
    "CreateRunSeed",
    "CreateFrameSeed",
    "CreateLoopSeed",
    "RecordLoopBodyFrameCompleted",
    "RecordLoopUntilConditionMet",
    "RecordLoopUntilConditionFailed",
    "AuthorizeFlowRunReducerCommand",
    "AuthorizeFlowFrameReducerCommand",
    "AuthorizeLoopIterationReducerCommand",
    "EnsureMember",
    "Reconcile",
    "WireMembers",
    "UnwireMembers",
    "WireExternalPeer",
    "UnwireExternalPeer",
    "BindMemberSession",
    "RotateMemberSession",
    "ReleaseMemberSession",
    "SessionIngressDetachedForMobDestroy",
    "SessionIngressDetachFailedForMobDestroy",
    "KickoffMarkPending",
    "KickoffMarkStarting",
    "StartupMarkReady",
    "KickoffResolveStarted",
    "KickoffResolveCallbackPending",
    "KickoffResolveFailed",
    "KickoffResolveCancelled",
    "KickoffCancelRequested",
    "KickoffClear",
];

fn mob_runtime_parity_probe_inventory_mismatches(schema: &MachineSchema) -> Vec<String> {
    let catalog_inputs = schema
        .inputs
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect::<BTreeSet<_>>();
    let surface_only_inputs = schema
        .surface_only_inputs
        .iter()
        .map(|name| name.as_str())
        .collect::<BTreeSet<_>>();
    let probed = MOB_RUNTIME_PARITY_PROBED_INPUT_VARIANTS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let carry_forward = MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut mismatches = Vec::new();

    for input in &probed {
        if !catalog_inputs.contains(input) {
            mismatches.push(format!("runtime_probe_only.{input}"));
        }
    }
    for input in &carry_forward {
        if !catalog_inputs.contains(input) {
            mismatches.push(format!("carry_forward_only.{input}"));
        }
    }
    for input in catalog_inputs.difference(&surface_only_inputs) {
        if !probed.contains(input) && !carry_forward.contains(input) {
            mismatches.push(format!("catalog_input_unprobed.{input}"));
        }
    }

    mismatches
}

#[test]
fn phase1_production_machine_schemas_match_catalog_shape() {
    let failures = phase1_schema_drift_report();
    assert!(
        failures.is_empty(),
        "catalog/production schema drift detected outside crate/module binding metadata:\n- {}",
        failures.join("\n- ")
    );
}

#[test]
fn phase1_schema_parity_inventory_reports_current_carry_forward_drift() {
    let failures = phase1_schema_drift_report();
    assert_eq!(
        failures,
        Vec::<String>::new(),
        "Phase 1 schema parity must stay closed once catalog-owned production bodies converge"
    );
}

#[test]
fn phase1_schema_parity_inventory_has_itemized_drift() {
    let items = phase1_schema_drift_item_report();
    assert!(
        !items.iter().any(|item| item.starts_with("AuthMachine::")),
        "AuthMachine drift should be resolved by the first Phase 1 batch, got {items:#?}"
    );
    assert!(
        !items
            .iter()
            .any(|item| item.starts_with("MeerkatMachine::")),
        "MeerkatMachine drift should be resolved by the catalog-owned Phase 1 lift, got {items:#?}"
    );
    assert!(
        !items.iter().any(|item| item.starts_with("MobMachine::")),
        "MobMachine drift should be resolved by the catalog-owned Phase 1 lift, got {items:#?}"
    );
    assert!(
        items.is_empty(),
        "Phase 1 schema parity inventory should be empty after convergence, got {items:#?}"
    );
}

#[test]
fn phase1_schema_parity_inventory_pins_remaining_drift_counts() {
    let counts = phase1_schema_drift_item_counts();
    let expected = BTreeMap::new();

    assert_eq!(
        counts, expected,
        "Phase 1 carry-forward drift must stay empty; update the parity ledger before reopening it"
    );
}

#[test]
fn mob_flow_projection_kernels_are_audited_as_non_canonical_support() {
    let canonical_machine_names = canonical_machine_schemas()
        .into_iter()
        .map(|schema| schema.machine.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mob_schema = dsl_mob_machine();
    let mob_inputs = mob_schema
        .inputs
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect::<BTreeSet<_>>();
    let audit = meerkat_mob::run::flow_projection_kernel_audit();

    assert_eq!(
        audit.iter().map(|entry| entry.module).collect::<Vec<_>>(),
        vec!["flow_run", "flow_frame", "loop_iteration"],
        "the live flow projection kernel audit must name every carried-forward helper"
    );

    for entry in audit {
        assert_eq!(
            entry.canonical_owner, "MobMachine",
            "{} must remain owned by MobMachine",
            entry.module
        );
        assert_eq!(
            entry.role,
            meerkat_mob::run::FlowProjectionKernelRole::MobMachineOwnedFailClosedProjection,
            "{} must be classified as MobMachine-owned fail-closed support, not as a production DSL",
            entry.module
        );
        assert!(
            !entry.canonical_machine,
            "{} must not claim canonical-machine status",
            entry.module
        );
        assert!(
            !canonical_machine_names.contains(entry.module),
            "{} must not be registered as a canonical machine",
            entry.module
        );
        for owning_input in entry.owning_inputs {
            let owning_input = *owning_input;
            assert!(
                mob_inputs.contains(owning_input),
                "{} owning input `{owning_input}` must exist on canonical MobMachine",
                entry.module
            );
        }
    }
}

#[test]
fn mob_runtime_parity_probe_inventory_is_closed_world() {
    let schema = dsl_mob_machine();
    let mismatches = mob_runtime_parity_probe_inventory_mismatches(&schema);
    assert!(
        mismatches.is_empty(),
        "Mob runtime parity probes must cover every non-surface MobMachine input or declare explicit carry-forward debt, got {mismatches:#?}"
    );
}

#[test]
fn mob_runtime_parity_probe_inventory_rejects_unprobed_catalog_inputs() {
    let mut schema = dsl_mob_machine();
    schema.inputs.variants.push(VariantSchema {
        name: EnumVariantId::parse("NewRuntimeInput").expect("variant id"),
        fields: vec![],
    });

    assert_eq!(
        mob_runtime_parity_probe_inventory_mismatches(&schema),
        vec!["catalog_input_unprobed.NewRuntimeInput"],
        "adding a MobMachine input without a runtime probe or explicit carry-forward entry must hard-fail"
    );
}

#[test]
#[ignore = "diagnostic: prints itemized catalog/production drift for parity-ledger classification"]
fn print_phase1_schema_drift_items() {
    for item in phase1_schema_drift_item_report() {
        println!("{item}");
    }
}

#[test]
fn schema_parity_gate_rejects_production_only_non_input_drift() {
    let catalog_schema = dsl_meerkat_machine();
    let mut production_schema = catalog_schema.clone();
    production_schema.effects.variants.push(VariantSchema {
        name: EnumVariantId::parse("ProductionOnlyEffectDrift").expect("variant id"),
        fields: vec![],
    });

    assert_eq!(
        schema_shape_mismatches_for_schemas(&catalog_schema, &production_schema),
        vec!["effects"],
        "full schema parity comparator must reject production-only non-input drift"
    );
    assert_eq!(
        input_alphabet(&catalog_schema).expect("catalog alphabet"),
        input_alphabet(&production_schema).expect("production alphabet"),
        "the old input-only gate would miss this effect drift"
    );
}

#[test]
fn schema_parity_gate_rejects_named_type_metadata_drift() {
    let catalog_schema = dsl_meerkat_machine();
    let mut production_schema = catalog_schema.clone();
    production_schema.named_types.pop();

    assert_eq!(
        schema_shape_mismatches_for_schemas(&catalog_schema, &production_schema),
        vec!["named types"],
        "full schema parity comparator must reject missing named-type bindings"
    );
    assert_eq!(
        input_alphabet(&catalog_schema).expect("catalog alphabet"),
        input_alphabet(&production_schema).expect("production alphabet"),
        "the old input-only gate would miss named-type metadata drift"
    );
}

#[test]
fn schema_parity_gate_rejects_runtime_internal_input_metadata_drift() {
    let catalog_schema = dsl_meerkat_machine();
    let mut production_schema = catalog_schema.clone();
    production_schema.runtime_internal_inputs.pop();

    assert_eq!(
        schema_shape_mismatches_for_schemas(&catalog_schema, &production_schema),
        vec!["runtime-internal input metadata"],
        "full schema parity comparator must reject missing runtime-internal input metadata"
    );
    assert_eq!(
        input_alphabet(&catalog_schema).expect("catalog alphabet"),
        input_alphabet(&production_schema).expect("production alphabet"),
        "the old input-only gate would miss runtime-internal input metadata drift"
    );
}

#[test]
fn production_schema_exports_include_metadata_without_catalog_schema_splicing() {
    for case in phase1_schema_parity_cases() {
        let catalog = (case.catalog_schema)();
        let production = (case.production_schema)();
        assert_eq!(
            production.named_types, catalog.named_types,
            "{} production export must carry named-type metadata before parity comparison",
            case.machine
        );
        assert_eq!(
            production.runtime_internal_inputs, catalog.runtime_internal_inputs,
            "{} production export must carry runtime-internal input metadata before parity comparison",
            case.machine
        );
    }
}

#[test]
#[ignore = "phase-0 failure fixture: old input gate misses non-input drift"]
fn input_only_parity_misses_production_only_effect_drift() -> Result<(), IdentityError> {
    let catalog_schema = dsl_meerkat_machine();
    let mut production_schema = catalog_schema.clone();
    production_schema.effects.variants.push(VariantSchema {
        name: EnumVariantId::parse("ProductionOnlyEffectDrift")?,
        fields: vec![],
    });

    assert_eq!(
        input_alphabet(&catalog_schema)?,
        input_alphabet(&production_schema)?,
        "an input-only parity gate would pass even though the production schema drifted"
    );
    assert!(
        catalog_schema != production_schema,
        "full schema parity must detect non-input drift"
    );

    Ok(())
}
