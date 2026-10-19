//! The host binaries' runtime allocator-statistics switch, called first in a
//! process, finds this workspace's SQLite already built without allocator
//! statistics. The switch is process-wide and must run before any other
//! SQLite use, so this binary holds exactly this one test.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use meerkat_sqlite::{MemoryStatisticsOutcome, SqliteLockProfile};

#[test]
fn switch_called_first_finds_statistics_already_off() {
    assert_eq!(meerkat_sqlite::memory_statistics_outcome(), None);
    // SAFETY: the only test in this binary, and nothing has used SQLite yet.
    #[allow(unsafe_code)]
    let outcome = unsafe { meerkat_sqlite::disable_memory_statistics_at_process_start() };
    assert_eq!(outcome, MemoryStatisticsOutcome::AlreadyOff);
    assert_eq!(
        meerkat_sqlite::memory_statistics_outcome(),
        Some(MemoryStatisticsOutcome::AlreadyOff)
    );
    // SAFETY: as above; a second call returns the recorded outcome.
    #[allow(unsafe_code)]
    let again = unsafe { meerkat_sqlite::disable_memory_statistics_at_process_start() };
    assert_eq!(again, MemoryStatisticsOutcome::AlreadyOff);
    assert_eq!(SqliteLockProfile::effective(), SqliteLockProfile::REQUIRED);
}
