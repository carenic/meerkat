//! Server-constrained credential selection for self-hosted models.
//!
//! [`Provider::SelfHosted`] is a provider CLASS, not an endpoint identity. Two
//! unrelated servers - a hosted OpenAI-compatible gateway and a private vLLM
//! box - are both classified `self_hosted` and hold DIFFERENT secrets. Keying
//! credential selection on the provider alone therefore leaves "which secret
//! authenticates this endpoint" without a canonical owner: whichever realm in
//! the chain happens to be nearest wins, and its secret is sent to whatever
//! endpoint the selected model's server declares. The far end replies
//! `401 Unauthorized`, which explains nothing about the fact that WE chose the
//! wrong secret.
//!
//! This module is the single owner of the missing distinction. It resolves a
//! realm binding for a NAMED self-hosted server ([`self_hosted_binding_server`]
//! computes the typed server identity of a binding;
//! [`resolve_self_hosted_binding_for_server`] selects one), and it fails closed
//! with a typed error that names the server and every binding considered when
//! the selection cannot be made honestly.
//!
//! Ownership of the server fact, in precedence order:
//!
//! 1. [`crate::connection::BackendProfile::server`] - the canonical declaration
//!    (`[realm.<r>.backend.<b>] server = "<server_id>"`).
//! 2. [`ProviderBinding::default_model`] - a named inference for configs
//!    written before the declaration existed: a binding whose default model is
//!    a configured `[self_hosted.models.<alias>]` serves that alias's server.
//! 3. Neither - the binding is [`SelfHostedBindingServer::Undeclared`] and is
//!    only usable when it is the single unconstrained self-hosted binding on
//!    the chain (the documented one-server setup keeps working unchanged).
//!
//! A binding that declares one server while defaulting to a model on another is
//! [`SelfHostedBindingServer::Conflicting`]: it never matches, and it is
//! reported in the considered list so the contradiction is visible.
//!
//! A binding whose auth profile runs a credential command authenticates only
//! the server its backend declares: the host administers where the command's
//! token goes through that declaration, so neither the binding's
//! `default_model` nor a request's model may decide it. Such a binding is
//! never matched by inference and never the unconstrained fallback, and an
//! explicit selection of one that declares no server is refused
//! ([`SelfHostedConnectionError::CredentialCommandServerUndeclared`]).

use thiserror::Error;

use crate::Config;
use crate::connection::{
    BindingId, BindingOrigin, ConnectionTargetError, CredentialSourceSpec, ProviderBinding,
    RealmChain, RealmConnectionSet, RealmId, ResolvedConnectionTarget,
    materialize_connection_target,
};
use crate::provider::Provider;

/// Which `[self_hosted.servers.<id>]` endpoint a realm binding authenticates,
/// and how that fact was established.
///
/// Rule 4: the distinction between "this binding is for THIS server" and "this
/// binding is somewhere in the self_hosted class" changes which secret is sent
/// to which endpoint, so it is typed rather than inferred at each call site.
///
/// Deliberately NOT serializable: nothing persists or wire-projects a binding's
/// server identity. It is recomputed from config on every resolution, so there
/// is no second copy to drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfHostedBindingServer {
    /// The backend profile declares `server = "<id>"`. Canonical.
    Declared { server: String },
    /// The backend declares no server, but the binding's `default_model` is a
    /// configured self-hosted alias served by this server.
    DerivedFromDefaultModel { server: String },
    /// The backend declares a server AND the binding's `default_model` resolves
    /// to a different one. Never matches; always reported.
    Conflicting {
        declared: String,
        default_model_server: String,
    },
    /// Nothing on the binding names a server.
    Undeclared,
}

impl SelfHostedBindingServer {
    /// The server this binding is known to authenticate, when one is known.
    #[must_use]
    pub fn server(&self) -> Option<&str> {
        match self {
            Self::Declared { server } | Self::DerivedFromDefaultModel { server } => {
                Some(server.as_str())
            }
            Self::Conflicting { .. } | Self::Undeclared => None,
        }
    }

    /// Whether this binding positively names `server_id`.
    #[must_use]
    pub fn matches(&self, server_id: &str) -> bool {
        self.server() == Some(server_id)
    }

    /// Whether this binding names no server at all (the compatibility tier).
    #[must_use]
    pub fn is_undeclared(&self) -> bool {
        matches!(self, Self::Undeclared)
    }
}

impl std::fmt::Display for SelfHostedBindingServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Declared { server } => write!(f, "server '{server}' (declared)"),
            Self::DerivedFromDefaultModel { server } => {
                write!(f, "server '{server}' (from default_model)")
            }
            Self::Conflicting {
                declared,
                default_model_server,
            } => write!(
                f,
                "conflicting servers (backend declares '{declared}', default_model serves '{default_model_server}')"
            ),
            Self::Undeclared => f.write_str("no declared server"),
        }
    }
}

/// One self-hosted binding examined during selection, with its typed server
/// identity. The considered list is what makes the fail-closed error
/// actionable: it names what was looked at and why each entry did not qualify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfHostedBindingCandidate {
    pub realm: RealmId,
    pub binding: BindingId,
    pub server: SelfHostedBindingServer,
}

impl std::fmt::Display for SelfHostedBindingCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} -> {}",
            self.realm.as_str(),
            self.binding.as_str(),
            self.server
        )
    }
}

/// Render a considered list for an error message without an intermediate
/// `Vec<String>` + `join` (dogma: `impl Display` over collect+join).
struct ConsideredList<'a>(&'a [SelfHostedBindingCandidate]);

impl std::fmt::Display for ConsideredList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (idx, candidate) in self.0.iter().enumerate() {
            if idx > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{candidate}")?;
        }
        Ok(())
    }
}

/// Fail-closed outcomes of server-constrained self-hosted credential selection.
///
/// Every variant names the server the model is served by, so an operator reads
/// "we could not tell which secret authenticates muse_vllm" instead of an
/// `Unauthorized` minted by the far end for a credential we chose wrongly.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SelfHostedConnectionError {
    /// No realm on the chain declares any self-hosted binding at all.
    #[error(
        "self-hosted server '{server}' has no credential binding: no realm on the '{head}' chain \
         declares a binding for provider 'self_hosted'. Declare a realm backend with \
         provider = \"self_hosted\", server = \"{server}\" plus an auth profile and binding, \
         or select one explicitly with an auth binding."
    )]
    NoSelfHostedBindings { server: String, head: String },
    /// Self-hosted bindings exist, but none names this server and none is an
    /// unconstrained fallback.
    #[error(
        "no credential binding for self-hosted server '{server}': considered {}. \
         Add server = \"{server}\" to the backend profile of the binding that authenticates it, \
         or select it explicitly with an auth binding.",
        ConsideredList(.considered)
    )]
    NoBindingForServer {
        server: String,
        considered: Vec<SelfHostedBindingCandidate>,
    },
    /// Several self-hosted bindings are reachable, none names this server, and
    /// more than one is unconstrained. Guessing here is what sends one server's
    /// secret to another server's endpoint.
    #[error(
        "cannot tell which credential binding authenticates self-hosted server '{server}': \
         {} unconstrained bindings are reachable and none names a server: considered {}. \
         Add server = \"{server}\" to the backend profile of the binding that authenticates it, \
         or select it explicitly with an auth binding.",
        .considered.iter().filter(|candidate| candidate.server.is_undeclared()).count(),
        ConsideredList(.considered)
    )]
    AmbiguousServerBinding {
        server: String,
        considered: Vec<SelfHostedBindingCandidate>,
    },
    /// An explicitly named binding declares a different server than the one the
    /// selected model is served by. Explicit requests are strict.
    #[error(
        "auth binding '{realm}:{binding}' declares self-hosted server '{declared}', but model \
         '{model}' is served by '{server}'; the binding's credential does not authenticate that \
         endpoint"
    )]
    ExplicitBindingServerMismatch {
        realm: String,
        binding: String,
        declared: String,
        server: String,
        model: String,
    },
    /// An explicitly named binding whose auth profile runs a credential
    /// command declares no server. The host administers where such a
    /// binding's token goes through the server its backend declares, so a
    /// binding that declares none authenticates no server: neither the one
    /// its `default_model` is served by nor the one the request's model is.
    #[error(
        "auth binding '{realm}:{binding}' runs a credential command, but its backend declares no \
         self-hosted server, so its token is sent to none, and model '{model}' is served by \
         '{server}'; add server = \"<id>\" to the binding's backend profile, naming the server \
         the command's token is for"
    )]
    CredentialCommandServerUndeclared {
        realm: String,
        binding: String,
        server: String,
        model: String,
    },
    /// The realm chain itself could not be resolved.
    #[error(transparent)]
    ConnectionTarget(#[from] ConnectionTargetError),
}

/// The typed server identity of one self-hosted binding.
///
/// `config` supplies the `[self_hosted.models]` map used for the
/// `default_model` inference; `realm` owns the backend/auth profiles the
/// binding references.
#[must_use]
pub fn self_hosted_binding_server(
    config: &Config,
    realm: &RealmConnectionSet,
    binding: &ProviderBinding,
) -> SelfHostedBindingServer {
    let declared = realm
        .backends
        .get(&binding.backend_profile)
        .and_then(|backend| backend.server.clone());
    let from_default_model = binding
        .default_model
        .as_deref()
        .and_then(|model| config.self_hosted.models.get(model))
        .map(|model| model.server.clone())
        .filter(|server| !server.is_empty());

    match (declared, from_default_model) {
        (Some(declared), Some(derived)) if declared != derived => {
            SelfHostedBindingServer::Conflicting {
                declared,
                default_model_server: derived,
            }
        }
        (Some(server), _) => SelfHostedBindingServer::Declared { server },
        (None, Some(server)) => SelfHostedBindingServer::DerivedFromDefaultModel { server },
        (None, None) => SelfHostedBindingServer::Undeclared,
    }
}

/// Self-hosted bindings of one realm in canonical preference order:
/// `default_binding`, then the typed `provider_default` marker, then binding-id
/// order. This is the same policy the provider-class resolver applies, narrowed
/// to the self-hosted class and applied to ALL of a realm's self-hosted
/// bindings instead of collapsing them to one before the server is known.
fn ordered_self_hosted_bindings(realm: &RealmConnectionSet) -> Vec<&ProviderBinding> {
    let mut bindings: Vec<&ProviderBinding> = realm
        .bindings
        .values()
        .filter(|binding| {
            let backend_is_self_hosted = realm
                .backends
                .get(&binding.backend_profile)
                .is_some_and(|backend| backend.provider == Provider::SelfHosted);
            let auth_is_self_hosted = realm
                .auth_profiles
                .get(&binding.auth_profile)
                .is_some_and(|auth| auth.provider == Provider::SelfHosted);
            backend_is_self_hosted && auth_is_self_hosted
        })
        .collect();
    // Stable sort: ties keep the deterministic BTreeMap (binding id) order.
    bindings.sort_by_key(|binding| {
        if realm.default_binding.as_deref() == Some(binding.id.as_str()) {
            0
        } else if binding.provider_default {
            1
        } else {
            2
        }
    });
    bindings
}

/// A self-hosted binding reachable from a head realm: the realm that states
/// it, the candidate an error reports, and whether its auth profile runs a
/// credential command.
struct ReachableBinding {
    realm: RealmConnectionSet,
    candidate: SelfHostedBindingCandidate,
    runs_command: bool,
}

impl ReachableBinding {
    /// Whether this binding names `server_id`. A credential command binding
    /// names a server only by declaring it: the host administers where its
    /// token goes through the declaration, not through its `default_model`.
    fn names(&self, server_id: &str) -> bool {
        match &self.candidate.server {
            SelfHostedBindingServer::Declared { server } => server == server_id,
            SelfHostedBindingServer::DerivedFromDefaultModel { server } => {
                !self.runs_command && server == server_id
            }
            SelfHostedBindingServer::Conflicting { .. } | SelfHostedBindingServer::Undeclared => {
                false
            }
        }
    }

    /// Whether this binding may stand in for any server: it names none and
    /// does not run a credential command, whose token goes only to the
    /// server its backend declares.
    fn is_unconstrained(&self) -> bool {
        self.candidate.server.is_undeclared() && !self.runs_command
    }
}

/// Collect every self-hosted binding reachable from `head`, child realm first,
/// each with its typed server identity.
///
/// Chain-member isolation matches [`crate::connection`]'s candidate collection:
/// an absent or structurally invalid member is SKIPPED, including the head, so
/// an unmaterialized session realm still inherits its chain.
fn collect_self_hosted_candidates(
    config: &Config,
    head: &RealmId,
) -> Result<Vec<ReachableBinding>, SelfHostedConnectionError> {
    let chain = RealmChain::resolve(config, head).map_err(ConnectionTargetError::from)?;
    let mut out = Vec::new();
    for member in chain.realms() {
        let Some(section) = config.realm.get(member.as_str()) else {
            continue;
        };
        let Ok(realm) = RealmConnectionSet::from_config(member.as_str(), section) else {
            continue;
        };
        for binding in ordered_self_hosted_bindings(&realm) {
            // A binding id that is not a valid typed `BindingId` can never be
            // named by a resolved target (every `AuthBindingRef` carries the
            // typed id), so it is not a candidate here either. This mirrors the
            // per-member isolation the provider-class candidate collector
            // applies: a malformed entry cannot take down resolution for the
            // valid ones.
            let Ok(binding_id) = BindingId::parse(binding.id.clone()) else {
                continue;
            };
            let server = self_hosted_binding_server(config, &realm, binding);
            let runs_command = realm
                .auth_profiles
                .get(&binding.auth_profile)
                .is_some_and(|auth| matches!(auth.source, CredentialSourceSpec::Command { .. }));
            out.push(ReachableBinding {
                realm: realm.clone(),
                candidate: SelfHostedBindingCandidate {
                    realm: realm.realm_id.clone(),
                    binding: binding_id,
                    server,
                },
                runs_command,
            });
        }
    }
    Ok(out)
}

/// Resolve the credential binding that authenticates self-hosted `server_id`.
///
/// Selection order:
/// 1. the nearest binding that positively names `server_id` (child realm
///    first; within a realm, `default_binding`, then the typed
///    `provider_default` marker, then binding-id order); a binding whose
///    auth profile runs a credential command names a server only by
///    declaring it;
/// 2. the single unconstrained ([`SelfHostedBindingServer::Undeclared`])
///    binding on the chain, if there is exactly one; a credential command
///    binding is never unconstrained.
///
/// Anything else fails closed with a typed [`SelfHostedConnectionError`] that
/// names the server and every binding considered. A realm's own
/// `default_binding` has NO privilege over the server constraint: the realm
/// default is a within-class preference, not evidence about which endpoint a
/// secret authenticates.
pub fn resolve_self_hosted_binding_for_server(
    config: &Config,
    server_id: &str,
    preferred_realm: Option<&RealmId>,
) -> Result<ResolvedConnectionTarget, SelfHostedConnectionError> {
    let global = RealmId::global();
    let head = preferred_realm.unwrap_or(&global);
    let candidates = collect_self_hosted_candidates(config, head)?;

    // One rule, no sub-tiers: the NEAREST binding that names this server wins.
    // Realm proximity is the established credential scoping rule; the server
    // identity is a filter over it, not a competing precedence. Declaration vs
    // inference decides whether a binding names the server at all, never which
    // of two naming bindings is preferred.
    if let Some(reachable) = candidates
        .iter()
        .find(|reachable| reachable.names(server_id))
    {
        return materialize(reachable.realm.clone(), reachable.candidate.binding.clone());
    }

    let considered: Vec<SelfHostedBindingCandidate> = candidates
        .iter()
        .map(|reachable| reachable.candidate.clone())
        .collect();
    if considered.is_empty() {
        return Err(SelfHostedConnectionError::NoSelfHostedBindings {
            server: server_id.to_string(),
            head: head.as_str().to_string(),
        });
    }

    let mut unconstrained = candidates
        .iter()
        .filter(|reachable| reachable.is_unconstrained());
    match (unconstrained.next(), unconstrained.next()) {
        // Exactly one unconstrained binding: the documented single-server setup
        // keeps resolving without annotation.
        (Some(reachable), None) => {
            materialize(reachable.realm.clone(), reachable.candidate.binding.clone())
        }
        (Some(_), Some(_)) => Err(SelfHostedConnectionError::AmbiguousServerBinding {
            server: server_id.to_string(),
            considered,
        }),
        _ => Err(SelfHostedConnectionError::NoBindingForServer {
            server: server_id.to_string(),
            considered,
        }),
    }
}

fn materialize(
    realm: RealmConnectionSet,
    binding: BindingId,
) -> Result<ResolvedConnectionTarget, SelfHostedConnectionError> {
    materialize_connection_target(
        realm,
        Some(Provider::SelfHosted),
        binding,
        None,
        BindingOrigin::Configured,
    )
    .map_err(SelfHostedConnectionError::ConnectionTarget)
}

/// Validate an EXPLICITLY selected target against the server that actually
/// serves `model`.
///
/// Only a [`SelfHostedBindingServer::Declared`] (or
/// [`SelfHostedBindingServer::Conflicting`]) mismatch fails: those are
/// operator declarations that contradict the request. An undeclared binding, or
/// one whose server is only inferred from `default_model`, still resolves - an
/// explicit selection remains the operator's escape hatch, including for a
/// credential deliberately shared across servers - unless its auth profile
/// runs a credential command: such a binding authenticates only the server
/// its backend declares, so naming one that declares none is refused
/// ([`SelfHostedConnectionError::CredentialCommandServerUndeclared`]).
pub fn validate_explicit_self_hosted_target(
    config: &Config,
    target: &ResolvedConnectionTarget,
    server_id: &str,
    model: &str,
) -> Result<(), SelfHostedConnectionError> {
    let identity = self_hosted_binding_server(config, &target.realm, &target.binding);
    let declared = match &identity {
        SelfHostedBindingServer::Declared { server } => server.clone(),
        SelfHostedBindingServer::Conflicting { declared, .. } => declared.clone(),
        SelfHostedBindingServer::DerivedFromDefaultModel { .. }
        | SelfHostedBindingServer::Undeclared => {
            if matches!(
                target.auth_profile.source,
                CredentialSourceSpec::Command { .. }
            ) {
                return Err(
                    SelfHostedConnectionError::CredentialCommandServerUndeclared {
                        realm: target.auth_binding.realm.as_str().to_string(),
                        binding: target.auth_binding.binding.as_str().to_string(),
                        server: server_id.to_string(),
                        model: model.to_string(),
                    },
                );
            }
            return Ok(());
        }
    };
    if declared == server_id {
        return Ok(());
    }
    Err(SelfHostedConnectionError::ExplicitBindingServerMismatch {
        realm: target.auth_binding.realm.as_str().to_string(),
        binding: target.auth_binding.binding.as_str().to_string(),
        declared,
        server: server_id.to_string(),
        model: model.to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::config::{
        SelfHostedApiStyle, SelfHostedModelConfig, SelfHostedServerConfig, SelfHostedTransport,
    };
    use crate::connection::{
        AuthProfileConfig, BackendProfileConfig, CredentialSourceSpec, ProviderBindingConfig,
    };

    fn model(server: &str) -> SelfHostedModelConfig {
        SelfHostedModelConfig {
            server: server.to_string(),
            remote_model: "remote".to_string(),
            ..Default::default()
        }
    }

    /// Two self-hosted servers, one alias each.
    fn base_config() -> Config {
        let mut config = Config::default();
        for (id, base_url) in [
            ("muse_vllm", "http://muse.invalid:8000"),
            ("cerebras", "https://api.cerebras.invalid/v1"),
        ] {
            config.self_hosted.servers.insert(
                id.to_string(),
                SelfHostedServerConfig {
                    transport: SelfHostedTransport::OpenAiCompatible,
                    base_url: base_url.to_string(),
                    api_style: SelfHostedApiStyle::ChatCompletions,
                },
            );
        }
        config
            .self_hosted
            .models
            .insert("test-muse-model".to_string(), model("muse_vllm"));
        config
            .self_hosted
            .models
            .insert("test-cerebras-model".to_string(), model("cerebras"));
        config
    }

    fn add_binding(
        config: &mut Config,
        realm_id: &str,
        binding_id: &str,
        declared_server: Option<&str>,
        default_model: Option<&str>,
        realm_default: bool,
    ) {
        let backend_id = format!("{binding_id}_backend");
        let auth_id = format!("{binding_id}_auth");
        let realm = config.realm.entry(realm_id.to_string()).or_default();
        realm.backend.insert(
            backend_id.clone(),
            BackendProfileConfig {
                provider: "self_hosted".to_string(),
                backend_kind: "self_hosted".to_string(),
                base_url: None,
                options: serde_json::Value::Null,
                server: declared_server.map(str::to_string),
            },
        );
        realm.auth.insert(
            auth_id.clone(),
            AuthProfileConfig {
                provider: "self_hosted".to_string(),
                auth_method: "static_bearer".to_string(),
                source: CredentialSourceSpec::InlineSecret {
                    secret: format!("{binding_id}-secret"),
                },
                constraints: Default::default(),
                metadata_defaults: Default::default(),
            },
        );
        realm.binding.insert(
            binding_id.to_string(),
            ProviderBindingConfig {
                backend_profile: backend_id,
                auth_profile: auth_id,
                credential_account: None,
                default_model: default_model.map(str::to_string),
                policy: Default::default(),
                provider_default: false,
            },
        );
        if realm_default {
            realm.default_binding = Some(binding_id.to_string());
        }
    }

    fn realm(id: &str) -> RealmId {
        RealmId::parse(id).unwrap()
    }

    #[test]
    fn declared_server_outranks_realm_default_binding() {
        let mut config = base_config();
        add_binding(&mut config, "ws", "cerebras", Some("cerebras"), None, true);
        add_binding(&mut config, "ws", "muse", Some("muse_vllm"), None, false);

        let target =
            resolve_self_hosted_binding_for_server(&config, "muse_vllm", Some(&realm("ws")))
                .expect("the binding declaring the server must resolve");
        assert_eq!(target.auth_binding.binding.as_str(), "muse");
        assert_eq!(
            target.auth_profile.source,
            CredentialSourceSpec::InlineSecret {
                secret: "muse-secret".to_string()
            }
        );
    }

    #[test]
    fn declaration_outranks_default_model_inference_on_the_same_binding() {
        let mut config = base_config();
        // Declares muse_vllm while defaulting to a cerebras-served model:
        // a contradiction, so it authenticates neither.
        add_binding(
            &mut config,
            "ws",
            "mixed",
            Some("muse_vllm"),
            Some("test-cerebras-model"),
            true,
        );
        let realm_set = RealmConnectionSet::from_config("ws", &config.realm["ws"]).unwrap();
        let identity =
            self_hosted_binding_server(&config, &realm_set, &realm_set.bindings["mixed"]);
        assert_eq!(
            identity,
            SelfHostedBindingServer::Conflicting {
                declared: "muse_vllm".to_string(),
                default_model_server: "cerebras".to_string(),
            }
        );
        assert!(!identity.matches("muse_vllm"));
        assert!(!identity.matches("cerebras"));

        let err = resolve_self_hosted_binding_for_server(&config, "muse_vllm", Some(&realm("ws")))
            .expect_err("a contradicting binding must not authenticate either server");
        let message = err.to_string();
        assert!(
            message.contains("muse_vllm") && message.contains("ws:mixed"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn single_unconstrained_binding_resolves_any_server() {
        let mut config = base_config();
        add_binding(&mut config, "ws", "local", None, None, true);
        for server in ["muse_vllm", "cerebras"] {
            let target =
                resolve_self_hosted_binding_for_server(&config, server, Some(&realm("ws")))
                    .expect("the single unconstrained binding stays usable");
            assert_eq!(target.auth_binding.binding.as_str(), "local");
        }
    }

    #[test]
    fn several_unconstrained_bindings_fail_closed_naming_considered() {
        let mut config = base_config();
        add_binding(&mut config, "ws", "one", None, None, true);
        add_binding(&mut config, "global", "two", None, None, false);

        let err = resolve_self_hosted_binding_for_server(&config, "muse_vllm", Some(&realm("ws")))
            .expect_err("guessing between two unconstrained bindings is the defect");
        assert!(matches!(
            err,
            SelfHostedConnectionError::AmbiguousServerBinding { .. }
        ));
        let message = err.to_string();
        for expected in ["muse_vllm", "ws:one", "global:two"] {
            assert!(
                message.contains(expected),
                "message must contain {expected}: {message}"
            );
        }
    }

    #[test]
    fn no_self_hosted_binding_at_all_names_the_server() {
        let config = base_config();
        let err = resolve_self_hosted_binding_for_server(&config, "muse_vllm", Some(&realm("ws")))
            .expect_err("no binding anywhere must fail closed");
        assert!(matches!(
            err,
            SelfHostedConnectionError::NoSelfHostedBindings { .. }
        ));
        assert!(err.to_string().contains("muse_vllm"), "{err}");
    }

    #[test]
    fn absent_head_realm_still_inherits_the_chain() {
        let mut config = base_config();
        add_binding(
            &mut config,
            "global",
            "muse",
            Some("muse_vllm"),
            None,
            false,
        );

        let target = resolve_self_hosted_binding_for_server(
            &config,
            "muse_vllm",
            Some(&realm("unmaterialized")),
        )
        .expect("an unmaterialized session realm still inherits global");
        assert_eq!(target.auth_binding.realm.as_str(), "global");
    }

    #[test]
    fn explicit_target_declaring_another_server_is_rejected() {
        let mut config = base_config();
        add_binding(&mut config, "ws", "cerebras", Some("cerebras"), None, true);
        let target =
            resolve_self_hosted_binding_for_server(&config, "cerebras", Some(&realm("ws")))
                .expect("its own server resolves");

        validate_explicit_self_hosted_target(&config, &target, "cerebras", "test-cerebras-model")
            .expect("matching server passes");

        let err =
            validate_explicit_self_hosted_target(&config, &target, "muse_vllm", "test-muse-model")
                .expect_err("a declared other server must be rejected");
        let message = err.to_string();
        for expected in ["muse_vllm", "cerebras", "test-muse-model"] {
            assert!(
                message.contains(expected),
                "message must contain {expected}: {message}"
            );
        }
    }

    /// Make `binding_id`'s auth profile in `realm_id` run a credential
    /// command.
    fn run_credential_command(config: &mut Config, realm_id: &str, binding_id: &str) {
        let auth = config
            .realm
            .get_mut(realm_id)
            .and_then(|realm| realm.auth.get_mut(&format!("{binding_id}_auth")))
            .expect("the binding's auth profile");
        auth.source = CredentialSourceSpec::Command {
            program: "/usr/bin/helper".into(),
            args: Vec::new(),
            cwd: None,
            env: std::collections::BTreeMap::new(),
            timeout_ms: 30_000,
            refresh_interval_ms: None,
        };
    }

    /// The explicitly selected target of `binding_id` in `realm_id`.
    fn explicit_target(
        config: &Config,
        realm_id: &str,
        binding_id: &str,
    ) -> ResolvedConnectionTarget {
        crate::connection::resolve_explicit_auth_binding_target(
            config,
            &crate::connection::AuthBindingRef {
                realm: realm(realm_id),
                binding: BindingId::parse(binding_id).unwrap(),
                profile: None,
                origin: BindingOrigin::Configured,
            },
        )
        .expect("the binding resolves when named")
    }

    /// A binding whose auth profile runs a credential command sends a token
    /// the host administers, so it authenticates only the server its backend
    /// declares. Without a declaration it is neither the unconstrained
    /// fallback for a model on another server nor, by its default model, the
    /// binding of that model's server, whether the request leaves the choice
    /// to the resolver or names the binding; a declared one still resolves
    /// for its own server alone.
    #[test]
    fn a_credential_command_binding_authenticates_only_its_declared_server() {
        let mut undeclared = base_config();
        add_binding(&mut undeclared, "ws", "helper", None, None, true);
        run_credential_command(&mut undeclared, "ws", "helper");
        let mut derived = base_config();
        add_binding(
            &mut derived,
            "ws",
            "helper",
            None,
            Some("test-cerebras-model"),
            true,
        );
        run_credential_command(&mut derived, "ws", "helper");

        for (case, config, server, model) in [
            ("undeclared", &undeclared, "muse_vllm", "test-muse-model"),
            (
                "derived from its default model",
                &derived,
                "cerebras",
                "test-cerebras-model",
            ),
        ] {
            let implicit =
                resolve_self_hosted_binding_for_server(config, server, Some(&realm("ws")));
            assert!(
                matches!(
                    implicit,
                    Err(SelfHostedConnectionError::NoBindingForServer { .. })
                ),
                "{case}: the resolver never picks it for {server}: {:?}",
                implicit.map(|target| target.auth_binding.binding.to_string())
            );
            let explicit = validate_explicit_self_hosted_target(
                config,
                &explicit_target(config, "ws", "helper"),
                server,
                model,
            )
            .expect_err("a named credential command binding needs a declared server");
            let message = explicit.to_string();
            assert!(
                message.contains("ws:helper")
                    && message.contains(server)
                    && message.contains("server = \""),
                "{case}: {message}"
            );
        }

        let mut declared = base_config();
        add_binding(&mut declared, "ws", "helper", Some("muse_vllm"), None, true);
        run_credential_command(&mut declared, "ws", "helper");
        let target =
            resolve_self_hosted_binding_for_server(&declared, "muse_vllm", Some(&realm("ws")))
                .expect("its declared server resolves");
        validate_explicit_self_hosted_target(&declared, &target, "muse_vllm", "test-muse-model")
            .expect("its declared server passes");
        assert!(
            resolve_self_hosted_binding_for_server(&declared, "cerebras", Some(&realm("ws")))
                .is_err()
        );
        assert!(
            validate_explicit_self_hosted_target(
                &declared,
                &target,
                "cerebras",
                "test-cerebras-model"
            )
            .is_err()
        );
    }

    #[test]
    fn explicit_target_without_a_declaration_is_accepted() {
        let mut config = base_config();
        add_binding(&mut config, "ws", "local", None, None, true);
        let target =
            resolve_self_hosted_binding_for_server(&config, "cerebras", Some(&realm("ws")))
                .expect("unconstrained binding resolves");
        validate_explicit_self_hosted_target(&config, &target, "muse_vllm", "test-muse-model")
            .expect("an unconstrained binding remains the operator's escape hatch");
    }
}
