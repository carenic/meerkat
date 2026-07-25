#![cfg(all(feature = "integration-real-tests", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//!
//! Idle-CPU regression gate for the mob runtime (turbo-s smoke lane).
//!
//! Three releases in a row shipped fixed-cadence idle loops that did
//! O(fleet-state) or O(session-document) work per tick (25ms identity
//! reconcile with full-document sha256 verification, ~1s scan verify, 250ms
//! monitor loops deep-cloning `MobMachineState`). Every suite passed because
//! fixtures were KB-scale; an 82MB production session dump was the only thing
//! that caught them. This gate boots a persistent 3-member mob with one
//! production-scale (~12MB) session document, waits for convergence, idles,
//! and asserts the PROCESS CPU-TIME delta over the idle window stays far
//! below the historical burn. It measures the driver (any fixed-cadence idle
//! work), so it asserts CPU time via `cpu_time::ProcessTime`, never wall
//! clock or %CPU sampling. This test must stay ALONE in its test binary so no
//! sibling test's CPU pollutes the measurement.
//!
//! The marker-less arm additionally asserts RESUME COST as a ratio against a
//! marked-baseline cold restart: idle CPU alone cannot see the decode-memo
//! regression (a mob with no repeat-load driver quiesces either way), but a
//! resume that re-pays decode-time graph verification per load runs ~3x
//! slower. Red-first verification: run with
//! `MEERKAT_DISABLE_GRAPH_DECODE_MEMO=1` and the ratio assertion must fail.
//!
//! No live provider is involved: members run against a scripted LLM client,
//! so the lane needs no API keys and the measured window is deterministic.
//!
//! Run with:
//!   cargo test -p meerkat-mob --test smoke_mob_idle_burn \
//!     --features integration-real-tests -- --ignored --nocapture

use meerkat::{AgentFactory, Config, FactoryAgentBuilder};
use meerkat_core::types::HandlingMode;
use meerkat_mob::definition::{OrchestratorConfig, WiringRules};
use meerkat_mob::{
    AgentIdentity, MobBuilder, MobDefinition, MobHandle, MobId, MobMemberStatus, MobRuntimeMode,
    MobStorage, Profile, ProfileBinding, ProfileName, SpawnMemberSpec, ToolConfig,
};
use meerkat_session::PersistentSessionService;
use meerkat_store::{JsonlStore, MemoryBlobStore, StoreAdapter};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::time::{Duration, Instant, sleep};

/// 10% of one core over the idle window. The historical defects consumed
/// ~30% of a core (and scaled with member count / document size), i.e.
/// ~9+ CPU-seconds over this window; a hot-loop recurrence trips this
/// immediately while CI noise stays well under it.
const IDLE_WINDOW: Duration = Duration::from_secs(30);
const MAX_IDLE_CPU: Duration = Duration::from_secs(3);

/// The large member's transcript is grown from turns carrying inputs of this
/// size, giving a persisted session document of at least
/// `LARGE_SESSION_TURNS * LARGE_TURN_INPUT_BYTES` ≈ 12 MB — the
/// production-dump scale class where size-proportional idle reads become
/// visible (synthetic; nothing committed).
const LARGE_TURN_INPUT_BYTES: usize = 3_000_000;
const LARGE_SESSION_TURNS: usize = 4;
const MIN_PERSISTED_BYTES: u64 = 10 * 1024 * 1024;

/// Marker-less resume cost is asserted as a RATIO against a marked-baseline
/// cold restart of the same fleet, so the bound is machine-speed-invariant.
/// The baseline documents carry no transcript history state on disk; the
/// resume-time system-prompt-refresh rewrite creates history mid-resume, so
/// the baseline is only PARTIALLY exposed to decode-memo loss (late-window
/// validation repeats), while the marker-less arm pays the heal probe plus
/// full graph validation from its first decode. Measured on the reference
/// box: memo on 41.3s / 26.9s (ratio 1.54 — the structural gap: a roughly
/// double-size document plus one memoized verification pass per document);
/// memo off (`MEERKAT_DISABLE_GRAPH_DECODE_MEMO=1`) 123.5s / 54.2s (ratio
/// 2.28). K = 1.75 sits between with ~25% margin each way. The absolute
/// floor keeps sub-second baseline resumes on fast machines from turning
/// timing noise into a failure.
const MARKERLESS_RESUME_RATIO: f64 = 1.75;
const MARKERLESS_RESUME_FLOOR: Duration = Duration::from_secs(5);

const MEMBER_IDS: [&str; 3] = ["lead-1", "w-1", "w-2"];
const LARGE_MEMBER_ID: &str = "w-1";

/// Answers "ok" to every turn and counts requests, so member turns complete
/// deterministically without a live provider and the test can observe turn
/// completion.
#[derive(Clone, Default)]
struct CaptureClient {
    requests: Arc<AtomicUsize>,
}

impl CaptureClient {
    fn count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl meerkat_client::LlmClient for CaptureClient {
    fn project_replay_messages(
        &self,
        messages: &[meerkat_core::Message],
    ) -> Result<Vec<meerkat_core::Message>, meerkat_client::LlmError> {
        Ok(messages.to_vec())
    }

    fn stream<'a>(
        &'a self,
        _request: &'a meerkat_client::LlmRequest,
    ) -> meerkat_client::types::LlmStream<'a> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let events = vec![
            meerkat_client::LlmEvent::TextDelta {
                delta: "ok".to_string(),
                meta: None,
            },
            meerkat_client::LlmEvent::Done {
                outcome: meerkat_client::LlmDoneOutcome::Success {
                    stop_reason: meerkat_core::StopReason::EndTurn,
                },
            },
        ];
        Box::pin(futures::stream::iter(events.into_iter().map(Ok)))
    }

    fn provider(&self) -> meerkat_core::Provider {
        meerkat_core::Provider::Other
    }

    async fn health_check(&self) -> Result<(), meerkat_client::LlmError> {
        Ok(())
    }
}

/// Total (user + system) CPU time this process has consumed since start.
fn process_cpu_time() -> Duration {
    cpu_time::ProcessTime::try_now()
        .expect("read process CPU time")
        .as_duration()
}

/// Probe the process CPU rate until a 2s probe reads idle-level. Trailing
/// durable commits are legitimate work; a mob that NEVER quiesces fails here
/// — which is exactly the defect class this gate exists to catch.
async fn wait_for_quiesce(what: &str) {
    let quiesce_deadline = Instant::now() + Duration::from_secs(240);
    loop {
        let probe_start = process_cpu_time();
        sleep(Duration::from_secs(2)).await;
        let probe_burn = process_cpu_time().saturating_sub(probe_start);
        if probe_burn < Duration::from_millis(200) {
            break;
        }
        assert!(
            Instant::now() < quiesce_deadline,
            "{what}: still burning {probe_burn:?} per 2s probe (an idle-CPU hot loop)"
        );
    }
}

/// Rewrite one persisted session document into the pre-marker fleet shape
/// (the HomeCore cutover state: written by 0.8.4-class code, `digest_format`
/// absent, current-format digests):
///   1. commit a REAL audited transcript rewrite so the document retains
///      transcript-history revision bodies like a production
///      compacted/rewritten session — the decode-time heal probe and graph
///      validation only exist for documents that carry this state;
///   2. re-stamp the checkpoint over the rewritten document (out-of-band
///      successor, same pattern as the cold-restart harness);
///   3. strip the `digest_format` marker from the serialized bytes exactly
///      as a pre-marker writer would have persisted them. The checkpoint
///      digest is marker-invariant, so the stamp keeps verifying before and
///      after the strip.
fn markerless_history_document(bytes: &[u8], member_id: &str) -> Vec<u8> {
    use meerkat_core::session::SESSION_TRANSCRIPT_HISTORY_STATE_KEY;
    use meerkat_core::{
        SessionCheckpointProvenance, SessionCheckpointStamp, SessionCheckpointState,
        TranscriptRewriteReason, TranscriptRewriteSelection,
    };

    let mut session: meerkat_core::Session =
        serde_json::from_slice(bytes).expect("decode persisted session document");
    let predecessor = match session
        .try_checkpoint_state()
        .expect("gate fixture checkpoint must verify")
    {
        SessionCheckpointState::Verified(stamp) => stamp,
        SessionCheckpointState::LegacyUnverified { .. } => {
            panic!("gate fixture documents are written stamped by this build")
        }
    };
    let message_count = session.messages().len();
    assert!(
        message_count >= 2,
        "seeded member {member_id} must have a transcript to rewrite"
    );
    session
        .commit_transcript_rewrite(
            TranscriptRewriteSelection::MessageRange {
                start: message_count - 1,
                end: message_count,
            },
            vec![meerkat_core::Message::User(
                meerkat_core::types::UserMessage::text(format!(
                    "marker-less fixture rewrite for {member_id}"
                )),
            )],
            TranscriptRewriteReason::new("idle-burn marker-less fixture"),
            None,
            None,
        )
        .expect("commit fixture transcript rewrite");
    let stamp = SessionCheckpointStamp::successor(
        &session,
        &predecessor,
        SessionCheckpointProvenance::TranscriptRewrite,
    )
    .expect("mint successor checkpoint over the rewritten document");
    session
        .install_checkpoint_stamp(stamp)
        .expect("install successor checkpoint");

    let mut document = serde_json::to_value(&session).expect("serialize rewritten session");
    let state = document["metadata"][SESSION_TRANSCRIPT_HISTORY_STATE_KEY]
        .as_object_mut()
        .expect("rewritten session must retain transcript history state");
    assert!(
        state.remove("digest_format").is_some(),
        "this build must have stamped the digest-format marker"
    );
    serde_json::to_vec(&document).expect("serialize marker-less session document")
}

fn idle_profile(peer_description: &str) -> Profile {
    Profile {
        model: "gpt-5.5".to_string(),
        provider: None,
        self_hosted_server_id: None,
        image_generation_provider: None,
        auto_compact_threshold: None,
        resume_overrides: Vec::new(),
        skills: vec![],
        tools: ToolConfig {
            comms: true,
            ..Default::default()
        },
        peer_description: peer_description.to_string(),
        external_addressable: true,
        backend: None,
        runtime_mode: MobRuntimeMode::TurnDriven,
        max_inline_peer_notifications: None,
        output_schema: None,
        provider_params: None,
    }
}

fn idle_mob_definition() -> MobDefinition {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        ProfileName::from("lead"),
        ProfileBinding::Inline(Box::new(idle_profile("Leads the idle-burn gate mob"))),
    );
    profiles.insert(
        ProfileName::from("worker"),
        ProfileBinding::Inline(Box::new(idle_profile("Idle-burn gate worker"))),
    );

    let mut definition = MobDefinition::explicit(MobId::from("idle-burn-gate"));
    definition.orchestrator = Some(OrchestratorConfig {
        profile: ProfileName::from("lead"),
    });
    definition.profiles = profiles;
    definition.wiring = WiringRules {
        auto_wire_orchestrator: true,
        role_wiring: vec![],
    };
    definition
}

/// Recursive on-disk size of the persisted session root. The historical
/// regressions were store-read/digest loops, so the gate verifies the large
/// document is actually durable (not just live in memory) before idling.
fn dir_size_bytes(root: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => dir_size_bytes(&path),
                Ok(meta) => meta.len(),
                Err(_) => 0,
            }
        })
        .sum()
}

async fn wait_for_requests(capture: &CaptureClient, at_least: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while capture.count() < at_least {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {} of {at_least} LLM requests observed",
            capture.count()
        );
        sleep(Duration::from_millis(100)).await;
    }
}

async fn active_member_count(handle: &MobHandle) -> usize {
    handle
        .list_members()
        .await
        .into_iter()
        .filter(|entry| entry.status == MobMemberStatus::Active)
        .count()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "lane:e2e-smoke"]
async fn e2e_smoke_mob_idle_burn_gate() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path();
    let user_config_root = root.join("user-config");
    let runtime_root = root.join("runtime-root");
    let project_root = root.join("project-root");
    let context_root = root.join("context-root");
    let sessions_root = root.join("sessions-jsonl");
    let mob_db_path = root.join("mob.db");
    for dir in [&project_root, &context_root] {
        fs::create_dir_all(dir).expect("create idle-burn project/context root");
    }

    let capture = CaptureClient::default();

    let factory = AgentFactory::new(runtime_root.join("factory-store"))
        .user_config_root(user_config_root)
        .runtime_root(runtime_root)
        .project_root(project_root)
        .context_root(context_root)
        .builtins(true)
        .comms(true);
    let mut builder = FactoryAgentBuilder::new(factory, Config::default());
    let store = Arc::new(JsonlStore::new(sessions_root.clone()));
    builder.default_session_store = Some(Arc::new(StoreAdapter::new(store.clone())));

    let store_dyn: Arc<dyn meerkat::SessionStore> = store;
    let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> =
        Arc::new(meerkat_runtime::InMemoryRuntimeStore::new());
    let blob_store: Arc<dyn meerkat_core::BlobStore> = Arc::new(MemoryBlobStore::default());
    let service = Arc::new(PersistentSessionService::new(
        builder,
        32,
        store_dyn,
        runtime_store.clone(),
        blob_store,
    ));

    let boot_start = Instant::now();
    let storage = MobStorage::persistent(&mob_db_path).expect("create persistent mob storage");
    let handle = MobBuilder::new(idle_mob_definition(), storage)
        .with_session_service(service.clone())
        .with_default_llm_client(Arc::new(capture.clone()))
        .create()
        .await
        .expect("create persistent idle-burn mob");

    handle
        .spawn_spec(SpawnMemberSpec::new("lead", AgentIdentity::from("lead-1")))
        .await
        .expect("spawn lead");
    handle
        .spawn_spec(SpawnMemberSpec::new("worker", AgentIdentity::from("w-1")))
        .await
        .expect("spawn worker 1");
    handle
        .spawn_spec(SpawnMemberSpec::new("worker", AgentIdentity::from("w-2")))
        .await
        .expect("spawn worker 2");

    let deadline = Instant::now() + Duration::from_secs(60);
    while active_member_count(&handle).await < MEMBER_IDS.len() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {} active members; roster: {:?}",
            MEMBER_IDS.len(),
            handle.list_members().await
        );
        sleep(Duration::from_millis(100)).await;
    }
    let boot_to_ready = boot_start.elapsed();
    eprintln!(
        "[idle-burn gate] boot-to-ready (build start → {MEMBER_IDS:?} active): {boot_to_ready:?}"
    );

    // Give every member a small persisted transcript so all three sessions
    // participate in whatever the idle path scans.
    for member_id in MEMBER_IDS {
        handle
            .member(&AgentIdentity::from(member_id))
            .await
            .expect("member handle")
            .send(
                format!("fixture transcript for {member_id}"),
                HandlingMode::Queue,
            )
            .await
            .expect("seed turn");
    }
    wait_for_requests(&capture, MEMBER_IDS.len(), "seed turns").await;

    // Grow ONE member to production-dump scale (~12 MB of persisted
    // transcript): the size-proportional idle-burn class only reproduces
    // against a large session document.
    let large_member = handle
        .member(&AgentIdentity::from(LARGE_MEMBER_ID))
        .await
        .expect("large member handle");
    for turn in 0..LARGE_SESSION_TURNS {
        let filler = format!("large-transcript filler {turn} ")
            .repeat(LARGE_TURN_INPUT_BYTES / 32)
            .chars()
            .take(LARGE_TURN_INPUT_BYTES)
            .collect::<String>();
        large_member
            .send(filler, HandlingMode::Queue)
            .await
            .expect("large seed turn");
    }
    wait_for_requests(
        &capture,
        MEMBER_IDS.len() + LARGE_SESSION_TURNS,
        "large seed turns",
    )
    .await;

    // The regression class under test is store-read/digest loops, so the
    // large document must actually be durable in the session store before
    // the measured window opens.
    let deadline = Instant::now() + Duration::from_secs(120);
    let persisted_bytes = loop {
        let bytes = dir_size_bytes(&sessions_root);
        if bytes >= MIN_PERSISTED_BYTES {
            break bytes;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the large session document to persist: \
             {bytes} bytes on disk (need >= {MIN_PERSISTED_BYTES})"
        );
        sleep(Duration::from_millis(250)).await;
    };
    eprintln!(
        "[idle-burn gate] persisted session store size: {:.1} MB",
        persisted_bytes as f64 / (1024.0 * 1024.0)
    );

    // Quiesce before opening the measured window: trailing durable commits of
    // the large turns are legitimate work.
    wait_for_quiesce("mob never quiesced after seeding").await;

    // The measured contract: a converged, idle mob must consume ~zero CPU
    // regardless of member count or session-document size.
    let cpu_before = process_cpu_time();
    sleep(IDLE_WINDOW).await;
    let idle_cpu = process_cpu_time().saturating_sub(cpu_before);
    eprintln!("[idle-burn gate] idle CPU over {IDLE_WINDOW:?}: {idle_cpu:?}");
    assert!(
        idle_cpu <= MAX_IDLE_CPU,
        "idle mob burned {idle_cpu:?} CPU over {IDLE_WINDOW:?} (limit {MAX_IDLE_CPU:?}); \
         the converged fleet must be event-driven, not busy re-reading or \
         re-verifying unchanged session documents"
    );

    // Cheap idle must not mean dead: the roster is still observable and a
    // member still serves a turn afterwards.
    assert_eq!(
        handle.list_members().await.len(),
        MEMBER_IDS.len(),
        "post-idle roster lost members"
    );
    let turns_before = capture.count();
    handle
        .member(&AgentIdentity::from("lead-1"))
        .await
        .expect("post-idle member handle")
        .send("post-idle liveness probe".to_string(), HandlingMode::Queue)
        .await
        .expect("post-idle turn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while capture.count() <= turns_before {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the post-idle turn to reach the LLM"
        );
        sleep(Duration::from_millis(100)).await;
    }

    // ---- Marker-less cutover leg (the HomeCore production shape) ----
    // A real fleet upgrades onto state migrated by PRE-marker builds: the
    // persisted documents carry transcript history but no `digest_format`
    // marker, so every decode re-runs the legacy heal probe (a full
    // head-transcript hash) plus the per-body graph validation. The idle
    // contract must hold on that state too: rewrite every persisted copy
    // into the pre-marker shape via direct store access, cold-restart the
    // mob on it, and re-measure the idle window.
    let mut member_session_ids = Vec::new();
    for member_id in MEMBER_IDS {
        let session_id = handle
            .resolve_bridge_session_id(&AgentIdentity::from(member_id))
            .await
            .expect("member session id");
        member_session_ids.push((member_id, session_id));
    }

    // Settle the liveness turn's trailing persistence before going down.
    wait_for_quiesce("mob never quiesced after the post-idle liveness turn").await;
    handle
        .shutdown()
        .await
        .expect("shutdown before marker-less restart");
    for (_, session_id) in &member_session_ids {
        service
            .discard_live_session(session_id)
            .await
            .expect("discard live session before same-process restart");
    }
    drop(handle);

    // ---- Marked-baseline cold restart (ratio denominator) ----
    // The persisted documents carry no transcript history state yet (the
    // resume itself will create it via the system-prompt-refresh rewrite),
    // so this is the least memo-exposed resume the runtime can perform on
    // this fleet. Dividing the marker-less resume below by this number
    // cancels machine speed and leaves the decode-time verification cost
    // the memo exists to absorb.
    let marked_resume_start = Instant::now();
    let storage = MobStorage::persistent(&mob_db_path).expect("reopen mob storage for baseline");
    let handle = MobBuilder::for_resume(storage)
        .with_session_service(service.clone())
        .with_default_llm_client(Arc::new(capture.clone()))
        .notify_orchestrator_on_resume(false)
        .resume()
        .await
        .expect("resume mob for the marked baseline");
    let deadline = Instant::now() + Duration::from_secs(240);
    while active_member_count(&handle).await < MEMBER_IDS.len() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {} active members after the marked baseline \
             restart; roster: {:?}",
            MEMBER_IDS.len(),
            handle.list_members().await
        );
        sleep(Duration::from_millis(100)).await;
    }
    let marked_resume = marked_resume_start.elapsed();
    eprintln!("[idle-burn gate] marked baseline resume-to-ready: {marked_resume:?}");

    wait_for_quiesce("mob never quiesced after the marked baseline restart").await;
    handle
        .shutdown()
        .await
        .expect("shutdown after the marked baseline restart");
    for (_, session_id) in &member_session_ids {
        service
            .discard_live_session(session_id)
            .await
            .expect("discard live session after the marked baseline restart");
    }
    drop(handle);

    for (member_id, session_id) in &member_session_ids {
        let path = sessions_root.join(format!("{session_id}.jsonl"));
        let bytes = fs::read(&path).expect("read persisted session document");
        let markerless = markerless_history_document(&bytes, member_id);
        fs::write(&path, &markerless).expect("write marker-less session document");

        // The runtime snapshot is a second full copy decoded on every
        // authoritative load; rewrite it through the store's guarded CAS so
        // both copies carry the identical marker-less document.
        let runtime_id = meerkat_runtime::LogicalRuntimeId::for_session(session_id);
        if let Some(current) = runtime_store
            .load_session_snapshot(&runtime_id)
            .await
            .expect("load runtime session snapshot")
        {
            let replaced = runtime_store
                .replace_session_snapshot_if_current(&runtime_id, &current, markerless.clone())
                .await
                .expect("replace runtime session snapshot");
            assert!(
                replaced,
                "runtime snapshot CAS must apply while the mob is down"
            );
        }
    }
    eprintln!(
        "[idle-burn gate] rewrote {} persisted documents into the marker-less pre-0.8.6 shape",
        member_session_ids.len()
    );

    // Cold restart on the marker-less state — the production cutover shape.
    let resume_start = Instant::now();
    let storage = MobStorage::persistent(&mob_db_path).expect("reopen mob storage");
    let handle = MobBuilder::for_resume(storage)
        .with_session_service(service.clone())
        .with_default_llm_client(Arc::new(capture.clone()))
        .notify_orchestrator_on_resume(false)
        .resume()
        .await
        .expect("resume mob on marker-less state");
    let deadline = Instant::now() + Duration::from_secs(240);
    while active_member_count(&handle).await < MEMBER_IDS.len() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {} active members after the marker-less \
             restart; roster: {:?}",
            MEMBER_IDS.len(),
            handle.list_members().await
        );
        sleep(Duration::from_millis(100)).await;
    }
    let markerless_resume = resume_start.elapsed();
    eprintln!(
        "[idle-burn gate] marker-less resume-to-ready: {markerless_resume:?} \
         (marked baseline: {marked_resume:?})"
    );

    // Resume-cost contract: without the process-lifetime decode memo the
    // marker-less resume re-runs the heal probe plus full graph validation
    // on EVERY decode of every document, and this ratio blows past the
    // budget (measured ~3x the memo-on resume; red-first verifiable with
    // MEERKAT_DISABLE_GRAPH_DECODE_MEMO=1).
    let allowed_markerless_resume =
        marked_resume.mul_f64(MARKERLESS_RESUME_RATIO) + MARKERLESS_RESUME_FLOOR;
    assert!(
        markerless_resume <= allowed_markerless_resume,
        "marker-less resume-to-ready took {markerless_resume:?}, exceeding \
         {allowed_markerless_resume:?} (marked baseline {marked_resume:?} x \
         {MARKERLESS_RESUME_RATIO} + {MARKERLESS_RESUME_FLOOR:?}): marker-less \
         (pre-0.8.6-written) documents are re-paying decode-time \
         transcript-graph verification on every load; the process-lifetime \
         decode memo is not absorbing repeat loads of unchanged marker-less bytes"
    );

    // A fixed-cadence loop that re-pays O(document) digest work per tick on
    // the marker-less documents never quiesces and fails here.
    wait_for_quiesce("mob never quiesced after the marker-less restart").await;

    let cpu_before = process_cpu_time();
    sleep(IDLE_WINDOW).await;
    let idle_cpu = process_cpu_time().saturating_sub(cpu_before);
    eprintln!("[idle-burn gate] marker-less idle CPU over {IDLE_WINDOW:?}: {idle_cpu:?}");
    assert!(
        idle_cpu <= MAX_IDLE_CPU,
        "idle mob burned {idle_cpu:?} CPU over {IDLE_WINDOW:?} (limit {MAX_IDLE_CPU:?}) \
         on marker-less (pre-0.8.6-written) session documents; repeat decodes \
         of unchanged marker-less bytes must be absorbed, not re-verified per tick"
    );

    // Cheap idle must not mean dead, on this state either.
    assert_eq!(
        handle.list_members().await.len(),
        MEMBER_IDS.len(),
        "post-marker-less-idle roster lost members"
    );
    let turns_before = capture.count();
    handle
        .member(&AgentIdentity::from("lead-1"))
        .await
        .expect("post-marker-less-idle member handle")
        .send(
            "post-marker-less-idle liveness probe".to_string(),
            HandlingMode::Queue,
        )
        .await
        .expect("post-marker-less-idle turn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while capture.count() <= turns_before {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the post-marker-less-idle turn to reach the LLM"
        );
        sleep(Duration::from_millis(100)).await;
    }

    handle.shutdown().await.expect("shutdown idle-burn mob");
}
