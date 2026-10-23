//! A check a host runs against its own store: Core's host store adapter puts
//! documents far larger than one batch through the realm (the adapter's
//! chunk scopes), and they come back byte-exact after a restart.
//!
//! [`large_documents`] drives the real adapter ([`SpiRealm`] and
//! [`SpiRuntimeStore`]) over handles the host opens with its
//! [`HostStoreOpener`]: a 20 MiB WholeBlob document is committed as a
//! session's base, written again as a run's round commit (a provisional
//! tail), and re-based after a compaction. Then the process is dropped, a
//! new one opens over the same realm and loads the run, and durable-tail
//! recovery reads it back. Every document must come back byte-exact.
//!
//! The store sees what Core writes for such documents: values over the
//! adapter's inline limit go to chunk scopes, written in pre-batches of at
//! most [`super::realm::CHUNK_ROWS_PER_BATCH`] rows of at most
//! [`meerkat_core::host_store::MAX_ROW_BYTES`] each before the one batch that
//! references them, and replaced chunks are deleted. A restart reads them
//! back and verifies each chunk's length and digest.
//!
//! The check opens each process with a takeover, so a run killed earlier
//! cannot block it; the restart therefore waits out the dropped process's
//! recorded lease ([`meerkat_core::host_store::FenceTiming::lease`]).
//! Start it on an empty realm.

use std::fmt::Write as _;
use std::sync::Arc;

use async_trait::async_trait;
use meerkat_core::host_store::{AcquireMode, HostStore, HostStoreError};
use meerkat_core::lifecycle::RunId;
use meerkat_core::types::{Message, UserMessage};
use meerkat_core::{
    RunResumePoint, Session, SessionRunCursor, TranscriptRewriteReason, TranscriptRewriteSelection,
};

use super::{SpiRealm, SpiRealmConfig, SpiRuntimeStore};
use crate::identifiers::LogicalRuntimeId;
use crate::recovery::{DurableTailRecoveryOutcome, recover_durable_tail};
use crate::store::{
    PreparedWholeBlobProvisionalTail, PreparedWholeBlobRunRebase, RuntimeStore,
    SerializedSessionSnapshot, VerifiedCommittedWholeBlobPayload, WholeBlobRunRebaseOutcome,
};

/// The size of the large message in every document of the check.
pub const LARGE_DOCUMENT_BYTES: usize = 20 * 1024 * 1024;

/// Opens handles over one host store realm. Each handle stands for a new
/// process: it holds nothing of the handles before it (for a remote backend,
/// a new client over the same dataset).
#[async_trait]
pub trait HostStoreOpener: Send + Sync {
    /// Open a new handle over the realm.
    async fn open(&self) -> Result<Arc<dyn HostStore>, HostStoreError>;
}

/// What the check found wrong, and at which step.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{step}: {detail}")]
pub struct ChapterFailure {
    /// The step: `open`, `base`, `round_commit`, `rebase`, `restart` or
    /// `recovery`.
    pub step: &'static str,
    /// What went wrong.
    pub detail: String,
}

fn failed(step: &'static str, detail: impl std::fmt::Display) -> ChapterFailure {
    ChapterFailure {
        step,
        detail: detail.to_string(),
    }
}

fn ensure(
    step: &'static str,
    holds: bool,
    detail: impl FnOnce() -> String,
) -> Result<(), ChapterFailure> {
    if holds {
        Ok(())
    } else {
        Err(ChapterFailure {
            step,
            detail: detail(),
        })
    }
}

/// One process of the check: a realm over a new handle, and its runtime
/// store.
async fn start(
    opener: &dyn HostStoreOpener,
    step: &'static str,
) -> Result<(Arc<SpiRealm>, SpiRuntimeStore), ChapterFailure> {
    let host = opener.open().await.map_err(|error| failed(step, error))?;
    let config = SpiRealmConfig {
        acquire: AcquireMode::Takeover,
        ..SpiRealmConfig::default()
    };
    let realm = SpiRealm::open(host, config)
        .await
        .map_err(|error| failed(step, error))?;
    let store = SpiRuntimeStore::open(Arc::clone(&realm))
        .await
        .map_err(|error| failed(step, error))?;
    Ok((realm, store))
}

/// A user message of [`LARGE_DOCUMENT_BYTES`] numbered lines, so a chunk
/// read back out of order or short does not match.
fn large_message() -> Message {
    let mut text = String::with_capacity(LARGE_DOCUMENT_BYTES + 32);
    let mut line = 0_u64;
    while text.len() < LARGE_DOCUMENT_BYTES {
        let _ = writeln!(text, "line {line:010} of the large document");
        line += 1;
    }
    Message::User(UserMessage::text(text))
}

fn encoded(step: &'static str, session: &Session) -> Result<Vec<u8>, ChapterFailure> {
    session
        .to_persisted_artifact()
        .map(|artifact| Vec::clone(&artifact.bytes_arc()))
        .map_err(|error| failed(step, error))
}

/// Put a 20 MiB WholeBlob document through a base commit, a run's round
/// commit and a re-base after a compaction, restart over the same realm, and
/// read every document back byte-exact, through the run's durable-tail
/// recovery as well.
///
/// # Errors
/// The first step that failed, with what it found.
pub async fn large_documents(opener: &dyn HostStoreOpener) -> Result<(), ChapterFailure> {
    let (realm, store) = start(opener, "open").await?;

    // The base: a small message and the large one.
    let mut base = Session::new();
    base.push(Message::User(UserMessage::text("context to compact")));
    base.push(large_message());
    let session_id = base.id().clone();
    let runtime_id = LogicalRuntimeId::for_session(&session_id);
    let base_bytes = encoded("base", &base)?;
    store
        .commit_session_snapshot(
            &runtime_id,
            SerializedSessionSnapshot {
                session_snapshot: Arc::new(base_bytes.clone()),
            },
        )
        .await
        .map_err(|error| failed("base", error))?;
    let committed = store
        .load_committed_whole_blob_snapshot(&runtime_id)
        .await
        .map_err(|error| failed("base", error))?
        .ok_or_else(|| failed("base", "the committed base is missing"))?;
    let base_authority = committed.authority().clone();
    ensure(
        "base",
        committed.session().messages() == base.messages(),
        || "the committed base does not read back as written".to_string(),
    )?;

    // A run's round commit: the base plus its answer, with the run cursor.
    let run_id = RunId::new();
    let mut live = base.clone();
    live.push(Message::User(UserMessage::text("the run's answer")));
    live.set_run_cursor(&SessionRunCursor::new(
        run_id.clone(),
        Vec::new(),
        0,
        RunResumePoint::AwaitingModel,
    ))
    .map_err(|error| failed("round_commit", error))?;
    let first_tail = store
        .write_prepared_whole_blob_provisional_tail(
            &runtime_id,
            PreparedWholeBlobProvisionalTail::prepare_from_session(
                base_authority,
                run_id.clone(),
                1,
                &live,
            )
            .map_err(|error| failed("round_commit", error))?,
        )
        .await
        .map_err(|error| failed("round_commit", error))?;
    let written_tail = store
        .load_whole_blob_provisional_tail(&runtime_id)
        .await
        .map_err(|error| failed("round_commit", error))?
        .ok_or_else(|| failed("round_commit", "the provisional tail is missing"))?;
    ensure(
        "round_commit",
        written_tail.candidate_bytes() == encoded("round_commit", &live)?.as_slice(),
        || "the round commit does not read back as written".to_string(),
    )?;

    // A compaction of the small message, then the re-base: the large one
    // stays, so both re-based documents are large again.
    let mut compacted = live;
    let parent = compacted
        .transcript_revision()
        .map_err(|error| failed("rebase", error))?;
    let commit = compacted
        .commit_transcript_rewrite(
            TranscriptRewriteSelection::MessageRange { start: 0, end: 1 },
            vec![Message::User(UserMessage::compaction_summary(
                "compacted context",
            ))],
            TranscriptRewriteReason::new("compaction"),
            Some("large-document-check".to_string()),
            Some(parent),
        )
        .map_err(|error| failed("rebase", error))?;
    let payload = VerifiedCommittedWholeBlobPayload::from_committed(
        &session_id,
        store
            .load_committed_whole_blob_snapshot(&runtime_id)
            .await
            .map_err(|error| failed("rebase", error))?
            .ok_or_else(|| failed("rebase", "the committed base is missing"))?,
    )
    .map_err(|error| failed("rebase", error))?;
    let rebase = PreparedWholeBlobRunRebase::prepare(
        payload,
        Some(first_tail),
        run_id.clone(),
        compacted.clone(),
        std::slice::from_ref(&commit),
    )
    .map_err(|error| failed("rebase", error))?;
    let (rebased, tail) = match store
        .commit_prepared_whole_blob_run_rebase(&runtime_id, rebase.store_parts())
        .await
        .map_err(|error| failed("rebase", error))?
    {
        WholeBlobRunRebaseOutcome::Applied { committed, tail } => (committed, tail),
        WholeBlobRunRebaseOutcome::Stale => {
            return Err(failed(
                "rebase",
                "the re-base answered Stale on a quiet realm",
            ));
        }
    };
    let candidate_bytes = encoded("rebase", &compacted)?;
    let mut successor = compacted.clone();
    successor.clear_run_cursor();

    // The process ends without its boundary commit; a new one opens over
    // the same realm and loads the run.
    drop(store);
    drop(realm);
    let (_realm, store) = start(opener, "restart").await?;
    let committed = store
        .load_committed_whole_blob_snapshot(&runtime_id)
        .await
        .map_err(|error| failed("restart", error))?
        .ok_or_else(|| failed("restart", "the re-based base is missing"))?;
    ensure("restart", committed.authority() == &rebased, || {
        format!(
            "the base after the restart is {:?}, not the re-based {rebased:?}",
            committed.authority()
        )
    })?;
    ensure(
        "restart",
        committed.session().messages() == successor.messages(),
        || "the re-based base does not read back as written".to_string(),
    )?;
    let loaded_tail = store
        .load_whole_blob_provisional_tail(&runtime_id)
        .await
        .map_err(|error| failed("restart", error))?
        .ok_or_else(|| failed("restart", "the re-based tail is missing"))?;
    ensure("restart", loaded_tail.authority() == &tail, || {
        format!(
            "the tail after the restart is {:?}, not the re-based {tail:?}",
            loaded_tail.authority()
        )
    })?;
    ensure(
        "restart",
        loaded_tail.candidate_bytes() == candidate_bytes.as_slice(),
        || "the re-based tail does not read back as written".to_string(),
    )?;

    // Recovery reads the run back, continued or committed, with the large
    // message intact.
    let recovered = match recover_durable_tail(&store, &session_id)
        .await
        .map_err(|error| failed("recovery", error))?
    {
        DurableTailRecoveryOutcome::Continue { candidate, .. } => *candidate,
        DurableTailRecoveryOutcome::Committed { recovered, .. } => *recovered,
        other => {
            return Err(failed(
                "recovery",
                format!("the run's tail must continue or commit, it answered {other:?}"),
            ));
        }
    };
    let large = large_message();
    ensure(
        "recovery",
        recovered.messages().iter().any(|message| message == &large),
        || "the recovered run lost its large message".to_string(),
    )?;
    Ok(())
}
