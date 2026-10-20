//! [`Session::with_inline_images_externalized`] over the transcript shapes a
//! compacted session whose images were written inline reaches a conversion
//! with: camera snapshots in tool results, an image in a runtime notice and
//! in a user message, held in the anchor, in rewrite replacements, in the
//! parent advance a later compaction swallowed, and in the live tail.

use super::*;
use crate::session_store::strand_layout_for_retained_history;
use crate::types::{
    AssistantBlock, BlockAssistantMessage, ContentBlock, ImageData, StopReason, SystemNoticeBlock,
    SystemNoticeKind, SystemNoticeMessage, ToolResult, UserMessage,
};

fn inline_png(data: &str) -> ContentBlock {
    ContentBlock::Image {
        media_type: "image/png".to_string(),
        data: ImageData::Inline {
            data: data.to_string(),
        },
    }
}

/// Base64 of a short payload distinct per `index`.
fn photo(index: usize) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(format!("PNG photo {index}"))
}

/// One camera turn: a user request, a tool call whose result carries two
/// snapshots, a runtime notice carrying a third, and the reply.
fn push_camera_turn(session: &mut Session, turn: usize, photos: &mut Vec<String>) {
    let call_id = format!("toolu_{turn}");
    let snapshots = [photo(photos.len()), photo(photos.len() + 1)];
    let notice = photo(photos.len() + 2);
    photos.extend(snapshots.iter().cloned());
    photos.push(notice.clone());
    session.push(Message::User(UserMessage::text(format!(
        "check the cameras ({turn})"
    ))));
    session.push(Message::BlockAssistant(BlockAssistantMessage::new(
        vec![AssistantBlock::ToolUse {
            id: call_id.clone(),
            name: "camera_snapshot".to_string(),
            args: serde_json::value::RawValue::from_string("{}".to_string()).unwrap(),
            meta: None,
        }],
        StopReason::ToolUse,
    )));
    session.push(Message::tool_results(vec![ToolResult::with_blocks(
        call_id,
        vec![
            ContentBlock::Text {
                text: "two snapshots".to_string(),
            },
            inline_png(&snapshots[0]),
            inline_png(&snapshots[1]),
        ],
        false,
    )]));
    session.push(Message::SystemNotice(SystemNoticeMessage::with_block(
        SystemNoticeKind::Generic,
        None,
        SystemNoticeBlock::RuntimeNotice {
            category: "motion".to_string(),
            detail: Some("the porch camera saw motion".to_string()),
            payload: None,
            content: vec![inline_png(&notice)],
        },
    )));
    session.push(Message::BlockAssistant(BlockAssistantMessage::new(
        vec![AssistantBlock::Text {
            text: format!("all quiet ({turn})"),
            meta: None,
        }],
        StopReason::EndTurn,
    )));
}

/// A session compacted `compactions` times, each compaction swallowing a
/// camera turn appended after the previous one and keeping a user photo in
/// its replacement, with one more photo in the live tail, as the persisted
/// WholeBlob document a conversion decodes. Returns it with every photo it
/// holds, in the order the transform first meets them.
fn compacted_session_with_inline_images(compactions: usize) -> (Session, Vec<String>) {
    let mut session = Session::new();
    let mut photos = Vec::new();
    for turn in 0..compactions {
        push_camera_turn(&mut session, turn, &mut photos);
        let kept = photo(photos.len());
        photos.push(kept.clone());
        let end = session.messages().len();
        session
            .commit_transcript_rewrite(
                TranscriptRewriteSelection::MessageRange { start: 0, end },
                vec![
                    Message::User(UserMessage::text(format!("summary {turn}"))),
                    Message::User(UserMessage::with_blocks(vec![inline_png(&kept)])),
                ],
                TranscriptRewriteReason::new("compaction"),
                Some("test".to_string()),
                None,
            )
            .unwrap();
    }
    let tail = photo(photos.len());
    photos.push(tail.clone());
    session.push(Message::User(UserMessage::with_blocks(vec![inline_png(
        &tail,
    )])));
    if compactions > 4 {
        assert!(
            session
                .retire_transcript_history(crate::TranscriptHistoryRetention::default())
                .unwrap()
                > 0
        );
    }
    let document = serde_json::to_vec(&session).unwrap();
    let decoded = Session::decode_whole_blob_document(&document)
        .unwrap()
        .into_session();
    (decoded, photos)
}

/// Every serialized row and graph edge a layout of `session` writes.
fn laid_out_bytes(session: &Session) -> Result<Vec<Vec<u8>>, crate::SessionStoreError> {
    let history = session
        .validated_transcript_history_state()
        .expect("a validated graph");
    let retained = strand_layout_for_retained_history(session, history.as_ref())?;
    let layout = retained.layout;
    let mut bytes = layout.serialized_anchor;
    for rewrite in layout.rewrites {
        bytes.push(rewrite.serialized_graph_edge);
        bytes.extend(rewrite.serialized_parent_suffix);
        bytes.extend(rewrite.serialized_replacement);
    }
    bytes.extend(layout.serialized_tail);
    Ok(bytes)
}

fn commits(session: &Session) -> Vec<TranscriptRewriteCommit> {
    session
        .transcript_history_state()
        .unwrap()
        .expect("a compacted session has a graph")
        .commits()
        .cloned()
        .collect()
}

fn assert_externalizes(compactions: usize) {
    let (source, photos) = compacted_session_with_inline_images(compactions);
    let source_refusal = laid_out_bytes(&source).expect_err("inline images refuse the layout");
    assert!(
        source_refusal.to_string().contains("carries inline media"),
        "{source_refusal}"
    );

    let externalized = source
        .with_inline_images_externalized()
        .unwrap()
        .expect("the session holds inline images");
    // Every distinct image, once, in the order the transform met it, named
    // by its content address.
    let retired_photos = if compactions > 4 {
        // The bodies of the retired occurrences are gone; their photos were
        // never in the retained graph.
        let retained = source
            .transcript_history_state()
            .unwrap()
            .unwrap()
            .retired_count();
        assert!(retained > 0);
        Some(retained)
    } else {
        None
    };
    let payload_data = externalized
        .payloads
        .iter()
        .map(|payload| payload.data.clone())
        .collect::<Vec<_>>();
    for payload in &externalized.payloads {
        assert_eq!(payload.media_type, "image/png");
        assert_eq!(
            payload.blob_id,
            crate::content_blob_id(&payload.media_type, &payload.data)
        );
    }
    match retired_photos {
        None => assert_eq!(payload_data, photos),
        Some(_) => {
            // A re-anchored graph holds a suffix of the photos: the retained
            // ones in order, ending with the tail photo.
            assert!(!payload_data.is_empty());
            let start = photos.len() - payload_data.len();
            assert_eq!(payload_data, photos[start..]);
        }
    }

    let session = &externalized.session;
    let document = serde_json::to_string(session).unwrap();
    for data in &payload_data {
        assert!(!document.contains(data.as_str()), "{data} is still inline");
    }
    assert_eq!(
        session.transcript_revision().unwrap(),
        source.transcript_revision().unwrap()
    );
    assert_eq!(commits(session), commits(&source));
    assert_eq!(
        session
            .transcript_history_state()
            .unwrap()
            .unwrap()
            .rewrite_prefix(),
        source
            .transcript_history_state()
            .unwrap()
            .unwrap()
            .rewrite_prefix()
    );
    for bytes in laid_out_bytes(session).expect("the externalized session lays out") {
        let row = String::from_utf8(bytes).unwrap();
        for data in &payload_data {
            assert!(!row.contains(data.as_str()), "a laid-out row holds {data}");
        }
    }
    // Every retained revision materializes, its images as blob references.
    for commit in commits(session).iter().skip(retired_photos.unwrap_or(0)) {
        for revision in [&commit.parent_revision, &commit.revision] {
            let rows = session
                .retained_transcript_revision_rows(revision)
                .unwrap()
                .expect("a retained revision");
            let rows = serde_json::to_string(rows.messages()).unwrap();
            for data in &payload_data {
                assert!(!rows.contains(data.as_str()), "revision {revision}");
            }
        }
    }
    // Deterministic, and a fixed point.
    assert_eq!(
        serde_json::to_value(
            &source
                .with_inline_images_externalized()
                .unwrap()
                .unwrap()
                .session
        )
        .unwrap(),
        serde_json::to_value(session).unwrap()
    );
    assert!(session.with_inline_images_externalized().unwrap().is_none());
}

#[test]
fn inline_images_in_a_whole_graph_become_blob_references_and_lay_out() {
    assert_externalizes(2);
}

#[test]
fn inline_images_in_a_re_anchored_graph_become_blob_references_and_lay_out() {
    assert_externalizes(6);
}

#[test]
fn a_session_without_inline_images_is_left_alone() {
    let mut session = Session::new();
    session.push(Message::User(UserMessage::text("no photos here")));
    assert!(session.with_inline_images_externalized().unwrap().is_none());
}
