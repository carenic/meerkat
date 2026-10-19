//! Shared SQLite mechanics for Meerkat stores.
//!
//! This crate is the single home for the connection policy, schema-evolution,
//! and maintenance-fence machinery that was previously copy-pasted across the
//! store crates (six independently authored PRAGMA setups with divergent busy
//! timeouts, `CREATE TABLE IF NOT EXISTS` everywhere, and no version ledger).
//! See `docs/plans/storage-unification-plan.md` (Phase 3) in the workspace
//! root for the design rationale.
//!
//! What lives here:
//!
//! - [`profile`]: DDL-free connection opening under named policy profiles
//!   ([`ConnectionProfile::Primary`], [`ConnectionProfile::ReadOnly`],
//!   [`ConnectionProfile::Maintenance`]). Opening a connection never runs
//!   schema DDL; stores apply their [`ledger`] domain after opening. The
//!   optional [`OpenOptions::schema_preflight`] proves an exact current,
//!   released-predecessor, or fresh-domain shape before any mutating pragma,
//!   [`JournalPolicy`] states per profile whether an open establishes WAL or
//!   preserves the mode it finds, and [`WriteContact`] types each profile's
//!   honest no-write guarantee (WAL reads may need sidecar files).
//! - [`ledger`]: the per-file migration ledger (`meerkat_schema(domain,
//!   version)`) with the pinned concurrent-open transaction protocol and the
//!   typed refusals for future, pre-floor, gap, unledgered-owned, and
//!   fingerprint-mismatched shapes. Ledger state that is malformed (wrong
//!   shape, duplicate rows, non-positive versions) is refused typed, never
//!   healed.
//! - [`json_column`]: the TEXT-or-BLOB tolerant JSON column codec.
//! - [`lock_profile`]: the process-wide lock profile of the bundled SQLite:
//!   the one declared `LIBSQLITE3_FLAGS` value that turns off its three
//!   process-wide mutexes, a read of the profile the process actually runs
//!   with, and the runtime allocator-statistics switch host binaries call
//!   first in `main`.
//! - [`fence`]: the per-operation maintenance-fence guards. Store operations
//!   take a shared guard; offline migration takes the exclusive side and
//!   waits for in-flight operations to drain and for kept connections (which
//!   hold the presence lock) to close. Stores built on this crate hold no
//!   bespoke opener, which is what makes them fence-aware for free.
//! - [`watch`]: the cross-process change watch over a database file and its
//!   sidecars (coalesced notification ticks plus a bounded sweep; a hint,
//!   never permission or custody).
//! - [`error`]: the crate error type plus the storage-level error
//!   classification (transient / corrupt) that store crates layer their
//!   staleness semantics on top of. Adoption contract: store crates route
//!   every raw rusqlite error through [`classify_sqlite_error`] instead of
//!   re-matching SQLite error codes locally.
//!
//! # Retryability
//!
//! Error classification alone does not authorize a retry: a transient failure
//! can land after a write committed but before success became observable, and
//! blind retry duplicates non-idempotent effects. Automatic retry is only
//! sound for idempotent or CAS-keyed operations; an indeterminate
//! non-idempotent write requires outcome reconciliation (read back, then
//! decide) before any retry. Bounded waiting for lock contention is the
//! connection busy handler's job, not a caller retry loop.

pub mod error;
pub mod fence;
pub mod json_column;
pub mod ledger;
pub mod lock_profile;
pub mod profile;
#[cfg(not(target_arch = "wasm32"))]
pub mod watch;

pub use error::{
    BridgeEligibility, CurrentCatalogUnavailability, SqliteErrorClass, SqliteStoreError,
    classify_sqlite_error, is_busy_or_locked,
};
pub use fence::{
    ExclusiveFence, KeptConnectionPresence, OperationGuard, fence_lock_path, presence_lock_path,
};
pub use json_column::JsonColumnBytes;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub use ledger::arm_preflight_probe_for_test;
pub use ledger::{
    LedgerReport, MaintenanceBridgeReport, MaintenancePrepareFn, MaintenancePrepareReport,
    MaintenanceRecordRefusal, Migration, SchemaDomain, SchemaObject, SchemaObjectKind,
    SchemaPredecessor, apply_domain_migrations, bridge_unledgered_domain, domain_version,
    preflight_domain_version, preflight_schema_eligibility, verify_released_schema_fingerprint,
    verify_released_schema_structure,
};
pub use lock_profile::{
    MemoryStatisticsOutcome, REQUIRED_LIBSQLITE3_FLAGS, SqliteLockProfile,
    SqliteLockProfileMismatch, check_lock_profile, disable_memory_statistics_at_process_start,
    memory_statistics_outcome,
};
pub use profile::{
    ConnectionProfile, DatabaseFileIdentity, JournalPolicy, OpenOptions, SHARED_BUSY_TIMEOUT,
    WriteContact, begin_immediate, open, open_with, validate_database_file,
};
/// The connection type [`open`] returns, so callers can name it without
/// taking their own direct `rusqlite` dependency.
pub use rusqlite::Connection;
