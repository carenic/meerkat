//! Realm custody of MCP stdio servers' service secrets.
//!
//! A stdio server may declare environment variables whose values are
//! secrets ([`crate::mcp_config::McpStdioConfig::secret_env`]). Only the
//! names are configuration. The realm whose own configuration document
//! defines the server keeps each value in its credential store under one
//! nonsecret reference ([`McpServiceSecretRef`]): that defining realm, the
//! server, the environment slot and the server's canonical launch meaning
//! ([`McpServerConfig::stdio_launch_meaning`]). A changed definition is a
//! different reference, so a stored value is only ever read for the exact
//! definition it was stored for; an identical definition in another realm (a
//! child realm repeating its parent's server, or an unrelated realm) has its
//! own reference and never reads this one.
//!
//! The value is server configuration, not a sign-in credential: it lives
//! under its own key kind and mode ([`TokenKey::mcp_service_secret`],
//! [`PersistedAuthMode::McpServiceSecret`]) with no lease or lifecycle. A
//! launch reads it unattended, never from the host environment; only the
//! defining realm's owner writes or removes it. A read is a snapshot: a
//! rotation or removal after it does not change a launch that already read
//! the value.

use std::collections::BTreeMap;

use super::token_store::{
    McpServiceSecretKey, PersistedAuthMode, PersistedTokens, TokenKey, TokenStore,
    TokenStoreAccess, TokenStoreError, TokenStoreObservation,
};
use crate::connection::RealmId;
use crate::mcp_config::{McpSecretEnvDeclarationError, McpServerConfig, McpTransportConfig};

/// The metadata field binding a stored value to its exact reference.
const REFERENCE_FIELD: &str = "mcp_service_secret";

/// The one nonsecret reference of a stored MCP service secret: the realm
/// whose own configuration defines the server, the server name, the
/// environment slot and the server's canonical launch meaning. It is
/// configuration equality, not the identity of an executable, an external
/// service or a verified account.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct McpServiceSecretRef {
    server: String,
    key: McpServiceSecretKey,
}

impl std::fmt::Debug for McpServiceSecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServiceSecretRef")
            .field("realm", self.key.realm())
            .field("server", &self.server)
            .field("env", &self.key.env())
            .field("meaning", &self.key.meaning())
            .finish()
    }
}

impl McpServiceSecretRef {
    /// The reference of slot `env` of `server` as `realm`'s own
    /// configuration defines it. Refused for a server that is not stdio, an
    /// invalid declaration ([`McpServerConfig::validate_secret_env`]) or a
    /// slot `server` does not declare.
    pub fn for_definition(
        realm: RealmId,
        server: &McpServerConfig,
        env: &str,
    ) -> Result<Self, McpSecretEnvDeclarationError> {
        let refused = |reason| McpSecretEnvDeclarationError {
            server: server.name.clone(),
            reason,
        };
        let McpTransportConfig::Stdio(stdio) = &server.transport else {
            return Err(refused("only a stdio server declares service secrets"));
        };
        server.validate_secret_env()?;
        if !stdio.secret_env.contains(env) {
            return Err(refused("the slot is not declared in secret_env"));
        }
        let meaning = server
            .stdio_launch_meaning()
            .ok_or_else(|| refused("only a stdio server declares service secrets"))?;
        let key = McpServiceSecretKey::new(realm, env, &meaning)
            .ok_or_else(|| refused("the slot is not an environment variable name"))?;
        Ok(Self {
            server: server.name.clone(),
            key,
        })
    }

    /// The realm whose own configuration defines the server.
    pub fn realm(&self) -> &RealmId {
        self.key.realm()
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    /// The environment variable the value is launched in.
    pub fn env(&self) -> &str {
        self.key.env()
    }

    /// The definition's canonical launch meaning (lowercase hex SHA-256).
    pub fn meaning(&self) -> &str {
        self.key.meaning()
    }

    pub(crate) fn key(&self) -> &McpServiceSecretKey {
        &self.key
    }

    fn record(&self) -> serde_json::Value {
        serde_json::json!({
            REFERENCE_FIELD: {
                "server": self.server,
                "env": self.key.env(),
                "meaning": self.key.meaning(),
            }
        })
    }

    /// Whether `tokens` is a service secret stored for exactly this
    /// reference.
    fn recorded_by(&self, tokens: &PersistedTokens) -> bool {
        tokens.auth_mode == PersistedAuthMode::McpServiceSecret && tokens.metadata == self.record()
    }
}

/// One service-secret value on its way to its slot. It has no `Debug`,
/// `Display`, `Clone` or serialization, and nothing returns it, so it never
/// reaches a log, an error, a wire or a record; the write that stores it
/// consumes it.
pub struct McpServiceSecretValue(String);

impl McpServiceSecretValue {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// The value, for the write that stores it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// The service-secret values resolved for one launch, by environment
/// variable name. `Debug` shows only the names.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct McpServiceSecretValues(BTreeMap<String, String>);

impl std::fmt::Debug for McpServiceSecretValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.0.keys().map(|name| (name, crate::redact::REDACTED)))
            .finish()
    }
}

impl McpServiceSecretValues {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the value launched in `env`.
    pub fn insert(&mut self, env: impl Into<String>, value: impl Into<String>) {
        self.0.insert(env.into(), value.into());
    }

    /// The environment variable names, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `(name, value)` pairs, consuming the values.
    pub fn into_env(self) -> impl Iterator<Item = (String, String)> {
        self.0.into_iter()
    }
}

/// Why a stdio server's declared service secrets were not resolved for its
/// launch. Each is local and typed, names no value, and comes before the
/// server's process exists; no value is ever taken from the host
/// environment instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpServiceSecretRefusal {
    /// The connection's credential source holds no service-secret custody.
    #[error("no service-secret custody serves this connection")]
    NoCustody,
    /// No realm's own configuration defines the server (an `mcp.toml`, host
    /// or live-added server has no defining realm).
    #[error("no realm's own configuration defines this server")]
    NoOwner,
    /// The launched definition is not the defining realm's current
    /// definition of the server.
    #[error("the launched definition is not its defining realm's current definition")]
    DefinitionChanged,
    /// The defining realm stores no value for the slot of the current
    /// definition.
    #[error("no secret is stored for slot '{env}' of the current definition")]
    NotStored { env: String },
    /// The configuration or the credential store could not be read.
    #[error("the service-secret custody is unavailable: {reason}")]
    Unavailable { reason: String },
}

/// What a status observation saw for one reference. It never shows the
/// value, and a stored value does not say which write stored it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServiceSecretPresence {
    /// A value is stored for exactly this reference.
    Stored,
    /// The store answered and holds no value for this reference.
    NotStored,
    /// The store did not answer without operating-system interaction, so
    /// nothing is known.
    NotObserved,
}

/// Read the value stored for exactly `reference`, for a launch: an
/// unattended read that raises no operating-system UI. Only a row stored as
/// an MCP service secret for exactly this reference, with a non-empty value,
/// is returned; anything else is `None`.
pub async fn load_mcp_service_secret(
    store: &dyn TokenStore,
    reference: &McpServiceSecretRef,
) -> Result<Option<String>, TokenStoreError> {
    let stored = store
        .load_for(
            &TokenKey::mcp_service_secret(reference),
            TokenStoreAccess::Unattended,
        )
        .await?;
    Ok(stored
        .filter(|tokens| reference.recorded_by(tokens))
        .and_then(|tokens| tokens.primary_secret)
        .filter(|value| !value.is_empty()))
}

/// Store (or replace) the value of `reference`: an explicit operator write.
/// `value` is not empty (the caller refuses an empty value). Returns the
/// exact row the store persisted.
pub async fn save_mcp_service_secret(
    store: &dyn TokenStore,
    reference: &McpServiceSecretRef,
    value: &str,
) -> Result<PersistedTokens, TokenStoreError> {
    let mut tokens = PersistedTokens::api_key(value);
    tokens.auth_mode = PersistedAuthMode::McpServiceSecret;
    tokens.metadata = reference.record();
    store
        .save_for(
            &TokenKey::mcp_service_secret(reference),
            &tokens,
            TokenStoreAccess::Interactive,
        )
        .await?;
    Ok(tokens)
}

/// Remove the value of `reference`, if any: an explicit operator write.
pub async fn clear_mcp_service_secret(
    store: &dyn TokenStore,
    reference: &McpServiceSecretRef,
) -> Result<(), TokenStoreError> {
    store
        .clear_for(
            &TokenKey::mcp_service_secret(reference),
            TokenStoreAccess::Interactive,
        )
        .await
}

/// Whether a value is stored for `reference`, observed as a status read: no
/// operating-system UI and no keychain read.
pub async fn observe_mcp_service_secret(
    store: &dyn TokenStore,
    reference: &McpServiceSecretRef,
) -> Result<McpServiceSecretPresence, TokenStoreError> {
    Ok(
        match store
            .observe_status(&TokenKey::mcp_service_secret(reference))
            .await?
        {
            TokenStoreObservation::Present(tokens) if reference.recorded_by(&tokens) => {
                McpServiceSecretPresence::Stored
            }
            TokenStoreObservation::Present(_) | TokenStoreObservation::Missing => {
                McpServiceSecretPresence::NotStored
            }
            _ => McpServiceSecretPresence::NotObserved,
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    fn server(name: &str, secret_env: &[&str]) -> McpServerConfig {
        let mut server = McpServerConfig::stdio(
            name,
            "search-server",
            vec!["--stdio".to_owned()],
            HashMap::from([("LOG_LEVEL".to_owned(), "info".to_owned())]),
        );
        if let McpTransportConfig::Stdio(stdio) = &mut server.transport {
            stdio.secret_env = secret_env.iter().map(|name| (*name).to_owned()).collect();
        }
        server
    }

    fn realm(id: &str) -> RealmId {
        RealmId::parse(id).expect("realm id")
    }

    #[test]
    fn a_reference_names_the_defining_realm_slot_and_exact_definition() {
        let defined = server("search", &["SEARCH_API_KEY"]);
        let reference =
            McpServiceSecretRef::for_definition(realm("team"), &defined, "SEARCH_API_KEY")
                .expect("declared slot");
        assert_eq!(reference.realm(), &realm("team"));
        assert_eq!(reference.server(), "search");
        assert_eq!(reference.env(), "SEARCH_API_KEY");
        assert_eq!(
            Some(reference.meaning()),
            defined.stdio_launch_meaning().as_deref()
        );
        let key = TokenKey::mcp_service_secret(&reference);
        assert!(key.is_mcp_service_secret());
        assert!(key.credential_identity().is_none());
        assert_eq!(key.realm(), &realm("team"));
        assert_eq!(
            TokenKey::mcp_service_secret_from_stem(&realm("team"), &key.storage_stem()),
            Some(key.clone()),
            "the storage stem names the same key"
        );
        let wire = serde_json::to_value(&key).expect("serialize");
        assert_eq!(
            serde_json::from_value::<TokenKey>(wire).expect("deserialize"),
            key
        );

        // Another realm, another slot or any changed launch field is another
        // reference.
        let other_realm =
            McpServiceSecretRef::for_definition(realm("child"), &defined, "SEARCH_API_KEY")
                .expect("declared slot");
        assert_ne!(
            TokenKey::mcp_service_secret(&other_realm),
            TokenKey::mcp_service_secret(&reference)
        );
        let mut changed = defined.clone();
        if let McpTransportConfig::Stdio(stdio) = &mut changed.transport {
            stdio.args.push("--verbose".to_owned());
        }
        let changed_ref =
            McpServiceSecretRef::for_definition(realm("team"), &changed, "SEARCH_API_KEY")
                .expect("declared slot");
        assert_ne!(changed_ref.meaning(), reference.meaning());

        // An access ceiling is a separate check, not part of the meaning.
        let mut ceiling = defined.clone();
        ceiling.access = Some(crate::mcp_config::McpServerAccess::allowing(["search"]));
        assert_eq!(
            ceiling.stdio_launch_meaning(),
            defined.stdio_launch_meaning()
        );
    }

    #[test]
    fn undeclared_slots_and_invalid_declarations_have_no_reference() {
        let defined = server("search", &["SEARCH_API_KEY"]);
        assert!(McpServiceSecretRef::for_definition(realm("team"), &defined, "OTHER").is_err());
        for invalid in [
            server("search", &["9LIVES"]),
            server("search", &["HAS-DASH"]),
            server("search", &["MEERKAT_CONNECTOR_BINDING"]),
            server("search", &["log_level"]),
            server("search", &["TOKEN", "token"]),
        ] {
            let McpTransportConfig::Stdio(stdio) = &invalid.transport else {
                unreachable!("stdio fixture");
            };
            let env = stdio.secret_env.iter().next().cloned().unwrap_or_default();
            assert!(invalid.validate_secret_env().is_err(), "{invalid:?}");
            assert!(McpServiceSecretRef::for_definition(realm("team"), &invalid, &env).is_err());
        }
        let http =
            McpServerConfig::streamable_http("remote", "https://mcp.example", HashMap::new());
        assert!(http.validate_secret_env().is_ok());
        assert!(http.stdio_launch_meaning().is_none());
        assert!(McpServiceSecretRef::for_definition(realm("team"), &http, "TOKEN").is_err());
    }

    #[test]
    fn values_and_declarations_print_only_names() {
        let mut values = McpServiceSecretValues::new();
        values.insert("SEARCH_API_KEY", "sk-live-value");
        let rendered = format!("{values:?}");
        assert!(rendered.contains("SEARCH_API_KEY"), "{rendered}");
        assert!(!rendered.contains("sk-live-value"), "{rendered}");
        let declared = server("search", &["SEARCH_API_KEY"]);
        let rendered = format!("{declared:?}");
        assert!(rendered.contains("SEARCH_API_KEY"), "{rendered}");
        assert_eq!(
            values.names().collect::<BTreeSet<_>>(),
            BTreeSet::from(["SEARCH_API_KEY"])
        );
    }
}
