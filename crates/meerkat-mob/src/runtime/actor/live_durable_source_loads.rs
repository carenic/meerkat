//! Single flight for the live open's durable-source check.
//!
//! The actor validates a member's roster and session binding inline for
//! `ValidateLiveDurableSourceAvailability`, then checks the durable source off
//! the actor loop: the store's own observation that the exact session is
//! committed and not archived. The check reads no session body, so a run's
//! provisional tail standing ahead of the committed snapshot does not refuse
//! it. It still goes to the store, which can stall. Without sharing, every
//! concurrent open started its own check, and a caller that timed out left its
//! check running while the next caller started another.
//!
//! This registry keeps at most one check per session in flight. A caller that
//! arrives while a check runs waits on that check's result instead of starting
//! a second one; a caller that goes away leaves the one check running for the
//! others. A check still running at [`LIVE_DURABLE_SOURCE_LOAD_CEILING`] is
//! abandoned as temporarily unavailable, and its slot is released so a stalled
//! store cannot pin every later open to one hung read.
//!
//! This is actor-owned shell state for read sharing. It holds no MobMachine
//! fact: the roster and binding checks stay inline in the actor.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use meerkat_core::types::SessionId;
use tokio::sync::{oneshot, watch};

use super::super::LiveBridgeOperationStartError;
use super::super::session_service::{LiveDurableSourceObservation, MobSessionService};

/// Longest one shared durable-source check is driven before it is abandoned
/// as temporarily unavailable.
pub(in crate::runtime) const LIVE_DURABLE_SOURCE_LOAD_CEILING: Duration = Duration::from_secs(30);

type LoadResult = Result<(), LiveBridgeOperationStartError>;
type LoadSlot = watch::Receiver<Option<LoadResult>>;

/// Durable-source body loads in flight, at most one per session.
#[derive(Clone, Default)]
pub(in crate::runtime) struct LiveDurableSourceLoads(
    Arc<std::sync::Mutex<HashMap<SessionId, LoadSlot>>>,
);

impl LiveDurableSourceLoads {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, LoadSlot>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// How many body loads are running.
    #[cfg(test)]
    pub(in crate::runtime) fn in_flight(&self) -> usize {
        self.lock().len()
    }

    /// Answer `reply_tx` from the one body load of `session_id` in flight,
    /// starting that load when none is running. Never blocks the caller.
    pub(in crate::runtime) fn validate(
        &self,
        session_service: Arc<dyn MobSessionService>,
        session_id: SessionId,
        reply_tx: oneshot::Sender<LoadResult>,
    ) {
        let (mut result_rx, owner) = {
            let mut loads = self.lock();
            if let Some(slot) = loads.get(&session_id) {
                (slot.clone(), None)
            } else {
                let (result_tx, result_rx) = watch::channel(None);
                loads.insert(session_id.clone(), result_rx.clone());
                (result_rx, Some(result_tx))
            }
        };
        if let Some(result_tx) = owner {
            let slot = LoadSlotRelease {
                loads: self.clone(),
                session_id: session_id.clone(),
                result_rx: result_rx.clone(),
            };
            tokio::spawn(async move {
                let result = tokio::time::timeout(
                    LIVE_DURABLE_SOURCE_LOAD_CEILING,
                    load_durable_source(session_service.as_ref(), &slot.session_id),
                )
                .await
                .unwrap_or_else(|_elapsed| {
                    tracing::warn!(
                        session_id = %slot.session_id,
                        ceiling_ms = LIVE_DURABLE_SOURCE_LOAD_CEILING.as_millis() as u64,
                        "live durable-source check abandoned at its ceiling"
                    );
                    Err(LiveBridgeOperationStartError::TemporarilyUnavailable)
                });
                // Release the slot before publishing: every caller that
                // joined already holds the result channel, and a caller that
                // arrives after this load settled starts a fresh one rather
                // than reading a finished result.
                drop(slot);
                result_tx.send_replace(Some(result));
            });
        }
        tokio::spawn(async move {
            let result = match result_rx.wait_for(Option::is_some).await {
                Ok(result) => result
                    .clone()
                    .unwrap_or(Err(LiveBridgeOperationStartError::Unavailable)),
                // The owning task went away without a result (actor exit).
                Err(_) => Err(LiveBridgeOperationStartError::Unavailable),
            };
            let _ = reply_tx.send(result);
        });
    }
}

/// Release of one load's registry slot, on completion or when the owning task
/// is dropped. A successor load's slot is never removed.
struct LoadSlotRelease {
    loads: LiveDurableSourceLoads,
    session_id: SessionId,
    result_rx: LoadSlot,
}

impl Drop for LoadSlotRelease {
    fn drop(&mut self) {
        let mut loads = self.loads.lock();
        if loads
            .get(&self.session_id)
            .is_some_and(|slot| slot.same_channel(&self.result_rx))
        {
            loads.remove(&self.session_id);
        }
    }
}

/// The durable-source check itself: the exact session is committed and not
/// archived. Body-free (the store's own observation), so a run whose
/// provisional tail stands ahead of the committed snapshot does not reject
/// it.
async fn load_durable_source(
    session_service: &dyn MobSessionService,
    session_id: &SessionId,
) -> LoadResult {
    match session_service
        .observe_live_durable_source(session_id)
        .await
    {
        Ok(LiveDurableSourceObservation::Committed { .. }) => Ok(()),
        Ok(_) | Err(_) => Err(LiveBridgeOperationStartError::Rejected),
    }
}
