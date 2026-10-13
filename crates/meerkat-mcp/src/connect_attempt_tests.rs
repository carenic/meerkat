//! A caller-held connect/inspection attempt over Streamable HTTP: a caller
//! that cancels or drops the attempt at each paused step stops every later
//! request and publication, and the attempt observes its local termination
//! separately from whether the server confirmed the session's deletion.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::StreamExt as _;
use meerkat_auth_core::{McpAuthMode, McpOAuthError, McpServerIdentity};
use meerkat_core::McpServerConfig;
use rmcp::transport::streamable_http_client::StreamableHttpClient as _;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::transport::streamable_http::{AuthChallengeRecorder, ReqwestStreamableHttpClient};
use crate::{
    HttpEgressProfile, McpAuthResolver, McpConnectAttempt, McpConnection, McpLocalTermination,
    McpRemoteSession,
};

const BOUND: Duration = Duration::from_secs(10);

/// The one step the server holds until released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pause {
    InitializeHeaders,
    InitializeBody,
    Initialized,
    ToolsList,
    Delete,
}

/// A Streamable HTTP MCP server that issues a session, logs every request
/// and holds one step.
struct Server {
    pause: Option<Pause>,
    /// The status its session deletion answers with.
    delete_status: StatusCode,
    /// Whether its GET opens an event stream it then holds open.
    sse_get: bool,
    /// Whether its initialize answer opens a session.
    issues_session: bool,
    reached: Notify,
    release: Notify,
    requests: Mutex<Vec<String>>,
}

impl Server {
    fn new(pause: Option<Pause>) -> Self {
        Self {
            pause,
            delete_status: StatusCode::OK,
            sse_get: false,
            issues_session: true,
            reached: Notify::new(),
            release: Notify::new(),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn log(&self, request: impl Into<String>) {
        self.requests.lock().unwrap().push(request.into());
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    async fn hold(&self, step: Pause) {
        if self.pause == Some(step) {
            self.reached.notify_one();
            self.release.notified().await;
        }
    }

    async fn reached(&self) {
        tokio::time::timeout(BOUND, self.reached.notified())
            .await
            .expect("the attempt reaches the held step");
    }
}

async fn spawn(pause: Option<Pause>) -> (String, Arc<Server>) {
    serve(Server::new(pause)).await
}

async fn serve(server: Server) -> (String, Arc<Server>) {
    let server = Arc::new(server);
    let app = Router::new()
        .route("/mcp", post(on_post).get(on_get).delete(on_delete))
        .with_state(Arc::clone(&server));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, server)
}

async fn on_post(State(server): State<Arc<Server>>, body: String) -> Response {
    let request: Value = serde_json::from_str(&body).unwrap();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("response")
        .to_owned();
    server.log(format!("POST {method}"));
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match method.as_str() {
        "initialize" => {
            server.hold(Pause::InitializeHeaders).await;
            let result = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "held-server", "version": "0.1.0" }
                }
            })
            .to_string();
            let body = if server.pause == Some(Pause::InitializeBody) {
                let server = Arc::clone(&server);
                Body::from_stream(futures::stream::once(async move {
                    server.hold(Pause::InitializeBody).await;
                    Ok::<_, Infallible>(Bytes::from(result))
                }))
            } else {
                Body::from(result)
            };
            let answer = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json");
            let answer = if server.issues_session {
                answer.header("mcp-session-id", "held-session")
            } else {
                answer
            };
            answer.body(body).unwrap()
        }
        "notifications/initialized" => {
            server.hold(Pause::Initialized).await;
            StatusCode::ACCEPTED.into_response()
        }
        "tools/list" => {
            server.hold(Pause::ToolsList).await;
            axum::Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [{
                    "name": "echo",
                    "description": "Echo input",
                    "inputSchema": { "type": "object", "properties": {} }
                }] }
            }))
            .into_response()
        }
        _ => axum::Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "unsupported" }
        }))
        .into_response(),
    }
}

async fn on_get(State(server): State<Arc<Server>>) -> Response {
    server.log("GET");
    if !server.sse_get {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    // An event stream that opens, then stays open until the client goes.
    let events =
        futures::stream::once(async { Ok::<_, Infallible>(Bytes::from_static(b": open\n\n")) })
            .chain(futures::stream::pending());
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(events))
        .unwrap()
}

async fn on_delete(State(server): State<Arc<Server>>) -> StatusCode {
    server.log("DELETE");
    server.hold(Pause::Delete).await;
    server.delete_status
}

fn config(url: String) -> McpServerConfig {
    McpServerConfig::streamable_http("held", url, HashMap::new())
}

async fn inspect(
    config: &McpServerConfig,
    attempt: &McpConnectAttempt,
) -> Result<(McpConnection, Vec<Arc<meerkat_core::ToolDef>>), crate::McpError> {
    McpConnection::connect_and_enumerate_in_attempt(
        config,
        McpAuthMode::Stored,
        None,
        None,
        &HttpEgressProfile::trusted_host(),
        attempt,
    )
    .await
}

/// After the attempt settled, releasing the held step brings no request.
async fn assert_nothing_more(server: &Server) -> Vec<String> {
    tokio::time::sleep(Duration::from_millis(100)).await;
    let settled = server.requests();
    server.release.notify_one();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server.requests(),
        settled,
        "no request after the attempt ended"
    );
    settled
}

#[tokio::test]
async fn a_dropped_inspection_held_in_initialize_headers_joins_and_sends_nothing_more() {
    let (url, server) = spawn(Some(Pause::InitializeHeaders)).await;
    let attempt = McpConnectAttempt::new();
    let inspection = tokio::spawn({
        let (config, attempt) = (config(url), attempt.clone());
        async move { inspect(&config, &attempt).await.map(|_| ()) }
    });
    server.reached().await;
    inspection.abort();
    assert!(
        inspection
            .await
            .err()
            .is_some_and(|error| error.is_cancelled())
    );
    let settled = attempt.settle(BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    // The initialize reached the server and its answer was never seen: the
    // server may have opened a session, so its absence is not proved.
    assert_eq!(settled.remote_session, McpRemoteSession::Unconfirmed);
    assert_eq!(assert_nothing_more(&server).await, vec!["POST initialize"]);
}

#[tokio::test]
async fn an_answer_without_a_session_proves_no_session_and_deletes_nothing() {
    let mut fixture = Server::new(None);
    fixture.issues_session = false;
    let (url, server) = serve(fixture).await;
    let attempt = McpConnectAttempt::new();
    let (connection, tools) = inspect(&config(url), &attempt)
        .await
        .expect("a stateless inspection");
    assert_eq!(tools.len(), 1);
    let settled = connection.close_in_attempt(&attempt, BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::NoSession);
    let requests = server.requests();
    assert!(!requests.contains(&"DELETE".to_owned()), "{requests:?}");
}

#[tokio::test]
async fn a_cancelled_inspection_held_in_the_initialize_body_publishes_nothing() {
    let (url, server) = spawn(Some(Pause::InitializeBody)).await;
    let attempt = McpConnectAttempt::new();
    let inspection = tokio::spawn({
        let (config, attempt) = (config(url), attempt.clone());
        async move {
            inspect(&config, &attempt)
                .await
                .map(|(_, tools)| tools.len())
        }
    });
    server.reached().await;
    // The body is held, but its head may still be in flight: cancel only once
    // the attempt saw the session id the head carries.
    tokio::time::timeout(BOUND, async {
        while attempt.settle(Duration::ZERO).await.remote_session == McpRemoteSession::NoSession {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the response head arrives before its held body");
    attempt.cancel();
    let inspected = tokio::time::timeout(BOUND, inspection)
        .await
        .expect("a cancelled inspection returns")
        .expect("inspection task");
    assert!(inspected.is_err(), "nothing is published: {inspected:?}");
    let settled = attempt.settle(BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    // The session id arrived with the head; it was never deleted.
    assert_eq!(settled.remote_session, McpRemoteSession::Unconfirmed);
    assert_eq!(assert_nothing_more(&server).await, vec!["POST initialize"]);
}

#[tokio::test]
async fn a_dropped_inspection_held_at_the_initialized_notification_sends_nothing_more() {
    let (url, server) = spawn(Some(Pause::Initialized)).await;
    let attempt = McpConnectAttempt::new();
    let inspection = tokio::spawn({
        let (config, attempt) = (config(url), attempt.clone());
        async move { inspect(&config, &attempt).await.map(|_| ()) }
    });
    server.reached().await;
    inspection.abort();
    assert!(
        inspection
            .await
            .err()
            .is_some_and(|error| error.is_cancelled())
    );
    let settled = attempt.settle(BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::Unconfirmed);
    assert_eq!(
        assert_nothing_more(&server).await,
        vec!["POST initialize", "POST notifications/initialized"]
    );
}

#[tokio::test]
async fn a_cancelled_inspection_held_in_tools_list_publishes_nothing_and_deletes_nothing() {
    let (url, server) = spawn(Some(Pause::ToolsList)).await;
    let attempt = McpConnectAttempt::new();
    let inspection = tokio::spawn({
        let (config, attempt) = (config(url), attempt.clone());
        async move {
            inspect(&config, &attempt)
                .await
                .map(|(_, tools)| tools.len())
        }
    });
    server.reached().await;
    attempt.cancel();
    let inspected = tokio::time::timeout(BOUND, inspection)
        .await
        .expect("a cancelled inspection returns")
        .expect("inspection task");
    assert!(inspected.is_err(), "nothing is published: {inspected:?}");
    let settled = attempt.settle(BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::Unconfirmed);
    let requests = assert_nothing_more(&server).await;
    assert!(
        requests.contains(&"POST tools/list".to_owned()),
        "{requests:?}"
    );
    assert!(!requests.contains(&"DELETE".to_owned()), "{requests:?}");
}

#[tokio::test]
async fn a_close_held_in_session_deletion_joins_locally_and_leaves_the_deletion_unconfirmed() {
    let (url, server) = spawn(Some(Pause::Delete)).await;
    let attempt = McpConnectAttempt::new();
    let (connection, tools) = inspect(&config(url), &attempt).await.expect("inspection");
    assert_eq!(tools.len(), 1);
    let close = tokio::spawn({
        let attempt = attempt.clone();
        async move {
            connection
                .close_in_attempt(&attempt, Duration::from_secs(2))
                .await
        }
    });
    server.reached().await;
    let settled = tokio::time::timeout(BOUND, close)
        .await
        .expect("a bounded close returns")
        .expect("close task");
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::Unconfirmed);
    let requests = assert_nothing_more(&server).await;
    assert_eq!(
        requests
            .iter()
            .filter(|request| *request == "DELETE")
            .count(),
        1,
        "{requests:?}"
    );
}

#[tokio::test]
async fn an_attempt_refuses_a_transport_it_cannot_observe_before_any_effect() {
    let attempt = McpConnectAttempt::new();
    let stdio = McpServerConfig::stdio("local", "/nonexistent/never-run", vec![], HashMap::new());
    let refused = inspect(&stdio, &attempt).await.err().expect("refused");
    assert!(
        matches!(refused, crate::McpError::ConnectionFailed { ref reason } if reason.contains("Streamable HTTP")),
        "{refused:?}"
    );
    assert!(!attempt.is_cancelled(), "nothing was started to cancel");
    let settled = attempt.settle(BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::NoSession);
}

#[tokio::test]
async fn a_normal_inspection_and_close_join_and_confirm_deletion_beside_a_held_sibling() {
    let (held_url, held) = spawn(Some(Pause::InitializeHeaders)).await;
    let held_attempt = McpConnectAttempt::new();
    let held_inspection = tokio::spawn({
        let (config, attempt) = (config(held_url), held_attempt.clone());
        async move { inspect(&config, &attempt).await.map(|_| ()) }
    });
    held.reached().await;

    // The permitted sibling inspects and closes normally meanwhile.
    let (url, server) = spawn(None).await;
    let attempt = McpConnectAttempt::new();
    let (connection, tools) = inspect(&config(url), &attempt)
        .await
        .expect("sibling inspection");
    assert_eq!(tools.len(), 1);
    let settled = connection.close_in_attempt(&attempt, BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::Deleted);
    let requests = server.requests();
    assert!(
        requests.contains(&"POST tools/list".to_owned()),
        "{requests:?}"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| *request == "DELETE")
            .count(),
        1,
        "{requests:?}"
    );

    held_attempt.cancel();
    let held_result = tokio::time::timeout(BOUND, held_inspection)
        .await
        .expect("the held inspection returns")
        .expect("held task");
    assert!(held_result.is_err());
    assert_eq!(
        held_attempt.settle(BOUND).await.local,
        McpLocalTermination::Joined
    );
}

/// A credential source whose stored-bearer read is held until released.
#[derive(Default)]
struct HeldPreflight {
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl McpAuthResolver for HeldPreflight {
    async fn stored_bearer_token(
        &self,
        _target: &McpServerIdentity,
    ) -> Result<Option<String>, McpOAuthError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(None)
    }

    async fn interactive_login(
        &self,
        target: &McpServerIdentity,
        _www_authenticate: Option<&str>,
    ) -> Result<String, McpOAuthError> {
        Err(McpOAuthError::HumanAuthorizationRequired {
            server_name: target.server_name().to_owned(),
        })
    }
}

#[tokio::test]
async fn a_cancelled_inspection_still_held_in_its_credential_preflight_is_not_joined_until_its_task_ends()
 {
    let (url, server) = spawn(None).await;
    let resolver = Arc::new(HeldPreflight::default());
    let attempt = McpConnectAttempt::new();
    let config = config(url);
    let egress = HttpEgressProfile::trusted_host();
    let mut inspection = Box::pin(McpConnection::connect_and_enumerate_in_attempt(
        &config,
        McpAuthMode::Stored,
        Some(Arc::clone(&resolver) as Arc<dyn McpAuthResolver>),
        None,
        &egress,
        &attempt,
    ));
    tokio::select! {
        biased;
        _ = &mut inspection => panic!("the credential preflight is held"),
        () = resolver.entered.notified() => {}
    }
    // Requested, not observed: the inspection is still held, unpolled.
    attempt.cancel();
    assert_eq!(
        attempt.settle(Duration::from_millis(200)).await.local,
        McpLocalTermination::NotObserved
    );
    drop(inspection);
    let settled = attempt.settle(BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::NoSession);
    assert!(server.requests().is_empty(), "nothing was sent");
}

#[tokio::test]
async fn a_held_response_stream_keeps_its_attempt_unjoined_until_the_stream_drops() {
    let mut fixture = Server::new(None);
    fixture.sse_get = true;
    let (url, server) = serve(fixture).await;
    let attempt = McpConnectAttempt::new();
    let client = ReqwestStreamableHttpClient::new_with_auth_challenge(
        reqwest::header::HeaderMap::new(),
        AuthChallengeRecorder::default(),
    )
    .with_attempt(&attempt);
    let stream = client
        .get_stream(
            url.into(),
            "held-session".into(),
            None,
            None,
            HashMap::new(),
        )
        .await
        .expect("the server holds an event stream open");
    // The task holding a response stream can outlive its client.
    drop(client);
    assert_eq!(
        attempt.settle(Duration::from_millis(200)).await.local,
        McpLocalTermination::NotObserved
    );
    drop(stream);
    assert_eq!(
        attempt.settle(BOUND).await.local,
        McpLocalTermination::Joined
    );
    assert_eq!(server.requests(), vec!["GET"]);
}

#[tokio::test]
async fn retained_work_stays_unjoined_for_a_dropped_waiter_and_concurrent_settles_until_it_ends() {
    let attempt = McpConnectAttempt::new();
    let release = Arc::new(Notify::new());
    attempt.spawn_retained({
        let release = Arc::clone(&release);
        async move { release.notified().await }
    });
    // A settlement dropped while it waits changes nothing a later one sees.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), attempt.settle(BOUND))
            .await
            .is_err()
    );
    assert_eq!(
        attempt.settle(Duration::from_millis(100)).await.local,
        McpLocalTermination::NotObserved
    );
    // Concurrent settlements each observe the work still running.
    let (first, second) = tokio::join!(
        attempt.settle(Duration::from_millis(100)),
        attempt.settle(Duration::from_millis(100))
    );
    assert_eq!(first.local, McpLocalTermination::NotObserved);
    assert_eq!(second.local, McpLocalTermination::NotObserved);
    release.notify_one();
    assert_eq!(
        attempt.settle(BOUND).await.local,
        McpLocalTermination::Joined
    );
}

#[tokio::test]
async fn a_close_held_in_its_deletion_is_unjoined_for_dropped_and_concurrent_settles() {
    let (url, server) = spawn(Some(Pause::Delete)).await;
    let attempt = McpConnectAttempt::new();
    let (connection, _) = inspect(&config(url), &attempt).await.expect("inspection");
    let close = tokio::spawn({
        let attempt = attempt.clone();
        async move { connection.close_in_attempt(&attempt, BOUND * 3).await }
    });
    server.reached().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), attempt.settle(BOUND))
            .await
            .is_err()
    );
    assert_eq!(
        attempt.settle(Duration::from_millis(100)).await.local,
        McpLocalTermination::NotObserved
    );
    let (first, second) = tokio::join!(
        attempt.settle(Duration::from_millis(100)),
        attempt.settle(Duration::from_millis(100))
    );
    assert_eq!(first.local, McpLocalTermination::NotObserved);
    assert_eq!(second.local, McpLocalTermination::NotObserved);
    server.release.notify_one();
    let closed = tokio::time::timeout(BOUND, close)
        .await
        .expect("the released close returns")
        .expect("close task");
    assert_eq!(closed.local, McpLocalTermination::Joined);
    assert_eq!(closed.remote_session, McpRemoteSession::Deleted);
}

#[tokio::test]
async fn a_deletion_answered_without_a_success_status_is_unconfirmed() {
    let mut fixture = Server::new(None);
    fixture.delete_status = StatusCode::MULTIPLE_CHOICES;
    let (url, server) = serve(fixture).await;
    let attempt = McpConnectAttempt::new();
    let (connection, _) = inspect(&config(url), &attempt).await.expect("inspection");
    let settled = connection.close_in_attempt(&attempt, BOUND).await;
    assert_eq!(settled.local, McpLocalTermination::Joined);
    assert_eq!(settled.remote_session, McpRemoteSession::Unconfirmed);
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| *request == "DELETE")
            .count(),
        1,
        "{requests:?}"
    );
}
