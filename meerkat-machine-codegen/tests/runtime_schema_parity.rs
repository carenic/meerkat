#![allow(clippy::expect_used)]

use meerkat_machine_schema::catalog::dsl::{
    dsl_auth_machine_production_schema, dsl_meerkat_machine, dsl_meerkat_machine_production_schema,
    dsl_mob_machine, dsl_mob_machine_production_schema, dsl_occurrence_lifecycle_machine,
    dsl_schedule_lifecycle_machine,
};
use meerkat_machine_schema::identity::{EnumVariantId, IdentityError, InputVariantId};
use meerkat_machine_schema::{
    EffectDispositionRule, FieldSchema, MachineSchema, NamedTypeBinding, TransitionSchema,
    VariantSchema, canonical_machine_schemas,
};
use meerkat_mob::MobMachineCatalogInput;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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
    if catalog.rust != production.rust {
        mismatches.push("rust binding metadata".to_owned());
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
            catalog_schema: dsl_meerkat_machine_production_schema,
            production_schema: meerkat_runtime::machine_schema_exports::meerkat_machine_schema,
        },
        SchemaParityCase {
            machine: "AuthMachine",
            catalog_schema: dsl_auth_machine_production_schema,
            production_schema: meerkat_runtime::machine_schema_exports::auth_machine_schema,
        },
        SchemaParityCase {
            machine: "MobMachine",
            catalog_schema: dsl_mob_machine_production_schema,
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
        if let Some(production_item) = production.get(name)
            && *catalog_item != *production_item
        {
            out.push(format!("{label}.changed.{name}"));
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

const MOB_RUNTIME_PARITY_PROBED_INPUT_VARIANTS: &[MobMachineCatalogInput] = &[
    MobMachineCatalogInput::Spawn,
    MobMachineCatalogInput::SubmitWork,
    MobMachineCatalogInput::RunFlow,
    MobMachineCatalogInput::CancelFlow,
    MobMachineCatalogInput::Retire,
    MobMachineCatalogInput::Respawn,
    MobMachineCatalogInput::RetireAll,
    MobMachineCatalogInput::CancelWork,
    MobMachineCatalogInput::CancelAllWork,
    MobMachineCatalogInput::Stop,
    MobMachineCatalogInput::Resume,
    MobMachineCatalogInput::Complete,
    MobMachineCatalogInput::Reset,
    MobMachineCatalogInput::Destroy,
    MobMachineCatalogInput::TaskCreate,
    MobMachineCatalogInput::TaskUpdate,
    MobMachineCatalogInput::SubscribeAgentEvents,
    MobMachineCatalogInput::SubscribeAllAgentEvents,
    MobMachineCatalogInput::SubscribeMobEvents,
    MobMachineCatalogInput::RecordOperatorActionProvenance,
    MobMachineCatalogInput::SetSpawnPolicy,
    MobMachineCatalogInput::Shutdown,
    MobMachineCatalogInput::ForceCancel,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MobRuntimeParityCarryForwardReason {
    FlowProjectionKernel,
    MemberRuntimeBinding,
    SessionIngressDetachHandoff,
    StartupKickoffLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MobRuntimeParityCarryForwardInput {
    input: MobMachineCatalogInput,
    reason: MobRuntimeParityCarryForwardReason,
}

const fn carry_forward_input(
    input: MobMachineCatalogInput,
    reason: MobRuntimeParityCarryForwardReason,
) -> MobRuntimeParityCarryForwardInput {
    MobRuntimeParityCarryForwardInput { input, reason }
}

const FLOW_PROJECTION_MECHANIC: MobRuntimeParityCarryForwardReason =
    MobRuntimeParityCarryForwardReason::FlowProjectionKernel;
const MEMBER_RUNTIME_BINDING_MECHANIC: MobRuntimeParityCarryForwardReason =
    MobRuntimeParityCarryForwardReason::MemberRuntimeBinding;
const SESSION_INGRESS_DETACH_MECHANIC: MobRuntimeParityCarryForwardReason =
    MobRuntimeParityCarryForwardReason::SessionIngressDetachHandoff;
const STARTUP_KICKOFF_LIFECYCLE_MECHANIC: MobRuntimeParityCarryForwardReason =
    MobRuntimeParityCarryForwardReason::StartupKickoffLifecycle;

const MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS: &[MobRuntimeParityCarryForwardInput] = &[
    carry_forward_input(
        MobMachineCatalogInput::CreateRunSeed,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::CreateFrameSeed,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::CreateLoopSeed,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::RecordLoopBodyFrameCompleted,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::RecordLoopUntilConditionMet,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::RecordLoopUntilConditionFailed,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::AuthorizeFlowRunReducerCommand,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::AuthorizeFlowFrameReducerCommand,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::AuthorizeLoopIterationReducerCommand,
        FLOW_PROJECTION_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::BindMemberSession,
        MEMBER_RUNTIME_BINDING_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::RotateMemberSession,
        MEMBER_RUNTIME_BINDING_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::ReleaseMemberSession,
        MEMBER_RUNTIME_BINDING_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::SessionIngressDetachedForMobDestroy,
        SESSION_INGRESS_DETACH_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::SessionIngressDetachFailedForMobDestroy,
        SESSION_INGRESS_DETACH_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffMarkPending,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffMarkStarting,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::StartupMarkReady,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffResolveStarted,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffResolveCallbackPending,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffResolveFailed,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffResolveCancelled,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffCancelRequested,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
    carry_forward_input(
        MobMachineCatalogInput::KickoffClear,
        STARTUP_KICKOFF_LIFECYCLE_MECHANIC,
    ),
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
        .map(InputVariantId::as_str)
        .collect::<BTreeSet<_>>();
    let runtime_internal_inputs = schema
        .runtime_internal_inputs
        .iter()
        .map(InputVariantId::as_str)
        .collect::<BTreeSet<_>>();
    let probed = MOB_RUNTIME_PARITY_PROBED_INPUT_VARIANTS
        .iter()
        .map(|input| input.as_str())
        .collect::<BTreeSet<_>>();
    let production_runtime_path = meerkat_mob::canonical_mob_machine_command_manifest()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let carry_forward = MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS
        .iter()
        .map(|record| record.input.as_str())
        .collect::<BTreeSet<_>>();
    let mut mismatches = Vec::new();

    for input in &probed {
        if !catalog_inputs.contains(input) {
            mismatches.push(format!("runtime_probe_only.{input}"));
        }
    }
    for input in &production_runtime_path {
        if !catalog_inputs.contains(input) {
            mismatches.push(format!("production_runtime_path_only.{input}"));
        }
    }
    for input in &carry_forward {
        if !catalog_inputs.contains(input) {
            mismatches.push(format!("carry_forward_only.{input}"));
        }
        if !runtime_internal_inputs.contains(input) {
            mismatches.push(format!("carry_forward_not_runtime_internal.{input}"));
        }
        if production_runtime_path.contains(input) {
            mismatches.push(format!("carry_forward_has_production_runtime_path.{input}"));
        }
        if probed.contains(input) {
            mismatches.push(format!("carry_forward_has_runtime_probe.{input}"));
        }
    }
    for input in catalog_inputs.difference(&surface_only_inputs) {
        if !probed.contains(input)
            && !production_runtime_path.contains(input)
            && !carry_forward.contains(input)
        {
            mismatches.push(format!("catalog_input_unprobed.{input}"));
        }
    }

    mismatches
}

fn mob_runtime_parity_carry_forward_inputs_by_reason(
    reason: MobRuntimeParityCarryForwardReason,
) -> BTreeSet<&'static str> {
    MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS
        .iter()
        .filter_map(|record| (record.reason == reason).then_some(record.input.as_str()))
        .collect()
}

fn repo_root() -> PathBuf {
    if let Some(root) = std::env::var_os("WORKSPACE_ROOT") {
        return PathBuf::from(root);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn mob_catalog_input_names(
    inputs: impl IntoIterator<Item = MobMachineCatalogInput>,
) -> BTreeSet<&'static str> {
    inputs
        .into_iter()
        .map(MobMachineCatalogInput::as_str)
        .collect()
}

fn critical_mob_runtime_command_inputs() -> BTreeSet<&'static str> {
    mob_catalog_input_names([
        MobMachineCatalogInput::RunFlow,
        MobMachineCatalogInput::CancelFlow,
        MobMachineCatalogInput::TaskCreate,
        MobMachineCatalogInput::SetSpawnPolicy,
        MobMachineCatalogInput::ForceCancel,
    ])
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
            let owning_input = owning_input.as_str();
            assert!(
                mob_inputs.contains(owning_input),
                "{} owning input `{owning_input}` must exist on canonical MobMachine",
                entry.module
            );
        }
    }
}

#[test]
fn mob_runtime_parity_production_command_manifest_closes_command_backed_inputs() {
    let schema = dsl_mob_machine();
    let catalog_inputs = schema
        .inputs
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect::<BTreeSet<_>>();
    let production_runtime_path = meerkat_mob::canonical_mob_machine_command_manifest()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let carry_forward = MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS
        .iter()
        .map(|record| record.input.as_str())
        .collect::<BTreeSet<_>>();
    let command_backed_runtime_inputs = mob_catalog_input_names([
        MobMachineCatalogInput::RunFlow,
        MobMachineCatalogInput::CancelFlow,
        MobMachineCatalogInput::EnsureMember,
        MobMachineCatalogInput::Reconcile,
        MobMachineCatalogInput::WireMembers,
        MobMachineCatalogInput::UnwireMembers,
        MobMachineCatalogInput::WireExternalPeer,
        MobMachineCatalogInput::UnwireExternalPeer,
        MobMachineCatalogInput::TaskCreate,
        MobMachineCatalogInput::SetSpawnPolicy,
        MobMachineCatalogInput::ForceCancel,
    ]);

    assert!(
        production_runtime_path.is_subset(&catalog_inputs),
        "production mob command classifications must name only catalog inputs"
    );
    assert!(
        command_backed_runtime_inputs.is_subset(&production_runtime_path),
        "command-backed MobMachine inputs must be proved by the production command manifest"
    );
    assert!(
        command_backed_runtime_inputs.is_disjoint(&carry_forward),
        "command-backed MobMachine inputs must not remain in the carry-forward ledger"
    );
}

#[test]
fn mob_runtime_parity_critical_command_inputs_cannot_be_shell_mechanics() {
    let production_runtime_path = meerkat_mob::canonical_mob_machine_command_manifest()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let carry_forward = MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS
        .iter()
        .map(|record| record.input.as_str())
        .collect::<BTreeSet<_>>();
    let critical = critical_mob_runtime_command_inputs();

    assert!(
        critical.is_subset(&production_runtime_path),
        "critical runtime commands must be catalog-command backed, not hidden behind shell-mechanic carry-forward debt"
    );
    assert!(
        critical.is_disjoint(&carry_forward),
        "critical runtime commands must never be carried forward as typed shell mechanics"
    );
}

#[test]
fn mob_runtime_parity_carry_forward_inputs_have_typed_shell_mechanic_reasons() {
    let schema = dsl_mob_machine();
    let surface_only_inputs = schema
        .surface_only_inputs
        .iter()
        .map(InputVariantId::as_str)
        .collect::<BTreeSet<_>>();
    let runtime_internal_inputs = schema
        .runtime_internal_inputs
        .iter()
        .map(InputVariantId::as_str)
        .collect::<BTreeSet<_>>();
    let carry_forward = MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS
        .iter()
        .map(|record| record.input.as_str())
        .collect::<BTreeSet<_>>();
    let production_runtime_path = meerkat_mob::canonical_mob_machine_command_manifest()
        .into_iter()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        carry_forward.len(),
        MOB_RUNTIME_PARITY_CARRY_FORWARD_UNPROBED_INPUTS.len(),
        "carry-forward inputs must be unique"
    );
    assert!(
        carry_forward.is_subset(&runtime_internal_inputs),
        "carry-forward inputs must be runtime-internal shell mechanics, not public runtime paths"
    );
    assert!(
        carry_forward.is_disjoint(&surface_only_inputs),
        "surface-only inputs do not need carry-forward debt"
    );
    assert!(
        carry_forward.is_disjoint(&production_runtime_path),
        "inputs with production mob command coverage must not be carried forward"
    );

    let audited_flow_projection_inputs = meerkat_mob::run::flow_projection_kernel_audit()
        .iter()
        .flat_map(|entry| entry.owning_inputs.iter().copied())
        .map(MobMachineCatalogInput::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        mob_runtime_parity_carry_forward_inputs_by_reason(
            MobRuntimeParityCarryForwardReason::FlowProjectionKernel
        ),
        audited_flow_projection_inputs,
        "flow projection carry-forward must match the production flow projection audit"
    );
    assert_eq!(
        mob_runtime_parity_carry_forward_inputs_by_reason(
            MobRuntimeParityCarryForwardReason::MemberRuntimeBinding
        ),
        mob_catalog_input_names([
            MobMachineCatalogInput::BindMemberSession,
            MobMachineCatalogInput::RotateMemberSession,
            MobMachineCatalogInput::ReleaseMemberSession,
        ]),
        "member runtime-binding carry-forward must stay explicitly classified"
    );
    assert_eq!(
        mob_runtime_parity_carry_forward_inputs_by_reason(
            MobRuntimeParityCarryForwardReason::SessionIngressDetachHandoff
        ),
        mob_catalog_input_names([
            MobMachineCatalogInput::SessionIngressDetachedForMobDestroy,
            MobMachineCatalogInput::SessionIngressDetachFailedForMobDestroy,
        ]),
        "session ingress detach carry-forward must stay explicitly classified"
    );
    assert_eq!(
        mob_runtime_parity_carry_forward_inputs_by_reason(
            MobRuntimeParityCarryForwardReason::StartupKickoffLifecycle
        ),
        mob_catalog_input_names([
            MobMachineCatalogInput::KickoffMarkPending,
            MobMachineCatalogInput::KickoffMarkStarting,
            MobMachineCatalogInput::StartupMarkReady,
            MobMachineCatalogInput::KickoffResolveStarted,
            MobMachineCatalogInput::KickoffResolveCallbackPending,
            MobMachineCatalogInput::KickoffResolveFailed,
            MobMachineCatalogInput::KickoffResolveCancelled,
            MobMachineCatalogInput::KickoffCancelRequested,
            MobMachineCatalogInput::KickoffClear,
        ]),
        "startup kickoff carry-forward must stay explicitly classified"
    );
}

#[test]
fn mob_machine_native_reducer_helpers_are_formally_defined() {
    let model = std::fs::read_to_string(repo_root().join("specs/machines/mob_machine/model.tla"))
        .expect("read generated MobMachine TLA");
    for helper in [
        "mob_machine_node_terminal",
        "mob_machine_frame_node_status_after_admit",
        "mob_machine_frame_ready_queue_after_admit",
        "mob_machine_frame_node_status_after_terminal",
        "mob_machine_frame_ready_queue_after_terminal",
    ] {
        assert!(
            model.contains(&format!("{helper}(")),
            "MobMachine TLA must call or define native helper `{helper}`"
        );
        assert!(
            model
                .lines()
                .any(|line| line.starts_with(&format!("{helper}(")) && line.contains("==")),
            "native helper `{helper}` must have a generated TLA operator definition"
        );
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
fn schema_parity_gate_rejects_rust_binding_metadata_drift() {
    let catalog_schema = dsl_mob_machine();
    let mut production_schema = catalog_schema.clone();
    production_schema.rust.module = "wrong::module".to_owned();

    assert_eq!(
        schema_shape_mismatches_for_schemas(&catalog_schema, &production_schema),
        vec!["rust binding metadata"],
        "full schema parity comparator must reject production rust binding drift"
    );
    assert_eq!(
        input_alphabet(&catalog_schema).expect("catalog alphabet"),
        input_alphabet(&production_schema).expect("production alphabet"),
        "the old input-only gate would miss rust binding metadata drift"
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
