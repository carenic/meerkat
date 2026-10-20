//! Ordered projection of one live channel's adapter observations, off the
//! transport's frame loop.
//!
//! The WebSocket and WebRTC pumps forward each adapter observation to their
//! client as it arrives. The observations that write the session (transcript
//! rows, output starts, tool calls, interrupts) are applied here instead, by
//! one task per channel, strictly in their arrival order. A row that waits
//! for the session's turn boundary while a member turn runs then holds back
//! only the rows behind it, never the channel's audio, its other events, or
//! the client's input.
//!
//! The task holds the boundary across consecutive rows that take it through
//! the channel's own guard (see
//! [`crate::LiveProjectionSink::hold_channel_projection_boundary`]), so the
//! rows that queued behind a member turn land together when it ends instead
//! of one row per queued member turn. It never waits for anything else while
//! it holds the boundary.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use meerkat_core::live_adapter::{LiveAdapterErrorCode, LiveAdapterObservation};
use tokio::sync::mpsc;

use crate::host::{
    LiveAdapterHost, LiveAdapterHostError, LiveChannelId, LiveProjectionBoundaryHold,
    LiveProjectionError, ObservationOutcome, ObservationRouting,
};

/// Serialized row bytes a channel's projection queue holds at most. The row
/// that reaches the bound is still queued and applied; the transport then
/// ends the channel with [`LiveAdapterHostError::ProjectionBacklogFull`]
/// instead of reading on, so no row it observed is dropped. Transcript
/// deltas are a few hundred bytes, so this is tens of thousands of rows:
/// minutes of continuous speech behind one member turn.
pub(crate) const LIVE_PROJECTION_QUEUE_BYTES: usize = 16 * 1024 * 1024;

/// Rows applied under one hold of the session's turn boundary at most.
const BOUNDARY_HOLD_ROWS: usize = 64;
/// How long one hold of the session's turn boundary lasts at most. With
/// [`BOUNDARY_HOLD_ROWS`] it bounds how long a queued member turn waits for
/// the rows ahead of it: this span plus the row in flight when it ends.
const BOUNDARY_HOLD_SPAN: Duration = Duration::from_secs(1);

/// One unit of session projection, in the channel's arrival order.
#[derive(Debug)]
pub(crate) enum ProjectionRow {
    /// An adapter observation that writes the session.
    Observation(Box<LiveAdapterObservation>),
    /// A barge-in the transport detected itself (WebRTC local speech over
    /// queued output audio). Its output audio was discarded when the
    /// transport queued it.
    #[cfg_attr(not(feature = "webrtc"), allow(dead_code))]
    TransportBargeIn,
}

impl ProjectionRow {
    /// Whether the row's projection takes the session's turn boundary through
    /// the channel's own guard, so it may run under the queue's hold. Every
    /// other row takes an unconditional guard, waits on a tool, or interrupts
    /// the running turn, and would wait for the hold itself.
    fn runs_under_boundary_hold(&self) -> bool {
        match self {
            Self::Observation(observation) => observation_runs_under_boundary_hold(observation),
            Self::TransportBargeIn => false,
        }
    }

    /// The row's size for the queue bound: its serialized length.
    fn queued_bytes(&self) -> usize {
        match self {
            Self::Observation(observation) => {
                let mut counter = ByteCounter(0);
                match serde_json::to_writer(&mut counter, observation) {
                    Ok(()) => counter.0,
                    // Unserializable rows are not expected; count them as
                    // large rather than free.
                    Err(_) => 64 * 1024,
                }
            }
            Self::TransportBargeIn => 64,
        }
    }

    /// The row's kind (its wire tag) and provider item id, for the error
    /// frame that names a row whose projection failed.
    fn identity(&self) -> (String, Option<String>) {
        let Self::Observation(observation) = self else {
            return ("transport_barge_in".to_string(), None);
        };
        let mut observation: &LiveAdapterObservation = observation;
        while let LiveAdapterObservation::WithContextObservation {
            observation: inner, ..
        } = observation
        {
            observation = inner;
        }
        let value = serde_json::to_value(observation).unwrap_or_default();
        let kind = value
            .get("observation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let item_id = ["provider_item_id", "item_id", "provider_call_id"]
            .iter()
            .find_map(|field| value.get(field).and_then(serde_json::Value::as_str))
            .or_else(|| {
                value
                    .get("event")
                    .and_then(|event| event.get("item_id"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::to_string);
        (kind, item_id)
    }
}

struct ByteCounter(usize);

impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn observation_runs_under_boundary_hold(observation: &LiveAdapterObservation) -> bool {
    match observation {
        LiveAdapterObservation::WithContextObservation { observation, .. } => {
            observation_runs_under_boundary_hold(observation)
        }
        // Without a provider item id the sinks commit the text through an
        // unconditional guard.
        LiveAdapterObservation::UserTranscriptFinal {
            provider_item_id, ..
        } => provider_item_id.is_some(),
        LiveAdapterObservation::AssistantTextDelta { .. }
        | LiveAdapterObservation::AssistantTranscriptDelta { .. }
        | LiveAdapterObservation::AssistantTranscriptFinal { .. }
        | LiveAdapterObservation::AssistantTranscriptTruncated { .. }
        | LiveAdapterObservation::AssistantOutputStarted { .. }
        | LiveAdapterObservation::AssistantPlaybackCompleted { .. }
        | LiveAdapterObservation::AssistantPlaybackTerminalObserved { .. } => true,
        _ => false,
    }
}

/// Whether applying `observation` writes the session, so a transport hands
/// it to its projection queue instead of applying it on its frame loop.
///
/// Close observations (`Error`, a terminal status) are not rows: the
/// transport drains the queue before it closes. Everything else (audio,
/// status, scoped command rejections, raw receipts) writes no transcript row
/// and is applied on the frame loop as before. A status can therefore be
/// recorded before rows queued ahead of it; status is a channel fact, not a
/// transcript row.
pub(crate) fn observation_writes_session(observation: &LiveAdapterObservation) -> bool {
    matches!(
        LiveAdapterHost::classify_observation(observation),
        ObservationRouting::AdmitAssistantOutput
            | ObservationRouting::AppendTranscript
            | ObservationRouting::AppendRealtimeTranscript
            | ObservationRouting::DispatchToolCall { .. }
            | ObservationRouting::SignalInterrupt
    )
}

/// What a channel's projection task tells its transport.
#[derive(Debug)]
pub(crate) enum ProjectionReport {
    /// A user-content receipt the host synthesized only after the session
    /// durably committed the content. The transport publishes it now.
    UserContentCommitted(LiveAdapterObservation),
    /// A row's projection terminalized the channel. The queue stopped.
    Terminal(LiveAdapterErrorCode),
    /// A row's projection failed. The queue stopped: no later row is applied.
    Failed {
        error: LiveAdapterHostError,
        /// The failed row's kind (its wire tag).
        row: String,
        /// The failed row's provider item id, when it has one.
        item_id: Option<String>,
    },
}

/// The error frame a transport sends when its channel's projection fails or
/// its queue reaches the bound, before it closes the channel. A client that
/// was already shown the frames of the failed row and of the rows after it
/// learns which row the session refused and how many later rows it did not
/// apply.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ProjectionFailureFrame {
    pub(crate) error: String,
    pub(crate) reason: &'static str,
    /// The failed row's kind (its wire tag); absent when no row failed (the
    /// queue reached its bound and every queued row is still applied).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) row: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) item_id: Option<String>,
    /// Rows observed after the failed one that the session did not apply.
    pub(crate) unapplied_rows: usize,
}

impl ProjectionFailureFrame {
    pub(crate) fn backlog_full(error: &LiveAdapterHostError) -> Self {
        Self {
            error: error.to_string(),
            reason: error.reason_code(),
            row: None,
            item_id: None,
            unapplied_rows: 0,
        }
    }

    pub(crate) fn failed_row(
        error: &LiveAdapterHostError,
        row: String,
        item_id: Option<String>,
        unapplied_rows: usize,
    ) -> Self {
        Self {
            error: error.to_string(),
            reason: error.reason_code(),
            row: Some(row),
            item_id,
            unapplied_rows,
        }
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

struct QueuedRow {
    row: ProjectionRow,
    bytes: usize,
}

/// A transport's handle on its channel's projection task.
///
/// The task applies every row it accepted, in order, also after the handle
/// is dropped (a transport that closes does not wait for a running member
/// turn), and then ends. It holds only the host handle and its queue, no
/// transport resources. It stops at the first failed or terminal row, as
/// the frame loop did when it applied rows itself, and counts and logs the
/// later rows it did not apply.
pub(crate) struct LiveProjectionQueue {
    rows: Option<mpsc::UnboundedSender<QueuedRow>>,
    queued_rows: Arc<AtomicUsize>,
    queued_bytes: Arc<AtomicUsize>,
    reports: mpsc::UnboundedReceiver<ProjectionReport>,
    channel_id: LiveChannelId,
}

impl LiveProjectionQueue {
    /// Spawn the projection task for `channel_id`.
    pub(crate) fn spawn(host: Arc<LiveAdapterHost>, channel_id: LiveChannelId) -> Self {
        let (rows_tx, rows_rx) = mpsc::unbounded_channel();
        let (reports_tx, reports_rx) = mpsc::unbounded_channel();
        let queued_rows = Arc::new(AtomicUsize::new(0));
        let queued_bytes = Arc::new(AtomicUsize::new(0));
        tokio::spawn(run_projection(
            host,
            channel_id.clone(),
            rows_rx,
            Arc::clone(&queued_rows),
            Arc::clone(&queued_bytes),
            reports_tx,
        ));
        Self {
            rows: Some(rows_tx),
            queued_rows,
            queued_bytes,
            reports: reports_rx,
            channel_id,
        }
    }

    /// Queue `row` without waiting.
    ///
    /// `Err` means the transport must end the channel instead of reading on:
    /// [`LiveAdapterHostError::ProjectionBacklogFull`] when this row reached
    /// the bound (it is still queued and will be applied), a projection error
    /// when the task is gone.
    pub(crate) fn enqueue(&self, row: ProjectionRow) -> Result<(), LiveAdapterHostError> {
        let Some(rows) = self.rows.as_ref() else {
            return Err(projection_task_gone());
        };
        let bytes = row.queued_bytes();
        self.queued_rows.fetch_add(1, Ordering::AcqRel);
        let before = self.queued_bytes.fetch_add(bytes, Ordering::AcqRel);
        if rows.send(QueuedRow { row, bytes }).is_err() {
            self.queued_rows.fetch_sub(1, Ordering::AcqRel);
            self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
            return Err(projection_task_gone());
        }
        let after = before.saturating_add(bytes);
        let half = LIVE_PROJECTION_QUEUE_BYTES / 2;
        if before < half && after >= half {
            tracing::warn!(
                channel = %self.channel_id,
                queued_bytes = after,
                queued_rows = self.queued_rows.load(Ordering::Acquire),
                bound_bytes = LIVE_PROJECTION_QUEUE_BYTES,
                "live projection queue is half full; the session's turn boundary is held"
            );
        }
        if after >= LIVE_PROJECTION_QUEUE_BYTES {
            return Err(LiveAdapterHostError::ProjectionBacklogFull {
                bound_bytes: LIVE_PROJECTION_QUEUE_BYTES,
            });
        }
        Ok(())
    }

    /// Accept no more rows: the task applies the rows it has, then ends, and
    /// [`Self::next_report`] returns `None` after its last report.
    pub(crate) fn seal(&mut self) {
        self.rows = None;
    }

    /// The task's next report; `None` once the task has ended. Cancel-safe.
    pub(crate) async fn next_report(&mut self) -> Option<ProjectionReport> {
        self.reports.recv().await
    }

    /// Rows queued and not applied. After a [`ProjectionReport::Failed`],
    /// once the transport stops queueing, this is the number of rows after
    /// the failed one that the session did not apply.
    pub(crate) fn unapplied_rows(&self) -> usize {
        self.queued_rows.load(Ordering::Acquire)
    }
}

fn projection_task_gone() -> LiveAdapterHostError {
    LiveAdapterHostError::ProjectionError(LiveProjectionError::Rejected(
        "the live channel's projection task has ended".to_string(),
    ))
}

async fn run_projection(
    host: Arc<LiveAdapterHost>,
    channel_id: LiveChannelId,
    mut rows: mpsc::UnboundedReceiver<QueuedRow>,
    queued_rows: Arc<AtomicUsize>,
    queued_bytes: Arc<AtomicUsize>,
    reports: mpsc::UnboundedSender<ProjectionReport>,
) {
    let mut hold = BoundaryHold::default();
    let mut stopped = false;
    let mut unapplied = 0_usize;
    loop {
        // Never wait for the next row while holding the boundary.
        let queued = match rows.try_recv() {
            Ok(queued) => queued,
            Err(mpsc::error::TryRecvError::Empty) => {
                hold.release();
                match rows.recv().await {
                    Some(queued) => queued,
                    None => break,
                }
            }
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        };
        queued_bytes.fetch_sub(queued.bytes, Ordering::AcqRel);
        if stopped {
            // Counted, not applied: it stays in `queued_rows` for the
            // transport's error frame.
            unapplied += 1;
            continue;
        }
        let row = queued.row;
        if row.runs_under_boundary_hold() {
            hold.take(&host, &channel_id).await;
        } else {
            hold.release();
        }
        let result = apply_row(&host, &channel_id, &row).await;
        hold.row_applied();
        let report = match result {
            Ok(ObservationOutcome::UserContentCommitted { observation }) => {
                Some(ProjectionReport::UserContentCommitted(*observation))
            }
            Ok(ObservationOutcome::Terminal { code }) => {
                stopped = true;
                Some(ProjectionReport::Terminal(code))
            }
            Ok(_) => None,
            Err(error) => {
                stopped = true;
                let (kind, item_id) = row.identity();
                tracing::warn!(
                    channel = %channel_id,
                    error = %error,
                    row = %kind,
                    item_id = item_id.as_deref().unwrap_or(""),
                    "live projection failed; the channel closes and its later rows are not applied"
                );
                Some(ProjectionReport::Failed {
                    error,
                    row: kind,
                    item_id,
                })
            }
        };
        queued_rows.fetch_sub(1, Ordering::AcqRel);
        if stopped {
            hold.release();
        }
        if let Some(report) = report {
            // The transport may have closed without waiting; the rows still
            // land, only their reports have no reader.
            let _ = reports.send(report);
        }
    }
    hold.release();
    if unapplied > 0 {
        tracing::warn!(
            channel = %channel_id,
            unapplied,
            "live projection stopped at a failed or terminal row; later rows were not applied"
        );
    }
}

async fn apply_row(
    host: &LiveAdapterHost,
    channel_id: &LiveChannelId,
    row: &ProjectionRow,
) -> Result<ObservationOutcome, LiveAdapterHostError> {
    match row {
        ProjectionRow::Observation(observation) => {
            host.apply_observation(channel_id, observation).await
        }
        ProjectionRow::TransportBargeIn => host
            .signal_transport_barge_in(channel_id)
            .await
            .map(|()| ObservationOutcome::InterruptSignalled),
    }
}

/// The queue's hold on its session's turn boundary.
#[derive(Default)]
struct BoundaryHold {
    hold: Option<LiveProjectionBoundaryHold>,
    taken_at: Option<tokio::time::Instant>,
    rows: usize,
}

impl BoundaryHold {
    /// Take the boundary for the next row unless it is already held. A
    /// refused hold (a sink without holds, a session that is gone) leaves the
    /// row to take the boundary itself, which meets the same answer.
    async fn take(&mut self, host: &LiveAdapterHost, channel_id: &LiveChannelId) {
        if self.hold.is_some() {
            return;
        }
        // Behind a running member turn this is where the queue waits.
        let hold = meerkat_core::slow_await::warn_if_slow(
            channel_id,
            "live projection: hold the turn boundary",
            Box::pin(host.hold_projection_boundary(channel_id)),
        )
        .await;
        if let Ok(Some(hold)) = hold {
            self.hold = Some(hold);
            self.taken_at = Some(tokio::time::Instant::now());
            self.rows = 0;
        }
    }

    /// A row landed: keep the hold only within its bounds.
    fn row_applied(&mut self) {
        if self.hold.is_none() {
            return;
        }
        self.rows += 1;
        let spent = self
            .taken_at
            .is_some_and(|taken_at| taken_at.elapsed() >= BOUNDARY_HOLD_SPAN);
        if self.rows >= BOUNDARY_HOLD_ROWS || spent {
            self.release();
        }
    }

    fn release(&mut self) {
        self.hold = None;
        self.taken_at = None;
        self.rows = 0;
    }
}

/// A projection sink whose session writes queue on a test turn boundary the
/// way the session's do, for the transports' projection tests.
#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod test_support {
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use meerkat_core::types::SessionId;

    use crate::host::{
        LiveAssistantOutputAddress, LiveChannelId, LiveProjectionBoundaryHold, LiveProjectionError,
        LiveProjectionSink, LiveTranscriptIdentity,
    };

    /// One session write the sink accepted.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct AppliedRow {
        pub(crate) row: &'static str,
        pub(crate) text: String,
        /// Whether it ran under the channel's boundary hold.
        pub(crate) under_hold: bool,
    }

    /// Channel-guarded writes (transcript rows with an item id, output and
    /// playback rows) take `gate` unless the channel's hold holds it; every
    /// other write always takes `gate`, so one made under the hold would
    /// wait for the hold itself. A test holds `gate` as the member turn.
    #[derive(Default)]
    pub(crate) struct GatedRecordingSink {
        pub(crate) gate: Arc<tokio::sync::Mutex<()>>,
        hold_active: Arc<AtomicBool>,
        holds_taken: AtomicUsize,
        applied: StdMutex<Vec<AppliedRow>>,
        applied_changed: tokio::sync::Notify,
        fail_row: StdMutex<Option<&'static str>>,
        refused: StdMutex<Vec<&'static str>>,
    }

    struct TestBoundaryHold {
        _boundary: tokio::sync::OwnedMutexGuard<()>,
        active: Arc<AtomicBool>,
    }

    impl Drop for TestBoundaryHold {
        fn drop(&mut self) {
            self.active.store(false, Ordering::SeqCst);
        }
    }

    impl GatedRecordingSink {
        pub(crate) fn applied(&self) -> Vec<AppliedRow> {
            self.applied
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        pub(crate) fn applied_rows(&self) -> Vec<&'static str> {
            self.applied().into_iter().map(|row| row.row).collect()
        }

        /// The writes the sink refused, in order.
        pub(crate) fn refused_rows(&self) -> Vec<&'static str> {
            self.refused
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        pub(crate) fn hold_active(&self) -> bool {
            self.hold_active.load(Ordering::SeqCst)
        }

        pub(crate) fn holds_taken(&self) -> usize {
            self.holds_taken.load(Ordering::SeqCst)
        }

        /// Refuse every later write of this row kind.
        pub(crate) fn fail_row(&self, row: &'static str) {
            *self
                .fail_row
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(row);
        }

        /// Wait until at least `count` writes were accepted.
        pub(crate) async fn wait_for_applied(&self, count: usize) -> Vec<AppliedRow> {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let notified = self.applied_changed.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    let applied = self.applied();
                    if applied.len() >= count {
                        return applied;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "expected {count} applied rows within 5 s, got {:?}",
                    self.applied()
                )
            })
        }

        async fn write(
            &self,
            row: &'static str,
            text: &str,
            channel_guarded: bool,
        ) -> Result<(), LiveProjectionError> {
            if *self
                .fail_row
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                == Some(row)
            {
                self.refused
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(row);
                return Err(LiveProjectionError::Rejected(format!(
                    "scripted refusal of {row}"
                )));
            }
            let under_hold = channel_guarded && self.hold_active();
            let _boundary = if under_hold {
                None
            } else {
                Some(self.gate.lock().await)
            };
            self.applied
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(AppliedRow {
                    row,
                    text: text.to_string(),
                    under_hold,
                });
            self.applied_changed.notify_waiters();
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl LiveProjectionSink for GatedRecordingSink {
        async fn append_user_transcript(
            &self,
            _session_id: &SessionId,
            text: &str,
            identity: LiveTranscriptIdentity<'_>,
        ) -> Result<(), LiveProjectionError> {
            // As the real sinks: without an item id the text is committed
            // through an unconditional guard.
            self.write(
                "user_transcript_final",
                text,
                identity.provider_item_id.is_some(),
            )
            .await
        }

        async fn append_assistant_text_delta(
            &self,
            _session_id: &SessionId,
            delta: &str,
            _identity: LiveTranscriptIdentity<'_>,
        ) -> Result<(), LiveProjectionError> {
            self.write("assistant_text_delta", delta, true).await
        }

        async fn append_assistant_transcript_delta(
            &self,
            _session_id: &SessionId,
            delta: &str,
            _identity: LiveTranscriptIdentity<'_>,
        ) -> Result<(), LiveProjectionError> {
            self.write("assistant_transcript_delta", delta, true).await
        }

        async fn append_assistant_text_final(
            &self,
            _session_id: &SessionId,
            text: &str,
            _identity: LiveTranscriptIdentity<'_>,
            _stop_reason: meerkat_core::types::StopReason,
            _usage: meerkat_core::types::Usage,
            _response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.write("assistant_text_final", text, true).await
        }

        async fn append_assistant_transcript_final(
            &self,
            _session_id: &SessionId,
            text: &str,
            _identity: LiveTranscriptIdentity<'_>,
            _stop_reason: meerkat_core::types::StopReason,
            _usage: meerkat_core::types::Usage,
            _response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.write("assistant_transcript_final", text, true).await
        }

        async fn admit_assistant_playback_target(
            &self,
            _session_id: &SessionId,
            channel_id: &LiveChannelId,
            _provider_turn_ref: &str,
            _response_id: &str,
            provider_item_id: &str,
            content_index: u32,
        ) -> Result<LiveAssistantOutputAddress, LiveProjectionError> {
            self.write("assistant_output_started", provider_item_id, true)
                .await?;
            Ok(LiveAssistantOutputAddress {
                channel_id: channel_id.clone(),
                output_id: "gated-recording-output".to_string(),
                content_index,
            })
        }

        async fn complete_assistant_playback(
            &self,
            _session_id: &SessionId,
            _channel_id: &LiveChannelId,
            _interaction_id: meerkat_core::InteractionId,
            _response_id: &str,
            provider_item_id: &str,
            _content_index: u32,
            _stop_reason: meerkat_core::types::StopReason,
            _usage: meerkat_core::TurnUsage,
        ) -> Result<(), LiveProjectionError> {
            self.write("assistant_playback_completed", provider_item_id, true)
                .await
        }

        async fn fail_assistant_output_publication(
            &self,
            _session_id: &SessionId,
            _address: &LiveAssistantOutputAddress,
        ) -> Result<(), LiveProjectionError> {
            Ok(())
        }

        async fn truncate_assistant_transcript(
            &self,
            _session_id: &SessionId,
            _channel_id: &LiveChannelId,
            _interaction_id: Option<meerkat_core::InteractionId>,
            provider_item_id: Option<&str>,
            _previous_item_id: Option<&str>,
            _content_index: Option<u32>,
            _response_id: Option<&str>,
            _text: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.write(
                "assistant_transcript_truncated",
                provider_item_id.unwrap_or_default(),
                true,
            )
            .await
        }

        async fn signal_turn_interrupt(
            &self,
            _session_id: &SessionId,
            response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            // A transport barge-in carries no provider response id.
            let row = if response_id.is_some() {
                "turn_interrupted"
            } else {
                "transport_barge_in"
            };
            self.write(row, response_id.unwrap_or_default(), false)
                .await
        }

        async fn signal_output_audio_degraded(
            &self,
            _session_id: &SessionId,
            _dropped: u64,
        ) -> Result<(), LiveProjectionError> {
            Ok(())
        }

        async fn signal_turn_completed(
            &self,
            _session_id: &SessionId,
            _stop_reason: meerkat_core::types::StopReason,
            _usage: meerkat_core::TurnUsage,
            response_id: Option<&str>,
        ) -> Result<(), LiveProjectionError> {
            self.write("turn_completed", response_id.unwrap_or_default(), false)
                .await
        }

        async fn signal_terminal_error(
            &self,
            _session_id: &SessionId,
            _code: meerkat_core::live_adapter::LiveAdapterErrorCode,
            message: &str,
        ) -> Result<(), LiveProjectionError> {
            self.write("terminal_error", message, false).await
        }

        async fn append_realtime_transcript(
            &self,
            _session_id: &SessionId,
            _event: &meerkat_core::RealtimeTranscriptEvent,
        ) -> Result<meerkat_core::RealtimeTranscriptApplyOutcome, LiveProjectionError> {
            self.write("realtime_transcript", "", false).await?;
            Ok(meerkat_core::RealtimeTranscriptApplyOutcome::default())
        }

        async fn hold_channel_projection_boundary(
            &self,
            _session_id: &SessionId,
            _channel_id: &LiveChannelId,
        ) -> Result<Option<LiveProjectionBoundaryHold>, LiveProjectionError> {
            let boundary = Arc::clone(&self.gate).lock_owned().await;
            self.hold_active.store(true, Ordering::SeqCst);
            self.holds_taken.fetch_add(1, Ordering::SeqCst);
            Ok(Some(LiveProjectionBoundaryHold::new(TestBoundaryHold {
                _boundary: boundary,
                active: Arc::clone(&self.hold_active),
            })))
        }
    }

    /// Observation builders for the projection tests.
    pub(crate) fn user_final(
        item: &str,
        text: &str,
    ) -> meerkat_core::live_adapter::LiveAdapterObservation {
        meerkat_core::live_adapter::LiveAdapterObservation::UserTranscriptFinal {
            provider_item_id: Some(item.to_string()),
            previous_item_id: None,
            content_index: Some(0),
            text: text.to_string(),
        }
    }

    pub(crate) fn assistant_delta(
        item: &str,
        delta_id: &str,
        delta: &str,
    ) -> meerkat_core::live_adapter::LiveAdapterObservation {
        meerkat_core::live_adapter::LiveAdapterObservation::AssistantTranscriptDelta {
            provider_item_id: Some(item.to_string()),
            previous_item_id: None,
            content_index: Some(0),
            response_id: Some("resp_1".to_string()),
            delta_id: Some(delta_id.to_string()),
            delta: delta.to_string(),
        }
    }

    pub(crate) fn assistant_final(
        item: &str,
        text: &str,
    ) -> meerkat_core::live_adapter::LiveAdapterObservation {
        meerkat_core::live_adapter::LiveAdapterObservation::AssistantTranscriptFinal {
            provider_item_id: item.to_string(),
            previous_item_id: None,
            content_index: Some(0),
            response_id: None,
            text: text.to_string(),
            stop_reason: meerkat_core::types::StopReason::EndTurn,
            usage: meerkat_core::types::Usage::default(),
        }
    }

    pub(crate) fn turn_completed(
        response: &str,
    ) -> meerkat_core::live_adapter::LiveAdapterObservation {
        meerkat_core::live_adapter::LiveAdapterObservation::TurnCompleted {
            response_id: Some(response.to_string()),
            stop_reason: meerkat_core::types::StopReason::EndTurn,
            usage: meerkat_core::TurnUsage::host_declared(
                meerkat_core::Provider::Other,
                "live-projection-test",
                meerkat_core::types::Usage::default(),
            ),
        }
    }

    pub(crate) fn turn_interrupted(
        response: &str,
    ) -> meerkat_core::live_adapter::LiveAdapterObservation {
        meerkat_core::live_adapter::LiveAdapterObservation::TurnInterrupted {
            response_id: Some(response.to_string()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::test_support::{
        GatedRecordingSink, assistant_delta, assistant_final, turn_completed, user_final,
    };
    use super::*;
    use meerkat_core::types::SessionId;

    async fn queue_with_gated_sink() -> (
        Arc<GatedRecordingSink>,
        Arc<LiveAdapterHost>,
        LiveChannelId,
        LiveProjectionQueue,
    ) {
        let sink = Arc::new(GatedRecordingSink::default());
        let host = Arc::new(LiveAdapterHost::new(
            Arc::clone(&sink) as Arc<dyn crate::LiveProjectionSink>
        ));
        let channel_id = host
            .open_channel_with_generated_test_machine_authority(SessionId::new())
            .await
            .unwrap();
        let queue = LiveProjectionQueue::spawn(Arc::clone(&host), channel_id.clone());
        (sink, host, channel_id, queue)
    }

    /// Rows queued while a member turn holds the session's turn boundary land
    /// in arrival order when it ends: the channel-guarded ones under one hold,
    /// and a channel-less row (here `TurnCompleted`) only after the hold is
    /// dropped (under it, its unconditional guard would wait forever).
    #[tokio::test]
    async fn rows_queued_behind_a_member_turn_land_in_order_under_one_hold() {
        let (sink, _host, _channel_id, queue) = queue_with_gated_sink().await;
        let member_turn = Arc::clone(&sink.gate).lock_owned().await;
        for row in [
            user_final("item_user", "hello"),
            assistant_delta("item_assistant", "d1", "Hi"),
            assistant_delta("item_assistant", "d2", " there"),
            assistant_final("item_assistant", "Hi there"),
            turn_completed("resp_1"),
            user_final("item_user_2", "again"),
        ] {
            queue
                .enqueue(ProjectionRow::Observation(Box::new(row)))
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            sink.applied().is_empty(),
            "no row lands while the member turn holds the boundary"
        );
        assert_eq!(queue.unapplied_rows(), 6);

        drop(member_turn);
        let applied = sink.wait_for_applied(6).await;
        assert_eq!(
            applied.iter().map(|row| row.row).collect::<Vec<_>>(),
            [
                "user_transcript_final",
                "assistant_transcript_delta",
                "assistant_transcript_delta",
                "assistant_transcript_final",
                "turn_completed",
                "user_transcript_final",
            ],
            "rows land in arrival order"
        );
        assert_eq!(
            applied.iter().map(|row| row.under_hold).collect::<Vec<_>>(),
            [true, true, true, true, false, true],
            "guarded rows run under the hold; the channel-less row without it"
        );
        assert_eq!(sink.holds_taken(), 2, "one hold per run of guarded rows");
        tokio::time::timeout(Duration::from_secs(5), async {
            while sink.hold_active() || queue.unapplied_rows() > 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the hold is dropped once the queue is empty");
    }

    /// A refused row stops the queue, as the frame loop stopped when it
    /// applied rows itself: the report names the row, and the rows after it
    /// stay counted as not applied.
    #[tokio::test]
    async fn a_refused_row_stops_the_queue_and_counts_the_rows_after_it() {
        let (sink, _host, _channel_id, mut queue) = queue_with_gated_sink().await;
        sink.fail_row("assistant_transcript_final");
        let member_turn = Arc::clone(&sink.gate).lock_owned().await;
        for row in [
            assistant_delta("item_assistant", "d1", "Hi"),
            assistant_final("item_assistant", "Hi"),
            assistant_delta("item_next", "d2", "later"),
            user_final("item_user", "later"),
        ] {
            queue
                .enqueue(ProjectionRow::Observation(Box::new(row)))
                .unwrap();
        }
        drop(member_turn);
        let report = tokio::time::timeout(Duration::from_secs(5), queue.next_report())
            .await
            .unwrap();
        let Some(ProjectionReport::Failed {
            error,
            row,
            item_id,
        }) = report
        else {
            panic!("expected a failed report, got {report:?}");
        };
        assert_eq!(error.reason_code(), "projection_error");
        assert_eq!(row, "assistant_transcript_final");
        assert_eq!(item_id.as_deref(), Some("item_assistant"));
        queue.seal();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), queue.next_report())
                .await
                .unwrap()
                .map(|report| format!("{report:?}")),
            None,
            "the task ends once sealed"
        );
        assert_eq!(
            queue.unapplied_rows(),
            2,
            "the two later rows were not applied"
        );
        assert_eq!(sink.applied_rows(), ["assistant_transcript_delta"]);
        assert!(!sink.hold_active(), "a stopped queue holds no boundary");
    }

    struct HoldObservingDispatcher {
        sink: Arc<GatedRecordingSink>,
        saw_hold: std::sync::Mutex<Vec<bool>>,
    }

    #[async_trait::async_trait]
    impl crate::LiveToolDispatcher for HoldObservingDispatcher {
        async fn dispatch_live_tool_call(
            &self,
            _session_id: &SessionId,
            _call: meerkat_core::types::ToolCall,
        ) -> Result<meerkat_core::ToolDispatchOutcome, crate::LiveToolDispatchError> {
            self.saw_hold.lock().unwrap().push(self.sink.hold_active());
            Err(crate::LiveToolDispatchError::Rejected(
                "scripted tool refusal".to_string(),
            ))
        }
    }

    struct AcceptingAdapter;

    #[async_trait::async_trait]
    impl meerkat_core::live_adapter::LiveAdapter for AcceptingAdapter {
        async fn send_command(
            &self,
            _command: meerkat_core::live_adapter::LiveAdapterCommand,
        ) -> Result<(), meerkat_core::live_adapter::LiveAdapterError> {
            Ok(())
        }

        async fn next_observation(
            &self,
        ) -> Result<Option<LiveAdapterObservation>, meerkat_core::live_adapter::LiveAdapterError>
        {
            std::future::pending().await
        }

        fn status(&self) -> meerkat_core::live_adapter::LiveAdapterStatus {
            meerkat_core::live_adapter::LiveAdapterStatus::Ready
        }

        async fn close(&self) -> Result<(), meerkat_core::live_adapter::LiveAdapterError> {
            Ok(())
        }
    }

    /// A tool call between guarded rows runs with no boundary hold held (a
    /// tool may wait for the session itself), and the rows around it keep
    /// their order.
    #[tokio::test]
    async fn tool_rows_run_with_no_boundary_hold() {
        let (sink, host, channel_id, queue) = queue_with_gated_sink().await;
        host.attach_adapter(&channel_id, Arc::new(AcceptingAdapter))
            .await
            .unwrap();
        let dispatcher = Arc::new(HoldObservingDispatcher {
            sink: Arc::clone(&sink),
            saw_hold: std::sync::Mutex::new(Vec::new()),
        });
        host.set_live_tool_dispatcher(Arc::clone(&dispatcher) as _);
        let member_turn = Arc::clone(&sink.gate).lock_owned().await;
        for row in [
            assistant_delta("item_assistant", "d1", "Let me check"),
            LiveAdapterObservation::ToolCallRequested {
                provider_call_id: meerkat_core::ToolCallId("call_1".to_string()),
                tool_name: meerkat_core::types::ToolName::new("lookup"),
                arguments: serde_json::json!({}),
            },
            assistant_delta("item_assistant", "d2", " done"),
        ] {
            queue
                .enqueue(ProjectionRow::Observation(Box::new(row)))
                .unwrap();
        }
        drop(member_turn);
        let applied = sink.wait_for_applied(2).await;
        assert_eq!(
            applied
                .iter()
                .map(|row| row.text.as_str())
                .collect::<Vec<_>>(),
            ["Let me check", " done"]
        );
        assert_eq!(
            *dispatcher.saw_hold.lock().unwrap(),
            [false],
            "the tool ran once, with no hold held"
        );
    }

    /// The row that reaches the byte bound is still queued and applied; the
    /// error tells the transport to stop reading and close.
    #[tokio::test]
    async fn the_row_reaching_the_byte_bound_is_queued_and_every_row_lands() {
        let (sink, _host, _channel_id, queue) = queue_with_gated_sink().await;
        let member_turn = Arc::clone(&sink.gate).lock_owned().await;
        let text = "x".repeat(1024 * 1024);
        let mut queued = 0;
        let error = loop {
            queued += 1;
            match queue.enqueue(ProjectionRow::Observation(Box::new(user_final(
                &format!("item_{queued}"),
                &text,
            )))) {
                Ok(()) => assert!(queued < 64, "the bound must be reached"),
                Err(error) => break error,
            }
        };
        assert_eq!(error.reason_code(), "projection_backlog_full");
        assert_eq!(queue.unapplied_rows(), queued);
        drop(queue);
        drop(member_turn);
        let applied = sink.wait_for_applied(queued).await;
        assert_eq!(applied.len(), queued, "no queued row is dropped");
    }
}
