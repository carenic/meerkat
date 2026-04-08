#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeMap;

use meerkat_machine_kernels::generated::session_turn_admission;
use meerkat_machine_kernels::{KernelInput, KernelValue};

fn input(variant: &str) -> KernelInput {
    KernelInput {
        variant: variant.to_string(),
        fields: BTreeMap::new(),
    }
}

#[test]
fn session_turn_admission_kernel_gracefully_drains_running_shutdown() {
    let state = session_turn_admission::initial_state().expect("initial state");
    let admitted = session_turn_admission::transition(&state, &input("RequestStartTurn"))
        .expect("request start")
        .next_state;
    let running = session_turn_admission::transition(&admitted, &input("BeginRun"))
        .expect("begin run")
        .next_state;
    let shutdown = session_turn_admission::transition(&running, &input("RequestShutdown"))
        .expect("request shutdown")
        .next_state;
    assert_eq!(shutdown.phase, "Running");
    assert_eq!(
        shutdown.fields.get("shutdown_pending"),
        Some(&KernelValue::Bool(true))
    );

    let completing = session_turn_admission::transition(&shutdown, &input("ResolveRun"))
        .expect("resolve run")
        .next_state;
    let finalized = session_turn_admission::transition(&completing, &input("FinalizeTurn"))
        .expect("finalize")
        .next_state;
    assert_eq!(finalized.phase, "ShuttingDown");
}

#[test]
fn session_turn_admission_kernel_interrupt_only_wakes_running_turns() {
    let state = session_turn_admission::initial_state().expect("initial state");
    assert!(
        session_turn_admission::transition(&state, &input("RequestInterrupt")).is_err(),
        "idle sessions must reject interrupts"
    );

    let admitted = session_turn_admission::transition(&state, &input("RequestStartTurn"))
        .expect("request start")
        .next_state;
    let running = session_turn_admission::transition(&admitted, &input("BeginRun"))
        .expect("begin run")
        .next_state;
    let interrupted = session_turn_admission::transition(&running, &input("RequestInterrupt"))
        .expect("running interrupt");
    assert_eq!(interrupted.next_state.phase, "Running");
    assert_eq!(interrupted.effects.len(), 1);
    assert_eq!(interrupted.effects[0].variant, "WakeInterrupt");
}
