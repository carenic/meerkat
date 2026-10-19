//! What SQLite's process-wide locks cost concurrent connections, for the
//! record: throughput of two workloads at 1, 16 and 64 threads, each thread
//! on its own database, with each workload's peak resident set size. Not a
//! gate. Run it in release under the declared build profile, then under the
//! bundled defaults (the `--config` value replaces the one `.cargo/config.toml`
//! forces) to compare:
//!
//! ```text
//! ./scripts/repo-cargo test --release -p meerkat-sqlite \
//!     --test process_wide_locks_bench -- --ignored --nocapture
//! ./scripts/repo-cargo test --release -p meerkat-sqlite \
//!     --config 'env.LIBSQLITE3_FLAGS.value=""' \
//!     --test process_wide_locks_bench -- --ignored --nocapture
//! ```
//!
//! - reopen: open a read-only connection to a store with an 80-table schema,
//!   run one query, close. Opening parses the schema: thousands of
//!   allocations, each taking the allocator-statistics lock when it is on.
//! - write: insert-and-select pairs on one long-lived WAL connection, which
//!   takes the shared page-cache lock on every page operation when it is on.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use meerkat_sqlite::SqliteLockProfile;
use rusqlite::{Connection, OpenFlags, params};

const THREADS: [usize; 3] = [1, 16, 64];
const OPENS_PER_THREAD: usize = 200;
const PAIRS_PER_THREAD: usize = 2_000;

#[test]
#[ignore = "benchmark: run on demand with --ignored --nocapture"]
fn concurrent_connection_throughput_by_lock_profile() {
    println!("lock profile: {}", SqliteLockProfile::compiled());
    let directory = tempfile::tempdir().unwrap();
    for threads in THREADS {
        let stores: Vec<PathBuf> = (0..threads)
            .map(|index| schema_store(directory.path(), index))
            .collect();
        reset_peak_resident_set();
        let started = Instant::now();
        run_threads(stores, |path| {
            for _ in 0..OPENS_PER_THREAD {
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
            }
        });
        report("reopen", threads, threads * OPENS_PER_THREAD, started);
    }
    for threads in THREADS {
        let databases: Vec<PathBuf> = (0..threads)
            .map(|index| {
                directory
                    .path()
                    .join(format!("write-{threads}-{index}.sqlite"))
            })
            .collect();
        reset_peak_resident_set();
        let started = Instant::now();
        run_threads(databases, |path| {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
            conn.pragma_update(None, "synchronous", "OFF").unwrap();
            conn.execute_batch(
                "CREATE TABLE t(id INTEGER PRIMARY KEY, k TEXT, v BLOB); CREATE INDEX tk ON t(k);",
            )
            .unwrap();
            let payload = vec![7_u8; 512];
            for n in 0..PAIRS_PER_THREAD {
                let key = format!("key-{n}");
                conn.execute("INSERT INTO t(k, v) VALUES (?1, ?2)", params![key, payload])
                    .unwrap();
                let _: i64 = conn
                    .query_row("SELECT count(*) FROM t WHERE k = ?1", [&key], |row| {
                        row.get(0)
                    })
                    .unwrap();
            }
        });
        report("write", threads, threads * PAIRS_PER_THREAD * 2, started);
    }
}

/// A WAL store with 80 tables and 80 indexes.
fn schema_store(directory: &Path, index: usize) -> PathBuf {
    let path = directory.join(format!("schema-{index}.sqlite"));
    if path.exists() {
        return path;
    }
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let mut ddl = String::from("BEGIN;");
    for table in 0..80 {
        ddl.push_str(&format!(
            "CREATE TABLE t{table}(id INTEGER PRIMARY KEY, a TEXT NOT NULL, b INTEGER, c BLOB, \
             UNIQUE(a, b)); CREATE INDEX t{table}_b ON t{table}(b, a);"
        ));
    }
    ddl.push_str("COMMIT;");
    conn.execute_batch(&ddl).unwrap();
    path
}

fn run_threads(paths: Vec<PathBuf>, work: fn(PathBuf)) {
    let handles: Vec<_> = paths
        .into_iter()
        .map(|path| thread::spawn(move || work(path)))
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

fn report(workload: &str, threads: usize, operations: usize, started: Instant) {
    let seconds = started.elapsed().as_secs_f64();
    let peak =
        peak_resident_set_kib().map_or_else(|| "unknown".to_string(), |kib| format!("{kib} KiB"));
    println!(
        "{workload} threads={threads} operations={operations} seconds={seconds:.3} \
         per_second={:.0} peak_rss={peak}",
        operations as f64 / seconds
    );
}

/// Restart the kernel's peak resident set count for this process (Linux).
fn reset_peak_resident_set() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// Peak resident set size since the last reset (Linux `VmHWM`).
fn peak_resident_set_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
}
