//! End-to-end integration test for the Track-B composition driver
//! stack: `RecomputeMobPeerOverlayDriver` → typed dispatches →
//! `CommsTrustReconciler` → simulated trust store.
//!
//! Drives a realistic mob respawn scenario through the full Track-B
//! pipeline and asserts the comms reconciler ends at the expected
//! applied peer set. This is the "integration proof" the shadow-mode
//! unit parity test in
//! `meerkat-runtime/src/recompute_mob_peer_overlay.rs` complements:
//! the unit test proves the driver's per-session overlay computation
//! is correct; this test wires the driver output through the
//! reconciler and a recording `CommsRuntime` to prove the mechanical
//! trust-store reconciliation behaves end-to-end.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use meerkat_core::agent::CommsRuntime;
use meerkat_core::comms::{SendError, TrustedPeerSpec};
use meerkat_runtime::meerkat_machine::dsl as mm_dsl;
use meerkat_runtime::{CommsTrustReconciler, RecomputeMobPeerOverlayDriver};

/// Lightweight trust-store recorder used as the `CommsRuntime`
/// implementation under test. Tracks the currently installed peer
/// set per registered add / unregistered remove.
#[derive(Default)]
struct TrustStoreRecorder {
    current: Mutex<BTreeMap<String, TrustedPeerSpec>>,
}

impl std::fmt::Debug for TrustStoreRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f.debug_struct("TrustStoreRecorder")
            .field("peer_count", &current.len())
            .finish()
    }
}

impl TrustStoreRecorder {
    fn peer_ids(&self) -> BTreeSet<String> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl CommsRuntime for TrustStoreRecorder {
    async fn drain_messages(&self) -> Vec<String> {
        Vec::new()
    }
    fn inbox_notify(&self) -> Arc<tokio::sync::Notify> {
        Arc::new(tokio::sync::Notify::new())
    }
    async fn add_trusted_peer(&self, peer: TrustedPeerSpec) -> Result<(), SendError> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(peer.peer_id.clone(), peer);
        Ok(())
    }
    async fn remove_trusted_peer(&self, peer_id: &str) -> Result<bool, SendError> {
        let removed = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(peer_id)
            .is_some();
        Ok(removed)
    }
}

fn endpoint(name: &str) -> mm_dsl::PeerEndpoint {
    mm_dsl::PeerEndpoint {
        name: format!("ep-{name}"),
        peer_id: format!("ed25519:{name}"),
        address: format!("inproc://{name}"),
    }
}

/// Reconcile every dispatch the driver produces against a dedicated
/// reconciler per session. This is the integration harness Commit 5's
/// eventual actor-level wiring will install:
///
///   driver.recompute_all()
///     .into_iter()
///     .for_each(|dispatch| reconciler_for(session).reconcile(epoch, endpoints))
async fn dispatch_all(
    driver: &RecomputeMobPeerOverlayDriver,
    reconcilers: &BTreeMap<String, Arc<CommsTrustReconciler>>,
) {
    for dispatch in driver.recompute_all() {
        let reconciler = reconcilers
            .get(&dispatch.session_id)
            .unwrap_or_else(|| panic!("missing reconciler for session {}", dispatch.session_id));
        reconciler
            .reconcile(dispatch.epoch, dispatch.endpoints)
            .await
            .expect("reconcile");
    }
}

#[tokio::test]
async fn full_respawn_flow_installs_and_rotates_trust_via_driver_and_reconciler() {
    // Setup: two sessions S1 and S2 with independent trust stores.
    let trust_s1 = Arc::new(TrustStoreRecorder::default());
    let trust_s2 = Arc::new(TrustStoreRecorder::default());
    let trust_s1_new = Arc::new(TrustStoreRecorder::default());
    let reconcilers = BTreeMap::from([
        (
            "S1".to_string(),
            Arc::new(CommsTrustReconciler::new(trust_s1.clone() as _)),
        ),
        (
            "S2".to_string(),
            Arc::new(CommsTrustReconciler::new(trust_s2.clone() as _)),
        ),
        (
            "S1_new".to_string(),
            Arc::new(CommsTrustReconciler::new(trust_s1_new.clone() as _)),
        ),
    ]);

    let driver = RecomputeMobPeerOverlayDriver::new();

    // Phase 1: wire M1↔M2, bind to S1/S2, publish E1/E2.
    driver.observe_wire("M1".into(), "M2".into());
    driver.observe_binding_change("M1".into(), None, Some("S1".into()));
    driver.observe_binding_change("M2".into(), None, Some("S2".into()));
    driver.observe_local_endpoint("S1".into(), Some(endpoint("E1")));
    driver.observe_local_endpoint("S2".into(), Some(endpoint("E2")));
    dispatch_all(&driver, &reconcilers).await;

    assert_eq!(
        trust_s1.peer_ids(),
        BTreeSet::from(["ed25519:E2".to_string()]),
        "S1 trust store must contain E2's peer id after initial wiring",
    );
    assert_eq!(
        trust_s2.peer_ids(),
        BTreeSet::from(["ed25519:E1".to_string()]),
        "S2 trust store must contain E1's peer id by symmetry",
    );

    // Phase 2: M1 respawns from S1 → S1_new with a new endpoint E1'.
    driver.observe_binding_change("M1".into(), Some("S1".into()), Some("S1_new".into()));
    driver.observe_local_endpoint("S1_new".into(), Some(endpoint("E1_new")));
    driver.observe_local_endpoint("S1".into(), None); // archived session
    dispatch_all(&driver, &reconcilers).await;

    assert_eq!(
        trust_s1_new.peer_ids(),
        BTreeSet::from(["ed25519:E2".to_string()]),
        "S1' (new session) must trust E2 — M1 still wired to M2",
    );
    assert_eq!(
        trust_s2.peer_ids(),
        BTreeSet::from(["ed25519:E1_new".to_string()]),
        "S2 trust must rotate to E1_new — M2 still wired to M1 but M1 is now on a new session",
    );
    assert_eq!(
        trust_s1.peer_ids(),
        BTreeSet::new(),
        "S1 (archived) trust must be empty — session torn down",
    );

    // Phase 3: unwire M1↔M2. Both trust stores must clear.
    driver.observe_unwire("M1".into(), "M2".into());
    dispatch_all(&driver, &reconcilers).await;

    assert_eq!(
        trust_s1_new.peer_ids(),
        BTreeSet::new(),
        "S1' trust empties after unwire",
    );
    assert_eq!(
        trust_s2.peer_ids(),
        BTreeSet::new(),
        "S2 trust empties after unwire",
    );
}
