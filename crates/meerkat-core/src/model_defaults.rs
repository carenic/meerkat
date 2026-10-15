//! Core-owned trait seam for model operational defaults lookup.
//!
//! This trait allows the facade/factory layer to inject model-aware
//! operational defaults (profile-derived call timeouts and stream-inactivity
//! windows) into the agent at build time.
//!
//! The resolver is invoked at **call time** (not build time) so that
//! hot-swapped model/provider identity is reflected in the resolved defaults.

use crate::Provider;
use std::time::Duration;

/// Resolver for model-specific operational defaults.
///
/// Implemented by the facade/factory layer using the injected
/// `ModelCatalog`/`ModelRegistry` profile lookup.
/// Injected into the agent at build time and consulted at each LLM call to resolve
/// profile-derived defaults for the current effective model/provider.
pub trait ModelOperationalDefaultsResolver: Send + Sync {
    /// Return the profile-derived default call timeout for the given model/provider,
    /// or `None` if the model is unknown or has no profiled default.
    ///
    /// The provider is the typed [`Provider`] owner: callers cross the string
    /// boundary once (at the LLM-client seam) rather than re-parsing a provider
    /// string the resolver implementation already needs typed.
    fn call_timeout_for(&self, provider: Provider, model: &str) -> Option<Duration>;

    /// Return the profile-derived stream-inactivity watchdog window for the
    /// given model/provider, or `None` when the model declares none (the
    /// agent then uses its retry policy's window).
    ///
    /// The agent uses a returned window as given. Config load checks the
    /// [`crate::MIN_MODEL_STREAM_INACTIVITY_TIMEOUT`] floor (60 s) only for
    /// the self-hosted models a config declares, so a resolver that serves
    /// other windows, from an injected catalog for example, owns that check:
    /// a shorter window can abort and retry a stream whose answer already
    /// arrived.
    ///
    /// Defaults to `None`, so a resolver that predates per-model windows
    /// keeps the retry policy's window for every model.
    fn stream_inactivity_timeout_for(&self, _provider: Provider, _model: &str) -> Option<Duration> {
        None
    }
}
