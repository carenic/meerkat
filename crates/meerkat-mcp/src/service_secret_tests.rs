//! Realm service secrets at the stdio launch seam: what a launch receives,
//! and that every refusal comes before the process exists. The custody
//! resolution itself is the host's ([`McpAuthResolver::stdio_service_secrets`]).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use meerkat_core::auth::{McpServiceSecretRefusal, McpServiceSecretValues};
use meerkat_core::mcp_attachment::{McpAttachmentAuthority, McpAttachmentRefusal};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const SLOT: &str = "SERVICE_TOKEN";
const BOUND: Duration = Duration::from_secs(15);

/// The fixture's digest of an environment value (its `env_digest` tool).
fn digest(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The fixture server reporting `SERVICE_TOKEN`, declaring it as a secret.
fn secret_server(name: &str) -> McpServerConfig {
    let mut server = McpServerConfig::stdio(
        name,
        mcp_test_server::fixture_binary()
            .to_string_lossy()
            .to_string(),
        vec!["--env-digest".to_owned(), SLOT.to_owned()],
        HashMap::new(),
    );
    if let McpTransportConfig::Stdio(stdio) = &mut server.transport {
        stdio.secret_env.insert(SLOT.to_owned());
    }
    server
}

/// A credential source whose service-secret custody the test controls.
#[derive(Default)]
struct Custody {
    /// The stored value; `None` is nothing stored.
    value: std::sync::Mutex<Option<String>>,
    /// A slot the custody answers in addition to the declared one.
    extra: Option<&'static str>,
    reads: AtomicUsize,
    /// When set, each read waits for `release` after signalling `entered`.
    held: Option<(tokio::sync::Notify, tokio::sync::Notify)>,
}

impl Custody {
    fn storing(value: &str) -> Self {
        Self {
            value: std::sync::Mutex::new(Some(value.to_owned())),
            ..Self::default()
        }
    }

    fn held(value: &str) -> Self {
        Self {
            held: Some((tokio::sync::Notify::new(), tokio::sync::Notify::new())),
            ..Self::storing(value)
        }
    }

    fn set(&self, value: Option<&str>) {
        *self.value.lock().unwrap() = value.map(str::to_owned);
    }

    async fn entered(&self) {
        let (entered, _) = self.held.as_ref().expect("held custody");
        tokio::time::timeout(BOUND, entered.notified())
            .await
            .expect("the launch reaches its custody read");
    }

    fn release(&self) {
        let (_, release) = self.held.as_ref().expect("held custody");
        release.notify_one();
    }
}

#[async_trait]
impl McpAuthResolver for Custody {
    async fn stored_bearer_token(
        &self,
        _target: &McpServerIdentity,
    ) -> Result<Option<String>, McpOAuthError> {
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

    async fn stdio_service_secrets(
        &self,
        _server: &McpServerConfig,
    ) -> Result<McpServiceSecretValues, McpServiceSecretRefusal> {
        if let Some((entered, release)) = &self.held {
            entered.notify_one();
            release.notified().await;
        }
        self.reads.fetch_add(1, Ordering::SeqCst);
        let Some(value) = self.value.lock().unwrap().clone() else {
            return Err(McpServiceSecretRefusal::NotStored {
                env: SLOT.to_owned(),
            });
        };
        let mut values = McpServiceSecretValues::new();
        values.insert(SLOT, value);
        if let Some(extra) = self.extra {
            values.insert(extra, "unrequested");
        }
        Ok(values)
    }
}

/// A credential source that serves no service secrets (the trait default).
struct OAuthOnly;

#[async_trait]
impl McpAuthResolver for OAuthOnly {
    async fn stored_bearer_token(
        &self,
        _target: &McpServerIdentity,
    ) -> Result<Option<String>, McpOAuthError> {
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

/// A governed control whose apply decision the test revokes.
struct Revocable {
    server: McpServerConfig,
    allow: AtomicBool,
    checks: AtomicUsize,
}

impl McpAttachmentAuthority for Revocable {
    fn server(&self) -> &McpServerConfig {
        &self.server
    }

    fn authorize_apply(&self) -> Result<(), McpAttachmentRefusal> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        if self.allow.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(McpAttachmentRefusal::Denied)
        }
    }

    fn authorize_persist(
        &self,
        _scope: meerkat_core::mcp_config::McpScope,
    ) -> Result<(), McpAttachmentRefusal> {
        Err(McpAttachmentRefusal::Denied)
    }
}

/// Connect `config` under `custody`'s process custody.
async fn launch(
    config: &McpServerConfig,
    resolver: Option<Arc<dyn McpAuthResolver>>,
    custody: &StdioChildCustody,
    control: Option<Arc<dyn McpAttachmentAuthority>>,
) -> Result<(McpConnection, Vec<Arc<ToolDef>>), McpError> {
    McpConnection::connect_and_enumerate_with_custody(
        config,
        McpAuthMode::Stored,
        resolver,
        None,
        Some(custody.clone()),
        &McpStdioLaunchProfile::trusted_host(),
        control,
    )
    .await
}

fn never_spawned(custody: &StdioChildCustody) -> bool {
    custody.spawned.borrow().is_none()
}

async fn launched_digest(connection: &McpConnection) -> String {
    let result = connection
        .call_tool("env_digest", &serde_json::json!({}))
        .await
        .expect("env_digest call");
    meerkat_core::types::text_content(&result)
}

#[tokio::test]
async fn a_declared_secret_reaches_only_its_launched_process_then_rotation_and_removal_apply_to_the_next_launch()
 {
    let config = secret_server("search");
    let custody = Arc::new(Custody::storing("first-value"));
    let resolver: Arc<dyn McpAuthResolver> = custody.clone();

    let process = StdioChildCustody::default();
    let (connection, tools) = tokio::time::timeout(
        BOUND,
        launch(&config, Some(Arc::clone(&resolver)), &process, None),
    )
    .await
    .unwrap()
    .expect("protected launch");
    assert!(tools.iter().any(|tool| tool.name == "env_digest"));
    assert_eq!(launched_digest(&connection).await, digest("first-value"));
    // The connection keeps only the nonsecret configuration.
    assert_eq!(connection.config(), &config);
    for rendered in [
        format!("{:?}", connection.config()),
        serde_json::to_string(connection.config()).unwrap(),
    ] {
        assert!(!rendered.contains("first-value"), "{rendered}");
        assert!(rendered.contains(SLOT), "{rendered}");
    }

    // A rotation does not reach the running process; its next launch reads
    // the current value after a joined shutdown.
    custody.set(Some("rotated-value"));
    assert_eq!(launched_digest(&connection).await, digest("first-value"));
    let receipt = tokio::time::timeout(BOUND, connection.close_with_receipt())
        .await
        .unwrap()
        .expect("joined close");
    assert!(receipt.is_some(), "the stdio close is observed");
    let restarted = StdioChildCustody::default();
    let (connection, _) = launch(&config, Some(Arc::clone(&resolver)), &restarted, None)
        .await
        .expect("restart");
    assert_eq!(launched_digest(&connection).await, digest("rotated-value"));
    connection.close().await.expect("close");

    // A removal refuses the next launch before anything exists.
    custody.set(None);
    let removed = StdioChildCustody::default();
    assert!(matches!(
        launch(&config, Some(resolver), &removed, None).await,
        Err(McpError::ServiceSecretRefused {
            refusal: McpServiceSecretRefusal::NotStored { .. },
            ..
        })
    ));
    assert!(never_spawned(&removed));
}

#[tokio::test]
async fn without_custody_a_declared_server_is_never_spawned() {
    let config = secret_server("search");
    // The same server persisted through the `mcp.toml` writer and reloaded,
    // as a fresh process reads it: an ownerless file server keeps its
    // declaration, so its launch still refuses before the process.
    let temp = tempfile::tempdir().unwrap();
    let authority = meerkat_core::mcp_config::McpConfigMutationAuthority::project(
        Some(temp.path().to_path_buf()),
        None,
    );
    meerkat_core::mcp_config::McpConfig::persist_add_with_rollback(&authority, config.clone())
        .await
        .unwrap();
    let path = authority.resolved_path().unwrap();
    let reloaded = meerkat_core::mcp_config::McpConfig::load_from_paths(None, Some(&path))
        .await
        .unwrap()
        .servers;
    for config in reloaded.iter().chain([&config]) {
        for resolver in [None, Some(Arc::new(OAuthOnly) as Arc<dyn McpAuthResolver>)] {
            let process = StdioChildCustody::default();
            let refused = launch(config, resolver, &process, None).await;
            assert!(matches!(
                refused,
                Err(McpError::ServiceSecretRefused {
                    refusal: McpServiceSecretRefusal::NoCustody,
                    ..
                })
            ));
            assert!(never_spawned(&process));
            let message = refused.err().unwrap().to_string();
            assert!(message.contains("search"), "{message}");
        }
    }
    assert_eq!(reloaded, vec![config]);
}

#[tokio::test]
async fn a_held_custody_read_cancelled_or_past_its_deadline_spawns_nothing() {
    let config = secret_server("search");

    // Cancelled while the read is held.
    let custody = Arc::new(Custody::held("value"));
    let process = StdioChildCustody::default();
    let attempt = tokio::spawn({
        let (config, process) = (config.clone(), process.clone());
        let resolver: Arc<dyn McpAuthResolver> = custody.clone();
        async move { launch(&config, Some(resolver), &process, None).await }
    });
    custody.entered().await;
    attempt.abort();
    assert!(
        attempt
            .await
            .err()
            .is_some_and(|error| error.is_cancelled()),
        "the held launch was cancelled"
    );
    custody.release();
    tokio::task::yield_now().await;
    assert!(never_spawned(&process));
    assert_eq!(custody.reads.load(Ordering::SeqCst), 0);

    // Past the connect deadline while the read is held.
    let custody = Arc::new(Custody::held("value"));
    let process = StdioChildCustody::default();
    let resolver: Arc<dyn McpAuthResolver> = custody.clone();
    let expired = tokio::time::timeout(
        Duration::from_millis(200),
        launch(&config, Some(resolver), &process, None),
    )
    .await;
    assert!(expired.is_err(), "the deadline ends the held launch");
    custody.release();
    tokio::task::yield_now().await;
    assert!(never_spawned(&process));
}

#[tokio::test]
async fn an_attachment_revoked_while_custody_is_read_spawns_nothing() {
    let config = secret_server("search");
    let custody = Arc::new(Custody::held("value"));
    let control = Arc::new(Revocable {
        server: config.clone(),
        allow: AtomicBool::new(true),
        checks: AtomicUsize::new(0),
    });
    let process = StdioChildCustody::default();
    let attempt = tokio::spawn({
        let (config, process) = (config.clone(), process.clone());
        let resolver: Arc<dyn McpAuthResolver> = custody.clone();
        let control: Arc<dyn McpAttachmentAuthority> = control.clone();
        async move { launch(&config, Some(resolver), &process, Some(control)).await }
    });
    custody.entered().await;
    // The final entry check has not run while the read waits.
    assert_eq!(control.checks.load(Ordering::SeqCst), 0);
    control.allow.store(false, Ordering::SeqCst);
    custody.release();
    let refused = tokio::time::timeout(BOUND, attempt)
        .await
        .unwrap()
        .expect("attempt task");
    assert!(matches!(
        refused,
        Err(McpError::AttachmentRefused(McpAttachmentRefusal::Denied))
    ));
    assert_eq!(control.checks.load(Ordering::SeqCst), 1);
    assert!(never_spawned(&process));
}

#[tokio::test]
async fn an_invalid_declaration_or_inexact_custody_answer_is_refused_before_the_process() {
    let custody = Arc::new(Custody::storing("value"));
    let resolver: Arc<dyn McpAuthResolver> = custody.clone();
    let mut invalid = secret_server("search");
    if let McpTransportConfig::Stdio(stdio) = &mut invalid.transport {
        stdio
            .secret_env
            .insert(meerkat_core::mcp_config::MCP_CONNECTOR_BINDING_ENV.to_owned());
    }
    let process = StdioChildCustody::default();
    assert!(matches!(
        launch(&invalid, Some(Arc::clone(&resolver)), &process, None).await,
        Err(McpError::InvalidSecretEnv(_))
    ));
    assert!(never_spawned(&process));
    assert_eq!(custody.reads.load(Ordering::SeqCst), 0, "nothing was read");

    let over_answering = Arc::new(Custody {
        extra: Some("UNDECLARED"),
        ..Custody::storing("value")
    });
    let process = StdioChildCustody::default();
    assert!(matches!(
        launch(
            &secret_server("search"),
            Some(over_answering as Arc<dyn McpAuthResolver>),
            &process,
            None
        )
        .await,
        Err(McpError::ServiceSecretRefused {
            refusal: McpServiceSecretRefusal::Unavailable { .. },
            ..
        })
    ));
    assert!(never_spawned(&process));
}

#[test]
fn the_launch_configuration_carries_resolved_secrets_under_the_connector_projection() {
    let config = secret_server("search");
    let McpTransportConfig::Stdio(stdio) = &config.transport else {
        unreachable!("stdio fixture");
    };
    assert!(matches!(
        launch_configuration(stdio, &config, None),
        std::borrow::Cow::Borrowed(_)
    ));
    let mut values = McpServiceSecretValues::new();
    values.insert(SLOT, "launch-only");
    let launch = launch_configuration(stdio, &config, Some(values));
    assert_eq!(
        launch.env.get(SLOT).map(String::as_str),
        Some("launch-only")
    );
    assert!(
        !stdio.env.contains_key(SLOT),
        "the configuration is never written"
    );
}

#[tokio::test]
async fn a_refused_secret_server_leaves_its_permitted_sibling_serving() {
    let handle = Arc::new(meerkat_runtime::RuntimeExternalToolSurfaceHandle::ephemeral());
    let mut router = crate::McpRouter::new_with_surface_handle_and_stdio_profile(
        handle,
        crate::McpStdioLaunchProfile::trusted_host(),
    );
    let refused = router.add_server(secret_server("search")).await;
    assert!(
        matches!(
            refused,
            Err(McpError::ServiceSecretRefused {
                refusal: McpServiceSecretRefusal::NoCustody,
                ..
            })
        ),
        "{refused:?}"
    );
    let sibling = McpServerConfig::stdio(
        "sibling",
        mcp_test_server::fixture_binary()
            .to_string_lossy()
            .to_string(),
        Vec::new(),
        HashMap::new(),
    );
    router.add_server(sibling).await.expect("permitted sibling");
    let result = router
        .call_tool("echo", &serde_json::json!({"message": "still serving"}))
        .await;
    router.shutdown().await;
    assert_eq!(
        meerkat_core::types::text_content(&result.expect("sibling call")),
        "still serving"
    );
}
