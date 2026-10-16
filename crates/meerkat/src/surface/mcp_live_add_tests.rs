//! The shared live MCP add: governed runtimes need a prepared control, and
//! persistence is decided by that control before any configuration write.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use meerkat_core::mcp_attachment::{McpAttachmentAuthority, McpAttachmentRefusal};
use meerkat_core::mcp_config::{McpConfigMutationAuthority, McpScope, McpServerConfig};

use super::{McpLiveAddError, stage_mcp_live_add};

struct Control {
    server: McpServerConfig,
    persist: Result<(), McpAttachmentRefusal>,
}

impl McpAttachmentAuthority for Control {
    fn server(&self) -> &McpServerConfig {
        &self.server
    }
    fn authorize_apply(&self) -> Result<(), McpAttachmentRefusal> {
        Ok(())
    }
    fn authorize_persist(&self, scope: McpScope) -> Result<(), McpAttachmentRefusal> {
        assert_eq!(scope, McpScope::Project);
        self.persist.clone()
    }
}

fn server() -> McpServerConfig {
    McpServerConfig::stdio(
        "attached",
        "/nonexistent/mcp-server".to_string(),
        vec![],
        HashMap::new(),
    )
}

fn control(persist: Result<(), McpAttachmentRefusal>) -> Arc<dyn McpAttachmentAuthority> {
    Arc::new(Control {
        server: server(),
        persist,
    })
}

fn adapter() -> meerkat_mcp::McpRouterAdapter {
    meerkat_mcp::McpRouterAdapter::new(
        meerkat_mcp::McpRouter::new_with_surface_handle_and_stdio_profile(
            Arc::new(meerkat_runtime::RuntimeExternalToolSurfaceHandle::ephemeral()),
            meerkat_mcp::McpStdioLaunchProfile::trusted_host(),
        ),
    )
}

fn project(dir: &tempfile::TempDir) -> McpConfigMutationAuthority {
    McpConfigMutationAuthority::project(Some(dir.path().to_path_buf()), None)
}

fn config_written(dir: &tempfile::TempDir) -> bool {
    dir.path().join(".rkat").join("mcp.toml").exists()
}

#[tokio::test]
async fn a_governed_runtime_refuses_an_add_without_a_control() {
    let dir = tempfile::tempdir().unwrap();
    let result = stage_mcp_live_add(&adapter(), true, None, &project(&dir), server(), true).await;
    assert!(matches!(
        result,
        Err(McpLiveAddError::GovernedRequiresControl)
    ));
    assert!(!config_written(&dir), "nothing is written before authority");
}

#[tokio::test]
async fn a_control_proves_nothing_on_an_ungoverned_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let result = stage_mcp_live_add(
        &adapter(),
        false,
        Some(control(Ok(()))),
        &project(&dir),
        server(),
        false,
    )
    .await;
    assert!(matches!(
        result,
        Err(McpLiveAddError::Refused(McpAttachmentRefusal::Unavailable))
    ));
}

#[tokio::test]
async fn a_refused_persist_writes_nothing_and_stages_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter();
    let result = stage_mcp_live_add(
        &adapter,
        true,
        Some(control(Err(McpAttachmentRefusal::Denied))),
        &project(&dir),
        server(),
        true,
    )
    .await;
    assert!(matches!(
        result,
        Err(McpLiveAddError::Refused(McpAttachmentRefusal::Denied))
    ));
    assert!(!config_written(&dir));
    let applied = adapter.apply_staged().await.unwrap();
    assert!(applied.delta.lifecycle_actions.is_empty());
}

#[tokio::test]
async fn an_authorized_persist_and_stage_report_both_effects() {
    let dir = tempfile::tempdir().unwrap();
    let outcome = stage_mcp_live_add(
        &adapter(),
        true,
        Some(control(Ok(()))),
        &project(&dir),
        server(),
        true,
    )
    .await
    .expect("authorized governed add");
    assert!(outcome.persisted && outcome.staged);
    assert!(config_written(&dir));
}

#[tokio::test]
async fn a_control_for_another_descriptor_is_refused_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut other = server();
    other.name = "other".into();
    let mismatched: Arc<dyn McpAttachmentAuthority> = Arc::new(Control {
        server: other,
        persist: Ok(()),
    });
    let result = stage_mcp_live_add(
        &adapter(),
        true,
        Some(mismatched),
        &project(&dir),
        server(),
        true,
    )
    .await;
    assert!(matches!(
        result,
        Err(McpLiveAddError::Refused(McpAttachmentRefusal::Mismatch))
    ));
    assert!(!config_written(&dir));
}

mod restore_identity {
    //! Restore re-stages a recorded request only when its identity proves the
    //! current installation is the accepted one: credential-bearing parts are
    //! compared through the realm's protected identity key.

    use std::collections::HashMap;
    use std::sync::Arc;

    use meerkat_core::auth::{
        McpAttachmentIdentityKey, ProviderAuthPersistence, TokenKey, TokenStore,
        load_mcp_attachment_identity_key,
    };
    use meerkat_core::mcp_config::McpServerConfig;
    use meerkat_core::{
        McpAttachmentDescriptorIdentity, McpAttachmentIntentDisposition, McpAttachmentTarget,
        McpSessionAttachmentIntent, RealmId,
    };

    use super::super::{
        McpAttachmentRestoreDisposition, McpAttachmentRestoreRefusal,
        accepted_mcp_attachment_identity, restore_refusal,
    };

    fn launcher(args: &[&str]) -> McpServerConfig {
        McpServerConfig::stdio(
            "docs",
            "npx".to_string(),
            args.iter().map(ToString::to_string).collect(),
            HashMap::new(),
        )
    }

    fn intent(descriptor: McpAttachmentDescriptorIdentity) -> McpSessionAttachmentIntent {
        McpSessionAttachmentIntent {
            descriptor,
            disposition: McpAttachmentIntentDisposition::Requested,
            revision: meerkat_core::MCP_ATTACHMENT_REVISION_BASE + 1,
        }
    }

    fn decide(
        recorded: &McpSessionAttachmentIntent,
        current: McpServerConfig,
        key: Option<&McpAttachmentIdentityKey>,
    ) -> Result<(), McpAttachmentRestoreDisposition> {
        restore_refusal(recorded, &move |_: &str| Some(current.clone()), key).map(|_| ())
    }

    fn persistence() -> (ProviderAuthPersistence, Arc<dyn TokenStore>) {
        let store: Arc<dyn TokenStore> =
            Arc::new(meerkat_providers::auth_store::EphemeralTokenStore::new());
        (
            ProviderAuthPersistence::new(
                Arc::clone(&store),
                Arc::new(meerkat_providers::auth_store::InMemoryCoordinator::new()),
            ),
            store,
        )
    }

    #[tokio::test]
    async fn an_unchanged_launcher_restores_and_a_changed_one_is_rebound() {
        let (persistence, store) = persistence();
        let realm = RealmId::parse("home").unwrap();
        let accepted = launcher(&["-y", "@acme/docs-mcp", "--token", "arg-secret"]);
        let recorded = intent(
            accepted_mcp_attachment_identity(&persistence, &realm, &accepted)
                .await
                .unwrap(),
        );
        assert!(recorded.descriptor.target.is_exact());
        assert!(
            !serde_json::to_string(&recorded)
                .unwrap()
                .contains("arg-secret")
        );

        // The key was persisted before the identity was returned; a restarted
        // host loads the same key and the same installation restores.
        let key = load_mcp_attachment_identity_key(store.as_ref(), &realm)
            .await
            .unwrap()
            .expect("the key is durable");
        assert_eq!(
            recorded
                .descriptor
                .target
                .binding()
                .map(|b| b.key_id.as_str()),
            Some(key.id())
        );
        assert_eq!(decide(&recorded, accepted.clone(), Some(&key)), Ok(()));
        let again = accepted_mcp_attachment_identity(&persistence, &realm, &accepted)
            .await
            .unwrap();
        assert_eq!(again, recorded.descriptor, "one key per realm, reused");

        // The same name launching a different package is not the accepted
        // target.
        assert_eq!(
            decide(
                &recorded,
                launcher(&["-y", "@other/mcp", "--token", "arg-secret"]),
                Some(&key)
            ),
            Err(McpAttachmentRestoreDisposition::Refused(
                McpAttachmentRestoreRefusal::Rebound
            ))
        );

        // The key is host key material, never a credential.
        let keys = store.list().await.unwrap();
        let stored = keys
            .iter()
            .find(|candidate| candidate.is_mcp_attachment_identity_key())
            .expect("the key is listed under its own kind");
        assert_eq!(stored.credential_identity(), None);
        assert_eq!(*stored, TokenKey::mcp_attachment_identity_key(realm));
    }

    #[test]
    fn a_lost_key_or_unprovable_record_needs_renewal_and_never_binds_current_config() {
        let key = McpAttachmentIdentityKey::generate();
        let accepted = launcher(&["-y", "@acme/docs-mcp"]);
        let recorded = intent(McpAttachmentDescriptorIdentity::of(&accepted, Some(&key)));
        let renewal = Err(McpAttachmentRestoreDisposition::Refused(
            McpAttachmentRestoreRefusal::RenewalRequired,
        ));
        // The key is gone, or the host has a different one.
        assert_eq!(decide(&recorded, accepted.clone(), None), renewal);
        assert_eq!(
            decide(
                &recorded,
                accepted.clone(),
                Some(&McpAttachmentIdentityKey::generate())
            ),
            renewal
        );
        // Recorded without a key, or before this identity format.
        let unbound = intent(McpAttachmentDescriptorIdentity::of(&accepted, None));
        assert_eq!(decide(&unbound, accepted.clone(), Some(&key)), renewal);
        let mut predecessor = recorded;
        predecessor.descriptor.target = McpAttachmentTarget::Predecessor;
        assert_eq!(decide(&predecessor, accepted, Some(&key)), renewal);
        // A URL with a query restores under its key; a changed query does not.
        let url = |query: &str| {
            McpServerConfig::streamable_http(
                "docs",
                format!("https://docs.example/mcp?{query}"),
                HashMap::new(),
            )
        };
        let recorded = intent(McpAttachmentDescriptorIdentity::of(
            &url("tenant=a"),
            Some(&key),
        ));
        assert_eq!(decide(&recorded, url("tenant=a"), Some(&key)), Ok(()));
        assert_eq!(
            decide(&recorded, url("tenant=b"), Some(&key)),
            Err(McpAttachmentRestoreDisposition::Refused(
                McpAttachmentRestoreRefusal::Rebound
            ))
        );
    }
}
