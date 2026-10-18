#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::SessionId;
use meerkat_jobs::{
    AttemptClaim, AttemptWriteAuthority, CanonicalArgumentsHash, DetachedJobService,
    DetachedJobStore, ExecutionIntentId, InsertJobOutcome, InteractionLineageId, JobId,
    JobNotification, JobOutboxEntry, JobOutboxPayload, JobSpec, JobSubmissionKey,
    JobTerminalResult, MemoryDetachedJobStore, PredicateComparison,
    PredicateDeliveryIdempotencyKey, PredicateDeliveryIdentity, PredicateDeliveryOutcome,
    PredicateEvaluation, PredicateObservation, PredicatePollingPolicy, PredicateSource,
    PredicateWatch, PredicateWatchId, RestartClass, RunnerHandleRef, RunnerIdentity, ScheduleIdRef,
    StoredJob, ToolIdentity, WorkerId,
};
use tokio::sync::Notify;

fn monitor_spec(key: &str) -> JobSpec {
    JobSpec::new(
        "realm-a",
        SessionId::new(),
        ExecutionIntentId::new(),
        InteractionLineageId::new(),
        ToolIdentity::new("monitor", "v1").expect("tool"),
        RunnerIdentity::new("meerkat.monitor", "v1").expect("runner"),
        RestartClass::CheckpointResumable,
        CanonicalArgumentsHash::new(format!("sha256:{key}")).expect("hash"),
        JobSubmissionKey::new(key).expect("key"),
    )
}

#[derive(Debug)]
struct PausingDeliveryAckStore {
    inner: Arc<MemoryDetachedJobStore>,
    pause_once: AtomicBool,
    ack_cas_entered: Notify,
    release_ack_cas: Notify,
}

impl PausingDeliveryAckStore {
    fn new(inner: Arc<MemoryDetachedJobStore>) -> Self {
        Self {
            inner,
            pause_once: AtomicBool::new(true),
            ack_cas_entered: Notify::new(),
            release_ack_cas: Notify::new(),
        }
    }
}

#[async_trait]
impl DetachedJobStore for PausingDeliveryAckStore {
    async fn insert_deduplicated(
        &self,
        job: StoredJob,
    ) -> Result<InsertJobOutcome, meerkat_jobs::DetachedJobError> {
        self.inner.insert_deduplicated(job).await
    }

    async fn get(
        &self,
        job_id: &JobId,
    ) -> Result<Option<StoredJob>, meerkat_jobs::DetachedJobError> {
        self.inner.get(job_id).await
    }

    async fn compare_and_swap(
        &self,
        expected_revision: u64,
        replacement: StoredJob,
    ) -> Result<StoredJob, meerkat_jobs::DetachedJobError> {
        if replacement.outbox.iter().any(|entry| entry.applied)
            && self.pause_once.swap(false, Ordering::AcqRel)
        {
            self.ack_cas_entered.notify_one();
            self.release_ack_cas.notified().await;
        }
        self.inner
            .compare_and_swap(expected_revision, replacement)
            .await
    }

    async fn list_pending_outbox(
        &self,
        limit: usize,
    ) -> Result<Vec<JobOutboxEntry>, meerkat_jobs::DetachedJobError> {
        self.inner.list_pending_outbox(limit).await
    }

    async fn list_for_origin(
        &self,
        realm_id: &str,
        origin_session_id: &SessionId,
        limit: usize,
    ) -> Result<Vec<StoredJob>, meerkat_jobs::DetachedJobError> {
        self.inner
            .list_for_origin(realm_id, origin_session_id, limit)
            .await
    }

    async fn list_all(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredJob>, meerkat_jobs::DetachedJobError> {
        self.inner.list_all(limit).await
    }

    async fn count_pending_outbox_jobs(
        &self,
        realm_id: Option<&str>,
    ) -> Result<u64, meerkat_jobs::DetachedJobError> {
        self.inner.count_pending_outbox_jobs(realm_id).await
    }

    async fn list_census_candidates(
        &self,
        realm_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredJob>, meerkat_jobs::DetachedJobError> {
        self.inner.list_census_candidates(realm_id, limit).await
    }

    fn is_persistent(&self) -> bool {
        self.inner.is_persistent()
    }

    fn outbox_commit_signal(&self) -> meerkat_jobs::JobOutboxCommitSignal {
        self.inner.outbox_commit_signal()
    }
}

#[tokio::test]
async fn concurrent_delivery_acknowledgements_converge_on_machine_authorized_state() {
    let memory = Arc::new(MemoryDetachedJobStore::new());
    let store = Arc::new(PausingDeliveryAckStore::new(memory));
    let service = DetachedJobService::new(store.clone());
    let job_id = service
        .submit(monitor_spec("concurrent-delivery-ack"))
        .await
        .expect("submit")
        .job_id;
    let claim = service
        .claim_attempt(
            &job_id,
            AttemptClaim::new(
                WorkerId::new("delivery-worker").expect("worker"),
                100,
                10_000,
                RunnerHandleRef::new("delivery-handle").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    let emitted = service
        .emit_notification(
            &job_id,
            (&claim).into(),
            200,
            JobNotification::new(
                "concurrent-delivery",
                "concurrent:delivery",
                "Concurrent delivery",
                "Concurrent delivery acknowledgement",
            )
            .expect("notification"),
        )
        .await
        .expect("emit notification");

    let first_service = service.clone();
    let first_job_id = job_id.clone();
    let first = tokio::spawn(async move {
        first_service
            .mark_delivery_applied(&first_job_id, emitted.delivery_sequence)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), store.ack_cas_entered.notified())
        .await
        .expect("first acknowledgement should enter the deterministic CAS gate");

    let winner = service
        .mark_delivery_applied(&job_id, emitted.delivery_sequence)
        .await
        .expect("racing delivery acknowledgement");
    store.release_ack_cas.notify_one();
    let reloaded = first
        .await
        .expect("first acknowledgement task")
        .expect("stale acknowledgement reloads generated-machine state");

    assert_eq!(reloaded.revision, winner.revision);
    assert_eq!(reloaded.outbox, winner.outbox);
    assert!(winner.outbox[0].applied);
}

#[tokio::test]
async fn predicate_delivery_key_atomically_deduplicates_the_entire_job_mutation_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(MemoryDetachedJobStore::new());
    let service = DetachedJobService::new(store.clone());
    let job_id = service
        .submit(monitor_spec("predicate-delivery-idempotency"))
        .await
        .expect("submit")
        .job_id;
    let claim = service
        .claim_attempt(
            &job_id,
            AttemptClaim::new(
                WorkerId::new("predicate-worker").expect("worker"),
                100,
                10_000,
                RunnerHandleRef::new("predicate-handle").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    let write = AttemptWriteAuthority::from(&claim);
    let watch = PredicateWatch::scheduled(
        PredicateWatchId::new("delivery-watch").expect("watch id"),
        ScheduleIdRef::new("schedule-delivery-watch").expect("schedule id"),
        PredicateSource::StableHttp {
            url: "https://example.invalid/delivery".into(),
            conditional_requests: true,
        },
        PredicateComparison::Changed,
        PredicatePollingPolicy::new(60, 1, 0, 1, 300).expect("policy"),
    )
    .expect("watch");
    let first_identity = PredicateDeliveryIdentity::new(
        PredicateDeliveryIdempotencyKey::new("schedule:schedule-delivery-watch:occurrence:first")
            .expect("delivery key"),
        "occurrence:first",
        "meerkat.predicate.evaluate.v1",
    )
    .expect("delivery identity");

    let first = service
        .evaluate_predicate_idempotent(
            &job_id,
            write.clone(),
            first_identity.clone(),
            &watch,
            PredicateObservation::available("v1", "Version v1").expect("observation"),
            200,
        )
        .await
        .expect("first evaluation");
    let PredicateDeliveryOutcome::Applied {
        receipt: first_receipt,
        snapshot: first_snapshot,
    } = first
    else {
        return Err(std::io::Error::other("first delivery must apply").into());
    };
    assert!(first_snapshot.outbox.is_empty());
    assert_eq!(first_receipt.committed_revision, first_snapshot.revision);
    let first_revision = first_snapshot.revision;
    let first_checkpoint = first_snapshot.checkpoint_ref.clone();
    let first_progress = first_snapshot.progress.clone();

    let duplicate = service
        .evaluate_predicate_idempotent(
            &job_id,
            write.clone(),
            first_identity.clone(),
            &watch,
            PredicateObservation::available("v2", "Version v2").expect("observation"),
            300,
        )
        .await
        .expect("duplicate evaluation");
    let PredicateDeliveryOutcome::Deduplicated { receipt, snapshot } = duplicate else {
        return Err(std::io::Error::other("same delivery identity must deduplicate").into());
    };
    assert_eq!(receipt, first_receipt);
    assert_eq!(snapshot.revision, first_revision);
    assert_eq!(snapshot.checkpoint_ref, first_checkpoint);
    assert_eq!(snapshot.progress, first_progress);
    assert!(snapshot.outbox.is_empty());

    let rebound_identity = PredicateDeliveryIdentity::new(
        first_identity.idempotency_key().clone(),
        "occurrence:tampered",
        "meerkat.predicate.evaluate.v1",
    )
    .expect("rebound identity");
    let rebound_error = service
        .predicate_delivery_applied(&job_id, &rebound_identity)
        .await
        .expect_err("one stable key cannot be rebound to another occurrence");
    assert!(rebound_error.to_string().contains("already bound"));

    let reopened = DetachedJobService::new(Arc::new(
        MemoryDetachedJobStore::from_snapshot(store.snapshot().await).expect("reopen"),
    ));
    assert!(
        reopened
            .predicate_delivery_applied(&job_id, &first_identity)
            .await
            .expect("read delivery fact")
    );
    let second_identity = PredicateDeliveryIdentity::new(
        PredicateDeliveryIdempotencyKey::new("schedule:schedule-delivery-watch:occurrence:second")
            .expect("delivery key"),
        "occurrence:second",
        "meerkat.predicate.evaluate.v1",
    )
    .expect("delivery identity");
    let second = reopened
        .evaluate_predicate_idempotent(
            &job_id,
            write,
            second_identity,
            &watch,
            PredicateObservation::available("v2", "Version v2").expect("observation"),
            400,
        )
        .await
        .expect("second occurrence");
    let PredicateDeliveryOutcome::Applied {
        receipt: _,
        snapshot,
    } = second
    else {
        return Err(std::io::Error::other("different delivery identity must apply").into());
    };
    assert_eq!(snapshot.outbox.len(), 1);
    Ok(())
}

#[tokio::test]
async fn notifications_are_nonterminal_deduplicated_and_share_job_delivery_sequence() {
    let store = Arc::new(MemoryDetachedJobStore::new());
    let service = DetachedJobService::new(store.clone());
    let job_id = service
        .submit(monitor_spec("notification-sequence"))
        .await
        .expect("submit")
        .job_id;
    let claim = service
        .claim_attempt(
            &job_id,
            AttemptClaim::new(
                WorkerId::new("monitor-worker").expect("worker"),
                100,
                10_000,
                RunnerHandleRef::new("monitor-handle").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    let write = AttemptWriteAuthority::from(&claim);

    let first = service
        .emit_notification(
            &job_id,
            write.clone(),
            200,
            JobNotification::new(
                "release-v1",
                "release:meerkat:v1",
                "Meerkat v1 released",
                "The release is now public.",
            )
            .expect("notification"),
        )
        .await
        .expect("first notification");
    assert!(!first.deduplicated);
    assert_eq!(first.delivery_sequence, 1);
    assert_eq!(first.snapshot.phase, meerkat_jobs::JobPhase::Running);
    assert_eq!(first.snapshot.attempt_count, 1);
    assert_eq!(first.snapshot.current_fence, claim.fence);
    assert!(matches!(
        &first.snapshot.outbox[0].payload,
        JobOutboxPayload::Notification(notification)
            if notification.idempotency_key() == "release:meerkat:v1"
    ));

    let duplicate = service
        .emit_notification(
            &job_id,
            write.clone(),
            201,
            JobNotification::new(
                "release-v1-replayed",
                "release:meerkat:v1",
                "Meerkat v1 released",
                "The release is now public.",
            )
            .expect("notification"),
        )
        .await
        .expect("duplicate notification");
    assert!(duplicate.deduplicated);
    assert_eq!(duplicate.delivery_sequence, 1);
    assert_eq!(duplicate.snapshot.revision, first.snapshot.revision);
    assert_eq!(duplicate.snapshot.outbox.len(), 1);

    let second = service
        .emit_notification(
            &job_id,
            write.clone(),
            300,
            JobNotification::new(
                "release-v2",
                "release:meerkat:v2",
                "Meerkat v2 released",
                "A later release is now public.",
            )
            .expect("notification"),
        )
        .await
        .expect("second notification");
    assert_eq!(second.delivery_sequence, 2);
    assert_eq!(second.snapshot.phase, meerkat_jobs::JobPhase::Running);
    assert_eq!(second.snapshot.outbox.len(), 2);

    let terminal = service
        .complete_attempt(&job_id, write, 400, None)
        .await
        .expect("terminal");
    assert_eq!(terminal.outbox.len(), 3);
    assert_eq!(terminal.outbox[2].delivery_sequence, 3);
    assert!(matches!(
        terminal.outbox[2].payload,
        JobOutboxPayload::Terminal(JobTerminalResult::Succeeded { .. })
    ));

    let reopened = DetachedJobService::new(Arc::new(
        MemoryDetachedJobStore::from_snapshot(store.snapshot().await).expect("reopen"),
    ));
    let recovered = reopened.get(&job_id).await.expect("read").expect("job");
    assert_eq!(recovered.outbox, terminal.outbox);
    assert_eq!(recovered.attempt_count, 1);
    assert_eq!(recovered.current_fence, claim.fence);

    let applied = reopened
        .mark_delivery_applied(&job_id, first.delivery_sequence)
        .await
        .expect("apply notification delivery");
    let duplicate = reopened
        .mark_delivery_applied(&job_id, first.delivery_sequence)
        .await
        .expect("machine-authorized duplicate acknowledgement");
    assert_eq!(duplicate.revision, applied.revision);
    assert_eq!(duplicate.outbox, applied.outbox);
}

#[tokio::test]
async fn predicate_crossing_replay_after_notification_commit_does_not_lose_or_duplicate_delivery() {
    let store = Arc::new(MemoryDetachedJobStore::new());
    let service = DetachedJobService::new(store.clone());
    let job_id = service
        .submit(monitor_spec("predicate-crash-window"))
        .await
        .expect("submit")
        .job_id;
    let claim = service
        .claim_attempt(
            &job_id,
            AttemptClaim::new(
                WorkerId::new("predicate-worker").expect("worker"),
                100,
                10_000,
                RunnerHandleRef::new("predicate-handle").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    let write = AttemptWriteAuthority::from(&claim);
    let watch = PredicateWatch::scheduled(
        PredicateWatchId::new("release-watch").expect("watch id"),
        ScheduleIdRef::new("schedule-release-watch").expect("schedule id"),
        PredicateSource::StableHttp {
            url: "https://example.invalid/releases/latest".into(),
            conditional_requests: true,
        },
        PredicateComparison::Changed,
        PredicatePollingPolicy::new(60, 1, 0, 1, 300).expect("policy"),
    )
    .expect("watch");

    let baseline = service
        .evaluate_predicate(
            &job_id,
            write.clone(),
            &watch,
            PredicateObservation::available("v1", "Version v1").expect("observation"),
            200,
        )
        .await
        .expect("record baseline");
    assert!(matches!(
        baseline.evaluation,
        PredicateEvaluation::Baseline { .. }
    ));
    assert!(baseline.notification.is_none());
    assert!(baseline.snapshot.outbox.is_empty());
    assert!(matches!(
        baseline
            .snapshot
            .progress
            .as_ref()
            .map(|progress| &progress.kind),
        Some(meerkat_jobs::JobProgressKind::Health {
            condition: meerkat_jobs::JobHealthCondition::Healthy
        })
    ));

    let unavailable = service
        .evaluate_predicate(
            &job_id,
            write.clone(),
            &watch,
            PredicateObservation::unavailable("source_timeout").expect("unavailable"),
            250,
        )
        .await
        .expect("persist source health");
    assert!(matches!(
        unavailable
            .snapshot
            .progress
            .as_ref()
            .map(|progress| &progress.kind),
        Some(meerkat_jobs::JobProgressKind::Health {
            condition: meerkat_jobs::JobHealthCondition::PredicateSourceUnavailable {
                retry_after_secs: 60
            }
        })
    ));
    assert!(unavailable.snapshot.outbox.is_empty());

    // Simulate the exact crash seam: the crossing notification commits, but
    // the later checkpoint write does not. Recovery must replay from v1 and
    // suppress the duplicate notification before advancing to v2.
    let crossing = watch
        .evaluate(
            baseline.evaluation.checkpoint(),
            PredicateObservation::available("v2", "Version v2").expect("observation"),
        )
        .expect("crossing");
    let notification = crossing
        .notification()
        .cloned()
        .expect("crossing notification");
    service
        .emit_notification(&job_id, write.clone(), 300, notification)
        .await
        .expect("commit notification before crash");

    let reopened = DetachedJobService::new(Arc::new(
        MemoryDetachedJobStore::from_snapshot(store.snapshot().await).expect("reopen"),
    ));
    let replayed = reopened
        .evaluate_predicate(
            &job_id,
            write.clone(),
            &watch,
            PredicateObservation::available("v2", "Version v2").expect("observation"),
            301,
        )
        .await
        .expect("replay crossing");
    assert!(
        replayed
            .notification
            .as_ref()
            .expect("deduplicated notification receipt")
            .deduplicated
    );
    assert_eq!(replayed.snapshot.outbox.len(), 1);
    assert_eq!(replayed.snapshot.attempt_count, 1);
    assert_eq!(replayed.snapshot.current_fence, claim.fence);

    let later = reopened
        .evaluate_predicate(
            &job_id,
            write,
            &watch,
            PredicateObservation::available("v3", "Version v3").expect("observation"),
            400,
        )
        .await
        .expect("later crossing");
    assert!(
        !later
            .notification
            .as_ref()
            .expect("later notification")
            .deduplicated
    );
    assert_eq!(later.snapshot.outbox.len(), 2);
    assert_eq!(later.snapshot.outbox[0].delivery_sequence, 1);
    assert_eq!(later.snapshot.outbox[1].delivery_sequence, 2);
}

#[tokio::test]
async fn terminal_named_notification_and_terminal_delivery_are_acked_by_their_own_arms() {
    let memory = Arc::new(MemoryDetachedJobStore::new());
    let service = DetachedJobService::new(memory.clone());
    let job_id = service
        .submit(monitor_spec("terminal-named-notification"))
        .await
        .expect("submit")
        .job_id;
    let claim = service
        .claim_attempt(
            &job_id,
            AttemptClaim::new(
                WorkerId::new("worker-a").expect("worker"),
                100,
                10_000,
                RunnerHandleRef::new("terminal-named-handle").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    let write = AttemptWriteAuthority::from(&claim);
    let emitted = service
        .emit_notification(
            &job_id,
            write.clone(),
            200,
            JobNotification::new(
                "terminal",
                "terminal:named",
                "Named like the terminal delivery",
                "A notification whose id equals the terminal delivery id",
            )
            .expect("notification"),
        )
        .await
        .expect("a notification may carry the terminal delivery's id");
    let completed = service
        .complete_attempt(&job_id, write, 300, None)
        .await
        .expect("complete");

    // Both outbox entries carry the delivery id "terminal"; their sequences
    // and runtime delivery ids differ.
    assert_eq!(completed.outbox.len(), 2);
    let notification_sequence = emitted.delivery_sequence;
    let notification_entry = completed
        .outbox
        .iter()
        .find(|entry| matches!(entry.payload, JobOutboxPayload::Notification(_)))
        .expect("notification entry");
    let terminal_entry = completed
        .outbox
        .iter()
        .find(|entry| matches!(entry.payload, JobOutboxPayload::Terminal(_)))
        .expect("terminal entry");
    assert_eq!(notification_entry.delivery_id, "terminal");
    assert_eq!(terminal_entry.delivery_id, "terminal");
    assert_eq!(notification_entry.delivery_sequence, notification_sequence);
    let terminal_sequence = terminal_entry.delivery_sequence;
    assert!(notification_sequence < terminal_sequence);
    assert_eq!(
        notification_entry.runtime_delivery_id(),
        format!("{job_id}:notification:terminal")
    );
    assert_eq!(terminal_entry.runtime_delivery_id(), job_id.to_string());

    let applied = |snapshot: &meerkat_jobs::JobSnapshot, sequence: u64| {
        snapshot
            .outbox
            .iter()
            .find(|entry| entry.delivery_sequence == sequence)
            .expect("outbox entry")
            .applied
    };

    // The terminal delivery is acknowledged by the terminal arm only.
    let after_terminal = service
        .mark_delivery_applied(&job_id, terminal_sequence)
        .await
        .expect("terminal acknowledgement");
    assert!(applied(&after_terminal, terminal_sequence));
    assert!(!applied(&after_terminal, notification_sequence));
    let state = memory
        .get(&job_id)
        .await
        .expect("load")
        .expect("job")
        .machine_state;
    assert!(state.terminal_delivery_applied);
    assert!(state.notification_applied.is_empty());

    // The notification is acknowledged by the notification arm only.
    let after_notification = service
        .mark_delivery_applied(&job_id, notification_sequence)
        .await
        .expect("notification acknowledgement");
    assert!(applied(&after_notification, notification_sequence));
    assert!(applied(&after_notification, terminal_sequence));
    let state = memory
        .get(&job_id)
        .await
        .expect("load")
        .expect("job")
        .machine_state;
    assert!(state.terminal_delivery_applied);
    assert_eq!(
        state.notification_applied.iter().collect::<Vec<_>>(),
        vec!["terminal"]
    );

    // Replays reach the already-applied arms and change nothing.
    for sequence in [terminal_sequence, notification_sequence] {
        let replayed = service
            .mark_delivery_applied(&job_id, sequence)
            .await
            .expect("replayed acknowledgement");
        assert_eq!(replayed.outbox, after_notification.outbox);
    }
}
