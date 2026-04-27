#![allow(clippy::redundant_closure_for_method_calls)]

use meerkat_machine_schema::TriggerKind;
use meerkat_machine_schema::catalog::dsl::{
    dsl_meerkat_machine as meerkat_machine, dsl_mob_machine as mob_machine,
};
use meerkat_machine_schema::identity::{IdentityError, InputVariantId, SignalVariantId};
use meerkat_mob::{
    MobMachineCommandClassification, MobMachineCommandClassificationRecord,
    canonical_mob_machine_command_classifications,
};
use meerkat_runtime::{
    MeerkatMachineCommandClassification, MeerkatMachineCommandClassificationRecord,
    canonical_meerkat_machine_command_classifications,
};
use std::collections::BTreeSet;
use std::path::Path;

fn variant_ids<'a>(
    variants: impl IntoIterator<Item = &'a meerkat_machine_schema::VariantSchema>,
) -> Result<BTreeSet<InputVariantId>, IdentityError> {
    variants
        .into_iter()
        .map(|variant| InputVariantId::parse(variant.name.as_str()))
        .collect()
}

fn command_ids(
    commands: impl IntoIterator<Item = &'static str>,
) -> Result<BTreeSet<InputVariantId>, IdentityError> {
    commands.into_iter().map(InputVariantId::parse).collect()
}

fn meerkat_runtime_command_ids(
    records: &[MeerkatMachineCommandClassificationRecord],
) -> Result<BTreeSet<InputVariantId>, IdentityError> {
    command_ids(
        records
            .iter()
            .flat_map(|record| record.classification.catalog_input_names()),
    )
}

fn mob_runtime_command_ids(
    records: &[MobMachineCommandClassificationRecord],
) -> Result<BTreeSet<InputVariantId>, IdentityError> {
    command_ids(
        records
            .iter()
            .flat_map(|record| record.classification.catalog_input_names()),
    )
}

fn assert_runtime_manifest_matches_schema(
    machine: &str,
    schema_inputs: &BTreeSet<InputVariantId>,
    dsl_internal_inputs: &BTreeSet<InputVariantId>,
    runtime_commands: &BTreeSet<InputVariantId>,
    shell_mechanics: &BTreeSet<InputVariantId>,
) {
    assert!(
        dsl_internal_inputs.is_subset(schema_inputs),
        "{machine} DSL-internal input declarations contain variants absent from the schema: {:?}",
        dsl_internal_inputs
            .difference(schema_inputs)
            .collect::<Vec<_>>()
    );

    assert!(
        runtime_commands.is_subset(schema_inputs),
        "{machine} runtime command manifest contains variants absent from the schema: {:?}",
        runtime_commands
            .difference(schema_inputs)
            .collect::<Vec<_>>()
    );
    assert!(
        shell_mechanics.is_disjoint(schema_inputs),
        "{machine} shell-mechanic command classifications must not mask schema inputs: {:?}",
        shell_mechanics
            .intersection(schema_inputs)
            .collect::<Vec<_>>()
    );
    assert!(
        shell_mechanics.is_disjoint(dsl_internal_inputs),
        "{machine} shell-mechanic command classifications must not mask DSL-internal inputs: {:?}",
        shell_mechanics
            .intersection(dsl_internal_inputs)
            .collect::<Vec<_>>()
    );

    let unexpected_schema_inputs = schema_inputs
        .difference(runtime_commands)
        .filter(|input| !dsl_internal_inputs.contains(input))
        .collect::<Vec<_>>();
    assert!(
        unexpected_schema_inputs.is_empty(),
        "{machine} schema inputs are missing from the runtime command manifest and are not declared DSL-internal inputs: {unexpected_schema_inputs:?}"
    );
}

fn classifier_function_body(source_path: &Path, function_name: &str) -> String {
    let source = std::fs::read_to_string(source_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", source_path.display()));
    let function_marker = format!("fn {function_name}");
    let function_start = source
        .find(&function_marker)
        .unwrap_or_else(|| panic!("missing classifier function `{function_name}`"));
    let function_tail = &source[function_start..];
    let body_start = function_tail
        .find('{')
        .unwrap_or_else(|| panic!("could not locate body of `{function_name}`"));
    let mut depth = 0usize;
    let mut body_end = None;
    for (offset, ch) in function_tail[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth
                    .checked_sub(1)
                    .unwrap_or_else(|| panic!("brace underflow in `{function_name}`"));
                if depth == 0 {
                    body_end = Some(body_start + offset + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let body_end = body_end.unwrap_or_else(|| panic!("could not locate end of `{function_name}`"));
    function_tail[body_start..body_end].to_owned()
}

fn catalog_input_enum_variants(source: &str, enum_name: &str) -> BTreeSet<String> {
    let enum_marker = format!("enum {enum_name}");
    let enum_start = source
        .find(&enum_marker)
        .unwrap_or_else(|| panic!("missing enum `{enum_name}`"));
    let enum_tail = &source[enum_start..];
    let body_start = enum_tail
        .find('{')
        .unwrap_or_else(|| panic!("could not locate body of enum `{enum_name}`"));
    let body_end = enum_tail[body_start..]
        .find('}')
        .unwrap_or_else(|| panic!("could not locate end of enum `{enum_name}`"));
    enum_tail[body_start + 1..body_start + body_end]
        .lines()
        .filter_map(|line| line.trim().strip_suffix(','))
        .filter(|line| !line.is_empty() && !line.starts_with("#["))
        .map(ToOwned::to_owned)
        .collect()
}

fn catalog_input_as_str_arms(source: &str, enum_name: &str) -> BTreeSet<(String, String)> {
    let impl_marker = format!("impl {enum_name}");
    let impl_start = source
        .find(&impl_marker)
        .unwrap_or_else(|| panic!("missing impl `{enum_name}`"));
    let impl_tail = &source[impl_start..];
    let function_start = impl_tail
        .find("fn as_str")
        .unwrap_or_else(|| panic!("missing `{enum_name}::as_str`"));
    impl_tail[function_start..]
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let arm = trimmed.strip_prefix("Self::")?;
            let (variant, literal_tail) = arm.split_once("=>")?;
            let literal = literal_tail
                .trim()
                .strip_prefix('"')?
                .split_once('"')?
                .0
                .to_owned();
            Some((variant.trim().to_owned(), literal))
        })
        .collect()
}

fn assert_catalog_input_enum_is_name_identity(source_path: &Path, enum_name: &str) {
    let source = std::fs::read_to_string(source_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", source_path.display()));
    let variants = catalog_input_enum_variants(&source, enum_name);
    let arms = catalog_input_as_str_arms(&source, enum_name);
    let arm_variants = arms
        .iter()
        .map(|(variant, _)| variant.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        variants, arm_variants,
        "{enum_name}::as_str must cover exactly the typed catalog input variants"
    );
    let non_identity = arms
        .into_iter()
        .filter(|(variant, literal)| variant != literal)
        .collect::<Vec<_>>();
    assert!(
        non_identity.is_empty(),
        "{enum_name}::as_str must be an identity projection to the catalog variant name, got {non_identity:?}"
    );
}

fn assert_classifier_has_no_catalog_input_wildcard(source_path: &Path, function_name: &str) {
    let function_body = classifier_function_body(source_path, function_name);
    assert!(
        !function_body.contains("_ =>") && !function_body.contains(".. =>"),
        "{function_name} must classify every command variant explicitly; a wildcard/default CatalogInput arm would make new commands bypass the #38 typed parity gate"
    );
}

fn assert_classifier_has_no_raw_catalog_input_mapping(source_path: &Path, function_name: &str) {
    let function_body = classifier_function_body(source_path, function_name);
    assert!(
        !function_body.contains("CatalogInput(\"")
            && !function_body.contains("CatalogInputs(&[\"")
            && !function_body.contains("variant.as_str()"),
        "{function_name} must map command variants to typed catalog input variants, not raw strings or command-name stringification"
    );
}

fn assert_classifier_uses_typed_catalog_inputs_and_shell_reasons(
    source_path: &Path,
    function_name: &str,
    catalog_input_type: &str,
    shell_reason_type: &str,
) {
    let function_body = classifier_function_body(source_path, function_name);
    let compact_body = function_body.split_whitespace().collect::<String>();
    assert!(
        compact_body.contains(&format!("CatalogInput({catalog_input_type}::"))
            || compact_body.contains(&format!("CatalogInputs(&[{catalog_input_type}::")),
        "{function_name} should positively classify semantic commands with typed {catalog_input_type} variants"
    );
    assert!(
        compact_body.contains(&format!("ShellMechanic({shell_reason_type}::")),
        "{function_name} should classify shell-only commands with typed {shell_reason_type} variants"
    );
}

#[test]
fn meerkat_machine_inputs_equal_runtime_manifest_exactly() -> Result<(), IdentityError> {
    let schema = meerkat_machine();
    let schema_inputs = variant_ids(&schema.inputs.variants)?;
    let dsl_internal_inputs: BTreeSet<InputVariantId> =
        schema.runtime_internal_inputs.iter().cloned().collect();
    let records = canonical_meerkat_machine_command_classifications();
    let runtime_commands = meerkat_runtime_command_ids(&records)?;
    let shell_mechanics = command_ids(records.iter().filter_map(|record| {
        matches!(
            record.classification,
            MeerkatMachineCommandClassification::ShellMechanic(_)
        )
        .then_some(record.command)
    }))?;

    assert_runtime_manifest_matches_schema(
        "MeerkatMachine",
        &schema_inputs,
        &dsl_internal_inputs,
        &runtime_commands,
        &shell_mechanics,
    );
    Ok(())
}

#[test]
fn mob_machine_inputs_equal_runtime_manifest_exactly() -> Result<(), IdentityError> {
    let schema = mob_machine();
    let schema_inputs = variant_ids(&schema.inputs.variants)?;
    let dsl_internal_inputs: BTreeSet<InputVariantId> =
        schema.runtime_internal_inputs.iter().cloned().collect();
    let records = canonical_mob_machine_command_classifications();
    let runtime_commands = mob_runtime_command_ids(&records)?;
    let shell_mechanics = command_ids(records.iter().filter_map(|record| {
        matches!(
            record.classification,
            MobMachineCommandClassification::ShellMechanic(_)
        )
        .then_some(record.command)
    }))?;

    assert_runtime_manifest_matches_schema(
        "MobMachine",
        &schema_inputs,
        &dsl_internal_inputs,
        &runtime_commands,
        &shell_mechanics,
    );
    Ok(())
}

#[test]
fn runtime_command_classifiers_are_exhaustive_not_defaulted() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has workspace parent");
    assert_classifier_has_no_catalog_input_wildcard(
        &workspace
            .join("meerkat-runtime")
            .join("src")
            .join("meerkat_machine_types.rs"),
        "meerkat_machine_command_classification",
    );
    assert_classifier_has_no_raw_catalog_input_mapping(
        &workspace
            .join("meerkat-runtime")
            .join("src")
            .join("meerkat_machine_types.rs"),
        "meerkat_machine_command_classification",
    );
    assert_classifier_uses_typed_catalog_inputs_and_shell_reasons(
        &workspace
            .join("meerkat-runtime")
            .join("src")
            .join("meerkat_machine_types.rs"),
        "meerkat_machine_command_classification",
        "MeerkatMachineCatalogInput",
        "MeerkatMachineShellMechanicReason",
    );
    assert_classifier_has_no_catalog_input_wildcard(
        &workspace
            .join("meerkat-mob")
            .join("src")
            .join("mob_machine.rs"),
        "mob_machine_command_classification",
    );
    assert_classifier_has_no_raw_catalog_input_mapping(
        &workspace
            .join("meerkat-mob")
            .join("src")
            .join("mob_machine.rs"),
        "mob_machine_command_classification",
    );
    assert_classifier_uses_typed_catalog_inputs_and_shell_reasons(
        &workspace
            .join("meerkat-mob")
            .join("src")
            .join("mob_machine.rs"),
        "mob_machine_command_classification",
        "MobMachineCatalogInput",
        "MobMachineShellMechanicReason",
    );
}

#[test]
fn typed_catalog_input_enums_cannot_remap_to_other_schema_strings() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has workspace parent");
    assert_catalog_input_enum_is_name_identity(
        &workspace
            .join("meerkat-runtime")
            .join("src")
            .join("meerkat_machine_types.rs"),
        "MeerkatMachineCatalogInput",
    );
    assert_catalog_input_enum_is_name_identity(
        &workspace
            .join("meerkat-mob")
            .join("src")
            .join("mob_machine.rs"),
        "MobMachineCatalogInput",
    );
}

#[test]
fn every_canonical_input_has_transition_coverage() -> Result<(), IdentityError> {
    for schema in [meerkat_machine(), mob_machine()] {
        let surface_only_inputs: BTreeSet<InputVariantId> =
            schema.surface_only_inputs.iter().cloned().collect();
        let covered: BTreeSet<InputVariantId> = schema
            .transitions
            .iter()
            .filter(|transition| transition.on.kind() == TriggerKind::Input)
            .map(|transition| InputVariantId::parse(transition.on.variant_str()))
            .collect::<Result<_, _>>()?;

        for input in &schema.inputs.variants {
            let input_id = InputVariantId::parse(input.name.as_str())?;
            if surface_only_inputs.contains(&input_id) {
                continue;
            }
            assert!(
                covered.contains(&input_id),
                "{} input `{}` has no transition coverage",
                schema.machine,
                input.name
            );
        }
    }
    Ok(())
}

#[test]
fn every_canonical_signal_has_transition_coverage() -> Result<(), IdentityError> {
    for schema in [meerkat_machine(), mob_machine()] {
        let covered: BTreeSet<SignalVariantId> = schema
            .transitions
            .iter()
            .filter(|transition| transition.on.kind() == TriggerKind::Signal)
            .map(|transition| SignalVariantId::parse(transition.on.variant_str()))
            .collect::<Result<_, _>>()?;

        for signal in &schema.signals.variants {
            let signal_id = SignalVariantId::parse(signal.name.as_str())?;
            assert!(
                covered.contains(&signal_id),
                "{} signal `{}` has no transition coverage",
                schema.machine,
                signal.name
            );
        }
    }
    Ok(())
}
