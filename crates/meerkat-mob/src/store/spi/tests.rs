//! Rows for the mob host store adapter. UNRUN (Policy v1.1): written for the
//! frozen candidate, which the validation owner runs.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use meerkat_core::host_store::{
    AcquireMode, CommitBatch, CommitId, FenceGrant, FenceToken, HeadPage, HostStore,
    HostStoreError, Row, RowIndex, ScopeHead, ScopeKind, ScopeName, ScopeRef, ScopeVersion,
    VersionedHead,
};
use meerkat_runtime::store::spi::{SpiRealm, SpiRealmConfig};

use super::*;

/// One scope of the in-test host store.
#[derive(Debug, Clone)]
struct HostScope {
    version: ScopeVersion,
    commit_id: CommitId,
    head: Vec<u8>,
    first: u64,
    next: u64,
    rows: BTreeMap<u64, Row>,
}

/// What one committed batch looked like.
#[derive(Debug, Clone)]
struct BatchRecord {
    scopes: Vec<ScopeRef>,
    payload: usize,
}

#[derive(Debug, Default)]
struct HostState {
    token: u64,
    scopes: BTreeMap<ScopeRef, HostScope>,
    landed: HashSet<String>,
    batches: Vec<BatchRecord>,
    fail_next_commit: Option<HostStoreError>,
}

/// A host store meeting every obligation at commit: versions are checked
/// (Conflict), lower tokens refused (FencedOut), commit ids idempotent.
#[derive(Debug, Default)]
struct MemoryHost {
    state: StdMutex<HostState>,
}

impl MemoryHost {
    fn state(&self) -> std::sync::MutexGuard<'_, HostState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn fail_next_commit(&self, error: HostStoreError) {
        self.state().fail_next_commit = Some(error);
    }

    fn batches(&self) -> Vec<BatchRecord> {
        self.state().batches.clone()
    }
}

#[async_trait]
impl HostStore for MemoryHost {
    async fn acquire_fence(&self, _mode: AcquireMode) -> Result<FenceGrant, HostStoreError> {
        let mut state = self.state();
        state.token += 1;
        Ok(FenceGrant {
            token: FenceToken::new(state.token),
            superseded_lease: Duration::ZERO,
        })
    }

    async fn renew_fence(&self, token: FenceToken) -> Result<(), HostStoreError> {
        let state = self.state();
        if token.get() < state.token {
            return Err(HostStoreError::FencedOut {
                current: FenceToken::new(state.token),
            });
        }
        Ok(())
    }

    async fn load_head(&self, scope: &ScopeRef) -> Result<Option<VersionedHead>, HostStoreError> {
        Ok(self.state().scopes.get(scope).map(|stored| VersionedHead {
            version: stored.version,
            commit_id: stored.commit_id.clone(),
            head: stored.head.clone(),
        }))
    }

    async fn list_heads(
        &self,
        kind: ScopeKind,
        after: Option<&ScopeName>,
        limit: usize,
    ) -> Result<HeadPage, HostStoreError> {
        let state = self.state();
        let mut heads: Vec<ScopeHead> = state
            .scopes
            .iter()
            .filter(|(scope, _)| {
                scope.kind == kind && after.is_none_or(|after| &scope.name > after)
            })
            .map(|(scope, stored)| ScopeHead {
                name: scope.name.clone(),
                head: VersionedHead {
                    version: stored.version,
                    commit_id: stored.commit_id.clone(),
                    head: stored.head.clone(),
                },
            })
            .collect();
        heads.sort_by(|left, right| {
            left.name
                .as_str()
                .as_bytes()
                .cmp(right.name.as_str().as_bytes())
        });
        let next = (heads.len() > limit).then(|| heads[limit - 1].name.clone());
        heads.truncate(limit);
        Ok(HeadPage { heads, next })
    }

    async fn read_rows(
        &self,
        scope: &ScopeRef,
        from: RowIndex,
        limit: usize,
    ) -> Result<Vec<Row>, HostStoreError> {
        let state = self.state();
        let Some(stored) = state.scopes.get(scope) else {
            return Ok(Vec::new());
        };
        if from.get() < stored.first {
            return Err(HostStoreError::Invalid(format!(
                "{scope}: row {from} was dropped"
            )));
        }
        Ok(stored
            .rows
            .range(from.get()..)
            .take(limit)
            .map(|(_, row)| row.clone())
            .collect())
    }

    async fn commit(&self, batch: &CommitBatch) -> Result<(), HostStoreError> {
        let mut state = self.state();
        if let Some(error) = state.fail_next_commit.take() {
            return Err(error);
        }
        if state.landed.contains(batch.commit_id().as_str()) {
            return Ok(());
        }
        if batch.fence().get() < state.token {
            return Err(HostStoreError::FencedOut {
                current: FenceToken::new(state.token),
            });
        }
        for update in batch.updates() {
            let current = state.scopes.get(&update.scope).map(|stored| stored.version);
            if current != update.previous {
                return Err(HostStoreError::Conflict {
                    scope: update.scope.clone(),
                    current,
                });
            }
        }
        for update in batch.updates() {
            if update.delete {
                state.scopes.remove(&update.scope);
                continue;
            }
            let stored = state
                .scopes
                .entry(update.scope.clone())
                .or_insert_with(|| HostScope {
                    version: update.next,
                    commit_id: batch.commit_id().clone(),
                    head: Vec::new(),
                    first: 0,
                    next: 0,
                    rows: BTreeMap::new(),
                });
            if let Some(truncate) = update.truncate_after {
                stored.rows.retain(|index, _| *index < truncate.get());
                stored.next = stored.next.min(truncate.get());
            }
            if let Some(drop_before) = update.drop_before {
                stored.rows.retain(|index, _| *index >= drop_before.get());
                stored.first = stored.first.max(drop_before.get().min(stored.next));
            }
            for row in &update.append {
                let index = stored.next;
                stored.rows.insert(index, row.clone());
                stored.next += 1;
            }
            if let Some(head) = &update.head {
                stored.head = head.clone();
            }
            stored.version = update.next;
            stored.commit_id = batch.commit_id().clone();
        }
        state.landed.insert(batch.commit_id().as_str().to_string());
        state.batches.push(BatchRecord {
            scopes: batch
                .updates()
                .iter()
                .map(|update| update.scope.clone())
                .collect(),
            payload: batch.payload_bytes(),
        });
        Ok(())
    }
}

/// One process over the host: its realm, its mob host, one open storage.
struct Process {
    stores: SpiMobStores,
    _mobs: SpiMobStoreHost,
    _realm: Arc<SpiRealm>,
}

async fn start(host: &Arc<MemoryHost>, key: &str) -> Process {
    let dyn_host: Arc<dyn HostStore> = Arc::clone(host) as Arc<dyn HostStore>;
    let realm = SpiRealm::open(dyn_host, SpiRealmConfig::default())
        .await
        .expect("open the realm");
    let mobs = SpiMobStoreHost::for_realm(&realm)
        .await
        .expect("claim the mob families");
    let stores = mobs.open(key).await.expect("open the storage");
    Process {
        stores,
        _mobs: mobs,
        _realm: realm,
    }
}

/// Every entry of every scope of the storage, from the in-memory state.
async fn full_export(stores: &SpiMobStores) -> BTreeMap<String, Vec<u8>> {
    let bundle = &stores.bundle;
    let mut out = BTreeMap::new();
    for (name, bytes) in bundle.mob_image().await.expect("mob image") {
        out.insert(format!("mb:{name}"), bytes.to_vec());
    }
    for (name, bytes) in bundle.identity_image().await.expect("identity image") {
        out.insert(format!("mbi:{name}"), bytes.to_vec());
    }
    for (name, bytes) in bundle.run_image().await.expect("run image") {
        out.insert(format!("mbr:{name}"), bytes.to_vec());
    }
    for (name, bytes) in bundle.status_image().await.expect("status image") {
        out.insert(format!("mbs:{name}"), bytes.to_vec());
    }
    out
}

/// Export, restart the process over the same host, and require the exact
/// same export from the reloaded storage.
async fn restart_and_compare(host: &Arc<MemoryHost>, key: &str, process: Process) -> Process {
    let before = full_export(&process.stores).await;
    drop(process);
    let reopened = start(host, key).await;
    assert_eq!(
        full_export(&reopened.stores).await,
        before,
        "a reopen must load exactly what the process served"
    );
    reopened
}

fn event(kind: MobEventKind, timestamp: Option<DateTime<Utc>>) -> NewMobEvent {
    NewMobEvent {
        mob_id: MobId::from("mob"),
        timestamp,
        kind,
    }
}

// UNRUN (Policy v1.1).
#[tokio::test]
async fn reopen_restores_the_exact_export_after_each_mutating_call() {
    let host = Arc::new(MemoryHost::default());
    let mut process = start(&host, "mob").await;

    // Events.
    let storage = process.stores.storage();
    for _ in 0..3 {
        storage
            .events
            .append(event(MobEventKind::MobCompleted, None))
            .await
            .expect("append");
    }
    process = restart_and_compare(&host, "mob", process).await;

    // A spec.
    let storage = process.stores.storage();
    storage
        .specs
        .put_spec(&MobId::from("mob"), &MobDefinition::explicit("mob"), None)
        .await
        .expect("put spec");
    process = restart_and_compare(&host, "mob", process).await;

    // A flow run.
    let storage = process.stores.storage();
    let run = MobRun::authority_backed_for_steps(
        RunId::new(),
        MobId::from("mob"),
        FlowId::from("flow-a"),
        [StepId::from("step-1")],
        MobRunStatus::Pending,
        serde_json::json!({"a": 1}),
    )
    .expect("sample run");
    storage.runs.create_run(run).await.expect("create run");
    process = restart_and_compare(&host, "mob", process).await;

    // Runtime metadata.
    let storage = process.stores.storage();
    storage
        .runtime_metadata
        .put_member_event_cursor(
            &MobId::from("mob"),
            &MobMemberEventCursorRecord {
                agent_identity: "worker-1".to_string(),
                host_id: String::new(),
                host_binding_generation: None,
                member_session_id: String::new(),
                generation: 1,
                fence_token: None,
                next_seq: 7,
            },
        )
        .await
        .expect("put cursor");
    process = restart_and_compare(&host, "mob", process).await;

    // Prune, then clear (a finished destroy).
    let storage = process.stores.storage();
    storage
        .events
        .prune(Utc::now() + chrono::Duration::minutes(1))
        .await
        .expect("prune");
    process = restart_and_compare(&host, "mob", process).await;
    let storage = process.stores.storage();
    storage.events.clear().await.expect("clear");
    let process = restart_and_compare(&host, "mob", process).await;
    assert!(
        process
            .stores
            .storage()
            .events
            .replay_all()
            .await
            .expect("replay")
            .is_empty()
    );
}

// UNRUN (Policy v1.1).
#[tokio::test]
async fn a_write_over_two_scopes_commits_one_batch() {
    let host = Arc::new(MemoryHost::default());
    let process = start(&host, "mob").await;
    let bundle = Arc::clone(&process.stores.bundle);
    let before = host.batches().len();
    let run = MobRun::authority_backed_for_steps(
        RunId::new(),
        MobId::from("mob"),
        FlowId::from("flow-a"),
        [StepId::from("step-1")],
        MobRunStatus::Pending,
        serde_json::json!({}),
    )
    .expect("sample run");
    let run_id = run.run_id.clone();
    let touch = Touch {
        events: Some(EventsTouch::Appended),
        run: Some(run_id),
        ..Touch::default()
    };
    let inner = Arc::clone(&bundle);
    bundle
        .write(touch, async move {
            inner
                .events
                .append(event(MobEventKind::MobCompleted, None))
                .await?;
            inner.runs.create_run(run).await
        })
        .await
        .expect("write");
    let batches = host.batches();
    assert_eq!(batches.len(), before + 1, "one batch, not one per scope");
    let last = batches.last().expect("a batch");
    assert!(last.scopes.iter().any(|scope| scope.kind == ScopeKind::Mob));
    assert!(
        last.scopes
            .iter()
            .any(|scope| scope.kind == ScopeKind::Record)
    );
}

// UNRUN (Policy v1.1).
#[tokio::test]
async fn an_event_append_writes_only_the_new_event() {
    let host = Arc::new(MemoryHost::default());
    let process = start(&host, "mob").await;
    let storage = process.stores.storage();
    storage
        .events
        .append(event(MobEventKind::MobCompleted, None))
        .await
        .expect("append");
    storage
        .events
        .append(event(MobEventKind::MobCompleted, None))
        .await
        .expect("append");
    let early = host.batches().last().expect("a batch").payload;
    for _ in 0..200 {
        storage
            .events
            .append(event(MobEventKind::MobCompleted, None))
            .await
            .expect("append");
    }
    let late = host.batches().last().expect("a batch").payload;
    assert!(
        late <= early + 64,
        "the 202nd append carried {late} bytes against {early} for the 2nd"
    );
}

// UNRUN (Policy v1.1).
#[tokio::test]
async fn a_failed_batch_poisons_its_scopes_and_only_them() {
    let host = Arc::new(MemoryHost::default());
    let process = start(&host, "mob").await;
    let storage = process.stores.storage();
    let mut live = storage.events.subscribe().expect("subscribe");
    host.fail_next_commit(HostStoreError::Invalid("refused by the test".to_string()));
    let refused = storage
        .events
        .append(event(MobEventKind::MobCompleted, None))
        .await;
    assert!(matches!(refused, Err(MobStoreError::WriteFailed(_))));
    assert!(
        live.try_recv().is_err(),
        "a subscriber never sees an event its batch did not commit"
    );
    let read = storage.events.replay_all().await;
    assert!(
        matches!(&read, Err(MobStoreError::ReadFailed(detail)) if detail.contains("poisoned")),
        "the poisoned mob scope refuses reads: {read:?}"
    );
    // Another scope of the same storage still serves.
    storage
        .specs
        .get_spec(&MobId::from("mob"))
        .await
        .expect_err("specs share the poisoned mob scope");
    storage
        .runs
        .get_run(&RunId::new())
        .await
        .expect("the run scope is not poisoned");
    // Another storage still writes.
    let other = SpiMobStoreHost::for_realm(&process._realm)
        .await
        .expect("the same host")
        .open("other")
        .await
        .expect("open another storage");
    other
        .storage()
        .events
        .append(event(MobEventKind::MobCompleted, None))
        .await
        .expect("another storage appends");
}

// UNRUN (Policy v1.1).
#[tokio::test]
async fn subscribers_receive_committed_events_in_cursor_order() {
    let host = Arc::new(MemoryHost::default());
    let process = start(&host, "mob").await;
    let storage = process.stores.storage();
    let mut live = storage.events.subscribe().expect("subscribe");
    for _ in 0..3 {
        storage
            .events
            .append(event(MobEventKind::MobCompleted, None))
            .await
            .expect("append");
    }
    let mut cursors = Vec::new();
    while let Ok(received) = live.try_recv() {
        cursors.push(received.cursor);
    }
    assert_eq!(cursors, vec![1, 2, 3]);
}

// UNRUN (Policy v1.1).
#[tokio::test]
async fn the_mob_families_are_claimed_once_per_realm() {
    let host = Arc::new(MemoryHost::default());
    {
        let process = start(&host, "mob").await;
        process
            .stores
            .storage()
            .events
            .append(event(MobEventKind::MobCompleted, None))
            .await
            .expect("append");
    }
    let dyn_host: Arc<dyn HostStore> = Arc::clone(&host) as Arc<dyn HostStore>;
    let realm = SpiRealm::open(dyn_host, SpiRealmConfig::default())
        .await
        .expect("reopen the realm");
    let first = SpiMobStoreHost::for_realm(&realm).await.expect("first");
    let second = SpiMobStoreHost::for_realm(&realm).await.expect("second");
    assert!(Arc::ptr_eq(&first.inner, &second.inner));
    let listed = second.stored_mobs();
    assert!(
        listed.iter().any(|stored| stored.storage_key == "mob"),
        "a second claim must not hide stored mobs: {listed:?}"
    );
}

// UNRUN (Policy v1.1).
#[test]
fn entries_of_an_unknown_format_are_refused() {
    let newer = br#"{"v":2,"d":{"mob_id":null,"finalized":false}}"#;
    assert!(matches!(
        decode_single::<MobMetaV1>(newer),
        Err(MobStoreError::ReadFailed(_))
    ));
    let current = encode_single(&MobMetaV1 {
        mob_id: None,
        finalized: true,
    })
    .expect("encode");
    assert_eq!(
        decode_single::<MobMetaV1>(&current).expect("decode"),
        MobMetaV1 {
            mob_id: None,
            finalized: true
        }
    );
}

// UNRUN (Policy v1.1).
#[test]
fn keyed_entry_names_are_injective_and_verified() {
    let left = hashed_name(G_SUPERVISOR, &MobId::from("a/b")).expect("name");
    let right = hashed_name(G_SUPERVISOR, &MobId::from("a")).expect("name");
    assert_ne!(left, right);
    let mut body: BTreeMap<String, EntryBytes> = BTreeMap::new();
    body.insert(
        right,
        encode_keyed(&MobId::from("a/b"), &1_u64).expect("encode"),
    );
    assert!(
        decode_group::<MobId, u64>(&body, G_SUPERVISOR).is_err(),
        "an entry whose name does not hash from its key is refused"
    );
}
