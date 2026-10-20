//! A connection kept open across an archive by rename must not touch the
//! database later created at its old path when it finally closes.
//!
//! SQLite checks on close that the database file has not moved (the unix
//! VFS compares the inode at the path with the open descriptor's) and then
//! neither checkpoints nor unlinks the WAL and shared-memory files by name.
//! The kept delivery reader relies on that only as a second line: the
//! maintenance fence waits for it to close first. This control fails if a
//! SQLite or VFS change reopens the hazard. Unix only: it renames an open
//! database, and the kept reader runs on unix only.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

#[derive(Clone, Copy, Debug)]
enum Archive {
    /// Rename the database's directory, as the migrate verb's archive does.
    Directory,
    /// Rename only the database file.
    File,
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn kept_connection_survives_an_archive(archive: Archive) {
    let root = tempfile::tempdir().expect("tempdir");
    let realm = root.path().join("realm");
    std::fs::create_dir(&realm).expect("realm");
    let db = realm.join("runtime.sqlite3");
    let wal = realm.join("runtime.sqlite3-wal");
    let shm = realm.join("runtime.sqlite3-shm");
    {
        let writer = Connection::open(&db).expect("old writer");
        writer
            .pragma_update(None, "journal_mode", "WAL")
            .expect("wal");
        writer
            .execute_batch(
                "CREATE TABLE g(id INTEGER PRIMARY KEY, v INTEGER); INSERT INTO g VALUES (1, 111);",
            )
            .expect("old rows");
    }
    // The kept connection reads once and stays open.
    let kept = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_WRITE).expect("kept");
    let old: i64 = kept
        .query_row("SELECT v FROM g WHERE id = 1", [], |row| row.get(0))
        .expect("kept read");
    assert_eq!(old, 111);
    // Another session commits while the connection is kept: its close cannot
    // checkpoint past the kept connection, so the WAL keeps frames.
    {
        let other = Connection::open(&db).expect("another writer");
        other
            .execute("INSERT INTO g VALUES (2, 333)", [])
            .expect("another commit");
    }
    assert!(
        std::fs::metadata(&wal).expect("wal").len() > 0,
        "the WAL keeps frames past the kept connection"
    );

    match archive {
        Archive::Directory => {
            std::fs::rename(&realm, root.path().join("realm.archived")).expect("archive dir");
            std::fs::create_dir(&realm).expect("new realm");
        }
        Archive::File => {
            std::fs::rename(&db, realm.join("runtime.sqlite3.archived")).expect("archive file");
        }
    }
    // A new database at the old path, its writer open and its sidecars live.
    let fresh = Connection::open(&db).expect("new writer");
    fresh
        .pragma_update(None, "journal_mode", "WAL")
        .expect("new wal");
    fresh
        .execute_batch(
            "CREATE TABLE h(id INTEGER PRIMARY KEY, v INTEGER); INSERT INTO h VALUES (1, 222);",
        )
        .expect("new rows");
    assert!(exists(&wal) && exists(&shm), "the new database's sidecars");

    kept.close()
        .map_err(|(_, error)| error)
        .expect("the kept connection closes");

    assert!(
        exists(&wal) && exists(&shm),
        "{archive:?}: closing the kept connection left the new database's sidecars alone"
    );
    let probe = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("probe");
    let row: i64 = probe
        .query_row("SELECT v FROM h WHERE id = 1", [], |row| row.get(0))
        .expect("a new connection reads the new database");
    assert_eq!(row, 222);
    let check: String = probe
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity check");
    assert_eq!(check, "ok", "{archive:?}");
    let old_tables: i64 = probe
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'g'",
            [],
            |row| row.get(0),
        )
        .expect("schema read");
    assert_eq!(
        old_tables, 0,
        "{archive:?}: the old database's WAL was not replayed into the new file"
    );
    drop(fresh);
}

#[test]
fn a_kept_connection_closing_after_a_directory_archive_leaves_the_new_database_whole() {
    kept_connection_survives_an_archive(Archive::Directory);
}

#[test]
fn a_kept_connection_closing_after_a_file_archive_leaves_the_new_database_whole() {
    kept_connection_survives_an_archive(Archive::File);
}
