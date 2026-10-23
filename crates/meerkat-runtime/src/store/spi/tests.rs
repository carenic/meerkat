//! Rows for the host store adapters, unrun until the candidate's
//! validation.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use meerkat_core::host_store::{AcquireMode, FenceTiming, ScopeKind};
use meerkat_core::lifecycle::InputId;

use super::realm::{CHECKPOINT_MIN_BYTES, INLINE_VALUE_LIMIT};
use super::test_host::TestHost;
use super::{
    ScopeDelta, ScopeFamily, ScopeId, SpiError, SpiRealm, SpiRealmConfig, SpiRuntimeStore,
};
use crate::identifiers::LogicalRuntimeId;
use crate::input_state::{InputStatePersistenceRecord, StoredInputState};
use crate::store::{
    RuntimeDeliveryAuthorityCasOutcome, RuntimeDeliveryAuthorityRecord, RuntimeDeliveryStoreRecord,
    RuntimeStore, RuntimeStoreError,
};

const TEST_FAMILY: ScopeFamily = ScopeFamily {
    tag: "tst",
    kind: ScopeKind::Record,
    pinned: &["pin"],
};

fn persistable(bundle: StoredInputState) -> InputStatePersistenceRecord {
    InputStatePersistenceRecord::from_machine_snapshot(bundle).expect("machine snapshot")
}

async fn open(host: &Arc<TestHost>) -> (Arc<SpiRealm>, SpiRuntimeStore) {
    let realm = SpiRealm::open(Arc::clone(host) as _, SpiRealmConfig::default())
        .await
        .expect("realm opens");
    let store = SpiRuntimeStore::open(Arc::clone(&realm))
        .await
        .expect("store opens");
    (realm, store)
}

/// Drop a process and open another over the same host, as a crash restart
/// whose lease ran out.
async fn reopen(
    host: &Arc<TestHost>,
    old: (Arc<SpiRealm>, SpiRuntimeStore),
) -> (Arc<SpiRealm>, SpiRuntimeStore) {
    drop(old);
    host.expire_lease();
    open(host).await
}

fn delivery_record(sequence: u64) -> RuntimeDeliveryStoreRecord {
    RuntimeDeliveryStoreRecord::from_parts(
        format!("delivery-{sequence}"),
        sequence,
        br#"{"kind":"test"}"#.to_vec(),
    )
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn writes_reach_the_host_and_a_reopen_sees_the_same_full_export() {
    let host = Arc::new(TestHost::new());
    let opened = open(&host).await;
    let alpha = LogicalRuntimeId::new("alpha");
    let beta = LogicalRuntimeId::new("beta");
    for runtime in [&alpha, &beta] {
        for _ in 0..3 {
            let bundle = StoredInputState::new_accepted(InputId::new());
            opened
                .1
                .persist_input_state(runtime, &persistable(bundle))
                .await
                .expect("persist");
        }
    }
    let outcome = opened
        .1
        .compare_and_swap_runtime_delivery_authority(
            &beta,
            None,
            RuntimeDeliveryAuthorityRecord::from_parts(1, br#"{"state":1}"#.to_vec()),
            Some(delivery_record(1)),
        )
        .await
        .expect("delivery cas");
    assert!(matches!(
        outcome,
        RuntimeDeliveryAuthorityCasOutcome::Applied(_)
    ));
    let high_water = meerkat_contracts::wire::supervisor_bridge::BridgeDirectMemberIncarnation {
        mob_id: "mob".into(),
        agent_identity: "member".into(),
        generation: 3,
        fence_token: 7,
    };
    opened
        .1
        .admit_direct_member_incarnation_high_water("member-session", &high_water)
        .await
        .expect("high water");
    let before_alpha = opened.1.export_runtime_for_test("alpha").await;
    let before_beta = opened.1.export_runtime_for_test("beta").await;
    assert_eq!(
        host.scopes_of(ScopeKind::Session).len(),
        2,
        "one session scope per runtime"
    );

    let reopened = reopen(&host, opened).await;
    // Touch both bodies so they are resident, then compare everything.
    reopened.1.load_input_states(&alpha).await.expect("alpha");
    reopened.1.load_input_states(&beta).await.expect("beta");
    assert_eq!(
        reopened.1.export_runtime_for_test("alpha").await,
        before_alpha
    );
    assert_eq!(
        reopened.1.export_runtime_for_test("beta").await,
        before_beta
    );
    assert!(
        host.scopes_of(ScopeKind::Record)
            .keys()
            .any(|name| name.starts_with("dmh:")),
        "the high water is a record scope"
    );
    let authorities = reopened
        .1
        .list_runtime_delivery_authorities()
        .await
        .expect("listing");
    assert_eq!(authorities.len(), 1);
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn a_body_recovery_does_not_need_stays_on_the_host_until_first_use() {
    let host = Arc::new(TestHost::new());
    let opened = open(&host).await;
    let quiet = LogicalRuntimeId::new("quiet");
    opened
        .1
        .compare_and_swap_runtime_delivery_authority(
            &quiet,
            None,
            RuntimeDeliveryAuthorityRecord::from_parts(1, br#"{"state":1}"#.to_vec()),
            Some(delivery_record(1)),
        )
        .await
        .expect("delivery cas");
    let busy = LogicalRuntimeId::new("busy");
    opened
        .1
        .persist_input_state(
            &busy,
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("nonterminal input");

    let reopened = reopen(&host, opened).await;
    assert!(
        reopened.1.runtime_loaded_for_test("busy").await,
        "a runtime with nonterminal inputs loads at boot"
    );
    assert!(
        !reopened.1.runtime_loaded_for_test("quiet").await,
        "a runtime recovery need not act on stays on the host"
    );
    // The pinned delivery authority is served without the body.
    assert!(
        reopened
            .1
            .load_runtime_delivery_authority(&quiet)
            .await
            .expect("authority")
            .is_some()
    );
    assert!(!reopened.1.runtime_loaded_for_test("quiet").await);
    // A body read loads it.
    let records = reopened
        .1
        .list_runtime_delivery_records(&quiet, 0, 10)
        .await
        .expect("records");
    assert_eq!(records.len(), 1);
    assert!(reopened.1.runtime_loaded_for_test("quiet").await);
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn a_failed_batch_poisons_its_scopes_and_nothing_else() {
    let host = Arc::new(TestHost::new());
    let (_realm, store) = open(&host).await;
    let doomed = LogicalRuntimeId::new("doomed");
    let healthy = LogicalRuntimeId::new("healthy");
    host.faults.lock().unwrap().conflict = 1;
    let refused = store
        .persist_input_state(
            &doomed,
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await;
    assert!(matches!(refused, Err(RuntimeStoreError::WriteFailed(_))));
    // The poisoned scope refuses reads too: its memory may be ahead.
    assert!(store.load_input_states(&doomed).await.is_err());
    // Another runtime keeps working.
    store
        .persist_input_state(
            &healthy,
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("independent scope");
    // Cross-scope reads fail closed while a scope is poisoned.
    assert!(
        RuntimeStore::list_runtime_session_catalog_entries(
            &store,
            meerkat_core::SessionFilter::default()
        )
        .await
        .is_err()
    );
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn an_unknown_outcome_that_landed_settles_from_the_head() {
    let host = Arc::new(TestHost::new());
    let (_realm, store) = open(&host).await;
    host.faults.lock().unwrap().indeterminate_after_apply = 1;
    let runtime = LogicalRuntimeId::new("settled");
    store
        .persist_input_state(
            &runtime,
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("the landed batch is acknowledged");
    assert_eq!(host.commit_count(), 1, "the batch landed once");
    // The scope is healthy: a second write uses the next version.
    store
        .persist_input_state(
            &runtime,
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("next version");
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn an_unavailable_store_is_retried_with_the_same_commit_id() {
    let host = Arc::new(TestHost::new());
    let (_realm, store) = open(&host).await;
    host.faults.lock().unwrap().unavailable = 2;
    store
        .persist_input_state(
            &LogicalRuntimeId::new("retried"),
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("retried within the budget");
    assert_eq!(host.commit_count(), 1);
}

// Unrun until the candidate's validation.
#[tokio::test(start_paused = true)]
async fn a_failed_renew_stops_writes_before_memory_changes() {
    let timing = FenceTiming::try_new(Duration::from_secs(1), Duration::from_secs(3)).unwrap();
    let host = Arc::new(TestHost::with_timing(timing));
    let (realm, store) = open(&host).await;
    // The background renew fails, then the re-acquire fails too.
    {
        let mut faults = host.faults.lock().unwrap();
        faults.renew_unavailable = 1;
        faults.acquire_unavailable = 1;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !realm.lease_is_live(),
        "a failed renew stops writes at once"
    );
    let runtime = LogicalRuntimeId::new("fenced");
    let refused = store
        .persist_input_state(
            &runtime,
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await;
    assert!(refused.is_err());
    assert_eq!(host.commit_count(), 0);
    // Memory did not change: once a renew succeeds the runtime is empty.
    assert!(
        store
            .load_input_states(&runtime)
            .await
            .expect("read")
            .is_empty()
    );
}

// Unrun until the candidate's validation.
#[tokio::test(start_paused = true)]
async fn an_exclusive_open_refuses_a_live_holder_and_a_takeover_waits_out_its_lease() {
    let host = Arc::new(TestHost::new());
    let (first, _store) = open(&host).await;
    let held = SpiRealm::open(Arc::clone(&host) as _, SpiRealmConfig::default()).await;
    assert!(matches!(held, Err(SpiError::Held { .. })));
    let started = tokio::time::Instant::now();
    let config = SpiRealmConfig {
        acquire: AcquireMode::Takeover,
        ..SpiRealmConfig::default()
    };
    let second = SpiRealm::open(Arc::clone(&host) as _, config)
        .await
        .expect("takeover");
    assert!(
        started.elapsed() > Duration::from_secs(1),
        "waited out the lease"
    );
    assert!(second.fence() > first.fence());
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn both_adapters_share_one_fence() {
    let host = Arc::new(TestHost::new());
    let (realm, store) = open(&host).await;
    store
        .persist_input_state(
            &LogicalRuntimeId::new("runtime"),
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("runtime adapter");
    // A second adapter (as the mob adapter does) commits through the same
    // realm: no second acquire, nobody fenced out.
    let id = ScopeId::new(TEST_FAMILY, "other");
    let mut guards = realm.lock_scopes(std::slice::from_ref(&id)).await;
    let mut states: Vec<_> = guards.iter_mut().map(|guard| &mut **guard).collect();
    realm
        .commit(
            &mut states,
            vec![ScopeDelta::Changes(vec![(
                "pin".into(),
                Some(Arc::new(b"one".to_vec())),
            )])],
        )
        .await
        .expect("second adapter");
    drop(states);
    drop(guards);
    store
        .persist_input_state(
            &LogicalRuntimeId::new("runtime"),
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("the runtime adapter still holds the fence");
}

async fn commit_one(realm: &SpiRealm, id: &ScopeId, delta: ScopeDelta) -> Result<(), SpiError> {
    let mut guards = realm.lock_scopes(std::slice::from_ref(id)).await;
    let mut states: Vec<_> = guards.iter_mut().map(|guard| &mut **guard).collect();
    realm.commit(&mut states, vec![delta]).await
}

async fn reload(
    host: &Arc<TestHost>,
    realm: Arc<SpiRealm>,
) -> (Arc<SpiRealm>, BTreeMap<String, Arc<Vec<u8>>>) {
    drop(realm);
    host.expire_lease();
    let realm = SpiRealm::open(Arc::clone(host) as _, SpiRealmConfig::default())
        .await
        .unwrap();
    let cells = realm.claim(TEST_FAMILY).unwrap();
    let mut image = BTreeMap::new();
    for cell in cells {
        let mut state = cell.lock().await;
        image.extend(realm.pinned_values(&state).await.unwrap());
        image.extend(realm.load_body(&mut state).await.unwrap());
    }
    (realm, image)
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn an_oversized_value_lives_in_a_chunk_that_dies_with_it() {
    let host = Arc::new(TestHost::new());
    let realm = SpiRealm::open(Arc::clone(&host) as _, SpiRealmConfig::default())
        .await
        .unwrap();
    let id = ScopeId::new(TEST_FAMILY, "big");
    let big = Arc::new(vec![7_u8; 3 * 1024 * 1024 + 11]);
    commit_one(
        &realm,
        &id,
        ScopeDelta::Changes(vec![("body".into(), Some(Arc::clone(&big)))]),
    )
    .await
    .unwrap();
    let chunks = |host: &TestHost| {
        host.scopes_of(ScopeKind::Record)
            .keys()
            .filter(|name| name.starts_with("ch:"))
            .count()
    };
    assert_eq!(chunks(&host), 1);
    let (realm, image) = reload(&host, realm).await;
    assert_eq!(image.get("body"), Some(&big));
    commit_one(
        &realm,
        &id,
        ScopeDelta::Changes(vec![("body".into(), Some(Arc::new(b"small".to_vec())))]),
    )
    .await
    .unwrap();
    assert_eq!(
        chunks(&host),
        0,
        "the superseded chunk is deleted in the same batch"
    );
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn a_change_set_over_the_batch_limit_spills_and_reloads() {
    let host = Arc::new(TestHost::new());
    let realm = SpiRealm::open(Arc::clone(&host) as _, SpiRealmConfig::default())
        .await
        .unwrap();
    let id = ScopeId::new(TEST_FAMILY, "wide");
    let changes: Vec<_> = (0..40)
        .map(|index| {
            (
                format!("entry/{index}"),
                Some(Arc::new(vec![index as u8; INLINE_VALUE_LIMIT - 64])),
            )
        })
        .collect();
    let expected: BTreeMap<String, Arc<Vec<u8>>> = changes
        .iter()
        .map(|(key, value)| (key.clone(), Arc::clone(value.as_ref().unwrap())))
        .collect();
    commit_one(&realm, &id, ScopeDelta::Changes(changes))
        .await
        .unwrap();
    let (_realm, image) = reload(&host, realm).await;
    assert_eq!(image, expected);
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn a_checkpoint_drops_every_earlier_row_in_one_batch() {
    let host = Arc::new(TestHost::new());
    let realm = SpiRealm::open(Arc::clone(&host) as _, SpiRealmConfig::default())
        .await
        .unwrap();
    let id = ScopeId::new(TEST_FAMILY, "churn");
    let value = |round: u8| Arc::new(vec![round; 200 * 1024]);
    let mut round = 0_u8;
    loop {
        round = round.wrapping_add(1);
        let wants_full = realm.cell(&id).lock().await.wants_full_image();
        let delta = if wants_full {
            let mut image = BTreeMap::new();
            image.insert("hot".to_string(), value(round));
            ScopeDelta::Full(image)
        } else {
            ScopeDelta::Changes(vec![("hot".into(), Some(value(round)))])
        };
        commit_one(&realm, &id, delta).await.unwrap();
        if wants_full {
            break;
        }
        assert!(u64::from(round) * 200 * 1024 < 4 * CHECKPOINT_MIN_BYTES);
    }
    let stored = host.scopes_of(ScopeKind::Record);
    let scope = stored
        .iter()
        .find(|(name, _)| name.starts_with("tst:"))
        .map(|(_, scope)| scope.clone())
        .unwrap();
    assert!(scope.start > 0, "the checkpoint dropped the earlier rows");
    let (_realm, image) = reload(&host, realm).await;
    assert_eq!(image.get("hot"), Some(&value(round)));
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn a_deleted_scope_comes_back_under_a_new_name() {
    let host = Arc::new(TestHost::new());
    let realm = SpiRealm::open(Arc::clone(&host) as _, SpiRealmConfig::default())
        .await
        .unwrap();
    let id = ScopeId::new(TEST_FAMILY, "phoenix");
    let set = |value: Option<&'static [u8]>| {
        ScopeDelta::Changes(vec![(
            "pin".into(),
            value.map(|bytes| Arc::new(bytes.to_vec())),
        )])
    };
    commit_one(&realm, &id, set(Some(b"one"))).await.unwrap();
    let first: Vec<String> = host.scopes_of(ScopeKind::Record).into_keys().collect();
    commit_one(&realm, &id, set(None)).await.unwrap();
    assert!(host.scopes_of(ScopeKind::Record).is_empty());
    // The test host refuses to create a deleted name again.
    commit_one(&realm, &id, set(Some(b"two"))).await.unwrap();
    let second: Vec<String> = host.scopes_of(ScopeKind::Record).into_keys().collect();
    assert_ne!(first, second);
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn mob_host_bindings_follow_the_sqlite_store() {
    let host = Arc::new(TestHost::new());
    let opened = open(&host).await;
    let store = &opened.1;
    assert!(
        store
            .put_mob_host_binding_if_absent("mob", b"one")
            .await
            .unwrap()
    );
    assert!(
        !store
            .put_mob_host_binding_if_absent("mob", b"two")
            .await
            .unwrap()
    );
    assert!(
        !store
            .compare_and_put_mob_host_binding("mob", b"two", b"three")
            .await
            .unwrap()
    );
    assert!(
        store
            .compare_and_put_mob_host_binding("mob", b"one", b"three")
            .await
            .unwrap()
    );
    assert!(
        !store
            .revoke_mob_host_binding("mob", b"one", b"receipt")
            .await
            .unwrap()
    );
    assert_eq!(store.load_mob_host_revocation("mob").await.unwrap(), None);
    assert!(
        store
            .revoke_mob_host_binding("mob", b"three", b"receipt")
            .await
            .unwrap()
    );
    assert_eq!(store.load_mob_host_binding("mob").await.unwrap(), None);
    assert_eq!(
        store.load_mob_host_revocation("mob").await.unwrap(),
        Some(b"receipt".to_vec())
    );
    // A fresh binding supersedes the receipt in the same batch.
    assert!(
        store
            .put_mob_host_binding_if_absent("mob", b"four")
            .await
            .unwrap()
    );
    assert_eq!(store.load_mob_host_revocation("mob").await.unwrap(), None);
    assert!(
        store
            .put_mob_host_binding_if_absent("another", b"five")
            .await
            .unwrap()
    );
    assert!(
        !store
            .delete_mob_host_binding("another", b"wrong")
            .await
            .unwrap()
    );
    let reopened = reopen(&host, opened).await;
    let store = &reopened.1;
    assert_eq!(
        store.list_mob_host_bindings().await.unwrap(),
        vec![
            ("another".to_string(), b"five".to_vec()),
            ("mob".to_string(), b"four".to_vec()),
        ]
    );
    assert!(store.list_mob_host_revocations().await.unwrap().is_empty());
    assert!(
        store
            .delete_mob_host_binding("another", b"five")
            .await
            .unwrap()
    );
    assert_eq!(store.load_mob_host_binding("another").await.unwrap(), None);
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn controller_custody_refuses_while_a_commit_is_in_flight() {
    let host = Arc::new(TestHost::new());
    let (_realm, store) = open(&host).await;
    let store = Arc::new(store);
    let gate = Arc::new(tokio::sync::Notify::new());
    *host.commit_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let writer = {
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            store
                .persist_input_state(
                    &LogicalRuntimeId::new("in-flight"),
                    &persistable(StoredInputState::new_accepted(InputId::new())),
                )
                .await
        })
    };
    tokio::task::yield_now().await;
    let claim = RuntimeStore::execution_custody(&*store)
        .expect("custody")
        .try_acquire_governed()
        .expect("claim");
    assert!(
        RuntimeStore::try_controller_mutation_custody(&*store, &claim).is_err(),
        "an in-flight commit holds the gate"
    );
    *host.commit_gate.lock().unwrap() = None;
    gate.notify_one();
    writer.await.unwrap().expect("the commit lands");
    assert!(RuntimeStore::try_controller_mutation_custody(&*store, &claim).is_ok());
}

// Unrun until the candidate's validation.
#[tokio::test]
async fn a_host_declared_self_fence_margin_bounds_the_write_window() {
    use meerkat_core::host_store::SelfFence;
    let timing = FenceTiming::try_with_margin(
        Duration::from_secs(5),
        Duration::from_secs(30),
        Duration::from_secs(12),
    )
    .expect("a 12 s margin on a 30 s lease at 5 s renews");
    let host = Arc::new(TestHost::with_timing(timing));
    let (realm, store) = open(&host).await;
    let window = realm.window();
    assert_eq!(window.window, Duration::from_secs(18));
    assert!(window.is_open());
    let fence = RuntimeStore::self_fence(&store).expect("a host store realm fences itself");
    assert_eq!(fence.window().window, Duration::from_secs(18));
}

// Unrun until the candidate's validation. The K3 re-acquire rule, with the
// visibility-lag counterexample: renews fail from some point, the lease
// lapses, another process acquires, and the old writer's connection comes
// back. A renew that now succeeded would prove nothing (the new acquire may
// have missed the last renew), so the old writer never resumes on a renew:
// it tries a fresh acquire, finds the new holder, and stays down.
#[tokio::test(start_paused = true)]
async fn a_writer_that_stopped_never_resumes_on_a_later_renew() {
    let timing = FenceTiming::try_new(Duration::from_secs(1), Duration::from_secs(3)).unwrap();
    let host = Arc::new(TestHost::with_timing(timing));
    let (old_realm, old_store) = open(&host).await;
    host.faults.lock().unwrap().renew_unavailable = 1;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !old_realm.lease_is_live(),
        "the failed renew stopped the old writer"
    );
    // Its lease lapses and a new process acquires.
    host.expire_lease();
    let (new_realm, _new_store) = open(&host).await;
    assert!(new_realm.fence() > old_realm.fence());
    // The old writer's next tick re-acquires, meets the new holder, and
    // poisons itself; it never wrote again.
    let before = host.commit_count();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert!(!old_realm.lease_is_live());
    let refused = old_store
        .persist_input_state(
            &LogicalRuntimeId::new("late"),
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await;
    assert!(refused.is_err());
    assert!(old_realm.realm_poisoned().is_some());
    assert_eq!(host.commit_count(), before);
}

// Unrun until the candidate's validation.
#[tokio::test(start_paused = true)]
async fn a_stopped_writer_reacquires_and_resumes_when_nothing_changed() {
    let timing = FenceTiming::try_new(Duration::from_secs(1), Duration::from_secs(3)).unwrap();
    let host = Arc::new(TestHost::with_timing(timing));
    let (realm, store) = open(&host).await;
    let first = realm.fence();
    host.faults.lock().unwrap().renew_unavailable = 1;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!realm.lease_is_live());
    // The next tick re-acquires over its own still-recorded lease.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert!(realm.lease_is_live());
    assert!(realm.fence() > first, "a fresh token");
    store
        .persist_input_state(
            &LogicalRuntimeId::new("resumed"),
            &persistable(StoredInputState::new_accepted(InputId::new())),
        )
        .await
        .expect("writes resume under the new token");
}

// Unrun until the candidate's validation.
#[tokio::test(start_paused = true)]
async fn a_stopped_writer_finds_a_foreign_write_and_stays_down() {
    let timing = FenceTiming::try_new(Duration::from_secs(1), Duration::from_secs(3)).unwrap();
    let host = Arc::new(TestHost::with_timing(timing));
    let (old_realm, old_store) = open(&host).await;
    host.faults.lock().unwrap().renew_unavailable = 1;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // Another process writes while the old writer is stopped, then dies.
    host.expire_lease();
    {
        let (other_realm, other_store) = open(&host).await;
        other_store
            .persist_input_state(
                &LogicalRuntimeId::new("foreign"),
                &persistable(StoredInputState::new_accepted(InputId::new())),
            )
            .await
            .unwrap();
        drop(other_store);
        drop(other_realm);
    }
    host.expire_lease();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert!(
        old_realm.realm_poisoned().is_some(),
        "a foreign batch since the stop keeps the old writer down"
    );
    assert!(
        old_store
            .load_input_states(&LogicalRuntimeId::new("foreign"))
            .await
            .is_err()
    );
}
