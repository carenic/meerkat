//! Surface implementation of [`LiveProjectionSink`].
//!
//! Bridges the runtime-side projection contract (A1-A6, A14) into canonical
//! Meerkat semantic state via [`SessionRuntime`]. The runtime crate defines
//! the trait shape; this module holds the only production implementation.
//!
//! Each sink method maps to an existing `SessionRuntime` seam:
//! - User transcripts -> `append_realtime_transcript_event`
//!   (`UserTranscriptFinal`) when a `provider_item_id` is present, so the
//!   realtime transcript layer's idempotent ordering owns dedup. Falls back
//!   to `append_external_user_content` when the provider does not surface a
//!   stable item id (legacy partial-final shape).
//! - Assistant deltas -> `append_realtime_transcript_event` (preserves identity)
//! - Assistant finals -> buffered until `signal_turn_completed` flushes them
//!   with the authoritative `stop_reason`/`usage` from `TurnCompleted`. The
//!   provider stamps best-effort sentinels onto `AssistantTranscriptFinal`
//!   (it does not have stop_reason/usage atomically with that event), so
//!   committing canonical history at final-time would persist the sentinel.
//!   We defer (P1#1).
//! - Truncation -> `append_realtime_transcript_event` (AssistantTranscriptTruncated)
//!   carrying the real `response_id` and `content_index` rather than fabricating
//!   empties (P1#3). Missing `response_id` surfaces as a typed Rejected.
//! - Turn-interrupt -> `interrupt_live_with_machine_authority`
//! - Turn-completed -> flushes the buffered assistant final with the
//!   authoritative stop_reason/usage. Orphan completions (no buffered final)
//!   commit an empty assistant block — still correct because no transcript
//!   was produced.
//! - Terminal error -> tracing log + best-effort channel termination
//!
//! Identity-fidelity note: the host trait now passes `LiveTranscriptIdentity`
//! end-to-end (closes A11 at the trait layer). Assistant deltas can populate
//! `RealtimeTranscriptEvent::AssistantTextDelta` with the real
//! `response_id` / `delta_id` / `previous_item_id` / `content_index` rather
//! than synthesizing empty defaults.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use async_trait::async_trait;
use meerkat_core::RealtimeTranscriptEvent;
use meerkat_core::live_adapter::LiveAdapterErrorCode;
use meerkat_core::types::{
    AssistantBlock, ContentInput, SessionId, StopReason, TranscriptSource, Usage,
};
use meerkat_live::{LiveProjectionError, LiveProjectionSink, LiveTranscriptIdentity};

use crate::session_runtime::SessionRuntime;

/// One buffered assistant content fragment awaiting an authoritative
/// `signal_turn_completed` flush.
///
/// T6/T10: typed lane discriminant so the same per-response buffer can
/// hold display text and spoken transcript and flush them in
/// arrival order as distinct `AssistantBlock` variants. Adjacent runs of
/// the same lane collapse into one `AssistantBlock` at flush time so
/// readers do not see one block per delta.
#[derive(Debug, Clone)]
enum PendingAssistantContent {
    /// Display lane — flushes as `AssistantBlock::Text`.
    Text(String),
    /// Spoken-transcript lane — flushes as
    /// `AssistantBlock::Transcript { source: TranscriptSource::Spoken, .. }`.
    Transcript(String),
}

#[derive(Debug, Default)]
struct PendingTurn {
    /// Lane-tagged content fragments in arrival order. Flush groups
    /// consecutive same-lane fragments into a single `AssistantBlock`.
    blocks: Vec<PendingAssistantContent>,
}

/// Bridges [`LiveProjectionSink`] callbacks into [`SessionRuntime`].
///
/// R6: the per-turn buffer is keyed by `(SessionId, Option<String>)` where
/// the second element is the provider's `response_id`. Pre-fix the buffer
/// keyed only on `SessionId`, so an interrupted, stale, or overlapping
/// `response.done` carrying the next turn's `stop_reason`/`usage` would
/// flush the wrong buffered transcript. Keying on the full pair lets the
/// sink isolate per-response buffers; orphan completions (no
/// `response_id` from the provider) still bucket under the same `None`
/// slot, which preserves the legacy behaviour for adapters that do not
/// surface response identity.
pub struct SessionServiceProjectionSink {
    runtime: Arc<SessionRuntime>,
    /// Per-(session, response_id) buffer of assistant finals awaiting an
    /// authoritative `TurnCompleted`. The lock is held only for short,
    /// sync sections — never across `.await`.
    pending_turns: StdMutex<HashMap<(SessionId, Option<String>), PendingTurn>>,
}

impl SessionServiceProjectionSink {
    pub fn new(runtime: Arc<SessionRuntime>) -> Self {
        Self {
            runtime,
            pending_turns: StdMutex::new(HashMap::new()),
        }
    }

    /// Push an assistant final onto the per-(session, response_id) buffer
    /// (P1#1 + R6 + T6).
    fn buffer_assistant_content(
        &self,
        session_id: &SessionId,
        response_id: Option<&str>,
        content: PendingAssistantContent,
    ) {
        let Ok(mut pending) = self.pending_turns.lock() else {
            // Lock poisoning here would mean a previous panic in this struct;
            // the only operations that hold the guard are short sync writes/
            // reads. Falling through silently preserves the prior buffer
            // rather than breaking the in-flight turn.
            return;
        };
        pending
            .entry((session_id.clone(), response_id.map(|s| s.to_string())))
            .or_default()
            .blocks
            .push(content);
    }

    /// Drain the per-(session, response_id) buffer at turn-completed time.
    ///
    /// R6: callers must pass the same `response_id` they buffered under so
    /// the matching slot drains; mismatched ids leave the buffer alone, which
    /// is the behaviour the external review demanded — a stale/overlapping
    /// `response.done` cannot flush a different turn's transcript.
    fn drain_pending_turn(&self, session_id: &SessionId, response_id: Option<&str>) -> PendingTurn {
        let Ok(mut pending) = self.pending_turns.lock() else {
            return PendingTurn::default();
        };
        pending
            .remove(&(session_id.clone(), response_id.map(|s| s.to_string())))
            .unwrap_or_default()
    }

    /// Drop only the **transcript** lane fragments from every buffered slot
    /// for `session_id`, preserving display-text fragments in arrival order.
    ///
    /// T7: barge-in invalidates spoken-transcript and audio state but leaves
    /// already-buffered display-text intact (the user did not speak over
    /// written output). The text fragments survive until
    /// `signal_turn_completed` flushes them as `AssistantBlock::Text`.
    fn drop_transcript_lane_for_session(&self, session_id: &SessionId) {
        let Ok(mut pending) = self.pending_turns.lock() else {
            return;
        };
        pending.retain(|(sid, _resp), turn| {
            if sid != session_id {
                return true;
            }
            turn.blocks
                .retain(|c| matches!(c, PendingAssistantContent::Text(_)));
            // Keep the slot even if empty — a subsequent text final / turn-
            // completion may still flush against this (session, response_id)
            // key, and dropping the slot would lose the keying contract.
            true
        });
    }

    /// Drain every buffered slot for `session_id` regardless of `response_id`.
    ///
    /// Used on terminal error: when the channel is torn down every buffered
    /// final becomes non-canonical, so the per-response keying does not
    /// need to be honoured for cleanup.
    fn drain_all_pending_turns(&self, session_id: &SessionId) {
        let Ok(mut pending) = self.pending_turns.lock() else {
            return;
        };
        pending.retain(|(sid, _resp), _| sid != session_id);
    }
}

/// Build the realtime-transcript event the production sink stages on the
/// **display-text** lane. Extracted so unit tests can assert the exact
/// event shape without spinning a full `SessionRuntime`.
fn build_assistant_text_delta_event(
    delta: &str,
    identity: LiveTranscriptIdentity<'_>,
) -> RealtimeTranscriptEvent {
    RealtimeTranscriptEvent::AssistantTextDelta {
        response_id: identity.response_id.unwrap_or_default().to_string(),
        delta_id: identity.delta_id.unwrap_or_default().to_string(),
        item_id: identity.provider_item_id.unwrap_or_default().to_string(),
        previous_item_id: identity.previous_item_id.map(|s| s.to_string()),
        content_index: identity.content_index.unwrap_or(0),
        delta: delta.to_string(),
    }
}

/// Build the realtime-transcript event the production sink stages on the
/// **spoken-transcript** lane. Extracted so unit tests can pin the
/// `AssistantTranscriptDelta` variant (T9/T10) without a full
/// `SessionRuntime`.
fn build_assistant_transcript_delta_event(
    delta: &str,
    identity: LiveTranscriptIdentity<'_>,
) -> RealtimeTranscriptEvent {
    RealtimeTranscriptEvent::AssistantTranscriptDelta {
        response_id: identity.response_id.unwrap_or_default().to_string(),
        delta_id: identity.delta_id.unwrap_or_default().to_string(),
        item_id: identity.provider_item_id.unwrap_or_default().to_string(),
        previous_item_id: identity.previous_item_id.map(|s| s.to_string()),
        content_index: identity.content_index.unwrap_or(0),
        delta: delta.to_string(),
    }
}

/// Collapse arrival-order lane-tagged fragments into a minimal
/// `AssistantBlock` sequence: consecutive `Text` fragments coalesce into one
/// `AssistantBlock::Text`; consecutive `Transcript` fragments coalesce into
/// one `AssistantBlock::Transcript { source: Spoken, .. }`. Inter-lane
/// boundaries produce distinct blocks, preserving the order they were
/// buffered.
fn collapse_pending_blocks(buffered: Vec<PendingAssistantContent>) -> Vec<AssistantBlock> {
    let mut out: Vec<AssistantBlock> = Vec::new();
    let mut current: Option<PendingAssistantContent> = None;
    for fragment in buffered {
        match (current.as_mut(), &fragment) {
            (Some(PendingAssistantContent::Text(buf)), PendingAssistantContent::Text(next)) => {
                buf.push_str(next)
            }
            (
                Some(PendingAssistantContent::Transcript(buf)),
                PendingAssistantContent::Transcript(next),
            ) => buf.push_str(next),
            _ => {
                if let Some(prev) = current.take() {
                    out.push(pending_to_block(prev));
                }
                current = Some(fragment);
            }
        }
    }
    if let Some(prev) = current.take() {
        out.push(pending_to_block(prev));
    }
    out
}

fn pending_to_block(fragment: PendingAssistantContent) -> AssistantBlock {
    match fragment {
        PendingAssistantContent::Text(text) => AssistantBlock::Text { text, meta: None },
        PendingAssistantContent::Transcript(text) => AssistantBlock::Transcript {
            text,
            source: TranscriptSource::Spoken,
            meta: None,
        },
    }
}

fn session_error_to_projection(
    err: meerkat_core::SessionError,
    id: &SessionId,
) -> LiveProjectionError {
    match err {
        meerkat_core::SessionError::NotFound { .. } => {
            LiveProjectionError::SessionNotFound(id.clone())
        }
        meerkat_core::SessionError::Unsupported(reason) => LiveProjectionError::Rejected(reason),
        other => LiveProjectionError::Internal(other.to_string()),
    }
}

#[async_trait]
impl LiveProjectionSink for SessionServiceProjectionSink {
    async fn append_user_transcript(
        &self,
        session_id: &SessionId,
        text: &str,
        identity: LiveTranscriptIdentity<'_>,
    ) -> Result<(), LiveProjectionError> {
        // P2#1: when the provider surfaces a stable `provider_item_id`, route
        // through the typed realtime transcript seam so its idempotent
        // ordering / dedup logic owns the canonical commit. Duplicate
        // provider finals (replays on reconnect, late re-emissions) collapse
        // by item_id + content_index instead of producing duplicate user
        // turns.
        if let Some(item_id) = identity.provider_item_id {
            let event = RealtimeTranscriptEvent::UserTranscriptFinal {
                item_id: item_id.to_string(),
                previous_item_id: identity.previous_item_id.map(|s| s.to_string()),
                content_index: identity.content_index.unwrap_or(0),
                text: text.to_string(),
            };
            return self
                .runtime
                .append_realtime_transcript_event(session_id, event)
                .await
                .map(|_outcome| ())
                .map_err(|err| session_error_to_projection(err, session_id));
        }

        // Legacy fallback: providers that emit `InputTranscriptFinal` without
        // a stable item id cannot be deduplicated by the realtime layer, so
        // commit directly into canonical history. This path is shrinking as
        // adapters start emitting `InputTranscriptFinalForItem`.
        self.runtime
            .append_external_user_content(session_id, ContentInput::Text(text.to_string()))
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_assistant_text_delta(
        &self,
        session_id: &SessionId,
        delta: &str,
        identity: LiveTranscriptIdentity<'_>,
    ) -> Result<(), LiveProjectionError> {
        // A3 (deltas): route through the typed realtime-transcript seam so the
        // session's idempotent ordering / staging logic owns delta application.
        // A11: response_id / delta_id / previous_item_id / content_index are
        // plumbed end-to-end via `LiveTranscriptIdentity`.
        // T6: this lane is **display text** (`AssistantBlock::Text`); the
        // spoken-transcript lane is `append_assistant_transcript_delta`.
        let event = build_assistant_text_delta_event(delta, identity);
        self.runtime
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_assistant_transcript_delta(
        &self,
        session_id: &SessionId,
        delta: &str,
        identity: LiveTranscriptIdentity<'_>,
    ) -> Result<(), LiveProjectionError> {
        // T6/T9/T10: spoken-transcript delta lane. Routes through the
        // dedicated `AssistantTranscriptDelta` realtime event so the
        // staging site at `session.rs::append_realtime_transcript_event`
        // tags the owning item with `TranscriptLane::Spoken`. The
        // materializer at `session.rs::materialize_realtime_transcript_ready_items`
        // then flushes `AssistantBlock::Transcript { source: Spoken, .. }`
        // instead of `AssistantBlock::Text` — fixing the Phase-2 follow-up
        // where transcript deltas were collapsing onto the display-text
        // staging path.
        let event = build_assistant_transcript_delta_event(delta, identity);
        self.runtime
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn append_assistant_text_final(
        &self,
        session_id: &SessionId,
        text: &str,
        _identity: LiveTranscriptIdentity<'_>,
        _stop_reason: StopReason,
        _usage: Usage,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // P1#1 + T6: buffer display-text final on the per-(session,
        // response_id) slot. `signal_turn_completed` drains and flushes as
        // `AssistantBlock::Text`. The display-text lane is preserved across
        // barge-in (T7) — only the transcript lane drains.
        self.buffer_assistant_content(
            session_id,
            response_id,
            PendingAssistantContent::Text(text.to_string()),
        );
        Ok(())
    }

    async fn append_assistant_transcript_final(
        &self,
        session_id: &SessionId,
        text: &str,
        _identity: LiveTranscriptIdentity<'_>,
        _stop_reason: StopReason,
        _usage: Usage,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // P1#1 + T6: buffer spoken-transcript final on the per-(session,
        // response_id) slot. `signal_turn_completed` drains and flushes as
        // `AssistantBlock::Transcript { source: Spoken, .. }`. T7: barge-in
        // (`signal_turn_interrupt`) drops these but keeps any buffered
        // display-text fragments in arrival order.
        self.buffer_assistant_content(
            session_id,
            response_id,
            PendingAssistantContent::Transcript(text.to_string()),
        );
        Ok(())
    }

    async fn truncate_assistant_transcript(
        &self,
        session_id: &SessionId,
        provider_item_id: Option<&str>,
        _previous_item_id: Option<&str>,
        content_index: Option<u32>,
        response_id: Option<&str>,
        text: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // P1#3: barge-in projection now carries authoritative response_id and
        // content_index from the source observation. Don't fabricate empty
        // values — the realtime transcript layer rejects empty response_id,
        // which made the previous projection inert. Surface a typed Rejected
        // when the provider genuinely did not supply the id (provider-fact
        // gap, not a silently-dropped event).
        let Some(response_id) = response_id else {
            return Err(LiveProjectionError::Rejected(
                "AssistantTranscriptTruncated missing response_id from adapter".to_string(),
            ));
        };
        let Some(item_id) = provider_item_id else {
            return Err(LiveProjectionError::Rejected(
                "AssistantTranscriptTruncated missing provider_item_id from adapter".to_string(),
            ));
        };
        let event = RealtimeTranscriptEvent::AssistantTranscriptTruncated {
            response_id: response_id.to_string(),
            item_id: item_id.to_string(),
            content_index: content_index.unwrap_or(0),
            text: text.unwrap_or_default().to_string(),
        };
        self.runtime
            .append_realtime_transcript_event(session_id, event)
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn signal_turn_interrupt(
        &self,
        session_id: &SessionId,
    ) -> Result<(), LiveProjectionError> {
        // A6: barge-in projects through the same machine-authority interrupt
        // path the user-facing `live/interrupt` RPC uses. Tolerate
        // `NotRunning` (typical when no turn is currently in flight) so that a
        // late provider-side interrupt does not poison the channel.
        // T7: drop ONLY the spoken-transcript lane (and any audio playback
        // state, future). Display-text finals are preserved — the user is
        // not "speaking over" written output, so a barge-in does not
        // invalidate buffered display-text. The next `signal_turn_completed`
        // flushes the surviving text fragments as `AssistantBlock::Text`.
        self.drop_transcript_lane_for_session(session_id);
        match self
            .runtime
            .interrupt_live_with_machine_authority(session_id)
            .await
        {
            Ok(()) => Ok(()),
            Err(meerkat_core::SessionError::NotRunning { .. }) => Ok(()),
            Err(err) => Err(session_error_to_projection(err, session_id)),
        }
    }

    async fn signal_turn_completed(
        &self,
        session_id: &SessionId,
        stop_reason: StopReason,
        usage: Usage,
        response_id: Option<&str>,
    ) -> Result<(), LiveProjectionError> {
        // P1#1: flush the per-turn assistant final buffer with the
        // authoritative `stop_reason` / `usage` from `TurnCompleted`. This is
        // the canonical commit seam — `append_assistant_*_final` are pure
        // buffer operations. Orphan completions (no buffered final) still
        // commit an empty assistant message, which is correct because no
        // assistant output was produced for the turn.
        //
        // R6: drain by `(session_id, response_id)` so a stale or overlapping
        // `response.done` cannot flush the wrong buffered transcript.
        // T6/T10: collapse arrival-order lane fragments — consecutive Text
        // runs become one `AssistantBlock::Text`; consecutive Transcript
        // runs become one `AssistantBlock::Transcript { source: Spoken }`.
        // Inter-lane boundaries produce distinct ordered blocks.
        let pending = self.drain_pending_turn(session_id, response_id);
        let blocks = collapse_pending_blocks(pending.blocks);
        self.runtime
            .append_external_assistant_output(session_id, blocks, stop_reason, usage)
            .await
            .map_err(|err| session_error_to_projection(err, session_id))
    }

    async fn signal_terminal_error(
        &self,
        session_id: &SessionId,
        code: LiveAdapterErrorCode,
        message: &str,
    ) -> Result<(), LiveProjectionError> {
        // Surface a tracing event so operators can correlate. The host has
        // already terminalized the channel state (status=Closed + reap timer);
        // session-level lifecycle remains in caller hands. A future enhancement
        // could route this to the runtime's session-event stream once a
        // canonical terminal-error seam exists on `SessionService`.
        // R6: drop ALL buffered finals across every response_id slot —
        // terminal error invalidates every in-flight response.
        self.drain_all_pending_turns(session_id);
        tracing::warn!(
            target: "meerkat_rpc::live_projection",
            session_id = %session_id,
            ?code,
            message,
            "live adapter terminal error",
        );
        Ok(())
    }

    async fn append_realtime_transcript(
        &self,
        session_id: &SessionId,
        event: &RealtimeTranscriptEvent,
    ) -> Result<(), LiveProjectionError> {
        // P1#2: forward provider-emitted realtime transcript events into the
        // same idempotent ordering / staging path used by deltas + truncation.
        // Without this override the host falls back to the default `Ok(())`
        // body and events (`ItemObserved`, `AssistantTurnCompleted`, etc.) are
        // silently dropped — the very gap the external review flagged.
        self.runtime
            .append_realtime_transcript_event(session_id, event.clone())
            .await
            .map(|_outcome| ())
            .map_err(|err| session_error_to_projection(err, session_id))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use meerkat_core::live_adapter::{
        LiveAdapter, LiveAdapterCommand, LiveAdapterError, LiveAdapterObservation,
        LiveAdapterStatus,
    };
    use meerkat_core::ops::ToolDispatchOutcome;
    use meerkat_core::types::{ContentBlock, ToolCallView, ToolResult};
    use meerkat_core::{AgentToolDispatcher, ToolDef};
    use meerkat_live::LiveAdapterHost;
    use std::sync::Mutex as StdMutex;

    // ------------------------------------------------------------------
    // Fakes that record sink/dispatcher activity without spinning a real
    // SessionRuntime. The tests below call host.apply_observation(...)
    // directly and assert the projection sink/dispatcher wiring.
    // ------------------------------------------------------------------

    /// Owned mirror of [`LiveTranscriptIdentity`] for capturing past the
    /// trait callsite (the borrowed form does not survive the await).
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    struct OwnedIdentity {
        provider_item_id: Option<String>,
        previous_item_id: Option<String>,
        content_index: Option<u32>,
        response_id: Option<String>,
        delta_id: Option<String>,
    }

    impl OwnedIdentity {
        fn from_borrowed(identity: LiveTranscriptIdentity<'_>) -> Self {
            Self {
                provider_item_id: identity.provider_item_id.map(|s| s.to_string()),
                previous_item_id: identity.previous_item_id.map(|s| s.to_string()),
                content_index: identity.content_index,
                response_id: identity.response_id.map(|s| s.to_string()),
                delta_id: identity.delta_id.map(|s| s.to_string()),
            }
        }
    }

    /// Captured truncation projection for assertion: full identity tuple.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedTruncation {
        session_id: SessionId,
        provider_item_id: Option<String>,
        previous_item_id: Option<String>,
        content_index: Option<u32>,
        response_id: Option<String>,
        text: Option<String>,
    }

    /// Captured realtime transcript event routed through the surface sink.
    /// Drives the P1#1 / P2#1 / P1#3 regression tests below.
    #[derive(Debug, Clone, PartialEq)]
    enum CapturedRuntimeCall {
        RealtimeTranscript(SessionId, RealtimeTranscriptEvent),
        ExternalAssistantOutput {
            session_id: SessionId,
            blocks: Vec<AssistantBlock>,
            stop_reason: StopReason,
            usage: Usage,
        },
        ExternalUserContent {
            session_id: SessionId,
            content: ContentInput,
        },
    }

    #[derive(Default)]
    struct RecordingSink {
        user_transcripts: StdMutex<Vec<(SessionId, String, OwnedIdentity)>>,
        text_deltas: StdMutex<Vec<(SessionId, String, OwnedIdentity)>>,
        transcript_deltas: StdMutex<Vec<(SessionId, String, OwnedIdentity)>>,
        text_finals: StdMutex<Vec<(SessionId, String, OwnedIdentity)>>,
        transcript_finals: StdMutex<Vec<(SessionId, String, OwnedIdentity)>>,
        truncations: StdMutex<Vec<CapturedTruncation>>,
        interrupts: StdMutex<Vec<SessionId>>,
        completed: StdMutex<Vec<SessionId>>,
        terminals: StdMutex<Vec<(SessionId, LiveAdapterErrorCode)>>,
    }

    #[async_trait]
    impl LiveProjectionSink for RecordingSink {
        async fn append_user_transcript(
            &self,
            session_id: &SessionId,
            text: &str,
            identity: LiveTranscriptIdentity<'_>,
        ) -> Result<(), LiveProjectionError> {
            self.user_transcripts.lock().unwrap().push((
                session_id.clone(),
                text.to_string(),
                OwnedIdentity::from_borrowed(identity),
            ));
            Ok(())
        }
        async fn append_assistant_text_delta(
            &self,
            session_id: &SessionId,
            delta: &str,
            identity: LiveTranscriptIdentity<'_>,
        ) -> Result<(), LiveProjectionError> {
            self.text_deltas.lock().unwrap().push((
                session_id.clone(),
                delta.to_string(),
                OwnedIdentity::from_borrowed(identity),
            ));
            Ok(())
        }
        async fn append_assistant_transcript_delta(
            &self,
            session_id: &SessionId,
            delta: &str,
            identity: LiveTranscriptIdentity<'_>,
        ) -> Result<(), LiveProjectionError> {
            self.transcript_deltas.lock().unwrap().push((
                session_id.clone(),
                delta.to_string(),
                OwnedIdentity::from_borrowed(identity),
            ));
            Ok(())
        }
        async fn append_assistant_text_final(
            &self,
            session_id: &SessionId,
            text: &str,
            identity: LiveTranscriptIdentity<'_>,
            _stop_reason: StopReason,
            _usage: Usage,
            _response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.text_finals.lock().unwrap().push((
                session_id.clone(),
                text.to_string(),
                OwnedIdentity::from_borrowed(identity),
            ));
            Ok(())
        }
        async fn append_assistant_transcript_final(
            &self,
            session_id: &SessionId,
            text: &str,
            identity: LiveTranscriptIdentity<'_>,
            _stop_reason: StopReason,
            _usage: Usage,
            _response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.transcript_finals.lock().unwrap().push((
                session_id.clone(),
                text.to_string(),
                OwnedIdentity::from_borrowed(identity),
            ));
            Ok(())
        }
        async fn truncate_assistant_transcript(
            &self,
            session_id: &SessionId,
            provider_item_id: Option<&str>,
            previous_item_id: Option<&str>,
            content_index: Option<u32>,
            response_id: Option<&str>,
            text: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.truncations.lock().unwrap().push(CapturedTruncation {
                session_id: session_id.clone(),
                provider_item_id: provider_item_id.map(|s| s.to_string()),
                previous_item_id: previous_item_id.map(|s| s.to_string()),
                content_index,
                response_id: response_id.map(|s| s.to_string()),
                text: text.map(|s| s.to_string()),
            });
            Ok(())
        }
        async fn signal_turn_interrupt(
            &self,
            session_id: &SessionId,
        ) -> Result<(), LiveProjectionError> {
            self.interrupts.lock().unwrap().push(session_id.clone());
            Ok(())
        }
        async fn signal_turn_completed(
            &self,
            session_id: &SessionId,
            _stop_reason: StopReason,
            _usage: Usage,
            _response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.completed.lock().unwrap().push(session_id.clone());
            Ok(())
        }
        async fn signal_terminal_error(
            &self,
            session_id: &SessionId,
            code: LiveAdapterErrorCode,
            _message: &str,
        ) -> Result<(), LiveProjectionError> {
            self.terminals
                .lock()
                .unwrap()
                .push((session_id.clone(), code));
            Ok(())
        }
    }

    /// Adapter that records SubmitToolResult/SubmitToolError commands and
    /// otherwise is a no-op.
    #[derive(Default)]
    struct RecordingAdapter {
        commands: StdMutex<Vec<LiveAdapterCommand>>,
    }

    #[async_trait]
    impl LiveAdapter for RecordingAdapter {
        fn status(&self) -> LiveAdapterStatus {
            LiveAdapterStatus::Ready
        }
        async fn send_command(&self, command: LiveAdapterCommand) -> Result<(), LiveAdapterError> {
            self.commands.lock().unwrap().push(command);
            Ok(())
        }
        async fn next_observation(
            &self,
        ) -> Result<Option<LiveAdapterObservation>, LiveAdapterError> {
            Ok(None)
        }
        async fn close(&self) -> Result<(), LiveAdapterError> {
            Ok(())
        }
    }

    /// Tool dispatcher that records calls and returns a synthetic text result.
    struct RecordingDispatcher {
        calls: StdMutex<Vec<(String, String)>>,
    }

    impl Default for RecordingDispatcher {
        fn default() -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AgentToolDispatcher for RecordingDispatcher {
        fn tools(&self) -> Arc<[Arc<ToolDef>]> {
            Arc::from([])
        }
        async fn dispatch(
            &self,
            view: ToolCallView<'_>,
        ) -> Result<ToolDispatchOutcome, meerkat_core::ToolError> {
            self.calls
                .lock()
                .unwrap()
                .push((view.id.to_string(), view.name.to_string()));
            Ok(ToolDispatchOutcome::sync_result(ToolResult {
                tool_use_id: view.id.to_string(),
                is_error: false,
                content: ContentBlock::text_vec("ok".to_string()),
            }))
        }
    }

    fn test_session_id() -> SessionId {
        SessionId::parse("00000000-0000-0000-0000-000000000001").unwrap()
    }

    #[tokio::test]
    async fn user_transcript_final_records_user_message() {
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let host = LiveAdapterHost::new()
            .with_projection_sink(Arc::clone(&sink) as Arc<dyn LiveProjectionSink>);
        let session_id = test_session_id();
        let channel = host.open_channel(session_id.clone()).await.unwrap();

        let obs = LiveAdapterObservation::UserTranscriptFinal {
            provider_item_id: Some("item_1".to_string()),
            previous_item_id: Some("item_0".to_string()),
            content_index: Some(2),
            text: "hello".to_string(),
        };

        host.apply_observation(&channel, &obs).await.unwrap();

        let recorded = sink.user_transcripts.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, session_id);
        assert_eq!(recorded[0].1, "hello");
        // A11: the user-side identity tuple flows end-to-end through
        // `LiveTranscriptIdentity` and is no longer dropped at the trait layer.
        let identity = &recorded[0].2;
        assert_eq!(identity.provider_item_id.as_deref(), Some("item_1"));
        assert_eq!(identity.previous_item_id.as_deref(), Some("item_0"));
        assert_eq!(identity.content_index, Some(2));
    }

    #[tokio::test]
    async fn assistant_delta_records_delta_append_with_full_identity() {
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let host = LiveAdapterHost::new()
            .with_projection_sink(Arc::clone(&sink) as Arc<dyn LiveProjectionSink>);
        let session_id = test_session_id();
        let channel = host.open_channel(session_id.clone()).await.unwrap();

        let obs = LiveAdapterObservation::AssistantTextDelta {
            provider_item_id: Some("item_2".to_string()),
            previous_item_id: Some("item_1".to_string()),
            content_index: Some(0),
            response_id: Some("resp_7".to_string()),
            delta_id: Some("delta_3".to_string()),
            delta: "partial".to_string(),
        };

        host.apply_observation(&channel, &obs).await.unwrap();

        let recorded = sink.text_deltas.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].1, "partial");
        // A11: the assistant-delta identity tuple — including response_id and
        // delta_id — must flow end-to-end through `LiveTranscriptIdentity`
        // rather than being synthesized as empty strings.
        let identity = &recorded[0].2;
        assert_eq!(identity.provider_item_id.as_deref(), Some("item_2"));
        assert_eq!(identity.previous_item_id.as_deref(), Some("item_1"));
        assert_eq!(identity.content_index, Some(0));
        assert_eq!(identity.response_id.as_deref(), Some("resp_7"));
        assert_eq!(identity.delta_id.as_deref(), Some("delta_3"));
    }

    #[tokio::test]
    async fn tool_call_dispatches_through_dispatcher() {
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let dispatcher: Arc<RecordingDispatcher> = Arc::new(RecordingDispatcher::default());
        let adapter: Arc<RecordingAdapter> = Arc::new(RecordingAdapter::default());
        let host = LiveAdapterHost::new()
            .with_projection_sink(Arc::clone(&sink) as Arc<dyn LiveProjectionSink>)
            .with_tool_dispatcher(Arc::clone(&dispatcher) as Arc<dyn AgentToolDispatcher>);
        let session_id = test_session_id();
        let channel = host.open_channel(session_id.clone()).await.unwrap();
        host.attach_adapter(&channel, Arc::clone(&adapter) as Arc<dyn LiveAdapter>)
            .await
            .unwrap();

        let obs = LiveAdapterObservation::ToolCallRequested {
            provider_call_id: "call_42".to_string(),
            tool_name: "echo".to_string(),
            arguments: serde_json::json!({"value": 1}),
        };

        host.apply_observation(&channel, &obs).await.unwrap();

        let calls = dispatcher.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "call_42");
        assert_eq!(calls[0].1, "echo");

        // A5: result must be submitted back to the adapter as
        // `SubmitToolResult` so the provider gets the answer.
        let cmds = adapter.commands.lock().unwrap();
        assert!(matches!(
            cmds.first(),
            Some(LiveAdapterCommand::SubmitToolResult { .. })
        ));
    }

    #[tokio::test]
    async fn turn_interrupted_signals_interrupt() {
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let host = LiveAdapterHost::new()
            .with_projection_sink(Arc::clone(&sink) as Arc<dyn LiveProjectionSink>);
        let session_id = test_session_id();
        let channel = host.open_channel(session_id.clone()).await.unwrap();

        host.apply_observation(&channel, &LiveAdapterObservation::TurnInterrupted)
            .await
            .unwrap();

        let recorded = sink.interrupts.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], session_id);
    }

    #[tokio::test]
    async fn assistant_transcript_truncated_carries_real_response_id_to_sink() {
        // P1#3 regression: scripted `AssistantTranscriptTruncated` with
        // non-empty response_id must reach the sink with the real id, not
        // be silently dropped via fabricated empty values.
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let host = LiveAdapterHost::new()
            .with_projection_sink(Arc::clone(&sink) as Arc<dyn LiveProjectionSink>);
        let session_id = test_session_id();
        let channel = host.open_channel(session_id.clone()).await.unwrap();

        let obs = LiveAdapterObservation::AssistantTranscriptTruncated {
            provider_item_id: Some("item_99".to_string()),
            previous_item_id: Some("item_98".to_string()),
            content_index: Some(0),
            response_id: Some("resp_42".to_string()),
            text: Some("partial".to_string()),
        };

        host.apply_observation(&channel, &obs).await.unwrap();

        let recorded = sink.truncations.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        let captured = &recorded[0];
        assert_eq!(captured.session_id, session_id);
        // The host must NOT fabricate empty defaults — the real provider
        // identity must reach the sink so the realtime transcript layer can
        // fold the truncation into the staged response. The original buggy
        // sink hard-coded `response_id: ""` and `content_index: 0`, which the
        // session layer rejects.
        assert_eq!(captured.response_id.as_deref(), Some("resp_42"));
        assert_eq!(captured.provider_item_id.as_deref(), Some("item_99"));
        assert_eq!(captured.content_index, Some(0));
        assert_eq!(captured.previous_item_id.as_deref(), Some("item_98"));
        assert_eq!(captured.text.as_deref(), Some("partial"));
    }

    // ----------------------------------------------------------------------
    // Buffer-flow regressions for SessionServiceProjectionSink. These tests
    // exercise the sink directly via a `RecordingRuntime` stand-in; the sink
    // is the seam the SessionRuntime traffic flows through, so we capture
    // calls without booting an entire runtime.
    // ----------------------------------------------------------------------

    /// Test sink that mimics `SessionServiceProjectionSink`'s buffering
    /// strategy and records every "would-be" runtime call. Asserting on
    /// `calls` lets us pin the seam contract (P1#1 / P2#1 / P1#3 / R6)
    /// without the SessionRuntime/SessionService overhead.
    ///
    /// R6: keys the pending buffer on `(SessionId, Option<String>)` to
    /// match the production sink's per-response_id bucketing.
    #[allow(clippy::type_complexity)]
    struct BufferingTestSink {
        calls: StdMutex<Vec<CapturedRuntimeCall>>,
        // T6: lane-tagged buffer mirroring `SessionServiceProjectionSink`.
        // Tests assert that the production sink groups consecutive same-
        // lane fragments into one `AssistantBlock` while preserving inter-
        // lane ordering.
        pending: StdMutex<HashMap<(SessionId, Option<String>), Vec<PendingAssistantContent>>>,
    }

    impl BufferingTestSink {
        fn new() -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
                pending: StdMutex::new(HashMap::new()),
            }
        }

        fn append_user_transcript(
            &self,
            session_id: &SessionId,
            text: &str,
            item_id: Option<&str>,
        ) {
            if let Some(item_id) = item_id {
                let event = RealtimeTranscriptEvent::UserTranscriptFinal {
                    item_id: item_id.to_string(),
                    previous_item_id: None,
                    content_index: 0,
                    text: text.to_string(),
                };
                self.calls
                    .lock()
                    .unwrap()
                    .push(CapturedRuntimeCall::RealtimeTranscript(
                        session_id.clone(),
                        event,
                    ));
            } else {
                self.calls
                    .lock()
                    .unwrap()
                    .push(CapturedRuntimeCall::ExternalUserContent {
                        session_id: session_id.clone(),
                        content: ContentInput::Text(text.to_string()),
                    });
            }
        }

        fn append_assistant_text_final(
            &self,
            session_id: &SessionId,
            text: &str,
            response_id: Option<&str>,
        ) {
            self.pending
                .lock()
                .unwrap()
                .entry((session_id.clone(), response_id.map(|s| s.to_string())))
                .or_default()
                .push(PendingAssistantContent::Text(text.to_string()));
        }

        fn append_assistant_transcript_final(
            &self,
            session_id: &SessionId,
            text: &str,
            response_id: Option<&str>,
        ) {
            self.pending
                .lock()
                .unwrap()
                .entry((session_id.clone(), response_id.map(|s| s.to_string())))
                .or_default()
                .push(PendingAssistantContent::Transcript(text.to_string()));
        }

        /// T7 — drop only spoken-transcript fragments; preserve display
        /// text in arrival order.
        fn signal_turn_interrupt(&self, session_id: &SessionId) {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|(sid, _resp), turn| {
                if sid != session_id {
                    return true;
                }
                turn.retain(|c| matches!(c, PendingAssistantContent::Text(_)));
                true
            });
        }

        fn signal_turn_completed(
            &self,
            session_id: &SessionId,
            stop_reason: StopReason,
            usage: Usage,
            response_id: Option<&str>,
        ) {
            let drained = self
                .pending
                .lock()
                .unwrap()
                .remove(&(session_id.clone(), response_id.map(|s| s.to_string())))
                .unwrap_or_default();
            let blocks = collapse_pending_blocks(drained);
            self.calls
                .lock()
                .unwrap()
                .push(CapturedRuntimeCall::ExternalAssistantOutput {
                    session_id: session_id.clone(),
                    blocks,
                    stop_reason,
                    usage,
                });
        }

        fn truncate_assistant_transcript(
            &self,
            session_id: &SessionId,
            response_id: Option<&str>,
            item_id: Option<&str>,
            content_index: Option<u32>,
            text: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            let response_id = response_id
                .ok_or_else(|| LiveProjectionError::Rejected("missing response_id".into()))?;
            let item_id = item_id
                .ok_or_else(|| LiveProjectionError::Rejected("missing provider_item_id".into()))?;
            let event = RealtimeTranscriptEvent::AssistantTranscriptTruncated {
                response_id: response_id.to_string(),
                item_id: item_id.to_string(),
                content_index: content_index.unwrap_or(0),
                text: text.unwrap_or_default().to_string(),
            };
            self.calls
                .lock()
                .unwrap()
                .push(CapturedRuntimeCall::RealtimeTranscript(
                    session_id.clone(),
                    event,
                ));
            Ok(())
        }
    }

    fn sentinel_usage() -> Usage {
        Usage::default()
    }

    fn real_usage() -> Usage {
        Usage {
            input_tokens: 17,
            output_tokens: 23,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }
    }

    #[test]
    fn p1_1_assistant_final_defers_to_turn_completed_with_real_stop_reason_and_usage() {
        // P1#1 regression: scripted `AssistantTranscriptFinal` (sentinel) +
        // `TurnCompleted` (real). Canonical history must record the REAL
        // stop_reason / usage, not the provider's sentinel default.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        // Provider stamps `AssistantTranscriptFinal` with sentinel values
        // because stop_reason/usage only arrive atomically with `response.done`.
        sink.append_assistant_text_final(&session_id, "the final text", Some("resp_1"));

        // Under the broken behavior, this is where canonical history was
        // committed — with the sentinel. With the fix it's pure buffering, so
        // no runtime call happens yet.
        assert!(
            sink.calls.lock().unwrap().is_empty(),
            "append_assistant_text_final must not commit canonical history; \
             defer until signal_turn_completed delivers authoritative \
             stop_reason/usage (P1#1)"
        );

        // Now `TurnCompleted` arrives carrying the authoritative stop_reason
        // and usage from `response.done`.
        sink.signal_turn_completed(
            &session_id,
            StopReason::EndTurn,
            real_usage(),
            Some("resp_1"),
        );

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput {
                session_id: sid,
                blocks,
                stop_reason,
                usage,
            } => {
                assert_eq!(sid, &session_id);
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AssistantBlock::Text { text, .. } => {
                        assert_eq!(text, "the final text");
                    }
                    other => panic!("expected text block, got {other:?}"),
                }
                // The authoritative values flow through, not the sentinel.
                assert_eq!(*stop_reason, StopReason::EndTurn);
                assert_eq!(*usage, real_usage());
                assert_ne!(
                    *usage,
                    sentinel_usage(),
                    "must not commit the sentinel Usage::default() that the provider \
                     stamped onto AssistantTranscriptFinal (P1#1)"
                );
            }
            other => panic!("expected ExternalAssistantOutput, got {other:?}"),
        }
    }

    #[test]
    fn p1_1_multi_part_assistant_final_buffers_into_one_canonical_message() {
        // P1#1 + T6 multi-content-part variant: provider emits multiple
        // `AssistantTranscriptFinal` events for one logical turn (one per
        // content part). The buffer collects all fragments and flushes them
        // as ONE assistant message at turn-completed boundary. T6 changes
        // the block shape: consecutive same-lane fragments now coalesce
        // into a single `AssistantBlock` so readers do not see one block
        // per delta. Both texts are display text → one coalesced
        // `AssistantBlock::Text` containing both parts.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.append_assistant_text_final(&session_id, "part one ", Some("resp_a"));
        sink.append_assistant_text_final(&session_id, "part two", Some("resp_a"));

        assert!(sink.calls.lock().unwrap().is_empty());

        sink.signal_turn_completed(
            &session_id,
            StopReason::EndTurn,
            real_usage(),
            Some("resp_a"),
        );

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                // T6: same-lane consecutive fragments coalesce into ONE
                // block. The committed text is the concatenation in
                // arrival order.
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AssistantBlock::Text { text, .. } => {
                        assert_eq!(text, "part one part two");
                    }
                    other => panic!("expected Text block, got {other:?}"),
                }
            }
            other => panic!("expected one ExternalAssistantOutput, got {other:?}"),
        }
    }

    #[test]
    fn p1_1_orphan_turn_completed_commits_empty_assistant_block() {
        // Orphan completion: `TurnCompleted` with no buffered final.
        // Must still commit (with empty blocks) — correct because no
        // transcript was produced.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.signal_turn_completed(&session_id, StopReason::EndTurn, real_usage(), None);

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                assert!(blocks.is_empty());
            }
            other => panic!("expected ExternalAssistantOutput with empty blocks, got {other:?}"),
        }
    }

    #[test]
    fn r6_assistant_final_buffer_keys_on_response_id() {
        // R6 regression: two `AssistantTranscriptFinal` events from
        // different provider responses must NOT pool into the same buffer.
        // Pre-fix the buffer keyed only on SessionId, so a stale or
        // overlapping `response.done` could flush text that belonged to a
        // different response.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        // Two responses interleaved on the same session.
        sink.append_assistant_text_final(&session_id, "from resp_a", Some("resp_a"));
        sink.append_assistant_text_final(&session_id, "from resp_b", Some("resp_b"));

        // Completion for resp_a flushes only the resp_a slot.
        sink.signal_turn_completed(
            &session_id,
            StopReason::EndTurn,
            real_usage(),
            Some("resp_a"),
        );

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AssistantBlock::Text { text, .. } => assert_eq!(text, "from resp_a"),
                    other => panic!("expected text block, got {other:?}"),
                }
            }
            other => panic!("expected ExternalAssistantOutput, got {other:?}"),
        }
        drop(calls);

        // The resp_b buffer survives — it is NOT flushed by resp_a's completion.
        sink.signal_turn_completed(
            &session_id,
            StopReason::EndTurn,
            real_usage(),
            Some("resp_b"),
        );

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        match &calls[1] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AssistantBlock::Text { text, .. } => assert_eq!(text, "from resp_b"),
                    other => panic!("expected text block, got {other:?}"),
                }
            }
            other => panic!("expected ExternalAssistantOutput, got {other:?}"),
        }
    }

    #[test]
    fn t7_barge_in_drops_only_transcript_lane_keeps_display_text() {
        // T7 regression: barge-in (`signal_turn_interrupt`) must drain only
        // the spoken-transcript fragments from the per-(session, response_id)
        // buffer. Display-text fragments that already arrived survive and
        // commit on the subsequent `signal_turn_completed`.
        //
        // Scenario:
        //   1. Provider emits a display-text final (`AssistantBlock::Text`).
        //   2. Provider emits a transcript final partway (`Transcript`).
        //   3. User barges in -> `signal_turn_interrupt`.
        //   4. `signal_turn_completed` arrives.
        //   -> The committed assistant message contains exactly the
        //      buffered text lane, no transcript block.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.append_assistant_text_final(&session_id, "written response", Some("resp_42"));
        sink.append_assistant_transcript_final(
            &session_id,
            "spoken response (interrupted)",
            Some("resp_42"),
        );

        // Pre-flush invariant: nothing committed yet.
        assert!(sink.calls.lock().unwrap().is_empty());

        // User barge-in.
        sink.signal_turn_interrupt(&session_id);

        // Drainage on completion flushes the surviving text fragment ONLY.
        sink.signal_turn_completed(
            &session_id,
            StopReason::EndTurn,
            real_usage(),
            Some("resp_42"),
        );

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                assert_eq!(
                    blocks.len(),
                    1,
                    "barge-in must drop transcript blocks; only display-text survives \
                     (got {blocks:?})"
                );
                match &blocks[0] {
                    AssistantBlock::Text { text, .. } => {
                        assert_eq!(text, "written response");
                    }
                    AssistantBlock::Transcript { .. } => {
                        panic!(
                            "barge-in left a Transcript block in the committed message; \
                             T7 requires the transcript lane to be drained at \
                             signal_turn_interrupt"
                        );
                    }
                    other => panic!("expected Text block, got {other:?}"),
                }
            }
            other => panic!("expected ExternalAssistantOutput, got {other:?}"),
        }
    }

    #[test]
    fn t6_mixed_lane_finals_collapse_into_ordered_blocks() {
        // T6 regression: arrival-order lane fragments must produce one
        // `AssistantBlock::Text` per consecutive Text run and one
        // `AssistantBlock::Transcript { source: Spoken }` per consecutive
        // Transcript run. Inter-lane boundaries split the runs; same-lane
        // adjacency coalesces text. This locks the design choice (Option
        // A: typed enum buffer in arrival order) for the production sink.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.append_assistant_text_final(&session_id, "intro ", Some("r1"));
        sink.append_assistant_text_final(&session_id, "more text ", Some("r1"));
        sink.append_assistant_transcript_final(&session_id, "spoken aside", Some("r1"));
        sink.append_assistant_text_final(&session_id, " after speech", Some("r1"));

        sink.signal_turn_completed(&session_id, StopReason::EndTurn, real_usage(), Some("r1"));

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                assert_eq!(blocks.len(), 3, "lanes split: text, transcript, text");
                match &blocks[0] {
                    AssistantBlock::Text { text, .. } => {
                        assert_eq!(text, "intro more text ");
                    }
                    other => panic!("expected coalesced Text block first, got {other:?}"),
                }
                match &blocks[1] {
                    AssistantBlock::Transcript { text, source, .. } => {
                        assert_eq!(text, "spoken aside");
                        assert_eq!(*source, TranscriptSource::Spoken);
                    }
                    other => panic!("expected Transcript block second, got {other:?}"),
                }
                match &blocks[2] {
                    AssistantBlock::Text { text, .. } => {
                        assert_eq!(text, " after speech");
                    }
                    other => panic!("expected Text block third, got {other:?}"),
                }
            }
            other => panic!("expected ExternalAssistantOutput, got {other:?}"),
        }
    }

    #[test]
    fn p2_1_user_transcript_with_item_id_routes_through_realtime_transcript_seam() {
        // P2#1 regression: user transcripts with a stable provider item id
        // route through `RealtimeTranscriptEvent::UserTranscriptFinal` so the
        // session's idempotent ordering owns dedup. Duplicate finals for the
        // same item_id must NOT produce two canonical user turns.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        // First final.
        sink.append_user_transcript(&session_id, "hello", Some("item_42"));
        // Duplicate provider final (e.g. reconnect replay) for the same item.
        sink.append_user_transcript(&session_id, "hello", Some("item_42"));

        let calls = sink.calls.lock().unwrap();
        // Both calls flow through the realtime transcript seam where the
        // session layer dedupes by item_id. Pre-fix they would have flowed
        // through `append_external_user_content` which appends unconditionally.
        assert_eq!(calls.len(), 2);
        for call in calls.iter() {
            match call {
                CapturedRuntimeCall::RealtimeTranscript(_, event) => match event {
                    RealtimeTranscriptEvent::UserTranscriptFinal { item_id, .. } => {
                        assert_eq!(item_id, "item_42");
                    }
                    other => panic!("expected UserTranscriptFinal, got {other:?}"),
                },
                CapturedRuntimeCall::ExternalUserContent { .. } => {
                    panic!(
                        "user finals carrying a provider_item_id MUST route through the \
                         realtime transcript seam (P2#1); the legacy external-user-content \
                         path bypasses idempotent ordering"
                    );
                }
                other => panic!("unexpected call shape: {other:?}"),
            }
        }
    }

    #[test]
    fn p2_1_user_transcript_without_item_id_falls_back_to_external_user_content() {
        // Legacy fallback: providers that emit `InputTranscriptFinal` without
        // a stable item id cannot be deduplicated by the realtime layer.
        // They commit directly into canonical history.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.append_user_transcript(&session_id, "no-id", None);

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            calls[0],
            CapturedRuntimeCall::ExternalUserContent { .. }
        ));
    }

    #[test]
    fn p1_3_truncation_routes_real_response_id_through_realtime_transcript_seam() {
        // P1#3 regression: scripted `AssistantTranscriptTruncated` with a
        // non-empty response_id must reach
        // `SessionRuntime::append_realtime_transcript_event` with the real
        // id, not silently dropped via fabricated empties. Pre-fix the sink
        // hard-coded `response_id: String::new()` which the session layer
        // rejects, so truncation projection was inert.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.truncate_assistant_transcript(
            &session_id,
            Some("resp_42"),
            Some("item_7"),
            Some(0),
            Some("partial heard prefix"),
        )
        .unwrap();

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::RealtimeTranscript(sid, event) => {
                assert_eq!(sid, &session_id);
                match event {
                    RealtimeTranscriptEvent::AssistantTranscriptTruncated {
                        response_id,
                        item_id,
                        content_index,
                        text,
                    } => {
                        // Real response_id from the source observation, not
                        // the fabricated empty string that triggered the
                        // realtime layer's normalize-rejection.
                        assert_eq!(response_id, "resp_42");
                        assert_ne!(response_id, "");
                        assert_eq!(item_id, "item_7");
                        assert_eq!(*content_index, 0);
                        assert_eq!(text, "partial heard prefix");
                    }
                    other => panic!("expected AssistantTranscriptTruncated, got {other:?}"),
                }
            }
            other => panic!("expected RealtimeTranscript call, got {other:?}"),
        }
    }

    #[test]
    fn p1_3_truncation_missing_response_id_surfaces_typed_rejection() {
        // P1#3 — when the provider genuinely did not surface a response_id,
        // the sink rejects with a typed error rather than committing an
        // empty value. Surfacing the gap as Rejected lets operators see the
        // provider-fact gap; pre-fix it was silently absorbed.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        let err = sink
            .truncate_assistant_transcript(
                &session_id,
                None, // missing response_id
                Some("item_7"),
                Some(0),
                None,
            )
            .unwrap_err();

        match err {
            LiveProjectionError::Rejected(reason) => {
                assert!(reason.contains("response_id"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // No runtime call was made — the inert fabricated-empty commit is gone.
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    // ------------------------------------------------------------------
    // T9/T10: production sink stages transcript-delta on the dedicated
    // realtime-transcript variant.
    //
    // Pre-T9/T10 the spoken-transcript delta path was reusing
    // `RealtimeTranscriptEvent::AssistantTextDelta`, which forced the
    // session materializer to flush every assistant transcript as
    // `AssistantBlock::Text`. The fix routes it through the new
    // `AssistantTranscriptDelta` variant so the materializer (verified
    // separately in `meerkat-core/src/session.rs` regression tests)
    // can flip to `AssistantBlock::Transcript { source: Spoken }`.
    // ------------------------------------------------------------------

    #[test]
    fn t10_assistant_text_delta_helper_builds_text_delta_event() {
        let identity = LiveTranscriptIdentity {
            provider_item_id: Some("item_text"),
            previous_item_id: Some("item_prev"),
            content_index: Some(2),
            response_id: Some("resp_text"),
            delta_id: Some("delta_text"),
        };
        let event = build_assistant_text_delta_event("display fragment", identity);
        match event {
            RealtimeTranscriptEvent::AssistantTextDelta {
                response_id,
                delta_id,
                item_id,
                previous_item_id,
                content_index,
                delta,
            } => {
                assert_eq!(response_id, "resp_text");
                assert_eq!(delta_id, "delta_text");
                assert_eq!(item_id, "item_text");
                assert_eq!(previous_item_id.as_deref(), Some("item_prev"));
                assert_eq!(content_index, 2);
                assert_eq!(delta, "display fragment");
            }
            other => panic!("display-text delta path must build AssistantTextDelta, got {other:?}"),
        }
    }

    #[test]
    fn t10_assistant_transcript_delta_helper_builds_transcript_delta_event() {
        // T10 acceptance: the spoken-transcript path no longer reuses
        // `AssistantTextDelta`; the staged event is the dedicated
        // `AssistantTranscriptDelta` variant so the materializer routes
        // it to `AssistantBlock::Transcript`.
        let identity = LiveTranscriptIdentity {
            provider_item_id: Some("item_tx"),
            previous_item_id: Some("item_prev"),
            content_index: Some(0),
            response_id: Some("resp_tx"),
            delta_id: Some("delta_tx"),
        };
        let event = build_assistant_transcript_delta_event("spoken fragment", identity);
        match event {
            RealtimeTranscriptEvent::AssistantTranscriptDelta {
                response_id,
                delta_id,
                item_id,
                previous_item_id,
                content_index,
                delta,
            } => {
                assert_eq!(response_id, "resp_tx");
                assert_eq!(delta_id, "delta_tx");
                assert_eq!(item_id, "item_tx");
                assert_eq!(previous_item_id.as_deref(), Some("item_prev"));
                assert_eq!(content_index, 0);
                assert_eq!(delta, "spoken fragment");
            }
            other => panic!(
                "spoken-transcript delta path must build AssistantTranscriptDelta, got {other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn t10_signal_turn_completed_flushes_transcript_block_after_transcript_final_buffered() {
        // T10 acceptance, end-to-end through the buffer: a buffered
        // transcript final must flush as `AssistantBlock::Transcript`
        // (not `AssistantBlock::Text`) when `signal_turn_completed`
        // drains the per-(session, response_id) slot. Mirrors the
        // production sink's `pending_to_block` dispatch.
        let sink = BufferingTestSink::new();
        let session_id = test_session_id();

        sink.append_assistant_transcript_final(&session_id, "I said hi", Some("resp_spoken"));
        sink.signal_turn_completed(
            &session_id,
            StopReason::EndTurn,
            real_usage(),
            Some("resp_spoken"),
        );

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            CapturedRuntimeCall::ExternalAssistantOutput { blocks, .. } => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    AssistantBlock::Transcript { text, source, .. } => {
                        assert_eq!(text, "I said hi");
                        assert_eq!(*source, TranscriptSource::Spoken);
                    }
                    other => panic!(
                        "transcript final must flush as AssistantBlock::Transcript, got {other:?}"
                    ),
                }
            }
            other => panic!("expected ExternalAssistantOutput, got {other:?}"),
        }
    }
}
