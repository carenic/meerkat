//! Comms trust reconciliation handler.
//!
//! Track-B (R5) Commit 4: mechanically reconciles a
//! `meerkat-comms` trust store against the effective peer set
//! declared by the MeerkatMachine DSL.
//!
//! Ownership split:
//!
//! * The DSL (`meerkat-machine-schema::catalog::dsl::meerkat_machine`)
//!   owns the declarative facts:
//!   `direct_peer_endpoints`, `mob_overlay_peer_endpoints`,
//!   `peer_projection_epoch`.
//! * The `CommsTrustReconcileRequested` effect fires whenever the
//!   effective set could have changed. The effect carries only the
//!   post-transition `peer_projection_epoch` — the DSL emits the fact
//!   "reconcile needed at epoch N", the shell does the mechanical
//!   diff.
//! * This handler owns the mechanical reconciliation: it maintains an
//!   "applied trust store" view and, given a fresh effective peer
//!   set, computes the add / remove delta and calls
//!   `CommsRuntime::add_trusted_peer` / `remove_trusted_peer`
//!   mechanically. No semantic decisions live here; failures surface
//!   through typed errors.
//!
//! The handler is stateless about the DSL — it does not read machine
//! snapshots directly. Callers supply the effective peer set
//! (`direct ∪ overlay`) as a single `BTreeSet<PeerEndpoint>` from
//! the DSL's state snapshot.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;

use meerkat_core::agent::CommsRuntime;
use meerkat_core::comms::{SendError, TrustedPeerSpec};

use crate::meerkat_machine::dsl::PeerEndpoint;

/// Typed error surfaced by the reconciliation handler.
#[derive(Debug, thiserror::Error)]
pub enum CommsTrustReconcileError {
    /// `add_trusted_peer` failed at the trust store. The handler
    /// surfaces the underlying `SendError` verbatim.
    #[error("add_trusted_peer for `{peer_id}` failed: {source}")]
    AddTrustFailed {
        peer_id: String,
        #[source]
        source: SendError,
    },
    /// `remove_trusted_peer` failed at the trust store.
    #[error("remove_trusted_peer for `{peer_id}` failed: {source}")]
    RemoveTrustFailed {
        peer_id: String,
        #[source]
        source: SendError,
    },
}

/// Structured summary of a single reconciliation pass.
///
/// Tests and observability consumers read this to understand what
/// the reconciler actually asked of the trust store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Peers that were newly registered on this pass.
    pub added: Vec<PeerEndpoint>,
    /// Peers that were unregistered on this pass.
    pub removed: Vec<PeerEndpoint>,
    /// Epoch watermark the reconciler applied.
    pub applied_epoch: u64,
}

/// Mechanical trust reconciliation handler.
///
/// Holds a reference to a `CommsRuntime` and an applied view the
/// reconciler uses to compute deltas. Thread-safe; the applied
/// view is guarded by a single `Mutex`.
pub struct CommsTrustReconciler {
    comms: Arc<dyn CommsRuntime>,
    applied: Mutex<AppliedView>,
}

impl std::fmt::Debug for CommsTrustReconciler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn CommsRuntime` does not implement `Debug`; print a
        // structural placeholder with the applied-view summary.
        let applied = self
            .applied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f.debug_struct("CommsTrustReconciler")
            .field("applied_epoch", &applied.epoch)
            .field("applied_peer_count", &applied.peers.len())
            .finish()
    }
}

#[derive(Debug, Default)]
struct AppliedView {
    /// Peers currently registered in the trust store (per the
    /// reconciler's view; failures are self-correcting on the next
    /// pass).
    peers: BTreeSet<PeerEndpoint>,
    /// Highest epoch applied so far. Reconciles with strictly-lower
    /// epoch are rejected to prevent out-of-order deliveries
    /// regressing the trust view.
    epoch: u64,
}

impl CommsTrustReconciler {
    /// Construct a reconciler bound to the given comms runtime.
    /// Initial applied view is empty at epoch 0.
    pub fn new(comms: Arc<dyn CommsRuntime>) -> Self {
        Self {
            comms,
            applied: Mutex::new(AppliedView::default()),
        }
    }

    /// Reconcile the trust store against `effective_peers`.
    ///
    /// Returns a [`ReconcileReport`] describing the add / remove
    /// calls the reconciler made. Out-of-order epochs (epoch <
    /// currently-applied) return `Ok` with an empty report — the
    /// reconciler treats them as stale.
    ///
    /// Trust-store failures surface as
    /// [`CommsTrustReconcileError`]; the applied view is only
    /// advanced for peers the trust store acknowledged, so failed
    /// adds/removes are retried on the next pass.
    pub async fn reconcile(
        &self,
        epoch: u64,
        effective_peers: BTreeSet<PeerEndpoint>,
    ) -> Result<ReconcileReport, CommsTrustReconcileError> {
        // Lock the applied view to snapshot the current state. We
        // release the lock before the async trust-store calls to
        // avoid holding a `std::sync::Mutex` across await points.
        let (previous_peers, previous_epoch) = {
            let guard = self
                .applied
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (guard.peers.clone(), guard.epoch)
        };

        if epoch < previous_epoch {
            // Stale delivery — silently accept the no-op. The caller
            // can observe by checking `applied_epoch` in the report.
            return Ok(ReconcileReport {
                added: Vec::new(),
                removed: Vec::new(),
                applied_epoch: previous_epoch,
            });
        }

        let to_add: Vec<PeerEndpoint> = effective_peers
            .iter()
            .filter(|ep| !previous_peers.contains(*ep))
            .cloned()
            .collect();
        let to_remove: Vec<PeerEndpoint> = previous_peers
            .iter()
            .filter(|ep| !effective_peers.contains(*ep))
            .cloned()
            .collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();

        // Perform adds first so a concurrent peer-send from the same
        // session sees the new peer available even if a remove for
        // an older session hasn't completed.
        for endpoint in to_add {
            let spec = TrustedPeerSpec {
                name: endpoint.name.clone(),
                peer_id: endpoint.peer_id.clone(),
                address: endpoint.address.clone(),
            };
            self.comms.add_trusted_peer(spec).await.map_err(|source| {
                CommsTrustReconcileError::AddTrustFailed {
                    peer_id: endpoint.peer_id.clone(),
                    source,
                }
            })?;
            added.push(endpoint);
        }

        for endpoint in to_remove {
            self.comms
                .remove_trusted_peer(&endpoint.peer_id)
                .await
                .map_err(|source| CommsTrustReconcileError::RemoveTrustFailed {
                    peer_id: endpoint.peer_id.clone(),
                    source,
                })?;
            removed.push(endpoint);
        }

        // Commit the new applied view.
        {
            let mut guard = self
                .applied
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.peers = effective_peers;
            guard.epoch = epoch;
        }

        Ok(ReconcileReport {
            added,
            removed,
            applied_epoch: epoch,
        })
    }

    /// Snapshot of the reconciler's current applied view (tests).
    #[cfg(test)]
    pub(crate) fn applied_snapshot(&self) -> (u64, BTreeSet<PeerEndpoint>) {
        let guard = self
            .applied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (guard.epoch, guard.peers.clone())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn endpoint(name: &str) -> PeerEndpoint {
        PeerEndpoint {
            name: format!("ep-{name}"),
            peer_id: format!("ed25519:{name}"),
            address: format!("inproc://{name}"),
        }
    }

    /// Records every `add_trusted_peer` / `remove_trusted_peer` call
    /// for assertion in tests.
    #[derive(Default)]
    struct RecordingCommsRuntime {
        adds: Mutex<Vec<TrustedPeerSpec>>,
        removes: Mutex<Vec<String>>,
        fail_next_add: AtomicBool,
        fail_next_remove: AtomicBool,
    }

    impl std::fmt::Debug for RecordingCommsRuntime {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RecordingCommsRuntime").finish()
        }
    }

    #[async_trait]
    impl CommsRuntime for RecordingCommsRuntime {
        async fn drain_messages(&self) -> Vec<String> {
            Vec::new()
        }

        fn inbox_notify(&self) -> Arc<tokio::sync::Notify> {
            Arc::new(tokio::sync::Notify::new())
        }

        async fn add_trusted_peer(&self, peer: TrustedPeerSpec) -> Result<(), SendError> {
            if self.fail_next_add.swap(false, Ordering::SeqCst) {
                return Err(SendError::Unsupported("synthetic failure".into()));
            }
            self.adds
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(peer);
            Ok(())
        }

        async fn remove_trusted_peer(&self, peer_id: &str) -> Result<bool, SendError> {
            if self.fail_next_remove.swap(false, Ordering::SeqCst) {
                return Err(SendError::Unsupported("synthetic failure".into()));
            }
            self.removes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(peer_id.to_string());
            Ok(true)
        }
    }

    impl RecordingCommsRuntime {
        fn add_calls(&self) -> Vec<TrustedPeerSpec> {
            self.adds
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn remove_calls(&self) -> Vec<String> {
            self.removes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    #[tokio::test]
    async fn first_reconcile_registers_all_effective_peers() {
        let comms = Arc::new(RecordingCommsRuntime::default());
        let reconciler = CommsTrustReconciler::new(comms.clone());
        let peers = BTreeSet::from([endpoint("A"), endpoint("B")]);

        let report = reconciler
            .reconcile(1, peers.clone())
            .await
            .expect("first reconcile succeeds");

        assert_eq!(report.applied_epoch, 1);
        assert_eq!(report.added.len(), 2);
        assert!(report.removed.is_empty());

        let add_calls = comms.add_calls();
        assert_eq!(add_calls.len(), 2);
        assert_eq!(comms.remove_calls().len(), 0);
        assert!(
            add_calls.iter().any(|spec| spec.peer_id == "ed25519:A"
                && spec.name == "ep-A"
                && spec.address == "inproc://A"),
            "add_trusted_peer must be called with the canonical (name, peer_id, address) triple",
        );
    }

    #[tokio::test]
    async fn subsequent_reconcile_adds_new_and_removes_departed() {
        let comms = Arc::new(RecordingCommsRuntime::default());
        let reconciler = CommsTrustReconciler::new(comms.clone());

        let peers_v1 = BTreeSet::from([endpoint("A"), endpoint("B")]);
        reconciler
            .reconcile(1, peers_v1)
            .await
            .expect("v1 reconcile");

        let peers_v2 = BTreeSet::from([endpoint("A"), endpoint("C")]);
        let report = reconciler
            .reconcile(2, peers_v2)
            .await
            .expect("v2 reconcile");

        // Added: C. Removed: B. Retained: A.
        assert_eq!(report.added, vec![endpoint("C")]);
        assert_eq!(report.removed, vec![endpoint("B")]);
        assert_eq!(report.applied_epoch, 2);

        // Trust-store calls: v1 {A,B} adds, v2 {C} add + {B} remove.
        // Across both passes the add calls are A, B, C and the remove
        // calls are B.
        assert_eq!(comms.add_calls().len(), 3);
        assert_eq!(comms.remove_calls(), vec!["ed25519:B"]);
    }

    #[tokio::test]
    async fn stale_epoch_reconcile_is_accepted_as_no_op() {
        let comms = Arc::new(RecordingCommsRuntime::default());
        let reconciler = CommsTrustReconciler::new(comms.clone());

        reconciler
            .reconcile(5, BTreeSet::from([endpoint("A")]))
            .await
            .expect("first reconcile");

        let report = reconciler
            .reconcile(4, BTreeSet::from([endpoint("B")]))
            .await
            .expect("stale reconcile accepted");
        assert!(report.added.is_empty());
        assert!(report.removed.is_empty());
        assert_eq!(
            report.applied_epoch, 5,
            "applied_epoch must remain at the newer watermark",
        );

        // Only the first reconcile's add made it to the trust store.
        assert_eq!(comms.add_calls().len(), 1);
        assert_eq!(comms.remove_calls().len(), 0);
    }

    #[tokio::test]
    async fn add_failure_surfaces_typed_error_and_does_not_update_applied_view() {
        let comms = Arc::new(RecordingCommsRuntime::default());
        comms.fail_next_add.store(true, Ordering::SeqCst);
        let reconciler = CommsTrustReconciler::new(comms.clone());

        let err = reconciler
            .reconcile(1, BTreeSet::from([endpoint("A")]))
            .await
            .expect_err("add_trust failure must surface");
        match err {
            CommsTrustReconcileError::AddTrustFailed { peer_id, .. } => {
                assert_eq!(peer_id, "ed25519:A");
            }
            other => panic!("expected AddTrustFailed, got {other:?}"),
        }
        let (epoch, applied) = reconciler.applied_snapshot();
        assert_eq!(
            epoch, 0,
            "applied epoch must NOT advance past a failed reconcile",
        );
        assert!(applied.is_empty());
    }

    #[tokio::test]
    async fn empty_effective_set_clears_all_previously_trusted_peers() {
        let comms = Arc::new(RecordingCommsRuntime::default());
        let reconciler = CommsTrustReconciler::new(comms.clone());

        reconciler
            .reconcile(1, BTreeSet::from([endpoint("A"), endpoint("B")]))
            .await
            .expect("seed");

        let report = reconciler
            .reconcile(2, BTreeSet::new())
            .await
            .expect("clear all");
        assert_eq!(report.removed.len(), 2);
        assert!(report.added.is_empty());

        let mut removes = comms.remove_calls();
        removes.sort();
        assert_eq!(removes, vec!["ed25519:A", "ed25519:B"]);
    }
}
