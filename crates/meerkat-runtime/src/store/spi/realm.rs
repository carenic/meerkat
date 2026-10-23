//! One realm over a host store: the fence, the lease, the scopes and the
//! commit engine every host store adapter of the process shares.
//!
//! A process opens ONE [`SpiRealm`] per realm (M3): it acquires the fence,
//! renews it, keeps the local lease clock, lists every head once, and owns
//! the per-scope journal state. The runtime adapter
//! ([`super::SpiRuntimeStore`]) and the mob adapter (in meerkat-mob) both
//! commit through it, so the second adapter never fences out the first.
//!
//! # Concurrency (M1)
//!
//! There is no realm-wide lock on the commit path. Each scope has its own
//! async mutex ([`ScopeCell`]); a call locks the cells of the scopes it
//! touches, in [`ScopeId`] order, and holds them across its host commit, so
//! no other call observes a scope's state before the host acknowledged it.
//! Adapters add a realm gate on top (shared for single-scope calls,
//! exclusive for cross-scope reads); the realm itself never serializes two
//! scopes' commits.
//!
//! # Fencing (K3 writer rules, A9/K2-1)
//!
//! - Open acquires with [`AcquireMode::Exclusive`] by default: a live holder
//!   answers [`SpiError::Held`] with what remains of its lease. After an
//!   acquire that superseded a live holder (a takeover), the open waits out
//!   [`FenceGrant::superseded_lease`](meerkat_core::host_store::FenceGrant)
//!   before the realm serves anything.
//! - A background task renews once per the host's renew interval
//!   ([`HostStore::fence_timing`]). A fenced out answer poisons the realm;
//!   any other failed renew stops writes until a renew succeeds.
//! - **Self-fence on the local clock:** a mutation runs only while the last
//!   renew did not fail and the last successful acquire or renew *started*
//!   less than [`FenceTiming::self_fence_window`] ago. Otherwise the call
//!   renews synchronously first, and refuses when that fails. In the steady
//!   state this is a local clock read: no store I/O is added to a round.
//!   Every per-round commit of a run (commits A and B) is such a mutation,
//!   so a writer past its window dispatches no tool and makes no model call
//!   after its next commit point.
//!
//! # Failure (D3)
//!
//! A batch that is refused, or whose outcome stays unknown after the retry
//! budget, poisons every scope it touched: the process may hold state the
//! host does not, so those scopes refuse every later call until the process
//! reopens the realm. A fence loss poisons the whole realm. An unknown
//! outcome is always settled (by retrying the same batch, or by reading the
//! head's commit id) before a scope's next version is written (C-2), and a
//! poisoned scope writes no next version at all.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use meerkat_core::host_store::{
    AcquireMode, CommitBatch, CommitId, FenceTiming, FenceToken, HostStore, HostStoreError,
    MAX_BATCH_BYTES, MAX_ROW_BYTES, Row, RowIndex, ScopeKind, ScopeName, ScopeRef, ScopeUpdate,
    ScopeVersion,
};
use std::sync::atomic::AtomicBool;
use tokio::sync::{Mutex, OwnedMutexGuard};

use super::SpiError;
use super::codec::{
    self, CHUNK_TAG, ChunkHeadV1, ChunkRef, FORMAT_V1, PinnedV1, RowOp, ScopeHeadV1, WindowV1,
};

/// A row entry value larger than this goes to a chunk scope.
pub const INLINE_VALUE_LIMIT: usize = 256 * 1024;

/// A pinned (head) value larger than this goes to a chunk scope.
pub const PINNED_INLINE_LIMIT: usize = 32 * 1024;

/// A head is kept below this; larger inline pinned values move to chunks.
pub const HEAD_LIMIT: usize = 256 * 1024;

/// The most head and row bytes the journal puts in one batch, leaving the
/// rest of [`MAX_BATCH_BYTES`] as margin.
pub const BATCH_BUDGET: usize = MAX_BATCH_BYTES - 512 * 1024;

/// Chunk rows written per batch.
pub const CHUNK_ROWS_PER_BATCH: usize = 7;

/// A checkpoint is written once the rows appended since the last one pass
/// `max(CHECKPOINT_FACTOR x live bytes, CHECKPOINT_MIN_BYTES)` (S4).
pub const CHECKPOINT_FACTOR: u64 = 4;

/// See [`CHECKPOINT_FACTOR`].
pub const CHECKPOINT_MIN_BYTES: u64 = 4 * 1024 * 1024;

/// Rows read per host call when loading.
const READ_PAGE_ROWS: usize = 16;

/// Heads read per host call when listing.
const LIST_PAGE_HEADS: usize = 256;

/// Values at least this large keep their allocation in the stamp, so an
/// unchanged value (the same allocation) is recognized without hashing.
const SHARED_STAMP_MIN: usize = 64 * 1024;

/// The scope kinds a boot lists.
const LISTED_KINDS: [ScopeKind; 4] = [
    ScopeKind::Session,
    ScopeKind::Identity,
    ScopeKind::Mob,
    ScopeKind::Record,
];

/// How a realm opens and how long it retries. The fence timing (renew
/// interval, lease, self-fence window) is the host's own
/// ([`HostStore::fence_timing`]).
#[derive(Debug, Clone)]
pub struct SpiRealmConfig {
    /// How the open acquires the fence.
    pub acquire: AcquireMode,
    /// How long one renew may take before it counts as failed.
    pub renew_timeout: Duration,
    /// How long an unknown or unavailable commit is retried before the
    /// scopes it touched are poisoned.
    pub commit_retry_budget: Duration,
}

impl Default for SpiRealmConfig {
    fn default() -> Self {
        Self {
            acquire: AcquireMode::Exclusive,
            renew_timeout: Duration::from_secs(5),
            commit_retry_budget: Duration::from_secs(30),
        }
    }
}

/// A family of journaled scopes: its tag (the scope name prefix), its kind,
/// and the entry groups it keeps in the head (pinned: readable at boot
/// without loading the body).
#[derive(Debug, Clone, Copy)]
pub struct ScopeFamily {
    pub tag: &'static str,
    pub kind: ScopeKind,
    pub pinned: &'static [&'static str],
}

impl ScopeFamily {
    /// The group of an entry key: the text before its first `/`.
    #[must_use]
    pub fn group(entry_key: &str) -> &str {
        entry_key
            .split_once('/')
            .map_or(entry_key, |(group, _)| group)
    }

    /// Whether the family keeps this entry in the head.
    #[must_use]
    pub fn is_pinned(&self, entry_key: &str) -> bool {
        self.pinned.contains(&Self::group(entry_key))
    }
}

impl PartialEq for ScopeFamily {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
    }
}

impl Eq for ScopeFamily {}

impl std::hash::Hash for ScopeFamily {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.tag.hash(state);
    }
}

impl PartialOrd for ScopeFamily {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScopeFamily {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.tag.cmp(other.tag)
    }
}

/// The logical identity of a journaled scope: its family and its key. The
/// host name of its current incarnation is kept in its [`ScopeState`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId {
    pub family: ScopeFamily,
    pub key: String,
}

impl ScopeId {
    #[must_use]
    pub fn new(family: ScopeFamily, key: impl Into<String>) -> Self {
        Self {
            family,
            key: key.into(),
        }
    }
}

impl std::fmt::Display for ScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.family.tag, self.key)
    }
}

/// One scope's lock and journal state.
pub type ScopeCell = Mutex<ScopeState>;

/// A locked scope.
pub type ScopeGuard = OwnedMutexGuard<ScopeState>;

#[derive(Debug, Clone)]
enum Pinned {
    Inline(Arc<Vec<u8>>),
    Chunk(ChunkRef),
}

/// What the journal keeps of one committed row entry.
#[derive(Debug, Clone)]
struct Stamp {
    digest: [u8; 32],
    len: u64,
    /// The chunk holding the value, when it is not inline.
    chunk: Option<ChunkRef>,
    /// The committed allocation of a large value (see [`SHARED_STAMP_MIN`]).
    shared: Option<Arc<Vec<u8>>>,
    /// The bytes the entry takes in a row.
    op_bytes: u64,
}

impl Stamp {
    fn matches(&self, value: &Arc<Vec<u8>>) -> bool {
        if let Some(shared) = &self.shared
            && Arc::ptr_eq(shared, value)
        {
            return true;
        }
        self.len == value.len() as u64 && self.digest == codec::sha256(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Body {
    /// Only the head is known (a listed scope not opened yet).
    Unloaded,
    /// The rows are folded into `rows` (or the scope does not exist yet).
    Loaded,
}

/// One scope's journal state: what the host holds for it, as far as this
/// process committed or loaded it.
#[derive(Debug)]
pub struct ScopeState {
    id: ScopeId,
    name: Option<ScopeName>,
    version: Option<ScopeVersion>,
    pinned: BTreeMap<String, Pinned>,
    rows: BTreeMap<String, Stamp>,
    window: WindowV1,
    live_bytes: u64,
    since_checkpoint: u64,
    /// Chunks referenced from rows of the window (spills), by row index.
    row_chunks: Vec<(u64, ChunkRef)>,
    body: Body,
    poison: Option<String>,
}

impl ScopeState {
    fn empty(id: ScopeId) -> Self {
        Self {
            id,
            name: None,
            version: None,
            pinned: BTreeMap::new(),
            rows: BTreeMap::new(),
            window: WindowV1::default(),
            live_bytes: 0,
            since_checkpoint: 0,
            row_chunks: Vec::new(),
            body: Body::Loaded,
            poison: None,
        }
    }

    fn from_head(
        id: ScopeId,
        name: ScopeName,
        version: ScopeVersion,
        head: &ScopeHeadV1,
    ) -> Result<Self, SpiError> {
        let mut pinned = BTreeMap::new();
        for (key, value) in &head.pin {
            let entry = match value {
                PinnedV1::Utf8(text) => Pinned::Inline(Arc::new(text.as_bytes().to_vec())),
                PinnedV1::Hex(text) => Pinned::Inline(Arc::new(codec::unhex(text)?)),
                PinnedV1::Chunk(chunk) => Pinned::Chunk(ChunkRef::from_wire(chunk)?),
            };
            pinned.insert(key.clone(), entry);
        }
        Ok(Self {
            id,
            name: Some(name),
            version: Some(version),
            pinned,
            rows: BTreeMap::new(),
            window: head.w,
            live_bytes: head.lb,
            since_checkpoint: head.sb,
            row_chunks: Vec::new(),
            body: if head.w.s == head.w.e {
                Body::Loaded
            } else {
                Body::Unloaded
            },
            poison: None,
        })
    }

    #[must_use]
    pub fn id(&self) -> &ScopeId {
        &self.id
    }

    /// Whether the host holds an incarnation of this scope.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.name.is_some()
    }

    /// Whether the rows are folded (or there are none).
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.body == Body::Loaded
    }

    /// The poison cause, when this scope is poisoned.
    #[must_use]
    pub fn poison(&self) -> Option<&str> {
        self.poison.as_deref()
    }

    /// Whether the next commit should carry the scope's full image, so the
    /// journal writes a checkpoint (S4: rows since the last checkpoint pass
    /// `max(4 x live bytes, 4 MiB)`).
    #[must_use]
    pub fn wants_full_image(&self) -> bool {
        self.since_checkpoint
            > (CHECKPOINT_FACTOR.saturating_mul(self.live_bytes)).max(CHECKPOINT_MIN_BYTES)
    }

    /// The committed entry keys under `group` (the entry `group` itself and
    /// every `group/...`), pinned and row entries alike.
    #[must_use]
    pub fn entry_keys_in_group(&self, group: &str) -> Vec<String> {
        let prefix = format!("{group}/");
        self.pinned
            .keys()
            .chain(self.rows.keys())
            .filter(|key| key.as_str() == group || key.starts_with(&prefix))
            .cloned()
            .collect()
    }

    /// Forget the folded rows (the body is dropped from memory). The next
    /// commit needs a [`SpiRealm::load_body`] first.
    pub fn unload(&mut self) {
        if self.window.s != self.window.e {
            self.rows.clear();
            self.row_chunks.clear();
            self.body = Body::Unloaded;
        }
    }

    fn chunk_refs(&self) -> Vec<ChunkRef> {
        let mut refs: Vec<ChunkRef> = self
            .pinned
            .values()
            .filter_map(|pinned| match pinned {
                Pinned::Chunk(chunk) => Some(chunk.clone()),
                Pinned::Inline(_) => None,
            })
            .collect();
        refs.extend(self.rows.values().filter_map(|stamp| stamp.chunk.clone()));
        refs.extend(self.row_chunks.iter().map(|(_, chunk)| chunk.clone()));
        refs
    }

    fn head(
        &self,
        pinned: &BTreeMap<String, Pinned>,
        window: WindowV1,
        live: u64,
        since: u64,
    ) -> Result<Vec<u8>, SpiError> {
        let pin = pinned
            .iter()
            .map(|(key, value)| {
                let wire = match value {
                    Pinned::Inline(bytes) => codec::pin_inline(bytes),
                    Pinned::Chunk(chunk) => PinnedV1::Chunk(chunk.to_wire()),
                };
                (key.clone(), wire)
            })
            .collect();
        ScopeHeadV1 {
            f: FORMAT_V1,
            fam: self.id.family.tag.to_string(),
            key: self.id.key.clone(),
            pin,
            w: window,
            lb: live,
            sb: since,
        }
        .encode()
    }
}

/// What one call changes in one locked scope.
#[derive(Debug, Clone)]
pub enum ScopeDelta {
    /// These entries are set (`Some`) or removed (`None`); the rest stay.
    Changes(Vec<(String, Option<Arc<Vec<u8>>>)>),
    /// The scope's complete image: every entry not in it is removed. Lets
    /// the journal write a checkpoint ([`ScopeState::wants_full_image`]).
    Full(BTreeMap<String, Arc<Vec<u8>>>),
}

/// A listed head not yet claimed by an adapter.
#[derive(Debug, Clone)]
struct ListedHead {
    name: ScopeName,
    version: ScopeVersion,
    head: ScopeHeadV1,
}

#[derive(Debug, Clone)]
struct ChunkMeta {
    version: ScopeVersion,
    owner: String,
}

/// A chunk to write before the batch that references it.
#[derive(Debug, Clone)]
struct PendingChunk {
    chunk: ChunkRef,
    owner: ScopeName,
    bytes: Arc<Vec<u8>>,
}

/// The planned update of one scope.
struct ScopePlan {
    index: usize,
    family_tag: &'static str,
    key: String,
    old_window: WindowV1,
    old_since: u64,
    scope: ScopeRef,
    previous: Option<ScopeVersion>,
    next: ScopeVersion,
    delete: bool,
    pinned_after: BTreeMap<String, Pinned>,
    rows: Vec<Row>,
    checkpoint: bool,
    row_changes: Vec<(String, Option<Stamp>)>,
    live_after: u64,
    new_chunks: Vec<PendingChunk>,
    dead_chunks: Vec<ChunkRef>,
    spill: Option<ChunkRef>,
}

impl ScopePlan {
    fn row_bytes(&self) -> usize {
        self.rows.iter().map(Row::len).sum()
    }
}

/// The shared realm handle. See the module documentation.
pub struct SpiRealm {
    host: Arc<dyn HostStore>,
    config: SpiRealmConfig,
    timing: FenceTiming,
    fence: FenceToken,
    writer: String,
    counter: AtomicU64,
    lease_start: StdMutex<Instant>,
    renew_failed: AtomicBool,
    renew_gate: Mutex<()>,
    realm_poison: StdMutex<Option<String>>,
    poisoned_scopes: AtomicUsize,
    cells: StdMutex<HashMap<ScopeId, Arc<ScopeCell>>>,
    listed: StdMutex<HashMap<String, Vec<ListedHead>>>,
    /// Families claimed so far, with the refusal of a claim that failed.
    claimed: StdMutex<HashMap<&'static str, Result<(), String>>>,
    chunks: StdMutex<HashMap<String, ChunkMeta>>,
    /// Per-realm adapter state, one value per type ([`SpiRealm::extension`]).
    extensions: Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl std::fmt::Debug for SpiRealm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpiRealm")
            .field("fence", &self.fence)
            .field("writer", &self.writer)
            .field("poisoned", &self.realm_poisoned())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn next_version(previous: Option<ScopeVersion>) -> Result<ScopeVersion, SpiError> {
    match previous {
        None => Ok(ScopeVersion::FIRST),
        Some(previous) => previous
            .next()
            .ok_or_else(|| SpiError::Corrupt("a scope version overflowed".into())),
    }
}

fn op_bytes(key: &str, value_len: usize) -> u64 {
    (1 + 4 + key.len() + 4 + value_len) as u64
}

impl SpiRealm {
    /// Acquire the realm's fence, start renewing it, wait out a superseded
    /// holder's lease, and list every head (C-5: after the acquire, before
    /// the first commit). Chunks whose owner no longer exists are deleted.
    ///
    /// # Errors
    /// [`SpiError::Held`] when an exclusive acquire meets a live holder; the
    /// host retries after `remaining`, or takes over.
    pub async fn open(
        host: Arc<dyn HostStore>,
        config: SpiRealmConfig,
    ) -> Result<Arc<Self>, SpiError> {
        let timing = host.fence_timing();
        let started = Instant::now();
        let grant = match host.acquire_fence(config.acquire).await {
            Ok(grant) => grant,
            Err(HostStoreError::Held { holder, remaining }) => {
                return Err(SpiError::Held { holder, remaining });
            }
            Err(error) => return Err(SpiError::Host(error)),
        };
        let realm = Arc::new(Self {
            host,
            timing,
            fence: grant.token,
            writer: uuid::Uuid::now_v7().simple().to_string(),
            counter: AtomicU64::new(0),
            lease_start: StdMutex::new(started),
            renew_failed: AtomicBool::new(false),
            renew_gate: Mutex::new(()),
            realm_poison: StdMutex::new(None),
            poisoned_scopes: AtomicUsize::new(0),
            cells: StdMutex::new(HashMap::new()),
            listed: StdMutex::new(HashMap::new()),
            claimed: StdMutex::new(HashMap::new()),
            chunks: StdMutex::new(HashMap::new()),
            extensions: Mutex::new(HashMap::new()),
            config,
        });
        realm.spawn_renewal();
        if !grant.superseded_lease.is_zero() {
            // The superseded holder fences itself a renew interval before
            // its recorded lease ends; nothing here serves until then.
            tokio::time::sleep(grant.superseded_lease).await;
        }
        realm.list_all().await?;
        realm.sweep_ownerless_chunks().await;
        Ok(realm)
    }

    /// The fence token this realm commits with.
    #[must_use]
    pub fn fence(&self) -> FenceToken {
        self.fence
    }

    /// The options this realm was opened with.
    #[must_use]
    pub fn config(&self) -> &SpiRealmConfig {
        &self.config
    }

    /// The host's fence timing.
    #[must_use]
    pub fn timing(&self) -> FenceTiming {
        self.timing
    }

    /// The realm poison cause, when the fence was lost.
    #[must_use]
    pub fn realm_poisoned(&self) -> Option<String> {
        lock(&self.realm_poison).clone()
    }

    /// Whether any scope is poisoned.
    #[must_use]
    pub fn has_poisoned_scopes(&self) -> bool {
        self.poisoned_scopes.load(Ordering::Acquire) > 0
    }

    /// Refuse when the realm is poisoned.
    pub fn check_realm(&self) -> Result<(), SpiError> {
        match self.realm_poisoned() {
            Some(cause) => Err(SpiError::RealmPoisoned(cause)),
            None => Ok(()),
        }
    }

    /// Refuse a cross-scope read while the realm or any scope is poisoned:
    /// a poisoned scope's memory may be ahead of the host (D3).
    pub fn check_cross_scope_read(&self) -> Result<(), SpiError> {
        self.check_realm()?;
        if self.has_poisoned_scopes() {
            return Err(SpiError::RealmPoisoned(
                "a scope is poisoned; cross-scope reads refuse until the realm is reopened".into(),
            ));
        }
        Ok(())
    }

    /// Refuse a call on a poisoned scope (or realm).
    pub fn check_scope(&self, state: &ScopeState) -> Result<(), SpiError> {
        self.check_realm()?;
        match &state.poison {
            Some(cause) => Err(SpiError::ScopePoisoned {
                scope: state.id.to_string(),
                cause: cause.clone(),
            }),
            None => Ok(()),
        }
    }

    fn lease_started(&self) -> Instant {
        *lock(&self.lease_start)
    }

    fn renewed(&self, started: Instant) {
        let mut slot = lock(&self.lease_start);
        if started > *slot {
            *slot = started;
        }
        self.renew_failed.store(false, Ordering::Release);
    }

    /// Whether this writer may write: its last renew did not fail, and its
    /// last successful acquire or renew started less than the host's
    /// self-fence window ago. A local clock read only, no store I/O.
    #[must_use]
    pub fn lease_is_live(&self) -> bool {
        !self.renew_failed.load(Ordering::Acquire)
            && self.lease_started().elapsed() < self.timing.self_fence_window()
    }

    /// Make sure the local lease is live before a mutation: a no-op while it
    /// is, otherwise one synchronous renew.
    ///
    /// # Errors
    /// [`SpiError::LeaseLapsed`] when the renew fails or times out (nothing
    /// is poisoned: the caller has not mutated anything yet), and
    /// [`SpiError::RealmPoisoned`] when the realm was fenced out.
    pub async fn ensure_lease(&self) -> Result<(), SpiError> {
        self.check_realm()?;
        if self.lease_is_live() {
            return Ok(());
        }
        let _gate = self.renew_gate.lock().await;
        if self.lease_is_live() {
            return Ok(());
        }
        let started = Instant::now();
        match tokio::time::timeout(self.config.renew_timeout, self.host.renew_fence(self.fence))
            .await
        {
            Ok(Ok(())) => {
                self.renewed(started);
                Ok(())
            }
            Ok(Err(HostStoreError::FencedOut { current })) => Err(self.poison_realm(format!(
                "fenced out: token {current} superseded token {}",
                self.fence
            ))),
            Ok(Err(error)) => Err(SpiError::LeaseLapsed(format!(
                "the lease lapsed and a renew failed: {error}"
            ))),
            Err(_) => Err(SpiError::LeaseLapsed(
                "the lease lapsed and a renew timed out".into(),
            )),
        }
    }

    fn poison_realm(&self, cause: String) -> SpiError {
        let mut slot = lock(&self.realm_poison);
        let cause = slot.get_or_insert(cause).clone();
        tracing::error!(%cause, "host store realm poisoned; every call refuses until it is reopened");
        SpiError::RealmPoisoned(cause)
    }

    /// Poison one scope: it refuses every call until the realm reopens.
    /// For adapters whose memory may now differ from the host for it.
    pub fn poison(&self, state: &mut ScopeState, cause: &str) {
        self.poison_scope(state, cause);
    }

    /// Poison the whole realm (an adapter rule was broken and the affected
    /// scope could not be singled out).
    pub fn poison_all(&self, cause: String) -> SpiError {
        self.poison_realm(cause)
    }

    fn poison_scope(&self, state: &mut ScopeState, cause: &str) {
        if state.poison.is_none() {
            state.poison = Some(cause.to_string());
            self.poisoned_scopes.fetch_add(1, Ordering::AcqRel);
            tracing::error!(scope = %state.id, %cause, "host store scope poisoned; it refuses until the realm is reopened");
        }
    }

    fn spawn_renewal(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let interval = self.timing.renew_interval();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(realm) = weak.upgrade() else {
                    return;
                };
                if realm.realm_poisoned().is_some() {
                    return;
                }
                let started = Instant::now();
                match tokio::time::timeout(
                    realm.config.renew_timeout,
                    realm.host.renew_fence(realm.fence),
                )
                .await
                {
                    Ok(Ok(())) => realm.renewed(started),
                    Ok(Err(HostStoreError::FencedOut { current })) => {
                        realm.poison_realm(format!(
                            "fenced out: token {current} superseded token {}",
                            realm.fence
                        ));
                        return;
                    }
                    Ok(Err(error)) => {
                        // Stop writing as soon as a renew fails: the next
                        // mutation renews synchronously or refuses.
                        realm.renew_failed.store(true, Ordering::Release);
                        tracing::warn!(%error, "host store fence renew failed; writes stop until a renew succeeds");
                    }
                    Err(_) => {
                        realm.renew_failed.store(true, Ordering::Release);
                        tracing::warn!(
                            "host store fence renew timed out; writes stop until a renew succeeds"
                        );
                    }
                }
            }
        });
    }

    fn next_token(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn commit_id(&self) -> Result<CommitId, SpiError> {
        CommitId::try_new(format!("{}-{}", self.writer, self.next_token())).map_err(SpiError::Host)
    }

    fn incarnation(&self) -> String {
        format!("{}.{}", self.writer, self.next_token())
    }

    async fn list_all(&self) -> Result<(), SpiError> {
        let mut listed: HashMap<String, Vec<ListedHead>> = HashMap::new();
        let mut chunks: HashMap<String, ChunkMeta> = HashMap::new();
        for kind in LISTED_KINDS {
            let mut after: Option<ScopeName> = None;
            loop {
                let page = self
                    .host
                    .list_heads(kind, after.as_ref(), LIST_PAGE_HEADS)
                    .await?;
                for scope in page.heads {
                    let Some(tag) = codec::name_tag(&scope.name).map(str::to_string) else {
                        // Not a name this journal writes: leave it alone.
                        continue;
                    };
                    if tag == CHUNK_TAG {
                        if kind != ScopeKind::Record {
                            return Err(SpiError::Corrupt(format!(
                                "chunk scope {} has kind {kind}",
                                scope.name
                            )));
                        }
                        let head = ChunkHeadV1::decode(&scope.head.head)?;
                        chunks.insert(
                            scope.name.as_str().to_string(),
                            ChunkMeta {
                                version: scope.head.version,
                                owner: head.owner,
                            },
                        );
                        continue;
                    }
                    let head = ScopeHeadV1::decode(&scope.head.head)?;
                    if head.fam != tag || !codec::name_encodes(&scope.name, &tag, &head.key) {
                        return Err(SpiError::Corrupt(format!(
                            "scope {} does not encode its head's family {} and key",
                            scope.name, head.fam
                        )));
                    }
                    listed.entry(tag).or_default().push(ListedHead {
                        name: scope.name,
                        version: scope.head.version,
                        head,
                    });
                }
                match page.next {
                    Some(next) => after = Some(next),
                    None => break,
                }
            }
        }
        *lock(&self.listed) = listed;
        *lock(&self.chunks) = chunks;
        Ok(())
    }

    /// Delete chunks whose owner is not a listed scope: garbage of a crash
    /// between a chunk and the batch that would have referenced it, or of a
    /// deleted owner. Best effort; a failure leaves garbage, not corruption.
    async fn sweep_ownerless_chunks(&self) {
        let owners: HashSet<String> = lock(&self.listed)
            .values()
            .flatten()
            .map(|listed| listed.name.as_str().to_string())
            .collect();
        let doomed: Vec<(String, ScopeVersion)> = lock(&self.chunks)
            .iter()
            .filter(|(_, meta)| !owners.contains(&meta.owner))
            .map(|(name, meta)| (name.clone(), meta.version))
            .collect();
        for group in doomed.chunks(64) {
            if let Err(error) = self.delete_chunks(group).await {
                tracing::warn!(%error, "could not delete orphaned host store chunks");
                return;
            }
        }
    }

    async fn delete_chunks(&self, chunks: &[(String, ScopeVersion)]) -> Result<(), SpiError> {
        if chunks.is_empty() {
            return Ok(());
        }
        let mut updates = Vec::with_capacity(chunks.len());
        for (name, version) in chunks {
            updates.push(ScopeUpdate {
                scope: ScopeRef::new(
                    ScopeKind::Record,
                    ScopeName::try_new(name.clone()).map_err(SpiError::Host)?,
                ),
                previous: Some(*version),
                next: next_version(Some(*version))?,
                head: None,
                append: Vec::new(),
                truncate_after: None,
                drop_before: None,
                delete: true,
            });
        }
        let batch = CommitBatch::try_new(self.commit_id()?, self.fence, updates)?;
        self.commit_settled(&batch).await?;
        let mut table = lock(&self.chunks);
        for (name, _) in chunks {
            table.remove(name);
        }
        Ok(())
    }

    /// Claim the listed scopes of `family`: register their cells (head
    /// only, bodies unloaded) and answer them in key order. A family is
    /// claimed once per realm: a later claim answers the family's registered
    /// cells, or the first claim's refusal again.
    ///
    /// # Errors
    /// [`SpiError::Corrupt`] when two live incarnations claim one key.
    pub fn claim(&self, family: ScopeFamily) -> Result<Vec<Arc<ScopeCell>>, SpiError> {
        let mut claimed = lock(&self.claimed);
        match claimed.get(family.tag) {
            Some(Ok(())) => return Ok(self.cells_of(family)),
            Some(Err(cause)) => return Err(SpiError::Corrupt(cause.clone())),
            None => {}
        }
        let outcome = self.claim_listed(family);
        claimed.insert(
            family.tag,
            outcome.as_ref().map(|_| ()).map_err(ToString::to_string),
        );
        outcome
    }

    fn cells_of(&self, family: ScopeFamily) -> Vec<Arc<ScopeCell>> {
        let cells = lock(&self.cells);
        let mut found: Vec<(&ScopeId, &Arc<ScopeCell>)> =
            cells.iter().filter(|(id, _)| id.family == family).collect();
        found.sort_by_key(|(id, _)| *id);
        found
            .into_iter()
            .map(|(_, cell)| Arc::clone(cell))
            .collect()
    }

    fn claim_listed(&self, family: ScopeFamily) -> Result<Vec<Arc<ScopeCell>>, SpiError> {
        let listed = lock(&self.listed).remove(family.tag).unwrap_or_default();
        let mut cells = lock(&self.cells);
        let mut claimed = BTreeMap::new();
        for entry in listed {
            if entry.head.fam != family.tag {
                return Err(SpiError::Corrupt(format!(
                    "scope {} names family {}",
                    entry.name, entry.head.fam
                )));
            }
            let id = ScopeId::new(family, entry.head.key.clone());
            if cells.contains_key(&id) || claimed.contains_key(&id) {
                return Err(SpiError::Corrupt(format!(
                    "two live incarnations of {id} exist"
                )));
            }
            let state = ScopeState::from_head(id.clone(), entry.name, entry.version, &entry.head)?;
            claimed.insert(id, Arc::new(Mutex::new(state)));
        }
        let mut out = Vec::with_capacity(claimed.len());
        for (id, cell) in claimed {
            cells.insert(id, Arc::clone(&cell));
            out.push(cell);
        }
        Ok(out)
    }

    /// The realm's one value of type `T`, created by `init` on first use:
    /// per-realm adapter state (such as the mob adapter's registry of open
    /// storages) without a process-wide registry. A failed `init` stores
    /// nothing. Hold the realm only weakly from `T`: the realm keeps `T`.
    pub async fn extension<T, E, F, Fut>(&self, init: F) -> Result<Arc<T>, E>
    where
        T: Any + Send + Sync,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut slots = self.extensions.lock().await;
        if let Some(existing) = slots.get(&TypeId::of::<T>())
            && let Ok(found) = Arc::clone(existing).downcast::<T>()
        {
            return Ok(found);
        }
        let value = Arc::new(init().await?);
        slots.insert(
            TypeId::of::<T>(),
            Arc::clone(&value) as Arc<dyn Any + Send + Sync>,
        );
        Ok(value)
    }

    /// The cell of a scope, created empty (no incarnation) when unknown.
    #[must_use]
    pub fn cell(&self, id: &ScopeId) -> Arc<ScopeCell> {
        let mut cells = lock(&self.cells);
        Arc::clone(
            cells
                .entry(id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(ScopeState::empty(id.clone())))),
        )
    }

    /// The ids of every registered scope of a family, in key order.
    #[must_use]
    pub fn ids_of(&self, family: ScopeFamily) -> Vec<ScopeId> {
        let mut ids: Vec<ScopeId> = lock(&self.cells)
            .keys()
            .filter(|id| id.family == family)
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    /// Lock several scopes in [`ScopeId`] order (deadlock free), once each.
    pub async fn lock_scopes(&self, ids: &[ScopeId]) -> Vec<ScopeGuard> {
        let unique: BTreeSet<&ScopeId> = ids.iter().collect();
        let mut guards = Vec::with_capacity(unique.len());
        for id in unique {
            guards.push(self.cell(id).lock_owned().await);
        }
        guards
    }

    /// The pinned values of a scope, chunked ones read and verified.
    pub async fn pinned_values(
        &self,
        state: &ScopeState,
    ) -> Result<BTreeMap<String, Arc<Vec<u8>>>, SpiError> {
        let mut out = BTreeMap::new();
        for (key, pinned) in &state.pinned {
            let value = match pinned {
                Pinned::Inline(bytes) => Arc::clone(bytes),
                Pinned::Chunk(chunk) => Arc::new(self.read_chunk(chunk).await?),
            };
            out.insert(key.clone(), value);
        }
        Ok(out)
    }

    /// Fold the scope's rows into its row entries (D2: a body loads when
    /// Core opens the scope). Unreferenced chunks the scope owns are deleted
    /// afterwards, best effort.
    pub async fn load_body(
        &self,
        state: &mut ScopeState,
    ) -> Result<BTreeMap<String, Arc<Vec<u8>>>, SpiError> {
        self.check_scope(state)?;
        let Some(name) = state.name.clone() else {
            state.body = Body::Loaded;
            return Ok(BTreeMap::new());
        };
        let scope = ScopeRef::new(state.id.family.kind, name.clone());
        let mut values: BTreeMap<String, Arc<Vec<u8>>> = BTreeMap::new();
        let mut stamps: BTreeMap<String, Stamp> = BTreeMap::new();
        let mut row_chunks = Vec::new();
        let mut index = state.window.s;
        while index < state.window.e {
            let want = usize::try_from(state.window.e - index)
                .unwrap_or(READ_PAGE_ROWS)
                .min(READ_PAGE_ROWS);
            let rows = self
                .host
                .read_rows(&scope, RowIndex::new(index), want)
                .await?;
            if rows.is_empty() {
                return Err(SpiError::Corrupt(format!(
                    "{scope} rows end at {index}, before its head's window end {}",
                    state.window.e
                )));
            }
            for row in &rows {
                for op in codec::decode_row(row.as_bytes())? {
                    match op {
                        RowOp::Spill(chunk) => {
                            let packed = self.read_chunk(&chunk).await?;
                            for inner in codec::unpack_rows(&packed)? {
                                for op in codec::decode_row(&inner)? {
                                    if matches!(op, RowOp::Spill(_)) {
                                        return Err(SpiError::Corrupt(format!(
                                            "{scope} nests a spill inside a spill"
                                        )));
                                    }
                                    self.fold(op, &mut values, &mut stamps).await?;
                                }
                            }
                            row_chunks.push((index, chunk));
                        }
                        op => self.fold(op, &mut values, &mut stamps).await?,
                    }
                }
                index += 1;
            }
        }
        state.rows = stamps;
        state.row_chunks = row_chunks;
        state.body = Body::Loaded;
        self.collect_owned_garbage(state).await;
        Ok(values)
    }

    async fn fold(
        &self,
        op: RowOp,
        values: &mut BTreeMap<String, Arc<Vec<u8>>>,
        stamps: &mut BTreeMap<String, Stamp>,
    ) -> Result<(), SpiError> {
        match op {
            RowOp::Delete(key) => {
                values.remove(&key);
                stamps.remove(&key);
            }
            RowOp::Put(key, value) => {
                let stamp = Stamp {
                    digest: codec::sha256(&value),
                    len: value.len() as u64,
                    chunk: None,
                    shared: (value.len() >= SHARED_STAMP_MIN).then(|| Arc::clone(&value)),
                    op_bytes: op_bytes(&key, value.len()),
                };
                stamps.insert(key.clone(), stamp);
                values.insert(key, value);
            }
            RowOp::PutChunk(key, chunk) => {
                let value = Arc::new(self.read_chunk(&chunk).await?);
                let stamp = Stamp {
                    digest: chunk.sha256,
                    len: chunk.len,
                    op_bytes: op_bytes(&key, chunk.name.as_str().len() + 40),
                    chunk: Some(chunk),
                    shared: Some(Arc::clone(&value)),
                };
                stamps.insert(key.clone(), stamp);
                values.insert(key, value);
            }
            RowOp::Reset => {
                values.clear();
                stamps.clear();
            }
            RowOp::Spill(_) => {
                return Err(SpiError::Corrupt("an unexpected spill".into()));
            }
        }
        Ok(())
    }

    /// Delete chunks this scope owns but no longer references (a crash
    /// between their write and the referencing batch). Best effort.
    async fn collect_owned_garbage(&self, state: &ScopeState) {
        let Some(name) = state.name.as_ref() else {
            return;
        };
        let referenced: HashSet<String> = state
            .chunk_refs()
            .into_iter()
            .map(|chunk| chunk.name.as_str().to_string())
            .collect();
        let garbage: Vec<(String, ScopeVersion)> = lock(&self.chunks)
            .iter()
            .filter(|(chunk, meta)| meta.owner == name.as_str() && !referenced.contains(*chunk))
            .map(|(chunk, meta)| (chunk.clone(), meta.version))
            .collect();
        for group in garbage.chunks(64) {
            if let Err(error) = self.delete_chunks(group).await {
                tracing::warn!(scope = %state.id, %error, "could not delete unreferenced host store chunks");
                return;
            }
        }
    }

    async fn read_chunk(&self, chunk: &ChunkRef) -> Result<Vec<u8>, SpiError> {
        let scope = ScopeRef::new(ScopeKind::Record, chunk.name.clone());
        let head = match self.host.load_head(&scope).await? {
            Some(head) => ChunkHeadV1::decode(&head.head)?,
            None => {
                return Err(SpiError::Corrupt(format!(
                    "chunk {} is referenced but missing",
                    chunk.name
                )));
            }
        };
        if !head.done || head.len != chunk.len || codec::unhex32(&head.sha)? != chunk.sha256 {
            return Err(SpiError::Corrupt(format!(
                "chunk {} is incomplete or differs from its reference",
                chunk.name
            )));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(head.len).unwrap_or(0));
        let mut index = 0_u64;
        while index < head.n {
            let want = usize::try_from(head.n - index)
                .unwrap_or(READ_PAGE_ROWS)
                .min(READ_PAGE_ROWS);
            let rows = self
                .host
                .read_rows(&scope, RowIndex::new(index), want)
                .await?;
            if rows.is_empty() {
                return Err(SpiError::Corrupt(format!(
                    "chunk {} ends before its {} rows",
                    chunk.name, head.n
                )));
            }
            for row in &rows {
                bytes.extend_from_slice(row.as_bytes());
            }
            index += rows.len() as u64;
        }
        if bytes.len() as u64 != chunk.len || codec::sha256(&bytes) != chunk.sha256 {
            return Err(SpiError::Corrupt(format!(
                "chunk {} does not match its digest",
                chunk.name
            )));
        }
        Ok(bytes)
    }

    /// Commit one call's changes to its locked scopes as ONE batch (plus the
    /// chunk batches that must land first), before the call answers.
    ///
    /// # Errors
    /// Any failure after which the host may differ from memory poisons
    /// every scope of the batch (D3); a fence loss poisons the realm.
    pub async fn commit(
        &self,
        scopes: &mut [&mut ScopeState],
        deltas: Vec<ScopeDelta>,
    ) -> Result<(), SpiError> {
        if scopes.len() != deltas.len() {
            return Err(SpiError::Misuse(
                "a commit pairs every locked scope with one delta".into(),
            ));
        }
        self.check_realm()?;
        for state in scopes.iter() {
            self.check_scope(state)?;
            if state.body != Body::Loaded {
                return Err(SpiError::Misuse(format!(
                    "{} is committed before its body was loaded",
                    state.id
                )));
            }
        }
        let mut plans = Vec::new();
        for (index, (state, delta)) in scopes.iter().zip(deltas).enumerate() {
            if let Some(plan) = self.plan(index, state, delta)? {
                plans.push(plan);
            }
        }
        if plans.is_empty() {
            return Ok(());
        }
        // The memory side already changed: from here on, a failure poisons.
        let outcome = self.write_plans(&mut plans).await;
        match outcome {
            Ok(()) => {
                for plan in plans {
                    let state = &mut *scopes[plan.index];
                    self.adopt(state, plan);
                }
                Ok(())
            }
            Err(error) => {
                let cause = error.to_string();
                for plan in &plans {
                    self.poison_scope(&mut *scopes[plan.index], &cause);
                }
                if matches!(error, SpiError::Host(HostStoreError::FencedOut { .. })) {
                    return Err(self.poison_realm(cause));
                }
                Err(error)
            }
        }
    }

    async fn write_plans(&self, plans: &mut [ScopePlan]) -> Result<(), SpiError> {
        // Self-fence again: the call may have outlived the lease.
        self.ensure_lease().await?;
        self.fit_batch(plans)?;
        for plan in &*plans {
            for chunk in &plan.new_chunks {
                self.write_chunk(chunk).await?;
            }
        }
        let mut updates = Vec::new();
        let mut dead: BTreeMap<String, ScopeVersion> = BTreeMap::new();
        {
            let table = lock(&self.chunks);
            for plan in &*plans {
                for chunk in &plan.dead_chunks {
                    if let Some(meta) = table.get(chunk.name.as_str()) {
                        dead.insert(chunk.name.as_str().to_string(), meta.version);
                    }
                }
            }
        }
        for plan in &*plans {
            updates.push(if plan.delete {
                ScopeUpdate {
                    scope: plan.scope.clone(),
                    previous: plan.previous,
                    next: plan.next,
                    head: None,
                    append: Vec::new(),
                    truncate_after: None,
                    drop_before: None,
                    delete: true,
                }
            } else {
                ScopeUpdate {
                    scope: plan.scope.clone(),
                    previous: plan.previous,
                    next: plan.next,
                    head: Some(plan_head_of(plan)?),
                    append: plan.rows.clone(),
                    truncate_after: None,
                    drop_before: plan
                        .checkpoint
                        .then(|| RowIndex::new(plan_window_of(plan).s)),
                    delete: false,
                }
            });
        }
        for (name, version) in &dead {
            updates.push(ScopeUpdate {
                scope: ScopeRef::new(
                    ScopeKind::Record,
                    ScopeName::try_new(name.clone()).map_err(SpiError::Host)?,
                ),
                previous: Some(*version),
                next: next_version(Some(*version))?,
                head: None,
                append: Vec::new(),
                truncate_after: None,
                drop_before: None,
                delete: true,
            });
        }
        let batch = CommitBatch::try_new(self.commit_id()?, self.fence, updates)?;
        self.commit_settled(&batch).await?;
        let mut table = lock(&self.chunks);
        for name in dead.keys() {
            table.remove(name);
        }
        Ok(())
    }

    /// Commit `batch`, settling an unknown or unavailable outcome by
    /// retrying the same batch (same commit id) and by reading the first
    /// scope's head, within the retry budget (C-2).
    async fn commit_settled(&self, batch: &CommitBatch) -> Result<(), SpiError> {
        let deadline = Instant::now() + self.config.commit_retry_budget;
        let mut attempt: u32 = 0;
        loop {
            let error = match self.host.commit(batch).await {
                Ok(()) => return Ok(()),
                Err(HostStoreError::Indeterminate(detail)) => {
                    if self.landed(batch).await {
                        return Ok(());
                    }
                    HostStoreError::Indeterminate(detail)
                }
                Err(HostStoreError::Unavailable(detail)) => HostStoreError::Unavailable(detail),
                Err(other) => return Err(SpiError::Host(other)),
            };
            attempt = attempt.saturating_add(1);
            let backoff = Duration::from_millis(50_u64.saturating_mul(1 << attempt.min(6)));
            if Instant::now() + backoff >= deadline {
                return Err(SpiError::Host(error));
            }
            tokio::time::sleep(backoff).await;
        }
    }

    /// Whether `batch` landed, read from its first update's scope: the head
    /// names the batch's commit id, or a deleted scope is gone.
    async fn landed(&self, batch: &CommitBatch) -> bool {
        let Some(update) = batch.updates().first() else {
            return false;
        };
        match self.host.load_head(&update.scope).await {
            Ok(Some(head)) => !update.delete && head.commit_id == *batch.commit_id(),
            Ok(None) => update.delete,
            Err(_) => false,
        }
    }

    async fn write_chunk(&self, pending: &PendingChunk) -> Result<(), SpiError> {
        let scope = ScopeRef::new(ScopeKind::Record, pending.chunk.name.clone());
        let pieces: Vec<&[u8]> = if pending.bytes.is_empty() {
            Vec::new()
        } else {
            pending.bytes.chunks(MAX_ROW_BYTES).collect()
        };
        let total = pieces.len() as u64;
        let groups: Vec<&[&[u8]]> = if pieces.is_empty() {
            vec![&[][..]]
        } else {
            pieces.chunks(CHUNK_ROWS_PER_BATCH).collect()
        };
        let last_group = groups.len().saturating_sub(1);
        let mut previous: Option<ScopeVersion> = None;
        let mut head = ChunkHeadV1 {
            f: FORMAT_V1,
            owner: pending.owner.as_str().to_string(),
            len: pending.chunk.len,
            sha: codec::hex(&pending.chunk.sha256),
            n: total,
            done: false,
        };
        for (index, group) in groups.into_iter().enumerate() {
            head.done = index == last_group;
            let mut append = Vec::with_capacity(group.len());
            for piece in group {
                append.push(Row::try_new(piece.to_vec())?);
            }
            let next = next_version(previous)?;
            let update = ScopeUpdate {
                scope: scope.clone(),
                previous,
                next,
                head: Some(head.encode()?),
                append,
                truncate_after: None,
                drop_before: None,
                delete: false,
            };
            let batch = CommitBatch::try_new(self.commit_id()?, self.fence, vec![update])?;
            self.commit_settled(&batch).await?;
            previous = Some(next);
            lock(&self.chunks).insert(
                pending.chunk.name.as_str().to_string(),
                ChunkMeta {
                    version: next,
                    owner: pending.owner.as_str().to_string(),
                },
            );
        }
        Ok(())
    }

    fn new_chunk(&self, owner: &ScopeName, value: &Arc<Vec<u8>>) -> Result<PendingChunk, SpiError> {
        let name = codec::scope_name(CHUNK_TAG, owner.as_str(), &self.incarnation())?;
        Ok(PendingChunk {
            chunk: ChunkRef {
                name,
                len: value.len() as u64,
                sha256: codec::sha256(value),
            },
            owner: owner.clone(),
            bytes: Arc::clone(value),
        })
    }

    /// Plan one scope's update, or `None` when nothing changes.
    fn plan(
        &self,
        index: usize,
        state: &ScopeState,
        delta: ScopeDelta,
    ) -> Result<Option<ScopePlan>, SpiError> {
        let family = state.id.family;
        // Resolve the delta into explicit changes, and whether it is a full
        // image eligible for a checkpoint.
        let (changes, full) = match delta {
            ScopeDelta::Changes(changes) => (changes, None),
            ScopeDelta::Full(image) => {
                let mut changes: Vec<(String, Option<Arc<Vec<u8>>>)> = state
                    .pinned
                    .keys()
                    .chain(state.rows.keys())
                    .filter(|key| !image.contains_key(*key))
                    .map(|key| (key.clone(), None))
                    .collect();
                changes.extend(
                    image
                        .iter()
                        .map(|(key, value)| (key.clone(), Some(Arc::clone(value)))),
                );
                (changes, Some(image))
            }
        };
        let mut pinned_after = state.pinned.clone();
        let mut row_changes: Vec<(String, Option<Arc<Vec<u8>>>)> = Vec::new();
        let mut changed_pins = false;
        for (key, value) in changes {
            if family.is_pinned(&key) {
                match value {
                    None => {
                        if pinned_after.remove(&key).is_some() {
                            changed_pins = true;
                        }
                    }
                    Some(value) => {
                        let same = match state.pinned.get(&key) {
                            Some(Pinned::Inline(old)) => old.as_slice() == value.as_slice(),
                            Some(Pinned::Chunk(chunk)) => {
                                chunk.len == value.len() as u64
                                    && chunk.sha256 == codec::sha256(&value)
                            }
                            None => false,
                        };
                        if !same {
                            pinned_after.insert(key, Pinned::Inline(value));
                            changed_pins = true;
                        }
                    }
                }
            } else {
                match (&value, state.rows.get(&key)) {
                    (None, None) => {}
                    (Some(value), Some(stamp)) if stamp.matches(value) => {}
                    _ => row_changes.push((key, value)),
                }
            }
        }
        let rows_after_empty = {
            let removed: BTreeSet<&String> = row_changes
                .iter()
                .filter(|(_, value)| value.is_none())
                .map(|(key, _)| key)
                .collect();
            let added = row_changes.iter().any(|(_, value)| value.is_some());
            !added && state.rows.keys().all(|key| removed.contains(key))
        };
        let delete = rows_after_empty && pinned_after.is_empty();
        let checkpoint = full.is_some() && state.wants_full_image() && !delete;
        if !changed_pins && row_changes.is_empty() && !checkpoint {
            return Ok(None);
        }
        if delete && state.name.is_none() {
            // Nothing exists and nothing remains.
            return Ok(None);
        }
        let name = match &state.name {
            Some(name) => name.clone(),
            None => codec::scope_name(family.tag, &state.id.key, &self.incarnation())?,
        };
        let scope = ScopeRef::new(family.kind, name.clone());
        let previous = state.version;
        let next = next_version(previous)?;
        if delete {
            return Ok(Some(ScopePlan {
                index,
                family_tag: family.tag,
                key: state.id.key.clone(),
                old_window: state.window,
                old_since: state.since_checkpoint,
                scope,
                previous,
                next,
                delete: true,
                pinned_after: BTreeMap::new(),
                rows: Vec::new(),
                checkpoint: false,
                row_changes: Vec::new(),
                live_after: 0,
                new_chunks: Vec::new(),
                dead_chunks: state.chunk_refs(),
                spill: None,
            }));
        }
        let mut new_chunks = Vec::new();
        let mut dead_chunks = Vec::new();
        // Pinned values: large ones move to chunks; replaced chunks die.
        for pinned in pinned_after.values_mut() {
            if let Pinned::Inline(value) = pinned
                && value.len() > PINNED_INLINE_LIMIT
            {
                let pending = self.new_chunk(&name, value)?;
                *pinned = Pinned::Chunk(pending.chunk.clone());
                new_chunks.push(pending);
            }
        }
        for (key, old) in &state.pinned {
            if let Pinned::Chunk(chunk) = old {
                let kept =
                    matches!(pinned_after.get(key), Some(Pinned::Chunk(now)) if now == chunk);
                if !kept {
                    dead_chunks.push(chunk.clone());
                }
            }
        }
        // Keep the head bounded: move the largest inline pins to chunks.
        loop {
            let size = state.head(&pinned_after, state.window, 0, 0)?.len();
            if size <= HEAD_LIMIT {
                break;
            }
            let largest = pinned_after
                .iter()
                .filter_map(|(key, pinned)| match pinned {
                    Pinned::Inline(value) => Some((key.clone(), value.len())),
                    Pinned::Chunk(_) => None,
                })
                .max_by_key(|(_, len)| *len);
            let Some((key, _)) = largest else {
                return Err(SpiError::Misuse(format!(
                    "{} has a head of {size} bytes even with every pinned value chunked",
                    state.id
                )));
            };
            if let Some(Pinned::Inline(value)) = pinned_after.get(&key).cloned() {
                let pending = self.new_chunk(&name, &value)?;
                pinned_after.insert(key, Pinned::Chunk(pending.chunk.clone()));
                new_chunks.push(pending);
            }
        }
        // Row entries.
        let mut stamps_after: Vec<(String, Option<Stamp>)> = Vec::new();
        let mut ops: Vec<RowOp> = Vec::new();
        let mut live_after = state.live_bytes;
        let checkpoint_image: Option<BTreeMap<String, Arc<Vec<u8>>>> = if checkpoint {
            full.map(|image| {
                image
                    .into_iter()
                    .filter(|(key, _)| !family.is_pinned(key))
                    .collect()
            })
        } else {
            None
        };
        let stamp_for = |key: &str, value: &Arc<Vec<u8>>, chunk: Option<ChunkRef>| -> Stamp {
            let op_len = match &chunk {
                Some(chunk) => op_bytes(key, chunk.name.as_str().len() + 40),
                None => op_bytes(key, value.len()),
            };
            Stamp {
                digest: chunk
                    .as_ref()
                    .map_or_else(|| codec::sha256(value), |chunk| chunk.sha256),
                len: value.len() as u64,
                chunk,
                shared: (value.len() >= SHARED_STAMP_MIN).then(|| Arc::clone(value)),
                op_bytes: op_len,
            }
        };
        if let Some(image) = checkpoint_image {
            // A checkpoint: RESET, then every live row entry. Unchanged
            // chunked values keep their chunk; every earlier spill dies.
            ops.push(RowOp::Reset);
            live_after = 0;
            let mut kept_chunks: HashSet<String> = HashSet::new();
            for (key, value) in &image {
                let reuse = state
                    .rows
                    .get(key)
                    .filter(|stamp| stamp.chunk.is_some() && stamp.matches(value))
                    .and_then(|stamp| stamp.chunk.clone());
                let stamp = if let Some(chunk) = reuse {
                    kept_chunks.insert(chunk.name.as_str().to_string());
                    ops.push(RowOp::PutChunk(key.clone(), chunk.clone()));
                    stamp_for(key, value, Some(chunk))
                } else if value.len() > INLINE_VALUE_LIMIT {
                    let pending = self.new_chunk(&name, value)?;
                    ops.push(RowOp::PutChunk(key.clone(), pending.chunk.clone()));
                    let stamp = stamp_for(key, value, Some(pending.chunk.clone()));
                    new_chunks.push(pending);
                    stamp
                } else {
                    ops.push(RowOp::Put(key.clone(), Arc::clone(value)));
                    stamp_for(key, value, None)
                };
                live_after += stamp.op_bytes;
                stamps_after.push((key.clone(), Some(stamp)));
            }
            for stamp in state.rows.values() {
                if let Some(chunk) = &stamp.chunk
                    && !kept_chunks.contains(chunk.name.as_str())
                {
                    dead_chunks.push(chunk.clone());
                }
            }
            dead_chunks.extend(state.row_chunks.iter().map(|(_, chunk)| chunk.clone()));
        } else {
            for (key, value) in row_changes {
                if let Some(old) = state.rows.get(&key) {
                    live_after = live_after.saturating_sub(old.op_bytes);
                    if let Some(chunk) = &old.chunk {
                        dead_chunks.push(chunk.clone());
                    }
                }
                match value {
                    None => {
                        ops.push(RowOp::Delete(key.clone()));
                        stamps_after.push((key, None));
                    }
                    Some(value) => {
                        let stamp = if value.len() > INLINE_VALUE_LIMIT {
                            let pending = self.new_chunk(&name, &value)?;
                            ops.push(RowOp::PutChunk(key.clone(), pending.chunk.clone()));
                            let stamp = stamp_for(&key, &value, Some(pending.chunk.clone()));
                            new_chunks.push(pending);
                            stamp
                        } else {
                            ops.push(RowOp::Put(key.clone(), Arc::clone(&value)));
                            stamp_for(&key, &value, None)
                        };
                        live_after += stamp.op_bytes;
                        stamps_after.push((key, Some(stamp)));
                    }
                }
            }
        }
        let rows = codec::encode_rows(&ops)?;
        Ok(Some(ScopePlan {
            index,
            family_tag: family.tag,
            key: state.id.key.clone(),
            old_window: state.window,
            old_since: state.since_checkpoint,
            scope,
            previous,
            next,
            delete: false,
            pinned_after,
            rows,
            checkpoint,
            row_changes: stamps_after,
            live_after,
            new_chunks,
            dead_chunks,
            spill: None,
        }))
    }

    /// Keep the main batch within [`BATCH_BUDGET`] (M5): spill the largest
    /// scopes' rows into chunks, each replaced by one spill row.
    fn fit_batch(&self, plans: &mut [ScopePlan]) -> Result<(), SpiError> {
        loop {
            let heads: usize = plans
                .iter()
                .filter(|plan| !plan.delete)
                .map(|plan| plan_head_of(plan).map_or(HEAD_LIMIT, |head| head.len()))
                .sum();
            let rows: usize = plans.iter().map(ScopePlan::row_bytes).sum();
            if heads + rows <= BATCH_BUDGET {
                return Ok(());
            }
            let Some(largest) = plans
                .iter_mut()
                .filter(|plan| plan.spill.is_none() && plan.row_bytes() > MAX_ROW_BYTES / 4)
                .max_by_key(|plan| plan.row_bytes())
            else {
                return Err(SpiError::Misuse(format!(
                    "a batch of {heads} head bytes and {rows} row bytes cannot be made to fit"
                )));
            };
            let packed = Arc::new(codec::pack_rows(&largest.rows));
            let owner = largest.scope.name.clone();
            let pending = self.new_chunk(&owner, &packed)?;
            let spill = pending.chunk.clone();
            largest.rows = codec::encode_rows(&[RowOp::Spill(spill.clone())])?;
            largest.new_chunks.push(pending);
            largest.spill = Some(spill);
        }
    }

    fn adopt(&self, state: &mut ScopeState, plan: ScopePlan) {
        if plan.delete {
            let id = state.id.clone();
            *state = ScopeState::empty(id);
            return;
        }
        let window = plan_window_of(&plan);
        state.name = Some(plan.scope.name.clone());
        state.version = Some(plan.next);
        state.pinned = plan.pinned_after;
        if plan.checkpoint {
            state.rows.clear();
            state.row_chunks.clear();
            state.since_checkpoint = plan.rows.iter().map(|row| row.len() as u64).sum();
        } else {
            state.since_checkpoint += plan.rows.iter().map(|row| row.len() as u64).sum::<u64>();
        }
        for (key, stamp) in plan.row_changes {
            match stamp {
                Some(stamp) => {
                    state.rows.insert(key, stamp);
                }
                None => {
                    state.rows.remove(&key);
                }
            }
        }
        if let Some(spill) = plan.spill {
            state.row_chunks.push((window.e.saturating_sub(1), spill));
        }
        state.live_bytes = plan.live_after;
        state.window = window;
        state.body = Body::Loaded;
    }
}

/// The scope's window after the plan: rows append at the old end; a
/// checkpoint drops every row before its first one.
fn plan_window_of(plan: &ScopePlan) -> WindowV1 {
    let old = plan.old_window;
    let appended = plan.rows.len() as u64;
    if plan.checkpoint {
        WindowV1 {
            s: old.e,
            e: old.e + appended,
        }
    } else {
        WindowV1 {
            s: old.s,
            e: old.e + appended,
        }
    }
}

fn plan_head_of(plan: &ScopePlan) -> Result<Vec<u8>, SpiError> {
    let window = plan_window_of(plan);
    let since = if plan.checkpoint {
        plan.rows.iter().map(|row| row.len() as u64).sum()
    } else {
        plan.old_since + plan.rows.iter().map(|row| row.len() as u64).sum::<u64>()
    };
    let pin = plan
        .pinned_after
        .iter()
        .map(|(key, value)| {
            let wire = match value {
                Pinned::Inline(bytes) => codec::pin_inline(bytes),
                Pinned::Chunk(chunk) => PinnedV1::Chunk(chunk.to_wire()),
            };
            (key.clone(), wire)
        })
        .collect();
    ScopeHeadV1 {
        f: FORMAT_V1,
        fam: plan.family_tag.to_string(),
        key: plan.key.clone(),
        pin,
        w: window,
        lb: plan.live_after,
        sb: since,
    }
    .encode()
}
