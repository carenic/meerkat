//! OpenAI provider runtime.

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

pub use auth::OpenAiAuthMethod;
pub use backend::OpenAiBackendKind;

/// Allowed (backend, auth) combinations for OpenAI. Phase 2 parses all
/// variants but only resolves ApiKey / StaticBearer / ExternalAuthorizer;
/// the ChatGpt combinations return `InteractiveLoginRequired` on resolve
/// and `MissingFeature("openai-chatgpt-auth")` on build.
pub const ALLOWED_BINDINGS: &[(OpenAiBackendKind, OpenAiAuthMethod)] = &[
    (OpenAiBackendKind::OpenAiApi, OpenAiAuthMethod::ApiKey),
    (OpenAiBackendKind::OpenAiApi, OpenAiAuthMethod::StaticBearer),
    (
        OpenAiBackendKind::OpenAiApi,
        OpenAiAuthMethod::ExternalAuthorizer,
    ),
    (
        OpenAiBackendKind::ChatGptBackend,
        OpenAiAuthMethod::ManagedChatGptOauth,
    ),
    (
        OpenAiBackendKind::ChatGptBackend,
        OpenAiAuthMethod::ExternalChatGptTokens,
    ),
];

pub struct OpenAiProviderRuntime;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ProviderRuntime for OpenAiProviderRuntime {
    fn provider_id(&self) -> Provider {
        Provider::OpenAI
    }

    /// **Scaffolding stub.** Returns `ProviderBindingError::ScaffoldingStub`
    /// until L2.8 lands. The T2 integration test fails at the assertion
    /// (not at a panic).
    fn validate_binding(
        &self,
        _backend: &BackendProfile,
        _auth: &AuthProfile,
    ) -> Result<ValidatedBinding, ProviderBindingError> {
        Err(ProviderBindingError::ScaffoldingStub)
    }

    /// **Scaffolding stub.** Returns `ProviderAuthError::ScaffoldingStub`
    /// until L2.7/L2.9.
    async fn resolve_binding(
        &self,
        _binding: &ValidatedBinding,
        _env: &ResolverEnvironment,
    ) -> Result<ResolvedConnection, ProviderAuthError> {
        Err(ProviderAuthError::ScaffoldingStub)
    }

    /// **Scaffolding stub.** Returns `ProviderClientError::ScaffoldingStub`
    /// until L2.9.
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
    fn allowed_bindings_contain_expected_combinations() {
        assert!(
            ALLOWED_BINDINGS.contains(&(OpenAiBackendKind::OpenAiApi, OpenAiAuthMethod::ApiKey))
        );
        assert!(ALLOWED_BINDINGS.contains(&(
            OpenAiBackendKind::ChatGptBackend,
            OpenAiAuthMethod::ManagedChatGptOauth,
        )));
    }

    #[test]
    fn provider_id_is_openai() {
        assert_eq!(OpenAiProviderRuntime.provider_id(), Provider::OpenAI);
    }
}
