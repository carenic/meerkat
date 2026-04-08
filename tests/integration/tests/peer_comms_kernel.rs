#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;

use meerkat_machine_kernels::generated::peer_comms;
use meerkat_machine_kernels::{KernelInput, KernelValue};

fn string(value: &str) -> KernelValue {
    KernelValue::String(value.to_string())
}

fn bool_value(value: bool) -> KernelValue {
    KernelValue::Bool(value)
}

fn raw_item_id(value: &str) -> KernelValue {
    string(value)
}

fn named_variant(enum_name: &str, variant: &str) -> KernelValue {
    KernelValue::NamedVariant {
        enum_name: enum_name.to_string(),
        variant: variant.to_string(),
    }
}

fn input(variant: &str, fields: Vec<(&str, KernelValue)>) -> KernelInput {
    KernelInput {
        variant: variant.to_string(),
        fields: fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn peer_comms_kernel_drops_untrusted_external_when_auth_required() {
    let state = peer_comms::initial_state().expect("initial state");
    let result = peer_comms::transition(
        &state,
        &input(
            "ClassifyExternalEnvelope",
            vec![
                ("raw_item_id", raw_item_id("raw-1")),
                ("require_peer_auth", bool_value(true)),
                ("sender_name_known", bool_value(false)),
                ("sender_name", string("")),
                ("fallback_sender_name", string("peer-a")),
                ("kind", named_variant("PeerEnvelopeKind", "Message")),
                ("intent", string("")),
                ("lifecycle_peer_present", bool_value(false)),
                ("lifecycle_peer", string("")),
                ("handling_mode_present", bool_value(false)),
                ("handling_mode", string("Queue")),
                ("silent_intent", bool_value(false)),
                ("dismiss_message", bool_value(false)),
            ],
        ),
    )
    .expect("classify external");

    assert_eq!(result.next_state.phase, "Ready");
    assert_eq!(result.effects.len(), 1);
    assert_eq!(result.effects[0].variant, "DropIngress");
}

#[test]
fn peer_comms_kernel_classifies_lifecycle_request_and_normalizes_mode() {
    let state = peer_comms::initial_state().expect("initial state");
    let result = peer_comms::transition(
        &state,
        &input(
            "ClassifyExternalEnvelope",
            vec![
                ("raw_item_id", raw_item_id("raw-2")),
                ("require_peer_auth", bool_value(false)),
                ("sender_name_known", bool_value(true)),
                ("sender_name", string("parent")),
                ("fallback_sender_name", string("parent")),
                ("kind", named_variant("PeerEnvelopeKind", "Request")),
                ("intent", string("mob.kickoff_failed")),
                ("lifecycle_peer_present", bool_value(true)),
                ("lifecycle_peer", string("helper-1")),
                ("handling_mode_present", bool_value(false)),
                ("handling_mode", string("Queue")),
                ("silent_intent", bool_value(false)),
                ("dismiss_message", bool_value(false)),
            ],
        ),
    )
    .expect("classify lifecycle request");

    assert_eq!(result.effects.len(), 1);
    assert_eq!(result.effects[0].variant, "EnqueueClassifiedEntry");
    assert_eq!(
        result.effects[0].fields.get("class"),
        Some(&named_variant(
            "PeerInputClass",
            "PeerLifecycleKickoffFailed"
        ))
    );
    assert_eq!(
        result.effects[0].fields.get("lifecycle_peer"),
        Some(&string("helper-1"))
    );
    assert_eq!(
        result.effects[0].fields.get("normalized_handling_mode"),
        Some(&named_variant("HandlingMode", "Queue"))
    );
}
