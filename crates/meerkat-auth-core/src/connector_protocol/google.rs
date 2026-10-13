//! Google OAuth 2.0 for web server applications (Gmail and other Google APIs).

use std::collections::BTreeSet;

use async_trait::async_trait;
use reqwest::Client;

use super::{
    ConnectorCredentialRejection, ConnectorProtocol, ConnectorProtocolEndpoints,
    ConnectorProtocolEvidence, ConnectorProtocolFailure, ConnectorProtocolGrant,
    ConnectorTokenGrantKind, RefreshTokenRule, bearer_target_allowed, json_body, normalize_scopes,
    protocol_binding, str_field,
};
use crate::auth_oauth::OAuthTokenResult;
use crate::connector_login::ConnectorAccountStrategy;
use crate::connector_oauth::{
    AccountSelection, ConnectorAccountObservation, ConnectorOAuthDescriptor,
    ConnectorOAuthParameters, ConnectorOAuthRefusal,
};
use crate::mcp_oauth::{CredentialHttpClientUnavailable, is_loopback_url, no_redirect_client};

/// Protocol id of the built-in Google protocol.
pub const GOOGLE_PROTOCOL_ID: &str = "google-v1";
/// Strategy id of the Google UserInfo account strategy.
pub const GOOGLE_USERINFO_STRATEGY_ID: &str = "google-userinfo-v1";
/// The Google issuer a login must name.
pub const GOOGLE_ISSUER: &str = "https://accounts.google.com";

const GOOGLE_AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// The scope every login requests so UserInfo can report the subject.
const IDENTITY_SCOPE: &str = "openid";

/// Google's expanded forms of the two OpenID Connect scope aliases. A
/// granted scope list names the URL even when the request used the alias.
/// No other scope equivalence is inferred.
const SCOPE_ALIASES: [(&str, &str); 2] = [
    ("https://www.googleapis.com/auth/userinfo.email", "email"),
    (
        "https://www.googleapis.com/auth/userinfo.profile",
        "profile",
    ),
];

/// The Google endpoints a protocol and its strategy use.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GoogleEndpoints {
    issuer: String,
    authorize: String,
    token: String,
    userinfo: String,
}

impl GoogleEndpoints {
    fn google() -> Self {
        Self {
            issuer: GOOGLE_ISSUER.to_owned(),
            authorize: GOOGLE_AUTHORIZE.to_owned(),
            token: GOOGLE_TOKEN.to_owned(),
            userinfo: GOOGLE_USERINFO.to_owned(),
        }
    }

    /// Every endpoint under one loopback base (local fixtures).
    fn under(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            issuer: base.to_owned(),
            authorize: format!("{base}/o/oauth2/v2/auth"),
            token: format!("{base}/token"),
            userinfo: format!("{base}/v1/userinfo"),
        }
    }
}

/// Google's OAuth 2.0 web server flow with PKCE and a confidential Web
/// application client.
///
/// - Fixed endpoints: `accounts.google.com` authorizes and
///   `oauth2.googleapis.com` issues tokens. The admitted issuer must be
///   [`GOOGLE_ISSUER`].
/// - Durable offline enrollment is local policy: the authorize request asks
///   for `access_type=offline` and `prompt=consent`, because every login
///   commits a new credential that needs its own refresh token and Google
///   may omit one when the user is not shown consent again. A protocol sees
///   no stored credential, so consent is requested on each login. A code
///   exchange that establishes no refresh token fails that policy
///   ([`ConnectorProtocolFailure::Policy`]); it is not a malformed response.
/// - Incremental authorization: `include_granted_scopes=true`, so a grant
///   may carry scopes beyond the request. Only Google's two documented
///   userinfo aliases are normalized. The owner still requires the granted
///   scopes to cover the admitted ones, so a narrower grant (granular
///   consent) is refused; extra scopes grant no native permission.
/// - Refresh: Google normally omits the refresh token; the owner then keeps
///   the exact stored predecessor and re-proves its subject. One it returns
///   replaces it through the guarded commit.
/// - Failures keep their meaning: a token-endpoint refusal carries Google's
///   real status and OAuth code for the owner's classifier (`invalid_grant`,
///   `invalid_client`, 429 and 5xx stay distinct); a success-status body
///   that is not a token is malformed, which the owner treats as
///   unavailable, never as a refusal.
/// - The ID token is never decoded. The account is the stable `sub`, proven
///   by UserInfo with the access token ([`GoogleUserInfoStrategy`]).
/// - Logout stays local. Google's revocation endpoint removes the user's
///   grant to the whole API project, so it is not part of slot logout.
///
/// The client secret is the host's client-auth profile secret; the owner
/// sends it on the exchange and every refresh. The redirect is the
/// configured target's; a protocol never sets it.
///
/// Account selection: the bound account (`Known`, the credential's account
/// and the receiver's verified subject) is the `sub`, scoped by the issuer
/// the credential records. A known login sends it as `login_hint`, which
/// Google accepts. An email address is never the account: a host that
/// knows only an email starts a `Discover` login, and the verified email
/// UserInfo reports ([`GoogleUserInfo::verified_email`]) is display
/// evidence only. A changed email therefore never rebinds a credential.
#[derive(Clone, Debug)]
pub struct GoogleProtocol {
    protocol_id: String,
    endpoints: GoogleEndpoints,
    hosted_domain: Option<String>,
}

impl Default for GoogleProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleProtocol {
    pub fn new() -> Self {
        Self {
            protocol_id: GOOGLE_PROTOCOL_ID.to_owned(),
            endpoints: GoogleEndpoints::google(),
            hosted_domain: None,
        }
    }

    /// Install under another id (for example one per Workspace domain).
    pub fn with_protocol_id(mut self, id: impl Into<String>) -> Self {
        self.protocol_id = id.into();
        self
    }

    /// Ask Google to offer only accounts of this Workspace domain (`hd`) and
    /// pin it as evidence; the strategy then requires UserInfo to report it.
    pub fn with_hosted_domain(mut self, domain: impl Into<String>) -> Self {
        self.hosted_domain = Some(domain.into().to_ascii_lowercase());
        self
    }

    /// Serve every endpoint from a loopback `base` (local fixtures). Any
    /// other base is refused: production always uses Google's endpoints.
    pub fn with_loopback_base(mut self, base: &str) -> Result<Self, ConnectorOAuthRefusal> {
        self.endpoints = GoogleEndpoints::under(loopback_base(base)?);
        Ok(self)
    }
}

/// An `http` or `https` base on a loopback host, or a refusal.
fn loopback_base(base: &str) -> Result<&str, ConnectorOAuthRefusal> {
    reqwest::Url::parse(base)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && is_loopback_url(url))
        .map(|_| base)
        .ok_or(ConnectorOAuthRefusal::InvalidDescriptor)
}

/// The OAuth error code of a Google error body, if it has one.
fn google_error(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| str_field(&value, "error").map(str::to_owned))
}

/// The granted scopes, space-separated, with Google's expanded aliases also
/// named by their short form.
fn google_scopes(raw: &str) -> String {
    let mut scopes: BTreeSet<String> = normalize_scopes(raw, ' ')
        .split(' ')
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect();
    for (expanded, alias) in SCOPE_ALIASES {
        if scopes.contains(expanded) {
            scopes.insert(alias.to_owned());
        }
    }
    scopes.into_iter().collect::<Vec<_>>().join(" ")
}

impl ConnectorProtocol for GoogleProtocol {
    fn protocol_id(&self) -> &str {
        &self.protocol_id
    }

    fn strategy_id(&self) -> &str {
        GOOGLE_USERINFO_STRATEGY_ID
    }

    fn binding(&self) -> String {
        protocol_binding(
            &self.protocol_id,
            &[
                "google-v1",
                &self.endpoints.issuer,
                &self.endpoints.authorize,
                &self.endpoints.token,
                self.hosted_domain.as_deref().unwrap_or(""),
                "offline-consent-incremental",
                "retain-if-omitted",
            ],
        )
    }

    fn endpoints(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Option<ConnectorProtocolEndpoints>, ConnectorOAuthRefusal> {
        if facts.issuer.trim_end_matches('/') != self.endpoints.issuer {
            return Err(ConnectorOAuthRefusal::InvalidDescriptor);
        }
        Ok(Some(ConnectorProtocolEndpoints {
            authorization_endpoint: self.endpoints.authorize.clone(),
            token_endpoint: self.endpoints.token.clone(),
        }))
    }

    fn authorize_params(
        &self,
        facts: &ConnectorOAuthParameters,
    ) -> Result<Vec<(String, String)>, ConnectorOAuthRefusal> {
        let mut scopes: BTreeSet<&str> = facts.scopes.iter().map(String::as_str).collect();
        scopes.insert(IDENTITY_SCOPE);
        let mut params = vec![
            (
                "scope".to_owned(),
                scopes.into_iter().collect::<Vec<_>>().join(" "),
            ),
            ("access_type".to_owned(), "offline".to_owned()),
            ("prompt".to_owned(), "consent".to_owned()),
            ("include_granted_scopes".to_owned(), "true".to_owned()),
        ];
        match &facts.expected_account {
            // The known account is the `sub`, which Google accepts as a hint.
            AccountSelection::Known(account) => {
                params.push(("login_hint".to_owned(), account.clone()));
            }
            AccountSelection::Discover => {}
            AccountSelection::Unverified => return Err(ConnectorOAuthRefusal::InvalidDescriptor),
        }
        if let Some(domain) = &self.hosted_domain {
            params.push(("hd".to_owned(), domain.clone()));
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
        if !(200..300).contains(&status) {
            return Err(ConnectorProtocolFailure::Refused {
                status,
                code: google_error(body),
            });
        }
        let value = json_body(body)?;
        // Google reports refusals with an error status; a success status
        // with an error body is not a refusal the owner may classify.
        if str_field(&value, "error").is_some() {
            return Err(ConnectorProtocolFailure::Malformed(
                "Google success response carries an error",
            ));
        }
        let access_token = str_field(&value, "access_token").ok_or(
            ConnectorProtocolFailure::Malformed("Google response has no access token"),
        )?;
        if !str_field(&value, "token_type").is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
        {
            return Err(ConnectorProtocolFailure::Malformed(
                "Google token is not a bearer token",
            ));
        }
        let expires_in_secs = value
            .get("expires_in")
            .and_then(serde_json::Value::as_u64)
            .filter(|expires| (1..=2_147_483_647_u64).contains(expires))
            .ok_or(ConnectorProtocolFailure::Malformed(
                "Google response has no valid expiry",
            ))?;
        let scope = str_field(&value, "scope")
            .map(google_scopes)
            .filter(|scope| !scope.is_empty());
        let refresh_token = str_field(&value, "refresh_token").map(str::to_owned);
        if kind == ConnectorTokenGrantKind::Exchange {
            if scope.is_none() {
                return Err(ConnectorProtocolFailure::Malformed(
                    "Google exchange states no scopes",
                ));
            }
            // Local policy: a login must establish durable offline access.
            if refresh_token.is_none() {
                return Err(ConnectorProtocolFailure::Policy(
                    "Google login established no offline refresh token",
                ));
            }
        }
        let mut evidence = ConnectorProtocolEvidence::default();
        if let Some(domain) = &self.hosted_domain {
            evidence
                .pinned
                .insert("hosted_domain".to_owned(), domain.clone());
        }
        Ok(ConnectorProtocolGrant {
            tokens: OAuthTokenResult {
                access_token: access_token.to_owned(),
                refresh_token,
                // Never decoded or trusted; UserInfo proves the account.
                id_token: None,
                expires_in_secs: Some(expires_in_secs),
                scope,
            },
            evidence,
        })
    }

    fn refresh_token_rule(&self) -> RefreshTokenRule {
        RefreshTokenRule::RetainIfOmitted
    }

    fn classify_rejection(
        &self,
        status: u16,
        _body: &[u8],
    ) -> Option<ConnectorCredentialRejection> {
        // A Google API answers a call whose access token it does not accept
        // with 401; the owner applies this only to the exact current token.
        // 403 is a missing scope, a domain policy, a disabled API or a quota,
        // and 429/5xx are load: refreshing would not change them.
        (status == 401).then_some(ConnectorCredentialRejection::InvalidCredential)
    }
}

/// What one Google UserInfo body proves about its own token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleUserInfo {
    /// The stable, never reused subject: the bound account.
    pub sub: String,
    /// Lowercased; present only when Google reports it verified. Display
    /// and selection evidence, never the account.
    pub verified_email: Option<String>,
    /// The Workspace domain (`hd`), lowercased.
    pub hosted_domain: Option<String>,
}

/// Parse a UserInfo body. A missing or malformed `sub` is refused; an email
/// counts only with `email_verified: true` (a boolean or the string
/// `"true"`).
pub fn parse_google_userinfo(body: &[u8]) -> Result<GoogleUserInfo, ConnectorOAuthRefusal> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
    let sub = str_field(&value, "sub")
        .filter(|sub| sub.len() <= 255 && sub.chars().all(|c| c.is_ascii_alphanumeric()))
        .ok_or(ConnectorOAuthRefusal::VerificationUnavailable)?;
    let verified = match value.get("email_verified") {
        Some(serde_json::Value::Bool(verified)) => *verified,
        Some(serde_json::Value::String(verified)) => verified == "true",
        _ => false,
    };
    let verified_email = str_field(&value, "email")
        .filter(|_| verified)
        .filter(|email| email.contains('@') && !email.chars().any(char::is_whitespace))
        .map(str::to_ascii_lowercase);
    Ok(GoogleUserInfo {
        sub: sub.to_owned(),
        verified_email,
        hosted_domain: str_field(&value, "hd").map(str::to_ascii_lowercase),
    })
}

/// Proves a Google account with OpenID Connect UserInfo, called with the
/// access token itself. The account is the stable `sub`; the owner requires
/// a known login, and every refresh, to observe exactly the bound one. A
/// hosted domain configured here and one pinned by the protocol are both
/// required; a conflict between them is refused before the UserInfo
/// request. The email UserInfo reports is never the account.
pub struct GoogleUserInfoStrategy {
    http: Result<Client, CredentialHttpClientUnavailable>,
    userinfo: String,
    hosted_domain: Option<String>,
}

impl Default for GoogleUserInfoStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleUserInfoStrategy {
    pub fn new() -> Self {
        Self {
            http: no_redirect_client(),
            userinfo: GOOGLE_USERINFO.to_owned(),
            hosted_domain: None,
        }
    }

    /// Require the account to belong to this Workspace domain. This adds to,
    /// and never replaces, a domain the login's protocol pins: when both
    /// are set they must agree, and the account must belong to it.
    pub fn with_hosted_domain(mut self, domain: impl Into<String>) -> Self {
        self.hosted_domain = Some(domain.into().to_ascii_lowercase());
        self
    }

    /// Serve UserInfo from a loopback `base` (local fixtures). Any other
    /// base is refused: production always uses Google's UserInfo endpoint.
    pub fn with_loopback_base(mut self, base: &str) -> Result<Self, ConnectorOAuthRefusal> {
        self.userinfo = GoogleEndpoints::under(loopback_base(base)?).userinfo;
        Ok(self)
    }

    async fn observe(
        &self,
        descriptor: &ConnectorOAuthDescriptor,
        tokens: &OAuthTokenResult,
        evidence: Option<&ConnectorProtocolEvidence>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        // Both the strategy's domain and the protocol's pinned domain are
        // constraints. Conflicting configuration is refused before the
        // UserInfo request; neither overrides the other.
        let required: Vec<&str> = self
            .hosted_domain
            .as_deref()
            .into_iter()
            .chain(evidence.and_then(|evidence| evidence.get("hosted_domain")))
            .collect();
        if required.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err(ConnectorOAuthRefusal::InvalidDescriptor);
        }
        let url = reqwest::Url::parse(&self.userinfo)
            .map_err(|_| ConnectorOAuthRefusal::VerificationUnavailable)?;
        if !bearer_target_allowed(&url) {
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
        let info = parse_google_userinfo(&body)?;
        // Every present domain constraint must hold.
        if required
            .iter()
            .any(|domain| info.hosted_domain.as_deref() != Some(*domain))
        {
            return Err(ConnectorOAuthRefusal::AccountMismatch);
        }
        // The owner compares this with a known or bound account exactly.
        Ok(ConnectorAccountObservation {
            account: info.sub,
            granted_scopes: super::observed_scopes(descriptor, tokens),
        })
    }
}

#[async_trait]
impl ConnectorAccountStrategy for GoogleUserInfoStrategy {
    fn strategy_id(&self) -> &str {
        GOOGLE_USERINFO_STRATEGY_ID
    }

    async fn observe_account(
        &self,
        descriptor: &ConnectorOAuthDescriptor,
        tokens: &OAuthTokenResult,
        _nonce: Option<&str>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        self.observe(descriptor, tokens, None).await
    }

    async fn observe_account_with_evidence(
        &self,
        descriptor: &ConnectorOAuthDescriptor,
        tokens: &OAuthTokenResult,
        evidence: Option<&ConnectorProtocolEvidence>,
        _nonce: Option<&str>,
    ) -> Result<ConnectorAccountObservation, ConnectorOAuthRefusal> {
        self.observe(descriptor, tokens, evidence).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn facts(account: AccountSelection) -> ConnectorOAuthParameters {
        ConnectorOAuthParameters {
            issuer: GOOGLE_ISSUER.to_owned(),
            client: "client.apps.googleusercontent.com".to_owned(),
            resource: "https://gmail.googleapis.com".to_owned(),
            scopes: ["https://www.googleapis.com/auth/gmail.readonly".to_owned()]
                .into_iter()
                .collect(),
            redirect_uri: "https://host.example/connector/oauth/callback".to_owned(),
            expected_account: account,
            strategy_id: GOOGLE_USERINFO_STRATEGY_ID.to_owned(),
            client_auth_profile: Some("google".to_owned()),
            protocol: Some(GOOGLE_PROTOCOL_ID.to_owned()),
        }
    }

    /// A captured Google code-exchange response (values replaced).
    const EXCHANGE: &str = r#"{
        "access_token": "ya29.a0-access",
        "expires_in": 3599,
        "refresh_token": "1//0g-refresh",
        "scope": "https://www.googleapis.com/auth/gmail.readonly openid https://www.googleapis.com/auth/userinfo.email",
        "token_type": "Bearer",
        "id_token": "eyJhbGciOiJSUzI1NiJ9.e30.sig"
    }"#;

    /// A captured Google refresh response: no refresh token.
    const REFRESH: &str = r#"{
        "access_token": "ya29.a0-next",
        "expires_in": 3599,
        "scope": "https://www.googleapis.com/auth/gmail.readonly openid https://www.googleapis.com/auth/userinfo.email",
        "token_type": "Bearer"
    }"#;

    #[test]
    fn authorize_requests_offline_consent_incremental_scopes_and_sub_hint() {
        let protocol = GoogleProtocol::new().with_hosted_domain("Example.COM");
        assert!(protocol.binding().starts_with("google-v1@"));
        assert_ne!(protocol.binding(), GoogleProtocol::new().binding());
        let endpoints = protocol
            .endpoints(&facts(AccountSelection::Discover))
            .unwrap()
            .unwrap();
        assert_eq!(endpoints.authorization_endpoint, GOOGLE_AUTHORIZE);
        assert_eq!(endpoints.token_endpoint, GOOGLE_TOKEN);
        let params = protocol
            .authorize_params(&facts(AccountSelection::Known("1098765".into())))
            .unwrap();
        let get = |name: &str| {
            params
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(
            get("scope"),
            Some("https://www.googleapis.com/auth/gmail.readonly openid")
        );
        assert_eq!(get("access_type"), Some("offline"));
        assert_eq!(get("prompt"), Some("consent"));
        assert_eq!(get("include_granted_scopes"), Some("true"));
        assert_eq!(get("login_hint"), Some("1098765"));
        assert_eq!(get("hd"), Some("example.com"));
        assert!(get("redirect_uri").is_none() && get("client_secret").is_none());
        assert!(
            protocol
                .authorize_params(&facts(AccountSelection::Unverified))
                .is_err()
        );
        let mut other = facts(AccountSelection::Discover);
        other.issuer = "https://accounts.example.com".into();
        assert!(protocol.endpoints(&other).is_err());
    }

    #[test]
    fn exchange_requires_offline_enrollment_and_never_keeps_the_id_token() {
        let protocol = GoogleProtocol::new();
        let grant = protocol
            .decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                EXCHANGE.as_bytes(),
                None,
            )
            .unwrap();
        assert_eq!(grant.tokens.access_token, "ya29.a0-access");
        assert_eq!(grant.tokens.refresh_token.as_deref(), Some("1//0g-refresh"));
        assert_eq!(grant.tokens.expires_in_secs, Some(3599));
        assert!(grant.tokens.id_token.is_none());
        let scopes = grant.tokens.scope.unwrap();
        assert!(scopes.split(' ').any(|scope| scope == "email"));
        assert!(
            scopes
                .split(' ')
                .any(|scope| scope == "https://www.googleapis.com/auth/gmail.readonly")
        );

        let mut no_refresh: serde_json::Value = serde_json::from_str(EXCHANGE).unwrap();
        no_refresh.as_object_mut().unwrap().remove("refresh_token");
        assert!(matches!(
            protocol.decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                no_refresh.to_string().as_bytes(),
                None,
            ),
            Err(ConnectorProtocolFailure::Policy(
                "Google login established no offline refresh token"
            ))
        ));
    }

    #[test]
    fn refresh_without_a_refresh_token_retains_the_stored_one() {
        let protocol = GoogleProtocol::new();
        assert_eq!(
            protocol.refresh_token_rule(),
            RefreshTokenRule::RetainIfOmitted
        );
        assert!(!protocol.refresh_requires_scope());
        let grant = protocol
            .decode_token_response(
                ConnectorTokenGrantKind::Refresh,
                200,
                REFRESH.as_bytes(),
                None,
            )
            .unwrap();
        assert!(grant.tokens.refresh_token.is_none());
        assert_eq!(grant.tokens.access_token, "ya29.a0-next");
    }

    #[test]
    fn failures_keep_their_status_and_code() {
        let protocol = GoogleProtocol::new();
        let refused = protocol.decode_token_response(
            ConnectorTokenGrantKind::Refresh,
            400,
            br#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
            None,
        );
        assert!(matches!(
            refused,
            Err(ConnectorProtocolFailure::Refused { status: 400, code: Some(code) }) if code == "invalid_grant"
        ));
        let client = protocol.decode_token_response(
            ConnectorTokenGrantKind::Exchange,
            401,
            br#"{"error":"invalid_client","error_description":"The OAuth client was not found."}"#,
            None,
        );
        assert!(matches!(
            client,
            Err(ConnectorProtocolFailure::Refused { status: 401, code: Some(code) }) if code == "invalid_client"
        ));
        // Load and outages keep their status for the owner's classifier;
        // they are not credential evidence.
        for (status, body) in [
            (429_u16, &br#"{"error":"rate_limit_exceeded"}"#[..]),
            (503, &b"<html>unavailable</html>"[..]),
        ] {
            assert!(matches!(
                protocol.decode_token_response(ConnectorTokenGrantKind::Refresh, status, body, None),
                Err(ConnectorProtocolFailure::Refused { status: got, .. }) if got == status
            ));
        }
        assert!(matches!(
            protocol.decode_token_response(
                ConnectorTokenGrantKind::Refresh,
                200,
                br#"{"error":"invalid_grant"}"#,
                None
            ),
            Err(ConnectorProtocolFailure::Malformed(_))
        ));
        let not_bearer = EXCHANGE.replace("\"Bearer\"", "\"mac\"");
        assert!(matches!(
            protocol.decode_token_response(
                ConnectorTokenGrantKind::Exchange,
                200,
                not_bearer.as_bytes(),
                None
            ),
            Err(ConnectorProtocolFailure::Malformed(_))
        ));
        let no_expiry = REFRESH.replace("\"expires_in\": 3599,", "");
        assert!(matches!(
            protocol.decode_token_response(
                ConnectorTokenGrantKind::Refresh,
                200,
                no_expiry.as_bytes(),
                None
            ),
            Err(ConnectorProtocolFailure::Malformed(_))
        ));
    }

    #[test]
    fn endpoint_overrides_are_loopback_only() {
        assert!(
            GoogleProtocol::new()
                .with_loopback_base("http://127.0.0.1:9")
                .is_ok()
        );
        assert!(
            GoogleProtocol::new()
                .with_loopback_base("https://accounts.example.com")
                .is_err()
        );
        assert!(
            GoogleProtocol::new()
                .with_loopback_base("ftp://127.0.0.1:9")
                .is_err()
        );
        assert!(
            GoogleUserInfoStrategy::new()
                .with_loopback_base("https://userinfo.example.com")
                .is_err()
        );
    }

    #[test]
    fn only_401_is_a_credential_rejection() {
        let protocol = GoogleProtocol::new();
        let unauthenticated = br#"{"error":{"code":401,"status":"UNAUTHENTICATED"}}"#;
        let forbidden = br#"{"error":{"code":403,"status":"PERMISSION_DENIED"}}"#;
        assert_eq!(
            protocol.classify_rejection(401, unauthenticated),
            Some(ConnectorCredentialRejection::InvalidCredential)
        );
        assert_eq!(protocol.classify_rejection(403, forbidden), None);
        assert_eq!(protocol.classify_rejection(429, b"{}"), None);
        assert_eq!(protocol.classify_rejection(503, b"{}"), None);
    }

    #[test]
    fn userinfo_names_the_sub_and_only_a_verified_email() {
        let info = parse_google_userinfo(
            br#"{"sub":"1098765","email":"A@Example.com","email_verified":true,"hd":"Example.com"}"#,
        )
        .unwrap();
        assert_eq!(info.sub, "1098765");
        assert_eq!(info.verified_email.as_deref(), Some("a@example.com"));
        assert_eq!(info.hosted_domain.as_deref(), Some("example.com"));
        let unverified = parse_google_userinfo(
            br#"{"sub":"1098765","email":"a@example.com","email_verified":false}"#,
        )
        .unwrap();
        assert!(unverified.verified_email.is_none());
        assert!(
            parse_google_userinfo(br#"{"email":"a@example.com","email_verified":true}"#).is_err()
        );
    }
}
