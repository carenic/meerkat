//! Live-channel orchestration.
//!
//! Populated by W2-A. Hosts the surface-agnostic helper free functions
//! that build live projection snapshots, extract realtime system
//! prompts, and decide when a live channel needs a forced close vs
//! in-place refresh.
//!
//! Gated by the `live` feature on the `meerkat` facade so surfaces
//! that don't ship a live channel (CLI today, MCP-server, embedded
//! examples) don't pull in the `meerkat-live` dependency.
//!
//! The load-bearing methods (`precheck_live_open`,
//! `recover_live_session_for_realtime_open`,
//! `materialize_staged_session_for_realtime_open`,
//! `realtime_session_open_config`, `live_open_config_for_session`,
//! `propagate_config_to_live_channels`) currently live in
//! `meerkat-rpc::SessionRuntime` because they consume a long list of
//! still-RPC-private helpers (`replay_promoted_system_context_on_service`,
//! `current_materialized_llm_identity`, `archive_runtime_cleanup`,
//! `runtime_state_ops`). Once the corresponding accessors land in
//! W3-A/W3-B they will be promoted onto `LiveOrchestrator<'a>`.
//!
//! These free functions only depend on `meerkat-llm-core`, `meerkat-core`,
//! and the model catalog — they do NOT import `meerkat-live`, so they
//! compile unconditionally regardless of the `live` feature. The
//! `live` feature is reserved for the future
//! [`crate::session_runtime::live_orchestration::LiveOrchestrator`]
//! struct that will own the methods consuming `LiveAdapterHost`.

use meerkat_core::types::{Message, SessionId, SystemMessage};
use meerkat_core::{
    PendingSystemContextAppend, Session, SessionLlmIdentity, SessionToolVisibilityState,
};
use meerkat_llm_core::realtime_session::RealtimeSessionOpenConfig;

use crate::session_runtime::errors::LiveOpenPrecheckError;

/// Apply the B19 (realtime-capability) and B18 (provider-supported) gates
/// to a resolved LLM identity. Shared between the staged-session and live-
/// session branches of `precheck_live_open` so both paths enforce
/// identical contracts.
pub fn precheck_identity(identity: &SessionLlmIdentity) -> Result<(), LiveOpenPrecheckError> {
    let realtime_capable = meerkat_core::model_profile::capabilities::capabilities_for(
        identity.provider,
        &identity.model,
    )
    .map(|caps| caps.realtime)
    .unwrap_or(false);
    apply_precheck_gates(identity.provider, &identity.model, realtime_capable)
}

/// Pure gate-ordering helper: B19 (realtime capability) fires before
/// B18 (provider has live adapter). Split out from `precheck_identity`
/// so unit tests can pin the B18 branch — the catalog cannot naturally
/// produce a realtime-capable non-OpenAI row, so e2e never reaches
/// `ProviderHasNoLiveAdapter` and a synthetic `realtime_capable: true`
/// is the only way to assert the non-OpenAI rejection contract. Also
/// documents the gate ordering as intentional: a non-realtime non-OpenAI
/// session reports `ModelNotRealtime` (the more specific failure), not
/// `ProviderHasNoLiveAdapter`.
pub fn apply_precheck_gates(
    provider: meerkat_core::Provider,
    model: &str,
    realtime_capable: bool,
) -> Result<(), LiveOpenPrecheckError> {
    if !realtime_capable {
        return Err(LiveOpenPrecheckError::ModelNotRealtime {
            model: model.to_string(),
            provider: provider.as_str(),
        });
    }
    if !matches!(provider, meerkat_core::Provider::OpenAI) {
        return Err(LiveOpenPrecheckError::ProviderHasNoLiveAdapter {
            provider: provider.as_str(),
        });
    }
    Ok(())
}

/// R10: extract the root system prompt from a projected `seed_messages`
/// vector for use in `LiveProjectionSnapshot.system_prompt`.
///
/// Mirror of `extract_system_prompt_from_seed_messages` in
/// `meerkat-rpc::handlers::live`; duplicated here so the runtime-side
/// `propagate_config_to_live_channels` builder can populate the snapshot
/// without depending on handler-private helpers (matches the existing
/// duplication pattern between the two snapshot builders). See R10 in
/// `LIVE_ADAPTER_REVIEW2_TODO.md` for the full rationale.
///
/// **Why both `System` AND `SystemNotice` are valid lead messages.**
/// `realtime_projection_messages` only rewrites `seed_messages[0]` when
/// `realtime_projection_root_system_message` returns `Some` (resolved
/// system prompt or build instructions present); when that returns
/// `None`, the canonical session transcript's original first message is
/// left in place — and that can legitimately be a `Message::SystemNotice`
/// (e.g. an idle pre-prompt session whose only lead is a runtime-injected
/// `[SYSTEM NOTICE][MCP_PENDING]` notice). Without honoring `SystemNotice`
/// here, a `propagate_config_to_live_channels` refresh whose snapshot
/// leads with one would silently emit empty instructions on
/// `session.update` and wipe the realtime provider's session-level
/// instructions. We use `SystemNoticeMessage::rendered_text()` (the
/// prefix-tagged form, matching the projection the root-system helper
/// itself emits) so the provider sees the same string the agent loop
/// would.
#[must_use]
pub fn extract_system_prompt_from_seed_messages_runtime(
    seed_messages: &[Message],
) -> Option<String> {
    match seed_messages.first()? {
        Message::System(system) => Some(system.content.clone()),
        Message::SystemNotice(notice) => Some(notice.rendered_text()),
        _ => None,
    }
}

/// P1#5: build a [`LiveProjectionSnapshot`] from the resolved
/// [`RealtimeSessionOpenConfig`].
///
/// Mirror of `build_live_projection_snapshot` in
/// `meerkat-rpc::handlers::live`; we duplicate here so
/// `propagate_config_to_live_channels` can run from the runtime layer
/// without depending on handler-private helpers.
///
/// R8: this builder stamps `snapshot_version: 0` as a placeholder. The
/// caller (`propagate_config_to_live_channels`) overwrites it with
/// `host.next_snapshot_version(channel_id)` before dispatch so adapters
/// gating on `snapshot_version` for stale-refresh detection see strictly
/// increasing generations. Do not treat the field returned here as the
/// final stamp.
#[must_use]
pub fn build_live_projection_snapshot_for_runtime(
    session_id: &SessionId,
    open_config: &RealtimeSessionOpenConfig,
) -> meerkat_core::live_adapter::LiveProjectionSnapshot {
    meerkat_core::live_adapter::LiveProjectionSnapshot {
        session_id: session_id.clone(),
        snapshot_version: 0,
        seed_messages: open_config.seed_messages.clone(),
        visible_tools: open_config.visible_tools.clone(),
        // R10: extract the root system prompt from the first
        // `Message::System` entry in `seed_messages`.
        system_prompt: extract_system_prompt_from_seed_messages_runtime(&open_config.seed_messages),
        model_id: open_config.llm_identity.model.clone(),
        provider_id: open_config.llm_identity.provider.as_str().to_string(),
        audio_config: None,
        // R3: forward typed runtime system-context so refresh snapshots
        // carry the same authoritative system instructions the open path
        // emitted (peer terminal, ops_lifecycle, etc.).
        runtime_system_context: open_config.runtime_system_context.clone(),
    }
}

/// R11: pure helper deciding whether a `config/patch`-resolved live
/// identity represents a model or provider swap relative to the identity
/// the channel was opened with.
///
/// Returns `true` when the channel must be closed (so the SDK can reopen
/// against the new identity); `false` when an in-place `Refresh` is safe.
///
/// `bound_identity = None` means the channel was opened without identity
/// recording (degraded factory-less path). Treat as "no swap" so the
/// legacy Refresh fallthrough path still applies.
///
/// Audio-rate change is intentionally NOT checked here. The OpenAI
/// Refresh guard rejects it, but R11's typed runtime path is scoped to
/// model + provider until `audio_config` is plumbed into the projection
/// snapshot. Audio mismatches still surface as the existing async
/// `LiveAdapterErrorCode::ConfigRejected` error from the adapter.
#[must_use]
pub fn live_channel_requires_close_for_identity_change(
    bound_identity: Option<&SessionLlmIdentity>,
    new_identity: &SessionLlmIdentity,
) -> bool {
    match bound_identity {
        Some(prev) => prev.model != new_identity.model || prev.provider != new_identity.provider,
        None => false,
    }
}

/// Build the projection-root system message for a realtime session. The
/// content is the union of the resolved `system_prompt` (or the first
/// existing `System`/`SystemNotice` lead) and any session-build
/// `additional_instructions`.
#[must_use]
pub fn realtime_projection_root_system_message(session: &Session) -> Option<Message> {
    let build_state = session.build_state().unwrap_or_default();
    let mut content = build_state
        .system_prompt
        .or_else(|| {
            session
                .messages()
                .first()
                .and_then(|message| match message {
                    Message::System(system) => Some(system.content.clone()),
                    Message::SystemNotice(notice) => Some(notice.rendered_text()),
                    _ => None,
                })
        })
        .unwrap_or_default();

    if let Some(additional_instructions) = build_state.additional_instructions
        && !additional_instructions.is_empty()
    {
        if !content.trim().is_empty() {
            content.push_str("\n\n");
        }
        content.push_str("[Session Build Instructions]");
        for instruction in additional_instructions {
            let instruction = instruction.trim();
            if instruction.is_empty() {
                continue;
            }
            content.push_str("\n- ");
            content.push_str(instruction);
        }
    }

    if content.trim().is_empty() {
        None
    } else {
        Some(Message::System(SystemMessage::new(content)))
    }
}

/// Project a session's transcript for realtime delivery: prepend or
/// rewrite the lead message with [`realtime_projection_root_system_message`]
/// when one is available.
#[must_use]
pub fn realtime_projection_messages(session: &Session) -> Vec<Message> {
    let mut projected = session.messages().to_vec();
    if let Some(root_system) = realtime_projection_root_system_message(session) {
        match projected.first() {
            Some(Message::System(_) | Message::SystemNotice(_)) => projected[0] = root_system,
            _ => projected.insert(0, root_system),
        }
    }
    projected
}

/// Project a session's runtime system context into the realtime
/// open-config shape (applied + pending appends concatenated).
#[must_use]
pub fn realtime_projection_runtime_system_context(
    session: &Session,
) -> Vec<PendingSystemContextAppend> {
    let state = session.system_context_state().unwrap_or_default();
    state.applied.into_iter().chain(state.pending).collect()
}

/// Read the typed visibility state directly from the session without
/// going through the realtime projection. Used by RPC tests to verify
/// projection equivalence; kept un-gated so `meerkat-rpc` test builds
/// can call into it even when the upstream `meerkat` crate is not
/// itself built in test mode.
#[allow(clippy::expect_used)]
pub fn exported_tool_visibility_state(session: &Session) -> SessionToolVisibilityState {
    session
        .tool_visibility_state()
        .expect("exported visibility state should decode")
        .unwrap_or_default()
}

/// Synthesize a builtin tool visibility witness that matches the agent
/// loop's stable owner key derivation for the builtin source. Used by
/// RPC tests; kept un-gated for the same reason as
/// [`exported_tool_visibility_state`].
#[must_use]
pub fn builtin_tool_visibility_witness() -> meerkat_core::ToolVisibilityWitness {
    let provenance = meerkat_core::ToolProvenance {
        kind: meerkat_core::ToolSourceKind::Builtin,
        source_id: "builtin".into(),
    };
    meerkat_core::ToolVisibilityWitness {
        stable_owner_key: Some(
            meerkat_core::tool_catalog::stable_owner_key_from_provenance(&provenance),
        ),
        last_seen_provenance: Some(provenance),
    }
}
