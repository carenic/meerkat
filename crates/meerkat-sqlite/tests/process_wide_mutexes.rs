//! Behaviour, not flag strings: on the bundled SQLite this workspace links,
//! ordinary connection work never enters the three process-wide static
//! mutexes that allocator statistics (`SQLITE_MUTEX_STATIC_MEM`), the shared
//! page cache (`SQLITE_MUTEX_STATIC_LRU`) and page-cache overflow statistics
//! (`SQLITE_MUTEX_STATIC_PMEM`) take.
//!
//! The test wraps SQLite's own mutex implementation with one that counts the
//! enters of each of those three static mutexes. `sqlite3_config` is
//! process-wide and not thread-safe, so this binary holds exactly this one
//! test, and it installs the wrapper before anything else uses SQLite.

#![allow(unsafe_code, clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::os::raw::c_int;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use rusqlite::{Connection, OpenFlags, ffi, params};

/// The static mutexes counted, with what takes them.
const COUNTED: [(c_int, &str); 3] = [
    (
        ffi::SQLITE_MUTEX_STATIC_MEM,
        "STATIC_MEM (allocator statistics)",
    ),
    (
        ffi::SQLITE_MUTEX_STATIC_LRU,
        "STATIC_LRU (shared page cache)",
    ),
    (
        ffi::SQLITE_MUTEX_STATIC_PMEM,
        "STATIC_PMEM (page-cache overflow statistics)",
    ),
];

static DEFAULT_METHODS: OnceLock<ffi::sqlite3_mutex_methods> = OnceLock::new();
static COUNTED_MUTEXES: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
static ENTERS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

fn default_methods() -> &'static ffi::sqlite3_mutex_methods {
    DEFAULT_METHODS
        .get()
        .expect("the default mutex methods are read before SQLite initializes")
}

fn note_enter(mutex: *mut ffi::sqlite3_mutex) {
    let address = mutex as usize;
    if address == 0 {
        return;
    }
    for (counted, enters) in COUNTED_MUTEXES.iter().zip(&ENTERS) {
        if counted.load(Ordering::Relaxed) == address {
            enters.fetch_add(1, Ordering::Relaxed);
        }
    }
}

unsafe extern "C" fn counting_alloc(id: c_int) -> *mut ffi::sqlite3_mutex {
    let alloc = default_methods().xMutexAlloc.expect("xMutexAlloc");
    // SAFETY: forwards SQLite's own call, unchanged, to its default method.
    let mutex = unsafe { alloc(id) };
    if let Some(index) = COUNTED.iter().position(|(counted, _)| *counted == id) {
        COUNTED_MUTEXES[index].store(mutex as usize, Ordering::Relaxed);
    }
    mutex
}

unsafe extern "C" fn counting_enter(mutex: *mut ffi::sqlite3_mutex) {
    note_enter(mutex);
    let enter = default_methods().xMutexEnter.expect("xMutexEnter");
    // SAFETY: forwards SQLite's own call, unchanged, to its default method.
    unsafe { enter(mutex) }
}

unsafe extern "C" fn counting_try(mutex: *mut ffi::sqlite3_mutex) -> c_int {
    note_enter(mutex);
    let try_enter = default_methods().xMutexTry.expect("xMutexTry");
    // SAFETY: forwards SQLite's own call, unchanged, to its default method.
    unsafe { try_enter(mutex) }
}

/// Wrap SQLite's mutex implementation with the counting one.
fn install_counting_mutexes() {
    // SAFETY: this binary holds one test and nothing in it has used SQLite
    // yet, so no other thread can call into SQLite while `sqlite3_config`
    // runs. Initializing and shutting down once makes SQLite fill in its
    // default mutex methods, which GETMUTEX then reads; MUTEX installs the
    // wrapper (SQLite copies the struct) before the next initialize.
    unsafe {
        assert_eq!(ffi::sqlite3_initialize(), ffi::SQLITE_OK);
        assert_eq!(ffi::sqlite3_shutdown(), ffi::SQLITE_OK);
        let mut defaults: ffi::sqlite3_mutex_methods = std::mem::zeroed();
        assert_eq!(
            ffi::sqlite3_config(ffi::SQLITE_CONFIG_GETMUTEX, &raw mut defaults),
            ffi::SQLITE_OK
        );
        assert!(
            defaults.xMutexAlloc.is_some()
                && defaults.xMutexEnter.is_some()
                && defaults.xMutexTry.is_some(),
            "SQLite reported no default mutex methods"
        );
        DEFAULT_METHODS
            .set(defaults)
            .expect("the counting mutexes are installed once");
        let mut counting = defaults;
        counting.xMutexAlloc = Some(counting_alloc);
        counting.xMutexEnter = Some(counting_enter);
        counting.xMutexTry = Some(counting_try);
        assert_eq!(
            ffi::sqlite3_config(ffi::SQLITE_CONFIG_MUTEX, &raw const counting),
            ffi::SQLITE_OK
        );
        assert_eq!(ffi::sqlite3_initialize(), ffi::SQLITE_OK);
    }
    for (counted, (id, name)) in COUNTED_MUTEXES.iter().zip(COUNTED) {
        assert_ne!(
            counted.load(Ordering::Relaxed),
            0,
            "SQLite did not allocate {name} (id {id}) at initialize"
        );
    }
}

/// Enters of each counted mutex since the previous call.
fn take_enters() -> [u64; 3] {
    [0, 1, 2].map(|index| ENTERS[index].swap(0, Ordering::Relaxed))
}

fn assert_no_enters(workload: &str, enters: [u64; 3]) {
    let entered: Vec<String> = COUNTED
        .iter()
        .zip(enters)
        .filter(|(_, count)| *count > 0)
        .map(|((_, name), count)| format!("{name}: {count}"))
        .collect();
    assert!(
        entered.is_empty(),
        "{workload} entered process-wide SQLite mutexes: {}",
        entered.join(", ")
    );
}

/// A long-lived connection running insert + select pairs.
fn write_workload(directory: &Path) -> [u64; 3] {
    let conn = Connection::open(directory.join("write.sqlite")).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, k TEXT, v BLOB); CREATE INDEX tk ON t(k);",
    )
    .unwrap();
    let payload = vec![7_u8; 512];
    let pair = |n: usize| {
        let key = format!("key-{n}");
        conn.execute("INSERT INTO t(k, v) VALUES (?1, ?2)", params![key, payload])
            .unwrap();
        let _: i64 = conn
            .query_row("SELECT count(*) FROM t WHERE k = ?1", [&key], |row| {
                row.get(0)
            })
            .unwrap();
    };
    for n in 0..200 {
        pair(n);
    }
    take_enters();
    for n in 200..2_200 {
        pair(n);
    }
    take_enters()
}

/// A fresh read connection per operation on a store with a large schema,
/// the shape of a delivery-owner store tick.
fn reopen_workload(directory: &Path) -> [u64; 3] {
    let path = directory.join("schema.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        let mut ddl = String::from("BEGIN;");
        for t in 0..80 {
            ddl.push_str(&format!(
                "CREATE TABLE t{t}(id INTEGER PRIMARY KEY, a TEXT NOT NULL, b INTEGER, c BLOB, \
                 UNIQUE(a, b)); CREATE INDEX t{t}_b ON t{t}(b, a);"
            ));
        }
        ddl.push_str("COMMIT;");
        conn.execute_batch(&ddl).unwrap();
    }
    let open_and_query = || {
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        let _: Option<i64> = conn
            .query_row("SELECT id FROM t79 WHERE b = ?1 LIMIT 1", [1], |row| {
                row.get(0)
            })
            .ok();
    };
    for _ in 0..10 {
        open_and_query();
    }
    take_enters();
    for _ in 0..200 {
        open_and_query();
    }
    take_enters()
}

#[test]
fn connection_work_enters_no_process_wide_sqlite_mutex() {
    install_counting_mutexes();
    let directory = tempfile::tempdir().unwrap();
    assert_no_enters(
        "4,000 statements on a long-lived connection",
        write_workload(directory.path()),
    );
    assert_no_enters(
        "200 fresh read connections on an 80-table store",
        reopen_workload(directory.path()),
    );
}
