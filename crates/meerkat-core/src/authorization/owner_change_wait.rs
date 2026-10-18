//! Bounded waiting for an owner change in progress on a work's own
//! authorization publication.
//!
//! An owner that publishes a change keeps the publication's sequence odd
//! until the change completes, and every observation made meanwhile refuses
//! with `ReprepareRequired`. The built-in rebuilds of preparation and of the
//! final check run back to back, so a single change held across a storage
//! commit outlasts all of them. An operation that refused that way waits here
//! for the change to complete before it rebuilds. Waiting only delays: the
//! rebuild still requires a coherent observation and every current check, and
//! no refusal is weakened.

use crate::time_compat::{DeadlineInstant, Duration};
#[cfg(target_arch = "wasm32")]
use crate::tokio;

use super::WorkAuthorization;

/// The longest one operation waits, in total wall-clock time from its first
/// wait and including the rebuilds in between, for owner changes in progress.
/// A storage commit with fsync typically completes well within 100 ms; a
/// change held longer than this is a stuck owner, and the operation then
/// refuses as it would without waiting.
pub const OWNER_CHANGE_WAIT_BOUND: Duration = Duration::from_secs(2);

/// Rebuilds one operation may make after a settled owner change without
/// spending its own rebuild budget.
pub const OWNER_CHANGE_FREE_REBUILDS: u8 = 3;

const FIRST_BACKOFF: Duration = Duration::from_millis(1);
const MAX_BACKOFF: Duration = Duration::from_millis(64);

/// One operation's waiting state across its rebuilds: a single wall-clock
/// deadline, set at its first wait and never re-armed, and the free rebuilds
/// it has spent.
#[derive(Debug, Default)]
pub struct OwnerChangeChurn {
    deadline: Option<DeadlineInstant>,
    free_rebuilds: u8,
}

/// How waiting for one refused operation's owner change ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OwnerChangeWait {
    /// No change is in progress any more: the operation may rebuild once
    /// without spending its own rebuild budget. This does not mean the
    /// change completed: a publication poisoned during the wait reports no
    /// change in progress either, and the rebuild then refuses
    /// `Unavailable`, exactly as without waiting.
    Settled {
        /// Time spent waiting for this change.
        waited: Duration,
    },
    /// No free rebuild: the deadline passed, or the operation already spent
    /// every free rebuild. The refusal takes its ordinary path.
    NotSettled {
        /// Time spent waiting for this change.
        waited: Duration,
    },
}

impl OwnerChangeWait {
    /// Whether the operation may rebuild without spending its own budget.
    #[must_use]
    pub fn settled(self) -> bool {
        matches!(self, Self::Settled { .. })
    }

    /// Time spent waiting.
    #[must_use]
    pub fn waited(self) -> Duration {
        match self {
            Self::Settled { waited } | Self::NotSettled { waited } => waited,
        }
    }
}

impl OwnerChangeChurn {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// After a `ReprepareRequired` refusal of an operation of `work`: when
    /// `work` reports an owner change in progress on its own publication,
    /// wait for that change, and for any later one, to complete. The first
    /// call sets the operation's deadline to [`OWNER_CHANGE_WAIT_BOUND`] from
    /// now, clamped to `budget_remaining`, and later calls keep it. A free
    /// rebuild is granted only once the sequence has moved past the change
    /// seen here and is not odd again, and at most
    /// [`OWNER_CHANGE_FREE_REBUILDS`] times per operation. A publication
    /// poisoned during the wait also ends it as settled; see
    /// [`OwnerChangeWait::Settled`].
    ///
    /// `None` when no change is in progress: the refusal takes its ordinary
    /// path, exactly as without this wait.
    pub async fn wait_for_settled_change(
        &mut self,
        work: &dyn WorkAuthorization,
        budget_remaining: Option<Duration>,
    ) -> Option<OwnerChangeWait> {
        work.owner_change_in_progress()?;
        if self.free_rebuilds >= OWNER_CHANGE_FREE_REBUILDS {
            return Some(OwnerChangeWait::NotSettled {
                waited: Duration::ZERO,
            });
        }
        let deadline = *self.deadline.get_or_insert_with(|| {
            let bound = budget_remaining.map_or(OWNER_CHANGE_WAIT_BOUND, |remaining| {
                remaining.min(OWNER_CHANGE_WAIT_BOUND)
            });
            DeadlineInstant::now() + bound
        });
        let wait = await_owner_change(work, deadline).await;
        if wait.settled() {
            self.free_rebuilds += 1;
        }
        Some(wait)
    }
}

/// Wait until no change is in progress on `work`'s publication, or until
/// `deadline`. A change that began after the one refused keeps the wait
/// going under the same deadline; only a read with no change in progress
/// settles it.
async fn await_owner_change(
    work: &dyn WorkAuthorization,
    deadline: DeadlineInstant,
) -> OwnerChangeWait {
    let start = DeadlineInstant::now();
    let mut backoff = FIRST_BACKOFF;
    loop {
        let now = DeadlineInstant::now();
        if work.owner_change_in_progress().is_none() {
            return OwnerChangeWait::Settled {
                waited: now.saturating_duration_since(start),
            };
        }
        if now >= deadline {
            return OwnerChangeWait::NotSettled {
                waited: now.saturating_duration_since(start),
            };
        }
        tokio::time::sleep(backoff.min(deadline.saturating_duration_since(now))).await;
        backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
    }
}
