//! OpenAI live adapter — bridges `LiveAdapter` to `RealtimeSession`.
//!
//! Translates between the Meerkat-owned `LiveAdapter` seam vocabulary and
//! the provider-specific `RealtimeSession`/`RealtimeSessionFactory` layer.
//! All OpenAI transport mechanics stay inside `OpenAiRealtimeSession`;
//! this module only handles the translation.

use async_trait::async_trait;

use meerkat_core::live_adapter::{
    LiveAdapter, LiveAdapterCommand, LiveAdapterError, LiveAdapterErrorCode,
    LiveAdapterObservation, LiveAdapterStatus, LiveInputChunk,
};
use meerkat_core::types::ToolResult;
use meerkat_llm_core::LlmError;
use meerkat_llm_core::realtime_session::{RealtimeSession, RealtimeSessionEvent};

use meerkat_contracts::{RealtimeAudioChunk, RealtimeInputChunk, RealtimeTextChunk};

/// OpenAI implementation of the `LiveAdapter` seam.
///
/// Wraps an `OpenAiRealtimeSession` (or any `RealtimeSession`) and
/// translates commands/observations through the typed boundary.
/// Provider-specific mechanics (session updates, response nudging,
/// truncation cursors) stay inside the underlying session.
pub struct OpenAiLiveAdapter {
    session: Option<Box<dyn RealtimeSession>>,
    status: LiveAdapterStatus,
}

impl OpenAiLiveAdapter {
    pub fn new(session: Box<dyn RealtimeSession>) -> Self {
        Self {
            session: Some(session),
            status: LiveAdapterStatus::Ready,
        }
    }

    fn session_mut(&mut self) -> Result<&mut Box<dyn RealtimeSession>, LiveAdapterError> {
        self.session.as_mut().ok_or(LiveAdapterError::Closed)
    }

    fn map_llm_error(err: LlmError) -> LiveAdapterError {
        LiveAdapterError::ProviderError {
            code: LiveAdapterErrorCode::ProviderError,
            message: err.to_string(),
        }
    }
}

#[async_trait]
impl LiveAdapter for OpenAiLiveAdapter {
    async fn send_command(&mut self, command: LiveAdapterCommand) -> Result<(), LiveAdapterError> {
        if !self.status.accepts_commands() {
            return Err(LiveAdapterError::NotReady {
                status: self.status.clone(),
            });
        }
        match command {
            LiveAdapterCommand::Open { snapshot: _ } => {
                // Open is handled at construction time — the session is
                // already connected. Future: support rebuild-from-snapshot
                // by closing the current session and opening a new one.
                Ok(())
            }
            LiveAdapterCommand::SendInput { chunk } => {
                let session = self.session_mut()?;
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
                session.send_input(input).await.map_err(Self::map_llm_error)
            }
            LiveAdapterCommand::CommitInput => {
                let session = self.session_mut()?;
                session.commit_turn().await.map_err(Self::map_llm_error)
            }
            LiveAdapterCommand::Interrupt => {
                let session = self.session_mut()?;
                session.interrupt().await.map_err(Self::map_llm_error)
            }
            LiveAdapterCommand::TruncateAssistantOutput {
                item_id,
                content_index,
                audio_played_ms,
            } => {
                let session = self.session_mut()?;
                session
                    .truncate_assistant_output(item_id, content_index, audio_played_ms)
                    .await
                    .map_err(Self::map_llm_error)
            }
            LiveAdapterCommand::SubmitToolResult { result } => {
                let session = self.session_mut()?;
                let tool_result = ToolResult {
                    tool_use_id: result.call_id,
                    content: vec![meerkat_core::types::ContentBlock::Text {
                        text: result.content.to_string(),
                    }],
                    is_error: result.is_error,
                };
                session
                    .submit_tool_result(tool_result)
                    .await
                    .map_err(Self::map_llm_error)
            }
            LiveAdapterCommand::SubmitToolError { call_id, error } => {
                let session = self.session_mut()?;
                session
                    .submit_tool_error(call_id, error)
                    .await
                    .map_err(Self::map_llm_error)
            }
            LiveAdapterCommand::Close => {
                if let Some(mut session) = self.session.take() {
                    let _ = session.close().await;
                }
                self.status = LiveAdapterStatus::Closed;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn next_observation(
        &mut self,
    ) -> Result<Option<LiveAdapterObservation>, LiveAdapterError> {
        let session = match self.session.as_mut() {
            Some(s) => s,
            None => return Ok(None),
        };
        let event = session.next_event().await.map_err(Self::map_llm_error)?;
        let Some(event) = event else {
            self.status = LiveAdapterStatus::Closed;
            return Ok(Some(LiveAdapterObservation::StatusChanged {
                status: LiveAdapterStatus::Closed,
            }));
        };
        Ok(Some(translate_event(event)))
    }

    fn status(&self) -> &LiveAdapterStatus {
        &self.status
    }

    async fn close(&mut self) -> Result<(), LiveAdapterError> {
        if let Some(mut session) = self.session.take() {
            let _ = session.close().await;
        }
        self.status = LiveAdapterStatus::Closed;
        Ok(())
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_contracts::RealtimeAudioChunk;
    use meerkat_core::types::{StopReason, Usage};

    #[test]
    fn translate_tool_call_preserves_provider_call_id() {
        let event = RealtimeSessionEvent::ToolCallRequested {
            call_id: "call_abc".into(),
            tool_name: "calculator".into(),
            arguments: serde_json::json!({"x": 42}),
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::ToolCallRequested {
                provider_call_id,
                tool_name,
                arguments,
            } => {
                assert_eq!(provider_call_id, "call_abc");
                assert_eq!(tool_name, "calculator");
                assert_eq!(arguments, serde_json::json!({"x": 42}));
            }
            other => panic!("expected ToolCallRequested, got {other:?}"),
        }
    }

    #[test]
    fn translate_interrupted_maps_to_turn_interrupted() {
        let event = RealtimeSessionEvent::Interrupted {
            response_id: Some("resp_1".into()),
        };
        let obs = translate_event(event);
        assert_eq!(obs, LiveAdapterObservation::TurnInterrupted);
    }

    #[test]
    fn translate_turn_completed_carries_usage() {
        let event = RealtimeSessionEvent::TurnCompleted {
            response_id: "resp_2".into(),
            stop_reason: StopReason::EndTurn,
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_tokens: None,
                cache_read_tokens: None,
            },
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::TurnCompleted { stop_reason, usage } => {
                assert_eq!(stop_reason, StopReason::EndTurn);
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(usage.output_tokens, 50);
            }
            other => panic!("expected TurnCompleted, got {other:?}"),
        }
    }

    #[test]
    fn translate_audio_chunk_decodes_base64_and_preserves_format() {
        use base64::Engine;
        let raw_pcm = vec![0u8; 480];
        let encoded = base64::engine::general_purpose::STANDARD.encode(&raw_pcm);
        let event = RealtimeSessionEvent::OutputAudioChunk {
            chunk: RealtimeAudioChunk {
                mime_type: "audio/pcm".into(),
                data: encoded,
                sample_rate_hz: 24000,
                channels: 1,
            },
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::AssistantAudioChunk {
                data,
                sample_rate_hz,
                channels,
            } => {
                assert_eq!(data.len(), 480);
                assert_eq!(sample_rate_hz, 24000);
                assert_eq!(channels, 1);
            }
            other => panic!("expected AssistantAudioChunk, got {other:?}"),
        }
    }

    #[test]
    fn translate_text_delta_with_item_id() {
        let event = RealtimeSessionEvent::OutputTextDeltaForItem {
            response_id: "resp_3".into(),
            delta_id: "delta_1".into(),
            item_id: "item_5".into(),
            previous_item_id: None,
            content_index: 0,
            delta: "hello".into(),
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::AssistantTextDelta {
                provider_item_id,
                delta,
            } => {
                assert_eq!(provider_item_id, "item_5");
                assert_eq!(delta, "hello");
            }
            other => panic!("expected AssistantTextDelta, got {other:?}"),
        }
    }

    #[test]
    fn translate_user_transcript_final_for_item() {
        let event = RealtimeSessionEvent::InputTranscriptFinalForItem {
            item_id: "item_user_1".into(),
            previous_item_id: None,
            content_index: 0,
            text: "how are you".into(),
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::UserTranscriptFinal {
                provider_item_id,
                text,
            } => {
                assert_eq!(provider_item_id, "item_user_1");
                assert_eq!(text, "how are you");
            }
            other => panic!("expected UserTranscriptFinal, got {other:?}"),
        }
    }

    #[test]
    fn translate_truncation_uses_best_effort_text() {
        let event = RealtimeSessionEvent::AssistantTranscriptTruncated {
            response_id: Some("resp_4".into()),
            item_id: "item_trunc".into(),
            audio_played_ms: 2500,
            truncated_text: Some("I was saying".into()),
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::AssistantTranscriptTruncated {
                provider_item_id,
                text,
            } => {
                assert_eq!(provider_item_id, "item_trunc");
                assert_eq!(text, "I was saying");
            }
            other => panic!("expected AssistantTranscriptTruncated, got {other:?}"),
        }
    }

    #[test]
    fn translate_truncation_without_text_defaults_empty() {
        let event = RealtimeSessionEvent::AssistantTranscriptTruncated {
            response_id: None,
            item_id: "item_no_text".into(),
            audio_played_ms: 1000,
            truncated_text: None,
        };
        let obs = translate_event(event);
        match obs {
            LiveAdapterObservation::AssistantTranscriptTruncated { text, .. } => {
                assert!(text.is_empty());
            }
            other => panic!("expected AssistantTranscriptTruncated, got {other:?}"),
        }
    }
}
