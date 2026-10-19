//! The supervisor bridge command manifest mirrors the `BridgeCommand` wire
//! enum.
//!
//! Workspace-only: it reads meerkat-contracts' wire source, which the
//! published package cannot reach, so the package leaves this file out
//! (ci/standalone-workspace-only.toml).

#![allow(clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use meerkat_runtime::SupervisorBridgeCommandKind;

/// Extract the variant identifiers of a wire enum from its source text.
///
/// The wire enums are `#[non_exhaustive]`, so a compile-time exhaustive match
/// is impossible outside `meerkat-contracts`; source parity is the available
/// structural truth (the `runtime_alphabet_parity` classifier-body checks and
/// `bridge_delivery_handler_does_not_wait_for_completion_inline` set the
/// precedent). Only payload-carrying variants (`Ident(...)`) are captured;
/// a future fieldless variant surfaces as a loud manifest mismatch.
fn parse_wire_enum_variant_names(source: &str, enum_marker: &str) -> BTreeSet<String> {
    let start = source
        .find(enum_marker)
        .unwrap_or_else(|| panic!("wire enum marker `{enum_marker}` not found"));
    let body_start = start + enum_marker.len();
    let mut depth = 1_usize;
    let mut body_end = None;
    for (offset, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    body_end = Some(body_start + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let body_end = body_end.unwrap_or_else(|| panic!("unterminated enum body for `{enum_marker}`"));
    source[body_start..body_end]
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with("//") || line.starts_with('#') {
                return None;
            }
            let ident: String = line
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric())
                .collect();
            if !ident
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
            {
                return None;
            }
            line[ident.len()..].starts_with('(').then_some(ident)
        })
        .collect()
}

/// Multi-host mobs plan §6.4: the supervisor bridge command manifest
/// (`SupervisorBridgeCommandKind`) is the closed typed completeness authority
/// over the `#[non_exhaustive]` `BridgeCommand` wire enum. A new wire command
/// cannot ship without a typed admission classification (route + realization)
/// in the member runtime's manifest.
#[test]
fn supervisor_bridge_command_manifest_matches_wire_enum() {
    let contracts_source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../meerkat-contracts/src/wire/supervisor_bridge.rs"),
    )
    .expect("read meerkat-contracts supervisor_bridge.rs");
    let wire_variants =
        parse_wire_enum_variant_names(&contracts_source, "pub enum BridgeCommand {");
    let manifest: BTreeSet<String> = SupervisorBridgeCommandKind::ALL
        .iter()
        .map(|kind| kind.variant_name().to_string())
        .collect();
    assert_eq!(
        wire_variants, manifest,
        "SupervisorBridgeCommandKind must exactly mirror the BridgeCommand wire enum: \
         classify every new wire command (admission route + realization) in \
         meerkat_machine_types.rs before shipping it"
    );
}
