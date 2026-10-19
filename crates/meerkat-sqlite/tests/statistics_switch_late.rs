//! The host binaries' runtime allocator-statistics switch, called after SQLite
//! was used, is refused by SQLite: the outcome is recorded as a degrade
//! (`TooLate`), never a failure, and the build's own profile stands. The
//! switch is process-wide, so this binary holds exactly this one test.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use meerkat_sqlite::{MemoryStatisticsOutcome, SqliteLockProfile};

#[test]
fn switch_called_after_sqlite_use_is_too_late() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let one: i64 = conn.query_row("SELECT 1", [], |row| row.get(0)).unwrap();
    assert_eq!(one, 1);
    // SAFETY: the only test in this binary; this thread is the only one that
    // used SQLite, and its connection is idle while the switch runs.
    #[allow(unsafe_code)]
    let outcome = unsafe { meerkat_sqlite::disable_memory_statistics_at_process_start() };
    assert_eq!(outcome, MemoryStatisticsOutcome::TooLate);
    assert_eq!(
        meerkat_sqlite::memory_statistics_outcome(),
        Some(MemoryStatisticsOutcome::TooLate)
    );
    // The build turns allocator statistics off, so the effective profile is
    // still the required one.
    assert_eq!(SqliteLockProfile::effective(), SqliteLockProfile::REQUIRED);
}
