//! Long run and durable worker chapters: a run killed at any point of a
//! round resumes from its last durable commit under the same run id, or,
//! when nothing of it is durable, its input runs again as a new run; either
//! way the input finishes with every round once and one receipt.
//!
//! Each row runs a scripted run through a [`LongRunHarness`] (or a durable
//! worker through a [`DurableWorkerHarness`]), kills it at a chosen point,
//! restarts over the same store, and checks the restart against what the
//! killed process committed (see [`takes_first_commit`] and [`resumes`]):
//! - a kill after a round's first commit dispatches the same calls again,
//!   under the same keys, without calling the model; a started call of an
//!   unrepeatable tool gets the typed interrupted result instead;
//! - a kill during a model call, or in a round that takes no first commit
//!   (after a compaction, a callback batch), asks the model again for that
//!   round, and its calls carry the same keys again;
//! - a kill in round 0, before any commit, requeues the input;
//! - a queued input waits for the resumed run; a stop or retire before the
//!   resume settles the run's inputs, and nothing resumes;
//! - over a store with fence leases, every restart waits out the killed
//!   process's lease.

use meerkat_core::{RunId, RunResumePoint, ToolCallDedupeKey};

use crate::failure::{ConformanceFailure, Steps};
use crate::long_run::{
    BeforeResume, DurableWorkerHarness, KillPoint, KilledRun, LongRunHarness, LongRunPlan,
    RestartReport, ResumeGate, UnrepeatableTool, call_id, resumes, takes_first_commit,
};

/// Rounds of every row's run.
const ROUNDS: u64 = 5;
/// Tool calls of each round but the last.
const CALLS: u32 = 2;

const fn plan(kill_round: u64, kill_point: KillPoint, before_resume: BeforeResume) -> LongRunPlan {
    LongRunPlan {
        rounds: ROUNDS,
        calls_per_round: CALLS,
        kill_round,
        kill_point,
        compact_in_kill_round: false,
        before_resume,
    }
}

const fn compacting(kill_round: u64, kill_point: KillPoint) -> LongRunPlan {
    LongRunPlan {
        compact_in_kill_round: true,
        ..plan(kill_round, kill_point, BeforeResume::Nothing)
    }
}

const fn unrepeatable(kill_round: u64, tool: UnrepeatableTool) -> LongRunPlan {
    plan(
        kill_round,
        KillPoint::DuringUnrepeatableTool(tool),
        BeforeResume::Nothing,
    )
}

/// The long run rows, by step name.
const LONG_RUN_ROWS: [(&str, LongRunPlan); 16] = [
    (
        "first_round_model_call_requeues",
        plan(0, KillPoint::ModelCall, BeforeResume::Nothing),
    ),
    (
        "first_round_tools_requeue",
        plan(0, KillPoint::DuringTools, BeforeResume::Nothing),
    ),
    (
        "before_dispatch",
        plan(1, KillPoint::BeforeDispatch, BeforeResume::Nothing),
    ),
    (
        "during_tools",
        plan(2, KillPoint::DuringTools, BeforeResume::Nothing),
    ),
    (
        "model_call",
        plan(2, KillPoint::ModelCall, BeforeResume::Nothing),
    ),
    (
        "during_process_tool",
        unrepeatable(1, UnrepeatableTool::BuiltInProcess),
    ),
    (
        "during_detached_tool",
        unrepeatable(1, UnrepeatableTool::DetachedNonResumable),
    ),
    (
        "during_comms_send",
        unrepeatable(2, UnrepeatableTool::CommsSend),
    ),
    (
        "during_comms_request",
        unrepeatable(2, UnrepeatableTool::CommsRequest),
    ),
    (
        "during_callback_tools",
        plan(2, KillPoint::DuringCallbackTools, BeforeResume::Nothing),
    ),
    (
        "compaction_round_model_call",
        compacting(3, KillPoint::ModelCall),
    ),
    (
        "compaction_round_tools",
        compacting(3, KillPoint::DuringTools),
    ),
    (
        "last_round_model_call",
        plan(ROUNDS - 1, KillPoint::ModelCall, BeforeResume::Nothing),
    ),
    (
        "queued_input_waits_for_resume",
        plan(2, KillPoint::DuringTools, BeforeResume::QueueInput),
    ),
    (
        "stop_before_resume",
        plan(2, KillPoint::BeforeDispatch, BeforeResume::Stop),
    ),
    (
        "retire_before_resume",
        plan(2, KillPoint::BeforeDispatch, BeforeResume::Retire),
    ),
];

/// The durable worker rows, by step name.
const DURABLE_WORKER_ROWS: [(&str, LongRunPlan, ResumeGate); 5] = [
    (
        "worker_resumes",
        plan(1, KillPoint::DuringTools, BeforeResume::Nothing),
        ResumeGate::Absent,
    ),
    (
        "worker_gate_continues",
        plan(2, KillPoint::BeforeDispatch, BeforeResume::Nothing),
        ResumeGate::Continue,
    ),
    (
        "worker_gate_superseded",
        plan(1, KillPoint::DuringTools, BeforeResume::Nothing),
        ResumeGate::Superseded,
    ),
    (
        "worker_gate_unavailable",
        plan(1, KillPoint::DuringTools, BeforeResume::Nothing),
        ResumeGate::Unavailable,
    ),
    (
        "worker_gate_unavailable_once",
        plan(2, KillPoint::DuringTools, BeforeResume::Nothing),
        ResumeGate::UnavailableOnce,
    ),
];

/// Long run chapter: every kill point of a round, unrepeatable and callback
/// tools, compaction rounds, the first and last rounds, a queued input, and
/// a stop or retire before the resume.
pub async fn long_run(harness: &dyn LongRunHarness) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter("long_run");
    let expect_lease_wait = harness.fence_leases();
    for (step, plan) in LONG_RUN_ROWS {
        let killed = harness.run_and_kill(&plan).await?;
        check_killed(&steps, step, &plan, &killed)?;
        let report = harness.restart(&plan).await?;
        check_restart(&steps, step, &plan, &killed, &report, expect_lease_wait)?;
    }
    Ok(())
}

/// Durable worker chapter: a durable worker killed mid run resumes under
/// the same identity and session, unless its resume gate refuses the resume
/// (superseded, or unavailable through every retry).
pub async fn durable_worker(harness: &dyn DurableWorkerHarness) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter("durable_worker");
    let expect_lease_wait = harness.fence_leases();
    for (step, plan, gate) in DURABLE_WORKER_ROWS {
        let killed = harness.spawn_and_kill(&plan).await?;
        check_killed(&steps, step, &plan, &killed.run)?;
        let restarted = harness.restart(&plan, gate).await?;
        steps.ensure(
            step,
            restarted.identity == killed.identity && restarted.session == killed.session,
            format!(
                "the worker came back as {} on session {}, not as {} on session {}",
                restarted.identity, restarted.session, killed.identity, killed.session
            ),
        )?;
        let refused = matches!(gate, ResumeGate::Superseded | ResumeGate::Unavailable);
        steps.ensure(
            step,
            restarted.refused == refused,
            format!(
                "the worker's run was refused: {}, expected {refused} for the gate answer {gate:?}",
                restarted.refused
            ),
        )?;
        if refused {
            check_not_resumed(
                &steps,
                step,
                &killed.run,
                &restarted.report,
                expect_lease_wait,
            )?;
        } else {
            check_restart(
                &steps,
                step,
                &plan,
                &killed.run,
                &restarted.report,
                expect_lease_wait,
            )?;
        }
    }
    Ok(())
}

/// The calls of `round`, in emission order; none for the last round.
fn calls_of(plan: &LongRunPlan, round: u64) -> Vec<String> {
    if round + 1 >= plan.rounds {
        return Vec::new();
    }
    (0..plan.calls_per_round)
        .map(|index| call_id(round, index))
        .collect()
}

/// Whether the restart dispatches the kill round's committed calls again
/// without calling the model: the round's first commit is durable and the
/// kill came after it.
fn resumes_with_tools_pending(plan: &LongRunPlan) -> bool {
    plan.kill_point != KillPoint::ModelCall && takes_first_commit(plan, plan.kill_round)
}

/// The kill round's started call that gets the interrupted result instead
/// of a dispatch.
fn interrupted_call(plan: &LongRunPlan) -> Option<String> {
    (matches!(plan.kill_point, KillPoint::DuringUnrepeatableTool(_))
        && resumes_with_tools_pending(plan))
    .then(|| call_id(plan.kill_round, 0))
}

/// A key's identity without its digest.
fn call_of(key: &ToolCallDedupeKey) -> (RunId, u64, String) {
    (key.run_id.clone(), key.round, key.call_id.clone())
}

/// The killed process's own account must fit the plan, or the restart
/// checks would compare against the wrong round.
fn check_killed(
    steps: &Steps,
    step: &'static str,
    plan: &LongRunPlan,
    killed: &KilledRun,
) -> Result<(), ConformanceFailure> {
    for key in &killed.dispatched {
        steps.ensure(
            step,
            key.run_id == killed.run_id,
            format!("the killed process dispatched {key:?} under another run"),
        )?;
        let in_kill_round_too_early = key.round == plan.kill_round
            && matches!(
                plan.kill_point,
                KillPoint::ModelCall | KillPoint::BeforeDispatch
            );
        steps.ensure(
            step,
            key.round <= plan.kill_round && !in_kill_round_too_early,
            format!(
                "the killed process dispatched {key:?}, past its kill point {:?} of round {}",
                plan.kill_point, plan.kill_round
            ),
        )?;
    }
    if let KillPoint::DuringUnrepeatableTool(tool) = plan.kill_point {
        let started = call_id(plan.kill_round, 0);
        steps.ensure(
            step,
            killed
                .dispatched
                .iter()
                .any(|key| key.round == plan.kill_round && key.call_id == started),
            format!("the {tool:?} call {started} had not started at the kill"),
        )?;
    }
    Ok(())
}

/// A restart after which nothing of the killed run resumed: a stop, a
/// retire, or a refused durable worker.
fn check_not_resumed(
    steps: &Steps,
    step: &'static str,
    killed: &KilledRun,
    report: &RestartReport,
    expect_lease_wait: bool,
) -> Result<(), ConformanceFailure> {
    steps.ensure(
        step,
        report.resumed.is_none() && report.runs.is_empty(),
        format!(
            "the restart resumed {:?} and ran {:?} for inputs settled before the resume",
            report.resumed, report.runs
        ),
    )?;
    steps.ensure(
        step,
        report.model_call_rounds.is_empty() && report.dispatched.is_empty(),
        format!(
            "the settled run {} called the model for rounds {:?} and dispatched {:?}",
            killed.run_id, report.model_call_rounds, report.dispatched
        ),
    )?;
    steps.ensure(
        step,
        report.inputs_settled,
        "the run's inputs are not terminal after it was settled",
    )?;
    check_lease_wait(steps, step, report, expect_lease_wait)
}

fn check_lease_wait(
    steps: &Steps,
    step: &'static str,
    report: &RestartReport,
    expect_lease_wait: bool,
) -> Result<(), ConformanceFailure> {
    steps.ensure(
        step,
        report.waited_out_lease == expect_lease_wait,
        if expect_lease_wait {
            "a restart over a store with fence leases did not wait out the killed process's lease"
        } else {
            "the restart waited out a fence lease, but the store records none"
        },
    )
}

/// A restart that carried the killed run's input to its end: by resuming
/// the killed run, or by running the requeued input as a new run.
fn check_restart(
    steps: &Steps,
    step: &'static str,
    plan: &LongRunPlan,
    killed: &KilledRun,
    report: &RestartReport,
    expect_lease_wait: bool,
) -> Result<(), ConformanceFailure> {
    if matches!(
        plan.before_resume,
        BeforeResume::Stop | BeforeResume::Retire
    ) {
        return check_not_resumed(steps, step, killed, report, expect_lease_wait);
    }
    check_lease_wait(steps, step, report, expect_lease_wait)?;
    let kill_round = plan.kill_round;
    let resumed_run = resumes(plan);
    let tools_pending = resumed_run && resumes_with_tools_pending(plan);

    // The killed run resumes from where its last durable commit left it, or
    // its input runs again as a new run; either way that run goes first.
    let expected_runs = match plan.before_resume {
        BeforeResume::QueueInput => 2,
        _ => 1,
    };
    let Some(input_run) = report.runs.first().cloned() else {
        return Err(steps.fail(step, "the restart ran nothing for the killed run's input"));
    };
    steps.ensure(
        step,
        report.runs.len() == expected_runs
            && (input_run == killed.run_id) == resumed_run
            && report.runs.iter().skip(1).all(|run| *run != input_run),
        format!(
            "the restart ran {:?}; the killed run {} must {} first, once",
            report.runs,
            killed.run_id,
            if resumed_run {
                "resume"
            } else {
                "be requeued as a new run that goes"
            }
        ),
    )?;
    if resumed_run {
        let Some(resumed) = &report.resumed else {
            return Err(steps.fail(step, "the killed run did not resume"));
        };
        steps.ensure(
            step,
            resumed.run_id == killed.run_id,
            format!(
                "run {} resumed instead of the killed run {}",
                resumed.run_id, killed.run_id
            ),
        )?;
        let point_fits = match &resumed.resume_point {
            RunResumePoint::ToolsPending { assistant_message } => {
                tools_pending
                    && assistant_message
                        .tool_calls()
                        .map(|call| call.id.to_string())
                        .collect::<Vec<_>>()
                        == calls_of(plan, kill_round)
            }
            RunResumePoint::AwaitingModel => !tools_pending,
        };
        steps.ensure(
            step,
            point_fits,
            format!(
                "the run resumed at {:?}; expected {} for round {kill_round}",
                resumed.resume_point,
                if tools_pending {
                    "the staged message with that round's calls"
                } else {
                    "awaiting the model"
                }
            ),
        )?;
    } else {
        steps.ensure(
            step,
            report.resumed.is_none(),
            format!(
                "{:?} resumed, but nothing of the killed run was durable",
                report.resumed
            ),
        )?;
    }

    // The model is asked again only for rounds that were not committed.
    let first_round = if resumed_run { kill_round } else { 0 };
    let first_model_round = if tools_pending {
        kill_round + 1
    } else {
        first_round
    };
    let expected_model_rounds: Vec<u64> = (first_model_round..plan.rounds).collect();
    steps.ensure(
        step,
        report.model_call_rounds == expected_model_rounds,
        format!(
            "the restart called the model for rounds {:?}, expected {expected_model_rounds:?}",
            report.model_call_rounds
        ),
    )?;

    // Every call from the first uncommitted round on is dispatched once
    // under the input's run, except a started unrepeatable call, which gets
    // the interrupted result instead.
    let interrupted = interrupted_call(plan);
    let expected_interrupted: Vec<String> = interrupted.iter().cloned().collect();
    steps.ensure(
        step,
        report.interrupted == expected_interrupted,
        format!(
            "the restart answered {:?} with the interrupted result, expected {expected_interrupted:?}",
            report.interrupted
        ),
    )?;
    let mut expected_calls: Vec<(RunId, u64, String)> = (first_round..plan.rounds)
        .flat_map(|round| {
            calls_of(plan, round)
                .into_iter()
                .map(move |call| (round, call))
        })
        .filter(|(_, call)| interrupted.as_ref() != Some(call))
        .map(|(round, call)| (input_run.clone(), round, call))
        .collect();
    let mut dispatched_calls: Vec<(RunId, u64, String)> =
        report.dispatched.iter().map(call_of).collect();
    let order = |call: &(RunId, u64, String)| (call.1, call.2.clone());
    expected_calls.sort_by_key(order);
    dispatched_calls.sort_by_key(order);
    steps.ensure(
        step,
        dispatched_calls == expected_calls,
        format!(
            "the restart dispatched {dispatched_calls:?}, expected each of {expected_calls:?} once"
        ),
    )?;

    // A call the killed process dispatched in the kill round comes back
    // under the same key, digest included, so its tool can tell the repeat.
    if resumed_run {
        for key in &killed.dispatched {
            if key.round == kill_round && interrupted.as_ref() != Some(&key.call_id) {
                steps.ensure(
                    step,
                    report.dispatched.contains(key),
                    format!("the call {key:?} was not dispatched again under the same key"),
                )?;
            }
        }
    }

    // The finished transcript holds every round of the input's run once, in
    // order, each call's result once, and the input has one receipt.
    let rounds = &report.transcript_rounds;
    let consecutive = rounds.windows(2).all(|pair| pair[1] == pair[0] + 1);
    let starts_right = match rounds.first() {
        Some(first) if plan.compact_in_kill_round => *first <= kill_round,
        Some(first) => *first == 0,
        None => false,
    };
    steps.ensure(
        step,
        consecutive && starts_right && rounds.last() == Some(&(plan.rounds - 1)),
        format!(
            "the transcript holds rounds {rounds:?}, expected each round once, in order, ending at {}",
            plan.rounds - 1
        ),
    )?;
    let mut results = report.transcript_results.clone();
    let mut expected_results: Vec<String> = rounds
        .iter()
        .flat_map(|round| calls_of(plan, *round))
        .collect();
    results.sort();
    expected_results.sort();
    steps.ensure(
        step,
        results == expected_results,
        format!(
            "the transcript holds the results {results:?}, expected each of {expected_results:?} once"
        ),
    )?;
    steps.ensure(
        step,
        report.receipts == 1,
        format!(
            "the input has {} run receipts, expected exactly one",
            report.receipts
        ),
    )?;
    steps.ensure(
        step,
        report.inputs_settled,
        "the run's inputs are not terminal after it finished",
    )
}
