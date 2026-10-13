//! Mechanical execution custody for one actual RuntimeStore backend.
//!
//! This guard grants no input, model, resource or credential permission. It
//! excludes a second execution owner while a governed machine owns this store.
//! Backend clones and decorators must expose the same owner, and every actual
//! machine or independently constructed persistent driver retains its claim.
use std::sync::{Arc, Mutex, TryLockError};

#[cfg(feature = "sqlite-store")]
mod physical;

/// A bounded custody attempt could not establish the requested store scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeStoreExecutionCustodyError {
    /// Another execution owner is active, or the short owner lock is busy.
    #[error("runtime store execution custody is busy")]
    Busy,
    /// This backend does not implement execution-lifetime custody.
    #[error("runtime store execution custody is unsupported")]
    Unsupported,
    /// The actual custody owner is poisoned or cannot represent a new claim.
    #[error("runtime store execution custody is unavailable")]
    Unavailable,
}

#[derive(Debug, Default)]
struct ExecutionOwners {
    shared: usize,
    governed: bool,
}

/// Backend-owned execution-lifetime exclusion, separate from row transactions.
///
/// A backend constructs this once for its actual shared state. A clone keeps
/// that same owner; constructing a new value around an existing backend would
/// violate the carrier's contract. SQLite binds the physical database instead
/// of the in-memory counters. Its lifetime lock is separate from row I/O, and
/// its admission gate is held only while acquiring or upgrading a claim.
#[derive(Debug, Clone)]
pub struct RuntimeStoreExecutionCustody {
    inner: ExecutionCustodyOwner,
}

#[derive(Debug, Clone)]
enum ExecutionCustodyOwner {
    Memory(Arc<Mutex<ExecutionOwners>>),
    #[cfg(feature = "sqlite-store")]
    Physical(Arc<physical::PhysicalExecutionCustody>),
}

impl Default for RuntimeStoreExecutionCustody {
    fn default() -> Self {
        Self {
            inner: ExecutionCustodyOwner::Memory(Arc::default()),
        }
    }
}

impl RuntimeStoreExecutionCustody {
    /// Construct the mechanical owner for a newly created backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind the actual SQLite owner's canonical, validated database path.
    /// This is private to store implementations; it grants no row authority.
    #[cfg(feature = "sqlite-store")]
    pub(super) fn for_sqlite_database(path: &std::path::Path) -> Self {
        Self {
            inner: ExecutionCustodyOwner::Physical(Arc::new(
                physical::PhysicalExecutionCustody::new(path),
            )),
        }
    }

    /// Match the actual retained carrier, not a same-path owner or a claim label.
    /// Backend implementors use this before acquiring administrative writer
    /// custody. A match grants no input or credential permission and does not
    /// replace retaining the claim and the actual writer through mutation.
    pub fn owns_governed_claim(&self, claim: &RuntimeStoreExecutionClaim) -> bool {
        match (&self.inner, &claim.owner) {
            (
                ExecutionCustodyOwner::Memory(owner),
                ExecutionClaimOwner::Memory {
                    owner: actual,
                    governed,
                },
            ) => *governed && Arc::ptr_eq(owner, actual),
            #[cfg(feature = "sqlite-store")]
            (ExecutionCustodyOwner::Physical(owner), ExecutionClaimOwner::Physical(actual)) => {
                actual.is_governed() && actual.belongs_to(owner)
            }
            #[cfg(feature = "sqlite-store")]
            _ => false,
        }
    }

    /// Whether `claim` binds this owner's own store, whichever owner instance
    /// took it: the same in-memory owner, or the same canonical SQLite
    /// database (an alias path resolves to it). A machine never retains such
    /// a claim as an associated one, which would admit it against itself.
    pub fn binds_same_store(&self, claim: &RuntimeStoreExecutionClaim) -> bool {
        match (&self.inner, &claim.owner) {
            (
                ExecutionCustodyOwner::Memory(owner),
                ExecutionClaimOwner::Memory { owner: actual, .. },
            ) => Arc::ptr_eq(owner, actual),
            #[cfg(feature = "sqlite-store")]
            (ExecutionCustodyOwner::Physical(owner), ExecutionClaimOwner::Physical(actual)) => {
                actual.binds_same_file(owner)
            }
            #[cfg(feature = "sqlite-store")]
            _ => false,
        }
    }

    /// Attempt one ordinary execution claim without waiting or polling.
    pub fn try_acquire_shared(
        &self,
    ) -> Result<RuntimeStoreExecutionClaim, RuntimeStoreExecutionCustodyError> {
        #[cfg(feature = "sqlite-store")]
        let inner = match &self.inner {
            ExecutionCustodyOwner::Memory(inner) => inner,
            #[cfg(feature = "sqlite-store")]
            ExecutionCustodyOwner::Physical(owner) => {
                return owner
                    .try_acquire(false)
                    .map(|claim| RuntimeStoreExecutionClaim {
                        owner: ExecutionClaimOwner::Physical(claim),
                        associated: Vec::new(),
                    });
            }
        };
        #[cfg(not(feature = "sqlite-store"))]
        let ExecutionCustodyOwner::Memory(inner) = &self.inner;
        let mut current = inner.try_lock().map_err(custody_lock_error)?;
        if current.governed {
            return Err(RuntimeStoreExecutionCustodyError::Busy);
        }
        current.shared = current
            .shared
            .checked_add(1)
            .ok_or(RuntimeStoreExecutionCustodyError::Unavailable)?;
        Ok(RuntimeStoreExecutionClaim {
            owner: ExecutionClaimOwner::Memory {
                owner: Arc::clone(inner),
                governed: false,
            },
            associated: Vec::new(),
        })
    }

    /// Attempt exclusive execution custody before constructing a governed owner.
    /// A failed attempt leaves every existing claim unchanged.
    pub fn try_acquire_governed(
        &self,
    ) -> Result<RuntimeStoreExecutionClaim, RuntimeStoreExecutionCustodyError> {
        #[cfg(feature = "sqlite-store")]
        let inner = match &self.inner {
            ExecutionCustodyOwner::Memory(inner) => inner,
            #[cfg(feature = "sqlite-store")]
            ExecutionCustodyOwner::Physical(owner) => {
                return owner
                    .try_acquire(true)
                    .map(|claim| RuntimeStoreExecutionClaim {
                        owner: ExecutionClaimOwner::Physical(claim),
                        associated: Vec::new(),
                    });
            }
        };
        #[cfg(not(feature = "sqlite-store"))]
        let ExecutionCustodyOwner::Memory(inner) = &self.inner;
        let mut current = inner.try_lock().map_err(custody_lock_error)?;
        if current.governed || current.shared != 0 {
            return Err(RuntimeStoreExecutionCustodyError::Busy);
        }
        current.governed = true;
        Ok(RuntimeStoreExecutionClaim {
            owner: ExecutionClaimOwner::Memory {
                owner: Arc::clone(inner),
                governed: true,
            },
            associated: Vec::new(),
        })
    }
}

fn custody_lock_error<T>(error: TryLockError<T>) -> RuntimeStoreExecutionCustodyError {
    match error {
        TryLockError::WouldBlock => RuntimeStoreExecutionCustodyError::Busy,
        TryLockError::Poisoned(_) => RuntimeStoreExecutionCustodyError::Unavailable,
    }
}

/// One real execution owner's retained claim. Share its lifetime through Arc,
/// rather than acquiring a second claim for a driver of the same machine.
#[derive(Debug)]
pub struct RuntimeStoreExecutionClaim {
    owner: ExecutionClaimOwner,
    /// Governed claims on other actual stores that this owner's composition
    /// is bound to ([`Self::retain_associated`]). They grant nothing here.
    /// Declared after `owner`, they are released with this claim, after it.
    associated: Vec<RuntimeStoreExecutionClaim>,
}

#[derive(Debug)]
enum ExecutionClaimOwner {
    Memory {
        owner: Arc<Mutex<ExecutionOwners>>,
        governed: bool,
    },
    #[cfg(feature = "sqlite-store")]
    Physical(physical::PhysicalExecutionClaim),
}

impl RuntimeStoreExecutionClaim {
    /// Atomically upgrade the only shared owner, before sharing the machine.
    /// A busy attempt preserves ordinary custody. If physical restoration is
    /// uncertain, the failed claim retains admission exclusion until drop and
    /// every retry refuses with `Unavailable`.
    pub fn try_upgrade_to_governed(&mut self) -> Result<(), RuntimeStoreExecutionCustodyError> {
        let (inner, governed) = match &mut self.owner {
            ExecutionClaimOwner::Memory { owner, governed } => (owner, governed),
            #[cfg(feature = "sqlite-store")]
            ExecutionClaimOwner::Physical(claim) => return claim.try_upgrade(),
        };
        let mut current = inner.try_lock().map_err(custody_lock_error)?;
        if *governed {
            return Ok(());
        }
        if current.governed || current.shared != 1 {
            return Err(RuntimeStoreExecutionCustodyError::Busy);
        }
        current.shared = 0;
        current.governed = true;
        *governed = true;
        Ok(())
    }

    /// Whether this retained claim exclusively owns execution of this backend.
    pub fn is_governed(&self) -> bool {
        match &self.owner {
            ExecutionClaimOwner::Memory { governed, .. } => *governed,
            #[cfg(feature = "sqlite-store")]
            ExecutionClaimOwner::Physical(claim) => claim.is_governed(),
        }
    }

    /// Retain `associated`, the governed claim on another actual store that
    /// this owner's composition is bound to, for exactly this claim's
    /// lifetime: every holder sharing this claim shares it, and it is
    /// released only when this claim is, after it. It grants no authority
    /// over either store; it only keeps that store's other execution owners
    /// out. Both claims must be governed, before this one is shared.
    pub(crate) fn retain_associated(
        &mut self,
        associated: RuntimeStoreExecutionClaim,
    ) -> Result<(), RuntimeStoreExecutionCustodyError> {
        if !self.is_governed() || !associated.is_governed() {
            return Err(RuntimeStoreExecutionCustodyError::Unavailable);
        }
        self.associated.push(associated);
        Ok(())
    }
}

impl Drop for RuntimeStoreExecutionClaim {
    fn drop(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        final_drop_holds::park(self);
        // This lock contains only mechanical counters. Poison remains set, so
        // releasing an old claim cannot make a poisoned backend usable again.
        let (inner, governed) = match &self.owner {
            ExecutionClaimOwner::Memory { owner, governed } => (owner, governed),
            #[cfg(feature = "sqlite-store")]
            ExecutionClaimOwner::Physical(_) => return,
        };
        let mut current = inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *governed {
            current.governed = false;
        } else {
            current.shared = current.shared.saturating_sub(1);
        }
    }
}

/// Test holds on the final destruction of one exact claim (by address): the
/// destructor reports the thread it runs on, then parks, still holding the
/// custody, until its hold is released or dropped. Other claims pass.
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod final_drop_holds {
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread::ThreadId;

    use super::RuntimeStoreExecutionClaim;

    type Hold = (usize, Sender<ThreadId>, Receiver<()>);

    static HOLDS: Mutex<Vec<Hold>> = Mutex::new(Vec::new());

    /// Hold the final destruction of `claim`. The first receiver reports the
    /// thread that parked in it; the destruction completes once the sender is
    /// used or dropped.
    pub(crate) fn hold(
        claim: &Arc<RuntimeStoreExecutionClaim>,
    ) -> (Receiver<ThreadId>, Sender<()>) {
        let (parked_tx, parked_rx) = channel();
        let (release_tx, release_rx) = channel();
        HOLDS.lock().unwrap_or_else(PoisonError::into_inner).push((
            Arc::as_ptr(claim).addr(),
            parked_tx,
            release_rx,
        ));
        (parked_rx, release_tx)
    }

    pub(super) fn park(claim: &RuntimeStoreExecutionClaim) {
        let address = std::ptr::from_ref(claim).addr();
        let hold = {
            let mut holds = HOLDS.lock().unwrap_or_else(PoisonError::into_inner);
            holds
                .iter()
                .position(|(held, _, _)| *held == address)
                .map(|index| holds.remove(index))
        };
        if let Some((_, parked, release)) = hold {
            let _ = parked.send(std::thread::current().id());
            let _ = release.recv();
        }
    }
}

#[cfg(test)]
mod tests;
