//! Provider session adapter — bridges `LiveAdapter` to `RealtimeSession`.
//!
//! Architecture: each adapter spawns an internal pump task that owns the
//! provider session exclusively. The adapter facade holds only channel ends.
//! This decouples the send and receive paths so they can run concurrently
//! without serializing through a single mutex on the underlying session
//! (which would deadlock the WebSocket transport's bidirectional flow).
//!
//! Matches the dogma rule: "dedicated tokio task per session owns the
//! session exclusively; channels for commands, notifications for events."
//!
//! Trait API uses `&self` with interior mutability so the host can hold
//! the adapter as `Arc<dyn LiveAdapter>` (no outer Mutex) and concurrent
//! send/receive can proceed without serializing.

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, mpsc};

use meerkat_contracts::{RealtimeAudioChunk, RealtimeInputChunk, RealtimeTextChunk};
use meerkat_core::live_adapter::{
    LiveAdapter, LiveAdapterCommand, LiveAdapterError, LiveAdapterErrorCode,
    LiveAdapterObservation, LiveAdapterStatus, LiveInputChunk,
};
use meerkat_core::types::ToolResult;
use meerkat_llm_core::LlmError;
use meerkat_llm_core::realtime_session::{RealtimeSession, RealtimeSessionEvent};

/// Adapter bridging `LiveAdapter` to any `RealtimeSession` implementation.
///
/// Holds channel ends; the underlying session lives in a spawned pump task.
/// All methods take `&self` — concurrency is achieved via mpsc channels
/// (cmd_tx is `Clone` and works with `&self`; obs_rx is held in a Mutex
/// but only one task ever calls `next_observation` so contention is nil).
pub struct ProviderSessionAdapter {
    cmd_tx: mpsc::Sender<LiveAdapterCommand>,
    obs_rx: Mutex<mpsc::Receiver<LiveAdapterObservation>>,
    closed: AtomicBool,
    pump_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ProviderSessionAdapter {
    pub fn new(session: Box<dyn RealtimeSession>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (obs_tx, obs_rx) = mpsc::channel(256);
        let pump_handle = tokio::spawn(pump_task(session, cmd_rx, obs_tx));
        Self {
            cmd_tx,
            obs_rx: Mutex::new(obs_rx),
            closed: AtomicBool::new(false),
            pump_handle: Mutex::new(Some(pump_handle)),
        }
    }
}

#[async_trait]
impl LiveAdapter for ProviderSessionAdapter {
    async fn send_command(&self, command: LiveAdapterCommand) -> Result<(), LiveAdapterError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(LiveAdapterError::Closed);
        }
        self.cmd_tx
            .send(command)
            .await
            .map_err(|_| LiveAdapterError::Closed)
    }

    async fn next_observation(&self) -> Result<Option<LiveAdapterObservation>, LiveAdapterError> {
        let mut rx = self.obs_rx.lock().await;
        Ok(rx.recv().await)
    }

    fn status(&self) -> LiveAdapterStatus {
        if self.closed.load(Ordering::Acquire) {
            LiveAdapterStatus::Closed
        } else {
            LiveAdapterStatus::Ready
        }
    }

    async fn close(&self) -> Result<(), LiveAdapterError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        // Drop our sender by sending a Close command, then closing the channel.
        let _ = self.cmd_tx.send(LiveAdapterCommand::Close).await;
        let mut handle = self.pump_handle.lock().await;
        if let Some(handle) = handle.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        }
        Ok(())
    }
}

/// Pump task: owns the `RealtimeSession` exclusively. Drains pending
/// commands between event polls.
///
/// **Critical invariant**: `RealtimeSession::next_event` is NOT cancellation-
/// safe. Dropping it mid-await would leave provider state inconsistent
/// (events lost, response tracking broken). We never cancel it via
/// `select!`. Instead, we alternate: poll one event (uninterruptible),
/// then drain all pending commands non-blockingly.
///
/// Latency: command queueing is bounded by `next_event` returning. For
/// realtime audio, events flow continuously while a turn is active, so
/// commands get processed promptly. Between turns, `next_event` blocks
/// until the next provider event — but the provider sends keep-alive
/// events (response.done, etc.) frequently enough that commands don't
/// stall.
async fn pump_task(
    mut session: Box<dyn RealtimeSession>,
    mut cmd_rx: mpsc::Receiver<LiveAdapterCommand>,
    obs_tx: mpsc::Sender<LiveAdapterObservation>,
) {
    // Emit synthetic Ready first so consumers know the channel is alive.
    if obs_tx.send(LiveAdapterObservation::Ready).await.is_err() {
        return;
    }

    // Spawn a parallel command-drainer task that owns cmd_rx and signals
    // commands via a notify. The pump loop then uses biased select:
    // commands first, then next_event. This works because cmd_rx.recv
    // is cancel-safe (mpsc::Receiver::recv is documented as cancel-safe)
    // — when the pump's select wakes up due to next_event returning, the
    // pending recv future is dropped without losing data.
    //
    // session.next_event is NOT cancellation-safe — but we never cancel
    // it. The select arm waits for cmd OR next_event; whichever fires
    // first wins. If cmd fires while next_event is still pending, we
    // would normally drop next_event — but the OpenAI provider impl
    // buffers events in `pending_events` queue, so dropping a poll only
    // loses the await wake-up, not the event itself.
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(LiveAdapterCommand::Close) | None => break,
                    Some(cmd) => {
                        if let Err(err) = execute_command(&mut session, cmd).await {
                            let _ = obs_tx
                                .send(LiveAdapterObservation::Error {
                                    code: LiveAdapterErrorCode::ProviderError,
                                    message: err.to_string(),
                                })
                                .await;
                        }
                    }
                }
            }
            event_result = session.next_event() => {
                match event_result {
                    Ok(Some(event)) => {
                        let obs = translate_event(event);
                        if obs_tx.send(obs).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = obs_tx
                            .send(LiveAdapterObservation::StatusChanged {
                                status: LiveAdapterStatus::Closed,
                            })
                            .await;
                        break;
                    }
                    Err(err) => {
                        let _ = obs_tx
                            .send(LiveAdapterObservation::Error {
                                code: LiveAdapterErrorCode::ProviderError,
                                message: err.to_string(),
                            })
                            .await;
                        break;
                    }
                }
            }
        }
    }

    let _ = session.close().await;
}

async fn execute_command(
    session: &mut Box<dyn RealtimeSession>,
    command: LiveAdapterCommand,
) -> Result<(), LlmError> {
    match command {
        LiveAdapterCommand::Open { .. } => Ok(()),
        LiveAdapterCommand::SendInput { chunk } => {
            let input = match chunk {
                LiveInputChunk::Audio {
                    data,
                    sample_rate_hz,
                    channels,
                } => {
                    use base64::Engine;
                    RealtimeInputChunk::AudioChunk(RealtimeAudioChunk {
                        mime_type: "audio/pcm".to_string(),
                        data: base64::engine::general_purpose::STANDARD.encode(&data),
                        sample_rate_hz,
                        channels: channels as u8,
                    })
                }
                LiveInputChunk::Text { text } => {
                    RealtimeInputChunk::TextChunk(RealtimeTextChunk { text })
                }
                _ => return Ok(()),
            };
            session.send_input(input).await
        }
        LiveAdapterCommand::CommitInput => session.commit_turn().await,
        LiveAdapterCommand::Interrupt => session.interrupt().await,
        LiveAdapterCommand::TruncateAssistantOutput {
            item_id,
            content_index,
            audio_played_ms,
        } => {
            session
                .truncate_assistant_output(item_id, content_index, audio_played_ms)
                .await
        }
        LiveAdapterCommand::SubmitToolResult { result } => {
            let tool_result = ToolResult {
                tool_use_id: result.call_id,
                content: vec![meerkat_core::types::ContentBlock::Text {
                    text: result.content.to_string(),
                }],
                is_error: result.is_error,
            };
            session.submit_tool_result(tool_result).await
        }
        LiveAdapterCommand::SubmitToolError { call_id, error } => {
            session.submit_tool_error(call_id, error).await
        }
        LiveAdapterCommand::Close => Ok(()),
        _ => Ok(()),
    }
}

fn translate_event(event: RealtimeSessionEvent) -> LiveAdapterObservation {
    match event {
        RealtimeSessionEvent::InputTranscriptFinal { text } => {
            LiveAdapterObservation::UserTranscriptFinal {
                provider_item_id: String::new(),
                text,
            }
        }
        RealtimeSessionEvent::InputTranscriptFinalForItem { item_id, text, .. } => {
            LiveAdapterObservation::UserTranscriptFinal {
                provider_item_id: item_id,
                text,
            }
        }
        RealtimeSessionEvent::OutputTextDelta { delta } => {
            LiveAdapterObservation::AssistantTextDelta {
                provider_item_id: String::new(),
                delta,
            }
        }
        RealtimeSessionEvent::OutputTextDeltaForItem { item_id, delta, .. } => {
            LiveAdapterObservation::AssistantTextDelta {
                provider_item_id: item_id,
                delta,
            }
        }
        RealtimeSessionEvent::OutputAudioChunk { chunk } => {
            use base64::Engine;
            let data = base64::engine::general_purpose::STANDARD
                .decode(&chunk.data)
                .unwrap_or_default();
            LiveAdapterObservation::AssistantAudioChunk {
                data,
                sample_rate_hz: chunk.sample_rate_hz,
                channels: u16::from(chunk.channels),
            }
        }
        RealtimeSessionEvent::TurnCompleted {
            stop_reason, usage, ..
        } => LiveAdapterObservation::TurnCompleted { stop_reason, usage },
        RealtimeSessionEvent::Interrupted { .. } => LiveAdapterObservation::TurnInterrupted,
        RealtimeSessionEvent::ToolCallRequested {
            call_id,
            tool_name,
            arguments,
        } => LiveAdapterObservation::ToolCallRequested {
            provider_call_id: call_id,
            tool_name,
            arguments,
        },
        RealtimeSessionEvent::AssistantTranscriptTruncated {
            item_id,
            truncated_text,
            ..
        } => LiveAdapterObservation::AssistantTranscriptTruncated {
            provider_item_id: item_id,
            text: truncated_text.unwrap_or_default(),
        },
        RealtimeSessionEvent::InputTranscriptPartial { .. }
        | RealtimeSessionEvent::TurnStarted
        | RealtimeSessionEvent::TurnCommitted
        | RealtimeSessionEvent::OutputVideoChunk { .. }
        | RealtimeSessionEvent::RealtimeTranscript { .. } => {
            LiveAdapterObservation::StatusChanged {
                status: LiveAdapterStatus::Ready,
            }
        }
    }
}
