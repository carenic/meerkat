//! Normalized binding shapes, resolved-connection shim, and concrete
//! lease implementations used by Phase 2 provider runtimes.
//!
//! `NormalizedBackendKind` / `NormalizedAuthMethod` are typed sums over
//! per-provider enums declared in `providers/<p>/{backend,auth}.rs`.
//! `ResolvedConnection.shim_credential` is the **Phase 2-only** seam that
//! `build_client` reads to get the resolved secret. Phase 3 deletes this
//! field when `build_client` owns HTTP request assembly directly.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use meerkat_core::{
    AuthError, AuthLease, AuthMetadata, AuthProfile, AuthRefreshReason, BackendProfile,
    BindingPolicy, HttpAuthorizer, Provider, ResolvedAuthKind,
};

#[cfg(feature = "anthropic")]
use crate::providers::anthropic::{AnthropicAuthMethod, AnthropicBackendKind};
#[cfg(feature = "gemini")]
use crate::providers::google::{GoogleAuthMethod, GoogleBackendKind};
#[cfg(feature = "openai")]
use crate::providers::openai::{OpenAiAuthMethod, OpenAiBackendKind};

/// Provider-tagged normalized backend kind. Each variant is produced by the
/// matching provider runtime's `validate_binding`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedBackendKind {
    #[cfg(feature = "openai")]
    OpenAi(OpenAiBackendKind),
    #[cfg(feature = "anthropic")]
    Anthropic(AnthropicBackendKind),
    #[cfg(feature = "gemini")]
    Google(GoogleBackendKind),
}

/// Provider-tagged normalized auth method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedAuthMethod {
    #[cfg(feature = "openai")]
    OpenAi(OpenAiAuthMethod),
    #[cfg(feature = "anthropic")]
    Anthropic(AnthropicAuthMethod),
    #[cfg(feature = "gemini")]
    Google(GoogleAuthMethod),
}

/// A binding that has been provider-validated but not yet resolved.
#[derive(Clone)]
pub struct ValidatedBinding {
    pub provider: Provider,
    pub backend: NormalizedBackendKind,
    pub auth: NormalizedAuthMethod,
    pub backend_profile: Arc<BackendProfile>,
    pub auth_profile: Arc<AuthProfile>,
    pub policy: BindingPolicy,
}

impl std::fmt::Debug for ValidatedBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedBinding")
            .field("provider", &self.provider)
            .field("backend", &self.backend)
            .field("auth", &self.auth)
            .field("backend_profile_id", &self.backend_profile.id)
            .field("auth_profile_id", &self.auth_profile.id)
            .finish()
    }
}

// Plan §6.11 deleted the `ShimCredential` enum. Credential material
// lives in the `auth_lease` directly: `StaticHeaders` with a
// `__secret__` entry for simple-secret flows, `DynamicAuthorizer` for
// authorizer-backed flows. `build_client` reads
// `ResolvedConnection::resolved_secret()` / `resolved_authorizer()`.

/// A fully resolved connection carries the trait-object lease alongside
/// backend metadata.
#[derive(Clone)]
pub struct ResolvedConnection {
    pub provider: Provider,
    pub backend: NormalizedBackendKind,
    pub backend_profile: Arc<BackendProfile>,
    pub auth_lease: Arc<dyn AuthLease>,
}

impl std::fmt::Debug for ResolvedConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedConnection")
            .field("provider", &self.provider)
            .field("backend", &self.backend)
            .field("backend_profile_id", &self.backend_profile.id)
            .finish()
    }
}

impl ResolvedConnection {
    /// Extract the resolved secret (api key, bearer token) from the
    /// auth lease via the `"__secret__"` synthetic header convention.
    /// Returns `None` if the lease is a `DynamicAuthorizer`, has no
    /// `__secret__` header, or is empty. Plan §6.11: new build_client
    /// code paths read this instead of `shim_credential`.
    pub fn resolved_secret(&self) -> Option<String> {
        match self.auth_lease.kind() {
            meerkat_core::ResolvedAuthKind::StaticHeaders(headers) => headers
                .iter()
                .find_map(|(k, v)| (k == "__secret__").then(|| v.clone())),
            _ => None,
        }
    }

    /// Extract the resolved dynamic authorizer (AWS SigV4, Google Auth,
    /// Azure AD, ExternalAuthorizer-backed) from the auth lease. Returns
    /// `None` for static-headers leases. Plan §6.11.
    pub fn resolved_authorizer(&self) -> Option<Arc<dyn HttpAuthorizer>> {
        match self.auth_lease.kind() {
            meerkat_core::ResolvedAuthKind::DynamicAuthorizer(auth) => Some(auth.clone()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------
// Lease implementations
// ---------------------------------------------------------------------

/// Static lease holding pre-projected headers + metadata. Used for api_key
/// and static_bearer resolutions.
pub struct StaticLease {
    kind: ResolvedAuthKind,
    metadata: AuthMetadata,
    expires_at: Option<DateTime<Utc>>,
    source_label: String,
}

impl StaticLease {
    pub fn new(
        headers: Vec<(String, String)>,
        metadata: AuthMetadata,
        expires_at: Option<DateTime<Utc>>,
        source_label: impl Into<String>,
    ) -> Self {
        Self {
            kind: ResolvedAuthKind::StaticHeaders(headers),
            metadata,
            expires_at,
            source_label: source_label.into(),
        }
    }

    /// Empty-header variant — used by Phase 2 resolvers that don't yet
    /// project real wire headers.
    pub fn empty(metadata: AuthMetadata, source_label: impl Into<String>) -> Self {
        Self::new(Vec::new(), metadata, None, source_label)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AuthLease for StaticLease {
    fn kind(&self) -> &ResolvedAuthKind {
        &self.kind
    }
    fn metadata(&self) -> &AuthMetadata {
        &self.metadata
    }
    fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
    fn source_label(&self) -> &str {
        &self.source_label
    }
    async fn refresh(&self, _reason: AuthRefreshReason) -> Result<(), AuthError> {
        // StaticLease has no refresh semantics in Phase 2.
        Ok(())
    }
}

/// Dynamic lease wrapping a runtime authorizer. Phase 2 build_client does
/// not accept this shape — authorizer-backed flows use this directly
/// `build_client` returns DynamicAuthorizerNotYetSupportedInShimMode.
pub struct DynamicLease {
    authorizer: Arc<dyn HttpAuthorizer>,
    metadata: AuthMetadata,
    expires_at: Option<DateTime<Utc>>,
    source_label: String,
    kind: ResolvedAuthKind,
}

impl DynamicLease {
    pub fn new(
        authorizer: Arc<dyn HttpAuthorizer>,
        metadata: AuthMetadata,
        expires_at: Option<DateTime<Utc>>,
        source_label: impl Into<String>,
    ) -> Self {
        let kind = ResolvedAuthKind::DynamicAuthorizer(authorizer.clone());
        Self {
            authorizer,
            metadata,
            expires_at,
            source_label: source_label.into(),
            kind,
        }
    }

    pub fn authorizer(&self) -> &Arc<dyn HttpAuthorizer> {
        &self.authorizer
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AuthLease for DynamicLease {
    fn kind(&self) -> &ResolvedAuthKind {
        &self.kind
    }
    fn metadata(&self) -> &AuthMetadata {
        &self.metadata
    }
    fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
    fn source_label(&self) -> &str {
        &self.source_label
    }
    async fn refresh(&self, _reason: AuthRefreshReason) -> Result<(), AuthError> {
        // DynamicLease refresh semantics land in Phase 1.5 (AuthLeaseMachine).
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // Plan §6.11 deleted the §6.11 deletion artifact cleanup.

    #[tokio::test]
    async fn static_lease_satisfies_trait() {
        let lease: Arc<dyn AuthLease> =
            Arc::new(StaticLease::empty(AuthMetadata::default(), "test"));
        assert!(matches!(lease.kind(), ResolvedAuthKind::StaticHeaders(_)));
        assert_eq!(lease.source_label(), "test");
        assert!(lease.refresh(AuthRefreshReason::Manual).await.is_ok());
    }
}
