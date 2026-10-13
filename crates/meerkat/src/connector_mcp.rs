//! The connector credential an admitted MCP tool call carries.
//!
//! [`ConnectorMcpCallContextProvider`] is an [`McpCallContextProvider`] for
//! trusted MCP server installations whose tools call a connector API with a
//! native connector credential. For a call to one of those installations it
//! admits the slot's credential for that exact call (the verified account,
//! stable context and required scopes checked on the exact credential), puts
//! it in the request's protected metadata, and attaches a final check that
//! the router runs at the call's final native entry, before the single
//! consuming reviewed entry and with no await before the request is handed
//! to the transport. A refusal at either point sends nothing and spends no
//! review. The checked credential's lifecycle guard then stays held through
//! the reviewed entry and the handoff, with the request.
//!
//! Installations come from one of two places. The live provider
//! ([`ConnectorMcpCallContextProvider::live`], the factory default) takes
//! each call's installation from that call's own staged server
//! configuration. A fixed provider ([`ConnectorMcpCallContextProvider::new`])
//! matches each call's configuration against its list by complete
//! configuration equality, never by name alone. Either way the installation
//! and destination origin are binding integrity, not permission: the call's
//! governed invocation context decides whether the actual operation may run,
//! a call without one is refused, and a server's access ceiling still
//! refuses an operation the binding lists but the ceiling does not admit.

use std::collections::BTreeMap;

use async_trait::async_trait;
use meerkat_core::types::ToolCallView;
use meerkat_core::{McpServerConfig, ToolDispatchContext};
use meerkat_mcp::{McpCallContext, McpCallContextError, McpCallContextProvider, McpCallTarget};
use meerkat_providers::connector_login::{ConnectorCallExpectation, ConnectorOAuthAuthority};

/// The protected `_meta` key a connector-bound call's credential travels in.
pub const CONNECTOR_CREDENTIAL_META_KEY: &str = "meerkat/connector_credential";

/// What one raw operation of a trusted installation needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorOperationBinding {
    pub expectation: ConnectorCallExpectation,
    /// `scheme://host[:port]` of the connector API the server calls with it.
    pub destination_origin: String,
    /// The host's typed read-only declaration for this operation.
    pub read_only: bool,
}

/// A trusted MCP server installation whose operations need a connector
/// credential: a stdio server configuration carrying a
/// [`meerkat_core::mcp_config::McpConnectorBinding`]. The binding is part of
/// the configuration the installation is matched on, and its expected account
/// reaches the server at startup in
/// [`meerkat_core::mcp_config::MCP_CONNECTOR_BINDING_ENV`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorMcpInstallation {
    config: McpServerConfig,
    operations: BTreeMap<String, ConnectorOperationBinding>,
}

impl ConnectorMcpInstallation {
    /// The installation `config` declares: `None` without a connector
    /// binding, an error for a binding that is not valid (a non-stdio
    /// server, an invalid slot or destination origin).
    pub fn from_config(config: &McpServerConfig) -> Option<Result<Self, String>> {
        let binding = config.connector.as_ref()?;
        Some(Self::from_binding(config, binding))
    }

    fn from_binding(
        config: &McpServerConfig,
        binding: &meerkat_core::mcp_config::McpConnectorBinding,
    ) -> Result<Self, String> {
        if !matches!(
            config.transport,
            meerkat_core::mcp_config::McpTransportConfig::Stdio(_)
        ) {
            return Err("a connector binding needs a stdio MCP server".to_owned());
        }
        let slot = meerkat_core::connection::CredentialAccountRef {
            realm: meerkat_core::connection::RealmId::parse(&binding.realm_id)
                .map_err(|error| format!("invalid connector realm id: {error}"))?,
            account: meerkat_core::connection::CredentialAccountId::parse(&binding.slot_id)
                .map_err(|error| format!("invalid connector slot id: {error}"))?,
        };
        let operations = binding
            .operations
            .iter()
            .map(|(operation, declared)| {
                Ok((
                    operation.clone(),
                    ConnectorOperationBinding {
                        expectation: ConnectorCallExpectation {
                            slot: slot.clone(),
                            account: binding.account.clone(),
                            issuer: binding.issuer.clone(),
                            client: binding.client.clone(),
                            resource: binding.resource.clone(),
                            strategy_id: binding.strategy_id.clone(),
                            required_scopes: declared.required_scopes.clone(),
                        },
                        destination_origin: destination_origin(&declared.destination_origin)?,
                        read_only: declared.read_only,
                    },
                ))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            config: config.clone(),
            operations,
        })
    }

    /// The exact server configuration to stage.
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

/// `scheme://host[:port]` only: https, or http to a loopback host.
fn destination_origin(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value)
        .map_err(|error| format!("invalid connector destination origin: {error}"))?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    let origin = url.origin().ascii_serialization();
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
        // Only the canonical form: the server receives this exact string at
        // startup and in every call, and compares them byte for byte.
        || origin != value
    {
        return Err(
            "connector destination origin must be https://host[:port] (http only to loopback)"
                .to_owned(),
        );
    }
    Ok(origin)
}

/// The `meerkat/connector_credential` call metadata: the admitted credential
/// and the actual verified account it is bound to. It has no `Debug`: it
/// carries the token.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCallCredentialMeta {
    /// `Bearer <access token>`.
    pub authorization: String,
    pub destination_origin: String,
    pub account: meerkat_contracts::WireConnectorVerifiedAccount,
    pub scopes: Vec<String>,
    /// RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Admits a connector credential for each call to a trusted installation.
pub struct ConnectorMcpCallContextProvider {
    authority: ConnectorOAuthAuthority,
    installations: Installations,
}

/// Where a provider's trusted installations come from.
enum Installations {
    /// A fixed list, matched on complete configuration.
    Fixed(Vec<ConnectorMcpInstallation>),
    /// The call's own staged configuration, on every call. The router holds
    /// only host-staged configuration, so a binding staged after the
    /// provider was built, or reloaded after a restart, is served too.
    Live,
}

impl ConnectorMcpCallContextProvider {
    pub fn new(
        authority: ConnectorOAuthAuthority,
        installations: Vec<ConnectorMcpInstallation>,
    ) -> Self {
        Self {
            authority,
            installations: Installations::Fixed(installations),
        }
    }

    /// The provider that takes each call's installation from the call's own
    /// staged server configuration, so servers staged later (a live add, a
    /// retained router's later staging) and reloaded after a restart are
    /// served without rebuilding it. Attach it once when the router is built.
    pub fn live(authority: ConnectorOAuthAuthority) -> Self {
        Self {
            authority,
            installations: Installations::Live,
        }
    }

    /// The provider for the connector-bound servers among `servers`, which
    /// must be trusted host configuration; `None` when none is bound. A
    /// server whose binding is not valid is left out (its calls carry no
    /// credential) with a warning.
    pub fn for_servers(
        authority: ConnectorOAuthAuthority,
        servers: &[McpServerConfig],
    ) -> Option<Self> {
        let installations: Vec<_> = servers
            .iter()
            .filter_map(
                |server| match ConnectorMcpInstallation::from_config(server)? {
                    Ok(installation) => Some(installation),
                    Err(error) => {
                        tracing::warn!(server = %server.name, %error, "connector binding refused");
                        None
                    }
                },
            )
            .collect();
        (!installations.is_empty()).then(|| Self::new(authority, installations))
    }
}

#[async_trait]
impl McpCallContextProvider for ConnectorMcpCallContextProvider {
    /// Only the host's typed per-operation declaration in the installation's
    /// binding makes an operation read-only; anything else (no binding, an
    /// invalid one, an unlisted operation, an undeclared one) is Unknown.
    fn tool_mutation_class(
        &self,
        config: &McpServerConfig,
        raw_operation: &str,
    ) -> meerkat_core::ToolMutationClass {
        let binding = match &self.installations {
            Installations::Fixed(installations) => {
                select_binding(installations, config, raw_operation)
                    .ok()
                    .flatten()
                    .cloned()
            }
            Installations::Live => live_binding(config, raw_operation).ok().flatten(),
        };
        if binding.is_some_and(|binding| binding.read_only) {
            meerkat_core::ToolMutationClass::ReadOnly
        } else {
            meerkat_core::ToolMutationClass::Unknown
        }
    }

    async fn prepare(
        &self,
        target: McpCallTarget<'_>,
        _call: ToolCallView<'_>,
        context: &ToolDispatchContext,
    ) -> Result<Option<McpCallContext>, McpCallContextError> {
        let selected = match &self.installations {
            Installations::Fixed(installations) => {
                select_binding(installations, target.config, target.raw_operation)?.cloned()
            }
            Installations::Live => live_binding(target.config, target.raw_operation)?,
        };
        let Some(binding) = selected else {
            return Ok(None);
        };
        let binding = &binding;
        // The invocation owner must authorize the actual operation; an
        // ungoverned call gets no credential.
        if context.work_authorization().is_none() {
            return Err(McpCallContextError::Denied);
        }
        let entry = self
            .authority
            .admit_call_credential(&binding.expectation)
            .await
            .map_err(call_context_error)?;
        let credential = entry.credential();
        let payload = serde_json::to_value(ConnectorCallCredentialMeta {
            authorization: format!("Bearer {}", credential.expose_access_token()),
            destination_origin: binding.destination_origin.clone(),
            account: meerkat_contracts::WireConnectorVerifiedAccount {
                issuer: binding.expectation.issuer.clone(),
                strategy_id: binding.expectation.strategy_id.clone(),
                subject: credential.account().to_owned(),
            },
            scopes: credential.scopes().to_vec(),
            expires_at: credential.expires_at().map(|at| at.to_rfc3339()),
        })
        .map_err(|_| McpCallContextError::Unavailable)?;
        let metadata =
            serde_json::Map::from_iter([(CONNECTOR_CREDENTIAL_META_KEY.into(), payload)]);
        // The entry holds the slot's lifecycle guard from here through the
        // final check, the reviewed entry and the local handoff (the router
        // keeps a passed check's return with the request), so no refresh,
        // replacement or logout lands before the request has left the call.
        Ok(Some(McpCallContext::new(metadata, ()).with_final_check(
            move || {
                entry.recheck().map_err(call_context_error)?;
                Ok(entry)
            },
        )))
    }
}

/// A refusal of the caller's request is `Denied`; an infrastructure or
/// upstream failure is `Unavailable`.
fn call_context_error(
    error: meerkat_providers::connector_login::ConnectorLoginError,
) -> McpCallContextError {
    if error.is_refusal() {
        tracing::debug!("connector credential refused for an MCP call");
        McpCallContextError::Denied
    } else {
        tracing::debug!("connector credential unavailable for an MCP call");
        McpCallContextError::Unavailable
    }
}

/// The binding a call to `config`'s `raw_operation` needs, taken from the
/// call's own staged configuration: `None` for a server without a connector
/// binding. A binding that is not valid serves no call (`Unavailable`), and
/// an operation it does not list is refused (`Denied`); neither is sent
/// unbound.
fn live_binding(
    config: &McpServerConfig,
    raw_operation: &str,
) -> Result<Option<ConnectorOperationBinding>, McpCallContextError> {
    match ConnectorMcpInstallation::from_config(config) {
        None => Ok(None),
        Some(Err(_)) => Err(McpCallContextError::Unavailable),
        Some(Ok(installation)) => installation
            .operations
            .get(raw_operation)
            .cloned()
            .map(Some)
            .ok_or(McpCallContextError::Denied),
    }
}

/// The binding a call to `config`'s `raw_operation` needs: `None` for a
/// server that is no trusted installation. A server that only shares an
/// installation's name, or an operation an installation does not list, is
/// refused rather than treated as unbound.
fn select_binding<'a>(
    installations: &'a [ConnectorMcpInstallation],
    config: &McpServerConfig,
    raw_operation: &str,
) -> Result<Option<&'a ConnectorOperationBinding>, McpCallContextError> {
    let Some(installation) = installations
        .iter()
        .find(|installation| &installation.config == config)
    else {
        if installations
            .iter()
            .any(|installation| installation.config.name == config.name)
        {
            return Err(McpCallContextError::Denied);
        }
        return Ok(None);
    };
    installation
        .operations
        .get(raw_operation)
        .map(Some)
        .ok_or(McpCallContextError::Denied)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn installation(command: &str) -> ConnectorMcpInstallation {
        let mut config = McpServerConfig::stdio("services", command, vec![], HashMap::new());
        config.connector = Some(meerkat_core::mcp_config::McpConnectorBinding {
            realm_id: "tenant-a".into(),
            slot_id: "mail-work".into(),
            issuer: "https://issuer.example".into(),
            client: "client".into(),
            resource: "https://mail.example".into(),
            strategy_id: "oidc-userinfo-v1".into(),
            account: "subject-a".into(),
            operations: BTreeMap::from([(
                "gmail_read_text_part".to_owned(),
                meerkat_core::mcp_config::McpConnectorOperation {
                    required_scopes: ["mail.readonly".to_owned()].into(),
                    destination_origin: "https://mail.example".into(),
                    read_only: false,
                },
            )]),
        });
        ConnectorMcpInstallation::from_config(&config)
            .expect("bound")
            .expect("valid binding")
    }

    /// A connector binding added with persistence (the path `mcp/add` with
    /// `persisted: true` takes) survives the reload a fresh process does: the
    /// reloaded server, staged on a fresh standalone router with the live
    /// provider attached by hand (the factory default; the factory itself is
    /// not exercised here), starts with the persisted expected binding, and
    /// its bound call, made without a governed invocation, is refused locally
    /// as `Denied` without reaching the server. The same server without a
    /// binding, under the same provider, is called normally. This is not
    /// credential or account evidence.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_persisted_binding_survives_reload_and_governs_the_fresh_router() {
        use meerkat_core::mcp_config::{
            McpConfig, McpConfigMutationAuthority, McpConnectorBinding, McpConnectorOperation,
        };
        let temp = tempfile::TempDir::new().unwrap();
        let env_seen = temp.path().join("binding.env");
        let calls = temp.path().join("calls.log");
        let script = format!(
            r#"printf '%s\n' "$MEERKAT_CONNECTOR_BINDING" > '{env}'
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":"2025-03-26","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"mail","version":"0"}}}}}}\n' "$id" ;;
    *'"method":"tools/list"'*) printf '{{"jsonrpc":"2.0","id":%s,"result":{{"tools":[{{"name":"gmail_read_text_part","inputSchema":{{"type":"object"}}}}]}}}}\n' "$id" ;;
    *'"method":"tools/call"'*) printf '%s\n' "$line" >> '{calls}'; printf '{{"jsonrpc":"2.0","id":%s,"result":{{"content":[{{"type":"text","text":"part"}}]}}}}\n' "$id" ;;
  esac
done
"#,
            env = env_seen.display(),
            calls = calls.display()
        );
        let unbound = McpServerConfig::stdio(
            "mail",
            "/bin/sh",
            vec!["-c".to_owned(), script],
            HashMap::new(),
        );
        let mut bound = unbound.clone();
        bound.connector = Some(McpConnectorBinding {
            realm_id: "tenant-a".into(),
            slot_id: "mail-work".into(),
            issuer: "https://issuer.example".into(),
            client: "client".into(),
            resource: "https://mail.example".into(),
            strategy_id: "oidc-userinfo-v1".into(),
            account: "subject-a".into(),
            operations: BTreeMap::from([(
                "gmail_read_text_part".to_owned(),
                McpConnectorOperation {
                    required_scopes: ["mail.readonly".to_owned()].into(),
                    destination_origin: "https://mail.example".into(),
                    read_only: true,
                },
            )]),
        });

        // Persist the way a persisted `mcp/add` does, then reload as a fresh
        // process would.
        let rollback = McpConfig::persist_add_with_rollback(
            &McpConfigMutationAuthority::project(Some(temp.path().to_path_buf()), None),
            bound.clone(),
        )
        .await
        .unwrap();
        let reloaded = McpConfig::load_from_paths(None, Some(&temp.path().join(".rkat/mcp.toml")))
            .await
            .unwrap()
            .servers;
        assert_eq!(
            reloaded,
            vec![bound.clone()],
            "the binding survived the reload"
        );

        let runtime = meerkat_runtime::MeerkatMachine::ephemeral();
        let authority = crate::HostAuthService::new(
            meerkat_providers::auth_store::ProviderAuthPersistence::new(
                Arc::new(meerkat_providers::auth_store::EphemeralTokenStore::new()),
                Arc::new(meerkat_providers::auth_store::InMemoryCoordinator::new()),
            ),
            runtime.provider_auth_runtime_authority(),
        )
        .connector_oauth_authority()
        .unwrap();
        let provider = Arc::new(ConnectorMcpCallContextProvider::live(authority));
        let control_provider = Arc::clone(&provider);
        assert_eq!(
            provider.tool_mutation_class(&reloaded[0], "gmail_read_text_part"),
            meerkat_core::ToolMutationClass::ReadOnly,
            "the persisted read-only declaration classifies the operation"
        );
        let mut router = crate::mcp::standalone_router().with_call_context_provider(provider);
        router.add_server(reloaded[0].clone()).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(&env_seen).unwrap().trim_end(),
            bound
                .connector
                .as_ref()
                .unwrap()
                .startup_projection()
                .to_string(),
            "the reloaded server started with the persisted expected binding"
        );
        let refused = router
            .call_tool("gmail_read_text_part", &serde_json::json!({}))
            .await;
        assert!(
            matches!(
                refused,
                Err(meerkat_mcp::McpError::CallContext(
                    McpCallContextError::Denied
                ))
            ),
            "a bound call without a governed invocation is refused as Denied: {:?}",
            refused.err()
        );
        assert!(
            std::fs::read_to_string(&calls)
                .unwrap_or_default()
                .is_empty(),
            "nothing reached the server"
        );
        router.shutdown().await;

        // Control: the same server without a binding, under the same
        // provider, is called normally.
        let mut plain =
            crate::mcp::standalone_router().with_call_context_provider(control_provider);
        plain.add_server(unbound).await.unwrap();
        plain
            .call_tool("gmail_read_text_part", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(!std::fs::read_to_string(&calls).unwrap().is_empty());
        plain.shutdown().await;
        rollback.rollback().await.unwrap();
    }

    #[test]
    fn refusals_stay_denied_and_infrastructure_failures_unavailable() {
        use meerkat_providers::connector_login::ConnectorLoginError;
        assert_eq!(
            call_context_error(ConnectorLoginError::ReauthRequired),
            McpCallContextError::Denied
        );
        assert_eq!(
            call_context_error(ConnectorLoginError::TokenStore("store".into())),
            McpCallContextError::Unavailable
        );
        assert_eq!(
            call_context_error(ConnectorLoginError::AuthLifecycle("lease".into())),
            McpCallContextError::Unavailable
        );
    }

    #[test]
    fn invalid_bindings_and_unbound_servers_are_not_installations() {
        let unbound = McpServerConfig::stdio("files", "/opt/files", vec![], HashMap::new());
        assert!(ConnectorMcpInstallation::from_config(&unbound).is_none());
        let mut http =
            McpServerConfig::streamable_http("svc", "https://svc.example/mcp", HashMap::new());
        http.connector = installation("/opt/services-mcp").config.connector;
        assert!(
            ConnectorMcpInstallation::from_config(&http)
                .unwrap()
                .is_err()
        );
        for origin in [
            "http://mail.example",
            "https://mail.example/",
            "https://mail.example/v1",
            "https://mail.example?x=1",
        ] {
            let mut config = installation("/opt/services-mcp").config;
            let binding = config.connector.as_mut().unwrap();
            binding
                .operations
                .get_mut("gmail_read_text_part")
                .unwrap()
                .destination_origin = origin.to_owned();
            assert!(
                ConnectorMcpInstallation::from_config(&config)
                    .unwrap()
                    .is_err(),
                "{origin}"
            );
        }
    }

    #[test]
    fn live_bindings_come_from_the_calls_own_configuration() {
        let bound = installation("/opt/services-mcp").config;
        assert_eq!(
            live_binding(&bound, "gmail_read_text_part")
                .unwrap()
                .map(|binding| binding.destination_origin),
            Some("https://mail.example".to_owned())
        );
        assert_eq!(
            live_binding(&bound, "gmail_send").err(),
            Some(McpCallContextError::Denied)
        );
        let mut invalid = bound;
        invalid
            .connector
            .as_mut()
            .unwrap()
            .operations
            .get_mut("gmail_read_text_part")
            .unwrap()
            .destination_origin = "https://mail.example/".into();
        assert_eq!(
            live_binding(&invalid, "gmail_read_text_part").err(),
            Some(McpCallContextError::Unavailable)
        );
        let unbound = McpServerConfig::stdio("files", "/opt/files", vec![], HashMap::new());
        assert!(live_binding(&unbound, "read").unwrap().is_none());

        // Only the typed per-operation declaration makes an operation
        // read-only.
        let mut declared = installation("/opt/services-mcp").config;
        assert!(
            !live_binding(&declared, "gmail_read_text_part")
                .unwrap()
                .unwrap()
                .read_only
        );
        declared
            .connector
            .as_mut()
            .unwrap()
            .operations
            .get_mut("gmail_read_text_part")
            .unwrap()
            .read_only = true;
        assert!(
            live_binding(&declared, "gmail_read_text_part")
                .unwrap()
                .unwrap()
                .read_only
        );
    }

    #[test]
    fn the_startup_projection_carries_the_expected_account_per_operation() {
        let projection = installation("/opt/services-mcp")
            .config
            .connector
            .unwrap()
            .startup_projection();
        assert_eq!(
            projection,
            serde_json::json!({
                "version": 1,
                "operations": {"gmail_read_text_part": {
                    "account": {
                        "issuer": "https://issuer.example",
                        "strategy_id": "oidc-userinfo-v1",
                        "subject": "subject-a",
                    },
                    "destination_origin": "https://mail.example",
                }},
            })
        );
    }

    #[test]
    fn only_the_exact_trusted_installation_and_its_listed_operations_bind() {
        let trusted = [installation("/opt/services-mcp")];
        let exact = trusted[0].config.clone();
        assert_eq!(
            select_binding(&trusted, &exact, "gmail_read_text_part")
                .unwrap()
                .map(|binding| binding.destination_origin.as_str()),
            Some("https://mail.example")
        );
        // An operation the installation does not list is refused.
        assert_eq!(
            select_binding(&trusted, &exact, "send_message").err(),
            Some(McpCallContextError::Denied)
        );
        // The same name with another launch is not the trusted installation.
        let impostor = installation("/opt/other-services-mcp").config;
        assert_eq!(
            select_binding(&trusted, &impostor, "gmail_read_text_part").err(),
            Some(McpCallContextError::Denied)
        );
        // Any other server is unbound and keeps its payload.
        let unrelated = McpServerConfig::stdio("files", "/opt/files", vec![], HashMap::new());
        assert!(
            select_binding(&trusted, &unrelated, "gmail_read_text_part")
                .unwrap()
                .is_none()
        );
    }
}
