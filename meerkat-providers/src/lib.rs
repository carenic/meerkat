//! meerkat-providers — provider-runtime registry, OAuth/auth primitives,
//! and per-provider runtimes (OpenAI, Anthropic, Google) for Meerkat.
//!
//! Deferral §3 extraction (2026-04-18): this crate hosts the typed
//! backend + auth-method matrix (previously `meerkat_client::providers`)
//! plus the resolver/binding/registry runtime (`meerkat_client::runtime`)
//! and the OAuth + TokenStore + authorizer primitives
//! (`meerkat_client::{auth_oauth, auth_store, authorizers}`). Splitting
//! it out of `meerkat-client` keeps the LLM-wire client surface small and
//! gives the auth subsystem a distinct publish unit.
//!
//! `meerkat-client` re-exports the public surface so downstream code that
//! previously imported these types from `meerkat_client::*` continues to
//! work without changes.

#[cfg(target_arch = "wasm32")]
pub mod tokio {
    pub use tokio_with_wasm::alias::*;
}

pub mod providers;
pub mod runtime;

// Token storage + refresh coordination + OAuth helpers + cloud
// authorizers. Non-wasm by construction: filesystem, keyring, and OS
// lockfile primitives are not available in the browser.
#[cfg(not(target_arch = "wasm32"))]
pub mod auth_oauth;
#[cfg(not(target_arch = "wasm32"))]
pub mod auth_store;
#[cfg(not(target_arch = "wasm32"))]
pub mod authorizers;

// Provider-runtime re-exports (Phase 2).
pub use runtime::{
    DynamicLease, ExternalAuthResolverHandle, NormalizedAuthMethod, NormalizedBackendKind,
    ProviderAuthError, ProviderBindingError, ProviderClientError, ProviderRuntime,
    ProviderRuntimeRegistry, ResolvedConnection, ResolverEnvironment, StaticLease,
    ValidatedBinding,
};

#[cfg(feature = "anthropic")]
pub use providers::anthropic::{
    AnthropicAuthMethod, AnthropicBackendKind, AnthropicProviderRuntime,
};
#[cfg(feature = "gemini")]
pub use providers::google::{GoogleAuthMethod, GoogleBackendKind, GoogleProviderRuntime};
#[cfg(feature = "openai")]
pub use providers::openai::{OpenAiAuthMethod, OpenAiBackendKind, OpenAiProviderRuntime};
