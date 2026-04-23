//! Runtime and input RPC wire contracts.

use serde::{Deserialize, Serialize};

use crate::wire::session::WireContentBlock;
use meerkat_core::comms::PeerName;

/// Request payload for `session/realtime_attachment_status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeRealtimeAttachmentStatusParams {
    pub session_id: String,
}

/// Terminal status for dedicated correlated peer-response ingress.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PeerResponseTerminalStatusWire {
    Completed,
    Failed,
    Cancelled,
}

/// Dedicated request payload for `session/peer_response_terminal`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SessionPeerResponseTerminalParams {
    pub session_id: String,
    pub peer_name: PeerName,
    pub request_id: String,
    pub status: PeerResponseTerminalStatusWire,
    pub result: serde_json::Value,
}

/// Typed event envelope for the generic `session/external_event` and
/// `/sessions/{id}/external-events` surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionExternalEventEnvelope {
    /// Generic external JSON event admitted as `Input::ExternalEvent`.
    GenericJson {
        event_type: String,
        payload: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocks: Option<Vec<WireContentBlock>>,
    },
    /// Reserved typed semantic. Callers must use the dedicated
    /// `session/peer_response_terminal` / `/peer-response-terminal` surface
    /// instead of routing terminal peer responses through the generic event
    /// ingress.
    PeerResponseTerminal {
        peer_name: PeerName,
        request_id: String,
        status: PeerResponseTerminalStatusWire,
        result: serde_json::Value,
    },
}

/// Request payload for `session/retire`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeRetireParams {
    pub session_id: String,
}

/// Request payload for `session/reset`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeResetParams {
    pub session_id: String,
}

/// Request payload for `session/submission`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InputStateParams {
    pub session_id: String,
    pub input_id: String,
}

/// Request payload for `session/submissions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InputListParams {
    pub session_id: String,
}

/// Public runtime state projection used by RPC surfaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireRuntimeState {
    Initializing,
    Idle,
    Attached,
    Running,
    Retired,
    Stopped,
    Destroyed,
}

/// Public live attachment status projection used by runtime and mob surfaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireRealtimeAttachmentStatus {
    Unattached,
    IntentPresentUnbound,
    BindingNotReady,
    BindingReady,
    ReplacementPending,
    ReattachRequired,
}

/// Response payload for `session/realtime_attachment_status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeRealtimeAttachmentStatusResult {
    pub status: WireRealtimeAttachmentStatus,
}

/// Discriminator for `session/submit` responses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAcceptOutcomeType {
    Accepted,
    Deduplicated,
    Rejected,
}

/// Public input lifecycle state projection used by RPC surfaces.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireInputLifecycleState {
    Accepted,
    Queued,
    Staged,
    Applied,
    AppliedPendingConsumption,
    Consumed,
    Superseded,
    Coalesced,
    Abandoned,
}

/// Input transition history entry for RPC-facing snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WireInputStateHistoryEntry {
    pub timestamp: String,
    pub from: WireInputLifecycleState,
    pub to: WireInputLifecycleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// RPC-facing input state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WireInputState {
    pub input_id: String,
    pub current_state: WireInputLifecycleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default)]
    pub recovery_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<WireInputStateHistoryEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconstruction_source: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persisted_input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_boundary_sequence: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Response payload for `session/submission`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum InputStateResult {
    Found(Box<WireInputState>),
    Missing(()),
}

/// Response payload for `session/submit`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeAcceptResult {
    pub outcome_type: RuntimeAcceptOutcomeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<WireInputState>,
}

/// Response payload for `session/retire`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeRetireResult {
    pub inputs_abandoned: usize,
    #[serde(default)]
    pub inputs_pending_drain: usize,
}

/// Response payload for `session/reset`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeResetResult {
    pub inputs_abandoned: usize,
}

/// Response payload for `session/submissions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InputListResult {
    pub input_ids: Vec<String>,
}
