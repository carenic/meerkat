use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use meerkat_core::{AuthBindingRef, SessionId, ToolCredentialContextRef};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::machines::detached_job::{
    DetachedJobMachineState, DetachedJobPhase, DetachedJobRestartClass, DetachedJobTerminalKind,
};
use crate::store::{
    PredicateDeliveryCommitOutcome, ensure_same_predicate_delivery_identity, next_revision,
    validate_job_replacement, validate_predicate_delivery_receipt, validate_stored_job,
};
use crate::{
    CanonicalArgumentsHash, DetachedJobError, DetachedJobStore, ExecutionIntentId,
    InsertJobOutcome, InteractionLineageId, JobId, JobOutboxEntry, JobOutboxPayload, JobProgress,
    JobSpec, JobSubmissionKey, JobSubscription, JobTerminalResult, OriginMemberId,
    PredicateDeliveryCommit, PredicateDeliveryIdentity, PredicateDeliveryReceipt, RunnerIdentity,
    RunnerSpecificationRef, StoredJob, ToolIdentity,
};

const STORED_JOB_FORMAT_VERSION: u32 = 1;
const PREDICATE_DELIVERY_RECEIPT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct StoredJobEnvelope {
    format_version: u32,
    job: PersistedStoredJob,
}

#[derive(Debug, Serialize, Deserialize)]
struct PredicateDeliveryReceiptEnvelope {
    format_version: u32,
    receipt: PredicateDeliveryReceipt,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedStoredJob {
    job_id: JobId,
    spec: PersistedJobSpec,
    revision: u64,
    machine_state: PersistedMachineState,
    progress: Option<JobProgress>,
    terminal_result: Option<JobTerminalResult>,
    #[serde(default)]
    subscriptions: Vec<JobSubscription>,
    outbox: Vec<PersistedJobOutboxEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedJobSpec {
    realm_id: String,
    origin_session_id: SessionId,
    origin_member_id: Option<OriginMemberId>,
    execution_intent_id: ExecutionIntentId,
    interaction_lineage_id: InteractionLineageId,
    tool: ToolIdentity,
    runner: RunnerIdentity,
    #[serde(default)]
    runner_specification_ref: Option<RunnerSpecificationRef>,
    restart_class: PersistedRestartClass,
    canonical_arguments_hash: CanonicalArgumentsHash,
    credential_context_refs: Vec<PersistedCredentialContextRef>,
    submission_key: JobSubmissionKey,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedCredentialContextRef {
    OwningProfile {
        required_scopes: std::collections::BTreeSet<String>,
    },
    AuthBinding {
        auth_binding: AuthBindingRef,
        required_scopes: std::collections::BTreeSet<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedRestartClass {
    Adoptable,
    CheckpointResumable,
    Replayable,
    NonResumable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedPhase {
    Unsubmitted,
    Queued,
    Claimed,
    Running,
    WaitingExternal,
    LossObserved,
    RetryScheduled,
    Succeeded,
    Failed,
    Cancelled,
    WorkerLost,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedTerminalKind {
    Succeeded,
    Failed,
    Cancelled,
    WorkerLost,
    NeedsAttention,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedMachineState {
    lifecycle_phase: PersistedPhase,
    job_id: String,
    restart_class: PersistedRestartClass,
    attempt_count: u64,
    current_attempt_id: Option<String>,
    current_fence: u64,
    current_worker_id: Option<String>,
    lease_expires_at_ms: Option<u64>,
    heartbeat_at_ms: Option<u64>,
    checkpoint_ref: Option<String>,
    runner_handle: Option<String>,
    progress_cursor: u64,
    lease_expired: bool,
    retry_due_at_ms: Option<u64>,
    cancel_requested: bool,
    #[serde(default)]
    delivery_sequence: u64,
    #[serde(default)]
    notification_ids: BTreeSet<String>,
    #[serde(default)]
    notification_idempotency_keys: BTreeSet<String>,
    #[serde(default)]
    notification_id_by_key: BTreeMap<String, String>,
    #[serde(default)]
    notification_delivery_ids: BTreeMap<String, String>,
    #[serde(default)]
    notification_sequences: BTreeMap<String, u64>,
    #[serde(default)]
    notification_applied: BTreeSet<String>,
    terminal_kind: Option<PersistedTerminalKind>,
    terminal_delivery_sequence: u64,
    terminal_delivery_applied: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedJobOutboxEntry {
    job_id: JobId,
    #[serde(default)]
    delivery_id: Option<String>,
    delivery_sequence: u64,
    #[serde(default)]
    payload: Option<JobOutboxPayload>,
    #[serde(default)]
    terminal_kind: Option<PersistedTerminalKind>,
    #[serde(default)]
    terminal_result: Option<JobTerminalResult>,
    #[serde(default)]
    targets: Vec<JobSubscription>,
    applied: bool,
}

pub const JOBS_DOMAIN: meerkat_sqlite::SchemaDomain = meerkat_sqlite::SchemaDomain {
    name: "jobs",
    migrations: &[
        meerkat_sqlite::Migration {
            version: 1,
            name: "base-schema",
            apply: migration_0001_jobs_schema,
        },
        meerkat_sqlite::Migration {
            version: 2,
            name: "notification-outbox-and-subscriptions",
            apply: migration_0002_notification_outbox_and_subscriptions,
        },
        meerkat_sqlite::Migration {
            version: 3,
            name: "predicate-delivery-ledger",
            apply: migration_0003_predicate_delivery_ledger,
        },
    ],
    initialize_current: initialize_current_jobs_schema,
    allowed_existing_versions: &[2, 3],
    bridge_recoverable_versions: &[1],
    released_predecessors: &[meerkat_sqlite::SchemaPredecessor {
        version: 2,
        verify: verify_released_jobs_v2_schema,
    }],
    owned_objects: &[
        meerkat_sqlite::SchemaObject {
            kind: meerkat_sqlite::SchemaObjectKind::Table,
            name: "detached_jobs",
        },
        meerkat_sqlite::SchemaObject {
            kind: meerkat_sqlite::SchemaObjectKind::Index,
            name: "idx_detached_jobs_pending_outbox",
        },
        meerkat_sqlite::SchemaObject {
            kind: meerkat_sqlite::SchemaObjectKind::Table,
            name: "detached_job_predicate_deliveries",
        },
    ],
    retired_objects: &[],
};

fn initialize_current_jobs_schema(tx: &Transaction<'_>) -> Result<(), rusqlite::Error> {
    migration_0001_jobs_schema(tx)?;
    migration_0002_notification_outbox_and_subscriptions(tx)?;
    migration_0003_predicate_delivery_ledger(tx)
}

fn migration_0001_jobs_schema(tx: &Transaction<'_>) -> Result<(), rusqlite::Error> {
    tx.execute_batch(
        r"
        CREATE TABLE detached_jobs (
            job_id TEXT PRIMARY KEY,
            realm_id TEXT NOT NULL,
            submission_key TEXT NOT NULL,
            revision BLOB NOT NULL CHECK (length(revision) = 8),
            has_pending_outbox INTEGER NOT NULL CHECK (has_pending_outbox IN (0, 1)),
            job_json BLOB NOT NULL,
            UNIQUE (realm_id, submission_key)
        );
        CREATE INDEX idx_detached_jobs_pending_outbox
            ON detached_jobs (has_pending_outbox, job_id);
        ",
    )
}

fn migration_0002_notification_outbox_and_subscriptions(
    _tx: &Transaction<'_>,
) -> Result<(), rusqlite::Error> {
    // The durable document remains in the existing job_json envelope, but
    // v2 stamps the first writer that may persist notification/subscription
    // fields. Older binaries therefore refuse the database before attempting
    // to decode a row they cannot understand.
    Ok(())
}

const RELEASED_JOBS_V2_OBJECTS: &[meerkat_sqlite::SchemaObject] = &[
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Table,
        name: "detached_jobs",
    },
    meerkat_sqlite::SchemaObject {
        kind: meerkat_sqlite::SchemaObjectKind::Index,
        name: "idx_detached_jobs_pending_outbox",
    },
];

fn verify_released_jobs_v2_schema(conn: &Connection) -> Result<(), String> {
    meerkat_sqlite::verify_released_schema_fingerprint(
        conn,
        &JOBS_DOMAIN,
        RELEASED_JOBS_V2_OBJECTS,
        migration_0001_jobs_schema,
    )
}

fn migration_0003_predicate_delivery_ledger(tx: &Transaction<'_>) -> Result<(), rusqlite::Error> {
    tx.execute_batch(
        r"
        CREATE TABLE detached_job_predicate_deliveries (
            job_id TEXT NOT NULL,
            delivery_idempotency_key TEXT NOT NULL,
            occurrence_id TEXT NOT NULL,
            runnable TEXT NOT NULL,
            committed_revision BLOB NOT NULL CHECK (length(committed_revision) = 8),
            receipt_json BLOB NOT NULL,
            PRIMARY KEY (job_id, delivery_idempotency_key)
        );
        ",
    )
}

#[derive(Debug, Clone)]
pub struct SqliteDetachedJobStore {
    path: PathBuf,
}

impl SqliteDetachedJobStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, DetachedJobError> {
        let store = Self { path: path.into() };
        store.with_connection(|_| Ok(()))?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn is_persistent(&self) -> bool {
        true
    }

    pub async fn list_for_origin(
        &self,
        realm_id: &str,
        origin_session_id: &SessionId,
        limit: usize,
    ) -> Result<Vec<StoredJob>, DetachedJobError> {
        DetachedJobStore::list_for_origin(self, realm_id, origin_session_id, limit).await
    }

    fn with_connection<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, DetachedJobError>,
    ) -> Result<T, DetachedJobError> {
        let _guard =
            meerkat_sqlite::OperationGuard::for_database(&self.path).map_err(sqlite_store_error)?;
        let mut conn = meerkat_sqlite::open_with(
            &self.path,
            meerkat_sqlite::ConnectionProfile::PRIMARY,
            meerkat_sqlite::OpenOptions {
                schema_preflight: &[&JOBS_DOMAIN],
                ..Default::default()
            },
        )
        .map_err(sqlite_store_error)?;
        meerkat_sqlite::apply_domain_migrations(&mut conn, &JOBS_DOMAIN)
            .map_err(sqlite_store_error)?;
        f(&mut conn)
    }
}

#[async_trait]
impl DetachedJobStore for SqliteDetachedJobStore {
    async fn insert_deduplicated(
        &self,
        job: StoredJob,
    ) -> Result<InsertJobOutcome, DetachedJobError> {
        validate_stored_job(&job)?;
        self.with_connection(|conn| {
            let tx = meerkat_sqlite::begin_immediate(conn).map_err(sqlite_store_error)?;
            let existing = select_by_submission(&tx, &job.spec.realm_id, &job.spec.submission_key)?;
            if let Some(existing) = existing {
                if !existing.spec.equivalent_submission(&job.spec) {
                    return Err(DetachedJobError::SubmissionConflict);
                }
                tx.commit().map_err(raw_sqlite_error)?;
                return Ok(InsertJobOutcome::Existing(existing));
            }
            let encoded = encode_job(&job)?;
            let pending = has_pending_outbox(&job);
            let revision = revision_bytes(job.revision);
            tx.execute(
                "INSERT INTO detached_jobs
                    (job_id, realm_id, submission_key, revision, has_pending_outbox, job_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    job.job_id.as_str(),
                    job.spec.realm_id,
                    job.spec.submission_key.as_str(),
                    revision.as_slice(),
                    pending,
                    encoded,
                ],
            )
            .map_err(raw_sqlite_error)?;
            tx.commit().map_err(raw_sqlite_error)?;
            Ok(InsertJobOutcome::Inserted(job))
        })
    }

    async fn get(&self, job_id: &JobId) -> Result<Option<StoredJob>, DetachedJobError> {
        self.with_connection(|conn| select_by_id(conn, job_id))
    }

    async fn compare_and_swap(
        &self,
        expected_revision: u64,
        mut replacement: StoredJob,
    ) -> Result<StoredJob, DetachedJobError> {
        self.with_connection(|conn| {
            let tx = meerkat_sqlite::begin_immediate(conn).map_err(sqlite_store_error)?;
            let current = select_by_id(&tx, &replacement.job_id)?
                .ok_or_else(|| DetachedJobError::NotFound(replacement.job_id.clone()))?;
            if current.revision != expected_revision {
                return Err(DetachedJobError::StaleRevision {
                    job_id: replacement.job_id.clone(),
                    expected: expected_revision,
                    actual: current.revision,
                });
            }
            if current.spec.submission_key != replacement.spec.submission_key
                || !current.spec.equivalent_submission(&replacement.spec)
            {
                return Err(DetachedJobError::Store(
                    "compare-and-swap cannot change the submitted job specification".into(),
                ));
            }
            validate_stored_job(&replacement)?;
            replacement.revision = next_revision(expected_revision)?;
            let encoded = encode_job(&replacement)?;
            let pending = has_pending_outbox(&replacement);
            let replacement_revision = revision_bytes(replacement.revision);
            let expected_revision_bytes = revision_bytes(expected_revision);
            let changed = tx
                .execute(
                    "UPDATE detached_jobs
                        SET revision = ?2, has_pending_outbox = ?3, job_json = ?4
                      WHERE job_id = ?1 AND revision = ?5",
                    params![
                        replacement.job_id.as_str(),
                        replacement_revision.as_slice(),
                        pending,
                        encoded,
                        expected_revision_bytes.as_slice(),
                    ],
                )
                .map_err(raw_sqlite_error)?;
            if changed != 1 {
                let actual = current_revision(&tx, &replacement.job_id)?.unwrap_or_default();
                return Err(DetachedJobError::StaleRevision {
                    job_id: replacement.job_id.clone(),
                    expected: expected_revision,
                    actual,
                });
            }
            tx.commit().map_err(raw_sqlite_error)?;
            Ok(replacement)
        })
    }

    async fn predicate_delivery_receipt(
        &self,
        job_id: &JobId,
        identity: &PredicateDeliveryIdentity,
    ) -> Result<Option<PredicateDeliveryReceipt>, DetachedJobError> {
        self.with_connection(|conn| {
            let tx = conn.transaction().map_err(raw_sqlite_error)?;
            let Some(job) = select_by_id(&tx, job_id)? else {
                return Err(DetachedJobError::NotFound(job_id.clone()));
            };
            let receipt = select_predicate_delivery_receipt(
                &tx,
                job_id,
                identity.idempotency_key().as_str(),
            )?;
            if let Some(receipt) = &receipt {
                ensure_same_predicate_delivery_identity(job_id, &receipt.identity, identity)?;
                validate_predicate_delivery_receipt(&job, identity.idempotency_key(), receipt)?;
            }
            tx.commit().map_err(raw_sqlite_error)?;
            Ok(receipt)
        })
    }

    async fn commit_predicate_delivery(
        &self,
        expected_revision: u64,
        mut replacement: StoredJob,
        commit: PredicateDeliveryCommit,
    ) -> Result<PredicateDeliveryCommitOutcome, DetachedJobError> {
        self.with_connection(|conn| {
            let tx = meerkat_sqlite::begin_immediate(conn).map_err(sqlite_store_error)?;
            let current = select_by_id(&tx, &replacement.job_id)?
                .ok_or_else(|| DetachedJobError::NotFound(replacement.job_id.clone()))?;
            if let Some(receipt) = select_predicate_delivery_receipt(
                &tx,
                &replacement.job_id,
                commit.identity.idempotency_key().as_str(),
            )? {
                ensure_same_predicate_delivery_identity(
                    &replacement.job_id,
                    &receipt.identity,
                    &commit.identity,
                )?;
                validate_predicate_delivery_receipt(
                    &current,
                    commit.identity.idempotency_key(),
                    &receipt,
                )?;
                tx.commit().map_err(raw_sqlite_error)?;
                return Ok(PredicateDeliveryCommitOutcome::Deduplicated {
                    job: current,
                    receipt,
                });
            }
            validate_job_replacement(&current, expected_revision, &replacement)?;
            replacement.revision = next_revision(expected_revision)?;
            let receipt = PredicateDeliveryReceipt {
                job_id: replacement.job_id.clone(),
                identity: commit.identity,
                committed_revision: replacement.revision,
                evaluation: commit.evaluation,
                notification: commit.notification,
            };
            validate_predicate_delivery_receipt(
                &replacement,
                receipt.identity.idempotency_key(),
                &receipt,
            )?;

            let encoded_job = encode_job(&replacement)?;
            let pending = has_pending_outbox(&replacement);
            let replacement_revision = revision_bytes(replacement.revision);
            let expected_revision_bytes = revision_bytes(expected_revision);
            let changed = tx
                .execute(
                    "UPDATE detached_jobs
                        SET revision = ?2, has_pending_outbox = ?3, job_json = ?4
                      WHERE job_id = ?1 AND revision = ?5",
                    params![
                        replacement.job_id.as_str(),
                        replacement_revision.as_slice(),
                        pending,
                        encoded_job,
                        expected_revision_bytes.as_slice(),
                    ],
                )
                .map_err(raw_sqlite_error)?;
            if changed != 1 {
                let actual = current_revision(&tx, &replacement.job_id)?.unwrap_or_default();
                return Err(DetachedJobError::StaleRevision {
                    job_id: replacement.job_id.clone(),
                    expected: expected_revision,
                    actual,
                });
            }
            let encoded_receipt = encode_predicate_delivery_receipt(&receipt)?;
            tx.execute(
                "INSERT INTO detached_job_predicate_deliveries
                    (job_id, delivery_idempotency_key, occurrence_id, runnable,
                     committed_revision, receipt_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    replacement.job_id.as_str(),
                    receipt.identity.idempotency_key().as_str(),
                    receipt.identity.occurrence_id(),
                    receipt.identity.runnable(),
                    replacement_revision.as_slice(),
                    encoded_receipt,
                ],
            )
            .map_err(raw_sqlite_error)?;
            tx.commit().map_err(raw_sqlite_error)?;
            Ok(PredicateDeliveryCommitOutcome::Committed {
                job: replacement,
                receipt,
            })
        })
    }

    async fn list_pending_outbox(
        &self,
        limit: usize,
    ) -> Result<Vec<JobOutboxEntry>, DetachedJobError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_connection(|conn| {
            let mut statement = conn
                .prepare(
                    "SELECT job_id, realm_id, submission_key, revision,
                            has_pending_outbox, job_json
                       FROM detached_jobs
                      WHERE has_pending_outbox = 1
                      ORDER BY job_id",
                )
                .map_err(raw_sqlite_error)?;
            let mut rows = statement.query([]).map_err(raw_sqlite_error)?;
            let mut pending = Vec::with_capacity(limit);
            while pending.len() < limit {
                let Some(row) = rows.next().map_err(raw_sqlite_error)? else {
                    break;
                };
                let job = decode_job_row(row)?;
                for entry in job.outbox.into_iter().filter(|entry| !entry.applied) {
                    pending.push(entry);
                    if pending.len() == limit {
                        break;
                    }
                }
            }
            Ok(pending)
        })
    }

    async fn list_for_origin(
        &self,
        realm_id: &str,
        origin_session_id: &SessionId,
        limit: usize,
    ) -> Result<Vec<StoredJob>, DetachedJobError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_connection(|conn| {
            let mut statement = conn
                .prepare(
                    "SELECT job_id, realm_id, submission_key, revision,
                            has_pending_outbox, job_json
                       FROM detached_jobs
                      WHERE realm_id = ?1
                      ORDER BY job_id",
                )
                .map_err(raw_sqlite_error)?;
            let mut rows = statement.query([realm_id]).map_err(raw_sqlite_error)?;
            // Recovery deliberately requests an unbounded origin view. Avoid
            // turning that semantic limit into an impossible eager allocation.
            let mut jobs = Vec::new();
            while jobs.len() < limit {
                let Some(row) = rows.next().map_err(raw_sqlite_error)? else {
                    break;
                };
                let job = decode_job_row(row)?;
                if &job.spec.origin_session_id == origin_session_id {
                    jobs.push(job);
                }
            }
            Ok(jobs)
        })
    }

    async fn list_all(&self, limit: usize) -> Result<Vec<StoredJob>, DetachedJobError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.with_connection(|conn| {
            let limit = i64::try_from(limit).unwrap_or(i64::MAX);
            let mut statement = conn
                .prepare(
                    "SELECT job_id, realm_id, submission_key, revision,
                            has_pending_outbox, job_json
                       FROM detached_jobs
                      ORDER BY job_id
                      LIMIT ?1",
                )
                .map_err(raw_sqlite_error)?;
            let mut rows = statement.query([limit]).map_err(raw_sqlite_error)?;
            let mut jobs = Vec::new();
            while let Some(row) = rows.next().map_err(raw_sqlite_error)? {
                jobs.push(decode_job_row(row)?);
            }
            Ok(jobs)
        })
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

fn select_by_id(conn: &Connection, job_id: &JobId) -> Result<Option<StoredJob>, DetachedJobError> {
    conn.query_row(
        "SELECT job_id, realm_id, submission_key, revision, has_pending_outbox, job_json
           FROM detached_jobs
          WHERE job_id = ?1",
        [job_id.as_str()],
        decode_job_row_sql,
    )
    .optional()
    .map_err(raw_sqlite_error)?
    .map(decode_checked_row)
    .transpose()
}

fn select_by_submission(
    conn: &Connection,
    realm_id: &str,
    submission_key: &crate::JobSubmissionKey,
) -> Result<Option<StoredJob>, DetachedJobError> {
    conn.query_row(
        "SELECT job_id, realm_id, submission_key, revision, has_pending_outbox, job_json
           FROM detached_jobs
          WHERE realm_id = ?1 AND submission_key = ?2",
        params![realm_id, submission_key.as_str()],
        decode_job_row_sql,
    )
    .optional()
    .map_err(raw_sqlite_error)?
    .map(decode_checked_row)
    .transpose()
}

fn select_predicate_delivery_receipt(
    conn: &Connection,
    job_id: &JobId,
    idempotency_key: &str,
) -> Result<Option<PredicateDeliveryReceipt>, DetachedJobError> {
    let encoded = conn
        .query_row(
            "SELECT job_id, delivery_idempotency_key, occurrence_id, runnable,
                    committed_revision, receipt_json
               FROM detached_job_predicate_deliveries
              WHERE job_id = ?1 AND delivery_idempotency_key = ?2",
            params![job_id.as_str(), idempotency_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, meerkat_sqlite::JsonColumnBytes>(5)?
                        .into_bytes(),
                ))
            },
        )
        .optional()
        .map_err(raw_sqlite_error)?;
    let Some((stored_job_id, stored_key, occurrence_id, runnable, revision, encoded)) = encoded
    else {
        return Ok(None);
    };
    let envelope: PredicateDeliveryReceiptEnvelope =
        serde_json::from_slice(&encoded).map_err(|error| {
            DetachedJobError::Store(format!(
                "stored predicate delivery receipt JSON is invalid: {error}"
            ))
        })?;
    if envelope.format_version != PREDICATE_DELIVERY_RECEIPT_FORMAT_VERSION {
        return Err(DetachedJobError::Store(format!(
            "stored predicate delivery receipt format version {} is unsupported; this binary supports {}",
            envelope.format_version, PREDICATE_DELIVERY_RECEIPT_FORMAT_VERSION
        )));
    }
    let receipt = envelope.receipt;
    if stored_job_id != job_id.as_str()
        || &receipt.job_id != job_id
        || stored_key != idempotency_key
        || occurrence_id != receipt.identity.occurrence_id()
        || runnable != receipt.identity.runnable()
        || revision_from_bytes(&revision)? != receipt.committed_revision
        || receipt.identity.idempotency_key().as_str() != idempotency_key
    {
        return Err(DetachedJobError::Store(format!(
            "predicate delivery row columns disagree with encoded receipt for job {job_id}"
        )));
    }
    Ok(Some(receipt))
}

type EncodedJobRow = (String, String, String, Vec<u8>, bool, Vec<u8>);

fn decode_job_row_sql(row: &rusqlite::Row<'_>) -> Result<EncodedJobRow, rusqlite::Error> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get::<_, meerkat_sqlite::JsonColumnBytes>(5)?
            .into_bytes(),
    ))
}

fn decode_job_row(row: &rusqlite::Row<'_>) -> Result<StoredJob, DetachedJobError> {
    decode_checked_row(decode_job_row_sql(row).map_err(raw_sqlite_error)?)
}

fn decode_checked_row(
    (job_id, realm_id, submission_key, encoded_revision, pending, encoded): EncodedJobRow,
) -> Result<StoredJob, DetachedJobError> {
    let revision = revision_from_bytes(&encoded_revision)?;
    let envelope: StoredJobEnvelope = serde_json::from_slice(&encoded)
        .map_err(|error| DetachedJobError::Store(format!("stored job JSON is invalid: {error}")))?;
    if envelope.format_version != STORED_JOB_FORMAT_VERSION {
        return Err(DetachedJobError::Store(format!(
            "stored job format version {} is unsupported; this binary supports {}",
            envelope.format_version, STORED_JOB_FORMAT_VERSION
        )));
    }
    let job = StoredJob::try_from(envelope.job)?;
    if job.job_id.as_str() != job_id
        || job.spec.realm_id != realm_id
        || job.spec.submission_key.as_str() != submission_key
        || job.revision != revision
        || has_pending_outbox(&job) != pending
    {
        return Err(DetachedJobError::Store(format!(
            "detached job row columns disagree with encoded job {job_id}"
        )));
    }
    validate_stored_job(&job)?;
    Ok(job)
}

fn current_revision(conn: &Connection, job_id: &JobId) -> Result<Option<u64>, DetachedJobError> {
    let encoded = conn
        .query_row(
            "SELECT revision FROM detached_jobs WHERE job_id = ?1",
            [job_id.as_str()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(raw_sqlite_error)?;
    encoded.map(|bytes| revision_from_bytes(&bytes)).transpose()
}

fn encode_job(job: &StoredJob) -> Result<Vec<u8>, DetachedJobError> {
    serde_json::to_vec(&StoredJobEnvelope {
        format_version: STORED_JOB_FORMAT_VERSION,
        job: PersistedStoredJob::from(job),
    })
    .map_err(|error| DetachedJobError::Store(format!("cannot encode stored job: {error}")))
}

fn encode_predicate_delivery_receipt(
    receipt: &PredicateDeliveryReceipt,
) -> Result<Vec<u8>, DetachedJobError> {
    serde_json::to_vec(&PredicateDeliveryReceiptEnvelope {
        format_version: PREDICATE_DELIVERY_RECEIPT_FORMAT_VERSION,
        receipt: receipt.clone(),
    })
    .map_err(|error| {
        DetachedJobError::Store(format!("cannot encode predicate delivery receipt: {error}"))
    })
}

fn has_pending_outbox(job: &StoredJob) -> bool {
    job.outbox.iter().any(|entry| !entry.applied)
}

fn sqlite_store_error(error: meerkat_sqlite::SqliteStoreError) -> DetachedJobError {
    DetachedJobError::Sqlite(error)
}

fn raw_sqlite_error(error: rusqlite::Error) -> DetachedJobError {
    DetachedJobError::Sqlite(error.into())
}

fn revision_bytes(revision: u64) -> [u8; 8] {
    revision.to_be_bytes()
}

fn revision_from_bytes(bytes: &[u8]) -> Result<u64, DetachedJobError> {
    let encoded: [u8; 8] = bytes.try_into().map_err(|_| {
        DetachedJobError::Store(format!(
            "stored detached job revision has {} bytes; expected 8",
            bytes.len()
        ))
    })?;
    Ok(u64::from_be_bytes(encoded))
}

impl From<&StoredJob> for PersistedStoredJob {
    fn from(job: &StoredJob) -> Self {
        Self {
            job_id: job.job_id.clone(),
            spec: PersistedJobSpec::from(&job.spec),
            revision: job.revision,
            machine_state: PersistedMachineState::from(&job.machine_state),
            progress: job.progress.clone(),
            terminal_result: job.terminal_result.clone(),
            subscriptions: job.subscriptions.clone(),
            outbox: job
                .outbox
                .iter()
                .map(PersistedJobOutboxEntry::from)
                .collect(),
        }
    }
}

impl TryFrom<PersistedStoredJob> for StoredJob {
    type Error = DetachedJobError;

    fn try_from(job: PersistedStoredJob) -> Result<Self, Self::Error> {
        Ok(Self {
            job_id: job.job_id,
            spec: JobSpec::from(job.spec),
            revision: job.revision,
            machine_state: DetachedJobMachineState::from(job.machine_state),
            progress: job.progress,
            terminal_result: job.terminal_result,
            subscriptions: job.subscriptions,
            outbox: job
                .outbox
                .into_iter()
                .map(JobOutboxEntry::try_from)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

impl From<&JobSpec> for PersistedJobSpec {
    fn from(spec: &JobSpec) -> Self {
        Self {
            realm_id: spec.realm_id.clone(),
            origin_session_id: spec.origin_session_id.clone(),
            origin_member_id: spec.origin_member_id.clone(),
            execution_intent_id: spec.execution_intent_id.clone(),
            interaction_lineage_id: spec.interaction_lineage_id.clone(),
            tool: spec.tool.clone(),
            runner: spec.runner.clone(),
            runner_specification_ref: spec.runner_specification_ref.clone(),
            restart_class: PersistedRestartClass::from(spec.restart_class),
            canonical_arguments_hash: spec.canonical_arguments_hash.clone(),
            credential_context_refs: spec
                .credential_context_refs
                .iter()
                .map(PersistedCredentialContextRef::from)
                .collect(),
            submission_key: spec.submission_key.clone(),
        }
    }
}

impl From<PersistedJobSpec> for JobSpec {
    fn from(spec: PersistedJobSpec) -> Self {
        Self {
            realm_id: spec.realm_id,
            origin_session_id: spec.origin_session_id,
            origin_member_id: spec.origin_member_id,
            execution_intent_id: spec.execution_intent_id,
            interaction_lineage_id: spec.interaction_lineage_id,
            tool: spec.tool,
            runner: spec.runner,
            runner_specification_ref: spec.runner_specification_ref,
            restart_class: DetachedJobRestartClass::from(spec.restart_class),
            canonical_arguments_hash: spec.canonical_arguments_hash,
            credential_context_refs: spec
                .credential_context_refs
                .into_iter()
                .map(ToolCredentialContextRef::from)
                .collect(),
            submission_key: spec.submission_key,
        }
    }
}

impl From<&ToolCredentialContextRef> for PersistedCredentialContextRef {
    fn from(reference: &ToolCredentialContextRef) -> Self {
        match reference {
            ToolCredentialContextRef::OwningProfile { required_scopes } => Self::OwningProfile {
                required_scopes: required_scopes.clone(),
            },
            ToolCredentialContextRef::AuthBinding {
                auth_binding,
                required_scopes,
            } => Self::AuthBinding {
                auth_binding: auth_binding.clone(),
                required_scopes: required_scopes.clone(),
            },
        }
    }
}

impl From<PersistedCredentialContextRef> for ToolCredentialContextRef {
    fn from(reference: PersistedCredentialContextRef) -> Self {
        match reference {
            PersistedCredentialContextRef::OwningProfile { required_scopes } => {
                Self::OwningProfile { required_scopes }
            }
            PersistedCredentialContextRef::AuthBinding {
                auth_binding,
                required_scopes,
            } => Self::AuthBinding {
                auth_binding,
                required_scopes,
            },
        }
    }
}

impl From<DetachedJobRestartClass> for PersistedRestartClass {
    fn from(value: DetachedJobRestartClass) -> Self {
        match value {
            DetachedJobRestartClass::Adoptable => Self::Adoptable,
            DetachedJobRestartClass::CheckpointResumable => Self::CheckpointResumable,
            DetachedJobRestartClass::Replayable => Self::Replayable,
            DetachedJobRestartClass::NonResumable => Self::NonResumable,
        }
    }
}

impl From<PersistedRestartClass> for DetachedJobRestartClass {
    fn from(value: PersistedRestartClass) -> Self {
        match value {
            PersistedRestartClass::Adoptable => Self::Adoptable,
            PersistedRestartClass::CheckpointResumable => Self::CheckpointResumable,
            PersistedRestartClass::Replayable => Self::Replayable,
            PersistedRestartClass::NonResumable => Self::NonResumable,
        }
    }
}

impl From<DetachedJobPhase> for PersistedPhase {
    fn from(value: DetachedJobPhase) -> Self {
        match value {
            DetachedJobPhase::Unsubmitted => Self::Unsubmitted,
            DetachedJobPhase::Queued => Self::Queued,
            DetachedJobPhase::Claimed => Self::Claimed,
            DetachedJobPhase::Running => Self::Running,
            DetachedJobPhase::WaitingExternal => Self::WaitingExternal,
            DetachedJobPhase::LossObserved => Self::LossObserved,
            DetachedJobPhase::RetryScheduled => Self::RetryScheduled,
            DetachedJobPhase::Succeeded => Self::Succeeded,
            DetachedJobPhase::Failed => Self::Failed,
            DetachedJobPhase::Cancelled => Self::Cancelled,
            DetachedJobPhase::WorkerLost => Self::WorkerLost,
            DetachedJobPhase::NeedsAttention => Self::NeedsAttention,
        }
    }
}

impl From<PersistedPhase> for DetachedJobPhase {
    fn from(value: PersistedPhase) -> Self {
        match value {
            PersistedPhase::Unsubmitted => Self::Unsubmitted,
            PersistedPhase::Queued => Self::Queued,
            PersistedPhase::Claimed => Self::Claimed,
            PersistedPhase::Running => Self::Running,
            PersistedPhase::WaitingExternal => Self::WaitingExternal,
            PersistedPhase::LossObserved => Self::LossObserved,
            PersistedPhase::RetryScheduled => Self::RetryScheduled,
            PersistedPhase::Succeeded => Self::Succeeded,
            PersistedPhase::Failed => Self::Failed,
            PersistedPhase::Cancelled => Self::Cancelled,
            PersistedPhase::WorkerLost => Self::WorkerLost,
            PersistedPhase::NeedsAttention => Self::NeedsAttention,
        }
    }
}

impl From<DetachedJobTerminalKind> for PersistedTerminalKind {
    fn from(value: DetachedJobTerminalKind) -> Self {
        match value {
            DetachedJobTerminalKind::Succeeded => Self::Succeeded,
            DetachedJobTerminalKind::Failed => Self::Failed,
            DetachedJobTerminalKind::Cancelled => Self::Cancelled,
            DetachedJobTerminalKind::WorkerLost => Self::WorkerLost,
            DetachedJobTerminalKind::NeedsAttention => Self::NeedsAttention,
        }
    }
}

impl From<PersistedTerminalKind> for DetachedJobTerminalKind {
    fn from(value: PersistedTerminalKind) -> Self {
        match value {
            PersistedTerminalKind::Succeeded => Self::Succeeded,
            PersistedTerminalKind::Failed => Self::Failed,
            PersistedTerminalKind::Cancelled => Self::Cancelled,
            PersistedTerminalKind::WorkerLost => Self::WorkerLost,
            PersistedTerminalKind::NeedsAttention => Self::NeedsAttention,
        }
    }
}

impl From<&DetachedJobMachineState> for PersistedMachineState {
    fn from(state: &DetachedJobMachineState) -> Self {
        Self {
            lifecycle_phase: PersistedPhase::from(state.lifecycle_phase),
            job_id: state.job_id.clone(),
            restart_class: PersistedRestartClass::from(state.restart_class),
            attempt_count: state.attempt_count,
            current_attempt_id: state.current_attempt_id.clone(),
            current_fence: state.current_fence,
            current_worker_id: state.current_worker_id.clone(),
            lease_expires_at_ms: state.lease_expires_at_ms,
            heartbeat_at_ms: state.heartbeat_at_ms,
            checkpoint_ref: state.checkpoint_ref.clone(),
            runner_handle: state.runner_handle.clone(),
            progress_cursor: state.progress_cursor,
            lease_expired: state.lease_expired,
            retry_due_at_ms: state.retry_due_at_ms,
            cancel_requested: state.cancel_requested,
            delivery_sequence: state.delivery_sequence,
            notification_ids: state.notification_ids.clone(),
            notification_idempotency_keys: state.notification_idempotency_keys.clone(),
            notification_id_by_key: state.notification_id_by_key.clone(),
            notification_delivery_ids: state.notification_delivery_ids.clone(),
            notification_sequences: state.notification_sequences.clone(),
            notification_applied: state.notification_applied.clone(),
            terminal_kind: state.terminal_kind.map(PersistedTerminalKind::from),
            terminal_delivery_sequence: state.terminal_delivery_sequence,
            terminal_delivery_applied: state.terminal_delivery_applied,
        }
    }
}

impl From<PersistedMachineState> for DetachedJobMachineState {
    fn from(mut state: PersistedMachineState) -> Self {
        if state.delivery_sequence == 0 && state.terminal_delivery_sequence > 0 {
            state.delivery_sequence = state.terminal_delivery_sequence;
        }
        Self {
            lifecycle_phase: DetachedJobPhase::from(state.lifecycle_phase),
            job_id: state.job_id,
            restart_class: DetachedJobRestartClass::from(state.restart_class),
            attempt_count: state.attempt_count,
            current_attempt_id: state.current_attempt_id,
            current_fence: state.current_fence,
            current_worker_id: state.current_worker_id,
            lease_expires_at_ms: state.lease_expires_at_ms,
            heartbeat_at_ms: state.heartbeat_at_ms,
            checkpoint_ref: state.checkpoint_ref,
            runner_handle: state.runner_handle,
            progress_cursor: state.progress_cursor,
            lease_expired: state.lease_expired,
            retry_due_at_ms: state.retry_due_at_ms,
            cancel_requested: state.cancel_requested,
            delivery_sequence: state.delivery_sequence,
            notification_ids: state.notification_ids,
            notification_idempotency_keys: state.notification_idempotency_keys,
            notification_id_by_key: state.notification_id_by_key,
            notification_delivery_ids: state.notification_delivery_ids,
            notification_sequences: state.notification_sequences,
            notification_applied: state.notification_applied,
            terminal_kind: state.terminal_kind.map(DetachedJobTerminalKind::from),
            terminal_delivery_sequence: state.terminal_delivery_sequence,
            terminal_delivery_applied: state.terminal_delivery_applied,
        }
    }
}

impl From<&JobOutboxEntry> for PersistedJobOutboxEntry {
    fn from(entry: &JobOutboxEntry) -> Self {
        Self {
            job_id: entry.job_id.clone(),
            delivery_id: Some(entry.delivery_id.clone()),
            delivery_sequence: entry.delivery_sequence,
            payload: Some(entry.payload.clone()),
            terminal_kind: None,
            terminal_result: None,
            targets: entry.targets.clone(),
            applied: entry.applied,
        }
    }
}

impl TryFrom<PersistedJobOutboxEntry> for JobOutboxEntry {
    type Error = DetachedJobError;

    fn try_from(entry: PersistedJobOutboxEntry) -> Result<Self, Self::Error> {
        let payload = match (entry.payload, entry.terminal_result) {
            (Some(payload), None) => payload,
            (None, Some(result)) => {
                if entry
                    .terminal_kind
                    .map(DetachedJobTerminalKind::from)
                    .is_some_and(|kind| kind != result.kind())
                {
                    return Err(DetachedJobError::Store(
                        "legacy terminal outbox kind disagrees with result".into(),
                    ));
                }
                JobOutboxPayload::Terminal(result)
            }
            (Some(_), Some(_)) => {
                return Err(DetachedJobError::Store(
                    "stored outbox entry contains both current and legacy payloads".into(),
                ));
            }
            (None, None) => {
                return Err(DetachedJobError::Store(
                    "stored outbox entry has no payload".into(),
                ));
            }
        };
        let delivery_id = entry.delivery_id.unwrap_or_else(|| match &payload {
            JobOutboxPayload::Terminal(_) => "terminal".to_string(),
            JobOutboxPayload::Notification(notification) => {
                notification.notification_id().as_str().to_string()
            }
        });
        Ok(Self {
            job_id: entry.job_id,
            delivery_id,
            delivery_sequence: entry.delivery_sequence,
            payload,
            targets: entry.targets,
            applied: entry.applied,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    #[test]
    fn revision_encoding_round_trips_the_full_u64_domain() {
        for revision in [1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
            assert_eq!(
                super::revision_from_bytes(&super::revision_bytes(revision)).expect("decode"),
                revision
            );
        }
    }
}
