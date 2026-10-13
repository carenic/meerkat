//! A stock open serves only the review rules it read itself: a read that
//! failed under the open's own layout is never replaced by another
//! document's rules or another open's read, and rules that changed from
//! those an earlier open of the same composition configured are never served
//! as the earlier ones. Either way the open's governed composition is
//! unavailable. Concurrent opens of one composition each compose from their
//! own read, whichever order their store staging completes in, and an open
//! cancelled in its staging leaves no read behind.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::governed_realm::stock::{StockGovernance, StockPrincipals, govern_stock_realm};
use meerkat_core::connection::RealmId;

const REALM: &str = "review-load";

/// Materialize the realm under `root` and govern it as the local operator.
async fn governed_realm(root: &std::path::Path) -> (RealmId, StockPrincipals) {
    drop(
        open_governed_realm_persistence_with_provider(
            &crate::DiskStorageProvider,
            root,
            REALM,
            Some(RealmBackend::Sqlite),
            None,
            None,
            RealmGovernance::Ungoverned,
        )
        .await
        .unwrap(),
    );
    let realm = RealmId::parse(REALM).unwrap();
    let principals = StockPrincipals::for_local_operator(&realm).unwrap();
    govern_stock_realm(root, REALM, &principals).await.unwrap();
    (realm, principals)
}

async fn open_stock(
    root: &std::path::Path,
    layout: Option<meerkat_core::StorageLayout>,
    stock: &Arc<StockGovernance>,
) -> PersistenceBundle {
    open_through(&crate::DiskStorageProvider, root, layout, stock).await
}

async fn open_through(
    provider: &dyn crate::storage_provider::RealmStorageProvider,
    root: &std::path::Path,
    layout: Option<meerkat_core::StorageLayout>,
    stock: &Arc<StockGovernance>,
) -> PersistenceBundle {
    open_governed_realm_persistence_with_provider(
        provider,
        root,
        REALM,
        Some(RealmBackend::Sqlite),
        None,
        layout,
        RealmGovernance::Stock(Arc::clone(stock)),
    )
    .await
    .unwrap()
    .1
}

/// The disk provider, holding each open in its store staging until
/// released: the open has read its review rules and has not composed yet.
#[derive(Default)]
struct HeldStaging {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::storage_provider::RealmStorageProvider for HeldStaging {
    fn name(&self) -> &'static str {
        "disk"
    }

    async fn open(
        &self,
        ctx: &crate::storage_provider::RealmOpenContext,
    ) -> Result<crate::storage_provider::RealmStoreSet, PersistenceError> {
        self.entered.notify_one();
        self.release.notified().await;
        crate::storage_provider::RealmStorageProvider::open(&crate::DiskStorageProvider, ctx).await
    }
}

/// Drive `open` until `held` holds it in its store staging.
async fn until_staged(
    open: &mut std::pin::Pin<Box<impl std::future::Future<Output = PersistenceBundle>>>,
    held: &HeldStaging,
) {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        tokio::select! {
            () = held.entered.notified() => {}
            _ = open.as_mut() => panic!("the open finished before its store staging"),
        }
    })
    .await
    .expect("the open reaches its store staging");
}

/// A layout for the realm under `root` whose global document (the layout's
/// `<user config root>/.rkat/config.toml`, under `base`) carries a malformed
/// governance section that does not parse. Every realm's effective
/// configuration reads the global document, so the read under this layout
/// fails, while the explicit-root layout's read of the realm does not.
fn malformed_layout(base: &std::path::Path, root: &std::path::Path) -> meerkat_core::StorageLayout {
    let user = base.join("user");
    std::fs::create_dir_all(user.join(".rkat")).unwrap();
    std::fs::write(
        user.join(".rkat").join("config.toml"),
        "[[governance.review.rules]]\n\
         target = { kind = \"mcp_server\", server = \"mail\" }\n\
         tier = r3\n",
    )
    .unwrap();
    meerkat_core::StorageLayout::resolve(
        meerkat_core::StorageLayoutInputs {
            invocation_context: base.to_path_buf(),
            user_config_root: Some(user),
            ..meerkat_core::StorageLayoutInputs::default()
        },
        &meerkat_core::RealmConfig {
            selection: meerkat_core::RealmSelection::Explicit {
                realm_id: REALM.to_string(),
            },
            state_root: Some(root.to_path_buf()),
            ..meerkat_core::RealmConfig::default()
        },
    )
    .unwrap()
    .layout
}

fn rule(server: &str) -> String {
    format!(
        "[[governance.review.rules]]\n\
         target = {{ kind = \"mcp_server\", server = \"{server}\" }}\n\
         tier = \"r3\"\n\
         consent_assurance = \"device_trusted\"\n"
    )
}

/// The open's own layout names a global document whose governance section
/// is malformed, while the explicit-root layout a composition could derive
/// reads the realm's configuration fine: the open's composition is
/// unavailable, never composed from that other document.
#[tokio::test]
async fn a_failed_rule_read_under_the_opens_layout_is_never_served_from_another_document() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let layout = malformed_layout(temp.path(), &root);
    // The premise: this layout's read fails, the explicit-root one's does not.
    assert!(
        stock::review::read_realm_review_rules(&root, &realm, &layout)
            .await
            .is_err()
    );
    let ambient = crate::persistence::layout_for_explicit_state_root(&root, REALM).unwrap();
    assert!(
        stock::review::read_realm_review_rules(&root, &realm, &ambient)
            .await
            .is_ok()
    );

    let stock = StockGovernance::new(realm, principals);
    let bundle = open_stock(&root, Some(layout), &stock).await;
    assert!(
        stock.configured_mcp_policy().is_none(),
        "the open's failed read makes its composition unavailable"
    );
    drop(bundle);
}

/// An earlier open of the same composition configured rules A. The trusted
/// host then changes the realm's rules to B: the next open of that
/// composition is unavailable rather than serving A. With the rules back at
/// A, an open composes again.
#[tokio::test]
async fn changed_rules_are_never_served_as_a_retained_compositions_earlier_rules() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let document = meerkat_store::realm_paths_in(&root, REALM).config_path;
    let original = std::fs::read_to_string(&document).unwrap_or_default();
    let write_rules = |server: &str| {
        std::fs::write(&document, format!("{original}\n{}", rule(server))).unwrap();
    };
    let stock = StockGovernance::new(realm, principals);

    write_rules("mail");
    let bundle = open_stock(&root, None, &stock).await;
    assert!(
        stock.configured_mcp_policy().is_some(),
        "the first open composes with the rules it read"
    );
    drop(bundle);
    assert!(
        stock.configured_mcp_policy().is_none(),
        "the closed open's composition is gone"
    );

    write_rules("calendar");
    let bundle = open_stock(&root, None, &stock).await;
    assert!(
        stock.configured_mcp_policy().is_none(),
        "changed rules make the open unavailable, never the earlier rules"
    );
    drop(bundle);

    write_rules("mail");
    let bundle = open_stock(&root, None, &stock).await;
    assert!(
        stock.configured_mcp_policy().is_some(),
        "the same rules compose again"
    );
    drop(bundle);
}

/// Open A, whose own layout's read fails, is held in its store staging
/// while open B of the same composition reads a valid layout and is held in
/// turn. A, released first, still refuses: it never composes with B's read.
/// B, released after A closed, composes with its own.
#[tokio::test]
async fn a_failed_read_held_in_staging_never_composes_with_another_opens_read() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let malformed = malformed_layout(temp.path(), &root);
    let stock = StockGovernance::new(realm, principals);
    let (held_a, held_b) = (HeldStaging::default(), HeldStaging::default());

    let mut a = Box::pin(open_through(&held_a, &root, Some(malformed), &stock));
    until_staged(&mut a, &held_a).await;
    let mut b = Box::pin(open_through(&held_b, &root, None, &stock));
    until_staged(&mut b, &held_b).await;

    held_a.release.notify_one();
    let bundle = a.await;
    assert!(
        stock.configured_mcp_policy().is_none(),
        "the open whose own read failed is unavailable"
    );
    drop(bundle);

    held_b.release.notify_one();
    let bundle = b.await;
    assert!(
        stock.configured_mcp_policy().is_some(),
        "the open whose own read succeeded composes"
    );
    drop(bundle);
}

/// The reverse order: open B, whose own read succeeded, is held in its
/// store staging while open A of the same composition fails its read and is
/// held in turn. B, released first, composes with its own read and is never
/// refused for A's; A, released after B closed, is unavailable.
#[tokio::test]
async fn a_valid_read_held_in_staging_is_never_refused_for_another_opens_failure() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let malformed = malformed_layout(temp.path(), &root);
    let stock = StockGovernance::new(realm, principals);
    let (held_a, held_b) = (HeldStaging::default(), HeldStaging::default());

    let mut b = Box::pin(open_through(&held_b, &root, None, &stock));
    until_staged(&mut b, &held_b).await;
    let mut a = Box::pin(open_through(&held_a, &root, Some(malformed), &stock));
    until_staged(&mut a, &held_a).await;

    held_b.release.notify_one();
    let bundle = b.await;
    assert!(
        stock.configured_mcp_policy().is_some(),
        "the open whose own read succeeded composes"
    );
    drop(bundle);

    held_a.release.notify_one();
    let bundle = a.await;
    assert!(
        stock.configured_mcp_policy().is_none(),
        "the open whose own read failed is unavailable"
    );
    drop(bundle);
}

/// An open cancelled in its store staging leaves no read behind: open A
/// fails its read and is dropped while held, and open B of the same
/// composition, held across that, composes with its own read.
#[tokio::test]
async fn an_open_cancelled_in_staging_leaves_no_read_behind() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let malformed = malformed_layout(temp.path(), &root);
    let stock = StockGovernance::new(realm, principals);
    let (held_a, held_b) = (HeldStaging::default(), HeldStaging::default());

    let mut b = Box::pin(open_through(&held_b, &root, None, &stock));
    until_staged(&mut b, &held_b).await;
    let mut a = Box::pin(open_through(&held_a, &root, Some(malformed), &stock));
    until_staged(&mut a, &held_a).await;
    drop(a);

    held_b.release.notify_one();
    let bundle = b.await;
    assert!(
        stock.configured_mcp_policy().is_some(),
        "the cancelled open's failed read is not another open's"
    );
    drop(bundle);
}
