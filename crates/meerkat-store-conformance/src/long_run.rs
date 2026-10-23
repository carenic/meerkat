//! The Core long run harness: what the long run and durable worker chapters
//! need from a Core build over one store.
//!
//! The chapters drive real Core (the agent loop, the runtime and recovery)
//! through a harness that the Core test suite implements: over the built-in
//! SQLite store with a child process killed by a signal, and over the
//! reference host store with every Core object dropped and Core reopened
//! over the same store instance.
//!
//! The harness scripts the model. A run of `rounds` rounds counts them from
//! zero, as [`ToolCallDedupeKey::round`] does. Every round but the last makes
//! `calls_per_round` tool calls, call `i` of round `r` with the id
//! [`call_id`]`(r, i)`; the last round answers without tool calls. Each round
//! makes two commits: the first holds the assistant message with its tool
//! calls, the second their results.

use async_trait::async_trait;
use meerkat_core::{RunId, RunResumePoint, ToolCallDedupeKey};

use crate::failure::ConformanceFailure;

/// The tool call id the scripted model gives call `index` of round `round`.
#[must_use]
pub fn call_id(round: u64, index: u32) -> String {
    format!("call-{round}-{index}")
}

/// Where in its round the killed process stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillPoint {
    /// During the round's model call. The round has committed nothing, and
    /// the round before it made both its commits.
    ModelCall,
    /// After the round's first commit and before any of its calls is
    /// dispatched.
    BeforeDispatch,
    /// While the round's calls run: some may have finished, and the round's
    /// second commit has not been made.
    DuringTools,
    /// While call 0 of the round, a process tool, runs: its built-in process
    /// custody shows a started process, and the round's second commit has
    /// not been made.
    DuringProcessTool,
}

/// How a harness kills its process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillMode {
    /// A child process is killed by a signal, and the operating system
    /// releases what it held at once.
    CrossProcess,
    /// Every Core object is dropped and Core reopens over the same store in
    /// the same process: the store outlives the "process".
    DropAndReopen,
}

/// What happens after the restart and before the killed run resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeforeResume {
    /// Nothing.
    Nothing,
    /// Another input for the same session is queued. The resumed run still
    /// goes first.
    QueueInput,
    /// The session's runtime is stopped. The run's inputs are settled and
    /// the run never resumes.
    Stop,
    /// The session's runtime is retired. As for a stop.
    Retire,
}

/// One long run row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LongRunPlan {
    /// Rounds of the run.
    pub rounds: u64,
    /// Tool calls of each round but the last.
    pub calls_per_round: u32,
    /// The round the kill lands in.
    pub kill_round: u64,
    /// Where in that round.
    pub kill_point: KillPoint,
    /// Compact the transcript in the kill round, before its model call.
    pub compact_in_kill_round: bool,
    /// What happens between the restart and the resume.
    pub before_resume: BeforeResume,
}

/// What the killed process did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KilledRun {
    /// The run that was killed.
    pub run_id: RunId,
    /// The calls the killed process dispatched, each with the key its tool
    /// saw, in dispatch order.
    pub dispatched: Vec<ToolCallDedupeKey>,
}

/// The run resumed event of a restarted process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumedRun {
    /// The run that resumed.
    pub run_id: RunId,
    /// Where it resumed.
    pub resume_point: RunResumePoint,
}

/// What the restarted process did, from the restart until its session was
/// idle again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartReport {
    /// The run resumed event, if the restarted process resumed a run.
    pub resumed: Option<ResumedRun>,
    /// Whether the restarted process waited out a live fence lease before
    /// it resumed or ran anything.
    pub waited_out_lease: bool,
    /// The runs the restarted process resumed or started, in order.
    pub runs: Vec<RunId>,
    /// The rounds of the killed run for which the restarted process called
    /// the model, in order.
    pub model_call_rounds: Vec<u64>,
    /// The calls of the killed run that the restarted process dispatched,
    /// each with the key its tool saw, in dispatch order.
    pub dispatched: Vec<ToolCallDedupeKey>,
    /// The calls of the killed run that got the typed interrupted result
    /// instead of a dispatch.
    pub interrupted: Vec<String>,
    /// The round of every assistant message of the killed run in the
    /// session's final transcript, in order.
    pub transcript_rounds: Vec<u64>,
    /// The call id of every tool result of the killed run in the session's
    /// final transcript, in order.
    pub transcript_results: Vec<String>,
    /// The run receipts the store holds for the killed run.
    pub receipts: usize,
    /// Whether every input bound to the killed run is terminal in the store.
    pub inputs_settled: bool,
}

/// A Core build over one store, for the long run chapter.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait LongRunHarness: Send + Sync {
    /// How this harness kills its process.
    fn kill_mode(&self) -> KillMode;

    /// Whether the store under this harness records fence leases (a host
    /// store). Then a drop and reopen waits out the dropped process's lease,
    /// while a cross process kill reclaims at once.
    fn fence_leases(&self) -> bool;

    /// Open a fresh session over a fresh store, run one input as `plan`
    /// scripts it, and kill the process at the plan's kill point.
    async fn run_and_kill(&self, plan: &LongRunPlan) -> Result<KilledRun, ConformanceFailure>;

    /// Restart over the same store, do the plan's `before_resume`, and run
    /// until the session is idle.
    async fn restart(&self, plan: &LongRunPlan) -> Result<RestartReport, ConformanceFailure>;
}

/// What a durable worker's resume gate answers at the restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeGate {
    /// No gate is installed, and the resume proceeds.
    Absent,
    /// The gate answers continue.
    Continue,
    /// The gate answers superseded: the work item moved.
    Superseded,
}

/// The durable worker the killed process ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KilledWorker {
    /// The worker's durable identity, as its mob names it.
    pub identity: String,
    /// The worker's session.
    pub session: String,
    /// Its run.
    pub run: KilledRun,
}

/// The durable worker after the restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartedWorker {
    /// The worker's durable identity, as its mob names it.
    pub identity: String,
    /// The worker's session.
    pub session: String,
    /// Whether the worker's run stopped typed as superseded.
    pub superseded: bool,
    /// What the restarted process did for the worker's session.
    pub report: RestartReport,
}

/// A Core build over one store, for the durable worker chapter.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait DurableWorkerHarness: Send + Sync {
    /// How this harness kills its process.
    fn kill_mode(&self) -> KillMode;

    /// Whether the store under this harness records fence leases.
    fn fence_leases(&self) -> bool;

    /// Open a fresh mob over a fresh store, spawn a durable worker (a
    /// profile with `durable` set), give it one input that runs as `plan`
    /// scripts it, and kill the process at the plan's kill point.
    async fn spawn_and_kill(&self, plan: &LongRunPlan) -> Result<KilledWorker, ConformanceFailure>;

    /// Restart over the same store with `gate` as the durable resume gate,
    /// and run until the mob is idle.
    async fn restart(
        &self,
        plan: &LongRunPlan,
        gate: ResumeGate,
    ) -> Result<RestartedWorker, ConformanceFailure>;
}
