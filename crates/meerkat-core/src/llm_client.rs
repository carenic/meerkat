//! Per-request authorization forwarding for existing model clients.
//!
//! This is operation data and a retained feature handle, not a provider policy,
//! credential grant, or session default. The actual provider supplies its final
//! resolved target and retains the returned check beside that exact request.

use crate::authorization::{
    AuthorizationOperation, ModelAuthorizationFacts, ModelAuthorizationUse,
    OperationAuthorizationError, OperationAuthorizationFacts, PreparedAuthorizationBinding,
    PreparedOperationCheck, WorkAuthorizationContext,
};
use crate::live_execution::CanonicalContextRevision;
use crate::{AuthCredentialIdentity, OperationId, RunId, SessionLlmIdentity};

/// Exact nonsecret model selection retained by the native work owner.
///
/// This is a projection of the existing provider factory's resolved target,
/// not authority to use it. The native owner must compare its admitted
/// selection with final operation facts and the current controller grant.
/// A credential binding identifies the selected credential owner; it does not
/// assert the external account behind a dynamically refreshed credential.
/// Request parameters are intentionally excluded: temperature and hosted-tool
/// toggles do not select another route. Their actual effects remain operation
/// facts evaluated by the current owner at request preparation.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ControllerModelSelection {
    model: String,
    provider: crate::Provider,
    self_hosted_server_id: Option<String>,
    auth_binding: Option<crate::AuthBindingRef>,
    credential: AuthCredentialIdentity,
    backend_profile_id: String,
    backend_kind: String,
    /// Absent from older records, which therefore stay credential-required.
    #[serde(
        default,
        skip_serializing_if = "ControllerCredentialRequirement::is_required"
    )]
    credential_requirement: ControllerCredentialRequirement,
}

/// Whether the selected route's actual resolved connection holds credential
/// material. Only the provider factory's resolution of the connection decides
/// this: an explicitly authless route (a self-hosted server configured
/// without auth) resolves to [`ControllerCredentialRequirement::Authless`]. A
/// missing lease, a missing secret or a caller's claim never does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ControllerCredentialRequirement {
    /// The route uses credential material whose lifecycle native admission
    /// must hold.
    #[default]
    Required,
    /// The resolved connection is explicitly authless: there is no credential
    /// lifecycle to hold.
    Authless,
}

impl ControllerCredentialRequirement {
    pub fn is_required(&self) -> bool {
        matches!(self, Self::Required)
    }
}

impl ControllerModelSelection {
    pub fn new(
        identity: SessionLlmIdentity,
        credential: AuthCredentialIdentity,
        backend_profile_id: String,
        backend_kind: String,
    ) -> Self {
        Self {
            model: identity.model,
            provider: identity.provider,
            self_hosted_server_id: identity.self_hosted_server_id,
            auth_binding: identity.auth_binding,
            credential,
            backend_profile_id,
            backend_kind,
            credential_requirement: ControllerCredentialRequirement::Required,
        }
    }

    /// The selection with the credential requirement its actual resolved
    /// connection reports.
    #[must_use]
    pub fn with_credential_requirement(
        mut self,
        requirement: ControllerCredentialRequirement,
    ) -> Self {
        self.credential_requirement = requirement;
        self
    }

    pub fn credential_requirement(&self) -> ControllerCredentialRequirement {
        self.credential_requirement
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn provider(&self) -> crate::Provider {
        self.provider
    }

    pub fn self_hosted_server_id(&self) -> Option<&str> {
        self.self_hosted_server_id.as_deref()
    }

    pub fn auth_binding(&self) -> Option<&crate::AuthBindingRef> {
        self.auth_binding.as_ref()
    }

    pub fn credential(&self) -> &AuthCredentialIdentity {
        &self.credential
    }

    pub fn backend_profile_id(&self) -> &str {
        &self.backend_profile_id
    }

    pub fn backend_kind(&self) -> &str {
        &self.backend_kind
    }

    /// Compare the admitted selection with actual resolved operation data.
    /// Endpoint, wire model and hosted capabilities still require the current
    /// owner's grant evaluation; this equality is not an authorization result.
    pub fn matches_model_facts(&self, facts: &ModelAuthorizationFacts) -> bool {
        self.model == facts.identity.model
            && self.provider == facts.identity.provider
            && self.self_hosted_server_id == facts.identity.self_hosted_server_id
            && self.auth_binding == facts.identity.auth_binding
            && facts.credential.as_ref() == Some(&self.credential)
            && facts.backend_profile_id.as_deref() == Some(self.backend_profile_id.as_str())
            && facts.backend_kind.as_ref() == self.backend_kind
    }
}

impl std::fmt::Debug for ControllerModelSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ControllerModelSelection([REDACTED])")
    }
}

/// Request-free facts supplied by the actual immutable selected client.
///
/// This is nonserializable data, never account permission or a request. The
/// trusted provider must use the same endpoint/model lowering as its transport.
#[derive(Clone)]
pub struct ControllerModelFacts {
    selection: ControllerModelSelection,
    endpoint: std::sync::Arc<str>,
    wire_model: std::sync::Arc<str>,
}

impl ControllerModelFacts {
    pub fn new(
        selection: ControllerModelSelection,
        endpoint: std::sync::Arc<str>,
        wire_model: std::sync::Arc<str>,
    ) -> Self {
        Self {
            selection,
            endpoint,
            wire_model,
        }
    }

    pub fn selection(&self) -> &ControllerModelSelection {
        &self.selection
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn wire_model(&self) -> &str {
        &self.wire_model
    }
}

impl std::fmt::Debug for ControllerModelFacts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ControllerModelFacts([REDACTED])")
    }
}

impl ControllerModelFacts {
    /// The canonical key of this actual lowered route
    /// ([`ControllerRouteKey`]).
    pub fn route_key(&self) -> ControllerRouteKey {
        ControllerRouteKey::of(ControllerRouteParts {
            provider: self.selection.provider,
            model: &self.selection.model,
            self_hosted_server_id: self.selection.self_hosted_server_id.as_deref(),
            auth_binding: self.selection.auth_binding.as_ref(),
            credential: &self.selection.credential,
            backend_profile_id: &self.selection.backend_profile_id,
            backend_kind: &self.selection.backend_kind,
            endpoint: &self.endpoint,
            wire_model: &self.wire_model,
        })
    }
}

/// The one complete actual lowered route a controller key is computed from.
/// Every field is a selected, nonsecret fact: the credential is the stable
/// credential owner (binding or account), never a token, token hash or
/// refresh generation. The endpoint and wire model are the provider's actual
/// lowering, used as they are.
pub(crate) struct ControllerRouteParts<'a> {
    pub(crate) provider: crate::Provider,
    pub(crate) model: &'a str,
    pub(crate) self_hosted_server_id: Option<&'a str>,
    pub(crate) auth_binding: Option<&'a crate::AuthBindingRef>,
    pub(crate) credential: &'a AuthCredentialIdentity,
    pub(crate) backend_profile_id: &'a str,
    pub(crate) backend_kind: &'a str,
    pub(crate) endpoint: &'a str,
    pub(crate) wire_model: &'a str,
}

/// The canonical key of one actual lowered controller route:
/// `model-route/v1/<sha256 hex>` over a versioned, injective, typed encoding
/// of the complete route (provider, model, self-hosted server, auth binding,
/// stable credential owner, backend profile and kind, endpoint and wire
/// model), with explicit variant and option tags.
///
/// The same key is computed at native admission from the selected client's
/// facts and at every controller request from that request's actual facts,
/// so a grant naming it admits that route only. A key is a route identity,
/// not proof of the external account behind a dynamic binding, and it
/// grants nothing by itself.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControllerRouteKey(String);

impl ControllerRouteKey {
    /// The prefix every key carries, naming its encoding version.
    pub const PREFIX: &'static str = "model-route/v1/";

    /// A key from its text: the prefix and 64 lowercase hex digits.
    pub fn parse(value: &str) -> Option<Self> {
        let digest = value.strip_prefix(Self::PREFIX)?;
        (digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn of(parts: ControllerRouteParts<'_>) -> Self {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(b"meerkat/controller-route\0v1\0");
        let mut field = |tag: u8, value: &str| {
            hash.update([tag]);
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        };
        field(1, parts.provider.as_str());
        field(2, parts.model);
        match parts.self_hosted_server_id {
            None => field(3, ""),
            Some(server) => {
                field(4, server);
            }
        }
        match parts.auth_binding {
            None => field(5, ""),
            Some(binding) => {
                field(6, binding.realm.as_str());
                field(7, binding.binding.as_str());
                match &binding.profile {
                    None => field(8, ""),
                    Some(profile) => field(9, profile.as_str()),
                }
                field(10, binding_origin(&binding.origin));
            }
        }
        match parts.credential {
            AuthCredentialIdentity::Binding(binding) => {
                field(11, binding.realm.as_str());
                field(12, binding.binding.as_str());
                match &binding.profile {
                    None => field(13, ""),
                    Some(profile) => field(14, profile.as_str()),
                }
                field(15, binding_origin(&binding.origin));
            }
            AuthCredentialIdentity::Account(account) => {
                field(16, account.realm.as_str());
                field(17, account.account.as_str());
            }
        }
        field(18, parts.backend_profile_id);
        field(19, parts.backend_kind);
        field(20, parts.endpoint);
        field(21, parts.wire_model);
        let digest = hash.finalize();
        let mut key = String::with_capacity(Self::PREFIX.len() + 64);
        key.push_str(Self::PREFIX);
        for byte in digest {
            key.push_str(&format!("{byte:02x}"));
        }
        Self(key)
    }
}

fn binding_origin(origin: &crate::BindingOrigin) -> &'static str {
    match origin {
        crate::BindingOrigin::Configured => "configured",
        crate::BindingOrigin::SyntheticEnvDefault => "synthetic-env-default",
    }
}

impl std::fmt::Debug for ControllerRouteKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ControllerRouteKey")
            .field(&self.0)
            .finish()
    }
}

impl std::fmt::Display for ControllerRouteKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The selected client cannot provide coherent request-free route facts.
/// This is a setup/data error, not a permission denial or a model response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("controller model facts unavailable")]
pub struct ControllerFactsUnavailable;

/// Runnable client retained by its existing selection owner for one work item.
///
/// A mutable fallback selector must pin its selected child, not itself. This
/// handle keeps that actual child alive; its selection remains data that the
/// native admission owner must match against the admitted controller grant.
/// Trusted custom hosts must uphold the same immutable-selection contract.
/// The handle is deliberately not serializable.
#[derive(Clone)]
pub struct ControllerModelClient {
    selection: ControllerModelSelection,
    client: std::sync::Arc<dyn crate::AgentLlmClient>,
}

impl ControllerModelClient {
    pub fn new(
        selection: ControllerModelSelection,
        client: std::sync::Arc<dyn crate::AgentLlmClient>,
    ) -> Self {
        Self { selection, client }
    }

    pub fn selection(&self) -> &ControllerModelSelection {
        &self.selection
    }

    pub fn client(&self) -> &std::sync::Arc<dyn crate::AgentLlmClient> {
        &self.client
    }

    /// Run existing-credential maintenance on this exact retained child.
    /// Selection is checked on both sides of the await; maintenance cannot
    /// silently select a fallback or replace the admitted controller identity.
    pub async fn prepare_controller_credential(&self) -> Result<(), crate::auth::AuthError> {
        if self.client.controller_model_selection().as_ref() != Some(&self.selection) {
            return Err(crate::auth::AuthError::StaleCredential);
        }
        self.client.prepare_controller_credential().await?;
        if self.client.controller_model_selection().as_ref() != Some(&self.selection) {
            return Err(crate::auth::AuthError::StaleCredential);
        }
        Ok(())
    }

    /// Read the pinned child's route without constructing or sending a request.
    /// Custom clients must uphold the immutable selection contract.
    pub fn plain_facts(&self) -> Result<ControllerModelFacts, ControllerFactsUnavailable> {
        if self.client.controller_model_selection().as_ref() != Some(&self.selection) {
            return Err(ControllerFactsUnavailable);
        }
        let facts = self.client.controller_model_facts()?;
        if facts.selection() != &self.selection {
            return Err(ControllerFactsUnavailable);
        }
        Ok(facts)
    }
}

impl std::fmt::Debug for ControllerModelClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ControllerModelClient([REDACTED])")
    }
}

/// The authorization context of one model operation in admitted work.
///
/// Cloning forwards the same work association. It does not authorize another
/// payload, provider, account or operation, and has no serialized form. Actor,
/// requester, represented subject and delegation stay with the feature-owned
/// work association; provider credentials never replace those identities.
#[derive(Clone)]
pub struct LlmRequestAuthorization {
    work: WorkAuthorizationContext,
    operation_id: OperationId,
    run_id: Option<RunId>,
    context_revision: Option<CanonicalContextRevision>,
    usage: ModelAuthorizationUse,
    review_attribution: Option<crate::approval::review::ReviewOperationAttribution>,
    delegate_attribution: Option<crate::authorization::ToolDelegateAttribution>,
}

impl LlmRequestAuthorization {
    pub fn new(
        work: WorkAuthorizationContext,
        operation_id: OperationId,
        usage: ModelAuthorizationUse,
    ) -> Self {
        Self {
            work,
            operation_id,
            run_id: None,
            context_revision: None,
            usage,
            review_attribution: None,
            delegate_attribution: None,
        }
    }

    /// A fresh independently authorized reviewer inference under the exact
    /// candidate's work and coordinates. No controller exemption is inherited.
    pub fn for_operation_review(
        attribution: crate::approval::review::ReviewOperationAttribution,
    ) -> Self {
        let facts = attribution.candidate_binding().facts();
        Self {
            work: attribution.work_authorization().clone(),
            operation_id: OperationId::new(),
            run_id: facts.run_id.clone(),
            context_revision: facts.context_revision.clone(),
            usage: ModelAuthorizationUse::Inference,
            review_attribution: Some(attribution),
            delegate_attribution: None,
        }
    }

    /// A builtin tool's own model request as the typed delegate of the call
    /// that entered the tool: a fresh inference operation under that call's
    /// work and coordinates. It is decided by its own owners from its own
    /// facts; nothing of the call's decision is inherited, and preparation
    /// refuses facts that are not the delegate role's own.
    pub fn for_tool_delegate(attribution: crate::authorization::ToolDelegateAttribution) -> Self {
        let facts = attribution.parent_binding().facts();
        Self {
            work: attribution.work_authorization().clone(),
            operation_id: OperationId::new(),
            run_id: facts.run_id.clone(),
            context_revision: facts.context_revision.clone(),
            usage: ModelAuthorizationUse::Inference,
            review_attribution: None,
            delegate_attribution: Some(attribution),
        }
    }

    /// Retain coordinates supplied by the existing work/turn owner. Missing
    /// coordinates remain absent rather than being inferred from a session.
    pub fn with_coordinates(
        mut self,
        run_id: Option<RunId>,
        context_revision: Option<CanonicalContextRevision>,
    ) -> Self {
        self.run_id = run_id;
        self.context_revision = context_revision;
        self
    }

    pub fn usage(&self) -> ModelAuthorizationUse {
        self.usage
    }

    /// The actual operation this request authorization belongs to.
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    /// Prepare once for the actual immutable request and final provider facts.
    /// The sink must call `current` after relevant waits and before sending.
    /// Any target or payload change requires a new call to this method.
    pub fn prepare(
        &self,
        mut target: ModelAuthorizationFacts,
    ) -> Result<PreparedOperationCheck, OperationAuthorizationError> {
        target.usage = self.usage;
        let facts = OperationAuthorizationFacts {
            operation_id: self.operation_id.clone(),
            execution_scope: self.work.execution_scope().clone(),
            run_id: self.run_id.clone(),
            context_revision: self.context_revision.clone(),
            operation: AuthorizationOperation::Model(target),
        };
        let binding = match (&self.review_attribution, &self.delegate_attribution) {
            (Some(attribution), None) => attribution.bind_child(
                facts,
                &self.work,
                crate::approval::review::ReviewOperationRole::ReviewerInference,
            )?,
            (None, Some(attribution)) => attribution.bind_child(facts, &self.work)?,
            (None, None) => PreparedAuthorizationBinding::new(facts),
            // No constructor sets both; an operation has one origin.
            (Some(_), Some(_)) => {
                return Err(crate::authorization::OperationRefused::new(
                    crate::authorization::OperationRefusalKind::MalformedFacts,
                )
                .into());
            }
        };
        PreparedOperationCheck::prepare(self.work.clone(), binding)
    }
}

impl std::fmt::Debug for LlmRequestAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LlmRequestAuthorization([REDACTED])")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn identity() -> SessionLlmIdentity {
        SessionLlmIdentity {
            model: "controller-model-canary".to_owned(),
            provider: crate::Provider::OpenAI,
            self_hosted_server_id: None,
            provider_params: None,
            auth_binding: Some(crate::AuthBindingRef {
                realm: crate::RealmId::parse("controller-realm").unwrap(),
                binding: crate::BindingId::parse("controller-binding").unwrap(),
                profile: None,
                origin: crate::BindingOrigin::Configured,
            }),
        }
    }

    fn selection() -> ControllerModelSelection {
        let identity = identity();
        let credential = AuthCredentialIdentity::Binding(identity.auth_binding.clone().unwrap());
        ControllerModelSelection::new(
            identity,
            credential,
            "controller-profile-canary".to_owned(),
            "openai".to_owned(),
        )
    }

    fn facts() -> ModelAuthorizationFacts {
        let identity = identity();
        ModelAuthorizationFacts {
            credential: Some(AuthCredentialIdentity::Binding(
                identity.auth_binding.clone().unwrap(),
            )),
            identity: Arc::new(identity),
            wire_model: Arc::from("actual-wire-model"),
            hosted_capabilities: Arc::from([]),
            backend_profile_id: Some(Arc::from("controller-profile-canary")),
            backend_kind: Arc::from("openai"),
            endpoint: Arc::from("https://example.invalid/responses"),
            usage: ModelAuthorizationUse::ControllerInference,
            live_channel: None,
        }
    }

    #[test]
    fn controller_selection_matches_every_route_coordinate() {
        let selected = selection();
        assert!(selected.matches_model_facts(&facts()));
        let mutations: [fn(&mut ModelAuthorizationFacts); 7] = [
            |f| Arc::make_mut(&mut f.identity).model = "other".to_owned(),
            |f| Arc::make_mut(&mut f.identity).provider = crate::Provider::Anthropic,
            |f| Arc::make_mut(&mut f.identity).self_hosted_server_id = Some("other".to_owned()),
            |f| Arc::make_mut(&mut f.identity).auth_binding = None,
            |f| f.credential = None,
            |f| f.backend_profile_id = None,
            |f| f.backend_kind = Arc::from("other"),
        ];
        for mutate in mutations {
            let mut changed = facts();
            mutate(&mut changed);
            assert!(!selected.matches_model_facts(&changed));
        }
    }

    #[test]
    fn the_route_key_is_the_same_at_admission_and_request_and_covers_every_coordinate() {
        let admitted = ControllerModelFacts::new(
            selection(),
            Arc::from("https://example.invalid/responses"),
            Arc::from("actual-wire-model"),
        )
        .route_key();
        assert_eq!(facts().controller_route_key(), Some(admitted.clone()));
        assert_eq!(
            ControllerRouteKey::parse(admitted.as_str()),
            Some(admitted.clone())
        );
        let mutations: [fn(&mut ModelAuthorizationFacts); 13] = [
            |f| Arc::make_mut(&mut f.identity).model = "other".to_owned(),
            |f| Arc::make_mut(&mut f.identity).provider = crate::Provider::Anthropic,
            |f| Arc::make_mut(&mut f.identity).self_hosted_server_id = Some(String::new()),
            |f| Arc::make_mut(&mut f.identity).auth_binding = None,
            |f| {
                if let Some(binding) = &mut Arc::make_mut(&mut f.identity).auth_binding {
                    binding.origin = crate::BindingOrigin::SyntheticEnvDefault;
                }
            },
            |f| {
                f.credential = Some(AuthCredentialIdentity::Account(
                    crate::connection::CredentialAccountRef {
                        realm: crate::RealmId::parse("plain-controller-test").unwrap(),
                        account: crate::connection::CredentialAccountId::parse("other").unwrap(),
                    },
                ));
            },
            |f| f.backend_profile_id = Some(Arc::from("other-profile")),
            |f| f.backend_kind = Arc::from("other"),
            |f| f.endpoint = Arc::from("https://another.invalid/responses"),
            |f| f.endpoint = Arc::from("https://example.invalid/responses/"),
            |f| f.wire_model = Arc::from("another-wire-model"),
            |f| f.credential = None,
            |f| f.backend_profile_id = None,
        ];
        for (index, mutate) in mutations.into_iter().enumerate() {
            let mut changed = facts();
            mutate(&mut changed);
            assert_ne!(
                changed.controller_route_key(),
                Some(admitted.clone()),
                "mutation {index} must change the route"
            );
        }
        // Request parameters and hosted capabilities are not the route.
        let mut parameters = facts();
        Arc::make_mut(&mut parameters.identity).provider_params =
            Some(crate::lifecycle::run_primitive::ProviderParamsOverride {
                temperature: Some(0.75),
                ..Default::default()
            });
        parameters.hosted_capabilities = Arc::from([crate::ServerToolKind::WebSearch]);
        assert_eq!(parameters.controller_route_key(), Some(admitted));
    }

    #[test]
    fn a_route_key_parses_only_its_own_versioned_shape() {
        for invalid in [
            "model-route",
            "model-route/v1/",
            "model-route/v2/0000000000000000000000000000000000000000000000000000000000000000",
            "model-route/v1/000000000000000000000000000000000000000000000000000000000000000",
            "model-route/v1/000000000000000000000000000000000000000000000000000000000000000A",
        ] {
            assert_eq!(ControllerRouteKey::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn controller_route_excludes_request_parameters_but_does_not_authorize_them() {
        let selected = selection();
        let mut changed = facts();
        Arc::make_mut(&mut changed.identity).provider_params =
            Some(crate::lifecycle::run_primitive::ProviderParamsOverride {
                temperature: Some(0.75),
                ..Default::default()
            });
        changed.wire_model = Arc::from("another-lowered-model");
        changed.endpoint = Arc::from("https://another.invalid/messages");
        changed.hosted_capabilities = Arc::from([crate::ServerToolKind::WebSearch]);
        assert!(selected.matches_model_facts(&changed));
        // These remain independently evaluated actual facts. Matching a route
        // is intentionally not the operation owner's permission decision.
        assert_eq!(
            selected,
            ControllerModelSelection::new(
                (*changed.identity).clone(),
                changed.credential.unwrap(),
                selected.backend_profile_id().to_owned(),
                selected.backend_kind().to_owned(),
            )
        );
    }

    #[test]
    fn controller_selection_is_strict_data_with_protected_debug() {
        fn requires_eq<T: Eq>() {}
        requires_eq::<ControllerModelSelection>();
        let selected = selection();
        let value = serde_json::to_value(&selected).unwrap();
        assert_eq!(
            serde_json::from_value::<ControllerModelSelection>(value.clone()).unwrap(),
            selected
        );
        assert_eq!(
            format!("{selected:?}"),
            "ControllerModelSelection([REDACTED])"
        );
        assert!(!value.to_string().contains("provider_params"));
        let mut unknown = value.clone();
        unknown["external_account_inferred_from_headers"] = serde_json::json!("forged");
        assert!(serde_json::from_value::<ControllerModelSelection>(unknown).is_err());
        let mut missing = value;
        missing.as_object_mut().unwrap().remove("credential");
        assert!(serde_json::from_value::<ControllerModelSelection>(missing).is_err());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
#[path = "llm_client/controller_facts_tests.rs"]
mod controller_facts_tests;
