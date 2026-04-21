#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::redundant_clone
)]

use indexmap::IndexMap;
use meerkat_core::types::ContentInput;
use meerkat_machine_kernels::generated::flow_run;
use meerkat_machine_kernels::test_oracle::{KernelInput, KernelValue};
use meerkat_mob::definition::{
    CollectionPolicy, DependencyMode, DispatchMode, FlowSpec, FlowStepSpec, LimitsSpec,
    StepOutputFormat,
};
use meerkat_mob::ids::{FlowId, ProfileName, StepId};
use meerkat_mob::run::{FlowRunConfig, MobRun};
use std::collections::BTreeMap;

fn str_val(s: &str) -> KernelValue {
    KernelValue::String(s.into())
}
fn frame_id(s: &str) -> KernelValue {
    str_val(s)
}
fn loop_inst(s: &str) -> KernelValue {
    str_val(s)
}
fn u64_val(n: u64) -> KernelValue {
    KernelValue::U64(n)
}
fn named_variant(enum_name: &str, variant: &str) -> KernelValue {
    KernelValue::NamedVariant {
        enum_name: enum_name.into(),
        variant: variant.into(),
    }
}

/// Build a FlowRunMachine in Running state with v2 scheduler limits.
/// Flows limits through LimitsSpec in FlowRunConfig → MobRun::flow_state_for_config,
/// verifying the full pipeline from config to machine state.
fn build_running_state_with_limits(
    max_active_nodes: u64,
    max_active_frames: u64,
    max_frame_depth: u64,
) -> flow_run::State {
    let mut steps = IndexMap::new();
    steps.insert(
        StepId::from("dummy-step"),
        FlowStepSpec {
            role: ProfileName::from("worker"),
            message: ContentInput::from("placeholder"),
            depends_on: Vec::new(),
            dispatch_mode: DispatchMode::FanOut,
            collection_policy: CollectionPolicy::All,
            condition: None,
            timeout_ms: None,
            expected_schema_ref: None,
            branch: None,
            depends_on_mode: DependencyMode::All,
            allowed_tools: None,
            blocked_tools: None,
            output_format: StepOutputFormat::Json,
        },
    );

    let config = FlowRunConfig {
        flow_id: FlowId::from("test-flow"),
        flow_spec: FlowSpec {
            description: None,
            steps,
            root: None,
        },
        topology: None,
        supervisor: None,
        limits: Some(LimitsSpec {
            max_flow_duration_ms: None,
            max_step_retries: None,
            max_orphaned_turns: None,
            cancel_grace_timeout_ms: None,
            max_active_nodes: Some(max_active_nodes),
            max_active_frames: Some(max_active_frames),
            max_frame_depth: Some(max_frame_depth),
        }),
        orchestrator_role: None,
    };

    // flow_state_for_config leaves the machine in Pending state (after CreateRun)
    let state = MobRun::flow_state_for_config(&config).expect("flow_state_for_config");

    apply_input(
        &state,
        &KernelInput {
            variant: "StartRun".into(),
            fields: BTreeMap::new(),
        },
    )
    .expect("StartRun")
    .next_state
}

fn build_running_state() -> flow_run::State {
    build_running_state_with_limits(10, 10, 10)
}

/// Register a frame then pump the node scheduler, returning the next state.
fn apply_register_and_pump(state: &flow_run::State, fid: &str) -> flow_run::State {
    let reg = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id(fid))]),
    };
    let state = apply_input(state, &reg)
        .expect("RegisterReadyFrame")
        .next_state;
    let pump = KernelInput {
        variant: "PumpNodeScheduler".into(),
        fields: BTreeMap::new(),
    };
    apply_input(&state, &pump)
        .expect("PumpNodeScheduler")
        .next_state
}

fn active_node_count(state: &flow_run::State) -> KernelValue {
    KernelValue::U64(u64::from(state.active_node_count))
}

fn active_frame_count(state: &flow_run::State) -> KernelValue {
    KernelValue::U64(u64::from(state.active_frame_count))
}

fn ready_frames_value(state: &flow_run::State) -> KernelValue {
    KernelValue::Seq(
        state
            .ready_frames
            .iter()
            .cloned()
            .map(KernelValue::String)
            .collect(),
    )
}

fn apply_rawish(state: &flow_run::State, input: &KernelInput) -> flow_run::Outcome {
    apply_input(state, input).expect("typed transition")
}

fn as_string(value: &KernelValue) -> String {
    match value {
        KernelValue::String(s) => s.clone(),
        other => panic!("expected string kernel value, got {other:?}"),
    }
}

fn as_u32(value: &KernelValue) -> u32 {
    match value {
        KernelValue::U64(v) => (*v).try_into().expect("u64->u32"),
        other => panic!("expected u64 kernel value, got {other:?}"),
    }
}

fn apply_input(
    state: &flow_run::State,
    input: &KernelInput,
) -> Result<flow_run::Outcome, flow_run::TransitionError> {
    let typed = match input.variant.as_str() {
        "StartRun" => flow_run::Input::StartRun(flow_run::inputs::StartRun {}),
        "RegisterReadyFrame" => {
            let frame_id = as_string(input.fields.get("frame_id").expect("frame_id"));
            flow_run::Input::RegisterReadyFrame(flow_run::inputs::RegisterReadyFrame { frame_id })
        }
        "PumpNodeScheduler" => {
            flow_run::Input::PumpNodeScheduler(flow_run::inputs::PumpNodeScheduler {})
        }
        "RegisterPendingBodyFrame" => {
            let loop_instance_id = as_string(
                input
                    .fields
                    .get("loop_instance_id")
                    .expect("loop_instance_id"),
            );
            let depth = as_u32(input.fields.get("depth").expect("depth"));
            flow_run::Input::RegisterPendingBodyFrame(flow_run::inputs::RegisterPendingBodyFrame {
                loop_instance_id,
                depth,
            })
        }
        "PumpFrameScheduler" => {
            flow_run::Input::PumpFrameScheduler(flow_run::inputs::PumpFrameScheduler {})
        }
        "NodeExecutionReleased" => {
            let frame_id = as_string(input.fields.get("frame_id").expect("frame_id"));
            flow_run::Input::NodeExecutionReleased(flow_run::inputs::NodeExecutionReleased {
                frame_id,
            })
        }
        "FrameTerminated" => {
            let frame_id = as_string(input.fields.get("frame_id").expect("frame_id"));
            flow_run::Input::FrameTerminated(flow_run::inputs::FrameTerminated { frame_id })
        }
        other => panic!("unsupported typed test input {other}"),
    };
    flow_run::transition(state, typed, &flow_run::EmptyContext)
}

/// REQ-04: RegisterReadyFrame + PumpNodeScheduler schedules frames for node-slot grants
#[test]
fn test_pump_node_scheduler_increments_active_node_count() {
    let state = build_running_state();

    // Register a ready frame
    let reg = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-1"))]),
    };
    let outcome = apply_rawish(&state, &reg);
    let state = outcome.next_state;

    // Before pump: active_node_count should be 0
    let count = Some(active_node_count(&state));
    assert_eq!(
        count,
        Some(u64_val(0)),
        "active_node_count should be 0 before pump"
    );

    // Pump
    let pump = KernelInput {
        variant: "PumpNodeScheduler".into(),
        fields: BTreeMap::new(),
    };
    let outcome = apply_rawish(&state, &pump);

    // active_node_count should be 1
    let count = Some(active_node_count(&outcome.next_state));
    assert_eq!(
        count,
        Some(u64_val(1)),
        "active_node_count should be 1 after pump"
    );

    // GrantNodeSlot should be emitted
    assert!(
        outcome
            .effects
            .iter()
            .any(|e| matches!(e, flow_run::Effect::GrantNodeSlot(_))),
        "GrantNodeSlot expected, got: {:?}",
        outcome.effects
    );

    // frame_id in GrantNodeSlot effect should match
    let grant = outcome
        .effects
        .iter()
        .find_map(|e| match e {
            flow_run::Effect::GrantNodeSlot(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        grant.frame_id, "frame-1",
        "GrantNodeSlot.frame_id should be 'frame-1'"
    );
}

/// REQ-06: node-slot release and frame-slot release are separate machine facts.
#[test]
fn test_node_and_frame_release_adjust_counters_independently() {
    let state = build_running_state();

    // Set up: register and pump node scheduler (active_node_count=1)
    let state = apply_register_and_pump(&state, "frame-1");

    // Now active_node_count=1, active_frame_count=0 (frame count incremented by PumpFrameScheduler, not PumpNodeScheduler)
    // Let's also pump a body frame to get active_frame_count to 1:
    // Register a pending body frame
    let reg_loop = KernelInput {
        variant: "RegisterPendingBodyFrame".into(),
        fields: BTreeMap::from([
            ("loop_instance_id".into(), loop_inst("loop-1")),
            ("depth".into(), u64_val(1)),
        ]),
    };
    let state = apply_rawish(&state, &reg_loop).next_state;
    let pump_frame = KernelInput {
        variant: "PumpFrameScheduler".into(),
        fields: BTreeMap::new(),
    };
    let state = apply_rawish(&state, &pump_frame).next_state;

    // active_frame_count should be 1 now
    let active_frames = Some(active_frame_count(&state));
    assert_eq!(
        active_frames,
        Some(u64_val(1)),
        "active_frame_count should be 1 after PumpFrameScheduler"
    );

    // active_node_count should be 1 (from the node pump)
    let active_nodes = Some(active_node_count(&state));
    assert_eq!(
        active_nodes,
        Some(u64_val(1)),
        "active_node_count should be 1 before NodeExecutionReleased"
    );

    let release = KernelInput {
        variant: "NodeExecutionReleased".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-1"))]),
    };
    let released_state = apply_rawish(&state, &release).next_state;

    let active_nodes = Some(active_node_count(&released_state));
    let active_frames = Some(active_frame_count(&released_state));

    assert_eq!(
        active_nodes,
        Some(u64_val(0)),
        "active_node_count should be 0 after NodeExecutionReleased"
    );
    assert_eq!(
        active_frames,
        Some(u64_val(1)),
        "active_frame_count should remain 1 until FrameTerminated"
    );

    let term = KernelInput {
        variant: "FrameTerminated".into(),
        fields: BTreeMap::from([
            ("frame_id".into(), frame_id("frame-1")),
            (
                "status".into(),
                named_variant("FlowFrameStatus", "Completed"),
            ),
        ]),
    };
    let outcome = apply_rawish(&released_state, &term);

    let active_nodes = Some(active_node_count(&outcome.next_state));
    let active_frames = Some(active_frame_count(&outcome.next_state));

    assert_eq!(
        active_nodes,
        Some(u64_val(0)),
        "active_node_count should stay at 0 after FrameTerminated"
    );
    assert_eq!(
        active_frames,
        Some(u64_val(0)),
        "active_frame_count should decrement by 1 after FrameTerminated"
    );
}

/// REQ-05 + REQ-07: max_active_frames enforced
#[test]
fn test_max_active_frames_enforced() {
    // max_active_frames=1, so only one body frame can be active at a time
    let state = build_running_state_with_limits(10, 1, 10);

    // Register first pending body frame
    let reg_loop1 = KernelInput {
        variant: "RegisterPendingBodyFrame".into(),
        fields: BTreeMap::from([
            ("loop_instance_id".into(), loop_inst("loop-1")),
            ("depth".into(), u64_val(1)),
        ]),
    };
    let state = apply_rawish(&state, &reg_loop1).next_state;

    // active_frame_count should be 0 (pending, not yet granted)
    let frames = Some(active_frame_count(&state));
    assert_eq!(
        frames,
        Some(u64_val(0)),
        "active_frame_count should be 0 while pending"
    );

    // PumpFrameScheduler should grant the first frame
    let pump = KernelInput {
        variant: "PumpFrameScheduler".into(),
        fields: BTreeMap::new(),
    };
    let outcome = apply_rawish(&state, &pump);
    let state = outcome.next_state;

    assert!(
        outcome
            .effects
            .iter()
            .any(|e| matches!(e, flow_run::Effect::GrantBodyFrameStart(_))),
        "GrantBodyFrameStart expected for first loop frame, got: {:?}",
        outcome.effects
    );
    let frames = Some(active_frame_count(&state));
    assert_eq!(
        frames,
        Some(u64_val(1)),
        "active_frame_count should be 1 after first grant"
    );

    // Register second pending body frame
    let reg_loop2 = KernelInput {
        variant: "RegisterPendingBodyFrame".into(),
        fields: BTreeMap::from([
            ("loop_instance_id".into(), loop_inst("loop-2")),
            ("depth".into(), u64_val(1)),
        ]),
    };
    let state = apply_rawish(&state, &reg_loop2).next_state;

    // Pump again — should NOT grant because active_frame_count == max_active_frames (1 == 1)
    let pump2 = KernelInput {
        variant: "PumpFrameScheduler".into(),
        fields: BTreeMap::new(),
    };
    let result = apply_input(&state, &pump2);
    match result {
        Ok(outcome) => {
            assert!(
                !outcome
                    .effects
                    .iter()
                    .any(|e| matches!(e, flow_run::Effect::GrantBodyFrameStart(_))),
                "GrantBodyFrameStart should NOT be emitted when max_active_frames reached, got: {:?}",
                outcome.effects
            );
        }
        Err(_) => {
            // NoMatchingTransition is also acceptable — guard rejected the pump
        }
    }
}

/// REQ-07: max_frame_depth enforced — registration itself is rejected
#[test]
fn test_max_frame_depth_rejected() {
    let state = build_running_state_with_limits(10, 10, 2); // max_frame_depth=2

    // depth=3 > max_frame_depth=2 should be rejected at RegisterPendingBodyFrame.
    // The schema guard is `depth < max_frame_depth` on RegisterPendingBodyFrame,
    // so depth=3 with max=2 fails (3 < 2 is false → NoMatchingTransition).
    let reg = KernelInput {
        variant: "RegisterPendingBodyFrame".into(),
        fields: BTreeMap::from([
            ("loop_instance_id".into(), loop_inst("deep-loop")),
            ("depth".into(), u64_val(3)),
        ]),
    };
    let result = apply_input(&state, &reg);
    assert!(
        result.is_err(),
        "RegisterPendingBodyFrame with depth > max_frame_depth should be rejected, got: {:?}",
        result.ok().map(|o| format!("{:?}", o.transition_id))
    );
}

/// Verify that ready_frames queue is cleaned up after PumpNodeScheduler
#[test]
fn test_pump_node_scheduler_removes_frame_from_queue() {
    let state = build_running_state();

    // Register two frames
    let reg1 = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-a"))]),
    };
    let state = apply_rawish(&state, &reg1).next_state;
    let reg2 = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-b"))]),
    };
    let state = apply_rawish(&state, &reg2).next_state;

    // ready_frames should have 2 entries
    let frames = ready_frames_value(&state);
    assert!(
        matches!(&frames, KernelValue::Seq(v) if v.len() == 2),
        "ready_frames should have 2 entries"
    );

    // Pump once
    let pump = KernelInput {
        variant: "PumpNodeScheduler".into(),
        fields: BTreeMap::new(),
    };
    let state = apply_rawish(&state, &pump).next_state;

    // ready_frames should now have 1 entry
    let frames = ready_frames_value(&state);
    assert!(
        matches!(&frames, KernelValue::Seq(v) if v.len() == 1),
        "ready_frames should have 1 entry after pump"
    );

    // active_node_count should be 1
    let count = Some(active_node_count(&state));
    assert_eq!(count, Some(u64_val(1)));
}

/// Verify deduplication: RegisterReadyFrame with same frame_id twice only adds once
#[test]
fn test_register_ready_frame_deduplicates() {
    let state = build_running_state();

    let reg = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-dup"))]),
    };
    let state = apply_rawish(&state, &reg).next_state;

    // Second registration of same frame should be rejected (guard: not in membership)
    let reg2 = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-dup"))]),
    };
    let result = apply_input(&state, &reg2);
    assert!(
        result.is_err(),
        "Second RegisterReadyFrame with same frame_id should be rejected"
    );
}

/// REQ-04 + max_active_nodes: PumpNodeScheduler respects max_active_nodes limit
#[test]
fn test_max_active_nodes_enforced() {
    // max_active_nodes=1: first pump grants a slot; second pump must not grant
    let state = build_running_state_with_limits(1, 10, 10);

    // Register two frames
    let reg1 = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-a"))]),
    };
    let outcome = apply_rawish(&state, &reg1);
    let state = outcome.next_state;

    let reg2 = KernelInput {
        variant: "RegisterReadyFrame".into(),
        fields: BTreeMap::from([("frame_id".into(), frame_id("frame-b"))]),
    };
    let outcome = apply_rawish(&state, &reg2);
    let state = outcome.next_state;

    // First pump: should grant slot for frame-a
    let pump1 = KernelInput {
        variant: "PumpNodeScheduler".into(),
        fields: BTreeMap::new(),
    };
    let outcome = apply_rawish(&state, &pump1);
    let state = outcome.next_state;
    assert!(
        outcome
            .effects
            .iter()
            .any(|e| matches!(e, flow_run::Effect::GrantNodeSlot(_))),
        "First pump should grant a slot"
    );
    let count = Some(active_node_count(&state));
    assert_eq!(count, Some(u64_val(1)), "active_node_count should be 1");

    // Second pump: should NOT grant because active_node_count == max_active_nodes
    let pump2 = KernelInput {
        variant: "PumpNodeScheduler".into(),
        fields: BTreeMap::new(),
    };
    let result = apply_input(&state, &pump2);
    match result {
        Ok(outcome) => {
            assert!(
                !outcome
                    .effects
                    .iter()
                    .any(|e| matches!(e, flow_run::Effect::GrantNodeSlot(_))),
                "Second pump should NOT grant when max_active_nodes reached, got: {:?}",
                outcome.effects
            );
        }
        Err(_) => {
            // NoMatchingTransition also acceptable — guard rejected the pump
        }
    }
}
