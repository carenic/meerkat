//! Member-origin addressing of detached jobs (#1790).
//!
//! A detached job's runtime deliveries (its notifications and terminal) are
//! addressed to its origin session's runtime, unless its submission recorded
//! the delivery address of the mob member whose session submitted it
//! ([`meerkat_jobs::JobSpec::origin_member_id`]). An explicit-resume repoint
//! moves a member to a successor session and migrates no delivery rows, so a
//! job addressed to the superseded session would strand there; addressed to
//! the member, it is applied in whichever session serves the member when the
//! row is delivered.

use std::sync::Arc;

use meerkat_core::SessionId;
use meerkat_jobs::{
    DetachedJobError, DetachedJobStore, InsertJobOutcome, JobId, JobOutboxCommitSignal,
    JobOutboxEntry, JobSubmissionKey, OriginMemberId, PredicateDeliveryCommit,
    PredicateDeliveryCommitOutcome, PredicateDeliveryIdentity, PredicateDeliveryReceipt, StoredJob,
};

/// A job store that records, with every new submission, the delivery address
/// of the mob member whose current session submits it, as the host's bound
/// continuation resolver reports it
/// ([`crate::ContinuationAddressResolver::member_address_of_session`]). It
/// stamps only while the submitting session is the member's current one, so
/// the address is that of the incarnation the job was started by. A
/// submission that already names an origin member, a session no member
/// serves, and a host with no bound resolver are stored as given.
///
/// A replay is transparent: a submission whose key already admitted a job
/// takes that job's origin member, whatever it recorded (none, for a job
/// submitted before 0.9.3, through the host's own store or while the
/// resolver could not tell), and the inner store's equivalence check decides
/// the rest as before. Every other operation is the inner store's.
pub(crate) struct MemberOriginJobStore {
    inner: Arc<dyn DetachedJobStore>,
    continuations: Arc<crate::ContinuationHostBindings>,
}

impl MemberOriginJobStore {
    pub(crate) fn new(
        inner: Arc<dyn DetachedJobStore>,
        continuations: Arc<crate::ContinuationHostBindings>,
    ) -> Self {
        Self {
            inner,
            continuations,
        }
    }
}

#[async_trait::async_trait]
impl DetachedJobStore for MemberOriginJobStore {
    async fn insert_deduplicated(
        &self,
        mut job: StoredJob,
    ) -> Result<InsertJobOutcome, DetachedJobError> {
        if job.spec.origin_member_id.is_none() {
            match self
                .inner
                .get_by_submission_key(&job.spec.realm_id, &job.spec.submission_key)
                .await
            {
                Ok(Some(admitted)) => {
                    job.spec.origin_member_id = admitted.spec.origin_member_id;
                }
                Ok(None) => {
                    if let Some(address) = self
                        .continuations
                        .member_address_of_session(&job.spec.origin_session_id)
                        .await
                    {
                        job.spec.origin_member_id = Some(OriginMemberId::new(address.0)?);
                    }
                }
                // A store that cannot read its submission index stamps
                // nothing: its jobs keep their session's address.
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "the submission index is unreadable; the job keeps its session's address"
                    );
                }
            }
        }
        self.inner.insert_deduplicated(job).await
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<StoredJob>, DetachedJobError> {
        self.inner.get(job_id).await
    }

    async fn get_by_submission_key(
        &self,
        realm_id: &str,
        submission_key: &JobSubmissionKey,
    ) -> Result<Option<StoredJob>, DetachedJobError> {
        self.inner
            .get_by_submission_key(realm_id, submission_key)
            .await
    }

    async fn compare_and_swap(
        &self,
        expected_revision: u64,
        replacement: StoredJob,
    ) -> Result<StoredJob, DetachedJobError> {
        self.inner
            .compare_and_swap(expected_revision, replacement)
            .await
    }

    async fn predicate_delivery_receipt(
        &self,
        job_id: &JobId,
        identity: &PredicateDeliveryIdentity,
    ) -> Result<Option<PredicateDeliveryReceipt>, DetachedJobError> {
        self.inner
            .predicate_delivery_receipt(job_id, identity)
            .await
    }

    async fn commit_predicate_delivery(
        &self,
        expected_revision: u64,
        replacement: StoredJob,
        commit: PredicateDeliveryCommit,
    ) -> Result<PredicateDeliveryCommitOutcome, DetachedJobError> {
        self.inner
            .commit_predicate_delivery(expected_revision, replacement, commit)
            .await
    }

    async fn list_pending_outbox(
        &self,
        limit: usize,
    ) -> Result<Vec<JobOutboxEntry>, DetachedJobError> {
        self.inner.list_pending_outbox(limit).await
    }

    async fn list_for_origin(
        &self,
        realm_id: &str,
        origin_session_id: &SessionId,
        limit: usize,
    ) -> Result<Vec<StoredJob>, DetachedJobError> {
        self.inner
            .list_for_origin(realm_id, origin_session_id, limit)
            .await
    }

    async fn list_all(&self, limit: usize) -> Result<Vec<StoredJob>, DetachedJobError> {
        self.inner.list_all(limit).await
    }

    async fn count_pending_outbox_jobs(
        &self,
        realm_id: Option<&str>,
    ) -> Result<u64, DetachedJobError> {
        self.inner.count_pending_outbox_jobs(realm_id).await
    }

    async fn list_census_candidates(
        &self,
        realm_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredJob>, DetachedJobError> {
        self.inner.list_census_candidates(realm_id, limit).await
    }

    fn is_persistent(&self) -> bool {
        self.inner.is_persistent()
    }

    fn outbox_commit_signal(&self) -> JobOutboxCommitSignal {
        self.inner.outbox_commit_signal()
    }
}
