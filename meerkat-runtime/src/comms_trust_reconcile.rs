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
    ///
    /// Concurrency: two reconciles can race past the initial
    /// epoch check because `.await` on the trust-store calls
    /// releases the lock (we can't hold `std::sync::Mutex` across
    /// await points). The commit section re-checks the epoch
    /// under the same lock that will commit the applied view:
    /// if a concurrent reconcile advanced past our epoch while we
    /// were awaiting the trust store, we return a stale-report
    /// WITHOUT committing the applied view. This prevents a
    /// stale-but-inflight reconcile from rolling back trust state
    /// a newer reconcile just committed.
    ///
    /// Concurrency tail: the trust-store calls themselves (add/
    /// remove) may still race when two reconciles interleave —
    /// but `add_trusted_peer` / `remove_trusted_peer` are
    /// idempotent at the comms layer, so the worst case is
    /// redundant work. The critical invariant this code enforces
    /// is that the *applied-view watermark* never regresses.
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

        // Commit the new applied view — but under the same lock,
        // re-check that our epoch is still strictly-at-least the
        // currently-applied one. If a concurrent reconcile landed
        // an epoch > ours while we were awaiting the trust store,
        // we MUST NOT commit — our applied-view overwrite would
        // regress the watermark. The trust store ops we already
        // performed are idempotent (adds/removes on same peer_id
        // are no-ops), so skipping the commit is safe.
        let applied_epoch;
        {
            let mut guard = self
                .applied
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if epoch >= guard.epoch {
                guard.peers = effective_peers;
                guard.epoch = epoch;
                applied_epoch = epoch;
            } else {
                // Concurrent reconcile raced ahead — report the
                // current watermark, empty add/remove (our work was
                // either idempotent-no-op or racily observed as
                // applied by the newer reconcile; returning the
                // recorded changes would misleadingly attribute them
                // to this pass).
                applied_epoch = guard.epoch;
                return Ok(ReconcileReport {
                    added: Vec::new(),
                    removed: Vec::new(),
                    applied_epoch,
                });
            }
        }

        Ok(ReconcileReport {
            added,
            removed,
            applied_epoch,
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

    /// Regression for PR #340 review item #4 (TOCTOU race):
    /// simulates a slow "older" reconcile that starts first and
    /// finishes last. The newer reconcile must have committed an
    /// advanced epoch while the older one was awaiting the trust
    /// store; the older one must see the advanced epoch at commit
    /// time and skip its own commit to avoid regressing the
    /// applied-view watermark.
    ///
    /// Concrete scenario:
    ///   t0: reconcile(epoch=1, {A}) starts, reads applied_epoch=0.
    ///   t1: reconcile(epoch=2, {B}) runs to completion — commits
    ///       applied_epoch=2, applied={B}.
    ///   t2: reconcile(epoch=1, {A}) resumes from the await,
    ///       re-checks commit-time: its epoch=1 is now < applied
    ///       epoch=2, must NOT commit. Returns a stale report
    ///       (applied_epoch=2, added=[], removed=[]).
    ///
    /// Deterministic harness: a gated `CommsRuntime` that blocks
    /// `add_trusted_peer` on a channel; the test orchestrates the
    /// two reconciles' completion order.
    #[tokio::test]
    async fn concurrent_stale_reconcile_does_not_regress_applied_view() {
        use tokio::sync::oneshot;

        #[derive(Default)]
        struct GatedCommsRuntime {
            release_add: Mutex<Option<oneshot::Receiver<()>>>,
            recorded_adds: Mutex<Vec<TrustedPeerSpec>>,
        }

        impl std::fmt::Debug for GatedCommsRuntime {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("GatedCommsRuntime").finish()
            }
        }

        #[async_trait]
        impl CommsRuntime for GatedCommsRuntime {
            async fn drain_messages(&self) -> Vec<String> {
                Vec::new()
            }
            fn inbox_notify(&self) -> Arc<tokio::sync::Notify> {
                Arc::new(tokio::sync::Notify::new())
            }
            async fn add_trusted_peer(&self, peer: TrustedPeerSpec) -> Result<(), SendError> {
                let gate = self
                    .release_add
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(rx) = gate {
                    let _ = rx.await;
                }
                self.recorded_adds
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(peer);
                Ok(())
            }
            async fn remove_trusted_peer(&self, _peer_id: &str) -> Result<bool, SendError> {
                Ok(true)
            }
        }

        let (release_tx, release_rx) = oneshot::channel();
        let comms = Arc::new(GatedCommsRuntime {
            release_add: Mutex::new(Some(release_rx)),
            recorded_adds: Mutex::new(Vec::new()),
        });
        let reconciler = Arc::new(CommsTrustReconciler::new(comms.clone()));

        // Start the "older" reconcile (epoch=1). It will block on
        // the first `add_trusted_peer` call.
        let reconciler_1 = reconciler.clone();
        let older_handle = tokio::spawn(async move {
            reconciler_1
                .reconcile(1, BTreeSet::from([endpoint("A")]))
                .await
        });

        // Yield so the older reconcile's first add_trusted_peer
        // call reaches the gate. On a single-threaded executor
        // tokio::task::yield_now() is enough.
        tokio::task::yield_now().await;
        // Subsequent adds are ungated (release_add was taken
        // on the first call).
        let newer_report = reconciler
            .reconcile(2, BTreeSet::from([endpoint("B")]))
            .await
            .expect("newer reconcile commits");
        assert_eq!(newer_report.applied_epoch, 2);

        // Release the older reconcile so it can finish.
        let _ = release_tx.send(());
        let older_report = older_handle
            .await
            .expect("older task joins")
            .expect("older reconcile returns Ok");

        // TOCTOU gate: the older reconcile must NOT regress the
        // applied watermark. Its report reflects the newer epoch
        // applied by the racing reconcile.
        assert_eq!(
            older_report.applied_epoch, 2,
            "older reconcile's applied_epoch must reflect the newer commit, not its own epoch",
        );
        assert!(
            older_report.added.is_empty(),
            "older reconcile must not claim it added peers — its commit was skipped",
        );

        // Applied view persists the newer reconcile's peer set.
        let (applied_epoch, applied_peers) = reconciler.applied_snapshot();
        assert_eq!(applied_epoch, 2);
        assert_eq!(applied_peers, BTreeSet::from([endpoint("B")]));
    }
}
