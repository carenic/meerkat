#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_closure_for_method_calls
)]

use meerkat_machine_schema::TriggerKind;
use meerkat_machine_schema::catalog::dsl::{
    dsl_meerkat_machine as meerkat_machine, dsl_mob_machine as mob_machine,
};
use meerkat_machine_schema::identity::{IdentityError, InputVariantId, SignalVariantId};
use meerkat_mob::{
    MobMachineCatalogInput as MobInput, MobMachineCommandClassification,
    MobMachineCommandClassificationRecord, canonical_mob_machine_command_classifications,
};
use meerkat_runtime::{
    MeerkatMachineCatalogInput as MeerkatInput, MeerkatMachineCommandClassification,
    MeerkatMachineCommandClassificationRecord, canonical_meerkat_machine_command_classifications,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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

fn parse_input_name(input: &'static str) -> InputVariantId {
    InputVariantId::parse(input).expect("catalog input names must be valid input identifiers")
}

fn assert_meerkat_command_records_are_identity_checked(
    schema_inputs: &BTreeSet<InputVariantId>,
    records: &[MeerkatMachineCommandClassificationRecord],
) {
    for record in records {
        let command_input = InputVariantId::parse(record.command).ok();
        let command_is_catalog_input = command_input
            .as_ref()
            .is_some_and(|input| schema_inputs.contains(input));
        let catalog_inputs = record.classification.catalog_input_names();
        let catalog_input_ids = catalog_inputs
            .iter()
            .copied()
            .map(parse_input_name)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            catalog_input_ids.len(),
            catalog_inputs.len(),
            "MeerkatMachine command `{}` must not duplicate catalog input classifications",
            record.command
        );
        assert!(
            catalog_input_ids.is_subset(schema_inputs),
            "MeerkatMachine command `{}` classifies to inputs absent from the schema: {:?}",
            record.command,
            catalog_input_ids
                .difference(schema_inputs)
                .collect::<Vec<_>>()
        );

        match record.classification {
            MeerkatMachineCommandClassification::CatalogInput(input) => {
                if command_is_catalog_input {
                    assert_eq!(
                        record.command,
                        input.as_str(),
                        "MeerkatMachine command `{}` is itself a catalog input and must classify to that exact typed input",
                        record.command
                    );
                }
            }
            MeerkatMachineCommandClassification::CatalogInputs(inputs) => {
                assert!(
                    !inputs.is_empty(),
                    "MeerkatMachine command `{}` must not use an empty typed catalog classification",
                    record.command
                );
                if let Some(command_input) =
                    command_input.filter(|input| schema_inputs.contains(input))
                {
                    assert!(
                        catalog_input_ids.contains(&command_input),
                        "MeerkatMachine command `{}` is itself a catalog input and must include that exact typed input",
                        record.command
                    );
                }
            }
            MeerkatMachineCommandClassification::ShellMechanic(_) => {
                assert!(
                    !command_is_catalog_input,
                    "MeerkatMachine shell-mechanic command `{}` must not bypass a catalog input",
                    record.command
                );
            }
        }
    }
}

fn assert_mob_command_records_are_identity_checked(
    schema_inputs: &BTreeSet<InputVariantId>,
    records: &[MobMachineCommandClassificationRecord],
) {
    for record in records {
        let command_input = InputVariantId::parse(record.command).ok();
        let command_is_catalog_input = command_input
            .as_ref()
            .is_some_and(|input| schema_inputs.contains(input));
        let catalog_inputs = record.classification.catalog_input_names();
        let catalog_input_ids = catalog_inputs
            .iter()
            .copied()
            .map(parse_input_name)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            catalog_input_ids.len(),
            catalog_inputs.len(),
            "MobMachine command `{}` must not duplicate catalog input classifications",
            record.command
        );
        assert!(
            catalog_input_ids.is_subset(schema_inputs),
            "MobMachine command `{}` classifies to inputs absent from the schema: {:?}",
            record.command,
            catalog_input_ids
                .difference(schema_inputs)
                .collect::<Vec<_>>()
        );

        match record.classification {
            MobMachineCommandClassification::CatalogInput(input) => {
                if command_is_catalog_input {
                    assert_eq!(
                        record.command,
                        input.as_str(),
                        "MobMachine command `{}` is itself a catalog input and must classify to that exact typed input",
                        record.command
                    );
                }
            }
            MobMachineCommandClassification::CatalogInputs(inputs) => {
                assert!(
                    !inputs.is_empty(),
                    "MobMachine command `{}` must not use an empty typed catalog classification",
                    record.command
                );
                if let Some(command_input) =
                    command_input.filter(|input| schema_inputs.contains(input))
                {
                    assert!(
                        catalog_input_ids.contains(&command_input),
                        "MobMachine command `{}` is itself a catalog input and must include that exact typed input",
                        record.command
                    );
                }
            }
            MobMachineCommandClassification::ShellMechanic(_) => {
                assert!(
                    !command_is_catalog_input,
                    "MobMachine shell-mechanic command `{}` must not bypass a catalog input",
                    record.command
                );
            }
        }
    }
}

fn assert_all_mob_catalog_inputs_are_identity_checked(schema_inputs: &BTreeSet<InputVariantId>) {
    let all_inputs = MobInput::ALL
        .iter()
        .copied()
        .map(MobInput::as_str)
        .map(parse_input_name)
        .collect::<BTreeSet<_>>();

    assert_eq!(
        all_inputs, *schema_inputs,
        "MobMachineCatalogInput::ALL must exactly mirror the catalog schema input alphabet"
    );

    for input in MobInput::ALL {
        let input_id = parse_input_name(input.as_str());
        assert!(
            schema_inputs.contains(&input_id),
            "typed MobMachine catalog input {input:?} must serialize to its exact schema input name"
        );
    }
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

fn assert_command_mapping_is_identity_or_pinned(
    machine: &str,
    records: impl IntoIterator<Item = (&'static str, Vec<InputVariantId>)>,
    pinned_non_identity: BTreeMap<&'static str, Vec<InputVariantId>>,
) {
    for (command, inputs) in records {
        if inputs.is_empty() {
            continue;
        }
        if let Some(expected) = pinned_non_identity.get(command) {
            assert_eq!(
                &inputs, expected,
                "{machine} command `{command}` changed its typed non-identity catalog mapping"
            );
        } else {
            assert_eq!(
                inputs,
                vec![parse_input_name(command)],
                "{machine} command `{command}` must map to the catalog input with the same name \
                 unless this semantic exception is explicitly pinned"
            );
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn function_body(source: &str, start_marker: &str, _end_marker: &str) -> String {
    let start = source
        .find(start_marker)
        .unwrap_or_else(|| panic!("missing start marker `{start_marker}`"));
    let rest = &source[start..];
    let body_start = rest
        .find('{')
        .unwrap_or_else(|| panic!("missing classifier body for `{start_marker}`"));
    let mut depth = 0usize;
    for (offset, ch) in rest[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth
                    .checked_sub(1)
                    .unwrap_or_else(|| panic!("brace underflow in `{start_marker}`"));
                if depth == 0 {
                    return rest[body_start..body_start + offset + ch.len_utf8()].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("missing classifier body end for `{start_marker}`")
}

fn assert_classifier_body_uses_typed_variants(path: &str, start_marker: &str, end_marker: &str) {
    let source = std::fs::read_to_string(repo_root().join(path)).expect("read classifier source");
    let body = function_body(&source, start_marker, end_marker);
    assert!(
        body.contains("match variant"),
        "{path} command classifier must dispatch on typed command variants"
    );
    assert!(
        !body.contains(".as_str()"),
        "{path} command classifier must not classify by stringified command names"
    );
    assert!(
        !body.contains("=> _") && !body.contains("_ =>"),
        "{path} command classifier must enumerate variants without wildcard fallback"
    );
    assert!(
        !body.contains('"'),
        "{path} command classifier must not contain string-literal command/input whitelists"
    );
}

#[test]
fn meerkat_machine_inputs_equal_runtime_manifest_exactly() -> Result<(), IdentityError> {
    let schema = meerkat_machine();
    let schema_inputs = variant_ids(&schema.inputs.variants)?;
    let dsl_internal_inputs: BTreeSet<InputVariantId> =
        schema.runtime_internal_inputs.iter().cloned().collect();
    let records = canonical_meerkat_machine_command_classifications();
    assert_meerkat_command_records_are_identity_checked(&schema_inputs, &records);
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
    assert_mob_command_records_are_identity_checked(&schema_inputs, &records);
    assert_all_mob_catalog_inputs_are_identity_checked(&schema_inputs);
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
fn meerkat_machine_command_to_input_mappings_are_typed_and_pinned() {
    let records = canonical_meerkat_machine_command_classifications();
    let pinned = BTreeMap::from([
        (
            "ConfigureModelRoutingBaseline",
            vec![parse_input_name(
                MeerkatInput::SetModelRoutingBaseline.as_str(),
            )],
        ),
        (
            "RequestSwitchTurn",
            vec![
                parse_input_name(MeerkatInput::RequestFiniteSwitchTurn.as_str()),
                parse_input_name(MeerkatInput::RequestUntilChangedSwitchTurn.as_str()),
            ],
        ),
        (
            "RuntimeRealtimeChannelStatus",
            vec![parse_input_name(
                MeerkatInput::RuntimeRealtimeAttachmentStatus.as_str(),
            )],
        ),
        (
            "SessionModelRoutingStatus",
            vec![parse_input_name(MeerkatInput::ModelRoutingStatus.as_str())],
        ),
    ]);

    assert_command_mapping_is_identity_or_pinned(
        "MeerkatMachine",
        records.iter().map(|record| {
            (
                record.command,
                record
                    .classification
                    .catalog_input_names()
                    .into_iter()
                    .map(parse_input_name)
                    .collect::<Vec<_>>(),
            )
        }),
        pinned,
    );
}

#[test]
fn mob_machine_command_to_input_mappings_are_typed_and_pinned() {
    let records = canonical_mob_machine_command_classifications();
    let pinned = BTreeMap::from([
        (
            "Wire",
            vec![
                parse_input_name(MobInput::WireMembers.as_str()),
                parse_input_name(MobInput::WireExternalPeer.as_str()),
            ],
        ),
        (
            "Unwire",
            vec![
                parse_input_name(MobInput::UnwireMembers.as_str()),
                parse_input_name(MobInput::UnwireExternalPeer.as_str()),
            ],
        ),
    ]);

    assert_command_mapping_is_identity_or_pinned(
        "MobMachine",
        records.iter().map(|record| {
            (
                record.command,
                record
                    .classification
                    .catalog_input_names()
                    .into_iter()
                    .map(parse_input_name)
                    .collect::<Vec<_>>(),
            )
        }),
        pinned,
    );
}

#[test]
fn command_classifiers_do_not_use_string_whitelists_or_wildcards() {
    assert_classifier_body_uses_typed_variants(
        "meerkat-runtime/src/meerkat_machine_types.rs",
        "const fn meerkat_machine_command_classification",
        "/// Snapshot of completion waiters",
    );
    assert_classifier_body_uses_typed_variants(
        "meerkat-mob/src/mob_machine.rs",
        "const fn mob_machine_command_classification",
        "",
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
