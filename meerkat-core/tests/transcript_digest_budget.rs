//! Turn-boundary digest budget: the work a save boundary spends on transcript
//! digests must not depend on how big the transcript is.
//!
//! `session_content_digest_computations()` counts full O(document)
//! canonical-JSON + SHA-256 passes over session content. It is a `thread_local`
//! counter, so every measurement here stays on one thread and inside one test
//! function.
//!
//! The production defect these tests pin: an identical one-word turn measured
//! 60 s at 14 MB and over 180 s at 94 MB, because each turn boundary recomputed
//! the whole-document digest a handful of times. Asserting "fewer digests" is
//! not enough — a constant number of O(document) passes still scales with the
//! document. These tests assert the counted passes are EQUAL at two very
//! different transcript sizes, and that the steady-state boundary count is
//! zero.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use meerkat_core::session_store::{SessionHead, TranscriptStrandId, append_only_save_guard};
use meerkat_core::types::{
    AssistantBlock, BlockAssistantMessage, Message, StopReason, UserMessage,
};
use meerkat_core::{Session, session_content_digest_computations};

const SMALL: usize = 8;
const LARGE: usize = 2_000;

fn user(text: &str) -> Message {
    Message::User(UserMessage::text(text))
}

fn assistant(text: &str) -> Message {
    Message::BlockAssistant(BlockAssistantMessage::new(
        vec![AssistantBlock::Text {
            text: text.to_string(),
            meta: None,
        }],
        StopReason::EndTurn,
    ))
}

/// A session with `turns` prior conversational turns.
fn session_with_turns(turns: usize) -> Session {
    let mut session = Session::new();
    session.set_system_prompt("system".to_string());
    for index in 0..turns {
        session.push(user(&format!(
            "question {index} with some body text to make the message non-trivial"
        )));
        session.push(assistant(&format!(
            "answer {index} with some body text to make the message non-trivial"
        )));
    }
    session
}

fn strand() -> TranscriptStrandId {
    TranscriptStrandId::root()
}

/// One ordinary turn boundary: append the turn, guard the save against the
/// previously persisted row, project the durable head row.
fn boundary_save_digest_count(turns: usize) -> u64 {
    let mut live = session_with_turns(turns);
    // Steady state: this session has already been saved at least once, so the
    // previous row and the live document are both warm. Two warm-up boundaries
    // keep the measurement out of first-sight seeding.
    let mut previous = live.clone();
    for warmup in 0..2 {
        append_only_save_guard(&live, Some(&previous)).expect("warm-up guard");
        SessionHead::from_session(&live, strand(), 0).expect("warm-up head");
        previous = live.clone();
        live.push(user(&format!("warm-up {warmup}")));
        live.push(assistant("warm-up reply"));
    }

    let before = session_content_digest_computations();
    append_only_save_guard(&live, Some(&previous)).expect("boundary guard");
    SessionHead::from_session(&live, strand(), 0).expect("boundary head");
    session_content_digest_computations() - before
}

#[test]
fn turn_boundary_digest_count_is_independent_of_transcript_size() {
    let small = boundary_save_digest_count(SMALL);
    let large = boundary_save_digest_count(LARGE);
    println!(
        "boundary-save full-document digest passes: {SMALL} turns => {small}, {LARGE} turns => {large}"
    );
    assert_eq!(
        small, large,
        "turn-boundary digest work must not depend on transcript size \
         ({SMALL} turns => {small} passes, {LARGE} turns => {large} passes)"
    );
}

#[test]
fn steady_state_turn_boundary_spends_no_full_document_digest() {
    assert_eq!(
        boundary_save_digest_count(LARGE),
        0,
        "a warm boundary save must serve every transcript digest from the \
         incremental accumulator"
    );
}

/// A single append must not re-hash the transcript it appended to.
#[test]
fn append_digest_count_is_independent_of_transcript_size() {
    fn measure(turns: usize) -> u64 {
        let mut live = session_with_turns(turns);
        // Seed: the first digest of a materialized session is the one
        // mandatory full pass per process.
        let seeded = live
            .transcript_content_digest()
            .expect("seed transcript digest");
        assert!(seeded.starts_with("sha256:"));
        let before = session_content_digest_computations();
        live.push(user("one word"));
        let after_append = live
            .transcript_content_digest()
            .expect("append transcript digest");
        assert_ne!(seeded, after_append);
        session_content_digest_computations() - before
    }

    let small = measure(SMALL);
    let large = measure(LARGE);
    println!(
        "append full-document digest passes: {SMALL} turns => {small}, {LARGE} turns => {large}"
    );
    assert_eq!(small, large);
    assert_eq!(large, 0, "an append must not re-hash the whole transcript");
}

/// The two save-guard acceptance branches that are not the plain prefix check
/// still have a size-independent digest budget. They are resume-shaped, not
/// turn-shaped, and they hash derived vectors (a suffix, a filtered
/// subsequence) that no prefix midstate can serve — so their budget is a small
/// CONSTANT number of full passes, and the point of pinning it is that the
/// constant does not grow.
#[test]
fn system_context_append_branch_digest_budget_is_constant() {
    fn measure(turns: usize) -> u64 {
        let mut previous = session_with_turns(turns);
        previous.set_system_prompt("system".to_string());
        let mut live = previous.clone();
        // A runtime system-context append rewrites message 0, which is exactly
        // the shape the plain prefix check cannot admit.
        live.set_system_prompt("system\n\n---\n\nruntime context".to_string());
        live.push(user("after refresh"));
        let before = session_content_digest_computations();
        let _ = append_only_save_guard(&live, Some(&previous));
        session_content_digest_computations() - before
    }

    let small = measure(SMALL);
    let large = measure(LARGE);
    println!(
        "system-context-append branch digest passes: {SMALL} turns => {small}, {LARGE} turns => {large}"
    );
    assert_eq!(
        small, large,
        "the system-context-append acceptance branch must keep a constant digest budget"
    );
}

#[test]
fn synthetic_notice_refresh_branch_digest_budget_is_constant() {
    use meerkat_core::types::{SystemNoticeBlock, SystemNoticeKind, SystemNoticeMessage};

    fn notice(server: &str) -> Message {
        Message::SystemNotice(SystemNoticeMessage::with_block(
            SystemNoticeKind::McpPending,
            None,
            SystemNoticeBlock::Mcp {
                server_id: Some(server.to_string()),
                operation: None,
                phase: None,
                persisted: false,
                detail: None,
                pending_sources: Vec::new(),
            },
        ))
    }

    fn measure(turns: usize) -> u64 {
        let mut previous = session_with_turns(turns);
        previous.push(notice("mcp pending"));
        let mut live = previous.clone();
        live.replace_synthetic_notices(SystemNoticeKind::McpPending, vec![notice("mcp ready")])
            .expect("synthetic notice refresh");
        let before = session_content_digest_computations();
        let _ = append_only_save_guard(&live, Some(&previous));
        session_content_digest_computations() - before
    }

    let small = measure(SMALL);
    let large = measure(LARGE);
    println!(
        "synthetic-notice-refresh branch digest passes: {SMALL} turns => {small}, {LARGE} turns => {large}"
    );
    assert_eq!(
        small, large,
        "the synthetic-notice-refresh acceptance branch must keep a constant digest budget"
    );
}

/// History-bearing sessions (anything that ever compacted or was rewritten)
/// used to pay TWO full graph validations plus a full head digest on every
/// appended batch, each of which hashes every retained revision body. An
/// append onto an already-validated graph proves nothing new about the
/// retained bodies, so the steady-state budget must be flat.
#[test]
fn history_bearing_append_digest_count_is_independent_of_transcript_size() {
    use meerkat_core::service::{TranscriptRewriteReason, TranscriptRewriteSelection};

    fn measure(turns: usize) -> u64 {
        let mut live = session_with_turns(turns);
        let end = live.messages().len();
        live.commit_transcript_rewrite(
            TranscriptRewriteSelection::MessageRange {
                start: end - 1,
                end,
            },
            vec![assistant("audited replacement")],
            TranscriptRewriteReason::new("unit-test"),
            Some("unit-test".to_string()),
            None,
        )
        .expect("audited rewrite");
        assert!(
            live.transcript_history_state()
                .expect("history state decodes")
                .is_some(),
            "fixture must carry a retained transcript graph"
        );
        // Warm up: the first append after a rewrite reseeds, later appends are
        // the steady state every subsequent turn sees.
        for warmup in 0..3 {
            live.push(user(&format!("warm-up {warmup}")));
        }

        let before = session_content_digest_computations();
        live.push(user("one word"));
        let after = session_content_digest_computations() - before;

        // The graph must still be exactly coherent with the live transcript.
        let state = live
            .transcript_history_state()
            .expect("history state decodes")
            .expect("history state present");
        assert_eq!(
            state.head,
            live.transcript_content_digest().expect("live digest"),
            "graph head must track the live transcript"
        );
        live.validate_transcript_history_state()
            .expect("graph must still validate after the fast-path appends");
        after
    }

    let small = measure(SMALL);
    let large = measure(LARGE);
    println!(
        "history-bearing append full-document digest passes: {SMALL} turns => {small}, {LARGE} turns => {large}"
    );
    assert_eq!(
        small, large,
        "appending to a history-bearing session must not scale with the retained graph"
    );
}
