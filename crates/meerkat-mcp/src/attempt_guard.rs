//! MCP attempt guards: a host-installed guard around each stdio MCP server
//! connect attempt.
//!
//! A host builds one [`McpAttemptGuards`] installation around its
//! [`McpAttemptGuardFactory`] and passes it to every router it builds
//! ([`crate::McpRouter::with_attempt_guards`]), with the owner those routers
//! act for. For every stdio server attempt, before secret resolution, the
//! attachment control's physical entry or any spawn, the router asks the
//! factory to begin a guard for that exact attempt ([`McpAttemptKey`]):
//!
//! - a refusal refuses the attempt, with nothing spawned and no guard;
//! - `Ok(None)` lets the attempt proceed unguarded;
//! - a guard is finished exactly once, by one async call, once both facts of
//!   its attempt are known: [`McpAttemptGuard::settle`] when the attempt's
//!   connection was installed, [`McpAttemptGuard::cancel`] for every other
//!   outcome. The [`McpAttemptDisposition`] it receives keeps the attempt's
//!   open outcome ([`McpAttemptOpenOutcome`]) apart from how its process
//!   ended ([`McpAttemptProcessEnd`]).
//!
//! A begin is never dropped. When the attempt stops waiting for it (its
//! bound, a router stop or drop, the attempt's task dropped, or the
//! installation closing), the attempt's [`McpAttemptCancellation`] is
//! requested with its cause and the attempt fails with nothing spawned; a
//! guard the begin returns later is cancelled in the begin's own task.
//!
//! Before the next begin for the same owner (mob member, else session) and
//! server, core awaits, bounded, every running begin and every started
//! finish for that pair; it never waits for a guard whose process is still
//! alive. A prior finish still running past the settle bound refuses the
//! new attempt ([`McpAttemptRefusal::PriorGuardUnsettled`]). Router shutdown
//! awaits the router's own begins and finishes, bounded, and reports them
//! ([`crate::McpRouterShutdownReport::guards`]); the host's
//! [`McpAttemptGuards::close_and_join`] awaits the rest, those of dropped
//! routers included, and must run before the tokio runtime shuts down.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use futures::FutureExt as _;
use meerkat_core::McpServerConfig;
use meerkat_core::MobMemberBinding;
use meerkat_core::mcp_attachment::{McpAttachmentRefusal, McpStdioSetupFailure};
use meerkat_core::types::SessionId;

use crate::McpStdioCloseReceipt;

static NEXT_CONSTRUCTION: AtomicU64 = AtomicU64::new(1);

/// The identity of one router construction: unique per [`crate::McpRouter`]
/// in this process. A rebuilt or reset member gets a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct McpConstructionId(u64);

impl McpConstructionId {
    pub(crate) fn allocate() -> Self {
        Self(NEXT_CONSTRUCTION.fetch_add(1, Ordering::Relaxed))
    }

    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Who one router construction acts for: its session, and the mob member
/// it is built for, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct McpConstructionOwner {
    pub session_id: SessionId,
    pub member: Option<MobMemberBinding>,
}

impl McpConstructionOwner {
    #[must_use]
    pub fn new(session_id: SessionId, member: Option<MobMemberBinding>) -> Self {
        Self { session_id, member }
    }
}

/// One exact attempt: its router construction, its server and the router's
/// sequence for it (the same number as
/// [`McpStdioSetupFailure::attempt`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct McpAttemptKey {
    pub construction: McpConstructionId,
    pub server: String,
    pub attempt: u64,
}

/// What the attempt connects for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpAttemptOperation {
    Add,
    Reload,
}

/// What a guard is asked to begin for. Owned, so a begin may outlive the
/// attempt. `server` is the staged descriptor: it never carries a resolved
/// secret (secrets are resolved later, inside the attempt).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct McpAttemptGuardRequest {
    pub key: McpAttemptKey,
    pub owner: Arc<McpConstructionOwner>,
    pub server: McpServerConfig,
    pub operation: McpAttemptOperation,
}

/// Why an attempt stopped waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpAttemptCancelCause {
    /// The begin did not answer within its bound.
    BeginBound,
    /// The router began its shutdown.
    RouterClosed,
    /// The attempt's own task was dropped.
    AttemptDropped,
    /// The router was dropped without a shutdown.
    RouterDropped,
    /// The host closed the installation ([`McpAttemptGuards::close_and_join`]).
    InstallationClosed,
}

/// One attempt's cancellation: requested once, with its first cause.
#[derive(Clone)]
pub struct McpAttemptCancellation {
    inner: Arc<CancellationInner>,
}

struct CancellationInner {
    token: tokio_util::sync::CancellationToken,
    cause: OnceLock<McpAttemptCancelCause>,
}

impl std::fmt::Debug for McpAttemptCancellation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAttemptCancellation")
            .field("cause", &self.cause())
            .finish()
    }
}

impl McpAttemptCancellation {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(CancellationInner {
                token: tokio_util::sync::CancellationToken::new(),
                cause: OnceLock::new(),
            }),
        }
    }

    /// Request the cancellation; the first cause stands.
    pub(crate) fn request(&self, cause: McpAttemptCancelCause) {
        let _ = self.inner.cause.set(cause);
        self.inner.token.cancel();
    }

    /// The cause, once requested.
    #[must_use]
    pub fn cause(&self) -> Option<McpAttemptCancelCause> {
        self.inner.cause.get().copied()
    }

    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.inner.token.is_cancelled()
    }

    /// Resolves once requested, with its cause.
    pub async fn requested(&self) -> McpAttemptCancelCause {
        self.inner.token.cancelled().await;
        self.cause()
            .unwrap_or(McpAttemptCancelCause::AttemptDropped)
    }

    fn downgrade(&self) -> Weak<CancellationInner> {
        Arc::downgrade(&self.inner)
    }

    fn upgrade(weak: &Weak<CancellationInner>) -> Option<Self> {
        weak.upgrade().map(|inner| Self { inner })
    }
}

/// A host's guard factory, asked once per stdio attempt before anything of
/// it runs. `Err` refuses the attempt with nothing spawned; `Ok(None)` lets
/// it proceed unguarded (no finish is ever called for it). `cancel` is
/// requested when the attempt stops waiting; the begin is never dropped,
/// and a guard it returns after that is cancelled at once.
///
/// Begin is called only for stdio attempts: remote (HTTP and SSE) attempts
/// are neither guarded nor refused by the factory.
#[async_trait::async_trait]
pub trait McpAttemptGuardFactory: Send + Sync {
    async fn begin(
        &self,
        request: McpAttemptGuardRequest,
        cancel: McpAttemptCancellation,
    ) -> Result<Option<Box<dyn McpAttemptGuard>>, McpAttachmentRefusal>;
}

/// One begun attempt's guard. Exactly one of its methods is called, exactly
/// once: `settle` when the attempt's connection was installed, `cancel` for
/// every other outcome, once both the open outcome and the process end are
/// known.
#[async_trait::async_trait]
pub trait McpAttemptGuard: Send {
    async fn settle(
        self: Box<Self>,
        disposition: McpAttemptDisposition,
    ) -> Result<(), McpGuardFinishError>;

    async fn cancel(
        self: Box<Self>,
        disposition: McpAttemptDisposition,
    ) -> Result<(), McpGuardFinishError>;
}

/// A guard's own failure to finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{detail}")]
#[non_exhaustive]
pub struct McpGuardFinishError {
    pub detail: String,
}

impl McpGuardFinishError {
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// How one attempt ended: whether it opened (separately from) how its
/// process ended.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct McpAttemptDisposition {
    pub key: McpAttemptKey,
    pub open: McpAttemptOpenOutcome,
    pub process: McpAttemptProcessEnd,
}

/// Whether the attempt's connection opened and was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpAttemptOpenOutcome {
    /// The router installed the attempt's connection.
    Installed { connection_generation: u64 },
    /// The setup failed; `failure` is the typed stdio setup failure the
    /// transport established, when it established one.
    SetupFailed {
        failure: Option<McpStdioSetupFailure>,
    },
    /// The attempt was refused before it could open.
    Refused { refusal: McpAttemptRefusal },
    /// The connection opened but was not installed.
    NotInstalled { reason: McpAttemptNotInstalled },
    /// The attempt stopped: see the cause.
    Cancelled { cause: McpAttemptCancelCause },
}

/// Why an opened connection was not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpAttemptNotInstalled {
    /// A newer intent for the server replaced the attempt.
    Superseded,
    /// The router refused to install it.
    InstallRefused,
}

/// Who refused the attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpAttemptRefusal {
    /// The guard factory.
    Guard,
    /// The attachment control at the physical entry.
    Control,
    /// Process confinement.
    Confinement,
    /// The service-secret resolver.
    ServiceSecret,
    /// The server's configuration.
    InvalidConfig,
    /// A prior guard of the same owner and server was still finishing past
    /// the settle bound.
    PriorGuardUnsettled,
}

/// How the attempt's process ended.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum McpAttemptProcessEnd {
    /// No process was spawned.
    NotSpawned,
    /// Its custody closed it; the receipt is the close's own.
    Closed(Arc<McpStdioCloseReceipt>),
    /// It was killed without its exit being observed (its custody or close
    /// was dropped).
    Abandoned,
}

/// Which call a completion record is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpGuardStage {
    Begin,
    Settle,
    Cancel,
}

/// How a call ended, as far as it was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpGuardCompletion {
    /// It returned (a begin with a guard, none or a refusal; a finish with
    /// `Ok`).
    Completed,
    /// A finish returned `Err`.
    Failed,
    /// It was still running when the bounded wait for it ended.
    TimedOut,
    /// It panicked.
    Panicked,
    /// A begin was not called (the installation was closed, or a prior
    /// guard was unsettled), or it returned after its attempt was cancelled.
    Cancelled,
    /// The guard's process was still alive, so its finish had not started.
    Held,
}

/// One observed guard call.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct McpGuardCompletionRecord {
    pub key: McpAttemptKey,
    pub stage: McpGuardStage,
    pub disposition: Option<McpAttemptDisposition>,
    pub completion: McpGuardCompletion,
}

/// The bounds of guard calls: how long an attempt waits for its begin, and
/// how long core waits for a cancel or a settle (and for prior guards, at
/// the settle bound). A bound limits the wait, never the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpAttemptGuardBounds {
    begin: Duration,
    cancel: Duration,
    settle: Duration,
}

/// A refused [`McpAttemptGuardBounds`] value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("MCP attempt guard {bound} bound of {value:?} exceeds the {max:?} maximum")]
pub struct McpAttemptGuardBoundsError {
    /// `"begin"`, `"cancel"` or `"settle"`.
    pub bound: &'static str,
    pub value: Duration,
    pub max: Duration,
}

impl McpAttemptGuardBounds {
    /// The largest bound accepted.
    pub const MAX: Duration = Duration::from_secs(60);
    /// 10 s to begin, 5 s to cancel, 5 s to settle.
    pub const DEFAULT: Self = Self {
        begin: Duration::from_secs(10),
        cancel: Duration::from_secs(5),
        settle: Duration::from_secs(5),
    };

    /// Bounds with these durations. A bound above [`Self::MAX`] is refused,
    /// not clamped.
    pub fn new(
        begin: Duration,
        cancel: Duration,
        settle: Duration,
    ) -> Result<Self, McpAttemptGuardBoundsError> {
        for (bound, value) in [("begin", begin), ("cancel", cancel), ("settle", settle)] {
            if value > Self::MAX {
                return Err(McpAttemptGuardBoundsError {
                    bound,
                    value,
                    max: Self::MAX,
                });
            }
        }
        Ok(Self {
            begin,
            cancel,
            settle,
        })
    }

    #[must_use]
    pub fn begin(&self) -> Duration {
        self.begin
    }

    #[must_use]
    pub fn cancel(&self) -> Duration {
        self.cancel
    }

    #[must_use]
    pub fn settle(&self) -> Duration {
        self.settle
    }
}

impl Default for McpAttemptGuardBounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A host-wide installation of one guard factory, shared by every router it
/// is passed to ([`crate::McpRouter::with_attempt_guards`]). Cloning shares
/// it. It tracks every begin and finish it started, those of dropped routers
/// included, until [`Self::close_and_join`] reports them (at most 1024
/// completed ones are kept; the oldest beyond that are forgotten).
///
/// Only stdio attempts are guarded: begin is called only for them. Remote
/// (HTTP and SSE) attempts are neither guarded nor refused by it.
#[derive(Clone)]
pub struct McpAttemptGuards(Arc<Installation>);

impl std::fmt::Debug for McpAttemptGuards {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAttemptGuards")
            .field("bounds", &self.0.bounds)
            .finish_non_exhaustive()
    }
}

/// How many completed calls an installation keeps for
/// [`McpAttemptGuards::close_and_join`]; the oldest beyond it are forgotten.
const MAX_RETAINED_COMPLETED: usize = 1024;

struct Installation {
    factory: Arc<dyn McpAttemptGuardFactory>,
    bounds: McpAttemptGuardBounds,
    state: Mutex<InstallationState>,
}

#[derive(Default)]
struct InstallationState {
    closed: bool,
    routers: Vec<Weak<RouterGuards>>,
    tasks: Vec<Arc<GuardTask>>,
    records: Vec<Weak<AttemptGuardRecord>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl McpAttemptGuards {
    /// An installation of `factory` under `bounds`.
    #[must_use]
    pub fn new(factory: Arc<dyn McpAttemptGuardFactory>, bounds: McpAttemptGuardBounds) -> Self {
        Self(Arc::new(Installation {
            factory,
            bounds,
            state: Mutex::new(InstallationState::default()),
        }))
    }

    #[must_use]
    pub fn bounds(&self) -> McpAttemptGuardBounds {
        self.0.bounds
    }

    /// Close the installation and join what it started, by `deadline`: new
    /// begins are refused, running begins are cancelled
    /// ([`McpAttemptCancelCause::InstallationClosed`]), and every tracked
    /// begin and finish (those of dropped routers, and those a router
    /// shutdown already reported, included) is awaited and reported. A call
    /// still running at `deadline` is reported
    /// [`McpGuardCompletion::TimedOut`] (it is not dropped); a guard whose
    /// process is still alive is reported [`McpGuardCompletion::Held`].
    /// Completed calls are then forgotten. Run it before the tokio runtime
    /// shuts down: calls spawned on a runtime that has shut down never run.
    pub async fn close_and_join(
        &self,
        deadline: tokio::time::Instant,
    ) -> Vec<McpGuardCompletionRecord> {
        let routers = {
            let mut state = lock(&self.0.state);
            state.closed = true;
            state.routers.clone()
        };
        for router in routers.iter().filter_map(Weak::upgrade) {
            router.close(McpAttemptCancelCause::InstallationClosed);
        }
        let mut reported = Vec::new();
        let begins = self.0.tracked(|task| task.stage == McpGuardStage::Begin);
        reported.extend(join_tasks(&begins, deadline).await);
        let finishes = self.0.tracked(|task| task.stage != McpGuardStage::Begin);
        reported.extend(join_tasks(&finishes, deadline).await);
        reported.extend(self.0.held(|_| true));
        self.0.forget_completed();
        reported
    }
}

impl Installation {
    fn track(&self, task: &Arc<GuardTask>) {
        let mut state = lock(&self.state);
        state.tasks.push(Arc::clone(task));
        // Completed calls of dropped routers are kept for close_and_join, up
        // to a bound.
        let completed = state
            .tasks
            .iter()
            .filter(|task| task.completion().is_some())
            .count();
        if completed > MAX_RETAINED_COMPLETED {
            let mut excess = completed - MAX_RETAINED_COMPLETED;
            state.tasks.retain(|task| {
                if excess > 0 && task.completion().is_some() {
                    excess -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    /// Every tracked call that `select`s.
    fn tracked(&self, select: impl Fn(&GuardTask) -> bool) -> Vec<Arc<GuardTask>> {
        lock(&self.state)
            .tasks
            .iter()
            .filter(|task| select(task))
            .cloned()
            .collect()
    }

    /// The guards still held (their finish not started) whose key `select`s.
    fn held(&self, select: impl Fn(&McpAttemptKey) -> bool) -> Vec<McpGuardCompletionRecord> {
        let records: Vec<Arc<AttemptGuardRecord>> = {
            let mut state = lock(&self.state);
            state.records.retain(|record| record.strong_count() > 0);
            state.records.iter().filter_map(Weak::upgrade).collect()
        };
        records
            .iter()
            .filter(|record| select(&record.key))
            .filter_map(|record| record.held())
            .collect()
    }

    /// Forget every completed call.
    fn forget_completed(&self) {
        lock(&self.state)
            .tasks
            .retain(|task| task.completion().is_none());
    }

    /// Wait, at most `bound`, for every running begin and started finish of
    /// `scope`; `false` if one is still running then.
    async fn await_prior(&self, scope: &GuardScope, bound: Duration) -> bool {
        let running: Vec<Arc<GuardTask>> = lock(&self.state)
            .tasks
            .iter()
            .filter(|task| &task.scope == scope && task.completion().is_none())
            .cloned()
            .collect();
        let deadline = tokio::time::Instant::now() + bound;
        for task in running {
            if task.wait_until(deadline).await.is_none() {
                return false;
            }
        }
        true
    }

    /// A call that was never made: its record is kept, completed at once.
    fn note_not_called(
        &self,
        key: McpAttemptKey,
        scope: GuardScope,
        disposition: McpAttemptDisposition,
    ) {
        let task = GuardTask::new(key, scope, McpGuardStage::Begin, Some(disposition));
        task.complete(McpGuardCompletion::Cancelled);
        self.track(&task);
    }

    /// Run one finish of `guard` (settle when the attempt was installed,
    /// cancel otherwise) on `runtime`, tracked.
    fn spawn_finish(
        self: &Arc<Self>,
        runtime: &tokio::runtime::Handle,
        guard: Box<dyn McpAttemptGuard>,
        scope: GuardScope,
        disposition: McpAttemptDisposition,
    ) {
        let task = finish_task(&scope, &disposition);
        self.track(&task);
        runtime.spawn(async move {
            let completion = run_finish(guard, disposition).await;
            task.complete(completion);
        });
    }
}

fn finish_task(scope: &GuardScope, disposition: &McpAttemptDisposition) -> Arc<GuardTask> {
    GuardTask::new(
        disposition.key.clone(),
        scope.clone(),
        finish_stage(&disposition.open),
        Some(disposition.clone()),
    )
}

fn finish_stage(open: &McpAttemptOpenOutcome) -> McpGuardStage {
    if matches!(open, McpAttemptOpenOutcome::Installed { .. }) {
        McpGuardStage::Settle
    } else {
        McpGuardStage::Cancel
    }
}

/// The one finish call: settle for an installed attempt, cancel for any
/// other.
async fn run_finish(
    guard: Box<dyn McpAttemptGuard>,
    disposition: McpAttemptDisposition,
) -> McpGuardCompletion {
    let call = match finish_stage(&disposition.open) {
        McpGuardStage::Settle => guard.settle(disposition),
        McpGuardStage::Begin | McpGuardStage::Cancel => guard.cancel(disposition),
    };
    match AssertUnwindSafe(call).catch_unwind().await {
        Ok(Ok(())) => McpGuardCompletion::Completed,
        Ok(Err(_)) => McpGuardCompletion::Failed,
        Err(_) => McpGuardCompletion::Panicked,
    }
}

/// Await `tasks` until `deadline` and record each: its completion, or
/// `TimedOut` while it still runs (it is not dropped, and a later join
/// reports its completion).
async fn join_tasks(
    tasks: &[Arc<GuardTask>],
    deadline: tokio::time::Instant,
) -> Vec<McpGuardCompletionRecord> {
    let mut records = Vec::with_capacity(tasks.len());
    for task in tasks {
        let completion = task.wait_until(deadline).await;
        records.push(task.record(completion.unwrap_or(McpGuardCompletion::TimedOut)));
    }
    records
}

/// The owner and server whose guards are awaited before a new begin: the
/// mob member when there is one (so a rebuild or reset of that member
/// waits), else the session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GuardScope {
    Member {
        mob_id: String,
        role: String,
        member: String,
        server: String,
    },
    Session {
        session_id: SessionId,
        server: String,
    },
}

impl GuardScope {
    fn of(owner: &McpConstructionOwner, server: &str) -> Self {
        match &owner.member {
            Some(member) => Self::Member {
                mob_id: member.mob_id.clone(),
                role: member.role.clone(),
                member: member.member.clone(),
                server: server.to_owned(),
            },
            None => Self::Session {
                session_id: owner.session_id.clone(),
                server: server.to_owned(),
            },
        }
    }
}

/// One tracked guard call.
struct GuardTask {
    key: McpAttemptKey,
    scope: GuardScope,
    stage: McpGuardStage,
    disposition: Option<McpAttemptDisposition>,
    done: tokio::sync::watch::Sender<Option<McpGuardCompletion>>,
}

impl GuardTask {
    fn new(
        key: McpAttemptKey,
        scope: GuardScope,
        stage: McpGuardStage,
        disposition: Option<McpAttemptDisposition>,
    ) -> Arc<Self> {
        Arc::new(Self {
            key,
            scope,
            stage,
            disposition,
            done: tokio::sync::watch::channel(None).0,
        })
    }

    fn complete(&self, completion: McpGuardCompletion) {
        self.done.send_replace(Some(completion));
    }

    fn completion(&self) -> Option<McpGuardCompletion> {
        *self.done.borrow()
    }

    async fn wait_until(&self, deadline: tokio::time::Instant) -> Option<McpGuardCompletion> {
        let mut done = self.done.subscribe();
        match tokio::time::timeout_at(deadline, done.wait_for(Option::is_some)).await {
            Ok(Ok(completion)) => *completion,
            Ok(Err(_)) | Err(_) => self.completion(),
        }
    }

    fn record(&self, completion: McpGuardCompletion) -> McpGuardCompletionRecord {
        McpGuardCompletionRecord {
            key: self.key.clone(),
            stage: self.stage,
            disposition: self.disposition.clone(),
            completion,
        }
    }
}

/// One router's part of an installation: its construction, its owner, and
/// the cancellations and records of its attempts.
pub(crate) struct RouterGuards {
    installation: Arc<Installation>,
    construction: McpConstructionId,
    owner: Arc<McpConstructionOwner>,
    /// Set once the router closed (or was dropped, or its installation
    /// closed): every later attempt is cancelled with it.
    closed: OnceLock<McpAttemptCancelCause>,
    attempts: Mutex<Vec<Weak<CancellationInner>>>,
    records: Mutex<Vec<Weak<AttemptGuardRecord>>>,
}

impl RouterGuards {
    pub(crate) fn new(
        guards: &McpAttemptGuards,
        construction: McpConstructionId,
        owner: McpConstructionOwner,
    ) -> Arc<Self> {
        let router = Arc::new(Self {
            installation: Arc::clone(&guards.0),
            construction,
            owner: Arc::new(owner),
            closed: OnceLock::new(),
            attempts: Mutex::new(Vec::new()),
            records: Mutex::new(Vec::new()),
        });
        let installation_closed = {
            let mut state = lock(&guards.0.state);
            state.routers.retain(|router| router.strong_count() > 0);
            state.routers.push(Arc::downgrade(&router));
            state.closed
        };
        if installation_closed {
            router.close(McpAttemptCancelCause::InstallationClosed);
        }
        router
    }

    /// Cancel every running attempt of this router with `cause`, and give
    /// every record whose open outcome is still unknown `Cancelled{cause}`.
    /// The first cause stands for later attempts too.
    pub(crate) fn close(&self, cause: McpAttemptCancelCause) {
        let cause = *self.closed.get_or_init(|| cause);
        let attempts: Vec<McpAttemptCancellation> = {
            let mut attempts = lock(&self.attempts);
            attempts.retain(|attempt| attempt.strong_count() > 0);
            attempts
                .iter()
                .filter_map(McpAttemptCancellation::upgrade)
                .collect()
        };
        for attempt in attempts {
            attempt.request(cause);
        }
        let records: Vec<Arc<AttemptGuardRecord>> = {
            let mut records = lock(&self.records);
            records.retain(|record| record.strong_count() > 0);
            records.iter().filter_map(Weak::upgrade).collect()
        };
        for record in records {
            record.open(McpAttemptOpenOutcome::Cancelled { cause });
        }
    }

    fn closed_cause(&self) -> Option<McpAttemptCancelCause> {
        self.closed.get().copied().or_else(|| {
            self.installation
                .is_closed()
                .then_some(McpAttemptCancelCause::InstallationClosed)
        })
    }

    /// Await this router's begins (by the cancel bound) and then its
    /// finishes (by the settle bound), and report them with the guards it
    /// still holds.
    pub(crate) async fn join(&self) -> Vec<McpGuardCompletionRecord> {
        let construction = self.construction;
        let bounds = self.installation.bounds;
        let mine = |task: &GuardTask| task.key.construction == construction;
        let begins = self
            .installation
            .tracked(|task| mine(task) && task.stage == McpGuardStage::Begin);
        let mut records = join_tasks(&begins, tokio::time::Instant::now() + bounds.cancel()).await;
        let finishes = self
            .installation
            .tracked(|task| mine(task) && task.stage != McpGuardStage::Begin);
        records.extend(join_tasks(&finishes, tokio::time::Instant::now() + bounds.settle()).await);
        records.extend(
            self.installation
                .held(|key| key.construction == construction),
        );
        records
    }

    /// Begin the guard of one stdio attempt, before anything of it runs.
    /// `Ok(Some)` carries its record (the attempt's custody holds it), and
    /// `Ok(None)` lets the attempt proceed unguarded. An `Err` refuses the
    /// attempt with nothing spawned: the factory's refusal, or
    /// `Unavailable` when the attempt stopped waiting (it is cancelled) or a
    /// prior guard of its owner and server was still finishing.
    pub(crate) async fn begin_attempt(
        self: &Arc<Self>,
        server: &McpServerConfig,
        attempt: u64,
        operation: McpAttemptOperation,
    ) -> Result<Option<Arc<AttemptGuardRecord>>, crate::McpError> {
        let key = McpAttemptKey {
            construction: self.construction,
            server: server.name.clone(),
            attempt,
        };
        let scope = GuardScope::of(&self.owner, &server.name);
        let installation = &self.installation;
        let bounds = installation.bounds;
        if let Some(cause) = self.closed_cause() {
            installation.note_not_called(
                key.clone(),
                scope,
                McpAttemptDisposition {
                    key,
                    open: McpAttemptOpenOutcome::Cancelled { cause },
                    process: McpAttemptProcessEnd::NotSpawned,
                },
            );
            return Err(unavailable());
        }
        if !installation.await_prior(&scope, bounds.settle()).await {
            installation.note_not_called(
                key.clone(),
                scope,
                McpAttemptDisposition {
                    key,
                    open: McpAttemptOpenOutcome::Refused {
                        refusal: McpAttemptRefusal::PriorGuardUnsettled,
                    },
                    process: McpAttemptProcessEnd::NotSpawned,
                },
            );
            return Err(unavailable());
        }
        let cancel = McpAttemptCancellation::new();
        self.register_attempt(&cancel);
        let runtime = tokio::runtime::Handle::current();
        let (handoff, delivered) = tokio::sync::oneshot::channel();
        spawn_begin(
            installation,
            &runtime,
            McpAttemptGuardRequest {
                key: key.clone(),
                owner: Arc::clone(&self.owner),
                server: server.clone(),
                operation,
            },
            cancel.clone(),
            handoff,
            scope.clone(),
        );
        let mut waiting = BeginWait {
            delivered: Some(delivered),
            cancel: cancel.clone(),
            installation: Arc::clone(installation),
            runtime: runtime.clone(),
            key: key.clone(),
            scope: scope.clone(),
        };
        let deadline = tokio::time::Instant::now() + bounds.begin();
        let answer = tokio::select! {
            biased;
            answer = waiting.delivered() => Some(answer),
            _ = cancel.requested() => None,
            () = tokio::time::sleep_until(deadline) => {
                cancel.request(McpAttemptCancelCause::BeginBound);
                None
            }
        };
        let Some(answer) = answer else {
            // Stopped waiting: a guard delivered meanwhile is cancelled.
            waiting.give_up();
            return Err(unavailable());
        };
        waiting.disarm();
        if cancel.is_requested() {
            // Answered, but the attempt was cancelled meanwhile: the guard is
            // late, and the attempt stops.
            if let Ok(Ok(Some(guard))) = answer {
                installation.spawn_finish(
                    &runtime,
                    guard,
                    scope,
                    McpAttemptDisposition {
                        key,
                        open: McpAttemptOpenOutcome::Cancelled {
                            cause: cancel
                                .cause()
                                .unwrap_or(McpAttemptCancelCause::AttemptDropped),
                        },
                        process: McpAttemptProcessEnd::NotSpawned,
                    },
                );
            }
            return Err(unavailable());
        }
        match answer {
            Ok(Ok(Some(guard))) => {
                let record = Arc::new(AttemptGuardRecord {
                    key,
                    scope,
                    installation: Arc::clone(installation),
                    runtime,
                    state: Mutex::new(RecordState {
                        guard: Some(guard),
                        open: None,
                        process: None,
                    }),
                });
                self.register_record(&record);
                Ok(Some(record))
            }
            Ok(Ok(None)) => Ok(None),
            Ok(Err(refusal)) => Err(crate::McpError::AttachmentRefused(refusal)),
            // The begin ended without an answer (it panicked).
            Err(_) => Err(unavailable()),
        }
    }

    fn register_attempt(&self, cancel: &McpAttemptCancellation) {
        let closed = {
            let mut attempts = lock(&self.attempts);
            attempts.retain(|attempt| attempt.strong_count() > 0);
            attempts.push(cancel.downgrade());
            self.closed_cause()
        };
        if let Some(cause) = closed {
            cancel.request(cause);
        }
    }

    fn register_record(&self, record: &Arc<AttemptGuardRecord>) {
        let closed = {
            let mut records = lock(&self.records);
            records.retain(|record| record.strong_count() > 0);
            records.push(Arc::downgrade(record));
            self.closed_cause()
        };
        {
            let mut state = lock(&self.installation.state);
            state.records.retain(|record| record.strong_count() > 0);
            state.records.push(Arc::downgrade(record));
        }
        if let Some(cause) = closed {
            record.open(McpAttemptOpenOutcome::Cancelled { cause });
        }
    }
}

fn unavailable() -> crate::McpError {
    crate::McpError::AttachmentRefused(McpAttachmentRefusal::Unavailable)
}

/// Whether an attempt of `config` is guarded: only stdio attempts are.
pub(crate) fn guards_transport(config: &McpServerConfig) -> bool {
    matches!(
        config.transport,
        meerkat_core::mcp_config::McpTransportConfig::Stdio(_)
    )
}

/// The guard operation of a surface completion obligation.
pub(crate) fn attempt_operation(
    operation: meerkat_core::ExternalToolSurfaceDeltaOperation,
) -> McpAttemptOperation {
    match operation {
        meerkat_core::ExternalToolSurfaceDeltaOperation::Reload => McpAttemptOperation::Reload,
        _ => McpAttemptOperation::Add,
    }
}

/// The open outcome of a failed attempt: who refused it, or its setup
/// failure with the attempt's own classified stdio failure, if any.
pub(crate) fn open_for_error(
    error: &crate::McpError,
    failure: Option<McpStdioSetupFailure>,
) -> McpAttemptOpenOutcome {
    use crate::McpError as E;
    let refused = |refusal| McpAttemptOpenOutcome::Refused { refusal };
    match error {
        E::AttachmentRefused(_) => refused(McpAttemptRefusal::Control),
        E::Confinement(_) => refused(McpAttemptRefusal::Confinement),
        E::ServiceSecretRefused { .. } => refused(McpAttemptRefusal::ServiceSecret),
        E::InvalidAccessPolicy(_) | E::InvalidSecretEnv(_) => {
            refused(McpAttemptRefusal::InvalidConfig)
        }
        E::RouterShutDown => McpAttemptOpenOutcome::Cancelled {
            cause: McpAttemptCancelCause::RouterClosed,
        },
        _ => McpAttemptOpenOutcome::SetupFailed { failure },
    }
}

type BeginAnswer = Result<Option<Box<dyn McpAttemptGuard>>, McpAttachmentRefusal>;

/// Run one begin on `runtime`, tracked and never dropped. Its answer goes to
/// the waiting attempt; when the attempt is gone, a guard it returns is
/// cancelled here, in the begin's own task, before the begin is complete.
fn spawn_begin(
    installation: &Arc<Installation>,
    runtime: &tokio::runtime::Handle,
    request: McpAttemptGuardRequest,
    cancel: McpAttemptCancellation,
    handoff: tokio::sync::oneshot::Sender<BeginAnswer>,
    scope: GuardScope,
) {
    let key = request.key.clone();
    let task = GuardTask::new(key.clone(), scope.clone(), McpGuardStage::Begin, None);
    installation.track(&task);
    let factory = Arc::clone(&installation.factory);
    let installation = Arc::clone(installation);
    runtime.spawn(async move {
        let began = AssertUnwindSafe(factory.begin(request, cancel.clone()))
            .catch_unwind()
            .await;
        let completion = match began {
            Err(_) => McpGuardCompletion::Panicked,
            Ok(answer) => match handoff.send(answer) {
                Ok(()) if cancel.is_requested() => McpGuardCompletion::Cancelled,
                Ok(()) => McpGuardCompletion::Completed,
                Err(answer) => {
                    if let Ok(Some(guard)) = answer {
                        let disposition = McpAttemptDisposition {
                            key,
                            open: McpAttemptOpenOutcome::Cancelled {
                                cause: cancel
                                    .cause()
                                    .unwrap_or(McpAttemptCancelCause::AttemptDropped),
                            },
                            process: McpAttemptProcessEnd::NotSpawned,
                        };
                        let late = finish_task(&scope, &disposition);
                        installation.track(&late);
                        late.complete(run_finish(guard, disposition).await);
                    }
                    McpGuardCompletion::Cancelled
                }
            },
        };
        task.complete(completion);
    });
}

/// An attempt waiting for its begin. Dropped while still waiting (its task
/// dropped), it cancels the attempt and cancels a guard already delivered.
struct BeginWait {
    delivered: Option<tokio::sync::oneshot::Receiver<BeginAnswer>>,
    cancel: McpAttemptCancellation,
    installation: Arc<Installation>,
    runtime: tokio::runtime::Handle,
    key: McpAttemptKey,
    scope: GuardScope,
}

impl BeginWait {
    async fn delivered(&mut self) -> Result<BeginAnswer, tokio::sync::oneshot::error::RecvError> {
        match self.delivered.as_mut() {
            Some(delivered) => delivered.await,
            None => std::future::pending().await,
        }
    }

    /// The answer was taken: nothing is left to rescue.
    fn disarm(&mut self) {
        self.delivered = None;
    }

    /// Stop waiting: no later answer can be delivered, and a guard already
    /// delivered is cancelled.
    fn give_up(&mut self) {
        let Some(mut delivered) = self.delivered.take() else {
            return;
        };
        delivered.close();
        if let Ok(Ok(Some(guard))) = delivered.try_recv() {
            let disposition = McpAttemptDisposition {
                key: self.key.clone(),
                open: McpAttemptOpenOutcome::Cancelled {
                    cause: self
                        .cancel
                        .cause()
                        .unwrap_or(McpAttemptCancelCause::AttemptDropped),
                },
                process: McpAttemptProcessEnd::NotSpawned,
            };
            self.installation
                .spawn_finish(&self.runtime, guard, self.scope.clone(), disposition);
        }
    }
}

impl Drop for BeginWait {
    fn drop(&mut self) {
        if self.delivered.is_some() {
            self.cancel.request(McpAttemptCancelCause::AttemptDropped);
            self.give_up();
        }
    }
}

/// The guard of one begun attempt, with its two facts. Its finish starts
/// once both are known; dropped first, it fills the unknown ones
/// (`Cancelled{RouterDropped}`, `Abandoned`) and starts it then.
pub(crate) struct AttemptGuardRecord {
    key: McpAttemptKey,
    scope: GuardScope,
    installation: Arc<Installation>,
    runtime: tokio::runtime::Handle,
    state: Mutex<RecordState>,
}

struct RecordState {
    guard: Option<Box<dyn McpAttemptGuard>>,
    open: Option<McpAttemptOpenOutcome>,
    process: Option<McpAttemptProcessEnd>,
}

impl std::fmt::Debug for AttemptGuardRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = lock(&self.state);
        f.debug_struct("AttemptGuardRecord")
            .field("key", &self.key)
            .field("open", &state.open)
            .field("finished", &state.guard.is_none())
            .finish_non_exhaustive()
    }
}

impl AttemptGuardRecord {
    /// The attempt's open outcome; the first one stands.
    pub(crate) fn open(&self, outcome: McpAttemptOpenOutcome) {
        let mut state = lock(&self.state);
        if state.open.is_none() {
            state.open = Some(outcome);
        }
        self.finish_if_known(state);
    }

    /// How the attempt's process ended; the first one stands.
    pub(crate) fn process_ended(&self, end: McpAttemptProcessEnd) {
        let mut state = lock(&self.state);
        if state.process.is_none() {
            state.process = Some(end);
        }
        self.finish_if_known(state);
    }

    fn finish_if_known(&self, mut state: std::sync::MutexGuard<'_, RecordState>) {
        let (Some(open), Some(process)) = (state.open.clone(), state.process.clone()) else {
            return;
        };
        let Some(guard) = state.guard.take() else {
            return;
        };
        drop(state);
        self.installation.spawn_finish(
            &self.runtime,
            guard,
            self.scope.clone(),
            McpAttemptDisposition {
                key: self.key.clone(),
                open,
                process,
            },
        );
    }

    /// A record of this guard while it is still held (its finish not
    /// started).
    fn held(&self) -> Option<McpGuardCompletionRecord> {
        let state = lock(&self.state);
        state.guard.as_ref()?;
        let stage = state
            .open
            .as_ref()
            .map_or(McpGuardStage::Cancel, finish_stage);
        Some(McpGuardCompletionRecord {
            key: self.key.clone(),
            stage,
            disposition: None,
            completion: McpGuardCompletion::Held,
        })
    }
}

impl Drop for AttemptGuardRecord {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(guard) = state.guard.take() else {
            return;
        };
        let disposition = McpAttemptDisposition {
            key: self.key.clone(),
            open: state
                .open
                .take()
                .unwrap_or(McpAttemptOpenOutcome::Cancelled {
                    cause: McpAttemptCancelCause::RouterDropped,
                }),
            process: state
                .process
                .take()
                .unwrap_or(McpAttemptProcessEnd::Abandoned),
        };
        self.installation
            .spawn_finish(&self.runtime, guard, self.scope.clone(), disposition);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// The one predicate that decides which attempts are guarded: stdio
    /// only.
    #[test]
    fn only_stdio_attempts_are_guarded() {
        let stdio = McpServerConfig::stdio(
            "local",
            "/bin/true",
            vec![],
            std::collections::HashMap::new(),
        );
        let http = McpServerConfig::streamable_http(
            "remote",
            "https://example.com/mcp",
            std::collections::HashMap::new(),
        );
        let sse = McpServerConfig::sse(
            "events",
            "https://example.com/sse",
            std::collections::HashMap::new(),
        );
        assert!(guards_transport(&stdio));
        assert!(!guards_transport(&http));
        assert!(!guards_transport(&sse));
    }

    /// Every typed refusal maps to its refuser; anything else is a setup
    /// failure carrying the attempt's own failure.
    #[test]
    fn errors_map_to_their_open_outcome() {
        use crate::McpError as E;
        let invalid =
            McpServerConfig::stdio("bad", "/bin/true", vec![], std::collections::HashMap::new());
        let mut invalid = invalid;
        if let meerkat_core::mcp_config::McpTransportConfig::Stdio(stdio) = &mut invalid.transport {
            stdio.secret_env.insert("1BAD".to_string());
        }
        let secret_env = invalid.validate_secret_env().unwrap_err();
        let cases = [
            (
                E::AttachmentRefused(McpAttachmentRefusal::Denied),
                McpAttemptOpenOutcome::Refused {
                    refusal: McpAttemptRefusal::Control,
                },
            ),
            (
                E::Confinement(
                    meerkat_core::confinement::ConfinementRefusal::UnsupportedRequirement,
                ),
                McpAttemptOpenOutcome::Refused {
                    refusal: McpAttemptRefusal::Confinement,
                },
            ),
            (
                E::InvalidSecretEnv(secret_env),
                McpAttemptOpenOutcome::Refused {
                    refusal: McpAttemptRefusal::InvalidConfig,
                },
            ),
            (
                E::RouterShutDown,
                McpAttemptOpenOutcome::Cancelled {
                    cause: McpAttemptCancelCause::RouterClosed,
                },
            ),
            (
                E::ConnectionFailed {
                    reason: "closed".into(),
                },
                McpAttemptOpenOutcome::SetupFailed { failure: None },
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(open_for_error(&error, None), expected, "{error}");
        }
    }
}
