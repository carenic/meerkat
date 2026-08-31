//! Gate 0 — the committed model-routing handoff log survives a real durable
//! resume through the SQLite session store.
//!
//! The in-crate representation tests prove the envelope and the HeadCanonical
//! head carry the log. This proves the store round-trip actually restores it:
//! an owed handoff must still be owed after a process restart, because the
//! whole point of the log is that a *later* owner realizes it.

#![cfg(feature = "sqlite")]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use meerkat_core::image_generation::{
    SwitchTurnDuration, SwitchTurnIntent, SwitchTurnOrigin, SwitchTurnReasonTextDisposition,
    SwitchTurnRequestId,
};
use meerkat_core::lifecycle::identifiers::RunId;
use meerkat_core::lifecycle::run_primitive::ModelId;
use meerkat_core::session::model_routing_control::{
    ModelRoutingIntentRecordDisposition, SessionModelRoutingControlRecord,
};
use meerkat_core::session_store::{
    IncrementalSessionStore, SessionHead, SessionHeadCas, TranscriptStrandId,
};
use meerkat_core::{Message, Session, SessionLlmIdentity, SessionStore, UserMessage};
use meerkat_store::SqliteSessionStore;
use std::sync::Arc;

fn intent(model: &str) -> SwitchTurnIntent {
    SwitchTurnIntent {
        target_model: ModelId::new(model),
        duration: SwitchTurnDuration::UntilChanged,
        origin: SwitchTurnOrigin::Model {
            reason: SwitchTurnReasonTextDisposition::NotProvided,
        },
    }
}

#[tokio::test]
async fn an_owed_handoff_survives_a_durable_resume() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.sqlite3");

    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("swap me".to_string())));
    let id = session.id().clone();
    let request_id = SwitchTurnRequestId::new(uuid::Uuid::new_v4());
    let run = RunId::new();
    session
        .append_model_routing_control_record(
            SessionModelRoutingControlRecord::request(
                request_id,
                run.clone(),
                intent("claude-opus-5"),
            )
            .expect("durable handoff"),
        )
        .expect("request appends");

    let store = SqliteSessionStore::open(&path).expect("open");
    store.save(&session).await.expect("save session");
    drop(store);

    let store = SqliteSessionStore::open(&path).expect("re-open");
    let resumed = store
        .load(&id)
        .await
        .expect("load resumed session")
        .expect("session present");

    assert_eq!(
        resumed.model_routing_control().disposition_of(&request_id),
        Some(ModelRoutingIntentRecordDisposition::Requested),
        "a resumed session must still owe its committed handoff"
    );
    let owed: Vec<_> = resumed
        .model_routing_control()
        .awaiting_decision()
        .collect();
    assert_eq!(owed.len(), 1, "exactly one handoff is owed");
    assert_eq!(
        owed[0].originating_run_id(),
        &run,
        "the exact originating run must survive resume; it is the receipt key"
    );
    assert_eq!(
        owed[0].intent().target_model,
        ModelId::new("claude-opus-5"),
        "the exact target must survive resume"
    );
}

#[tokio::test]
async fn a_settled_handoff_resumes_as_terminal_and_is_not_re_owed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.sqlite3");

    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("swap me".to_string())));
    let id = session.id().clone();
    let request_id = SwitchTurnRequestId::new(uuid::Uuid::new_v4());
    let run = RunId::new();
    session
        .append_model_routing_control_record(
            SessionModelRoutingControlRecord::request(
                request_id,
                run.clone(),
                intent("claude-opus-5"),
            )
            .expect("durable handoff"),
        )
        .expect("request appends");
    session
        .append_model_routing_control_record(
            SessionModelRoutingControlRecord::ModelRoutingIntentRealized {
                request_id,
                originating_run_id: run,
                intent: intent("claude-opus-5"),
                applied_identity: Box::new(SessionLlmIdentity {
                    model: "claude-opus-5".to_string(),
                    provider: meerkat_core::Provider::Anthropic,
                    self_hosted_server_id: None,
                    provider_params: None,
                    auth_binding: None,
                }),
            },
        )
        .expect("realized appends");

    let store = SqliteSessionStore::open(&path).expect("open");
    store.save(&session).await.expect("save session");
    drop(store);

    let store = SqliteSessionStore::open(&path).expect("re-open");
    let resumed = store
        .load(&id)
        .await
        .expect("load resumed session")
        .expect("session present");

    assert_eq!(
        resumed.model_routing_control().disposition_of(&request_id),
        Some(ModelRoutingIntentRecordDisposition::Realized),
        "a settled handoff must resume terminal"
    );
    assert!(
        resumed
            .model_routing_control()
            .awaiting_decision()
            .next()
            .is_none(),
        "a realized handoff must never be re-owed after resume; that would rotate the model twice"
    );
}

#[tokio::test]
async fn a_session_that_owes_nothing_resumes_with_an_empty_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.sqlite3");

    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("no swap".to_string())));
    let id = session.id().clone();

    let store = SqliteSessionStore::open(&path).expect("open");
    store.save(&session).await.expect("save session");
    drop(store);

    let store = SqliteSessionStore::open(&path).expect("re-open");
    let resumed = store
        .load(&id)
        .await
        .expect("load resumed session")
        .expect("session present");
    assert!(
        resumed.model_routing_control().is_empty(),
        "a session that never asked for a swap owes nothing after resume"
    );
}

// ---------------------------------------------------------------------------
// HeadCanonical: the branch the runtime actually uses for a live session
// ---------------------------------------------------------------------------

/// Seed a head-canonical session through the incremental contract so `save`
/// takes the head-canonical branch instead of falling through to WholeBlob.
async fn seed_head_canonical(inc: &Arc<dyn IncrementalSessionStore>, session: &Session) {
    let root = TranscriptStrandId::root();
    inc.append_messages(session.id(), &root, 0, session.messages())
        .await
        .expect("append root strand rows");
    let head = SessionHead::from_session(session, root, 0).expect("project head");
    inc.save_head(&head, SessionHeadCas::Create)
        .await
        .expect("create head row");
}

fn owed_request(
    request_id: SwitchTurnRequestId,
    run: RunId,
    model: &str,
) -> SessionModelRoutingControlRecord {
    SessionModelRoutingControlRecord::request(request_id, run, intent(model))
        .expect("durable handoff")
}

#[tokio::test]
async fn an_owed_handoff_survives_a_head_canonical_resume() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.sqlite3");

    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("swap me".to_string())));
    let id = session.id().clone();

    let store = Arc::new(SqliteSessionStore::open(&path).expect("open"));
    let inc: Arc<dyn IncrementalSessionStore> = store.clone();
    seed_head_canonical(&inc, &session).await;

    // Now the head row exists, so the append takes the head-canonical branch.
    let request_id = SwitchTurnRequestId::new(uuid::Uuid::new_v4());
    let run = RunId::new();
    session
        .append_model_routing_control_record(owed_request(request_id, run.clone(), "claude-opus-5"))
        .expect("request appends");
    store.save(&session).await.expect("head-canonical save");
    drop(inc);
    drop(store);

    let store = SqliteSessionStore::open(&path).expect("re-open");
    let resumed = store
        .load(&id)
        .await
        .expect("load resumed session")
        .expect("session present");
    assert_eq!(
        resumed.model_routing_control().disposition_of(&request_id),
        Some(ModelRoutingIntentRecordDisposition::Requested),
        "a head-canonical resume must still owe the committed handoff"
    );
    assert_eq!(
        resumed
            .model_routing_control()
            .awaiting_decision()
            .next()
            .expect("one owed handoff")
            .originating_run_id(),
        &run,
        "the exact originating run must survive the head-canonical resume"
    );
}

#[tokio::test]
async fn a_stale_writer_cannot_shrink_or_resurrect_the_committed_handoff_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.sqlite3");

    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("swap me".to_string())));

    let store = Arc::new(SqliteSessionStore::open(&path).expect("open"));
    let inc: Arc<dyn IncrementalSessionStore> = store.clone();
    seed_head_canonical(&inc, &session).await;

    // A stale materialization taken before the handoff was committed.
    let stale = session.clone();

    let request_id = SwitchTurnRequestId::new(uuid::Uuid::new_v4());
    let run = RunId::new();
    session
        .append_model_routing_control_record(owed_request(request_id, run.clone(), "claude-opus-5"))
        .expect("request appends");
    store.save(&session).await.expect("commit the handoff");

    // SHRINK: the stale writer would drop a durably committed obligation.
    let shrink = store.save(&stale).await;
    assert!(
        shrink.is_err(),
        "a save that drops a committed handoff must be refused, got {shrink:?}"
    );

    // RESURRECT: settle the handoff, then replay the still-owed snapshot. This
    // is the dangerous direction — it would make the pre-admission seam rotate
    // the model a second time for an already-applied request.
    let owed_snapshot = session.clone();
    session
        .append_model_routing_control_record(
            SessionModelRoutingControlRecord::ModelRoutingIntentRealized {
                request_id,
                originating_run_id: run,
                intent: intent("claude-opus-5"),
                applied_identity: Box::new(SessionLlmIdentity {
                    model: "claude-opus-5".to_string(),
                    provider: meerkat_core::Provider::Anthropic,
                    self_hosted_server_id: None,
                    provider_params: None,
                    auth_binding: None,
                }),
            },
        )
        .expect("realized appends");
    store.save(&session).await.expect("commit the realization");

    let resurrect = store.save(&owed_snapshot).await;
    assert!(
        resurrect.is_err(),
        "a save that re-owes an already-realized handoff must be refused, got {resurrect:?}"
    );

    let id = session.id().clone();
    drop(inc);
    drop(store);
    let store = SqliteSessionStore::open(&path).expect("re-open");
    let resumed = store
        .load(&id)
        .await
        .expect("load")
        .expect("session present");
    assert_eq!(
        resumed.model_routing_control().disposition_of(&request_id),
        Some(ModelRoutingIntentRecordDisposition::Realized),
        "the committed log must still read as settled after the refused replays"
    );
    assert!(
        resumed
            .model_routing_control()
            .awaiting_decision()
            .next()
            .is_none(),
        "an already-applied handoff must never be re-owed"
    );
}

// ---------------------------------------------------------------------------
// The UNGUARDED lanes both Gate 0 reviewers proved open: WholeBlob (no head
// seeded) and JSONL (which has no head lane at all). The seeded test above
// manufactures the head row that switches the head-canonical guard on, so it
// could never have covered these.
// ---------------------------------------------------------------------------

/// Commit an owed handoff, then a realization, returning the still-owed
/// snapshot a stale writer would replay.
fn owed_and_settled(
    session: &mut Session,
    request_id: SwitchTurnRequestId,
    run: RunId,
) -> (Session, SessionModelRoutingControlRecord) {
    session
        .append_model_routing_control_record(
            SessionModelRoutingControlRecord::request(
                request_id,
                run.clone(),
                intent("claude-opus-5"),
            )
            .expect("durable handoff"),
        )
        .expect("request appends");
    let owed_snapshot = session.clone();
    let realized = SessionModelRoutingControlRecord::ModelRoutingIntentRealized {
        request_id,
        originating_run_id: run,
        intent: intent("claude-opus-5"),
        applied_identity: Box::new(SessionLlmIdentity {
            model: "claude-opus-5".to_string(),
            provider: meerkat_core::Provider::Anthropic,
            self_hosted_server_id: None,
            provider_params: None,
            auth_binding: None,
        }),
    };
    (owed_snapshot, realized)
}

#[tokio::test]
async fn a_stale_wholeblob_writer_cannot_shrink_or_resurrect_the_committed_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.sqlite3");
    let store = SqliteSessionStore::open(&path).expect("open");

    // Deliberately NO head seeding: this is the WholeBlob branch, which is the
    // same lane `an_owed_handoff_survives_a_durable_resume` exercises.
    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("swap me".to_string())));
    let id = session.id().clone();
    store.save(&session).await.expect("first save");

    let stale = session.clone();
    let request_id = SwitchTurnRequestId::new(uuid::Uuid::new_v4());
    let run = RunId::new();
    let (owed_snapshot, realized) = owed_and_settled(&mut session, request_id, run);
    store.save(&session).await.expect("commit the handoff");

    let shrink = store.save(&stale).await;
    assert!(
        shrink.is_err(),
        "a WholeBlob save that drops a committed handoff must be refused, got {shrink:?}"
    );

    session
        .append_model_routing_control_record(realized)
        .expect("realized appends");
    store.save(&session).await.expect("commit the realization");

    let resurrect = store.save(&owed_snapshot).await;
    assert!(
        resurrect.is_err(),
        "a WholeBlob save that re-owes an applied handoff must be refused, got {resurrect:?}"
    );

    let resumed = store
        .load(&id)
        .await
        .expect("load")
        .expect("session present");
    assert_eq!(
        resumed.model_routing_control().disposition_of(&request_id),
        Some(ModelRoutingIntentRecordDisposition::Realized)
    );
    assert!(
        resumed
            .model_routing_control()
            .awaiting_decision()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn a_stale_jsonl_writer_cannot_shrink_or_resurrect_the_committed_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = meerkat_store::JsonlStore::new(dir.path().to_path_buf());

    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("swap me".to_string())));
    let id = session.id().clone();
    store.save(&session).await.expect("first save");

    let stale = session.clone();
    let request_id = SwitchTurnRequestId::new(uuid::Uuid::new_v4());
    let run = RunId::new();
    let (owed_snapshot, realized) = owed_and_settled(&mut session, request_id, run);
    store.save(&session).await.expect("commit the handoff");

    // A transcript-MONOTONIC stale save: the guard already refuses a shrinking
    // transcript, so this isolates the handoff log as the only regression.
    let shrink = store.save(&stale).await;
    assert!(
        shrink.is_err(),
        "a jsonl save that drops a committed handoff must be refused, got {shrink:?}"
    );

    session
        .append_model_routing_control_record(realized)
        .expect("realized appends");
    store.save(&session).await.expect("commit the realization");

    let resurrect = store.save(&owed_snapshot).await;
    assert!(
        resurrect.is_err(),
        "a jsonl save that re-owes an applied handoff must be refused, got {resurrect:?}"
    );

    let resumed = store
        .load(&id)
        .await
        .expect("load")
        .expect("session present");
    assert_eq!(
        resumed.model_routing_control().disposition_of(&request_id),
        Some(ModelRoutingIntentRecordDisposition::Realized),
        "jsonl must resume the settled handoff, not the resurrected one"
    );
}
