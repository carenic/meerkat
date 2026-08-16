//! Bounded, typed supervision of the staged -> executing run transition.
//!
//! `StageForRun` binds an input to a run and removes it from its work lane.
//! From that moment the input is owned by exactly one consumer: the executor
//! the runtime loop calls `CoreExecutor::apply` on. Nothing downstream of
//! staging was bounded, so a consumer that never picked the run up left the
//! input `Staged` forever, with no error, no state change, and no log line -
//! a caller could wait indefinitely on work no one was doing.
//!
//! Run establishment proves state authority (the input is queued, lane-bound,
//! sequence-bound, not already run-associated, and the run matches the
//! machine's `current_run_id`). It proves nothing about the consumer's ability
//! to consume. The runtime loop's own liveness is proven by construction - it
//! is the thing that stages and then calls `apply` - but the actor behind
//! `apply` is unverified at that point and cannot be probed non-destructively.
//!
//! What *is* observable is the machine's own turn state, and only when the loop
//! actually signalled this run's turn start. The loop applies
//! `StartConversationRun`/`StartImmediateAppend` in
//! `prepare_turn_state_for_primitive`, which writes
//! `TurnPhase::ApplyingPrimitive`; the agent applies `PrimitiveApplied` from
//! inside the turn - before the first LLM call - moving the phase off
//! `ApplyingPrimitive` on the same shared authority. So "this run began
//! executing" is a machine-owned fact for exactly those runs, and the bound can
//! be armed honestly: a turn that is slow *after* beginning has already left
//! `ApplyingPrimitive` and is never disturbed.
//!
//! Note what that does *not* say. `ApplyingPrimitive` covers everything from
//! the loop's turn-start transition to the agent's `PrimitiveApplied`, and
//! session hydration happens inside that span, so the bound is not incapable of
//! firing on live work - it is incapable of firing on work that has begun its
//! turn. See [`RUN_EXECUTION_START_BOUND`] for what that costs and why the
//! bound is set where it is.
//!
//! `prepare_turn_state_for_primitive` deliberately skips the turn-start
//! transition for two classes (an appends-empty staged primitive, and the
//! retired drain). For those the phase field says nothing about this run, so
//! this module reports [`RunExecutionProgress::ExecutionStartUnobservable`] and
//! refuses to escalate rather than reading the previous turn's leftover phase
//! as a clean bill of health.
//!
//! Supervision is split in two because the two halves have different reach:
//!
//! * [`StagedRunStartWatchdog`] runs in its own task from the durable
//!   `StageForRun` commit. It only reports, never terminalizes, so it can cover
//!   the whole window - including the pre-`apply` segment, which takes a
//!   blocking `std` mutex and therefore cannot be supervised by a `select!` in
//!   the loop's own task.
//! * [`apply_with_execution_start_bound`] owns the escalation. It can only arm
//!   once the `apply` future exists, because escalating means dropping that
//!   future, but its deadline is measured from the staging instant so the
//!   window it bounds is the staged -> executing window and not merely the
//!   apply -> executing one.
//!
//! The shell supplies only the observation (the window elapsed and the run's
//! primitive is still un-applied). The resolution stays machine-owned: the
//! typed error travels the existing failed-apply path, which realizes the
//! machine's run terminal and resolves completion waiters.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use meerkat_core::lifecycle::core_executor::{CoreApplyOutput, CoreExecutorError};
use meerkat_core::lifecycle::run_primitive::RunPrimitive;
use meerkat_core::lifecycle::{CoreExecutor, InputId, RunId};

use crate::meerkat_machine::dsl as mm_dsl;

// Monotonic clock for the staged -> executing window. `tokio_with_wasm`'s time
// alias has no `Instant`, so wasm32 takes the workspace's browser-safe one
// (`performance.now()`); native takes tokio's, which follows the test runtime's
// virtual clock so the window can be exercised without real sleeping.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::tokio::time::Instant;
#[cfg(target_arch = "wasm32")]
pub(crate) use meerkat_core::time_compat::Instant;

/// How long a staged run may sit without visibly beginning execution before
/// the condition is reported, and how often the report repeats while it holds.
///
/// The notice tier never terminalizes anything, so it cannot harm live work:
/// it states a fact ("this run has not begun executing yet") that an operator
/// previously had to reconstruct from state tables.
///
/// "Reported" means a `tracing` line and nothing else. There is no event-stream
/// or wire delivery of this condition; a caller waiting on the run sees no
/// change at this tier.
pub(crate) const RUN_EXECUTION_START_NOTICE: Duration = Duration::from_secs(120);

/// How long a staged run may sit with its primitive provably un-applied before
/// the runtime loop concludes the consumer will never pick it up.
///
/// Legitimate pre-LLM latency is dominated by session hydration, which scales
/// with transcript size: production measured 14MB at ~60s and 94MB at ~180s.
/// That is a curve, not a ceiling, and because a run that reaches this bound
/// is terminalized without re-queuing, a false positive costs the caller its
/// request permanently. The notice tier above is what closes the reported
/// blindness at two minutes, so this hard bound is deliberately set far clear
/// of any plausible extrapolation of that curve rather than close to it.
///
/// Precisely: this bound cannot fire on work that has begun its turn, because
/// `PrimitiveApplied` moves the phase off `ApplyingPrimitive` before the first
/// LLM call. It can in principle fire on a *live* hydration that exceeds an
/// hour - roughly 20x the largest measured. That false positive costs the
/// request.
///
/// It cannot double-execute: the contributor is terminalized in the same
/// realization as the run and is never returned to a work lane, so no
/// successor picks it up. The stronger claim - that it cannot leave any
/// durable residue at all - rests on `PrimitiveUnapplied` meaning the agent
/// loop has not yet applied the primitive, which is true of the CONVERSATION.
/// Whether every session-service path between staging and that transition is
/// likewise free of durable writes is NOT independently verified here, so
/// this comment does not assert it.
pub(crate) const RUN_EXECUTION_START_BOUND: Duration = Duration::from_secs(3_600);

// A notice tier at or past the hard bound would mean the window is escalated
// before it is ever reported, and the emitted `bound_secs` would stop
// describing the deadline actually used. Escalation terminalizes a run and
// abandons a household instruction, so the ordering is a compile-time fact
// rather than a runtime clamp.
const _: () = assert!(
    RUN_EXECUTION_START_NOTICE.as_secs() < RUN_EXECUTION_START_BOUND.as_secs(),
    "the run-execution notice tier must fire strictly before the hard bound"
);

/// Whether the runtime loop actually signalled this run's turn start on the
/// shared machine authority.
///
/// This gates the *interpretation* of `turn_phase`, it does not replace the
/// read: `turn_phase` is a single field shared by every run on the session, so
/// it only describes this run once this run's turn-start transition has been
/// applied against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnStartSignal {
    /// The loop applied the machine's turn-start transition for this run.
    Signalled,
    /// The loop deliberately skipped the turn-start transition (an
    /// appends-empty staged primitive, or the retired drain), so the phase
    /// field carries no information about this run.
    NotSignalled,
}

/// Shared, monotonic record of whether this run's turn start was signalled.
///
/// The watchdog starts at the durable `StageForRun` commit, before the loop
/// reaches `prepare_turn_state_for_primitive`, so the signal has to be
/// observable after the fact rather than captured up front. Until it flips,
/// every observation is honestly unobservable.
#[derive(Clone, Default)]
pub(crate) struct TurnStartSignalCell(Arc<AtomicBool>);

impl TurnStartSignalCell {
    pub(crate) fn mark_signalled(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn signal(&self) -> TurnStartSignal {
        if self.0.load(Ordering::SeqCst) {
            TurnStartSignal::Signalled
        } else {
            TurnStartSignal::NotSignalled
        }
    }
}

/// The exact machine facts an execution-start observation is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunTurnStateFacts {
    /// A runtime binding is recorded, so a session-owned turn-state handle was
    /// minted against this authority.
    pub(crate) runtime_bound: bool,
    /// Machine authority reports this exact run as `current_run_id`.
    pub(crate) run_is_current: bool,
    /// The shared turn phase is `ApplyingPrimitive`.
    pub(crate) applying_primitive: bool,
}

/// Machine-observed answer to "has this exact run begun executing?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunExecutionProgress {
    /// Machine authority shows the run's primitive applied (or the turn
    /// already past it). The consumer is alive and working.
    Executing,
    /// Machine authority still shows the run's primitive un-applied. The
    /// consumer accepted ownership of the staged input and did nothing.
    PrimitiveUnapplied,
    /// Machine authority no longer reports this run as current, so the staged
    /// -> executing window is no longer the thing being measured.
    RunNotCurrent,
    /// No runtime binding is recorded, so no session-owned turn-state handle
    /// was minted against this authority and the agent's turn writes land
    /// somewhere this observer cannot see. Unobservable, never escalated.
    RuntimeUnbound,
    /// This run's turn start was never signalled on this authority, so the
    /// shared `turn_phase` describes some other run (or a fresh session's
    /// default) and says nothing about whether this one started. Unobservable,
    /// never escalated.
    ExecutionStartUnobservable,
    /// Machine authority could not be read without blocking. Unprovable,
    /// never escalated - but also not evidence that the window closed, so the
    /// watchdog keeps reporting rather than standing down.
    Unreadable,
}

impl RunExecutionProgress {
    /// Only a positively proven un-applied primitive may terminalize a run.
    /// Every other observation refuses rather than risking live work.
    pub(crate) fn proves_execution_never_started(self) -> bool {
        matches!(self, Self::PrimitiveUnapplied)
    }

    /// Whether this observation is a positive fact that the staged ->
    /// executing window is over, and supervision can stand down silently.
    ///
    /// `Executing` and `RunNotCurrent` are such facts: the turn began, or the
    /// run moved on. Everything else either proves non-progress or is the
    /// absence of a fact, and standing down on absence would be the original
    /// defect in miniature - a consumer wedged while holding the authority
    /// mutex makes every read unreadable, which is precisely the shape this
    /// supervision exists for.
    pub(crate) fn closes_execution_start_window(self) -> bool {
        matches!(self, Self::Executing | Self::RunNotCurrent)
    }

    /// Whether this observation means the escalation bound cannot arm at all
    /// for this run, which an operator needs told: the safety property this
    /// release adds is off for that run.
    pub(crate) fn execution_start_is_unobservable(self) -> bool {
        matches!(
            self,
            Self::RuntimeUnbound | Self::ExecutionStartUnobservable
        )
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Executing => "Executing",
            Self::PrimitiveUnapplied => "PrimitiveUnapplied",
            Self::RunNotCurrent => "RunNotCurrent",
            Self::RuntimeUnbound => "RuntimeUnbound",
            Self::ExecutionStartUnobservable => "ExecutionStartUnobservable",
            Self::Unreadable => "Unreadable",
        }
    }
}

/// Classify a set of machine facts under the turn-start signal that says
/// whether those facts describe this run at all.
pub(crate) fn classify_execution_start(
    facts: RunTurnStateFacts,
    turn_start: TurnStartSignal,
) -> RunExecutionProgress {
    if !facts.runtime_bound {
        return RunExecutionProgress::RuntimeUnbound;
    }
    if turn_start == TurnStartSignal::NotSignalled {
        return RunExecutionProgress::ExecutionStartUnobservable;
    }
    if !facts.run_is_current {
        return RunExecutionProgress::RunNotCurrent;
    }
    if facts.applying_primitive {
        RunExecutionProgress::PrimitiveUnapplied
    } else {
        RunExecutionProgress::Executing
    }
}

/// Read seam for the machine-owned run-execution fact.
///
/// Kept as a trait so the classification and the bound can be exercised
/// without standing up a machine, and so the supervisor never reaches for a
/// driver lock the wedged party may be holding.
pub(crate) trait RunExecutionProgressSource: Send + Sync {
    fn observe(&self, run_id: &RunId) -> RunExecutionProgress;
}

/// Production source: the session's shared generated-machine authority.
pub(crate) struct AuthorityRunExecutionProgress {
    authority: crate::driver::ephemeral::SharedIngressDslAuthority,
    turn_start: TurnStartSignalCell,
}

impl AuthorityRunExecutionProgress {
    pub(crate) fn new(
        authority: crate::driver::ephemeral::SharedIngressDslAuthority,
        turn_start: TurnStartSignalCell,
    ) -> Self {
        Self {
            authority,
            turn_start,
        }
    }
}

impl RunExecutionProgressSource for AuthorityRunExecutionProgress {
    fn observe(&self, run_id: &RunId) -> RunExecutionProgress {
        // `try_lock` is deliberate: a wedged holder of this authority must not
        // be able to wedge the supervisor too. An unreadable authority is an
        // unprovable one, which never escalates.
        let authority = match self.authority.try_lock() {
            Ok(authority) => authority,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return RunExecutionProgress::Unreadable,
        };
        let state = authority.state();
        let current = state
            .current_run_id
            .as_ref()
            .and_then(crate::meerkat_machine::dsl_authority::current_run_id_from_dsl);
        let facts = RunTurnStateFacts {
            runtime_bound: state.active_runtime_id.is_some(),
            run_is_current: current.as_ref() == Some(run_id),
            applying_primitive: state.turn_phase == mm_dsl::TurnPhase::ApplyingPrimitive,
        };
        classify_execution_start(facts, self.turn_start.signal())
    }
}

/// Non-escalating supervision of the whole staged -> executing window, loud in
/// the log and nowhere else.
///
/// The escalation bound can only arm once `apply` is entered, because
/// escalating means dropping the `apply` future. Everything between the
/// durable `StageForRun` commit and that call runs in the loop's own task and
/// part of it takes a blocking `std` mutex, so a wedge there cannot be
/// observed by a `select!` in that same task - the thread is not free to poll
/// it. This watchdog therefore lives in its own task, which is what makes the
/// reported field shape (staged, silent, forever) impossible *in the log*, no
/// matter where in the window the loop is stuck.
///
/// The reach of that is narrower than "loud" suggests, so state it plainly:
/// this emits `tracing` records and nothing else. No runtime event, no
/// completion, and no wire delivery carries the condition to a caller or a
/// host. For the classes the escalation bound can never arm on
/// ([`RunExecutionProgress::RuntimeUnbound`],
/// [`RunExecutionProgress::ExecutionStartUnobservable`], and a persistently
/// [`RunExecutionProgress::Unreadable`] authority) that log line is the only
/// signal that exists anywhere, and the caller still waits.
///
/// It never terminalizes anything. Escalation stays with the task that owns
/// the `apply` future; a supervisor that could terminalize a run from outside
/// that task would be a fresh double-execution hazard.
pub(crate) struct StagedRunStartWatchdog {
    handle: crate::tokio::task::JoinHandle<()>,
}

impl StagedRunStartWatchdog {
    pub(crate) fn spawn(
        progress: Arc<dyn RunExecutionProgressSource + 'static>,
        run_id: RunId,
        input_ids: Vec<InputId>,
        staged_at: Instant,
        notice_every: Duration,
    ) -> Self {
        let handle = crate::tokio::spawn(async move {
            loop {
                crate::tokio::time::sleep(notice_every).await;
                let observed = progress.observe(&run_id);
                if observed.closes_execution_start_window() {
                    return;
                }
                let staged_secs = staged_at.elapsed().as_secs();
                let inputs = input_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                if observed.execution_start_is_unobservable() {
                    tracing::warn!(
                        %run_id,
                        %inputs,
                        observed = observed.as_str(),
                        staged_secs,
                        "staged run has not returned and its execution start is unobservable; \
                         the execution-start bound cannot arm for this run"
                    );
                } else {
                    tracing::error!(
                        %run_id,
                        %inputs,
                        observed = observed.as_str(),
                        staged_secs,
                        bound_secs = RUN_EXECUTION_START_BOUND.as_secs(),
                        "staged run has not begun executing; its consumer accepted the run and \
                         applied nothing"
                    );
                }
            }
        });
        Self { handle }
    }
}

impl Drop for StagedRunStartWatchdog {
    fn drop(&mut self) {
        // RAII so every early return between staging and the end of `apply`
        // retires the watchdog without hand-threading an abort through them.
        self.handle.abort();
    }
}

/// Apply a run primitive under a bounded staged -> executing window.
///
/// The `apply` future is pinned and polled first (`biased`) for the whole
/// call, so a slow turn is never cancelled and can never be double-executed by
/// this path. The single escalating branch requires positive proof that the
/// primitive was never applied - the turn had not begun mutating the
/// conversation - which is what makes dropping the future there containment
/// rather than a lost turn.
///
/// The deadline is measured from `staged_at`, not from entry, so time spent
/// between the `StageForRun` commit and this call counts against the same
/// window rather than extending it.
///
/// No release, requeue or retry happens here: the staged input stays owned by
/// its run and travels the machine's failed-apply terminal instead.
pub(crate) async fn apply_with_execution_start_bound(
    executor: &mut dyn CoreExecutor,
    progress: &dyn RunExecutionProgressSource,
    run_id: RunId,
    primitive: RunPrimitive,
    staged_at: Instant,
    bound: Duration,
) -> Result<CoreApplyOutput, CoreExecutorError> {
    let apply_future = executor.apply(run_id.clone(), primitive);
    let mut apply_future = std::pin::pin!(apply_future);

    let deadline = crate::tokio::time::sleep(bound.saturating_sub(staged_at.elapsed()));
    let mut deadline = std::pin::pin!(deadline);
    crate::tokio::select! {
        biased;
        result = &mut apply_future => return result,
        () = deadline.as_mut() => {}
    }

    let observed = progress.observe(&run_id);
    let staged_secs = staged_at.elapsed().as_secs();
    if !observed.proves_execution_never_started() {
        if observed.execution_start_is_unobservable() {
            tracing::warn!(
                %run_id,
                observed = observed.as_str(),
                staged_secs,
                bound_secs = bound.as_secs(),
                "staged run passed its execution-start bound with its start unobservable; \
                 leaving the run in flight"
            );
        } else {
            tracing::error!(
                %run_id,
                observed = observed.as_str(),
                staged_secs,
                bound_secs = bound.as_secs(),
                "staged run passed its execution-start bound but non-progress is unproven; \
                 leaving the run in flight"
            );
        }
        return apply_future.await;
    }

    tracing::error!(
        %run_id,
        observed = observed.as_str(),
        staged_secs,
        bound_secs = bound.as_secs(),
        "runtime loop concluded its executor never began executing a staged run; terminalizing the run and handing the executor off"
    );
    Err(
        CoreExecutorError::executor_not_progressing_requires_teardown(format!(
            "runtime loop observed run {run_id} with its primitive still un-applied {staged_secs} seconds after staging; the executor never began executing it"
        )),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::lifecycle::core_executor::CoreExecutorTeardownReason;
    use std::sync::atomic::{AtomicU8, AtomicUsize};

    struct ScriptedProgress {
        observations: std::sync::Mutex<Vec<RunExecutionProgress>>,
        cursor: AtomicU8,
    }

    impl ScriptedProgress {
        fn new(observations: Vec<RunExecutionProgress>) -> Arc<Self> {
            Arc::new(Self {
                observations: std::sync::Mutex::new(observations),
                cursor: AtomicU8::new(0),
            })
        }

        fn observation_count(&self) -> u8 {
            self.cursor.load(Ordering::SeqCst)
        }
    }

    impl RunExecutionProgressSource for ScriptedProgress {
        fn observe(&self, _run_id: &RunId) -> RunExecutionProgress {
            let index = usize::from(self.cursor.fetch_add(1, Ordering::SeqCst));
            let observations = self
                .observations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            observations
                .get(index)
                .copied()
                .or_else(|| observations.last().copied())
                .unwrap_or(RunExecutionProgress::Unreadable)
        }
    }

    struct ScriptedExecutor {
        delay: Option<Duration>,
        cancelled: Arc<AtomicBool>,
    }

    impl ScriptedExecutor {
        fn wedged(cancelled: Arc<AtomicBool>) -> Self {
            Self {
                delay: None,
                cancelled,
            }
        }

        fn slow(delay: Duration, cancelled: Arc<AtomicBool>) -> Self {
            Self {
                delay: Some(delay),
                cancelled,
            }
        }
    }

    /// Records whether the `apply` future was dropped before completing, i.e.
    /// whether the supervisor cancelled in-flight work.
    struct CancelWitness {
        cancelled: Arc<AtomicBool>,
        completed: bool,
    }

    impl Drop for CancelWitness {
        fn drop(&mut self) {
            if !self.completed {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    impl CoreExecutor for ScriptedExecutor {
        async fn apply(
            &mut self,
            _run_id: RunId,
            _primitive: RunPrimitive,
        ) -> Result<CoreApplyOutput, CoreExecutorError> {
            let mut witness = CancelWitness {
                cancelled: Arc::clone(&self.cancelled),
                completed: false,
            };
            match self.delay {
                Some(delay) => {
                    crate::tokio::time::sleep(delay).await;
                    witness.completed = true;
                    Err(CoreExecutorError::Internal("scripted completion".into()))
                }
                None => {
                    // The field shape: the consumer owns the run and does
                    // nothing with it, forever.
                    std::future::pending::<()>().await;
                    unreachable!("wedged executor never returns")
                }
            }
        }

        async fn cancel_after_boundary(
            &mut self,
            _reason: String,
        ) -> Result<(), CoreExecutorError> {
            Ok(())
        }

        async fn stop_runtime_executor(
            &mut self,
            _reason: String,
        ) -> Result<(), CoreExecutorError> {
            Ok(())
        }
    }

    fn run_id() -> RunId {
        RunId::new()
    }

    fn staged_primitive() -> RunPrimitive {
        RunPrimitive::StagedInput(meerkat_core::lifecycle::run_primitive::StagedRunInput {
            boundary: meerkat_core::lifecycle::run_primitive::RunApplyBoundary::RunStart,
            appends: Vec::new(),
            contributing_input_ids: Vec::new(),
            turn_metadata: None,
        })
    }

    const TEST_BOUND: Duration = Duration::from_secs(900);

    /// RED without the bound: `apply_with_execution_start_bound` degenerates to
    /// a bare `executor.apply(..).await` against a consumer that never returns,
    /// and the outer timeout is what turns that into a reported failure instead
    /// of an unattributable hang in a lane whose subject is mute hangs.
    #[tokio::test(start_paused = true)]
    async fn wedged_consumer_produces_a_typed_bounded_outcome() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut executor = ScriptedExecutor::wedged(Arc::clone(&cancelled));
        let progress = ScriptedProgress::new(vec![RunExecutionProgress::PrimitiveUnapplied]);
        let run_id = run_id();

        let error = crate::tokio::time::timeout(
            TEST_BOUND * 4,
            apply_with_execution_start_bound(
                &mut executor,
                progress.as_ref(),
                run_id.clone(),
                staged_primitive(),
                Instant::now(),
                TEST_BOUND,
            ),
        )
        .await
        .expect("a consumer that never began executing must not hang mute")
        .expect_err("a consumer that never began executing must produce a typed outcome");

        assert!(
            matches!(
                error,
                CoreExecutorError::TeardownRequired {
                    reason: CoreExecutorTeardownReason::ExecutorNotProgressing,
                    ..
                }
            ),
            "expected a typed ExecutorNotProgressing teardown, got {error:?}"
        );
        assert!(
            error.requires_runtime_teardown(),
            "a wedged consumer must hand its executor off instead of receiving the next batch"
        );
        assert_eq!(
            progress.observation_count(),
            1,
            "escalation must rest on exactly one observation, taken at the bound"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn slow_but_executing_turn_is_not_disturbed_by_the_bound() {
        let cancelled = Arc::new(AtomicBool::new(false));
        // Ten times the bound: the window measures staged -> executing, not
        // how long a live turn is allowed to take.
        let mut executor = ScriptedExecutor::slow(TEST_BOUND * 10, Arc::clone(&cancelled));
        let progress = ScriptedProgress::new(vec![RunExecutionProgress::Executing]);

        let result = apply_with_execution_start_bound(
            &mut executor,
            progress.as_ref(),
            run_id(),
            staged_primitive(),
            Instant::now(),
            TEST_BOUND,
        )
        .await;

        assert!(
            matches!(result, Err(CoreExecutorError::Internal(message)) if message == "scripted completion"),
            "an executing turn must return its own outcome"
        );
        assert!(
            !cancelled.load(Ordering::SeqCst),
            "a live turn must never be cancelled by the execution-start bound"
        );
    }

    /// Time already spent between the `StageForRun` commit and this call is
    /// part of the same window. A run that was staged a full bound ago must
    /// escalate immediately rather than being granted a fresh bound.
    #[tokio::test(start_paused = true)]
    async fn the_bound_is_measured_from_staging_not_from_apply_entry() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut executor = ScriptedExecutor::wedged(Arc::clone(&cancelled));
        let progress = ScriptedProgress::new(vec![RunExecutionProgress::PrimitiveUnapplied]);

        let staged_at = Instant::now();
        crate::tokio::time::sleep(TEST_BOUND).await;
        let elapsed_before = Instant::now();

        let error = crate::tokio::time::timeout(
            TEST_BOUND * 4,
            apply_with_execution_start_bound(
                &mut executor,
                progress.as_ref(),
                run_id(),
                staged_primitive(),
                staged_at,
                TEST_BOUND,
            ),
        )
        .await
        .expect("a run already past its window must not be granted a fresh one")
        .expect_err("a run already past its window must produce a typed outcome");

        assert!(
            matches!(
                error,
                CoreExecutorError::TeardownRequired {
                    reason: CoreExecutorTeardownReason::ExecutorNotProgressing,
                    ..
                }
            ),
            "expected a typed ExecutorNotProgressing teardown, got {error:?}"
        );
        assert!(
            elapsed_before.elapsed() < TEST_BOUND,
            "pre-apply time must count against the window, not extend it"
        );
    }

    /// The deliberately-unbounded case: at the bound, an observation that does
    /// not prove non-progress leaves the run in flight even when the consumer
    /// is in fact wedged. That is the price of refusing to terminalize on
    /// unproven evidence, and it is stated here rather than left implied.
    #[tokio::test(start_paused = true)]
    async fn unprovable_observations_leave_even_a_wedged_run_in_flight() {
        for observed in [
            RunExecutionProgress::Unreadable,
            RunExecutionProgress::RuntimeUnbound,
            RunExecutionProgress::ExecutionStartUnobservable,
            RunExecutionProgress::RunNotCurrent,
            RunExecutionProgress::Executing,
        ] {
            let cancelled = Arc::new(AtomicBool::new(false));
            let mut executor = ScriptedExecutor::wedged(Arc::clone(&cancelled));
            let progress = ScriptedProgress::new(vec![observed]);

            // The outer timeout is the harness, not the code under test: it is
            // what turns "still waiting" into an assertion instead of a hang.
            // It also drops the `apply` future, so `cancelled` says nothing
            // here; the sibling slow-but-live test is what proves the bound
            // itself never cancels.
            let outcome = crate::tokio::time::timeout(
                TEST_BOUND * 4,
                apply_with_execution_start_bound(
                    &mut executor,
                    progress.as_ref(),
                    run_id(),
                    staged_primitive(),
                    Instant::now(),
                    TEST_BOUND,
                ),
            )
            .await;

            assert!(
                outcome.is_err(),
                "{} must refuse to terminalize and keep awaiting its consumer",
                observed.as_str()
            );
        }
    }

    /// A merely-slow consumer must keep its own outcome even when the
    /// observation at the bound is unprovable rather than positively healthy.
    #[tokio::test(start_paused = true)]
    async fn unprovable_observations_do_not_disturb_a_slow_but_live_turn() {
        for observed in [
            RunExecutionProgress::Unreadable,
            RunExecutionProgress::RuntimeUnbound,
            RunExecutionProgress::ExecutionStartUnobservable,
            RunExecutionProgress::RunNotCurrent,
        ] {
            let cancelled = Arc::new(AtomicBool::new(false));
            let mut executor = ScriptedExecutor::slow(TEST_BOUND * 10, Arc::clone(&cancelled));
            let progress = ScriptedProgress::new(vec![observed]);

            let result = apply_with_execution_start_bound(
                &mut executor,
                progress.as_ref(),
                run_id(),
                staged_primitive(),
                Instant::now(),
                TEST_BOUND,
            )
            .await;

            assert!(
                matches!(result, Err(CoreExecutorError::Internal(message)) if message == "scripted completion"),
                "{} must leave the run in flight rather than terminalize it",
                observed.as_str()
            );
            assert!(
                !cancelled.load(Ordering::SeqCst),
                "{} must not cancel an in-flight turn",
                observed.as_str()
            );
        }
    }

    /// The defect this classification closes: with the turn start unsignalled,
    /// `turn_phase` belongs to whatever ran last (or a fresh session's
    /// default), so reading "not ApplyingPrimitive" as `Executing` would hand
    /// a false clean bill of health to exactly the classes that skip the
    /// turn-start transition - the transient-turn-context class and the
    /// retired drain.
    #[test]
    fn an_unsignalled_turn_start_is_unobservable_not_executing() {
        for applying_primitive in [true, false] {
            let facts = RunTurnStateFacts {
                runtime_bound: true,
                run_is_current: true,
                applying_primitive,
            };
            assert_eq!(
                classify_execution_start(facts, TurnStartSignal::NotSignalled),
                RunExecutionProgress::ExecutionStartUnobservable,
                "an unsignalled turn start must never be read as a fact about this run"
            );
        }
    }

    #[test]
    fn a_signalled_turn_start_classifies_the_shared_phase_as_this_run() {
        let base = RunTurnStateFacts {
            runtime_bound: true,
            run_is_current: true,
            applying_primitive: true,
        };
        assert_eq!(
            classify_execution_start(base, TurnStartSignal::Signalled),
            RunExecutionProgress::PrimitiveUnapplied
        );
        assert_eq!(
            classify_execution_start(
                RunTurnStateFacts {
                    applying_primitive: false,
                    ..base
                },
                TurnStartSignal::Signalled
            ),
            RunExecutionProgress::Executing
        );
        assert_eq!(
            classify_execution_start(
                RunTurnStateFacts {
                    run_is_current: false,
                    ..base
                },
                TurnStartSignal::Signalled
            ),
            RunExecutionProgress::RunNotCurrent
        );
        for turn_start in [TurnStartSignal::Signalled, TurnStartSignal::NotSignalled] {
            assert_eq!(
                classify_execution_start(
                    RunTurnStateFacts {
                        runtime_bound: false,
                        ..base
                    },
                    turn_start
                ),
                RunExecutionProgress::RuntimeUnbound,
                "an unbound runtime is unobservable regardless of the turn-start signal"
            );
        }
    }

    #[test]
    fn only_positive_facts_close_the_execution_start_window() {
        for observed in [
            RunExecutionProgress::Executing,
            RunExecutionProgress::RunNotCurrent,
        ] {
            assert!(
                observed.closes_execution_start_window(),
                "{} is a positive fact that closes the window",
                observed.as_str()
            );
        }
        for observed in [
            RunExecutionProgress::PrimitiveUnapplied,
            RunExecutionProgress::Unreadable,
            RunExecutionProgress::RuntimeUnbound,
            RunExecutionProgress::ExecutionStartUnobservable,
        ] {
            assert!(
                !observed.closes_execution_start_window(),
                "{} leaves the staged -> executing window open",
                observed.as_str()
            );
        }
    }

    #[test]
    fn only_a_proven_unapplied_primitive_may_terminalize() {
        assert!(RunExecutionProgress::PrimitiveUnapplied.proves_execution_never_started());
        for observed in [
            RunExecutionProgress::Executing,
            RunExecutionProgress::RunNotCurrent,
            RunExecutionProgress::RuntimeUnbound,
            RunExecutionProgress::ExecutionStartUnobservable,
            RunExecutionProgress::Unreadable,
        ] {
            assert!(
                !observed.proves_execution_never_started(),
                "{} is not proof that execution never started",
                observed.as_str()
            );
        }
    }

    struct CountingProgress {
        observation: RunExecutionProgress,
        observations: AtomicUsize,
    }

    impl RunExecutionProgressSource for CountingProgress {
        fn observe(&self, _run_id: &RunId) -> RunExecutionProgress {
            self.observations.fetch_add(1, Ordering::SeqCst);
            self.observation
        }
    }

    /// The watchdog is the half that covers the pre-`apply` segment, so it must
    /// keep reporting for as long as the window stays open and it must never
    /// terminalize anything. `Unreadable` is the shape a consumer wedged while
    /// holding the authority mutex produces, and
    /// `ExecutionStartUnobservable` is the class the bound cannot arm for at
    /// all - both are exactly when an operator most needs the line.
    #[tokio::test(start_paused = true)]
    async fn the_watchdog_keeps_reporting_an_open_window() {
        for observation in [
            RunExecutionProgress::PrimitiveUnapplied,
            RunExecutionProgress::Unreadable,
            RunExecutionProgress::ExecutionStartUnobservable,
            RunExecutionProgress::RuntimeUnbound,
        ] {
            let progress = Arc::new(CountingProgress {
                observation,
                observations: AtomicUsize::new(0),
            });
            let watchdog = StagedRunStartWatchdog::spawn(
                Arc::clone(&progress) as Arc<dyn RunExecutionProgressSource>,
                run_id(),
                vec![InputId::new()],
                Instant::now(),
                Duration::from_secs(120),
            );

            crate::tokio::time::sleep(Duration::from_secs(500)).await;
            drop(watchdog);
            let reported = progress.observations.load(Ordering::SeqCst);
            assert!(
                reported >= 4,
                "{} leaves the window open and must be re-reported every notice interval, got {reported}",
                observation.as_str()
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_watchdog_stands_down_once_the_run_is_executing() {
        let progress = Arc::new(CountingProgress {
            observation: RunExecutionProgress::Executing,
            observations: AtomicUsize::new(0),
        });
        let watchdog = StagedRunStartWatchdog::spawn(
            Arc::clone(&progress) as Arc<dyn RunExecutionProgressSource>,
            run_id(),
            vec![InputId::new()],
            Instant::now(),
            Duration::from_secs(120),
        );

        crate::tokio::time::sleep(Duration::from_secs(500)).await;
        drop(watchdog);
        assert_eq!(
            progress.observations.load(Ordering::SeqCst),
            1,
            "a run that began executing must stop being supervised after one observation"
        );
    }
}
