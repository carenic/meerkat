//! Long run and durable worker chapters: a run killed at any point of a
//! round resumes at its last committed round, under the same run id, and
//! finishes with every round once and one receipt.
//!
//! Each row runs a scripted run through a [`LongRunHarness`] (or a durable
//! worker through a [`DurableWorkerHarness`]), kills it at a chosen point,
//! restarts over the same store, and checks the restart against what the
//! killed process committed: a kill during a model call redoes that model
//! call; a kill after the round's first commit dispatches the same calls
//! again under the same keys without calling the model; a started process
//! tool is not dispatched again but answered with the typed interrupted
//! result; a stop or retire before the resume settles the run's inputs and
//! nothing resumes; a queued input waits for the resumed run.

use meerkat_core::{RunResumePoint, ToolCallDedupeKey};

use crate::failure::{ConformanceFailure, Steps};
use crate::long_run::{
    BeforeResume, DurableWorkerHarness, KillMode, KillPoint, KilledRun, LongRunHarness,
    LongRunPlan, RestartReport, ResumeGate, call_id,
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

/// The long run rows, by step name.
const LONG_RUN_ROWS: [(&str, LongRunPlan); 10] = [
    (
        "first_round_during_tools",
        plan(0, KillPoint::DuringTools, BeforeResume::Nothing),
    ),
    (
        "before_dispatch",
        plan(1, KillPoint::BeforeDispatch, BeforeResume::Nothing),
    ),
    (
        "during_process_tool",
        plan(1, KillPoint::DuringProcessTool, BeforeResume::Nothing),
    ),
    (
        "model_call",
        plan(2, KillPoint::ModelCall, BeforeResume::Nothing),
    ),
    (
        "during_tools",
        plan(2, KillPoint::DuringTools, BeforeResume::Nothing),
    ),
    (
        "compaction_round",
        LongRunPlan {
            compact_in_kill_round: true,
            ..plan(3, KillPoint::ModelCall, BeforeResume::Nothing)
        },
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
const DURABLE_WORKER_ROWS: [(&str, LongRunPlan, ResumeGate); 3] = [
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
];

/// Long run chapter: every kill point of a round, a compaction round, the
/// last round, a queued input, and a stop or retire before the resume.
pub async fn long_run(harness: &dyn LongRunHarness) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter("long_run");
    let expect_lease_wait =
        harness.kill_mode() == KillMode::DropAndReopen && harness.fence_leases();
    for (step, plan) in LONG_RUN_ROWS {
        let killed = harness.run_and_kill(&plan).await?;
        check_killed(&steps, step, &plan, &killed)?;
        let report = harness.restart(&plan).await?;
        check_restart(&steps, step, &plan, &killed, &report, expect_lease_wait)?;
    }
    Ok(())
}

/// Durable worker chapter: a durable worker killed mid run resumes under
/// the same identity and session, unless its resume gate answers superseded.
pub async fn durable_worker(harness: &dyn DurableWorkerHarness) -> Result<(), ConformanceFailure> {
    let steps = Steps::chapter("durable_worker");
    let expect_lease_wait =
        harness.kill_mode() == KillMode::DropAndReopen && harness.fence_leases();
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
        let superseded = gate == ResumeGate::Superseded;
        steps.ensure(
            step,
            restarted.superseded == superseded,
            format!(
                "the worker's run stopped as superseded: {}, expected {superseded}",
                restarted.superseded
            ),
        )?;
        if superseded {
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

/// The process tool call of the kill round, when the plan kills during one.
fn process_call(plan: &LongRunPlan) -> Option<String> {
    (plan.kill_point == KillPoint::DuringProcessTool).then(|| call_id(plan.kill_round, 0))
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
    if let Some(process) = process_call(plan) {
        steps.ensure(
            step,
            killed
                .dispatched
                .iter()
                .any(|key| key.round == plan.kill_round && key.call_id == process),
            format!("the process tool call {process} had not started at the kill"),
        )?;
    }
    Ok(())
}

/// A restart after which nothing of the killed run resumed: a stop, a
/// retire, or a superseded durable worker.
fn check_not_resumed(
    steps: &Steps,
    step: &'static str,
    killed: &KilledRun,
    report: &RestartReport,
    expect_lease_wait: bool,
) -> Result<(), ConformanceFailure> {
    steps.ensure(
        step,
        report.resumed.is_none() && !report.runs.contains(&killed.run_id),
        format!(
            "the run resumed ({:?}, runs {:?}) although it was settled before the resume",
            report.resumed, report.runs
        ),
    )?;
    steps.ensure(
        step,
        report.runs.is_empty(),
        format!(
            "the restart started runs {:?} for settled inputs",
            report.runs
        ),
    )?;
    steps.ensure(
        step,
        report.model_call_rounds.is_empty() && report.dispatched.is_empty(),
        format!(
            "the settled run called the model for rounds {:?} and dispatched {:?}",
            report.model_call_rounds, report.dispatched
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
            "a drop and reopen over a store with fence leases did not wait out the dropped \
             process's lease"
        } else {
            "the restart waited out a fence lease although nothing held one"
        },
    )
}

/// A restart that resumed the killed run and finished it.
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

    // The same run resumes, from where its last commit left it.
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
    let expected_point = if plan.kill_point == KillPoint::ModelCall {
        RunResumePoint::AwaitingModel
    } else {
        RunResumePoint::ToolsPending {
            call_ids: calls_of(plan, kill_round),
        }
    };
    steps.ensure(
        step,
        resumed.resume_point == expected_point,
        format!(
            "the run resumed at {:?}, expected {expected_point:?}",
            resumed.resume_point
        ),
    )?;
    let expected_runs = match plan.before_resume {
        BeforeResume::QueueInput => 2,
        _ => 1,
    };
    steps.ensure(
        step,
        report.runs.len() == expected_runs
            && report.runs.first() == Some(&killed.run_id)
            && report.runs.iter().skip(1).all(|run| *run != killed.run_id),
        format!(
            "the restart ran {:?}; the resumed run {} must go first, once",
            report.runs, killed.run_id
        ),
    )?;

    // A model call is redone only for the round it was lost in.
    let first_model_round = if plan.kill_point == KillPoint::ModelCall {
        kill_round
    } else {
        kill_round + 1
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

    // Every call from the kill round on is dispatched once, under its own
    // key, except a started process tool, which gets the interrupted result.
    let process = process_call(plan);
    let expected_interrupted: Vec<String> = process.iter().cloned().collect();
    steps.ensure(
        step,
        report.interrupted == expected_interrupted,
        format!(
            "the restart answered {:?} with the interrupted result, expected {expected_interrupted:?}",
            report.interrupted
        ),
    )?;
    let mut expected_dispatched: Vec<ToolCallDedupeKey> = (kill_round..plan.rounds)
        .flat_map(|round| {
            calls_of(plan, round)
                .into_iter()
                .map(move |call_id| (round, call_id))
        })
        .filter(|(_, call_id)| process.as_ref() != Some(call_id))
        .map(|(round, call_id)| ToolCallDedupeKey {
            run_id: killed.run_id.clone(),
            round,
            call_id,
        })
        .collect();
    let mut dispatched = report.dispatched.clone();
    let order = |key: &ToolCallDedupeKey| (key.round, key.call_id.clone());
    expected_dispatched.sort_by_key(order);
    dispatched.sort_by_key(order);
    steps.ensure(
        step,
        dispatched == expected_dispatched,
        format!(
            "the restart dispatched {dispatched:?}, expected each of {expected_dispatched:?} once"
        ),
    )?;
    for key in &killed.dispatched {
        if key.round == kill_round && process.as_ref() != Some(&key.call_id) {
            steps.ensure(
                step,
                report.dispatched.contains(key),
                format!("the call {key:?} was not dispatched again under the same key"),
            )?;
        }
    }

    // The finished transcript holds every round once, in order, each call's
    // result once, and the run has one receipt.
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
            "the run has {} receipts, expected exactly one",
            report.receipts
        ),
    )?;
    steps.ensure(
        step,
        report.inputs_settled,
        "the run's inputs are not terminal after it finished",
    )
}
