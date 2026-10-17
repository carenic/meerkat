//! Guarded config writes over one state root are serialized: a store
//! declared with its realm chain holds the state root's composition lock
//! from before it reads its own document, through the composition check, to
//! after its write. A writer whose check reads another document can then
//! never check against a state another writer is about to replace, so two
//! writes that each pass alone cannot combine into a composition neither
//! checked.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::config::{ConfigDelta, ConfigError, ConfigWarning};
use meerkat_core::connection::RealmId;
use meerkat_core::{
    Config, ConfigStore, EffectiveConfigReader, FileConfigStore, RealmConfigSource,
};
use meerkat_store::FilesystemRealmConfigSource;
use tokio::sync::Notify;

const REVIEW_RULE: &str = "[[governance.review.rules]]\n\
                           target = { kind = \"mcp_server\", server = \"mail\" }\n\
                           tier = \"r2\"\n";

/// The documents of one state root, as a host composes them, with every
/// read counted. A gated source parks at its first read until the test
/// releases it, and from that read on serves a copy of the documents taken
/// then: its writer checks against the documents as they were when its check
/// began, the widest window an unserialized read, check and write has.
struct Documents {
    live: FilesystemRealmConfigSource,
    state_root: PathBuf,
    user_document: PathBuf,
    reads: AtomicUsize,
    gate: Option<Gate>,
    snapshot: tokio::sync::OnceCell<FilesystemRealmConfigSource>,
}

struct Gate {
    parked: Arc<Notify>,
    release: Arc<Notify>,
    snapshot_root: PathBuf,
}

impl Documents {
    fn new(state_root: &Path, user_document: &Path, gate: Option<Gate>) -> Self {
        Self {
            live: FilesystemRealmConfigSource::new(
                state_root,
                user_document,
                meerkat_models::canonical(),
            ),
            state_root: state_root.to_path_buf(),
            user_document: user_document.to_path_buf(),
            reads: AtomicUsize::new(0),
            gate,
            snapshot: tokio::sync::OnceCell::new(),
        }
    }

    /// The source a read goes to: the copy taken at a gated source's first
    /// read, else the live documents.
    async fn source(&self) -> &FilesystemRealmConfigSource {
        if self.reads.fetch_add(1, Ordering::SeqCst) == 0
            && let Some(gate) = &self.gate
        {
            let state_root = gate.snapshot_root.join("realms");
            let user_document = gate.snapshot_root.join("user.toml");
            copy_tree(&self.state_root, &state_root);
            std::fs::copy(&self.user_document, &user_document).unwrap();
            let _ = self.snapshot.set(FilesystemRealmConfigSource::new(
                state_root,
                user_document,
                meerkat_models::canonical(),
            ));
            gate.parked.notify_one();
            gate.release.notified().await;
        }
        self.snapshot.get().unwrap_or(&self.live)
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[async_trait]
impl RealmConfigSource for Documents {
    async fn config_for_realm(&self, realm: &RealmId) -> Result<Option<Config>, ConfigError> {
        self.source().await.config_for_realm(realm).await
    }

    async fn config_for_realm_with_warnings(
        &self,
        realm: &RealmId,
    ) -> Result<Option<(Config, Vec<ConfigWarning>)>, ConfigError> {
        self.source()
            .await
            .config_for_realm_with_warnings(realm)
            .await
    }

    async fn raw_config_for_realm(
        &self,
        realm: &RealmId,
    ) -> Result<Option<toml::Value>, ConfigError> {
        self.source().await.raw_config_for_realm(realm).await
    }

    fn composition_lock_path(&self) -> Option<PathBuf> {
        self.live.composition_lock_path()
    }
}

/// Writer A (the user document) is parked inside its composition check
/// when writer B (realm `team`'s own document) starts. Each check reads the
/// other's document: A composes `team` with `team`'s own document, and B
/// composes `team` over the user document. B reads nothing until A has
/// finished and released the state root's lock; unserialized, B would check
/// against the user document A is about to replace. The writes are checked
/// one after the other, and `team` keeps the user document's governance.
#[tokio::test]
async fn guarded_writers_over_one_state_root_take_turns() {
    let temp = tempfile::tempdir().unwrap();
    let state_root = temp.path().join("realms");
    let user_document = temp.path().join("user").join(".rkat").join("config.toml");
    let user = format!("[realm.team]\nparent = \"global\"\n\n{REVIEW_RULE}");
    tokio::fs::create_dir_all(user_document.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&user_document, &user).await.unwrap();
    let team = RealmId::parse("team").unwrap();
    let team_document =
        FilesystemRealmConfigSource::new(&state_root, &user_document, meerkat_models::canonical())
            .config_doc_path(&team);
    tokio::fs::create_dir_all(team_document.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&team_document, "[realm.team]\n\n[realm.global]\n")
        .await
        .unwrap();

    let parked = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let documents_a = Arc::new(Documents::new(
        &state_root,
        &user_document,
        Some(Gate {
            parked: Arc::clone(&parked),
            release: Arc::clone(&release),
            snapshot_root: temp.path().join("snapshot"),
        }),
    ));
    let documents_b = Arc::new(Documents::new(&state_root, &user_document, None));
    let store_a = FileConfigStore::new(user_document.clone(), meerkat_models::canonical())
        .with_realm_chain(
            RealmId::global(),
            Arc::clone(&documents_a) as Arc<dyn RealmConfigSource>,
        );
    let store_b = FileConfigStore::new(team_document.clone(), meerkat_models::canonical())
        .with_realm_chain(
            team.clone(),
            Arc::clone(&documents_b) as Arc<dyn RealmConfigSource>,
        );

    let writer_a = tokio::spawn(async move {
        store_a
            .patch(ConfigDelta(serde_json::json!({"realm": {"team": null}})))
            .await
            .map(|_| ())
    });
    tokio::time::timeout(Duration::from_secs(30), parked.notified())
        .await
        .expect("writer A reaches its composition check");

    let writer_b = tokio::spawn(async move {
        store_b
            .patch(ConfigDelta(serde_json::json!({"realm": null})))
            .await
            .map(|_| ())
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let reads_while_a_checks = documents_b.reads.load(Ordering::SeqCst);
    let b_finished_while_a_checks = writer_b.is_finished();
    release.notify_one();
    let outcome_a = writer_a.await.unwrap();
    let outcome_b = writer_b.await.unwrap();

    let composed = EffectiveConfigReader::new(Arc::new(FilesystemRealmConfigSource::new(
        &state_root,
        &user_document,
        meerkat_models::canonical(),
    )))
    .effective_config(&team)
    .await;
    let mut failures = Vec::new();
    if reads_while_a_checks != 0 || b_finished_while_a_checks {
        failures.push(format!(
            "writer B ran while writer A was between its check and its write \
             ({reads_while_a_checks} reads, finished: {b_finished_while_a_checks})"
        ));
    }
    if !composed
        .as_ref()
        .is_ok_and(|config| config.governance.is_some())
    {
        failures.push(format!(
            "team composes the user document's governance after both writes \
             (A: {outcome_a:?}, B: {outcome_b:?}): {:?}",
            composed.map(|config| config.governance)
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
