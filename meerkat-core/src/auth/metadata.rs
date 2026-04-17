//! Auth metadata types — generic, provider-neutral.
//!
//! Per-provider marker structs (`OpenAiRouteHints`, `AnthropicAuthMetadata`,
//! etc.) are intentionally empty `#[non_exhaustive]` placeholders in Phase 1.
//! They pin the route-hint shape so the public `AuthRouteHints` enum is
//! stable, but `meerkat-core` does not own provider-specific semantics —
//! those land in `meerkat-client` provider runtimes when the fields are
//! actually needed.

use serde::{Deserialize, Serialize};

/// Generic auth metadata attached to a resolved lease. Provider-specific
/// metadata goes under [`AuthMetadata::provider_metadata`] / [`AuthMetadata::route_hints`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuthMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default)]
    pub route_hints: AuthRouteHints,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderAuthMetadata>,
}

/// Non-secret defaults merged during auth profile resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuthMetadataDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub route_hints: AuthRouteHints,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderAuthMetadata>,
}

/// Provider-specific route hints (boxed to keep the enum small).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum AuthRouteHints {
    #[default]
    None,
    OpenAi(Box<OpenAiRouteHints>),
    Anthropic(Box<AnthropicRouteHints>),
    Google(Box<GoogleRouteHints>),
}

/// Provider-tagged auth metadata. Content is opaque to `meerkat-core`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum ProviderAuthMetadata {
    OpenAi(OpenAiAuthMetadata),
    Anthropic(AnthropicAuthMetadata),
    Google(GoogleAuthMetadata),
}

// Per-provider marker structs. Empty + non_exhaustive means `meerkat-core`
// declares the shape without owning the contents. Provider runtimes fill
// them in as real fields become necessary.

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct OpenAiRouteHints {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AnthropicRouteHints {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct GoogleRouteHints {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct OpenAiAuthMetadata {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AnthropicAuthMetadata {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct GoogleAuthMetadata {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn auth_metadata_default_is_empty() {
        let m = AuthMetadata::default();
        assert!(m.account_id.is_none());
        assert_eq!(m.route_hints, AuthRouteHints::None);
        assert!(m.provider_metadata.is_none());
    }

    #[test]
    fn route_hints_serde_roundtrip() {
        for hints in [
            AuthRouteHints::None,
            AuthRouteHints::OpenAi(Box::default()),
            AuthRouteHints::Anthropic(Box::default()),
            AuthRouteHints::Google(Box::default()),
        ] {
            let s = serde_json::to_string(&hints).unwrap();
            let back: AuthRouteHints = serde_json::from_str(&s).unwrap();
            assert_eq!(back, hints);
        }
    }

    #[test]
    fn provider_auth_metadata_roundtrip() {
        let m = ProviderAuthMetadata::OpenAi(OpenAiAuthMetadata::default());
        let s = serde_json::to_string(&m).unwrap();
        let back: ProviderAuthMetadata = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }
}
