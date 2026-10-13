//! Realm-configured connector OAuth clients.
//!
//! A realm declares its connector clients in its config document
//! (`[realm.<realm>.connector_client.<profile>]`). This module is the one
//! loader that turns the realm's effective configuration into the connector
//! owner's client-auth profiles, and the source every host installs so the
//! owner reads the current profiles at each login and refresh.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use meerkat_core::connection::{RealmChain, RealmId};
use meerkat_core::{Config, ConnectorClientAuthMethod, ConnectorClientConfig};
use meerkat_providers::connector_login::{
    ConnectorClientAuthProfile, ConnectorClientAuthSource, ConnectorClientSecret,
    ConnectorLoginError, clear_connector_client_secret, load_connector_client_secret,
    save_connector_client_secret,
};

/// The connector client-auth profiles `realm` declares in its effective
/// (realm-chain composed) `config`. The nearest realm defining an id wins.
/// Each profile is validated as the connector owner would.
pub fn connector_client_auth_profiles(
    config: &Config,
    realm: &RealmId,
) -> Result<Vec<ConnectorClientAuthProfile>, ConnectorLoginError> {
    let chain = RealmChain::resolve(config, realm)
        .map_err(|error| ConnectorLoginError::ClientConfig(error.to_string()))?;
    let mut profiles = BTreeMap::new();
    for owner in chain.realms() {
        let Some(section) = config.realm.get(owner.as_str()) else {
            continue;
        };
        for (id, client) in &section.connector_client {
            if profiles.contains_key(id) {
                continue;
            }
            let profile = connector_client_profile(owner, id, client)?;
            profile.validate().map_err(|_| {
                ConnectorLoginError::ClientConfig(format!(
                    "connector client '{id}' in realm '{owner}' is invalid"
                ))
            })?;
            profiles.insert(id.clone(), profile);
        }
    }
    Ok(profiles.into_values().collect())
}

fn connector_client_profile(
    owner: &RealmId,
    id: &str,
    client: &ConnectorClientConfig,
) -> Result<ConnectorClientAuthProfile, ConnectorLoginError> {
    // The secret belongs to the realm defining the client, so a child realm
    // inheriting it uses the same secret.
    let client_secret = match client.client_auth {
        ConnectorClientAuthMethod::None => None,
        ConnectorClientAuthMethod::ClientSecretPost => Some(ConnectorClientSecret::RealmStore {
            realm: owner.clone(),
        }),
    };
    Ok(ConnectorClientAuthProfile {
        id: id.to_owned(),
        issuer: client.issuer.clone(),
        client: client.client.clone(),
        client_secret,
        authorize_params: client.authorize_params.clone(),
        resource_indicator: client.resource_indicator,
        protocol: client.protocol.clone(),
        registered_redirect_uris: client.registered_redirect_uris.clone(),
    })
}

type EffectiveRealmConfig =
    Arc<dyn Fn() -> BoxFuture<'static, Result<Option<(Config, RealmId)>, String>> + Send + Sync>;

/// The realm's connector clients as a [`ConnectorClientAuthSource`]: each
/// read composes the realm's current effective configuration, so profiles
/// added or changed after the host was built apply on the next use.
#[derive(Clone)]
pub struct RealmConnectorClients {
    effective: EffectiveRealmConfig,
}

impl RealmConnectorClients {
    /// Read `realm`'s effective configuration from `source` (its own document
    /// folded over its parent chain).
    pub fn from_realm_source(
        source: Arc<dyn meerkat_core::RealmConfigSource>,
        realm: RealmId,
    ) -> Self {
        Self {
            effective: Arc::new(move || {
                let reader = meerkat_core::EffectiveConfigReader::new(Arc::clone(&source));
                let realm = realm.clone();
                Box::pin(async move {
                    let config = reader
                        .effective_config(&realm)
                        .await
                        .map_err(|error| error.to_string())?;
                    Ok(Some((config, realm)))
                })
            }),
        }
    }

    /// Read the effective configuration through `read`, which yields the
    /// composed config and its head realm, or `None` when the host serves no
    /// realm. Only the MCP auth-resolver install path builds one this way.
    #[cfg(feature = "mcp")]
    pub(crate) fn from_effective_config(
        read: impl Fn() -> BoxFuture<'static, Result<Option<(Config, RealmId)>, String>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            effective: Arc::new(read),
        }
    }
}

#[async_trait::async_trait]
impl ConnectorClientAuthSource for RealmConnectorClients {
    async fn current_profiles(
        &self,
    ) -> Result<Vec<ConnectorClientAuthProfile>, ConnectorLoginError> {
        match (self.effective)()
            .await
            .map_err(ConnectorLoginError::ClientConfig)?
        {
            Some((config, realm)) => connector_client_auth_profiles(&config, &realm),
            None => Ok(Vec::new()),
        }
    }
}

/// How [`put_connector_client`] treats the client's secret.
pub enum ConnectorClientSecretUpdate {
    /// Keep the stored secret. A client that sends a secret must already
    /// have one.
    Keep,
    /// Store this secret (replacing any stored one).
    Set(String),
}

/// Why a connector client could not be written, or how far it got.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorClientUpdateError {
    #[error("connector client '{id}' is invalid: {reason}")]
    Invalid { id: String, reason: String },
    #[error(
        "connector client '{0}' sends a client secret but none is stored for its exact definition; supply one"
    )]
    SecretRequired(String),
    #[error("connector client '{0}' sends no client secret")]
    SecretNotUsed(String),
    #[error("connector client '{id}' is not configured in realm '{realm}'")]
    Unknown { id: String, realm: RealmId },
    #[error(transparent)]
    Config(#[from] meerkat_core::ConfigError),
    #[error(transparent)]
    Store(#[from] ConnectorLoginError),
    /// The profile's update ownership could not be acquired; the update did
    /// not start.
    #[error("connector client update ownership is unavailable: {0}")]
    Ownership(String),
    /// The coordinator lost the update's outcome (for example it reports
    /// cancellation). The update may have run, fully or in part.
    #[error("connector client update outcome is unknown: {0}")]
    OutcomeUnknown(String),
    /// The secret for this exact definition was stored, then the
    /// configuration write failed. An existing definition with the same
    /// meaning may already use that secret, and the store error does not say
    /// whether the configuration was written.
    #[error(
        "connector client '{id}': the secret was stored but the configuration write failed and may not have been applied: {error}"
    )]
    ConfigFailedAfterSecretStored {
        id: String,
        error: meerkat_core::ConfigError,
    },
    /// The configuration was written, then removing the secret it no longer
    /// uses failed: the configuration is in effect and that unused secret
    /// remains stored.
    #[error(
        "connector client '{id}': the configuration was written but its unused secret could not be removed: {error}"
    )]
    SecretCleanupFailed {
        id: String,
        error: ConnectorLoginError,
    },
}

/// Run one update of `realm`'s connector client `id` under the persistence's
/// existing exclusive-mutation coordinator, keyed by the profile id. Every
/// put, rotation and removal of that profile (in this or another process
/// sharing the coordinator) runs one at a time, so an older update can never
/// clear a secret a newer one published.
async fn with_profile_update(
    persistence: &meerkat_providers::auth_store::ProviderAuthPersistence,
    realm: &RealmId,
    id: &str,
    update: impl std::future::Future<Output = Result<(), ConnectorClientUpdateError>> + Send + 'static,
) -> Result<(), ConnectorClientUpdateError> {
    use meerkat_core::auth::{CredentialMutationFn, CredentialMutationOutcome};
    let key = meerkat_core::auth::TokenKey::connector_client_update(realm.clone(), id).map_err(
        |error| ConnectorClientUpdateError::Invalid {
            id: id.to_owned(),
            reason: error.to_string(),
        },
    )?;
    let outcome = Arc::new(std::sync::Mutex::new(None));
    let slot = Arc::clone(&outcome);
    let mutation: CredentialMutationFn = Box::new(move || {
        Box::pin(async move {
            let result = update.await;
            *slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
            Ok(CredentialMutationOutcome::Cleared)
        })
    });
    let admitted = persistence
        .refresh_coordinator()
        .with_exclusive_mutation(key, mutation)
        .await;
    let result = outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    match (admitted, result) {
        // The update ran to its own typed result.
        (_, Some(result)) => result,
        // Ownership was never acquired: the update did not start.
        (Err(meerkat_core::auth::CredentialMutationError::LockFailed(error)), None) => {
            Err(ConnectorClientUpdateError::Ownership(error))
        }
        // Admission was refused while earlier work for the key still runs.
        (
            Err(error @ meerkat_core::auth::CredentialMutationError::CredentialAccess { .. }),
            None,
        ) => Err(ConnectorClientUpdateError::Ownership(error.to_string())),
        // Anything else (including cancellation) lost the outcome.
        (Err(error), None) => Err(ConnectorClientUpdateError::OutcomeUnknown(
            error.to_string(),
        )),
        (Ok(_), None) => Err(ConnectorClientUpdateError::OutcomeUnknown(
            "the coordinator returned without the update's result".to_owned(),
        )),
    }
}

/// The non-secret profile binding of `realm`'s client `id` as defined.
fn client_binding(
    realm: &RealmId,
    id: &str,
    client: &ConnectorClientConfig,
) -> Result<String, ConnectorClientUpdateError> {
    let profile = connector_client_profile(realm, id, client).map_err(|error| {
        ConnectorClientUpdateError::Invalid {
            id: id.to_owned(),
            reason: error.to_string(),
        }
    })?;
    profile
        .validate()
        .map_err(|error| ConnectorClientUpdateError::Invalid {
            id: id.to_owned(),
            reason: error.to_string(),
        })?;
    Ok(profile.binding())
}

/// The merge patch writing `client` (or removing it, for `None`) as `realm`'s
/// connector client `id`. Every owned field is written explicitly, so the
/// result is exactly `client` whatever definition was there before.
fn connector_client_patch(
    realm: &RealmId,
    id: &str,
    client: Option<&ConnectorClientConfig>,
) -> Result<meerkat_core::ConfigDelta, ConnectorClientUpdateError> {
    let value = match client {
        Some(client) => {
            let mut value = serde_json::to_value(client).map_err(|error| {
                ConnectorClientUpdateError::Invalid {
                    id: id.to_owned(),
                    reason: error.to_string(),
                }
            })?;
            if let Some(fields) = value.as_object_mut() {
                fields
                    .entry("authorize_params")
                    .or_insert_with(|| serde_json::json!([]));
                fields
                    .entry("registered_redirect_uris")
                    .or_insert_with(|| serde_json::json!([]));
                fields.entry("protocol").or_insert(serde_json::Value::Null);
            }
            value
        }
        None => serde_json::Value::Null,
    };
    Ok(meerkat_core::ConfigDelta(serde_json::json!({
        "realm": { realm.as_str(): { "connector_client": { id: value } } }
    })))
}

/// Configure `realm`'s connector client `id`, writing the client to the
/// realm's own config document (`config_store`, never a composed view) and
/// its secret to the realm credential store.
///
/// A stored secret is bound to the client's exact non-secret meaning
/// (issuer, client, mode, parameters, protocol, redirects): `Keep` keeps only
/// a secret stored for this exact definition, so a retargeted client never
/// inherits the previous client's secret. A new secret is stored before the
/// configuration is written, and a secret the new definition no longer uses
/// is removed after. The two stores are written separately, not atomically:
/// if the process stops in between, a configured client without its exact
/// secret refuses each login and refresh with
/// [`ConnectorLoginError::ClientSecretUnavailable`], and a failure after the
/// first write is reported as the partial outcome it is.
pub async fn put_connector_client(
    config_store: Arc<dyn meerkat_core::ConfigStore>,
    persistence: &meerkat_providers::auth_store::ProviderAuthPersistence,
    realm: &RealmId,
    id: &str,
    client: ConnectorClientConfig,
    secret: ConnectorClientSecretUpdate,
) -> Result<(), ConnectorClientUpdateError> {
    let token_store = persistence.token_store();
    let (realm_owned, id_owned) = (realm.clone(), id.to_owned());
    with_profile_update(persistence, realm, id, async move {
        put_unowned(
            config_store.as_ref(),
            token_store.as_ref(),
            &realm_owned,
            &id_owned,
            client,
            secret,
        )
        .await
    })
    .await
}

async fn put_unowned(
    config_store: &dyn meerkat_core::ConfigStore,
    token_store: &dyn meerkat_core::auth::TokenStore,
    realm: &RealmId,
    id: &str,
    client: ConnectorClientConfig,
    secret: ConnectorClientSecretUpdate,
) -> Result<(), ConnectorClientUpdateError> {
    let binding = client_binding(realm, id, &client)?;
    let sends_secret = client.client_auth == ConnectorClientAuthMethod::ClientSecretPost;
    let own = config_store.get().await?;
    let previous = own
        .realm
        .get(realm.as_str())
        .and_then(|section| section.connector_client.get(id))
        .cloned();
    let previous_binding = previous
        .as_ref()
        .filter(|previous| previous.client_auth == ConnectorClientAuthMethod::ClientSecretPost)
        .map(|previous| client_binding(realm, id, previous))
        .transpose()?;
    let mut stored_new_secret = false;
    match (&secret, sends_secret) {
        (ConnectorClientSecretUpdate::Set(value), true) => {
            save_connector_client_secret(token_store, realm, &binding, value).await?;
            stored_new_secret = true;
        }
        (ConnectorClientSecretUpdate::Keep, true) => {
            if load_connector_client_secret(token_store, realm, &binding)
                .await?
                .is_none()
            {
                return Err(ConnectorClientUpdateError::SecretRequired(id.to_owned()));
            }
        }
        (ConnectorClientSecretUpdate::Set(_), false) => {
            return Err(ConnectorClientUpdateError::SecretNotUsed(id.to_owned()));
        }
        (ConnectorClientSecretUpdate::Keep, false) => {}
    }
    let patch = connector_client_patch(realm, id, Some(&client))?;
    if let Err(error) = config_store.patch(patch).await {
        return Err(if stored_new_secret {
            ConnectorClientUpdateError::ConfigFailedAfterSecretStored {
                id: id.to_owned(),
                error,
            }
        } else {
            error.into()
        });
    }
    if let Some(previous_binding) = previous_binding
        && (!sends_secret || previous_binding != binding)
    {
        clear_unreferenced_secret(config_store, token_store, realm, id, &previous_binding).await?;
    }
    Ok(())
}

/// Remove the secret stored for `realm`'s client binding `binding` unless the
/// realm's own configuration names that exact client again (a concurrent
/// writer may have restored it since this update read the configuration).
async fn clear_unreferenced_secret(
    config_store: &dyn meerkat_core::ConfigStore,
    token_store: &dyn meerkat_core::auth::TokenStore,
    realm: &RealmId,
    id: &str,
    binding: &str,
) -> Result<(), ConnectorClientUpdateError> {
    let cleanup_failed = |error| ConnectorClientUpdateError::SecretCleanupFailed {
        id: id.to_owned(),
        error,
    };
    let current = config_store
        .get()
        .await
        .map_err(|error| cleanup_failed(ConnectorLoginError::ClientConfig(error.to_string())))?;
    let still_named = current
        .realm
        .get(realm.as_str())
        .and_then(|section| section.connector_client.get(id))
        .filter(|client| client.client_auth == ConnectorClientAuthMethod::ClientSecretPost)
        .is_some_and(|client| {
            client_binding(realm, id, client).is_ok_and(|current| current == binding)
        });
    if still_named {
        return Ok(());
    }
    clear_connector_client_secret(token_store, realm, binding)
        .await
        .map_err(cleanup_failed)
}

/// Replace the stored secret of `realm`'s connector client `id`, which must be
/// defined in the realm's own config document and send a secret. The secret
/// is stored for the client's current exact definition; the next code
/// exchange or refresh uses it, and credentials obtained under the old one
/// stay valid.
pub async fn rotate_connector_client_secret(
    config_store: Arc<dyn meerkat_core::ConfigStore>,
    persistence: &meerkat_providers::auth_store::ProviderAuthPersistence,
    realm: &RealmId,
    id: &str,
    secret: &str,
) -> Result<(), ConnectorClientUpdateError> {
    let token_store = persistence.token_store();
    let (realm_owned, id_owned, secret) = (realm.clone(), id.to_owned(), secret.to_owned());
    with_profile_update(persistence, realm, id, async move {
        rotate_unowned(
            config_store.as_ref(),
            token_store.as_ref(),
            &realm_owned,
            &id_owned,
            &secret,
        )
        .await
    })
    .await
}

async fn rotate_unowned(
    config_store: &dyn meerkat_core::ConfigStore,
    token_store: &dyn meerkat_core::auth::TokenStore,
    realm: &RealmId,
    id: &str,
    secret: &str,
) -> Result<(), ConnectorClientUpdateError> {
    let own = config_store.get().await?;
    let client = own
        .realm
        .get(realm.as_str())
        .and_then(|section| section.connector_client.get(id))
        .ok_or_else(|| ConnectorClientUpdateError::Unknown {
            id: id.to_owned(),
            realm: realm.clone(),
        })?;
    if client.client_auth != ConnectorClientAuthMethod::ClientSecretPost {
        return Err(ConnectorClientUpdateError::SecretNotUsed(id.to_owned()));
    }
    let binding = client_binding(realm, id, client)?;
    save_connector_client_secret(token_store, realm, &binding, secret).await?;
    Ok(())
}

/// Remove `realm`'s connector client `id` from its own config document, then
/// its stored secret. Credentials minted under it stop refreshing.
pub async fn remove_connector_client(
    config_store: Arc<dyn meerkat_core::ConfigStore>,
    persistence: &meerkat_providers::auth_store::ProviderAuthPersistence,
    realm: &RealmId,
    id: &str,
) -> Result<(), ConnectorClientUpdateError> {
    let token_store = persistence.token_store();
    let (realm_owned, id_owned) = (realm.clone(), id.to_owned());
    with_profile_update(persistence, realm, id, async move {
        remove_unowned(
            config_store.as_ref(),
            token_store.as_ref(),
            &realm_owned,
            &id_owned,
        )
        .await
    })
    .await
}

async fn remove_unowned(
    config_store: &dyn meerkat_core::ConfigStore,
    token_store: &dyn meerkat_core::auth::TokenStore,
    realm: &RealmId,
    id: &str,
) -> Result<(), ConnectorClientUpdateError> {
    let own = config_store.get().await?;
    let previous_binding = own
        .realm
        .get(realm.as_str())
        .and_then(|section| section.connector_client.get(id))
        .filter(|previous| previous.client_auth == ConnectorClientAuthMethod::ClientSecretPost)
        .map(|previous| client_binding(realm, id, previous))
        .transpose()?;
    config_store
        .patch(connector_client_patch(realm, id, None)?)
        .await?;
    if let Some(previous_binding) = previous_binding {
        clear_unreferenced_secret(config_store, token_store, realm, id, &previous_binding).await?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    const DOCUMENT: &str = r#"
[realm.global.connector_client.shared]
issuer = "https://issuer.global.example"
client = "global-client"
authorize_params = [["access_type", "offline"], ["prompt", "consent"]]

[realm.global.connector_client.overridden]
issuer = "https://issuer.global.example"
client = "global-client"

[realm.child.connector_client.overridden]
issuer = "https://issuer.child.example"
client = "child-client"
resource_indicator = false
protocol = "slack"
registered_redirect_uris = ["https://host.example/callback"]
"#;

    fn config(document: &str) -> Config {
        Config::from_persisted_toml(document)
            .expect("config parses")
            .0
    }

    fn realm(id: &str) -> RealmId {
        RealmId::parse(id).expect("realm id")
    }

    #[test]
    fn the_nearest_realm_defining_a_client_wins_and_fields_map_in_order() {
        let profiles =
            connector_client_auth_profiles(&config(DOCUMENT), &realm("child")).expect("profiles");
        let by_id: BTreeMap<_, _> = profiles
            .iter()
            .map(|profile| (profile.id.as_str(), profile))
            .collect();
        assert_eq!(by_id.len(), 2);
        let overridden = by_id["overridden"];
        assert_eq!(overridden.issuer, "https://issuer.child.example");
        assert_eq!(overridden.client, "child-client");
        assert!(!overridden.resource_indicator);
        assert_eq!(overridden.protocol.as_deref(), Some("slack"));
        assert_eq!(
            overridden.registered_redirect_uris,
            vec!["https://host.example/callback".to_owned()]
        );
        let shared = by_id["shared"];
        assert!(shared.resource_indicator, "the default sends the resource");
        assert!(shared.client_secret.is_none());
        assert_eq!(
            shared.authorize_params,
            vec![
                ("access_type".to_owned(), "offline".to_owned()),
                ("prompt".to_owned(), "consent".to_owned()),
            ]
        );
        let global = connector_client_auth_profiles(&config(DOCUMENT), &RealmId::global())
            .expect("global profiles");
        assert!(global.iter().any(|profile| profile.id == "overridden"
            && profile.issuer == "https://issuer.global.example"));
    }

    #[test]
    fn a_secret_client_reads_the_defining_realms_store() {
        let document = r#"
[realm.global.connector_client.desktop]
issuer = "https://issuer.example"
client = "client"
client_auth = "client_secret_post"
"#;
        let profiles =
            connector_client_auth_profiles(&config(document), &realm("child")).expect("profiles");
        assert_eq!(
            profiles[0].client_secret,
            Some(ConnectorClientSecret::RealmStore {
                realm: RealmId::global()
            }),
            "an inherited client uses its defining realm's secret"
        );
    }

    #[test]
    fn an_invalid_client_is_a_configuration_error_naming_it() {
        let document = r#"
[realm.global.connector_client.broken]
issuer = "https://issuer.example"
client = "client"
authorize_params = [["client_secret", "smuggled"]]
"#;
        let error = connector_client_auth_profiles(&config(document), &RealmId::global())
            .expect_err("reserved parameter");
        let ConnectorLoginError::ClientConfig(detail) = error else {
            panic!("expected a configuration error: {error:?}");
        };
        assert!(detail.contains("broken"), "{detail}");
        assert!(!detail.contains("smuggled"), "{detail}");
    }

    #[test]
    fn an_unknown_client_field_is_refused_at_load() {
        let document = r#"
[realm.global.connector_client.typo]
issuer = "https://issuer.example"
client = "client"
client_secret = "never-in-config"
"#;
        assert!(Config::from_persisted_toml(document).is_err());
    }

    #[tokio::test]
    async fn put_rotate_and_remove_keep_config_and_secret_consistent() {
        use meerkat_core::ConfigStore;
        use meerkat_providers::auth_store::{EphemeralTokenStore, TokenStore};
        let store = Arc::new(meerkat_core::MemoryConfigStore::new(
            Config::default(),
            meerkat_models::canonical(),
        ));
        let secrets = Arc::new(EphemeralTokenStore::new());
        let persistence = meerkat_providers::auth_store::ProviderAuthPersistence::new(
            Arc::clone(&secrets) as Arc<dyn TokenStore>,
            Arc::new(meerkat_providers::auth_store::InMemoryCoordinator::new()),
        );
        let realm = RealmId::global();
        let client = |client_auth, protocol: Option<&str>| ConnectorClientConfig {
            issuer: "https://issuer.example".into(),
            client: "client".into(),
            client_auth,
            authorize_params: vec![("prompt".into(), "consent".into())],
            resource_indicator: true,
            protocol: protocol.map(str::to_owned),
            registered_redirect_uris: Vec::new(),
        };
        // The secret the current definition of "desktop" reads, if any.
        async fn stored(
            store: &meerkat_core::MemoryConfigStore,
            secrets: &EphemeralTokenStore,
        ) -> Option<String> {
            let profiles =
                connector_client_auth_profiles(&store.get().await.unwrap(), &RealmId::global())
                    .unwrap();
            let profile = profiles.iter().find(|profile| profile.id == "desktop")?;
            let Some(ConnectorClientSecret::RealmStore { realm }) = &profile.client_secret else {
                return None;
            };
            load_connector_client_secret(secrets, realm, &profile.binding())
                .await
                .unwrap()
        }
        async fn stored_secret_rows(secrets: &EphemeralTokenStore) -> usize {
            secrets
                .list()
                .await
                .unwrap()
                .iter()
                .filter(|key| key.is_connector_client_secret())
                .count()
        }

        // A client sending a secret needs one: nothing is written.
        let refused = put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            client(ConnectorClientAuthMethod::ClientSecretPost, None),
            ConnectorClientSecretUpdate::Keep,
        )
        .await;
        assert!(matches!(
            refused,
            Err(ConnectorClientUpdateError::SecretRequired(_))
        ));
        assert!(
            connector_client_auth_profiles(&store.get().await.unwrap(), &realm)
                .unwrap()
                .is_empty()
        );

        put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            client(ConnectorClientAuthMethod::ClientSecretPost, Some("slack")),
            ConnectorClientSecretUpdate::Set("secret-one".into()),
        )
        .await
        .unwrap();
        let config = store.get().await.unwrap();
        let profiles = connector_client_auth_profiles(&config, &realm).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].protocol.as_deref(), Some("slack"));
        assert!(
            !toml::to_string(&config).unwrap().contains("secret-one"),
            "the secret never reaches the config document"
        );
        assert_eq!(
            stored(&store, &secrets).await,
            Some("secret-one".to_owned())
        );

        rotate_connector_client_secret(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            "secret-two",
        )
        .await
        .unwrap();
        assert_eq!(
            stored(&store, &secrets).await,
            Some("secret-two".to_owned())
        );

        // Retargeted to another client with Keep: the previous client's
        // secret is not reused, and nothing changes.
        let retargeted = ConnectorClientConfig {
            client: "replacement-client".into(),
            ..client(ConnectorClientAuthMethod::ClientSecretPost, Some("slack"))
        };
        assert!(matches!(
            put_connector_client(
                store.clone(),
                &persistence,
                &realm,
                "desktop",
                retargeted.clone(),
                ConnectorClientSecretUpdate::Keep,
            )
            .await,
            Err(ConnectorClientUpdateError::SecretRequired(_))
        ));
        assert_eq!(
            stored(&store, &secrets).await,
            Some("secret-two".to_owned())
        );
        // Retargeted with its own secret: the old client's secret is removed.
        put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            retargeted,
            ConnectorClientSecretUpdate::Set("replacement-secret".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            stored(&store, &secrets).await,
            Some("replacement-secret".to_owned())
        );
        assert_eq!(stored_secret_rows(&secrets).await, 1);

        // Redefined without a protocol or a secret: the old values go.
        put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            client(ConnectorClientAuthMethod::None, None),
            ConnectorClientSecretUpdate::Keep,
        )
        .await
        .unwrap();
        let profiles = connector_client_auth_profiles(&store.get().await.unwrap(), &realm).unwrap();
        assert_eq!(profiles[0].protocol, None);
        assert!(profiles[0].client_secret.is_none());
        assert!(stored(&store, &secrets).await.is_none());
        assert_eq!(stored_secret_rows(&secrets).await, 0);
        assert!(matches!(
            rotate_connector_client_secret(store.clone(), &persistence, &realm, "desktop", "x")
                .await,
            Err(ConnectorClientUpdateError::SecretNotUsed(_))
        ));

        remove_connector_client(store.clone(), &persistence, &realm, "desktop")
            .await
            .unwrap();
        assert!(
            connector_client_auth_profiles(&store.get().await.unwrap(), &realm)
                .unwrap()
                .is_empty()
        );
    }

    /// A token store whose first clear waits until released, so an older
    /// update can be held between publishing its config and clearing.
    struct HeldClearStore {
        inner: meerkat_providers::auth_store::EphemeralTokenStore,
        saves: std::sync::atomic::AtomicUsize,
        clears_done: std::sync::atomic::AtomicUsize,
        entered: tokio::sync::Notify,
        release: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    }

    #[async_trait::async_trait]
    impl meerkat_providers::auth_store::TokenStore for HeldClearStore {
        async fn load(
            &self,
            key: &meerkat_core::auth::TokenKey,
        ) -> Result<Option<meerkat_core::auth::PersistedTokens>, meerkat_core::auth::TokenStoreError>
        {
            self.inner.load(key).await
        }
        async fn save(
            &self,
            key: &meerkat_core::auth::TokenKey,
            tokens: &meerkat_core::auth::PersistedTokens,
        ) -> Result<(), meerkat_core::auth::TokenStoreError> {
            self.saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.save(key, tokens).await
        }
        async fn clear(
            &self,
            key: &meerkat_core::auth::TokenKey,
        ) -> Result<(), meerkat_core::auth::TokenStoreError> {
            let held = self.release.lock().await.take();
            if let Some(release) = held {
                self.entered.notify_one();
                let _ = release.await;
            }
            let cleared = self.inner.clear(key).await;
            self.clears_done
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            cleared
        }
        async fn list(
            &self,
        ) -> Result<Vec<meerkat_core::auth::TokenKey>, meerkat_core::auth::TokenStoreError>
        {
            self.inner.list().await
        }
        fn backend_name(&self) -> &'static str {
            "held-clear"
        }
    }

    /// The in-memory coordinator, reporting each exclusive-mutation request
    /// before it waits for admission.
    struct SignalingCoordinator {
        inner: meerkat_providers::auth_store::InMemoryCoordinator,
        requested: tokio::sync::mpsc::UnboundedSender<()>,
    }

    #[async_trait::async_trait]
    impl meerkat_core::auth::RefreshCoordinator for SignalingCoordinator {
        async fn with_exclusive_mutation(
            &self,
            key: meerkat_core::auth::TokenKey,
            mutation_fn: meerkat_core::auth::CredentialMutationFn,
        ) -> Result<
            meerkat_core::auth::CredentialMutationOutcome,
            meerkat_core::auth::CredentialMutationError,
        > {
            let _ = self.requested.send(());
            self.inner.with_exclusive_mutation(key, mutation_fn).await
        }

        async fn with_refresh(
            &self,
            key: meerkat_core::auth::TokenKey,
            refresh_fn: meerkat_core::auth::RefreshFn,
        ) -> Result<meerkat_core::auth::PersistedTokens, meerkat_core::auth::RefreshError> {
            self.inner.with_refresh(key, refresh_fn).await
        }
    }

    #[tokio::test]
    async fn an_older_held_clear_never_removes_a_newer_updates_secret() {
        use meerkat_core::ConfigStore;
        use meerkat_providers::auth_store::{EphemeralTokenStore, TokenStore};
        let store = Arc::new(meerkat_core::MemoryConfigStore::new(
            Config::default(),
            meerkat_models::canonical(),
        ));
        let (release, held) = tokio::sync::oneshot::channel();
        let secrets = Arc::new(HeldClearStore {
            inner: EphemeralTokenStore::new(),
            saves: std::sync::atomic::AtomicUsize::new(0),
            clears_done: std::sync::atomic::AtomicUsize::new(0),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Mutex::new(None),
        });
        let (requested, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let persistence = meerkat_providers::auth_store::ProviderAuthPersistence::new(
            Arc::clone(&secrets) as Arc<dyn TokenStore>,
            Arc::new(SignalingCoordinator {
                inner: meerkat_providers::auth_store::InMemoryCoordinator::new(),
                requested,
            }),
        );
        let realm = RealmId::global();
        let secret_client = ConnectorClientConfig {
            issuer: "https://issuer.example".into(),
            client: "client".into(),
            client_auth: ConnectorClientAuthMethod::ClientSecretPost,
            authorize_params: Vec::new(),
            resource_indicator: true,
            protocol: None,
            registered_redirect_uris: Vec::new(),
        };
        let public_client = ConnectorClientConfig {
            client_auth: ConnectorClientAuthMethod::None,
            ..secret_client.clone()
        };
        put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            secret_client.clone(),
            ConnectorClientSecretUpdate::Set("secret-one".into()),
        )
        .await
        .unwrap();

        while requests.try_recv().is_ok() {}
        // Older update: switch to a public client; it publishes its config
        // and is then held before clearing the old secret.
        *secrets.release.lock().await = Some(held);
        let older = tokio::spawn({
            let (store, persistence, realm) = (store.clone(), persistence.clone(), realm.clone());
            async move {
                put_connector_client(
                    store,
                    &persistence,
                    &realm,
                    "desktop",
                    public_client,
                    ConnectorClientSecretUpdate::Keep,
                )
                .await
            }
        });
        secrets.entered.notified().await;
        requests
            .recv()
            .await
            .expect("the older update requested ownership");
        // Its caller leaves; the admitted update still completes in order.
        older.abort();
        assert!(
            older.await.is_err_and(|error| error.is_cancelled()),
            "the older caller is gone"
        );
        let clears_before = secrets
            .clears_done
            .load(std::sync::atomic::Ordering::SeqCst);
        let saves_before = secrets.saves.load(std::sync::atomic::Ordering::SeqCst);
        // Newer update: back to the secret client with a new secret.
        let newer = tokio::spawn({
            let (store, persistence, realm) = (store.clone(), persistence.clone(), realm.clone());
            async move {
                put_connector_client(
                    store,
                    &persistence,
                    &realm,
                    "desktop",
                    secret_client,
                    ConnectorClientSecretUpdate::Set("secret-two".into()),
                )
                .await
            }
        });
        // The newer update has reached the coordinator. The signal does not
        // prove it is polling the profile mutex; what is asserted is that,
        // while the older cleanup is held, it has written nothing and not
        // finished, and that it completes only after the cleanup does.
        requests
            .recv()
            .await
            .expect("the newer update requested ownership");
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert!(!newer.is_finished(), "the newer update has not completed");
        assert_eq!(
            secrets.saves.load(std::sync::atomic::Ordering::SeqCst),
            saves_before,
            "the newer update saved nothing while the cleanup was held"
        );
        assert!(release.send(()).is_ok(), "the held clear is released");
        newer.await.unwrap().unwrap();
        assert_eq!(
            secrets
                .clears_done
                .load(std::sync::atomic::Ordering::SeqCst),
            clears_before + 1,
            "the held cleanup completed its clear"
        );

        let profiles = connector_client_auth_profiles(&store.get().await.unwrap(), &realm).unwrap();
        assert_eq!(
            load_connector_client_secret(secrets.as_ref(), &realm, &profiles[0].binding())
                .await
                .unwrap(),
            Some("secret-two".to_owned()),
            "the last successful update keeps its own secret"
        );
    }

    #[tokio::test]
    async fn an_update_clears_previously_non_empty_lists() {
        use meerkat_core::ConfigStore;
        use meerkat_providers::auth_store::{EphemeralTokenStore, TokenStore};
        let store = Arc::new(meerkat_core::MemoryConfigStore::new(
            Config::default(),
            meerkat_models::canonical(),
        ));
        let persistence = meerkat_providers::auth_store::ProviderAuthPersistence::new(
            Arc::new(EphemeralTokenStore::new()) as Arc<dyn TokenStore>,
            Arc::new(meerkat_providers::auth_store::InMemoryCoordinator::new()),
        );
        let realm = RealmId::global();
        let full = ConnectorClientConfig {
            issuer: "https://issuer.example".into(),
            client: "client".into(),
            client_auth: ConnectorClientAuthMethod::None,
            authorize_params: vec![
                ("access_type".into(), "offline".into()),
                ("prompt".into(), "consent".into()),
            ],
            resource_indicator: true,
            protocol: None,
            registered_redirect_uris: vec!["https://host.example/callback".into()],
        };
        put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            full.clone(),
            ConnectorClientSecretUpdate::Keep,
        )
        .await
        .unwrap();
        let profiles = connector_client_auth_profiles(&store.get().await.unwrap(), &realm).unwrap();
        assert_eq!(profiles[0].authorize_params.len(), 2);
        assert_eq!(profiles[0].registered_redirect_uris.len(), 1);

        put_connector_client(
            store.clone(),
            &persistence,
            &realm,
            "desktop",
            ConnectorClientConfig {
                authorize_params: Vec::new(),
                registered_redirect_uris: Vec::new(),
                ..full
            },
            ConnectorClientSecretUpdate::Keep,
        )
        .await
        .unwrap();
        let profiles = connector_client_auth_profiles(&store.get().await.unwrap(), &realm).unwrap();
        assert!(profiles[0].authorize_params.is_empty(), "{:?}", profiles[0]);
        assert!(
            profiles[0].registered_redirect_uris.is_empty(),
            "{:?}",
            profiles[0]
        );
    }

    /// Replacing a profile with empty lists and no protocol clears what the
    /// stored profile had, in both config stores, whether that profile was
    /// written through this API or directly in the document: the patch is
    /// complete on its own, not shaped by a prior read.
    #[tokio::test]
    async fn a_replacement_clears_previously_set_lists_and_protocol() {
        use meerkat_core::ConfigStore;
        use meerkat_providers::auth_store::{EphemeralTokenStore, TokenStore};
        let persistence = meerkat_providers::auth_store::ProviderAuthPersistence::new(
            Arc::new(EphemeralTokenStore::new()) as Arc<dyn TokenStore>,
            Arc::new(meerkat_providers::auth_store::InMemoryCoordinator::new()),
        );
        let realm = RealmId::global();
        let populated = ConnectorClientConfig {
            issuer: "https://issuer.example".into(),
            client: "client".into(),
            client_auth: ConnectorClientAuthMethod::None,
            authorize_params: vec![("prompt".into(), "consent".into())],
            resource_indicator: true,
            protocol: Some("slack".into()),
            registered_redirect_uris: vec!["https://app.example/callback".into()],
        };
        let emptied = ConnectorClientConfig {
            authorize_params: Vec::new(),
            protocol: None,
            registered_redirect_uris: Vec::new(),
            ..populated.clone()
        };
        let stored = |config: &Config| {
            config
                .realm
                .get(realm.as_str())
                .and_then(|section| section.connector_client.get("desktop"))
                .cloned()
        };

        // Written through this API, in memory.
        let memory: Arc<dyn ConfigStore> = Arc::new(meerkat_core::MemoryConfigStore::new(
            Config::default(),
            meerkat_models::canonical(),
        ));
        put_connector_client(
            Arc::clone(&memory),
            &persistence,
            &realm,
            "desktop",
            populated.clone(),
            ConnectorClientSecretUpdate::Keep,
        )
        .await
        .unwrap();
        assert_eq!(
            stored(&memory.get().await.unwrap()),
            Some(populated.clone())
        );
        put_connector_client(
            Arc::clone(&memory),
            &persistence,
            &realm,
            "desktop",
            emptied.clone(),
            ConnectorClientSecretUpdate::Keep,
        )
        .await
        .unwrap();
        assert_eq!(
            stored(&memory.get().await.unwrap()),
            Some(emptied.clone()),
            "the in-memory replacement clears both lists and the protocol"
        );

        // Written directly in a file document, then replaced through this API.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[realm.global.connector_client.desktop]
issuer = "https://issuer.example"
client = "client"
authorize_params = [["prompt", "consent"]]
protocol = "slack"
registered_redirect_uris = ["https://app.example/callback"]
"#,
        )
        .unwrap();
        let file: Arc<dyn ConfigStore> = Arc::new(meerkat_core::FileConfigStore::new(
            path.clone(),
            meerkat_models::canonical(),
        ));
        assert_eq!(stored(&file.get().await.unwrap()), Some(populated));
        put_connector_client(
            Arc::clone(&file),
            &persistence,
            &realm,
            "desktop",
            emptied.clone(),
            ConnectorClientSecretUpdate::Keep,
        )
        .await
        .unwrap();
        assert_eq!(
            stored(&file.get().await.unwrap()),
            Some(emptied),
            "the file replacement clears both lists and the protocol"
        );
        let document = std::fs::read_to_string(&path).unwrap();
        assert!(
            !document.contains("consent")
                && !document.contains("app.example")
                && !document.contains("slack"),
            "no old value survives in the document: {document}"
        );
    }
}
