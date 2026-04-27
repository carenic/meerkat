#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use meerkat_machine_kernels::generated::meerkat;
use meerkat_machine_kernels::test_oracle::{
    GeneratedMachineKernel, KernelInput, KernelSignal, KernelState, KernelValue,
};
use meerkat_machine_schema::identity::{
    EffectVariantId, FieldId, InputVariantId, NamedTypeId, PhaseId, SignalVariantId,
};

fn field(slug: &str) -> FieldId {
    FieldId::parse(slug).expect("field id")
}

fn input(slug: &str) -> InputVariantId {
    InputVariantId::parse(slug).expect("input id")
}

fn signal(slug: &str) -> SignalVariantId {
    SignalVariantId::parse(slug).expect("signal id")
}

fn effect(slug: &str) -> EffectVariantId {
    EffectVariantId::parse(slug).expect("effect id")
}

fn phase(slug: &str) -> PhaseId {
    PhaseId::parse(slug).expect("phase id")
}

fn named_string(type_name: &str, value: &str) -> KernelValue {
    KernelValue::Named {
        type_name: NamedTypeId::parse(type_name).expect("type id"),
        value: Box::new(KernelValue::String(value.into())),
    }
}

fn named_u64(type_name: &str, value: u64) -> KernelValue {
    KernelValue::Named {
        type_name: NamedTypeId::parse(type_name).expect("type id"),
        value: Box::new(KernelValue::U64(value)),
    }
}

fn tool_filter_all() -> KernelValue {
    named_string(
        "ToolFilter",
        &serde_json::to_string(&meerkat_core::ToolFilter::All)
            .expect("tool filter should serialize"),
    )
}

fn witness(value: &str) -> KernelValue {
    named_string("ToolVisibilityWitness", value)
}

fn prepared_meerkat_state(kernel: &GeneratedMachineKernel) -> KernelState {
    let initialized = kernel
        .transition_signal(
            &kernel.initial_state().expect("initial state"),
            &KernelSignal {
                variant: signal("Initialize"),
                fields: BTreeMap::new(),
            },
        )
        .expect("initialize")
        .next_state;
    let registered = kernel
        .transition(
            &initialized,
            &KernelInput {
                variant: input("RegisterSession"),
                fields: BTreeMap::from([(
                    field("session_id"),
                    named_string("SessionId", "session-1"),
                )]),
            },
        )
        .expect("register session")
        .next_state;
    kernel
        .transition(
            &registered,
            &KernelInput {
                variant: input("PrepareBindings"),
                fields: BTreeMap::from([
                    (
                        field("agent_runtime_id"),
                        named_string("AgentRuntimeId", "runtime-7"),
                    ),
                    (field("fence_token"), named_u64("FenceToken", 3)),
                    (field("generation"), named_u64("Generation", 1)),
                    (field("session_id"), named_string("SessionId", "session-1")),
                ]),
            },
        )
        .expect("prepare bindings")
        .next_state
}

#[test]
fn session_tool_visibility_kernel_publishes_committed_set_from_attached() {
    let kernel = GeneratedMachineKernel::new(meerkat::schema());
    let attached = prepared_meerkat_state(&kernel);

    let published = kernel
        .transition(
            &attached,
            &KernelInput {
                variant: input("PublishCommittedVisibleSet"),
                fields: BTreeMap::from([
                    (field("active_filter"), tool_filter_all()),
                    (field("staged_filter"), tool_filter_all()),
                    (
                        field("active_requested_deferred_names"),
                        KernelValue::Set(BTreeSet::new()),
                    ),
                    (
                        field("staged_requested_deferred_names"),
                        KernelValue::Set(BTreeSet::new()),
                    ),
                    (field("active_visibility_revision"), KernelValue::U64(0)),
                    (field("staged_visibility_revision"), KernelValue::U64(0)),
                ]),
            },
        )
        .expect("publish committed visible set");

    assert_eq!(published.next_state.phase, phase("Attached"));
    assert_eq!(published.effects.len(), 1);
    assert_eq!(
        published.effects[0].variant,
        effect("CommittedVisibleSetPublished")
    );
}

#[test]
fn session_tool_visibility_kernel_accepts_deferred_request_without_phase_change() {
    let kernel = GeneratedMachineKernel::new(meerkat::schema());
    let attached = prepared_meerkat_state(&kernel);
    let requested = kernel
        .transition(
            &attached,
            &KernelInput {
                variant: input("RequestDeferredTools"),
                fields: BTreeMap::from([
                    (
                        field("names"),
                        KernelValue::Set(BTreeSet::from([
                            KernelValue::String("search".to_string()),
                            KernelValue::String("view_image".to_string()),
                        ])),
                    ),
                    (
                        field("witnesses"),
                        KernelValue::Map(BTreeMap::from([
                            (
                                KernelValue::String("search".to_string()),
                                witness("verified"),
                            ),
                            (
                                KernelValue::String("view_image".to_string()),
                                witness("verified"),
                            ),
                        ])),
                    ),
                ]),
            },
        )
        .expect("request deferred tools")
        .next_state;

    assert_eq!(requested.phase, phase("Attached"));
}
