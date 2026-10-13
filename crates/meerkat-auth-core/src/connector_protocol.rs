//! Provider protocol components for connector OAuth logins.
//!
//! The generic connector owner ([`crate::connector_login`]) speaks RFC 6749
//! with RFC 8414 discovery. Some providers deviate in ways a parameter cannot
//! express: fixed endpoints without discovery metadata, a scope parameter of
//! another name, a token response that nests the user token, success-status
//! error bodies, refresh-token rotation rules, and their own way of saying an
//! access token is no longer valid. A [`ConnectorProtocol`] is the one
//! trusted, host-installed component that knows those facts for one
//! provider.
//!
//! It formats the admitted request and decodes the provider's response into
//! the owner's normalized grant plus private, normalized evidence. It holds
//! no authority: the connector owner still runs the flow, the account
//! strategy still proves the account, and the refresh coordinator and the
//! atomic credential commit still decide what is stored. A protocol is bound
//! by id through the host's client-auth profile, which the admitted
//! descriptor and the stored credential name, so a login and every refresh of
//! its credential use the same protocol.

use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::Client;
use sha2::{Digest, Sha256};

use crate::auth_oauth::OAuthTokenResult;
use crate::connector_login::{ConnectorAccountStrategy, ConnectorLoginError};
use crate::connector_oauth::{
    AccountSelection, ConnectorAccountObservation, ConnectorOAuthDescriptor,
    ConnectorOAuthParameters, ConnectorOAuthRefusal,
};
use crate::mcp_oauth::{CredentialHttpClientUnavailable, is_loopback_url, no_redirect_client};

/// Authorization and token endpoints a protocol fixes for its provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorProtocolEndpoints {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
}

/// Which token request a response answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorTokenGrantKind {
    /// The authorization-code exchange that completes a login.
    Exchange,
    /// A refresh of a stored credential.
    Refresh,
}

pub use crate::connector_oauth::ConnectorProtocolEvidence;

/// A decoded token response: the owner's normalized grant (its `scope` is
/// space-separated) and the protocol's evidence.
#[derive(Clone)]
pub struct ConnectorProtocolGrant {
    pub tokens: OAuthTokenResult,
    pub evidence: ConnectorProtocolEvidence,
}

impl fmt::Debug for ConnectorProtocolGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectorProtocolGrant")
            .field("tokens", &self.tokens)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// Why a protocol did not accept a token response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorProtocolFailure {
    /// The provider refused the request. `code` is its error in OAuth terms
    /// (`invalid_grant` for a refresh token it no longer honours), which the
    /// refresh owner classifies like any token-endpoint refusal.
    Refused { status: u16, code: Option<String> },
    /// The response broke the protocol (missing or inconsistent fields, a
    /// failed signature, an untrusted URL). Nothing from it is used.
    Malformed(&'static str),
    /// A well-formed response that does not satisfy a local policy of the
    /// protocol (for example a login that established no durable offline
    /// access). It is neither a provider refusal nor a malformed response.
    Policy(&'static str),
}

/// What a refresh response must do with the refresh token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTokenRule {
    /// A response without a refresh token keeps the stored one; a response
    /// with one replaces it.
    RetainIfOmitted,
    /// Every refresh response must carry a replacement, and the stored one
    /// is never reused (it may already be consumed).
    ReplacementRequired,
}

/// A protocol's reading of a resource-server response to a call that used a
/// connector access token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorCredentialRejection {
    /// The provider said this access token is not (or no longer) valid.
    InvalidCredential,
}

/// The provider-specific shape of one connector's OAuth protocol. See the
/// module docs for what it may and may not decide.
pub trait ConnectorProtocol: Send + Sync {
    /// The id a host client-auth profile names this protocol by.
    fn protocol_id(&self) -> &str;

    /// The account strategy every login under this protocol must use: the
    /// strategy reads this protocol's evidence.
    fn strategy_id(&self) -> &str;

    /// The protocol's canonical non-secret meaning, `<protocol id>@<digest>`
    /// over its configuration (endpoints, pins, refresh rules). It is bound
    /// into the admitted descriptor and the credential's metadata; a
    /// completion or refresh under a protocol whose meaning changed under
    /// the same id is refused.
    fn binding(&self) -> String;

    /// Fixed endpoints for the admitted facts, or `None` to discover them
    /// from the issuer (RFC 8414 / OpenID Connect).
    fn endpoints(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Option<ConnectorProtocolEndpoints>, ConnectorOAuthRefusal>;

    /// The authorize-request parameters that carry the admitted scopes and
    /// any account hint, replacing the default `scope` parameter. The owner
    /// adds `response_type`, `client_id`, `redirect_uri`, PKCE and `state`;
    /// a protocol never sets those.
    fn authorize_params(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Vec<(String, String)>, ConnectorOAuthRefusal>;

    /// Decode one token-endpoint response. `client_secret` is the profile's
    /// secret, for protocols whose responses are signed with it.
    fn decode_token_response(
        &self,
        kind: ConnectorTokenGrantKind,
        status: u16,
        body: &[u8],
        client_secret: Option<&str>,
    ) -> Result<ConnectorProtocolGrant, ConnectorProtocolFailure>;

    /// What a refresh response must do with the refresh token.
    fn refresh_token_rule(&self) -> RefreshTokenRule;

    /// Whether a refresh response must state its granted scopes (otherwise
    /// an omitted `scope` keeps the original grant).
    fn refresh_requires_scope(&self) -> bool {
        false
    }

    /// Classify a resource-server response to a call made with a connector
    /// access token. `None` is not a credential rejection, and nothing is
    /// refreshed or replayed for it.
    fn classify_rejection(&self, status: u16, body: &[u8]) -> Option<ConnectorCredentialRejection>;
}

/// `<id>@<sha256 over the length-framed fields>`.
fn protocol_binding(id: &str, fields: &[&str]) -> String {
    let mut hash = Sha256::new();
    for value in fields {
        hash.update(value.len().to_string().as_bytes());
        hash.update(b":");
        hash.update(value.as_bytes());
    }
    format!("{id}@{:x}", hash.finalize())
}

mod google;
pub use google::{
    GOOGLE_ISSUER, GOOGLE_PROTOCOL_ID, GOOGLE_USERINFO_STRATEGY_ID, GoogleProtocol, GoogleUserInfo,
    GoogleUserInfoStrategy, parse_google_userinfo,
};

/// The protocols a host installs, by id.
#[derive(Clone, Default)]
pub struct ConnectorProtocols {
    by_id: BTreeMap<String, std::sync::Arc<dyn ConnectorProtocol>>,
}

impl ConnectorProtocols {
    /// The built-in protocols: Slack user tokens ([`SLACK_USER_PROTOCOL_ID`]),
    /// Salesforce ([`SALESFORCE_PROTOCOL_ID`]) and Google
    /// ([`GOOGLE_PROTOCOL_ID`]), each without pins. A
    /// protocol applies only to logins whose client-auth profile names it.
    pub fn with_defaults() -> Self {
        Self::default()
            .with(std::sync::Arc::new(SlackUserProtocol::new()))
            .with(std::sync::Arc::new(SalesforceProtocol::new()))
            .with(std::sync::Arc::new(GoogleProtocol::new()))
    }

    pub fn with(mut self, protocol: std::sync::Arc<dyn ConnectorProtocol>) -> Self {
        self.by_id
            .insert(protocol.protocol_id().to_owned(), protocol);
        self
    }

    pub(crate) fn get(&self, id: &str) -> Option<&std::sync::Arc<dyn ConnectorProtocol>> {
        self.by_id.get(id)
    }
}

/// POST one token request and return the raw status and body. Redirects are
/// refused (the client follows none, and a redirect status is an error).
pub(crate) async fn post_token_form(
    http: &Client,
    token_endpoint: &str,
    form: &[(&str, String)],
) -> Result<(u16, Vec<u8>), ConnectorLoginError> {
    let response = http
        .post(token_endpoint)
        .form(form)
        .send()
        .await
        .map_err(|error| ConnectorLoginError::RefreshFailed(error.to_string()))?;
    let status = response.status();
    if status.is_redirection() {
        return Err(ConnectorLoginError::RefreshFailed(
            "connector token endpoint answered with a redirect".to_owned(),
        ));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| ConnectorLoginError::RefreshFailed(error.to_string()))?;
    Ok((status.as_u16(), body.to_vec()))
}

/// A URL a protocol may send a bearer token to: https, or loopback (tests and
/// local providers).
fn bearer_target_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https" || is_loopback_url(url)
}

/// The space-separated form of a provider scope list.
fn normalize_scopes(raw: &str, separator: char) -> String {
    raw.split(separator)
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The granted scopes an observation reports: the response's own, or the
/// admitted ones when it states none (the owner's rule for the same
/// response).
fn observed_scopes(
    descriptor: &ConnectorOAuthDescriptor,
    tokens: &OAuthTokenResult,
) -> std::collections::BTreeSet<String> {
    descriptor.granted_scopes_from_response(tokens)
}

fn json_body(body: &[u8]) -> Result<serde_json::Value, ConnectorProtocolFailure> {
    serde_json::from_slice(body)
        .map_err(|_| ConnectorProtocolFailure::Malformed("body is not JSON"))
}

fn str_field<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Slack (OAuth v2, user token)
// ---------------------------------------------------------------------------

/// Protocol id of the built-in Slack user-token protocol.
pub const SLACK_USER_PROTOCOL_ID: &str = "slack-user-v1";
/// Strategy id of the Slack `auth.test` account strategy.
pub const SLACK_AUTH_TEST_STRATEGY_ID: &str = "slack-auth-test-v1";
const SLACK_ISSUER: &str = "https://slack.com";

/// Slack token-endpoint refusals that mean the grant (code or refresh
/// token) is not honoured.
const SLACK_DEAD_REFRESH_ERRORS: [&str; 4] = [
    "invalid_refresh_token",
    "invalid_code",
    "invalid_grant",
    "token_revoked",
];

/// Slack errors that mean the access token is not (or no longer) valid.
const SLACK_INVALID_CREDENTIAL_ERRORS: [&str; 5] = [
    "invalid_auth",
    "not_authed",
    "token_expired",
    "token_revoked",
    "account_inactive",
];

/// Slack OAuth v2 for a user token, with PKCE.
///
/// - The authorize request carries the admitted scopes as `user_scope`
///   (comma-separated) and, for a known account, the workspace as `team`.
/// - The code exchange answers with the user token nested under
///   `authed_user`; a refresh answers with it at the top level.
/// - Slack reports errors as HTTP 200 with `ok: false`.
/// - Token rotation: every refresh must return a new refresh token and the
///   granted scopes.
///
/// The account is `<team id>:<user id>`, proven by `auth.test`
/// ([`SlackAuthTestStrategy`]). The app id the token was issued to is pinned
/// evidence; with `expected_app_id` set, any other app is refused.
#[derive(Clone, Debug)]
pub struct SlackUserProtocol {
    protocol_id: String,
    expected_app_id: Option<String>,
    /// Endpoint base, `https://slack.com` unless overridden for tests.
    base: String,
}

impl Default for SlackUserProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl SlackUserProtocol {
    pub fn new() -> Self {
        Self {
            protocol_id: SLACK_USER_PROTOCOL_ID.to_owned(),
            expected_app_id: None,
            base: SLACK_ISSUER.to_owned(),
        }
    }

    /// Refuse tokens issued to any app other than `app_id`.
    pub fn with_expected_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.expected_app_id = Some(app_id.into());
        self
    }

    /// Install under another id (for example one per pinned app).
    pub fn with_protocol_id(mut self, id: impl Into<String>) -> Self {
        self.protocol_id = id.into();
        self
    }

    /// Use another endpoint base (loopback fixtures).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    fn slack_error(value: &serde_json::Value) -> Option<String> {
        (value.get("ok").and_then(serde_json::Value::as_bool) != Some(true)).then(|| {
            str_field(value, "error")
                .unwrap_or("unknown_error")
                .to_owned()
        })
    }
}

/// The team and user of a Slack account string `<team>:<user>`.
fn slack_account_parts(account: &str) -> Option<(&str, &str)> {
    let (team, user) = account.split_once(':')?;
    (!team.is_empty() && !user.is_empty() && !user.contains(':')).then_some((team, user))
}

impl ConnectorProtocol for SlackUserProtocol {
    fn protocol_id(&self) -> &str {
        &self.protocol_id
    }

    fn strategy_id(&self) -> &str {
        SLACK_AUTH_TEST_STRATEGY_ID
    }

    fn binding(&self) -> String {
        protocol_binding(
            &self.protocol_id,
            &[
                "slack-user-v1",
                &self.base,
                self.expected_app_id.as_deref().unwrap_or(""),
            ],
        )
    }

    fn endpoints(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Option<ConnectorProtocolEndpoints>, ConnectorOAuthRefusal> {
        if facts.issuer.trim_end_matches('/') != self.base.trim_end_matches('/') {
            return Err(ConnectorOAuthRefusal::InvalidDescriptor);
        }
        let base = self.base.trim_end_matches('/');
        Ok(Some(ConnectorProtocolEndpoints {
            authorization_endpoint: format!("{base}/oauth/v2/authorize"),
            token_endpoint: format!("{base}/api/oauth.v2.access"),
        }))
    }

    fn authorize_params(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Vec<(String, String)>, ConnectorOAuthRefusal> {
        // No bot scopes; the user scopes are comma-separated.
        let mut params = vec![
            ("scope".to_owned(), String::new()),
            (
                "user_scope".to_owned(),
                facts.scopes.iter().cloned().collect::<Vec<_>>().join(","),
            ),
        ];
        match &facts.expected_account {
            AccountSelection::Known(account) => {
                let (team, _) =
                    slack_account_parts(account).ok_or(ConnectorOAuthRefusal::InvalidDescriptor)?;
                params.push(("team".to_owned(), team.to_owned()));
            }
            AccountSelection::Discover => {}
            AccountSelection::Unverified => return Err(ConnectorOAuthRefusal::InvalidDescriptor),
        }
        Ok(params)
    }

    fn decode_token_response(
        &self,
        kind: ConnectorTokenGrantKind,
        status: u16,
        body: &[u8],
        _client_secret: Option<&str>,
    ) -> Result<ConnectorProtocolGrant, ConnectorProtocolFailure> {
        if status != 200 {
            return Err(ConnectorProtocolFailure::Refused { status, code: None });
        }
        let value = json_body(body)?;
        if let Some(error) = Self::slack_error(&value) {
            let code = if SLACK_DEAD_REFRESH_ERRORS.contains(&error.as_str()) {
                "invalid_grant".to_owned()
            } else {
                error
            };
            return Err(ConnectorProtocolFailure::Refused {
                status,
                code: Some(code),
            });
        }
        let app_id = slack_identifier(&value, "app_id").ok_or(
            ConnectorProtocolFailure::Malformed("Slack response has no app_id"),
        )?;
        if self
            .expected_app_id
            .as_deref()
            .is_some_and(|expected| expected != app_id)
        {
            return Err(ConnectorProtocolFailure::Malformed(
                "Slack token was issued to another app",
            ));
        }
        let team_id = value
            .get("team")
            .and_then(|team| slack_identifier(team, "id"))
            .ok_or(ConnectorProtocolFailure::Malformed(
                "Slack response has no team id",
            ))?;
        // An enterprise-wide install is not a user grant this protocol takes.
        if value
            .get("is_enterprise_install")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            return Err(ConnectorProtocolFailure::Malformed(
                "Slack enterprise installs are not supported",
            ));
        }
        // The exchange nests the user token; a refresh returns it at the top
        // level and need not name the user (auth.test observes it).
        let (token, user_id) = match kind {
            ConnectorTokenGrantKind::Exchange => {
                let user = value
                    .get("authed_user")
                    .filter(|user| user.is_object())
                    .ok_or(ConnectorProtocolFailure::Malformed(
                        "Slack exchange has no authed_user",
                    ))?;
                let id = slack_identifier(user, "id").ok_or(
                    ConnectorProtocolFailure::Malformed("Slack exchange has no user id"),
                )?;
                (user, Some(id))
            }
            ConnectorTokenGrantKind::Refresh => (&value, slack_identifier(&value, "user_id")),
        };
        if str_field(token, "token_type") != Some("user") {
            return Err(ConnectorProtocolFailure::Malformed(
                "Slack token is not a user token",
            ));
        }
        let access_token = str_field(token, "access_token").ok_or(
            ConnectorProtocolFailure::Malformed("Slack response has no user access token"),
        )?;
        // An exchange must state its scopes. A refresh that omits them is
        // decoded as such: the owner first closes it for a missing
        // replacement, then refuses it for the missing scopes.
        let scope = str_field(token, "scope")
            .map(|scope| normalize_scopes(scope, ','))
            .filter(|scope| !scope.is_empty());
        if scope.is_none() && kind == ConnectorTokenGrantKind::Exchange {
            return Err(ConnectorProtocolFailure::Malformed(
                "Slack response states no scopes",
            ));
        }
        // Token rotation: a refresh token and its expiry come together. A
        // refresh that omits the replacement is decoded as such; the owner
        // closes the credential for it (the predecessor may be consumed).
        let refresh_token = str_field(token, "refresh_token").map(str::to_owned);
        let expires_in = token.get("expires_in");
        let expires = expires_in
            .and_then(serde_json::Value::as_u64)
            .filter(|expires| (1..=2_147_483_647_u64).contains(expires));
        let expires_in_secs = match (kind, &refresh_token, expires_in) {
            (ConnectorTokenGrantKind::Exchange, None, None) => None,
            (ConnectorTokenGrantKind::Refresh, None, _) => expires,
            (_, Some(_), Some(_)) if expires.is_some() => expires,
            _ => {
                return Err(ConnectorProtocolFailure::Malformed(
                    "Slack rotating token lacks its refresh token or a valid expiry",
                ));
            }
        };
        let mut evidence = ConnectorProtocolEvidence::default();
        evidence
            .pinned
            .insert("app_id".to_owned(), app_id.to_owned());
        evidence
            .pinned
            .insert("team_id".to_owned(), team_id.to_owned());
        if let Some(enterprise) = value
            .get("enterprise")
            .and_then(|e| slack_identifier(e, "id"))
        {
            evidence
                .pinned
                .insert("enterprise_id".to_owned(), enterprise.to_owned());
        }
        // The user is the bound account, proven by auth.test, not a pin: a
        // refresh may not restate it.
        if let Some(user_id) = user_id {
            evidence
                .facts
                .insert("user_id".to_owned(), user_id.to_owned());
        }
        Ok(ConnectorProtocolGrant {
            tokens: OAuthTokenResult {
                access_token: access_token.to_owned(),
                refresh_token,
                id_token: None,
                expires_in_secs,
                scope,
            },
            evidence,
        })
    }

    fn refresh_token_rule(&self) -> RefreshTokenRule {
        RefreshTokenRule::ReplacementRequired
    }

    fn refresh_requires_scope(&self) -> bool {
        true
    }

    fn classify_rejection(&self, status: u16, body: &[u8]) -> Option<ConnectorCredentialRejection> {
        if status == 401 {
            return Some(ConnectorCredentialRejection::InvalidCredential);
        }
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        let error = Self::slack_error(&value)?;
        SLACK_INVALID_CREDENTIAL_ERRORS
            .contains(&error.as_str())
            .then_some(ConnectorCredentialRejection::InvalidCredential)
    }
}

/// A Slack id (`A`, `T`, `U`, `E` ids): 1-128 ASCII letters and digits.
fn slack_identifier<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    str_field(value, name)
        .filter(|id| id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Proves a Slack account with `auth.test` on the token itself: a user token
/// (no bot), of the evidenced app, team and enterprise, and of the evidenced
/// user when the token response named one. Scopes the probe reports
/// (`x-oauth-scopes`) must cover the admitted ones. The account is
/// `<team>:<user>`.
pub struct SlackAuthTestStrategy {
    http: Result<Client, CredentialHttpClientUnavailable>,
    base: String,
}

impl Default for SlackAuthTestStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl SlackAuthTestStrategy {
    pub fn new() -> Self {
        Self {
            http: no_redirect_client(),
            base: SLACK_ISSUER.to_owned(),
        }
    }

    /// Use another endpoint base (loopback fixtures).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }
}

/// What one successful `auth.test` body says about its own token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackAuthTest {
    pub team_id: String,
    pub user_id: String,
    pub app_id: Option<String>,
    pub enterprise_id: Option<String>,
}

/// Parse a successful `auth.test` body for a user token. A bot token, a
/// non-user token type or an enterprise-wide install is refused.
pub fn parse_slack_auth_test(body: &[u8]) -> Result<SlackAuthTest, ConnectorOAuthRefusal> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true)
        || value.get("bot_id").is_some()
        || value
            .get("token_type")
            .is_some_and(|token_type| token_type.as_str() != Some("user"))
        || value
            .get("is_enterprise_install")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        return Err(ConnectorOAuthRefusal::VerificationUnavailable);
    }
    match (
        slack_identifier(&value, "team_id"),
        slack_identifier(&value, "user_id"),
    ) {
        (Some(team), Some(user)) => Ok(SlackAuthTest {
            team_id: team.to_owned(),
            user_id: user.to_owned(),
            app_id: slack_identifier(&value, "app_id").map(str::to_owned),
            enterprise_id: slack_identifier(&value, "enterprise_id").map(str::to_owned),
        }),
        _ => Err(ConnectorOAuthRefusal::VerificationUnavailable),
    }
}

#[async_trait]
impl ConnectorAccountStrategy for SlackAuthTestStrategy {
    fn strategy_id(&self) -> &str {
        SLACK_AUTH_TEST_STRATEGY_ID
    }

    async fn observe_account(
        &self,
        _descriptor: &ConnectorOAuthDescriptor,
        _tokens: &OAuthTokenResult,
        _nonce: Option<&str>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        // Slack proof needs the token response's evidence.
        Err(ConnectorOAuthRefusal::VerificationUnavailable)
    }

    async fn observe_account_with_evidence(
        &self,
        descriptor: &ConnectorOAuthDescriptor,
        tokens: &OAuthTokenResult,
        evidence: Option<&ConnectorProtocolEvidence>,
        _nonce: Option<&str>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        let evidence = evidence.ok_or(ConnectorOAuthRefusal::VerificationUnavailable)?;
        let http = self
            .http
            .as_ref()
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        let url = format!("{}/api/auth.test", self.base.trim_end_matches('/'));
        let parsed = reqwest::Url::parse(&url)
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        if !bearer_target_allowed(&parsed) {
            return Err(ConnectorOAuthRefusal::VerificationUnavailable);
        }
        let response = http
            .post(parsed)
            .bearer_auth(&tokens.access_token)
            .send()
            .await
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        if !response.status().is_success() {
            return Err(ConnectorOAuthRefusal::VerificationUnavailable);
        }
        let probe_scopes: Option<std::collections::BTreeSet<String>> = response
            .headers()
            .get("x-oauth-scopes")
            .map(|header| {
                header
                    .to_str()
                    .map(|scopes| {
                        normalize_scopes(scopes, ',')
                            .split(' ')
                            .map(str::to_owned)
                            .collect()
                    })
                    .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)
            })
            .transpose()?;
        let body = response
            .bytes()
            .await
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        let probe = parse_slack_auth_test(&body)?;
        if evidence.get("team_id") != Some(probe.team_id.as_str())
            || evidence
                .get("user_id")
                .is_some_and(|evidenced| evidenced != probe.user_id)
            || probe
                .app_id
                .as_deref()
                .is_some_and(|app| evidence.get("app_id") != Some(app))
            || (probe.enterprise_id.is_some()
                && evidence.get("enterprise_id").is_some()
                && probe.enterprise_id.as_deref() != evidence.get("enterprise_id"))
        {
            return Err(ConnectorOAuthRefusal::AccountMismatch);
        }
        if probe_scopes.is_some_and(|scopes| !descriptor.parameters().scopes.is_subset(&scopes)) {
            return Err(ConnectorOAuthRefusal::MissingScopes);
        }
        Ok(ConnectorAccountObservation {
            account: format!("{}:{}", probe.team_id, probe.user_id),
            granted_scopes: observed_scopes(descriptor, tokens),
        })
    }
}

// ---------------------------------------------------------------------------
// Salesforce (web server flow with PKCE)
// ---------------------------------------------------------------------------

/// Protocol id of the built-in Salesforce protocol.
pub const SALESFORCE_PROTOCOL_ID: &str = "salesforce-v1";
/// Strategy id of the Salesforce identity-service account strategy.
pub const SALESFORCE_IDENTITY_STRATEGY_ID: &str = "salesforce-identity-v1";

/// Salesforce's OAuth 2.0 web server flow with PKCE.
///
/// - The issuer is the login host (`https://login.salesforce.com`,
///   `https://test.salesforce.com` or a My Domain host); the endpoints are
///   its `/services/oauth2/authorize` and `/services/oauth2/token`.
/// - The token response carries no expiry: the provider's validity is
///   unknown and stays unknown (no synthetic lifetime). An invalid session is
///   reported by the API (`401` with `INVALID_SESSION_ID`) and fed back
///   through [`ConnectorProtocol::classify_rejection`].
/// - The response names the org and user in its identity URL
///   (`.../id/<org id>/<user id>`) and the API base in `instance_url`, which
///   must be an https Salesforce host (or the pinned My Domain host).
/// - With a client secret, the response `signature` (HMAC-SHA256 of
///   `id + issued_at`) must verify; PKCE-only clients have no secret and
///   rely on the identity service instead.
/// - Refresh: without rotation an omitted refresh token keeps the stored
///   one; with rotation configured a replacement is required.
///
/// The org is pinned evidence. With `expected_org` set, any other org is
/// refused; otherwise the bound account (`<org>:<user>`) pins it after the
/// first login.
#[derive(Clone, Debug)]
pub struct SalesforceProtocol {
    protocol_id: String,
    expected_org: Option<String>,
    my_domain: Option<String>,
    rotation: bool,
    allow_loopback: bool,
}

impl Default for SalesforceProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl SalesforceProtocol {
    pub fn new() -> Self {
        Self {
            protocol_id: SALESFORCE_PROTOCOL_ID.to_owned(),
            expected_org: None,
            my_domain: None,
            rotation: false,
            allow_loopback: false,
        }
    }

    pub fn with_protocol_id(mut self, id: impl Into<String>) -> Self {
        self.protocol_id = id.into();
        self
    }

    /// Refuse credentials of any other org (18-character org id).
    pub fn with_expected_org(mut self, org_id: impl Into<String>) -> Self {
        self.expected_org = Some(org_id.into());
        self
    }

    /// Require `instance_url` to be this My Domain host (a stricter host
    /// pin; not proof of the org).
    pub fn with_my_domain(mut self, host: impl Into<String>) -> Self {
        self.my_domain = Some(host.into());
        self
    }

    /// The connected app rotates refresh tokens: every refresh must return
    /// a replacement.
    pub fn with_refresh_token_rotation(mut self) -> Self {
        self.rotation = true;
        self
    }

    /// Accept loopback http hosts (fixtures only).
    pub fn allowing_loopback(mut self) -> Self {
        self.allow_loopback = true;
        self
    }

    fn host_allowed(&self, url: &reqwest::Url) -> bool {
        if self.allow_loopback && is_loopback_url(url) {
            return true;
        }
        if url.scheme() != "https" {
            return false;
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        if let Some(pinned) = &self.my_domain {
            return host.eq_ignore_ascii_case(pinned);
        }
        let host = host.to_ascii_lowercase();
        ["salesforce.com", "force.com"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    }

    /// The org and user of an identity URL `<scheme>://<host>/id/<org>/<user>`.
    fn identity_ids(id_url: &reqwest::Url) -> Option<(String, String)> {
        let mut segments = id_url.path_segments()?;
        if segments.next()? != "id" {
            return None;
        }
        let org = segments.next()?.to_owned();
        let user = segments.next()?.to_owned();
        (segments.next().is_none() && valid_salesforce_id(&org) && valid_salesforce_id(&user))
            .then_some((org, user))
    }
}

/// A 15- or 18-character Salesforce record id.
fn valid_salesforce_id(id: &str) -> bool {
    matches!(id.len(), 15 | 18) && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// HMAC-SHA256 (RFC 2104).
pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// Constant-time byte equality.
pub(crate) fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl ConnectorProtocol for SalesforceProtocol {
    fn protocol_id(&self) -> &str {
        &self.protocol_id
    }

    fn strategy_id(&self) -> &str {
        SALESFORCE_IDENTITY_STRATEGY_ID
    }

    fn binding(&self) -> String {
        protocol_binding(
            &self.protocol_id,
            &[
                "salesforce-v1",
                self.expected_org.as_deref().unwrap_or(""),
                self.my_domain.as_deref().unwrap_or(""),
                if self.rotation {
                    "rotation"
                } else {
                    "no-rotation"
                },
                if self.allow_loopback {
                    "loopback"
                } else {
                    "no-loopback"
                },
            ],
        )
    }

    fn endpoints(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Option<ConnectorProtocolEndpoints>, ConnectorOAuthRefusal> {
        let issuer = reqwest::Url::parse(&facts.issuer)
            .map_err(|_| ConnectorOAuthRefusal::InvalidDescriptor)?;
        let login_host_ok = if self.allow_loopback && is_loopback_url(&issuer) {
            true
        } else {
            issuer.scheme() == "https"
                && issuer.host_str().is_some_and(|host| {
                    let host = host.to_ascii_lowercase();
                    host == "login.salesforce.com"
                        || host == "test.salesforce.com"
                        || host.ends_with(".my.salesforce.com")
                })
        };
        if !login_host_ok || !issuer.path().trim_matches('/').is_empty() {
            return Err(ConnectorOAuthRefusal::InvalidDescriptor);
        }
        let base = facts.issuer.trim_end_matches('/');
        Ok(Some(ConnectorProtocolEndpoints {
            authorization_endpoint: format!("{base}/services/oauth2/authorize"),
            token_endpoint: format!("{base}/services/oauth2/token"),
        }))
    }

    fn authorize_params(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Vec<(String, String)>, ConnectorOAuthRefusal> {
        if facts.expected_account.is_unverified() {
            return Err(ConnectorOAuthRefusal::InvalidDescriptor);
        }
        Ok(vec![(
            "scope".to_owned(),
            facts.scopes.iter().cloned().collect::<Vec<_>>().join(" "),
        )])
    }

    fn decode_token_response(
        &self,
        _kind: ConnectorTokenGrantKind,
        status: u16,
        body: &[u8],
        client_secret: Option<&str>,
    ) -> Result<ConnectorProtocolGrant, ConnectorProtocolFailure> {
        if !(200..300).contains(&status) {
            let code = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|value| str_field(&value, "error").map(str::to_owned));
            return Err(ConnectorProtocolFailure::Refused { status, code });
        }
        let value = json_body(body)?;
        let access_token = str_field(&value, "access_token").ok_or(
            ConnectorProtocolFailure::Malformed("Salesforce response has no access token"),
        )?;
        let id = str_field(&value, "id").ok_or(ConnectorProtocolFailure::Malformed(
            "Salesforce response has no identity URL",
        ))?;
        let instance = str_field(&value, "instance_url").ok_or(
            ConnectorProtocolFailure::Malformed("Salesforce response has no instance URL"),
        )?;
        let id_url = reqwest::Url::parse(id).map_err(|_| {
            ConnectorProtocolFailure::Malformed("Salesforce identity URL is invalid")
        })?;
        let instance_url = reqwest::Url::parse(instance).map_err(|_| {
            ConnectorProtocolFailure::Malformed("Salesforce instance URL is invalid")
        })?;
        if !self.host_allowed(&instance_url) || !self.host_allowed_for_identity(&id_url) {
            return Err(ConnectorProtocolFailure::Malformed(
                "Salesforce response names an untrusted host",
            ));
        }
        let (org, user) =
            Self::identity_ids(&id_url).ok_or(ConnectorProtocolFailure::Malformed(
                "Salesforce identity URL does not name an org and user",
            ))?;
        if self
            .expected_org
            .as_deref()
            .is_some_and(|expected| expected != org)
        {
            return Err(ConnectorProtocolFailure::Malformed(
                "Salesforce credential belongs to another org",
            ));
        }
        if let Some(secret) = client_secret {
            let issued_at = str_field(&value, "issued_at").ok_or(
                ConnectorProtocolFailure::Malformed("Salesforce response has no issued_at"),
            )?;
            let signature = str_field(&value, "signature")
                .and_then(|signature| BASE64.decode(signature).ok())
                .ok_or(ConnectorProtocolFailure::Malformed(
                    "Salesforce response has no signature",
                ))?;
            let expected = hmac_sha256(secret.as_bytes(), format!("{id}{issued_at}").as_bytes());
            if !bytes_equal(&signature, &expected) {
                return Err(ConnectorProtocolFailure::Malformed(
                    "Salesforce response signature does not verify",
                ));
            }
        }
        let mut evidence = ConnectorProtocolEvidence::default();
        evidence.pinned.insert("org_id".to_owned(), org);
        evidence.pinned.insert("user_id".to_owned(), user);
        evidence.facts.insert(
            "instance_url".to_owned(),
            instance.trim_end_matches('/').to_owned(),
        );
        evidence
            .facts
            .insert("identity_url".to_owned(), id.to_owned());
        Ok(ConnectorProtocolGrant {
            tokens: OAuthTokenResult {
                access_token: access_token.to_owned(),
                refresh_token: str_field(&value, "refresh_token").map(str::to_owned),
                id_token: None,
                // Salesforce states no expiry; it stays unknown.
                expires_in_secs: None,
                scope: str_field(&value, "scope").map(|scope| normalize_scopes(scope, ' ')),
            },
            evidence,
        })
    }

    fn refresh_token_rule(&self) -> RefreshTokenRule {
        if self.rotation {
            RefreshTokenRule::ReplacementRequired
        } else {
            RefreshTokenRule::RetainIfOmitted
        }
    }

    fn classify_rejection(&self, status: u16, body: &[u8]) -> Option<ConnectorCredentialRejection> {
        // The REST API answers an invalid or expired session with 401 and
        // an INVALID_SESSION_ID error code.
        (status == 401 && String::from_utf8_lossy(body).contains("INVALID_SESSION_ID"))
            .then_some(ConnectorCredentialRejection::InvalidCredential)
    }
}

impl SalesforceProtocol {
    /// The identity URL is served by the login host or the org's instance:
    /// any trusted Salesforce host.
    fn host_allowed_for_identity(&self, url: &reqwest::Url) -> bool {
        if self.allow_loopback && is_loopback_url(url) {
            return true;
        }
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                let host = host.to_ascii_lowercase();
                ["salesforce.com", "force.com"]
                    .iter()
                    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
            })
    }
}

/// Proves a Salesforce account with the identity service: the identity URL
/// from the token response, called with the token itself, must report an
/// asserted, active user whose org and user equal the URL's. URL or body
/// equality alone could describe another queried user; `asserted_user`
/// says the identity is the token's own.
pub struct SalesforceIdentityStrategy {
    http: Result<Client, CredentialHttpClientUnavailable>,
    allow_loopback: bool,
}

impl Default for SalesforceIdentityStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl SalesforceIdentityStrategy {
    pub fn new() -> Self {
        Self {
            http: no_redirect_client(),
            allow_loopback: false,
        }
    }

    /// Accept loopback identity URLs (fixtures only).
    pub fn allowing_loopback(mut self) -> Self {
        self.allow_loopback = true;
        self
    }
}

/// The `(org_id, user_id)` a Salesforce identity-service body proves for its
/// own token: it must be the asserted user and active.
pub fn parse_salesforce_identity(body: &[u8]) -> Result<(String, String), ConnectorOAuthRefusal> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
    if value
        .get("asserted_user")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
        || value.get("active").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err(ConnectorOAuthRefusal::VerificationUnavailable);
    }
    match (
        str_field(&value, "organization_id"),
        str_field(&value, "user_id"),
    ) {
        (Some(org), Some(user)) => Ok((org.to_owned(), user.to_owned())),
        _ => Err(ConnectorOAuthRefusal::VerificationUnavailable),
    }
}

#[async_trait]
impl ConnectorAccountStrategy for SalesforceIdentityStrategy {
    fn strategy_id(&self) -> &str {
        SALESFORCE_IDENTITY_STRATEGY_ID
    }

    async fn observe_account(
        &self,
        _descriptor: &ConnectorOAuthDescriptor,
        _tokens: &OAuthTokenResult,
        _nonce: Option<&str>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        Err(ConnectorOAuthRefusal::VerificationUnavailable)
    }

    async fn observe_account_with_evidence(
        &self,
        descriptor: &ConnectorOAuthDescriptor,
        tokens: &OAuthTokenResult,
        evidence: Option<&ConnectorProtocolEvidence>,
        _nonce: Option<&str>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        let evidence = evidence.ok_or(ConnectorOAuthRefusal::VerificationUnavailable)?;
        let (Some(org), Some(user), Some(id_url)) = (
            evidence.get("org_id"),
            evidence.get("user_id"),
            evidence.get("identity_url"),
        ) else {
            return Err(ConnectorOAuthRefusal::VerificationUnavailable);
        };
        let url = reqwest::Url::parse(id_url)
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        if !(url.scheme() == "https" || (self.allow_loopback && is_loopback_url(&url))) {
            return Err(ConnectorOAuthRefusal::VerificationUnavailable);
        }
        let http = self
            .http
            .as_ref()
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        let response = http
            .get(url)
            .bearer_auth(&tokens.access_token)
            .send()
            .await
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        if !response.status().is_success() {
            return Err(ConnectorOAuthRefusal::VerificationUnavailable);
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        let (observed_org, observed_user) = parse_salesforce_identity(&body)?;
        if observed_org != org || observed_user != user {
            return Err(ConnectorOAuthRefusal::AccountMismatch);
        }
        Ok(ConnectorAccountObservation {
            account: format!("{org}:{user}"),
            granted_scopes: observed_scopes(descriptor, tokens),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn facts(issuer: &str, scopes: &[&str], account: AccountSelection) -> ConnectorOAuthParameters {
        ConnectorOAuthParameters {
            issuer: issuer.to_owned(),
            client: "client".to_owned(),
            resource: issuer.to_owned(),
            scopes: scopes
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<BTreeSet<_>>(),
            redirect_uri: "http://127.0.0.1:8765/connector/oauth/callback".to_owned(),
            expected_account: account,
            strategy_id: "s".to_owned(),
            client_auth_profile: None,
            protocol: None,
        }
    }

    // Captured shapes of Slack OAuth v2 user-token responses (values made up).
    const SLACK_EXCHANGE: &str = r#"{"ok":true,"app_id":"A0APP","authed_user":{"id":"U0USER","scope":"search:read,channels:history","access_token":"xoxp-access","token_type":"user","refresh_token":"xoxe-1-refresh","expires_in":43200},"team":{"id":"T0TEAM","name":"Team"},"enterprise":null,"is_enterprise_install":false}"#;
    const SLACK_REFRESH: &str = r#"{"ok":true,"app_id":"A0APP","scope":"search:read,channels:history","token_type":"user","access_token":"xoxp-access-2","refresh_token":"xoxe-1-refresh-2","expires_in":43200,"user_id":"U0USER","team":{"id":"T0TEAM","name":"Team"},"enterprise":null,"is_enterprise_install":false}"#;

    #[test]
    fn slack_authorize_carries_user_scope_and_known_team() {
        let protocol = SlackUserProtocol::new();
        let params = protocol
            .authorize_params(&facts(
                "https://slack.com",
                &["search:read", "channels:history"],
                AccountSelection::Known("T0TEAM:U0USER".to_owned()),
            ))
            .unwrap();
        assert_eq!(
            params,
            vec![
                ("scope".to_owned(), String::new()),
                (
                    "user_scope".to_owned(),
                    "channels:history,search:read".to_owned()
                ),
                ("team".to_owned(), "T0TEAM".to_owned()),
            ]
        );
        let endpoints = protocol
            .endpoints(&facts(
                "https://slack.com",
                &["a"],
                AccountSelection::Discover,
            ))
            .unwrap()
            .unwrap();
        assert_eq!(
            endpoints.token_endpoint,
            "https://slack.com/api/oauth.v2.access"
        );
        assert!(
            protocol
                .endpoints(&facts(
                    "https://evil.example",
                    &["a"],
                    AccountSelection::Discover
                ))
                .is_err()
        );
    }

    #[test]
    fn slack_exchange_reads_the_nested_user_token() {
        let grant = SlackUserProtocol::new()
            .decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                SLACK_EXCHANGE.as_bytes(),
                None,
            )
            .unwrap();
        assert_eq!(grant.tokens.access_token, "xoxp-access");
        assert_eq!(
            grant.tokens.refresh_token.as_deref(),
            Some("xoxe-1-refresh")
        );
        assert_eq!(
            grant.tokens.scope.as_deref(),
            Some("search:read channels:history")
        );
        assert_eq!(grant.tokens.expires_in_secs, Some(43200));
        assert_eq!(grant.evidence.get("team_id"), Some("T0TEAM"));
        assert_eq!(grant.evidence.get("user_id"), Some("U0USER"));
        assert_eq!(grant.evidence.get("app_id"), Some("A0APP"));
    }

    #[test]
    fn slack_refresh_reads_the_top_level_user_token() {
        let protocol = SlackUserProtocol::new();
        let grant = protocol
            .decode_token_response(
                ConnectorTokenGrantKind::Refresh,
                200,
                SLACK_REFRESH.as_bytes(),
                None,
            )
            .unwrap();
        assert_eq!(grant.tokens.access_token, "xoxp-access-2");
        assert_eq!(
            grant.tokens.refresh_token.as_deref(),
            Some("xoxe-1-refresh-2")
        );
        assert_eq!(
            protocol.refresh_token_rule(),
            RefreshTokenRule::ReplacementRequired
        );
        assert!(protocol.refresh_requires_scope());
        // The nested exchange shape is not a refresh answer.
        assert!(
            protocol
                .decode_token_response(
                    ConnectorTokenGrantKind::Refresh,
                    200,
                    SLACK_EXCHANGE.as_bytes(),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn slack_ok_false_is_a_refusal_and_dead_refresh_tokens_are_invalid_grant() {
        let protocol = SlackUserProtocol::new();
        let refused = protocol
            .decode_token_response(
                ConnectorTokenGrantKind::Refresh,
                200,
                br#"{"ok":false,"error":"invalid_refresh_token"}"#,
                None,
            )
            .unwrap_err();
        assert_eq!(
            refused,
            ConnectorProtocolFailure::Refused {
                status: 200,
                code: Some("invalid_grant".to_owned())
            }
        );
        let other = protocol
            .decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                br#"{"ok":false,"error":"ratelimited"}"#,
                None,
            )
            .unwrap_err();
        assert_eq!(
            other,
            ConnectorProtocolFailure::Refused {
                status: 200,
                code: Some("ratelimited".to_owned())
            }
        );
    }

    #[test]
    fn slack_rotation_needs_both_the_refresh_token_and_its_expiry() {
        let no_expiry = SLACK_EXCHANGE.replace(r#","expires_in":43200"#, "");
        assert!(
            SlackUserProtocol::new()
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    no_expiry.as_bytes(),
                    None
                )
                .is_err()
        );
        let long_lived = SLACK_EXCHANGE.replace(
            r#","refresh_token":"xoxe-1-refresh","expires_in":43200"#,
            "",
        );
        let grant = SlackUserProtocol::new()
            .decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                long_lived.as_bytes(),
                None,
            )
            .unwrap();
        assert_eq!(grant.tokens.refresh_token, None);
        assert_eq!(grant.tokens.expires_in_secs, None);
        // A refresh without a replacement decodes as such; the owner closes it.
        let omitted = SLACK_REFRESH.replace(r#""refresh_token":"xoxe-1-refresh-2","#, "");
        let grant = SlackUserProtocol::new()
            .decode_token_response(
                ConnectorTokenGrantKind::Refresh,
                200,
                omitted.as_bytes(),
                None,
            )
            .unwrap();
        assert_eq!(grant.tokens.refresh_token, None);
        let enterprise = SLACK_EXCHANGE.replace(
            r#""is_enterprise_install":false"#,
            r#""is_enterprise_install":true"#,
        );
        assert!(
            SlackUserProtocol::new()
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    enterprise.as_bytes(),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn slack_refuses_bot_tokens_and_other_apps() {
        let bot = SLACK_REFRESH.replace(r#""token_type":"user""#, r#""token_type":"bot""#);
        assert!(
            SlackUserProtocol::new()
                .decode_token_response(ConnectorTokenGrantKind::Refresh, 200, bot.as_bytes(), None)
                .is_err()
        );
        assert!(
            SlackUserProtocol::new()
                .with_expected_app_id("A0OTHER")
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    SLACK_EXCHANGE.as_bytes(),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn slack_auth_test_and_rejections() {
        assert_eq!(
            parse_slack_auth_test(br#"{"ok":true,"team_id":"T0TEAM","user_id":"U0USER","app_id":"A0APP","url":"https://x.slack.com/"}"#)
                .unwrap(),
            SlackAuthTest {
                team_id: "T0TEAM".to_owned(),
                user_id: "U0USER".to_owned(),
                app_id: Some("A0APP".to_owned()),
                enterprise_id: None,
            }
        );
        assert!(parse_slack_auth_test(br#"{"ok":false,"error":"invalid_auth"}"#).is_err());
        assert!(
            parse_slack_auth_test(
                br#"{"ok":true,"team_id":"T0TEAM","user_id":"U0USER","bot_id":"B0"}"#
            )
            .is_err(),
            "a bot token is not a user grant"
        );
        assert!(
            parse_slack_auth_test(
                br#"{"ok":true,"team_id":"T0TEAM","user_id":"U0USER","is_enterprise_install":true}"#
            )
            .is_err()
        );
        let protocol = SlackUserProtocol::new();
        assert_eq!(
            protocol.classify_rejection(200, br#"{"ok":false,"error":"token_expired"}"#),
            Some(ConnectorCredentialRejection::InvalidCredential)
        );
        assert_eq!(
            protocol.classify_rejection(200, br#"{"ok":false,"error":"channel_not_found"}"#),
            None
        );
        assert_eq!(protocol.classify_rejection(200, br#"{"ok":true}"#), None);
    }

    const SF_ID: &str = "https://login.salesforce.com/id/00D000000000001AAA/005000000000001AAA";

    fn sf_body(extra: &str) -> String {
        format!(
            r#"{{"access_token":"00D!session","refresh_token":"5Aep-refresh","instance_url":"https://acme.my.salesforce.com","id":"{SF_ID}","token_type":"Bearer","issued_at":"1700000000000","scope":"api refresh_token"{extra}}}"#
        )
    }

    #[test]
    fn salesforce_exchange_keeps_unknown_expiry_and_names_org_and_user() {
        let grant = SalesforceProtocol::new()
            .decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                sf_body("").as_bytes(),
                None,
            )
            .unwrap();
        assert_eq!(grant.tokens.expires_in_secs, None, "no synthetic expiry");
        assert_eq!(grant.evidence.get("org_id"), Some("00D000000000001AAA"));
        assert_eq!(grant.evidence.get("user_id"), Some("005000000000001AAA"));
        assert_eq!(
            grant.evidence.get("instance_url"),
            Some("https://acme.my.salesforce.com")
        );
        assert!(grant.evidence.facts.contains_key("instance_url"));
        assert!(grant.evidence.pinned.contains_key("org_id"));
    }

    #[test]
    fn salesforce_signature_verifies_only_with_a_secret() {
        let secret = "consumer-secret";
        let signature = BASE64.encode(hmac_sha256(
            secret.as_bytes(),
            format!("{SF_ID}1700000000000").as_bytes(),
        ));
        let signed = sf_body(&format!(r#","signature":"{signature}""#));
        let protocol = SalesforceProtocol::new();
        assert!(
            protocol
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    signed.as_bytes(),
                    Some(secret)
                )
                .is_ok()
        );
        assert!(
            protocol
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    signed.as_bytes(),
                    Some("other")
                )
                .is_err()
        );
        assert!(
            protocol
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    sf_body("").as_bytes(),
                    Some(secret)
                )
                .is_err(),
            "a secret-bearing client requires the signature"
        );
    }

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let hex = mac.iter().fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn salesforce_refuses_untrusted_hosts_other_orgs_and_bad_logins() {
        let protocol = SalesforceProtocol::new();
        let evil = sf_body("").replace("https://acme.my.salesforce.com", "https://evil.example");
        assert!(
            protocol
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    evil.as_bytes(),
                    None
                )
                .is_err()
        );
        assert!(
            SalesforceProtocol::new()
                .with_expected_org("00D000000000002AAA")
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    sf_body("").as_bytes(),
                    None
                )
                .is_err()
        );
        assert!(
            SalesforceProtocol::new()
                .with_my_domain("other.my.salesforce.com")
                .decode_token_response(
                    ConnectorTokenGrantKind::Exchange,
                    200,
                    sf_body("").as_bytes(),
                    None
                )
                .is_err()
        );
        assert!(
            protocol
                .endpoints(&facts(
                    "https://evil.example",
                    &["api"],
                    AccountSelection::Discover
                ))
                .is_err()
        );
        assert_eq!(
            protocol
                .endpoints(&facts(
                    "https://login.salesforce.com",
                    &["api"],
                    AccountSelection::Discover
                ))
                .unwrap()
                .unwrap()
                .token_endpoint,
            "https://login.salesforce.com/services/oauth2/token"
        );
    }

    #[test]
    fn salesforce_refresh_rules_and_refusals() {
        assert_eq!(
            SalesforceProtocol::new().refresh_token_rule(),
            RefreshTokenRule::RetainIfOmitted
        );
        assert_eq!(
            SalesforceProtocol::new()
                .with_refresh_token_rotation()
                .refresh_token_rule(),
            RefreshTokenRule::ReplacementRequired
        );
        assert_eq!(
            SalesforceProtocol::new()
                .decode_token_response(
                    ConnectorTokenGrantKind::Refresh,
                    400,
                    br#"{"error":"invalid_grant","error_description":"expired access/refresh token"}"#,
                    None
                )
                .unwrap_err(),
            ConnectorProtocolFailure::Refused {
                status: 400,
                code: Some("invalid_grant".to_owned())
            }
        );
        let protocol = SalesforceProtocol::new();
        assert_eq!(
            protocol.classify_rejection(
                401,
                br#"[{"message":"Session expired or invalid","errorCode":"INVALID_SESSION_ID"}]"#
            ),
            Some(ConnectorCredentialRejection::InvalidCredential)
        );
        assert_eq!(
            protocol.classify_rejection(403, b"INVALID_SESSION_ID"),
            None
        );
        assert_eq!(
            protocol.classify_rejection(401, b"REQUEST_LIMIT_EXCEEDED"),
            None
        );
    }

    #[test]
    fn salesforce_identity_requires_the_asserted_active_user() {
        let own = br#"{"organization_id":"00D000000000001AAA","user_id":"005000000000001AAA","asserted_user":true,"active":true}"#;
        assert_eq!(
            parse_salesforce_identity(own).unwrap(),
            (
                "00D000000000001AAA".to_owned(),
                "005000000000001AAA".to_owned()
            )
        );
        let queried = br#"{"organization_id":"00D000000000001AAA","user_id":"005000000000001AAA","asserted_user":false,"active":true}"#;
        assert!(
            parse_salesforce_identity(queried).is_err(),
            "another queried user"
        );
        let inactive = br#"{"organization_id":"00D000000000001AAA","user_id":"005000000000001AAA","asserted_user":true,"active":false}"#;
        assert!(parse_salesforce_identity(inactive).is_err());
    }
}
