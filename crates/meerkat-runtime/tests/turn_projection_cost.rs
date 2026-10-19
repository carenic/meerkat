//! Opt-in micro benchmark for the turn-state projection (#24).
//!
//! The fixture state is reached only through generated turn inputs on an
//! ephemeral `RuntimeTurnStateHandle`. `turn_projection_cost_fixture_is_valid`
//! runs in the ordinary lane and pins the phase and terminality every read
//! of the benchmark's turn observes, so the timed loop can never measure a
//! refusal path. The timing test is ignored; build one optimized binary and
//! run it alone:
//!
//! ```text
//! ./scripts/repo-cargo test --locked --release -p meerkat-runtime \
//!   --test turn_projection_cost --no-run --message-format=json
//! "$BINARY" --exact turn_projection_cost_b1 --ignored --nocapture --test-threads=1
//! ```
//!
//! It prints one JSON line per case with the raw per-sample nanoseconds.
//! A single run is not speedup evidence: compare two binaries in interleaved
//! pairs on the same lane and CPU set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::time::Instant;

use meerkat_core::handles::TurnStateHandle;
use meerkat_core::lifecycle::RunId;
use meerkat_core::ops::{AsyncOpRef, OperationId, WaitPolicy};
use meerkat_core::turn_execution_authority::{ContentShape, TurnPhase, TurnPrimitiveKind};
use meerkat_runtime::RuntimeTurnStateHandle;
use uuid::Uuid;

const SNAPSHOT_WARMUP: usize = 1_000;
const SNAPSHOT_SAMPLES: usize = 20_000;
const TURN_WARMUP: usize = 200;
const TURN_SAMPLES: usize = 4_000;
const TOOL_CALLS: u64 = 3;
const PENDING_OPS: u128 = 8;
const BARRIER_OPS: u128 = 2;

fn run_id(n: u128) -> RunId {
    RunId(Uuid::from_u128(0x7e57_0000 + n))
}

fn op(n: u128) -> OperationId {
    OperationId(Uuid::from_u128(0x0b0b_0000 + n))
}

fn pending_ops() -> (BTreeSet<AsyncOpRef>, BTreeSet<OperationId>) {
    let refs = (0..PENDING_OPS)
        .map(|i| AsyncOpRef {
            operation_id: op(i),
            wait_policy: if i < BARRIER_OPS {
                WaitPolicy::Barrier
            } else {
                WaitPolicy::Detached
            },
        })
        .collect();
    let barrier = (0..BARRIER_OPS).map(op).collect();
    (refs, barrier)
}

/// Start a run and stop in `WaitingForOps` with pending and barrier ops.
fn enter_waiting_for_ops(handle: &RuntimeTurnStateHandle, run: &RunId) {
    handle
        .start_conversation_run(
            run.clone(),
            TurnPrimitiveKind::ConversationTurn,
            ContentShape::Conversation,
            false,
            false,
            0,
        )
        .unwrap();
    handle.primitive_applied(run.clone()).unwrap();
    handle
        .llm_returned_tool_calls(run.clone(), TOOL_CALLS)
        .unwrap();
    let (refs, barrier) = pending_ops();
    handle
        .register_pending_ops(run.clone(), refs, barrier)
        .unwrap();
}

/// One whole turn with a tool round, reading the projection after each
/// input the way a turn loop does. Returns each read's phase and verdict.
fn tool_turn(handle: &RuntimeTurnStateHandle, run: &RunId) -> [(TurnPhase, bool); 6] {
    let read = || {
        let snapshot = handle.snapshot();
        (snapshot.turn_phase, snapshot.turn_terminal)
    };
    enter_waiting_for_ops(handle, run);
    let waiting = read();
    let (_, barrier) = pending_ops();
    handle.ops_barrier_satisfied(run.clone(), barrier).unwrap();
    let satisfied = read();
    handle.tool_calls_resolved(run.clone()).unwrap();
    let resolved = read();
    handle.boundary_continue(run.clone()).unwrap();
    let continued = read();
    handle.llm_returned_terminal(run.clone()).unwrap();
    let returned = read();
    handle.boundary_complete(run.clone()).unwrap();
    let completed = read();
    [waiting, satisfied, resolved, continued, returned, completed]
}

const EXPECTED_TOOL_TURN: [(TurnPhase, bool); 6] = [
    (TurnPhase::WaitingForOps, false),
    (TurnPhase::WaitingForOps, false),
    (TurnPhase::DrainingBoundary, false),
    (TurnPhase::CallingLlm, false),
    (TurnPhase::DrainingBoundary, false),
    (TurnPhase::Completed, true),
];

#[test]
fn turn_projection_cost_fixture_is_valid() {
    let handle = RuntimeTurnStateHandle::ephemeral();
    let run = run_id(0);
    enter_waiting_for_ops(&handle, &run);
    let snapshot = handle.snapshot();
    assert_eq!(snapshot.turn_phase, TurnPhase::WaitingForOps);
    assert_eq!(snapshot.active_run_id, Some(run));
    assert!(!snapshot.turn_terminal);
    assert_eq!(snapshot.pending_op_refs.len(), PENDING_OPS as usize);
    assert_eq!(snapshot.barrier_operation_ids.len(), BARRIER_OPS as usize);
    assert!(snapshot.has_barrier_ops);

    let handle = RuntimeTurnStateHandle::ephemeral();
    for n in 1..=3 {
        assert_eq!(tool_turn(&handle, &run_id(n)), EXPECTED_TOOL_TURN);
    }
}

fn emit(case: &str, warmup: usize, samples: &[u64]) {
    println!(
        "{}",
        serde_json::json!({
            "bench": "b1",
            "case": case,
            "warmup": warmup,
            "n": samples.len(),
            "ns": samples,
        })
    );
}

fn elapsed_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[test]
#[ignore = "opt-in timing; see the module docs"]
fn turn_projection_cost_b1() {
    // One projection read of a mid-turn state.
    let handle = RuntimeTurnStateHandle::ephemeral();
    enter_waiting_for_ops(&handle, &run_id(0));
    for _ in 0..SNAPSHOT_WARMUP {
        std::hint::black_box(handle.snapshot());
    }
    let mut samples = Vec::with_capacity(SNAPSHOT_SAMPLES);
    for _ in 0..SNAPSHOT_SAMPLES {
        let start = Instant::now();
        std::hint::black_box(handle.snapshot());
        samples.push(elapsed_ns(start));
    }
    emit("snapshot_waiting_for_ops", SNAPSHOT_WARMUP, &samples);

    // One whole tool turn of inputs, each followed by a read.
    let handle = RuntimeTurnStateHandle::ephemeral();
    let mut next = 1_u128;
    for _ in 0..TURN_WARMUP {
        std::hint::black_box(tool_turn(&handle, &run_id(next)));
        next += 1;
    }
    let mut samples = Vec::with_capacity(TURN_SAMPLES);
    for _ in 0..TURN_SAMPLES {
        let run = run_id(next);
        next += 1;
        let start = Instant::now();
        let reads = tool_turn(&handle, &run);
        samples.push(elapsed_ns(start));
        assert_eq!(reads, EXPECTED_TOOL_TURN);
    }
    emit("tool_turn_inputs_and_reads", TURN_WARMUP, &samples);
}
