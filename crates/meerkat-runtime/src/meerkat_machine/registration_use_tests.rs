//! Every path that hands an existing registration to a new user stamps a new
//! registration use, so an earlier owner's use-gated unregister or discard
//! sees that the registration was taken over. One row per adoption path that
//! no mob-level test reaches.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

use meerkat_core::lifecycle::RunId;
use meerkat_core::lifecycle::core_executor::{CoreApplyOutput, CoreExecutor, CoreExecutorError};
use meerkat_core::lifecycle::run_primitive::{RunApplyBoundary, RunPrimitive};
use meerkat_core::lifecycle::run_receipt::RunBoundaryReceiptDraft;

struct NoopExecutor;

#[async_trait::async_trait]
impl CoreExecutor for NoopExecutor {
    async fn apply(
        &mut self,
        run_id: RunId,
        primitive: RunPrimitive,
    ) -> Result<CoreApplyOutput, CoreExecutorError> {
        Ok(CoreApplyOutput::with_untyped_snapshot(
            RunBoundaryReceiptDraft {
                run_id,
                boundary: RunApplyBoundary::RunStart,
                contributing_input_ids: primitive.contributing_input_ids().to_vec(),
                conversation_digest: None,
                message_count: 0,
            },
            None,
            None,
        ))
    }

    async fn cancel_after_boundary(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }

    async fn stop_runtime_executor(&mut self, _reason: String) -> Result<(), CoreExecutorError> {
        Ok(())
    }
}

/// The session's current registration and the use it carries now.
async fn current_use(
    machine: &MeerkatMachine,
    session_id: &SessionId,
) -> (
    RuntimeSessionRegistrationWitness,
    RuntimeSessionRegistrationUse,
) {
    let witness = machine
        .current_session_registration_witness(session_id)
        .await
        .expect("the session is registered");
    let registration_use = machine
        .session_registration_use(&witness)
        .await
        .expect("the registration is current");
    (witness, registration_use)
}

/// An archive resume whose prepared candidate loses its publication to a
/// registration that published first adopts that registration as it is.
#[tokio::test]
async fn an_archive_resume_that_adopts_the_published_registration_stamps_a_new_use() {
    let store: Arc<dyn crate::store::RuntimeStore> =
        Arc::new(crate::store::InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> =
        Arc::new(meerkat_store::MemoryBlobStore::new());
    let machine = MeerkatMachine::persistent(store, blob_store).expect("persistent runtime");
    let session_id = SessionId::new();
    let prepared = machine
        .prepare_archive_session_registration(session_id.clone(), false, None)
        .await
        .expect("an archive candidate");
    machine
        .register_session(session_id.clone())
        .await
        .expect("the ordinary registration publishes first");
    let (witness, before) = current_use(&machine, &session_id).await;

    let outcome = machine
        .commit_prepared_archive_session_registration_under_transaction(prepared)
        .await
        .expect("the candidate adopts the published registration");

    assert!(
        matches!(
            outcome,
            super::session_management::RegisterSessionInnerOutcome::Existing
        ),
        "the candidate lost its publication and reused the registration"
    );
    let after = machine
        .session_registration_use(&witness)
        .await
        .expect("the same registration stays current");
    assert_ne!(
        after, before,
        "the archive resume took the registration over, so it carries a new use"
    );
}

/// Recovering the actor of a live committed executor attachment hands the
/// registration to the recovering caller.
#[tokio::test]
async fn an_attached_actor_recovery_stamps_a_new_use() {
    let machine = Arc::new(MeerkatMachine::ephemeral());
    let session_id = SessionId::new();
    let mut prepared = machine
        .prepare_session_materialization(session_id.clone())
        .await
        .expect("prepare the actor materialization");
    crate::begin_session_runtime_actor_materialization(prepared.bindings())
        .expect("claim the initial actor construction")
        .commit()
        .expect("record the initial actor");
    let pending = match prepared
        .ensure_executor_attachment(|_witness| Box::new(NoopExecutor))
        .await
        .expect("attach an executor through the actor claim")
    {
        EnsureRuntimeExecutorAttachment::Pending(pending) => pending,
        EnsureRuntimeExecutorAttachment::Existing(witness) => {
            panic!("a fresh actor materialization reused {witness:?}")
        }
    };
    let attachment = pending.commit().await.expect("commit the attachment");
    let (witness, before) = current_use(&machine, &session_id).await;

    let recovery = machine
        .prepare_attached_session_actor_recovery(&attachment)
        .await
        .expect("open the actor-only recovery");

    let after = machine
        .session_registration_use(&witness)
        .await
        .expect("the same registration stays current");
    assert_ne!(
        after, before,
        "the actor recovery took the registration over, so it carries a new use"
    );
    drop(recovery);
    machine
        .unregister_session(&session_id)
        .await
        .expect("remove the fixture registration");
}

/// An actor-materialization claim granted for a given registration, which
/// registers nothing itself, hands that registration to the caller.
#[tokio::test]
async fn a_materialization_claim_for_a_registration_stamps_a_new_use() {
    let machine = Arc::new(MeerkatMachine::ephemeral());
    let session_id = SessionId::new();
    machine
        .register_session(session_id.clone())
        .await
        .expect("a registration without a runtime owner");
    let (witness, before) = current_use(&machine, &session_id).await;

    let mut prepared = machine
        .prepare_local_session_materialization_for_registration(witness.clone())
        .await
        .expect("claim the registration's actor materialization");

    let after = machine
        .session_registration_use(&witness)
        .await
        .expect("the same registration stays current");
    assert_ne!(
        after, before,
        "the materialization claim took the registration over, so it carries a new use"
    );
    prepared
        .rollback_now()
        .await
        .expect("release the materialization claim");
}

/// The refusal check that opens every existing-entry reuse branch of session
/// management: a registration being unregistered is refused, and any other
/// existing registration is reused as it is.
const REUSE_BRANCH_ANCHOR: &str = "existing.registration_blocked_by_unregister(&session_id)";

/// The stamp every reuse branch makes before it reuses the registration.
const REUSE_STAMP: &str = "existing.stamp_adoption(self.next_registration_use());";

/// The reuse branches session_management.rs has. The count includes the
/// wasm32 forms (no native test reaches them) and the two insert-race
/// re-checks the registration transaction keeps unreachable. A new reuse
/// branch must stamp, and raises this.
const REUSE_BRANCHES: usize = 7;

fn trimmed_line<'a>(lines: &[&'a str], index: usize) -> &'a str {
    match lines.get(index) {
        Some(line) => line.trim(),
        None => "",
    }
}

/// The number of reuse branches in `source`, or the 1-based lines of the
/// branches whose first statement after the refusal check is not the stamp.
fn reuse_branch_census(source: &str) -> Result<usize, Vec<usize>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut branches = 0;
    let mut unstamped = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains(REUSE_BRANCH_ANCHOR) {
            continue;
        }
        branches += 1;
        let refusal_closed = trimmed_line(&lines, index + 1) == "return Err(error);"
            && trimmed_line(&lines, index + 2) == "}";
        let mut next = index + 3;
        while trimmed_line(&lines, next).starts_with("//") {
            next += 1;
        }
        if !(refusal_closed && trimmed_line(&lines, next) == REUSE_STAMP) {
            unstamped.push(index + 1);
        }
    }
    if unstamped.is_empty() {
        Ok(branches)
    } else {
        Err(unstamped)
    }
}

/// Every existing-entry reuse branch stamps a new use before it reuses the
/// registration. A source census, so a native run also covers the wasm32
/// forms, the insert-race re-checks and any new reuse branch.
#[test]
fn every_existing_entry_reuse_branch_stamps_a_new_use() {
    assert_eq!(
        reuse_branch_census(include_str!("session_management.rs")),
        Ok(REUSE_BRANCHES),
        "every reuse branch stamps first, and a new branch raises the pinned count"
    );
}

/// The census sees a reuse branch that lost its stamp, and a reuse branch
/// added without one.
#[test]
fn the_reuse_branch_census_sees_a_dropped_stamp_and_an_unstamped_branch() {
    let source = include_str!("session_management.rs");
    let first_stamp_dropped = source.replacen(REUSE_STAMP, "", 1);
    assert!(
        matches!(reuse_branch_census(&first_stamp_dropped), Err(lines) if lines.len() == 1),
        "a reuse branch without its stamp fails the census"
    );
    let unstamped_branch_added = format!(
        "{source}\nfn added(&self) {{\n    if let Some(error) = {REUSE_BRANCH_ANCHOR} {{\n        return Err(error);\n    }}\n    existing.reuse();\n}}\n"
    );
    assert!(
        matches!(reuse_branch_census(&unstamped_branch_added), Err(lines) if lines.len() == 1),
        "an added reuse branch without a stamp fails the census"
    );
}
