//! Anthropic provider runtime.

pub mod auth;
pub mod backend;

use std::sync::Arc;

use async_trait::async_trait;

use meerkat_core::{AuthProfile, BackendProfile, Provider};

use crate::runtime::binding::{ResolvedConnection, ValidatedBinding};
use crate::runtime::errors::{ProviderAuthError, ProviderBindingError, ProviderClientError};
use crate::runtime::provider_runtime::ProviderRuntime;
use crate::runtime::registry::ResolverEnvironment;
use crate::types::LlmClient;

pub use auth::AnthropicAuthMethod;
pub use backend::AnthropicBackendKind;

/// Allowed (backend, auth) combinations for Anthropic.
pub const ALLOWED_BINDINGS: &[(AnthropicBackendKind, AnthropicAuthMethod)] = &[
    (
        AnthropicBackendKind::AnthropicApi,
        AnthropicAuthMethod::ApiKey,
    ),
    (
        AnthropicBackendKind::AnthropicApi,
        AnthropicAuthMethod::StaticBearer,
    ),
    (
        AnthropicBackendKind::AnthropicApi,
        AnthropicAuthMethod::ClaudeAiOauth,
    ),
    (
        AnthropicBackendKind::AnthropicApi,
        AnthropicAuthMethod::OauthToApiKey,
    ),
    (
        AnthropicBackendKind::AnthropicApi,
        AnthropicAuthMethod::ExternalAuthorizer,
    ),
];

pub struct AnthropicProviderRuntime;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ProviderRuntime for AnthropicProviderRuntime {
    fn provider_id(&self) -> Provider {
        Provider::Anthropic
    }

    fn validate_binding(
        &self,
        _backend: &BackendProfile,
        _auth: &AuthProfile,
    ) -> Result<ValidatedBinding, ProviderBindingError> {
        Err(ProviderBindingError::ScaffoldingStub)
    }

    async fn resolve_binding(
        &self,
        _binding: &ValidatedBinding,
        _env: &ResolverEnvironment,
    ) -> Result<ResolvedConnection, ProviderAuthError> {
        Err(ProviderAuthError::ScaffoldingStub)
    }

    fn build_client(
        &self,
        _connection: ResolvedConnection,
    ) -> Result<Arc<dyn LlmClient>, ProviderClientError> {
        Err(ProviderClientError::ScaffoldingStub)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn allowed_bindings_cover_api_key_and_oauth_variants() {
        assert!(ALLOWED_BINDINGS.contains(&(
            AnthropicBackendKind::AnthropicApi,
            AnthropicAuthMethod::ApiKey,
        )));
        assert!(ALLOWED_BINDINGS.contains(&(
            AnthropicBackendKind::AnthropicApi,
            AnthropicAuthMethod::ClaudeAiOauth,
        )));
    }

    #[test]
    fn provider_id_is_anthropic() {
        assert_eq!(AnthropicProviderRuntime.provider_id(), Provider::Anthropic);
    }
}
