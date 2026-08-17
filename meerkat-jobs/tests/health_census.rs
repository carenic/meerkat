//! What the operational census may claim, and about which rows.
//!
//! The defect these pin: the census read a window of `ORDER BY job_id LIMIT n`
//! and reported plain counts off it. `job_id` is a time-ordered primary key,
//! so the window fills with the OLDEST rows, which in a long-lived store are
//! terminal rows that contribute nothing - and the newest live wedged job is
//! behind them. That degrades with AGE, not load, and it degrades silently:
//! `jobs: ok` published off rows nobody looked at.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use meerkat_core::SessionId;
use meerkat_jobs::{
    AttemptClaim, AttemptWriteAuthority, CanonicalArgumentsHash, DetachedJobService,
    DetachedJobStore, ExecutionIntentId, InteractionLineageId, JobHealthCoverage, JobHealthReading,
    JobId, JobResultRef, JobSpec, JobSubmissionKey, RestartClass, RunnerHandleRef, RunnerIdentity,
    SqliteDetachedJobStore, ToolIdentity, WorkerId,
};

fn spec(realm_id: &str, key: &str) -> JobSpec {
    JobSpec::new(
        realm_id,
        SessionId::new(),
        ExecutionIntentId::new(),
        InteractionLineageId::new(),
        ToolIdentity::new("security_scan", "v1").expect("tool identity"),
        RunnerIdentity::new("homecore.security_scan", "v1").expect("runner identity"),
        RestartClass::NonResumable,
        CanonicalArgumentsHash::new("sha256:scan-a").expect("arguments hash"),
        JobSubmissionKey::new(key).expect("submission key"),
    )
}

/// Drive one job all the way to `Succeeded` with its terminal delivery
/// applied: a row that is durably present and operationally finished.
async fn drained_terminal_job(service: &DetachedJobService, realm_id: &str, key: &str) -> JobId {
    let job_id = terminal_job_with_pending_delivery(service, realm_id, key).await;
    let snapshot = service.get(&job_id).await.expect("get").expect("present");
    let entry = snapshot.outbox.first().expect("terminal outbox entry");
    service
        .mark_delivery_applied(&job_id, entry.delivery_sequence)
        .await
        .expect("mark applied");
    job_id
}

/// Terminal, but its terminal delivery was never applied: the row is finished
/// and the delivery is still owed.
async fn terminal_job_with_pending_delivery(
    service: &DetachedJobService,
    realm_id: &str,
    key: &str,
) -> JobId {
    let receipt = service
        .submit(spec(realm_id, key))
        .await
        .expect("submit terminal job");
    let claim = service
        .claim_attempt(
            &receipt.job_id,
            AttemptClaim::new(
                WorkerId::new("worker-a").expect("worker"),
                10,
                10_000,
                RunnerHandleRef::new("external:scan").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    service
        .complete_attempt(
            &receipt.job_id,
            AttemptWriteAuthority::from(&claim),
            20,
            Some(JobResultRef::new("artifact:result").expect("result ref")),
        )
        .await
        .expect("complete");
    receipt.job_id
}

/// A live job holding an expired lease: the wedge an operator pages on.
async fn wedged_running_job(
    service: &DetachedJobService,
    realm_id: &str,
    key: &str,
    lease_expires_at_ms: u64,
) -> JobId {
    let receipt = service
        .submit(spec(realm_id, key))
        .await
        .expect("submit live job");
    service
        .claim_attempt(
            &receipt.job_id,
            AttemptClaim::new(
                WorkerId::new("worker-b").expect("worker"),
                10,
                lease_expires_at_ms,
                RunnerHandleRef::new("external:scan").expect("handle"),
            ),
        )
        .await
        .expect("claim");
    receipt.job_id
}

/// A census that stopped at its window has not established `ok`.
///
/// This is the false-green case in its purest form: every row in the store is
/// finished and drained, so the counts are all zero and the OLD `is_degraded()`
/// answered `false` - "healthy" - on the strength of a scan that never reached
/// the end of the population.
#[tokio::test]
async fn a_window_that_filled_reports_unreadable_rather_than_ok() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        SqliteDetachedJobStore::open(temp.path().join("jobs.sqlite3")).expect("open store"),
    );
    let service = DetachedJobService::new(store);
    for index in 0..3 {
        drained_terminal_job(&service, "realm-a", &format!("terminal-{index}")).await;
    }

    let saturated = service
        .health_snapshot_for_realm("realm-a", 1_000, 3)
        .await
        .expect("saturated census");
    assert_eq!(
        saturated.coverage,
        JobHealthCoverage::Truncated {
            scanned: 3,
            limit: 3
        },
        "a scan that came back exactly full stopped at the window, not at the end of the rows"
    );
    assert_eq!(
        saturated.reading(),
        JobHealthReading::Unreadable,
        "zero counts off a truncated window are not evidence of health"
    );

    let complete = service
        .health_snapshot_for_realm("realm-a", 1_000, 4)
        .await
        .expect("complete census");
    assert_eq!(complete.coverage, JobHealthCoverage::Complete);
    assert_eq!(
        complete.reading(),
        JobHealthReading::Ok,
        "the same store read to the end is genuinely healthy"
    );
}

/// The known negative for the rung: a store made entirely of finished work is
/// `Ok`, not `Degraded`.
///
/// Retention is not a fault. A monitor that alarmed on the existence of
/// terminal rows would page on ordinary healthy history, which is the failure
/// mode opposite to the one this lane fixes and just as useless.
#[tokio::test]
async fn a_store_of_finished_drained_work_is_healthy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        SqliteDetachedJobStore::open(temp.path().join("jobs.sqlite3")).expect("open store"),
    );
    let service = DetachedJobService::new(store);
    for index in 0..5 {
        drained_terminal_job(&service, "realm-a", &format!("terminal-{index}")).await;
    }

    let health = service
        .health_snapshot_for_realm("realm-a", 1_000, 100)
        .await
        .expect("census");
    assert_eq!(health.coverage, JobHealthCoverage::Complete);
    assert_eq!(health.queued, 0);
    assert_eq!(health.running, 0);
    assert_eq!(health.stale_leases, 0);
    assert_eq!(health.needs_attention, 0);
    assert_eq!(health.pending_outbox_jobs, 0);
    assert_eq!(health.reading(), JobHealthReading::Ok);
}

/// The outbox backlog is exact, uncapped, phase-blind, and realm-scoped.
///
/// Each of those four words is a separate way the old scan got this wrong:
/// it counted only within the window (capped), it counted only rows the
/// window reached (inexact), and it applied the realm filter after the cap.
/// Phase-blind is the one that must NOT change: a terminal job holding an
/// unapplied terminal delivery is the canonical delivery wedge.
#[tokio::test]
async fn pending_outbox_is_counted_outside_the_scan_window() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn DetachedJobStore> = Arc::new(
        SqliteDetachedJobStore::open(temp.path().join("jobs.sqlite3")).expect("open store"),
    );
    let service = DetachedJobService::new(Arc::clone(&store));
    for index in 0..4 {
        drained_terminal_job(&service, "realm-a", &format!("terminal-{index}")).await;
    }
    terminal_job_with_pending_delivery(&service, "realm-a", "owed-delivery").await;
    terminal_job_with_pending_delivery(&service, "realm-b", "other-realm-owed").await;

    assert_eq!(
        store
            .count_pending_outbox_jobs(Some("realm-a"))
            .await
            .expect("count realm-a"),
        1,
        "the realm filter belongs in the query, not after a window"
    );
    assert_eq!(
        store
            .count_pending_outbox_jobs(Some("realm-b"))
            .await
            .expect("count realm-b"),
        1
    );
    assert_eq!(
        store
            .count_pending_outbox_jobs(None)
            .await
            .expect("count all realms"),
        2
    );

    // A window of one row cannot see six rows. The outbox count is taken
    // outside it and stays exact; only the phase counts are truncated.
    let health = service
        .health_snapshot_for_realm("realm-a", 1_000, 1)
        .await
        .expect("census");
    assert_eq!(
        health.pending_outbox_jobs, 1,
        "an owed delivery must not depend on the row landing inside the scan window"
    );
    assert_eq!(
        health.reading(),
        JobHealthReading::Unreadable,
        "the phase half of this census still did not complete"
    );
}

/// An owed delivery on its own degrades a fully-read census.
#[tokio::test]
async fn an_owed_delivery_degrades_a_complete_census() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        SqliteDetachedJobStore::open(temp.path().join("jobs.sqlite3")).expect("open store"),
    );
    let service = DetachedJobService::new(store);
    terminal_job_with_pending_delivery(&service, "realm-a", "owed-delivery").await;

    let health = service
        .health_snapshot_for_realm("realm-a", 1_000, 100)
        .await
        .expect("census");
    assert_eq!(health.coverage, JobHealthCoverage::Complete);
    assert_eq!(health.pending_outbox_jobs, 1);
    assert_eq!(health.reading(), JobHealthReading::Degraded);
}

/// A wedged live job is `Degraded`, and says which term found it.
#[tokio::test]
async fn an_expired_lease_degrades_a_complete_census() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        SqliteDetachedJobStore::open(temp.path().join("jobs.sqlite3")).expect("open store"),
    );
    let service = DetachedJobService::new(store);
    wedged_running_job(&service, "realm-a", "wedged", 100).await;

    let health = service
        .health_snapshot_for_realm("realm-a", 1_000, 100)
        .await
        .expect("census");
    assert_eq!(health.running, 1);
    assert_eq!(health.stale_leases, 1);
    assert_eq!(health.pending_outbox_jobs, 0);
    assert_eq!(health.coverage, JobHealthCoverage::Complete);
    assert_eq!(health.reading(), JobHealthReading::Degraded);
}
