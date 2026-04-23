#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, unused_imports)]

//! Tripwire for wave-c (Section 1.5 #3). Flipped green by **C-6r**
//! (meerkat-runtime retype — PeerEndpoint twin carries typed newtypes).
//!
//! Invariant: both `PeerEndpoint` copies (the runtime DSL at
//! `meerkat-runtime/src/meerkat_machine/dsl.rs` and the schema catalog
//! at `meerkat-machine-schema/src/catalog/dsl/meerkat_machine.rs`) must
//! carry typed fields — `PeerId`, `PeerAddress`, `PeerName` — not bare
//! `String`. The two copies are required to stay structurally
//! equivalent (per the comments in both files); if C-6r only retypes
//! one side, this tripwire catches the drift.
//!
//! Predicted failure today: both copies use `String` for
//! `name` / `peer_id` / `address`. This test asserts the files contain
//! the typed-field signatures and fails with a clear diff if they are
//! still `String`.
//!
//! We do the check at the source-text level because `meerkat-runtime`
//! is not a dependency of `meerkat-machine-schema` (and wiring one
//! just for this test would inject a large coupling). Text scan is
//! equivalent in intent: if the typed newtypes appear at the named
//! struct sites, the types are typed; if `: String` appears, they are
//! not.

use std::fs;
use std::path::{Path, PathBuf};

const RUNTIME_DSL: &str = "meerkat-runtime/src/meerkat_machine/dsl.rs";
const SCHEMA_CATALOG: &str = "meerkat-machine-schema/src/catalog/dsl/meerkat_machine.rs";

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for a test target is the crate root. Walk up
    // until we see a workspace-level `Cargo.toml` containing
    // `[workspace]`.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let toml = p.join("Cargo.toml");
        if toml.exists() {
            let text = fs::read_to_string(&toml).unwrap_or_default();
            if text.contains("[workspace]") {
                return p;
            }
        }
        assert!(p.pop(), "could not locate workspace root");
    }
}

fn read_peer_endpoint_block(root: &Path, relative: &str) -> String {
    let path = root.join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

fn assert_typed_peer_endpoint(label: &str, body: &str) {
    // Find the PeerEndpoint struct definition and read a generous
    // window around it.
    let Some(idx) = body.find("pub struct PeerEndpoint") else {
        panic!(
            "{label}: `pub struct PeerEndpoint` not found — expected \
             twin copies per Section 1.5 #3"
        );
    };
    let window_end = body
        .get(idx..)
        .and_then(|rest| rest.find("\n}\n").map(|e| idx + e + 3))
        .unwrap_or(body.len());
    let block = &body[idx..window_end];

    // Today (c.0): these assertions should FAIL because fields are
    // String. After C-6r: they must pass.
    let typed_fields = [
        ("name", "PeerName"),
        ("peer_id", "PeerId"),
        ("address", "PeerAddress"),
    ];

    let mut violations = Vec::new();
    for (field, ty) in typed_fields {
        // Look for `pub {field}: {ty}` — the typed form.
        let typed_needle = format!("pub {field}: {ty}");
        if !block.contains(&typed_needle) {
            violations.push(format!("expected `{typed_needle}` in {label}"));
        }
    }

    assert!(
        violations.is_empty(),
        "{label} PeerEndpoint is not fully typed (flipped green by C-6r). \
         Violations:\n  - {}\n\n---- block ----\n{block}\n---- end ----",
        violations.join("\n  - ")
    );
}

#[test]
fn peer_endpoint_runtime_and_schema_both_carry_typed_fields() {
    let root = workspace_root();
    let runtime_body = read_peer_endpoint_block(&root, RUNTIME_DSL);
    let schema_body = read_peer_endpoint_block(&root, SCHEMA_CATALOG);
    assert_typed_peer_endpoint("runtime DSL", &runtime_body);
    assert_typed_peer_endpoint("schema catalog", &schema_body);
}
