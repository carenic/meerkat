//! The under-custody rechecks of a host's preparation: a realm whose posture
//! no longer reads governed once the host holds its custody, and a
//! composition another preparation composed while this one waited for the
//! custody, are refused; nothing is composed, and the custody is released.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use meerkat_core::OperationAuthorizationError;
use meerkat_store::RealmBackend;

use super::*;
use crate::governed_realm::stock::caller::{
    StockCallerAccess, StockCallerRequest, StockLoginLifetime,
};
use crate::governed_realm::stock::issuance::DEFAULT_STOCK_AGENT;
use crate::governed_realm::stock::{StockPrincipals, govern_stock_realm};

const REALM: &str = "stock-host-recheck";

/// A step armed for the preparations under one realms root.
type Hook = (PathBuf, Box<dyn FnOnce() + Send>);

/// The step armed to run once the host holds the realm's custody.
static AFTER_CUSTODY: Mutex<Option<Hook>> = Mutex::new(None);

/// The step armed to run after the early checks, before any custody.
static BEFORE_CUSTODY: Mutex<Option<Hook>> = Mutex::new(None);

/// Run the step armed in `slot` for `realms_root`, once.
fn run_armed(slot: &Mutex<Option<Hook>>, realms_root: &Path) {
    let hook = slot.lock().ok().and_then(|mut slot| {
        slot.take_if(|(root, _)| root.as_path() == realms_root)
            .map(|(_, hook)| hook)
    });
    if let Some(hook) = hook {
        hook();
    }
}

/// Run the armed step, once, under the custody the host now holds.
pub(super) fn after_custody(realms_root: &Path) {
    run_armed(&AFTER_CUSTODY, realms_root);
}

/// Run the armed step, once, before the host takes any custody.
pub(super) fn before_custody(realms_root: &Path) {
    run_armed(&BEFORE_CUSTODY, realms_root);
}

struct AdmitAll;

impl StockCallerAccess for AdmitAll {
    fn authorize_caller(
        &self,
        _request: &StockCallerRequest<'_>,
    ) -> Result<StockLoginLifetime, OperationAuthorizationError> {
        Ok(StockLoginLifetime::UntilRetired)
    }
}

fn host_stores(dir: &Path) -> RealmStoreSet {
    std::fs::create_dir_all(dir).unwrap();
    let durable = |domain: &str| {
        meerkat_core::DurabilityDeclaration::durable(
            domain,
            meerkat_core::DurabilityResolution::Persistent,
        )
    };
    RealmStoreSet {
        session_store: Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.join("sessions.sqlite3")).unwrap(),
        ),
        runtime_store: Arc::new(
            meerkat_runtime::store::SqliteRuntimeStore::new_whole_blob(dir.join("runtime.db"))
                .unwrap(),
        ),
        schedule_store: Arc::new(meerkat_schedule::DisabledScheduleStore),
        workgraph_store: Arc::new(meerkat_workgraph::DisabledWorkGraphStore),
        job_store: Arc::new(
            meerkat_jobs::SqliteDetachedJobStore::open(dir.join("jobs.sqlite3")).unwrap(),
        ),
        blob_store: Arc::new(meerkat_store::FsBlobStore::new(dir.join("blobs"))),
        artifact_store: Arc::new(meerkat_store::FsArtifactStore::new(dir.join("artifacts"))),
        store_path: dir.to_path_buf(),
        projection_root: None,
        durability: [
            "sessions",
            "runtime",
            "schedule",
            "workgraph",
            "jobs",
            "blobs",
            "artifacts",
        ]
        .iter()
        .map(|domain| durable(domain))
        .collect(),
    }
}

/// Materialize the realm under `root` and govern it as the local operator.
async fn governed_realm(root: &Path) -> (RealmId, StockPrincipals) {
    drop(
        super::super::open_governed_realm_persistence_with_provider(
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

#[tokio::test]
async fn a_posture_lost_under_the_custody_refuses_and_releases_it() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let paths = meerkat_store::realm_paths_in(&root, REALM);

    // Once the host holds the custody, the manifest reads ungoverned.
    let manifest_path = paths.manifest_path.clone();
    *AFTER_CUSTODY.lock().unwrap() = Some((
        root.clone(),
        Box::new(move || {
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
            let fields = manifest.as_object_mut().unwrap();
            fields.remove("governance");
            fields.insert("manifest_format".to_string(), serde_json::json!(1));
            std::fs::write(
                &manifest_path,
                serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
        }),
    ));
    let stock = StockGovernance::host_authenticated_for_agent(
        realm,
        principals,
        DEFAULT_STOCK_AGENT,
        Arc::new(AdmitAll),
    )
    .unwrap();
    let refused = prepare_stock_host_composition(
        StockHostCustody::new(&root, REALM).unwrap(),
        RealmGovernance::Stock(Arc::clone(&stock)),
        host_stores(&temp.path().join("host")),
    )
    .await
    .expect_err("the posture is reread under the custody");
    assert!(
        matches!(refused, StockHostError::NotGoverned { .. }),
        "{refused:?}"
    );
    assert!(!stock.is_composed(), "no governed owner was read");
    drop(
        meerkat_runtime::store::SqliteRuntimeStore::try_acquire_governed_execution_custody(
            &paths.sessions_sqlite_path,
        )
        .expect("the refused host released the realm's custody"),
    );
}

/// A preparation that passed its early checks and then waited while another
/// preparation of the same composition composed it and released the realm is
/// refused under the custody it then takes: nothing is composed again, and
/// the custody is released.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_preparation_that_waits_past_another_composition_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("realms");
    let (realm, principals) = governed_realm(&root).await;
    let stock = StockGovernance::host_authenticated_for_agent(
        realm,
        principals,
        DEFAULT_STOCK_AGENT,
        Arc::new(AdmitAll),
    )
    .unwrap();

    let (reached, reached_rx) = std::sync::mpsc::channel::<()>();
    let (resume, resume_rx) = std::sync::mpsc::channel::<()>();
    *BEFORE_CUSTODY.lock().unwrap() = Some((
        root.clone(),
        Box::new(move || {
            reached.send(()).unwrap();
            resume_rx.recv().unwrap();
        }),
    ));
    let delayed = tokio::spawn(prepare_stock_host_composition(
        StockHostCustody::new(&root, REALM).unwrap(),
        RealmGovernance::Stock(Arc::clone(&stock)),
        host_stores(&temp.path().join("delayed-host")),
    ));
    tokio::task::spawn_blocking(move || reached_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        !stock.is_composed(),
        "the delayed preparation passed its early checks"
    );

    drop(
        prepare_stock_host_composition(
            StockHostCustody::new(&root, REALM).unwrap(),
            RealmGovernance::Stock(Arc::clone(&stock)),
            host_stores(&temp.path().join("host")),
        )
        .await
        .expect("the other preparation composes and releases the realm"),
    );
    assert!(stock.is_composed());

    resume.send(()).unwrap();
    let refused = delayed
        .await
        .unwrap()
        .expect_err("the delayed preparation is refused under the custody");
    assert!(
        matches!(refused, StockHostError::CompositionReused { .. }),
        "{refused:?}"
    );
    drop(
        meerkat_runtime::store::SqliteRuntimeStore::try_acquire_governed_execution_custody(
            &meerkat_store::realm_paths_in(&root, REALM).sessions_sqlite_path,
        )
        .expect("the refused preparation released the realm's custody"),
    );
}
