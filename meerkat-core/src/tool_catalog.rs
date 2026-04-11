use crate::types::ToolDef;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Which projection plane a catalog entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPlaneClass {
    Session,
    Control,
}

/// Whether a catalog entry may be deferred behind the control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCatalogDeferredEligibility {
    InlineOnly,
    DeferredEligible { stable_owner_key: String },
}

/// Precedence-resolved catalog entry for one canonical tool name.
///
/// Entries represent the canonical winner for a tool identity even when that
/// tool is not currently callable. Policy-hidden names and collision losers are
/// omitted from the catalog entirely.
#[derive(Debug, Clone)]
pub struct ToolCatalogEntry {
    pub tool: Arc<ToolDef>,
    pub plane: ToolPlaneClass,
    pub currently_callable: bool,
    pub deferred_eligibility: ToolCatalogDeferredEligibility,
}

impl ToolCatalogEntry {
    pub fn session_inline(tool: Arc<ToolDef>, currently_callable: bool) -> Self {
        Self {
            tool,
            plane: ToolPlaneClass::Session,
            currently_callable,
            deferred_eligibility: ToolCatalogDeferredEligibility::InlineOnly,
        }
    }

    pub fn control_inline(tool: Arc<ToolDef>, currently_callable: bool) -> Self {
        Self {
            tool,
            plane: ToolPlaneClass::Control,
            currently_callable,
            deferred_eligibility: ToolCatalogDeferredEligibility::InlineOnly,
        }
    }

    pub fn session_deferred(
        tool: Arc<ToolDef>,
        currently_callable: bool,
        stable_owner_key: String,
    ) -> Self {
        Self {
            tool,
            plane: ToolPlaneClass::Session,
            currently_callable,
            deferred_eligibility: ToolCatalogDeferredEligibility::DeferredEligible {
                stable_owner_key,
            },
        }
    }
}

/// Dispatcher-level catalog support flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolCatalogCapabilities {
    /// True only when `tool_catalog()` is an exact precedence-resolved registry
    /// for this dispatcher.
    pub exact_catalog: bool,
}

/// Canonical rejection reasons for deferred tool loads.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCatalogLoadRejectedReason {
    UnknownKey,
    NotDeferredEligible,
    AlreadyRequested,
    NotFilterable,
}

/// Structured result for one requested catalog name.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ToolCatalogLoadResolution {
    pub name: String,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_reason: Option<ToolCatalogLoadRejectedReason>,
}
