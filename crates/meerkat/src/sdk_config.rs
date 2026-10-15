//! SDK-facing configuration store helpers.

use std::path::PathBuf;
use std::sync::Arc;

use meerkat_core::config::ConfigError;
use meerkat_core::{Config, ConfigDelta, ConfigStore, FileConfigStore, MemoryConfigStore};

/// SDK config store wrapper.
///
/// Defaults to an in-memory store. Use `with_path` for explicit persistence.
#[derive(Clone)]
pub struct SdkConfigStore {
    store: Arc<dyn ConfigStore>,
}

impl SdkConfigStore {
    /// Create a default in-memory config store.
    pub fn new() -> Self {
        let mut config = Config::default();
        let _ = config.apply_env_overrides();
        Self {
            store: Arc::new(MemoryConfigStore::new(config, meerkat_models::canonical())),
        }
    }

    /// Create an in-memory store with a specific config.
    pub fn with_config(config: Config) -> Self {
        let mut config = config;
        let _ = config.apply_env_overrides();
        Self {
            store: Arc::new(MemoryConfigStore::new(config, meerkat_models::canonical())),
        }
    }

    /// Create a file-backed store for explicit persistence.
    ///
    /// Its writes keep the document's own `[confinement]` and
    /// `[governance]` sections, but not the realm sections that compose a
    /// realm's ancestors' ones: for a realm's own document, use
    /// [`Self::with_realm_document`].
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(FileConfigStore::new(
                path.into(),
                meerkat_models::canonical(),
            )),
        }
    }

    /// Create a file-backed store for `realm`'s own document at `path`,
    /// composed with the other documents `documents` supplies (its ancestors
    /// and the user document) as a host does at start
    /// ([`FileConfigStore::with_realm_chain`]): a write that would change
    /// the `[confinement]` or `[governance]` the realm composes through its
    /// realm sections is refused.
    pub fn with_realm_document(
        path: impl Into<PathBuf>,
        realm: meerkat_core::connection::RealmId,
        documents: Arc<dyn meerkat_core::RealmConfigSource>,
    ) -> Self {
        Self {
            store: Arc::new(
                FileConfigStore::new(path.into(), meerkat_models::canonical())
                    .with_realm_chain(realm, documents),
            ),
        }
    }

    /// Fetch the current config.
    pub async fn get(&self) -> Result<Config, ConfigError> {
        let mut config = self.store.get().await?;
        let _ = config.apply_env_overrides();
        Ok(config)
    }

    /// Persist the provided config.
    pub async fn set(&self, config: Config) -> Result<(), ConfigError> {
        self.store.set(config).await
    }

    /// Apply a config patch and return the updated config.
    pub async fn patch(&self, delta: ConfigDelta) -> Result<Config, ConfigError> {
        self.store.patch(delta).await
    }

    /// Return the inner store (for advanced integration).
    pub fn inner(&self) -> Arc<dyn ConfigStore> {
        Arc::clone(&self.store)
    }
}

impl Default for SdkConfigStore {
    fn default() -> Self {
        Self::new()
    }
}
