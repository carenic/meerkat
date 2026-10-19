//! The kept delivery-generation reader (#549b): one verified open, then one
//! guarded snapshot read per store tick, closed for maintenance, for a file
//! replaced at its path, and for a schema or ledger change. Unix only: the
//! store keeps the reader on unix, and the replacement tests rename open
//! files.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use meerkat_runtime::LogicalRuntimeId;
use meerkat_runtime::store::sqlite::delivery_generation_test_hooks as hooks;
use meerkat_runtime::store::{
    DeliveryGenerationReader, RuntimeDeliveryAuthorityCasOutcome, RuntimeDeliveryAuthorityRecord,
    RuntimeStore, RuntimeStoreError, SqliteRuntimeStore,
};
use meerkat_sqlite::{ExclusiveFence, OperationGuard, SqliteStoreError};

/// A store at `dir/runtime.sqlite3` with `commits` delivery commits, so its
/// delivery generation reads `commits`.
async fn store_with_generation(dir: &Path, commits: u64) -> Arc<SqliteRuntimeStore> {
    let store = Arc::new(
        SqliteRuntimeStore::new(dir.join("runtime.sqlite3")).expect("open the runtime store"),
    );
    commit_deliveries(&store, "kept-reader-runtime", commits).await;
    store
}

/// `commits` delivery commits for one runtime, each advancing the store's
/// delivery generation by one.
async fn commit_deliveries(store: &SqliteRuntimeStore, runtime: &str, commits: u64) {
    let runtime_id = LogicalRuntimeId::new(runtime);
    let mut observed = store
        .load_runtime_delivery_authority(&runtime_id)
        .await
        .expect("read authority")
        .map(|record| record.revision());
    for _ in 0..commits {
        let revision = observed.map_or(1, |revision| revision + 1);
        let outcome = store
            .compare_and_swap_runtime_delivery_authority(
                &runtime_id,
                observed,
                RuntimeDeliveryAuthorityRecord::from_parts(
                    revision,
                    br#"{"opaque":true}"#.to_vec(),
                ),
                None,
            )
            .await
            .expect("delivery commit");
        assert!(
            matches!(outcome, RuntimeDeliveryAuthorityCasOutcome::Applied(_)),
            "{outcome:?}"
        );
        observed = Some(revision);
    }
}

fn kept_reader(store: &SqliteRuntimeStore) -> Box<dyn DeliveryGenerationReader> {
    store
        .delivery_generation_reader()
        .expect("the SQLite store keeps a delivery-generation reader on this platform")
}

/// T1, read side: a hundred reads with no write between them cost one
/// verified open through the kept reader, and a hundred through the
/// fresh-open path (the path every read took before the kept reader).
#[tokio::test]
async fn a_hundred_quiet_reads_cost_one_verified_open_when_kept() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 3).await;
    let path = store.path().to_path_buf();

    let mut reader = kept_reader(&store);
    let before = hooks::verified_opens(&path);
    for _ in 0..100 {
        assert_eq!(reader.read().await.expect("kept read"), 3);
        assert!(reader.keeps_connection());
    }
    assert_eq!(
        hooks::verified_opens(&path) - before,
        1,
        "the kept reader opens once"
    );
    drop(reader);

    hooks::force_fresh_reads(&path);
    assert!(
        store.delivery_generation_reader().is_none(),
        "forced onto the fresh-open path"
    );
    let before = hooks::verified_opens(&path);
    for _ in 0..100 {
        assert_eq!(
            store.load_delivery_generation().await.expect("fresh read"),
            3
        );
    }
    assert_eq!(
        hooks::verified_opens(&path) - before,
        100,
        "the fresh-open path opens per read"
    );
}

/// T2, reader side: while maintenance holds the fence lock and waits for the
/// presence lock, the kept reader's next read is refused typed and closes
/// its connection, and the fence then completes; once the fence is gone the
/// reader opens and keeps again.
#[tokio::test]
async fn a_kept_reader_closes_when_maintenance_waits_for_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 2).await;
    let path = store.path().to_path_buf();
    let mut reader = kept_reader(&store);
    assert_eq!(reader.read().await.expect("first read"), 2);
    assert!(reader.keeps_connection());

    let waiting = {
        let path = path.clone();
        std::thread::spawn(move || ExclusiveFence::acquire(&path, Duration::from_secs(10)))
    };
    // A probe guard of its own, not the reader's: refused once the fence
    // holds its fence lock and waits for the presence lock.
    let started = Instant::now();
    loop {
        match OperationGuard::for_database(&path) {
            Err(SqliteStoreError::MaintenanceFenceHeld { .. }) => break,
            Ok(guard) => drop(guard),
            Err(other) => panic!("unexpected guard error: {other}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the waiting fence never took its fence lock"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !waiting.is_finished(),
        "maintenance waits for the kept connection"
    );
    let refused = Instant::now();
    let err = reader
        .read()
        .await
        .expect_err("the read under a waiting fence is refused");
    assert!(
        matches!(err, RuntimeStoreError::MaintenanceFenceHeld { .. }),
        "refused typed: {err:?}"
    );
    assert!(
        !reader.keeps_connection(),
        "the refused read closed the kept connection"
    );
    let fence = waiting
        .join()
        .expect("thread")
        .expect("maintenance gets the database once the reader closed");
    assert!(
        refused.elapsed() < Duration::from_secs(2),
        "the fence completed at its next try: {:?}",
        refused.elapsed()
    );
    drop(fence);

    assert_eq!(reader.read().await.expect("read after maintenance"), 2);
    assert!(reader.keeps_connection(), "kept again after maintenance");
}

/// Move the database at `path` aside and put a database whose generation is
/// `generation` in its place, as an archive by rename followed by a new
/// database does.
async fn archive_and_replace(dir: &Path, path: &Path, generation: u64) -> PathBuf {
    let replacement_dir = dir.join("replacement");
    std::fs::create_dir(&replacement_dir).expect("replacement dir");
    let replacement = store_with_generation(&replacement_dir, generation).await;
    let replacement_path = replacement.path().to_path_buf();
    drop(replacement);
    let archived = path.with_file_name("runtime.sqlite3.archived");
    std::fs::rename(path, &archived).expect("archive the database by rename");
    std::fs::rename(&replacement_path, path).expect("put the new database in place");
    archived
}

/// T4: a database archived and replaced just before a kept read takes its
/// guard is read as the new file, after one verified open: the reader
/// compares the file identity under the guard.
#[tokio::test]
async fn a_database_replaced_before_the_guard_is_read_from_the_new_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 2).await;
    let path = store.path().to_path_buf();
    let mut reader = kept_reader(&store);
    assert_eq!(reader.read().await.expect("first read"), 2);
    let opens = hooks::verified_opens(&path);

    // Build the replacement first; the hook only renames.
    let replacement_dir = dir.path().join("replacement");
    std::fs::create_dir(&replacement_dir).expect("replacement dir");
    let replacement = store_with_generation(&replacement_dir, 5).await;
    let replacement_path = replacement.path().to_path_buf();
    drop(replacement);
    let archived = path.with_file_name("runtime.sqlite3.archived");
    let mut replace_once = Some((path.clone(), replacement_path, archived));
    hooks::before_kept_read_guard(&path, move || {
        if let Some((path, replacement, archived)) = replace_once.take() {
            std::fs::rename(&path, &archived).expect("archive by rename");
            std::fs::rename(&replacement, &path).expect("new database in place");
        }
    });

    assert_eq!(
        reader.read().await.expect("read after the replacement"),
        5,
        "the read returns the new file's generation"
    );
    assert_eq!(
        hooks::verified_opens(&path) - opens,
        1,
        "after one verified open of the new file"
    );
    assert!(reader.keeps_connection());
}

/// The replacement helper is also a plain-rename control: a replacement made
/// between two reads (no hook) is read from the new file too.
#[tokio::test]
async fn a_database_replaced_between_reads_is_read_from_the_new_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 2).await;
    let path = store.path().to_path_buf();
    let mut reader = kept_reader(&store);
    assert_eq!(reader.read().await.expect("first read"), 2);

    let archived = archive_and_replace(dir.path(), &path, 7).await;
    assert_eq!(reader.read().await.expect("read after the replacement"), 7);
    assert!(archived.exists(), "the archive stays where it was moved");
}

/// T5: DDL from another connection costs one more verified open; a data-only
/// write (a delivery commit) keeps the connection; a future domain version
/// stamped by another process is refused typed, as the verified open
/// refuses it.
#[tokio::test]
async fn schema_and_ledger_changes_reopen_and_data_writes_do_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 1).await;
    let path = store.path().to_path_buf();
    let mut reader = kept_reader(&store);
    assert_eq!(reader.read().await.expect("first read"), 1);
    let opens = hooks::verified_opens(&path);

    // Data only: a delivery commit moves the generation, not the schema.
    commit_deliveries(&store, "kept-reader-runtime", 1).await;
    assert_eq!(reader.read().await.expect("read after a commit"), 2);
    assert_eq!(
        hooks::verified_opens(&path),
        opens,
        "a data-only write keeps the connection"
    );

    // DDL: another connection changes the schema.
    let other = rusqlite::Connection::open(&path).expect("another connection");
    other
        .execute_batch("CREATE TABLE unrelated_after_open (x INTEGER)")
        .expect("ddl");
    assert_eq!(reader.read().await.expect("read after ddl"), 2);
    assert_eq!(
        hooks::verified_opens(&path) - opens,
        1,
        "a schema change costs one verified open"
    );

    // A newer binary's stamp: data in the ledger, no DDL.
    other
        .execute(
            "UPDATE meerkat_schema SET version = 99 WHERE domain = 'runtime-delivery'",
            [],
        )
        .expect("stamp a future version");
    let err = reader
        .read()
        .await
        .expect_err("a future delivery domain is refused");
    assert!(
        matches!(
            err,
            RuntimeStoreError::SchemaFromTheFuture { ref domain, found: 99, .. }
                if domain == "runtime-delivery"
        ),
        "refused typed: {err:?}"
    );
    assert!(!reader.keeps_connection());
}

/// T7: a process that holds the database's exclusive fence reads through
/// the fresh-open path (its own guards pass through the holder registry)
/// and never waits on itself; once it releases the fence, the reader keeps.
#[tokio::test]
async fn the_fence_holding_process_reads_fresh_and_never_waits_on_itself() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 4).await;
    let path = store.path().to_path_buf();
    let mut reader = kept_reader(&store);

    let fence = ExclusiveFence::try_acquire(&path)
        .expect("io")
        .expect("this process takes the fence");
    let started = Instant::now();
    assert_eq!(reader.read().await.expect("holder read"), 4);
    assert!(
        !reader.keeps_connection(),
        "nothing is kept while this process holds the fence"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "no wait on its own fence: {:?}",
        started.elapsed()
    );
    drop(fence);

    assert_eq!(reader.read().await.expect("read after the fence"), 4);
    assert!(reader.keeps_connection());
}

/// A database that goes missing under a kept connection (its directory
/// archived, an empty one left at the path) reads zero, as the fresh-open
/// path does, keeps nothing, and plants no lock file in the new directory.
#[tokio::test]
async fn a_missing_database_reads_zero_and_keeps_nothing() {
    let root = tempfile::tempdir().expect("tempdir");
    let realm = root.path().join("realm");
    std::fs::create_dir(&realm).expect("realm");
    let store = store_with_generation(&realm, 2).await;
    let mut reader = kept_reader(&store);
    assert_eq!(reader.read().await.expect("first read"), 2);
    assert!(reader.keeps_connection());

    std::fs::rename(&realm, root.path().join("realm.archived")).expect("archive the directory");
    std::fs::create_dir(&realm).expect("an empty directory at the path");
    assert_eq!(reader.read().await.expect("read"), 0);
    assert!(!reader.keeps_connection());
    let planted: Vec<_> = std::fs::read_dir(&realm)
        .expect("read the new directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert!(
        planted.is_empty(),
        "nothing planted next to the missing database: {planted:?}"
    );
}

/// T8, numbers only: the cost of one quiet delivery-generation read through
/// each path on one store. Run in release:
///
/// ```text
/// ./scripts/repo-cargo test --release -p meerkat-runtime --features sqlite-store,test-support \
///     --test kept_delivery_reader -- --ignored --nocapture bench_
/// ```
#[tokio::test]
#[ignore = "benchmark: prints numbers, asserts nothing about them"]
async fn bench_fresh_and_kept_generation_reads() {
    const READS: u32 = 2_000;
    let dir = tempfile::tempdir().expect("tempdir");
    let store = store_with_generation(dir.path(), 3).await;

    let mut reader = kept_reader(&store);
    reader.read().await.expect("open");
    let started = Instant::now();
    for _ in 0..READS {
        reader.read().await.expect("kept read");
    }
    let kept = started.elapsed() / READS;
    drop(reader);

    let started = Instant::now();
    for _ in 0..READS {
        store.load_delivery_generation().await.expect("fresh read");
    }
    let fresh = started.elapsed() / READS;
    println!("delivery-generation read: fresh {fresh:?}, kept {kept:?} ({READS} reads each)");
}
