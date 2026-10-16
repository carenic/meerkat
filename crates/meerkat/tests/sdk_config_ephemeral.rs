#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use meerkat::{ConfigDelta, SdkConfigStore};
use serde_json::json;

#[tokio::test]
async fn test_sdk_config_ephemeral() {
    let store = SdkConfigStore::new();
    let baseline = store.get().await.expect("get config");

    let patched = store
        .patch(ConfigDelta(json!({
            "agent": { "model": "ephemeral-model" }
        })))
        .await
        .expect("patch config");

    assert_eq!(patched.agent.model, "ephemeral-model");

    let fresh_store = SdkConfigStore::new();
    let fresh = fresh_store.get().await.expect("get config");

    assert_eq!(
        fresh.agent.model, baseline.agent.model,
        "ephemeral override should not persist across stores"
    );
}

/// The documents a realm document store composes with, by realm: none, or a
/// realm's own.
#[derive(Default)]
struct Documents(std::collections::BTreeMap<String, meerkat::Config>);

#[async_trait::async_trait]
impl meerkat_core::RealmConfigSource for Documents {
    async fn config_for_realm(
        &self,
        realm: &meerkat_core::connection::RealmId,
    ) -> Result<Option<meerkat::Config>, meerkat::ConfigError> {
        Ok(self.0.get(realm.as_str()).cloned())
    }
}

/// The store for the user document (the `global` realm's own) keeps what
/// other realms compose of it. A realm without a section of its own takes
/// the section the user document states for it, so removing a
/// `[realm.<id>]` section whose parent is `global` from a user document that
/// states `[governance]` would drop the governance that realm composes: the
/// write is refused, naming the section and the realm, and nothing is
/// written. A realm whose own document states its own section decides its
/// own chain, so the same edit is then ordinary config.
#[tokio::test]
async fn a_user_document_write_cannot_drop_a_stand_in_section_a_realm_composes_through() {
    const USER: &str = "[realm.team]\nparent = \"global\"\n\n\
                        [[governance.review.rules]]\n\
                        target = { kind = \"mcp_server\", server = \"mail\" }\n\
                        tier = \"r2\"\n";
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    std::fs::write(&path, USER).unwrap();
    let store = |documents: Documents| {
        SdkConfigStore::with_realm_document(
            &path,
            meerkat_core::connection::RealmId::global(),
            std::sync::Arc::new(documents),
        )
    };
    let remove_team = || ConfigDelta(json!({"realm": {"team": null}}));

    let refused = store(Documents::default())
        .patch(remove_team())
        .await
        .expect_err("the realm without a document would drop the user's governance");
    let message = refused.to_string();
    assert!(
        refused.administered_section() == Some(meerkat_core::AdministeredSection::Governance)
            && message.contains("removing [realm.team]")
            && message.contains("realm 'team'"),
        "{message}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), USER);

    let mut own = meerkat::Config::default();
    own.realm.insert(
        "team".to_string(),
        meerkat_core::RealmConfigSection::default(),
    );
    store(Documents(std::collections::BTreeMap::from([(
        "team".to_string(),
        own,
    )])))
    .patch(remove_team())
    .await
    .expect("a realm stating its own section does not compose the user's section for it");
}
