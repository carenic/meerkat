//! Store-level session hosting and the cross-process delivery wake (#1813).
//!
//! The hosting vocabulary (claims, capability, cold-delivery ownership) lives
//! in [`meerkat_core::session_hosting`]; this module adds what needs native
//! runtime machinery:
//!
//! - [`os_lock_if_trusted`]: a store's capability, downgraded when the OS
//!   lock cannot be trusted on its filesystem;
//! - `watch_delivery_store`: the shared SQLite change watch
//!   (`meerkat_sqlite::watch`, the contract the mob event bus uses) over a
//!   shared store's database file. Its ticks are a wake HINT: a coalesced
//!   file notification, or the bounded sweep when none arrived. A delivery
//!   owner answers a tick with cheap reads (the store's delivery generation,
//!   a try of the cold-delivery lock, the local routes of recipients it is
//!   waiting on) and reconciles only when one of them moved. A tick never
//!   grants permission, custody or expiry.

#[cfg(not(target_arch = "wasm32"))]
pub use meerkat_core::session_hosting::spawn_blocking_holding_claim;
pub use meerkat_core::session_hosting::{
    ColdDeliveryOwnership, HostingAcquisition, HostingCapability, HostingClaim,
    HostingClaimUnavailable, HostingOwner, HostingOwnerLossProof, HostingPaths, HostingRefused,
    OwnerLossAttachment, OwnerLossReservation, ServedElsewhere, SessionHostingAuthority,
    SessionServing, grant_session_hosting, local_session_serving, try_cold_delivery_ownership,
    with_write_hosting,
};

/// [`HostingCapability::OsLock`] over `paths`, unless the OS lock cannot be
/// trusted on their filesystem (network and userspace filesystems), in which
/// case the store reports [`HostingCapability::None`]: it never claims
/// multi-process safety it cannot prove. Explicit multi-process startup
/// refuses a `None` capability.
pub fn os_lock_if_trusted(paths: HostingPaths) -> HostingCapability {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match native::lock_filesystem_is_local(&paths.hosting_lock_dir) {
            Ok(true) => HostingCapability::OsLock(std::sync::Arc::new(paths)),
            Ok(false) => {
                tracing::warn!(
                    hosting_lock_dir = %paths.hosting_lock_dir.display(),
                    "session hosting locks are unreliable on this filesystem (network or \
                     userspace); the runtime store reports single-process hosting"
                );
                HostingCapability::None
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    hosting_lock_dir = %paths.hosting_lock_dir.display(),
                    "cannot determine whether session hosting locks are reliable here; the \
                     runtime store reports single-process hosting"
                );
                HostingCapability::None
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = paths;
        HostingCapability::None
    }
}

/// The default sweep interval of `watch_delivery_store`: the bound on how
/// long a missed file notification can delay a cross-process delivery.
#[cfg(all(not(target_arch = "wasm32"), feature = "sqlite-store"))]
pub const DELIVERY_STORE_SWEEP: std::time::Duration = meerkat_sqlite::watch::SQLITE_WATCH_SWEEP;
/// The default sweep interval of `watch_delivery_store` (a build without the
/// SQLite store never establishes the watch).
#[cfg(all(not(target_arch = "wasm32"), not(feature = "sqlite-store")))]
pub const DELIVERY_STORE_SWEEP: std::time::Duration = std::time::Duration::from_secs(5);

/// A store watch's file notification registration that the operating system
/// still held when the watch stopped (`DeliveryStoreWatch::stop`). Nothing
/// can interrupt that platform call (macOS FSEvents can hold it for tens of
/// seconds), so the stop cancels the registration instead: its thread stays
/// with the process until the call returns, then discards the result
/// (`meerkat_sqlite::watch::retained_registrations` lists it until then). It
/// never reads or writes the store.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetainedNotificationRegistration {
    /// The directory it registers: the store database's resolved parent
    /// directory, as the process listing names it.
    pub directory: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{DeliveryStoreWatch, watch_delivery_store};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};

    /// Whether OS file locks under `path` are reliable across processes of
    /// this kernel: a local filesystem, not a network or userspace one.
    #[cfg(target_os = "linux")]
    pub(super) fn lock_filesystem_is_local(path: &Path) -> std::io::Result<bool> {
        const NFS: i64 = 0x6969;
        const SMB: i64 = 0x517B;
        const CIFS: i64 = 0xFF53_4D42;
        const SMB2: i64 = 0xFE53_4D42;
        const V9FS: i64 = 0x0102_1997;
        const AFS: i64 = 0x5346_414F;
        const CEPH: i64 = 0x00C3_6400;
        const FUSE: i64 = 0x6573_5546;
        let probe = existing_ancestor(path)?;
        // `fs_type_t` is a signed or unsigned long depending on the libc target.
        #[allow(clippy::unnecessary_cast)]
        let kind = nix::sys::statfs::statfs(&probe)
            .map_err(std::io::Error::from)?
            .filesystem_type()
            .0 as i64;
        Ok(![NFS, SMB, CIFS, SMB2, V9FS, AFS, CEPH, FUSE].contains(&kind))
    }

    #[cfg(target_os = "macos")]
    pub(super) fn lock_filesystem_is_local(path: &Path) -> std::io::Result<bool> {
        let probe = existing_ancestor(path)?;
        let stat = nix::sys::statfs::statfs(&probe).map_err(std::io::Error::from)?;
        Ok(!matches!(
            stat.filesystem_type_name(),
            "nfs" | "smbfs" | "afpfs" | "webdav" | "cifs" | "macfuse" | "osxfuse"
        ))
    }

    /// Other platforms (Windows): `LockFileEx` locks are released by the OS
    /// when the holding process exits or its handle closes, so the lock is
    /// trusted.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub(super) fn lock_filesystem_is_local(_path: &Path) -> std::io::Result<bool> {
        Ok(true)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn existing_ancestor(path: &Path) -> std::io::Result<PathBuf> {
        let mut candidate = Some(path);
        while let Some(current) = candidate {
            if current.exists() {
                return Ok(current.to_path_buf());
            }
            candidate = current.parent();
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no existing ancestor of {}", path.display()),
        ))
    }

    /// A running wake watch over a shared store's database. Its tick
    /// receiver moves once per coalesced file notification and once per
    /// sweep interval without one. Dropping the watch stops it (the watch
    /// thread exits at its next wait); [`Self::stop`] also says what the
    /// stop left with the process.
    pub struct DeliveryStoreWatch {
        #[cfg(feature = "sqlite-store")]
        watch: meerkat_sqlite::watch::SqliteChangeWatch,
        ticks: crate::tokio::sync::watch::Receiver<u64>,
    }

    impl std::fmt::Debug for DeliveryStoreWatch {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("DeliveryStoreWatch").finish_non_exhaustive()
        }
    }

    impl DeliveryStoreWatch {
        /// Observe ticks. Each is a hint to re-read durable state, nothing
        /// more.
        pub fn subscribe_ticks(&self) -> crate::tokio::sync::watch::Receiver<u64> {
            self.ticks.clone()
        }

        /// Stop the watch as dropping it does, without waiting: its thread
        /// exits at its next wait. A file notification registration the
        /// operating system still holds is cancelled and returned: its
        /// thread stays with the process until the platform call returns.
        pub fn stop(self) -> Option<super::RetainedNotificationRegistration> {
            #[cfg(feature = "sqlite-store")]
            {
                // The request comes first: a registration still pending after
                // it discards its result when the platform call returns.
                self.watch.request_stop();
                self.watch.pending_registration().map(|pending| {
                    super::RetainedNotificationRegistration {
                        directory: pending.directory,
                    }
                })
            }
            #[cfg(not(feature = "sqlite-store"))]
            {
                None
            }
        }

        /// Where the watch's file notification registration stands.
        #[cfg(all(test, feature = "sqlite-store"))]
        pub(crate) fn notification_registration(
            &self,
        ) -> meerkat_sqlite::watch::SqliteWatchRegistration {
            self.watch.registration()
        }
    }

    /// Start the wake watch over `capability`'s store database, ticking at
    /// least every `sweep`. `Ok(None)` for a store without cross-process
    /// hosting; an error when the watch's sweep cannot start (no database
    /// file named, an unresolvable directory, no thread, or a build without
    /// this crate's `sqlite-store` feature), which an owner reports as
    /// unavailable cross-process wake and explicit multi-process startup
    /// refuses. It never waits for file notifications: they register on a
    /// thread of their own, and a store whose notifications cannot be
    /// registered still ticks every `sweep`.
    pub fn watch_delivery_store(
        capability: &meerkat_core::session_hosting::HostingCapability,
        sweep: std::time::Duration,
    ) -> Result<Option<DeliveryStoreWatch>, String> {
        let Some(paths) = capability.paths() else {
            return Ok(None);
        };
        #[cfg(not(feature = "sqlite-store"))]
        {
            let _ = (paths, sweep);
            Err(
                "this build has no SQLite store watch (meerkat-runtime's sqlite-store feature \
                 is off)"
                    .to_string(),
            )
        }
        #[cfg(feature = "sqlite-store")]
        watch_sqlite_delivery_store(paths, sweep)
    }

    #[cfg(feature = "sqlite-store")]
    fn watch_sqlite_delivery_store(
        paths: &meerkat_core::session_hosting::HostingPaths,
        sweep: std::time::Duration,
    ) -> Result<Option<DeliveryStoreWatch>, String> {
        let database = paths
            .database
            .as_ref()
            .ok_or_else(|| "the shared store names no database file to watch".to_string())?;
        let (ticks_tx, ticks) = crate::tokio::sync::watch::channel(0_u64);
        let watch = meerkat_sqlite::watch::start_sqlite_change_watch(
            database,
            "meerkat-delivery-store-watch",
            sweep,
            move |_tick| {
                if ticks_tx.is_closed() {
                    return meerkat_sqlite::watch::SqliteWatchControl::Stop;
                }
                ticks_tx.send_modify(|count| *count = count.wrapping_add(1));
                meerkat_sqlite::watch::SqliteWatchControl::Handled
            },
        )?;
        Ok(Some(DeliveryStoreWatch { watch, ticks }))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn paths(root: &std::path::Path, database: std::path::PathBuf) -> HostingPaths {
        HostingPaths {
            hosting_lock_dir: root.join("hosting"),
            cold_delivery_lock: root.join("delivery").join("cold-delivery.lock"),
            database: Some(database),
        }
    }

    #[test]
    fn a_local_store_gets_os_lock_hosting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let database = dir.path().join("runtime.sqlite3");
        assert!(os_lock_if_trusted(paths(dir.path(), database)).is_cross_process());
    }

    #[cfg(feature = "sqlite-store")]
    #[tokio::test]
    async fn a_store_file_write_ticks_the_delivery_store_watch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let database = dir.path().join("runtime.sqlite3");
        std::fs::write(&database, b"").expect("seed file");
        let capability =
            HostingCapability::OsLock(std::sync::Arc::new(paths(dir.path(), database.clone())));
        let watch = watch_delivery_store(&capability, DELIVERY_STORE_SWEEP)
            .expect("watch starts")
            .expect("an OsLock store is watched");
        // File notifications register after the watch starts. Once they
        // have, the write must tick through its notification, well before
        // the first sweep could.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while watch.notification_registration()
            == meerkat_sqlite::watch::SqliteWatchRegistration::Pending
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the notification registration ends within the hang guard"
            );
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert_eq!(
            watch.notification_registration(),
            meerkat_sqlite::watch::SqliteWatchRegistration::Registered
        );
        let mut ticks = watch.subscribe_ticks();
        std::fs::write(&database, b"commit").expect("write");
        tokio::time::timeout(DELIVERY_STORE_SWEEP / 2, ticks.changed())
            .await
            .expect("the write is observed through its notification, before any sweep")
            .expect("watch open");
    }

    /// The bound on a start, a tick or a stop that must not wait for a held
    /// notification registration. The hold itself lasts until the test
    /// releases it.
    #[cfg(feature = "sqlite-store")]
    const HELD_BOUND: std::time::Duration = std::time::Duration::from_secs(5);
    /// A short sweep, so a watch whose registration is held ticks well
    /// within the bound.
    #[cfg(feature = "sqlite-store")]
    const HELD_SWEEP: std::time::Duration = std::time::Duration::from_millis(50);

    /// A shared store in `root` whose file notification registrations are
    /// held, as a stalled macOS FSEvents registration holds them, until the
    /// returned hold is released or dropped.
    #[cfg(feature = "sqlite-store")]
    fn held_store(
        root: &std::path::Path,
    ) -> (
        meerkat_sqlite::watch::test_support::HeldRegistrations,
        HostingCapability,
    ) {
        let held = meerkat_sqlite::watch::test_support::hold_registrations_in(root)
            .expect("hold the store's registrations");
        let database = root.join("runtime.sqlite3");
        std::fs::write(&database, b"").expect("seed file");
        let capability = HostingCapability::OsLock(std::sync::Arc::new(paths(root, database)));
        (held, capability)
    }

    /// A held notification registration holds neither the start of the
    /// delivery store watch nor its sweep: the start returns while the
    /// registration is held, and the sweep ticks.
    #[cfg(feature = "sqlite-store")]
    #[tokio::test]
    async fn a_held_registration_holds_neither_the_delivery_store_watch_start_nor_its_sweep() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (held, capability) = held_store(dir.path());
        // A start that waited for the registration would block its thread
        // until the hold ends, which the unwind of a failed assertion does.
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let starter = std::thread::spawn(move || {
            let _ = started_tx.send(watch_delivery_store(&capability, HELD_SWEEP));
        });
        assert!(
            held.wait_entered(1, HELD_BOUND),
            "the registration reached its platform call"
        );
        let watch = started_rx
            .recv_timeout(HELD_BOUND)
            .expect("the start does not wait for the held registration")
            .expect("watch starts")
            .expect("an OsLock store is watched");
        let mut ticks = watch.subscribe_ticks();
        tokio::time::timeout(HELD_BOUND, ticks.changed())
            .await
            .expect("the sweep ticks while the registration is held")
            .expect("watch open");
        held.release();
        starter.join().expect("the starter returns");
    }

    #[cfg(feature = "sqlite-store")]
    fn retained_in(directory: &std::path::Path) -> usize {
        meerkat_sqlite::watch::retained_registrations()
            .iter()
            .filter(|retained| retained.directory == directory)
            .count()
    }

    /// Stopping the delivery store watch while its registration is held
    /// reports that registration, typed, by the directory the process
    /// listing names. The stop does not wait for it: the process keeps its
    /// thread until the platform call returns, then joins it.
    #[cfg(feature = "sqlite-store")]
    #[test]
    fn stopping_the_delivery_store_watch_reports_its_held_registration() {
        let dir = tempfile::tempdir().expect("tempdir");
        let directory = dir
            .path()
            .canonicalize()
            .expect("canonical store directory");
        let (held, capability) = held_store(dir.path());
        let watch = watch_delivery_store(&capability, HELD_SWEEP)
            .expect("watch starts")
            .expect("an OsLock store is watched");
        assert!(
            held.wait_entered(1, HELD_BOUND),
            "the registration reached its platform call"
        );
        assert_eq!(
            watch.notification_registration(),
            meerkat_sqlite::watch::SqliteWatchRegistration::Pending
        );

        let retained = watch.stop().expect("the held registration is reported");
        assert_eq!(retained.directory, directory);
        assert_eq!(
            retained_in(&directory),
            1,
            "the process keeps the registration's thread"
        );
        assert_eq!(held.live_sources(), 1);

        held.release();
        let deadline = std::time::Instant::now() + HELD_BOUND;
        while retained_in(&directory) > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            retained_in(&directory),
            0,
            "the process joins the thread once the platform call returns"
        );
        assert_eq!(
            (held.live_sources(), held.dropped_sources()),
            (0, 1),
            "the cancelled registration discarded its source"
        );
    }

    /// A watch whose registration completed reports nothing when it stops,
    /// and leaves nothing with the process.
    #[cfg(feature = "sqlite-store")]
    #[test]
    fn stopping_a_registered_delivery_store_watch_reports_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let directory = dir
            .path()
            .canonicalize()
            .expect("canonical store directory");
        let (held, capability) = held_store(dir.path());
        // Released before the start: the registration completes at once.
        held.release();
        let watch = watch_delivery_store(&capability, HELD_SWEEP)
            .expect("watch starts")
            .expect("an OsLock store is watched");
        let deadline = std::time::Instant::now() + HELD_BOUND;
        while watch.notification_registration()
            == meerkat_sqlite::watch::SqliteWatchRegistration::Pending
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(
            watch.notification_registration(),
            meerkat_sqlite::watch::SqliteWatchRegistration::Registered
        );
        assert_eq!(watch.stop(), None);
        assert_eq!(retained_in(&directory), 0);
    }

    #[cfg(feature = "sqlite-store")]
    #[test]
    fn process_local_stores_are_never_watched() {
        assert!(matches!(
            watch_delivery_store(&HostingCapability::ProcessLocal, DELIVERY_STORE_SWEEP),
            Ok(None)
        ));
    }
}
