//! # 024 - Multi-Turn Event Processing (Rust)
//!
//! Demonstrates explicit multi-turn processing with `EphemeralSessionService`.
//! The program directly submits each turn and the in-process session retains
//! context between calls.
//!
//! ## What this example demonstrates
//! - `EphemeralSessionService`: standalone in-memory session lifecycle
//! - Multi-turn processing via repeated `start_turn()` calls
//! - Event streaming via `AgentEvent` across multiple injected turns
//! - Reading session state to observe accumulating context
//!
//! ## How it works
//! `EphemeralSessionService` spawns a dedicated tokio task per session. That task
//! exclusively owns the `Agent` and processes commands via channels.
//! `create_session()` runs the first turn; subsequent `start_turn()` calls inject
//! new prompts. The agent retains full conversation history across turns in
//! the current process.
//!
//! This example does not configure comms, external-event ingress, schedules, or
//! process recovery. For those behaviors, use a runtime-backed surface such as
//! `rkat run --keep-alive --comms-name processor`.
//!
//! ## Run
//! ```bash
//! ANTHROPIC_API_KEY=... ./scripts/repo-cargo run -p meerkat --example 024-host-mode-event-mesh --features jsonl-store
//! ```

use std::sync::Arc;

use meerkat::{
    AgentEvent, AgentFactory, Config, CreateSessionRequest, EphemeralSessionService,
    FactoryAgentBuilder, SessionService, StartTurnRequest, StartTurnRuntimeSemantics,
};
use meerkat_core::EventEnvelope;
use meerkat_core::service::InitialTurnPolicy;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "Set ANTHROPIC_API_KEY to run this example")?;

    // ── Architecture overview ──────────────────────────────────────────────

    println!(
        r"=== In-Process Multi-Turn Session ===

Single-turn mode:
  User prompt --> Agent runs --> Agent stops

This example:
  create_session() --> run initial prompt
  start_turn(...)  --> run monitoring update with prior context
  start_turn(...)  --> run resolution update with prior context
  archive()        --> stop the in-process session task

The prompts are submitted directly by this program. No webhook, peer message,
timer, or durable external ingress is configured here.
"
    );

    // ── 1. Build the session service ───────────────────────────────────────

    // AgentFactory handles provider resolution, prompt assembly, and tool
    // dispatcher setup. The factory reads ANTHROPIC_API_KEY from the
    // environment to authenticate LLM calls.
    let _tmp = tempfile::tempdir()?;
    let store_dir = _tmp.path().join("sessions");
    std::fs::create_dir_all(&store_dir)?;

    let factory = AgentFactory::new(store_dir);
    let config = Config::default();

    // FactoryAgentBuilder bridges AgentFactory into the SessionAgentBuilder
    // trait used by EphemeralSessionService. We can optionally inject a
    // default LLM client so all sessions use it without per-request overrides.
    let llm_client: Arc<dyn meerkat_client::LlmClient> =
        Arc::new(meerkat::AnthropicClient::new(api_key)?);

    let mut builder = FactoryAgentBuilder::new(factory, config);
    builder.default_llm_client = Some(llm_client);

    // EphemeralSessionService: in-memory session lifecycle. Each session gets
    // a dedicated tokio task. Max 4 concurrent sessions.
    let service = Arc::new(EphemeralSessionService::new(builder, 4));

    // ── 2. Create session (first turn) ─────────────────────────────────────

    println!("--- Turn 1: Initial alert ---\n");

    let (event_tx, event_rx) = mpsc::channel::<EventEnvelope<AgentEvent>>(256);
    let event_collector = spawn_event_collector(event_rx);

    let result = service
        .create_session(CreateSessionRequest {
            injected_context: Vec::new(),
            model: "claude-sonnet-4-6".to_string(),
            prompt: "An alert just fired: 'CPU usage on prod-web-03 exceeded 95% for \
                     5 minutes.' Acknowledge the alert and describe your initial triage \
                     steps. Keep your response to 2-3 sentences."
                .into(),
            system_prompt: meerkat::SystemPromptOverride::Set(
                "You are a concise incident-response coordinator. \
                 You maintain context across multiple event injections, building an \
                 evolving picture of the incident. When you receive new information, \
                 integrate it with what you already know and adjust your response plan. \
                 Always be brief: 2-3 sentences max."
                    .to_string(),
            ),
            max_tokens: Some(256),
            event_tx: Some(event_tx),
            initial_turn: InitialTurnPolicy::RunImmediately,
            deferred_prompt_policy: meerkat_core::service::DeferredPromptPolicy::Discard,
            build: None,
            labels: None,
        })
        .await?;

    let session_id = result.session_id.clone();
    let events = event_collector.await?;
    print_turn_summary(1, &result.text, &events);

    // ── 3. Inject event: new monitoring data ───────────────────────────────

    println!("\n--- Turn 2: Monitoring event injected ---\n");

    let (event_tx, event_rx) = mpsc::channel::<EventEnvelope<AgentEvent>>(256);
    let event_collector = spawn_event_collector(event_rx);

    // start_turn submits a new prompt to the in-process session. The agent has
    // access to the prior conversation history. This call is made directly by
    // the example; it is not external-event ingress.
    let result = service
        .start_turn(
            &session_id,
            StartTurnRequest {
                injected_context: Vec::new(),
                prompt: "[MONITORING EVENT] Memory usage on prod-web-03 is now at 89%. \
                         Three other nodes in the cluster show normal metrics. \
                         The deployment log shows a new release was pushed 12 minutes ago."
                    .into(),
                system_prompt: None,
                event_tx: Some(event_tx),
                runtime: StartTurnRuntimeSemantics::default(),
            },
        )
        .await?;

    let events = event_collector.await?;
    print_turn_summary(2, &result.text, &events);

    // ── 4. Read session state to show accumulated context ──────────────────

    let view = service.read(&session_id).await?;
    println!(
        "  [Session state: {} messages, {} tokens, active={}]\n",
        view.state.message_count, view.billing.total_tokens, view.state.is_active,
    );

    // ── 5. Inject event: resolution update ─────────────────────────────────

    println!("--- Turn 3: Resolution event injected ---\n");

    let (event_tx, event_rx) = mpsc::channel::<EventEnvelope<AgentEvent>>(256);
    let event_collector = spawn_event_collector(event_rx);

    let result = service
        .start_turn(
            &session_id,
            StartTurnRequest {
                injected_context: Vec::new(),
                prompt: "[RESOLUTION EVENT] The team rolled back the release on prod-web-03. \
                         CPU is back to 40%, memory at 52%. All health checks passing. \
                         Summarize the full incident timeline and close it out."
                    .into(),
                system_prompt: None,
                event_tx: Some(event_tx),
                runtime: StartTurnRuntimeSemantics::default(),
            },
        )
        .await?;

    let events = event_collector.await?;
    print_turn_summary(3, &result.text, &events);

    // ── 6. Final session state ─────────────────────────────────────────────

    let view = service.read(&session_id).await?;
    println!(
        "  [Final session: {} messages, {} total tokens]\n",
        view.state.message_count, view.billing.total_tokens,
    );

    // ── 7. Archive (clean shutdown) ────────────────────────────────────────

    service.archive(&session_id).await?;
    println!("  Session archived (task stopped).\n");

    // Scope and runtime boundary

    println!(
        r"
=== Scope and Runtime Boundary ===

Configured here:
  - Direct create_session() and start_turn() calls
  - Context retention within the current process
  - AgentEvent streaming for each submitted turn
  - Explicit archive() cleanup

Not configured here:
  - Comms identity or peer messaging
  - Webhook or external-event ingress
  - Scheduler wakeups
  - Runtime-backed recovery after process exit

Use the runtime-backed CLI, REST, JSON-RPC, MCP, or SDK host when those
operational behaviors are required.
"
    );

    Ok(())
}

/// Spawn a task that collects events and returns them when the channel closes.
fn spawn_event_collector(
    mut event_rx: mpsc::Receiver<EventEnvelope<AgentEvent>>,
) -> tokio::task::JoinHandle<Vec<AgentEvent>> {
    tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(envelope) = event_rx.recv().await {
            events.push(envelope.payload);
        }
        events
    })
}

/// Print a summary of a turn: the response text and event statistics.
fn print_turn_summary(turn: usize, text: &str, events: &[AgentEvent]) {
    let mut text_deltas = 0usize;
    let mut delta_bytes = 0usize;
    let mut turns_started = 0usize;
    let mut turns_completed = 0usize;

    for event in events {
        match event {
            AgentEvent::TextDelta { delta } => {
                text_deltas += 1;
                delta_bytes += delta.len();
            }
            AgentEvent::TurnStarted { .. } => turns_started += 1,
            AgentEvent::TurnCompleted { .. } => turns_completed += 1,
            _ => {}
        }
    }

    println!("  Turn {turn} response: {text}");
    println!(
        "  Events: {} total ({} text deltas, {} bytes streamed, {} turns started, {} completed)",
        events.len(),
        text_deltas,
        delta_bytes,
        turns_started,
        turns_completed,
    );
}
