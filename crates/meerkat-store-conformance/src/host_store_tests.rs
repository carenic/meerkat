//! Tests of the host store harness, unrun until the validation candidate:
//! the reference store passes the host store chapter, stores that break an
//! obligation fail it at the row that names the obligation, and the long
//! run chapters accept a Core that resumes as the contract says and refuse
//! one that calls the model again for a committed round.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::host_store::{
    AcquireMode, CommitBatch, CommitId, FenceGrant, FenceTiming, FenceToken, HeadPage, HostStore,
    HostStoreError, Row, RowIndex, ScopeKind, ScopeName, ScopeRef, ScopeUpdate, ScopeVersion,
    VersionedHead,
};
use meerkat_core::{RunId, RunResumePoint, ToolCallDedupeKey};

use crate::long_run::{
    BeforeResume, DurableWorkerHarness, KillMode, KillPoint, KilledRun, KilledWorker,
    LongRunHarness, LongRunPlan, RestartReport, RestartedWorker, ResumeGate, ResumedRun, call_id,
};
use crate::{ConformanceFailure, HostStoreFactory, InMemoryHostStore, chapters};

/// Hands out handles over one store: one realm that outlives its processes.
struct Shared {
    store: Arc<dyn HostStore>,
    memory: Arc<InMemoryHostStore>,
}

impl Shared {
    fn reference(memory: InMemoryHostStore) -> Self {
        let memory = Arc::new(memory);
        Self {
            store: Arc::clone(&memory) as Arc<dyn HostStore>,
            memory,
        }
    }

    fn canary(memory: Arc<InMemoryHostStore>, store: Arc<dyn HostStore>) -> Self {
        Self { store, memory }
    }
}

#[async_trait]
impl HostStoreFactory for Shared {
    async fn open(&self) -> Result<Arc<dyn HostStore>, ConformanceFailure> {
        Ok(Arc::clone(&self.store))
    }

    async fn lapse_fence_leases(&self) -> Result<(), ConformanceFailure> {
        self.memory.lapse_fence_leases();
        Ok(())
    }
}

/// Returns `Pending` once, waking itself, so a canary's commit stops between
/// two scopes the way a remote store's commit stops between two requests.
#[derive(Default)]
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// The canary stores: each forwards to the reference store and breaks one
/// obligation in its commit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Breaks {
    /// Commits every batch under the newest token: no fencing.
    Fencing,
    /// Applies a batch one scope at a time, stopping between scopes: not
    /// all or nothing.
    Atomicity,
    /// Moves each scope from its current version, whatever the batch says:
    /// two batches for one version both land.
    VersionChain,
}

struct Canary {
    inner: Arc<InMemoryHostStore>,
    breaks: Breaks,
    newest: AtomicU64,
}

impl Canary {
    fn factory(breaks: Breaks) -> Shared {
        let inner = Arc::new(InMemoryHostStore::new());
        let canary = Arc::new(Self {
            inner: Arc::clone(&inner),
            breaks,
            newest: AtomicU64::new(0),
        });
        Shared::canary(inner, canary)
    }
}

#[async_trait]
impl HostStore for Canary {
    async fn acquire_fence(&self, mode: AcquireMode) -> Result<FenceGrant, HostStoreError> {
        let grant = self.inner.acquire_fence(mode).await?;
        self.newest.fetch_max(grant.token.get(), Ordering::SeqCst);
        Ok(grant)
    }

    async fn renew_fence(&self, token: FenceToken) -> Result<(), HostStoreError> {
        self.inner.renew_fence(token).await
    }

    fn fence_timing(&self) -> FenceTiming {
        self.inner.fence_timing()
    }

    async fn load_head(&self, scope: &ScopeRef) -> Result<Option<VersionedHead>, HostStoreError> {
        self.inner.load_head(scope).await
    }

    async fn list_heads(
        &self,
        kind: ScopeKind,
        after: Option<&ScopeName>,
        limit: usize,
    ) -> Result<HeadPage, HostStoreError> {
        self.inner.list_heads(kind, after, limit).await
    }

    async fn read_rows(
        &self,
        scope: &ScopeRef,
        from: RowIndex,
        limit: usize,
    ) -> Result<Vec<Row>, HostStoreError> {
        self.inner.read_rows(scope, from, limit).await
    }

    async fn commit(&self, batch: &CommitBatch) -> Result<(), HostStoreError> {
        match self.breaks {
            Breaks::Fencing => {
                let newest = FenceToken::new(self.newest.load(Ordering::SeqCst));
                let rebatched = CommitBatch::try_new(
                    batch.commit_id().clone(),
                    newest,
                    batch.updates().to_vec(),
                )?;
                self.inner.commit(&rebatched).await
            }
            Breaks::Atomicity => {
                for index in 0..batch.updates().len() {
                    if index > 0 {
                        YieldOnce::default().await;
                    }
                    self.inner.apply_one_update(batch, index)?;
                }
                Ok(())
            }
            Breaks::VersionChain => {
                let mut updates = Vec::with_capacity(batch.updates().len());
                for update in batch.updates() {
                    let current = self
                        .inner
                        .load_head(&update.scope)
                        .await?
                        .map(|head| head.version);
                    let next = current
                        .map_or(Some(ScopeVersion::FIRST), ScopeVersion::next)
                        .ok_or_else(|| HostStoreError::Invalid("version overflow".to_string()))?;
                    updates.push(ScopeUpdate {
                        previous: current,
                        next,
                        ..update.clone()
                    });
                }
                let rebatched =
                    CommitBatch::try_new(batch.commit_id().clone(), batch.fence(), updates)?;
                self.inner.commit(&rebatched).await
            }
        }
    }
}

#[tokio::test]
async fn reference_host_store_passes_the_host_store_chapter() {
    chapters::host_store(&Shared::reference(InMemoryHostStore::new()))
        .await
        .expect("the reference host store must satisfy the host store chapter");
}

#[tokio::test]
async fn reference_host_store_passes_with_a_host_sized_lease() {
    let timing = FenceTiming::try_new(Duration::from_secs(2), Duration::from_secs(6))
        .expect("a two second interval with a six second lease is valid");
    chapters::host_store(&Shared::reference(InMemoryHostStore::with_fence_timing(
        timing,
    )))
    .await
    .expect("the reference host store must satisfy the chapter with a host sized lease");
}

#[tokio::test]
async fn fence_blind_store_fails_the_host_store_chapter() {
    let failure = chapters::host_store(&Canary::factory(Breaks::Fencing))
        .await
        .expect_err("a store that ignores fence tokens must fail");
    assert_eq!(failure.chapter(), "host_store");
    assert!(
        ["kill", "stale_fence", "takeover_unknown_commit"].contains(&failure.step()),
        "a fence blind store failed at {}",
        failure.step()
    );
}

#[tokio::test]
async fn store_splitting_a_batch_fails_the_host_store_chapter() {
    let failure = chapters::host_store(&Canary::factory(Breaks::Atomicity))
        .await
        .expect_err("a store that applies a batch one scope at a time must fail");
    assert_eq!(failure.chapter(), "host_store");
    assert_eq!(failure.step(), "kill");
}

#[tokio::test]
async fn store_landing_two_batches_for_one_version_fails_the_host_store_chapter() {
    let failure = chapters::host_store(&Canary::factory(Breaks::VersionChain))
        .await
        .expect_err("a store that lands two batches for one version must fail");
    assert_eq!(failure.chapter(), "host_store");
    assert_eq!(failure.step(), "same_version_twice");
}

fn scope(name: &str) -> ScopeRef {
    ScopeRef::new(
        ScopeKind::Session,
        ScopeName::try_new(name).expect("valid name"),
    )
}

fn create(scope: &ScopeRef, rows: &[&str]) -> ScopeUpdate {
    ScopeUpdate {
        scope: scope.clone(),
        previous: None,
        next: ScopeVersion::FIRST,
        head: Some(b"head".to_vec()),
        append: rows
            .iter()
            .map(|row| Row::try_new(row.as_bytes().to_vec()).expect("small row"))
            .collect(),
        truncate_after: None,
        drop_before: None,
        delete: false,
    }
}

fn batch(id: &str, token: FenceToken, updates: Vec<ScopeUpdate>) -> CommitBatch {
    CommitBatch::try_new(CommitId::try_new(id).expect("valid id"), token, updates)
        .expect("valid batch")
}

#[tokio::test]
async fn reference_host_store_leases() {
    let store = InMemoryHostStore::new();
    let first = store
        .acquire_fence(AcquireMode::Exclusive)
        .await
        .expect("an empty realm grants");
    assert_eq!(first.token, FenceToken::new(1));
    assert_eq!(first.superseded_lease, Duration::ZERO);

    let lease = FenceTiming::default().lease();
    assert_eq!(
        store.acquire_fence(AcquireMode::Exclusive).await,
        Err(HostStoreError::Held {
            holder: first.token,
            remaining: lease,
        })
    );
    store.advance_clock(lease - Duration::from_secs(1));
    assert_eq!(
        store.acquire_fence(AcquireMode::Exclusive).await,
        Err(HostStoreError::Held {
            holder: first.token,
            remaining: Duration::from_secs(1),
        })
    );
    store
        .renew_fence(first.token)
        .await
        .expect("the holder renews");
    let second = store
        .acquire_fence(AcquireMode::Takeover)
        .await
        .expect("a takeover always grants");
    assert_eq!(second.token, FenceToken::new(2));
    assert_eq!(second.superseded_lease, lease);
    assert_eq!(
        store.renew_fence(first.token).await,
        Err(HostStoreError::FencedOut {
            current: second.token
        })
    );
    store.lapse_fence_leases();
    let third = store
        .acquire_fence(AcquireMode::Exclusive)
        .await
        .expect("a lapsed lease does not hold the fence");
    assert_eq!(third.token, FenceToken::new(3));
    assert_eq!(third.superseded_lease, Duration::ZERO);
    assert!(matches!(
        store.renew_fence(FenceToken::new(9)).await,
        Err(HostStoreError::Invalid(_))
    ));
}

#[tokio::test]
async fn reference_host_store_leases_follow_the_host_timing() {
    let timing =
        FenceTiming::try_new(Duration::from_secs(2), Duration::from_secs(6)).expect("valid timing");
    let store = InMemoryHostStore::with_fence_timing(timing);
    assert_eq!(store.fence_timing(), timing);
    let grant = store
        .acquire_fence(AcquireMode::Exclusive)
        .await
        .expect("an empty realm grants");
    assert_eq!(
        store.acquire_fence(AcquireMode::Exclusive).await,
        Err(HostStoreError::Held {
            holder: grant.token,
            remaining: Duration::from_secs(6),
        })
    );
}

#[tokio::test]
async fn reference_host_store_reads_an_outside_clock() {
    let now = Arc::new(Mutex::new(Duration::ZERO));
    let read = Arc::clone(&now);
    let store = InMemoryHostStore::with_clock(FenceTiming::default(), move || {
        *read.lock().unwrap_or_else(PoisonError::into_inner)
    });
    store
        .acquire_fence(AcquireMode::Exclusive)
        .await
        .expect("an empty realm grants");
    *now.lock().unwrap_or_else(PoisonError::into_inner) = FenceTiming::default().lease();
    let grant = store
        .acquire_fence(AcquireMode::Exclusive)
        .await
        .expect("the lease ran out on the outside clock");
    assert_eq!(grant.superseded_lease, Duration::ZERO);
}

#[tokio::test]
async fn reference_host_store_refuses_writer_rule_breaks() {
    let store = InMemoryHostStore::new();
    let token = store
        .acquire_fence(AcquireMode::Exclusive)
        .await
        .expect("an empty realm grants")
        .token;
    let s = scope("s");
    store
        .commit(&batch("c1", token, vec![create(&s, &["r0", "r1"])]))
        .await
        .expect("a create commits");

    // A landed commit id answers Ok again, and is refused with other updates.
    store
        .commit(&batch("c1", token, vec![create(&s, &["r0", "r1"])]))
        .await
        .expect("a retry of a landed batch answers Ok");
    assert!(matches!(
        store
            .commit(&batch("c1", token, vec![create(&s, &["other"])]))
            .await,
        Err(HostStoreError::Invalid(_))
    ));
    assert_eq!(
        store
            .read_rows(&s, RowIndex::new(0), 10)
            .await
            .expect("rows read")
            .len(),
        2
    );

    // A stale previous version is a typed conflict.
    let stale = ScopeUpdate {
        previous: None,
        ..create(&s, &[])
    };
    assert_eq!(
        store.commit(&batch("c2", token, vec![stale])).await,
        Err(HostStoreError::Conflict {
            scope: s.clone(),
            current: Some(ScopeVersion::FIRST),
        })
    );

    // A truncate below the first visible row is refused, and nothing of
    // its batch applies.
    let drop_first_row = ScopeUpdate {
        scope: s.clone(),
        previous: Some(ScopeVersion::FIRST),
        next: ScopeVersion::new(2),
        head: None,
        append: Vec::new(),
        truncate_after: None,
        drop_before: Some(RowIndex::new(1)),
        delete: false,
    };
    store
        .commit(&batch("c3", token, vec![drop_first_row]))
        .await
        .expect("a drop commits");
    let below = ScopeUpdate {
        scope: s.clone(),
        previous: Some(ScopeVersion::new(2)),
        next: ScopeVersion::new(3),
        head: Some(b"moved".to_vec()),
        append: Vec::new(),
        truncate_after: Some(RowIndex::new(0)),
        drop_before: None,
        delete: false,
    };
    assert!(matches!(
        store.commit(&batch("c4", token, vec![below])).await,
        Err(HostStoreError::Invalid(_))
    ));
    let head = store
        .load_head(&s)
        .await
        .expect("head loads")
        .expect("scope exists");
    assert_eq!(head.version, ScopeVersion::new(2));
    assert_eq!(head.head, b"head".to_vec());

    // A deleted name is never created again.
    let delete = ScopeUpdate {
        scope: s.clone(),
        previous: Some(ScopeVersion::new(2)),
        next: ScopeVersion::new(3),
        head: None,
        append: Vec::new(),
        truncate_after: None,
        drop_before: None,
        delete: true,
    };
    store
        .commit(&batch("c5", token, vec![delete]))
        .await
        .expect("a delete commits");
    assert_eq!(store.load_head(&s).await, Ok(None));
    assert!(matches!(
        store
            .commit(&batch("c6", token, vec![create(&s, &[])]))
            .await,
        Err(HostStoreError::Invalid(_))
    ));
}

#[test]
fn fence_timing_bounds() {
    let timing = FenceTiming::default();
    assert_eq!(timing.renew_interval(), Duration::from_secs(10));
    assert_eq!(timing.lease(), Duration::from_secs(30));
    assert_eq!(timing.self_fence_window(), Duration::from_secs(20));
    let secs = Duration::from_secs;
    assert!(FenceTiming::try_new(Duration::from_millis(500), secs(3)).is_err());
    assert!(FenceTiming::try_new(secs(2), secs(5)).is_err());
    assert!(FenceTiming::try_new(secs(10), secs(2 * 60 * 60)).is_err());
    let host = FenceTiming::try_new(secs(2), secs(6)).expect("three intervals is enough");
    assert_eq!(host.self_fence_window(), secs(4));
}

/// A Core that resumes exactly as the contract says, or (`recalls_model`)
/// one that calls the model again for a round whose calls were committed.
struct ScriptedCore {
    recalls_model: bool,
    kill_mode: KillMode,
    fence_leases: bool,
    run_id: Mutex<Option<RunId>>,
}

impl ScriptedCore {
    fn new(recalls_model: bool) -> Self {
        Self {
            recalls_model,
            kill_mode: KillMode::DropAndReopen,
            fence_leases: true,
            run_id: Mutex::new(None),
        }
    }

    fn calls(plan: &LongRunPlan, round: u64) -> Vec<String> {
        if round + 1 >= plan.rounds {
            return Vec::new();
        }
        (0..plan.calls_per_round)
            .map(|index| call_id(round, index))
            .collect()
    }

    fn key(run_id: &RunId, round: u64, call: String) -> ToolCallDedupeKey {
        ToolCallDedupeKey {
            run_id: run_id.clone(),
            round,
            call_id: call,
        }
    }

    fn kill(&self, plan: &LongRunPlan) -> KilledRun {
        let run_id = RunId::new();
        *self.run_id.lock().unwrap_or_else(PoisonError::into_inner) = Some(run_id.clone());
        let mut dispatched: Vec<ToolCallDedupeKey> = (0..plan.kill_round)
            .flat_map(|round| {
                Self::calls(plan, round)
                    .into_iter()
                    .map(move |call| (round, call))
            })
            .map(|(round, call)| Self::key(&run_id, round, call))
            .collect();
        if matches!(
            plan.kill_point,
            KillPoint::DuringTools | KillPoint::DuringProcessTool
        ) {
            dispatched.push(Self::key(
                &run_id,
                plan.kill_round,
                call_id(plan.kill_round, 0),
            ));
        }
        KilledRun { run_id, dispatched }
    }

    fn restart_report(&self, plan: &LongRunPlan) -> RestartReport {
        let run_id = self
            .run_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_else(RunId::new);
        let waited_out_lease = self.kill_mode == KillMode::DropAndReopen && self.fence_leases;
        if matches!(
            plan.before_resume,
            BeforeResume::Stop | BeforeResume::Retire
        ) {
            return RestartReport {
                resumed: None,
                waited_out_lease,
                runs: Vec::new(),
                model_call_rounds: Vec::new(),
                dispatched: Vec::new(),
                interrupted: Vec::new(),
                transcript_rounds: Vec::new(),
                transcript_results: Vec::new(),
                receipts: 0,
                inputs_settled: true,
            };
        }
        let kill_round = plan.kill_round;
        let model_call = plan.kill_point == KillPoint::ModelCall;
        let process =
            (plan.kill_point == KillPoint::DuringProcessTool).then(|| call_id(kill_round, 0));
        let first_model_round = if model_call || self.recalls_model {
            kill_round
        } else {
            kill_round + 1
        };
        let mut runs = vec![run_id.clone()];
        if plan.before_resume == BeforeResume::QueueInput {
            runs.push(RunId::new());
        }
        let dispatched = (kill_round..plan.rounds)
            .flat_map(|round| {
                Self::calls(plan, round)
                    .into_iter()
                    .map(move |call| (round, call))
            })
            .filter(|(_, call)| process.as_ref() != Some(call))
            .map(|(round, call)| Self::key(&run_id, round, call))
            .collect();
        let first_transcript_round = if plan.compact_in_kill_round {
            kill_round
        } else {
            0
        };
        let transcript_rounds: Vec<u64> = (first_transcript_round..plan.rounds).collect();
        let transcript_results = transcript_rounds
            .iter()
            .flat_map(|round| Self::calls(plan, *round))
            .collect();
        RestartReport {
            resumed: Some(ResumedRun {
                run_id,
                resume_point: if model_call {
                    RunResumePoint::AwaitingModel
                } else {
                    RunResumePoint::ToolsPending {
                        call_ids: Self::calls(plan, kill_round),
                    }
                },
            }),
            waited_out_lease,
            runs,
            model_call_rounds: (first_model_round..plan.rounds).collect(),
            dispatched,
            interrupted: process.into_iter().collect(),
            transcript_rounds,
            transcript_results,
            receipts: 1,
            inputs_settled: true,
        }
    }
}

#[async_trait]
impl LongRunHarness for ScriptedCore {
    fn kill_mode(&self) -> KillMode {
        self.kill_mode
    }

    fn fence_leases(&self) -> bool {
        self.fence_leases
    }

    async fn run_and_kill(&self, plan: &LongRunPlan) -> Result<KilledRun, ConformanceFailure> {
        Ok(self.kill(plan))
    }

    async fn restart(&self, plan: &LongRunPlan) -> Result<RestartReport, ConformanceFailure> {
        Ok(self.restart_report(plan))
    }
}

#[async_trait]
impl DurableWorkerHarness for ScriptedCore {
    fn kill_mode(&self) -> KillMode {
        self.kill_mode
    }

    fn fence_leases(&self) -> bool {
        self.fence_leases
    }

    async fn spawn_and_kill(&self, plan: &LongRunPlan) -> Result<KilledWorker, ConformanceFailure> {
        Ok(KilledWorker {
            identity: "worker".to_string(),
            session: "worker-session".to_string(),
            run: self.kill(plan),
        })
    }

    async fn restart(
        &self,
        plan: &LongRunPlan,
        gate: ResumeGate,
    ) -> Result<RestartedWorker, ConformanceFailure> {
        let superseded = gate == ResumeGate::Superseded;
        let report = if superseded {
            self.restart_report(&LongRunPlan {
                before_resume: BeforeResume::Stop,
                ..*plan
            })
        } else {
            self.restart_report(plan)
        };
        Ok(RestartedWorker {
            identity: "worker".to_string(),
            session: "worker-session".to_string(),
            superseded,
            report,
        })
    }
}

#[tokio::test]
async fn long_run_chapter_accepts_a_core_that_resumes_as_the_contract_says() {
    chapters::long_run(&ScriptedCore::new(false))
        .await
        .expect("a contract Core passes the long run chapter");
    chapters::durable_worker(&ScriptedCore::new(false))
        .await
        .expect("a contract Core passes the durable worker chapter");
}

#[tokio::test]
async fn long_run_chapter_refuses_a_core_that_calls_the_model_again() {
    let failure = chapters::long_run(&ScriptedCore::new(true))
        .await
        .expect_err("calling the model again for a committed round must fail");
    assert_eq!(failure.chapter(), "long_run");
    assert_eq!(failure.step(), "first_round_during_tools");
}
