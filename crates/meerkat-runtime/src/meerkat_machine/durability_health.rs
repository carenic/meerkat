//! Shared fail-closed health authority for one persistent runtime session.
//!
//! A persistence outcome that cannot be reconciled in place degrades the
//! shared handle to [`DurabilityHealthState::ReloadRequired`]. The handle has
//! deliberately no ordinary path back to Ready: the only capability that can
//! publish readiness is the non-cloneable
//! [`DurabilityRehydrationAuthority`] minted for one registration-authorized
//! cold install.

use std::sync::{Arc, Mutex};

/// Stable reason a persistent runtime must be cold-loaded again before it may
/// execute or mutate durable session state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("durability reload required after `{operation}`: {reason}")]
pub(crate) struct DurabilityReloadRequired {
    operation: String,
    reason: String,
}

impl DurabilityReloadRequired {
    fn new(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    pub(crate) fn operation(&self) -> &str {
        &self.operation
    }

    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }
}

/// Public projection of the shared durability gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DurabilityHealthState {
    /// Durable state and the live runtime shell are one verified image.
    Ready,
    /// The live shell may no longer be used; registration must cold-load a new
    /// image before execution can resume.
    ReloadRequired(DurabilityReloadRequired),
}

#[derive(Debug)]
struct DurabilityHealthRecord {
    state: DurabilityHealthState,
    /// True only between minting and consuming the one registration cold
    /// install capability. Any real degradation revokes it before replacing
    /// the provisional ReloadRequired reason.
    cold_install_open: bool,
    /// Set by the transition that performs a real degradation, while it holds
    /// this record's lock. The provisional cold-install state is not a
    /// degradation, so it never sets it.
    degraded: crate::tokio::sync::watch::Sender<bool>,
    /// Owned store writes (input admissions) whose caller stopped waiting at
    /// the store's commit outcome deadline and whose store has not answered
    /// yet. Each still holds the session's mutation gate.
    writes_past_deadline: usize,
}

/// Cloneable durability gate shared by the runtime entry and its persistent
/// driver.
///
/// The transition surface is intentionally asymmetric. Any owner may degrade
/// the shared session, while no handle clone can restore readiness.
#[derive(Debug, Clone)]
pub(crate) struct DurabilityHealthHandle {
    inner: Arc<Mutex<DurabilityHealthRecord>>,
}

impl DurabilityHealthHandle {
    /// Refuse execution or mutation unless registration published a verified
    /// cold-installed image and no later durability failure degraded it.
    pub(crate) fn require_ready(&self) -> Result<(), DurabilityReloadRequired> {
        let record = self.inner.lock().map_err(|_| {
            DurabilityReloadRequired::new(
                "durability_health_poison",
                "persistent health owner is not a verified image",
            )
        })?;
        match &record.state {
            DurabilityHealthState::Ready => Ok(()),
            DurabilityHealthState::ReloadRequired(required) => Err(required.clone()),
        }
    }

    /// Degrade this session exactly once. The first real failure is retained
    /// as the reload cause; later callers cannot overwrite its evidence.
    ///
    /// Returns `true` only for the caller that performed the Ready (or
    /// provisional cold-install) -> ReloadRequired transition.
    pub(crate) fn mark_reload_required(
        &self,
        operation: &'static str,
        reason: impl Into<String>,
    ) -> bool {
        let mut record = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !record.cold_install_open
            && matches!(&record.state, DurabilityHealthState::ReloadRequired(_))
        {
            return false;
        }
        record.state =
            DurabilityHealthState::ReloadRequired(DurabilityReloadRequired::new(operation, reason));
        record.cold_install_open = false;
        // Published under the record's lock: a waiter that subscribes and
        // reads the level under the same lock sees this transition either in
        // the level or on its receiver, never in neither.
        record.degraded.send_replace(true);
        true
    }

    /// Subscribe to this session's real degradation. The level and the
    /// receiver are read under the lock every degradation publishes under,
    /// so a degradation that happened before the call is reported as already
    /// fired and one after it is delivered: none falls between the read and
    /// the subscribe. The provisional cold-install state is not a
    /// degradation.
    pub(crate) fn subscribe_degradation(&self) -> DurabilityDegradation {
        let record = match self.inner.lock() {
            Ok(record) => record,
            Err(_) => {
                return DurabilityDegradation::Fired(DurabilityReloadRequired::new(
                    "durability_health_poison",
                    "persistent health owner is not a verified image",
                ));
            }
        };
        if *record.degraded.borrow()
            && let DurabilityHealthState::ReloadRequired(required) = &record.state
        {
            return DurabilityDegradation::Fired(required.clone());
        }
        DurabilityDegradation::Pending(DurabilityDegradationWatch {
            rx: record.degraded.subscribe(),
            handle: self.clone(),
        })
    }

    /// Begin an owned store write of this session that its caller waits for
    /// only until the store's commit outcome deadline (see
    /// [`OwnedStoreWrite`]).
    pub(crate) fn begin_owned_store_write(&self) -> OwnedStoreWrite {
        OwnedStoreWrite {
            state: Arc::new(Mutex::new(OwnedStoreWriteState::InFlight)),
            health: self.clone(),
        }
    }

    /// Whether an owned store write of this session is past its commit
    /// outcome deadline and its store has not answered: the write still
    /// holds the session's mutation gate.
    pub(crate) fn has_owned_write_past_deadline(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .writes_past_deadline
            > 0
    }

    /// The retained cause of a real degradation, or `None` while the session
    /// is Ready or still in its provisional cold install.
    fn degradation_cause(&self) -> Option<DurabilityReloadRequired> {
        let record = match self.inner.lock() {
            Ok(record) => record,
            Err(_) => {
                return Some(DurabilityReloadRequired::new(
                    "durability_health_poison",
                    "persistent health owner is not a verified image",
                ));
            }
        };
        match (&record.state, *record.degraded.borrow()) {
            (DurabilityHealthState::ReloadRequired(required), true) => Some(required.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    fn state(&self) -> DurabilityHealthState {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .state
            .clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnedStoreWriteState {
    /// The caller waits for the write.
    InFlight,
    /// The caller stopped waiting at the deadline; the store has not
    /// answered.
    PastDeadline,
    /// The store answered (or the write ended without an answer).
    Answered,
}

/// One owned store write of a persistent session (an input admission) that
/// its caller waits for only until the store's declared commit outcome
/// deadline. The write owns the session's mutation gate and keeps it until
/// the store answers. The caller's deadline and the write's answer settle
/// this record under one lock, so a write that answers as the deadline
/// passes is either taken by the caller or never counted as past it.
#[derive(Debug, Clone)]
pub(crate) struct OwnedStoreWrite {
    state: Arc<Mutex<OwnedStoreWriteState>>,
    health: DurabilityHealthHandle,
}

impl OwnedStoreWrite {
    /// The caller's deadline passed. Unless the write answered first, count
    /// it as past its deadline until it answers, so the reload-required
    /// discard refuses instead of waiting for the gate the write holds, and
    /// degrade the session with `operation` and `reason`. Returns `false`
    /// when the write had answered: the caller takes its answer.
    pub(crate) fn mark_past_deadline(
        &self,
        operation: &'static str,
        reason: impl Into<String>,
    ) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *state != OwnedStoreWriteState::InFlight {
            return false;
        }
        *state = OwnedStoreWriteState::PastDeadline;
        {
            let mut record = self
                .health
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            record.writes_past_deadline = record.writes_past_deadline.saturating_add(1);
        }
        self.health.mark_reload_required(operation, reason);
        true
    }

    /// Settle the write as answered once dropped: the write holds it across
    /// its store call and drops it before it hands the gate back, so a
    /// write that panics settles too.
    pub(crate) fn answer_on_drop(self) -> OwnedStoreWriteAnswer {
        OwnedStoreWriteAnswer(self)
    }

    fn answered(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *state == OwnedStoreWriteState::PastDeadline {
            let mut record = self
                .health
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            record.writes_past_deadline = record.writes_past_deadline.saturating_sub(1);
        }
        *state = OwnedStoreWriteState::Answered;
    }
}

/// Settles its [`OwnedStoreWrite`] as answered when dropped.
pub(crate) struct OwnedStoreWriteAnswer(OwnedStoreWrite);

impl Drop for OwnedStoreWriteAnswer {
    fn drop(&mut self) {
        self.0.answered();
    }
}

/// What [`DurabilityHealthHandle::subscribe_degradation`] found.
pub(crate) enum DurabilityDegradation {
    /// The session had already degraded: treat the signal as fired.
    Fired(DurabilityReloadRequired),
    /// Not degraded when subscribed; the watch resolves on the first real
    /// degradation.
    Pending(DurabilityDegradationWatch),
}

impl DurabilityDegradation {
    /// Resolve with the degradation's retained cause: at once when it had
    /// already happened, otherwise on the first real degradation.
    pub(crate) async fn fired(self) -> DurabilityReloadRequired {
        match self {
            Self::Fired(required) => required,
            Self::Pending(mut watch) => watch.degraded().await,
        }
    }
}

/// Resolves on a session's first real degradation.
pub(crate) struct DurabilityDegradationWatch {
    rx: crate::tokio::sync::watch::Receiver<bool>,
    handle: DurabilityHealthHandle,
}

impl DurabilityDegradationWatch {
    /// Wait for the first real degradation and return its retained cause.
    /// Pends forever when the health owner is gone without degrading.
    pub(crate) async fn degraded(&mut self) -> DurabilityReloadRequired {
        loop {
            if self.rx.wait_for(|degraded| *degraded).await.is_err() {
                std::future::pending::<()>().await;
            }
            if let Some(cause) = self.handle.degradation_cause() {
                return cause;
            }
        }
    }
}

/// Sole capability for publishing a persistent runtime as Ready.
///
/// This authority is private to the MeerkatMachine registration implementation
/// and is not Clone. Consuming it succeeds only while its exact cold install is
/// still open; a concurrent degradation revokes it permanently.
pub(super) struct DurabilityRehydrationAuthority {
    handle: DurabilityHealthHandle,
}

impl DurabilityRehydrationAuthority {
    pub(super) fn mark_ready(self) -> Result<(), DurabilityReloadRequired> {
        let mut record = self.handle.inner.lock().map_err(|_| {
            DurabilityReloadRequired::new(
                "durability_health_poison",
                "cold install cannot publish a poisoned health owner",
            )
        })?;
        if !record.cold_install_open {
            return match &record.state {
                DurabilityHealthState::Ready => Ok(()),
                DurabilityHealthState::ReloadRequired(required) => Err(required.clone()),
            };
        }
        record.state = DurabilityHealthState::Ready;
        record.cold_install_open = false;
        Ok(())
    }
}

/// Mint the health gate for one registration-authorized persistent cold
/// install. The returned handle remains fail-closed until the paired authority
/// is consumed after the entire install succeeds.
pub(super) fn begin_registration_cold_install()
-> (DurabilityHealthHandle, DurabilityRehydrationAuthority) {
    let handle = DurabilityHealthHandle {
        inner: Arc::new(Mutex::new(DurabilityHealthRecord {
            state: DurabilityHealthState::ReloadRequired(DurabilityReloadRequired::new(
                "registration_cold_install",
                "persistent runtime cold install has not published readiness",
            )),
            cold_install_open: true,
            degraded: crate::tokio::sync::watch::channel(false).0,
            writes_past_deadline: 0,
        })),
    };
    let authority = DurabilityRehydrationAuthority {
        handle: handle.clone(),
    };
    (handle, authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An owned store write past its deadline degrades the session and
    /// counts as past it until its store answers; one that answered first
    /// is the caller's to take and is never counted.
    #[test]
    fn an_owned_write_counts_past_its_deadline_only_until_it_answers() {
        let (health, rehydration) = begin_registration_cold_install();
        rehydration.mark_ready().unwrap();

        let write = health.begin_owned_store_write();
        let answer = write.clone().answer_on_drop();
        assert!(write.mark_past_deadline("runtime_admission_deadline", "no outcome"));
        assert!(health.has_owned_write_past_deadline());
        assert_eq!(
            health.require_ready().unwrap_err().operation(),
            "runtime_admission_deadline"
        );
        drop(answer);
        assert!(!health.has_owned_write_past_deadline());

        let (health, rehydration) = begin_registration_cold_install();
        rehydration.mark_ready().unwrap();
        let write = health.begin_owned_store_write();
        drop(write.clone().answer_on_drop());
        assert!(!write.mark_past_deadline("runtime_admission_deadline", "no outcome"));
        assert!(!health.has_owned_write_past_deadline());
        assert!(health.require_ready().is_ok());
    }

    #[test]
    fn cold_install_is_the_only_ready_transition_and_degradation_is_monotonic() {
        let (health, rehydration) = begin_registration_cold_install();
        assert!(health.require_ready().is_err());

        rehydration.mark_ready().unwrap();
        assert_eq!(health.state(), DurabilityHealthState::Ready);
        assert!(health.require_ready().is_ok());

        assert!(health.mark_reload_required("atomic_apply", "outcome unknown"));
        assert!(!health.mark_reload_required("later_retry", "must not replace first cause"));
        let required = health.require_ready().unwrap_err();
        assert_eq!(required.operation(), "atomic_apply");
        assert_eq!(required.reason(), "outcome unknown");
    }

    #[test]
    fn degradation_during_cold_install_revokes_rehydration_authority() {
        let (health, rehydration) = begin_registration_cold_install();
        assert!(health.mark_reload_required("recovery_cas", "durable image changed"));

        let error = rehydration.mark_ready().unwrap_err();
        assert_eq!(error.operation(), "recovery_cas");
        assert_eq!(health.require_ready().unwrap_err(), error);
    }
    #[test]
    fn poisoned_ready_owner_cannot_authorize_persistent_operations() {
        let (health, install) = begin_registration_cold_install();
        install.mark_ready().unwrap();
        let _ = std::panic::catch_unwind(|| {
            let _actual_owner = health.inner.lock().unwrap();
            panic!("fault while holding actual health owner");
        });
        assert!(
            health.require_ready().is_err(),
            "poison is not a verified Ready image"
        );
    }

    #[tokio::test]
    async fn the_degradation_signal_reaches_an_earlier_subscriber_and_reads_as_fired_after() {
        let (health, rehydration) = begin_registration_cold_install();
        rehydration.mark_ready().unwrap();
        let subscribed = health.subscribe_degradation();
        assert!(
            matches!(subscribed, DurabilityDegradation::Pending(_)),
            "a Ready session has not degraded"
        );
        assert!(health.mark_reload_required("atomic_apply", "outcome unknown"));
        let fired =
            crate::tokio::time::timeout(std::time::Duration::from_secs(5), subscribed.fired())
                .await
                .expect("a subscriber from before the degradation is woken by it");
        assert_eq!(fired.operation(), "atomic_apply");
        match health.subscribe_degradation() {
            DurabilityDegradation::Fired(required) => {
                assert_eq!(required.operation(), "atomic_apply");
            }
            DurabilityDegradation::Pending(_) => {
                panic!("a subscriber after the degradation reads it as already fired")
            }
        }
    }

    #[tokio::test]
    async fn the_provisional_cold_install_never_fires_the_degradation_signal() {
        let (health, rehydration) = begin_registration_cold_install();
        let subscribed = health.subscribe_degradation();
        assert!(
            matches!(subscribed, DurabilityDegradation::Pending(_)),
            "the provisional ReloadRequired of a cold install is not a degradation"
        );
        rehydration.mark_ready().unwrap();
        assert!(
            crate::tokio::time::timeout(std::time::Duration::from_millis(200), subscribed.fired(),)
                .await
                .is_err(),
            "publishing readiness is not a degradation"
        );
    }

    #[tokio::test]
    async fn a_real_degradation_during_the_cold_install_fires_the_signal() {
        let (health, _rehydration) = begin_registration_cold_install();
        let subscribed = health.subscribe_degradation();
        assert!(health.mark_reload_required("recovery_cas", "durable image changed"));
        let fired =
            crate::tokio::time::timeout(std::time::Duration::from_secs(5), subscribed.fired())
                .await
                .expect("a real degradation replaces the provisional state and fires");
        assert_eq!(fired.operation(), "recovery_cas");
    }

    #[test]
    fn poisoned_cold_owner_cannot_publish_readiness() {
        let (health, install) = begin_registration_cold_install();
        let _ = std::panic::catch_unwind(|| {
            let _actual_owner = health.inner.lock().unwrap();
            panic!("fault while holding actual cold-install owner");
        });
        assert!(
            install.mark_ready().is_err(),
            "poison invalidates the cold-install proof"
        );
        assert!(health.require_ready().is_err());
    }
}
