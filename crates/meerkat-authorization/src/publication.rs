//! Coherent local publication for disposable authorization projections.
//!
//! This is synchronization, not a policy store or grant authority. The host
//! composes every relevant owner mutation through one publication instance:
//! policy, identity, grant ancestors, resource labels, destinations and route
//! accounts. The owner still decides and persists each actual change.
//!
//! Acquire the publication guard before owner write guards, and retain it until
//! the accepted facts are locally visible. Never hold it across network I/O.
//! A retained projection cannot be retagged; a changed stamp requires rebuilding
//! from its owners. A missed owner mutation is a composition defect, not a
//! freshness claim that this helper can repair.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering, fence};
use std::sync::{Arc, Mutex, MutexGuard};

const INVALID: u64 = u64::MAX;

struct PublicationInner {
    instance: uuid::Uuid,
    sequence: AtomicU64,
    writer: Mutex<()>,
}

/// Process-local ordering shared by all authorization-relevant fact owners.
///
/// There is no serialization or recovery constructor. Restart reconstructs
/// decisions from retained owner facts under a new publication instance.
#[derive(Clone)]
pub struct LocalAuthorizationPublication {
    inner: Arc<PublicationInner>,
}

impl fmt::Debug for LocalAuthorizationPublication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalAuthorizationPublication")
            .finish_non_exhaustive()
    }
}

impl Default for LocalAuthorizationPublication {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalAuthorizationPublication {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PublicationInner {
                instance: uuid::Uuid::new_v4(),
                sequence: AtomicU64::new(0),
                writer: Mutex::new(()),
            }),
        }
    }

    /// Start publishing an owner-authorized local change. This never authorizes
    /// the mutation and does not acquire any owner-specific state or store lock.
    ///
    /// # Errors
    /// A poisoned or exhausted publication cannot issue fresh projections.
    pub fn begin_owner_change(&self) -> Result<LocalPublicationGuard<'_>, PublicationError> {
        let mut guard = self.reserve_owner_change()?;
        guard.publish();
        Ok(guard)
    }

    /// [`Self::begin_owner_change`] without waiting, for a synchronous step
    /// that holds native custody it must not wait under.
    ///
    /// Take this writer first, then try-lock the owner. When the owner is
    /// contended, change no facts and drop the guard: dropping it advances the
    /// sequence, which conservatively invalidates every disposable observation
    /// without claiming that a change happened. Retry by rebuilding from the
    /// actual owners; never revalidate an observation made before the guard.
    ///
    /// # Errors
    /// Writer contention or poison is `Unavailable` before any mutation and
    /// leaves publication and every stamp unchanged. A poisoned or exhausted
    /// publication stays unavailable, as for [`Self::begin_owner_change`].
    pub fn try_begin_owner_change(&self) -> Result<LocalPublicationGuard<'_>, PublicationError> {
        let mut guard = self.try_reserve_owner_change()?;
        guard.publish();
        Ok(guard)
    }

    /// Reserve the writer without invalidating observations yet. This private
    /// path is only for a mutation whose complete facts stay inaccessible under
    /// one owner mutex until `publish` is called. Validate exhaustion before any
    /// mutation; after successful apply, publication cannot fail.
    ///
    /// Lock order remains publication writer, then owner. A rejected canonical
    /// apply may drop the reservation unchanged only when it changed no facts.
    pub(crate) fn reserve_owner_change(
        &self,
    ) -> Result<LocalPublicationGuard<'_>, PublicationError> {
        let writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| PublicationError::Unavailable)?;
        self.reservation_with_writer(writer)
    }

    /// Reserve without waiting while native custody remains held. Contention
    /// or poison returns before mutation and leaves publication unchanged.
    pub(crate) fn try_reserve_owner_change(
        &self,
    ) -> Result<LocalPublicationGuard<'_>, PublicationError> {
        let writer = self
            .inner
            .writer
            .try_lock()
            .map_err(|_| PublicationError::Unavailable)?;
        self.reservation_with_writer(writer)
    }

    fn reservation_with_writer<'a>(
        &'a self,
        writer: MutexGuard<'a, ()>,
    ) -> Result<LocalPublicationGuard<'a>, PublicationError> {
        let previous = self.inner.sequence.load(Ordering::Acquire);
        if previous >= INVALID - 2 || !previous.is_multiple_of(2) {
            self.inner.sequence.store(INVALID, Ordering::Release);
            return Err(PublicationError::Unavailable);
        }
        Ok(LocalPublicationGuard {
            inner: &self.inner,
            completed_sequence: previous + 2,
            published: false,
            _writer: writer,
        })
    }

    /// Permanently retire this publication before reconstructing its owners.
    /// Taking the actual writer lock waits for any older guard's final store;
    /// no guard can subsequently overwrite retirement with a healthy sequence.
    /// Poison is retained. This never resets or reuses a publication instance.
    pub(crate) fn retire(&self) {
        let _writer = match self.inner.writer.lock() {
            Ok(writer) => writer,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.inner.sequence.store(INVALID, Ordering::Release);
    }

    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Observe the current owners and bind that disposable view to one coherent
    /// publication. The closure must read real owner data or their immutable
    /// snapshots, never a separately refreshed policy cache. It must not do I/O.
    ///
    /// # Errors
    /// A concurrent owner mutation invalidates the observation. The caller may
    /// rebuild once; it must not relabel old facts with a newer stamp.
    pub fn observe<T>(
        &self,
        observe_owners: impl FnOnce() -> T,
    ) -> Result<(T, LocalPublicationStamp), PublicationError> {
        let sequence = self.inner.sequence.load(Ordering::Acquire);
        if sequence == INVALID {
            return Err(PublicationError::Unavailable);
        }
        if !sequence.is_multiple_of(2) {
            return Err(PublicationError::Changed);
        }
        let value = observe_owners();
        let stamp = LocalPublicationStamp {
            inner: Arc::clone(&self.inner),
            sequence,
        };
        // Complete the owner reads before validating the enclosing sequence.
        fence(Ordering::Acquire);
        stamp.check_current()?;
        Ok((value, stamp))
    }
}

/// Only holds publication custody; all semantic change stays with its owner.
pub struct LocalPublicationGuard<'a> {
    inner: &'a PublicationInner,
    completed_sequence: u64,
    published: bool,
    _writer: MutexGuard<'a, ()>,
}

impl LocalPublicationGuard<'_> {
    /// Infallible after reservation. For the deferred path this is the mutation
    /// linearization point: canonical apply has succeeded, but the actual owner
    /// lock must still hide its facts. Keep that lock until this returns.
    pub(crate) fn publish(&mut self) {
        if !self.published {
            self.inner
                .sequence
                .swap(self.completed_sequence - 1, Ordering::AcqRel);
            self.published = true;
        }
    }

    /// Whether `stamp` was observed on this same publication and no owner
    /// change has published since. [`LocalPublicationStamp::check_current`]
    /// alone compares only with the stamp's own publication, so a stamp of
    /// another publication must be refused here first. No other owner change
    /// can publish while this reservation is held, so the answer holds until
    /// this guard publishes or drops.
    pub(crate) fn stamp_is_current(&self, stamp: &LocalPublicationStamp) -> bool {
        std::ptr::eq(self.inner, Arc::as_ptr(&stamp.inner)) && stamp.check_current().is_ok()
    }
}

impl Drop for LocalPublicationGuard<'_> {
    fn drop(&mut self) {
        // Even a deferred reservation poisons publication on panic: the owner
        // may have been partially mutated. Ordinary rejected inputs changed no
        // state and leave the sequence intact.
        if std::thread::panicking() {
            self.inner.sequence.store(INVALID, Ordering::Release);
        } else if self.published {
            self.inner
                .sequence
                .store(self.completed_sequence, Ordering::Release);
        }
    }
}

/// Immutable, nonserializable stamp for a rebuilt view of local owner facts.
/// It is not an authorization decision and contains no policy or resource data.
#[derive(Clone)]
pub struct LocalPublicationStamp {
    inner: Arc<PublicationInner>,
    sequence: u64,
}

impl fmt::Debug for LocalPublicationStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPublicationStamp")
            .finish_non_exhaustive()
    }
}

impl LocalPublicationStamp {
    /// Historical data from this exact coherent read, never the latest counter.
    pub(crate) fn policy_observation(
        &self,
    ) -> meerkat_core::authorization::PolicyPublicationObservation {
        meerkat_core::authorization::PolicyPublicationObservation::LocalPublication {
            instance: self.inner.instance,
            sequence: self.observation_sequence(),
        }
    }

    /// Historical process-local diagnostic data only. Not a permission,
    /// durable policy epoch, or cross-process freshness proof.
    pub(crate) fn observation_sequence(&self) -> u64 {
        self.sequence
    }

    /// One allocation-free local atomic read at the final operation boundary.
    /// The prepared operation must separately retain its exact binding and
    /// deadline. No awaited preparation may follow the final entry check.
    ///
    /// # Errors
    /// Any publication since observation, including a writer in progress,
    /// requires a new owner observation. Poison is permanently unavailable.
    pub fn check_current(&self) -> Result<(), PublicationError> {
        match self.inner.sequence.load(Ordering::Acquire) {
            INVALID => Err(PublicationError::Unavailable),
            current if current == self.sequence => Ok(()),
            _ => Err(PublicationError::Changed),
        }
    }
}

/// Human-consent approver eligibility as of one coherent publication of the
/// host's eligibility owner facts.
///
/// The approval owner checks it again when a consent decision commits, with
/// the same allocation-free atomic read as an operation's final entry check:
/// any owner change published since the observation (a revocation, a role
/// change) refuses the decision. It is evidence of eligibility, never an
/// authentication: the approver is still the host's authenticated assertion.
pub struct PublishedConsentEligibility {
    stamp: LocalPublicationStamp,
}

impl PublishedConsentEligibility {
    /// Observe the eligibility owner under `publication`. `eligible` must
    /// read the owner's real facts for the exact approver and approval,
    /// without I/O. An ineligible approver, or an owner change during the
    /// observation, yields no evidence.
    ///
    /// # Errors
    /// [`meerkat_core::approval::consent::ConsentIneligible`] when there is
    /// no current evidence of eligibility.
    pub fn observe(
        publication: &LocalAuthorizationPublication,
        eligible: impl FnOnce() -> bool,
    ) -> Result<
        Arc<dyn meerkat_core::approval::consent::ConsentEligibility>,
        meerkat_core::approval::consent::ConsentIneligible,
    > {
        match publication.observe(eligible) {
            Ok((true, stamp)) => Ok(Arc::new(Self { stamp })),
            Ok((false, _)) | Err(_) => Err(meerkat_core::approval::consent::ConsentIneligible),
        }
    }
}

impl fmt::Debug for PublishedConsentEligibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublishedConsentEligibility")
            .finish_non_exhaustive()
    }
}

impl meerkat_core::approval::consent::ConsentEligibility for PublishedConsentEligibility {
    fn check_current(&self) -> Result<(), meerkat_core::approval::consent::ConsentIneligible> {
        self.stamp
            .check_current()
            .map_err(|_| meerkat_core::approval::consent::ConsentIneligible)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PublicationError {
    #[error("local authorization facts changed")]
    Changed,
    #[error("local authorization facts are unavailable")]
    Unavailable,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn publication_invalidates_before_owner_change_and_cannot_retag_old_view() {
        let publication = LocalAuthorizationPublication::new();
        let (value, original) = publication.observe(|| 7).expect("initial view");
        assert_eq!(value, 7);
        assert_eq!(original.check_current(), Ok(()));
        let change = publication.begin_owner_change().expect("owner publication");
        assert_eq!(original.check_current(), Err(PublicationError::Changed));
        assert!(matches!(
            publication.observe(|| 8),
            Err(PublicationError::Changed)
        ));
        drop(change);
        assert_eq!(original.check_current(), Err(PublicationError::Changed));
        let (_, rebuilt) = publication.observe(|| 8).expect("new owner view");
        assert_eq!(rebuilt.check_current(), Ok(()));
    }

    #[test]
    fn historical_observation_retains_its_instance_and_captured_sequence() {
        let publication = LocalAuthorizationPublication::new();
        let ((), original) = publication.observe(|| ()).expect("initial view");
        let before = original.policy_observation();
        drop(publication.begin_owner_change().expect("owner change"));
        let ((), refreshed) = publication.observe(|| ()).expect("fresh view");
        assert_eq!(original.policy_observation(), before);
        assert_ne!(refreshed.policy_observation(), before);
        let other = LocalAuthorizationPublication::new();
        let ((), same_sequence) = other.observe(|| ()).expect("other publication");
        assert_eq!(
            original.observation_sequence(),
            same_sequence.observation_sequence()
        );
        assert_ne!(same_sequence.policy_observation(), before);
    }

    #[test]
    fn owner_change_during_preparation_refuses_mixed_snapshot() {
        let publication = LocalAuthorizationPublication::new();
        let observed = publication.observe(|| {
            let change = publication.begin_owner_change().expect("concurrent change");
            drop(change);
            11
        });
        assert!(matches!(observed, Err(PublicationError::Changed)));
    }

    #[test]
    #[allow(clippy::panic)] // Deliberately inject a partial owner-mutation panic.
    fn panic_during_owner_mutation_never_publishes_partial_facts() {
        let publication = LocalAuthorizationPublication::new();
        let ((), stamp) = publication.observe(|| ()).expect("initial stamp");
        let result = std::panic::catch_unwind(|| {
            let _change = publication.begin_owner_change().expect("owner change");
            panic!("simulated partial owner mutation");
        });
        assert!(result.is_err());
        assert_eq!(stamp.check_current(), Err(PublicationError::Unavailable));
        assert!(matches!(
            publication.observe(|| ()),
            Err(PublicationError::Unavailable)
        ));
        assert!(matches!(
            publication.begin_owner_change(),
            Err(PublicationError::Unavailable)
        ));
    }

    #[test]
    fn a_contended_try_begin_changes_nothing_and_keeps_the_stamp() {
        let publication = LocalAuthorizationPublication::new();
        let ((), stamp) = publication.observe(|| ()).expect("initial stamp");
        let reservation = publication
            .try_reserve_owner_change()
            .expect("another owner holds the writer");
        assert!(matches!(
            publication.try_begin_owner_change(),
            Err(PublicationError::Unavailable)
        ));
        assert_eq!(stamp.check_current(), Ok(()));
        drop(reservation);
        assert_eq!(stamp.check_current(), Ok(()));
        let ((), unchanged) = publication.observe(|| ()).expect("nothing was published");
        assert_eq!(
            unchanged.observation_sequence(),
            stamp.observation_sequence()
        );
    }

    #[test]
    fn a_contended_source_owner_keeps_its_facts_and_invalidates_the_stamp() {
        let publication = LocalAuthorizationPublication::new();
        let owner = Mutex::new("not admitted");
        let (facts, stamp) = publication
            .observe(|| *owner.lock().expect("owner"))
            .expect("initial view");
        let busy = owner.lock().expect("the owner is busy elsewhere");
        let change = publication
            .try_begin_owner_change()
            .expect("the writer is free");
        assert!(owner.try_lock().is_err(), "the source owner is contended");
        drop(change);
        drop(busy);
        assert_eq!(*owner.lock().expect("owner"), facts, "no fact changed");
        assert_eq!(stamp.check_current(), Err(PublicationError::Changed));

        let (rebuilt, retry_stamp) = publication
            .observe(|| *owner.lock().expect("owner"))
            .expect("a retry rebuilds from the actual owner");
        assert_eq!(rebuilt, facts);
        let change = publication
            .try_begin_owner_change()
            .expect("the writer is free");
        *owner.try_lock().expect("the source owner is free") = "admitted";
        drop(change);
        assert_eq!(retry_stamp.check_current(), Err(PublicationError::Changed));
        let (after, current) = publication
            .observe(|| *owner.lock().expect("owner"))
            .expect("the retry published");
        assert_eq!(after, "admitted");
        assert_eq!(current.check_current(), Ok(()));
    }

    #[test]
    fn a_reservation_admits_only_a_current_stamp_of_its_own_publication() {
        let publication = LocalAuthorizationPublication::new();
        let ((), stale) = publication.observe(|| ()).expect("initial stamp");
        drop(publication.begin_owner_change().expect("owner change"));
        let ((), current) = publication.observe(|| ()).expect("current stamp");
        let unrelated = LocalAuthorizationPublication::new();
        let ((), foreign) = unrelated.observe(|| ()).expect("unrelated stamp");
        assert_eq!(foreign.check_current(), Ok(()), "current on its own");

        let reservation = publication
            .try_reserve_owner_change()
            .expect("the writer is free");
        assert!(reservation.stamp_is_current(&current));
        assert!(!reservation.stamp_is_current(&stale));
        assert!(!reservation.stamp_is_current(&foreign));
        drop(reservation);
        assert_eq!(current.check_current(), Ok(()), "nothing was published");
    }

    #[test]
    fn a_successful_try_begin_publishes_coherently() {
        let publication = LocalAuthorizationPublication::new();
        let owner = Mutex::new("before");
        let (_, before) = publication
            .observe(|| *owner.lock().expect("owner"))
            .expect("initial view");
        let change = publication
            .try_begin_owner_change()
            .expect("the writer is free");
        assert_eq!(before.check_current(), Err(PublicationError::Changed));
        assert!(matches!(
            publication.observe(|| *owner.lock().expect("owner")),
            Err(PublicationError::Changed)
        ));
        *owner.try_lock().expect("the source owner is free") = "after";
        drop(change);
        let (after, stamp) = publication
            .observe(|| *owner.lock().expect("owner"))
            .expect("a coherent view of the new facts");
        assert_eq!(after, "after");
        assert_eq!(stamp.check_current(), Ok(()));
        assert_eq!(before.check_current(), Err(PublicationError::Changed));
    }

    #[test]
    #[allow(clippy::panic)] // Deliberately inject a partial owner-mutation panic.
    fn a_panic_under_try_begin_stays_poisoned() {
        let publication = LocalAuthorizationPublication::new();
        let ((), stamp) = publication.observe(|| ()).expect("initial stamp");
        let result = std::panic::catch_unwind(|| {
            let _change = publication.try_begin_owner_change().expect("owner change");
            panic!("simulated partial owner mutation");
        });
        assert!(result.is_err());
        assert_eq!(stamp.check_current(), Err(PublicationError::Unavailable));
        assert!(matches!(
            publication.try_begin_owner_change(),
            Err(PublicationError::Unavailable)
        ));
        assert!(matches!(
            publication.begin_owner_change(),
            Err(PublicationError::Unavailable)
        ));
        assert!(matches!(
            publication.observe(|| ()),
            Err(PublicationError::Unavailable)
        ));
    }

    #[test]
    fn sequence_exhaustion_does_not_reuse_an_old_projection() {
        let publication = LocalAuthorizationPublication::new();
        publication
            .inner
            .sequence
            .store(INVALID - 1, Ordering::Release);
        let ((), stamp) = publication.observe(|| ()).expect("last even stamp");
        assert!(matches!(
            publication.begin_owner_change(),
            Err(PublicationError::Unavailable)
        ));
        assert_eq!(stamp.check_current(), Err(PublicationError::Unavailable));
    }
}
