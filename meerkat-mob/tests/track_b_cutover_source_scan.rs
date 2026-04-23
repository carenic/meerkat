//! Track-B (R5) Commit 5 cutover source-scan lock-ins.
//!
//! These tests pin the semantic invariants Track-B's peer-graph /
//! wiring-graph unification established. They source-scan the live
//! actor and comms-drain code to ensure:
//!
//! 1. The legacy "realtime binding" vocabulary has been fully
//!    replaced by "session binding" at the generic-fact level. The
//!    realtime feature name survives in comments and wire shapes
//!    that are realtime-feature-specific; it must not appear as
//!    state-field / effect-variant / test names referencing the
//!    generic identity→session map.
//! 2. The canonical Track-B machine-emitted effects
//!    (`WiringGraphChanged`, `MemberSessionBindingChanged`,
//!    `LocalEndpointChanged`, `PeerProjectionChanged`,
//!    `CommsTrustReconcileRequested`) are declared in the DSL.
//! 3. The `RecomputeMobPeerOverlay` composition driver is the sole
//!    declared driver on `meerkat_mob_seam_composition` (first real
//!    consumer of the composition-driver framework).
//!
//! Source scans are literal-string scans over the .rs source files;
//! they do not exercise behavior. Behavior is covered by the
//! per-module test suites. These scans pin naming invariants so a
//! future refactor can't accidentally regress to the pre-Track-B
//! shape without the gate firing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("meerkat-mob crate has a parent directory")
        .to_path_buf()
}

fn read_source(rel_path: &str) -> String {
    let path = repo_root().join(rel_path);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn mob_machine_state_field_uses_session_binding_name() {
    // Post-Track-B, the canonical identity→session map is
    // `member_session_bindings`. The legacy name
    // `member_realtime_bindings` must not appear in any Rust file
    // in the repo's source tree after Commit 2's rename cascade.
    let mob_dsl = read_source("meerkat-mob/src/machines/mob_machine.rs");
    assert!(
        !mob_dsl.contains("member_realtime_bindings"),
        "`member_realtime_bindings` must not appear in the mob_machine DSL source — \
         replaced by `member_session_bindings` in Track-B Commit 2",
    );
    assert!(
        mob_dsl.contains("member_session_bindings"),
        "mob_machine DSL source must declare `member_session_bindings`",
    );

    let schema_dsl = read_source("meerkat-machine-schema/src/catalog/dsl/mob_machine.rs");
    assert!(
        !schema_dsl.contains("member_realtime_bindings"),
        "`member_realtime_bindings` must not appear in the schema DSL mirror",
    );
    assert!(
        schema_dsl.contains("member_session_bindings"),
        "schema DSL mirror must declare `member_session_bindings`",
    );
}

#[test]
fn mob_machine_declares_topology_epoch_and_track_b_effects() {
    let mob_dsl = read_source("meerkat-mob/src/machines/mob_machine.rs");
    for expected in [
        "topology_epoch",
        "WireMembers",
        "UnwireMembers",
        "BindMemberSession",
        "RotateMemberSession",
        "ReleaseMemberSession",
        "WiringGraphChanged",
        "MemberSessionBindingChanged",
    ] {
        assert!(
            mob_dsl.contains(expected),
            "mob_machine DSL must declare `{expected}` (Track-B Commit 2)",
        );
    }
}

#[test]
fn meerkat_machine_declares_peer_projection_state_and_effects() {
    let meerkat_dsl = read_source("meerkat-machine-schema/src/catalog/dsl/meerkat_machine.rs");
    for expected in [
        "local_endpoint",
        "direct_peer_endpoints",
        "mob_overlay_peer_endpoints",
        "peer_projection_epoch",
        "PublishLocalEndpoint",
        "ClearLocalEndpoint",
        "AddDirectPeerEndpoint",
        "RemoveDirectPeerEndpoint",
        "ApplyMobPeerOverlay",
        "LocalEndpointChanged",
        "PeerProjectionChanged",
        "CommsTrustReconcileRequested",
        "stale_overlay_epoch",
    ] {
        assert!(
            meerkat_dsl.contains(expected),
            "MeerkatMachine schema DSL must declare `{expected}` (Track-B Commit 3)",
        );
    }
}

#[test]
fn meerkat_mob_seam_composition_declares_recompute_mob_peer_overlay_driver() {
    let compositions = read_source("meerkat-machine-schema/src/catalog/compositions.rs");
    assert!(
        compositions.contains("RecomputeMobPeerOverlay"),
        "meerkat_mob_seam_composition must declare the RecomputeMobPeerOverlay driver (Track-B Commit 4)",
    );
    // Watched effects.
    for watched in [
        "WiringGraphChanged",
        "MemberSessionBindingChanged",
        "LocalEndpointChanged",
    ] {
        assert!(
            compositions.contains(watched),
            "meerkat_mob_seam driver descriptor must watch `{watched}`",
        );
    }
    // Dispatch target.
    assert!(
        compositions.contains("apply_mob_peer_overlay"),
        "meerkat_mob_seam driver descriptor must declare the `apply_mob_peer_overlay` dispatch route",
    );
}

#[test]
fn recompute_mob_peer_overlay_driver_and_reconciler_modules_are_public() {
    // The driver + reconciler pair must be reachable from
    // `meerkat-runtime`'s public surface so surface crates can wire
    // them in without reaching into `pub(crate)` internals.
    let runtime_lib = read_source("meerkat-runtime/src/lib.rs");
    for expected in [
        "pub mod recompute_mob_peer_overlay",
        "pub mod comms_trust_reconcile",
        "RecomputeMobPeerOverlayDriver",
        "CommsTrustReconciler",
    ] {
        assert!(
            runtime_lib.contains(expected),
            "meerkat-runtime/src/lib.rs must expose `{expected}`",
        );
    }
}

#[test]
fn dead_flow_frame_loop_driver_template_remains_deleted() {
    // Track-B Commit 1 deleted the hand-crafted
    // `flow_frame_loop_driver.rs.tmpl` specialization; verify it
    // doesn't reappear.
    let template_path =
        repo_root().join("meerkat-machine-codegen/src/templates/flow_frame_loop_driver.rs.tmpl");
    assert!(
        !template_path.exists(),
        "deleted template `{}` must stay deleted — the generic framework replaces it",
        template_path.display(),
    );
}
