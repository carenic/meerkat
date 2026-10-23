//! Transcript-history retention inside one long run, through the real agent
//! loop.
//!
//! A session that a durable HeadCanonical head describes keeps every rewrite
//! occurrence that head does not cover yet: the next checkpoint proves the
//! pending rewrite suffix from that head, which needs their bodies. A run
//! that compacts many times before its run-end checkpoint must still keep its
//! in-memory history bounded by the retention, not by how often it compacted.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use crate as meerkat_core;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use meerkat_core::compact::{
    COMPACTION_SUMMARY_PREFIX, CompactionContext, CompactionDiscard, CompactionResult,
    CompactionRetained, CompactionSummary, Compactor, SummarizationSource,
    SummarizationSourceBudget,
};
use meerkat_core::types::UserMessage;
use meerkat_core::{
    AgentBuilder, AgentError, AgentLlmClient, AgentSessionStore, AgentToolDispatcher,
    AssistantBlock, LlmStreamResult, Message, RunCheckpointReceipt, RunId, Session,
    SessionCheckpointer, StopReason, ToolCallView, ToolDef, ToolResult, TranscriptHistoryRetention,
    TurnUsage, Usage,
};
use serde_json::value::RawValue;

const RETAINED_REWRITES: usize = 2;
const TOOL_TURNS: usize = 7;
const COMPACTION_PROMPT: &str = "COMPACT NOW";

fn mock_usage() -> Usage {
    TurnUsage::host_declared(
        meerkat_core::Provider::Other,
        "mock-model",
        Usage::default(),
    )
    .into_inner()
}

/// Calls one tool on each of its first `TOOL_TURNS` ordinary requests, then
/// answers. A compaction summary request gets a short summary.
struct ToolLoopClient {
    ordinary_calls: AtomicUsize,
}

#[async_trait]
impl AgentLlmClient for ToolLoopClient {
    async fn stream_response(
        &self,
        messages: &[Message],
        _tools: &[Arc<ToolDef>],
        _max_tokens: u32,
        _temperature: Option<f32>,
        _provider_params: Option<&meerkat_core::lifecycle::run_primitive::ProviderParamsOverride>,
    ) -> Result<LlmStreamResult, AgentError> {
        let summary_request = messages
            .iter()
            .rev()
            .find_map(|message| match message {
                Message::User(user) => Some(user.text_content()),
                _ => None,
            })
            .is_some_and(|text| text == COMPACTION_PROMPT);
        if summary_request {
            return Ok(LlmStreamResult::new(
                vec![AssistantBlock::Text {
                    text: "summary".to_string(),
                    meta: None,
                }],
                StopReason::EndTurn,
                mock_usage(),
            ));
        }
        let call = self.ordinary_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call <= TOOL_TURNS {
            return Ok(LlmStreamResult::new(
                vec![AssistantBlock::ToolUse {
                    id: format!("call-{call}"),
                    name: "probe".into(),
                    args: RawValue::from_string("{}".to_string()).unwrap(),
                    meta: None,
                }],
                StopReason::ToolUse,
                mock_usage(),
            ));
        }
        Ok(LlmStreamResult::new(
            vec![AssistantBlock::Text {
                text: "done".to_string(),
                meta: None,
            }],
            StopReason::EndTurn,
            mock_usage(),
        ))
    }

    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::Other
    }

    fn model(&self) -> &'static str {
        "mock-model"
    }
}

struct ProbeTool {
    tools: Arc<[Arc<ToolDef>]>,
}

#[async_trait]
impl AgentToolDispatcher for ProbeTool {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        Arc::clone(&self.tools)
    }

    async fn dispatch(
        &self,
        call: ToolCallView<'_>,
    ) -> Result<meerkat_core::ToolDispatchOutcome, meerkat_core::ToolError> {
        Ok(ToolResult::new(call.id.to_string(), "probe ok".to_string(), false).into())
    }
}

/// Compacts at every model boundary after the first: the System rows stay,
/// everything else becomes one summary row.
struct EveryBoundaryCompactor;

impl Compactor for EveryBoundaryCompactor {
    fn should_compact(&self, ctx: &CompactionContext) -> bool {
        ctx.session_boundary_index >= 1
            && ctx.last_compaction_boundary_index != Some(ctx.session_boundary_index)
    }

    fn transcript_history_retention(&self) -> TranscriptHistoryRetention {
        TranscriptHistoryRetention::from_count(RETAINED_REWRITES).expect("non-zero retention")
    }

    fn compaction_prompt(&self) -> &str {
        COMPACTION_PROMPT
    }

    fn max_summary_tokens(&self) -> u32 {
        32
    }

    fn prepare_for_summarization(
        &self,
        messages: &[Message],
        _budget: Option<SummarizationSourceBudget>,
    ) -> SummarizationSource {
        SummarizationSource::whole(
            meerkat_core::types::materialize_latest_system_prompt_versions(messages),
        )
    }

    fn rebuild_history(&self, messages: &[Message], summary: &str) -> CompactionResult {
        let mut rebuilt = Vec::new();
        let mut retained = Vec::new();
        let mut discarded = Vec::new();
        for (offset, message) in messages.iter().enumerate() {
            let source_offset = u64::try_from(offset).unwrap_or(u64::MAX);
            if matches!(message, Message::System(_)) {
                retained.push(CompactionRetained::new(
                    source_offset,
                    u64::try_from(rebuilt.len()).unwrap_or(u64::MAX),
                    message.clone(),
                ));
                rebuilt.push(message.clone());
            } else {
                discarded.push(CompactionDiscard::new(source_offset, message.clone()));
            }
        }
        let summary_offset = u64::try_from(rebuilt.len()).unwrap_or(u64::MAX);
        let summary_message = Message::User(UserMessage::compaction_summary(format!(
            "{COMPACTION_SUMMARY_PREFIX}{summary}"
        )));
        rebuilt.push(summary_message.clone());
        CompactionResult {
            messages: rebuilt,
            summary: CompactionSummary::new(summary_offset, summary_message),
            retained,
            discarded,
        }
    }
}

/// What a HeadCanonical store checkpointer does with the live session, without
/// the store: it proves the pending rewrite suffix from the durable head (so
/// it refuses when the live graph retired an occurrence that head does not
/// cover), then acknowledges the successor, whose head covers every rewrite.
#[derive(Default)]
struct HeadCanonicalCheckpointer {
    /// `(commit_count, retired_count)` of the live graph at each checkpoint.
    checkpoints: Mutex<Vec<(usize, usize)>>,
    /// The checkpointed session's tail at each checkpoint.
    tails: Mutex<Vec<CheckpointTail>>,
}

/// What a checkpoint saw at the end of the session it persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckpointTail {
    /// The run's own tool call, then its results: a classifiable live tail.
    RunToolUseThenResults,
    /// The run's own final answer.
    RunAnswer,
    /// A compaction summary: nothing the run wrote after its last rewrite.
    CompactionSummary,
    /// Any other shape.
    Other,
}

fn checkpoint_tail(session: &Session, run_id: &RunId) -> CheckpointTail {
    let stamped = |message: &Message| match message {
        Message::BlockAssistant(assistant) => assistant.identity.run_id.as_ref() == Some(run_id),
        _ => false,
    };
    match session.messages() {
        [.., Message::User(user)] if user.transcript_role.is_compaction_summary() => {
            CheckpointTail::CompactionSummary
        }
        [.., call, Message::ToolResults { .. }] if stamped(call) => {
            CheckpointTail::RunToolUseThenResults
        }
        [.., answer] if stamped(answer) => CheckpointTail::RunAnswer,
        _ => CheckpointTail::Other,
    }
}

#[async_trait]
impl SessionCheckpointer for HeadCanonicalCheckpointer {
    async fn checkpoint_run(
        &self,
        session: &mut Session,
        run_id: &RunId,
        _previous: Option<&RunCheckpointReceipt>,
    ) -> Result<Option<RunCheckpointReceipt>, AgentError> {
        self.tails
            .lock()
            .unwrap()
            .push(checkpoint_tail(session, run_id));
        let Some(graph) = session
            .transcript_history_state()
            .map_err(|error| AgentError::InternalError(error.to_string()))?
        else {
            return Ok(None);
        };
        if let Some(durable) = session.durable_transcript_rewrite_count_for_test() {
            graph.refuse_if_retired_count(durable).map_err(|error| {
                AgentError::InternalError(format!(
                    "failed to prove pending rewrite suffix: {error}"
                ))
            })?;
        }
        self.checkpoints
            .lock()
            .unwrap()
            .push((graph.commit_count(), graph.retired_count()));
        session.note_durable_transcript_rewrites(graph.commit_count() as u64);
        Ok(None)
    }
}

struct NoopStore;

#[async_trait]
impl AgentSessionStore for NoopStore {
    async fn save(&self, _session: &Session) -> Result<(), AgentError> {
        Ok(())
    }

    async fn load(&self, _id: &str) -> Result<Option<Session>, AgentError> {
        Ok(None)
    }
}

/// One long run of `TOOL_TURNS` tool turns that compacts at every model
/// boundary after the first. `durable_floor` is the rewrite count of the
/// durable HeadCanonical head the session was materialized from (`None` for a
/// session no such head describes, as with WholeBlob); `checkpointer` is the
/// run's checkpointer, if any.
async fn run_long_compacting_run(
    durable_floor: Option<u64>,
    checkpointer: Option<Arc<HeadCanonicalCheckpointer>>,
) -> Session {
    let mut session = Session::new();
    session
        .set_build_state(meerkat_core::SessionBuildState::default())
        .expect("test session build state should serialize");
    if let Some(floor) = durable_floor {
        session.note_durable_transcript_rewrites(floor);
    }
    let mut builder = AgentBuilder::new()
        .resume_session(session)
        .with_turn_state_handle(Arc::new(
            crate::agent::test_turn_state_handle::TestTurnStateHandle::new(),
        ))
        .with_runtime_execution_kind_for_test(
            meerkat_core::lifecycle::RuntimeExecutionKind::ContentTurn,
        )
        .compactor(Arc::new(EveryBoundaryCompactor));
    if let Some(checkpointer) = checkpointer {
        builder = builder.with_checkpointer(checkpointer);
    }
    let mut agent = builder
        .build_standalone(
            Arc::new(ToolLoopClient {
                ordinary_calls: AtomicUsize::new(0),
            }),
            Arc::new(ProbeTool {
                tools: Arc::from([Arc::new(ToolDef::new(
                    "probe",
                    "returns",
                    serde_json::json!({ "type": "object" }),
                ))]),
            }),
            Arc::new(NoopStore),
        )
        .await;

    agent.run("start".into()).await.expect("the run completes");
    agent.session().clone()
}

/// One run compacts at each of its `TOOL_TURNS` tool turns. Its session was
/// materialized from a durable HeadCanonical head that covers no rewrite, and
/// the run checkpoints only at its end: before the fix, retirement waited for
/// that checkpoint, so the run held every compaction's body until it ended.
#[tokio::test]
async fn a_long_run_keeps_its_history_within_the_retention_while_it_compacts() {
    let checkpointer = Arc::new(HeadCanonicalCheckpointer::default());
    let session = run_long_compacting_run(Some(0), Some(checkpointer.clone())).await;

    let graph = session
        .transcript_history_state()
        .expect("graph decodes")
        .expect("graph present");
    assert_eq!(
        graph.commit_count(),
        TOOL_TURNS,
        "one compaction rewrite per tool turn"
    );
    let retained = graph.commit_count() - graph.retired_count();
    assert!(
        retained <= RETAINED_REWRITES,
        "after {} compactions in one run the live graph keeps {retained} rewrite bodies; \
         the retention is {RETAINED_REWRITES} (checkpoints: {:?})",
        graph.commit_count(),
        checkpointer.checkpoints.lock().unwrap()
    );
    let checkpoints = checkpointer.checkpoints.lock().unwrap().clone();
    assert!(
        checkpoints
            .iter()
            .all(|(commits, retired)| commits - retired <= RETAINED_REWRITES + 1),
        "no checkpoint saw more than one rewrite body beyond the retention: {checkpoints:?}"
    );
}

/// The checkpoints a long compacting run takes before its end come after the
/// run's own tool results, never right after a compaction: a durable tail
/// always holds a row the run wrote after its last rewrite, so recovery can
/// attribute it to the run instead of holding it.
#[tokio::test]
async fn a_long_run_checkpoints_only_after_its_own_tool_results() {
    let checkpointer = Arc::new(HeadCanonicalCheckpointer::default());
    run_long_compacting_run(Some(0), Some(checkpointer.clone())).await;

    let tails = checkpointer.tails.lock().unwrap().clone();
    let (last, mid_run) = tails.split_last().expect("the run checkpoints at its end");
    assert!(
        !mid_run.is_empty(),
        "the run must checkpoint before its end to bound its history: {tails:?}"
    );
    assert!(
        mid_run
            .iter()
            .all(|tail| *tail == CheckpointTail::RunToolUseThenResults),
        "every mid-run checkpoint follows the run's own tool call and results: {tails:?}"
    );
    assert_eq!(*last, CheckpointTail::RunAnswer, "{tails:?}");
}

/// A session no durable HeadCanonical head describes (WholeBlob) retires
/// freely, so the run never takes an extra checkpoint: the only checkpoint is
/// the one at its end.
#[tokio::test]
async fn a_long_run_without_a_durable_head_checkpoints_only_at_its_end() {
    let checkpointer = Arc::new(HeadCanonicalCheckpointer::default());
    let session = run_long_compacting_run(None, Some(checkpointer.clone())).await;

    let tails = checkpointer.tails.lock().unwrap().clone();
    assert_eq!(tails, vec![CheckpointTail::RunAnswer]);
    let graph = session
        .transcript_history_state()
        .expect("graph decodes")
        .expect("graph present");
    assert_eq!(graph.commit_count(), TOOL_TURNS);
    assert!(graph.commit_count() - graph.retired_count() <= RETAINED_REWRITES);
}

/// Without a checkpointer a run cannot move its durable head, so it takes no
/// checkpoint and keeps what that head does not cover, as before.
#[tokio::test]
async fn a_long_run_without_a_checkpointer_keeps_what_its_durable_head_lacks() {
    let session = run_long_compacting_run(Some(0), None).await;

    let graph = session
        .transcript_history_state()
        .expect("graph decodes")
        .expect("graph present");
    assert_eq!(graph.commit_count(), TOOL_TURNS);
    assert_eq!(
        graph.retired_count(),
        0,
        "nothing the durable head lacks is retired without a checkpoint"
    );
}
