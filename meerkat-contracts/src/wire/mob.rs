//! Mob wiring RPC wire contracts.

use serde::{Deserialize, Serialize};

/// Minimal trusted peer spec for public mob wiring surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WireTrustedPeerSpec {
    pub name: String,
    pub peer_id: String,
    pub address: String,
}

/// Target for a mob wire/unwire call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MobPeerTarget {
    Local(String),
    External(WireTrustedPeerSpec),
}

/// Request payload for `mob/wire`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MobWireParams {
    pub mob_id: String,
    pub member: String,
    pub peer: MobPeerTarget,
}

/// Response payload for `mob/wire`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MobWireResult {
    pub wired: bool,
}

/// Request payload for `mob/unwire`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MobUnwireParams {
    pub mob_id: String,
    pub member: String,
    pub peer: MobPeerTarget,
}

/// Response payload for `mob/unwire`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MobUnwireResult {
    pub unwired: bool,
}

/// Public handling mode for mob member delivery.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireHandlingMode {
    Queue,
    Steer,
}

/// Public render class contract for mob member delivery.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireRenderClass {
    UserPrompt,
    PeerMessage,
    PeerRequest,
    PeerResponse,
    ExternalEvent,
    FlowStep,
    Continuation,
    SystemNotice,
    ToolScopeNotice,
    OpsProgress,
}

/// Public render salience contract for mob member delivery.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WireRenderSalience {
    Background,
    Normal,
    Important,
    Urgent,
}

/// Public render metadata contract for mob member delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WireRenderMetadata {
    pub class: WireRenderClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salience: Option<WireRenderSalience>,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn mob_wire_params_reject_legacy_local_target_shape() {
        let err = serde_json::from_value::<MobWireParams>(serde_json::json!({
            "mob_id": "mob-1",
            "local": "member-a",
            "target": { "local": "member-b" }
        }))
        .expect_err("legacy local/target shape must be rejected");

        let msg = err.to_string();
        assert!(
            msg.contains("unknown field `local`") || msg.contains("missing field `member`"),
            "unexpected error: {msg}"
        );
    }
}
