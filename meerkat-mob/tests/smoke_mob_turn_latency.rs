#![cfg(all(feature = "integration-real-tests", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//!
//! Turn-latency size-independence gate for the mob runtime (turbo-s smoke
//! lane), sibling to `smoke_mob_idle_burn`.
//!
//! The production defect this pins (measured 2026-07-25 on a live 0.8.6
//! fleet): identical one-word-ACK turns took 60 seconds at a 14 MB session
//! and over 180 seconds at 94 MB, because turn-boundary work — canonical
//! serialization + SHA-256 over the whole session document, authority
//! reloads that re-decode the full snapshot, and whole-blob persistence —
//! is O(document) regardless of how small the turn's actual delta is.
//!
//! The asserted contract is SIZE INDEPENDENCE, not "faster than before":
//! a threshold calibrated against today's cost rots, and "2x faster" still
//! scales. Two members are grown to very different transcript sizes in the
//! SAME process on the SAME machine, both are driven through N identical
//! tiny turns, and the per-turn cost of the large member must stay within a
//! small constant factor of the small member's. Any O(document) pass left
//! on the turn boundary makes the large member's per-turn cost track its
//! document size and trips the ratio.
//!
//! The primary signal is process CPU time (`cpu_time::ProcessTime`):
//! thread-agnostic (the boundary work runs on tokio workers and
//! `spawn_blocking` threads), robust against CI scheduler noise, and the
//! defect itself is CPU-bound (serialize + digest passes). The per-thread
//! `session_content_digest_computations` counter would be the ideal
//! deterministic signal but is `thread_local!` and therefore under-counts
//! from the test thread under the multi-threaded runtime, so it is not
//! used. Wall time per turn — the thing users actually feel — is printed
//! as a diagnostic but never asserted.
//!
//! This test must stay ALONE in its test binary so no sibling test's CPU
//! pollutes the measurement. No live provider is involved: members run
//! against a scripted LLM client, so the lane needs no API keys.
//!
//! Run with:
//!   cargo test -p meerkat-mob --test smoke_mob_turn_latency \
//!     --features integration-real-tests -- --ignored --nocapture

use meerkat::{AgentFactory, Config, FactoryAgentBuilder};
use meerkat_core::types::HandlingMode;
use meerkat_mob::definition::{OrchestratorConfig, WiringRules};
use meerkat_mob::{
    AgentIdentity, MobBuilder, MobDefinition, MobHandle, MobId, MobMemberStatus, MobRuntimeMode,
    MobStorage, Profile, ProfileBinding, ProfileName, SpawnMemberSpec, ToolConfig,
};
use meerkat_session::PersistentSessionService;
use meerkat_store::{MemoryBlobStore, SqliteSessionStore, StoreAdapter};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::time::{Duration, Instant, sleep};

/// One-word ACK driven at both fixtures. Identical inputs are the point:
/// only the accumulated document size differs between the two measurements.
const MEASURED_TURN_PROMPT: &str = "ack?";
const MEASURED_TURNS: usize = 4;

/// The small member's transcript: one modest seed turn (~256 KB), so the
/// baseline exercises the same boundary machinery over a real but small
/// document.
const SMALL_SEED_INPUT_BYTES: usize = 256 * 1024;

/// The large member's transcript is grown from turns carrying inputs of
/// this size, giving an accumulated transcript of at least
/// `LARGE_SESSION_TURNS * LARGE_TURN_INPUT_BYTES` ≈ 10 MB — the
/// production-scale class where O(document) turn-boundary work became
/// minutes per turn (synthetic; nothing committed).
const LARGE_TURN_INPUT_BYTES: usize = 2_500_000;
const LARGE_SESSION_TURNS: usize = 4;

/// The durable stores must grow by at least this much after the large
/// member is seeded, proving the large fixture is actually large ON DISK
/// (a green gate with a small-on-disk "large" fixture would be measuring
/// nothing).
const MIN_LARGE_GROWTH_BYTES: u64 = 8 * 1024 * 1024;

/// Flatness tolerance: per-turn CPU at ~10 MB must stay within K× the
/// per-turn CPU at ~256 KB. With turn cost truly independent of document
/// size the ratio is ~1 (same fixed path, delta-only content), so K = 3
/// leaves real headroom for CI noise and O(delta) variance — while any
/// O(document) boundary pass at a ~40× size ratio measures far above it.
const MAX_LARGE_TO_SMALL_CPU_RATIO: u32 = 3;

/// Ratio floor for the small side: if the large member's per-turn CPU is
/// within `K × max(small, floor)`, turns are cheap in absolute terms and
/// flat in the only sense that matters. This keeps a post-fix run where
/// both sides cost mere tens of milliseconds from failing on ratio noise,
/// without masking the defect (a size-proportional turn at 10 MB costs
/// seconds of CPU, far above 300 ms).
const SMALL_COST_FLOOR: Duration = Duration::from_millis(100);

const MEMBER_IDS: [&str; 3] = ["lead-1", "w-small", "w-large"];
const SMALL_MEMBER_ID: &str = "w-small";
const LARGE_MEMBER_ID: &str = "w-large";

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

fn gate_profile(peer_description: &str) -> Profile {
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

fn gate_mob_definition() -> MobDefinition {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        ProfileName::from("lead"),
        ProfileBinding::Inline(Box::new(gate_profile("Leads the turn-latency gate mob"))),
    );
    profiles.insert(
        ProfileName::from("worker"),
        ProfileBinding::Inline(Box::new(gate_profile("Turn-latency gate worker"))),
    );

    let mut definition = MobDefinition::explicit(MobId::from("turn-latency-gate"));
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

/// Recursive on-disk size of the durable store root (SQLite dbs + WAL
/// sidecars). Used to prove the large fixture is actually large on disk
/// before anything is measured.
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
    let deadline = Instant::now() + Duration::from_secs(240);
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

/// Wait until a 2s process-CPU probe reads idle-level, so trailing durable
/// boundary commits are finished (and their CPU is attributed) before a
/// measurement window closes or the next one opens.
async fn quiesce(what: &str) {
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        let probe_start = process_cpu_time();
        sleep(Duration::from_secs(2)).await;
        let probe_burn = process_cpu_time().saturating_sub(probe_start);
        if probe_burn < Duration::from_millis(200) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "mob never quiesced {what}: still burning {probe_burn:?} per 2s probe"
        );
    }
}

struct TurnCost {
    cpu_per_turn: Duration,
    wall_per_turn: Duration,
}

/// Drive `MEASURED_TURNS` identical tiny turns at one member and return the
/// per-turn process-CPU and wall cost. The window opens after a quiesce and
/// closes after the trailing quiesce, so asynchronous turn-boundary
/// persistence is attributed to the turn that caused it.
async fn measure_member_turns(
    handle: &MobHandle,
    capture: &CaptureClient,
    member_id: &str,
) -> TurnCost {
    quiesce(&format!("before measuring {member_id}")).await;
    let member = handle
        .member(&AgentIdentity::from(member_id))
        .await
        .expect("measured member handle");

    let cpu_start = process_cpu_time();
    let wall_start = Instant::now();
    for turn in 0..MEASURED_TURNS {
        let expected = capture.count() + 1;
        member
            .send(MEASURED_TURN_PROMPT.to_string(), HandlingMode::Queue)
            .await
            .expect("measured turn send");
        wait_for_requests(
            capture,
            expected,
            &format!("measured turn {turn} at {member_id}"),
        )
        .await;
    }
    quiesce(&format!("after measuring {member_id}")).await;
    let cpu = process_cpu_time().saturating_sub(cpu_start);
    let wall = wall_start.elapsed();

    TurnCost {
        cpu_per_turn: cpu / MEASURED_TURNS as u32,
        wall_per_turn: wall / MEASURED_TURNS as u32,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "lane:e2e-smoke"]
async fn e2e_smoke_mob_turn_latency_gate() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path();
    let user_config_root = root.join("user-config");
    let runtime_root = root.join("runtime-root");
    let project_root = root.join("project-root");
    let context_root = root.join("context-root");
    let stores_root = root.join("stores");
    let mob_db_path = root.join("mob.db");
    for dir in [&project_root, &context_root, &stores_root] {
        fs::create_dir_all(dir).expect("create turn-gate roots");
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
    // Production shape: an INCREMENTAL SQLite session store (so boundary
    // saves take the O(delta)-rows projection branch — the path the fix
    // must make flat) and a SQLite runtime store (whole-blob snapshot
    // commits with their decode + save-guard costs).
    let store =
        Arc::new(SqliteSessionStore::open(stores_root.join("sessions.db")).expect("session store"));
    builder.default_session_store = Some(Arc::new(StoreAdapter::new(store.clone())));

    let store_dyn: Arc<dyn meerkat::SessionStore> = store;
    let runtime_store: Arc<dyn meerkat_runtime::RuntimeStore> = Arc::new(
        meerkat_runtime::SqliteRuntimeStore::new(stores_root.join("runtime.db"))
            .expect("runtime store"),
    );
    let blob_store: Arc<dyn meerkat_core::BlobStore> = Arc::new(MemoryBlobStore::default());
    let service = Arc::new(PersistentSessionService::new(
        builder,
        32,
        store_dyn,
        runtime_store,
        blob_store,
    ));

    let storage = MobStorage::persistent(&mob_db_path).expect("create persistent mob storage");
    let handle = MobBuilder::new(gate_mob_definition(), storage)
        .with_session_service(service.clone())
        .with_default_llm_client(Arc::new(capture.clone()))
        .create()
        .await
        .expect("create persistent turn-latency mob");

    handle
        .spawn_spec(SpawnMemberSpec::new("lead", AgentIdentity::from("lead-1")))
        .await
        .expect("spawn lead");
    handle
        .spawn_spec(SpawnMemberSpec::new(
            "worker",
            AgentIdentity::from(SMALL_MEMBER_ID),
        ))
        .await
        .expect("spawn small worker");
    handle
        .spawn_spec(SpawnMemberSpec::new(
            "worker",
            AgentIdentity::from(LARGE_MEMBER_ID),
        ))
        .await
        .expect("spawn large worker");

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

    // Seed transcripts. The lead gets a token turn; the SMALL member gets
    // its ~256 KB baseline document.
    handle
        .member(&AgentIdentity::from("lead-1"))
        .await
        .expect("lead handle")
        .send(
            "fixture transcript for lead-1".to_string(),
            HandlingMode::Queue,
        )
        .await
        .expect("lead seed turn");
    let small_seed = "small baseline transcript "
        .repeat(SMALL_SEED_INPUT_BYTES / 24)
        .chars()
        .take(SMALL_SEED_INPUT_BYTES)
        .collect::<String>();
    handle
        .member(&AgentIdentity::from(SMALL_MEMBER_ID))
        .await
        .expect("small member handle")
        .send(small_seed, HandlingMode::Queue)
        .await
        .expect("small seed turn");
    wait_for_requests(&capture, 2, "lead + small seed turns").await;
    quiesce("after small seeding").await;
    let baseline_store_bytes = dir_size_bytes(&stores_root);

    // Grow ONE member to production scale (~10 MB of accumulated
    // transcript). Only the document size distinguishes the two fixtures.
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
    wait_for_requests(&capture, 2 + LARGE_SESSION_TURNS, "large seed turns").await;

    // The defect class is size-proportional durable-boundary work, so the
    // large document must actually be durable before anything is measured.
    let deadline = Instant::now() + Duration::from_secs(240);
    let grown_store_bytes = loop {
        let bytes = dir_size_bytes(&stores_root);
        if bytes >= baseline_store_bytes + MIN_LARGE_GROWTH_BYTES {
            break bytes;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the large session document to persist: \
             {bytes} bytes on disk (baseline {baseline_store_bytes}, need >= \
             {MIN_LARGE_GROWTH_BYTES} of growth)"
        );
        sleep(Duration::from_millis(250)).await;
    };
    eprintln!(
        "[turn-latency gate] durable stores: {:.2} MB after small seed, {:.2} MB after large growth",
        baseline_store_bytes as f64 / (1024.0 * 1024.0),
        grown_store_bytes as f64 / (1024.0 * 1024.0),
    );

    // Measure both fixtures in the same process, back to back: small first,
    // then large. Each window is quiesce-bracketed so trailing boundary
    // persistence is attributed to its own fixture.
    let small = measure_member_turns(&handle, &capture, SMALL_MEMBER_ID).await;
    eprintln!(
        "[turn-latency gate] small (~{} KB doc): {:?} CPU / {:?} wall per turn over {MEASURED_TURNS} turns",
        SMALL_SEED_INPUT_BYTES / 1024,
        small.cpu_per_turn,
        small.wall_per_turn,
    );
    let large = measure_member_turns(&handle, &capture, LARGE_MEMBER_ID).await;
    eprintln!(
        "[turn-latency gate] large (~{} MB doc): {:?} CPU / {:?} wall per turn over {MEASURED_TURNS} turns",
        (LARGE_TURN_INPUT_BYTES * LARGE_SESSION_TURNS) / (1024 * 1024),
        large.cpu_per_turn,
        large.wall_per_turn,
    );

    let small_effective = small.cpu_per_turn.max(SMALL_COST_FLOOR);
    let cpu_ratio = large.cpu_per_turn.as_secs_f64() / small_effective.as_secs_f64();
    let wall_ratio =
        large.wall_per_turn.as_secs_f64() / small.wall_per_turn.max(SMALL_COST_FLOOR).as_secs_f64();
    eprintln!(
        "[turn-latency gate] per-turn cost ratio large/small: {cpu_ratio:.1}x CPU \
         (floored small = {small_effective:?}), {wall_ratio:.1}x wall (diagnostic only)",
    );

    // The measured contract: an identical tiny turn must cost the same
    // whether the accumulated document is ~256 KB or ~10 MB. Any
    // O(document) pass left on the turn boundary (whole-document canonical
    // serialize, whole-document digest, full-snapshot decode, whole-blob
    // rewrite) makes this ratio track the ~40x size ratio.
    assert!(
        large.cpu_per_turn <= small_effective * MAX_LARGE_TO_SMALL_CPU_RATIO,
        "turn cost scales with document size: {:?} CPU per turn at the ~10 MB \
         member vs {:?} at the ~256 KB member ({cpu_ratio:.1}x, limit \
         {MAX_LARGE_TO_SMALL_CPU_RATIO}x; wall {:?} vs {:?}). Turn-boundary \
         work must be O(delta), not O(document): a one-word reply on a large \
         session may not re-serialize, re-digest, or re-persist the whole \
         accumulated document",
        large.cpu_per_turn,
        small.cpu_per_turn,
        large.wall_per_turn,
        small.wall_per_turn,
    );

    handle.shutdown().await.expect("shutdown turn-latency mob");
}
