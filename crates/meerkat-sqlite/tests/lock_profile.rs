//! The bundled SQLite this workspace links is built without its process-wide
//! locks: allocator statistics, the shared page cache and page-cache overflow
//! statistics are all compiled off by
//! [`meerkat_sqlite::REQUIRED_LIBSQLITE3_FLAGS`], which `.cargo/config.toml`
//! sets for cargo and `MODULE.bazel` sets for Bazel.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use meerkat_sqlite::{REQUIRED_LIBSQLITE3_FLAGS, SqliteLockProfile};

#[test]
fn bundled_sqlite_is_built_without_process_wide_locks() {
    let compiled = SqliteLockProfile::compiled();
    assert_eq!(
        compiled,
        SqliteLockProfile::REQUIRED,
        "the linked SQLite runs with process-wide locks ({compiled}): this build did not \
         set LIBSQLITE3_FLAGS=\"{REQUIRED_LIBSQLITE3_FLAGS}\""
    );
    assert_eq!(SqliteLockProfile::effective(), SqliteLockProfile::REQUIRED);
    meerkat_sqlite::check_lock_profile().expect("the required SQLite lock profile");
}
