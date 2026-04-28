#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;

use meerkat_machine_kernels::generated::meerkat;
use meerkat_machine_kernels::test_oracle::{GeneratedMachineKernel, KernelSignal};
use meerkat_machine_schema::identity::{PhaseId, SignalVariantId};

fn signal(slug: &str) -> SignalVariantId {
    SignalVariantId::parse(slug).expect("signal id")
}

fn phase(slug: &str) -> PhaseId {
    PhaseId::parse(slug).expect("phase id")
}

#[test]
fn peer_directory_reachability_kernel_initializes_with_typed_signal() {
    let kernel = GeneratedMachineKernel::new(meerkat::schema());
    let state = kernel.initial_state().expect("initial state");
    let initialized = kernel
        .transition_signal(
            &state,
            &KernelSignal {
                variant: signal("Initialize"),
                fields: BTreeMap::new(),
            },
        )
        .expect("initialize")
        .next_state;

    assert_ne!(initialized.phase, phase("Initializing"));
}

#[test]
fn peer_directory_reachability_kernel_fields_removed_from_state() {
    let state = meerkat::initial_state();
    let value = serde_json::to_value(&state).expect("serialize typed state");
    let object = value.as_object().expect("typed state serializes as object");

    assert!(
        !object.contains_key("resolved_peer_keys"),
        "resolved_peer_keys field should not exist"
    );
    assert!(
        !object.contains_key("peer_reachability"),
        "peer_reachability field should not exist"
    );
    assert!(
        !object.contains_key("peer_last_reason"),
        "peer_last_reason field should not exist"
    );
}
