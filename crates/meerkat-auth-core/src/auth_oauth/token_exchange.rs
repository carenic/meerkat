//! OAuth token exchange: authorization_code + refresh_token grants.

use reqwest::Client;

use super::{OAuthEndpoints, OAuthError, OAuthTokenRequestFormat, OAuthTokenResult};

#[derive(serde::Deserialize)]
struct TokenResponseWire {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

/// How a token request authenticates the OAuth client (RFC 6749 section
/// 2.3.1). A confidential client's secret goes only where its method puts it
/// and nowhere else; `Debug` never shows it.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum OAuthClientAuthentication<'a> {
    /// A public client: its `client_id` in the request body, no secret.
    Public,
    /// `client_secret_basic`: HTTP Basic authentication with the
    /// form-encoded client id and secret, and no client credentials in the
    /// body.
    SecretBasic(&'a str),
    /// `client_secret_post`: `client_id` and `client_secret` in the body.
    SecretPost(&'a str),
}

impl<'a> OAuthClientAuthentication<'a> {
    /// The body form the existing exchange functions take: a secret, when
    /// given, is sent as `client_secret` in the body.
    fn from_posted_secret(client_secret: Option<&'a str>) -> Self {
        client_secret.map_or(Self::Public, Self::SecretPost)
    }

    /// Where this method puts the client's credentials.
    fn credentials(self, client_id: &str) -> ClientCredentials<'a> {
        match self {
            Self::Public => ClientCredentials {
                body_client_id: Some(client_id.to_owned()),
                body_client_secret: None,
                basic_secret: None,
            },
            Self::SecretPost(secret) => ClientCredentials {
                body_client_id: Some(client_id.to_owned()),
                body_client_secret: Some(secret.to_owned()),
                basic_secret: None,
            },
            Self::SecretBasic(secret) => ClientCredentials {
                body_client_id: None,
                body_client_secret: None,
                basic_secret: Some(secret),
            },
        }
    }
}

/// The client credentials of one token request: in the body, or the secret
/// HTTP Basic authentication carries with the client id.
struct ClientCredentials<'a> {
    body_client_id: Option<String>,
    body_client_secret: Option<String>,
    basic_secret: Option<&'a str>,
}

impl std::fmt::Debug for OAuthClientAuthentication<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Public => "Public",
            Self::SecretBasic(_) => "SecretBasic(<redacted>)",
            Self::SecretPost(_) => "SecretPost(<redacted>)",
        })
    }
}

/// Exchange an authorization code (with PKCE verifier) for a token bundle.
pub async fn exchange_authorization_code(
    http: &Client,
    endpoints: &OAuthEndpoints,
    code: &str,
    pkce_verifier: &str,
    client_secret: Option<&str>,
) -> Result<OAuthTokenResult, OAuthError> {
    exchange_authorization_code_with_state(
        http,
        endpoints,
        code,
        pkce_verifier,
        client_secret,
        None,
    )
    .await
}

/// Exchange an authorization code while echoing the callback state for
/// providers that require it in the token request.
pub async fn exchange_authorization_code_with_state(
    http: &Client,
    endpoints: &OAuthEndpoints,
    code: &str,
    pkce_verifier: &str,
    client_secret: Option<&str>,
    state: Option<&str>,
) -> Result<OAuthTokenResult, OAuthError> {
    exchange_authorization_code_with_client_auth(
        http,
        endpoints,
        code,
        pkce_verifier,
        OAuthClientAuthentication::from_posted_secret(client_secret),
        state,
    )
    .await
}

/// [`exchange_authorization_code_with_state`] authenticating the client by
/// `client_auth`.
pub async fn exchange_authorization_code_with_client_auth(
    http: &Client,
    endpoints: &OAuthEndpoints,
    code: &str,
    pkce_verifier: &str,
    client_auth: OAuthClientAuthentication<'_>,
    state: Option<&str>,
) -> Result<OAuthTokenResult, OAuthError> {
    let credentials = client_auth.credentials(&endpoints.client_id);
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", endpoints.redirect_uri.clone()),
    ];
    if let Some(client_id) = credentials.body_client_id {
        form.push(("client_id", client_id));
    }
    form.push(("code_verifier", pkce_verifier.to_string()));
    if let Some(secret) = credentials.body_client_secret {
        form.push(("client_secret", secret));
    }
    if endpoints.include_state_in_token_exchange {
        let state = state.ok_or_else(|| {
            OAuthError::InvalidConfig("provider requires state in token exchange".into())
        })?;
        form.push(("state", state.to_string()));
    }
    for (key, value) in &endpoints.extra_token_params {
        form.push((key.as_str(), value.clone()));
    }
    send_token_request(http, endpoints, &form, credentials.basic_secret).await
}

/// Exchange a refresh_token for a fresh access token.
pub async fn exchange_refresh_token(
    http: &Client,
    endpoints: &OAuthEndpoints,
    refresh_token: &str,
    client_secret: Option<&str>,
) -> Result<OAuthTokenResult, OAuthError> {
    exchange_refresh_token_with_client_auth(
        http,
        endpoints,
        refresh_token,
        OAuthClientAuthentication::from_posted_secret(client_secret),
    )
    .await
}

/// [`exchange_refresh_token`] authenticating the client by `client_auth`.
pub async fn exchange_refresh_token_with_client_auth(
    http: &Client,
    endpoints: &OAuthEndpoints,
    refresh_token: &str,
    client_auth: OAuthClientAuthentication<'_>,
) -> Result<OAuthTokenResult, OAuthError> {
    let credentials = client_auth.credentials(&endpoints.client_id);
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(client_id) = credentials.body_client_id {
        form.push(("client_id", client_id));
    }
    if let Some(secret) = credentials.body_client_secret {
        form.push(("client_secret", secret));
    }
    if !endpoints.refresh_scopes.is_empty() {
        form.push(("scope", endpoints.refresh_scopes.join(" ")));
    }
    for (key, value) in &endpoints.extra_token_params {
        form.push((key.as_str(), value.clone()));
    }
    send_token_request(http, endpoints, &form, credentials.basic_secret).await
}

/// `basic_secret`: the client secret HTTP Basic authentication carries with
/// `endpoints.client_id` (RFC 6749 section 2.3.1: each form-encoded, then
/// joined by `:`). The header is marked sensitive.
async fn send_token_request(
    http: &Client,
    endpoints: &OAuthEndpoints,
    params: &[(&str, String)],
    basic_secret: Option<&str>,
) -> Result<OAuthTokenResult, OAuthError> {
    let mut req = match endpoints.token_request_format {
        OAuthTokenRequestFormat::FormUrlEncoded => http.post(&endpoints.token_url).form(params),
        OAuthTokenRequestFormat::Json => {
            let body = params
                .iter()
                .map(|(key, value)| ((*key).to_string(), serde_json::Value::String(value.clone())))
                .collect::<serde_json::Map<_, _>>();
            http.post(&endpoints.token_url).json(&body)
        }
    };
    if let Some(secret) = basic_secret {
        req = req.basic_auth(
            urlencoding::encode(&endpoints.client_id),
            Some(urlencoding::encode(secret)),
        );
    }
    for (k, v) in &endpoints.extra_headers {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| {
        // The host egress profile's own resolver refused the endpoint.
        match crate::http_egress::egress_refusal_in(&e) {
            Some(refusal) => OAuthError::EgressRefused(refusal),
            None => super::credential_request_error(e),
        }
    })?;
    let status = resp.status();
    super::refuse_credential_redirect(status)?;
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OAuthError::TokenEndpoint {
            status: status.as_u16(),
            body,
        });
    }
    let wire: TokenResponseWire = resp.json().await.map_err(super::credential_decode_error)?;
    Ok(OAuthTokenResult {
        access_token: wire.access_token,
        refresh_token: wire.refresh_token,
        id_token: wire.id_token,
        expires_in_secs: wire.expires_in,
        scope: wire.scope,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::auth_oauth::redirect_fixture::{
        BODY_CANARY, LOCATION_CANARY, spawn_redirecting_endpoint,
    };
    use crate::auth_oauth::{credential_http_client, poll_device_code, request_device_code};
    use std::sync::atomic::Ordering;

    fn endpoints(token_url: &str) -> OAuthEndpoints {
        OAuthEndpoints {
            client_id: "client".into(),
            authorize_url: "https://issuer.example/authorize".into(),
            token_url: token_url.into(),
            device_code_url: Some(token_url.into()),
            redirect_uri: "http://127.0.0.1:1/callback".into(),
            scopes: vec!["scope".into()],
            extra_authorize_params: Vec::new(),
            token_request_format: OAuthTokenRequestFormat::FormUrlEncoded,
            include_state_in_token_exchange: false,
            extra_token_params: Vec::new(),
            refresh_scopes: Vec::new(),
            extra_headers: Vec::new(),
        }
    }

    fn assert_refused(error: OAuthError) {
        assert!(
            matches!(error, OAuthError::RedirectRefused { status: 302 }),
            "{error:?}"
        );
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains(LOCATION_CANARY), "{rendered}");
        assert!(!rendered.contains(BODY_CANARY), "{rendered}");
    }

    #[tokio::test]
    async fn credential_endpoints_never_follow_a_redirect() {
        let http = credential_http_client().unwrap();
        let (url, target_hits) = spawn_redirecting_endpoint().await;
        let endpoints = endpoints(&url);
        assert_refused(
            exchange_authorization_code(&http, &endpoints, "code", "verifier", None)
                .await
                .unwrap_err(),
        );
        assert_refused(
            exchange_refresh_token(&http, &endpoints, "refresh-secret", Some("client-secret"))
                .await
                .unwrap_err(),
        );
        assert_refused(request_device_code(&http, &endpoints).await.unwrap_err());
        assert_refused(
            poll_device_code(&http, &endpoints, "device-code", None)
                .await
                .err()
                .expect("a redirect answer is refused"),
        );
        assert_eq!(
            target_hits.load(Ordering::SeqCst),
            0,
            "no grant, refresh token or device code reached the redirect target"
        );
    }

    /// A token endpoint that accepts the request and never answers ends the
    /// exchange as [`OAuthError::Timeout`]. Virtual time.
    #[tokio::test(start_paused = true)]
    async fn a_token_request_its_endpoint_never_answers_is_oauth_timeout() {
        let address = crate::auth_oauth::redirect_fixture::spawn_slow_endpoint(None).await;
        let http = credential_http_client().unwrap();
        let endpoints = endpoints(&format!("http://{address}/token"));
        let bound = crate::auth_oauth::CREDENTIAL_REQUEST_TIMEOUT * 10;
        let error = tokio::time::timeout(
            bound,
            exchange_authorization_code(&http, &endpoints, "code", "verifier", None),
        )
        .await
        .expect("the client ends the exchange itself")
        .unwrap_err();
        assert!(matches!(error, OAuthError::Timeout), "{error:?}");
        let error = tokio::time::timeout(
            bound,
            exchange_refresh_token(&http, &endpoints, "refresh-secret", None),
        )
        .await
        .expect("the client ends the refresh itself")
        .unwrap_err();
        assert!(matches!(error, OAuthError::Timeout), "{error:?}");
    }

    #[test]
    fn same_origin_compares_scheme_host_and_port() {
        use crate::auth_oauth::same_origin;
        let url = |raw: &str| reqwest::Url::parse(raw).unwrap();
        assert!(same_origin(
            &url("https://h.example/mcp"),
            &url("https://h.example/mcp/")
        ));
        assert!(same_origin(
            &url("https://h.example/a"),
            &url("https://h.example:443/b")
        ));
        // A downgrade on the same host and port is another origin.
        assert!(!same_origin(
            &url("https://h.example:8443/mcp"),
            &url("http://h.example:8443/mcp")
        ));
        assert!(!same_origin(
            &url("https://h.example/mcp"),
            &url("https://cdn.example/mcp")
        ));
        assert!(!same_origin(
            &url("https://h.example/mcp"),
            &url("https://h.example:444/mcp")
        ));
    }

    /// One token request as the endpoint received it: its `Authorization`
    /// header and its form body.
    type RecordedTokenRequest = (Option<String>, std::collections::BTreeMap<String, String>);

    type RecordedTokenRequests = std::sync::Arc<parking_lot::Mutex<Vec<RecordedTokenRequest>>>;

    async fn record_token_request(
        axum::extract::State(recorded): axum::extract::State<RecordedTokenRequests>,
        headers: axum::http::HeaderMap,
        axum::Form(body): axum::Form<std::collections::BTreeMap<String, String>>,
    ) -> axum::Json<serde_json::Value> {
        let authorization = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        recorded.lock().push((authorization, body));
        axum::Json(serde_json::json!({
            "access_token": "access",
            "refresh_token": "refresh-next",
            "expires_in": 60
        }))
    }

    /// A token endpoint that records each request and answers a token.
    async fn spawn_recording_token_endpoint() -> (String, RecordedTokenRequests) {
        let recorded = RecordedTokenRequests::default();
        let app = axum::Router::new()
            .route("/token", axum::routing::post(record_token_request))
            .with_state(std::sync::Arc::clone(&recorded));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/token"), recorded)
    }

    /// The client id and secret of a Basic header, decoded as an
    /// authorization server does (RFC 6749 section 2.3.1: each part
    /// form-encoded, then joined by `:`).
    fn basic_credentials(header: &str) -> (String, String) {
        use base64::Engine as _;
        let encoded = header.strip_prefix("Basic ").expect("a Basic header");
        let decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
        )
        .unwrap();
        // The client id's own `:` is encoded, so the first one separates.
        let (client_id, secret) = decoded.split_once(':').unwrap();
        (
            urlencoding::decode(client_id).unwrap().into_owned(),
            urlencoding::decode(secret).unwrap().into_owned(),
        )
    }

    const CLIENT_SECRET: &str = "s3cret/with+reserved=chars ~";

    #[tokio::test]
    async fn client_secret_basic_authenticates_in_the_header_only() {
        let (url, recorded) = spawn_recording_token_endpoint().await;
        let http = credential_http_client().unwrap();
        let mut endpoints = endpoints(&url);
        endpoints.client_id = "fixed.client:1".into();
        let auth = OAuthClientAuthentication::SecretBasic(CLIENT_SECRET);
        exchange_authorization_code_with_client_auth(
            &http, &endpoints, "code", "verifier", auth, None,
        )
        .await
        .unwrap();
        exchange_refresh_token_with_client_auth(&http, &endpoints, "refresh", auth)
            .await
            .unwrap();
        let recorded = recorded.lock().clone();
        assert_eq!(recorded.len(), 2);
        for (authorization, body) in &recorded {
            let authorization = authorization.as_deref().expect("HTTP Basic is sent");
            assert_eq!(
                basic_credentials(authorization),
                ("fixed.client:1".to_owned(), CLIENT_SECRET.to_owned())
            );
            assert!(!body.contains_key("client_id"), "{body:?}");
            assert!(!body.contains_key("client_secret"), "{body:?}");
        }
        assert_eq!(recorded[0].1["grant_type"], "authorization_code");
        assert_eq!(recorded[0].1["code_verifier"], "verifier");
        assert_eq!(recorded[1].1["grant_type"], "refresh_token");
        assert_eq!(recorded[1].1["refresh_token"], "refresh");
    }

    #[tokio::test]
    async fn client_secret_post_authenticates_in_the_body_only() {
        let (url, recorded) = spawn_recording_token_endpoint().await;
        let http = credential_http_client().unwrap();
        let endpoints = endpoints(&url);
        let auth = OAuthClientAuthentication::SecretPost(CLIENT_SECRET);
        exchange_authorization_code_with_client_auth(
            &http, &endpoints, "code", "verifier", auth, None,
        )
        .await
        .unwrap();
        exchange_refresh_token_with_client_auth(&http, &endpoints, "refresh", auth)
            .await
            .unwrap();
        // The existing form: a secret given to it is posted the same way.
        exchange_refresh_token(&http, &endpoints, "refresh", Some(CLIENT_SECRET))
            .await
            .unwrap();
        let recorded = recorded.lock().clone();
        assert_eq!(recorded.len(), 3);
        for (authorization, body) in &recorded {
            assert_eq!(authorization, &None);
            assert_eq!(body["client_id"], "client");
            assert_eq!(body["client_secret"], CLIENT_SECRET);
        }
    }

    #[tokio::test]
    async fn a_public_client_sends_its_id_and_no_secret() {
        let (url, recorded) = spawn_recording_token_endpoint().await;
        let http = credential_http_client().unwrap();
        let endpoints = endpoints(&url);
        exchange_authorization_code(&http, &endpoints, "code", "verifier", None)
            .await
            .unwrap();
        exchange_refresh_token_with_client_auth(
            &http,
            &endpoints,
            "refresh",
            OAuthClientAuthentication::Public,
        )
        .await
        .unwrap();
        for (authorization, body) in recorded.lock().iter() {
            assert_eq!(authorization, &None);
            assert_eq!(body["client_id"], "client");
            assert!(!body.contains_key("client_secret"), "{body:?}");
        }
    }

    #[test]
    fn client_authentication_debug_never_shows_the_secret() {
        for auth in [
            OAuthClientAuthentication::SecretBasic(CLIENT_SECRET),
            OAuthClientAuthentication::SecretPost(CLIENT_SECRET),
        ] {
            let rendered = format!("{auth:?} {auth:#?}");
            assert!(!rendered.contains("s3cret"), "{rendered}");
            assert!(rendered.contains("<redacted>"), "{rendered}");
        }
    }
}
