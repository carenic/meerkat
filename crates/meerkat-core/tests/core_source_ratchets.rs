//! Source ratchets over meerkat-core's own files, for shapes the type system
//! cannot pin. They read this package's sources, so they also run from the
//! published package.

#![allow(clippy::expect_used)]

use std::fs;
use std::path::Path;

fn read_core_source(relative: &str) -> Result<String, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest_dir.join(relative))
        .map_err(|err| format!("read core source `{relative}`: {err}"))
}

fn peer_input_candidate_struct_body(source: &str) -> Result<&str, String> {
    let start = source
        .find("pub struct PeerInputCandidate")
        .ok_or_else(|| "PeerInputCandidate struct exists".to_string())?;
    let source = &source[start..];
    let end = source
        .find("\n}\n\n")
        .ok_or_else(|| "PeerInputCandidate struct body has closing brace".to_string())?;
    Ok(&source[..end])
}

#[test]
fn peer_input_candidate_does_not_duplicate_ingress_class_or_auth() -> Result<(), String> {
    let source = read_core_source("src/interaction.rs")?;
    let body = peer_input_candidate_struct_body(&source)?;
    for forbidden in ["pub class:", "pub auth:"] {
        assert!(
            !body.contains(forbidden),
            "PeerInputCandidate must derive class/auth from PeerIngressFact, not carry duplicate field: {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn public_agentfactory_value_is_not_core_policy_authority() {
    let builder =
        read_core_source("src/agent/builder.rs").expect("read core AgentBuilder implementation");

    assert!(
        builder.contains("validate_factory_policy()?")
            && !builder.contains("std::panic::Location::caller")
            && !builder.contains("validate_factory_policy_caller")
            && !builder.contains("canonical_factory_source_path")
            && !builder.contains("pub struct AgentFactoryPolicyAuthority")
            && !builder.contains("pub fn from_registered_source<A: 'static>")
            && !builder.contains("pub fn agent_factory_policy_authority"),
        "core factory-policy build must validate factory-composed policy state \
         without trusting runtime source paths or exposing a public core \
         authority type or minting helper"
    );
}
