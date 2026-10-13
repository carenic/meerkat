//! A stock grant for a connector-bound installation covers the exact account
//! binding installed when it was issued. Replacing the installation with
//! another account (A to B) leaves the old grant refusing the new binding
//! until the operator grants it explicitly; reinstalling the same binding
//! keeps the grant.
#![cfg(all(
    feature = "session-store",
    feature = "local-authorization",
    feature = "mcp"
))]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use meerkat::DiskStorageProvider;
use meerkat::governed_realm::stock::issuance::{
    McpOperations, StockGrantRequest, StockGrantTarget, grant_stock_agent,
};
use meerkat::governed_realm::stock::{StockGovernance, StockPrincipals, govern_stock_realm};
use meerkat::governed_realm::{RealmGovernance, open_governed_realm_persistence_with_provider};
use meerkat_core::SessionId;
use meerkat_core::connection::RealmId;
use meerkat_core::mcp_attachment::McpAttachmentRefusal;
use meerkat_core::mcp_config::{McpConnectorBinding, McpServerConfig};
use meerkat_store::{RealmBackend, RealmPaths};

const SERVER: &str = "records";

fn realm_paths(root: &Path) -> RealmPaths {
    meerkat_store::realm_paths_in(root, "governed")
}

async fn open(root: &Path, governance: RealmGovernance) -> meerkat::PersistenceBundle {
    open_governed_realm_persistence_with_provider(
        &DiskStorageProvider,
        root,
        "governed",
        Some(RealmBackend::Sqlite),
        None,
        None,
        governance,
    )
    .await
    .expect("the realm opens in every posture")
    .1
}

fn realm_config(paths: &RealmPaths) -> meerkat_core::ConfigRuntime {
    meerkat_core::ConfigRuntime::new(
        Arc::new(meerkat_core::FileConfigStore::new(
            paths.config_path.clone(),
            meerkat_models::canonical(),
        )),
        paths.config_path.with_file_name("config_state.json"),
    )
}

async fn install(paths: &RealmPaths, server: McpServerConfig) {
    let runtime = realm_config(paths);
    meerkat_core::McpRealmPersistTarget::new(&runtime)
        .add(server, None)
        .await
        .expect("realm installation");
}

/// Replace the installation as an operator does: remove, then add.
async fn replace(paths: &RealmPaths, server: McpServerConfig) {
    let runtime = realm_config(paths);
    meerkat_core::McpRealmPersistTarget::new(&runtime)
        .remove(&server.name, None)
        .await
        .expect("realm removal");
    install(paths, server).await;
}

/// The fixture server bound to `account`, with a ceiling of `echo`.
fn records(account: &str) -> McpServerConfig {
    let mut server = McpServerConfig::stdio(
        SERVER,
        mcp_test_server::fixture_binary()
            .to_string_lossy()
            .to_string(),
        Vec::new(),
        HashMap::new(),
    );
    server.access = Some(meerkat_core::McpServerAccess::allowing(["echo"]));
    server.connector = Some(McpConnectorBinding {
        realm_id: "governed".into(),
        slot_id: SERVER.into(),
        issuer: "https://issuer.example".into(),
        client: "client".into(),
        resource: "https://api.example".into(),
        strategy_id: "oidc".into(),
        account: account.into(),
        operations: std::collections::BTreeMap::new(),
    });
    server
}

async fn govern_as_operator(root: &Path) -> (RealmId, StockPrincipals) {
    let realm = RealmId::parse("governed").unwrap();
    let principals = StockPrincipals::for_user(&realm, 4242).unwrap();
    govern_stock_realm(root, "governed", &principals)
        .await
        .expect("governed");
    (realm, principals)
}

async fn grant(root: &Path, principals: &StockPrincipals, target: StockGrantTarget) {
    grant_stock_agent(
        root,
        "governed",
        principals,
        &StockGrantRequest {
            agent: "agent-a".to_string(),
            target,
            expires_in_ms: None,
        },
    )
    .await
    .expect("issued and selected");
}

async fn grant_records(root: &Path, principals: &StockPrincipals) {
    grant(
        root,
        principals,
        StockGrantTarget::Mcp {
            server: SERVER.to_string(),
            operations: McpOperations::Exact(["echo".to_string()].into()),
        },
    )
    .await;
}

/// Open a fresh realm snapshot as agent-a, bind a new session to the agent
/// and decide `server` as a configured attachment.
async fn bound_verdict(
    root: &Path,
    realm: &RealmId,
    principals: &StockPrincipals,
    server: &McpServerConfig,
) -> Result<(), McpAttachmentRefusal> {
    let stock = StockGovernance::new_for_agent(realm.clone(), principals.clone(), "agent-a")
        .expect("agent principal");
    let bundle = open(root, RealmGovernance::Stock(Arc::clone(&stock))).await;
    let machine = bundle.runtime_adapter();
    let session = SessionId::new();
    machine
        .register_session(session.clone())
        .await
        .expect("registration");
    let bindings = machine
        .prepare_bindings(session.clone())
        .await
        .expect("bindings");
    let authority = bindings
        .configured_mcp_attachment()
        .cloned()
        .expect("the governed host decides configured servers");
    stock
        .bind_session_agent(&machine, &session, [server.name.as_str()])
        .await
        .expect("bound");
    let verdict = authority.authorize_configured(server);
    drop(bindings);
    drop(bundle);
    drop(stock);
    verdict
}

/// Account A is installed and granted; the operator replaces it with
/// account B. The grant issued for A refuses B (a different connector
/// approval, not a missing record), and A's descriptor is no longer the
/// installation. An explicit regrant covers B; reinstalling the identical
/// B binding (an identical-binding reinstall) keeps that grant.
#[tokio::test]
async fn replacing_the_account_requires_an_explicit_regrant() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let paths = realm_paths(root);
    drop(open(root, RealmGovernance::Ungoverned).await);
    install(&paths, records("account-a")).await;
    let (realm, principals) = govern_as_operator(root).await;
    grant(
        root,
        &principals,
        StockGrantTarget::Controller {
            routes: [meerkat_core::ControllerRouteKey::parse(&format!(
                "{}{}",
                meerkat_core::ControllerRouteKey::PREFIX,
                "0".repeat(64)
            ))
            .unwrap()]
            .into(),
        },
    )
    .await;
    grant_records(root, &principals).await;

    let (a, b) = (records("account-a"), records("account-b"));
    assert_eq!(
        bound_verdict(root, &realm, &principals, &a).await,
        Ok(()),
        "the grant covers the account it was issued for"
    );

    replace(&paths, b.clone()).await;
    assert_eq!(
        bound_verdict(root, &realm, &principals, &b).await,
        Err(McpAttachmentRefusal::Denied),
        "a grant issued for account A never covers account B"
    );
    assert_eq!(
        bound_verdict(root, &realm, &principals, &a).await,
        Err(McpAttachmentRefusal::Unavailable),
        "account A is no longer the installation"
    );

    grant_records(root, &principals).await;
    assert_eq!(
        bound_verdict(root, &realm, &principals, &b).await,
        Ok(()),
        "the explicit regrant covers account B"
    );
    assert_ne!(
        bound_verdict(root, &realm, &principals, &a).await,
        Ok(()),
        "the regrant does not reopen account A"
    );

    replace(&paths, records("account-b")).await;
    assert_eq!(
        bound_verdict(root, &realm, &principals, &b).await,
        Ok(()),
        "reinstalling the identical binding keeps the grant"
    );
}
